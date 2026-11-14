//! 实时屏幕翻译：会话循环（取景 → 变化检测 → 识字 → 翻译 → 贴字层）、窗口、命令、快捷键入口。
//! 三种模式共用一个循环：标准 / 字幕 / 漫画。系统声音实时字幕在 `liveaudio`，借用这里的字幕条窗口。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{AppError, AppResult};
use crate::livecore::{self, Group};
use crate::livetx;
use crate::livewin::{self, Frame};
use crate::settings::{LiveProfile, LiveSettings};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub const OV: &str = "live-overlay";
pub const SUBBAR: &str = "live-subbar";
pub const PICK: &str = "live-pick";
pub const EVT_BLOCKS: &str = "live://blocks";
pub const EVT_STATE: &str = "live://state";
pub const EVT_SUB: &str = "live://sub";
pub const EVT_HIDDEN: &str = "live://hidden";
pub const EVT_SHOT: &str = "live://shot";

// ---------- 数据 ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Target {
    /// region / window / screen
    pub kind: String,
    /// 屏幕物理像素 x, y, w, h（window 会随窗口更新）
    pub rect: [i32; 4],
    pub hwnd: isize,
    pub exe: String,
    pub title: String,
}

/// 贴在屏幕上的一块译文。坐标是相对贴字层窗口左上角的物理像素。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OBlock {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: String,
    pub orig: String,
    pub fg: String,
    pub bg: String,
    /// 起始字号（物理像素），界面会按框大小再缩
    pub font: f32,
    pub vertical: bool,
    pub lines: usize,
    /// 居中排（字幕盖住原位用）
    pub center: bool,
    /// 原文的对齐：0 靠左 / 1 居中 / 2 靠右（多行段落按各行端点判断，单行按靠左）
    pub align: u8,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OPayload {
    seq: u64,
    mode: String,
    rect: [i32; 4],
    blocks: Vec<OBlock>,
    bilingual: bool,
    opacity: u8,
    font_min: u8,
    /// 目标语言是中日韩（竖排时用正立排版）
    cjk: bool,
    /// 漫画：先用底色盖掉原文
    erase: bool,
    /// 译文字体族（漫画模式用；空表示默认）
    font_family: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubPayload {
    pub seq: u64,
    pub text: String,
    pub orig: String,
    pub bilingual: bool,
    pub font: u8,
    pub source: String,
    /// 边说边出字的草稿（还没说完，没有译文）：字幕条里淡一点显示，整句说完后被定稿替换
    #[serde(default)]
    pub draft: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub supported: bool,
    pub running: bool,
    pub mode: String,
    pub target: Option<Target>,
    pub hidden: bool,
    pub paused: bool,
    pub msg: String,
    pub audio: bool,
}

// ---------- 会话表 ----------

struct Session {
    stop: Arc<AtomicBool>,
    hidden: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    mode: Arc<Mutex<String>>,
    target: Target,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static LAST: Mutex<Option<Target>> = Mutex::new(None);
static MSG: Mutex<String> = Mutex::new(String::new());
static SEQ: AtomicU64 = AtomicU64::new(1);
static LAST_PAYLOAD: Mutex<Option<Value>> = Mutex::new(None);
static APPLIED: Mutex<[i32; 4]> = Mutex::new([0; 4]);
static PICK_GRAB: Mutex<Option<crate::scrcap::Grab>> = Mutex::new(None);
static PICK_EXE: Mutex<(isize, String)> = Mutex::new((0, String::new()));

fn lock<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn status() -> Status {
    let s = lock(&SESSION);
    Status {
        supported: livewin::SUPPORTED,
        running: s.is_some(),
        mode: s.as_ref().map(|s| lock_in(&s.mode).clone()).unwrap_or_default(),
        target: s.as_ref().map(|s| s.target.clone()).or_else(|| lock(&LAST).clone()),
        hidden: s.as_ref().is_some_and(|s| s.hidden.load(Ordering::Relaxed)),
        paused: s.as_ref().is_some_and(|s| s.paused.load(Ordering::Relaxed)),
        msg: lock(&MSG).clone(),
        audio: crate::liveaudio::running(),
    }
}

fn lock_in<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn emit_state(app: &AppHandle, msg: &str) {
    *lock(&MSG) = msg.to_string();
    let _ = app.emit(EVT_STATE, status());
}

// ---------- 纯逻辑：变化门限 / 字幕跟踪 / 排字 ----------

#[derive(Debug, PartialEq)]
pub enum Act {
    Idle,
    Wait,
    Process,
}

/// 画面变化 → 等稳定 → 处理。背景一直在动（游戏）时，变化持续超过 `force_ms` 也强制处理一次。
#[derive(Default)]
pub struct Gate {
    changed_at: Option<u64>,
    first_change: Option<u64>,
}

impl Gate {
    pub fn step(&mut self, now: u64, prev: f32, base: f32, thr: f32, stable_ms: u64, force_ms: u64) -> Act {
        if prev > thr {
            self.changed_at = Some(now);
            self.first_change.get_or_insert(now);
        }
        if base <= thr {
            self.first_change = None;
            return Act::Idle;
        }
        let stable = prev <= thr && self.changed_at.is_none_or(|t| now.saturating_sub(t) >= stable_ms);
        let forced = self.first_change.is_some_and(|t| now.saturating_sub(t) >= force_ms);
        if stable || forced {
            self.first_change = None;
            Act::Process
        } else {
            Act::Wait
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SubAct {
    Nothing,
    /// 这一句字幕稳定了，翻译它（可能已经和上一屏接成整句）
    Commit(String),
    /// 字幕消失够久了，撤掉译文
    Clear,
}

/// 字幕跟踪：同一句要稳定一会儿才翻；相同 / 很像的不重复翻；一句话分两屏时接成整句；消失后保留一会儿。
#[derive(Default)]
pub struct SubTrack {
    pending: String,
    pending_since: u64,
    /// 正在显示的那句（OCR 原文）
    shown: String,
    /// 连续几屏接起来的整句原文
    chain: String,
    chain_at: u64,
    empty_since: Option<u64>,
}

impl SubTrack {
    #[allow(clippy::too_many_arguments)]
    pub fn feed(&mut self, now: u64, text: &str, stable_ms: u64, keep_ms: u64, dedupe: bool, merge: bool) -> SubAct {
        let text = text.trim();
        if text.is_empty() {
            self.pending.clear();
            let since = *self.empty_since.get_or_insert(now);
            if !self.shown.is_empty() && now.saturating_sub(since) >= keep_ms {
                self.shown.clear();
                self.chain_at = since;
                return SubAct::Clear;
            }
            return SubAct::Nothing;
        }
        self.empty_since = None;
        if livecore::similarity(text, &self.pending) >= 0.85 {
            // 同一句：用更长的那个（逐字出现时越来越长）
            if text.chars().count() > self.pending.chars().count() {
                self.pending = text.to_string();
            }
        } else {
            self.pending = text.to_string();
            self.pending_since = now;
        }
        if now.saturating_sub(self.pending_since) < stable_ms {
            return SubAct::Nothing;
        }
        let same = if dedupe { livecore::similarity(&self.pending, &self.shown) >= 0.85 } else { self.pending == self.shown };
        if same {
            return SubAct::Nothing;
        }
        let cur = self.pending.clone();
        let gap = now.saturating_sub(self.chain_at.max(1));
        let full = if merge { livecore::merge_continuation(&self.chain, &cur, if self.shown.is_empty() { gap } else { 0 }, 2500) } else { None };
        self.shown = cur.clone();
        match full {
            Some(f) => {
                self.chain = f.clone();
                self.chain_at = now;
                SubAct::Commit(f)
            }
            None => {
                self.chain = cur.clone();
                self.chain_at = now;
                SubAct::Commit(cur)
            }
        }
    }

    pub fn reset(&mut self) {
        *self = SubTrack::default();
    }
}

/// 一组文字 → 贴字块：取底色和字色，起始字号取单行字高。
pub fn blocks_from(groups: &[Group], trans: &[String], f: &Frame, live: &LiveSettings) -> Vec<OBlock> {
    groups
        .iter()
        .zip(trans)
        .filter(|(_, t)| !t.trim().is_empty())
        .map(|(g, t)| {
            let (bg, fg) = livecore::sample_colors(&f.bgra, f.w, f.h, g.x, g.y, g.w, g.h);
            let font = (g.em * 0.9 * f32::from(live.font_scale) / 100.0).max(f32::from(live.font_min));
            OBlock {
                x: g.x,
                y: g.y,
                w: g.w,
                h: g.h,
                text: t.trim().to_string(),
                orig: g.text.clone(),
                fg: livecore::hex(fg),
                bg: livecore::hex(bg),
                font,
                vertical: g.vertical,
                lines: g.lines,
                center: false,
                align: g.align,
            }
        })
        .collect()
}

fn is_cjk_lang(lang: &str) -> bool {
    matches!(livecore::lang_class(lang), "zh" | "ja" | "ko")
}

// ---------- 窗口 ----------

fn small_window(app: &AppHandle, label: &str, hash: &str, title: &str, protected: bool) -> Option<tauri::WebviewWindow> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window(label) {
        return Some(w);
    }
    let built = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("index.html#{hash}").into()))
        .title(title)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .content_protected(protected)
        .build();
    match built {
        Ok(w) => {
            crate::webhealth::note_ok();
            crate::winframe::guard(&w);
            Some(w)
        }
        Err(e) => {
            // 窗口组件断开：限频写日志并通知界面，不再每次都刷一条
            if !crate::webhealth::note_failure(app, title, &e) {
                log::warn!("创建{title}失败：{e}");
            }
            None
        }
    }
}

fn place(w: &tauri::WebviewWindow, r: [i32; 4]) {
    use tauri::{PhysicalPosition, PhysicalSize};
    let _ = w.set_position(PhysicalPosition::new(r[0], r[1]));
    let _ = w.set_size(PhysicalSize::new(r[2].max(1) as u32, r[3].max(1) as u32));
}

/// 贴字层：透明、点击穿透、始终在最上、不出现在截图里（所以不会被自己再识别一遍）。
fn manga_font_css(k: &str) -> String {
    match k {
        "serif" => "'Songti SC', 'SimSun', 'Noto Serif CJK SC', serif",
        "sans" => "'Microsoft YaHei', 'PingFang SC', 'Noto Sans CJK SC', sans-serif",
        _ => "'Yuppy SC', 'Xingkai SC', 'STXingkai', 'KaiTi', 'Microsoft YaHei', sans-serif",
    }
    .to_string()
}

fn ensure_overlay(app: &AppHandle, rect: [i32; 4]) {
    let fresh = app.get_webview_window(OV).is_none();
    let Some(w) = small_window(app, OV, "liveov", "清影屏幕翻译", true) else { return };
    let mut applied = lock(&APPLIED);
    if fresh || *applied != rect {
        place(&w, rect);
        *applied = rect;
    }
    if fresh {
        let _ = w.set_ignore_cursor_events(true);
        crate::winframe::show_no_activate(&w);
    }
}

fn close_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(OV) {
        let _ = w.destroy();
    }
    *lock(&APPLIED) = [0; 4];
    *lock(&LAST_PAYLOAD) = None;
}

/// 字幕悬浮条：可拖动，位置和大小记在设置里。
pub fn ensure_subbar(app: &AppHandle, live: &LiveSettings) {
    let fresh = app.get_webview_window(SUBBAR).is_none();
    let Some(w) = small_window(app, SUBBAR, "livesub", "清影实时字幕", true) else { return };
    if !fresh {
        return;
    }
    let r = if live.sub_bar[2] >= 200 {
        live.sub_bar
    } else {
        // 默认放在主屏幕下方居中
        let (sw, sh, ox, oy) = app
            .primary_monitor()
            .ok()
            .flatten()
            .map(|m| (m.size().width as i32, m.size().height as i32, m.position().x, m.position().y))
            .unwrap_or((1920, 1080, 0, 0));
        let w = (sw * 6 / 10).clamp(500, 1400);
        [ox + (sw - w) / 2, oy + sh - 220, w, 120]
    };
    place(&w, r);
    let _ = w.set_resizable(true);
    let app2 = app.clone();
    let last = Arc::new(Mutex::new(Instant::now()));
    w.on_window_event(move |ev| {
        if matches!(ev, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
            // 拖动时事件很密：隔 500 毫秒存一次
            let mut t = lock_in(&last);
            if t.elapsed() < Duration::from_millis(500) {
                return;
            }
            *t = Instant::now();
            drop(t);
            if let (Some(win), Some(st)) = (app2.get_webview_window(SUBBAR), livetx::state_of(&app2)) {
                if let (Ok(p), Ok(s)) = (win.outer_position(), win.outer_size()) {
                    save_live(&st, |l| l.sub_bar = [p.x, p.y, s.width as i32, s.height as i32]);
                }
            }
        }
    });
    crate::winframe::show_no_activate(&w);
}

pub fn close_subbar(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SUBBAR) {
        let _ = w.destroy();
    }
}

