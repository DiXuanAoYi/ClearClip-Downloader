//! 下载队列：任务持久化、并发与按网站限流、暂停 / 继续、断点续传、失败重试、直链过期重新解析、
//! 多输入任务（音视频分离时先分别下载再合并）、临时目录与文件冲突策略。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::watch;

use crate::db::{self, JobRow, NewDownload};
use crate::ed2k::{self, link as ed2k_link};
use crate::engine::ftp as ftp_engine;
use crate::engine::hls::{self as hls_engine, HlsOptions};
use crate::engine::http::{self as http_engine, HttpOptions, HttpRequest};
use crate::engine::sftp as sftp_engine;
use crate::engine::stream as stream_engine;
use crate::engine::{wait_ctrl, DlError, ResumeMeta, CTRL_CANCEL, CTRL_PAUSE, CTRL_RUN};
use crate::error::ErrorKind;
use crate::model::{AppResult, Asset, AssetKind, Chapter, Clip, MediaInfo, Protocol};
use crate::settings::{ConflictPolicy, Settings};
use crate::{naming, postprocess, providers, AppState};

/// 下载中进度写入数据库的最小间隔
const PERSIST_EVERY: Duration = Duration::from_secs(2);
/// 留给 ".part" 和重名序号的路径长度余量后，路径的上限
const MAX_PATH_LEN: usize = 240;

pub const EVT_TASKS: &str = "tasks://updated";
pub const EVT_PROGRESS: &str = "tasks://progress";

pub use crate::engine::http::build_download_client;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
    Canceled,
}

impl TaskStatus {
    fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Paused => "paused",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Canceled => "canceled",
        }
    }

    fn parse(s: &str) -> TaskStatus {
        match s {
            "queued" => TaskStatus::Queued,
            "running" => TaskStatus::Running,
            "done" => TaskStatus::Done,
            "failed" => TaskStatus::Failed,
            "canceled" => TaskStatus::Canceled,
            _ => TaskStatus::Paused,
        }
    }
}

/// 后处理选项（阶段 3、4 逐步实现）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostOptions {
    /// 提取音频为 mp3 / m4a
    pub extract_audio: Option<String>,
    /// 写入封面和标题等元数据
    pub embed_metadata: bool,
    /// 字幕文件的目标格式（`srt` / `ass`），为空表示保持原样。由加入队列时根据设置决定，重启后保持一致
    pub sub_to: Option<String>,
    /// 内嵌或烧录进视频的字幕（解析结果里选中的字幕轨）
    pub embed_subs: Vec<Asset>,
    /// `soft` 内嵌为可切换的字幕轨 / `burn` 烧录进画面
    pub sub_mode: Option<String>,
    /// 只保留这个时间段
    pub clip: Option<Clip>,
    /// 按这些章节另外拆分成多个文件（保留完整文件）
    pub split_chapters: Vec<Chapter>,
}

/// 一个下载任务的内容：一个或多个输入（音视频分离时为视频轨 + 音频轨），完成后合并为一个文件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSpec {
    pub inputs: Vec<Asset>,
    #[serde(default)]
    pub post: PostOptions,
    /// 排序优先级：越大越先开始
    #[serde(default)]
    pub priority: i64,
    /// 定时开始（Unix 秒）；未到时间时保持等待
    #[serde(default)]
    pub start_at: Option<i64>,
    /// 来源：manual（默认）/ subscription / live
    #[serde(default)]
    pub origin: Option<String>,
    /// 来自订阅时对应的条目（订阅 ID，条目 ID），完成后更新条目状态
    #[serde(default)]
    pub sub_item: Option<(i64, String)>,
    /// 已经试过的视频格式（自动降级时避免重复）
    #[serde(default)]
    pub tried: Vec<String>,
}

impl JobSpec {
    fn primary(&self) -> &Asset {
        &self.inputs[0]
    }
}

/// 每个输入各自的续传信息。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct JobMeta {
    pub inputs: Vec<ResumeMeta>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: i64,
    pub platform: String,
    pub platform_name: String,
    /// 登录用的网站（内置平台 ID 或域名），出现“需要登录”时用来打开对应的登录窗口
    pub site: String,
    pub media_id: String,
    pub title: String,
    pub author: String,
    pub cover: Option<String>,
    pub asset_id: String,
    pub asset_label: String,
    pub asset_kind: AssetKind,
    pub file_path: String,
    pub status: TaskStatus,
    pub received: u64,
    pub total: Option<u64>,
    pub speed: u64,
    pub error: Option<String>,
    pub error_kind: Option<ErrorKind>,
    pub note: Option<String>,
    /// 下载成功但有附带问题（如字幕没有处理成功），完成后仍显示
    pub warning: Option<String>,
    pub resumable: Option<bool>,
    /// 当前步骤：download / merge / post
    pub step: Option<String>,
    /// 输入数量（大于 1 时需要合并）
    pub inputs: usize,
    pub priority: i64,
    pub start_at: Option<i64>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

struct Entry {
    snap: TaskSnapshot,
    spec: JobSpec,
    media: Arc<MediaInfo>,
    ctrl: watch::Sender<u8>,
    meta: JobMeta,
    /// 任务详情里显示的过程记录（只在内存里，最多保留最近 300 条）
    log: Vec<LogLine>,
}

/// 任务过程记录里的一行。`at` 是 Unix 毫秒。
#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub at: i64,
    pub text: String,
}

const LOG_CAP: usize = 300;

/// 给任务追加一条过程记录。
pub(crate) fn task_log(st: &AppState, id: i64, text: impl Into<String>) {
    let line = LogLine { at: chrono::Utc::now().timestamp_millis(), text: text.into() };
    with_entry(st, id, |e| {
        if e.log.len() >= LOG_CAP {
            e.log.remove(0);
        }
        e.log.push(line);
    });
}

