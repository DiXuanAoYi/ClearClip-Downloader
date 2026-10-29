//! AI 功能：字幕翻译、摘要与章节（兼容 OpenAI 的对话接口），语音转文字（兼容 OpenAI 的
//! `/audio/transcriptions`，或本机的 whisper.cpp）。API 密钥保存在加密的保险箱里。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::media_tools::JobCtx;
use crate::model::Chapter;
use crate::postprocess::{base_args, find_ffmpeg, run_ffmpeg_in};
use crate::settings::Settings;
use crate::subtitle::{self, Cue};
use crate::{subtitle_tools, AppState};

pub const KEY_CHAT: &str = "ai.api_key";
pub const KEY_STT: &str = "stt.api_key";

// ---------- 接口 ----------

pub struct Endpoint {
    pub base: String,
    pub key: Option<String>,
    pub client: reqwest::Client,
    /// 429 / 5xx / 网络错误时的重试间隔基数（毫秒）
    pub retry_ms: u64,
}

fn endpoint(st: &AppState, settings: &Settings, base: &str, key_name: &str) -> AppResult<Endpoint> {
    if base.is_empty() {
        return Err(AppError::invalid("请先在“设置 → AI”里填写接口地址。"));
    }
    let client = st.net.clients_for(&settings.network, base)?.download;
    Ok(Endpoint { base: base.to_string(), key: st.vault.get(key_name).filter(|k| !k.is_empty()), client, retry_ms: 2000 })
}

pub fn chat_endpoint(st: &AppState, settings: &Settings) -> AppResult<Endpoint> {
    if settings.ai.model.is_empty() {
        return Err(AppError::invalid("请先在“设置 → AI”里填写模型名称。"));
    }
    endpoint(st, settings, &settings.ai.base_url, KEY_CHAT)
}

pub fn stt_endpoint(st: &AppState, settings: &Settings) -> AppResult<Endpoint> {
    let base = settings.ai.stt_url();
    // 语音转文字单独没有密钥时沿用对话接口的密钥（同一家服务商）
    let key_name = if st.vault.has(KEY_STT) { KEY_STT } else { KEY_CHAT };
    endpoint(st, settings, base, key_name)
}

fn api_error(status: u16, body: &str) -> AppError {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")).or_else(|| v.pointer("/error")).and_then(|m| m.as_str().map(str::to_string)))
        .unwrap_or_else(|| body.chars().take(200).collect());
    match status {
        401 | 403 => AppError::new(ErrorKind::NeedLogin, format!("API 密钥无效或没有权限（{status}）：{msg}")),
        404 => AppError::invalid(format!("接口地址或模型名称不对（404）：{msg}")),
        429 => AppError::new(ErrorKind::RateLimited, format!("请求太频繁或额度用完（429）：{msg}")),
        s if s >= 500 => AppError::new(ErrorKind::Network, format!("服务暂时不可用（{s}）：{msg}")),
        s => AppError::msg(format!("接口返回 {s}：{msg}")),
    }
}

async fn send(ep: &Endpoint, path: &str, content_type: &str, body: Vec<u8>, timeout: Duration) -> AppResult<String> {
    let url = format!("{}{path}", ep.base);
    let mut last: Option<AppError> = None;
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(ep.retry_ms * (1 << (attempt - 1)))).await;
        }
        let mut req = ep.client.post(&url).header("Content-Type", content_type).timeout(timeout).body(body.clone());
        if let Some(k) = &ep.key {
            req = req.header("Authorization", format!("Bearer {k}"));
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp.text().await.map_err(|e| AppError::new(ErrorKind::Network, format!("读取响应失败：{e}")))?;
                if (200..300).contains(&status) {
                    return Ok(text);
                }
                let err = api_error(status, &text);
                if status == 429 || status >= 500 {
                    last = Some(err);
                    continue;
                }
                return Err(err);
            }
            Err(e) => last = Some(AppError::new(ErrorKind::Network, format!("无法连接接口：{e}"))),
        }
    }
    Err(last.unwrap_or_else(|| AppError::msg("请求失败")))
}

