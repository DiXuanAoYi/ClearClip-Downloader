//! 前端可调用的命令。涉及数据库、文件和网络的命令都是异步命令，不占用界面线程。

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;
use tauri_plugin_opener::OpenerExt;
use url::Url;

use crate::cookies::{self, AccountSummary};
use crate::db::{HistoryItem, LibraryFilter, LibraryItem};
use crate::download::{self, EnqueueResult, OrphanPart, PostOptions, TaskSnapshot};
use crate::model::{AppError, AppResult, MediaInfo, MediaKind, PlaylistEntry, SeriesInfo};
use crate::providers::{self, DetectedLink};
use crate::settings::Settings;
use crate::{clipboard, diagnostics, live, phone, quality, subs, tools, tray, AppState};

const REPO: &str = "DiXuanAoYi/ClearClip-Downloader";

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    id: &'static str,
    name: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    providers: Vec<ProviderInfo>,
    repo: &'static str,
    os: &'static str,
    /// Cookie 加密密钥是否存放在系统钥匙串
    key_in_keyring: bool,
    shortcut_error: Option<String>,
    /// 构建所对应的提交（短哈希）
    commit: &'static str,
    /// 调试版（desktop-debug 流水线或本机 debug 构建）
    debug: bool,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: St<'_>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        providers: providers::all().iter().map(|p| ProviderInfo { id: p.id(), name: p.name() }).collect(),
        repo: REPO,
        os: std::env::consts::OS,
        key_in_keyring: state.cookies.key_in_keyring,
        shortcut_error: state.shortcut_error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        commit: crate::COMMIT,
        debug: cfg!(debug_assertions),
    }
}

#[tauri::command]
pub fn get_settings(state: St<'_>) -> Settings {
    state.settings_raw()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsResult {
    settings: Settings,
    /// 全局快捷键注册失败的原因（设置仍会保存）
    shortcut_error: Option<String>,
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, state: St<'_>, settings: Settings) -> AppResult<SaveSettingsResult> {
    let mut s = settings;
    s.normalize();
    let old = state.settings_raw();
    // 旧版明文 Cookie 字段不再写回
    s.cookies.clear();
    s.cookie_updated_at.clear();
    // 手机发送的令牌和已配对设备只通过专门的命令修改，避免被设置页的旧数据覆盖
    s.phone = old.phone.clone();
    s.save(&state.settings_path)?;
    *state.settings.write().unwrap_or_else(|e| e.into_inner()) = s.clone();
    if old.shortcut != s.shortcut || old.security.panic_shortcut != s.security.panic_shortcut {
        apply_shortcut(&app, &s.shortcut);
    }
    if old.language != s.language {
        tray::apply_language(&app, crate::i18n::is_en(&s.language));
    }
    if old.float_ball != s.float_ball {
        tray::apply_float(&app, s.float_ball);
    }
    if old.security.content_protection != s.security.content_protection {
        crate::security::apply_window_protection(&app, s.security.content_protection);
    }
    if old.network != s.network {
        state.net.invalidate();
    }
    state.net.refresh_limit(&s.speed_schedule);
    tray::sync_watch_item(&app);
    if !s.prevent_sleep {
        crate::power::keep_awake(false);
    }
    if old.launch_at_login != s.launch_at_login {
        use tauri_plugin_autostart::ManagerExt;
        let r = if s.launch_at_login { app.autolaunch().enable() } else { app.autolaunch().disable() };
        if let Err(e) = r {
            log::warn!("autostart change failed: {e}");
        }
    }
    if old.concurrency != s.concurrency || old.per_site_concurrency != s.per_site_concurrency || old.metered_mode != s.metered_mode {
        download::schedule(&app);
    }
    let shortcut_error = state.shortcut_error.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Ok(SaveSettingsResult { settings: s, shortcut_error })
}

/// 注册全局快捷键，失败原因记录到状态里供界面显示。
pub fn apply_shortcut(app: &AppHandle, shortcut: &str) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let panic = app.try_state::<Arc<AppState>>().map(|st| st.settings_raw().security.panic_shortcut).unwrap_or_default();
    let mut errors = vec![];
    for (what, sc) in [("快捷键", shortcut.trim()), ("老板键", panic.trim())] {
        if sc.is_empty() {
            continue;
        }
        if let Err(e) = gs.register(sc) {
            log::warn!("register shortcut {sc} failed: {e}");
            errors.push(format!("{what} {sc} 注册失败，可能格式不对或已被其他软件占用：{e}"));
        }
    }
    if let Some(st) = app.try_state::<Arc<AppState>>() {
        *st.shortcut_error.lock().unwrap_or_else(|e| e.into_inner()) = if errors.is_empty() { None } else { Some(errors.join("\n")) };
    }
}

#[tauri::command]
pub fn detect_links(text: String) -> Vec<DetectedLink> {
    providers::detect_links(&text)
}

#[tauri::command]
pub async fn resolve_link(app: AppHandle, state: St<'_>, text: String) -> AppResult<MediaInfo> {
    let settings = state.settings();
    let info = match providers::resolve_text(&state.parse_ctx(&settings), &text).await {
        Ok(info) => info,
        Err(e) => {
            log::info!("resolve failed ({:?}): {e}", e.kind);
            crate::inbox::record_manual_failure(&app, &text, &e);
            return Err(e);
        }
    };
    if !settings.security.privacy_mode {
        let _ = state.db.upsert_history(&info);
    }
    let _ = state.db.inbox_mark_parsed(&text, &info);
    Ok(info)
}

/// 解析页默认勾选的字幕（按偏好语言）。
#[tauri::command]
pub fn preferred_subtitles(state: St<'_>, media: MediaInfo) -> Vec<String> {
    quality::preferred_subtitles(&media, &state.settings())
}

/// 播放列表里已经下载过的条目（条目 ID）。
#[tauri::command]
pub fn downloaded_entries(state: St<'_>, platform: String, entries: Vec<PlaylistEntry>) -> AppResult<Vec<String>> {
    let pairs: Vec<(String, String)> = entries.into_iter().map(|e| (e.id, e.url)).collect();
    state.db.downloaded_entry_ids(&platform, &pairs)
}

#[tauri::command]
pub async fn resolve_and_enqueue(app: AppHandle, text: String) -> AppResult<String> {
    clipboard::resolve_and_enqueue(&app, &text).await
}

#[tauri::command]
pub async fn enqueue(app: AppHandle, media: MediaInfo, asset_ids: Vec<String>, post: Option<PostOptions>) -> AppResult<EnqueueResult> {
    if asset_ids.is_empty() {
        return Err(AppError::invalid("请至少选择一项要下载的内容。"));
    }
    download::enqueue_with(&app, media, &asset_ids, post.unwrap_or_default())
}

