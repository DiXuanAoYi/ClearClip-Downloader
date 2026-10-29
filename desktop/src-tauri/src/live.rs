//! 直播录制：添加直播间后自动监控，开播即录制，下播自动结束。
//!
//! - 不转码：直接保存 FLV / TS（程序崩溃或断电时已录部分仍可播放）
//! - 自动分段：按时长或大小；每段重新连接，保证每个文件都有完整的文件头
//! - 断流重连：中断后重新获取直播流，继续录到新分段
//! - 磁盘保护：每段开始前和录制中检查剩余空间
//! - 录完处理：可选无损转为 MP4、合并同一场的分段

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::providers::live::{self as lp, LiveStatus, LiveStream, StreamFormat};
use crate::{naming, AppState};

pub const EVT_LIVE: &str = "live://updated";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveSettings {
    /// 开播后自动录制
    pub auto_record: bool,
    /// 清晰度名称；为空时录最高清晰度
    pub quality: String,
    /// 开播检测间隔（秒），不低于平台下限
    pub check_interval_s: u64,
    /// 每段时长（分钟），0 表示不按时长分段
    pub segment_minutes: u64,
    /// 每段大小（MB），0 表示不按大小分段
    pub segment_mb: u64,
    /// 录完后无损转为 MP4
    pub convert_mp4: bool,
    /// 录完后合并同一场的分段（需要转 MP4）
    pub merge_segments: bool,
    /// 录完后按这个视频规整预设处理（见“工具箱 → 视频规整”）；留空不处理。会在录制文件旁边生成新文件，原文件保留
    pub normalize: String,
    /// 保存目录；为空时为“下载目录 / 直播 / 主播名”
    pub dir: String,
    pub notify: bool,
    /// 预约时段：不为空时只在这些时段检测开播和录制，时段结束时自动开始的录制随之停止
    pub schedule: Vec<TimeWindow>,
}

/// 每周重复的时间段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TimeWindow {
    /// 星期几：1 = 周一 … 7 = 周日；为空表示每天
    pub days: Vec<u8>,
    /// `HH:MM`
    pub start: String,
    pub end: String,
}

impl Default for TimeWindow {
    fn default() -> Self {
        TimeWindow { days: vec![], start: "20:00".into(), end: "23:00".into() }
    }
}

/// `HH:MM` → 当天的第几分钟。
pub fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// 现在（星期几 1–7、当天第几分钟）是否在预约时段里。没有设置时段表示一直允许。结束时间早于开始时间表示跨过午夜。
pub fn in_schedule(windows: &[TimeWindow], weekday: u8, minute: u32) -> bool {
    if windows.is_empty() {
        return true;
    }
    let yesterday = if weekday == 1 { 7 } else { weekday - 1 };
    windows.iter().any(|w| {
        let (Some(s), Some(e)) = (parse_hhmm(&w.start), parse_hhmm(&w.end)) else { return false };
        let on = |d: u8| w.days.is_empty() || w.days.contains(&d);
        match s.cmp(&e) {
            std::cmp::Ordering::Less => on(weekday) && (s..e).contains(&minute),
            std::cmp::Ordering::Greater => (on(weekday) && minute >= s) || (on(yesterday) && minute < e),
            std::cmp::Ordering::Equal => false,
        }
    })
}

fn schedule_allows(settings: &LiveSettings) -> bool {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    in_schedule(&settings.schedule, now.weekday().number_from_monday() as u8, now.hour() * 60 + now.minute())
}

impl Default for LiveSettings {
    fn default() -> Self {
        LiveSettings {
            auto_record: true,
            quality: String::new(),
            check_interval_s: 60,
            segment_minutes: 60,
            segment_mb: 0,
            convert_mp4: false,
            merge_segments: false,
            normalize: String::new(),
            dir: String::new(),
            notify: true,
            schedule: vec![],
        }
    }
}

