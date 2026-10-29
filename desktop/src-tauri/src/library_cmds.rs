//! 媒体库的前端命令：标签 / 收藏 / 评分、导入、统计、回收站、整理、重复文件、字幕搜索、预览图、导出与备份。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::LibraryItem;
use crate::error::{AppError, AppResult, ErrorKind};
use crate::library::{self, CueHit, DupGroup, ImportReport, MovePlan, MoveReport, Stats, TagCount, TrashItem};
use crate::model::Chapter;
use crate::{backup, library_media, postprocess, AppState};

type St<'a> = State<'a, Arc<AppState>>;

pub const EVT_PROGRESS: &str = "library://progress";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    task: &'static str,
    done: usize,
    total: usize,
}

fn progress(app: &AppHandle, task: &'static str, done: usize, total: usize) {
    let _ = app.emit(EVT_PROGRESS, Progress { task, done, total });
}

fn ffmpeg(st: &AppState) -> AppResult<PathBuf> {
    postprocess::find_ffmpeg(st).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”中安装 ffmpeg 后重试。"))
}

// ---------- 收藏、评分、备注、标签 ----------

#[tauri::command]
pub async fn library_set_meta(state: St<'_>, id: i64, favorite: Option<bool>, rating: Option<i64>, note: Option<String>) -> AppResult<()> {
    state.db.set_item_meta(id, favorite, rating, note.as_deref())
}

#[tauri::command]
pub async fn library_set_tags(state: St<'_>, id: i64, tags: Vec<String>) -> AppResult<()> {
    state.db.set_item_tags(id, &tags)
}

#[tauri::command]
pub async fn library_bulk_tags(state: St<'_>, ids: Vec<i64>, tags: Vec<String>, remove: bool) -> AppResult<()> {
    state.db.bulk_tags(&ids, &tags, remove)
}

#[tauri::command]
pub async fn library_bulk_favorite(state: St<'_>, ids: Vec<i64>, favorite: bool) -> AppResult<()> {
    for id in ids {
        state.db.set_item_meta(id, Some(favorite), None, None)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn library_tags(state: St<'_>) -> AppResult<Vec<TagCount>> {
    state.db.all_tags()
}

// ---------- 删除与回收站 ----------

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeleteReport {
    pub removed: usize,
    pub trashed: usize,
    pub failed: Vec<String>,
}

/// 删除媒体库记录。`delete_files` 时同时处理磁盘上的文件：开启回收站则放进回收站，否则直接删除。
pub fn delete_items(st: &AppState, ids: &[i64], delete_files: bool) -> DeleteReport {
    let settings = st.settings();
    let root = settings.download_root();
    let mut rep = DeleteReport::default();
    for id in ids {
        let Ok(Some(item)) = st.db.library_item(*id) else { continue };
        if delete_files && settings.library.use_trash && item.exists {
            match library::move_to_trash(&st.db, &root, &item) {
                Ok(Some(_)) => {
                    rep.removed += 1;
                    rep.trashed += 1;
                }
                Ok(None) => rep.removed += 1,
                Err(e) => rep.failed.push(format!("{}：{e}", item.title)),
            }
            continue;
        }
        let _ = st.db.delete_library(*id);
        rep.removed += 1;
        if delete_files {
            match std::fs::remove_file(&item.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => rep.failed.push(format!("{}：记录已删除，但文件删除失败（{e}）", item.title)),
            }
        }
    }
    rep
}

#[tauri::command]
pub async fn library_delete(state: St<'_>, ids: Vec<i64>, delete_files: bool) -> AppResult<DeleteReport> {
    Ok(delete_items(&state, &ids, delete_files))
}

#[tauri::command]
pub async fn library_remove_missing(state: St<'_>) -> AppResult<usize> {
    let ids = state.db.missing_ids()?;
    for id in &ids {
        state.db.delete_library(*id)?;
    }
    Ok(ids.len())
}

#[tauri::command]
pub async fn trash_list(state: St<'_>) -> AppResult<Vec<TrashItem>> {
    state.db.trash_list()
}

#[tauri::command]
pub async fn trash_restore(state: St<'_>, id: i64) -> AppResult<String> {
    library::restore_from_trash(&state.db, id)
}

#[tauri::command]
pub async fn trash_purge(state: St<'_>, id: i64) -> AppResult<()> {
    library::purge_trash(&state.db, id)
}

#[tauri::command]
pub async fn trash_empty(state: St<'_>) -> AppResult<usize> {
    let items = state.db.trash_list()?;
    for t in &items {
        library::purge_trash(&state.db, t.id)?;
    }
    Ok(items.len())
}

/// 启动时清理超过保留期限的回收站内容。
pub fn purge_old_trash(st: &AppState) {
    let days = st.settings().library.trash_keep_days;
    if days == 0 {
        return;
    }
    if let Ok(ids) = st.db.trash_older_than(days as i64 * 86400) {
        for id in &ids {
            let _ = library::purge_trash(&st.db, *id);
        }
        if !ids.is_empty() {
            log::info!("purged {} old trash items", ids.len());
        }
    }
}

// ---------- 导入、统计 ----------

#[tauri::command]
pub async fn library_import(app: AppHandle, state: St<'_>, dir: String, recursive: bool) -> AppResult<ImportReport> {
    let st2 = state.inner().clone();
    let (rep, new_media) =
        tokio::task::spawn_blocking(move || library::import_folder(&st2.db, Path::new(&dir), recursive)).await.map_err(|e| AppError::msg(e.to_string()))??;
    let st3 = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        index_unindexed(&st3);
        enrich_items(&app, &st3, &new_media).await;
        let _ = app.emit("library://changed", ());
    });
    Ok(rep)
}