/// 一次对话，返回模型的回答文字。
pub async fn chat(ep: &Endpoint, model: &str, system: &str, user: &str) -> AppResult<String> {
    let body = json!({
        "model": model,
        "temperature": 0.2,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    });
    let text = send(ep, "/chat/completions", "application/json", serde_json::to_vec(&body)?, Duration::from_secs(180)).await?;
    let v: Value =
        serde_json::from_str(&text).map_err(|_| AppError::parser("接口返回的不是有效的 JSON。请确认接口地址是兼容 OpenAI 的（通常以 /v1 结尾）。"))?;
    v.pointer("/choices/0/message/content").and_then(Value::as_str).map(str::to_string).ok_or_else(|| AppError::parser("接口没有返回内容，请检查模型名称。"))
}

// ---------- 翻译 ----------

pub fn translate_system(lang: &str) -> String {
    format!(
        "You are a professional subtitle translator. Translate every subtitle line in the user's JSON array into {lang}. \
         Rules: (1) Reply with ONE JSON array only, no commentary and no code fences. (2) The array must have exactly the same number of \
         strings, in the same order. Never merge or split lines. (3) Keep names and terms consistent, use natural spoken language. \
         (4) A line break inside a subtitle is written as \\n and must stay as \\n."
    )
}

fn strip_fences(s: &str) -> &str {
    let t = s.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```JSON")).or_else(|| t.strip_prefix("```")).unwrap_or(t);
    t.strip_suffix("```").unwrap_or(t).trim()
}

/// 从模型回答里取出 `n` 条译文。回答里夹带说明文字、代码围栏、`{"translations": [...]}` 都能处理；数量不对返回 None。
pub fn parse_array(resp: &str, n: usize) -> Option<Vec<String>> {
    let t = strip_fences(resp);
    let arr: Vec<Value> = if let (Some(a), Some(b)) = (t.find('['), t.rfind(']')) {
        // 如果最外层是对象，里面的数组也在 [ ] 之间，取最外面的一对即可
        serde_json::from_str(&t[a..=b]).ok()?
    } else {
        return None;
    };
    let out: Vec<String> = arr
        .into_iter()
        .map(|v| match v {
            Value::String(s) => s,
            other => other.to_string(),
        })
        .collect();
    (out.len() == n).then_some(out)
}

pub struct Translated {
    pub texts: Vec<Option<String>>,
    /// 翻译失败、保留原文的条数
    pub failed: usize,
}