fn save_live(st: &AppState, f: impl FnOnce(&mut LiveSettings)) {
    let _ = st.update_settings(|s| {
        f(&mut s.live);
        s.live.sanitize();
    });
}

// ---------- 悬浮窗 ----------
// 屏幕翻译的控制以前在一个单独的小悬浮钮里；现在并进清影的悬浮窗（tray.rs 的 FLOAT），桌面上只有一个。

// ---------- 排版稳定：前后两次识别的同一块译文，位置和字号不抖 ----------

/// 把这一轮识别出的块和上一轮已经贴出去的块对一下：原文几乎一样、位置差不多的，沿用上一轮的位置、大小和字号。
/// 识字每次会有几个像素的偏差、框宽高差一点点、字号估算差一两个像素，不处理的话译文会在原地一抖一抖；
/// 位置真的动了（滚动、拖动）就按新的来。字号同时吸附到整数像素，减少无意义的变化。
pub(crate) fn stabilize(prev: &[OBlock], mut new: Vec<OBlock>) -> Vec<OBlock> {
    let mut used = vec![false; prev.len()];
    for b in &mut new {
        b.font = b.font.round().max(1.0);
        let mut best: Option<(usize, f32)> = None;
        for (i, p) in prev.iter().enumerate() {
            if used[i] || p.vertical != b.vertical {
                continue;
            }
            let tol = (b.h.min(p.h) * 0.45).clamp(4.0, 24.0);
            let close = (b.x - p.x).abs() <= tol * 1.5 && (b.y - p.y).abs() <= tol && (b.w - p.w).abs() <= tol * 2.0 && (b.h - p.h).abs() <= tol * 1.5;
            if !close {
                continue;
            }
            let sim = livecore::similarity(&b.orig, &p.orig);
            if sim >= 0.8 && best.is_none_or_lt(sim) {
                best = Some((i, sim));
            }
        }
        if let Some((i, _)) = best {
            used[i] = true;
            let p = &prev[i];
            b.x = p.x;
            b.y = p.y;
            b.w = p.w;
            b.h = p.h;
            if (b.font - p.font).abs() <= (p.font * 0.15).max(1.0) {
                b.font = p.font;
            }
            // 原文完全一样：译文也沿用，不因为模型这次换了个说法就跳字
            if p.orig == b.orig && !p.text.is_empty() {
                b.text = p.text.clone();
            }
        }
    }
    new
}

