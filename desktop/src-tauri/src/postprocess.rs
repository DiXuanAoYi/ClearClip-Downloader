//! 后处理：用 ffmpeg 合并音视频、转封装、提取音频、写入元数据。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::error::{AppError, AppResult, ErrorKind};
use crate::AppState;

/// ffmpeg 的位置：优先用程序管理的组件目录，其次系统 PATH。
pub fn find_ffmpeg(st: &AppState) -> Option<PathBuf> {
    crate::tools::resolve(st, crate::tools::Tool::Ffmpeg)
}

/// 由目标文件扩展名推断 ffmpeg 的输出格式（输出到 .part 临时文件时必须显式指定）。
pub fn muxer_for(final_path: &Path) -> &'static str {
    match final_path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("mkv") => "matroska",
        Some("webm") => "webm",
        Some("m4a") => "ipod",
        Some("mp3") => "mp3",
        Some("ts") => "mpegts",
        Some("flv") => "flv",
        _ => "mp4",
    }
}

pub fn ffmpeg_missing() -> AppError {
    AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg 才能合并音视频。请在“设置 → 组件”中安装 ffmpeg 后重试。")
}

pub(crate) fn base_args() -> Vec<String> {
    vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into()]
}

/// 输出参数：TS 里的 AAC 写入 MP4 时需要转换为 ASC 格式；MP4 把索引移到文件头，便于边下边播。
fn output_args(args: &mut Vec<String>, output: &Path, final_path: &Path, aac_fix: bool) {
    let muxer = muxer_for(final_path);
    if aac_fix && matches!(muxer, "mp4" | "ipod") {
        args.extend(["-bsf:a".into(), "aac_adtstoasc".into()]);
    }
    if muxer == "mp4" {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-f".into(), muxer.into(), output.to_string_lossy().into_owned()]);
}

/// 把视频轨和音频轨无损合并为一个文件（不重新编码）。`aac_fix` 用于来自 m3u8（TS）的输入。
pub async fn merge(st: &AppState, inputs: &[PathBuf], output: &Path, final_path: &Path, aac_fix: bool) -> AppResult<()> {
    let ffmpeg = find_ffmpeg(st).ok_or_else(ffmpeg_missing)?;
    let mut args = base_args();
    for p in inputs {
        args.push("-i".into());
        args.push(p.to_string_lossy().into_owned());
    }
    for i in 0..inputs.len() {
        args.push("-map".into());
        args.push(format!("{i}"));
    }
    args.extend(["-c".into(), "copy".into()]);
    output_args(&mut args, output, final_path, aac_fix);
    run_ffmpeg(&ffmpeg, &args).await
}

/// 无损转封装（如 m3u8 拼接出的 TS → MP4）。
pub async fn remux(st: &AppState, input: &Path, output: &Path, final_path: &Path, aac_fix: bool) -> AppResult<()> {
    let ffmpeg = find_ffmpeg(st).ok_or_else(ffmpeg_missing)?;
    let mut args = base_args();
    args.extend(["-i".into(), input.to_string_lossy().into_owned(), "-map".into(), "0".into(), "-c".into(), "copy".into()]);
    output_args(&mut args, output, final_path, aac_fix);
    run_ffmpeg(&ffmpeg, &args).await
}

/// 提取音频的编码参数：m4a 先尝试直接复制 AAC 音轨，mp3 用 LAME 高质量 VBR。
pub fn audio_codec_args(fmt: &str, copy: bool) -> Vec<String> {
    match (fmt, copy) {
        ("m4a", true) => vec!["-c:a".into(), "copy".into()],
        ("m4a", false) => vec!["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()],
        ("opus", _) => vec!["-c:a".into(), "libopus".into(), "-b:a".into(), "128k".into()],
        ("flac", _) => vec!["-c:a".into(), "flac".into()],
        ("wav", _) => vec!["-c:a".into(), "pcm_s16le".into()],
        _ => vec!["-c:a".into(), "libmp3lame".into(), "-q:a".into(), "2".into()],
    }
}

/// 从视频（或其他音频）中提取音频为 `fmt` 格式。
pub async fn extract_audio(st: &AppState, input: &Path, output: &Path, fmt: &str) -> AppResult<()> {
    let ffmpeg = find_ffmpeg(st).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg 才能提取音频。请在“设置 → 组件”中安装 ffmpeg 后重试。"))?;
    let muxer = match fmt {
        "m4a" => "ipod",
        "opus" => "ogg",
        "flac" => "flac",
        "wav" => "wav",
        _ => "mp3",
    };
    let run = |copy: bool| {
        let mut args = base_args();
        args.extend(["-i".into(), input.to_string_lossy().into_owned(), "-vn".into(), "-map".into(), "0:a:0".into()]);
        args.extend(audio_codec_args(fmt, copy));
        args.extend(["-f".into(), muxer.into(), output.to_string_lossy().into_owned()]);
        args
    };
    if fmt == "m4a" && run_ffmpeg(&ffmpeg, &run(true)).await.is_ok() {
        return Ok(());
    }
    run_ffmpeg(&ffmpeg, &run(false)).await
}

/// 把 Pixiv 动图的帧压缩包合成为视频。依次尝试 H.264（MP4）、VP9（WebM）、GIF，
/// 返回实际使用的扩展名（LGPL 版 ffmpeg 不含 x264，会落到 WebM）。
pub async fn ugoira(st: &AppState, zip_path: &Path, frames: &[(String, u64)], output: &Path) -> AppResult<&'static str> {
    let ffmpeg = find_ffmpeg(st)
        .ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg 才能把动图合成为视频。请在“设置 → 组件”中安装 ffmpeg，或改为下载原始帧压缩包。"))?;
    let mut dir = output.as_os_str().to_owned();
    dir.push(".frames");
    let dir = PathBuf::from(dir);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let result = async {
        let zp = zip_path.to_path_buf();
        let d = dir.clone();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(&zp)?).map_err(|e| AppError::msg(format!("动图压缩包损坏：{e}")))?;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i).map_err(|e| AppError::msg(e.to_string()))?;
                let Some(name) = f.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_owned())) else { continue };
                let mut out = std::fs::File::create(d.join(name))?;
                std::io::copy(&mut f, &mut out)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
        let list = dir.join("frames.ffconcat");
        std::fs::write(&list, ffconcat(frames))?;
        let base = |codec: &[&str], muxer: &str| {
            let mut a = base_args();
            a.extend(["-f".into(), "concat".into(), "-safe".into(), "0".into(), "-i".into(), list.to_string_lossy().into_owned()]);
            a.extend(codec.iter().map(|s| s.to_string()));
            a.extend(["-f".into(), muxer.into(), output.to_string_lossy().into_owned()]);
            a
        };
        let even = "pad=ceil(iw/2)*2:ceil(ih/2)*2";
        let attempts: [(&[&str], &str, &'static str); 3] = [
            (&["-vsync", "vfr", "-vf", even, "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "18", "-movflags", "+faststart"], "mp4", "mp4"),
            (&["-vsync", "vfr", "-vf", even, "-pix_fmt", "yuv420p", "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "24"], "webm", "webm"),
            (&["-vf", "split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"], "gif", "gif"),
        ];
        let mut last = AppError::msg("动图合成失败");
        for (codec, muxer, ext) in attempts {
            match run_ffmpeg(&ffmpeg, &base(codec, muxer)).await {
                Ok(()) => return Ok(ext),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
    .await;
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// ffconcat 帧列表：每帧的显示时长；最后一帧重复一次，否则它的时长会被忽略。
pub fn ffconcat(frames: &[(String, u64)]) -> String {
    let mut s = String::from("ffconcat version 1.0\n");
    for (file, delay) in frames {
        s.push_str(&format!("file '{}'\nduration {:.3}\n", file.replace('\'', ""), *delay as f64 / 1000.0));
    }
    if let Some((last, _)) = frames.last() {
        s.push_str(&format!("file '{}'\n", last.replace('\'', "")));
    }
    s
}

/// 图集合成视频：每张图片显示 `secs` 秒，统一缩放到同一画布（多数为竖图时用竖屏），可配背景音乐。
/// 依次尝试 H.264（x264 / OpenH264）和 MPEG-4，LGPL 版 ffmpeg 也能生成。
pub async fn slideshow(st: &AppState, images: &[(PathBuf, u32, u32)], music: Option<&Path>, secs: f64, output: &Path) -> AppResult<()> {
    let ffmpeg = find_ffmpeg(st).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg 才能把图集合成视频。请在“设置 → 组件”中安装 ffmpeg。"))?;
    if images.is_empty() {
        return Err(AppError::invalid("没有图片"));
    }
    let portrait = images.iter().filter(|(_, w, h)| h > w).count() * 2 >= images.len();
    let (w, h) = if portrait { (1080, 1920) } else { (1920, 1080) };
    let total = secs * images.len() as f64;
    // 每张图片单独输入再用 concat 滤镜拼接（图片格式可以不同，concat 分离器要求格式一致）
    let mut graph = String::new();
    for i in 0..images.len() {
        graph.push_str(&format!(
            "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps=30,format=yuv420p[v{i}];"
        ));
    }
    for i in 0..images.len() {
        graph.push_str(&format!("[v{i}]"));
    }
    graph.push_str(&format!("concat=n={}:v=1:a=0[v]", images.len()));
    let n = images.len();
    let make = |codec: &[&str]| {
        let mut a = base_args();
        for (p, _, _) in images {
            a.extend(["-loop".into(), "1".into(), "-t".into(), format!("{secs:.3}"), "-i".into(), p.to_string_lossy().into_owned()]);
        }
        if let Some(m) = music {
            a.extend(["-stream_loop".into(), "-1".into(), "-i".into(), m.to_string_lossy().into_owned()]);
        }
        a.extend(["-filter_complex".into(), graph.clone(), "-map".into(), "[v]".into()]);
        if music.is_some() {
            a.extend(["-map".into(), format!("{n}:a")]);
            a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "160k".into(), "-af".into(), format!("afade=t=out:st={:.2}:d=1.5", (total - 1.5).max(0.0))]);
        }
        a.extend(["-t".into(), format!("{total:.3}")]);
        a.extend(codec.iter().map(|s| s.to_string()));
        a.extend(["-movflags".into(), "+faststart".into(), "-f".into(), "mp4".into(), output.to_string_lossy().into_owned()]);
        a
    };
    let attempts: [&[&str]; 3] =
        [&["-c:v", "libx264", "-preset", "medium", "-crf", "20"], &["-c:v", "libopenh264", "-b:v", "6M"], &["-c:v", "mpeg4", "-q:v", "3"]];
    let mut last = AppError::msg("合成失败");
    for codec in attempts {
        match run_ffmpeg(&ffmpeg, &make(codec)).await {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// 视频编码器候选：x264 → OpenH264（LGPL 版 ffmpeg）→ mpeg4。
const VIDEO_ENCODERS: [&[&str]; 3] =
    [&["-c:v", "libx264", "-preset", "veryfast", "-crf", "20"], &["-c:v", "libopenh264", "-b:v", "6M"], &["-c:v", "mpeg4", "-q:v", "3"]];

/// 依次用各个编码器重新编码，第一个成功的为准。`make` 根据编码器参数生成完整命令行。
async fn run_with_encoders(ffmpeg: &Path, make: impl Fn(&[&str]) -> Vec<String>, cwd: Option<&Path>) -> AppResult<()> {
    let mut last = AppError::msg("ffmpeg 处理失败");
    for enc in VIDEO_ENCODERS {
        match run_ffmpeg_in(ffmpeg, &make(enc), cwd).await {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// 复制模式下起点的容差：关键帧的时间戳常比整数秒晚几毫秒（容器的起始偏移），
/// 不留容差的话 ffmpeg 会退到再前一个关键帧，多出整整一个 GOP（通常 1–2 秒）。
pub const COPY_SEEK_SLACK_MS: u64 = 40;

/// 只保留 `start_ms..end_ms` 这一段。默认不重新编码（速度快，起点落在前一个关键帧上，可能提前几秒）；
/// `precise` 时重新编码，起点精确到帧。
pub async fn clip(ffmpeg: &Path, input: &Path, output: &Path, final_path: &Path, start_ms: u64, end_ms: Option<u64>, precise: bool) -> AppResult<()> {
    let seek_ms = if precise { start_ms } else { start_ms + COPY_SEEK_SLACK_MS };
    let ss = format!("{:.3}", seek_ms as f64 / 1000.0);
    let dur = end_ms.map(|e| format!("{:.3}", e.saturating_sub(start_ms) as f64 / 1000.0));
    let is_audio = matches!(final_path.extension().and_then(|e| e.to_str()), Some("mp3" | "m4a" | "opus" | "ogg" | "flac" | "wav"));
    let make = |codec: &[&str]| {
        let mut a = base_args();
        a.extend(["-ss".into(), ss.clone(), "-i".into(), input.to_string_lossy().into_owned()]);
        if let Some(d) = &dur {
            a.extend(["-t".into(), d.clone()]);
        }
        a.extend(["-map".into(), "0:v?".into(), "-map".into(), "0:a?".into(), "-map".into(), "0:s?".into()]);
        a.extend(codec.iter().map(|c| c.to_string()));
        output_args(&mut a, output, final_path, false);
        a
    };
    if precise && !is_audio {
        let make_precise = |enc: &[&str]| {
            let mut a = make(enc);
            // 音频重新编码为 AAC（输出参数在最后，插到 -f 之前）
            let at = a.iter().position(|x| x == "-f").unwrap_or(a.len());
            a.splice(at..at, ["-c:a".to_string(), "aac".into(), "-b:a".into(), "192k".into(), "-c:s".into(), "copy".into()]);
            a
        };
        return run_with_encoders(ffmpeg, make_precise, None).await;
    }
    let copy: &[&str] = &["-c", "copy", "-avoid_negative_ts", "make_zero"];
    if run_ffmpeg(ffmpeg, &make(copy)).await.is_ok() {
        return Ok(());
    }
    // 个别容器不接受源文件里的字幕流：去掉字幕流再试一次
    let all = make(copy);
    let mut args = Vec::with_capacity(all.len());
    let mut i = 0;
    while i < all.len() {
        if all[i] == "-map" && all.get(i + 1).is_some_and(|n| n == "0:s?") {
            i += 2;
            continue;
        }
        args.push(all[i].clone());
        i += 1;
    }
    run_ffmpeg(ffmpeg, &args).await
}

/// 要放进视频的一条字幕轨。
#[derive(Debug, Clone)]
pub struct SubTrack {
    pub path: PathBuf,
    /// 语言代码（如 zh-Hans），写成 ISO 639-2 标记
    pub lang: String,
    pub title: String,
}

/// 把字幕作为独立的字幕轨封装进视频（软字幕，播放器里可以切换），不重新编码视频。
/// MP4 / MOV 用 mov_text（只支持文本字幕，ASS 弹幕会被跳过）；MKV 保留 SRT / ASS；WebM 转为 WebVTT。
/// 返回实际写入的字幕数量；容器不支持时返回 0（不报错）。
pub async fn embed_subtitles(ffmpeg: &Path, input: &Path, output: &Path, final_path: &Path, tracks: &[SubTrack]) -> AppResult<usize> {
    let muxer = muxer_for(final_path);
    let usable: Vec<&SubTrack> = tracks
        .iter()
        .filter(|t| {
            let ass = t.path.extension().and_then(|e| e.to_str()) == Some("ass");
            match muxer {
                "matroska" => true,
                "mp4" => !ass,
                "webm" => !ass,
                _ => false,
            }
        })
        .collect();
    if usable.is_empty() {
        return Ok(0);
    }
    let mut args = base_args();
    args.extend(["-i".into(), input.to_string_lossy().into_owned()]);
    for t in &usable {
        args.extend(["-i".into(), t.path.to_string_lossy().into_owned()]);
    }
    args.extend(["-map".into(), "0:v?".into(), "-map".into(), "0:a?".into()]);
    for i in 0..usable.len() {
        args.extend(["-map".into(), format!("{}", i + 1)]);
    }
    args.extend(["-c:v".into(), "copy".into(), "-c:a".into(), "copy".into()]);
    args.extend([
        "-c:s".into(),
        (if muxer == "mp4" {
            "mov_text"
        } else if muxer == "webm" {
            "webvtt"
        } else {
            "copy"
        })
        .into(),
    ]);
    for (i, t) in usable.iter().enumerate() {
        args.extend([
            format!("-metadata:s:s:{i}"),
            format!("language={}", crate::subtitle::iso639_2(&t.lang)),
            format!("-metadata:s:s:{i}"),
            format!("title={}", t.title),
        ]);
    }
    output_args(&mut args, output, final_path, false);
    run_ffmpeg(ffmpeg, &args).await?;
    Ok(usable.len())
}

/// 把字幕烧录进画面（硬字幕，任何播放器都能看到，需要重新编码视频）。支持 SRT 和 ASS（弹幕），可以同时烧录多个。
/// ffmpeg 在字幕所在目录里运行、只传文件名，避免滤镜参数里的路径转义问题；字幕文件必须在同一个目录里。
pub async fn burn_subtitles(ffmpeg: &Path, input: &Path, output: &Path, final_path: &Path, subs: &[PathBuf]) -> AppResult<()> {
    let dir = subs.first().and_then(|s| s.parent()).ok_or_else(|| AppError::msg("没有要烧录的字幕"))?;
    let mut filters = vec![];
    for sub in subs {
        let name = sub.file_name().and_then(|n| n.to_str()).ok_or_else(|| AppError::msg("字幕路径无效"))?;
        if sub.parent() != Some(dir) || name.chars().any(|c| matches!(c, '\'' | ':' | ',' | '[' | ']' | ';' | '\\')) {
            return Err(AppError::msg("字幕文件名包含特殊字符，或不在同一个目录里"));
        }
        filters.push(if name.ends_with(".ass") { format!("ass={name}") } else { format!("subtitles={name}:charenc=UTF-8:force_style='Outline=1,Shadow=0'") });
    }
    let filter = filters.join(",");
    let make = |enc: &[&str]| {
        let mut a = base_args();
        a.extend([
            "-i".into(),
            input.to_string_lossy().into_owned(),
            "-vf".into(),
            filter.clone(),
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            "0:a?".into(),
        ]);
        a.extend(enc.iter().map(|c| c.to_string()));
        a.extend(["-c:a".into(), "copy".into()]);
        output_args(&mut a, output, final_path, false);
        a
    };
    run_with_encoders(ffmpeg, make, Some(dir)).await.map_err(|e| {
        if e.message.contains("No such filter") || e.message.contains("Filter not found") {
            AppError::new(ErrorKind::NeedUpdate, "当前 ffmpeg 不支持烧录字幕（缺少 libass）。请在“设置 → 组件”中重新安装 ffmpeg。")
        } else {
            e
        }
    })
}

/// 文件检查结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    pub duration_ms: Option<u64>,
    pub has_video: bool,
    pub has_audio: bool,
    /// 第一条视频轨的宽高
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 文件自带的标题、艺术家等信息（键为小写）
    pub tags: std::collections::BTreeMap<String, String>,
    /// 无法读取时 ffmpeg 给出的原因
    pub error: Option<String>,
}

impl Probe {
    /// 能否正常读取：有时长或有音视频流，且没有读取错误。
    pub fn readable(&self) -> bool {
        self.error.is_none() && (self.has_video || self.has_audio)
    }
}

/// 解析 `ffmpeg -i file` 的输出（不指定输出文件时 ffmpeg 会打印信息并以错误退出，这是正常的）。
pub fn parse_probe(stderr: &str) -> Probe {
    static DIMS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"[ ,](\d{2,5})x(\d{2,5})[ ,\[]").unwrap());
    let mut p = Probe::default();
    // 全局元数据：第一个“Metadata:”到第一个“Duration:”之间缩进 4 格的 `键 : 值`
    let (mut in_meta, mut meta_done) = (false, false);
    for line in stderr.lines() {
        let l = line.trim();
        if !meta_done {
            if l == "Metadata:" && line.starts_with("  Metadata:") {
                in_meta = true;
                continue;
            }
            if l.starts_with("Duration:") || l.starts_with("Stream #") {
                in_meta = false;
                meta_done = true;
            } else if in_meta {
                if let Some((k, v)) = l.split_once(':') {
                    let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
                    if !k.is_empty() && !v.is_empty() {
                        p.tags.insert(k, v);
                    }
                }
                continue;
            }
        }
        if let Some(rest) = l.strip_prefix("Duration:") {
            let t = rest.split(',').next().unwrap_or("").trim();
            if t != "N/A" {
                let parts: Vec<&str> = t.split(':').collect();
                if let [h, m, sec] = parts.as_slice() {
                    if let (Ok(h), Ok(m), Ok(sec)) = (h.parse::<f64>(), m.parse::<f64>(), sec.parse::<f64>()) {
                        p.duration_ms = Some(((h * 3600.0 + m * 60.0 + sec) * 1000.0).round() as u64);
                    }
                }
            }
        } else if l.starts_with("Stream #") {
            if l.contains(": Video:") && !l.contains("attached pic") {
                if !p.has_video {
                    if let Some(c) = DIMS.captures(l) {
                        p.width = c[1].parse().ok();
                        p.height = c[2].parse().ok();
                    }
                }
                p.has_video = true;
            } else if l.contains(": Audio:") {
                p.has_audio = true;
            }
        } else if l.contains("Invalid data found")
            || l.contains("moov atom not found")
            || l.contains("No such file")
            || l.contains("Permission denied")
            || l.contains("End of file")
        {
            // 去掉 ffmpeg 内部的 "[mov,mp4 @ 0x…] " 前缀
            let msg = if l.starts_with('[') { l.split_once("] ").map_or(l, |(_, m)| m) } else { l };
            p.error.get_or_insert_with(|| msg.to_string());
        }
    }
    p
}

pub async fn probe(ffmpeg: &Path, file: &Path) -> Probe {
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-i"]).arg(file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    match tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output()).await {
        Ok(Ok(out)) => parse_probe(&String::from_utf8_lossy(&out.stderr)),
        // 无法运行 ffmpeg 或超时：不能判断文件是否损坏，当作可读
        _ => Probe { has_video: true, has_audio: true, ..Default::default() },
    }
}

/// 按章节拆分成多个文件（不重新编码，起点落在关键帧上）。文件名：`NN 章节标题.扩展名`。返回成功拆出的数量。
pub async fn split_chapters(ffmpeg: &Path, input: &Path, dir: &Path, ext: &str, chapters: &[crate::model::Chapter]) -> AppResult<usize> {
    std::fs::create_dir_all(dir)?;
    let width = chapters.len().to_string().len().max(2);
    let mut done = 0;
    for (i, c) in chapters.iter().enumerate() {
        let title = crate::naming::sanitize(&c.title);
        let name = if title.is_empty() { format!("{:0width$}", i + 1) } else { format!("{:0width$} {title}", i + 1) };
        let name: String = name.chars().take(80).collect();
        let out = dir.join(format!("{name}.{ext}"));
        let mut a = base_args();
        a.extend([
            "-ss".into(),
            format!("{:.3}", (c.start_ms + COPY_SEEK_SLACK_MS) as f64 / 1000.0),
            "-i".into(),
            input.to_string_lossy().into_owned(),
            "-t".into(),
            format!("{:.3}", c.end_ms.saturating_sub(c.start_ms) as f64 / 1000.0),
            "-map".into(),
            "0:v?".into(),
            "-map".into(),
            "0:a?".into(),
            "-map".into(),
            "0:s?".into(),
            "-c".into(),
            "copy".into(),
            "-avoid_negative_ts".into(),
            "make_zero".into(),
            "-f".into(),
            muxer_for(&out).into(),
            out.to_string_lossy().into_owned(),
        ]);
        match run_ffmpeg(ffmpeg, &a).await {
            Ok(()) => done += 1,
            Err(e) => {
                log::warn!("split chapter {} failed: {e}", i + 1);
                let _ = std::fs::remove_file(&out);
            }
        }
    }
    Ok(done)
}

pub async fn run_ffmpeg_in(ffmpeg: &Path, args: &[String], cwd: Option<&Path>) -> AppResult<()> {
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    #[cfg(windows)]
    {
        // 不弹出控制台窗口
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().await.map_err(|e| AppError::new(ErrorKind::NeedUpdate, format!("无法运行 ffmpeg：{e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        Err(AppError::msg(format!("ffmpeg 处理失败：{tail}")))
    }
}

/// 运行 ffmpeg 并取回标准输出和标准错误（用于读取帧数据、分析结果）。超时后强制结束。
pub async fn run_ffmpeg_capture(ffmpeg: &Path, args: &[String], timeout: std::time::Duration) -> AppResult<(Vec<u8>, String)> {
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let child = cmd.spawn().map_err(|e| AppError::new(ErrorKind::NeedUpdate, format!("无法运行 ffmpeg：{e}")))?;
    let out = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(r) => r.map_err(|e| AppError::msg(format!("ffmpeg 运行失败：{e}")))?,
        Err(_) => return Err(AppError::msg("ffmpeg 处理超时。")),
    };
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    if out.status.success() {
        Ok((out.stdout, err))
    } else {
        let tail: String = err.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        Err(AppError::msg(format!("ffmpeg 处理失败：{tail}")))
    }
}

pub async fn run_ffmpeg(ffmpeg: &Path, args: &[String]) -> AppResult<()> {
    run_ffmpeg_in(ffmpeg, args, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 需要 ffmpeg 的测试：本机没有 ffmpeg 时跳过 ----

    fn ffmpeg_bin() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-test-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 生成 6 秒的测试视频（每秒一个关键帧）。
    async fn make_video(ff: &Path, dir: &Path, name: &str) -> PathBuf {
        let out = dir.join(name);
        let args: Vec<String> = [
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=6",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6",
            "-c:v",
            "mpeg4",
            "-g",
            "25",
            "-c:a",
            "aac",
            "-shortest",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([out.to_string_lossy().into_owned()])
        .collect();
        run_ffmpeg(ff, &args).await.unwrap();
        out
    }

    #[test]
    fn probe_output_is_parsed() {
        let out = "Input #0, mov,mp4, from 'a.mp4':\n  Duration: 00:01:02.50, start: 0.000000, bitrate: 800 kb/s\n  Stream #0:0(und): Video: h264 (High), yuv420p, 1280x720\n  Stream #0:1(und): Audio: aac (LC), 44100 Hz\n";
        let p = parse_probe(out);
        assert_eq!(p.duration_ms, Some(62_500));
        assert!(p.has_video && p.has_audio && p.readable());
        let bad = parse_probe("a.mp4: Invalid data found when processing input\n");
        assert!(!bad.readable());
        let bad = parse_probe("[mov,mp4,m4a,3gp,3g2,mj2 @ 0x557340f16f80] moov atom not found\n");
        assert_eq!(bad.error.as_deref(), Some("moov atom not found"));
        let sized = parse_probe("  Metadata:\n    major_brand     : isom\n    Title           : 我的歌\n    artist          : 某人\n  Duration: 00:00:05.00, start: 0.0\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(progressive), 1080x1920 [SAR 1:1 DAR 9:16], 2000 kb/s\n    Metadata:\n      handler_name    : VideoHandler\n");
        assert_eq!((sized.width, sized.height), (Some(1080), Some(1920)));
        assert_eq!(sized.tags.get("title").map(String::as_str), Some("我的歌"));
        assert_eq!(sized.tags.get("artist").map(String::as_str), Some("某人"));
        assert!(!sized.tags.contains_key("handler_name"), "stream metadata is not file metadata");
        let cover_only = parse_probe("  Duration: N/A, bitrate: N/A\n  Stream #0:0: Video: mjpeg, 100x100 (attached pic)\n");
        assert!(!cover_only.has_video, "attached pictures are not video");
        assert_eq!(cover_only.duration_ms, None);
    }

    #[tokio::test]
    async fn clip_probe_and_split() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("clip");
        let src = make_video(&ff, &dir, "src.mp4").await;
        let p = probe(&ff, &src).await;
        assert!(p.readable() && p.has_video && p.has_audio);
        assert!((5_900..=6_200).contains(&p.duration_ms.unwrap()), "{p:?}");

        // 复制模式：2s–4s，起点落在关键帧上
        let out = dir.join("clip.mp4");
        clip(&ff, &src, &out, &out, 2000, Some(4000), false).await.unwrap();
        let d = probe(&ff, &out).await.duration_ms.unwrap();
        assert!((1_800..=2_300).contains(&d), "copy clip duration {d}");
        // 从裁剪后的文件再拆章节：关键帧时间戳带偏移也不能多出一个 GOP
        let chunks = vec![
            crate::model::Chapter { title: "a".into(), start_ms: 0, end_ms: 1000 },
            crate::model::Chapter { title: "b".into(), start_ms: 1000, end_ms: 3000 },
        ];
        let src2 = dir.join("clip_src.mp4");
        clip(&ff, &src, &src2, &src2, 1000, Some(5000), false).await.unwrap();
        assert_eq!(split_chapters(&ff, &src2, &dir.join("c2"), "mp4", &chunks).await.unwrap(), 2);
        let d1 = probe(&ff, &dir.join("c2").join("01 a.mp4")).await.duration_ms.unwrap();
        let d2 = probe(&ff, &dir.join("c2").join("02 b.mp4")).await.duration_ms.unwrap();
        assert!((900..=1_300).contains(&d1), "first chunk {d1}");
        assert!((1_900..=2_300).contains(&d2), "second chunk {d2}");
        // 精确模式：重新编码
        let out2 = dir.join("clip2.mp4");
        clip(&ff, &src, &out2, &out2, 1500, Some(3500), true).await.unwrap();
        let d = probe(&ff, &out2).await.duration_ms.unwrap();
        assert!((1_900..=2_200).contains(&d), "precise clip duration {d}");
        // 只有起点：到结尾
        let out3 = dir.join("clip3.mp4");
        clip(&ff, &src, &out3, &out3, 4000, None, false).await.unwrap();
        assert!((1_800..=2_300).contains(&probe(&ff, &out3).await.duration_ms.unwrap()));

        // 章节拆分
        let chapters = vec![
            crate::model::Chapter { title: "开场/介绍".into(), start_ms: 0, end_ms: 2000 },
            crate::model::Chapter { title: "正文".into(), start_ms: 2000, end_ms: 6000 },
        ];
        let n = split_chapters(&ff, &src, &dir.join("章节"), "mp4", &chapters).await.unwrap();
        assert_eq!(n, 2);
        assert!(
            dir.join("章节").join("01 开场_介绍.mp4").exists(),
            "{:?}",
            std::fs::read_dir(dir.join("章节")).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>()
        );
        assert!(dir.join("章节").join("02 正文.mp4").exists());

        // 损坏的文件
        let bad = dir.join("bad.mp4");
        std::fs::write(&bad, b"this is not a video at all").unwrap();
        assert!(!probe(&ff, &bad).await.readable());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn embed_and_burn_subtitles() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("subs");
        let src = make_video(&ff, &dir, "src.mp4").await;
        let srt = dir.join("字幕 en.srt");
        std::fs::write(&srt, "1\n00:00:01,000 --> 00:00:03,000\nHello world\n\n").unwrap();
        let ass = dir.join("danmaku.ass");
        std::fs::write(&ass, "[Script Info]\nPlayResX: 320\nPlayResY: 240\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: D,Arial,20,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.00,D,,0,0,0,,{\\an7\\pos(10,10)}hi\n").unwrap();

        // 软字幕：mp4 里的 ASS 被跳过，SRT 写入
        let out = dir.join("soft.mp4");
        let tracks = vec![
            SubTrack { path: srt.clone(), lang: "en".into(), title: "English".into() },
            SubTrack { path: ass.clone(), lang: "zh".into(), title: "弹幕".into() },
        ];
        let n = embed_subtitles(&ff, &src, &out, &out, &tracks).await.unwrap();
        assert_eq!(n, 1, "mp4 cannot hold ASS");
        let info = run_ffmpeg_probe_text(&ff, &out).await;
        assert!(info.contains("Subtitle: mov_text"), "{info}");
        assert!(info.contains("(eng)"), "language tag: {info}");
        // mkv 两条都保留
        let mkv = dir.join("soft.mkv");
        assert_eq!(embed_subtitles(&ff, &src, &mkv, &mkv, &tracks).await.unwrap(), 2);
        // 不支持的容器：返回 0，不报错
        let ts = dir.join("soft.ts");
        assert_eq!(embed_subtitles(&ff, &src, &ts, &ts, &tracks).await.unwrap(), 0);

        // 烧录（文件名里有空格也能处理）
        let burned = dir.join("burned.mp4");
        burn_subtitles(&ff, &src, &burned, &burned, std::slice::from_ref(&srt)).await.unwrap();
        let p = probe(&ff, &burned).await;
        assert!(p.readable() && p.has_video && p.has_audio);
        let burned2 = dir.join("burned2.mp4");
        burn_subtitles(&ff, &src, &burned2, &burned2, std::slice::from_ref(&ass)).await.unwrap();
        assert!(probe(&ff, &burned2).await.readable());
        // 同时烧录字幕和弹幕
        let burned3 = dir.join("burned3.mp4");
        burn_subtitles(&ff, &src, &burned3, &burned3, &[srt.clone(), ass.clone()]).await.unwrap();
        assert!(probe(&ff, &burned3).await.readable());
        // 特殊字符的文件名被拒绝
        let weird = dir.join("a'b.srt");
        std::fs::write(&weird, "1\n00:00:01,000 --> 00:00:02,000\nx\n\n").unwrap();
        assert!(burn_subtitles(&ff, &src, &dir.join("w.mp4"), &dir.join("w.mp4"), &[weird]).await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn run_ffmpeg_probe_text(ff: &Path, file: &Path) -> String {
        let out = tokio::process::Command::new(ff).args(["-hide_banner", "-i"]).arg(file).output().await.unwrap();
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    #[test]
    fn ffconcat_list() {
        let s = ffconcat(&[("000000.jpg".into(), 80), ("000001.jpg".into(), 1200)]);
        assert_eq!(s, "ffconcat version 1.0\nfile '000000.jpg'\nduration 0.080\nfile '000001.jpg'\nduration 1.200\nfile '000001.jpg'\n");
    }

    #[test]
    fn muxer_from_extension() {
        assert_eq!(muxer_for(Path::new("/a/b.mp4")), "mp4");
        assert_eq!(muxer_for(Path::new("/a/b.MKV")), "matroska");
        assert_eq!(muxer_for(Path::new("/a/b.m4a")), "ipod");
        assert_eq!(muxer_for(Path::new("/a/b")), "mp4");
    }
}
