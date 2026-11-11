//! 批量下载：一次添加的多条链接组成一个“批次”。
//!
//! - 预览：逐条解析，去重、标出已下载过 / 已在队列 / 需要登录 / 不支持，边解析边通知界面；
//! - 加入队列：按整批设置（清晰度、保存位置、批次文件夹、并发、间隔、限速、完成动作、下载时段）建任务；
//!   播放列表展开出的条目只带链接，后台按同站间隔逐条解析，未处理完的链接随批次保存，重启后继续；
//! - 队列调度：批次并发上限、同网站相邻两条之间的随机间隔、只在设定时段下载、批次限速、批次完成后的动作；
//! - 批次操作：整批暂停 / 继续 / 取消 / 清理已完成、重试失败项、导出失败清单。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::download::{self, EnqueueExtra, TaskStatus};
use crate::error::ErrorKind;
use crate::model::{AppResult, MediaInfo, MediaKind, PlaylistEntry};
use crate::net::SpeedLimiter;
use crate::settings::{QualityPreset, Settings};
use crate::{providers, quality, AppState};

pub const EVT_PREVIEW: &str = "batch://preview";
pub const EVT_BATCHES: &str = "batch://changed";
/// 外部（浏览器扩展）请求主窗口打开批量添加面板
pub const EVT_OPEN: &str = "batch://open";
/// 一次最多处理的链接数
pub const MAX_LINKS: usize = 500;
/// 读取链接清单文件的大小上限
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// 任务所属的批次和在批次里的序号（从 1 开始）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BatchTag {
    pub id: String,
    pub seq: u32,
}

/// 整批设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchOpts {
    /// 批次名；留空自动生成（批量-月日-时分）
    pub name: String,
    /// 保存位置；留空用设置里的下载目录
    pub dir: String,
    /// 放进以批次名命名的子文件夹
    pub folder: bool,
    /// 清晰度：空表示用设置里的；best / 1080 / 720 / audio
    pub quality: String,
    /// 这批同时下载的任务数；0 表示不单独限制
    pub concurrency: usize,
    /// 同一网站相邻两条开始之间的随机间隔（秒）
    pub jitter_min: u32,
    pub jitter_max: u32,
    /// 整批限速（KB/s），0 表示不限
    pub speed_limit_kbps: u64,
    /// 批次全部完成后：none / notify / open / sleep / shutdown
    pub after_done: String,
    /// 只在设置里的下载时段内下载
    pub use_window: bool,
    /// 加入后先保持暂停
    pub start_paused: bool,
    /// 文件名模板（可用 {index} 序号）；留空用设置里的
    pub template: String,
}