trait BestSim {
    fn is_none_or_lt(&self, v: f32) -> bool;
}

impl BestSim for Option<(usize, f32)> {
    fn is_none_or_lt(&self, v: f32) -> bool {
        self.is_none_or(|(_, s)| v > s)
    }
}

// ---------- 会话循环 ----------

struct Runner {
    app: AppHandle,
    st: Arc<AppState>,
    stop: Arc<AtomicBool>,
    hidden: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    mode: Arc<Mutex<String>>,
    target: Target,
    t0: Instant,
    prev_sig: Option<Vec<u8>>,
    base_sig: Option<Vec<u8>>,
    gate: Gate,
    sub: SubTrack,
    last_key: String,
    shown: bool,
    /// 漫画：本次会话里已确定的人名译法，下一页接着用
    names: Vec<(String, String)>,
    empty_since: Option<u64>,
    backoff_until: u64,
    gone_ticks: u32,
    seen_hist: std::collections::VecDeque<String>,
    last_mode: String,
    errors: u32,
    /// 上一轮贴出去的块（排版稳定用）
    last_blocks: std::sync::Mutex<Vec<OBlock>>,
}

impl Runner {
    fn now(&self) -> u64 {
        self.t0.elapsed().as_millis() as u64
    }

    fn rect(&mut self) -> Option<[i32; 4]> {
        match self.target.kind.as_str() {
            "window" => match livewin::window_bounds(self.target.hwnd) {
                Some((x, y, w, h)) if w >= 40 && h >= 24 => {
                    self.gone_ticks = 0;
                    self.target.rect = [x, y, w, h];
                    Some(self.target.rect)
                }
                _ => {
                    self.gone_ticks += 1;
                    None
                }
            },
            _ => Some(self.target.rect),
        }
    }

    fn run(mut self) {
        livewin::init_thread();
        emit_state(&self.app, "已开始，等待画面");
        while !self.stop.load(Ordering::Relaxed) {
            let tick = Instant::now();
            let raw = self.st.settings_raw();
            let mode = lock_in(&self.mode).clone();
            let live = livetx::effective(&raw.live, &self.target.exe, Some(&mode));
            if mode != self.last_mode {
                self.reset_for_mode(&mode);
            }
            match self.tick(&live, &raw.ai.target_lang) {
                Ok(()) => self.errors = 0,
                Err(e) => {
                    self.errors += 1;
                    log::warn!("屏幕翻译：{}", e.message);
                    emit_state(&self.app, &e.message);
                    // 连续出错放慢，免得一直打满请求
                    self.backoff_until = self.now() + 2000 * u64::from(self.errors.min(5));
                    if self.gone_ticks > 600 {
                        break;
                    }
                }
            }
            let fps = if mode == "subtitle" { live.sub_fps } else { live.fps };
            let want = Duration::from_millis(1000 / u64::from(fps.max(1)));
            if let Some(rest) = want.checked_sub(tick.elapsed()) {
                std::thread::sleep(rest);
            }
        }
        self.finish();
    }

    fn reset_for_mode(&mut self, mode: &str) {
        self.last_mode = mode.to_string();
        self.prev_sig = None;
        self.base_sig = None;
        self.last_key.clear();
        self.sub.reset();
        self.clear();
    }

    /// 循环结束收尾。只收拾自己的会话：重新开始（换区域、换模式、悬浮窗和快捷键先后按了开始）时，
    /// 新会话已经登记好了，旧线程在这里清掉它的话，新会话就再也停不下来（停止按钮找不到它，它还一直贴译文）。
    fn finish(&mut self) {
        let mine = {
            let mut g = lock(&SESSION);
            let mine = g.as_ref().is_none_or(|s| Arc::ptr_eq(&s.stop, &self.stop));
            if mine {
                *g = None;
            }
            mine
        };
        if mine {
            close_overlay(&self.app);
            close_subbar_if_idle(&self.app);
            emit_state(&self.app, "已停止");
        }
    }

    fn clear(&mut self) {
        if self.shown {
            self.shown = false;
            self.emit_blocks(&self.last_mode.clone(), vec![], &LiveSettings::default(), false, "", false);
            let _ = self.app.emit_to(
                SUBBAR,
                EVT_SUB,
                SubPayload {
                    seq: SEQ.fetch_add(1, Ordering::Relaxed),
                    text: String::new(),
                    orig: String::new(),
                    bilingual: false,
                    font: 24,
                    source: "screen".into(),
                    draft: false,
                },
            );
        }
    }

    fn emit_blocks(&self, mode: &str, blocks: Vec<OBlock>, live: &LiveSettings, erase: bool, lang: &str, bilingual: bool) {
        // 已经停了（这一轮的识字 / 翻译还在路上）：不再贴译文，免得停止后译文又冒出来
        if self.stop.load(Ordering::Relaxed) {
            return;
        }
        let blocks = {
            let mut last = self.last_blocks.lock().unwrap_or_else(|e| e.into_inner());
            let out = if blocks.is_empty() { blocks } else { stabilize(&last, blocks) };
            last.clone_from(&out);
            out
        };
        let p = OPayload {
            seq: SEQ.fetch_add(1, Ordering::Relaxed),
            mode: mode.to_string(),
            rect: self.target.rect,
            blocks,
            bilingual,
            opacity: live.opacity,
            font_min: live.font_min,
            cjk: is_cjk_lang(lang),
            erase,
            font_family: if mode == "manga" { manga_font_css(&live.manga_font) } else { String::new() },
        };
        if let Ok(v) = serde_json::to_value(&p) {
            *lock(&LAST_PAYLOAD) = Some(v);
        }
        let _ = self.app.emit_to(OV, EVT_BLOCKS, p);
    }

    fn tick(&mut self, live: &LiveSettings, ai_target: &str) -> AppResult<()> {
        let mode = self.last_mode.clone();
        let Some(rect) = self.rect() else {
            self.clear();
            emit_state(&self.app, "目标窗口最小化或已关闭");
            return Ok(());
        };
        if self.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let bar_only = mode == "subtitle" && live.sub_place == "bar";
        if bar_only {
            ensure_subbar(&self.app, live);
        } else {
            ensure_overlay(&self.app, rect);
        }
        if self.hidden.load(Ordering::Relaxed) || self.paused.load(Ordering::Relaxed) {
            return Ok(());
        }
        if self.now() < self.backoff_until {
            return Ok(());
        }
        let frame = livewin::grab_rect(rect[0], rect[1], rect[2].max(1) as u32, rect[3].max(1) as u32).map_err(AppError::msg)?;
        let lang = livetx::target_lang(live, ai_target);
        if mode == "subtitle" {
            return self.tick_subtitle(live, &frame, &lang);
        }
        // ---- 标准 / 漫画：等画面稳定再处理 ----
        let now = self.now();
        let sig = livecore::signature(&frame.bgra, frame.w, frame.h);
        let thr = f32::from(live.change_pct) / 100.0;
        let prev = self.prev_sig.as_ref().map_or(1.0, |p| livecore::change_ratio(p, &sig));
        let base = self.base_sig.as_ref().map_or(1.0, |b| livecore::change_ratio(b, &sig));
        self.prev_sig = Some(sig.clone());
        // 大幅变化（换场景 / 翻页）：旧译文马上撤掉，别盖在新画面上
        if base > 0.45 && self.shown && (mode != "manga" || live.manga_follow) {
            self.clear();
            self.last_key.clear();
        }
        let stable_ms = if mode == "manga" { u64::from(live.stable_ms).max(700) } else { u64::from(live.stable_ms) };
        match self.gate.step(now, prev, base, thr, stable_ms, 2500) {
            Act::Idle | Act::Wait => return Ok(()),
            Act::Process => {}
        }
        emit_state(&self.app, "识别中");
        let done = if live.ocr == "vision" || (mode == "manga" && live.manga_engine == "vision") {
            self.process_vision(live, &frame, &lang, &mode)?
        } else {
            self.process_ocr(live, &frame, &lang, &mode)?
        };
        if done {
            self.base_sig = Some(sig);
        }
        Ok(())
    }