#[derive(Default)]
pub struct DownloadManager {
    inner: Mutex<Vec<Entry>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueResult {
    pub tasks: Vec<TaskSnapshot>,
    /// 之前已下载过（或目标文件已存在且策略为跳过）而跳过的数量
    pub already_downloaded: usize,
    /// 已经在队列里（等待、下载中、暂停、失败）而跳过的数量
    pub already_queued: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanPart {
    pub path: String,
    pub size: u64,
    pub modified: i64,
}

impl DownloadManager {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshots(&self) -> Vec<TaskSnapshot> {
        self.lock().iter().map(|e| e.snap.clone()).collect()
    }

    /// 是否有正在下载的任务（定时未到的不算）。
    pub fn has_active_running(&self) -> bool {
        self.lock().iter().any(|e| e.snap.status == TaskStatus::Running)
    }

    pub fn has_active(&self) -> bool {
        self.lock().iter().any(|e| matches!(e.snap.status, TaskStatus::Running | TaskStatus::Queued))
    }
}

fn state(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

fn emit_all(app: &AppHandle) {
    let _ = app.emit(EVT_TASKS, state(app).downloads.snapshots());
}

fn persist(st: &AppState, e: &Entry) {
    let meta = serde_json::to_string(&e.meta).unwrap_or_else(|_| "{}".into());
    if let Err(err) = st.db.update_job(
        e.snap.id,
        e.snap.status.as_str(),
        e.snap.received as i64,
        e.snap.total.map(|t| t as i64),
        e.snap.error.as_deref(),
        &e.snap.file_path,
        &meta,
        e.snap.finished_at,
    ) {
        log::warn!("persist job {} failed: {err}", e.snap.id);
    }
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// 第 `i` 个输入的临时文件路径：设置了临时目录时放在临时目录，否则与目标文件同目录。
pub fn input_part_path(settings: &Settings, job_id: i64, final_path: &Path, i: usize, count: usize) -> PathBuf {
    if !settings.temp_dir.is_empty() {
        return PathBuf::from(&settings.temp_dir).join(format!("clearclip-{job_id}-{i}.part"));
    }
    if count == 1 {
        part_path(final_path)
    } else {
        let mut s = final_path.as_os_str().to_owned();
        s.push(format!(".f{i}.part"));
        PathBuf::from(s)
    }
}

fn parse_spec(json: &str) -> Option<JobSpec> {
    serde_json::from_str::<JobSpec>(json).ok().filter(|s| !s.inputs.is_empty()).or_else(|| {
        serde_json::from_str::<Asset>(json).ok().map(|a| JobSpec {
            inputs: vec![a],
            post: PostOptions::default(),
            priority: 0,
            start_at: None,
            origin: None,
            sub_item: None,
            tried: vec![],
        })
    })
}

fn parse_meta(json: &str) -> JobMeta {
    serde_json::from_str::<JobMeta>(json)
        .ok()
        .filter(|m| !m.inputs.is_empty())
        .or_else(|| serde_json::from_str::<ResumeMeta>(json).ok().map(|m| JobMeta { inputs: vec![m] }))
        .unwrap_or_default()
}

/// 已下载的字节数：分段下载的文件是预分配的，按各段记录的进度计算；单线程下载按临时文件大小计算。
fn received_on_disk(settings: &Settings, id: i64, path: &str, inputs: usize, meta: &JobMeta) -> u64 {
    let empty = ResumeMeta::default();
    (0..inputs).map(|i| input_received(&input_part_path(settings, id, Path::new(path), i, inputs), meta.inputs.get(i).unwrap_or(&empty))).sum()
}

/// 启动时从数据库恢复任务：未完成的任务恢复为“已暂停”（或按设置自动继续），进度以临时文件实际大小为准。
pub fn restore(app: &AppHandle) {
    let st = state(app);
    let settings = st.settings();
    let rows = match st.db.load_jobs() {
        Ok(r) => r,
        Err(e) => {
            log::error!("load jobs failed: {e}");
            return;
        }
    };
    let mut restored = 0;
    {
        let mut list = st.downloads.lock();
        for row in rows {
            let (Ok(media), Some(spec)) = (serde_json::from_str::<MediaInfo>(&row.media_json), parse_spec(&row.asset_json)) else {
                log::warn!("skip unreadable job {}", row.id);
                continue;
            };
            let meta = parse_meta(&row.meta_json);
            let mut status = TaskStatus::parse(&row.status);
            if matches!(status, TaskStatus::Running | TaskStatus::Queued) {
                status = if settings.auto_resume { TaskStatus::Queued } else { TaskStatus::Paused };
            }
            let received = if status == TaskStatus::Done {
                row.received.max(0) as u64
            } else {
                received_on_disk(&settings, row.id, &row.file_path, spec.inputs.len(), &meta)
            };
            let (ctrl, _) = watch::channel(CTRL_RUN);
            let mut snap = snapshot_for(row.id, &media, &spec, &row.file_path, status, row.created_at);
            snap.received = received;
            snap.total = row.total.map(|t| t as u64);
            snap.resumable = meta.inputs.first().and_then(|m| m.resumable);
            snap.error = row.error;
            snap.finished_at = row.finished_at;
            if status == TaskStatus::Paused && received > 0 {
                snap.note = Some(format!("上次下载到 {}，继续时从这里开始", human_bytes(received)));
            }
            let e = Entry { snap, spec, media: Arc::new(media), ctrl, meta, log: Vec::new() };
            persist(&st, &e);
            list.push(e);
            restored += 1;
        }
    }
    log::info!("restored {restored} download tasks");
    schedule(app);
}

fn snapshot_for(id: i64, media: &MediaInfo, spec: &JobSpec, path: &str, status: TaskStatus, created_at: i64) -> TaskSnapshot {
    let a = spec.primary();
    let label = if spec.inputs.len() > 1 { format!("{}（音视频合并）", a.label) } else { a.label.clone() };
    TaskSnapshot {
        id,
        platform: media.platform.clone(),
        platform_name: media.platform_name.clone(),
        site: crate::inbox::site_for(&media.source_url),
        media_id: media.id.clone(),
        title: media.title.clone(),
        author: media.author.clone(),
        cover: media.cover.clone(),
        asset_id: a.id.clone(),
        asset_label: label,
        asset_kind: a.kind,
        file_path: path.to_string(),
        status,
        received: 0,
        total: None,
        speed: 0,
        error: None,
        error_kind: None,
        note: None,
        warning: None,
        resumable: None,
        step: None,
        inputs: spec.inputs.len(),
        priority: spec.priority,
        start_at: spec.start_at,
        created_at,
        finished_at: None,
    }
}

/// 根据选中的资源 ID 组成任务：无声视频轨自动带上它的音频轨，合并成一个任务。
pub fn build_specs(media: &MediaInfo, asset_ids: &[String]) -> Vec<JobSpec> {
    media
        .assets
        .iter()
        .filter(|a| asset_ids.contains(&a.id))
        .map(|a| {
            let mut inputs = vec![a.clone()];
            if a.kind == AssetKind::Video && a.has_audio == Some(false) {
                if let Some(audio) = a.pair_audio.as_deref().and_then(|id| media.asset(id)) {
                    inputs.push(audio.clone());
                }
            }
            JobSpec { inputs, post: PostOptions::default(), priority: 0, start_at: None, origin: None, sub_item: None, tried: vec![] }
        })
        .collect()
}

/// 把作品中选中的资源加入队列。已在队列中、之前已下载过的资源会跳过。
pub fn enqueue(app: &AppHandle, media: MediaInfo, asset_ids: &[String]) -> AppResult<EnqueueResult> {
    enqueue_with(app, media, asset_ids, PostOptions::default())
}

pub fn enqueue_with(app: &AppHandle, media: MediaInfo, asset_ids: &[String], post: PostOptions) -> AppResult<EnqueueResult> {
    enqueue_ext(app, media, asset_ids, post, EnqueueExtra::default())
}

/// 订阅等来源的额外选项。
#[derive(Debug, Clone, Default)]
pub struct EnqueueExtra {
    /// 保存目录（替代下载目录）
    pub dir: Option<PathBuf>,
    /// 文件命名模板（替代设置里的模板）
    pub template: Option<String>,
    pub origin: Option<String>,
    /// 对应的订阅条目（订阅 ID，条目 ID）
    pub sub_item: Option<(i64, String)>,
    /// 已经试过的视频格式（自动降级时使用）
    pub tried: Vec<String>,
}

/// 这个任务适用的后处理选项：字幕转换、内嵌、裁剪、章节只对相应类型的资源有意义。
fn post_for(post: &PostOptions, asset: &Asset, settings: &Settings) -> PostOptions {
    let mut p = post.clone();
    match asset.kind {
        AssetKind::Video => {
            p.sub_to = None;
        }
        AssetKind::Audio => {
            p.sub_to = None;
            p.embed_subs.clear();
            p.sub_mode = None;
            p.split_chapters.clear();
        }
        AssetKind::Subtitle => {
            // 有裁剪区间时 VTT 一律转 SRT，才能同步裁剪
            let convert = settings.subtitle_convert || post.clip.is_some();
            let to = crate::subtitle::target_ext(&asset.ext, convert, settings.danmaku_ass);
            p.sub_to = (to != asset.ext).then_some(to);
            p = PostOptions { sub_to: p.sub_to, clip: post.clip.clone(), ..Default::default() };
        }
        AssetKind::Image | AssetKind::Cover | AssetKind::File => {
            p = PostOptions::default();
        }
    }
    p
}

/// 这些任务在队列里显示的文件名（按“文件命名”模板生成，已处理重名）。找不到的任务跳过。
pub fn file_names(app: &AppHandle, task_ids: &[i64]) -> Vec<String> {
    let snaps = state(app).downloads.snapshots();
    task_ids.iter().filter_map(|id| snaps.iter().find(|t| t.id == *id)).map(|t| naming::display_name(&t.file_path)).collect()
}

pub fn enqueue_ext(app: &AppHandle, media: MediaInfo, asset_ids: &[String], post: PostOptions, extra: EnqueueExtra) -> AppResult<EnqueueResult> {
    let st = state(app);
    let settings = st.settings();
    let media = Arc::new(media);
    // 内嵌 / 烧录：选中的字幕不单独下载，交给视频任务处理（没有同时选视频时仍作为文件保存）
    let mut post = post;
    let mut asset_ids: Vec<String> = asset_ids.to_vec();
    let is_sub = |id: &String| media.asset(id).is_some_and(|a| a.kind == AssetKind::Subtitle);
    let has_video = asset_ids.iter().any(|id| media.asset(id).is_some_and(|a| a.kind == AssetKind::Video));
    match (post.sub_mode.as_deref(), has_video) {
        (Some("soft" | "burn"), true) if asset_ids.iter().any(is_sub) || !post.embed_subs.is_empty() => {
            let picked: Vec<Asset> = asset_ids.iter().filter(|id| is_sub(id)).filter_map(|id| media.asset(id).cloned()).collect();
            if !picked.is_empty() {
                post.embed_subs = picked;
            }
            asset_ids.retain(|id| !is_sub(id));
        }
        _ => {
            post.sub_mode = None;
            post.embed_subs.clear();
        }
    }
    let asset_ids = &asset_ids[..];
    let root = extra.dir.clone().unwrap_or_else(|| settings.download_root());
    let mut dir = if extra.dir.is_some() { root } else { naming::target_dir(&root, &media, settings.subfolder_by_platform) };
    // 文件名主体按任务单独生成（模板里的 {quality} 随各个资源的清晰度变化）；剧集命名可能带子文件夹
    let (series_dirs, _) = naming::plan_base(extra.template.as_deref(), &settings.series_template, &settings.filename_template, &media, "");
    dir.extend(series_dirs);
    let media_json = serde_json::to_string(&*media)?;

    let mut result = EnqueueResult { tasks: vec![], already_downloaded: 0, already_queued: 0 };
    {
        let mut list = st.downloads.lock();
        for mut spec in build_specs(&media, asset_ids) {
            spec.post = post_for(&post, spec.primary(), &settings);
            spec.origin = extra.origin.clone();
            spec.sub_item = extra.sub_item.clone();
            spec.tried = extra.tried.clone();
            let primary = spec.primary().clone();
            let in_queue = list.iter().any(|e| {
                e.snap.platform == media.platform
                    && e.snap.media_id == media.id
                    && e.snap.asset_id == primary.id
                    && matches!(e.snap.status, TaskStatus::Queued | TaskStatus::Running | TaskStatus::Paused | TaskStatus::Failed)
            });
            if in_queue {
                result.already_queued += 1;
                continue;
            }
            if settings.skip_existing && st.db.existing_download(&media.platform, &media.id, &primary.id)?.is_some() {
                result.already_downloaded += 1;
                continue;
            }
            let mut out_asset = primary.clone();
            if spec.inputs.len() > 1 && settings.merge_container == "mkv" {
                out_asset.ext = "mkv".into();
            }
            if let Some(to) = &spec.post.sub_to {
                out_asset.ext = to.clone();
            }
            if let Some(fmt) = &spec.post.extract_audio {
                out_asset.ext = fmt.clone();
            }
            let (_, base) = naming::plan_base(
                extra.template.as_deref(),
                &settings.series_template,
                &settings.filename_template,
                &media,
                primary.quality.as_deref().unwrap_or(""),
            );
            let wanted = naming::fit_path(&dir, &base, &out_asset, MAX_PATH_LEN);
            let path = match settings.conflict_policy {
                ConflictPolicy::Skip if wanted.exists() => {
                    result.already_downloaded += 1;
                    continue;
                }
                ConflictPolicy::Overwrite => wanted,
                _ => {
                    // 已完成但文件已不在的任务不再占用文件名（重新下载时沿用原名）
                    let taken: Vec<String> = list
                        .iter()
                        .filter(|e| e.snap.status != TaskStatus::Canceled && !(e.snap.status == TaskStatus::Done && !Path::new(&e.snap.file_path).exists()))
                        .map(|e| e.snap.file_path.clone())
                        .collect();
                    naming::unique_path(wanted, &|p: &Path| p.exists() || part_path(p).exists() || taken.iter().any(|t| Path::new(t) == p))
                }
            };
            let path_str = path.to_string_lossy().into_owned();
            let now = db::now();
            let id = st.db.insert_job(&JobRow {
                id: 0,
                media_json: media_json.clone(),
                asset_json: serde_json::to_string(&spec)?,
                file_path: path_str.clone(),
                status: "queued".into(),
                received: 0,
                total: None,
                error: None,
                meta_json: "{}".into(),
                created_at: now,
                finished_at: None,
            })?;
            let (ctrl, _) = watch::channel(CTRL_RUN);
            let snap = snapshot_for(id, &media, &spec, &path_str, TaskStatus::Queued, now);
            result.tasks.push(snap.clone());
            let meta = JobMeta { inputs: vec![ResumeMeta::default(); spec.inputs.len()] };
            list.push(Entry {
                snap,
                spec,
                media: media.clone(),
                ctrl,
                meta,
                log: vec![LogLine { at: chrono::Utc::now().timestamp_millis(), text: "已加入队列".into() }],
            });
        }
    }
    // 收到的链接里等待处理的同一作品，关联到这些任务
    let ids: Vec<i64> = result.tasks.iter().map(|t| t.id).collect();
    if st.db.inbox_link_tasks(&media.platform, &media.id, &ids).unwrap_or(false) {
        crate::inbox::emit_changed(app);
    }
    schedule(app);
    Ok(result)
}

/// 每个网站同时下载的上限；带登录账号时更保守（减半，至少 1）。
fn site_limit(st: &AppState, settings: &Settings, platform: &str) -> usize {
    let base = settings.per_site_concurrency.max(1);
    if st.cookies.has_account(platform) {
        (base / 2).max(1)
    } else {
        base
    }
}

/// 按全局并发数和按网站限流启动等待中的任务。
pub fn schedule(app: &AppHandle) {
    let st = state(app);
    let settings = st.settings();
    let mut to_start = Vec::new();
    {
        let mut list = st.downloads.lock();
        let mut running: Vec<String> = list.iter().filter(|e| e.snap.status == TaskStatus::Running).map(|e| e.snap.platform.clone()).collect();
        let now = db::now();
        // 按优先级（大的先）和加入顺序
        let mut order: Vec<usize> = (0..list.len()).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(list[i].spec.priority), list[i].snap.id));
        for i in order {
            if running.len() >= settings.concurrency {
                break;
            }
            if list[i].snap.status != TaskStatus::Queued || list[i].spec.start_at.is_some_and(|t| t > now) {
                continue;
            }
            let platform = list[i].snap.platform.clone();
            if running.iter().filter(|p| **p == platform).count() >= site_limit(&st, &settings, &platform) {
                continue;
            }
            let e = &mut list[i];
            e.snap.status = TaskStatus::Running;
            e.snap.error = None;
            e.snap.error_kind = None;
            let _ = e.ctrl.send(CTRL_RUN);
            running.push(platform);
            to_start.push(e.snap.id);
            persist(&st, e);
        }
    }
    for id in to_start {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { run_task(app, id).await });
    }
    crate::power::keep_awake(settings.prevent_sleep && st.downloads.has_active_running());
    emit_all(app);
}

/// 定时任务：每 20 秒检查一次是否有到点的任务。
pub fn spawn_timer(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(20)).await;
            let st = state(&app);
            let now = db::now();
            let due = st.downloads.lock().iter().any(|e| e.snap.status == TaskStatus::Queued && e.spec.start_at.is_some_and(|t| t <= now));
            if due {
                schedule(&app);
            }
        }
    });
}