impl LiveSettings {
    pub fn normalize(&mut self, platform: &str) {
        self.check_interval_s = self.check_interval_s.clamp(lp::min_interval(platform), 3600);
        self.segment_minutes = self.segment_minutes.min(24 * 60);
        self.quality = self.quality.trim().to_string();
        self.dir = self.dir.trim().to_string();
        if !self.convert_mp4 {
            self.merge_segments = false;
        }
        if crate::vidnorm::spec::preset(&self.normalize).is_none() {
            self.normalize.clear();
        }
        self.schedule.retain(|w| parse_hhmm(&w.start).is_some() && parse_hhmm(&w.end).is_some() && w.start.trim() != w.end.trim());
        self.schedule.truncate(14);
        for w in &mut self.schedule {
            w.days.retain(|d| (1..=7).contains(d));
            w.days.sort_unstable();
            w.days.dedup();
            w.start = w.start.trim().to_string();
            w.end = w.end.trim().to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveRoom {
    pub id: i64,
    pub url: String,
    pub platform: String,
    pub platform_name: String,
    pub streamer: String,
    pub title: String,
    pub avatar: Option<String>,
    pub cover: Option<String>,
    pub settings: LiveSettings,
    /// 是否监控开播
    pub monitoring: bool,
    pub created_at: i64,
    // ---- 运行时状态 ----
    /// offline / live / recording / error / checking
    pub state: String,
    pub last_check: Option<i64>,
    pub error: Option<String>,
    pub rec_started: Option<i64>,
    pub rec_bytes: u64,
    pub rec_file: Option<String>,
    pub rec_segments: usize,
    pub qualities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: i64,
    pub live_id: i64,
    pub streamer: String,
    pub title: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub files: Vec<String>,
    pub size: i64,
    pub status: String,
}

/// 每个直播间的运行时状态。
#[derive(Debug, Clone, Default)]
struct Runtime {
    state: String,
    last_check: Option<i64>,
    next_check: i64,
    error: Option<String>,
    rec_started: Option<i64>,
    rec_bytes: u64,
    rec_file: Option<String>,
    rec_segments: usize,
    qualities: Vec<String>,
    stop: Option<watch::Sender<bool>>,
    /// 用户手动停止：本场直播不再自动录制，下播后恢复
    manual_stop: bool,
    /// 当前这场录制是监控到开播后自动开始的（预约时段结束时只停止这样的录制）
    auto_started: bool,
}

#[derive(Default)]
pub struct LiveState {
    rt: Mutex<HashMap<i64, Runtime>>,
}

impl LiveState {
    fn with<R>(&self, id: i64, f: impl FnOnce(&mut Runtime) -> R) -> R {
        let mut m = self.rt.lock().unwrap_or_else(|e| e.into_inner());
        f(m.entry(id).or_default())
    }

    fn recording_count(&self) -> usize {
        self.rt.lock().unwrap_or_else(|e| e.into_inner()).values().filter(|r| r.stop.is_some()).count()
    }

    pub fn any_recording(&self) -> bool {
        self.recording_count() > 0
    }
}

fn st(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVT_LIVE, ());
}

// ---------- 数据库 ----------

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS live_rooms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    platform TEXT NOT NULL,
    platform_name TEXT NOT NULL,
    streamer TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    avatar TEXT,
    cover TEXT,
    settings_json TEXT NOT NULL,
    monitoring INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS live_recordings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    live_id INTEGER NOT NULL,
    streamer TEXT NOT NULL,
    title TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    files_json TEXT NOT NULL DEFAULT '[]',
    size INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'recording'
);
";

const ROOM_COLS: &str = "id, url, platform, platform_name, streamer, title, avatar, cover, settings_json, monitoring, created_at";

fn room_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LiveRoom> {
    let s: String = r.get(8)?;
    Ok(LiveRoom {
        id: r.get(0)?,
        url: r.get(1)?,
        platform: r.get(2)?,
        platform_name: r.get(3)?,
        streamer: r.get(4)?,
        title: r.get(5)?,
        avatar: r.get(6)?,
        cover: r.get(7)?,
        settings: serde_json::from_str(&s).unwrap_or_default(),
        monitoring: r.get::<_, i64>(9)? != 0,
        created_at: r.get(10)?,
        state: "offline".into(),
        last_check: None,
        error: None,
        rec_started: None,
        rec_bytes: 0,
        rec_file: None,
        rec_segments: 0,
        qualities: vec![],
    })
}

impl Db {
    pub fn live_rooms(&self) -> AppResult<Vec<LiveRoom>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {ROOM_COLS} FROM live_rooms ORDER BY created_at"))?;
        let rows = stmt.query_map([], room_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn live_room(&self, id: i64) -> AppResult<Option<LiveRoom>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {ROOM_COLS} FROM live_rooms WHERE id=?1"))?;
        Ok(stmt.query_row(params![id], room_row).optional()?)
    }

