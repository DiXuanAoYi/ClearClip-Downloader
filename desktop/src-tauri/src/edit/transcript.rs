//! 词级文稿：把一段素材的语音转成“带时间的词”，给文稿剪辑、口头禅、智能字幕用。
//! 识别用设置里已有的语音转文字（本机 whisper.cpp 或兼容 OpenAI 的接口）；这里只负责取词级时间、解析、缓存。
//! 时间都是素材自己的时间（毫秒），和片段的入点 / 出点同一把尺。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use crate::ai;
use crate::error::{AppError, AppResult, ErrorKind};
use crate::postprocess::{base_args, run_ffmpeg_in};
use crate::settings::Settings;
use crate::track::cmds::{begin, end, need_ffmpeg, EVT_TRACK};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// 一次最多转写这么长（毫秒）
const MAX_SPAN_MS: u64 = 3 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    /// 这个词的文字；西文词前面带着空格，所以把所有词接起来就是原文
    pub t: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub words: Vec<Word>,
    /// 识别出的词是不是真的带词级时间（false 表示只有整句时间，词是按字数摊出来的）
    pub word_level: bool,
    pub engine: String,
    pub from_ms: u64,
    pub to_ms: u64,
}

// ---------- 摊词：把一段文字按字数分到一段时间里 ----------

fn is_cjk(c: char) -> bool {
    c.is_alphanumeric() && matches!(c as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF66..=0xFF9F | 0xAC00..=0xD7AF | 0x20000..=0x2FFFF)
}

fn is_punct(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace()
}

/// 把文字拆成“词”：中日韩每个字一个，西文按空格分（空格留在词前面），标点跟在前一个词后面。
pub fn split_units(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut pending_space = String::new();
    let mut in_latin = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space.push(c);
            in_latin = false;
        } else if is_cjk(c) {
            out.push(format!("{pending_space}{c}"));
            pending_space.clear();
            in_latin = false;
        } else if in_latin || (is_punct(c) && !out.is_empty() && pending_space.is_empty()) {
            // 西文词里的字母，或者紧跟在前一个词后面的标点
            out.last_mut().unwrap().push(c);
        } else {
            out.push(format!("{pending_space}{c}"));
            pending_space.clear();
            in_latin = true;
        }
    }
    out
}

fn weight(unit: &str) -> f64 {
    unit.trim().chars().filter(|c| !is_punct(*c)).count().max(1) as f64
}

/// 把一句话的文字按字数摊到 `[start, end]` 里。
pub fn spread(text: &str, start_ms: u64, end_ms: u64) -> Vec<Word> {
    let units = split_units(text);
    if units.is_empty() {
        return vec![];
    }
    let end_ms = end_ms.max(start_ms + units.len() as u64);
    let total: f64 = units.iter().map(|u| weight(u)).sum();
    let span = (end_ms - start_ms) as f64;
    let mut acc = 0.0;
    units
        .into_iter()
        .map(|u| {
            let s = start_ms + (acc / total * span).round() as u64;
            acc += weight(&u);
            let e = start_ms + (acc / total * span).round() as u64;
            Word { t: u, start_ms: s, end_ms: e.max(s + 1) }
        })
        .collect()
}

fn is_noise(text: &str) -> bool {
    let t = text.trim();
    t.is_empty() || (t.starts_with('[') && t.ends_with(']')) || (t.starts_with('(') && t.ends_with(')') && t.chars().count() > 2) || t.starts_with("<|")
}

// ---------- 解析 ----------