/// 给新登记的视频 / 音频记录时长，并给视频截一张图当封面。
async fn enrich_items(app: &AppHandle, st: &Arc<AppState>, ids: &[i64]) {
    let Ok(ff) = ffmpeg(st) else { return };
    let covers = st.data_dir.join("covers");
    for (n, id) in ids.iter().enumerate() {
        progress(app, "import", n, ids.len());
        let Ok(Some(item)) = st.db.library_item(*id) else { continue };
        enrich_one(st, &ff, &covers, &item).await;
    }
    progress(app, "import", ids.len(), ids.len());
}

async fn enrich_one(st: &AppState, ff: &Path, covers: &Path, item: &LibraryItem) {
    if !item.exists || !matches!(item.kind.as_str(), "video" | "audio") {
        return;
    }
    let file = Path::new(&item.path);
    let dur = match item.duration_ms {
        Some(d) => Some(d),
        None => {
            let d = library_media::duration_ms(ff, file).await;
            if let Some(d) = d {
                let _ = st.db.set_duration(item.id, d);
            }
            d
        }
    };
    if item.kind == "video" && item.cover_path.is_none() && item.cover.is_none() {
        let out = covers.join(format!("lib-{}.jpg", item.id));
        let at = dur.map(|d| (d as u64) / 10).unwrap_or(1000);
        if library_media::extract_frame(ff, file, at, &out, 360).await.is_ok() {
            let _ = st.db.set_cover_path(&item.platform, &item.media_id, &out.to_string_lossy());
        }
    }
}

/// 下载完成后的收尾：记录时长、没有封面时截图、字幕建立搜索索引。
pub async fn after_download(app: AppHandle, platform: String, media_id: String, asset_id: String) {
    let st = app.state::<Arc<AppState>>().inner().clone();
    let Ok(Some(id)) = st.db.id_by_key(&platform, &media_id, &asset_id) else { return };
    let Ok(Some(item)) = st.db.library_item(id) else { return };
    if item.kind == "subtitle" {
        let _ = library::index_subtitle_item(&st.db, &item);
        crate::ai::auto_translate(&app, &st, &item);
        return;
    }
    if item.kind == "video" {
        crate::ai::auto_transcribe(&app, &st, &item);
    }
    // 自动规则可能会移动文件，所以放在上传之前，上传时重新读取记录
    crate::rules::run_for(&app, &st, &item);
    let item = st.db.library_item(item.id).ok().flatten().unwrap_or(item);
    crate::upload::auto_upload(&app, &st, &item);
    if !st.settings().library.probe_new_files {
        return;
    }
    if let Some(ff) = postprocess::find_ffmpeg(&st) {
        enrich_one(&st, &ff, &st.data_dir.join("covers"), &item).await;
    }
}