/// 调整等待中任务的顺序：top / up / down / bottom。
pub fn move_task(app: &AppHandle, id: i64, to: &str) {
    let st = state(app);
    {
        let mut list = st.downloads.lock();
        let waiting = |e: &Entry| matches!(e.snap.status, TaskStatus::Queued | TaskStatus::Paused | TaskStatus::Failed);
        let mut order: Vec<usize> = (0..list.len()).filter(|&i| waiting(&list[i])).collect();
        order.sort_by_key(|&i| (std::cmp::Reverse(list[i].spec.priority), list[i].snap.id));
        let Some(pos) = order.iter().position(|&i| list[i].snap.id == id) else { return };
        reorder(&mut order, pos, to);
        let n = order.len() as i64;
        for (k, &i) in order.iter().enumerate() {
            let p = n - k as i64;
            if list[i].spec.priority != p {
                list[i].spec.priority = p;
                list[i].snap.priority = p;
                let _ = st.db.update_job_asset(list[i].snap.id, &serde_json::to_string(&list[i].spec).unwrap_or_default());
            }
        }
    }
    emit_all(app);
}

/// 把 `pos` 处的元素移到 top / up / down / bottom。
fn reorder<T>(v: &mut Vec<T>, pos: usize, to: &str) {
    let item = v.remove(pos);
    let new_pos = match to {
        "top" => 0,
        "up" => pos.saturating_sub(1),
        "down" => (pos + 1).min(v.len()),
        _ => v.len(),
    };
    v.insert(new_pos, item);
}

/// 设置定时开始时间（None 表示立即）；暂停或失败的任务会改为等待。
pub fn schedule_task(app: &AppHandle, id: i64, start_at: Option<i64>) {
    let st = state(app);
    with_entry(&st, id, |e| {
        e.spec.start_at = start_at;
        e.snap.start_at = start_at;
        if matches!(e.snap.status, TaskStatus::Paused | TaskStatus::Failed | TaskStatus::Canceled) {
            e.snap.status = TaskStatus::Queued;
            e.snap.error = None;
            e.snap.error_kind = None;
        }
        let _ = st.db.update_job_asset(e.snap.id, &serde_json::to_string(&e.spec).unwrap_or_default());
        persist(&st, e);
    });
    schedule(app);
}

fn with_entry<R>(st: &AppState, id: i64, f: impl FnOnce(&mut Entry) -> R) -> Option<R> {
    let mut list = st.downloads.lock();
    list.iter_mut().find(|e| e.snap.id == id).map(f)
}

pub(crate) fn build_request(st: &AppState, media: &MediaInfo, asset: &Asset) -> HttpRequest {
    let mut req = HttpRequest::new(&asset.url);
    let has = |k: &str| asset.headers.iter().any(|(h, _)| h.eq_ignore_ascii_case(k));
    if !has("referer") {
        req = req.header("Referer", providers::referer_for(&media.platform));
    }
    // Cookie 按域名匹配：只发给它所属的网站，不会发给 CDN
    // 资源自带 Cookie（浏览器扩展接管的下载）时不再附加账号库里的
    if !has("cookie") {
        if let Some(c) = st.cookies.header_for(&asset.url) {
            req = req.header("Cookie", c);
        }
    }
    req.headers.extend(asset.headers.iter().cloned());
    req
}

async fn run_task(app: AppHandle, id: i64) {
    run_task_inner(app, id, true).await
}

/// `retry_ok`：下载后的文件检查没通过时，允许清理后从头重新下载一次。
fn run_task_inner(app: AppHandle, id: i64, retry_ok: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(async move { run_task_body(app, id, retry_ok).await })
}