pub const EVT_BATCH: &str = "batch://progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgress {
    pub title: String,
    pub done: usize,
    pub total: usize,
    pub queued: usize,
    /// 之前已下载或已在队列中而跳过的数量
    pub skipped: usize,
    pub failed: Vec<String>,
    pub finished: bool,
}

/// 把播放列表 / 合集中选中的条目逐个解析并加入队列（后台进行，进度通过事件通知）。
#[tauri::command]
pub async fn enqueue_entries(app: AppHandle, state: St<'_>, playlist: MediaInfo, entry_ids: Vec<String>) -> AppResult<usize> {
    let entries: Vec<PlaylistEntry> = playlist.entries.iter().filter(|e| entry_ids.contains(&e.id)).cloned().collect();
    if entries.is_empty() {
        return Err(AppError::invalid("请至少选择一个条目。"));
    }
    let n = entries.len();
    if state.db.inbox_playlist_chosen(&playlist.platform, &playlist.id, n).unwrap_or(false) {
        crate::inbox::emit_changed(&app);
    }
    let st = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let settings = st.settings();
        let mut p = BatchProgress { title: playlist.title.clone(), done: 0, total: n, queued: 0, skipped: 0, failed: vec![], finished: false };
        let _ = app.emit(EVT_BATCH, p.clone());
        for e in entries {
            let ctx = st.parse_ctx(&settings);
            match providers::resolve_url(&ctx, &e.url).await {
                Ok(mut info) if info.kind != MediaKind::Playlist => {
                    // 列表里的条目按“列表名 / 第 N 集”归档
                    if info.series.is_none() {
                        info.series = Some(SeriesInfo { name: playlist.title.clone(), season: None, episode: Some(e.index) });
                    }
                    let (ids, post) = quality::auto_selection(&info, &settings);
                    match download::enqueue_with(&app, info, &ids, post) {
                        Ok(r) => {
                            p.queued += r.tasks.len();
                            p.skipped += r.already_downloaded + r.already_queued;
                        }
                        Err(err) => p.failed.push(format!("{}：{err}", e.title)),
                    }
                }
                Ok(_) => p.failed.push(format!("{}：嵌套的列表请单独打开", e.title)),
                Err(err) => p.failed.push(format!("{}：{err}", e.title)),
            }
            p.done += 1;
            let _ = app.emit(EVT_BATCH, p.clone());
        }
        p.finished = true;
        let _ = app.emit(EVT_BATCH, p);
    });
    Ok(n)
}

// ---------- 手机发送 ----------

#[tauri::command]
pub fn phone_info(app: AppHandle) -> phone::PhoneInfo {
    phone::info(&app)
}

#[tauri::command]
pub async fn phone_enable(app: AppHandle, on: bool) -> AppResult<phone::PhoneInfo> {
    phone::set_enabled(&app, on).await
}

#[tauri::command]
pub fn phone_reset_token(app: AppHandle) -> AppResult<phone::PhoneInfo> {
    phone::reset_token(&app)
}

#[tauri::command]
pub fn phone_revoke(app: AppHandle, device_id: String) -> AppResult<phone::PhoneInfo> {
    phone::revoke(&app, &device_id)
}

#[tauri::command]
pub fn phone_pair_respond(app: AppHandle, device_id: String, accept: bool) -> AppResult<phone::PhoneInfo> {
    phone::respond_pair(&app, &device_id, accept)
}

#[tauri::command]
pub fn phone_set_options(app: AppHandle, options: phone::PhoneOptions) -> AppResult<phone::PhoneInfo> {
    phone::set_options(&app, options)
}

// ---------- 手机与电脑互传 ----------

#[tauri::command]
pub fn transfer_list(app: AppHandle) -> AppResult<crate::transfer::TransferList> {
    crate::transfer::list(&app)
}

#[derive(Serialize)]
pub struct SendResult {
    pub added: usize,
    pub skipped: Vec<String>,
}

/// 把电脑上的文件发给手机（只是加入“电脑发来的”列表，手机点开才会传输）。
#[tauri::command]
pub fn transfer_send_files(app: AppHandle, paths: Vec<String>) -> AppResult<SendResult> {
    let (added, skipped) = crate::transfer::send_files(&app, &paths)?;
    Ok(SendResult { added, skipped })
}

#[tauri::command]
pub fn transfer_send_text(app: AppHandle, text: String) -> AppResult<i64> {
    crate::transfer::send_text(&app, &text)
}

#[tauri::command]
pub fn transfer_remove(app: AppHandle, id: i64, delete_file: bool) -> AppResult<()> {
    crate::transfer::remove(&app, id, delete_file)
}

#[tauri::command]
pub fn transfer_clear(app: AppHandle, direction: String) -> AppResult<()> {
    crate::transfer::clear(&app, &direction)
}

// ---------- 图集合成视频 ----------

