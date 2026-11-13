//! AI 功能的前端命令：密钥管理、接口测试、转写 / 翻译 / 摘要任务。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::{ai, ai_pool, media_tools, vault, AppState};

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

/// 翻译记忆：一共几条、其中几条是人工改过的。
#[tauri::command]
pub fn tm_stats(state: St<'_>) -> (u64, u64) {
    crate::tm::count(&state.data_dir)
}

/// 清空翻译记忆里的机器译文（人工改过的保留），返回清掉几条。
#[tauri::command]
pub fn tm_clear(state: St<'_>) -> AppResult<u64> {
    crate::tm::clear_machine(&state.data_dir)
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
    /// 实际会用的 whisper-cli 和模型（设置里没填或失效时自动找到的）
    pub whisper_path: Option<String>,
    pub whisper_model_path: Option<String>,
}

#[tauri::command]
pub fn ai_status(state: St<'_>) -> AiStatus {
    let s = state.settings();
    let chat_key = state.vault.has(ai::KEY_CHAT);
    let stt_key = state.vault.has(ai::KEY_STT) || chat_key;
    AiStatus {
        chat_ready: (!s.ai.base_url.is_empty() && !s.ai.model.is_empty() && chat_key)
            || s.ai.pool.iter().any(|e| e.enabled && !e.base_url.is_empty() && !e.model.is_empty() && state.vault.has(&ai_pool::key_name(&e.id))),
        chat_key,
        stt_ready: if ai::is_cloud_stt(&s) {
            ai::stt_ready(&state, &s).is_ok()
        } else if ai::use_local_stt(&state, &s) {
            ai::local_stt_ready(&s)
        } else {
            (!s.ai.stt_url().is_empty() && !s.ai.stt_model.is_empty() && stt_key) || ai::stt_pool_endpoint(&state, &s).is_some()
        },
        stt_key,
        whisper_found: ai::whisper_available(&s),
        whisper_path: ai::find_whisper_bin(&s).map(|p| p.to_string_lossy().into_owned()),
        whisper_model_path: ai::find_whisper_model(&s).map(|p| p.to_string_lossy().into_owned()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolStatus {
    pub id: String,
    pub has_key: bool,
    pub used: u32,
    pub ok: u32,
    pub fail: u32,
    /// 还要冷却多少秒，0 = 可用
    pub cooling_secs: i64,
    pub cool_reason: String,
    pub bad_key: bool,
    pub last_error: String,
    /// 本月已翻译的字符（机器翻译额度）
    pub chars_used: u64,
}

/// 模型池里每个服务（含“默认”）今天用了多少次、是否在冷却、最近的错误。
#[tauri::command]
pub fn ai_pool_status(state: St<'_>) -> Vec<PoolStatus> {
    let s = state.settings();
    let now = crate::db::now();
    let rt = state.ai_pool.snapshot(now);
    let mut ids: Vec<(String, bool)> = s.ai.pool.iter().map(|e| (e.id.clone(), state.vault.has(&ai_pool::key_name(&e.id)))).collect();
    ids.push(("default".into(), state.vault.has(ai::KEY_CHAT)));
    ids.into_iter()
        .map(|(id, has_key)| {
            let r = rt.get(&id).cloned().unwrap_or_default();
            PoolStatus {
                has_key,
                used: r.used,
                ok: r.ok,
                fail: r.fail,
                cooling_secs: (r.cool_until - now).max(0),
                cool_reason: if r.cool_until > now { r.cool_reason } else { String::new() },
                bad_key: r.bad_key,
                last_error: r.last_error,
                chars_used: state.ai_pool.chars_used(&id, now),
                id,
            }
        })
        .collect()
}

#[tauri::command]
pub fn ai_pool_presets() -> Vec<ai_pool::Preset> {
    ai_pool::PRESETS.to_vec()
}

/// 清除某个服务的冷却和今日计数（改了密钥、或想马上再试）。
#[tauri::command]
pub fn ai_pool_reset(state: St<'_>, id: String) {
    state.ai_pool.reset(&id);
}

/// 只用这一个服务发一句话，测试接口、密钥和模型。
#[tauri::command]
pub async fn ai_pool_test(state: St<'_>, id: String) -> AppResult<String> {
    let settings = state.settings();
    let ep = ai::entry_endpoint(&state, &settings, &id)?;
    // 机器翻译服务：真的译一句，看接口、密钥和区域是否可用
    if ep.cands.first().is_some_and(|c| c.kind.is_mt()) {
        let sys = ai::translate_system(&settings.ai.target_lang);
        let reply = ai::chat(&ep, "", &sys, "[\"Hello, world!\"]").await?;
        let first = ai::parse_array(&reply, 1).and_then(|v| v.into_iter().next()).unwrap_or(reply);
        return Ok(format!("Hello, world! → {first}").chars().take(80).collect());
    }
    let reply = ai::chat(&ep, "", "You are a connectivity test. Answer with the single word: OK", "ping").await?;
    Ok(reply.chars().take(80).collect())
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

/// 字幕工坊：给没有字幕的视频 / 音频生成字幕（识别 → 过滤 → AI 纠错 → 断句 → 翻译 → 导出）。
#[tauri::command]
pub fn subgen_start(app: AppHandle, path: String, opts: crate::subgen::GenOpts) -> AppResult<u64> {
    let p = existing_file(&path)?;
    for f in &opts.formats {
        if !matches!(f.as_str(), "srt" | "vtt" | "ass" | "lrc" | "txt" | "json") {
            return Err(AppError::invalid("输出格式只能是 SRT、VTT、ASS、LRC、TXT 或 JSON。"));
        }
    }
    Ok(media_tools::spawn_job(&app, title_of(&p), "生成字幕", move |ctx| async move { crate::subgen::run_subgen(&ctx, &p, opts).await }))
}

/// 对已有字幕做 AI 纠错（补标点、改同音错字），保存为新文件，原字幕不动。
#[tauri::command]
pub fn subgen_correct(app: AppHandle, path: String, mode: Option<String>) -> AppResult<u64> {
    let p = existing_file(&path)?;
    Ok(media_tools::spawn_job(&app, title_of(&p), "字幕纠错", move |ctx| async move { crate::subgen::run_correct(&ctx, &p, mode).await }))
}

/// 文件翻译：TXT / Markdown / 字幕 / Word / EPUB 整份翻译，按原格式输出双语或仅译文。
#[tauri::command]
pub fn filetr_start(app: AppHandle, path: String, opts: crate::filetr::Opts) -> AppResult<u64> {
    let p = existing_file(&path)?;
    if crate::filetr::kind_of(&p).is_none() {
        return Err(AppError::invalid("支持 TXT、Markdown、PDF、SRT / VTT / ASS 字幕、Word(docx)、EPUB，以及图片（PNG / JPG / WebP）。"));
    }
    // 记下源文件和选项：文件翻译页切走再回来，靠它认出“这个任务是哪个文件、是不是只预览”
    let meta =
        media_tools::JobMeta { kind: Some("filetr"), input: Some(p.to_string_lossy().into_owned()), params: serde_json::to_value(&opts).unwrap_or_default() };
    let title = title_of(&p);
    Ok(media_tools::spawn_job_meta(&app, title.clone(), "文件翻译", meta, move |ctx| async move {
        use crate::filetr_ext as ext;
        // 记一条任务记录：任务列表只在内存里，重启后靠它认出“这个文件翻译到哪儿了”，还能继续
        let st = ctx.st.clone();
        let target = if opts.target.trim().is_empty() { st.settings().ai.target_lang.clone() } else { opts.target.trim().to_string() };
        let path_s = p.to_string_lossy().into_owned();
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        ext::recent_put(&st.data_dir, ext::Recent { path: path_s.clone(), title, target: target.clone(), opts: opts.clone(), at, ..Default::default() });
        let r = crate::filetr::run(&ctx, &p, opts.clone()).await;
        match &r {
            Ok(out) => ext::recent_finish(&st.data_dir, &path_s, &target, "ok", (!opts.preview).then(|| out.to_string_lossy().into_owned()), ""),
            Err(_) if ctx.canceled() => ext::recent_finish(&st.data_dir, &path_s, &target, "cancel", None, ""),
            Err(e) => ext::recent_finish(&st.data_dir, &path_s, &target, "err", None, &e.message),
        }
        r
    }))
}

/// 文件翻译：开始前的估算（几页、几段、多少字、缓存里已有多少、预计请求数），按页范围和“不翻译”的设置算。
#[tauri::command]
pub async fn filetr_plan(state: St<'_>, path: String, opts: crate::filetr::Opts) -> AppResult<crate::filetr_ext::PlanInfo> {
    let p = existing_file(&path)?;
    crate::filetr_ext::plan(&state, &p, &opts).await
}

/// 文件翻译：任务记录（重启后还在）。
#[tauri::command]
pub fn filetr_recent(state: St<'_>) -> Vec<crate::filetr_ext::RecentView> {
    crate::filetr_ext::recent_list(&state.data_dir)
}

/// 文件翻译：去掉一条任务记录（译文缓存不动，再拖入同一个文件会接着用）。
#[tauri::command]
pub fn filetr_recent_remove(state: St<'_>, path: String, target: String) {
    crate::filetr_ext::recent_remove(&state.data_dir, &path, &target);
}

/// 文件翻译：把已有的译文存成文件。`kind`：txt 译文 / md 双语 Markdown / srt 双语字幕 / srt-trans 仅译文字幕。
#[tauri::command]
pub fn filetr_export(state: St<'_>, path: String, target: String, kind: String, dest: String) -> AppResult<()> {
    let p = existing_file(&path)?;
    let text = crate::filetr_ext::export_text(&state.data_dir, &p, target.trim(), &kind)?;
    std::fs::write(&dest, text).map_err(|e| AppError::invalid(format!("没法保存到这个位置：{e}")))
}

/// 文件翻译：删掉这份文件的译文缓存（含阅读器里改过的译文），返回删了几个文件。
#[tauri::command]
pub fn filetr_cache_delete(state: St<'_>, path: String, target: String) -> usize {
    crate::filetr_ext::delete_cache(&state.data_dir, Path::new(&path), target.trim())
}

/// 文件翻译：不翻译，只把文件切成页，给阅读器先看原文。
#[tauri::command]
pub fn filetr_preview(path: String) -> AppResult<crate::filetr::View> {
    let p = existing_file(&path)?;
    crate::filetr::preview(&p)
}

/// 文件翻译：读出这个文件（和目标语言）已经译好的内容，没有就返回空。
#[tauri::command]
pub fn filetr_view(state: St<'_>, path: String, target: Option<String>) -> AppResult<Option<crate::filetr::View>> {
    let p = existing_file(&path)?;
    let t = target.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| state.settings().ai.target_lang.clone());
    Ok(crate::filetr::view_for(&state.data_dir, &p, &t))
}

/// 文件翻译：版面画布的一页数据（原页图 + 每块的位置、背景色、原文、译文）。
#[tauri::command]
pub async fn filetr_page_layout(state: St<'_>, path: String, target: String, ocr: Option<String>, page: usize) -> AppResult<crate::filetr::PageLayout> {
    let p = existing_file(&path)?;
    crate::filetr::page_layout(&state, &p, target.trim(), ocr.as_deref().unwrap_or("vision"), page).await
}

/// 文件翻译：导出效果——把译文真的写到原页上再渲染，返回原页和译文页两张图。
#[tauri::command]
pub async fn filetr_page_preview(state: St<'_>, path: String, target: String, ocr: Option<String>, page: usize) -> AppResult<crate::filetr::PagePreview> {
    let p = existing_file(&path)?;
    crate::filetr::page_preview(&state, &p, target.trim(), ocr.as_deref().unwrap_or("vision"), page).await
}

/// 文件翻译：阅读器里改了一段译文（空白 = 去掉这段译文）。
#[tauri::command]
pub fn filetr_edit(state: St<'_>, path: String, target: String, page: usize, index: usize, text: String) -> AppResult<()> {
    let p = existing_file(&path)?;
    crate::filetr::edit_seg(&state.data_dir, &p, target.trim(), page, index, &text)
}

/// 文件翻译：版面里手动调了一块的位置 / 字号（`rect` 为 (左, 下, 右, 上)，PDF 点；两个都不给 = 还原自动排版）。
#[tauri::command]
pub fn filetr_block_adjust(state: St<'_>, path: String, target: String, page: usize, index: usize, rect: Option<[f32; 4]>, size: Option<f32>) -> AppResult<()> {
    let p = existing_file(&path)?;
    let adj = (rect.is_some() || size.is_some()).then_some(crate::filetr::BlockAdj { rect, size, ..Default::default() });
    crate::filetr::adjust_block(&state.data_dir, &p, target.trim(), page, index, adj)
}

/// 文件翻译：多选后批量改样式（文字色、背景、对齐、外扩、字号、位置）。每一块一份改动，全部通过才写回。
#[tauri::command]
pub fn filetr_block_style(state: St<'_>, path: String, target: String, page: usize, edits: Vec<crate::filetr::BlockEdit>) -> AppResult<()> {
    let p = existing_file(&path)?;
    crate::filetr::style_blocks(&state.data_dir, &p, target.trim(), page, &edits)
}

/// 文件翻译：把文字颜色 `from` 换成 `to`；`page` 不给 = 整份文件。`dry_run` 只数有几处。返回改（或会改）了几块。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn filetr_replace_color(
    state: St<'_>,
    path: String,
    target: String,
    page: Option<usize>,
    from: [u8; 3],
    to: [u8; 3],
    tolerance: Option<u32>,
    dry_run: bool,
) -> AppResult<usize> {
    let p = existing_file(&path)?;
    crate::filetr::replace_text_color(&state, &p, target.trim(), page, from, to, tolerance.unwrap_or(24).min(120), dry_run).await
}

/// 文件翻译：把阅读器里的一段原文重新翻一遍，返回新译文。
#[tauri::command]
pub async fn filetr_retranslate(state: St<'_>, path: String, target: String, page: usize, index: usize) -> AppResult<String> {
    let p = existing_file(&path)?;
    crate::filetr::retranslate_seg(&state, &p, target.trim(), page, index).await
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