/// 分批翻译。某一批回答的数量对不上就拆成两半重试，直到单条；单条还不行就保留原文。
/// 网络 / 密钥错误直接返回错误（继续试也没用）。
pub async fn translate_cues(
    ep: &Endpoint,
    model: &str,
    lang: &str,
    cues: &[Cue],
    batch: usize,
    ctx: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Translated> {
    let texts: Vec<String> = cues.iter().map(|c| c.lines.join("\n")).collect();
    let mut out: Vec<Option<String>> = vec![None; texts.len()];
    let mut queue: VecDeque<(usize, usize)> = (0..texts.len()).step_by(batch.max(1)).map(|s| (s, (s + batch.max(1)).min(texts.len()))).collect();
    let system = translate_system(lang);
    let mut done = 0usize;
    let mut failed = 0usize;
    while let Some((a, b)) = queue.pop_front() {
        ctx(done, texts.len())?;
        let user = serde_json::to_string(&texts[a..b])?;
        let resp = chat(ep, model, &system, &user).await;
        let parsed = match resp {
            Ok(r) => parse_array(&r, b - a),
            Err(e) if matches!(e.kind, ErrorKind::NeedLogin | ErrorKind::Invalid | ErrorKind::RateLimited | ErrorKind::Network) => return Err(e),
            Err(_) => None,
        };
        match parsed {
            Some(v) => {
                for (i, t) in v.into_iter().enumerate() {
                    out[a + i] = Some(t);
                }
                done += b - a;
            }
            None if b - a > 1 => {
                let mid = a + (b - a) / 2;
                queue.push_front((mid, b));
                queue.push_front((a, mid));
            }
            None => {
                failed += 1;
                done += 1;
            }
        }
    }
    ctx(texts.len(), texts.len())?;
    Ok(Translated { texts: out, failed })
}

/// 把译文合回字幕。`bilingual` 时原文在上、译文在下；没翻译成功的条目保持原文。
pub fn apply_translation(cues: &[Cue], texts: &[Option<String>], bilingual: bool) -> Vec<Cue> {
    cues.iter()
        .zip(texts)
        .map(|(c, t)| {
            let lines = match t {
                Some(t) if bilingual => c.lines.iter().cloned().chain(t.split('\n').map(str::to_string)).collect(),
                Some(t) => t.split('\n').map(str::to_string).collect(),
                None => c.lines.clone(),
            };
            Cue { start: c.start, end: c.end, lines }
        })
        .collect()
}

/// 目标语言的文件名标记：`课.zh.srt`。认不出的用 `ai`。
pub fn lang_tag(target: &str) -> &'static str {
    let t = target.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["繁体", "繁體", "traditional", "zh-hant", "zh-tw"]) {
        "zh-Hant"
    } else if has(&["中文", "汉语", "漢語", "chinese", "zh"]) {
        "zh"
    } else if has(&["english", "英语", "英文", "en"]) {
        "en"
    } else if has(&["日语", "日本語", "日文", "japanese", "ja"]) {
        "ja"
    } else if has(&["韩语", "한국어", "韩文", "korean", "ko"]) {
        "ko"
    } else if has(&["法语", "french", "fr"]) {
        "fr"
    } else if has(&["德语", "german", "de"]) {
        "de"
    } else if has(&["西班牙", "spanish", "es"]) {
        "es"
    } else if has(&["俄语", "russian", "ru"]) {
        "ru"
    } else {
        "ai"
    }
}

/// 去掉字幕文件名末尾的语言标记（`课.en` → `课`）。
pub fn strip_lang_suffix(stem: &str) -> &str {
    match stem.rsplit_once('.') {
        Some((head, tail)) if !head.is_empty() && tail.len() <= 8 && tail.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && !tail.is_empty() => head,
        _ => stem,
    }
}

// ---------- 摘要与章节 ----------

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub summary: String,
    pub points: Vec<String>,
    pub chapters: Vec<Chapter>,
}

