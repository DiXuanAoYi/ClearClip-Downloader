//! 安全与隐私的前端命令：应用锁、加密保险箱、一键清除。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult};
use crate::media_tools::spawn_job;
use crate::safebox::{Kdf, SafeEntry, SafeStatus};
use crate::security::{self, LockStatus, WipeOptions, WipeReport};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// 应用锁定时，不允许操作保险箱和清除数据（界面本来就被遮住了，这里是第二道门）。
fn require_unlocked(st: &AppState) -> AppResult<()> {
    if security::status(st).locked {
        return Err(AppError::invalid("应用已锁定，请先解锁。"));
    }
    Ok(())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::msg(e.to_string()))?
}

// ---------- 应用锁 ----------

#[tauri::command]
pub fn lock_status(state: St<'_>) -> LockStatus {
    security::status(&state)
}

#[tauri::command]
pub async fn lock_set_password(state: St<'_>, current: Option<String>, new_password: String) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || security::set_password(&st, current.as_deref(), &new_password, Kdf::STRONG)).await
}

#[tauri::command]
pub async fn lock_remove_password(app: AppHandle, state: St<'_>, current: Option<String>) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || security::remove_password(&app, &st, current.as_deref())).await
}

#[tauri::command]
pub fn lock_now(app: AppHandle) -> bool {
    security::lock_now(&app)
}

#[tauri::command]
pub async fn lock_unlock(app: AppHandle, state: St<'_>, password: String) -> AppResult<()> {
    let st = state.inner().clone();
    blocking(move || security::unlock(&app, &st, &password)).await
}

// ---------- 一键清除 ----------

#[tauri::command]
pub async fn wipe_traces(app: AppHandle, state: St<'_>, options: WipeOptions) -> AppResult<WipeReport> {
    require_unlocked(&state)?;
    Ok(security::wipe(&app, &options))
}

// ---------- 加密保险箱 ----------

#[tauri::command]
pub fn safebox_status(state: St<'_>) -> SafeStatus {
    state.safebox.status()
}

#[tauri::command]
pub async fn safebox_create(app: AppHandle, state: St<'_>, password: String) -> AppResult<()> {
    require_unlocked(&state)?;
    let st = state.inner().clone();
    blocking(move || st.safebox.create(&password, Kdf::STRONG)).await?;
    let _ = app.emit(security::EVT_SAFEBOX, ());
    Ok(())
}

#[tauri::command]
pub async fn safebox_unlock(app: AppHandle, state: St<'_>, password: String) -> AppResult<()> {
    require_unlocked(&state)?;
    let st = state.inner().clone();
    blocking(move || st.safebox.unlock(&password)).await?;
    let _ = app.emit(security::EVT_SAFEBOX, ());
    Ok(())
}

#[tauri::command]
pub fn safebox_lock(app: AppHandle, state: St<'_>) {
    security::seal_safebox(&state);
    let _ = app.emit(security::EVT_SAFEBOX, ());
}

#[tauri::command]
pub async fn safebox_change_password(state: St<'_>, current: String, new_password: String) -> AppResult<()> {
    require_unlocked(&state)?;
    let st = state.inner().clone();
    blocking(move || st.safebox.change_password(&current, &new_password, Kdf::STRONG)).await
}

#[tauri::command]
pub fn safebox_list(state: St<'_>) -> AppResult<Vec<SafeEntry>> {
    require_unlocked(&state)?;
    state.safebox.list()
}