    fn recognize(&self, live: &LiveSettings, frame: &Frame, engine: &str) -> AppResult<Vec<livecore::OLine>> {
        let cx = crate::ocr::Ctx { data_dir: &self.st.data_dir, client: &self.st.dl_client, live };
        let lines = crate::ocr::recognize(&cx, engine, &frame.bgra, frame.w, frame.h)?;
        Ok(if live.skip_icons { livecore::drop_icons(lines, Some((&frame.bgra, frame.w, frame.h))) } else { lines })
    }

    fn history(&mut self, live: &LiveSettings, mode: &str, items: &[(String, String)]) {
        if !live.history {
            return;
        }
        for (src, text) in items {
            let k = livecore::text_key_of(src);
            if k.is_empty() || self.seen_hist.contains(&k) {
                continue;
            }
            self.seen_hist.push_back(k);
            if self.seen_hist.len() > 200 {
                self.seen_hist.pop_front();
            }
            livetx::history_push(
                &self.st.data_dir,
                &livetx::Entry { at: crate::db::now(), mode: mode.into(), exe: self.target.exe.clone(), src: src.clone(), text: text.clone() },
            );
        }
    }

    /// 本机 / 云端识字 → 分段 → 翻译 → 贴字。返回是否处理完（失败不更新基准，下一轮重试）。
    fn process_ocr(&mut self, live: &LiveSettings, frame: &Frame, lang: &str, mode: &str) -> AppResult<bool> {
        let engine = if live.ocr == "vision" { "system" } else { live.ocr.as_str() };
        let lines = self.recognize(live, frame, engine)?;
        let mut groups = livecore::group_lines(&lines);
        if live.skip_same {
            groups.retain(|g| !livecore::should_skip(&g.text, lang));
        }
        let now = self.now();
        if groups.is_empty() {
            // 文字真的没了：保留一会儿再撤，免得识字偶尔漏一帧就闪一下
            let since = *self.empty_since.get_or_insert(now);
            if now.saturating_sub(since) >= u64::from(live.keep_ms) {
                self.clear();
                self.last_key.clear();
                return Ok(true);
            }
            return Ok(false);
        }
        self.empty_since = None;
        let key = livecore::text_key(&groups);
        if key == self.last_key && self.shown {
            return Ok(true);
        }
        emit_state(&self.app, "翻译中");
        let texts: Vec<String> = groups.iter().map(|g| g.text.clone()).collect();
        let tr = livetx::translate(&self.app, &texts, lang, &self.target.title, &self.target.exe, &self.stop)?;
        let blocks = blocks_from(&groups, &tr, frame, live);
        let hist: Vec<(String, String)> = blocks.iter().map(|b| (b.orig.clone(), b.text.clone())).collect();
        self.emit_blocks(mode, blocks, live, mode == "manga", lang, live.bilingual);
        self.history(live, mode, &hist);
        self.last_key = key;
        self.shown = true;
        emit_state(&self.app, &format!("正在翻译（{} 段）", texts.len()));
        Ok(true)
    }