fn clock_label(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// `[0:12] 文字` 一行一条，给模型看带时间的文稿。
pub fn transcript_lines(cues: &[Cue]) -> Vec<String> {
    cues.iter().map(|c| format!("[{}] {}", clock_label(c.start), c.lines.join(" "))).collect()
}

/// 把文稿按字数切块，不拆开单行。
pub fn chunk_lines(lines: &[String], max_chars: usize) -> Vec<String> {
    let mut chunks = vec![];
    let mut cur = String::new();
    for l in lines {
        if !cur.is_empty() && cur.chars().count() + l.chars().count() + 1 > max_chars {
            chunks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(l);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// `1:23` / `01:02:03` / `83` → 毫秒。
pub fn parse_label(s: &str) -> Option<u64> {
    let s = s.trim().trim_matches(['[', ']']);
    let nums: Vec<u64> = s.split(':').map(|p| p.trim().split('.').next().unwrap_or("").parse().ok()).collect::<Option<_>>()?;
    let secs = match nums.as_slice() {
        [s] => *s,
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(secs * 1000)
}

pub fn parse_summary(resp: &str) -> Option<Summary> {
    let t = strip_fences(resp);
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    let v: Value = serde_json::from_str(&t[a..=b]).ok()?;
    let summary = v.get("summary").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let points: Vec<String> = v
        .get("points")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|p| p.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut chapters: Vec<Chapter> = v
        .get("chapters")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let start = c.get("time").and_then(|t| t.as_str().and_then(parse_label).or_else(|| t.as_u64().map(|n| n * 1000)))?;
                    let title = c.get("title").and_then(Value::as_str)?.trim().to_string();
                    (!title.is_empty()).then_some(Chapter { title, start_ms: start, end_ms: start })
                })
                .collect()
        })
        .unwrap_or_default();
    chapters.sort_by_key(|c| c.start_ms);
    chapters.dedup_by_key(|c| c.start_ms);
    if summary.is_empty() && points.is_empty() && chapters.is_empty() {
        return None;
    }
    Some(Summary { summary, points, chapters })
}

fn summary_system(lang: &str, final_pass: bool) -> String {
    let chapters = if final_pass { "" } else { " and \"chapters\" (array of {\"time\": \"m:ss\" or \"h:mm:ss\", \"title\": short title})" };
    format!(
        "You summarize video transcripts. The transcript lines look like \"[m:ss] text\". Write in {lang}. Reply with ONE JSON object only, no code fences, \
         with keys \"summary\" (2-4 sentences), \"points\" (array of 3-8 key takeaways){chapters}. \
         Chapters must use timestamps that appear in the transcript, start near the beginning, be in order and be 4-12 in number for long videos (fewer for short ones)."
    )
}

/// 摘要、要点和章节。长文稿分块处理后再合并。
pub async fn summarize(ep: &Endpoint, model: &str, lang: &str, cues: &[Cue], progress: &(dyn Fn(usize, usize) -> AppResult<()> + Sync)) -> AppResult<Summary> {
    let lines = transcript_lines(cues);
    let chunks = chunk_lines(&lines, 12_000);
    if chunks.is_empty() {
        return Err(AppError::invalid("字幕里没有内容。"));
    }
    let system = summary_system(lang, false);
    let mut parts: Vec<Summary> = vec![];
    for (i, c) in chunks.iter().enumerate() {
        progress(i, chunks.len() + 1)?;
        let resp = chat(ep, model, &system, c).await?;
        parts.push(parse_summary(&resp).ok_or_else(|| AppError::parser("模型没有按要求返回摘要，请换一个模型或稍后重试。"))?);
    }
    if parts.len() == 1 {
        progress(1, 1)?;
        return Ok(parts.remove(0));
    }
    // 合并：各块的摘要再总结一次，章节直接按时间拼接
    progress(chunks.len(), chunks.len() + 1)?;
    let joined = parts.iter().enumerate().map(|(i, p)| format!("Part {}: {}\n{}", i + 1, p.summary, p.points.join("; "))).collect::<Vec<_>>().join("\n\n");
    let resp = chat(ep, model, &summary_system(lang, true), &joined).await?;
    let merged = parse_summary(&resp).unwrap_or_default();
    let mut chapters: Vec<Chapter> = parts.into_iter().flat_map(|p| p.chapters).collect();
    chapters.sort_by_key(|c| c.start_ms);
    chapters.dedup_by_key(|c| c.start_ms);
    progress(chunks.len() + 1, chunks.len() + 1)?;
    Ok(Summary { summary: merged.summary, points: merged.points, chapters })
}

/// 补全每章的结束时间（下一章的开始；最后一章用总时长）。
pub fn close_chapters(mut chapters: Vec<Chapter>, total_ms: u64) -> Vec<Chapter> {
    for i in 0..chapters.len() {
        let end = chapters.get(i + 1).map(|n| n.start_ms).unwrap_or(total_ms).max(chapters[i].start_ms + 1);
        chapters[i].end_ms = end;
    }
    chapters
}

/// 视频平台描述里常见的章节写法：每行 `0:00 标题`。
pub fn chapters_text(chapters: &[Chapter]) -> String {
    chapters.iter().map(|c| format!("{} {}", clock_label(c.start_ms), c.title)).collect::<Vec<_>>().join("\n")
}

pub fn summary_markdown(title: &str, s: &Summary) -> String {
    let mut out = format!("# {title}\n\n");
    if !s.summary.is_empty() {
        out.push_str(&format!("## 摘要\n\n{}\n\n", s.summary));
    }
    if !s.points.is_empty() {
        out.push_str("## 要点\n\n");
        for p in &s.points {
            out.push_str(&format!("- {p}\n"));
        }
        out.push('\n');
    }
    if !s.chapters.is_empty() {
        out.push_str("## 章节\n\n");
        for c in &s.chapters {
            out.push_str(&format!("- {} {}\n", clock_label(c.start_ms), c.title));
        }
    }
    out
}

// ---------- 语音转文字 ----------

/// 手工拼 multipart/form-data（不需要额外的依赖）。
pub fn multipart(fields: &[(&str, &str)], file_field: &str, file_name: &str, mime: &str, bytes: &[u8], boundary: &str) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    for (k, v) in fields {
        out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    out.extend(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    out.extend(bytes);
    out.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// 把音频切成每段 `seg_secs` 秒的小文件（单声道 16 kHz AAC，体积小，接口大小限制不会超）。返回（文件，起点毫秒）。
pub async fn split_audio(ffmpeg: &Path, input: &Path, dir: &Path, seg_secs: u64) -> AppResult<Vec<(PathBuf, u64)>> {
    std::fs::create_dir_all(dir)?;
    let mut a = base_args();
    a.extend([
        "-i".to_string(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-map".into(),
        "0:a:0".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "32k".into(),
        "-f".into(),
        "segment".into(),
        "-segment_time".into(),
        seg_secs.to_string(),
        "-segment_format".into(),
        "mp4".into(),
        "-reset_timestamps".into(),
        "1".into(),
        dir.join("part_%03d.m4a").to_string_lossy().into_owned(),
    ]);
    run_ffmpeg_in(ffmpeg, &a, None).await?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "m4a")).collect();
    files.sort();
    if files.is_empty() {
        return Err(AppError::invalid("这个文件里没有可以转写的音频。"));
    }
    Ok(files.into_iter().enumerate().map(|(i, p)| (p, i as u64 * seg_secs * 1000)).collect())
}

/// 调用 `/audio/transcriptions` 转写一段音频，返回 SRT 文字。
pub async fn transcribe_chunk(ep: &Endpoint, model: &str, language: &str, file: &Path) -> AppResult<String> {
    let bytes = std::fs::read(file)?;
    let boundary = format!("----clearclip{:x}", crate::db::now() as u64 ^ (bytes.len() as u64).rotate_left(17));
    let mut fields = vec![("model", model), ("response_format", "srt"), ("temperature", "0")];
    if !language.is_empty() {
        fields.push(("language", language));
    }
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "audio.m4a".into());
    let body = multipart(&fields, "file", &name, "audio/mp4", &bytes, &boundary);
    let text = send(ep, "/audio/transcriptions", &format!("multipart/form-data; boundary={boundary}"), body, Duration::from_secs(600)).await?;
    if text.contains("-->") {
        Ok(text)
    } else {
        Err(AppError::parser("接口没有返回字幕。请确认它支持 response_format=srt（OpenAI、Groq 等支持）。"))
    }
}

/// whisper.cpp 的命令行参数。
pub fn whisper_args(model: &str, wav: &Path, out_prefix: &Path, language: &str) -> Vec<String> {
    vec![
        "-m".into(),
        model.into(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-l".into(),
        if language.is_empty() { "auto".into() } else { language.into() },
        "-osrt".into(),
        "-of".into(),
        out_prefix.to_string_lossy().into_owned(),
        "-pp".into(),
    ]
}

/// whisper.cpp 输出里的进度（`progress =  35%`）。
pub fn whisper_progress(line: &str) -> Option<f64> {
    let rest = line.split("progress =").nth(1)?;
    rest.trim().trim_end_matches('%').trim().parse::<f64>().ok()
}

fn find_whisper_bin(settings: &Settings) -> Option<PathBuf> {
    let configured = settings.ai.whisper_bin.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return p.is_file().then_some(p);
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe"] } else { &[""] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["whisper-cli", "whisper-cpp", "main"] {
            for e in exts {
                let p = dir.join(format!("{name}{e}"));
                // “main” 太常见，只接受明确是 whisper.cpp 的（同目录有模型的情况不好判断，所以要求用户在设置里指定）
                if name != "main" && p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// 是否能找到本机的 whisper.cpp。
pub fn whisper_available(settings: &Settings) -> bool {
    find_whisper_bin(settings).is_some()
}

async fn run_whisper_local(ctx: &JobCtx, settings: &Settings, ffmpeg: &Path, input: &Path, language: &str, work: &Path) -> AppResult<String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let bin = find_whisper_bin(settings)
        .ok_or_else(|| AppError::invalid("找不到 whisper.cpp（whisper-cli）。请在“设置 → AI”里指定它的位置，或把它放进系统 PATH。"))?;
    let model = settings.ai.whisper_model.trim();
    if model.is_empty() || !Path::new(model).is_file() {
        return Err(AppError::invalid("请在“设置 → AI”里选择 whisper.cpp 的模型文件（ggml-*.bin）。"));
    }
    ctx.note("正在提取音频…");
    let wav = work.join("audio.wav");
    let mut a = base_args();
    a.extend([
        "-i".to_string(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        wav.to_string_lossy().into_owned(),
    ]);
    run_ffmpeg_in(ffmpeg, &a, None).await?;
    ctx.check()?;
    ctx.note("正在用本机 whisper.cpp 转写（第一次会比较慢）…");
    let prefix = work.join("out");
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args(whisper_args(model, &wav, &prefix, language))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| AppError::new(ErrorKind::NeedUpdate, format!("无法运行 whisper.cpp：{e}")))?;
    let stderr = child.stderr.take();
    let reader = async {
        let mut tail = String::new();
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(p) = whisper_progress(&line) {
                    ctx.percent(p);
                } else {
                    tail = line;
                }
            }
        }
        tail
    };
    let cancelled = async {
        loop {
            if ctx.canceled() {
                return;
            }
            ctx.wake.notified().await;
        }
    };
    let status = tokio::select! {
        r = async { let tail = reader.await; (child.wait().await, tail) } => r,
        _ = cancelled => return Err(AppError::msg("canceled")),
    };
    match status {
        (Ok(s), _) if s.success() => std::fs::read_to_string(prefix.with_extension("srt")).map_err(|_| AppError::msg("whisper.cpp 没有生成字幕文件。")),
        (_, tail) => Err(AppError::msg(format!("whisper.cpp 运行失败：{tail}"))),
    }
}

/// 把若干段 SRT 按各自的起点偏移合并。
pub fn merge_srt_parts(parts: Vec<(String, u64)>) -> Vec<Cue> {
    let mut out: Vec<Cue> = vec![];
    for (text, offset) in parts {
        let cues = subtitle::parse_srt(&text);
        out.extend(subtitle_tools::shift(&cues, offset as i64));
    }
    out.sort_by_key(|c| c.start);
    out
}

// ---------- 下载后自动处理 ----------

fn is_local_url(url: &str) -> bool {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h == "localhost" || h == "127.0.0.1" || h == "::1" || h == "[::1]")).unwrap_or(false)
}