/// 下载图集图片（和背景音乐），合成为一个视频，保存在下载目录并记入媒体库。返回文件路径。
#[tauri::command]
pub async fn make_slideshow(state: St<'_>, media: MediaInfo, image_ids: Vec<String>, music_id: Option<String>, seconds: f64) -> AppResult<String> {
    let settings = state.settings();
    let secs = seconds.clamp(0.5, 30.0);
    let images: Vec<&crate::model::Asset> = media.assets.iter().filter(|a| image_ids.contains(&a.id) && a.kind == crate::model::AssetKind::Image).collect();
    if images.is_empty() {
        return Err(AppError::invalid("请至少选择一张图片。"));
    }
    let dir = crate::naming::target_dir(&settings.download_root(), &media, settings.subfolder_by_platform);
    std::fs::create_dir_all(&dir)?;
    let base = crate::naming::render_base(&settings.filename_template, &media);
    let work = dir.join(format!(".clearclip-slides-{}", crate::db::now()));
    std::fs::create_dir_all(&work)?;
    let fetch = |url: String, headers: Vec<(String, String)>, path: std::path::PathBuf| {
        let state = state.inner().clone();
        let settings = settings.clone();
        let referer = providers::referer_for(&media.platform).to_string();
        async move {
            let client = state.net.clients_for(&settings.network, &url)?.download;
            let mut req = client.get(&url);
            if !referer.is_empty() && !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("referer")) {
                req = req.header("Referer", referer);
            }
            if let Some(c) = state.cookies.header_for(&url) {
                req = req.header("Cookie", c);
            }
            for (k, v) in headers {
                req = req.header(k, v);
            }
            let resp = req.send().await?;
            if !resp.status().is_success() {
                return Err(AppError::from_status(resp.status().as_u16(), "图片"));
            }
            std::fs::write(&path, resp.bytes().await?)?;
            Ok::<_, AppError>(path)
        }
    };
    let result = async {
        let mut files = vec![];
        for (i, a) in images.iter().enumerate() {
            let p = fetch(a.url.clone(), a.headers.clone(), work.join(format!("{i:03}.{}", a.ext))).await?;
            files.push((p, a.width.unwrap_or(1080), a.height.unwrap_or(1920)));
        }
        let music = match music_id.as_deref().and_then(|id| media.asset(id)) {
            Some(a) => Some(fetch(a.url.clone(), a.headers.clone(), work.join(format!("music.{}", a.ext))).await?),
            None => None,
        };
        let out = crate::naming::unique_path(dir.join(format!("{base}_图集视频.mp4")), &|p: &std::path::Path| p.exists());
        crate::postprocess::slideshow(&state, &files, music.as_deref(), secs, &out).await?;
        Ok::<_, AppError>(out)
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    let out = result?;
    let path = out.to_string_lossy().into_owned();
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    let _ = state.db.record_download(&crate::db::NewDownload {
        platform: &media.platform,
        media_id: &media.id,
        asset_id: "slideshow",
        title: &media.title,
        author: &media.author,
        cover: media.cover.as_deref(),
        path: &path,
        size: size as i64,
        kind: "video",
        source: "manual",
        source_url: &media.source_url,
        platform_name: &media.platform_name,
    });
    Ok(path)
}

// ---------- 直播录制 ----------

#[tauri::command]
pub fn live_rooms(app: AppHandle) -> AppResult<Vec<live::LiveRoom>> {
    live::rooms(&app)
}

#[tauri::command]
pub async fn live_check(app: AppHandle, url: String) -> AppResult<providers::live::LiveStatus> {
    let url = providers::extract_urls(&url).into_iter().next().ok_or_else(|| AppError::invalid("没有找到链接。"))?;
    live::check_status(&app, &url).await
}

#[tauri::command]
pub async fn live_add(app: AppHandle, url: String, settings: live::LiveSettings) -> AppResult<live::LiveRoom> {
    let url = providers::extract_urls(&url).into_iter().next().ok_or_else(|| AppError::invalid("没有找到链接。"))?;
    live::add(&app, &url, settings).await
}

#[tauri::command]
pub fn live_update(app: AppHandle, state: St<'_>, id: i64, streamer: String, settings: live::LiveSettings) -> AppResult<()> {
    let room = state.db.live_room(id)?.ok_or_else(|| AppError::not_found("直播间不存在"))?;
    let mut settings = settings;
    settings.normalize(&room.platform);
    state.db.live_update_settings(id, streamer.trim(), &settings)?;
    let _ = app.emit(live::EVT_LIVE, ());
    Ok(())
}

#[tauri::command]
pub fn live_set_monitoring(app: AppHandle, id: i64, on: bool) -> AppResult<()> {
    live::set_monitoring(&app, id, on)
}

#[tauri::command]
pub fn live_delete(app: AppHandle, id: i64) -> AppResult<()> {
    live::delete(&app, id)
}

#[tauri::command]
pub fn live_start(app: AppHandle, id: i64) {
    live::start_recording(&app, id, None)
}

#[tauri::command]
pub fn live_stop(app: AppHandle, id: i64) {
    live::stop_recording(&app, id)
}

#[tauri::command]
pub fn live_recordings(state: St<'_>, id: Option<i64>) -> AppResult<Vec<live::Recording>> {
    state.db.recordings(id)
}

// ---------- 订阅 ----------

#[tauri::command]
pub fn subs_list(app: AppHandle) -> AppResult<Vec<subs::Subscription>> {
    subs::list(&app)
}

#[tauri::command]
pub async fn subs_preview(app: AppHandle, url: String) -> AppResult<providers::listing::ListResult> {
    let url = providers::extract_urls(&url).into_iter().next().ok_or_else(|| AppError::invalid("没有找到链接。"))?;
    subs::preview(&app, &url).await
}

#[tauri::command]
pub async fn subs_add(app: AppHandle, url: String, title: Option<String>, settings: subs::SubSettings) -> AppResult<subs::Subscription> {
    let url = providers::extract_urls(&url).into_iter().next().ok_or_else(|| AppError::invalid("没有找到链接。"))?;
    subs::add(&app, &url, title, settings).await
}

#[tauri::command]
pub fn subs_update(app: AppHandle, state: St<'_>, id: i64, title: String, settings: subs::SubSettings) -> AppResult<()> {
    let mut settings = settings;
    settings.normalize();
    state.db.sub_update(id, title.trim(), &settings)?;
    let _ = app.emit(subs::EVT_SUBS, ());
    Ok(())
}

#[tauri::command]
pub fn subs_set_paused(app: AppHandle, state: St<'_>, id: i64, paused: bool) -> AppResult<()> {
    state.db.sub_set_status(id, if paused { "paused" } else { "active" })?;
    let _ = app.emit(subs::EVT_SUBS, ());
    Ok(())
}

#[tauri::command]
pub fn subs_delete(app: AppHandle, state: St<'_>, id: i64) -> AppResult<()> {
    state.db.sub_delete(id)?;
    let _ = app.emit(subs::EVT_SUBS, ());
    Ok(())
}

#[tauri::command]
pub async fn subs_check(app: AppHandle, id: i64) -> AppResult<usize> {
    subs::check(&app, id).await
}

#[tauri::command]
pub fn subs_items(state: St<'_>, id: i64, statuses: Vec<String>) -> AppResult<Vec<subs::SubItem>> {
    let refs: Vec<&str> = statuses.iter().map(String::as_str).collect();
    state.db.sub_items(id, &refs)
}

#[tauri::command]
pub async fn subs_download_items(app: AppHandle, id: i64, item_ids: Vec<String>) -> AppResult<usize> {
    subs::download_items(&app, id, &item_ids).await
}

#[tauri::command]
pub fn subs_ignore_items(app: AppHandle, id: i64, item_ids: Vec<String>) -> AppResult<()> {
    subs::ignore_items(&app, id, &item_ids)
}

#[tauri::command]
pub fn subs_clear_new(state: St<'_>, id: i64) -> AppResult<()> {
    state.db.sub_clear_new(id)
}

// ---------- 组件（yt-dlp / ffmpeg） ----------

