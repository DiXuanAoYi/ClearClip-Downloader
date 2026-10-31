//! 视频摘要接口：给浏览器扩展用的“文稿 → 摘要 / 导图 / 问答 …”。
//! 扩展在网页里取到字幕后发给本机的清影，清影用“设置 → AI”里配好的模型处理，密钥始终留在这里。
//! 长任务用“提交 → 轮询”：`POST /api/v1/ai/run` 返回任务编号，`GET /api/v1/ai/job?id=` 查进度和结果。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::ai::{self, chat, chunk_lines, clock_label, parse_label, parse_summary, Endpoint};
use crate::error::{AppError, AppResult};
use crate::subtitle::Cue;

/// 一次请求最多处理多少字（再长的视频建议先在清影里转写后处理）。
pub const MAX_CHARS: usize = 600_000;
const CHUNK_CHARS: usize = 12_000;
const ASK_CHARS: usize = 12_000;
/// 同时保留多少个任务结果（扩展取走后很快就用不到了）。
const KEEP_JOBS: usize = 48;
const KEEP_FOR: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Line {
    /// 毫秒；文章没有时间时为 0
    #[serde(default)]
    pub t: u64,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Turn {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Req {
    /// summary / mindmap / cards / quotes / ask / translate / rewrite / article
    pub kind: String,
    pub title: String,
    /// 输出语言，直接写进提示词；留空用“设置 → AI”里的目标语言
    pub lang: String,
    /// brief / standard / detailed / plain / study
    pub style: String,
    /// rewrite 的目标形式：xhs / thread / blog / email / notes
    pub format: String,
    pub lines: Vec<Line>,
    /// 文章正文，或 rewrite 的原文
    pub text: String,
    pub question: String,
    pub history: Vec<Turn>,
    /// transcribe：要转写的网页视频地址
    pub url: String,
}

// ---------- 任务表 ----------

struct Job {
    note: String,
    done: usize,
    total: usize,
    result: Option<Value>,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    created: Instant,
}

static JOBS: LazyLock<Mutex<HashMap<u64, Job>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT: AtomicU64 = AtomicU64::new(1);

fn with_job<R>(id: u64, f: impl FnOnce(&mut Job) -> R) -> Option<R> {
    JOBS.lock().ok()?.get_mut(&id).map(f)
}

pub fn job_json(id: u64) -> Option<Value> {
    with_job(id, |j| {
        let state = if j.error.is_some() {
            "error"
        } else if j.result.is_some() {
            "done"
        } else {
            "running"
        };
        json!({"state": state, "done": j.done, "total": j.total, "note": j.note, "result": j.result, "error": j.error})
    })
}

pub fn cancel(id: u64) -> bool {
    with_job(id, |j| j.cancel.store(true, Ordering::Relaxed)).is_some()
}

fn new_job() -> (u64, Arc<AtomicBool>) {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    jobs.retain(|_, j| j.created.elapsed() < KEEP_FOR);
    while jobs.len() >= KEEP_JOBS {
        let Some(oldest) = jobs.iter().min_by_key(|(_, j)| j.created).map(|(k, _)| *k) else { break };
        jobs.remove(&oldest);
    }
    jobs.insert(id, Job { note: String::new(), done: 0, total: 1, result: None, error: None, cancel: cancel.clone(), created: Instant::now() });
    (id, cancel)
}

/// 校验请求并在后台开始处理，返回任务编号。
pub fn start(app: &tauri::AppHandle, req: Req) -> AppResult<u64> {
    use tauri::Manager;
    check(&req)?;
    if req.kind == "transcribe" || req.kind == "subtitles" {
        return start_transcribe(app, req);
    }
    let st = app.state::<Arc<crate::AppState>>().inner().clone();
    let settings = st.settings();
    let ep = ai::chat_endpoint(&st, &settings)?;
    let model = settings.ai.model.clone();
    let default_lang = settings.ai.target_lang.clone();
    let batch = settings.ai.batch_size;
    let (id, cancel) = new_job();
    tauri::async_runtime::spawn(async move {
        let progress = move |done: usize, total: usize| -> AppResult<()> {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::msg("已取消"));
            }
            with_job(id, |j| {
                j.done = done;
                j.total = total.max(1);
            });
            Ok(())
        };
        let out = process(&ep, &model, &default_lang, batch, &req, &progress).await;
        with_job(id, |j| match out {
            Ok(v) => j.result = Some(v),
            Err(e) => j.error = Some(e.message),
        });
    });
    Ok(id)
}