/// 把电脑上的文件加密放进保险箱。`delete_original` 为真时，加密成功后覆盖并删除原文件。
#[tauri::command]
pub fn safebox_add_files(app: AppHandle, state: St<'_>, paths: Vec<String>, delete_original: bool) -> AppResult<Vec<u64>> {
    require_unlocked(&state)?;
    if !state.safebox.is_unlocked() {
        return Err(AppError::invalid("保险箱已锁定，请先输入密码解锁。"));
    }
    let mut ids = vec![];
    for p in paths {
        let path = PathBuf::from(&p);
        if !path.is_file() {
            return Err(AppError::invalid(format!("不是文件：{p}")));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        ids.push(spawn_add(&app, path, name, "file", delete_original, None));
    }
    Ok(ids)
}

/// 把媒体库里的作品移进保险箱：加密成功后，从媒体库里移除记录，并覆盖删除原文件和封面缓存。
#[tauri::command]
pub fn safebox_add_library(app: AppHandle, state: St<'_>, ids: Vec<i64>) -> AppResult<Vec<u64>> {
    require_unlocked(&state)?;
    if !state.safebox.is_unlocked() {
        return Err(AppError::invalid("保险箱已锁定，请先输入密码解锁。"));
    }
    let mut jobs = vec![];
    for id in ids {
        let Some(item) = state.db.library_item(id)? else { continue };
        let path = PathBuf::from(&item.path);
        if !path.is_file() {
            return Err(AppError::not_found(format!("文件已不存在：{}", item.title)));
        }
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let name = if ext.is_empty() { crate::naming::sanitize(&item.title) } else { format!("{}.{ext}", crate::naming::sanitize(&item.title)) };
        jobs.push(spawn_add(&app, path, name, "library", true, Some(id)));
    }
    Ok(jobs)
}

fn spawn_add(app: &AppHandle, src: PathBuf, name: String, from: &'static str, delete_original: bool, item_id: Option<i64>) -> u64 {
    spawn_job(app, name.clone(), "加密入库", move |ctx| async move {
        let c = ctx.clone();
        let (src2, name2) = (src.clone(), name.clone());
        tokio::task::spawn_blocking(move || {
            c.st.safebox.add_file(&src2, &name2, from, &mut |done, total| {
                c.percent(done as f64 / total.max(1) as f64 * 100.0);
                !c.canceled()
            })
        })
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
        if delete_original {
            ctx.note("正在清除原文件…");
            if let Some(id) = item_id {
                if let Ok(Some(item)) = ctx.st.db.library_item(id) {
                    if let Some(cover) = item.cover_path {
                        crate::safebox::shred_file(Path::new(&cover));
                    }
                }
                crate::library_cmds::delete_items(&ctx.st, &[id], false);
                use tauri::Emitter;
                let _ = ctx.app.emit("library://changed", ());
            }
            let s = src.clone();
            let _ = tokio::task::spawn_blocking(move || crate::safebox::shred_file(&s)).await;
        }
        ctx.note("");
        let _ = ctx.app.emit(security::EVT_SAFEBOX, ());
        Ok(PathBuf::new())
    })
}

/// 解密到临时文件夹并用系统默认程序打开。上锁、退出或下次启动时这些临时文件会被覆盖删除。
#[tauri::command]
pub fn safebox_open(app: AppHandle, state: St<'_>, id: String) -> AppResult<u64> {
    require_unlocked(&state)?;
    let entry = state.safebox.entry(&id)?;
    let dir = security::view_dir(&state).join(hex::encode(crate::safebox::random_bytes::<6>()));
    let dst = dir.join(crate::naming::sanitize(&entry.name));
    Ok(spawn_job(&app, entry.name.clone(), "解密查看", move |ctx| async move {
        let (c, d) = (ctx.clone(), dst.clone());
        tokio::task::spawn_blocking(move || {
            c.st.safebox.extract(&id, &d, &mut |done, total| {
                c.percent(done as f64 / total.max(1) as f64 * 100.0);
                !c.canceled()
            })
        })
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
        ctx.app.opener().open_path(dst.to_string_lossy().into_owned(), None::<&str>).map_err(|e| AppError::msg(format!("无法打开文件：{e}")))?;
        Ok(PathBuf::new())
    }))
}

/// 解密导出到指定文件夹。
#[tauri::command]
pub fn safebox_export(app: AppHandle, state: St<'_>, id: String, dir: String) -> AppResult<u64> {
    require_unlocked(&state)?;
    let entry = state.safebox.entry(&id)?;
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        return Err(AppError::invalid("请选择一个已存在的文件夹。"));
    }
    let base = crate::naming::sanitize(&entry.name);
    let dst = crate::naming::unique_path(dir.join(&base), &|p: &Path| p.exists());
    Ok(spawn_job(&app, entry.name.clone(), "解密导出", move |ctx| async move {
        let (c, d) = (ctx.clone(), dst.clone());
        tokio::task::spawn_blocking(move || {
            c.st.safebox.extract(&id, &d, &mut |done, total| {
                c.percent(done as f64 / total.max(1) as f64 * 100.0);
                !c.canceled()
            })
        })
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
        Ok(dst)
    }))
}

#[tauri::command]
pub fn safebox_remove(app: AppHandle, state: St<'_>, ids: Vec<String>) -> AppResult<usize> {
    require_unlocked(&state)?;
    let n = state.safebox.remove(&ids)?;
    let _ = app.emit(security::EVT_SAFEBOX, ());
    Ok(n)
}