impl Default for BatchOpts {
    fn default() -> Self {
        BatchOpts {
            name: String::new(),
            dir: String::new(),
            folder: false,
            quality: String::new(),
            concurrency: 0,
            jitter_min: 2,
            jitter_max: 6,
            speed_limit_kbps: 0,
            after_done: "none".into(),
            use_window: false,
            start_paused: false,
            template: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingLink {
    pub url: String,
    pub title: String,
    pub seq: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FailedLink {
    pub url: String,
    pub title: String,
    pub error: String,
}

/// 保存下来的批次。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchRec {
    pub id: String,
    pub name: String,
    pub source: String,
    pub created_at: i64,
    pub opts: BatchOpts,
    pub total: usize,
    /// 还没解析的链接（播放列表展开的条目）
    pub pending: Vec<PendingLink>,
    /// 解析失败的链接
    pub failed: Vec<FailedLink>,
    /// 批次完成动作已经执行过
    pub fired: bool,
    /// 保存位置（任务详情里显示、完成后打开）
    pub base_dir: String,
}

/// 界面里的批次：保存的内容加上任务计数。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchView {
    pub id: String,
    pub name: String,
    pub source: String,
    pub created_at: i64,
    pub opts: BatchOpts,
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub paused: usize,
    pub done: usize,
    pub failed: usize,
    pub pending: usize,
    pub failed_links: Vec<FailedLink>,
    pub base_dir: String,
}

pub enum Gate {
    Go,
    /// 同一网站刚开始过一条，等这么久
    Wait(Duration),
    Block,
}

#[derive(Default)]
pub struct BatchStore {
    recs: Mutex<Vec<BatchRec>>,
    limiters: Mutex<HashMap<String, Arc<SpeedLimiter>>>,
    site_next: Mutex<HashMap<String, Instant>>,
    wake: AtomicBool,
    window_paused: Mutex<HashSet<i64>>,
    workers: Mutex<HashSet<String>>,
    cancel_preview: Mutex<HashSet<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn state(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

/// 此刻是否在设置里的下载时段内。
pub fn in_window(settings: &Settings) -> bool {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    settings.run_window.contains(now.weekday().number_from_monday() as u8, now.hour() * 60 + now.minute())
}

fn rand_between(lo: u32, hi: u32) -> u32 {
    if hi <= lo {
        return lo;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    lo + nanos % (hi - lo + 1)
}

fn min_nonzero(a: u64, b: u64) -> u64 {
    match (a, b) {
        (0, x) | (x, 0) => x,
        (x, y) => x.min(y),
    }
}

impl BatchStore {
    fn with_rec<R>(&self, id: &str, f: impl FnOnce(&mut BatchRec) -> R) -> Option<R> {
        lock(&self.recs).iter_mut().find(|r| r.id == id).map(f)
    }

    pub fn snapshot(&self) -> Vec<BatchRec> {
        lock(&self.recs).clone()
    }

    /// 这条任务现在能不能开始：批次并发上限、下载时段、同网站间隔。
    pub fn gate(&self, id: &str, site: &str, running_in_batch: usize, settings: &Settings) -> Gate {
        let recs = lock(&self.recs);
        let Some(r) = recs.iter().find(|r| r.id == id) else { return Gate::Go };
        if r.opts.concurrency > 0 && running_in_batch >= r.opts.concurrency {
            return Gate::Block;
        }
        if r.opts.use_window && settings.run_window_on && !in_window(settings) && settings.run_window.limit_kbps == 0 {
            return Gate::Block;
        }
        if r.opts.jitter_max > 0 {
            if let Some(t) = lock(&self.site_next).get(site) {
                let now = Instant::now();
                if *t > now {
                    return Gate::Wait(*t - now);
                }
            }
        }
        Gate::Go
    }

    /// 同一网站开始了一条：下一条至少要等随机的一小段时间。
    pub fn note_start(&self, id: &str, site: &str, _settings: &Settings) {
        let Some((lo, hi)) = self.with_rec(id, |r| (r.opts.jitter_min, r.opts.jitter_max)) else { return };
        if hi == 0 {
            return;
        }
        let gap = Duration::from_secs(rand_between(lo, hi) as u64);
        lock(&self.site_next).insert(site.to_string(), Instant::now() + gap);
    }

    /// 批次此刻的限速（KB/s）：批次自己的限速和“下载时段外”的限速取更小的。
    fn effective_kbps(&self, id: &str, settings: &Settings) -> u64 {
        let Some((own, use_window)) = self.with_rec(id, |r| (r.opts.speed_limit_kbps, r.opts.use_window)) else { return 0 };
        if use_window && settings.run_window_on && !in_window(settings) {
            min_nonzero(own, settings.run_window.limit_kbps)
        } else {
            own
        }
    }

    /// 批次共用的限速器（没有限速时为 None）。
    pub fn limiter_for(&self, id: &str, settings: &Settings) -> Option<(Arc<SpeedLimiter>, u64)> {
        let kbps = self.effective_kbps(id, settings);
        if kbps == 0 {
            return None;
        }
        let lim = lock(&self.limiters).entry(id.to_string()).or_default().clone();
        Some((lim, kbps))
    }

    /// 不能共用限速器的下载方式（yt-dlp 自己限速）：把批次限速平均分给批次里同时下载的任务。
    /// `remaining`：批次里还没结束的任务数（含这条）。收尾阶段只剩一两条时，
    /// 每条分到的份额按实际数量算，而不是按并发上限算，否则最后几条会被白白压慢。
    pub fn task_limit(&self, tag: Option<&BatchTag>, settings: &Settings, global: u64, remaining: usize) -> u64 {
        let Some(tag) = tag else { return global };
        let kbps = self.effective_kbps(&tag.id, settings);
        if kbps == 0 {
            return global;
        }
        let conc = self.with_rec(&tag.id, |r| r.opts.concurrency).unwrap_or(0);
        let conc = if conc == 0 { settings.concurrency.max(1) } else { conc };
        let share = conc.min(remaining.max(1)) as u64;
        min_nonzero(global, (kbps / share).max(1))
    }
}

fn persist(st: &AppState, rec: &BatchRec) {
    if let Err(e) = st.db.save_batch(&rec.id, &serde_json::to_string(rec).unwrap_or_default(), rec.created_at) {
        log::warn!("save batch {} failed: {e}", rec.id);
    }
}

fn changed(app: &AppHandle) {
    let _ = app.emit(EVT_BATCHES, ());
}

/// 一段时间后重新调度（用于等待同网站的间隔）。
pub fn wake_later(app: &AppHandle, d: Duration) {
    let st = state(app);
    if st.batches.wake.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(d + Duration::from_millis(80)).await;
        state(&app).batches.wake.store(false, Ordering::SeqCst);
        download::schedule(&app);
    });
}

// ---------- 启动恢复 ----------

/// 启动时读取保存的批次，继续解析还没处理完的链接。
pub fn restore(app: &AppHandle) {
    let st = state(app);
    let rows = st.db.load_batches().unwrap_or_default();
    let recs: Vec<BatchRec> = rows.iter().filter_map(|j| serde_json::from_str(j).ok()).collect();
    let resume: Vec<String> = recs.iter().filter(|r| !r.pending.is_empty()).map(|r| r.id.clone()).collect();
    *lock(&st.batches.recs) = recs;
    for id in resume {
        spawn_worker(app, id);
    }
}

// ---------- 链接提取 ----------

/// 去掉不影响内容的参数和片段，作为去重的依据。
pub fn normalize_key(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url.trim()) else { return url.trim().to_string() };
    u.set_fragment(None);
    let keep: Vec<(String, String)> = u
        .query_pairs()
        .filter(|(k, _)| {
            !matches!(
                k.as_ref(),
                "t" | "si"
                    | "feature"
                    | "pp"
                    | "spm_id_from"
                    | "vd_source"
                    | "share_source"
                    | "utm_source"
                    | "utm_medium"
                    | "utm_campaign"
                    | "from"
                    | "share_token"
            )
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    u.set_query(None);
    let path = u.path().trim_end_matches('/').to_string();
    u.set_path(&path);
    if !keep.is_empty() {
        u.query_pairs_mut().extend_pairs(keep);
    }
    u.to_string().trim_end_matches('/').to_ascii_lowercase()
}

/// 从文字里取出所有链接（网页链接和 ed2k / 磁力 / FTP 等），按出现顺序去重。
pub fn extract(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    let mut push = |u: String| {
        if seen.insert(normalize_key(&u)) {
            out.push(u);
        }
    };
    for u in providers::extract_urls(text) {
        push(u);
    }
    for u in crate::links::extract_links(text) {
        push(u);
    }
    out
}

/// 从清单文件里取链接：.html 里读 href，.csv 里按行取，其余当作普通文字。
pub fn extract_file_text(name: &str, text: &str) -> Vec<String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        static HREF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)href\s*=\s*["']([^"']+)["']"#).unwrap());
        let joined: Vec<String> = HREF.captures_iter(text).map(|c| c[1].replace("&amp;", "&")).collect();
        return extract(&joined.join("\n"));
    }
    extract(text)
}

// ---------- 预览 ----------

/// 预览里一条链接的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub key: String,
    pub index: usize,
    pub url: String,
    /// ok / queued（已在队列）/ done（之前下载过）/ dup（重复）/ login / bad / file / playlist
    pub status: String,
    pub title: String,
    pub platform: String,
    pub platform_name: String,
    /// 需要登录时用来打开登录窗口的网站
    pub site: String,
    pub duration_ms: Option<u64>,
    pub size_est: Option<u64>,
    pub quality: String,
    pub error: Option<String>,
    pub media: Option<MediaInfo>,
    pub entries: Vec<PlaylistEntry>,
}

