//! 中文语音识别：SenseVoice（阿里 FunASR 开源，中 / 英 / 日 / 韩 / 粤）+ sherpa-onnx 命令行。
//!
//! 比 Whisper 快得多（int8 模型在普通 CPU 上通常是实时的几倍到十几倍），中文准确率也更高，自带标点。
//! 做法：用 sherpa-onnx 的 `sherpa-onnx-vad-with-offline-asr`——先用 Silero VAD 把音频切成一句一句，
//! 再逐句识别，输出 `起点 -- 终点: 文字`，正好就是带时间的字幕。
//!
//! 需要三样东西，都放在数据目录的 `sensevoice/` 下（便携版会随文件夹搬家）：
//! 运行库（Windows 一键下载官方包；macOS / Linux 需要自行安装，装好放进 PATH 后点“重新检测”）、
//! 模型 `model.int8.onnx` + `tokens.txt`、VAD 模型 `silero_vad.onnx`。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::subtitle::Cue;
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// 设置里“本地语音模型”的取值
pub const MODEL_ID: &str = "sensevoice";
/// 进度事件沿用模型下载的通道，用这个 id 区分。
pub const ID: &str = "sensevoice";

const RUNTIME_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.12.20/sherpa-onnx-v1.12.20-win-x64-shared.tar.bz2";
const MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2";
const VAD_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
const MODEL_FILE: &str = "model.int8.onnx";
const TOKENS_FILE: &str = "tokens.txt";
const VAD_FILE: &str = "silero_vad.onnx";

pub fn bin_name() -> &'static str {
    if cfg!(windows) {
        "sherpa-onnx-vad-with-offline-asr.exe"
    } else {
        "sherpa-onnx-vad-with-offline-asr"
    }
}

pub fn dir_of(data_dir: &Path) -> PathBuf {
    data_dir.join("sensevoice")
}

fn runtime_dir(data_dir: &Path) -> PathBuf {
    dir_of(data_dir).join("runtime")
}

/// 识别要用的全部文件。
#[derive(Debug, Clone)]
pub struct Files {
    pub bin: PathBuf,
    pub model: PathBuf,
    pub tokens: PathBuf,
    pub vad: PathBuf,
}