/// whisper.cpp 的 `-oj` 输出：`transcription[]`，每项 `offsets.from / to`（毫秒）和 `text`。
/// 逐词输出时每项就是一个词；没能逐词输出（比如中文没有空格）的整句，按字数摊开。
pub fn parse_whisper_json(bytes: &[u8], offset_ms: u64) -> Option<(Vec<Word>, bool)> {
    let text = String::from_utf8_lossy(bytes);
    let v: Value = serde_json::from_str(&text).ok()?;
    let list = v.get("transcription")?.as_array()?;
    let mut words: Vec<Word> = vec![];
    let mut single = 0usize;
    let mut entries = 0usize;
    for e in list {
        let raw = e.get("text").and_then(Value::as_str).unwrap_or("");
        let from = e.pointer("/offsets/from").and_then(Value::as_u64);
        let to = e.pointer("/offsets/to").and_then(Value::as_u64);
        let (Some(from), Some(to)) = (from, to) else { continue };
        // 逐词输出时，半个汉字（不完整的 UTF-8）会变成替换字符：去掉，把时间并给前一个词
        let cleaned: String = raw.chars().filter(|c| *c != '\u{FFFD}').collect();
        if is_noise(&cleaned) {
            if cleaned.trim().is_empty() && raw.contains('\u{FFFD}') {
                if let Some(last) = words.last_mut() {
                    last.end_ms = last.end_ms.max(to + offset_ms);
                }
            }
            continue;
        }
        entries += 1;
        let pieces = spread(&cleaned, from + offset_ms, to + offset_ms);
        if pieces.len() == 1 {
            single += 1;
        }
        words.extend(pieces);
    }
    if words.is_empty() {
        return None;
    }
    Some((words, entries > 0 && single * 10 >= entries * 8))
}

/// OpenAI 兼容接口的 `verbose_json`：优先用 `words[]`，没有就用 `segments[]` 摊开。
pub fn parse_verbose_json(text: &str, offset_ms: u64) -> Option<(Vec<Word>, bool)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let ms = |x: &Value| (x.as_f64().unwrap_or(0.0).max(0.0) * 1000.0).round() as u64 + offset_ms;
    if let Some(list) = v.get("words").and_then(Value::as_array).filter(|l| !l.is_empty()) {
        let mut out: Vec<Word> = vec![];
        for w in list {
            let raw = w.get("word").and_then(Value::as_str).unwrap_or("");
            if is_noise(raw) {
                continue;
            }
            let (s, e) = (ms(&w["start"]), ms(&w["end"]));
            let word = raw.trim();
            // 接口给的西文词没有前导空格：和前一个词都是西文时补一个
            let latin_pair = out.last().is_some_and(|p| {
                p.t.chars().last().is_some_and(|c| !is_cjk(c) && !c.is_whitespace()) && word.chars().next().is_some_and(|c| !is_cjk(c) && !is_punct(c))
            });
            let t = if latin_pair { format!(" {word}") } else { word.to_string() };
            out.push(Word { t, start_ms: s, end_ms: e.max(s + 1) });
        }
        if !out.is_empty() {
            return Some((out, true));
        }
    }
    let mut out: Vec<Word> = vec![];
    for s in v.get("segments").and_then(Value::as_array).into_iter().flatten() {
        let t = s.get("text").and_then(Value::as_str).unwrap_or("");
        if is_noise(t) {
            continue;
        }
        out.extend(spread(t, ms(&s["start"]), ms(&s["end"])));
    }
    if out.is_empty() {
        // 只有整段文字
        let t = v.get("text").and_then(Value::as_str)?;
        let end = v.get("duration").map(ms).unwrap_or(offset_ms + 1000);
        out = spread(t, offset_ms, end);
    }
    (!out.is_empty()).then_some((out, false))
}

// ---------- 识别 ----------

/// whisper.cpp 的命令行：JSON 输出，逐词（`-ml 1`）。中日韩没有空格，按词切会把整句当一个词，所以不加 `-sow`，让它按 token 切。
pub fn whisper_word_args(model: &str, wav: &Path, out_prefix: &Path, language: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-m".into(),
        model.into(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-l".into(),
        if language.is_empty() { "auto".into() } else { language.into() },
    ];
    a.extend(["-oj".into(), "-of".into(), out_prefix.to_string_lossy().into_owned(), "-ml".into(), "1".into(), "-pp".into()]);
    let cjk = ["zh", "ja", "ko", "yue"].iter().any(|l| language.starts_with(l));
    if !cjk {
        a.push("-sow".into());
    }
    a
}