    /// 视觉模型：一次完成识字、翻译、给位置和方向。漫画模式默认走这条。
    fn process_vision(&mut self, live: &LiveSettings, frame: &Frame, lang: &str, mode: &str) -> AppResult<bool> {
        if !live.manga_ack && mode == "manga" {
            return Err(AppError::new(crate::error::ErrorKind::Other, "漫画模式会把画面发送给视觉模型。到“屏幕翻译”里点“我知道了”后再开始。"));
        }
        emit_state(&self.app, "视觉模型识别中");
        let png = vision_png(frame)?;
        let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png.0));
        let settings = self.st.settings();
        let ep = crate::ai::chat_endpoint_for(&self.st, &settings, crate::ai_pool::Task::Translate)?;
        let model = settings.ai.model.clone();
        let known: &[(String, String)] = if live.manga_names { &self.names } else { &[] };
        // 漫画模式用漫画的提示词；标准模式选了“视觉模型”识字时，用软件界面的（以前也用漫画的：会跳过界面文字、把图标当字）
        let system = if mode == "manga" { livetx::manga_system(lang, live.manga_rtl, live.manga_sfx, known) } else { livetx::screen_system(lang, known) };
        let ask = if mode == "manga" { "Translate the text on this page." } else { "Translate the text on this screen." };
        let resp = tauri::async_runtime::block_on(crate::ai::chat_img(&ep, &model, &system, ask, &url)).map_err(crate::imgtrans::vision_hint)?;
        if live.manga_names {
            for (s, t) in livetx::parse_names(&resp) {
                if let Some(e) = self.names.iter_mut().find(|e| e.0 == s) {
                    e.1 = t;
                } else if self.names.len() < 60 {
                    self.names.push((s, t));
                }
            }
        }
        let list = livetx::parse_manga(&resp).ok_or_else(|| AppError::msg("视觉模型没有按要求返回结果，换一个更强的视觉模型试试。"))?;
        let (fw, fh) = (frame.w as f32, frame.h as f32);
        let mut blocks = vec![];
        for b in &list {
            // 框是模型估的：先换成像素，再按画面里真正的“字”收紧一下
            let (mut x, mut y, mut w, mut h) = (b.rect[0] * fw, b.rect[1] * fh, b.rect[2] * fw, b.rect[3] * fh);
            let (bg0, _) = livecore::sample_colors(&frame.bgra, frame.w, frame.h, x, y, w, h);
            if let Some(t) = livecore::ink_bbox(&frame.bgra, frame.w, frame.h, x, y, w, h, bg0, 0.15) {
                (x, y, w, h) = (t[0], t[1], t[2], t[3]);
            }
            // 擦掉原文要比字略大一圈
            let pad = (w.min(h) * 0.12).clamp(2.0, 14.0);
            let (ex, ey) = ((x - pad).max(0.0), (y - pad).max(0.0));
            let (ew, eh) = ((w + 2.0 * pad).min(fw - ex), (h + 2.0 * pad).min(fh - ey));
            let (bg, fg) = livecore::sample_colors(&frame.bgra, frame.w, frame.h, ex, ey, ew, eh);
            if b.sfx && live.manga_sfx {
                // 拟声词：原字不动，在旁边挂一个小注
                let nw = (b.translation.chars().count() as f32 * 18.0 + 12.0).max(40.0).min(fw);
                let nx = (x + w / 2.0 - nw / 2.0).clamp(0.0, (fw - nw).max(0.0));
                let ny = (y + h + 2.0).min((fh - 22.0).max(0.0));
                blocks.push(OBlock {
                    x: nx,
                    y: ny,
                    w: nw,
                    h: 22.0,
                    text: b.translation.clone(),
                    orig: String::new(),
                    fg: "#ffffff".into(),
                    bg: "#000000b8".into(),
                    font: 15.0,
                    vertical: false,
                    lines: 1,
                    center: true,
                    align: 1,
                });
                continue;
            }
            let bg_css = if live.manga_erase == "inpaint" {
                livecore::edge_gradient(&frame.bgra, frame.w, frame.h, ex, ey, ew, eh)
                    .map(|(a, c)| format!("linear-gradient({}, {})", livecore::hex(a), livecore::hex(c)))
            } else {
                None
            };
            let vertical = livetx::is_vertical(b, fw, fh);
            // 起始字号：竖排看列宽，横排看行高（界面再按框缩）
            let em = if vertical { (w / b.translation.chars().count().max(1) as f32).max(w.min(h) * 0.5) } else { h };
            let font = (em * 0.9 * f32::from(live.font_scale) / 100.0).max(f32::from(live.font_min));
            blocks.push(OBlock {
                x: ex,
                y: ey,
                w: ew,
                h: eh,
                text: b.translation.clone(),
                orig: b.text.clone(),
                fg: livecore::hex(fg),
                bg: bg_css.unwrap_or_else(|| livecore::hex(bg)),
                font,
                vertical,
                lines: 1,
                center: true,
                align: 1,
            });
        }
        if blocks.is_empty() {
            self.clear();
            return Ok(true);
        }
        let hist: Vec<(String, String)> = blocks.iter().map(|b| (b.orig.clone(), b.text.clone())).collect();
        let erase = live.manga_erase != "none";
        self.emit_blocks(mode, blocks, live, erase, lang, live.bilingual);
        self.history(live, mode, &hist);
        self.shown = true;
        emit_state(&self.app, &format!("正在翻译（{} 段）", list.len()));
        Ok(true)
    }

    fn tick_subtitle(&mut self, live: &LiveSettings, frame: &Frame, lang: &str) -> AppResult<()> {
        let engine = if live.ocr == "vision" { "system" } else { live.ocr.as_str() };
        let lines = self.recognize(live, frame, engine)?;
        let mut groups = livecore::group_lines(&lines);
        groups.sort_by(|a, b| a.y.total_cmp(&b.y));
        let text = groups.iter().fold(String::new(), |acc, g| livecore::join_text(&acc, &g.text));
        // 字幕已经是目标语言（或只是符号）就当没有
        let text = if live.skip_same && !text.is_empty() && livecore::should_skip(&text, lang) { String::new() } else { text };
        let now = self.now();
        match self.sub.feed(now, &text, u64::from(live.sub_stable_ms), u64::from(live.sub_keep_ms), live.sub_dedupe, live.sub_merge) {
            SubAct::Nothing => Ok(()),
            SubAct::Clear => {
                self.shown = true;
                self.clear();
                Ok(())
            }
            SubAct::Commit(src) => {
                if live.skip_same && livecore::should_skip(&src, lang) {
                    return Ok(());
                }
                let tr = livetx::translate(&self.app, std::slice::from_ref(&src), lang, &self.target.title, &self.target.exe, &self.stop)?;
                let t = tr.into_iter().next().unwrap_or_default();
                self.history(live, "subtitle", &[(src.clone(), t.clone())]);
                self.shown = true;
                if live.sub_place == "bar" {
                    let _ = self.app.emit_to(
                        SUBBAR,
                        EVT_SUB,
                        SubPayload {
                            seq: SEQ.fetch_add(1, Ordering::Relaxed),
                            text: t,
                            orig: src,
                            bilingual: live.sub_bilingual,
                            font: live.sub_font,
                            source: "screen".into(),
                            draft: false,
                        },
                    );
                } else {
                    // 盖住原字幕：整条字幕带用底色盖掉，译文居中
                    let (bg, fg) = livecore::sample_colors(&frame.bgra, frame.w, frame.h, 0.0, 0.0, frame.w as f32, frame.h as f32);
                    let font = (frame.h as f32 * if live.sub_bilingual { 0.5 } else { 0.7 } * f32::from(live.font_scale) / 100.0).max(f32::from(live.font_min));
                    let b = OBlock {
                        x: 0.0,
                        y: 0.0,
                        w: frame.w as f32,
                        h: frame.h as f32,
                        text: t,
                        orig: src,
                        fg: livecore::hex(fg),
                        bg: livecore::hex(bg),
                        font,
                        vertical: false,
                        lines: 1,
                        center: true,
                        align: 1,
                    };
                    self.emit_blocks("subtitle", vec![b], live, true, lang, live.sub_bilingual);
                }
                Ok(())
            }
        }
    }
}

/// 视觉模型用的图：长边不超过 1600；全灰就编成灰度 PNG（漫画体积小很多）。
fn vision_png(f: &Frame) -> AppResult<(Vec<u8>, usize, usize)> {
    let k = (1600.0 / f.w.max(f.h) as f32).min(1.0);
    let (nw, nh) = (((f.w as f32 * k).round() as usize).max(1), ((f.h as f32 * k).round() as usize).max(1));
    let small = if nw == f.w { f.bgra.clone() } else { livecore::resize_bgra(&f.bgra, f.w, f.h, nw, nh) };
    let gray = small.as_chunks::<4>().0.iter().step_by(7).all(|p| p[0].abs_diff(p[1]) < 10 && p[1].abs_diff(p[2]) < 10);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, nw as u32, nh as u32);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let data: Vec<u8> = if gray {
            enc.set_color(png::ColorType::Grayscale);
            small.as_chunks::<4>().0.iter().map(|p| p[1]).collect()
        } else {
            enc.set_color(png::ColorType::Rgb);
            crate::scrcap::bgra_to_rgb(&small)
        };
        let mut wr = enc.write_header().map_err(|e| AppError::msg(e.to_string()))?;
        wr.write_image_data(&data).map_err(|e| AppError::msg(e.to_string()))?;
    }
    if out.len() > 6_000_000 {
        return Err(AppError::invalid("画面太大，请框小一点的区域。"));
    }
    Ok((out, nw, nh))
}

pub fn close_subbar_if_idle(app: &AppHandle) {
    if !crate::liveaudio::running() {
        close_subbar(app);
    }
}

// ---------- 启停 ----------

pub fn is_running() -> bool {
    lock(&SESSION).is_some()
}

pub fn stop(app: &AppHandle) {
    let s = lock(&SESSION).take();
    if let Some(s) = s {
        s.stop.store(true, Ordering::Relaxed);
        // 循环线程自己会收尾；这里先把窗口收掉，界面马上有反应
        close_overlay(app);
        close_subbar_if_idle(app);
    }
    emit_state(app, "已停止");
}

fn remember_profile(st: &AppState, t: &Target, mode: &str) {
    if t.exe.is_empty() || crate::writeassist::is_own_window(t.hwnd) {
        return;
    }
    let exe = t.exe.to_lowercase();
    save_live(st, |l| {
        let idx = l.profiles.iter().position(|p| p.exe == exe);
        let p = match idx {
            Some(i) => &mut l.profiles[i],
            None => {
                l.profiles.push(LiveProfile { exe: exe.clone(), ..Default::default() });
                l.profiles.last_mut().expect("刚推进去")
            }
        };
        p.target = if t.kind == "window" { "window".into() } else { "region".into() };
        if t.kind == "region" {
            p.region = t.rect;
        }
        p.mode = mode.to_string();
    });
}