fn blank(key: &str, index: usize, url: &str) -> PreviewItem {
    PreviewItem {
        key: key.to_string(),
        index,
        url: url.to_string(),
        status: "bad".into(),
        title: String::new(),
        platform: String::new(),
        platform_name: String::new(),
        site: crate::inbox::site_for(url),
        duration_ms: None,
        size_est: None,
        quality: String::new(),
        error: None,
        media: None,
        entries: vec![],
    }
}

fn preset_of(q: &str) -> Option<QualityPreset> {
    match q {
        "best" => Some(QualityPreset::Best),
        "1080" => Some(QualityPreset::Max1080),
        "720" => Some(QualityPreset::Small),
        "audio" => Some(QualityPreset::Audio),
        _ => None,
    }
}

/// 套用整批的清晰度选择。
fn settings_for(settings: &Settings, quality: &str) -> Settings {
    let mut s = settings.clone();
    if let Some(p) = preset_of(quality) {
        s.quality_preset = p;
    }
    s
}

fn classify(settings: &Settings, st: &AppState, info: MediaInfo, mut item: PreviewItem, seen: &Mutex<HashSet<String>>) -> PreviewItem {
    item.title = info.title.clone();
    item.platform = info.platform.clone();
    item.platform_name = info.platform_name.clone();
    item.duration_ms = info.duration_ms;
    if info.kind == MediaKind::Playlist {
        item.status = "playlist".into();
        item.entries = info.entries.clone();
        return item;
    }
    let (ids, _) = quality::auto_selection(&info, settings);
    let picked: Vec<&crate::model::Asset> = ids.iter().filter_map(|id| info.asset(id)).collect();
    item.quality = picked.iter().find_map(|a| a.quality.clone()).unwrap_or_default();
    item.size_est = picked.iter().filter_map(|a| quality::estimated_size(&info, a)).reduce(|a, b| a + b);
    let key = format!("{}|{}", info.platform, info.id);
    if !lock(seen).insert(key) {
        item.status = "dup".into();
        item.media = Some(info);
        return item;
    }
    let in_queue = st.downloads.snapshots().iter().any(|t| {
        t.platform == info.platform
            && t.media_id == info.id
            && ids.contains(&t.asset_id)
            && matches!(t.status, TaskStatus::Queued | TaskStatus::Running | TaskStatus::Paused)
    });
    let downloaded = !ids.is_empty() && ids.iter().all(|id| st.db.existing_download(&info.platform, &info.id, id).ok().flatten().is_some());
    item.status = if in_queue {
        "queued"
    } else if downloaded {
        "done"
    } else if info.kind == MediaKind::File {
        "file"
    } else {
        "ok"
    }
    .into();
    item.media = Some(info);
    item
}