// ---------- 没有字幕时：取音频 → 转写 ----------

/// 把网页视频转写成带时间戳的文稿。作为清影的后台任务运行（任务列表里也看得到、可取消），结果交给扩展。
fn start_transcribe(app: &tauri::AppHandle, req: Req) -> AppResult<u64> {
    use tauri::Manager;
    let st = app.state::<Arc<crate::AppState>>().inner().clone();
    let settings = st.settings();
    let subs_only = req.kind == "subtitles";
    // 先取在线字幕，取不到才转写，所以转写的条件（语音转文字、ffmpeg）等到真要转写时再检查
    if !settings.use_ytdlp || crate::tools::resolve(&st, crate::tools::Tool::YtDlp).is_none() {
        return Err(AppError::new(crate::error::ErrorKind::NeedUpdate, "取网页视频的字幕 / 音频需要 yt-dlp 组件。请在“设置 → 组件”中安装 yt-dlp 后重试。"));
    }
    let (id, cancel) = new_job();
    let title = if req.title.trim().is_empty() { "网页视频".to_string() } else { req.title.trim().chars().take(80).collect() };
    let url = req.url.clone();
    let lang = if req.lang.trim().is_empty() { String::new() } else { req.lang.trim().to_string() };
    crate::media_tools::spawn_job(app, title, if subs_only { "取在线字幕" } else { "视频转写" }, move |ctx| async move {
        // 扩展点了取消 → 取消清影里对应的后台任务；后台任务的进度 → 扩展轮询到的进度
        let watcher = {
            let (st, media_id) = (ctx.st.clone(), ctx.id);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    if cancel.load(Ordering::Relaxed) {
                        st.media_jobs.cancel(media_id);
                    }
                    if let Some(s) = st.media_jobs.snapshot().into_iter().find(|s| s.id == media_id) {
                        with_job(id, |j| {
                            j.done = s.percent.round() as usize;
                            j.total = 100;
                            j.note =
                                if s.status == "queued" { "排队中（清影里还有别的任务在处理）".into() } else { s.note.unwrap_or_default() };
                        });
                    }
                }
            })
        };
        let out = transcribe_page(&ctx, &url, &lang, subs_only).await;
        watcher.abort();
        with_job(id, |j| match &out {
            Ok(v) => j.result = Some(v.clone()),
            Err(e) => j.error = Some(if e.message == "canceled" { "已取消".to_string() } else { e.message.clone() }),
        });
        out.map(|_| std::path::PathBuf::new())
    });
    Ok(id)
}