    fn live_insert(&self, url: &str, s: &LiveStatus, settings: &LiveSettings) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO live_rooms (url, platform, platform_name, streamer, title, avatar, cover, settings_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![url, s.platform, s.platform_name, if s.streamer.is_empty() { &s.room_id } else { &s.streamer }, s.title, s.avatar, s.cover, serde_json::to_string(settings)?, now()],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => AppError::invalid("这个直播间已经添加过了。"),
            e => e.into(),
        })?;
        Ok(conn.last_insert_rowid())
    }

    pub fn live_update_settings(&self, id: i64, streamer: &str, settings: &LiveSettings) -> AppResult<()> {
        self.conn().execute("UPDATE live_rooms SET streamer=?2, settings_json=?3 WHERE id=?1", params![id, streamer, serde_json::to_string(settings)?])?;
        Ok(())
    }

    fn live_update_info(&self, id: i64, s: &LiveStatus) -> AppResult<()> {
        self.conn().execute(
            "UPDATE live_rooms SET title=?2, cover=COALESCE(?3, cover), avatar=COALESCE(?4, avatar), streamer=CASE WHEN ?5<>'' THEN ?5 ELSE streamer END WHERE id=?1",
            params![id, s.title, s.cover, s.avatar, s.streamer],
        )?;
        Ok(())
    }

    pub fn live_set_monitoring(&self, id: i64, on: bool) -> AppResult<()> {
        self.conn().execute("UPDATE live_rooms SET monitoring=?2 WHERE id=?1", params![id, on as i64])?;
        Ok(())
    }

    pub fn live_delete(&self, id: i64) -> AppResult<()> {
        self.conn().execute("DELETE FROM live_rooms WHERE id=?1", params![id])?;
        Ok(())
    }

    fn rec_start(&self, live_id: i64, streamer: &str, title: &str) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute("INSERT INTO live_recordings (live_id, streamer, title, started_at) VALUES (?1, ?2, ?3, ?4)", params![live_id, streamer, title, now()])?;
        Ok(conn.last_insert_rowid())
    }

    fn rec_update(&self, id: i64, files: &[String], size: u64, status: &str, ended: bool) -> AppResult<()> {
        let ended_at = ended.then(now);
        self.conn().execute(
            "UPDATE live_recordings SET files_json=?2, size=?3, status=?4, ended_at=COALESCE(?5, ended_at) WHERE id=?1",
            params![id, serde_json::to_string(files)?, size as i64, status, ended_at],
        )?;
        Ok(())
    }

    pub fn recordings(&self, live_id: Option<i64>) -> AppResult<Vec<Recording>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, live_id, streamer, title, started_at, ended_at, files_json, size, status FROM live_recordings WHERE (?1 IS NULL OR live_id=?1) ORDER BY started_at DESC LIMIT 500",
        )?;
        let rows = stmt.query_map(params![live_id], |r| {
            let files: String = r.get(6)?;
            Ok(Recording {
                id: r.get(0)?,
                live_id: r.get(1)?,
                streamer: r.get(2)?,
                title: r.get(3)?,
                started_at: r.get(4)?,
                ended_at: r.get(5)?,
                files: serde_json::from_str(&files).unwrap_or_default(),
                size: r.get(7)?,
                status: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 删除早于 `before` 结束的录像记录，返回它们的文件。
    fn rec_take_old(&self, before: i64) -> AppResult<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT files_json FROM live_recordings WHERE status<>'recording' AND COALESCE(ended_at, started_at) < ?1")?;
        let files: Vec<String> = stmt
            .query_map(params![before], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .flat_map(|j| serde_json::from_str::<Vec<String>>(&j).unwrap_or_default())
            .collect();
        conn.execute("DELETE FROM live_recordings WHERE status<>'recording' AND COALESCE(ended_at, started_at) < ?1", params![before])?;
        Ok(files)
    }

    /// 启动时把上次异常退出时仍在“录制中”的记录标记为中断。
    fn rec_mark_interrupted(&self) -> AppResult<()> {
        self.conn().execute("UPDATE live_recordings SET status='interrupted', ended_at=COALESCE(ended_at, started_at) WHERE status='recording'", [])?;
        Ok(())
    }
}

// ---------- 对外操作 ----------

pub fn rooms(app: &AppHandle) -> AppResult<Vec<LiveRoom>> {
    let state = st(app);
    let mut rooms = state.db.live_rooms()?;
    let rt = state.live.rt.lock().unwrap_or_else(|e| e.into_inner());
    for r in &mut rooms {
        if let Some(x) = rt.get(&r.id) {
            r.state = if x.state.is_empty() { "offline".into() } else { x.state.clone() };
            r.last_check = x.last_check;
            r.error = x.error.clone();
            r.rec_started = x.rec_started;
            r.rec_bytes = x.rec_bytes;
            r.rec_file = x.rec_file.clone();
            r.rec_segments = x.rec_segments;
            r.qualities = x.qualities.clone();
        }
    }
    Ok(rooms)
}

pub async fn check_status(app: &AppHandle, url: &str) -> AppResult<LiveStatus> {
    let state = st(app);
    let settings = state.settings();
    let ctx = state.parse_ctx(&settings);
    lp::status(&ctx, url).await
}

pub async fn add(app: &AppHandle, url: &str, mut settings: LiveSettings) -> AppResult<LiveRoom> {
    let url = url.trim().to_string();
    let status = check_status(app, &url).await?;
    settings.normalize(lp::platform_of(&url));
    let state = st(app);
    let id = state.db.live_insert(&url, &status, &settings)?;
    state.live.with(id, |r| {
        r.state = if status.live { "live".into() } else { "offline".into() };
        r.last_check = Some(now());
        r.qualities = qualities(&status);
    });
    emit(app);
    if status.live && settings.auto_record {
        start_recording(app, id, Some(status));
    }
    state.db.live_room(id)?.ok_or_else(|| AppError::msg("保存失败"))
}

fn qualities(s: &LiveStatus) -> Vec<String> {
    let mut v: Vec<(u32, String)> = s.streams.iter().map(|x| (x.rank, x.quality.clone())).collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.0));
    let mut out: Vec<String> = vec![];
    for (_, q) in v {
        if !out.contains(&q) {
            out.push(q);
        }
    }
    out
}

pub fn stop_recording(app: &AppHandle, id: i64) {
    if let Some(tx) = st(app).live.with(id, |r| {
        r.manual_stop = true;
        r.stop.clone()
    }) {
        let _ = tx.send(true);
    }
}

pub fn set_monitoring(app: &AppHandle, id: i64, on: bool) -> AppResult<()> {
    let state = st(app);
    state.db.live_set_monitoring(id, on)?;
    state.live.with(id, |r| r.next_check = 0);
    emit(app);
    Ok(())
}

pub fn delete(app: &AppHandle, id: i64) -> AppResult<()> {
    stop_recording(app, id);
    let state = st(app);
    state.db.live_delete(id)?;
    state.live.rt.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    emit(app);
    Ok(())
}

/// 手动开始录制（不论是否开启自动录制）。
pub fn start_recording(app: &AppHandle, id: i64, status: Option<LiveStatus>) {
    let state = st(app);
    if state.live.with(id, |r| r.stop.is_some()) {
        return;
    }
    let (tx, rx) = watch::channel(false);
    state.live.with(id, |r| {
        r.manual_stop = false;
        r.auto_started = false;
        r.stop = Some(tx);
        r.state = "recording".into();
        r.error = None;
    });
    emit(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = record_session(&app, id, status, rx).await;
        let state = st(&app);
        state.live.with(id, |r| {
            r.stop = None;
            r.rec_file = None;
            r.state = match &result {
                Err(e) => {
                    r.error = Some(e.message.clone());
                    "error".into()
                }
                Ok(_) => "offline".into(),
            };
            // 结束后尽快再检测一次（可能只是断流）
            r.next_check = now() + 10;
        });
        crate::power::keep_awake(state.downloads.has_active_running() || state.live.any_recording());
        emit(&app);
    });
}