/// 后台逐条解析 `urls`，每解析完一条发一次预览事件。同网站的请求之间保持间隔；用 `cancel_preview` 取消。
pub async fn preview(app: AppHandle, key: String, urls: Vec<String>, quality: String) {
    let st = state(&app);
    lock(&st.batches.cancel_preview).remove(&key);
    let settings = settings_for(&st.settings(), &quality);
    let seen: Arc<Mutex<HashSet<String>>> = Default::default();
    let gap = Duration::from_millis(settings.site_request_interval_ms.max(400));
    let jobs = urls.into_iter().enumerate().map(|(i, url)| {
        let (app, st, settings, seen, key) = (app.clone(), st.clone(), settings.clone(), seen.clone(), key.clone());
        async move {
            if lock(&st.batches.cancel_preview).contains(&key) {
                return;
            }
            let mut item = blank(&key, i, &url);
            let site = if item.site.is_empty() { url.clone() } else { item.site.clone() };
            st.net.wait_turn(&site, gap).await;
            if lock(&st.batches.cancel_preview).contains(&key) {
                return;
            }
            let ctx = st.parse_ctx(&settings);
            let item = match providers::resolve_url(&ctx, &url).await {
                Ok(info) => classify(&settings, &st, info, item, &seen),
                Err(e) => {
                    item.status = if e.kind == ErrorKind::NeedLogin { "login" } else { "bad" }.into();
                    item.error = Some(e.message);
                    item
                }
            };
            let _ = app.emit(EVT_PREVIEW, item);
        }
    });
    futures_util::stream::iter(jobs).buffer_unordered(3).collect::<Vec<()>>().await;
    let _ = app.emit(EVT_PREVIEW, serde_json::json!({ "key": key, "finished": true }));
}

pub fn cancel_preview(app: &AppHandle, key: &str) {
    lock(&state(app).batches.cancel_preview).insert(key.to_string());
}

// ---------- 加入队列 ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// 预览时已解析好的结果；没有的（播放列表展开的条目）稍后在后台解析
    #[serde(default)]
    pub media: Option<MediaInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub id: String,
    pub name: String,
    pub queued: usize,
    pub already_downloaded: usize,
    pub already_queued: usize,
    pub pending: usize,
}

fn seq_label(seq: u32, total: usize) -> String {
    let width = total.to_string().len().max(2);
    format!("{seq:0width$}")
}

fn extra_for(rec: &BatchRec, seq: u32, settings: &Settings) -> EnqueueExtra {
    let root = if rec.opts.dir.trim().is_empty() { settings.download_root() } else { std::path::PathBuf::from(rec.opts.dir.trim()) };
    let custom_root = !rec.opts.dir.trim().is_empty();
    let dir = if rec.opts.folder {
        Some(root.join(crate::naming::sanitize(&rec.name)))
    } else if custom_root {
        Some(root)
    } else {
        None
    };
    EnqueueExtra {
        dir,
        template: (!rec.opts.template.trim().is_empty()).then(|| rec.opts.template.trim().to_string()),
        origin: Some("batch".into()),
        batch: Some(BatchTag { id: rec.id.clone(), seq }),
        seq_label: Some(seq_label(seq, rec.total)),
        ..Default::default()
    }
}

fn enqueue_media(app: &AppHandle, rec: &BatchRec, seq: u32, mut info: MediaInfo, settings: &Settings, acc: &mut BatchResult) -> Result<(), String> {
    let s = settings_for(settings, &rec.opts.quality);
    quality::sort_videos(&mut info, &s);
    let (ids, post) = quality::auto_selection(&info, &s);
    if ids.is_empty() {
        return Err("没有可下载的内容".into());
    }
    let r = download::enqueue_ext(app, info, &ids, post, extra_for(rec, seq, settings)).map_err(|e| e.message)?;
    acc.queued += r.tasks.len();
    acc.already_downloaded += r.already_downloaded;
    acc.already_queued += r.already_queued;
    if rec.opts.start_paused {
        for t in &r.tasks {
            download::pause(app, t.id);
        }
    }
    Ok(())
}