/// 资源 ID（`sub-en`、`auto-en`、`sub-ai-zh-ab12cd`）里的语言部分。
fn lang_of_asset(asset_id: &str) -> &str {
    asset_id.split_once('-').map(|(_, l)| l).unwrap_or("")
}

/// 作品下是否已有目标语言的字幕。
pub fn has_lang(assets: &[String], tag: &str) -> bool {
    tag != "ai" && assets.iter().any(|a| subtitle::lang_matches(lang_of_asset(a), tag))
}

/// 下载到字幕后，如果不是目标语言、作品下也没有目标语言字幕，就自动翻译（设置里开启时）。
pub fn auto_translate(app: &tauri::AppHandle, st: &Arc<AppState>, item: &crate::db::LibraryItem) {
    let s = st.settings();
    if s.metered_mode
        || !s.ai.auto_translate
        || item.kind != "subtitle"
        || item.source == "ai"
        || item.path.ends_with(".xml")
        || !Path::new(&item.path).exists()
    {
        return;
    }
    if st.vault.get(KEY_CHAT).is_none() && !is_local_url(&s.ai.base_url) {
        return;
    }
    let tag = lang_tag(&s.ai.target_lang);
    let assets = st.db.subtitle_assets_for(&item.platform, &item.media_id).unwrap_or_default();
    if has_lang(&assets, tag) {
        return;
    }
    let path = PathBuf::from(&item.path);
    crate::media_tools::spawn_job(app, stem_of(&path), "翻译字幕", move |ctx| async move { run_translate(&ctx, &path).await });
}

