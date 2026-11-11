//! 写作浮标（桌面端）：在其他程序里点进输入框时，在输入框右下角出现一个小浮标，点一下弹出写作窗。
//!
//! 判断“该不该出现”的规则是纯逻辑（[`decide`]），Linux 上也能测；系统监听（Windows UI Automation 的焦点事件）在 `win` 模块里。
//! 与浏览器扩展去重：扩展每 30 秒向本机报一次“我在这个浏览器里开着”，对应浏览器的窗口里桌面端不出浮标；
//! 扩展管不到的页面（浏览器自带页面、PDF 查看器等）在没有报到时桌面端会接手。

use crate::settings::WritingSettings;
use std::sync::Mutex;

/// 扩展报到后多久之内算“在线”（秒）。扩展 30 秒报一次，留出三次的余量。
const EXT_ALIVE_SECS: i64 = 100;

static EXT_SEEN: Mutex<Vec<(String, i64)>> = Mutex::new(Vec::new());
/// 最近一次出浮标的程序，托盘里“在这个程序里不再出现”用。
static LAST_APP: Mutex<String> = Mutex::new(String::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 浏览器进程名 → 扩展报到时用的浏览器系列。
pub fn browser_family(exe: &str) -> Option<&'static str> {
    match exe.trim().to_lowercase().as_str() {
        "msedge.exe" => Some("edge"),
        "firefox.exe" => Some("firefox"),
        "opera.exe" | "opera_gx.exe" => Some("opera"),
        "chrome.exe" | "brave.exe" | "vivaldi.exe" | "chromium.exe" | "360chrome.exe" | "360se.exe" | "qqbrowser.exe" | "sogouexplorer.exe"
        | "2345explorer.exe" => Some("chrome"),
        _ => None,
    }
}

/// 扩展报到。
pub fn mark_ext(family: &str, now: i64) {
    let f = family.trim().to_lowercase();
    if !["chrome", "edge", "firefox", "opera"].contains(&f.as_str()) {
        return;
    }
    let mut v = lock(&EXT_SEEN);
    v.retain(|(n, _)| *n != f);
    v.push((f, now));
}

/// 这个程序里扩展正在负责吗。
pub fn ext_covers(exe: &str, now: i64) -> bool {
    let Some(fam) = browser_family(exe) else { return false };
    lock(&EXT_SEEN).iter().any(|(n, t)| n == fam && now - *t <= EXT_ALIVE_SECS)
}

/// 名称、说明里带这些词的输入框不出浮标（密码、验证码、银行卡、身份证……）。
pub fn sensitive_name(s: &str) -> bool {
    let t = s.to_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "密码",
        "口令",
        "验证码",
        "短信码",
        "cvv",
        "cvc",
        "card number",
        "cardnumber",
        "银行卡",
        "卡号",
        "身份证",
        "支付",
        "pin code",
        "otp",
        "security code",
    ]
    .iter()
    .any(|k| t.contains(k))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Edit,
    Document,
    Other,
}

/// 焦点元素的情况。
#[derive(Debug, Clone)]
pub struct Focus {
    pub kind: Kind,
    pub password: bool,
    pub read_only: bool,
    /// 是否有可写的值接口（Document 类必须有，免得网页正文、只读文档也冒浮标）
    pub has_value: bool,
    pub name: String,
    /// 元素在屏幕上的位置和大小（物理像素）：左、上、右、下
    pub rect: (i32, i32, i32, i32),
    pub exe: String,
    pub fullscreen: bool,
    pub own: bool,
}

/// 不出浮标的原因，方便写日志和测试。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Skip {
    Disabled,
    Paused,
    OffApp,
    Own,
    Fullscreen,
    Extension,
    NotInput,
    Password,
    Sensitive,
    ReadOnly,
    Small,
}

/// 该不该出浮标；该出时给出浮标的锚点（输入框右下角，物理像素）。
pub fn decide(f: &Focus, cfg: &WritingSettings, now: i64) -> Result<(i32, i32), Skip> {
    if !cfg.badge {
        return Err(Skip::Disabled);
    }
    if cfg.paused(now) {
        return Err(Skip::Paused);
    }
    if f.own {
        return Err(Skip::Own);
    }
    if cfg.app_off(&f.exe) {
        return Err(Skip::OffApp);
    }
    if f.fullscreen {
        return Err(Skip::Fullscreen);
    }
    if ext_covers(&f.exe, now) {
        return Err(Skip::Extension);
    }
    match f.kind {
        Kind::Other => return Err(Skip::NotInput),
        Kind::Document if !f.has_value => return Err(Skip::NotInput),
        _ => {}
    }
    if f.password {
        return Err(Skip::Password);
    }
    if sensitive_name(&f.name) {
        return Err(Skip::Sensitive);
    }
    if f.read_only {
        return Err(Skip::ReadOnly);
    }
    let (l, t, r, b) = f.rect;
    // 太小的输入框（单行的小框）不出，和扩展的规则一致：宽至少 120、高至少 22
    if r - l < 120 || b - t < 22 {
        return Err(Skip::Small);
    }
    Ok((r, b))
}