pub fn enqueue(app: &AppHandle, items: Vec<BatchItem>, mut opts: BatchOpts, source: String) -> AppResult<BatchResult> {
    if items.is_empty() {
        return Err(crate::error::AppError::invalid("没有要下载的内容。"));
    }
    if items.len() > MAX_LINKS {
        return Err(crate::error::AppError::invalid(format!("一次最多 {MAX_LINKS} 条，请分批添加。")));
    }
    let st = state(app);
    let settings = st.settings();
    let now = chrono::Local::now();
    if opts.name.trim().is_empty() {
        opts.name = format!("批量-{}", now.format("%m%d-%H%M"));
    }
    opts.name = opts.name.trim().to_string();
    opts.jitter_max = opts.jitter_max.min(120);
    opts.jitter_min = opts.jitter_min.min(opts.jitter_max);
    let base = if opts.dir.trim().is_empty() { settings.download_root() } else { std::path::PathBuf::from(opts.dir.trim()) };
    let base_dir = if opts.folder { base.join(crate::naming::sanitize(&opts.name)) } else { base };
    let mut rec = BatchRec {
        id: format!("b{:x}{:04x}", now.timestamp_millis(), rand_between(0, 0xffff)),
        name: opts.name.clone(),
        source,
        created_at: crate::db::now(),
        total: items.len(),
        opts,
        base_dir: base_dir.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut acc = BatchResult { id: rec.id.clone(), name: rec.name.clone(), queued: 0, already_downloaded: 0, already_queued: 0, pending: 0 };
    // 先登记批次（任务创建时调度器要找得到它）
    lock(&st.batches.recs).push(rec.clone());
    for (i, it) in items.into_iter().enumerate() {
        let seq = i as u32 + 1;
        match it.media {
            Some(info) if info.kind != MediaKind::Playlist => {
                if let Err(e) = enqueue_media(app, &rec, seq, info.clone(), &settings, &mut acc) {
                    rec.failed.push(FailedLink { url: if info.source_url.is_empty() { it.url } else { info.source_url }, title: info.title, error: e });
                }
            }
            _ => rec.pending.push(PendingLink { url: it.url, title: it.title, seq }),
        }
    }
    acc.pending = rec.pending.len();
    st.batches.with_rec(&rec.id, |r| {
        r.pending = rec.pending.clone();
        r.failed = rec.failed.clone();
    });
    persist(&st, &rec);
    if !rec.pending.is_empty() {
        spawn_worker(app, rec.id.clone());
    }
    changed(app);
    download::schedule(app);
    Ok(acc)
}

/// 不经过预览面板，按设置里的默认选项把一组链接整批加入队列（悬浮窗、迷你窗、手机发来的多条链接）。
pub fn enqueue_quick(app: &AppHandle, urls: Vec<String>, source: &str) -> AppResult<BatchResult> {
    let st = state(app);
    let s = st.settings();
    let mut urls = urls;
    urls.truncate(MAX_LINKS);
    let opts = BatchOpts {
        folder: urls.len() >= s.batch_folder_min,
        jitter_min: s.batch_jitter_min,
        jitter_max: s.batch_jitter_max,
        concurrency: s.per_site_concurrency.clamp(1, 5),
        after_done: "notify".into(),
        ..Default::default()
    };
    let items = urls.into_iter().map(|url| BatchItem { url, title: String::new(), media: None }).collect();
    enqueue(app, items, opts, source.to_string())
}

/// 后台逐条解析批次里还没处理的链接并加入队列：同一网站两条之间按批次的间隔等待。
fn spawn_worker(app: &AppHandle, id: String) {
    let st = state(app);
    if !lock(&st.batches.workers).insert(id.clone()) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = state(&app);
        while let Some((rec, next)) = st.batches.with_rec(&id, |r| (r.clone(), r.pending.first().cloned())).and_then(|(r, n)| n.map(|n| (r, n))) {
            let settings = st.settings();
            let (lo, hi) = (rec.opts.jitter_min, rec.opts.jitter_max);
            let site = crate::inbox::site_for(&next.url);
            let site = if site.is_empty() { next.url.clone() } else { site };
            let gap = Duration::from_secs(rand_between(lo, hi).max(1) as u64).max(Duration::from_millis(settings.site_request_interval_ms));
            st.net.wait_turn(&site, gap).await;
            // 批次已被删除
            if st.batches.with_rec(&id, |_| ()).is_none() {
                break;
            }
            let s = settings_for(&settings, &rec.opts.quality);
            let ctx = st.parse_ctx(&s);
            let outcome = providers::resolve_url(&ctx, &next.url).await;
            let mut acc = BatchResult { id: id.clone(), name: rec.name.clone(), queued: 0, already_downloaded: 0, already_queued: 0, pending: 0 };
            let failure = match outcome {
                Ok(info) if info.kind == MediaKind::Playlist => {
                    Some(format!("这个链接是个列表（{} 个视频），请在“解析”页单独打开，再选择要下载的视频", info.entries.len()))
                }
                Ok(info) => enqueue_media(&app, &rec, next.seq, info, &settings, &mut acc).err(),
                Err(e) => Some(e.message),
            };
            let snapshot = st.batches.with_rec(&id, |r| {
                r.pending.retain(|p| !(p.url == next.url && p.seq == next.seq));
                if let Some(error) = failure {
                    r.failed.push(FailedLink { url: next.url.clone(), title: next.title.clone(), error });
                }
                r.clone()
            });
            if let Some(r) = snapshot {
                persist(&st, &r);
            }
            changed(&app);
        }
        lock(&st.batches.workers).remove(&id);
        download::schedule(&app);
    });
}