async fn run_task_body(app: AppHandle, id: i64, retry_ok: bool) {
    let st = state(&app);
    with_entry(&st, id, |e| e.snap.warning = None);
    let Some((mut spec, media, mut rx, mut final_path, mut meta)) =
        with_entry(&st, id, |e| (e.spec.clone(), e.media.clone(), e.ctrl.subscribe(), PathBuf::from(&e.snap.file_path), e.meta.clone()))
    else {
        return;
    };
    task_log(
        &st,
        id,
        format!(
            "开始下载：{} 个输入（{}）",
            spec.inputs.len(),
            spec.inputs.iter().map(|a| format!("{} · {}", a.label, protocol_name(a.protocol))).collect::<Vec<_>>().join("；")
        ),
    );
    let settings = st.settings();
    let count = spec.inputs.len();
    meta.inputs.resize(count, ResumeMeta::default());
    let parts: Vec<PathBuf> = (0..count).map(|i| input_part_path(&settings, id, &final_path, i, count)).collect();
    let opts = HttpOptions {
        segments: settings.segments,
        min_segment_size: settings.segment_min_mb * 1024 * 1024,
        speed_limit_kbps: settings.speed_limit_kbps,
        disk_reserve: settings.disk_reserve_mb * 1024 * 1024,
        net: Some(st.net.clone()),
    };
    let start_len: u64 = (0..count).map(|i| input_received(&parts[i], &meta.inputs[i])).sum();
    if start_len > 0 {
        set_note(&app, id, Some(&format!("从 {} 处继续下载", human_bytes(start_len))));
    }

    let mut outcome: Result<u64, DlError> = Ok(0);
    let mut fmp4 = false;
    for i in 0..count {
        // 已完成的输入（合并前中断）直接跳过
        if let Some(t) = meta.inputs[i].total {
            if meta.inputs[i].segments.is_empty() && file_len(&parts[i]) == t {
                continue;
            }
        }
        set_step(&st, id, "download");
        let done_before: u64 = (0..i).map(|k| meta.inputs[k].total.unwrap_or_else(|| file_len(&parts[k]))).sum();
        outcome = download_input(&app, &st, &settings, id, &media, &mut spec, i, &parts[i], &mut meta, &mut rx, &opts, done_before, &mut fmp4).await;
        if outcome.is_err() {
            break;
        }
    }

    let final_part = if count > 1 { part_path(&final_path) } else { parts[0].clone() };
    // 合并 / 转封装使用源格式的容器；需要提取音频时最终文件才是音频格式
    let container = match &spec.post.extract_audio {
        Some(_) => final_path.with_extension(if spec.primary().ext.is_empty() { "mp4" } else { spec.primary().ext.as_str() }),
        None => final_path.clone(),
    };
    let mut outcome = match outcome {
        Ok(_) if count > 1 => {
            set_step(&st, id, "merge");
            set_note(&app, id, Some("正在合并音视频…"));
            let aac_fix = spec.inputs.iter().any(|a| a.protocol == Protocol::Hls) && !fmp4;
            match postprocess::merge(&st, &parts, &final_part, &container, aac_fix).await {
                Ok(()) => {
                    for p in &parts {
                        remove_part(p);
                    }
                    Ok(file_len(&final_part))
                }
                Err(e) => Err(DlError::Other(e.message)),
            }
        }
        // Pixiv 动图：帧压缩包合成为视频
        Ok(_) if providers::pixiv::ugoira_frames(spec.primary()).is_some() => {
            set_step(&st, id, "post");
            set_note(&app, id, Some("正在把动图合成为视频…"));
            let frames = providers::pixiv::ugoira_frames(spec.primary()).unwrap_or_default();
            let out = suffixed(&final_part, ".remux");
            match postprocess::ugoira(&st, &final_part, &frames, &out).await {
                Ok(ext) => {
                    let _ = std::fs::remove_file(&final_part);
                    let _ = std::fs::rename(&out, &final_part);
                    final_path = final_path.with_extension(ext);
                    Ok(file_len(&final_part))
                }
                Err(e) => {
                    remove_part(&out);
                    Err(DlError::Other(e.message))
                }
            }
        }
        // m3u8 拼接出的是 TS 流：有 ffmpeg 时无损转封装为 MP4，否则保存为 .ts
        Ok(size) if spec.primary().protocol == Protocol::Hls => {
            if postprocess::find_ffmpeg(&st).is_some() {
                set_step(&st, id, "post");
                set_note(&app, id, Some("正在转换为 MP4…"));
                let out = suffixed(&final_part, ".remux");
                match postprocess::remux(&st, &final_part, &out, &container, !fmp4).await {
                    Ok(()) => {
                        let _ = std::fs::rename(&out, &final_part);
                        Ok(file_len(&final_part))
                    }
                    Err(e) => {
                        remove_part(&out);
                        Err(DlError::Other(e.message))
                    }
                }
            } else {
                if !fmp4 && spec.post.extract_audio.is_none() {
                    final_path = final_path.with_extension("ts");
                }
                Ok(size)
            }
        }
        other => other,
    };

    // 提取音频
    if let (Ok(_), Some(fmt)) = (&outcome, spec.post.extract_audio.clone()) {
        if spec.primary().ext != fmt {
            set_step(&st, id, "post");
            set_note(&app, id, Some(&format!("正在提取音频（{}）…", fmt.to_uppercase())));
            let out = suffixed(&final_part, ".audio");
            outcome = match postprocess::extract_audio(&st, &final_part, &out, &fmt).await {
                Ok(()) => {
                    let _ = std::fs::remove_file(&final_part);
                    let _ = std::fs::rename(&out, &final_part);
                    final_path = final_path.with_extension(&fmt);
                    Ok(file_len(&final_part))
                }
                Err(e) => {
                    remove_part(&out);
                    Err(DlError::Other(e.message))
                }
            };
        }
    }
    // 裁剪所选时间段
    if outcome.is_ok() && matches!(spec.primary().kind, AssetKind::Video | AssetKind::Audio) {
        if let Some(c) = spec.post.clip.clone() {
            match postprocess::find_ffmpeg(&st) {
                None => outcome = Err(DlError::Other(postprocess::ffmpeg_missing().message)),
                Some(ff) => {
                    set_step(&st, id, "post");
                    set_note(&app, id, Some(if c.precise { "正在精确裁剪（重新编码）…" } else { "正在裁剪所选时间段…" }));
                    let out = suffixed(&final_part, ".clip");
                    outcome = match postprocess::clip(&ff, &final_part, &out, &final_path, c.start_ms, c.end_ms, c.precise).await {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&final_part);
                            let _ = std::fs::rename(&out, &final_part);
                            Ok(file_len(&final_part))
                        }
                        Err(e) => {
                            remove_part(&out);
                            Err(DlError::Other(format!("裁剪失败：{}", e.message)))
                        }
                    };
                }
            }
        }
    }
    // 内嵌 / 烧录字幕（失败不影响视频本身，原因显示在任务的提示里）
    if outcome.is_ok() && spec.primary().kind == AssetKind::Video && !spec.post.embed_subs.is_empty() && spec.post.extract_audio.is_none() {
        let mode = spec.post.sub_mode.clone().unwrap_or_else(|| "soft".into());
        let warning = match postprocess::find_ffmpeg(&st) {
            None => Some(format!("字幕未处理：{}", postprocess::ffmpeg_missing().message)),
            Some(ff) => {
                set_step(&st, id, "post");
                set_note(&app, id, Some(if mode == "burn" { "正在把字幕烧录进画面（需要重新编码，较慢）…" } else { "正在内嵌字幕…" }));
                let work = suffixed(&final_part, ".subs");
                crate::subtitle_io::VideoSubs {
                    st: &st,
                    settings: &settings,
                    media: &media,
                    embed: &spec.post.embed_subs,
                    mode: &mode,
                    clip: spec.post.clip.as_ref(),
                    ffmpeg: &ff,
                    input: &final_part,
                    final_path: &final_path,
                    work: &work,
                }
                .apply()
                .await
            }
        };
        if let Some(w) = warning {
            log::warn!("job {id}: {w}");
            with_entry(&st, id, |e| e.snap.warning = Some(w));
        }
        outcome = Ok(file_len(&final_part));
    }
    // 写入标题、作者、封面
    let primary = spec.primary();
    let out_ext = final_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if outcome.is_ok()
        && settings.embed_metadata
        && matches!(primary.kind, AssetKind::Video | AssetKind::Audio)
        && crate::organize::supports_metadata(&out_ext)
        && postprocess::find_ffmpeg(&st).is_some()
    {
        set_step(&st, id, "post");
        set_note(&app, id, Some("正在写入标题和封面…"));
        let cover = crate::organize::fetch_cover(&st, &settings, &media, &final_part).await;
        let out = suffixed(&final_part, ".meta");
        match crate::organize::embed(&st, &media, &final_part, &out, &final_path, cover.as_deref()).await {
            Ok(()) => {
                let _ = std::fs::remove_file(&final_part);
                let _ = std::fs::rename(&out, &final_part);
                outcome = Ok(file_len(&final_part));
            }
            // 写入元数据失败不影响下载结果
            Err(e) => {
                log::warn!("job {id}: embedding metadata failed: {e}");
                let _ = std::fs::remove_file(&out);
            }
        }
        if let Some(c) = cover {
            let _ = std::fs::remove_file(c);
        }
    }
    // 下载后检查文件能否正常读取；损坏时清理后从头重新下载一次
    if outcome.is_ok() && settings.verify_downloads && matches!(spec.primary().kind, AssetKind::Video | AssetKind::Audio) {
        if let Some(ff) = postprocess::find_ffmpeg(&st) {
            set_step(&st, id, "post");
            set_note(&app, id, Some("正在检查文件…"));
            let p = postprocess::probe(&ff, &final_part).await;
            if !p.readable() {
                let why = p.error.clone().unwrap_or_else(|| "没有检测到音频或视频".into());
                if retry_ok {
                    log::warn!("job {id}: verification failed ({why}), downloading again");
                    for part in parts.iter().chain(std::iter::once(&final_part)) {
                        remove_part(part);
                    }
                    let n = spec.inputs.len();
                    let st2 = st.clone();
                    with_entry(&st, id, |e| {
                        e.meta = JobMeta { inputs: vec![ResumeMeta::default(); n] };
                        e.snap.received = 0;
                        e.snap.total = None;
                        persist(&st2, e);
                    });
                    set_note(&app, id, Some("下载的文件没通过检查，正在重新下载…"));
                    return run_task_inner(app, id, false).await;
                }
                outcome = Err(DlError::Other(format!("下载的文件无法播放（{why}）。请重试；如果反复出现，可能是网站限制了这个资源。")));
            } else if let (Some(actual), Some(expect), None) = (p.duration_ms, media.duration_ms, spec.post.clip.as_ref()) {
                // 只提示，不判失败：跳过广告的 m3u8、网站标注的时长不准都会造成差异
                let ads_skipped = settings.hls_skip_ads && spec.inputs.iter().any(|a| a.protocol == Protocol::Hls);
                if expect >= 10_000 && actual + 3_000 < expect * 9 / 10 && !ads_skipped {
                    let w = format!("文件时长 {} 秒，网站标注为 {} 秒，可能不完整", actual / 1000, expect / 1000);
                    with_entry(&st, id, |e| e.snap.warning = Some(w));
                }
            }
        }
    }
    // 期望的文件哈希：不一致说明下载内容被改动或损坏，清理后从头重新下载一次
    if outcome.is_ok() && spec.inputs.len() == 1 {
        if let Some(expect) = spec.inputs[0].checksum.as_deref().and_then(crate::checksum::parse) {
            set_step(&st, id, "post");
            set_note(&app, id, Some("正在校验文件…"));
            match crate::checksum::verify(&final_part, &expect).await {
                Ok(Ok(())) => {}
                Ok(Err(actual)) if retry_ok => {
                    log::warn!("job {id}: {} mismatch (got {actual}), downloading again", expect.algo.name());
                    for part in parts.iter().chain(std::iter::once(&final_part)) {
                        remove_part(part);
                    }
                    let n = spec.inputs.len();
                    let st2 = st.clone();
                    with_entry(&st, id, |e| {
                        e.meta = JobMeta { inputs: vec![ResumeMeta::default(); n] };
                        e.snap.received = 0;
                        e.snap.total = None;
                        persist(&st2, e);
                    });
                    set_note(&app, id, Some("文件校验没通过，正在重新下载…"));
                    return run_task_inner(app, id, false).await;
                }
                Ok(Err(actual)) => {
                    outcome = Err(DlError::Other(format!(
                        "文件校验失败：{} 应为 {}，实际是 {actual}。文件可能被改动或下载不完整，请重试或换一个下载源。",
                        expect.algo.name().to_uppercase(),
                        expect.hex
                    )));
                }
                Err(e) => outcome = Err(DlError::Other(format!("无法校验文件：{e}"))),
            }
            set_note(&app, id, None);
        }
    }
    // 按章节另外拆分（保留完整文件）
    if outcome.is_ok() && !spec.post.split_chapters.is_empty() && matches!(spec.primary().kind, AssetKind::Video | AssetKind::Audio) {
        if let Some(ff) = postprocess::find_ffmpeg(&st) {
            set_step(&st, id, "post");
            set_note(&app, id, Some("正在按章节拆分…"));
            let chapters = clip_chapters(&spec.post.split_chapters, spec.post.clip.as_ref());
            let stem = final_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let dir = final_path.parent().map(|p| p.join(format!("{stem} 章节"))).unwrap_or_else(|| PathBuf::from(format!("{stem} 章节")));
            let ext = final_path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
            let total = chapters.len();
            let w = match postprocess::split_chapters(&ff, &final_part, &dir, &ext, &chapters).await {
                Ok(n) if n == total => None,
                Ok(n) => Some(format!("章节只拆分出 {n}/{total} 个")),
                Err(e) => Some(format!("章节拆分失败：{}", e.message)),
            };
            if let Some(w) = w {
                with_entry(&st, id, |e| e.snap.warning = Some(w));
            }
        } else {
            with_entry(&st, id, |e| e.snap.warning = Some("章节没有拆分：需要安装 ffmpeg".into()));
        }
    }
    finish(&app, &st, id, outcome, &final_part, &final_path, &parts, &settings);
}

/// 裁剪后章节的时间需要平移：只保留落在区间内的部分，时间从 0 开始。
pub fn clip_chapters(chapters: &[Chapter], clip: Option<&Clip>) -> Vec<Chapter> {
    let Some(c) = clip else { return chapters.to_vec() };
    chapters
        .iter()
        .filter_map(|ch| {
            let start = ch.start_ms.max(c.start_ms);
            let end = c.end_ms.map_or(ch.end_ms, |e| ch.end_ms.min(e));
            (end > start + 500).then(|| Chapter { title: ch.title.clone(), start_ms: start - c.start_ms, end_ms: end - c.start_ms })
        })
        .collect()
}

/// 进度上报：限制频率、计算速度、定期写入数据库。
struct Reporter {
    app: AppHandle,
    st: Arc<AppState>,
    id: i64,
    i: usize,
    done_before: u64,
    baseline: Option<(Instant, u64)>,
    last_persist: Instant,
}

impl Reporter {
    fn new(app: &AppHandle, st: &Arc<AppState>, id: i64, i: usize, done_before: u64) -> Self {
        Reporter { app: app.clone(), st: st.clone(), id, i, done_before, baseline: None, last_persist: Instant::now() }
    }