/// 浮标的左上角：贴着输入框右下角内侧，限制在工作区里。
pub fn badge_pos(anchor: (i32, i32), size: i32, area: (i32, i32, i32, i32)) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let x = (anchor.0 - size - 6).min(ax + aw - size).max(ax);
    let y = (anchor.1 - size - 6).min(ay + ah - size).max(ay);
    (x, y)
}

pub fn last_app() -> String {
    lock(&LAST_APP).clone()
}

#[cfg(windows)]
pub use win::start;

/// 其他系统暂无自动浮标，只有快捷键。
#[cfg(not(windows))]
pub fn start(_app: &tauri::AppHandle) {}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::AppState;
    use std::sync::{mpsc, Arc, OnceLock};
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Manager};
    use windows::core::Interface;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, SetWinEventHook, UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_TextPatternId,
        UIA_ValuePatternId, UnhookWinEvent, HWINEVENTHOOK,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, GetWindowRect, TranslateMessage, EVENT_OBJECT_FOCUS, MSG, WINEVENT_OUTOFCONTEXT,
    };

    const BADGE: &str = "wbadge";
    const BADGE_SIZE: i32 = 28;
    /// 浮标没人理，多久后自己收起。
    const SHOW_FOR: Duration = Duration::from_secs(12);

    static TX: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();

    unsafe extern "system" fn on_focus(_h: HWINEVENTHOOK, _event: u32, _hwnd: HWND, _id: i32, _child: i32, _thread: u32, _time: u32) {
        if let Some(tx) = TX.get() {
            let _ = lock(tx).send(());
        }
    }

    fn now() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }

    pub fn start(app: &AppHandle) {
        let (tx, rx) = mpsc::channel::<()>();
        if TX.set(Mutex::new(tx)).is_err() {
            return;
        }
        // 线程一：挂焦点事件钩子并跑消息循环
        std::thread::spawn(|| unsafe {
            let hook = SetWinEventHook(EVENT_OBJECT_FOCUS, EVENT_OBJECT_FOCUS, None, Some(on_focus), 0, 0, WINEVENT_OUTOFCONTEXT);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWinEvent(hook);
        });
        // 线程二：查焦点元素、决定显示或收起
        let app = app.clone();
        std::thread::spawn(move || worker(app, rx));
    }

    fn worker(app: AppHandle, rx: mpsc::Receiver<()>) {
        let auto: IUIAutomation = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(a) => a,
                Err(e) => {
                    log::warn!("写作浮标：初始化 UI Automation 失败：{e}");
                    return;
                }
            }
        };
        let mut shown_at: Option<Instant> = None;
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(()) => {
                    // 合并连续的焦点事件，等界面稳定一下
                    std::thread::sleep(Duration::from_millis(120));
                    while rx.try_recv().is_ok() {}
                    if app.try_state::<Arc<AppState>>().is_none() {
                        continue;
                    }
                    match probe(&app, &auto) {
                        Some(pos) => {
                            show(&app, pos);
                            shown_at = Some(Instant::now());
                        }
                        None => {
                            hide(&app);
                            shown_at = None;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if shown_at.is_some_and(|t| t.elapsed() > SHOW_FOR) {
                        hide(&app);
                        shown_at = None;
                    } else if shown_at.is_none() && app.try_state::<Arc<AppState>>().is_some() {
                        // 有些程序不发焦点事件（或首次查询时还没开放无障碍），每秒补查一次
                        if let Some(pos) = probe(&app, &auto) {
                            show(&app, pos);
                            shown_at = Some(Instant::now());
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// 读当前焦点元素，交给 [`decide`]；该出浮标时返回浮标左上角（物理像素）。
    fn probe(app: &AppHandle, auto: &IUIAutomation) -> Option<(i32, i32)> {
        let cfg = app.try_state::<Arc<AppState>>()?.settings_raw().writing;
        if !cfg.badge || cfg.paused(now()) {
            return None;
        }
        let (hwnd, exe) = crate::writeassist::foreground();
        if hwnd == 0 {
            return None;
        }
        let own = exe == own_exe() || crate::writeassist::is_own_window(hwnd);
        let el = unsafe { auto.GetFocusedElement() }.ok()?;
        let ct = unsafe { el.CurrentControlType() }.ok()?;
        let mut kind = if ct == UIA_EditControlTypeId {
            Kind::Edit
        } else if ct == UIA_DocumentControlTypeId {
            Kind::Document
        } else {
            Kind::Other
        };
        let (mut has_value, mut read_only) = (false, false);
        if let Ok(p) = unsafe { el.GetCurrentPattern(UIA_ValuePatternId) } {
            if let Ok(v) = p.cast::<IUIAutomationValuePattern>() {
                has_value = true;
                read_only = unsafe { v.CurrentIsReadOnly() }.map(|b| b.as_bool()).unwrap_or(false);
            }
        }
        // 富文本框（Word、记事本、很多聊天框）只有文本接口没有值接口：按可写处理，写回时再试
        if !has_value && kind == Kind::Document && unsafe { el.GetCurrentPattern(UIA_TextPatternId) }.is_ok() {
            has_value = true;
        }
        // 控件类型是别的，但有可写的值接口（网页里的自绘输入框等）：当作输入框
        if kind == Kind::Other && has_value && !read_only {
            kind = Kind::Edit;
        }
        let password = unsafe { el.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false);
        let name = unsafe { el.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
        let aid = unsafe { el.CurrentAutomationId() }.map(|s| s.to_string()).unwrap_or_default();
        let r = unsafe { el.CurrentBoundingRectangle() }.unwrap_or_default();
        let mut area = (0, 0, 1920, 1080);
        let mut fullscreen = false;
        if let Ok(Some(m)) = app.monitor_from_point(f64::from(r.right), f64::from(r.bottom)) {
            area = (m.work_area().position.x, m.work_area().position.y, m.work_area().size.width as i32, m.work_area().size.height as i32);
            let mut wr = RECT::default();
            if unsafe { GetWindowRect(HWND(hwnd as *mut _), &mut wr) }.is_ok() {
                let (mp, ms) = (m.position(), m.size());
                fullscreen = wr.left <= mp.x && wr.top <= mp.y && wr.right >= mp.x + ms.width as i32 && wr.bottom >= mp.y + ms.height as i32;
            }
        }
        let f = Focus {
            kind,
            password,
            read_only,
            has_value,
            name: format!("{name} {aid}"),
            rect: (r.left, r.top, r.right, r.bottom),
            exe: exe.clone(),
            fullscreen,
            own,
        };
        match decide(&f, &cfg, now()) {
            Ok(anchor) => {
                *lock(&LAST_APP) = exe;
                crate::writeassist::remember_target(hwnd);
                Some(badge_pos(anchor, BADGE_SIZE, area))
            }
            Err(why) => {
                log::debug!("写作浮标：不出现（{why:?}）");
                None
            }
        }
    }

    fn own_exe() -> String {
        std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default()
    }

    fn show(app: &AppHandle, pos: (i32, i32)) {
        use tauri::{PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
        if let Some(w) = app.get_webview_window(BADGE) {
            let _ = w.set_position(PhysicalPosition::new(pos.0, pos.1));
            return;
        }
        // 窗口组件断开时，每隔几秒就试一次只会刷日志，隔几分钟再试
        if !crate::webhealth::should_try() {
            return;
        }
        let built = WebviewWindowBuilder::new(app, BADGE, WebviewUrl::App("index.html#wbadge".into()))
            .title("清影写作浮标")
            .inner_size(f64::from(BADGE_SIZE), f64::from(BADGE_SIZE))
            .resizable(false)
            .decorations(false)
            .shadow(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .build();
        match built {
            Ok(w) => {
                crate::webhealth::note_ok();
                crate::winframe::guard(&w);
                let _ = w.set_size(PhysicalSize::new(BADGE_SIZE as u32, BADGE_SIZE as u32));
                let _ = w.set_position(PhysicalPosition::new(pos.0, pos.1));
                crate::winframe::show_no_activate(&w);
            }
            Err(e) => {
                if !crate::webhealth::note_failure(app, "写作浮标", &e) {
                    log::warn!("创建写作浮标失败：{e}");
                }
            }
        }
    }

    pub fn hide(app: &AppHandle) {
        if let Some(w) = app.get_webview_window(BADGE) {
            let _ = w.destroy();
        }
    }
}

/// 收起浮标（其他系统没有浮标，什么也不做）。
pub fn hide(app: &tauri::AppHandle) {
    #[cfg(windows)]
    win::hide(app);
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus() -> Focus {
        Focus {
            kind: Kind::Edit,
            password: false,
            read_only: false,
            has_value: true,
            name: "消息".into(),
            rect: (100, 100, 700, 220),
            exe: "weixin.exe".into(),
            fullscreen: false,
            own: false,
        }
    }
    fn cfg() -> WritingSettings {
        WritingSettings::default()
    }

    #[test]
    fn badge_shows_at_the_input_corner() {
        assert_eq!(decide(&focus(), &cfg(), 1000), Ok((700, 220)));
    }

    #[test]
    fn badge_stays_away_from_private_or_unfit_places() {
        let c = cfg();
        let mut f = focus();
        f.password = true;
        assert_eq!(decide(&f, &c, 1), Err(Skip::Password));
        f = focus();
        f.name = "请输入验证码 code".into();
        assert_eq!(decide(&f, &c, 1), Err(Skip::Sensitive));
        f = focus();
        f.read_only = true;
        assert_eq!(decide(&f, &c, 1), Err(Skip::ReadOnly));
        f = focus();
        f.rect = (0, 0, 100, 20);
        assert_eq!(decide(&f, &c, 1), Err(Skip::Small));
        f = focus();
        f.fullscreen = true;
        assert_eq!(decide(&f, &c, 1), Err(Skip::Fullscreen));
        f = focus();
        f.own = true;
        assert_eq!(decide(&f, &c, 1), Err(Skip::Own));
        f = focus();
        f.kind = Kind::Other;
        assert_eq!(decide(&f, &c, 1), Err(Skip::NotInput));
        f = focus();
        f.kind = Kind::Document;
        f.has_value = false;
        assert_eq!(decide(&f, &c, 1), Err(Skip::NotInput), "没有可写值接口的文档（网页正文、只读文档）不出");
    }

    #[test]
    fn settings_switch_the_badge_off() {
        let f = focus();
        assert_eq!(decide(&f, &WritingSettings { badge: false, ..cfg() }, 1), Err(Skip::Disabled));
        assert_eq!(decide(&f, &WritingSettings { paused_until: 500, ..cfg() }, 100), Err(Skip::Paused));
        assert!(decide(&f, &WritingSettings { paused_until: 500, ..cfg() }, 600).is_ok(), "暂停到点自动恢复");
        assert_eq!(decide(&f, &WritingSettings { off_apps: vec!["weixin.exe".into()], ..cfg() }, 1), Err(Skip::OffApp));
    }

    #[test]
    fn desktop_yields_to_a_live_extension_only_in_that_browser() {
        let mut f = focus();
        f.exe = "msedge.exe".into();
        assert!(decide(&f, &cfg(), 5000).is_ok(), "扩展没报到：桌面端接手");
        mark_ext("edge", 5000);
        assert_eq!(decide(&f, &cfg(), 5050), Err(Skip::Extension));
        f.exe = "chrome.exe".into();
        assert!(decide(&f, &cfg(), 5050).is_ok(), "Edge 的扩展不管 Chrome");
        f.exe = "msedge.exe".into();
        assert!(decide(&f, &cfg(), 5000 + EXT_ALIVE_SECS + 1).is_ok(), "扩展一段时间没报到：桌面端接手");
        mark_ext("bogus", 5000);
        assert!(!ext_covers("bogus.exe", 5000));
    }

    #[test]
    fn browser_families() {
        assert_eq!(browser_family("Brave.exe"), Some("chrome"));
        assert_eq!(browser_family("firefox.exe"), Some("firefox"));
        assert_eq!(browser_family("winword.exe"), None);
    }

    #[test]
    fn sensitive_words() {
        assert!(sensitive_name("Password"));
        assert!(sensitive_name("银行卡号"));
        assert!(!sensitive_name("给王经理的回复"));
    }

    #[test]
    fn badge_is_clamped_into_the_work_area() {
        let area = (0, 0, 1920, 1040);
        assert_eq!(badge_pos((700, 220), 28, area), (666, 186));
        assert_eq!(badge_pos((1930, 1060), 28, area), (1892, 1012));
        assert_eq!(badge_pos((10, 10), 28, area), (0, 0));
    }
}