// ---------- 列表与操作 ----------

pub fn list(app: &AppHandle) -> Vec<BatchView> {
    let st = state(app);
    let snaps = st.downloads.snapshots();
    st.batches
        .snapshot()
        .into_iter()
        .rev()
        .map(|r| {
            let mine: Vec<_> = snaps.iter().filter(|t| t.batch_id.as_deref() == Some(r.id.as_str())).collect();
            let count = |f: &dyn Fn(TaskStatus) -> bool| mine.iter().filter(|t| f(t.status)).count();
            BatchView {
                queued: count(&|s| s == TaskStatus::Queued),
                running: count(&|s| s == TaskStatus::Running),
                paused: count(&|s| s == TaskStatus::Paused),
                done: count(&|s| s == TaskStatus::Done),
                failed: count(&|s| s == TaskStatus::Failed),
                pending: r.pending.len(),
                failed_links: r.failed.clone(),
                id: r.id,
                name: r.name,
                source: r.source,
                created_at: r.created_at,
                opts: r.opts,
                total: r.total,
                base_dir: r.base_dir,
            }
        })
        .collect()
}

fn task_ids(st: &AppState, id: &str, f: impl Fn(TaskStatus) -> bool) -> Vec<i64> {
    st.downloads.snapshots().into_iter().filter(|t| t.batch_id.as_deref() == Some(id) && f(t.status)).map(|t| t.id).collect()
}

/// 批次操作：pause / resume / cancel / clear（移除已完成和已取消的）/ retry（重试失败项）/ delete（取消并移除整个批次）。
pub fn action(app: &AppHandle, id: &str, what: &str) -> AppResult<()> {
    let st = state(app);
    match what {
        "pause" => {
            for t in task_ids(&st, id, |s| matches!(s, TaskStatus::Queued | TaskStatus::Running)) {
                download::pause(app, t);
            }
        }
        "resume" => {
            for t in task_ids(&st, id, |s| s == TaskStatus::Paused) {
                download::resume(app, t);
            }
        }
        "retry" => {
            for t in task_ids(&st, id, |s| matches!(s, TaskStatus::Failed)) {
                download::resume(app, t);
            }
            let retried = st.batches.with_rec(id, |r| {
                let failed = std::mem::take(&mut r.failed);
                for (i, f) in failed.iter().enumerate() {
                    r.pending.push(PendingLink { url: f.url.clone(), title: f.title.clone(), seq: r.total as u32 + 1 + i as u32 });
                }
                r.total += failed.len();
                r.fired = false;
                (r.clone(), !failed.is_empty())
            });
            if let Some((r, any)) = retried {
                persist(&st, &r);
                if any {
                    spawn_worker(app, id.to_string());
                }
            }
        }
        "cancel" => {
            for t in task_ids(&st, id, |s| matches!(s, TaskStatus::Queued | TaskStatus::Running | TaskStatus::Paused)) {
                download::cancel(app, t);
            }
            if let Some(r) = st.batches.with_rec(id, |r| {
                r.pending.clear();
                r.clone()
            }) {
                persist(&st, &r);
            }
        }
        "clear" => {
            for t in task_ids(&st, id, |s| matches!(s, TaskStatus::Done | TaskStatus::Canceled)) {
                download::remove(app, t);
            }
        }
        "delete" => {
            for t in task_ids(&st, id, |_| true) {
                download::remove(app, t);
            }
            lock(&st.batches.recs).retain(|r| r.id != id);
            let _ = st.db.delete_batch(id);
        }
        "top" => {
            for t in task_ids(&st, id, |s| matches!(s, TaskStatus::Queued | TaskStatus::Paused | TaskStatus::Failed)).into_iter().rev() {
                download::move_task(app, t, "top");
            }
        }
        _ => return Err(crate::error::AppError::invalid("不支持的批次操作。")),
    }
    changed(app);
    download::schedule(app);
    Ok(())
}