/// 视频下载完成一小段时间后，作品下还是没有任何字幕，就自动转写（设置里开启时）。
pub fn auto_transcribe(app: &tauri::AppHandle, st: &Arc<AppState>, item: &crate::db::LibraryItem) {
    let s = st.settings();
    if s.metered_mode || !s.ai.auto_transcribe || item.kind != "video" || matches!(item.source.as_str(), "tool" | "ai" | "import") {
        return;
    }
    let ready = if s.ai.stt_engine == "local" {
        !s.ai.whisper_model.is_empty()
    } else {
        st.vault.get(KEY_STT).or_else(|| st.vault.get(KEY_CHAT)).is_some() || is_local_url(s.ai.stt_url())
    };
    if !ready {
        return;
    }
    let (app, st, item) = (app.clone(), st.clone(), item.clone());
    tauri::async_runtime::spawn(async move {
        // 字幕通常和视频一起下载，等一会儿再看有没有
        tokio::time::sleep(Duration::from_secs(20)).await;
        if st.db.subtitle_assets_for(&item.platform, &item.media_id).map(|a| !a.is_empty()).unwrap_or(true) || !Path::new(&item.path).exists() {
            return;
        }
        let path = PathBuf::from(&item.path);
        crate::media_tools::spawn_job(&app, stem_of(&path), "语音转文字", move |ctx| async move { run_transcribe(&ctx, &path, None).await });
    });
}