// ---------- 录制 ----------

enum SegEnd {
    /// 达到分段时长 / 大小
    Full,
    /// 用户停止
    Stopped,
    /// 直播流结束或中断
    Ended(String),
}

fn segment_path(dir: &Path, started: chrono::DateTime<chrono::Local>, title: &str, index: usize, ext: &str) -> PathBuf {
    let title: String = naming::sanitize(title).chars().take(40).collect();
    let title = if title.trim().is_empty() { "直播".to_string() } else { title };
    let base = naming::avoid_reserved(format!("{}_{}_{index:02}", started.format("%Y-%m-%d_%H%M%S"), title.trim()));
    dir.join(format!("{base}.{ext}"))
}

async fn record_session(app: &AppHandle, id: i64, initial: Option<LiveStatus>, mut stop: watch::Receiver<bool>) -> AppResult<()> {
    let state = st(app);
    let room = state.db.live_room(id)?.ok_or_else(|| AppError::not_found("直播间不存在"))?;
    let settings = state.settings();
    let mut status = match initial {
        Some(s) => s,
        None => check_status(app, &room.url).await?,
    };
    if !status.live {
        return Err(AppError::invalid("主播还没有开播"));
    }
    let _ = state.db.live_update_info(id, &status);
    let streamer = if status.streamer.is_empty() { room.streamer.clone() } else { status.streamer.clone() };
    let dir = if room.settings.dir.is_empty() {
        settings.download_root().join("直播").join(naming::sanitize(&streamer))
    } else {
        PathBuf::from(&room.settings.dir)
    };
    tokio::fs::create_dir_all(&dir).await.map_err(|e| AppError::new(crate::error::ErrorKind::Disk, format!("无法创建目录：{e}")))?;
    let started = chrono::Local::now();
    let rec_id = state.db.rec_start(id, &streamer, &status.title)?;
    state.live.with(id, |r| {
        r.rec_started = Some(now());
        r.rec_bytes = 0;
        r.rec_segments = 0;
    });
    crate::power::keep_awake(settings.prevent_sleep);
    if room.settings.notify {
        notify(app, "开始录制", &format!("{streamer}：{}", status.title));
    }

    let mut files: Vec<String> = vec![];
    let mut total: u64 = 0;
    let mut index = 1;
    let mut failures = 0;
    let reserve = settings.disk_reserve_mb * 1024 * 1024;
    let outcome: AppResult<()> = loop {
        if *stop.borrow() {
            break Ok(());
        }
        if let Err(e) = crate::engine::ensure_space(&dir, 512 * 1024 * 1024, reserve) {
            break Err(AppError::new(crate::error::ErrorKind::Disk, format!("磁盘空间不足，已停止录制：{e}")));
        }
        let Some(stream) = status.pick(&room.settings.quality).cloned() else {
            break Err(AppError::msg("没有可录制的直播流"));
        };
        let ext = if stream.format == StreamFormat::Flv { "flv" } else { "ts" };
        let path = segment_path(&dir, started, &status.title, index, ext);
        files.push(path.to_string_lossy().into_owned());
        state.live.with(id, |r| {
            r.rec_file = Some(path.to_string_lossy().into_owned());
            r.rec_segments = index;
        });
        let _ = state.db.rec_update(rec_id, &files, total, "recording", false);
        emit(app);
        let max_bytes = room.settings.segment_mb * 1024 * 1024;
        let max_time = Duration::from_secs(room.settings.segment_minutes * 60);
        let base_total = total;
        let progress = |bytes: u64| {
            state.live.with(id, |r| r.rec_bytes = base_total + bytes);
        };
        let res = match stream.format {
            StreamFormat::Flv => record_flv(&state, &settings, &stream, &path, max_bytes, max_time, &mut stop, &progress).await,
            StreamFormat::Hls => record_hls(&state, &settings, &stream, &path, max_bytes, max_time, &mut stop, &progress).await,
        };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        total += size;
        if size == 0 {
            // 没录到内容的分段不保留
            let _ = std::fs::remove_file(&path);
            files.pop();
        }
        state.live.with(id, |r| r.rec_bytes = total);
        match res {
            Ok(SegEnd::Stopped) => break Ok(()),
            Ok(SegEnd::Full) => {
                index += 1;
                failures = 0;
            }
            Ok(SegEnd::Ended(reason)) | Err(reason) => {
                log::info!("live {id}: stream ended ({reason}), rechecking");
                // 断流：确认是否仍在直播，最多重试几次
                let mut still = None;
                for attempt in 0..3 {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5 + attempt * 5)) => {}
                        _ = stop.changed() => break,
                    }
                    if *stop.borrow() {
                        break;
                    }
                    match check_status(app, &room.url).await {
                        Ok(s) if s.live && !s.streams.is_empty() => {
                            still = Some(s);
                            break;
                        }
                        Ok(_) => break,
                        Err(e) => log::info!("live {id}: recheck failed: {e}"),
                    }
                }
                match still {
                    Some(s) => {
                        status = s;
                        if size > 0 {
                            index += 1;
                        }
                        failures += 1;
                        if failures > 20 {
                            break Err(AppError::msg("直播流反复中断，已停止录制"));
                        }
                    }
                    None => break Ok(()),
                }
            }
        }
    };

    let status_name = if outcome.is_ok() { "done" } else { "error" };
    let _ = state.db.rec_update(rec_id, &files, total, status_name, true);
    state.live.with(id, |r| {
        r.rec_started = None;
        r.rec_file = None;
    });
    emit(app);
    let mut finals = files.clone();
    if !files.is_empty() && room.settings.convert_mp4 {
        let converted = post_process(&state, &files, room.settings.merge_segments).await;
        if let Ok(f) = converted {
            let size = f.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
            let _ = state.db.rec_update(rec_id, &f, size, status_name, true);
            finals = f;
        }
    }
    // 按预设规整录制文件：修正时间戳和音画不同步、统一响度等
    if let Some(spec) = crate::vidnorm::spec::preset(&room.settings.normalize) {
        for f in finals.iter().filter(|f| Path::new(f).is_file()) {
            let job = crate::media_tools::ToolJob {
                inputs: vec![f.clone()],
                op: crate::media_tools::ToolOp::Normalize { spec: spec.clone(), preset: Some(room.settings.normalize.clone()) },
                output_dir: None,
            };
            if let Err(e) = crate::media_tools::start(app, job) {
                log::warn!("live recording normalize failed to start: {e}");
            }
        }
    }
    if room.settings.notify {
        match &outcome {
            Ok(()) => notify(app, "录制结束", &format!("{streamer}：共 {} 个文件，{:.1} MB", files.len(), total as f64 / 1048576.0)),
            Err(e) => notify(app, "录制异常", &format!("{streamer}：{e}")),
        }
    }
    outcome
}