fn tool_of(id: &str) -> AppResult<tools::Tool> {
    tools::Tool::parse(id).ok_or_else(|| AppError::invalid("未知组件"))
}

#[tauri::command]
pub async fn tools_status(state: St<'_>) -> AppResult<Vec<tools::ToolStatus>> {
    let mut out = vec![];
    for t in tools::Tool::all() {
        out.push(tools::status(&state, t).await);
    }
    Ok(out)
}

#[tauri::command]
pub async fn install_tool(app: AppHandle, tool: String) -> AppResult<tools::ToolStatus> {
    tools::install(&app, tool_of(&tool)?).await
}

#[tauri::command]
pub async fn rollback_tool(state: St<'_>, tool: String) -> AppResult<tools::ToolStatus> {
    tools::rollback(&state, tool_of(&tool)?).await
}

#[tauri::command]
pub async fn import_tool(state: St<'_>, tool: String, path: String) -> AppResult<tools::ToolStatus> {
    tools::import(&state, tool_of(&tool)?, std::path::Path::new(&path)).await
}

#[tauri::command]
pub async fn list_extractors(state: St<'_>) -> AppResult<Vec<String>> {
    tools::list_extractors(&state).await
}

#[tauri::command]
pub async fn open_tools_dir(app: AppHandle, state: St<'_>) -> AppResult<()> {
    std::fs::create_dir_all(&state.tools_dir)?;
    app.opener().open_path(state.tools_dir.to_string_lossy(), None::<&str>).map_err(|e| AppError::msg(e.to_string()))
}

#[tauri::command]
pub fn list_tasks(state: St<'_>) -> Vec<TaskSnapshot> {
    state.downloads.snapshots()
}

#[tauri::command]
pub fn task_detail(app: AppHandle, id: i64) -> AppResult<download::TaskDetail> {
    download::task_detail(&app, id).ok_or_else(|| AppError::not_found("任务不存在或已被移除"))
}

#[tauri::command]
pub fn pause_task(app: AppHandle, id: i64) {
    download::pause(&app, id)
}

#[tauri::command]
pub fn resume_task(app: AppHandle, id: i64) {
    download::resume(&app, id)
}

#[tauri::command]
pub fn cancel_task(app: AppHandle, id: i64) {
    download::cancel(&app, id)
}

#[tauri::command]
pub async fn remove_task(app: AppHandle, id: i64) -> AppResult<()> {
    download::remove(&app, id);
    Ok(())
}

#[tauri::command]
pub async fn clear_finished(app: AppHandle) -> AppResult<()> {
    download::clear_finished(&app);
    Ok(())
}

#[tauri::command]
pub fn pause_all(app: AppHandle) {
    download::pause_all(&app)
}

#[tauri::command]
pub fn resume_all(app: AppHandle) {
    download::resume_all(&app)
}

#[tauri::command]
pub async fn list_orphan_parts(app: AppHandle) -> AppResult<Vec<OrphanPart>> {
    Ok(download::orphan_parts(&app))
}

#[tauri::command]
pub async fn delete_orphan_parts(app: AppHandle, paths: Vec<String>) -> AppResult<usize> {
    Ok(download::delete_orphans(&app, &paths))
}

#[tauri::command]
pub async fn list_history(state: St<'_>, query: Option<String>) -> AppResult<Vec<HistoryItem>> {
    state.db.list_history(query.as_deref().unwrap_or(""), 500)
}

#[tauri::command]
pub async fn delete_history(state: St<'_>, id: i64) -> AppResult<()> {
    state.db.delete_history(id)
}

#[tauri::command]
pub async fn clear_history(state: St<'_>) -> AppResult<()> {
    state.db.clear_history()
}

#[tauri::command]
pub async fn list_library(state: St<'_>, filter: Option<LibraryFilter>) -> AppResult<Vec<LibraryItem>> {
    state.db.search_library(&filter.unwrap_or_default(), 5000)
}

#[tauri::command]
pub async fn library_platforms(state: St<'_>) -> AppResult<Vec<crate::db::PlatformCount>> {
    state.db.library_platforms()
}