/// 运行程序：清影自己装的优先，其次 PATH 里的。
pub fn find_bin(data_dir: &Path) -> Option<PathBuf> {
    let managed = runtime_dir(data_dir).join(bin_name());
    if managed.is_file() {
        return Some(managed);
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe"] } else { &[""] };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|d| exts.iter().map(|e| d.join(format!("sherpa-onnx-vad-with-offline-asr{e}"))).find(|p| p.is_file()))
}

pub fn find_files(data_dir: &Path) -> Option<Files> {
    let d = dir_of(data_dir);
    let f = Files { bin: find_bin(data_dir)?, model: d.join(MODEL_FILE), tokens: d.join(TOKENS_FILE), vad: d.join(VAD_FILE) };
    (f.model.is_file() && f.tokens.is_file() && f.vad.is_file()).then_some(f)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SenseVoiceStatus {
    /// 运行程序找到了吗
    pub runtime: bool,
    pub runtime_path: Option<String>,
    /// 模型（含词表和 VAD）都在吗
    pub model: bool,
    pub ready: bool,
    /// 这个系统能一键下载运行库吗（目前只有 Windows x64）
    pub auto_install: bool,
    pub note: String,
}

pub fn status_of(data_dir: &Path) -> SenseVoiceStatus {
    let d = dir_of(data_dir);
    let bin = find_bin(data_dir);
    let model = d.join(MODEL_FILE).is_file() && d.join(TOKENS_FILE).is_file() && d.join(VAD_FILE).is_file();
    let auto = cfg!(all(windows, target_arch = "x86_64"));
    let note = if bin.is_some() && model {
        String::new()
    } else if auto {
        "一键下载：运行库约 25 MB，模型约 160 MB，都只在本机运行。".to_string()
    } else {
        "这个系统没有官方现成的单文件包：请先安装 sherpa-onnx（pip install sherpa-onnx，或从它的发布页下载），让 sherpa-onnx-vad-with-offline-asr 能在 PATH 里找到，再点“下载模型”。".to_string()
    };
    SenseVoiceStatus {
        runtime: bin.is_some(),
        runtime_path: bin.map(|p| p.to_string_lossy().into_owned()),
        model,
        ready: find_files(data_dir).is_some(),
        auto_install: auto,
        note,
    }
}

#[tauri::command]
pub fn sensevoice_status(state: St<'_>) -> SenseVoiceStatus {
    status_of(&state.data_dir)
}

/// 去掉 SenseVoice 偶尔带出来的 `<|zh|><|NEUTRAL|>` 一类标记。
pub fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("<|") {
        out.push_str(&rest[..i]);
        match rest[i..].find("|>") {
            Some(j) => rest = &rest[i + j + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// 解析 sherpa-onnx 的输出：每句一行，`0.352 -- 3.168: 文字`（秒）。其它行（日志、用时统计）都忽略。
pub fn parse_segments(output: &str) -> Vec<Cue> {
    let mut cues = vec![];
    for line in output.lines() {
        let line = line.trim();
        let Some((range, text)) = line.split_once(':') else { continue };
        let Some((a, b)) = range.split_once("--") else { continue };
        let (Ok(a), Ok(b)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) else { continue };
        let text = strip_tags(text);
        if text.is_empty() || b < a {
            continue;
        }
        cues.push(Cue { start: (a * 1000.0).round() as u64, end: ((b * 1000.0).round() as u64).max((a * 1000.0).round() as u64 + 200), lines: vec![text] });
    }
    cues
}

/// 音频语言：清影里的代码 → SenseVoice 认的代码（auto / zh / en / ja / ko / yue）。
pub fn lang_arg(lang: &str) -> &'static str {
    match lang.trim().to_lowercase().split(['-', '_']).next().unwrap_or("") {
        "zh" | "cmn" | "chi" => "zh",
        "en" => "en",
        "ja" | "jp" => "ja",
        "ko" => "ko",
        "yue" => "yue",
        _ => "auto",
    }
}

pub fn args(f: &Files, wav: &Path, lang: &str, threads: usize) -> Vec<String> {
    vec![
        format!("--silero-vad-model={}", f.vad.display()),
        "--silero-vad-threshold=0.5".into(),
        "--silero-vad-min-silence-duration=0.35".into(),
        "--silero-vad-min-speech-duration=0.25".into(),
        "--silero-vad-max-speech-duration=10".into(),
        format!("--sense-voice-model={}", f.model.display()),
        format!("--tokens={}", f.tokens.display()),
        "--sense-voice-use-itn=true".into(),
        format!("--sense-voice-language={}", lang_arg(lang)),
        format!("--num-threads={}", threads.max(1)),
        wav.to_string_lossy().into_owned(),
    ]
}

/// 在子进程里识别一个 16 kHz 单声道 wav，返回带时间的句子。
pub async fn recognize(files: &Files, wav: &Path, lang: &str, canceled: impl Fn() -> bool) -> AppResult<Vec<Cue>> {
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().min(8));
    let mut cmd = tokio::process::Command::new(&files.bin);
    cmd.args(args(files, wav, lang, threads))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // 动态库和程序在同一个目录（Windows 默认就会去那里找；Linux / macOS 的包把库放在上一级的 lib/）
    if let Some(dir) = files.bin.parent() {
        #[cfg(target_os = "linux")]
        if let Some(lib) = dir.parent().map(|p| p.join("lib")).filter(|p| p.is_dir()) {
            cmd.env("LD_LIBRARY_PATH", lib);
        }
        #[cfg(target_os = "macos")]
        if let Some(lib) = dir.parent().map(|p| p.join("lib")).filter(|p| p.is_dir()) {
            cmd.env("DYLD_LIBRARY_PATH", lib);
        }
        let _ = dir;
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let child = cmd.spawn().map_err(|e| AppError::msg(format!("无法运行 SenseVoice（sherpa-onnx）：{e}")))?;
    let waiting = child.wait_with_output();
    tokio::pin!(waiting);
    let out = loop {
        tokio::select! {
            r = &mut waiting => break r.map_err(|e| AppError::msg(format!("SenseVoice 运行失败：{e}")))?,
            () = tokio::time::sleep(std::time::Duration::from_millis(300)) => {
                if canceled() {
                    return Err(AppError::msg("canceled"));
                }
            }
        }
    };
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let cues = parse_segments(&text);
    if out.status.success() || !cues.is_empty() {
        return Ok(cues);
    }
    let tail: String = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("（无输出）").chars().take(300).collect();
    Err(AppError::msg(format!("SenseVoice 运行失败：{tail}")))
}

// ---------- 下载安装 ----------

async fn download(app: &AppHandle, state: &AppState, url: &str, part: &Path, base: u64, total_all: u64) -> AppResult<u64> {
    let resp = state.dl_client.get(url).send().await.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), "SenseVoice"));
    }
    let size = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(part).await?;
    let (mut got, mut last) = (0u64, 0u64);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
        file.write_all(&chunk).await?;
        got += chunk.len() as u64;
        if got - last > 256 * 1024 {
            last = got;
            let _ = app.emit(super::store::EVT_ML, serde_json::json!({ "id": ID, "got": base + got, "total": total_all.max(base + size) }));
        }
    }
    file.flush().await?;
    if got < 100_000 {
        return Err(AppError::msg("下载到的文件太小，不是有效的文件"));
    }
    Ok(got)
}