/// 取出 `[from, to]` 的声音（单声道 16 kHz WAV）。
async fn extract_wav(ffmpeg: &Path, input: &Path, from_ms: u64, to_ms: u64, out: &Path) -> AppResult<()> {
    let mut a = base_args();
    a.extend([
        "-ss".to_string(),
        format!("{:.3}", from_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", (to_ms - from_ms) as f64 / 1000.0),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-map".into(),
        "0:a:0".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        out.to_string_lossy().into_owned(),
    ]);
    run_ffmpeg_in(ffmpeg, &a, None).await.map_err(|e| {
        if e.message.contains("Stream map") || e.message.contains("matches no streams") {
            AppError::invalid("这个素材里没有声音，没法生成文稿。")
        } else {
            e
        }
    })
}

async fn run_whisper(
    settings: &Settings,
    wav: &Path,
    work: &Path,
    lang: &str,
    offset_ms: u64,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> AppResult<(Vec<Word>, bool)> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let bin = ai::find_whisper_bin(settings)
        .ok_or_else(|| AppError::invalid("找不到 whisper.cpp（whisper-cli）。请在“设置 → AI”里指定它的位置，或把它放进系统 PATH。"))?;
    let model = settings.ai.whisper_model.trim();
    if model.is_empty() || !Path::new(model).is_file() {
        return Err(AppError::invalid("请在“设置 → AI”里选择 whisper.cpp 的模型文件（ggml-*.bin）。"));
    }
    let prefix = work.join("out");
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args(whisper_word_args(model, wav, &prefix, lang))
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
                if let Some(p) = ai::whisper_progress(&line) {
                    progress(p);
                } else {
                    tail = line;
                }
            }
        }
        tail
    };
    let cancelled = async {
        while !cancel.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    };
    let status = tokio::select! {
        r = async { let tail = reader.await; (child.wait().await, tail) } => r,
        _ = cancelled => return Err(AppError::msg("canceled")),
    };
    match status {
        (Ok(s), _) if s.success() => {
            let bytes = std::fs::read(prefix.with_extension("json")).map_err(|_| AppError::msg("whisper.cpp 没有生成结果文件。"))?;
            parse_whisper_json(&bytes, offset_ms).ok_or_else(|| AppError::msg("没有识别出任何语音。"))
        }
        (_, tail) => Err(AppError::msg(format!("whisper.cpp 运行失败：{tail}"))),
    }
}

async fn api_chunk(ep: &ai::Endpoint, model: &str, language: &str, file: &Path) -> AppResult<String> {
    let bytes = std::fs::read(file)?;
    let boundary = format!("----clearclip{:x}", crate::db::now() as u64 ^ (bytes.len() as u64).rotate_left(17));
    let mut fields = vec![
        ("model", model),
        ("response_format", "verbose_json"),
        ("temperature", "0"),
        ("timestamp_granularities[]", "word"),
        ("timestamp_granularities[]", "segment"),
    ];
    if !language.is_empty() {
        fields.push(("language", language));
    }
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "audio.m4a".into());
    let body = ai::multipart(&fields, "file", &name, "audio/mp4", &bytes, &boundary);
    ai::send(ep, "/audio/transcriptions", &format!("multipart/form-data; boundary={boundary}"), body, Duration::from_secs(600)).await
}