/// 重新下载媒体库里的一项（文件被移动或删除时）：重新解析原链接，下载同一个资源。
#[tauri::command]
pub async fn redownload(app: AppHandle, state: St<'_>, id: i64) -> AppResult<EnqueueResult> {
    let item = state.db.library_item(id)?.ok_or_else(|| AppError::not_found("记录不存在"))?;
    if item.source_url.is_empty() {
        return Err(AppError::invalid("这条记录没有保存原链接，无法重新下载。请重新粘贴链接解析。"));
    }
    if item.exists {
        return Err(AppError::invalid("文件还在原位置，不需要重新下载。"));
    }
    let settings = state.settings();
    let info = providers::resolve_url(&state.parse_ctx(&settings), &item.source_url).await?;
    let (ids, post) = match info.asset(&item.asset_id) {
        Some(a) => (vec![a.id.clone()], PostOptions::default()),
        None => quality::auto_selection(&info, &settings),
    };
    download::enqueue_with(&app, info, &ids, post)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResult {
    platform: String,
    name: String,
    sample: Option<String>,
    ok: bool,
    millis: u64,
    message: String,
    kind: Option<crate::error::ErrorKind>,
}

/// 内置示例链接（长期稳定的公开内容）；其他平台用最近一次成功解析的链接。
const HEALTH_SAMPLES: &[(&str, &str)] =
    &[("bilibili", "https://www.bilibili.com/video/BV1GJ411x7h7"), ("yt-dlp", "https://www.youtube.com/watch?v=jNQXAC9IVRw")];

/// 平台健康检查：用示例链接测试各平台解析是否正常。
/// 试用一条自定义站点规则：用它解析给定网址，返回解析结果（不下载）。
#[tauri::command]
pub async fn site_rule_test(state: St<'_>, rule: crate::settings::SiteRule, url: String) -> AppResult<MediaInfo> {
    let settings = state.settings();
    let ctx = state.parse_ctx(&settings);
    let mut rule = rule;
    rule.enabled = true;
    if rule.video_regex.trim().is_empty() {
        return Err(AppError::invalid("请先填写“视频地址”的正则。"));
    }
    providers::custom::resolve(&ctx, &rule, url.trim()).await
}

#[tauri::command]
pub async fn health_check(state: St<'_>) -> AppResult<Vec<HealthResult>> {
    let settings = state.settings();
    let mut targets: Vec<(String, String)> = providers::all().iter().map(|p| (p.id().to_string(), p.name().to_string())).collect();
    targets.push(("yt-dlp".into(), "yt-dlp（其他网站）".into()));
    let mut out = vec![];
    for (id, name) in targets {
        let sample = state.db.latest_source_url(&id)?.or_else(|| HEALTH_SAMPLES.iter().find(|(p, _)| *p == id).map(|(_, u)| u.to_string()));
        let Some(url) = sample.clone() else {
            out.push(HealthResult {
                platform: id,
                name,
                sample: None,
                ok: false,
                millis: 0,
                message: "没有示例链接：成功解析过一次该平台的链接后即可检测".into(),
                kind: None,
            });
            continue;
        };
        let ctx = state.parse_ctx(&settings);
        if id == "yt-dlp" && ctx.ytdlp.is_none() {
            out.push(HealthResult {
                platform: id,
                name,
                sample,
                ok: false,
                millis: 0,
                message: "未安装 yt-dlp".into(),
                kind: Some(crate::error::ErrorKind::NeedUpdate),
            });
            continue;
        }
        let t = std::time::Instant::now();
        let r = if id == "yt-dlp" {
            tokio::time::timeout(std::time::Duration::from_secs(60), providers::ytdlp::resolve(&ctx, &url)).await
        } else {
            tokio::time::timeout(std::time::Duration::from_secs(30), providers::resolve_url(&ctx, &url)).await
        };
        let millis = t.elapsed().as_millis() as u64;
        out.push(match r {
            Ok(Ok(info)) => HealthResult {
                platform: id,
                name,
                sample,
                ok: true,
                millis,
                message: format!("正常：{}（{} 个资源）", info.title, info.assets.len().max(info.entries.len())),
                kind: None,
            },
            Ok(Err(e)) => HealthResult { platform: id, name, sample, ok: false, millis, message: e.message.clone(), kind: Some(e.kind) },
            Err(_) => HealthResult { platform: id, name, sample, ok: false, millis, message: "超时".into(), kind: Some(crate::error::ErrorKind::Network) },
        });
    }
    Ok(out)
}

// ---------- 队列排序、定时、完成后动作 ----------

#[tauri::command]
pub fn move_task(app: AppHandle, id: i64, to: String) {
    download::move_task(&app, id, &to)
}

#[tauri::command]
pub fn schedule_task(app: AppHandle, id: i64, start_at: Option<i64>) {
    download::schedule_task(&app, id, start_at)
}

#[tauri::command]
pub fn get_after_all_done(state: St<'_>) -> String {
    state.after_all_done.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[tauri::command]
pub fn set_after_all_done(state: St<'_>, action: String) -> AppResult<()> {
    if !matches!(action.as_str(), "none" | "sleep" | "shutdown") {
        return Err(AppError::invalid("未知操作"));
    }
    *state.after_all_done.lock().unwrap_or_else(|e| e.into_inner()) = action;
    Ok(())
}

/// 读取拖入的文本文件（只读前 1 MB），用于批量导入链接。
#[tauri::command]
pub async fn read_links_file(path: String) -> AppResult<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(&path)?;
    let mut buf = vec![];
    f.by_ref().take(1024 * 1024).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if providers::extract_urls(&text).is_empty() {
        return Err(AppError::invalid("文件里没有找到链接。"));
    }
    Ok(text)
}

#[tauri::command]
pub fn is_portable() -> bool {
    crate::portable_dir().is_some()
}

#[tauri::command]
pub async fn delete_library(state: St<'_>, id: i64, delete_file: bool) -> AppResult<()> {
    let rep = crate::library_cmds::delete_items(&state, &[id], delete_file);
    match rep.failed.into_iter().next() {
        Some(msg) => Err(AppError::new(crate::error::ErrorKind::Disk, msg)),
        None => Ok(()),
    }
}

/// 写入剪贴板，并让监听忽略这次变化。
#[tauri::command]
pub fn copy_text(app: AppHandle, state: St<'_>, text: String) -> AppResult<()> {
    *state.clipboard_ignore.lock().unwrap_or_else(|e| e.into_inner()) = Some(text.clone());
    app.clipboard().write_text(text).map_err(|e| AppError::msg(format!("复制失败：{e}")))
}

#[tauri::command]
pub fn open_file(app: AppHandle, path: String) -> AppResult<()> {
    app.opener().open_path(path, None::<&str>).map_err(|e| AppError::msg(format!("无法打开文件：{e}")))
}

#[tauri::command]
pub fn reveal_file(app: AppHandle, path: String) -> AppResult<()> {
    let p = std::path::PathBuf::from(&path);
    if p.is_dir() {
        return app.opener().open_path(path, None::<&str>).map_err(|e| AppError::msg(format!("无法打开文件夹：{e}")));
    }
    app.opener().reveal_item_in_dir(p).map_err(|e| AppError::msg(format!("无法打开所在文件夹：{e}")))
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> AppResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(AppError::invalid("只能打开网页链接。"));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| AppError::msg(format!("无法打开链接：{e}")))
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    tray::show_main(&app)
}

#[tauri::command]
pub fn hide_mini(app: AppHandle) {
    tray::close_mini(&app);
}

/// 迷你窗页面加载好、事件监听都挂上之后调用，取回创建它时对应的链接和已经出来的自动下载结果。
#[tauri::command]
pub fn mini_ready() -> crate::clipboard::MiniState {
    crate::clipboard::take_mini_state()
}

// ---------- 账号与 Cookie ----------

/// 登录地址：内置平台用平台的登录页；其他网站用传入的地址（必须是 https）。
fn login_target(site: &str, url: Option<&str>) -> AppResult<(String, Url)> {
    if let Some(p) = providers::by_id(site) {
        return Ok((p.name().to_string(), p.login_url().parse().expect("valid url")));
    }
    let raw = url.map(String::from).unwrap_or_else(|| format!("https://www.{site}/"));
    let u = Url::parse(&raw).map_err(|_| AppError::invalid("登录地址格式不正确。"))?;
    if u.scheme() != "https" {
        return Err(AppError::invalid("登录地址必须是 https 网页。"));
    }
    Ok((site.to_string(), u))
}

fn login_label(site: &str) -> String {
    format!("login-{}", site.replace(|c: char| !c.is_ascii_alphanumeric(), "-"))
}