/// 先用 yt-dlp 取网页视频自带的在线字幕（人工 / 自动），有就直接用；没有且不是“只取字幕”才取音频转写。
async fn transcribe_page(ctx: &crate::media_tools::JobCtx, url: &str, lang: &str, subs_only: bool) -> AppResult<Value> {
    let settings = ctx.st.settings();
    let work = std::env::temp_dir().join(format!("clearclip-vsum-{}-{}", std::process::id(), ctx.id));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let result = async {
        let pctx = ctx.st.parse_ctx(&settings);
        ctx.note("正在取视频的在线字幕…");
        match crate::providers::ytdlp::fetch_subtitles(&pctx, url, &work, || ctx.canceled()).await {
            Ok(found) => {
                if let Some((code, path)) = crate::providers::ytdlp::pick_subtitle(&found, lang) {
                    let cues = crate::subtitle::parse_srt(&std::fs::read_to_string(path)?);
                    let lines = cues_to_lines(&cues);
                    if !lines.is_empty() {
                        let duration = cues.iter().map(|c| c.end).max().unwrap_or(0);
                        return Ok(json!({"lines": lines, "durationMs": duration, "track": format!("在线字幕（{code}）"), "source": "subtitles"}));
                    }
                }
            }
            Err(e) if e.message == "canceled" => return Err(e),
            // 取字幕失败不挡着转写；只取字幕时才把原因告诉用户
            Err(e) if subs_only => return Err(e),
            Err(_) => {}
        }
        ctx.check()?;
        if subs_only {
            return Ok(json!({"lines": [], "durationMs": 0, "track": "", "source": "subtitles"}));
        }
        ai::stt_ready(&ctx.st, &settings)?;
        let ffmpeg = crate::postprocess::find_ffmpeg(&ctx.st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
        let lang = if lang.is_empty() { settings.ai.stt_language.clone() } else { lang.to_string() };
        ctx.note("这个视频没有在线字幕，正在取音频…");
        let audio = crate::providers::ytdlp::fetch_audio(&pctx, url, &work, || ctx.canceled(), |p| ctx.percent(p * 0.3)).await?;
        ctx.check()?;
        let cues = ai::transcribe_cues(ctx, &settings, &ffmpeg, &audio, &lang, &work).await?;
        let lines = cues_to_lines(&cues);
        let duration = cues.iter().map(|c| c.end).max().unwrap_or(0);
        Ok(json!({"lines": lines, "durationMs": duration, "track": "清影转写", "source": "whisper"}))
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn cues_to_lines(cues: &[crate::subtitle::Cue]) -> Vec<Value> {
    cues.iter().map(|c| (c.start, c.lines.join(" ").trim().to_string())).filter(|(_, t)| !t.is_empty()).map(|(t, text)| json!({"t": t, "text": text})).collect()
}

pub fn check(req: &Req) -> AppResult<()> {
    const KINDS: [&str; 10] = ["summary", "mindmap", "cards", "quotes", "ask", "translate", "rewrite", "article", "transcribe", "subtitles"];
    if !KINDS.contains(&req.kind.as_str()) {
        return Err(AppError::invalid("不支持的类型"));
    }
    let size: usize = req.lines.iter().map(|l| l.text.chars().count() + 8).sum::<usize>() + req.text.chars().count();
    if size > MAX_CHARS {
        return Err(AppError::invalid("内容太长，超过了一次能处理的上限。"));
    }
    let needs_lines = matches!(req.kind.as_str(), "summary" | "mindmap" | "cards" | "quotes" | "ask" | "translate");
    if needs_lines && req.lines.iter().all(|l| l.text.trim().is_empty()) {
        return Err(AppError::invalid("没有可处理的字幕。"));
    }
    if matches!(req.kind.as_str(), "article" | "rewrite") && req.text.trim().is_empty() {
        return Err(AppError::invalid("没有可处理的文字。"));
    }
    if matches!(req.kind.as_str(), "transcribe" | "subtitles") && !(req.url.starts_with("http://") || req.url.starts_with("https://")) {
        return Err(AppError::invalid("没有可转写的视频地址。"));
    }
    if req.kind == "ask" && req.question.trim().is_empty() {
        return Err(AppError::invalid("请输入问题。"));
    }
    if req.kind == "rewrite" && rewrite_brief(&req.format).is_none() {
        return Err(AppError::invalid("不支持的改写形式"));
    }
    Ok(())
}

// ---------- 提示词 ----------

fn style_hint(style: &str) -> &'static str {
    match style {
        "brief" => "Keep it very short: summary 1-2 sentences, 3-4 points.",
        "detailed" => "Be thorough: summary 5-8 sentences, 8-15 points that keep concrete names, numbers and examples.",
        "plain" => "Explain in plain everyday words, as if to a curious beginner; avoid jargon or explain it briefly.",
        "study" => "Write like study notes: define key terms, keep the logical order, include formulas or steps when they appear.",
        _ => "summary 2-4 sentences, 4-8 points.",
    }
}

fn summary_system(lang: &str, style: &str, chapters: bool, final_pass: bool) -> String {
    let chap = if chapters && !final_pass {
        " and \"chapters\" (array of {\"time\": \"m:ss\" or \"h:mm:ss\", \"title\": short title}). Chapters must use timestamps that appear in the transcript, start near the beginning, be in order and be 4-12 in number for long videos (fewer for short ones)"
    } else {
        ""
    };
    format!(
        "You summarize video transcripts or articles. Transcript lines look like \"[m:ss] text\". Write in {lang}. {} \
         Reply with ONE JSON object only, no code fences, with keys \"summary\", \"points\" (array of key takeaways), \
         \"keywords\" (array of 3-8 short topic tags){chap}.",
        style_hint(style)
    )
}

fn mindmap_system(lang: &str) -> String {
    format!(
        "You turn video transcripts into a detailed, well-organised mind map. Write in {lang}. Output ONLY a Markdown outline: the first line is \"# \" plus a short title, \
         then nested bullets starting with \"- \" and indented by 2 spaces per level. Use 4 levels: 5-10 top-level branches that follow the video's structure, \
         each with 2-5 sub-branches, and sub-branches get concrete detail (names, numbers, steps, examples, reasons) as child nodes. \
         Every node is a short phrase or one sentence under 24 words; keep specifics instead of vague wording. No other text."
    )
}

fn cards_system(lang: &str) -> String {
    format!(
        "You make study flashcards from a transcript. Lines look like \"[m:ss] text\". Write in {lang}. Reply with ONE JSON object only: \
         {{\"cards\": [{{\"q\": question, \"a\": short answer, \"time\": \"m:ss\" where the answer appears}}]}}. 4-10 cards per part; test understanding, not trivia."
    )
}

fn quotes_system(lang: &str) -> String {
    format!(
        "You pick the most quotable, insightful sentences from a transcript. Lines look like \"[m:ss] text\". Write in {lang} (keep the original wording when it is already in that language). \
         Reply with ONE JSON object only: {{\"quotes\": [{{\"text\": the sentence, lightly cleaned of filler words, \"time\": \"m:ss\"}}]}}. 3-6 quotes per part."
    )
}

fn ask_system(lang: &str) -> String {
    format!(
        "You answer questions about a video using ONLY its transcript excerpts. Lines look like \"[m:ss] text\". Answer in {lang}, concisely. \
         Cite the moments you rely on as [m:ss] (or [h:mm:ss]) right after the claim. If the excerpts do not contain the answer, say so plainly instead of guessing."
    )
}

fn rewrite_brief(format: &str) -> Option<&'static str> {
    Some(match format {
        "xhs" => "a Xiaohongshu-style post: catchy title with emoji, short punchy paragraphs, a few relevant emoji, 5-8 hashtags at the end",
        "thread" => "a social-media thread: 5-10 numbered posts, each under 260 characters, the first one a strong hook",
        "blog" => "a blog post: title, short intro, 3-5 sections with headings, a one-paragraph conclusion",
        "email" => "a short email that shares the key points with a colleague: subject line, greeting, 3-5 bullet points, one suggested next step",
        "notes" => "structured meeting-style notes: topic, key points, decisions or conclusions, action items (omit a section if empty)",
        _ => return None,
    })
}

fn rewrite_system(lang: &str, format: &str) -> String {
    format!(
        "Rewrite the given video summary as {}. Write in {lang}. Use only facts from the input; do not invent details. Output the result only, as Markdown where it helps.",
        rewrite_brief(format).unwrap_or("notes")
    )
}

// ---------- 解析 ----------

/// 回答里的 `[m:ss]` / `[h:mm:ss]` 引用 → 毫秒（按出现顺序，去重）。
pub fn extract_refs(answer: &str) -> Vec<u64> {
    let mut out: Vec<u64> = vec![];
    let mut rest = answer;
    while let Some(a) = rest.find('[') {
        let after = &rest[a + 1..];
        let Some(b) = after.find(']') else { break };
        let label = &after[..b];
        if label.len() <= 8 && label.contains(':') && label.chars().all(|c| c.is_ascii_digit() || c == ':') {
            if let Some(ms) = parse_label(label) {
                if !out.contains(&ms) {
                    out.push(ms);
                }
            }
        }
        rest = &after[b + 1..];
    }
    out
}

fn json_of(resp: &str) -> Option<Value> {
    let t = ai::strip_fences(resp);
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    serde_json::from_str(&t[a..=b]).ok()
}

fn strings(v: Option<&Value>, max: usize) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).take(max).collect())
        .unwrap_or_default()
}