#[allow(clippy::too_many_arguments)]
async fn run_api(
    st: &AppState,
    settings: &Settings,
    ffmpeg: &Path,
    wav: &Path,
    work: &Path,
    lang: &str,
    offset_ms: u64,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> AppResult<(Vec<Word>, bool)> {
    let ep = ai::stt_endpoint(st, settings)?;
    if settings.ai.stt_model.is_empty() {
        return Err(AppError::invalid("请先在“设置 → AI”里填写语音转文字的模型名称（OpenAI 是 whisper-1）。"));
    }
    let parts = ai::split_audio(ffmpeg, wav, &work.join("parts"), 600).await?;
    let mut words: Vec<Word> = vec![];
    let mut level = true;
    for (i, (file, off)) in parts.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::msg("canceled"));
        }
        progress(i as f64 / parts.len() as f64 * 100.0);
        let text = api_chunk(&ep, &settings.ai.stt_model, lang, file).await?;
        match parse_verbose_json(&text, offset_ms + off) {
            Some((w, l)) => {
                level &= l;
                words.extend(w);
            }
            None => {
                // 接口不认 verbose_json（或这一段没有语音）：有语音但解析不了就报错
                if text.trim_start().starts_with('{') && !text.contains("\"text\":\"\"") {
                    return Err(AppError::parser("接口返回的结果里没有时间信息。请确认它支持 response_format=verbose_json（OpenAI、Groq 等支持）。"));
                }
            }
        }
    }
    Ok((words, level))
}

fn cache_key(path: &str, from: u64, to: u64, lang: &str, engine: &str, model: &str) -> String {
    let meta = std::fs::metadata(path).ok();
    let size = meta.as_ref().map_or(0, |m| m.len());
    let mtime = meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    crate::media_cmds::short_hash(&format!("{path}|{size}|{mtime}|{from}|{to}|{lang}|{engine}|{model}"))
}

fn engine_label(s: &Settings) -> String {
    if s.ai.stt_engine == "local" {
        "whisper.cpp".into()
    } else {
        s.ai.stt_model.clone()
    }
}