    fn update(&mut self, received: u64, total: Option<u64>, speed: Option<u64>, meta_now: Option<&ResumeMeta>) {
        let now = Instant::now();
        let (t0, b0) = *self.baseline.get_or_insert((now, received));
        let elapsed = now.duration_since(t0);
        if elapsed < Duration::from_millis(250) && Some(received) != total {
            return;
        }
        let speed = speed.unwrap_or_else(|| (received.saturating_sub(b0) as f64 / elapsed.as_secs_f64().max(0.001)) as u64);
        self.baseline = Some((now, received));
        let save = self.last_persist.elapsed() >= PERSIST_EVERY;
        if save {
            self.last_persist = now;
        }
        let (i, done_before, st) = (self.i, self.done_before, &self.st);
        let snap = with_entry(st, self.id, |e| {
            e.snap.received = done_before + received;
            e.snap.total = total.map(|t| done_before + t);
            e.snap.speed = speed;
            if let Some(m) = meta_now {
                e.snap.resumable = m.resumable;
                if e.meta.inputs.len() > i {
                    e.meta.inputs[i] = m.clone();
                }
            }
            if save {
                persist(st, e);
            }
            e.snap.clone()
        });
        if let Some(s) = snap {
            let _ = self.app.emit(EVT_PROGRESS, s);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn download_input(
    app: &AppHandle,
    st: &Arc<AppState>,
    settings: &Settings,
    id: i64,
    media: &MediaInfo,
    spec: &mut JobSpec,
    i: usize,
    part: &Path,
    meta: &mut JobMeta,
    rx: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    done_before: u64,
    fmp4: &mut bool,
) -> Result<u64, DlError> {
    let mut re_resolved = false;
    let mut net_attempts = 0u32;
    loop {
        if let Some(dir) = part.parent() {
            if let Err(e) = tokio::fs::create_dir_all(dir).await {
                return Err(DlError::Io(format!("无法创建目录 {}：{e}", dir.display())));
            }
        }
        let asset = spec.inputs[i].clone();
        let mut reporter = Reporter::new(app, st, id, i, done_before);
        let res = if asset.kind == AssetKind::Subtitle {
            // 字幕文件很小：直接取回并按设置转换（不走分段下载引擎）
            match crate::subtitle_io::fetch_converted(st, settings, media, &asset, spec.post.sub_to.as_deref(), spec.post.clip.as_ref()).await {
                Ok((text, _)) => match tokio::fs::write(part, text.as_bytes()).await {
                    Ok(()) => {
                        let len = text.len() as u64;
                        meta.inputs[i] = ResumeMeta { total: Some(len), ..Default::default() };
                        reporter.update(len, Some(len), None, None);
                        Ok(len)
                    }
                    Err(e) => Err(DlError::Io(format!("无法保存字幕：{e}"))),
                },
                Err(e) => Err(e),
            }
        } else {
            match asset.protocol {
                Protocol::Http => {
                    let req = build_request(st, media, &asset);
                    let client = st.net.clients_for(&settings.network, &asset.url).map_err(|e| DlError::Other(e.message))?.download;
                    http_engine::download(&client, &req, part, &mut meta.inputs[i], rx, opts, |received, total, meta_now| {
                        reporter.update(received, total, None, Some(meta_now))
                    })
                    .await
                }
                Protocol::Hls => {
                    let req = build_request(st, media, &asset);
                    let client = st.net.clients_for(&settings.network, &asset.url).map_err(|e| DlError::Other(e.message))?.download;
                    let hopts = HlsOptions {
                        concurrency: settings.hls_concurrency,
                        skip_ads: settings.hls_skip_ads,
                        speed_limit_kbps: settings.speed_limit_kbps,
                        disk_reserve: opts.disk_reserve,
                        net: opts.net.clone(),
                    };
                    hls_engine::download(&client, &req, part, rx, &hopts, |received, total| reporter.update(received, total, None, None)).await.map(|r| {
                        *fmp4 = r.fmp4;
                        if r.skipped_ads > 0 {
                            log::info!("job {id}: skipped {} ad segments", r.skipped_ads);
                        }
                        meta.inputs[i] = ResumeMeta { total: Some(r.size), ..Default::default() };
                        r.size
                    })
                }
                Protocol::Ftp | Protocol::Sftp => {
                    let url = url::Url::parse(&asset.url).map_err(|_| DlError::Other("下载地址格式不正确".into()))?;
                    let cred = asset_credentials(&asset);
                    if asset.protocol == Protocol::Ftp {
                        ftp_engine::download(&url, cred.as_deref(), part, &mut meta.inputs[i], rx, opts, |received, total, meta_now| {
                            reporter.update(received, total, None, Some(meta_now))
                        })
                        .await
                    } else {
                        let known = sftp_engine::KnownHosts::new(st.data_dir.join("sftp_known_hosts.json"));
                        sftp_engine::download(&url, cred.as_deref(), &known, part, &mut meta.inputs[i], rx, opts, |received, total, meta_now| {
                            reporter.update(received, total, None, Some(meta_now))
                        })
                        .await
                    }
                }
                Protocol::Ed2k => {
                    let link = match ed2k_link::parse(&asset.url) {
                        Ok(ed2k_link::Ed2kLink::File(f)) => f,
                        _ => return Err(DlError::Other("ed2k 链接无法解析".into())),
                    };
                    let client = st.net.clients_for(&settings.network, &settings.ed2k_server_met_url).map_err(|e| DlError::Other(e.message))?.download;
                    let servers = ed2k::server::collect_servers(&client, &st.data_dir, &settings.ed2k_servers, &settings.ed2k_server_met_url).await;
                    if servers.is_empty() && link.sources.is_empty() {
                        task_log(st, id, "没有可用的 ed2k 服务器，也没有链接自带的来源");
                    }
                    ed2k::download::download(&link, &servers, part, rx, opts, ed2k::download::Config::default(), |received| {
                        reporter.update(received, Some(link.size), None, None)
                    })
                    .await
                    .inspect(|size| meta.inputs[i] = ResumeMeta { total: Some(*size), ..Default::default() })
                }
                Protocol::Bt => {
                    let probe = if asset.url.starts_with("magnet:") { crate::bt::trackers::DEFAULT_SOURCES[0] } else { asset.url.as_str() };
                    let client = st.net.clients_for(&settings.network, probe).map_err(|e| DlError::Other(e.message))?.download;
                    crate::bt::trackers::refresh_if_due(&client, &st.data_dir, settings).await;
                    let bt_opts = crate::bt::DownloadOptions {
                        trackers: crate::bt::trackers::effective(&st.data_dir, settings),
                        speed_limit_kbps: st.net.effective_limit(settings.speed_limit_kbps),
                        disk_reserve: opts.disk_reserve,
                        initial_peers: vec![],
                    };
                    let idx = asset.format_id.as_deref().and_then(|s| s.parse().ok());
                    crate::bt::download(&client, &asset.url, idx, part, rx, &bt_opts, |received, total, speed| {
                        reporter.update(received, Some(total), Some(speed), None)
                    })
                    .await
                    .inspect(|size| meta.inputs[i] = ResumeMeta { total: Some(*size), ..Default::default() })
                }
                Protocol::Stream => {
                    let ffmpeg = postprocess::find_ffmpeg(st).ok_or_else(|| DlError::Other(postprocess::ffmpeg_missing().message))?;
                    let cred = asset_credentials(&asset);
                    stream_engine::download(&ffmpeg, &asset.url, cred.as_deref(), part, rx, opts.disk_reserve, |n| reporter.update(n, None, None, None))
                        .await
                        .inspect(|size| meta.inputs[i] = ResumeMeta { total: Some(*size), ..Default::default() })
                }
                Protocol::Ytdlp => {
                    let ctx = st.parse_ctx(settings);
                    let format_id = asset.format_id.clone().unwrap_or_else(|| "best".into());
                    let page = if media.source_url.is_empty() { asset.url.clone() } else { media.source_url.clone() };
                    providers::ytdlp::download(&ctx, &page, &format_id, &asset.ext, part, rx, st.net.effective_limit(settings.speed_limit_kbps), |p| {
                        reporter.update(p.downloaded, p.total, p.speed, None)
                    })
                    .await
                    .inspect(|size| meta.inputs[i] = ResumeMeta { total: Some(*size), ..Default::default() })
                }
            }
        };
        let m = meta.clone();
        with_entry(st, id, |e| e.meta = m);

        match res {
            Err(DlError::Status(code))
                if matches!(code, 403 | 404 | 410)
                    && !re_resolved
                    && !media.source_url.is_empty()
                    && matches!(asset.protocol, Protocol::Http | Protocol::Hls) =>
            {
                re_resolved = true;
                task_log(st, id, format!("服务器返回 HTTP {code}，下载地址可能已过期"));
                set_note(app, id, Some("下载地址已过期，正在重新解析…"));
                let ctx = st.parse_ctx(settings);
                let fresh = providers::resolve_url(&ctx, &media.source_url).await.map_err(|e| DlError::Other(format!("下载地址已过期，重新解析失败：{e}")))?;
                for k in 0..spec.inputs.len() {
                    if let Some(a) = fresh.asset(&spec.inputs[k].id) {
                        spec.inputs[k] = a.clone();
                    } else if k == i {
                        return Err(DlError::Other("重新解析后找不到这个资源".into()));
                    }
                }
                let note = if asset.protocol == Protocol::Hls {
                    "已获取新地址，继续下载剩余分片".to_string()
                } else if keep_part_after_refresh(st, settings, media, &spec.inputs[i], part, &mut meta.inputs[i]).await {
                    format!("已获取新地址，从 {} 处继续", human_bytes(file_len(part)))
                } else {
                    "已获取新地址，文件已变化，从头下载".to_string()
                };
                let spec_json = serde_json::to_string(&*spec).unwrap_or_default();
                let _ = st.db.update_job_asset(id, &spec_json);
                let (s2, m2) = (spec.clone(), meta.clone());
                with_entry(st, id, |e| {
                    e.spec = s2;
                    e.meta = m2;
                });
                set_note(app, id, Some(&note));
            }
            Err(DlError::Net(msg)) if net_attempts < settings.max_retries => {
                net_attempts += 1;
                log::info!("job {id} network error, retry {net_attempts}: {msg}");
                let delay = Duration::from_secs(settings.retry_delay_secs.saturating_mul(1 << (net_attempts - 1).min(5)));
                task_log(st, id, format!("网络错误：{msg}；{} 秒后进行第 {net_attempts}/{} 次重试", delay.as_secs(), settings.max_retries));
                set_note(app, id, Some(&format!("网络中断，第 {net_attempts} 次重试…")));
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    c = wait_ctrl(rx) => return Err(DlError::from_ctrl(c)),
                }
            }
            other => return other,
        }
    }
}

/// FTP / SFTP / 流媒体资源的登录信息（"用户名:密码"，放在 Authorization 头里，不出现在地址中）。
fn asset_credentials(asset: &Asset) -> Option<String> {
    asset.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("authorization")).map(|(_, v)| v.clone())
}

fn push_log(e: &mut Entry, text: String) {
    if e.log.len() >= LOG_CAP {
        e.log.remove(0);
    }
    e.log.push(LogLine { at: chrono::Utc::now().timestamp_millis(), text });
}

fn protocol_name(p: Protocol) -> &'static str {
    match p {
        Protocol::Http => "HTTP 直链",
        Protocol::Hls => "m3u8 分片",
        Protocol::Ytdlp => "yt-dlp",
        Protocol::Ftp => "FTP",
        Protocol::Sftp => "SFTP",
        Protocol::Stream => "流媒体（ffmpeg）",
        Protocol::Ed2k => "ed2k",
        Protocol::Bt => "BT / 磁力",
    }
}