// ---------- 流程（后台任务） ----------

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
}

fn out_beside(input: &Path, base: &str, tag: &str, ext: &str) -> PathBuf {
    crate::naming::unique_path(input.with_file_name(format!("{base}.{tag}.{ext}")), &|p: &Path| p.exists())
}

/// 把新字幕登记进媒体库：挂在原视频（或原字幕）所属的作品下，并建立搜索索引。
fn register_subtitle(st: &AppState, source: &Path, sub: &Path, lang: &str) {
    let source_str = source.to_string_lossy().into_owned();
    let (platform, media_id, platform_name, title) = match st.db.item_by_path(&source_str).ok().flatten() {
        Some(i) => (i.platform, i.media_id, i.platform_name, stem_of(sub)),
        None => ("local".to_string(), format!("ai-{}", crate::library::path_key(source)), "AI 生成".to_string(), stem_of(sub)),
    };
    let asset = format!("sub-ai-{lang}-{}", &crate::library::path_key(sub)[..6]);
    let size = std::fs::metadata(sub).map(|m| m.len() as i64).unwrap_or(0);
    let ok = st
        .db
        .record_download(&crate::db::NewDownload {
            platform: &platform,
            media_id: &media_id,
            asset_id: &asset,
            title: &title,
            author: "",
            cover: None,
            path: &sub.to_string_lossy(),
            size,
            kind: "subtitle",
            source: "ai",
            source_url: "",
            platform_name: &platform_name,
        })
        .is_ok();
    if ok {
        if let Ok(Some(id)) = st.db.id_by_key(&platform, &media_id, &asset) {
            if let Ok(Some(item)) = st.db.library_item(id) {
                let _ = crate::library::index_subtitle_item(&st.db, &item);
            }
        }
    }
}

