//! AI 功能的前端命令：密钥管理、接口测试、转写 / 翻译 / 摘要任务。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::{ai, media_tools, vault, AppState};

type St<'a> = State<'a, Arc<AppState>>;

/// 允许前端读写的密钥名前缀。
const SECRET_PREFIXES: &[&str] = &["ai.", "stt.", "notify.", "upload.", "api.", "webhook."];

fn check_secret_name(name: &str) -> AppResult<()> {
    if vault::valid_name(name) && SECRET_PREFIXES.iter().any(|p| name.starts_with(p)) {
        Ok(())
    } else {
        Err(AppError::invalid("不支持的密钥名称。"))
    }
}

#[tauri::command]
pub fn secret_set(state: St<'_>, name: String, value: String) -> AppResult<()> {
    check_secret_name(&name)?;
    state.vault.set(&name, value.trim())
}

#[tauri::command]
pub fn secret_has(state: St<'_>, name: String) -> AppResult<bool> {
    check_secret_name(&name)?;
    Ok(state.vault.has(&name))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub chat_ready: bool,
    pub chat_key: bool,
    pub stt_ready: bool,
    pub stt_key: bool,
    pub whisper_found: bool,
}

#[tauri::command]
pub fn ai_status(state: St<'_>) -> AiStatus {
    let s = state.settings();
    let chat_key = state.vault.has(ai::KEY_CHAT);
    let stt_key = state.vault.has(ai::KEY_STT) || chat_key;
    AiStatus {
        chat_ready: !s.ai.base_url.is_empty() && !s.ai.model.is_empty() && chat_key,
        chat_key,
        stt_ready: if s.ai.stt_engine == "local" {
            !s.ai.whisper_model.is_empty()
        } else {
            !s.ai.stt_url().is_empty() && !s.ai.stt_model.is_empty() && stt_key
        },
        stt_key,
        whisper_found: ai::whisper_available(&s),
    }
}

/// 发一句话测试对话接口、密钥和模型是否可用。
#[tauri::command]
pub async fn ai_test(state: St<'_>) -> AppResult<String> {
    let settings = state.settings();
    let ep = ai::chat_endpoint(&state, &settings)?;
    let reply = ai::chat(&ep, &settings.ai.model, "You are a connectivity test. Answer with the single word: OK", "ping").await?;
    Ok(reply.chars().take(80).collect())
}

fn existing_file(path: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(path);
    if p.is_file() {
        Ok(p)
    } else {
        Err(AppError::not_found("找不到文件。"))
    }
}

fn title_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

#[tauri::command]
pub fn ai_translate(app: AppHandle, path: String) -> AppResult<u64> {
    let p = existing_file(&path)?;
    Ok(media_tools::spawn_job(&app, title_of(&p), "翻译字幕", move |ctx| async move { ai::run_translate(&ctx, &p).await }))
}

#[tauri::command]
pub fn ai_summarize(app: AppHandle, path: String) -> AppResult<u64> {
    let p = existing_file(&path)?;
    Ok(media_tools::spawn_job(&app, title_of(&p), "摘要与章节", move |ctx| async move { ai::run_summarize(&ctx, &p).await }))
}

#[tauri::command]
pub fn ai_transcribe(app: AppHandle, path: String, language: Option<String>) -> AppResult<u64> {
    let p = existing_file(&path)?;
    Ok(media_tools::spawn_job(&app, title_of(&p), "语音转文字", move |ctx| async move {
        ai::run_transcribe(&ctx, &p, language.filter(|l| !l.is_empty())).await
    }))
}

/// 读取 AI 任务输出的文本结果（摘要 .md、章节 .txt、字幕 .srt）。只能读任务列表里自己生成的文件。
#[tauri::command]
pub fn media_job_text(state: St<'_>, id: u64, which: Option<String>) -> AppResult<String> {
    let job = state.media_jobs.snapshot().into_iter().find(|j| j.id == id).ok_or_else(|| AppError::not_found("任务不存在"))?;
    let out = PathBuf::from(job.output.ok_or_else(|| AppError::not_found("任务还没有输出"))?);
    let path = if which.as_deref() == Some("chapters") { out.with_extension("章节.txt") } else { out };
    if !matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "txt" | "srt")) {
        return Err(AppError::invalid("这个任务的输出不是文本。"));
    }
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 2 * 1024 * 1024 {
        return Err(AppError::invalid("文件太大。"));
    }
    std::fs::read_to_string(&path).map_err(|_| AppError::not_found("读不到输出文件"))
}

/// 给指定的通知渠道发一条测试通知。
#[tauri::command]
pub async fn notify_test(state: St<'_>, id: String) -> AppResult<()> {
    crate::notify::test(&state, &id).await
}

/// 测试自动上传的连接（WebDAV 或目标文件夹）。
#[tauri::command]
pub async fn upload_test(state: St<'_>) -> AppResult<String> {
    crate::upload::test(&state).await
}