fn build_req(state: &AppState, settings: &crate::settings::Settings, stream: &LiveStream, url: &str) -> Result<reqwest::RequestBuilder, String> {
    let client = state.net.clients_for(&settings.network, url).map_err(|e| e.message)?.download;
    let mut req = client.get(url);
    for (k, v) in &stream.headers {
        req = req.header(k, v);
    }
    Ok(req)
}

/// 按 FLV 标签切分数据：只输出完整的标签，分段结束时丢弃不完整的尾部，保证每个文件都能正常播放和合并。
#[derive(Default)]
pub struct FlvCutter {
    buf: Vec<u8>,
    header_done: bool,
    /// 不是标准 FLV 时原样写入
    passthrough: bool,
}

impl FlvCutter {
    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        if self.passthrough {
            return data.to_vec();
        }
        self.buf.extend_from_slice(data);
        let mut out = vec![];
        if !self.header_done {
            if self.buf.len() < 13 {
                return out;
            }
            if &self.buf[..3] != b"FLV" {
                self.passthrough = true;
                return std::mem::take(&mut self.buf);
            }
            let offset = u32::from_be_bytes([self.buf[5], self.buf[6], self.buf[7], self.buf[8]]) as usize;
            let head = offset.max(9) + 4;
            if self.buf.len() < head {
                return out;
            }
            out.extend(self.buf.drain(..head));
            self.header_done = true;
        }
        let mut pos = 0;
        while self.buf.len() - pos >= 11 {
            let size = ((self.buf[pos + 1] as usize) << 16) | ((self.buf[pos + 2] as usize) << 8) | self.buf[pos + 3] as usize;
            let total = 11 + size + 4;
            if self.buf.len() - pos < total {
                break;
            }
            pos += total;
        }
        out.extend(self.buf.drain(..pos));
        out
    }
}