fn index_unindexed(st: &AppState) -> usize {
    let mut n = 0;
    if let Ok(items) = st.db.unindexed_subtitles() {
        for it in items.iter().filter(|i| Path::new(&i.path).exists()) {
            n += library::index_subtitle_item(&st.db, it).unwrap_or(0);
        }
    }
    n
}

/// 启动时的后台维护：清理回收站、补全字幕索引。
pub fn spawn_maintenance(app: &AppHandle) {
    let st = app.state::<Arc<AppState>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let st2 = st.clone();
        let _ = tokio::task::spawn_blocking(move || {
            purge_old_trash(&st2);
            let n = index_unindexed(&st2);
            if n > 0 {
                log::info!("indexed {n} subtitle cues");
            }
        })
        .await;
    });
}

#[tauri::command]
pub async fn library_stats(state: St<'_>) -> AppResult<Stats> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || st.db.library_stats()).await.map_err(|e| AppError::msg(e.to_string()))?
}

// ---------- 按规则整理目录 ----------

#[tauri::command]
pub async fn reorganize_plan(state: St<'_>, template: Option<String>) -> AppResult<Vec<MovePlan>> {
    let settings = state.settings();
    let template = template.filter(|t| !t.trim().is_empty()).unwrap_or(settings.library.reorganize_template.clone());
    let st = state.inner().clone();
    let root = settings.download_root();
    tokio::task::spawn_blocking(move || library::plan_reorganize(&st.db, &root, &template)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub async fn reorganize_apply(state: St<'_>, plans: Vec<MovePlan>) -> AppResult<MoveReport> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || library::apply_reorganize(&st.db, &plans)).await.map_err(|e| AppError::msg(e.to_string()))?
}

// ---------- 重复文件 ----------

#[tauri::command]
pub async fn duplicates_exact(state: St<'_>) -> AppResult<Vec<DupGroup>> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || library::exact_duplicates(&st.db)).await.map_err(|e| AppError::msg(e.to_string()))?
}

/// 画面相似的视频：给还没有指纹的视频计算指纹（每个视频取 3 帧），再比较。
#[tauri::command]
pub async fn duplicates_similar(app: AppHandle, state: St<'_>) -> AppResult<Vec<DupGroup>> {
    let ff = ffmpeg(&state)?;
    let videos: Vec<LibraryItem> = state
        .db
        .search_library(&crate::db::LibraryFilter { kind: Some("video".into()), ..Default::default() }, 100_000)?
        .into_iter()
        .filter(|i| i.exists)
        .collect();
    let total = videos.len();
    let mut hashed: Vec<(LibraryItem, Vec<u64>)> = vec![];
    for (n, v) in videos.into_iter().enumerate() {
        progress(&app, "similar", n, total);
        let hash = match state.db.phash_of(v.id)? {
            Some(h) => Some(h),
            None => {
                let dur = match v.duration_ms {
                    Some(d) => Some(d),
                    None => library_media::duration_ms(&ff, Path::new(&v.path)).await,
                };
                if let Some(d) = dur {
                    let _ = state.db.set_duration(v.id, d);
                }
                match dur {
                    Some(d) if d > 0 => library_media::compute_phash(&ff, Path::new(&v.path), d as u64).await,
                    _ => None,
                }
                .inspect(|h| {
                    let _ = state.db.set_phash(v.id, h);
                })
            }
        };
        let mut item = v;
        item.duration_ms = state.db.library_item(item.id)?.and_then(|i| i.duration_ms);
        if let Some(h) = hash {
            hashed.push((item, library::parse_phash(&h)));
        }
    }
    progress(&app, "similar", total, total);
    Ok(library::group_similar(hashed))
}

// ---------- 字幕搜索 ----------

#[tauri::command]
pub async fn cues_search(state: St<'_>, query: String) -> AppResult<Vec<CueHit>> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || st.db.search_cues(&query, 200, 5)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub async fn cues_reindex(state: St<'_>) -> AppResult<usize> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || library::reindex_all(&st.db)).await.map_err(|e| AppError::msg(e.to_string()))?
}

// ---------- 预览图与镜头 ----------