/// 开始。`mode` 为空时用该程序记过的模式，再不然用全局设置。
pub fn start(app: &AppHandle, st: &Arc<AppState>, mut target: Target, mode: Option<String>) -> AppResult<Status> {
    if !livewin::SUPPORTED {
        return Err(AppError::msg("实时屏幕翻译目前只支持 Windows。"));
    }
    if is_running() {
        stop(app);
        std::thread::sleep(Duration::from_millis(120));
    }
    // 之前新开窗口失败过（窗口组件断开）：先试着开一个，开不出来就明说，不要开始了却什么都看不到
    if crate::webhealth::is_broken() {
        match small_window(app, OV, "liveov", "清影屏幕翻译", true) {
            Some(w) => {
                let _ = w.destroy();
            }
            None => return Err(AppError::msg(crate::webhealth::HINT)),
        }
    }
    if target.kind == "window" {
        let (x, y, w, h) = livewin::window_bounds(target.hwnd).ok_or_else(|| AppError::msg("这个窗口找不到了（可能已最小化或关闭）。"))?;
        target.rect = [x, y, w, h];
        if target.exe.is_empty() {
            target.exe = crate::writeassist::exe_of_hwnd(target.hwnd);
        }
    }
    if target.rect[2] < 40 || target.rect[3] < 20 {
        return Err(AppError::invalid("选的区域太小，至少要 40×20 像素。"));
    }
    let raw = st.settings_raw();
    let live = livetx::effective(&raw.live, &target.exe, mode.as_deref());
    let mode = live.mode.clone();
    remember_profile(st, &target, &mode);
    *lock(&LAST) = Some(target.clone());

    let stop_flag = Arc::new(AtomicBool::new(false));
    let hidden = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let mode_cell = Arc::new(Mutex::new(mode.clone()));
    *lock(&SESSION) =
        Some(Session { stop: stop_flag.clone(), hidden: hidden.clone(), paused: paused.clone(), mode: mode_cell.clone(), target: target.clone() });
    let runner = Runner {
        app: app.clone(),
        st: st.clone(),
        stop: stop_flag,
        hidden,
        paused,
        mode: mode_cell,
        target,
        t0: Instant::now(),
        prev_sig: None,
        base_sig: None,
        gate: Gate::default(),
        sub: SubTrack::default(),
        last_key: String::new(),
        shown: false,
        names: Vec::new(),
        empty_since: None,
        backoff_until: 0,
        gone_ticks: 0,
        seen_hist: Default::default(),
        last_mode: mode,
        errors: 0,
        last_blocks: std::sync::Mutex::new(Vec::new()),
    };
    std::thread::Builder::new().name("live-translate".into()).spawn(move || runner.run()).map_err(|e| AppError::msg(e.to_string()))?;
    Ok(status())
}

pub fn toggle(app: &AppHandle) {
    let Some(st) = livetx::state_of(app) else { return };
    if is_running() {
        stop(app);
        return;
    }
    // 语音翻译模式：不抓屏、不用框选，开始键就是开始 / 停止听
    if st.settings_raw().live.mode == "voice" {
        if crate::liveaudio::running() {
            crate::liveaudio::stop(app);
        } else if let Err(e) = crate::liveaudio::start_audio(app, &st) {
            let _ = app.emit("toast://error", e.message);
        }
        return;
    }
    let (hwnd, exe) = crate::writeassist::foreground();
    let own = crate::writeassist::is_own_window(hwnd);
    let raw = st.settings_raw();
    // 前台程序记过设置：直接按它的来
    let from_profile = (!own).then(|| raw.live.profile(&exe)).flatten().and_then(|p| match p.target.as_str() {
        "region" if p.region[2] > 0 => Some(Target { kind: "region".into(), rect: p.region, hwnd, exe: exe.clone(), title: String::new() }),
        "window" => Some(Target { kind: "window".into(), rect: [0; 4], hwnd, exe: exe.clone(), title: String::new() }),
        _ => None,
    });
    let target = from_profile.or_else(|| lock(&LAST).clone());
    match target {
        Some(t) => {
            if let Err(e) = start(app, &st, t, None) {
                let _ = app.emit("toast://error", e.message);
            }
        }
        None => open_picker(app),
    }
}

pub fn cycle_mode(app: &AppHandle) {
    // 屏幕会话里只有三种画面模式；没在翻译时多一个“语音”
    let next = |m: &str, with_voice: bool| match m {
        "standard" => "subtitle",
        "subtitle" => "manga",
        "manga" if with_voice => "voice",
        _ => "standard",
    };
    let cur = lock(&SESSION).as_ref().map(|s| lock_in(&s.mode).clone());
    match cur {
        Some(m) => set_mode(app, next(&m, false)),
        None => {
            if let Some(st) = livetx::state_of(app) {
                let m = next(&st.settings_raw().live.mode, true).to_string();
                save_live(&st, |l| l.mode = m);
                let _ = app.emit(EVT_STATE, status());
            }
        }
    }
}

pub fn set_mode(app: &AppHandle, mode: &str) {
    if !["standard", "subtitle", "manga", "voice"].contains(&mode) {
        return;
    }
    // 语音翻译不抓屏：屏幕会话里不能切到它（会话里只有三种画面模式）
    if mode == "voice" && lock(&SESSION).is_some() {
        return;
    }
    if let Some(s) = lock(&SESSION).as_ref() {
        *lock_in(&s.mode) = mode.to_string();
    }
    let _ = app.emit(EVT_STATE, status());
}

pub fn set_hidden(app: &AppHandle, hidden: Option<bool>) -> bool {
    let v = {
        let g = lock(&SESSION);
        let Some(sess) = g.as_ref() else { return false };
        let v = hidden.unwrap_or(!sess.hidden.load(Ordering::Relaxed));
        sess.hidden.store(v, Ordering::Relaxed);
        v
    };
    let _ = app.emit_to(OV, EVT_HIDDEN, v);
    let _ = app.emit_to(SUBBAR, EVT_HIDDEN, v);
    let _ = app.emit(EVT_STATE, status());
    v
}

/// 暂停 / 继续：暂停时停止抓屏，已贴的译文留在原处。
pub fn set_paused(app: &AppHandle, paused: Option<bool>) -> bool {
    let v = {
        let g = lock(&SESSION);
        let Some(sess) = g.as_ref() else { return false };
        let v = paused.unwrap_or(!sess.paused.load(Ordering::Relaxed));
        sess.paused.store(v, Ordering::Relaxed);
        v
    };
    let _ = app.emit(EVT_STATE, status());
    v
}

/// 全局快捷键入口：toggle / pick / hide / mode。
pub fn hotkey(app: &AppHandle, which: &str) {
    if app.try_state::<Arc<AppState>>().is_some_and(|st| crate::security::status(&st).locked) {
        return;
    }
    match which {
        "toggle" => toggle(app),
        "pick" => open_picker(app),
        "hide" => {
            set_hidden(app, None);
        }
        "mode" => cycle_mode(app),
        "shot" => {
            let _ = shot_request(app);
        }
        _ => {}
    }
}

// ---------- 取景窗口（框选区域 / 点选窗口） ----------

pub fn open_picker(app: &AppHandle) {
    let app = app.clone();
    // 先记下“用户当时在用哪个程序”，选完要按它存设置
    *lock(&PICK_EXE) = crate::writeassist::foreground();
    tauri::async_runtime::spawn_blocking(move || {
        close_picker(&app);
        std::thread::sleep(Duration::from_millis(80));
        match crate::scrcap::grab_screen() {
            Ok(g) => {
                let (x, y, w, h) = (g.x, g.y, g.w, g.h);
                *lock(&PICK_GRAB) = Some(g);
                if let Some(win) = small_window(&app, PICK, "livepick", "清影屏幕翻译取景", true) {
                    // 取景窗口要拿键盘（Esc 取消），而且要在截图里被截到之前就已冻结，所以不排除也行
                    let _ = win.set_ignore_cursor_events(false);
                    place(&win, [x, y, w as i32, h as i32]);
                    let _ = win.show();
                    let _ = win.set_focus();
                } else if crate::webhealth::is_broken() {
                    let _ = app.emit("toast://error", AppError::msg(crate::webhealth::HINT));
                }
            }
            Err(e) => {
                let _ = app.emit("toast://error", e);
            }
        }
    });
}