/// 转写 `path` 的 `[from_ms, to_ms]`。结果按素材 + 范围 + 语言 + 引擎缓存在数据目录里。
#[allow(clippy::too_many_arguments)]
pub async fn transcribe(
    st: &AppState,
    path: &str,
    from_ms: u64,
    to_ms: u64,
    language: Option<String>,
    force: bool,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f64) + Sync),
) -> AppResult<Transcript> {
    if !Path::new(path).is_file() {
        return Err(AppError::invalid("找不到这个素材文件。"));
    }
    if to_ms <= from_ms || to_ms - from_ms > MAX_SPAN_MS {
        return Err(AppError::invalid("要转写的范围不对（最长 3 小时）。"));
    }
    let settings = st.settings();
    let lang = language.unwrap_or_else(|| settings.ai.stt_language.clone());
    let engine = engine_label(&settings);
    let dir = st.data_dir.join("transcripts");
    let key = cache_key(path, from_ms, to_ms, &lang, &settings.ai.stt_engine, &engine);
    let cache = dir.join(format!("{key}.json"));
    if !force {
        if let Some(t) = std::fs::read_to_string(&cache).ok().and_then(|s| serde_json::from_str::<Transcript>(&s).ok()) {
            return Ok(t);
        }
    }
    let ffmpeg = need_ffmpeg(st)?;
    let work = std::env::temp_dir().join(format!("clearclip-tr-{}-{key}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let result = async {
        let wav = work.join("audio.wav");
        extract_wav(&ffmpeg, Path::new(path), from_ms, to_ms, &wav).await?;
        let (words, word_level) = if settings.ai.stt_engine == "local" {
            run_whisper(&settings, &wav, &work, &lang, from_ms, cancel, progress).await?
        } else {
            run_api(st, &settings, &ffmpeg, &wav, &work, &lang, from_ms, cancel, progress).await?
        };
        if words.is_empty() {
            return Err(AppError::msg("没有识别出任何语音。"));
        }
        let t = Transcript { words, word_level, engine, from_ms, to_ms };
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&cache, serde_json::to_vec(&t).unwrap_or_default());
        Ok(t)
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

// ---------- 命令 ----------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeReq {
    pub id: u64,
    pub path: String,
    pub from_ms: u64,
    pub to_ms: u64,
    pub language: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[tauri::command]
pub async fn edit_transcribe(app: AppHandle, state: St<'_>, req: TranscribeReq) -> AppResult<Transcript> {
    let id = req.id;
    let cancel = begin(id);
    let app2 = app.clone();
    let progress = move |p: f64| {
        let _ = app2.emit(EVT_TRACK, serde_json::json!({ "id": id, "percent": p.round() }));
    };
    let res = transcribe(&state, &req.path, req.from_ms, req.to_ms, req.language, req.force, &cancel, &progress).await;
    end(id);
    let _ = app.emit(EVT_TRACK, serde_json::json!({ "id": id, "percent": 100 }));
    res
}

/// 把字幕文字翻译成目标语言（用“设置 → AI”里配置的对话模型），双语字幕用。译不出来的条目返回空。
/// 文字会发给你配置的接口，所以界面上默认关闭、勾选才会调用。
#[tauri::command]
pub async fn edit_translate_lines(state: St<'_>, lines: Vec<String>, language: String) -> AppResult<Vec<Option<String>>> {
    if lines.is_empty() {
        return Ok(vec![]);
    }
    if lines.len() > 2000 {
        return Err(AppError::invalid("一次最多翻译 2000 条字幕。"));
    }
    let settings = state.settings();
    let ep = ai::chat_endpoint(&state, &settings)?;
    let cues: Vec<crate::subtitle::Cue> = lines
        .iter()
        .enumerate()
        .map(|(i, t)| crate::subtitle::Cue { start: i as u64, end: i as u64 + 1, lines: t.split('\n').map(str::to_string).collect() })
        .collect();
    let done = ai::translate_cues(&ep, &settings.ai.model, &language, &cues, 20, &|_, _| Ok(())).await?;
    Ok(done.texts)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SttStatus {
    pub ready: bool,
    pub engine: String,
    /// 没准备好时，缺什么
    pub reason: String,
}

#[tauri::command]
pub fn edit_stt_status(state: St<'_>) -> SttStatus {
    let s = state.settings();
    let engine = s.ai.stt_engine.clone();
    let reason = if engine == "local" {
        if !ai::whisper_available(&s) {
            "找不到 whisper.cpp，请在“设置 → AI”里指定 whisper-cli 的位置。"
        } else if s.ai.whisper_model.trim().is_empty() || !Path::new(s.ai.whisper_model.trim()).is_file() {
            "请在“设置 → AI”里选择 whisper.cpp 的模型文件（ggml-*.bin）。"
        } else {
            ""
        }
    } else if s.ai.stt_url().is_empty() || s.ai.stt_model.is_empty() {
        "请先在“设置 → AI”里填写语音转文字的接口地址和模型名称。"
    } else {
        ""
    };
    SttStatus { ready: reason.is_empty(), engine, reason: reason.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_split_cjk_per_char_and_latin_per_word() {
        assert_eq!(split_units("你好，世界。"), vec!["你", "好，", "世", "界。"]);
        assert_eq!(split_units(" Hello, world!"), vec![" Hello,", " world!"]);
        assert_eq!(split_units("用 Rust 写"), vec!["用", " Rust", " 写"]);
        assert!(split_units("  ").is_empty());
    }

    #[test]
    fn spreading_keeps_the_text_and_covers_the_span() {
        let w = spread("今天天气不错", 1000, 4000);
        assert_eq!(w.iter().map(|x| x.t.as_str()).collect::<String>(), "今天天气不错");
        assert_eq!(w.first().unwrap().start_ms, 1000);
        assert_eq!(w.last().unwrap().end_ms, 4000);
        assert!(w.windows(2).all(|p| p[0].end_ms <= p[1].start_ms + 1 && p[0].start_ms < p[0].end_ms));
        let e = spread(" a bb", 0, 3000);
        assert_eq!((e[0].start_ms, e[0].end_ms, e[1].end_ms), (0, 1000, 3000));
    }

    #[test]
    fn whisper_json_word_level_with_offsets_and_noise() {
        let j = r#"{"transcription":[
            {"offsets":{"from":0,"to":400},"text":" [_BEG_]"},
            {"offsets":{"from":400,"to":900},"text":" Hello"},
            {"offsets":{"from":900,"to":1500},"text":" world."},
            {"offsets":{"from":1500,"to":2000},"text":" [BLANK_AUDIO]"}]}"#;
        let (w, level) = parse_whisper_json(j.as_bytes(), 10_000).unwrap();
        assert!(level);
        assert_eq!(w.len(), 2);
        assert_eq!((w[0].t.as_str(), w[0].start_ms, w[0].end_ms), (" Hello", 10_400, 10_900));
        assert_eq!(w[1].t, " world.");
    }

    #[test]
    fn whisper_json_sentence_level_is_spread_and_broken_bytes_are_dropped() {
        let j = r#"{"transcription":[{"offsets":{"from":0,"to":3000},"text":"今天天气不错"}]}"#;
        let (w, level) = parse_whisper_json(j.as_bytes(), 0).unwrap();
        assert!(!level);
        assert_eq!(w.len(), 6);
        // 半个汉字被截断成非法 UTF-8：不能让整个解析失败
        let mut raw = r#"{"transcription":[{"offsets":{"from":0,"to":500},"text":"你"},{"offsets":{"from":500,"to":700},"text":""#.as_bytes().to_vec();
        raw.extend([0xE5, 0xA5]);
        raw.extend(r#""},{"offsets":{"from":700,"to":1200},"text":"好"}]}"#.as_bytes());
        let (w, _) = parse_whisper_json(&raw, 0).unwrap();
        assert_eq!(w.iter().map(|x| x.t.as_str()).collect::<String>(), "你好");
        assert_eq!(w[0].end_ms, 700);
        assert!(parse_whisper_json(b"not json", 0).is_none());
    }

    #[test]
    fn verbose_json_prefers_words_and_adds_spaces_between_latin_words() {
        let j = r#"{"text":"Hello world","words":[{"word":"Hello","start":0.1,"end":0.5},{"word":"world","start":0.6,"end":1.0}],"segments":[]}"#;
        let (w, level) = parse_verbose_json(j, 2000).unwrap();
        assert!(level);
        assert_eq!(w[0].t, "Hello");
        assert_eq!(w[1].t, " world");
        assert_eq!((w[1].start_ms, w[1].end_ms), (2600, 3000));
    }

    #[test]
    fn verbose_json_falls_back_to_segments_then_plain_text() {
        let seg = r#"{"segments":[{"start":0.0,"end":2.0,"text":"你好吗"},{"start":2.0,"end":2.5,"text":" [Music]"}]}"#;
        let (w, level) = parse_verbose_json(seg, 0).unwrap();
        assert!(!level);
        assert_eq!(w.len(), 3);
        let plain = r#"{"text":"Hi there","duration":2.0}"#;
        let (w, _) = parse_verbose_json(plain, 0).unwrap();
        assert_eq!(w.len(), 2);
        assert!(parse_verbose_json(r#"{"text":""}"#, 0).is_none());
    }

    #[test]
    fn whisper_args_split_on_word_only_for_spaced_languages() {
        let a = whisper_word_args("m.bin", Path::new("a.wav"), Path::new("out"), "en");
        assert!(a.contains(&"-sow".to_string()) && a.windows(2).any(|w| w == ["-ml", "1"]));
        let z = whisper_word_args("m.bin", Path::new("a.wav"), Path::new("out"), "zh");
        assert!(!z.contains(&"-sow".to_string()));
        assert!(whisper_word_args("m.bin", Path::new("a.wav"), Path::new("out"), "").contains(&"auto".to_string()));
    }
}