fn time_of(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_str().and_then(parse_label).or_else(|| v.as_u64().map(|n| n * 1000))
}

pub fn parse_keywords(resp: &str) -> Vec<String> {
    json_of(resp).map(|v| strings(v.get("keywords"), 10)).unwrap_or_default()
}

pub fn parse_cards(resp: &str) -> Vec<Value> {
    let Some(v) = json_of(resp) else { return vec![] };
    v.get("cards")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let q = c.get("q").and_then(Value::as_str)?.trim();
                    let ans = c.get("a").and_then(Value::as_str)?.trim();
                    (!q.is_empty() && !ans.is_empty()).then(|| json!({"q": q, "a": ans, "t": time_of(c.get("time"))}))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_quotes(resp: &str) -> Vec<Value> {
    let Some(v) = json_of(resp) else { return vec![] };
    v.get("quotes")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let text = c.get("text").and_then(Value::as_str)?.trim();
                    (!text.is_empty()).then(|| json!({"text": text, "t": time_of(c.get("time"))}))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 导图大纲：保留 `# 标题` 和缩进的 `- ` 列表，丢掉模型夹带的其他文字。至少要有两个节点才算有效。
pub fn clean_outline(resp: &str) -> Option<String> {
    let mut out: Vec<String> = vec![];
    let mut nodes = 0;
    for raw in ai::strip_fences(resp).lines() {
        let line = raw.trim_end().replace('\t', "  ");
        let t = line.trim_start();
        if t.starts_with("# ") && out.is_empty() {
            out.push(t.to_string());
        } else if let Some(text) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            let depth = ((line.len() - t.len()) / 2).min(5);
            if text.trim().is_empty() {
                continue;
            }
            out.push(format!("{}- {}", "  ".repeat(depth), text.trim()));
            nodes += 1;
        }
    }
    (nodes >= 2).then(|| out.join("\n"))
}

// ---------- 文稿处理 ----------

fn fmt_lines(lines: &[Line]) -> Vec<String> {
    lines.iter().filter(|l| !l.text.trim().is_empty()).map(|l| format!("[{}] {}", clock_label(l.t), l.text.trim().replace('\n', " "))).collect()
}

/// 拿关键词在文稿里找最相关的片段：问题里的英文单词和中文二字组合，按重合数给每个窗口打分，取最高的几个并按时间排好。
pub fn relevant_excerpt(lines: &[String], question: &str, budget: usize) -> String {
    let total: usize = lines.iter().map(|l| l.chars().count() + 1).sum();
    if total <= budget {
        return lines.join("\n");
    }
    let terms = tokens(question);
    const WIN: usize = 8;
    let mut windows: Vec<(usize, usize)> = (0..lines.len())
        .step_by(WIN)
        .map(|s| {
            let e = (s + WIN).min(lines.len());
            let text = lines[s..e].join(" ").to_lowercase();
            (terms.iter().filter(|t| text.contains(t.as_str())).count(), s)
        })
        .collect();
    windows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut chosen: Vec<usize> = vec![];
    let mut used = 0usize;
    for (_, s) in windows {
        let e = (s + WIN).min(lines.len());
        let size: usize = lines[s..e].iter().map(|l| l.chars().count() + 1).sum();
        if used + size > budget {
            continue;
        }
        used += size;
        chosen.push(s);
    }
    chosen.sort_unstable();
    chosen.iter().map(|&s| lines[s..(s + WIN).min(lines.len())].join("\n")).collect::<Vec<_>>().join("\n…\n")
}

fn tokens(q: &str) -> Vec<String> {
    let q = q.to_lowercase();
    let mut out: Vec<String> = vec![];
    let mut word = String::new();
    let mut cjk: Vec<char> = vec![];
    let flush_cjk = |cjk: &mut Vec<char>, out: &mut Vec<String>| {
        if cjk.len() == 1 {
            out.push(cjk[0].to_string());
        }
        for w in cjk.windows(2) {
            out.push(w.iter().collect());
        }
        cjk.clear();
    };
    for c in q.chars() {
        if c.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk, &mut out);
            word.push(c);
        } else {
            if word.len() >= 3 {
                out.push(std::mem::take(&mut word));
            }
            word.clear();
            if ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c) {
                cjk.push(c);
            } else {
                flush_cjk(&mut cjk, &mut out);
            }
        }
    }
    if word.len() >= 3 {
        out.push(word);
    }
    flush_cjk(&mut cjk, &mut out);
    out.sort();
    out.dedup();
    out
}