/// 录制 HTTP-FLV：持续写入直到达到分段条件、停止或断流。
#[allow(clippy::too_many_arguments)]
async fn record_flv(
    state: &AppState,
    settings: &crate::settings::Settings,
    stream: &LiveStream,
    path: &Path,
    max_bytes: u64,
    max_time: Duration,
    stop: &mut watch::Receiver<bool>,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<SegEnd, String> {
    let resp = build_req(state, settings, stream, &stream.url)?.send().await.map_err(|e| crate::error::describe_reqwest(&e))?;
    if !resp.status().is_success() {
        return Err(format!("直播流返回 {}", resp.status().as_u16()));
    }
    let mut file = tokio::fs::File::create(path).await.map_err(|e| format!("无法写入文件：{e}"))?;
    let mut body = resp.bytes_stream();
    let started = Instant::now();
    let mut written: u64 = 0;
    let mut cutter = FlvCutter::default();
    let mut last_report = Instant::now();
    loop {
        let chunk = tokio::select! {
            c = tokio::time::timeout(Duration::from_secs(30), body.next()) => c,
            _ = stop.changed() => {
                let _ = file.flush().await;
                return Ok(SegEnd::Stopped);
            }
        };
        match chunk {
            Err(_) => return Ok(SegEnd::Ended("30 秒没有收到数据".into())),
            Ok(None) => return Ok(SegEnd::Ended("直播流结束".into())),
            Ok(Some(Err(e))) => return Ok(SegEnd::Ended(e.to_string())),
            Ok(Some(Ok(data))) => {
                let data = cutter.feed(&data);
                file.write_all(&data).await.map_err(|e| format!("写入失败：{e}"))?;
                written += data.len() as u64;
                if last_report.elapsed() > Duration::from_millis(800) {
                    last_report = Instant::now();
                    progress(written);
                }
                if (max_bytes > 0 && written >= max_bytes) || (!max_time.is_zero() && started.elapsed() >= max_time) {
                    let _ = file.flush().await;
                    progress(written);
                    return Ok(SegEnd::Full);
                }
            }
        }
    }
}

/// 录制直播 HLS：轮询播放列表，按媒体序号追加新分片。
#[allow(clippy::too_many_arguments)]
async fn record_hls(
    state: &AppState,
    settings: &crate::settings::Settings,
    stream: &LiveStream,
    path: &Path,
    max_bytes: u64,
    max_time: Duration,
    stop: &mut watch::Receiver<bool>,
    progress: &(dyn Fn(u64) + Send + Sync),
) -> Result<SegEnd, String> {
    use crate::engine::hls;
    let fetch = |url: String| async move {
        let r = build_req(state, settings, stream, &url)?.timeout(Duration::from_secs(30)).send().await.map_err(|e| crate::error::describe_reqwest(&e))?;
        if !r.status().is_success() {
            return Err(format!("直播流返回 {}", r.status().as_u16()));
        }
        r.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
    };
    // 主播放列表：选码率最高的子流
    let mut media_url = stream.url.clone();
    let first = String::from_utf8_lossy(&fetch(media_url.clone()).await?).to_string();
    if hls::is_master(&first) {
        media_url = hls::parse_master(&first, &media_url).into_iter().max_by_key(|v| v.bandwidth).map(|v| v.url).ok_or("主播放列表里没有子流")?;
    }
    let mut file = tokio::fs::File::create(path).await.map_err(|e| format!("无法写入文件：{e}"))?;
    let started = Instant::now();
    let mut written: u64 = 0;
    let mut last_seq: Option<u64> = None;
    let mut last_new = Instant::now();
    loop {
        if *stop.borrow() {
            return Ok(SegEnd::Stopped);
        }
        let text = match fetch(media_url.clone()).await {
            Ok(b) => String::from_utf8_lossy(&b).to_string(),
            Err(e) => return Ok(SegEnd::Ended(e)),
        };
        let pl = match hls::parse_media(&text, &media_url) {
            Ok(p) => p,
            Err(e) => return Ok(SegEnd::Ended(e.to_string())),
        };
        let mut wait = Duration::from_secs_f64((pl.target_duration / 2.0).clamp(1.0, 6.0));
        let after = last_seq;
        for seg in pl.segments.iter().filter(|s| after.map_or(true, |l| s.seq > l)) {
            let data = match fetch(seg.url.clone()).await {
                Ok(d) => d,
                Err(e) => {
                    log::info!("live hls segment failed: {e}");
                    continue;
                }
            };
            file.write_all(&data).await.map_err(|e| format!("写入失败：{e}"))?;
            written += data.len() as u64;
            last_seq = Some(seg.seq);
            last_new = Instant::now();
            progress(written);
            if (max_bytes > 0 && written >= max_bytes) || (!max_time.is_zero() && started.elapsed() >= max_time) {
                let _ = file.flush().await;
                return Ok(SegEnd::Full);
            }
            wait = Duration::from_millis(200);
        }
        if pl.end_list {
            let _ = file.flush().await;
            return Ok(SegEnd::Ended("直播已结束".into()));
        }
        if last_new.elapsed() > Duration::from_secs(60) {
            return Ok(SegEnd::Ended("60 秒没有新分片".into()));
        }
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return Ok(SegEnd::Stopped),
        }
    }
}