/// `p` 后面加上后缀（不替换扩展名）。
fn suffixed(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// yt-dlp 下载时可能留下的临时文件（输出名为“临时文件名.扩展名”，下载中再加 .part / .ytdl）。
fn ytdlp_leftovers(p: &Path) -> Vec<PathBuf> {
    ["mp4", "m4a", "webm", "mkv", "mp3", "flv", "ts", "mov", "opus", "ogg", "flac", "aac"]
        .iter()
        .flat_map(|ext| {
            let out = providers::ytdlp::output_path(p, ext);
            [suffixed(&out, ".part"), suffixed(&out, ".ytdl"), out]
        })
        .collect()
}

/// 删除一个输入的临时文件，连同 m3u8 分片目录和 yt-dlp 的临时文件。
fn remove_part(p: &Path) {
    let _ = std::fs::remove_file(p);
    for f in ytdlp_leftovers(p) {
        let _ = std::fs::remove_file(f);
    }
    let _ = std::fs::remove_dir_all(hls_engine::segment_dir(p));
    let _ = std::fs::remove_dir_all(suffixed(p, ".subs"));
    crate::bt::remove_staging(p);
    let _ = std::fs::remove_file(ed2k::download::sidecar_path(p));
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()).unwrap_or(0)
}

/// 某个输入已下载的字节数：分段下载按各段进度，m3u8 按已下载的分片，yt-dlp 按它的 .part 文件。
fn input_received(part: &Path, meta: &ResumeMeta) -> u64 {
    if !meta.segments.is_empty() {
        return meta.segmented_received();
    }
    // ed2k 的 .part 是预先分配好大小的，实际进度看已校验的分块
    if let Some(n) = ed2k::download::verified_bytes(part) {
        return n;
    }
    let done = file_len(part);
    if done > 0 {
        return done;
    }
    ytdlp_leftovers(part).iter().map(|f| file_len(f)).sum::<u64>() + dir_size(&hls_engine::segment_dir(part))
}

/// 移动到最终位置；跨磁盘时改为复制后删除。
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finish(app: &AppHandle, st: &AppState, id: i64, outcome: Result<u64, DlError>, part: &Path, final_path: &Path, inputs: &[PathBuf], settings: &Settings) {
    let mut notify: Option<(String, String)> = None;
    let mut done_file: Option<(PathBuf, Arc<MediaInfo>)> = None;
    let mut done_key: Option<(String, String, String)> = None;
    let mut downgrade: Option<(Arc<MediaInfo>, JobSpec, ErrorKind)> = None;
    let mut failed_note: Option<(String, String)> = None;
    let mut sub_touched = false;
    let leftovers: Vec<PathBuf> = inputs.iter().cloned().chain(std::iter::once(part.to_path_buf())).collect();
    let private = settings.security.privacy_mode;
    let found = with_entry(st, id, |e| {
        e.snap.speed = 0;
        e.snap.note = None;
        e.snap.step = None;
        match outcome {
            Ok(size) => {
                let dest = match settings.conflict_policy {
                    ConflictPolicy::Overwrite => {
                        let _ = std::fs::remove_file(final_path);
                        final_path.to_path_buf()
                    }
                    _ if final_path.exists() => naming::unique_path(final_path.to_path_buf(), &|p: &Path| p.exists()),
                    _ => final_path.to_path_buf(),
                };
                if let Some(dir) = dest.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                match move_file(part, &dest) {
                    Ok(()) => {
                        push_log(e, format!("下载完成（{}），已保存到 {}", human_bytes(size), dest.display()));
                        e.snap.file_path = dest.to_string_lossy().into_owned();
                        e.snap.status = TaskStatus::Done;
                        e.snap.received = size;
                        e.snap.total = Some(size);
                        e.snap.finished_at = Some(db::now());
                        // 隐私模式：不进媒体库、不写 nfo / info.json，也不触发后续的自动规则
                        if !private {
                            let _ = st.db.record_download(&NewDownload {
                                platform: &e.snap.platform,
                                media_id: &e.snap.media_id,
                                asset_id: &e.snap.asset_id,
                                title: &e.snap.title,
                                author: &e.snap.author,
                                cover: e.snap.cover.as_deref(),
                                path: &e.snap.file_path,
                                size: size as i64,
                                kind: kind_str(e.snap.asset_kind),
                                source: e.spec.origin.as_deref().unwrap_or("manual"),
                                source_url: &e.media.source_url,
                                platform_name: &e.media.platform_name,
                            });
                        }
                        notify = Some((naming::display_name(&e.snap.file_path), e.snap.asset_label.clone()));
                        if !private && matches!(e.snap.asset_kind, AssetKind::Video | AssetKind::Audio) {
                            crate::organize::write_sidecars(settings, &e.media, e.spec.primary(), &dest);
                        }
                        done_file = Some((dest.clone(), e.media.clone()));
                        if !private {
                            done_key = Some((e.snap.platform.clone(), e.snap.media_id.clone(), e.snap.asset_id.clone()));
                        }
                    }
                    Err(err) => {
                        push_log(e, format!("保存文件失败：{err}"));
                        e.snap.status = TaskStatus::Failed;
                        e.snap.error = Some(format!("保存文件失败：{err}"));
                        e.snap.error_kind = Some(ErrorKind::Disk);
                    }
                }
            }
            Err(DlError::Paused) => {
                push_log(e, format!("已暂停（已下载 {}）", human_bytes(e.snap.received)));
                e.snap.status = TaskStatus::Paused;
            }
            Err(DlError::Canceled) => {
                push_log(e, "已取消".into());
                e.snap.status = TaskStatus::Canceled;
                if !settings.keep_part_on_cancel {
                    e.snap.received = 0;
                    for p in &leftovers {
                        remove_part(p);
                    }
                    e.meta = JobMeta { inputs: vec![ResumeMeta::default(); e.spec.inputs.len()] };
                }
            }
            Err(err) => {
                log::warn!("job {} failed: {err}", e.snap.id);
                push_log(e, format!("失败（{:?}）：{err}", err.kind()));
                if settings.auto_downgrade {
                    downgrade = Some((e.media.clone(), e.spec.clone(), err.kind()));
                }
                e.snap.status = TaskStatus::Failed;
                e.snap.error_kind = Some(err.kind());
                e.snap.error = Some(err.to_string());
                failed_note = Some((naming::display_name(&e.snap.file_path), err.to_string()));
            }
        }
        persist(st, e);
        // 隐私模式：结束的任务不在数据库里留记录（本次运行期间列表里仍可看到）
        if private && matches!(e.snap.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Canceled) {
            let _ = st.db.delete_job(e.snap.id);
        }
        // 订阅条目跟随任务结果更新
        if let Some((sub_id, item_id)) = &e.spec.sub_item {
            let status = match e.snap.status {
                TaskStatus::Done => Some("downloaded"),
                TaskStatus::Failed => Some("failed"),
                _ => None,
            };
            if let Some(s) = status {
                crate::subs::mark_item(st, *sub_id, item_id, s, Some(&e.snap.title));
                sub_touched = true;
            }
        }
    });
    if sub_touched {
        let _ = app.emit(crate::subs::EVT_SUBS, ());
    }
    let mut downgraded = false;
    if let (Some((media, spec, kind)), Some(_)) = (downgrade, found) {
        downgraded = try_downgrade(app, id, &media, &spec, kind);
    }
    if let (Some((title, err)), false) = (failed_note, downgraded) {
        crate::notify::emit(app, crate::notify::Event::Failed, "下载失败", &format!("{title}\n{err}"));
    }
    if found.is_none() {
        // 任务已被移除：清理残留
        for p in &leftovers {
            remove_part(p);
        }
    }
    if let (Some((_, media)), false) = (&done_file, private) {
        spawn_cover_cache(app, media.clone());
    }
    if let Some((platform, media_id, asset_id)) = done_key {
        tauri::async_runtime::spawn(crate::library_cmds::after_download(app.clone(), platform, media_id, asset_id));
    }
    if let Some((file, media)) = &done_file {
        if settings.post_script_enabled && !settings.post_script.trim().is_empty() {
            run_post_script(&settings.post_script, file, media);
        }
    }
    let all_done = !st.downloads.has_active();
    if let Some((title, label)) = &notify {
        crate::notify::emit(app, crate::notify::Event::Done, "下载完成", &format!("{title}（{label}）"));
    }
    if let Some((title, label)) = notify {
        if settings.notify_on_complete && all_done {
            crate::security::system_notification(app, "下载完成", &format!("{title}（{label}）"), true);
        }
    }
    if all_done && done_file.is_some() {
        on_all_done(app, st, settings, done_file.as_ref().map(|(f, _)| f.as_path()));
    }
    crate::inbox::sync_tasks(app);
    schedule(app);
}

/// 下载失败后自动改用低一档的视频格式重新加入队列。成功加入时去掉失败的旧任务，返回 true。
/// 只处理网络、资源不存在、文件损坏等“换个格式可能就好”的失败；订阅任务不处理（保存位置由订阅决定）。
fn try_downgrade(app: &AppHandle, failed_id: i64, media: &MediaInfo, spec: &JobSpec, kind: ErrorKind) -> bool {
    let primary = spec.primary();
    if primary.kind != AssetKind::Video || spec.sub_item.is_some() || !matches!(kind, ErrorKind::Network | ErrorKind::NotFound | ErrorKind::Other) {
        return false;
    }
    let mut tried = spec.tried.clone();
    if !tried.contains(&primary.id) {
        tried.push(primary.id.clone());
    }
    if tried.len() > 3 {
        return false;
    }
    let Some(next) = crate::quality::next_lower(media, primary, &tried) else { return false };
    let describe = |a: &Asset| a.quality.clone().unwrap_or_else(|| a.label.clone());
    let note = format!("{} 下载失败，已自动改用 {}", describe(primary), describe(next));
    let extra = EnqueueExtra { origin: spec.origin.clone(), tried, ..Default::default() };
    let result = enqueue_ext(app, media.clone(), std::slice::from_ref(&next.id), spec.post.clone(), extra);
    match result {
        Ok(r) if !r.tasks.is_empty() => {
            let st = state(app);
            with_entry(&st, r.tasks[0].id, |e| e.snap.warning = Some(note));
            log::info!("job {failed_id}: downgraded to {}", next.id);
            remove(app, failed_id);
            true
        }
        _ => false,
    }
}

fn kind_str(k: AssetKind) -> &'static str {
    match k {
        AssetKind::Video => "video",
        AssetKind::Image => "image",
        AssetKind::Audio => "audio",
        AssetKind::Cover => "cover",
        AssetKind::Subtitle => "subtitle",
        AssetKind::File => "file",
    }
}

/// 媒体库封面缓存目录。
pub fn covers_dir(st: &AppState) -> PathBuf {
    st.data_dir.join("covers")
}