/// 修改批次的设置（并发、间隔、限速、完成动作、下载时段、名称）。
pub fn update(app: &AppHandle, id: &str, o: BatchOpts) -> AppResult<()> {
    let st = state(app);
    let r = st.batches.with_rec(id, |r| {
        if !o.name.trim().is_empty() {
            r.name = o.name.trim().to_string();
        }
        r.opts.name = r.name.clone();
        r.opts.concurrency = o.concurrency;
        r.opts.jitter_max = o.jitter_max.min(120);
        r.opts.jitter_min = o.jitter_min.min(r.opts.jitter_max);
        r.opts.speed_limit_kbps = o.speed_limit_kbps;
        r.opts.use_window = o.use_window;
        if o.after_done != r.opts.after_done {
            r.fired = false;
        }
        r.opts.after_done = o.after_done;
        r.clone()
    });
    let Some(r) = r else { return Err(crate::error::AppError::invalid("找不到这个批次。")) };
    persist(&st, &r);
    changed(app);
    download::schedule(app);
    Ok(())
}

/// 失败清单：每行一条原始链接（失败的任务和解析失败的链接），可直接再次粘贴到批量添加里。
pub fn export_failed(app: &AppHandle, id: &str) -> AppResult<String> {
    let st = state(app);
    let mut lines: Vec<String> = download::batch_sources(&st, id)
        .into_iter()
        .filter(|(_, url, status, _)| *status == TaskStatus::Failed && !url.is_empty())
        .map(|(title, url, _, err)| format!("# {title}{}\n{url}", err.map(|e| format!("（{e}）")).unwrap_or_default().replace('\n', " ")))
        .collect();
    if let Some(r) = st.batches.snapshot().into_iter().find(|r| r.id == id) {
        lines.extend(r.failed.iter().map(|f| format!("# {}（{}）\n{}", f.title, f.error.replace('\n', " "), f.url)));
    }
    if lines.is_empty() {
        return Err(crate::error::AppError::invalid("这个批次没有失败的条目。"));
    }
    Ok(lines.join("\n") + "\n")
}

/// 读取链接清单文件（限制大小）并取出链接。
pub fn read_links_file(path: &str) -> AppResult<(Vec<String>, bool)> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(crate::error::AppError::invalid("文件太大（超过 8 MB），不像是链接清单。"));
    }
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let mut links = extract_file_text(path, &text);
    let truncated = links.len() > MAX_LINKS;
    links.truncate(MAX_LINKS);
    Ok((links, truncated))
}

// ---------- 批次完成与下载时段 ----------

/// 批次里的任务都结束了（完成或失败）：执行批次的完成动作，只执行一次。
pub fn check_done(app: &AppHandle) {
    let st = state(app);
    let snaps = st.downloads.snapshots();
    let mut fire: Vec<(BatchRec, usize, usize, Option<String>)> = vec![];
    {
        let mut recs = lock(&st.batches.recs);
        for r in recs.iter_mut() {
            if r.fired || !r.pending.is_empty() {
                continue;
            }
            let mine: Vec<_> = snaps.iter().filter(|t| t.batch_id.as_deref() == Some(r.id.as_str())).collect();
            if mine.is_empty() || mine.iter().any(|t| matches!(t.status, TaskStatus::Queued | TaskStatus::Running | TaskStatus::Paused)) {
                continue;
            }
            r.fired = true;
            let done = mine.iter().filter(|t| t.status == TaskStatus::Done).count();
            let failed = mine.iter().filter(|t| t.status == TaskStatus::Failed).count() + r.failed.len();
            let dir = mine
                .iter()
                .find(|t| t.status == TaskStatus::Done)
                .and_then(|t| std::path::Path::new(&t.file_path).parent().map(|p| p.to_string_lossy().into_owned()));
            fire.push((r.clone(), done, failed, dir));
        }
    }
    for (rec, done, failed, dir) in fire {
        persist(&st, &rec);
        changed(app);
        let body = format!("{}：完成 {done} 条{}", rec.name, if failed > 0 { format!("，失败 {failed} 条") } else { String::new() });
        crate::notify::emit(app, crate::notify::Event::Done, "批次下载完成", &body);
        match rec.opts.after_done.as_str() {
            "none" | "" => {}
            "open" => {
                use tauri_plugin_opener::OpenerExt;
                if let Some(d) = dir {
                    let _ = app.opener().open_path(d, None::<&str>);
                }
                crate::security::system_notification(app, "批次下载完成", &body, true);
            }
            "sleep" | "shutdown" => {
                *lock(&st.after_all_done) = rec.opts.after_done.clone();
                download::start_power_countdown(app, &rec.opts.after_done);
            }
            _ => crate::security::system_notification(app, "批次下载完成", &body, true),
        }
    }
}