async fn fetch(app: &AppHandle, state: &AppState, url: &str, part: &Path, base: u64, total_all: u64) -> AppResult<u64> {
    let mut last_err = AppError::msg("下载失败");
    for u in crate::tools::candidates(url, &state.settings().component_mirrors) {
        match download(app, state, &u, part, base, total_all).await {
            Ok(n) => return Ok(n),
            Err(e) => last_err = e,
        }
    }
    let _ = std::fs::remove_file(part);
    Err(last_err)
}

/// 解开 `.tar.bz2`，只取 `wanted` 里列出的文件名（不带目录），摊平放进 `dest`；`dll` 为真时也取所有 .dll。
/// 返回取到的文件名。路径里的 `..` 之类都被丢掉，只认文件名。
pub fn untar_bz2_flat(archive: &Path, dest: &Path, wanted: &[&str], dll: bool) -> AppResult<Vec<String>> {
    std::fs::create_dir_all(dest)?;
    let f = std::fs::File::open(archive)?;
    let mut ar = tar::Archive::new(bzip2::read::BzDecoder::new(f));
    let mut got = vec![];
    for e in ar.entries().map_err(|e| AppError::msg(format!("压缩包打不开：{e}")))? {
        let mut e = e.map_err(|e| AppError::msg(format!("压缩包损坏：{e}")))?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let path = e.path().map_err(|e| AppError::msg(e.to_string()))?.into_owned();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        let lower = name.to_ascii_lowercase();
        if !(wanted.iter().any(|w| w.eq_ignore_ascii_case(&name)) || (dll && lower.ends_with(".dll"))) {
            continue;
        }
        let tmp = dest.join(format!("{name}.part"));
        let mut buf = Vec::with_capacity(e.size() as usize);
        e.read_to_end(&mut buf)?;
        std::fs::write(&tmp, buf)?;
        std::fs::rename(&tmp, dest.join(&name))?;
        got.push(name);
    }
    Ok(got)
}

#[tauri::command]
pub async fn sensevoice_install(app: AppHandle, state: St<'_>) -> AppResult<SenseVoiceStatus> {
    let data = state.data_dir.clone();
    let have = status_of(&data);
    let d = dir_of(&data);
    std::fs::create_dir_all(&d)?;
    let _ = app.emit(super::store::EVT_ML, serde_json::json!({ "id": ID, "got": 0, "total": 0 }));
    let total_all: u64 = 25_000_000 * u64::from(!have.runtime && have.auto_install) + 165_000_000 * u64::from(!have.model);
    let mut base = 0u64;
    // 1) 运行库（只有 Windows 一键装；其它系统用户自己装）
    if !have.runtime {
        if !have.auto_install {
            return Err(AppError::invalid(have.note));
        }
        let tmp = d.join("runtime.tar.bz2.part");
        base += fetch(&app, &state, RUNTIME_URL, &tmp, base, total_all).await.map_err(|e| e.context("下载运行库失败："))?;
        let (t2, dest) = (tmp.clone(), runtime_dir(&data));
        let r = tokio::task::spawn_blocking(move || untar_bz2_flat(&t2, &dest, &[bin_name()], true)).await.map_err(|e| AppError::msg(e.to_string()))?;
        let _ = std::fs::remove_file(&tmp);
        if !r?.iter().any(|n| n.eq_ignore_ascii_case(bin_name())) {
            return Err(AppError::msg("压缩包里没有 sherpa-onnx-vad-with-offline-asr。可能官方改了打包方式，请手动安装后点“重新检测”。"));
        }
    }
    // 2) 模型 + 词表
    if !d.join(MODEL_FILE).is_file() || !d.join(TOKENS_FILE).is_file() {
        let tmp = d.join("model.tar.bz2.part");
        base += fetch(&app, &state, MODEL_URL, &tmp, base, total_all).await.map_err(|e| e.context("下载 SenseVoice 模型失败："))?;
        let (t2, dest) = (tmp.clone(), d.clone());
        let r = tokio::task::spawn_blocking(move || untar_bz2_flat(&t2, &dest, &[MODEL_FILE, TOKENS_FILE], false))
            .await
            .map_err(|e| AppError::msg(e.to_string()))?;
        let _ = std::fs::remove_file(&tmp);
        let got = r?;
        if !(got.iter().any(|n| n == MODEL_FILE) && got.iter().any(|n| n == TOKENS_FILE)) {
            return Err(AppError::msg("模型压缩包里没有找到 model.int8.onnx / tokens.txt。"));
        }
    }
    // 3) VAD
    if !d.join(VAD_FILE).is_file() {
        let tmp = d.join("silero_vad.onnx.part");
        fetch(&app, &state, VAD_URL, &tmp, base, total_all.max(base + 700_000)).await.map_err(|e| e.context("下载 VAD 模型失败："))?;
        std::fs::rename(&tmp, d.join(VAD_FILE))?;
    }
    // 装好后自动选它：用户点“下载”就是想用
    select_in_settings(&state);
    let _ = app.emit(super::store::EVT_ML, serde_json::json!({ "id": ID, "got": 1, "total": 1 }));
    Ok(status_of(&data))
}