/// 在后台把封面缓存到本地（远程封面地址常常会过期）。
fn spawn_cover_cache(app: &AppHandle, media: Arc<MediaInfo>) {
    if media.cover.is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = state(&app);
        let settings = st.settings();
        let dir = covers_dir(&st);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        use sha2::Digest;
        let key = hex::encode(&sha2::Sha256::digest(format!("{}:{}", media.platform, media.id).as_bytes())[..12]);
        if let Some(existing) = ["jpg", "png"].iter().map(|e| dir.join(format!("{key}.{e}"))).find(|p| p.exists()) {
            let _ = st.db.set_cover_path(&media.platform, &media.id, &existing.to_string_lossy());
            return;
        }
        let near = dir.join(&key);
        if let Some(tmp) = crate::organize::fetch_cover(&st, &settings, &media, &near).await {
            let ext = tmp.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_string();
            let dest = dir.join(format!("{key}.{ext}"));
            if std::fs::rename(&tmp, &dest).is_ok() {
                let _ = st.db.set_cover_path(&media.platform, &media.id, &dest.to_string_lossy());
            }
        }
    });
}

/// 每个任务完成后运行用户设置的命令（文件路径、标题、原链接通过环境变量传入，不拼接进命令行）。
fn run_post_script(script: &str, file: &Path, media: &MediaInfo) {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", script]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", script]);
        c
    };
    cmd.env("CLEARCLIP_FILE", file)
        .env("CLEARCLIP_TITLE", &media.title)
        .env("CLEARCLIP_AUTHOR", &media.author)
        .env("CLEARCLIP_URL", &media.source_url)
        .env("CLEARCLIP_PLATFORM", &media.platform)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.spawn() {
        Ok(_) => log::info!("post script started for {}", file.display()),
        Err(e) => log::warn!("post script failed: {e}"),
    }
}

pub const EVT_POWER_COUNTDOWN: &str = "app://power-countdown";

/// 队列全部完成：打开文件夹、睡眠 / 关机（关机前倒计时 60 秒，期间可在界面取消）。
fn on_all_done(app: &AppHandle, st: &AppState, settings: &Settings, last_file: Option<&Path>) {
    crate::power::keep_awake(false);
    if settings.open_folder_on_done {
        if let Some(dir) = last_file.and_then(|f| f.parent()) {
            use tauri_plugin_opener::OpenerExt;
            let _ = app.opener().open_path(dir.to_string_lossy(), None::<&str>);
        }
    }
    let action = st.after_all_done.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if action == "none" {
        return;
    }
    let _ = app.emit(EVT_POWER_COUNTDOWN, serde_json::json!({ "action": action, "seconds": 60 }));
    crate::tray::show_main(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let st = state(&app);
        let still = st.after_all_done.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if still == action && !st.downloads.has_active() {
            *st.after_all_done.lock().unwrap_or_else(|e| e.into_inner()) = "none".into();
            crate::power::run_power_action(&action);
        }
    });
}

/// 地址更新后判断能否保留已下载部分：新地址的文件总大小与记录一致才保留。
async fn keep_part_after_refresh(st: &AppState, settings: &Settings, media: &MediaInfo, asset: &Asset, part: &Path, meta: &mut ResumeMeta) -> bool {
    let have = file_len(part);
    let Ok(client) = st.net.clients_for(&settings.network, &asset.url).map(|c| c.download) else { return false };
    let req = build_request(st, media, asset);
    let fresh_total = http_engine::probe_total(&client, &req).await;
    let keep = have > 0 && meta.total.is_some() && fresh_total == meta.total;
    if keep {
        // 新地址的 ETag 与旧 CDN 不同，续传时只依赖大小和 Content-Range 校验
        meta.etag = None;
        meta.last_modified = None;
    } else {
        let _ = std::fs::remove_file(part);
        *meta = ResumeMeta::default();
    }
    keep
}

fn set_note(app: &AppHandle, id: i64, note: Option<&str>) {
    let st = state(app);
    with_entry(&st, id, |e| e.snap.note = note.map(String::from));
    if let Some(n) = note {
        task_log(&st, id, n);
    }
    emit_all(app);
}

fn set_step(st: &AppState, id: i64, step: &str) {
    with_entry(st, id, |e| e.snap.step = Some(step.to_string()));
}

pub fn pause(app: &AppHandle, id: i64) {
    let st = state(app);
    with_entry(&st, id, |e| match e.snap.status {
        TaskStatus::Running => {
            let _ = e.ctrl.send(CTRL_PAUSE);
        }
        TaskStatus::Queued => {
            e.snap.status = TaskStatus::Paused;
            persist(&st, e);
        }
        _ => {}
    });
    emit_all(app);
}

/// 继续暂停的任务，或重试失败 / 已取消的任务。
pub fn resume(app: &AppHandle, id: i64) {
    let st = state(app);
    with_entry(&st, id, |e| {
        if matches!(e.snap.status, TaskStatus::Paused | TaskStatus::Failed | TaskStatus::Canceled) {
            push_log(e, "继续 / 重试，重新排队".into());
            e.snap.status = TaskStatus::Queued;
            e.snap.error = None;
            e.snap.error_kind = None;
            persist(&st, e);
        }
    });
    schedule(app);
}

fn all_parts(settings: &Settings, e: &Entry) -> Vec<PathBuf> {
    let n = e.spec.inputs.len();
    let fp = Path::new(&e.snap.file_path);
    let mut v: Vec<PathBuf> = (0..n).map(|i| input_part_path(settings, e.snap.id, fp, i, n)).collect();
    v.push(part_path(fp));
    // yt-dlp 下载时自己的临时文件，以及后处理的中间文件
    let extra: Vec<PathBuf> = v
        .iter()
        .flat_map(|p| {
            [suffixed(p, ".remux"), suffixed(p, ".audio"), suffixed(p, ".meta"), suffixed(p, ".clip"), suffixed(p, ".embed")]
                .into_iter()
                .chain(ytdlp_leftovers(p))
        })
        .collect();
    v.extend(extra);
    v
}

pub fn cancel(app: &AppHandle, id: i64) {
    let st = state(app);
    let settings = st.settings();
    with_entry(&st, id, |e| match e.snap.status {
        TaskStatus::Running => {
            let _ = e.ctrl.send(CTRL_CANCEL);
        }
        TaskStatus::Queued | TaskStatus::Paused | TaskStatus::Failed => {
            e.snap.status = TaskStatus::Canceled;
            if !settings.keep_part_on_cancel {
                e.snap.received = 0;
                for p in all_parts(&settings, e) {
                    remove_part(&p);
                }
                e.meta = JobMeta { inputs: vec![ResumeMeta::default(); e.spec.inputs.len()] };
            }
            persist(&st, e);
        }
        _ => {}
    });
    emit_all(app);
}

pub fn remove(app: &AppHandle, id: i64) {
    let st = state(app);
    let settings = st.settings();
    {
        let mut list = st.downloads.lock();
        if let Some(pos) = list.iter().position(|e| e.snap.id == id) {
            let e = list.remove(pos);
            if e.snap.status == TaskStatus::Running {
                let _ = e.ctrl.send(CTRL_CANCEL);
            } else if e.snap.status != TaskStatus::Done {
                for p in all_parts(&settings, &e) {
                    remove_part(&p);
                }
            }
            let _ = st.db.delete_job(id);
        }
    }
    schedule(app);
}

/// 任务详情：过程记录、每个输入的原始请求（敏感信息已打码）、所在路径。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskSnapshot,
    pub source_url: String,
    pub log: Vec<LogLine>,
    pub requests: Vec<RequestInfo>,
    /// 下载中的临时文件
    pub part_files: Vec<String>,
    /// 后处理选项的文字说明
    pub post: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestInfo {
    pub label: String,
    pub protocol: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// 使用的网络出口：直连 / 系统代理 / 自定义代理
    pub route: String,
}

/// 链接里常见的令牌类参数只留前几个字符，复制给别人看时不会泄露登录态。
pub fn mask_url(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url) else { return url.to_string() };
    let _ = u.set_username("");
    let _ = u.set_password(None);
    let pairs: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if pairs.is_empty() {
        return u.to_string();
    }
    u.set_query(None);
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in pairs {
            let sensitive = matches!(
                k.to_ascii_lowercase().as_str(),
                "token" | "sign" | "signature" | "key" | "auth" | "sig" | "session" | "sessionid" | "password" | "passwd" | "secret"
            );
            if sensitive && v.chars().count() > 6 {
                q.append_pair(&k, &format!("{}…", v.chars().take(4).collect::<String>()));
            } else {
                q.append_pair(&k, &v);
            }
        }
    }
    u.to_string()
}

/// 请求头的值：Cookie / 令牌只显示长度。
pub fn mask_header(name: &str, value: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "cookie" | "authorization" | "proxy-authorization" | "x-csrf-token" => format!("（已隐藏，{} 个字符）", value.chars().count()),
        _ => value.to_string(),
    }
}

pub fn task_detail(app: &AppHandle, id: i64) -> Option<TaskDetail> {
    let st = state(app);
    let settings = st.settings();
    let (snap, spec, media, log) = with_entry(&st, id, |e| (e.snap.clone(), e.spec.clone(), e.media.clone(), e.log.clone()))?;
    let requests = spec
        .inputs
        .iter()
        .map(|a| {
            let (url, headers) = if a.protocol == Protocol::Ytdlp {
                (media.source_url.clone(), vec![])
            } else {
                let req = build_request(&st, &media, a);
                (a.url.clone(), req.headers)
            };
            let route = match st.net.route_for(&settings.network, &url) {
                crate::settings::Route::Direct => "直连".to_string(),
                crate::settings::Route::System => "系统代理".to_string(),
                crate::settings::Route::Proxy(pid) => format!("代理 {pid}"),
            };
            RequestInfo {
                label: a.label.clone(),
                protocol: protocol_name(a.protocol).to_string(),
                url: mask_url(&url),
                headers: headers.into_iter().map(|(k, v)| (k.clone(), mask_header(&k, &v))).collect(),
                route,
            }
        })
        .collect();
    let part_files: Vec<String> = (0..spec.inputs.len())
        .map(|i| input_part_path(&settings, id, Path::new(&snap.file_path), i, spec.inputs.len()))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let p = &spec.post;
    let mut post = vec![];
    if spec.inputs.len() > 1 {
        post.push("下载后合并音视频".to_string());
    }
    if let Some(c) = &p.clip {
        let end = c.end_ms.map(|e| format!("{:.1}", e as f64 / 1000.0)).unwrap_or_else(|| "结尾".into());
        post.push(format!("只保留 {:.1} 秒到 {end}{}", c.start_ms as f64 / 1000.0, if c.precise { "（精确裁剪）" } else { "" }));
    }
    if let Some(m) = &p.sub_mode {
        post.push(format!("字幕：{}", if m == "burn" { "烧录进画面" } else { "内嵌为字幕轨" }));
    }
    if let Some(f) = &p.extract_audio {
        post.push(format!("提取音频（{}）", f.to_uppercase()));
    }
    if !p.split_chapters.is_empty() {
        post.push(format!("按 {} 个章节另外拆分", p.split_chapters.len()));
    }
    Some(TaskDetail { source_url: mask_url(&media.source_url), task: snap, log, requests, part_files, post })
}

pub fn clear_finished(app: &AppHandle) {
    let st = state(app);
    st.downloads.lock().retain(|e| !matches!(e.snap.status, TaskStatus::Done | TaskStatus::Canceled));
    let _ = st.db.delete_jobs_with_status(&["done", "canceled"]);
    emit_all(app);
}