pub async fn run_translate(ctx: &JobCtx, input: &Path) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ep = chat_endpoint(&ctx.st, &settings)?;
    let cues = subtitle_tools::read(input)?;
    ctx.note(format!("正在翻译成{}…", settings.ai.target_lang));
    let progress = |done: usize, total: usize| -> AppResult<()> {
        ctx.check()?;
        ctx.percent(done as f64 / total.max(1) as f64 * 100.0);
        Ok(())
    };
    let t = translate_cues(&ep, &settings.ai.model, &settings.ai.target_lang, &cues, settings.ai.batch_size, &progress).await?;
    let merged = apply_translation(&cues, &t.texts, settings.ai.bilingual);
    let tag = lang_tag(&settings.ai.target_lang);
    let base = strip_lang_suffix(&stem_of(input)).to_string();
    let out = out_beside(input, &base, tag, "srt");
    std::fs::write(&out, subtitle::format_srt(&merged))?;
    if t.failed > 0 {
        ctx.note(format!("有 {} 条没有翻译成功，保留了原文。", t.failed));
    }
    register_subtitle(&ctx.st, input, &out, tag);
    Ok(out)
}

pub async fn run_summarize(ctx: &JobCtx, input: &Path) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ep = chat_endpoint(&ctx.st, &settings)?;
    let cues = subtitle_tools::read(input)?;
    ctx.note("正在总结…");
    let progress = |done: usize, total: usize| -> AppResult<()> {
        ctx.check()?;
        ctx.percent(done as f64 / total.max(1) as f64 * 100.0);
        Ok(())
    };
    let s = summarize(&ep, &settings.ai.model, &settings.ai.target_lang, &cues, &progress).await?;
    let base = strip_lang_suffix(&stem_of(input)).to_string();
    let md = out_beside(input, &base, "摘要", "md");
    std::fs::write(&md, summary_markdown(&base, &s))?;
    if !s.chapters.is_empty() {
        let txt = md.with_extension("章节.txt");
        std::fs::write(txt, chapters_text(&s.chapters))?;
    }
    Ok(md)
}

pub async fn run_transcribe(ctx: &JobCtx, input: &Path, language: Option<String>) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ffmpeg = find_ffmpeg(&ctx.st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let lang = language.unwrap_or_else(|| settings.ai.stt_language.clone());
    let work = std::env::temp_dir().join(format!("clearclip-stt-{}-{}", std::process::id(), ctx.id));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let result = async {
        let cues = if settings.ai.stt_engine == "local" {
            let srt = run_whisper_local(ctx, &settings, &ffmpeg, input, &lang, &work).await?;
            subtitle::parse_srt(&srt)
        } else {
            let ep = stt_endpoint(&ctx.st, &settings)?;
            if settings.ai.stt_model.is_empty() {
                return Err(AppError::invalid("请先在“设置 → AI”里填写语音转文字的模型名称（OpenAI 是 whisper-1）。"));
            }
            ctx.note("正在提取并切分音频…");
            let parts = split_audio(&ffmpeg, input, &work, 600).await?;
            let mut texts = vec![];
            for (i, (file, offset)) in parts.iter().enumerate() {
                ctx.check()?;
                ctx.note(format!("正在转写第 {} / {} 段…", i + 1, parts.len()));
                ctx.percent(i as f64 / parts.len() as f64 * 100.0);
                texts.push((transcribe_chunk(&ep, &settings.ai.stt_model, &lang, file).await?, *offset));
            }
            merge_srt_parts(texts)
        };
        if cues.is_empty() {
            return Err(AppError::msg("没有识别出任何语音。"));
        }
        let tag = if lang.is_empty() { "ai".to_string() } else { lang.clone() };
        let out = out_beside(input, &stem_of(input), &tag, "srt");
        std::fs::write(&out, subtitle::format_srt(&cues))?;
        register_subtitle(&ctx.st, input, &out, &tag);
        Ok(out)
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