pub fn close_picker(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(PICK) {
        let _ = w.destroy();
    }
    *lock(&PICK_GRAB) = None;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickInfo {
    pub data_url: String,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub windows: Vec<livewin::WinInfo>,
}

#[tauri::command]
pub fn sc_pick_take() -> Option<PickInfo> {
    let g = lock(&PICK_GRAB).take()?;
    let data_url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&g.png));
    Some(PickInfo { data_url, x: g.x, y: g.y, w: g.w, h: g.h, windows: livewin::list_windows() })
}

#[tauri::command]
pub fn sc_pick_cancel(app: AppHandle) {
    close_picker(&app);
}

/// 框选完成：`hwnd` 不为 0 表示点选了一个窗口，否则用 `rect`（屏幕物理像素）。
#[tauri::command]
pub fn sc_pick_done(app: AppHandle, state: St<'_>, rect: [i32; 4], hwnd: isize, mode: Option<String>) -> AppResult<Status> {
    close_picker(&app);
    let (_, exe0) = lock(&PICK_EXE).clone();
    let t = if hwnd != 0 {
        Target { kind: "window".into(), rect, hwnd, exe: crate::writeassist::exe_of_hwnd(hwnd), title: String::new() }
    } else {
        Target { kind: "region".into(), rect, hwnd: lock(&PICK_EXE).0, exe: exe0, title: String::new() }
    };
    start(&app, state.inner(), t, mode)
}

// ---------- 命令 ----------

#[tauri::command]
pub fn sc_status() -> Status {
    status()
}

#[tauri::command]
pub fn sc_start(app: AppHandle, state: St<'_>, target: Option<Target>, mode: Option<String>) -> AppResult<Status> {
    let t = target.or_else(|| lock(&LAST).clone()).ok_or_else(|| AppError::invalid("还没有选择要翻译的区域。"))?;
    start(&app, state.inner(), t, mode)
}

#[tauri::command]
pub fn sc_stop(app: AppHandle) -> Status {
    stop(&app);
    status()
}

#[tauri::command]
pub fn sc_toggle(app: AppHandle) -> Status {
    toggle(&app);
    status()
}

#[tauri::command]
pub fn sc_pick(app: AppHandle) {
    open_picker(&app);
}

#[tauri::command]
pub fn sc_set_mode(app: AppHandle, state: St<'_>, mode: String) -> Status {
    if is_running() {
        set_mode(&app, &mode);
    } else {
        save_live(&state, |l| l.mode = mode);
    }
    status()
}

#[tauri::command]
pub fn sc_pause(app: AppHandle, paused: Option<bool>) -> bool {
    set_paused(&app, paused)
}

#[tauri::command]
pub fn sc_hide(app: AppHandle, hidden: Option<bool>) -> bool {
    set_hidden(&app, hidden)
}

#[tauri::command]
pub fn sc_windows() -> Vec<livewin::WinInfo> {
    livewin::list_windows()
}

#[tauri::command]
pub fn sc_ocr_langs() -> Vec<livewin::OcrLang> {
    livewin::ocr_languages()
}

#[tauri::command]
pub fn sc_overlay_state() -> Option<Value> {
    lock(&LAST_PAYLOAD).clone()
}

// ---------- 保存带译文的截图 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotFrame {
    /// 取景区（屏幕物理像素 x, y, w, h），也就是贴字层窗口的位置
    pub rect: [i32; 4],
    /// 干净的画面（没有译文）：`data:image/png;base64,…`
    pub png: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotSaved {
    pub path: String,
    pub copied: bool,
}

/// 抓一帧取景区的画面。贴字层是“不进截图”的窗口，所以这一帧里没有译文，由贴字层自己叠上去。
#[tauri::command]
pub fn sc_shot_frame() -> AppResult<ShotFrame> {
    let rect = *lock(&APPLIED);
    if rect[2] <= 0 || rect[3] <= 0 {
        return Err(AppError::invalid("还没有开始屏幕翻译，没有可以截的区域。"));
    }
    let f = livewin::grab_rect(rect[0], rect[1], rect[2] as u32, rect[3] as u32).map_err(AppError::msg)?;
    let png = crate::scrcap::encode_png(&crate::scrcap::bgra_to_rgb(&f.bgra), f.w as u32, f.h as u32).map_err(AppError::msg)?;
    Ok(ShotFrame { rect, png: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)) })
}

/// 存合成好的截图：写到“下载文件夹/屏幕翻译截图”，同时放进剪贴板。
#[tauri::command]
pub fn sc_shot_save(app: AppHandle, state: St<'_>, data: String) -> AppResult<ShotSaved> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let bytes = crate::liveshot::png_from_data(&data)?;
    let base = state.settings().download_dir.trim().to_string();
    let dir = if base.is_empty() { std::env::temp_dir() } else { std::path::PathBuf::from(base) }.join("屏幕翻译截图");
    std::fs::create_dir_all(&dir)?;
    let path = crate::liveshot::shot_name(&dir, chrono::Local::now());
    std::fs::write(&path, &bytes)?;
    let copied =
        crate::liveshot::png_to_rgba(&bytes).is_some_and(|(rgba, w, h)| app.clipboard().write_image(&tauri::image::Image::new_owned(rgba, w, h)).is_ok());
    let shown = path.to_string_lossy().to_string();
    emit_state(&app, &format!("已保存译图{}：{shown}", if copied { "（也已复制到剪贴板）" } else { "" }));
    Ok(ShotSaved { path: shown, copied })
}

/// 请贴字层把译文叠到干净画面上，存成图片。贴字层不在（还没开始翻译）时返回 false。
fn shot_request(app: &AppHandle) -> bool {
    if app.get_webview_window(OV).is_none() {
        emit_state(app, "还没有在翻译，先开始屏幕翻译再保存译图。");
        return false;
    }
    let _ = app.emit_to(OV, EVT_SHOT, ());
    true
}

#[tauri::command]
pub fn sc_shot(app: AppHandle) -> bool {
    shot_request(&app)
}

#[tauri::command]
pub fn sc_ack_manga(state: St<'_>) {
    save_live(&state, |l| l.manga_ack = true);
}