pub fn pause_all(app: &AppHandle) {
    let ids: Vec<i64> =
        state(app).downloads.snapshots().into_iter().filter(|s| matches!(s.status, TaskStatus::Running | TaskStatus::Queued)).map(|s| s.id).collect();
    for id in ids {
        pause(app, id);
    }
}

pub fn resume_all(app: &AppHandle) {
    let st = state(app);
    {
        let mut list = st.downloads.lock();
        for e in list.iter_mut().filter(|e| matches!(e.snap.status, TaskStatus::Paused | TaskStatus::Failed)) {
            e.snap.status = TaskStatus::Queued;
            e.snap.error = None;
            e.snap.error_kind = None;
            persist(&st, e);
        }
    }
    schedule(app);
}

/// 下载目录和临时目录里不属于任何任务的 .part 文件。
pub fn orphan_parts(app: &AppHandle) -> Vec<OrphanPart> {
    let st = state(app);
    let settings = st.settings();
    let known: Vec<PathBuf> = st.downloads.lock().iter().filter(|e| e.snap.status != TaskStatus::Done).flat_map(|e| all_parts(&settings, e)).collect();
    let mut out = Vec::new();
    collect_parts(&settings.download_root(), 0, &known, &mut out);
    if !settings.temp_dir.is_empty() {
        collect_parts(Path::new(&settings.temp_dir), 3, &known, &mut out);
    }
    out
}

fn collect_parts(dir: &Path, depth: usize, known: &[PathBuf], out: &mut Vec<OrphanPart>) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        let Ok(md) = entry.metadata() else { continue };
        if md.is_dir() {
            collect_parts(&p, depth + 1, known, out);
        } else if p.extension().is_some_and(|x| x == "part") && !known.contains(&p) {
            let modified = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            out.push(OrphanPart { path: p.to_string_lossy().into_owned(), size: md.len(), modified });
        }
    }
}

/// 删除指定的孤立 .part 文件；只接受当前确实是孤立文件的路径。
pub fn delete_orphans(app: &AppHandle, paths: &[String]) -> usize {
    let orphans = orphan_parts(app);
    paths.iter().filter(|p| orphans.iter().any(|o| &o.path == *p)).filter(|p| std::fs::remove_file(p).is_ok()).count()
}

pub fn part_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn human_bytes(n: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if n as f64 >= MB {
        format!("{:.1} MB", n as f64 / MB)
    } else {
        format!("{:.0} KB", (n as f64 / 1024.0).max(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaKind;

    fn media() -> MediaInfo {
        let mut v1080 = Asset::video("https://cdn/v1080.m4s".into(), Some(1920), Some(1080));
        v1080.id = "video-1080".into();
        v1080.has_audio = Some(false);
        v1080.pair_audio = Some("audio-best".into());
        let mut audio = Asset::base("audio-best", AssetKind::Audio, "https://cdn/a.m4s".into(), "音频轨", "m4a");
        audio.has_audio = Some(true);
        let v720 = Asset { id: "video-720".into(), ..Asset::video("https://cdn/v720.mp4".into(), Some(1280), Some(720)) };
        MediaInfo {
            platform: "bilibili".into(),
            platform_name: "B站".into(),
            id: "BV1".into(),
            source_url: "x".into(),
            title: "t".into(),
            author: "a".into(),
            cover: None,
            duration_ms: None,
            kind: MediaKind::Video,
            width: None,
            height: None,
            published_at: None,
            assets: vec![v1080, v720, audio],
            entries: vec![],
            series: None,
            chapters: vec![],
            extractor: None,
        }
    }

    fn asset_of(kind: AssetKind, ext: &str) -> Asset {
        Asset::base("x", kind, "https://x".into(), "x", ext)
    }

    #[test]
    fn post_options_apply_per_asset_kind() {
        let sub = asset_of(AssetKind::Subtitle, "vtt");
        let ch = Chapter { title: "c".into(), start_ms: 0, end_ms: 5000 };
        let post = PostOptions {
            extract_audio: None,
            sub_mode: Some("soft".into()),
            embed_subs: vec![sub.clone()],
            clip: Some(Clip { start_ms: 1000, end_ms: None, precise: false }),
            split_chapters: vec![ch],
            ..Default::default()
        };
        let s = Settings::default();
        let video = post_for(&post, &asset_of(AssetKind::Video, "mp4"), &s);
        assert_eq!((video.embed_subs.len(), video.sub_mode.as_deref(), video.split_chapters.len()), (1, Some("soft"), 1));
        assert!(video.clip.is_some() && video.sub_to.is_none());
        let audio = post_for(&post, &asset_of(AssetKind::Audio, "m4a"), &s);
        assert!(audio.clip.is_some() && audio.embed_subs.is_empty() && audio.sub_mode.is_none() && audio.split_chapters.is_empty());
        // 字幕：转换目标按格式决定，裁剪区间保留，其他选项清掉
        let vtt = post_for(&post, &sub, &s);
        assert_eq!(vtt.sub_to.as_deref(), Some("srt"));
        assert!(vtt.clip.is_some() && vtt.embed_subs.is_empty() && vtt.sub_mode.is_none());
        assert_eq!(post_for(&post, &asset_of(AssetKind::Subtitle, "xml"), &s).sub_to.as_deref(), Some("ass"));
        assert_eq!(post_for(&post, &asset_of(AssetKind::Subtitle, "json"), &s).sub_to.as_deref(), Some("srt"));
        assert_eq!(post_for(&post, &asset_of(AssetKind::Subtitle, "srt"), &s).sub_to, None);
        assert_eq!(post_for(&post, &asset_of(AssetKind::Subtitle, "ass"), &s).sub_to, None);
        // 关闭转换且没有裁剪：VTT 保持原样；有裁剪时仍要转 SRT 才能同步裁剪
        let keep = Settings { subtitle_convert: false, danmaku_ass: false, ..Settings::default() };
        let no_clip = PostOptions::default();
        assert_eq!(post_for(&no_clip, &sub, &keep).sub_to, None);
        assert_eq!(post_for(&no_clip, &asset_of(AssetKind::Subtitle, "xml"), &keep).sub_to, None);
        assert_eq!(post_for(&post, &sub, &keep).sub_to.as_deref(), Some("srt"));
        assert_eq!(post_for(&post, &asset_of(AssetKind::Image, "jpg"), &s), PostOptions::default());
        assert_eq!(post_for(&post, &asset_of(AssetKind::Cover, "jpg"), &s), PostOptions::default());
    }

    #[test]
    fn chapters_follow_clip() {
        let chs = vec![
            Chapter { title: "一".into(), start_ms: 0, end_ms: 10_000 },
            Chapter { title: "二".into(), start_ms: 10_000, end_ms: 30_000 },
            Chapter { title: "三".into(), start_ms: 30_000, end_ms: 40_000 },
        ];
        assert_eq!(clip_chapters(&chs, None), chs);
        let clip = Clip { start_ms: 5_000, end_ms: Some(32_000), precise: false };
        let got = clip_chapters(&chs, Some(&clip));
        assert_eq!(got.len(), 3);
        assert_eq!((got[0].start_ms, got[0].end_ms), (0, 5_000));
        assert_eq!((got[1].start_ms, got[1].end_ms), (5_000, 25_000));
        assert_eq!((got[2].start_ms, got[2].end_ms), (25_000, 27_000));
        // 只覆盖了不到半秒的章节被丢弃
        let clip = Clip { start_ms: 0, end_ms: Some(10_200), precise: false };
        assert_eq!(clip_chapters(&chs, Some(&clip)).len(), 1);
    }

    #[test]
    fn silent_video_is_paired_with_audio() {
        let m = media();
        let specs = build_specs(&m, &["video-1080".into(), "video-720".into()]);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].inputs.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["video-1080", "audio-best"]);
        assert_eq!(specs[1].inputs.len(), 1, "video with audio is downloaded alone");
    }

    #[test]
    fn legacy_rows_parse() {
        let a = Asset::video("u".into(), None, None);
        let spec = parse_spec(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(spec.inputs.len(), 1);
        let meta = parse_meta(r#"{"etag":"e","total":5}"#);
        assert_eq!(meta.inputs[0].etag.as_deref(), Some("e"));
    }

    #[test]
    fn part_paths_for_inputs() {
        let s = Settings::default();
        let f = Path::new("/d/a.mp4");
        assert_eq!(input_part_path(&s, 1, f, 0, 1), PathBuf::from("/d/a.mp4.part"));
        assert_eq!(input_part_path(&s, 1, f, 1, 2), PathBuf::from("/d/a.mp4.f1.part"));
        let t = Settings { temp_dir: "/tmp/cc".into(), ..Settings::default() };
        assert_eq!(input_part_path(&t, 7, f, 0, 1), PathBuf::from("/tmp/cc/clearclip-7-0.part"));
    }

    #[test]
    fn move_file_works_within_same_fs() {
        let dir = std::env::temp_dir().join(format!("clearclip-mv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), b"x").unwrap();
        move_file(&dir.join("a"), &dir.join("b")).unwrap();
        assert!(dir.join("b").exists() && !dir.join("a").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reorder_moves() {
        let base = vec![1, 2, 3, 4];
        let mv = |pos: usize, to: &str| {
            let mut v = base.clone();
            reorder(&mut v, pos, to);
            v
        };
        assert_eq!(mv(2, "top"), vec![3, 1, 2, 4]);
        assert_eq!(mv(2, "up"), vec![1, 3, 2, 4]);
        assert_eq!(mv(0, "up"), vec![1, 2, 3, 4]);
        assert_eq!(mv(1, "down"), vec![1, 3, 2, 4]);
        assert_eq!(mv(3, "down"), vec![1, 2, 3, 4]);
        assert_eq!(mv(0, "bottom"), vec![2, 3, 4, 1]);
    }

    #[test]
    fn status_roundtrip() {
        for s in [TaskStatus::Queued, TaskStatus::Running, TaskStatus::Paused, TaskStatus::Done, TaskStatus::Failed, TaskStatus::Canceled] {
            assert_eq!(TaskStatus::parse(s.as_str()), s);
        }
    }

    #[test]
    fn urls_and_headers_are_masked() {
        assert_eq!(mask_url("https://user:pw@cdn.example.com/v.mp4?token=abcdef123456&x=1"), "https://cdn.example.com/v.mp4?token=abcd%E2%80%A6&x=1");
        assert_eq!(mask_url("https://a.com/v.m3u8"), "https://a.com/v.m3u8");
        assert_eq!(mask_url("not a url"), "not a url");
        assert_eq!(mask_header("Cookie", "SESSDATA=abc"), "（已隐藏，12 个字符）");
        assert_eq!(mask_header("authorization", "Bearer x"), "（已隐藏，8 个字符）");
        assert_eq!(mask_header("Referer", "https://b.com/"), "https://b.com/");
    }
}