/// 录完处理：无损转为 MP4（修正时间戳），可选合并同一场的分段。返回最终文件列表。
async fn post_process(state: &AppState, files: &[String], merge: bool) -> AppResult<Vec<String>> {
    let ffmpeg = crate::postprocess::find_ffmpeg(state).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let mut mp4s = vec![];
    for f in files {
        let src = Path::new(f);
        let out = src.with_extension("mp4");
        let args: Vec<String> = vec![
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-y".into(),
            "-fflags".into(),
            "+genpts+igndts".into(),
            "-i".into(),
            f.clone(),
            "-map".into(),
            "0".into(),
            "-c".into(),
            "copy".into(),
            "-movflags".into(),
            "+faststart".into(),
            out.to_string_lossy().into_owned(),
        ];
        match crate::postprocess::run_ffmpeg(&ffmpeg, &args).await {
            Ok(()) => {
                let _ = std::fs::remove_file(src);
                mp4s.push(out.to_string_lossy().into_owned());
            }
            Err(e) => {
                log::warn!("live convert failed for {f}: {e}");
                let _ = std::fs::remove_file(&out);
                mp4s.push(f.clone());
            }
        }
    }
    if !merge || mp4s.len() < 2 || mp4s.iter().any(|f| !f.ends_with(".mp4")) {
        return Ok(mp4s);
    }
    let first = PathBuf::from(&mp4s[0]);
    let list = first.with_extension("concat.txt");
    let body: String = mp4s.iter().map(|f| format!("file '{}'\n", f.replace('\'', "'\\''"))).collect();
    std::fs::write(&list, body)?;
    let merged = first.with_file_name(first.file_name().map(|n| n.to_string_lossy().replace("_01.mp4", "_合并.mp4")).unwrap_or_else(|| "merged.mp4".into()));
    let args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        list.to_string_lossy().into_owned(),
        "-c".into(),
        "copy".into(),
        "-movflags".into(),
        "+faststart".into(),
        merged.to_string_lossy().into_owned(),
    ];
    let r = crate::postprocess::run_ffmpeg(&ffmpeg, &args).await;
    let _ = std::fs::remove_file(&list);
    // 合并结果明显偏小说明中途出错（例如某段损坏），保留分段文件
    let parts: u64 = mp4s.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    let merged_size = std::fs::metadata(&merged).map(|m| m.len()).unwrap_or(0);
    let r = r.and_then(|_| {
        if merged_size * 10 >= parts * 9 {
            Ok(())
        } else {
            Err(AppError::msg(format!("合并结果不完整（{merged_size} / {parts} 字节）")))
        }
    });
    match r {
        Ok(()) => {
            for f in &mp4s {
                let _ = std::fs::remove_file(f);
            }
            Ok(vec![merged.to_string_lossy().into_owned()])
        }
        Err(e) => {
            log::warn!("live merge failed: {e}");
            let _ = std::fs::remove_file(&merged);
            Ok(mp4s)
        }
    }
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    crate::security::system_notification(app, title, body, false);
    crate::notify::emit(app, crate::notify::Event::Live, title, body);
}

// ---------- 监控 ----------