fn merge_unique(parts: impl IntoIterator<Item = Vec<String>>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for k in parts.into_iter().flatten() {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&k)) {
            out.push(k);
        }
    }
    out.truncate(max);
    out
}

type Progress<'a> = &'a (dyn Fn(usize, usize) -> AppResult<()> + Sync);

fn model_err() -> AppError {
    AppError::parser("模型没有按要求返回内容，请换一个模型或稍后重试。")
}

/// 按类型处理，返回给扩展的 JSON。
pub async fn process(ep: &Endpoint, model: &str, default_lang: &str, batch: usize, req: &Req, progress: Progress<'_>) -> AppResult<Value> {
    let lang = if req.lang.trim().is_empty() { default_lang } else { req.lang.trim() };
    let lang = if lang.is_empty() { "简体中文" } else { lang };
    let lines = fmt_lines(&req.lines);
    match req.kind.as_str() {
        "summary" | "article" => {
            let article = req.kind == "article";
            let lines = if article { req.text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect() } else { lines };
            let chunks = chunk_lines(&lines, CHUNK_CHARS);
            if chunks.is_empty() {
                return Err(AppError::invalid("没有可处理的内容。"));
            }
            let system = summary_system(lang, &req.style, !article, false);
            let n = chunks.len() + usize::from(chunks.len() > 1);
            let mut parts: Vec<(ai::Summary, Vec<String>)> = vec![];
            for (i, c) in chunks.iter().enumerate() {
                progress(i, n)?;
                let resp = chat(ep, model, &system, c).await?;
                parts.push((parse_summary(&resp).ok_or_else(model_err)?, parse_keywords(&resp)));
            }
            let (mut sum, keywords) = if parts.len() == 1 {
                let (s, k) = parts.remove(0);
                (s, k)
            } else {
                progress(chunks.len(), n)?;
                let joined = parts
                    .iter()
                    .enumerate()
                    .map(|(i, (p, _))| format!("Part {}: {}\n{}", i + 1, p.summary, p.points.join("; ")))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let resp = chat(ep, model, &summary_system(lang, &req.style, false, true), &joined).await?;
                let merged = parse_summary(&resp).unwrap_or_default();
                let mut kw = parse_keywords(&resp);
                if kw.is_empty() {
                    kw = merge_unique(parts.iter().map(|(_, k)| k.clone()), 8);
                }
                let mut chapters: Vec<crate::model::Chapter> = parts.into_iter().flat_map(|(p, _)| p.chapters).collect();
                chapters.sort_by_key(|c| c.start_ms);
                chapters.dedup_by_key(|c| c.start_ms);
                (ai::Summary { summary: merged.summary, points: merged.points, chapters }, kw)
            };
            let total = req.lines.iter().map(|l| l.t).max().unwrap_or(0) + 1000;
            sum.chapters = ai::close_chapters(sum.chapters, total);
            progress(n, n)?;
            Ok(json!({"summary": sum.summary, "points": sum.points, "chapters": sum.chapters, "keywords": keywords}))
        }
        "mindmap" => {
            let chunks = chunk_lines(&lines, CHUNK_CHARS);
            let n = chunks.len() + usize::from(chunks.len() > 1);
            let system = mindmap_system(lang);
            let mut outlines = vec![];
            for (i, c) in chunks.iter().enumerate() {
                progress(i, n)?;
                outlines.push(chat(ep, model, &system, c).await?);
            }
            let text = if outlines.len() == 1 {
                outlines.remove(0)
            } else {
                progress(chunks.len(), n)?;
                let joined = outlines.iter().enumerate().map(|(i, o)| format!("Part {}:\n{o}", i + 1)).collect::<Vec<_>>().join("\n\n");
                chat(ep, model, &format!("{} The input is several partial outlines of one video; merge them into one.", mindmap_system(lang)), &joined).await?
            };
            progress(n, n)?;
            Ok(json!({"outline": clean_outline(&text).ok_or_else(model_err)?}))
        }
        "cards" | "quotes" => {
            let cards = req.kind == "cards";
            let chunks = chunk_lines(&lines, CHUNK_CHARS);
            let system = if cards { cards_system(lang) } else { quotes_system(lang) };
            let mut items: Vec<Value> = vec![];
            for (i, c) in chunks.iter().enumerate() {
                progress(i, chunks.len())?;
                let resp = chat(ep, model, &system, c).await?;
                items.extend(if cards { parse_cards(&resp) } else { parse_quotes(&resp) });
            }
            progress(chunks.len(), chunks.len())?;
            if items.is_empty() {
                return Err(model_err());
            }
            items.truncate(if cards { 30 } else { 18 });
            let mut out = serde_json::Map::new();
            out.insert(if cards { "cards" } else { "quotes" }.into(), Value::Array(items));
            Ok(Value::Object(out))
        }
        "ask" => {
            progress(0, 1)?;
            let excerpt = relevant_excerpt(&lines, &req.question, ASK_CHARS);
            let mut user = format!("Transcript excerpts:\n{excerpt}\n\n");
            let recent: Vec<&Turn> = req.history.iter().filter(|t| !t.text.trim().is_empty()).rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
            if !recent.is_empty() {
                user.push_str("Conversation so far:\n");
                for t in recent {
                    user.push_str(&format!("{}: {}\n", if t.role == "user" { "User" } else { "Assistant" }, t.text.trim()));
                }
                user.push('\n');
            }
            user.push_str(&format!("Question: {}", req.question.trim()));
            let answer = chat(ep, model, &ask_system(lang), &user).await?;
            progress(1, 1)?;
            let answer = answer.trim().to_string();
            Ok(json!({"refs": extract_refs(&answer), "answer": answer}))
        }
        "translate" => {
            let cues: Vec<Cue> = req
                .lines
                .iter()
                .filter(|l| !l.text.trim().is_empty())
                .map(|l| Cue { start: l.t, end: l.t + 1, lines: vec![l.text.trim().to_string()] })
                .collect();
            let t = ai::translate_cues(ep, model, lang, &cues, batch, progress).await?;
            let out: Vec<Value> =
                cues.iter().zip(&t.texts).map(|(c, tr)| json!({"t": c.start, "text": tr.clone().unwrap_or_else(|| c.lines.join(" "))})).collect();
            Ok(json!({"lines": out, "failed": t.failed}))
        }
        "rewrite" => {
            progress(0, 1)?;
            let text = chat(ep, model, &rewrite_system(lang, &req.format), req.text.trim()).await?;
            progress(1, 1)?;
            Ok(json!({"text": text.trim()}))
        }
        _ => Err(AppError::invalid("不支持的类型")),
    }
}

/// 扩展用来判断“能不能用”：是否填好了模型、能不能转写。
pub fn status(st: &crate::AppState) -> Value {
    let s = st.settings();
    let ready = ai::chat_endpoint(st, &s).is_ok();
    json!({
        "ready": ready,
        "model": s.ai.model,
        "lang": s.ai.target_lang,
        "transcribe": ai::stt_ready(st, &s).is_ok(),
        "subtitles": s.use_ytdlp && crate::tools::resolve(st, crate::tools::Tool::YtDlp).is_some(),
        "maxChars": MAX_CHARS,
    })
}

#[cfg(test)]
#[path = "vsum_tests.rs"]
mod tests;