#[tauri::command]
pub fn sc_bar(app: AppHandle, state: St<'_>, on: bool) {
    save_live(&state, |l| l.float_button = on);
    crate::tray::sync_float(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TryResult {
    pub lines: Vec<String>,
    pub ms: u64,
    pub w: usize,
    pub h: usize,
    pub engine: String,
}

/// 试一次：抓一帧、识字，不翻译。设置页用来确认识字能不能用、认得准不准。
#[tauri::command]
pub async fn sc_try(state: St<'_>, target: Option<Target>) -> AppResult<TryResult> {
    let t = target.or_else(|| lock(&LAST).clone()).ok_or_else(|| AppError::invalid("先选一个区域或窗口。"))?;
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        livewin::init_thread();
        let rect = if t.kind == "window" {
            livewin::window_bounds(t.hwnd).map(|(x, y, w, h)| [x, y, w, h]).ok_or_else(|| AppError::msg("这个窗口找不到了。"))?
        } else {
            t.rect
        };
        let live = livetx::effective(&st.settings_raw().live, &t.exe, None);
        let t0 = Instant::now();
        let f = livewin::grab_rect(rect[0], rect[1], rect[2].max(1) as u32, rect[3].max(1) as u32).map_err(AppError::msg)?;
        let engine = if live.ocr == "vision" { "system".to_string() } else { live.ocr.clone() };
        let cx = crate::ocr::Ctx { data_dir: &st.data_dir, client: &st.dl_client, live: &live };
        let lines = crate::ocr::recognize(&cx, &engine, &f.bgra, f.w, f.h)?;
        let lines = if live.skip_icons { livecore::drop_icons(lines, Some((&f.bgra, f.w, f.h))) } else { lines };
        let groups = livecore::group_lines(&lines);
        Ok(TryResult { lines: groups.into_iter().map(|g| g.text).collect(), ms: t0.elapsed().as_millis() as u64, w: f.w, h: f.h, engine })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub fn sc_history(state: St<'_>, n: Option<usize>) -> Vec<livetx::Entry> {
    livetx::history_read(&state.data_dir, n.unwrap_or(200).min(1000))
}

#[tauri::command]
pub fn sc_history_clear(state: St<'_>) {
    livetx::history_clear(&state.data_dir);
}

#[tauri::command]
pub fn sc_cache_clear() {
    livetx::cache_clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ob(x: f32, y: f32, w: f32, h: f32, orig: &str, text: &str, font: f32) -> OBlock {
        OBlock {
            x,
            y,
            w,
            h,
            text: text.into(),
            orig: orig.into(),
            fg: "#fff".into(),
            bg: "#000".into(),
            font,
            vertical: false,
            lines: 1,
            center: false,
            align: 0,
        }
    }

    #[test]
    fn jitter_between_recognitions_is_absorbed_but_real_movement_is_not() {
        let prev = vec![ob(100.0, 200.0, 300.0, 40.0, "Start new game", "开始新游戏", 22.0), ob(100.0, 300.0, 300.0, 40.0, "Options", "选项", 22.0)];
        // 同一块：偏了 2 像素、宽高差 3、字号 23：全部沿用上一轮，译文不换
        let new = vec![ob(102.0, 199.0, 303.0, 41.0, "Start new game", "开始一局新游戏", 23.2)];
        let out = stabilize(&prev, new);
        assert_eq!((out[0].x, out[0].y, out[0].w, out[0].h, out[0].font), (100.0, 200.0, 300.0, 40.0, 22.0));
        assert_eq!(out[0].text, "开始新游戏");
        // 往下滚了 60 像素：按新的位置
        let moved = stabilize(&prev, vec![ob(100.0, 260.0, 300.0, 40.0, "Start new game", "开始新游戏", 22.0)]);
        assert_eq!(moved[0].y, 260.0);
        // 原文变了（换了一句）：不沿用旧译文
        let changed = stabilize(&prev, vec![ob(101.0, 200.0, 300.0, 40.0, "Continue game", "继续游戏", 22.0)]);
        assert_eq!(changed[0].text, "继续游戏");
        assert_eq!(changed[0].x, 101.0);
        // 两块不会都去配同一个旧块
        let two = stabilize(&prev, vec![ob(100.0, 201.0, 300.0, 40.0, "Options", "选项", 22.0), ob(100.0, 300.0, 300.0, 40.0, "Options", "选项", 22.0)]);
        assert_eq!(two.len(), 2);
        // 字号吸附到整数；竖排和横排不互相配
        assert_eq!(stabilize(&[], vec![ob(0.0, 0.0, 10.0, 10.0, "a", "b", 17.4)])[0].font, 17.0);
    }

    #[test]
    fn gate_waits_for_stability_then_processes() {
        let mut g = Gate::default();
        // 画面刚变：等
        assert_eq!(g.step(0, 0.5, 0.5, 0.03, 500, 2500), Act::Wait);
        assert_eq!(g.step(200, 0.0, 0.5, 0.03, 500, 2500), Act::Wait);
        assert_eq!(g.step(600, 0.0, 0.5, 0.03, 500, 2500), Act::Process);
        // 和上次处理的画面差不多：什么都不做
        assert_eq!(g.step(700, 0.0, 0.01, 0.03, 500, 2500), Act::Idle);
    }

    #[test]
    fn gate_forces_when_background_never_settles() {
        let mut g = Gate::default();
        assert_eq!(g.step(0, 0.2, 0.2, 0.03, 500, 2500), Act::Wait);
        assert_eq!(g.step(1000, 0.2, 0.3, 0.03, 500, 2500), Act::Wait);
        assert_eq!(g.step(2600, 0.2, 0.3, 0.03, 500, 2500), Act::Process);
        // 强制处理后重新计时
        assert_eq!(g.step(2700, 0.2, 0.3, 0.03, 500, 2500), Act::Wait);
    }

    #[test]
    fn subtitle_commits_once_when_stable() {
        let mut s = SubTrack::default();
        let f = |s: &mut SubTrack, t: u64, x: &str| s.feed(t, x, 300, 600, true, true);
        assert_eq!(f(&mut s, 0, "あの日から、ずっと探していたんだ。"), SubAct::Nothing);
        assert_eq!(f(&mut s, 150, "あの日から、ずっと探していたんだ。"), SubAct::Nothing);
        assert_eq!(f(&mut s, 320, "あの日から、ずっと探していたんだ。"), SubAct::Commit("あの日から、ずっと探していたんだ。".into()));
        // 同一句不重复；识字有一两个字抖动也不重复
        assert_eq!(f(&mut s, 500, "あの日から、ずっと探していたんだ。"), SubAct::Nothing);
        assert_eq!(f(&mut s, 700, "あの日から、ずっと探していたんた。"), SubAct::Nothing);
        // 消失不到 600ms 保留，之后撤掉
        assert_eq!(f(&mut s, 800, ""), SubAct::Nothing);
        assert_eq!(f(&mut s, 1200, ""), SubAct::Nothing);
        assert_eq!(f(&mut s, 1500, ""), SubAct::Clear);
        assert_eq!(f(&mut s, 1600, ""), SubAct::Nothing);
    }

    #[test]
    fn subtitle_merges_a_sentence_split_over_two_screens() {
        let mut s = SubTrack::default();
        let f = |s: &mut SubTrack, t: u64, x: &str| s.feed(t, x, 300, 600, true, true);
        f(&mut s, 0, "I have been looking for you");
        assert_eq!(f(&mut s, 350, "I have been looking for you"), SubAct::Commit("I have been looking for you".into()));
        // 上一屏没说完，紧接着换了下一屏：接成整句
        f(&mut s, 400, "ever since that day.");
        assert_eq!(f(&mut s, 750, "ever since that day."), SubAct::Commit("I have been looking for you ever since that day.".into()));
        // 句子结束后，新的一句是新的
        f(&mut s, 900, "Where did you go?");
        assert_eq!(f(&mut s, 1250, "Where did you go?"), SubAct::Commit("Where did you go?".into()));
    }

    #[test]
    fn subtitle_dedupe_off_translates_every_change() {
        let mut s = SubTrack::default();
        s.feed(0, "Hello there", 100, 600, false, false);
        assert!(matches!(s.feed(150, "Hello there", 100, 600, false, false), SubAct::Commit(_)));
        assert_eq!(s.feed(300, "Hello there", 100, 600, false, false), SubAct::Nothing);
    }

    #[test]
    fn blocks_follow_groups_and_scale() {
        let (w, h) = (100usize, 40usize);
        let mut bgra = vec![0u8; w * h * 4];
        for px in bgra.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[30, 30, 30, 255]);
        }
        let f = Frame { bgra, w, h };
        let g = Group { text: "开始".into(), x: 10.0, y: 5.0, w: 40.0, h: 20.0, vertical: false, lines: 1, em: 20.0, ..Default::default() };
        let live = LiveSettings { font_scale: 150, ..Default::default() };
        let b = blocks_from(&[g.clone(), g], &["Start".into(), "  ".into()], &f, &live);
        assert_eq!(b.len(), 1, "空译文的不贴");
        assert_eq!(b[0].text, "Start");
        assert!((b[0].font - 27.0).abs() < 0.01, "{}", b[0].font);
        assert_eq!(b[0].bg, "#1e1e1e");
    }
}