/// 每隔一会儿检查：下载时段结束时暂停批次里正在下载的任务（时段外不限速的话），时段开始后继续。
pub fn window_tick(app: &AppHandle) {
    let st = state(app);
    let settings = st.settings();
    let inside = in_window(&settings);
    let pause_outside = settings.run_window_on && !inside && settings.run_window.limit_kbps == 0;
    if pause_outside {
        let recs = st.batches.snapshot();
        for t in st.downloads.snapshots() {
            let uses = t.batch_id.as_deref().is_some_and(|b| recs.iter().any(|r| r.id == b && r.opts.use_window));
            if t.status == TaskStatus::Running && uses {
                lock(&st.batches.window_paused).insert(t.id);
                download::pause(app, t.id);
            }
        }
    } else {
        let ids: Vec<i64> = lock(&st.batches.window_paused).drain().collect();
        for id in ids {
            if st.downloads.snapshots().iter().any(|t| t.id == id && t.status == TaskStatus::Paused) {
                download::resume(app, id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_tracking_params_and_case() {
        assert_eq!(normalize_key("https://www.bilibili.com/video/BV1xx/?spm_id_from=333&p=2#t"), normalize_key("https://www.bilibili.com/video/BV1xx?p=2"));
        assert_ne!(normalize_key("https://a.com/v?id=1"), normalize_key("https://a.com/v?id=2"));
    }

    #[test]
    fn extracts_unique_links_from_text() {
        let t = "看这个 https://youtu.be/abc?si=1 还有\n【抖音】https://v.douyin.com/i8aKq2/ 复制\nhttps://youtu.be/abc 重复\nmagnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01";
        let l = extract(t);
        assert_eq!(l.len(), 3, "{l:?}");
        assert!(l[0].contains("youtu.be/abc"));
        assert!(l.last().unwrap().starts_with("magnet:"));
    }

    #[test]
    fn extracts_links_from_bookmark_html_and_csv() {
        let html = r#"<DL><DT><A HREF="https://example.com/a.mp4?x=1&amp;y=2" ADD_DATE="1">A</A><DT><a href='https://example.com/b'>B</a></DL>"#;
        let l = extract_file_text("bookmarks.html", html);
        assert_eq!(l, vec!["https://example.com/a.mp4?x=1&y=2", "https://example.com/b"]);
        let csv = "标题,链接,备注\n第一讲,https://example.com/1,x\n第二讲,https://example.com/2,y\n";
        assert_eq!(extract_file_text("list.csv", csv).len(), 2);
    }

    #[test]
    fn seq_labels_pad_to_total_width() {
        assert_eq!(seq_label(3, 17), "03");
        assert_eq!(seq_label(3, 120), "003");
    }

    #[test]
    fn jitter_stays_in_range_and_limits_combine() {
        for _ in 0..50 {
            let v = rand_between(2, 6);
            assert!((2..=6).contains(&v));
        }
        assert_eq!(rand_between(5, 3), 5);
        assert_eq!(min_nonzero(0, 300), 300);
        assert_eq!(min_nonzero(500, 300), 300);
        assert_eq!(min_nonzero(0, 0), 0);
    }

    #[test]
    fn gate_enforces_batch_concurrency_and_site_spacing() {
        let store = BatchStore::default();
        let rec = BatchRec { id: "b1".into(), opts: BatchOpts { concurrency: 2, jitter_min: 3, jitter_max: 3, ..Default::default() }, ..Default::default() };
        lock(&store.recs).push(rec);
        let s = Settings::default();
        assert!(matches!(store.gate("b1", "a.com", 2, &s), Gate::Block));
        assert!(matches!(store.gate("b1", "a.com", 1, &s), Gate::Go));
        store.note_start("b1", "a.com", &s);
        assert!(matches!(store.gate("b1", "a.com", 1, &s), Gate::Wait(_)));
        assert!(matches!(store.gate("b1", "b.com", 1, &s), Gate::Go));
        assert!(matches!(store.gate("unknown", "a.com", 9, &s), Gate::Go));
    }

    #[test]
    fn batch_limit_is_split_across_tasks() {
        let store = BatchStore::default();
        lock(&store.recs).push(BatchRec {
            id: "b1".into(),
            opts: BatchOpts { concurrency: 2, speed_limit_kbps: 1000, ..Default::default() },
            ..Default::default()
        });
        let s = Settings::default();
        let tag = BatchTag { id: "b1".into(), seq: 1 };
        assert_eq!(store.task_limit(Some(&tag), &s, 0, 10), 500);
        assert_eq!(store.task_limit(Some(&tag), &s, 300, 10), 300);
        assert_eq!(store.task_limit(None, &s, 700, 10), 700);
        // 收尾：只剩 1 条时独享整个批次限速，剩 2 条时各一半
        assert_eq!(store.task_limit(Some(&tag), &s, 0, 1), 1000);
        assert_eq!(store.task_limit(Some(&tag), &s, 0, 2), 500);
        assert_eq!(store.task_limit(Some(&tag), &s, 0, 0), 1000);
        assert!(store.limiter_for("b1", &s).is_some());
        assert!(store.limiter_for("nope", &s).is_none());
    }
}