/// 后台监控：按每个直播间的间隔检测开播，开播且开启自动录制时开始录制。
pub fn spawn_monitor(app: &AppHandle) {
    let _ = st(app).db.rec_mark_interrupted();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        let mut last_cleanup = 0i64;
        loop {
            let state = st(&app);
            // 按设置清理旧录像（每 6 小时检查一次）
            let days = state.settings().live_cleanup_days;
            if days > 0 && now() - last_cleanup > 6 * 3600 {
                last_cleanup = now();
                if let Ok(files) = state.db.rec_take_old(now() - days as i64 * 86400) {
                    for f in &files {
                        let _ = std::fs::remove_file(f);
                    }
                    if !files.is_empty() {
                        log::info!("removed {} old live recordings", files.len());
                    }
                }
            }
            let rooms = state.db.live_rooms().unwrap_or_default();
            let t = now();
            for room in rooms.into_iter().filter(|r| r.monitoring) {
                if !schedule_allows(&room.settings) {
                    // 不在预约时段：不检测；时段结束时停止自动开始的录制
                    let stop = state.live.with(room.id, |r| {
                        if r.stop.is_none() {
                            r.state = "scheduled".into();
                        }
                        (r.auto_started && r.stop.is_some()).then(|| r.stop.clone()).flatten()
                    });
                    if let Some(tx) = stop {
                        log::info!("live room {}: schedule window ended, stopping the recording", room.id);
                        let _ = tx.send(true);
                    }
                    continue;
                }
                let (busy, due) = state.live.with(room.id, |r| (r.stop.is_some() || r.state == "checking", r.next_check <= t));
                if busy || !due {
                    continue;
                }
                let interval = room.settings.check_interval_s.max(lp::min_interval(&room.platform));
                state.live.with(room.id, |r| {
                    r.state = "checking".into();
                    r.next_check = t + interval as i64;
                });
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move { monitor_once(&app2, room).await });
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn monitor_once(app: &AppHandle, room: LiveRoom) {
    let state = st(app);
    let was_live = state.live.with(room.id, |r| r.qualities.len()) > 0 && room.state == "live";
    let res = check_status(app, &room.url).await;
    let max = state.settings().live_max_recordings.max(1);
    match res {
        Ok(s) => {
            let _ = state.db.live_update_info(room.id, &s);
            state.live.with(room.id, |r| {
                r.last_check = Some(now());
                r.error = None;
                r.qualities = qualities(&s);
                r.state = if s.live { "live".into() } else { "offline".into() };
                if !s.live {
                    r.manual_stop = false;
                }
            });
            let manual_stop = state.live.with(room.id, |r| r.manual_stop);
            if s.live && room.settings.auto_record && !manual_stop {
                if state.live.recording_count() < max {
                    start_recording(app, room.id, Some(s));
                    state.live.with(room.id, |r| r.auto_started = true);
                } else {
                    state.live.with(room.id, |r| r.error = Some(format!("同时录制数已达上限（{max}）")));
                }
            } else if s.live && !was_live && room.settings.notify {
                notify(app, "开播提醒", &format!("{} 开播了：{}", room.streamer, s.title));
            }
        }
        Err(e) => state.live.with(room.id, |r| {
            r.last_check = Some(now());
            r.state = "error".into();
            r.error = Some(e.message);
        }),
    }
    emit(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_windows() {
        let w = |days: &[u8], s: &str, e: &str| TimeWindow { days: days.to_vec(), start: s.into(), end: e.into() };
        assert!(in_schedule(&[], 3, 0), "no schedule: always allowed");
        let evening = [w(&[6, 7], "20:00", "23:00")];
        assert!(in_schedule(&evening, 6, 20 * 60) && in_schedule(&evening, 7, 22 * 60 + 59));
        assert!(!in_schedule(&evening, 6, 23 * 60), "end is exclusive");
        assert!(!in_schedule(&evening, 5, 21 * 60), "wrong weekday");
        assert!(!in_schedule(&evening, 6, 19 * 60 + 59));
        // 每天（days 为空）
        assert!(in_schedule(&[w(&[], "08:00", "09:00")], 2, 8 * 60 + 30));
        // 跨午夜：周五 22:00 – 周六 02:00
        let night = [w(&[5], "22:00", "02:00")];
        assert!(in_schedule(&night, 5, 23 * 60), "Friday late");
        assert!(in_schedule(&night, 6, 60), "Saturday early morning belongs to Friday's window");
        assert!(!in_schedule(&night, 6, 3 * 60));
        assert!(!in_schedule(&night, 5, 60), "Friday early morning is Thursday's window");
        let sun_night = [w(&[7], "23:00", "01:00")];
        assert!(in_schedule(&sun_night, 1, 30), "Monday 00:30 follows Sunday's window");
        // 多个时段、非法时段
        assert!(in_schedule(&[w(&[1], "08:00", "09:00"), w(&[2], "10:00", "11:00")], 2, 10 * 60 + 5));
        assert!(!in_schedule(&[w(&[], "bad", "09:00")], 1, 8 * 60));
        assert_eq!(parse_hhmm("7:05"), Some(425));
        assert_eq!(parse_hhmm("24:00"), None);
        let mut s = LiveSettings { schedule: vec![w(&[9, 3, 3, 0], " 20:00 ", "21:00"), w(&[], "x", "y"), w(&[], "10:00", "10:00")], ..Default::default() };
        s.normalize("bilibili");
        assert_eq!(s.schedule, vec![w(&[3], "20:00", "21:00")], "invalid and empty windows are dropped, days cleaned");
    }

    #[test]
    fn segment_names() {
        let t = chrono::Local::now();
        let p = segment_path(Path::new("/r"), t, "今晚 / 打团", 3, "flv");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with("_今晚 _ 打团_03.flv"), "{name}");
        assert!(segment_path(Path::new("/r"), t, "", 1, "ts").to_string_lossy().ends_with("_直播_01.ts"));
    }

    #[test]
    fn flv_cutter_keeps_whole_tags() {
        // FLV 头（9 字节）+ PreviousTagSize0，然后两个标签：数据 5 字节和 3 字节
        let mut stream = b"FLV\x01\x05\x00\x00\x00\x09\x00\x00\x00\x00".to_vec();
        let tag = |n: u8, size: usize| {
            let mut t = vec![9u8, 0, 0, size as u8, 0, 0, 0, 0, 0, 0, 0];
            t.extend(std::iter::repeat_n(n, size));
            t.extend([0, 0, 0, (11 + size) as u8]);
            t
        };
        let t1 = tag(1, 5);
        let t2 = tag(2, 3);
        stream.extend(&t1);
        stream.extend(&t2);
        let mut c = FlvCutter::default();
        let mut out = vec![];
        // 按 7 字节一块喂入，最后一个标签只给一半
        let cut = stream.len() - 6;
        for chunk in stream[..cut].chunks(7) {
            out.extend(c.feed(chunk));
        }
        assert_eq!(out.len(), 13 + t1.len(), "only complete tags are written");
        out.extend(c.feed(&stream[cut..]));
        assert_eq!(out, stream);
        let mut raw = FlvCutter::default();
        assert_eq!(raw.feed(b"not an flv file at all"), b"not an flv file at all".to_vec());
    }

    #[test]
    fn settings_normalize() {
        let mut s = LiveSettings { check_interval_s: 5, convert_mp4: false, merge_segments: true, ..Default::default() };
        s.normalize("douyin");
        assert_eq!(s.check_interval_s, 60);
        assert!(!s.merge_segments);
    }

    #[test]
    fn db_rooms_and_recordings() {
        let db = Db::open_in_memory().unwrap();
        let s = LiveStatus {
            platform: "bilibili".into(),
            platform_name: "B站".into(),
            room_id: "1".into(),
            streamer: "主播".into(),
            title: "t".into(),
            cover: None,
            avatar: None,
            live: false,
            streams: vec![],
        };
        let id = db.live_insert("https://live.bilibili.com/1", &s, &LiveSettings::default()).unwrap();
        assert!(db.live_insert("https://live.bilibili.com/1", &s, &LiveSettings::default()).is_err());
        db.live_set_monitoring(id, false).unwrap();
        assert!(!db.live_room(id).unwrap().unwrap().monitoring);
        let r = db.rec_start(id, "主播", "t").unwrap();
        db.rec_update(r, &["a.flv".into()], 10, "recording", false).unwrap();
        db.rec_mark_interrupted().unwrap();
        let recs = db.recordings(Some(id)).unwrap();
        assert_eq!(recs[0].status, "interrupted");
        assert_eq!(recs[0].files, vec!["a.flv"]);
        assert!(db.rec_take_old(0).unwrap().is_empty());
        assert_eq!(db.rec_take_old(now() + 10).unwrap(), vec!["a.flv"]);
        assert!(db.recordings(Some(id)).unwrap().is_empty());
    }
}