/// 生成（或取缓存的）预览图，返回图片路径。
#[tauri::command]
pub async fn library_preview(state: St<'_>, id: i64) -> AppResult<String> {
    let item = state.db.library_item(id)?.ok_or_else(|| AppError::not_found("记录不存在"))?;
    if item.kind != "video" || !item.exists {
        return Err(AppError::invalid("只有存在的视频文件才能生成预览图。"));
    }
    let out = library_media::sheet_path(&state.data_dir, id);
    if out.exists() && std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(out.to_string_lossy().into_owned());
    }
    let ff = ffmpeg(&state)?;
    let dur = match item.duration_ms {
        Some(d) => d,
        None => library_media::duration_ms(&ff, Path::new(&item.path)).await.ok_or_else(|| AppError::msg("读不出视频时长。"))?,
    };
    let _ = state.db.set_duration(id, dur);
    library_media::contact_sheet(&ff, Path::new(&item.path), dur.max(1) as u64, &out, 4, 3).await?;
    Ok(out.to_string_lossy().into_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneList {
    pub duration_ms: u64,
    pub scenes: Vec<Chapter>,
}

#[tauri::command]
pub async fn library_scenes(state: St<'_>, id: i64, threshold: Option<f64>) -> AppResult<SceneList> {
    let item = state.db.library_item(id)?.ok_or_else(|| AppError::not_found("记录不存在"))?;
    if item.kind != "video" || !item.exists {
        return Err(AppError::invalid("只有存在的视频文件才能检测镜头。"));
    }
    let ff = ffmpeg(&state)?;
    let file = PathBuf::from(&item.path);
    let dur = library_media::duration_ms(&ff, &file).await.ok_or_else(|| AppError::msg("读不出视频时长。"))? as u64;
    let cuts = library_media::detect_scenes(&ff, &file, threshold.unwrap_or(0.4).clamp(0.1, 0.9)).await?;
    Ok(SceneList { duration_ms: dur, scenes: library_media::scenes_to_chapters(&cuts, dur, 2_000) })
}

/// 按镜头拆分成多个文件（不重新编码，起点落在关键帧上），放在视频旁边的“<文件名> 镜头”文件夹里。
#[tauri::command]
pub async fn library_split_scenes(state: St<'_>, id: i64, scenes: Vec<Chapter>) -> AppResult<String> {
    let item = state.db.library_item(id)?.ok_or_else(|| AppError::not_found("记录不存在"))?;
    if !item.exists || scenes.len() < 2 {
        return Err(AppError::invalid("没有可以拆分的镜头。"));
    }
    let ff = ffmpeg(&state)?;
    let file = PathBuf::from(&item.path);
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = file.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "mp4".into());
    let dir = file.with_file_name(format!("{stem} 镜头"));
    let n = postprocess::split_chapters(&ff, &file, &dir, &ext, &scenes).await?;
    if n == 0 {
        return Err(AppError::msg("拆分失败。"));
    }
    Ok(dir.to_string_lossy().into_owned())
}

// ---------- 导出与备份 ----------

fn export_ids(st: &AppState, ids: &Option<Vec<i64>>) -> AppResult<Vec<LibraryItem>> {
    match ids {
        Some(ids) if !ids.is_empty() => st.db.library_items(ids),
        _ => st.db.search_library(&crate::db::LibraryFilter::default(), 200_000),
    }
}

/// 导出媒体库清单。`format`：csv / json。
#[tauri::command]
pub async fn library_export(state: St<'_>, format: String, dest: String, ids: Option<Vec<i64>>) -> AppResult<usize> {
    let items = export_ids(&state, &ids)?;
    let text = match format.as_str() {
        "json" => serde_json::to_string_pretty(&items)?,
        _ => library::library_csv(&items),
    };
    if let Some(dir) = Path::new(&dest).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&dest, text)?;
    Ok(items.len())
}

#[tauri::command]
pub async fn backup_export(app: AppHandle, state: St<'_>, dest: String) -> AppResult<()> {
    let st = state.inner().clone();
    let version = app.package_info().version.to_string();
    tokio::task::spawn_blocking(move || backup::export(&st.db, &st.settings(), &version, Path::new(&dest))).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub async fn backup_import(state: St<'_>, src: String) -> AppResult<backup::BackupInfo> {
    let data_dir = state.data_dir.clone();
    tokio::task::spawn_blocking(move || backup::stage_import(Path::new(&src), &data_dir)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}