/// 打开登录窗口。窗口内是网站官网，没有任何调用本程序的权限。
/// 已知登录 Cookie 的网站会自动识别登录完成：保存账号、关闭窗口并通知前端。
#[tauri::command]
pub fn open_login(app: AppHandle, site: String, url: Option<String>, label: Option<String>) -> AppResult<()> {
    let label_win = login_label(&site);
    if let Some(w) = app.get_webview_window(&label_win) {
        let _ = w.set_focus();
        return Ok(());
    }
    let (name, target) = login_target(&site, url.as_deref())?;
    let auto = cookies::login_cookie_names(&site).is_some();
    let hint = if auto { "登录完成后会自动保存并关闭窗口" } else { "登录完成后回到清影点击“保存登录状态”" };
    let win =
        WebviewWindowBuilder::new(&app, label_win, WebviewUrl::External(target)).title(format!("登录{name}：{hint}")).inner_size(1100.0, 780.0).build()?;
    if app.state::<Arc<AppState>>().settings_raw().security.content_protection {
        let _ = win.set_content_protected(true);
    }
    if auto {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { watch_login(app, site, label).await });
    }
    Ok(())
}

pub const EVT_LOGIN_SAVED: &str = "login://auto-saved";

/// 登录窗口里属于该网站的 Cookie。
fn window_cookies(window: &tauri::WebviewWindow, site: &str) -> Vec<cookies::StoredCookie> {
    let Ok((_, target)) = login_target(site, None).or_else(|_| window.url().map(|u| (site.to_string(), u)).map_err(AppError::from)) else { return vec![] };
    let domain = target.host_str().map(cookies::registrable_domain).unwrap_or_default();
    let mut list: Vec<cookies::StoredCookie> =
        window.cookies().unwrap_or_default().iter().filter_map(cookies::from_webview).filter(|c| cookies::registrable_domain(&c.domain) == domain).collect();
    if list.is_empty() {
        list = window.cookies_for_url(target).unwrap_or_default().iter().filter_map(cookies::from_webview).collect();
    }
    list
}

fn login_cookie_values(list: &[cookies::StoredCookie], names: &[&str]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> =
        list.iter().filter(|c| names.contains(&c.name.as_str()) && !c.value.is_empty()).map(|c| (c.name.clone(), c.value.clone())).collect();
    v.sort();
    v
}

/// 等待登录 Cookie 出现（或变化），然后自动保存。最多等 20 分钟；窗口关闭即停止。
async fn watch_login(app: AppHandle, site: String, label: Option<String>) {
    let Some(names) = cookies::login_cookie_names(&site) else { return };
    let win = login_label(&site);
    // 打开时已有的登录 Cookie（之前登录过）不算，换账号时要等新的登录
    tokio::time::sleep(Duration::from_secs(2)).await;
    let initial = match app.get_webview_window(&win) {
        Some(w) => login_cookie_values(&window_cookies(&w, &site), names),
        None => return,
    };
    for _ in 0..800 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let Some(w) = app.get_webview_window(&win) else { return };
        let now = login_cookie_values(&window_cookies(&w, &site), names);
        if now.is_empty() || now == initial {
            continue;
        }
        // 等网站把其余 Cookie 写完
        tokio::time::sleep(Duration::from_secs(2)).await;
        let state = app.state::<Arc<AppState>>().inner().clone();
        match save_window_login(&app, &state, &site, label.as_deref()) {
            Ok(_) => {
                let _ = app.emit(EVT_LOGIN_SAVED, &site);
                let _ = app.emit("accounts://updated", ());
                // 顺便检查一次登录状态（拿到用户名）
                if let Some(id) =
                    state.cookies.summaries().into_iter().find(|a| a.site == site && a.label == label.clone().unwrap_or_else(|| "默认".into())).map(|a| a.id)
                {
                    if verify_account(&state, &id).await.is_ok() {
                        let _ = app.emit("accounts://updated", ());
                    }
                }
            }
            Err(e) => log::warn!("auto-save login for {site} failed: {e}"),
        }
        return;
    }
}

/// 读取登录窗口里属于该网站的全部 Cookie，保存为一个账号，关闭登录窗口，并重试需要登录的链接。
fn save_window_login(app: &AppHandle, state: &AppState, site: &str, label: Option<&str>) -> AppResult<()> {
    let window = app.get_webview_window(&login_label(site)).ok_or_else(|| AppError::invalid("登录窗口已关闭。请先点击“打开登录窗口”并完成登录。"))?;
    let list = window_cookies(&window, site);
    if list.is_empty() {
        return Err(AppError::need_login("没有读取到 Cookie，请确认已在登录窗口中完成登录。"));
    }
    state.cookies.upsert(site, label.unwrap_or("默认"), list)?;
    let _ = window.close();
    log::info!("saved login cookies for {site}");
    crate::inbox::retry_after_login(app, site, false);
    Ok(())
}

#[tauri::command]
pub async fn save_login_cookies(app: AppHandle, state: St<'_>, site: String, label: Option<String>) -> AppResult<Vec<AccountSummary>> {
    save_window_login(&app, &state, &site, label.as_deref())?;
    Ok(state.cookies.summaries())
}

#[tauri::command]
pub fn list_accounts(state: St<'_>) -> Vec<AccountSummary> {
    state.cookies.summaries()
}

/// 导入 Netscape 格式的 cookies.txt 文件。
#[tauri::command]
pub async fn import_cookies_file(app: AppHandle, state: St<'_>, path: String, label: Option<String>) -> AppResult<Vec<AccountSummary>> {
    let meta = std::fs::metadata(&path)?;
    if meta.len() > 5 * 1024 * 1024 {
        return Err(AppError::invalid("文件过大，不像是 cookies.txt。"));
    }
    let text = std::fs::read_to_string(&path)?;
    let before: Vec<String> = state.cookies.summaries().into_iter().map(|a| a.id).collect();
    let (sites, n) = state.cookies.import_netscape(&text, label.as_deref().unwrap_or("导入"))?;
    log::info!("imported {n} cookies for {sites} sites");
    retry_changed_sites(&app, &state, &before);
    Ok(state.cookies.summaries())
}

/// 导入粘贴的内容：Netscape 格式，或 `a=1; b=2` 形式的 Cookie 头（需要指定网站）。
#[tauri::command]
pub async fn import_cookies_text(app: AppHandle, state: St<'_>, site: String, text: String, label: Option<String>) -> AppResult<Vec<AccountSummary>> {
    let label = label.unwrap_or_else(|| "手动填写".into());
    let before: Vec<String> = state.cookies.summaries().into_iter().map(|a| a.id).collect();
    if text.contains('\t') {
        state.cookies.import_netscape(&text, &label)?;
        retry_changed_sites(&app, &state, &before);
    } else {
        let site = site.trim().to_ascii_lowercase();
        if site.is_empty() {
            return Err(AppError::invalid("请选择或填写这些 Cookie 属于哪个网站。"));
        }
        let (_, target) = login_target(&site, None)?;
        let domain = target.host_str().map(cookies::registrable_domain).unwrap_or(site.clone());
        state.cookies.upsert(&site, &label, cookies::parse_header(&text, &domain))?;
        crate::inbox::retry_after_login(&app, &site, false);
    }
    Ok(state.cookies.summaries())
}