/// 装好后把“本地语音模型”设成 SenseVoice，并让语音转文字走本机。
fn select_in_settings(state: &AppState) {
    let _ = state.update_settings(|s| {
        s.ai.stt_local_model = MODEL_ID.into();
        s.ai.stt_engine = "local".into();
    });
}

#[tauri::command]
pub fn sensevoice_remove(state: St<'_>) -> AppResult<SenseVoiceStatus> {
    let d = dir_of(&state.data_dir);
    for f in [MODEL_FILE, TOKENS_FILE, VAD_FILE] {
        let _ = std::fs::remove_file(d.join(f));
    }
    let _ = state.update_settings(|s| {
        if s.ai.stt_local_model == MODEL_ID {
            s.ai.stt_local_model = String::new();
        }
    });
    Ok(status_of(&state.data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timed_sentences_and_ignores_log_lines() {
        let out = "Started\nNum threads: 2\n0.352 -- 3.168: 对不起，你听得懂吗？\n3.500 -- 5.000: <|zh|><|NEUTRAL|>好的。\n\nElapsed seconds: 1.2 s\nreal time factor (RTF): 0.1\n7.0 -- 6.0: 倒挂的\n8.0 -- 8.0:\n";
        let c = parse_segments(out);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start, c[0].end), (352, 3168));
        assert_eq!(c[0].lines, vec!["对不起，你听得懂吗？".to_string()]);
        assert_eq!(c[1].lines, vec!["好的。".to_string()], "标记要去掉");
    }

    #[test]
    fn text_with_colons_is_kept_whole() {
        let c = parse_segments("1.0 -- 2.0: 提示：请看这里：第三行\n");
        assert_eq!(c[0].lines[0], "提示：请看这里：第三行");
        let c = parse_segments("1.0 -- 2.0: time is 10:30 now\n");
        assert_eq!(c[0].lines[0], "time is 10:30 now");
    }

    #[test]
    fn very_short_segments_get_a_minimum_length() {
        let c = parse_segments("1.000 -- 1.000: 嗯\n");
        assert_eq!(c[0].end - c[0].start, 200);
    }

    #[test]
    fn language_codes_map_to_what_sensevoice_accepts() {
        assert_eq!(lang_arg("zh-CN"), "zh");
        assert_eq!(lang_arg("ZH"), "zh");
        assert_eq!(lang_arg("en"), "en");
        assert_eq!(lang_arg("ja"), "ja");
        assert_eq!(lang_arg("fr"), "auto");
        assert_eq!(lang_arg(""), "auto");
    }

    #[test]
    fn tags_are_stripped_even_when_unclosed() {
        assert_eq!(strip_tags("<|en|>hello<|x|> world"), "hello world");
        assert_eq!(strip_tags("abc<|oops"), "abc");
    }

    #[test]
    fn extracts_wanted_files_flat_from_a_tar_bz2() {
        use bzip2::write::BzEncoder;
        let dir = std::env::temp_dir().join(format!("cc-sv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let arc = dir.join("a.tar.bz2");
        {
            let enc = BzEncoder::new(std::fs::File::create(&arc).unwrap(), bzip2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            for (n, body) in [
                ("pkg/bin/sherpa-onnx-vad-with-offline-asr.exe", "exe"),
                ("pkg/bin/onnxruntime.dll", "dll"),
                ("pkg/bin/other.exe", "x"),
                ("pkg/README.md", "r"),
            ] {
                let mut h = tar::Header::new_gnu();
                h.set_size(body.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, n, body.as_bytes()).unwrap();
            }
            b.into_inner().unwrap().finish().unwrap();
        }
        let out = dir.join("out");
        let got = untar_bz2_flat(&arc, &out, &["sherpa-onnx-vad-with-offline-asr.exe"], true).unwrap();
        assert_eq!(got.len(), 2);
        assert!(out.join("sherpa-onnx-vad-with-offline-asr.exe").is_file());
        assert!(out.join("onnxruntime.dll").is_file());
        assert!(!out.join("other.exe").exists());
        assert!(!out.join("README.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