/// 导入 cookies.txt 后：对更新过的网站重试需要登录的链接。
fn retry_changed_sites(app: &AppHandle, state: &AppState, before: &[String]) {
    let now = crate::db::now();
    let mut sites: Vec<String> = state.cookies.summaries().into_iter().filter(|a| !before.contains(&a.id) || now - a.updated_at < 60).map(|a| a.site).collect();
    sites.dedup();
    for s in sites {
        crate::inbox::retry_after_login(app, &s, false);
    }
}

// ---------- 收到的链接 ----------

/// 一段文本里的链接需要登录时对应的网站（内置平台 ID 或域名）。
#[tauri::command]
pub fn login_site(text: String) -> Option<String> {
    crate::inbox::primary_url(&text).map(|u| crate::inbox::site_for(&u)).filter(|s| !s.is_empty())
}

#[tauri::command]
pub fn inbox_list(app: AppHandle, state: St<'_>, filter: Option<crate::inbox::InboxFilter>) -> AppResult<Vec<crate::inbox::InboxItem>> {
    crate::inbox::sync_tasks(&app);
    state.db.inbox_list(&filter.unwrap_or_default())
}

#[tauri::command]
pub fn inbox_counts(state: St<'_>) -> AppResult<crate::inbox::InboxCounts> {
    state.db.inbox_counts()
}

#[tauri::command]
pub fn inbox_retry(app: AppHandle, ids: Vec<i64>) -> AppResult<usize> {
    let mut n = 0;
    let mut last_err = None;
    for id in ids {
        match crate::inbox::retry(&app, id) {
            Ok(()) => n += 1,
            Err(e) => last_err = Some(e),
        }
    }
    match (n, last_err) {
        (0, Some(e)) => Err(e),
        _ => Ok(n),
    }
}

/// 重试全部失败的记录。
#[tauri::command]
pub fn inbox_retry_failed(app: AppHandle, state: St<'_>) -> AppResult<usize> {
    let ids: Vec<i64> = state.db.inbox_with_status("('failed')")?.into_iter().map(|i| i.id).collect();
    inbox_retry(app, ids)
}

#[tauri::command]
pub fn inbox_ignore(app: AppHandle, ids: Vec<i64>) -> AppResult<()> {
    crate::inbox::ignore(&app, &ids)
}

#[tauri::command]
pub fn inbox_delete(app: AppHandle, state: St<'_>, ids: Vec<i64>) -> AppResult<()> {
    state.db.inbox_delete(&ids)?;
    crate::inbox::emit_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn inbox_clear(app: AppHandle, state: St<'_>, scope: String) -> AppResult<usize> {
    let n = state.db.inbox_clear(&scope)?;
    crate::inbox::emit_changed(&app);
    Ok(n)
}

#[tauri::command]
pub async fn rename_account(state: St<'_>, id: String, label: String) -> AppResult<Vec<AccountSummary>> {
    state.cookies.rename(&id, &label)?;
    Ok(state.cookies.summaries())
}

#[tauri::command]
pub async fn set_default_account(state: St<'_>, id: String) -> AppResult<Vec<AccountSummary>> {
    state.cookies.set_default(&id)?;
    Ok(state.cookies.summaries())
}

#[tauri::command]
pub async fn delete_account(state: St<'_>, id: String) -> AppResult<Vec<AccountSummary>> {
    state.cookies.delete(&id)?;
    Ok(state.cookies.summaries())
}

/// 检查账号是否仍处于登录状态（目前支持 B站、微博），并记录用户名。
#[tauri::command]
pub async fn check_account(app: AppHandle, state: St<'_>, id: String) -> AppResult<providers::AccountStatus> {
    let st = state.inner().clone();
    let r = verify_account(&st, &id).await;
    let _ = app.emit("accounts://updated", ());
    r
}

/// 检查账号的登录状态并记录结果：支持的平台联网验证，其他已知网站检查登录 Cookie 是否存在且未过期。
pub async fn verify_account(state: &AppState, id: &str) -> AppResult<providers::AccountStatus> {
    let site = state.cookies.account_site(id).ok_or_else(|| AppError::not_found("账号不存在。"))?;
    let online = providers::by_id(&site).filter(|p| cookies::ONLINE_CHECK.contains(&p.id()));
    let status = match online {
        Some(provider) => {
            let settings = state.settings();
            let mut ctx = state.parse_ctx(&settings);
            ctx.account = Some(id.to_string());
            provider.account_status(&ctx).await?.ok_or_else(|| AppError::unsupported(format!("暂不支持检查{}的登录状态。", provider.name())))?
        }
        None => {
            let ok = state.cookies.has_login_cookie(id).ok_or_else(|| AppError::unsupported("暂不支持检查这个网站的登录状态。"))?;
            providers::AccountStatus { logged_in: ok, user_name: None, vip: None }
        }
    };
    let name = match (&status.user_name, &status.vip) {
        (Some(n), Some(v)) => Some(format!("{n}（{v}）")),
        (n, _) => n.clone(),
    };
    state.cookies.set_check_result(id, status.logged_in, if status.logged_in { name } else { None })?;
    Ok(status)
}

/// 测试某个地址在当前网络规则下能否访问。
/// 对所有出口（直连、系统代理、自定义代理）测速。
#[tauri::command]
pub async fn route_speedtest(state: St<'_>, url: String) -> AppResult<Vec<crate::net::RouteSpeed>> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::invalid("请填写以 https:// 开头的地址，最好是一个视频文件或大一点的资源。"));
    }
    let net = state.settings_raw().network;
    Ok(state.net.speed_test(&net, &url).await)
}

#[tauri::command]
pub async fn test_route(state: St<'_>, url: String) -> AppResult<crate::net::RouteTest> {
    let url = if url.starts_with("http") { url } else { format!("https://{url}/") };
    state.net.test(&state.settings().network, &url).await
}

// ---------- 诊断 ----------

/// 生成诊断信息（已隐去 Cookie 和令牌），用户可以复制后反馈问题。
#[tauri::command]
pub async fn get_diagnostics(app: AppHandle, state: St<'_>) -> AppResult<String> {
    let s = state.settings();
    let tasks = state.downloads.snapshots();
    let count = |st: download::TaskStatus| tasks.iter().filter(|t| t.status == st).count();
    let accounts: Vec<String> = state.cookies.summaries().iter().map(|a| format!("{}（{} 个 Cookie）", a.site, a.cookie_count)).collect();
    let mut out = String::new();
    out.push_str(&format!("清影 ClearClip {}\n", app.package_info().version));
    out.push_str(&format!("系统：{} {}\n", std::env::consts::OS, std::env::consts::ARCH));
    out.push_str(&format!("时间：{}\n\n", chrono::Utc::now().to_rfc3339()));
    out.push_str(&format!(
        "设置：解析模式 {:?}，远程 API {}，并发 {}，重试 {} 次，自动继续 {}，剪贴板监听 {}，录制样本 {}\n",
        s.parse_mode,
        if s.remote_endpoint.is_empty() { "未设置" } else { "已设置" },
        s.concurrency,
        s.max_retries,
        s.auto_resume,
        s.watch_clipboard,
        s.record_samples
    ));
    out.push_str(&format!("Cookie 密钥位置：{}\n", if state.cookies.key_in_keyring { "系统钥匙串" } else { "本地密钥文件" }));
    out.push_str(&format!("已保存账号：{}\n", if accounts.is_empty() { "无".to_string() } else { accounts.join("，") }));
    out.push_str(&format!(
        "任务：等待 {}，下载中 {}，暂停 {}，失败 {}，完成 {}\n",
        count(download::TaskStatus::Queued),
        count(download::TaskStatus::Running),
        count(download::TaskStatus::Paused),
        count(download::TaskStatus::Failed),
        count(download::TaskStatus::Done)
    ));
    for t in tasks.iter().filter(|t| t.status == download::TaskStatus::Failed).take(10) {
        out.push_str(&format!("  失败：[{}] {:?} {}\n", t.platform, t.error_kind, t.error.as_deref().unwrap_or("")));
    }
    out.push_str("\n最近日志：\n");
    out.push_str(&diagnostics::log_tail(&state.log_dir, 200));
    Ok(diagnostics::redact(&out))
}

#[tauri::command]
pub fn open_log_dir(app: AppHandle, state: St<'_>) -> AppResult<()> {
    let _ = std::fs::create_dir_all(&state.log_dir);
    app.opener().open_path(state.log_dir.to_string_lossy().into_owned(), None::<&str>).map_err(|e| AppError::msg(format!("无法打开日志目录：{e}")))
}

#[tauri::command]
pub fn open_samples_dir(app: AppHandle, state: St<'_>) -> AppResult<()> {
    let _ = std::fs::create_dir_all(&state.samples_dir);
    app.opener().open_path(state.samples_dir.to_string_lossy().into_owned(), None::<&str>).map_err(|e| AppError::msg(format!("无法打开样本目录：{e}")))
}

// ---------- 更新 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current: String,
    latest: Option<String>,
    has_update: bool,
    url: String,
    /// 可在程序内下载安装（签名更新已配置）
    can_install: bool,
}

/// 签名更新的公钥在构建时传入（CLEARCLIP_UPDATER_PUBKEY），没有时只提示前往发布页下载。
pub const UPDATER_PUBKEY: Option<&str> = option_env!("CLEARCLIP_UPDATER_PUBKEY");

fn updater_endpoints(state: &AppState) -> Vec<Url> {
    let latest = format!("https://github.com/{REPO}/releases/latest/download/latest.json");
    crate::tools::candidates(&latest, &state.settings().component_mirrors).into_iter().filter_map(|u| Url::parse(&u).ok()).collect()
}

async fn signed_update(app: &AppHandle, state: &AppState) -> AppResult<Option<tauri_plugin_updater::Update>> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater_builder()
        .endpoints(updater_endpoints(state))
        .map_err(|e| AppError::msg(e.to_string()))?
        .build()
        .map_err(|e| AppError::msg(e.to_string()))?;
    updater.check().await.map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("检查更新失败：{e}")))
}

/// 下载并安装签名更新，完成后重启。
#[tauri::command]
pub async fn install_update(app: AppHandle, state: St<'_>) -> AppResult<()> {
    if UPDATER_PUBKEY.filter(|k| !k.is_empty()).is_none() {
        return Err(AppError::invalid("这个版本不支持程序内更新，请前往发布页下载。"));
    }
    let update = signed_update(&app, &state).await?.ok_or_else(|| AppError::invalid("已是最新版本"))?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| AppError::msg(format!("安装更新失败：{e}")))?;
    app.restart();
}

/// 通过 GitHub Releases 检查新版本（不自动安装，只提示并打开下载页）。
#[tauri::command]
pub async fn check_update(app: AppHandle, state: St<'_>) -> AppResult<UpdateInfo> {
    let current = app.package_info().version.to_string();
    let releases_page = format!("https://github.com/{REPO}/releases");
    if UPDATER_PUBKEY.is_some_and(|k| !k.is_empty()) {
        match signed_update(&app, &state).await {
            Ok(Some(u)) => return Ok(UpdateInfo { current, latest: Some(u.version.clone()), has_update: true, url: releases_page, can_install: true }),
            Ok(None) => return Ok(UpdateInfo { latest: Some(current.clone()), current, has_update: false, url: releases_page, can_install: true }),
            Err(e) => log::info!("signed update check failed, falling back to GitHub API: {e}"),
        }
    }
    let resp = state
        .client
        .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .header("User-Agent", "ClearClip")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if resp.status().as_u16() == 404 {
        return Ok(UpdateInfo { current, latest: None, has_update: false, url: releases_page, can_install: false });
    }
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), "GitHub "));
    }
    let data: serde_json::Value = resp.json().await?;
    let tag = data.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
    let tag = tag.trim_start_matches("desktop-").trim_start_matches(['v', 'V']).to_string();
    let url = data.get("html_url").and_then(|v| v.as_str()).unwrap_or(&releases_page).to_string();
    let has_update = is_newer(&tag, &current);
    Ok(UpdateInfo { current, latest: Some(tag).filter(|t| !t.is_empty()), has_update, url, can_install: false })
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    let (a, b) = (parse(latest), parse(current));
    !latest.is_empty() && a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn login_targets() {
        assert_eq!(login_target("bilibili", None).unwrap().1.as_str(), "https://www.bilibili.com/");
        assert_eq!(login_target("youtube.com", None).unwrap().1.as_str(), "https://www.youtube.com/");
        assert!(login_target("x.com", Some("http://x.com/")).is_err());
        assert_eq!(login_label("youtube.com"), "login-youtube-com");
    }
}
