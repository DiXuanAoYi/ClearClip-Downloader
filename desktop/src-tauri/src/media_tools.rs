//! 媒体工具箱：用 ffmpeg 对本地文件做压缩、转 GIF、竖转横、倍速、响度标准化、旋转、转封装 / 转音频、
//! 截图、拼接、裁剪、去音轨、写入音频标签。任务在后台运行，带进度，可以取消。
//!
//! 管理的 ffmpeg 是 LGPL 版本，不含 x264：视频重新编码时按 `libx264 → libopenh264 → VP9 → mpeg4` 的顺序选择可用的编码器。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{Notify, Semaphore};

use crate::error::{AppError, AppResult};
use crate::postprocess::{base_args, find_ffmpeg, probe, run_ffmpeg_capture, Probe};

pub const EVT_JOBS: &str = "media://jobs";

// ---------- 任务描述 ----------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ToolOp {
    /// 重新编码缩小体积。quality：small / balanced / high
    Compress {
        quality: String,
        max_height: Option<u32>,
    },
    Gif {
        start_ms: u64,
        duration_ms: u64,
        width: u32,
        fps: u32,
    },
    /// 竖屏视频放进横屏画面。mode：blur（模糊背景）/ black（黑边）
    Landscape {
        width: u32,
        height: u32,
        mode: String,
    },
    Speed {
        factor: f64,
    },
    /// 响度标准化到目标 LUFS（视频画面不重新编码）
    Loudness {
        target: f64,
    },
    /// cw / ccw / flip180 / hflip / vflip
    Rotate {
        mode: String,
    },
    /// 转封装或转音频。format：mp4 / mkv / mov / webm / mp3 / m4a / flac / opus / wav
    Convert {
        format: String,
        reencode: bool,
    },
    Frame {
        at_ms: u64,
        format: String,
    },
    Frames {
        every_secs: f64,
        format: String,
    },
    /// 把多个文件首尾相接。reencode 为 false 时先尝试直接拼接（要求编码参数一致），失败自动改为重新编码
    Concat {
        reencode: bool,
    },
    Trim {
        start_ms: u64,
        end_ms: Option<u64>,
        precise: bool,
    },
    Mute,
    /// 把章节写进视频（MP4 / MKV，不重新编码）
    Chapters {
        chapters: Vec<crate::model::Chapter>,
    },
    /// 视频规整：把画面、帧率、色彩、声音统一成指定规格（见 `vidnorm`）。`preset` 只用于记录来自哪个预设
    Normalize {
        spec: crate::vidnorm::NormSpec,
        preset: Option<String>,
    },
    /// 防抖。strength：light / normal / strong
    Stabilize {
        strength: String,
    },
    /// 音效：降噪、人声增强、混响、响度标准化（见 `audiofx`）
    AudioFx {
        spec: crate::audiofx::AudioFxSpec,
    },
    /// 剪辑：多段素材在时间线上排布，加转场、文字、配乐，导出成一个视频（见 `edit`）。`inputs` 是工程用到的素材文件
    Edit {
        project: crate::edit::spec::Project,
    },
    /// 写入标签。None 表示不改，Some("") 表示清除
    Tags {
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        year: Option<String>,
        genre: Option<String>,
        comment: Option<String>,
        cover: Option<String>,
    },
}

impl ToolOp {
    pub fn label(&self) -> &'static str {
        match self {
            ToolOp::Compress { .. } => "压缩",
            ToolOp::Gif { .. } => "转 GIF",
            ToolOp::Landscape { .. } => "竖转横",
            ToolOp::Speed { .. } => "倍速",
            ToolOp::Loudness { .. } => "响度标准化",
            ToolOp::Rotate { .. } => "旋转",
            ToolOp::Convert { .. } => "转换格式",
            ToolOp::Frame { .. } => "截图",
            ToolOp::Frames { .. } => "定时截图",
            ToolOp::Concat { .. } => "拼接",
            ToolOp::Trim { .. } => "裁剪",
            ToolOp::Mute => "去除音轨",
            ToolOp::Chapters { .. } => "写入章节",
            ToolOp::Tags { .. } => "音频标签",
            ToolOp::Normalize { .. } => "视频规整",
            ToolOp::Stabilize { .. } => "防抖",
            ToolOp::Edit { .. } => "剪辑",
            ToolOp::AudioFx { spec } => {
                if spec.reverb.is_some() && spec.denoise == "off" && !spec.enhance {
                    "混响"
                } else {
                    "音效"
                }
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolJob {
    pub inputs: Vec<String>,
    #[serde(flatten)]
    pub op: ToolOp,
    /// 输出文件夹；留空放在第一个输入文件旁边
    #[serde(default)]
    pub output_dir: Option<String>,
}

// ---------- 编码器 ----------

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Encoders {
    /// libx264 / libopenh264
    pub h264: Option<&'static str>,
    pub vp9: bool,
    pub mp3: bool,
    pub opus: bool,
}

impl Encoders {
    /// 加入试编码通过的硬件 H.264 编码器：有 x264 时仍用 x264；没有 x264 时硬件编码器排在 openh264 前面。
    pub fn with_hardware(mut self, hw: Option<&'static str>) -> Encoders {
        if let Some(hw) = hw {
            if !matches!(self.h264, Some("libx264")) {
                self.h264 = Some(hw);
            }
        }
        self
    }
}

pub fn parse_encoders(out: &str) -> Encoders {
    let has = |name: &str| out.lines().any(|l| l.split_whitespace().nth(1) == Some(name));
    Encoders {
        h264: if has("libx264") {
            Some("libx264")
        } else if has("libopenh264") {
            Some("libopenh264")
        } else {
            None
        },
        vp9: has("libvpx-vp9"),
        mp3: has("libmp3lame"),
        opus: has("libopus"),
    }
}

pub async fn detect_encoders(ffmpeg: &Path) -> Encoders {
    match run_ffmpeg_capture(ffmpeg, &["-hide_banner".into(), "-encoders".into()], std::time::Duration::from_secs(20)).await {
        Ok((out, _)) => parse_encoders(&String::from_utf8_lossy(&out)),
        Err(_) => Encoders::default(),
    }
}

/// 视频编码方案：编码参数 + 适合的容器 + 对应的音频编码。
pub(crate) struct VideoCodec {
    pub(crate) args: Vec<String>,
    /// 输出容器扩展名（要求 mp4 却没有 H.264 时改用 mkv）
    pub(crate) ext: &'static str,
    pub(crate) audio: Vec<String>,
    pub(crate) note: Option<String>,
    /// 编码器要求的像素格式（滤镜链最后一个 format 用它）
    pub(crate) pix: &'static str,
}

pub(crate) fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// 按画面大小和帧率估一个目标码率（kbps）：`level` 0/1/2 对应小/均衡/高质量，每像素每帧约 0.045 / 0.075 / 0.12 比特。
pub(crate) fn target_kbps(pixels_per_sec: f64, level: usize) -> u32 {
    let bpp = [0.045, 0.075, 0.12][level.min(2)];
    ((pixels_per_sec * bpp / 1000.0) as u32).clamp(600, 40_000)
}

/// 硬件 H.264 编码器的参数。
fn hw_h264_args(name: &str, level: usize, kbps: u32) -> (Vec<String>, &'static str) {
    let q = ["30", "25", "21"][level.min(2)];
    let rate = format!("{kbps}k");
    match name {
        "h264_nvenc" => (s(&["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", q, "-b:v", "0", "-profile:v", "high"]), "yuv420p"),
        "h264_qsv" => (s(&["-c:v", "h264_qsv", "-global_quality", q, "-preset", "medium", "-profile:v", "high"]), "nv12"),
        "h264_amf" => (s(&["-c:v", "h264_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", q, "-qp_p", q, "-profile:v", "high"]), "yuv420p"),
        // VideoToolbox 的质量参数在旧版本上不可用，统一用码率
        _ => (s(&["-c:v", name, "-b:v", &rate, "-profile:v", "high", "-allow_sw", "1"]), "yuv420p"),
    }
}

pub(crate) fn video_codec(enc: &Encoders, quality: &str, want_ext: &str) -> VideoCodec {
    video_codec_for(enc, quality, want_ext, None)
}

/// `pixels_per_sec`（宽 × 高 × 帧率）已知时，码率型编码器（硬件编码器、openh264）按它估码率，否则用固定码率。
pub(crate) fn video_codec_for(enc: &Encoders, quality: &str, want_ext: &str, pixels_per_sec: Option<f64>) -> VideoCodec {
    let level = match quality {
        "small" => 0,
        "high" => 2,
        _ => 1,
    };
    let aac = |k: &str| s(&["-c:a", "aac", "-b:a", k]);
    let mp4_or_mkv = if want_ext == "mkv" { "mkv" } else { "mp4" };
    match enc.h264 {
        Some("libx264") => VideoCodec {
            args: [s(&["-c:v", "libx264", "-preset", "medium", "-crf", ["30", "25", "21"][level], "-pix_fmt", "yuv420p"])].concat(),
            ext: mp4_or_mkv,
            audio: aac(["96k", "128k", "192k"][level]),
            note: None,
            pix: "yuv420p",
        },
        Some("libopenh264") => {
            let rate = pixels_per_sec.map(|p| format!("{}k", target_kbps(p, level))).unwrap_or_else(|| ["900k", "2500k", "6000k"][level].to_string());
            VideoCodec {
                args: [s(&["-c:v", "libopenh264", "-b:v", &rate, "-pix_fmt", "yuv420p"])].concat(),
                ext: mp4_or_mkv,
                audio: aac(["96k", "128k", "192k"][level]),
                note: None,
                pix: "yuv420p",
            }
        }
        Some(hw) => {
            let kbps = pixels_per_sec.map(|p| target_kbps(p, level)).unwrap_or([900, 2500, 6000][level]);
            let (args, pix) = hw_h264_args(hw, level, kbps);
            VideoCodec {
                args, ext: mp4_or_mkv, audio: aac(["96k", "128k", "192k"][level]), note: Some(format!("没有 x264，使用硬件编码器 {hw}。")), pix
            }
        }
        None if enc.vp9 => VideoCodec {
            args: [s(&[
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "0",
                "-crf",
                ["38", "33", "28"][level],
                "-row-mt",
                "1",
                "-deadline",
                "good",
                "-cpu-used",
                "4",
                "-pix_fmt",
                "yuv420p",
            ])]
            .concat(),
            ext: "mkv",
            audio: if enc.opus { s(&["-c:a", "libopus", "-b:a", ["64k", "96k", "128k"][level]]) } else { aac("128k") },
            note: Some("当前的 ffmpeg 没有 H.264 编码器，已改用 VP9 编码并保存为 MKV。要输出 H.264，请在“设置 → 组件”里安装完整版 ffmpeg。".into()),
            pix: "yuv420p",
        },
        None => VideoCodec {
            args: [s(&["-c:v", "mpeg4", "-q:v", ["8", "5", "3"][level], "-pix_fmt", "yuv420p"])].concat(),
            ext: mp4_or_mkv,
            audio: aac("128k"),
            note: Some("当前的 ffmpeg 没有 H.264 和 VP9 编码器，已改用 MPEG-4 编码，压缩效果较差。在“设置 → 组件”里安装完整版 ffmpeg 可以解决。".into()),
            pix: "yuv420p",
        },
    }
}

pub(crate) fn audio_codec(enc: &Encoders, fmt: &str) -> AppResult<Vec<String>> {
    Ok(match fmt {
        "mp3" if enc.mp3 => s(&["-c:a", "libmp3lame", "-q:a", "2"]),
        "mp3" => return Err(AppError::invalid("当前的 ffmpeg 没有 MP3 编码器，请改选 M4A 或 FLAC。")),
        "m4a" => s(&["-c:a", "aac", "-b:a", "192k"]),
        "opus" if enc.opus => s(&["-c:a", "libopus", "-b:a", "128k"]),
        "opus" => return Err(AppError::invalid("当前的 ffmpeg 没有 Opus 编码器。")),
        "flac" => s(&["-c:a", "flac"]),
        "wav" => s(&["-c:a", "pcm_s16le"]),
        _ => return Err(AppError::invalid("不支持的音频格式。")),
    })
}

// ---------- 计划 ----------

#[derive(Debug, Clone)]
pub struct Input {
    pub path: PathBuf,
    pub probe: Probe,
}

#[derive(Debug)]
pub struct Plan {
    /// ffmpeg 参数（不含 `-hide_banner -y` 等公共部分）
    pub args: Vec<String>,
    /// 输出文件，或输出文件夹（`is_dir`）
    pub output: PathBuf,
    pub is_dir: bool,
    /// video / audio / image
    pub kind: &'static str,
    /// 预计输出时长（毫秒），用于计算进度
    pub out_ms: Option<u64>,
    pub note: Option<String>,
    /// 直接拼接失败时改用的参数
    pub fallback: Option<Vec<String>>,
    /// 任务结束后要删除的临时文件
    pub temp_files: Vec<PathBuf>,
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
}

pub(crate) fn out_path(input: &Path, dir: Option<&Path>, tag: &str, ext: &str) -> PathBuf {
    let dir = dir.map(Path::to_path_buf).or_else(|| input.parent().map(Path::to_path_buf)).unwrap_or_default();
    crate::naming::unique_path(dir.join(format!("{}.{tag}.{ext}", stem_of(input))), &|p: &Path| p.exists())
}

fn path_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub(crate) fn muxer_args(ext: &str, out: &Path) -> Vec<String> {
    let mut a = vec![];
    if ext == "mp4" || ext == "m4a" || ext == "mov" {
        a.extend(s(&["-movflags", "+faststart"]));
    }
    a.push(path_arg(out));
    a
}

/// 倍速的音频滤镜：atempo 单个只支持 0.5–2，超出就串联。
pub fn atempo_chain(mut f: f64) -> String {
    let mut parts = vec![];
    while f > 2.0 + 1e-9 {
        parts.push("atempo=2.0".to_string());
        f /= 2.0;
    }
    while f < 0.5 - 1e-9 {
        parts.push("atempo=0.5".to_string());
        f /= 0.5;
    }
    parts.push(format!("atempo={f:.4}"));
    parts.join(",")
}

fn require_video(i: &Input) -> AppResult<()> {
    if !i.probe.has_video {
        return Err(AppError::invalid(format!("“{}”里没有视频画面，这个工具只适用于视频文件。", stem_of(&i.path))));
    }
    Ok(())
}

fn concat_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| format!("file '{}'\n", p.to_string_lossy().replace('\'', "'\\''"))).collect()
}

/// 把任务翻译成 ffmpeg 参数，模糊背景用默认的 `boxblur`（测试和不需要探测滤镜的场合用）。
pub fn plan(job: &ToolJob, inputs: &[Input], enc: &Encoders) -> AppResult<Plan> {
    plan_with(job, inputs, enc, "boxblur=25:5")
}

/// 把任务翻译成 ffmpeg 参数。纯函数（只有拼接会写一个临时清单文件）。
/// `blur` 是“模糊背景”用的模糊滤镜（`Caps::background_blur`）：精简版 ffmpeg 没有 `boxblur`，要换成 `gblur`。
pub fn plan_with(job: &ToolJob, inputs: &[Input], enc: &Encoders, blur: &str) -> AppResult<Plan> {
    let first = inputs.first().ok_or_else(|| AppError::invalid("请先选择文件。"))?;
    let dir = job.output_dir.as_deref().filter(|d| !d.trim().is_empty()).map(Path::new);
    let inp = path_arg(&first.path);
    let dur = first.probe.duration_ms;
    let mut p = Plan { args: vec![], output: PathBuf::new(), is_dir: false, kind: "video", out_ms: dur, note: None, fallback: None, temp_files: vec![] };
    let in_ext = first.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();

    match &job.op {
        ToolOp::Compress { quality, max_height } => {
            require_video(first)?;
            let vc = video_codec(enc, quality, "mp4");
            p.output = out_path(&first.path, dir, "压缩", vc.ext);
            p.args = s(&["-i", &inp, "-map", "0:v:0", "-map", "0:a:0?"]);
            if let Some(h) = max_height.filter(|h| *h >= 144) {
                p.args.extend(["-vf".into(), format!("scale=-2:'min(ih,{h})'")]);
            }
            p.args.extend(vc.args);
            p.args.extend(vc.audio);
            p.args.extend(muxer_args(vc.ext, &p.output));
            p.note = vc.note;
        }
        ToolOp::Gif { start_ms, duration_ms, width, fps } => {
            require_video(first)?;
            let (w, f) = ((*width).clamp(120, 1280), (*fps).clamp(5, 30));
            let d = (*duration_ms).clamp(500, 60_000);
            p.output = out_path(&first.path, dir, "动图", "gif");
            p.kind = "image";
            p.out_ms = Some(d);
            p.args = s(&["-ss", &format!("{:.3}", *start_ms as f64 / 1000.0), "-t", &format!("{:.3}", d as f64 / 1000.0), "-i", &inp, "-an", "-vf"]);
            p.args.push(format!("fps={f},scale={w}:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4"));
            p.args.extend(s(&["-loop", "0"]));
            p.args.push(path_arg(&p.output));
        }
        ToolOp::Landscape { width, height, mode } => {
            require_video(first)?;
            let (w, h) = ((*width).clamp(320, 3840) & !1, (*height).clamp(240, 2160) & !1);
            let vc = video_codec(enc, "high", "mp4");
            p.output = out_path(&first.path, dir, "横屏", vc.ext);
            let graph = if mode == "black" {
                format!("[0:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,setsar=1[v]")
            } else {
                format!(
                    "[0:v]split[a][b];[a]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},{blur}[bg];[b]scale=-2:{h}[fg];[bg][fg]overlay=(W-w)/2:(H-h)/2,setsar=1[v]"
                )
            };
            p.args = s(&["-i", &inp, "-filter_complex", &graph, "-map", "[v]", "-map", "0:a:0?"]);
            p.args.extend(vc.args);
            p.args.extend(s(&["-c:a", "copy"]));
            p.args.extend(muxer_args(vc.ext, &p.output));
            p.note = vc.note;
        }
        ToolOp::Speed { factor } => {
            let f = factor.clamp(0.25, 4.0);
            let video = first.probe.has_video;
            if !video && !first.probe.has_audio {
                return Err(AppError::invalid("文件里没有音频或视频。"));
            }
            p.out_ms = dur.map(|d| (d as f64 / f) as u64);
            if video {
                let vc = video_codec(enc, "high", "mp4");
                p.output = out_path(&first.path, dir, &format!("{}倍速", trim_float(f)), vc.ext);
                p.args = s(&["-i", &inp, "-filter_complex"]);
                if first.probe.has_audio {
                    p.args.push(format!("[0:v]setpts=PTS/{f:.4}[v];[0:a]{}[a]", atempo_chain(f)));
                    p.args.extend(s(&["-map", "[v]", "-map", "[a]"]));
                } else {
                    p.args.push(format!("[0:v]setpts=PTS/{f:.4}[v]"));
                    p.args.extend(s(&["-map", "[v]"]));
                }
                p.args.extend(vc.args);
                if first.probe.has_audio {
                    p.args.extend(vc.audio);
                }
                p.args.extend(muxer_args(vc.ext, &p.output));
                p.note = vc.note;
            } else {
                let fmt = audio_ext(&in_ext);
                p.kind = "audio";
                p.output = out_path(&first.path, dir, &format!("{}倍速", trim_float(f)), fmt);
                p.args = s(&["-i", &inp, "-vn", "-af", &atempo_chain(f)]);
                p.args.extend(audio_codec(enc, fmt)?);
                p.args.push(path_arg(&p.output));
            }
        }
        ToolOp::Loudness { target } => {
            let t = target.clamp(-30.0, -5.0);
            let af = format!("loudnorm=I={t}:TP=-1.5:LRA=11");
            if first.probe.has_video {
                let ext = if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov") { in_ext.as_str() } else { "mp4" };
                p.output = out_path(&first.path, dir, "响度", ext);
                p.args = s(&["-i", &inp, "-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-af", &af, "-c:a", "aac", "-b:a", "192k"]);
                p.args.extend(muxer_args(ext, &p.output));
            } else if first.probe.has_audio {
                let fmt = audio_ext(&in_ext);
                p.kind = "audio";
                p.output = out_path(&first.path, dir, "响度", fmt);
                p.args = s(&["-i", &inp, "-vn", "-af", &af]);
                p.args.extend(audio_codec(enc, fmt)?);
                p.args.push(path_arg(&p.output));
            } else {
                return Err(AppError::invalid("文件里没有音频。"));
            }
        }
        ToolOp::Rotate { mode } => {
            require_video(first)?;
            let vf = match mode.as_str() {
                "cw" => "transpose=1",
                "ccw" => "transpose=2",
                "flip180" => "transpose=1,transpose=1",
                "hflip" => "hflip",
                "vflip" => "vflip",
                _ => return Err(AppError::invalid("不支持的旋转方式。")),
            };
            let vc = video_codec(enc, "high", "mp4");
            p.output = out_path(&first.path, dir, "旋转", vc.ext);
            p.args = s(&["-i", &inp, "-map", "0:v:0", "-map", "0:a:0?", "-vf", vf]);
            p.args.extend(vc.args);
            p.args.extend(s(&["-c:a", "copy"]));
            p.args.extend(muxer_args(vc.ext, &p.output));
            p.note = vc.note;
        }
        ToolOp::Convert { format, reencode } => {
            let fmt = format.to_ascii_lowercase();
            if matches!(fmt.as_str(), "mp3" | "m4a" | "flac" | "opus" | "wav") {
                if !first.probe.has_audio {
                    return Err(AppError::invalid("文件里没有音频。"));
                }
                p.kind = "audio";
                p.output = out_path(&first.path, dir, "音频", &fmt);
                p.args = s(&["-i", &inp, "-vn", "-map", "0:a:0"]);
                p.args.extend(audio_codec(enc, &fmt)?);
                p.args.push(path_arg(&p.output));
            } else if matches!(fmt.as_str(), "mp4" | "mkv" | "mov" | "webm") {
                require_video(first)?;
                if *reencode || fmt == "webm" {
                    if fmt == "webm" {
                        if !enc.vp9 {
                            return Err(AppError::invalid("当前的 ffmpeg 没有 VP9 编码器，无法输出 WebM。"));
                        }
                        p.output = out_path(&first.path, dir, "转换", "webm");
                        p.args = s(&[
                            "-i",
                            &inp,
                            "-map",
                            "0:v:0",
                            "-map",
                            "0:a:0?",
                            "-c:v",
                            "libvpx-vp9",
                            "-b:v",
                            "0",
                            "-crf",
                            "32",
                            "-row-mt",
                            "1",
                            "-deadline",
                            "good",
                            "-cpu-used",
                            "4",
                            "-c:a",
                        ]);
                        p.args.push(if enc.opus {
                            "libopus".into()
                        } else {
                            return Err(AppError::invalid("当前的 ffmpeg 没有 Opus 编码器，无法输出 WebM。"));
                        });
                        p.args.push(path_arg(&p.output));
                    } else {
                        let vc = video_codec(enc, "high", &fmt);
                        p.output = out_path(&first.path, dir, "转换", vc.ext);
                        p.args = s(&["-i", &inp, "-map", "0:v:0", "-map", "0:a:0?"]);
                        p.args.extend(vc.args);
                        p.args.extend(vc.audio);
                        p.args.extend(muxer_args(vc.ext, &p.output));
                        p.note = vc.note;
                    }
                } else {
                    p.output = out_path(&first.path, dir, "转换", &fmt);
                    p.args = s(&["-i", &inp, "-map", "0:v:0", "-map", "0:a:0?", "-c", "copy"]);
                    p.args.extend(muxer_args(&fmt, &p.output));
                }
            } else {
                return Err(AppError::invalid("不支持的输出格式。"));
            }
        }
        ToolOp::Frame { at_ms, format } => {
            require_video(first)?;
            let ext = if format == "png" { "png" } else { "jpg" };
            p.kind = "image";
            p.out_ms = None;
            p.output = out_path(&first.path, dir, &format!("截图{}", crate::subtitle::fmt_srt_time(*at_ms).replace([':', ','], "-")), ext);
            p.args = s(&["-ss", &format!("{:.3}", *at_ms as f64 / 1000.0), "-i", &inp, "-frames:v", "1", "-an"]);
            if ext == "jpg" {
                p.args.extend(s(&["-q:v", "2"]));
            }
            p.args.push(path_arg(&p.output));
        }
        ToolOp::Frames { every_secs, format } => {
            require_video(first)?;
            let ext = if format == "png" { "png" } else { "jpg" };
            let every = every_secs.clamp(0.2, 3600.0);
            let folder = dir.map(Path::to_path_buf).or_else(|| first.path.parent().map(Path::to_path_buf)).unwrap_or_default();
            let folder = crate::naming::unique_path(folder.join(format!("{} 截图", stem_of(&first.path))), &|p: &Path| p.exists());
            std::fs::create_dir_all(&folder)?;
            p.is_dir = true;
            p.kind = "image";
            p.args = s(&["-i", &inp, "-an", "-vf", &format!("fps=1/{every}")]);
            if ext == "jpg" {
                p.args.extend(s(&["-q:v", "2"]));
            }
            p.args.push(path_arg(&folder.join(format!("%04d.{ext}"))));
            p.output = folder;
        }
        ToolOp::Concat { reencode } => {
            if inputs.len() < 2 {
                return Err(AppError::invalid("拼接至少需要两个文件。"));
            }
            let video = inputs.iter().all(|i| i.probe.has_video);
            let audio_only = inputs.iter().all(|i| !i.probe.has_video && i.probe.has_audio);
            if !video && !audio_only {
                return Err(AppError::invalid("请只选择视频文件，或只选择音频文件，不要混在一起。"));
            }
            let total: u64 = inputs.iter().filter_map(|i| i.probe.duration_ms).sum();
            p.out_ms = (total > 0).then_some(total);
            let ext = if audio_only {
                audio_ext(&in_ext)
            } else if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov") {
                in_ext.as_str()
            } else {
                "mp4"
            };
            p.kind = if audio_only { "audio" } else { "video" };
            p.output = out_path(&first.path, dir, "拼接", ext);
            // 清单文件放在系统临时目录
            let list = std::env::temp_dir().join(format!("clearclip-concat-{}-{}.txt", std::process::id(), crate::db::now()));
            let paths: Vec<PathBuf> = inputs.iter().map(|i| i.path.clone()).collect();
            std::fs::write(&list, concat_list(&paths))?;
            p.temp_files.push(list.clone());
            let mut copy = s(&["-f", "concat", "-safe", "0", "-i", &path_arg(&list), "-c", "copy"]);
            copy.extend(muxer_args(ext, &p.output));
            // 重新编码：统一成第一个文件的尺寸
            let n = inputs.len();
            let (w, h) = (first.probe.width.unwrap_or(1280) & !1, first.probe.height.unwrap_or(720) & !1);
            let all_audio = inputs.iter().all(|i| i.probe.has_audio);
            let mut re: Vec<String> = vec![];
            for i in inputs {
                re.extend(["-i".into(), path_arg(&i.path)]);
            }
            let mut graph = String::new();
            let mut labels = String::new();
            for k in 0..n {
                if video {
                    graph.push_str(&format!(
                        "[{k}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{k}];"
                    ));
                    labels.push_str(&format!("[v{k}]"));
                }
                if all_audio {
                    graph.push_str(&format!("[{k}:a]aresample=44100,aformat=sample_fmts=fltp:channel_layouts=stereo[a{k}];"));
                    labels.push_str(&format!("[a{k}]"));
                }
            }
            graph.push_str(&format!(
                "{labels}concat=n={n}:v={}:a={}[ov]{}",
                u8::from(video),
                u8::from(all_audio),
                if video && all_audio { "[oa]" } else { "" }
            ));
            // 输出标签：视频+音频 [ov][oa]；只有视频 [ov]；只有音频 [ov] 当作音频
            re.extend(["-filter_complex".into(), graph]);
            let vc = video_codec(enc, "high", ext);
            if video {
                re.extend(s(&["-map", "[ov]"]));
                if all_audio {
                    re.extend(s(&["-map", "[oa]"]));
                }
                re.extend(vc.args);
                if all_audio {
                    re.extend(vc.audio);
                }
            } else {
                re.extend(s(&["-map", "[ov]"]));
                re.extend(audio_codec(enc, ext)?);
            }
            re.extend(muxer_args(ext, &p.output));
            let dims_differ = video && inputs.iter().any(|i| (i.probe.width, i.probe.height) != (first.probe.width, first.probe.height));
            if *reencode || dims_differ {
                p.args = re;
                p.note = if dims_differ && !*reencode {
                    Some("这些视频的画面尺寸不一样，不能直接拼接，已改为重新编码（统一成第一个视频的尺寸）。".into())
                } else {
                    vc.note
                };
            } else {
                p.args = copy;
                p.fallback = Some(re);
            }
        }
        ToolOp::Trim { start_ms, end_ms, precise } => {
            if !first.probe.has_video && !first.probe.has_audio {
                return Err(AppError::invalid("文件里没有音频或视频。"));
            }
            if let Some(e) = end_ms {
                if e <= start_ms {
                    return Err(AppError::invalid("结束时间必须晚于开始时间。"));
                }
            }
            let ext = if first.probe.has_video {
                if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov" | "webm") {
                    in_ext.clone()
                } else {
                    "mp4".into()
                }
            } else {
                audio_ext(&in_ext).to_string()
            };
            p.kind = if first.probe.has_video { "video" } else { "audio" };
            p.output = out_path(&first.path, dir, "裁剪", &ext);
            let len = end_ms.map(|e| e - start_ms).or_else(|| dur.map(|d| d.saturating_sub(*start_ms)));
            p.out_ms = len;
            // 不重新编码时开始点要留一点余量，避免多带一个 GOP（见 postprocess::COPY_SEEK_SLACK_MS）
            let ss = if *precise { *start_ms } else { *start_ms + crate::postprocess::COPY_SEEK_SLACK_MS };
            p.args = s(&["-ss", &format!("{:.3}", ss as f64 / 1000.0), "-i", &inp]);
            if let Some(l) = len {
                p.args.extend(["-t".into(), format!("{:.3}", l as f64 / 1000.0)]);
            }
            p.args.extend(s(&["-map", "0:v:0?", "-map", "0:a:0?"]));
            if *precise && first.probe.has_video {
                let vc = video_codec(enc, "high", &ext);
                p.args.extend(vc.args);
                p.args.extend(vc.audio);
                p.note = vc.note;
            } else {
                p.args.extend(s(&["-c", "copy", "-avoid_negative_ts", "make_zero"]));
            }
            p.args.extend(muxer_args(&ext, &p.output));
        }
        ToolOp::Mute => {
            require_video(first)?;
            let ext = if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov" | "webm") { in_ext.as_str() } else { "mp4" };
            p.output = out_path(&first.path, dir, "无声", ext);
            p.args = s(&["-i", &inp, "-map", "0:v:0", "-c:v", "copy", "-an"]);
            p.args.extend(muxer_args(ext, &p.output));
        }
        ToolOp::Chapters { chapters } => {
            require_video(first).or_else(|e| if first.probe.has_audio { Ok(()) } else { Err(e) })?;
            if chapters.is_empty() {
                return Err(AppError::invalid("没有章节。"));
            }
            let ext = if first.probe.has_video {
                if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov") {
                    in_ext.as_str()
                } else {
                    "mp4"
                }
            } else if matches!(in_ext.as_str(), "m4a" | "mp3") {
                in_ext.as_str()
            } else {
                "m4a"
            };
            p.kind = if first.probe.has_video { "video" } else { "audio" };
            p.output = out_path(&first.path, dir, "章节", ext);
            let meta = std::env::temp_dir().join(format!("clearclip-chapters-{}-{}.txt", std::process::id(), crate::db::now()));
            std::fs::write(&meta, ffmetadata(chapters, dur))?;
            p.temp_files.push(meta.clone());
            p.args = s(&["-i", &inp, "-i", &path_arg(&meta), "-map", "0", "-map_metadata", "0", "-map_chapters", "1", "-c", "copy"]);
            p.args.extend(muxer_args(ext, &p.output));
        }
        ToolOp::Normalize { .. } | ToolOp::Stabilize { .. } | ToolOp::Edit { .. } | ToolOp::AudioFx { .. } => {
            return Err(AppError::msg("视频规整、防抖、剪辑和音效由专用流程处理。"))
        }
        ToolOp::Tags { title, artist, album, year, genre, comment, cover } => {
            if !first.probe.has_audio {
                return Err(AppError::invalid("文件里没有音频。"));
            }
            let ext = if first.probe.has_video {
                if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov") {
                    in_ext.as_str()
                } else {
                    "mp4"
                }
            } else {
                audio_ext(&in_ext)
            };
            p.kind = if first.probe.has_video { "video" } else { "audio" };
            p.output = out_path(&first.path, dir, "标签", ext);
            p.args = s(&["-i", &inp]);
            let cover_ok = cover.as_deref().filter(|c| !c.is_empty() && matches!(ext, "mp3" | "m4a" | "flac" | "mp4"));
            if let Some(c) = cover_ok {
                p.args.extend(["-i".into(), c.to_string()]);
            }
            p.args.extend(s(&["-map", "0:a", "-map_metadata", "0"]));
            if first.probe.has_video {
                p.args.extend(s(&["-map", "0:v?"]));
            }
            if cover_ok.is_some() {
                p.args.extend(s(&["-map", "1:v", "-c:v:0", "copy", "-disposition:v:0", "attached_pic"]));
                // 封面是 PNG / WebP 时转成 JPEG，兼容性最好
                let ci = if first.probe.has_video { 1 } else { 0 };
                p.args.extend(s(&[&format!("-c:v:{ci}"), "mjpeg"]));
            }
            p.args.extend(s(&["-c:a", "copy"]));
            if first.probe.has_video && cover_ok.is_none() {
                p.args.extend(s(&["-c:v", "copy"]));
            }
            for (k, v) in [("title", title), ("artist", artist), ("album", album), ("date", year), ("genre", genre), ("comment", comment)] {
                if let Some(v) = v {
                    p.args.push("-metadata".into());
                    p.args.push(format!("{k}={v}"));
                }
            }
            if ext == "mp3" {
                p.args.extend(s(&["-id3v2_version", "3"]));
            }
            p.args.push(path_arg(&p.output));
            p.out_ms = None;
        }
    }
    Ok(p)
}

fn meta_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '=' | ';' | '#' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// ffmpeg 的章节元数据文件。每章的结束时间取下一章的开始；最后一章到视频结尾。
pub fn ffmetadata(chapters: &[crate::model::Chapter], total_ms: Option<u64>) -> String {
    let mut out = String::from(";FFMETADATA1\n");
    for (i, c) in chapters.iter().enumerate() {
        let end = chapters.get(i + 1).map(|n| n.start_ms).or(total_ms).unwrap_or(c.start_ms + 1000).max(c.start_ms + 1);
        out.push_str(&format!("[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n", c.start_ms, end, meta_escape(&c.title)));
    }
    out
}

fn trim_float(f: f64) -> String {
    let t = format!("{f:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn audio_ext(in_ext: &str) -> &'static str {
    match in_ext {
        "mp3" => "mp3",
        "flac" => "flac",
        "wav" => "wav",
        "opus" | "ogg" => "opus",
        _ => "m4a",
    }
}

// ---------- 读取标签 ----------

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfoLite {
    pub duration_ms: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_video: bool,
    pub has_audio: bool,
    pub tags: BTreeMap<String, String>,
}

impl From<Probe> for MediaInfoLite {
    fn from(p: Probe) -> Self {
        MediaInfoLite { duration_ms: p.duration_ms, width: p.width, height: p.height, has_video: p.has_video, has_audio: p.has_audio, tags: p.tags }
    }
}

// ---------- 任务管理 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnap {
    pub id: u64,
    pub title: String,
    pub op: String,
    /// queued / running / done / failed / canceled
    pub status: String,
    pub percent: f64,
    pub output: Option<String>,
    pub error: Option<String>,
    pub note: Option<String>,
    pub finished_at: Option<i64>,
}

struct Entry {
    snap: JobSnap,
    cancel: Arc<AtomicBool>,
    wake: Arc<Notify>,
}

pub struct MediaJobs {
    next: AtomicU64,
    list: Mutex<Vec<Entry>>,
    sem: Arc<Semaphore>,
}

impl Default for MediaJobs {
    fn default() -> Self {
        MediaJobs { next: AtomicU64::new(1), list: Mutex::new(vec![]), sem: Arc::new(Semaphore::new(2)) }
    }
}

impl MediaJobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry>> {
        self.list.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn snapshot(&self) -> Vec<JobSnap> {
        self.lock().iter().map(|e| e.snap.clone()).collect()
    }

    fn update<F: FnOnce(&mut JobSnap)>(&self, id: u64, f: F) {
        if let Some(e) = self.lock().iter_mut().find(|e| e.snap.id == id) {
            f(&mut e.snap);
        }
    }

    fn add(&self, title: String, op: &str) -> (u64, Arc<AtomicBool>, Arc<Notify>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (cancel, wake) = (Arc::new(AtomicBool::new(false)), Arc::new(Notify::new()));
        self.lock().push(Entry {
            snap: JobSnap { id, title, op: op.into(), status: "queued".into(), percent: 0.0, output: None, error: None, note: None, finished_at: None },
            cancel: cancel.clone(),
            wake: wake.clone(),
        });
        (id, cancel, wake)
    }

    pub fn cancel(&self, id: u64) {
        if let Some(e) = self.lock().iter().find(|e| e.snap.id == id) {
            e.cancel.store(true, Ordering::Relaxed);
            e.wake.notify_waiters();
        }
    }

    pub fn clear_finished(&self) {
        self.lock().retain(|e| matches!(e.snap.status.as_str(), "queued" | "running"));
    }
}

/// 解析 `-progress pipe:1` 输出里的一行，返回已处理的毫秒数。
pub fn parse_progress_line(line: &str) -> Option<u64> {
    let (k, v) = line.split_once('=')?;
    match k.trim() {
        // 两个键单位不同：out_time_us 是微秒，out_time_ms 在多数版本里也是微秒
        "out_time_us" | "out_time_ms" => v.trim().parse::<i64>().ok().filter(|v| *v >= 0).map(|v| v as u64 / 1000),
        _ => None,
    }
}

pub(crate) enum RunEnd {
    Ok,
    Canceled,
    Failed(String),
}

pub(crate) async fn run_ffmpeg_progress(
    ffmpeg: &Path,
    args: &[String],
    out_ms: Option<u64>,
    cancel: &AtomicBool,
    wake: &Notify,
    on_percent: &(dyn Fn(f64) + Send + Sync),
) -> RunEnd {
    let mut full = base_args();
    full.extend(s(&["-progress", "pipe:1", "-nostats"]));
    full.extend(args.iter().cloned());
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(&full).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return RunEnd::Failed(format!("无法运行 ffmpeg：{e}")),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let err_task = tokio::spawn(async move {
        let mut text = String::new();
        if let Some(mut e) = stderr {
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut e, &mut text).await;
        }
        text
    });
    let progress = async {
        if let Some(out) = stdout {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let (Some(ms), Some(total)) = (parse_progress_line(&line), out_ms) {
                    on_percent((ms as f64 / total.max(1) as f64 * 100.0).clamp(0.0, 99.0));
                }
            }
        }
    };
    let wait = async {
        progress.await;
        child.wait().await
    };
    let cancelled = async {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            wake.notified().await;
        }
    };
    tokio::select! {
        status = wait => {
            let err = err_task.await.unwrap_or_default();
            match status {
                Ok(s) if s.success() => RunEnd::Ok,
                Ok(_) => {
                    let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
                    let tail = tail.iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" ");
                    RunEnd::Failed(if tail.is_empty() { "ffmpeg 处理失败".into() } else { tail })
                }
                Err(e) => RunEnd::Failed(format!("ffmpeg 运行失败：{e}")),
            }
        }
        _ = cancelled => {
            // child 在 wait 的 future 里被借用，丢弃后由 kill_on_drop 结束进程
            RunEnd::Canceled
        }
    }
}

/// 后台任务的上下文：更新进度和说明，检查是否被取消。
#[derive(Clone)]
pub struct JobCtx {
    pub app: tauri::AppHandle,
    pub st: Arc<crate::AppState>,
    pub id: u64,
    pub cancel: Arc<AtomicBool>,
    pub wake: Arc<Notify>,
    last_emit: Arc<Mutex<std::time::Instant>>,
}

impl JobCtx {
    pub fn emit(&self) {
        use tauri::Emitter;
        let _ = self.app.emit(EVT_JOBS, self.st.media_jobs.snapshot());
    }

    /// 更新进度（0–100）；界面刷新限制在每 0.4 秒一次。
    pub fn percent(&self, p: f64) {
        self.st.media_jobs.update(self.id, |s| s.percent = p.clamp(0.0, 100.0));
        let mut t = self.last_emit.lock().unwrap_or_else(|e| e.into_inner());
        if t.elapsed() > std::time::Duration::from_millis(400) {
            *t = std::time::Instant::now();
            drop(t);
            self.emit();
        }
    }

    pub fn note(&self, note: impl Into<String>) {
        let note = note.into();
        self.st.media_jobs.update(self.id, |s| s.note = Some(note));
        self.emit();
    }

    pub fn canceled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// 已取消时返回错误，用在长流程的各个步骤之间。
    pub fn check(&self) -> AppResult<()> {
        if self.canceled() {
            Err(AppError::msg("canceled"))
        } else {
            Ok(())
        }
    }
}

/// 创建后台任务（和工具箱的 ffmpeg 任务共用任务列表、进度和取消），返回任务编号。
/// `f` 返回输出文件的路径；返回错误消息 `canceled` 表示用户取消。
pub fn spawn_job<F, Fut>(app: &tauri::AppHandle, title: String, label: &str, f: F) -> u64
where
    F: FnOnce(JobCtx) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = AppResult<PathBuf>> + Send + 'static,
{
    use tauri::Manager;
    let st = app.state::<Arc<crate::AppState>>().inner().clone();
    let (id, cancel, wake) = st.media_jobs.add(title, label);
    let ctx = JobCtx { app: app.clone(), st: st.clone(), id, cancel: cancel.clone(), wake, last_emit: Arc::new(Mutex::new(std::time::Instant::now())) };
    ctx.emit();
    tauri::async_runtime::spawn(async move {
        let permit = st.media_jobs.sem.clone().acquire_owned().await;
        if cancel.load(Ordering::Relaxed) {
            st.media_jobs.update(id, |s| {
                s.status = "canceled".into();
                s.finished_at = Some(crate::db::now());
            });
            ctx.emit();
            return;
        }
        st.media_jobs.update(id, |s| s.status = "running".into());
        ctx.emit();
        let result = f(ctx.clone()).await;
        st.media_jobs.update(id, |s| {
            s.finished_at = Some(crate::db::now());
            match &result {
                Ok(out) => {
                    s.status = "done".into();
                    s.percent = 100.0;
                    s.output = Some(out.to_string_lossy().into_owned());
                }
                Err(e) if e.message == "canceled" => s.status = "canceled".into(),
                Err(e) => {
                    s.status = "failed".into();
                    s.error = Some(e.message.clone());
                }
            }
        });
        ctx.emit();
        drop(permit);
    });
    id
}

/// 创建 ffmpeg 处理任务并在后台运行，返回任务编号。
pub fn start(app: &tauri::AppHandle, mut job: ToolJob) -> AppResult<u64> {
    use tauri::Manager;
    // 剪辑的素材就是工程里用到的文件，先在这里校验工程，错误马上提示，不用等任务启动
    if let ToolOp::Edit { project } = &job.op {
        job.inputs = project.clone().checked().map_err(AppError::invalid)?.media_paths();
    }
    if job.inputs.is_empty() {
        return Err(AppError::invalid("请先选择文件。"));
    }
    for i in &job.inputs {
        if !Path::new(i).is_file() {
            return Err(AppError::invalid(format!("找不到文件：{i}")));
        }
    }
    let st = app.state::<Arc<crate::AppState>>().inner().clone();
    find_ffmpeg(&st).ok_or_else(|| AppError::new(crate::error::ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”中安装 ffmpeg 后重试。"))?;
    let edit_title = match &job.op {
        ToolOp::Edit { project } if !project.title.trim().is_empty() => Some(project.title.trim().to_string()),
        _ => None,
    };
    let title = if let Some(t) = edit_title {
        t
    } else if job.inputs.len() > 1 {
        format!("{} 等 {} 个文件", stem_of(Path::new(&job.inputs[0])), job.inputs.len())
    } else {
        stem_of(Path::new(&job.inputs[0]))
    };
    let label = job.op.label();
    Ok(spawn_job(app, title, label, move |ctx| async move {
        let out = run_job(&ctx, &job).await?;
        // 视频规整发现素材已经符合规格时返回输入文件本身，没有新文件可登记
        if !job.inputs.iter().any(|i| Path::new(i) == out) {
            register_output(&ctx.app, &ctx.st, &job, &out).await;
        }
        Ok(out)
    }))
}

async fn run_job(ctx: &JobCtx, job: &ToolJob) -> AppResult<PathBuf> {
    match &job.op {
        ToolOp::Normalize { spec, .. } => return crate::vidnorm::job::run_normalize(ctx, job, spec).await,
        ToolOp::Stabilize { strength } => return crate::vidnorm::job::run_stabilize(ctx, job, strength).await,
        ToolOp::Edit { project } => return crate::edit::job::run_edit(ctx, job, project).await,
        _ => {}
    }
    let st = &ctx.st;
    let ffmpeg = find_ffmpeg(st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let mut inputs = vec![];
    for i in &job.inputs {
        let path = PathBuf::from(i);
        let pr = probe(&ffmpeg, &path).await;
        if let Some(e) = &pr.error {
            return Err(AppError::invalid(format!("“{}”无法读取：{e}", stem_of(&path))));
        }
        inputs.push(Input { path, probe: pr });
    }
    let caps = st.vcaps.get(&ffmpeg).await;
    let plan = match &job.op {
        ToolOp::AudioFx { spec } => {
            let first = inputs.first().ok_or_else(|| AppError::invalid("请先选择文件。"))?;
            let dir = job.output_dir.as_deref().filter(|d| !d.trim().is_empty()).map(Path::new);
            let rnn = crate::audiofx::rnn_model_path(&st.data_dir);
            let nf = if spec.denoise != "off" && !rnn.is_file() { crate::audiofx::estimate_noise_floor(&ffmpeg, &first.path, 0.0, 30).await } else { None };
            crate::audiofx::plan(first, spec, &caps, &caps.encoders, dir, Some(rnn), nf)?
        }
        _ => plan_with(job, &inputs, &caps.encoders, caps.background_blur())?,
    };
    if let Some(n) = &plan.note {
        ctx.note(n.clone());
    }
    let on_percent = |p: f64| ctx.percent(p);
    let mut end = run_ffmpeg_progress(&ffmpeg, &plan.args, plan.out_ms, &ctx.cancel, &ctx.wake, &on_percent).await;
    if let (RunEnd::Failed(_), Some(fb)) = (&end, &plan.fallback) {
        let _ = std::fs::remove_file(&plan.output);
        ctx.note("两个文件的编码参数不一致，无法直接拼接，已改为重新编码。");
        end = run_ffmpeg_progress(&ffmpeg, fb, plan.out_ms, &ctx.cancel, &ctx.wake, &on_percent).await;
    }
    for t in &plan.temp_files {
        let _ = std::fs::remove_file(t);
    }
    match end {
        RunEnd::Ok => {
            let nonempty = if plan.is_dir {
                std::fs::read_dir(&plan.output).map(|mut d| d.next().is_some()).unwrap_or(false)
            } else {
                std::fs::metadata(&plan.output).map(|m| m.len() > 0).unwrap_or(false)
            };
            if !nonempty {
                let _ = if plan.is_dir { std::fs::remove_dir(&plan.output) } else { std::fs::remove_file(&plan.output) };
                return Err(AppError::msg("没有生成任何输出，可能是时间点超出了视频长度。"));
            }
            Ok(plan.output)
        }
        RunEnd::Canceled => {
            if plan.is_dir {
                let _ = std::fs::remove_dir_all(&plan.output);
            } else {
                let _ = std::fs::remove_file(&plan.output);
            }
            Err(AppError::msg("canceled"))
        }
        RunEnd::Failed(msg) => {
            if plan.is_dir {
                let _ = std::fs::remove_dir(&plan.output);
            } else {
                let _ = std::fs::remove_file(&plan.output);
            }
            Err(AppError::msg(msg))
        }
    }
}

/// 把输出文件登记进媒体库（截图文件夹不登记）。
async fn register_output(app: &tauri::AppHandle, st: &Arc<crate::AppState>, job: &ToolJob, out: &Path) {
    use tauri::Emitter;
    if out.is_dir() {
        return;
    }
    let kind = match out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("gif" | "jpg" | "jpeg" | "png") => "image",
        Some("mp3" | "m4a" | "flac" | "opus" | "wav") => "audio",
        _ => "video",
    };
    let path = out.to_string_lossy().into_owned();
    let media_id = format!("tool-{}", crate::library::path_key(out));
    let size = std::fs::metadata(out).map(|m| m.len() as i64).unwrap_or(0);
    let ok = st
        .db
        .record_download(&crate::db::NewDownload {
            platform: "local",
            media_id: &media_id,
            asset_id: "file",
            title: &stem_of(out),
            author: "",
            cover: None,
            path: &path,
            size,
            kind,
            source: "tool",
            source_url: "",
            platform_name: "本地处理",
        })
        .is_ok();
    if ok {
        let _ = st.db.set_item_tags_by_key("local", &media_id, "file", &["工具箱".to_string(), job.op.label().to_string()]);
        let _ = app.emit("library://changed", ());
        crate::library_cmds::after_download(app.clone(), "local".into(), media_id, "file".into()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_bin() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-mt-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake(path: &str, video: bool, audio: bool, dur: u64) -> Input {
        Input {
            path: PathBuf::from(path),
            probe: Probe { duration_ms: Some(dur), has_video: video, has_audio: audio, width: Some(1920), height: Some(1080), ..Default::default() },
        }
    }

    fn job(inputs: &[&str], op: ToolOp) -> ToolJob {
        ToolJob { inputs: inputs.iter().map(|s| s.to_string()).collect(), op, output_dir: None }
    }

    fn all_enc() -> Encoders {
        Encoders { h264: Some("libx264"), vp9: true, mp3: true, opus: true }
    }

    #[test]
    fn encoder_detection() {
        let out = " V....D libx264              libx264 H.264\n V....D libvpx-vp9           libvpx VP9\n A....D libmp3lame           MP3\n A....D aac                  AAC\n";
        let e = parse_encoders(out);
        assert_eq!(e, Encoders { h264: Some("libx264"), vp9: true, mp3: true, opus: false });
        assert_eq!(parse_encoders(" V..... libopenh264 x\n").h264, Some("libopenh264"));
        assert_eq!(parse_encoders("").h264, None);
    }

    #[test]
    fn atempo_is_chained_within_limits() {
        assert_eq!(atempo_chain(1.5), "atempo=1.5000");
        assert_eq!(atempo_chain(4.0), "atempo=2.0,atempo=2.0000");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.5000");
        assert_eq!(atempo_chain(3.0), "atempo=2.0,atempo=1.5000");
    }

    #[test]
    fn ops_serialize_in_camel_case() {
        let j: ToolJob = serde_json::from_str(r#"{"inputs":["a.mp4"],"op":"gif","startMs":1000,"durationMs":3000,"width":480,"fps":12}"#).unwrap();
        assert_eq!(j.op, ToolOp::Gif { start_ms: 1000, duration_ms: 3000, width: 480, fps: 12 });
        let j: ToolJob = serde_json::from_str(r#"{"inputs":["a.mp3"],"op":"tags","title":"歌","cover":null}"#).unwrap();
        assert!(matches!(j.op, ToolOp::Tags { title: Some(ref t), artist: None, .. } if t == "歌"));
        assert!(serde_json::from_str::<ToolJob>(r#"{"inputs":[],"op":"nope"}"#).is_err());
    }

    #[test]
    fn encoder_fallbacks_are_reported() {
        let no_x264 = Encoders { h264: None, vp9: true, mp3: true, opus: true };
        let p = plan(
            &job(&["/m/a.mp4"], ToolOp::Compress { quality: "balanced".into(), max_height: Some(720) }),
            &[fake("/m/a.mp4", true, true, 10_000)],
            &no_x264,
        )
        .unwrap();
        assert!(p.output.to_string_lossy().ends_with("a.压缩.mkv"), "{:?}", p.output);
        assert!(p.args.contains(&"libvpx-vp9".to_string()));
        assert!(p.note.as_deref().unwrap().contains("H.264"));
        assert!(p.args.iter().any(|a| a == "scale=-2:'min(ih,720)'"));
        let p = plan(&job(&["/m/a.mp4"], ToolOp::Compress { quality: "small".into(), max_height: None }), &[fake("/m/a.mp4", true, true, 10_000)], &all_enc())
            .unwrap();
        assert!(p.args.windows(2).any(|w| w == ["-crf", "30"]));
        assert!(p.output.to_string_lossy().ends_with(".mp4") && p.note.is_none());
        let none = Encoders::default();
        let p = plan(&job(&["/m/a.mp4"], ToolOp::Compress { quality: "high".into(), max_height: None }), &[fake("/m/a.mp4", true, true, 1000)], &none).unwrap();
        assert!(p.args.contains(&"mpeg4".to_string()));
    }

    #[test]
    fn landscape_blur_follows_the_available_filter() {
        let landscape = || job(&["/m/a.mp4"], ToolOp::Landscape { width: 1920, height: 1080, mode: "blur".into() });
        let inputs = [fake("/m/a.mp4", true, true, 10_000)];
        let graph = |p: &Plan| p.args[p.args.iter().position(|a| a == "-filter_complex").unwrap() + 1].clone();
        let boxblur = graph(&plan(&landscape(), &inputs, &all_enc()).unwrap());
        assert!(boxblur.contains("crop=1920:1080,boxblur=25:5[bg]"), "{boxblur}");
        let gblur = graph(&plan_with(&landscape(), &inputs, &all_enc(), "gblur=sigma=30").unwrap());
        assert!(gblur.contains("crop=1920:1080,gblur=sigma=30[bg]") && !gblur.contains("boxblur"), "精简版 ffmpeg 没有 boxblur：{gblur}");
    }

    #[test]
    fn invalid_jobs_are_rejected() {
        let audio = fake("/m/a.mp3", false, true, 5000);
        let video = fake("/m/a.mp4", true, true, 5000);
        let e = all_enc();
        assert!(
            plan(&job(&["/m/a.mp3"], ToolOp::Compress { quality: "small".into(), max_height: None }), std::slice::from_ref(&audio), &e).is_err(),
            "audio cannot be compressed as video"
        );
        assert!(plan(&job(&["/m/a.mp4"], ToolOp::Rotate { mode: "weird".into() }), std::slice::from_ref(&video), &e).is_err());
        assert!(plan(&job(&["/m/a.mp4"], ToolOp::Concat { reencode: false }), std::slice::from_ref(&video), &e).is_err(), "one file");
        assert!(plan(&job(&["/m/a.mp4", "/m/a.mp3"], ToolOp::Concat { reencode: false }), &[video.clone(), audio.clone()], &e).is_err(), "mixed");
        assert!(plan(&job(&["/m/a.mp4"], ToolOp::Trim { start_ms: 5000, end_ms: Some(1000), precise: false }), std::slice::from_ref(&video), &e).is_err());
        assert!(plan(&job(&["/m/a.mp4"], ToolOp::Convert { format: "exe".into(), reencode: false }), std::slice::from_ref(&video), &e).is_err());
        assert!(
            plan(
                &job(&["/m/a.mp3"], ToolOp::Convert { format: "mp3".into(), reencode: false }),
                std::slice::from_ref(&audio),
                &Encoders { mp3: false, ..e.clone() }
            )
            .is_err(),
            "no mp3 encoder"
        );
        assert!(plan(&job(&["/m/a.mp3"], ToolOp::Mute), &[audio], &e).is_err());
        assert!(plan(&job(&[], ToolOp::Mute), &[], &e).is_err());
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress_line("out_time_us=2500000"), Some(2500));
        assert_eq!(parse_progress_line("out_time_ms=1500000"), Some(1500));
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("progress=continue"), None);
        assert_eq!(parse_progress_line("out_time_us=-9223372036854775807"), None);
    }

    // ---- 需要 ffmpeg 的测试 ----

    async fn make(ff: &Path, out: &Path, src: &str, secs: u32, size: &str, audio: bool) {
        let mut a = base_args();
        a.extend(["-f".to_string(), "lavfi".into(), "-i".into(), format!("{src}=size={size}:rate=25")]);
        if audio {
            a.extend(["-f".to_string(), "lavfi".into(), "-i".into(), "sine=frequency=440".into()]);
        }
        a.extend(["-t".to_string(), secs.to_string(), "-c:v".into(), "mpeg4".into(), "-g".into(), "25".into(), "-pix_fmt".into(), "yuv420p".into()]);
        if audio {
            a.extend(["-c:a".to_string(), "aac".into()]);
        }
        a.push(out.to_string_lossy().into_owned());
        crate::postprocess::run_ffmpeg(ff, &a).await.unwrap();
    }

    /// 走和真实任务一样的流程：探测 → 计划 → 运行（含直接拼接失败时的回退），返回输出路径。
    async fn exec(ff: &Path, j: &ToolJob) -> (PathBuf, Option<String>) {
        let mut inputs = vec![];
        for i in &j.inputs {
            inputs.push(Input { path: PathBuf::from(i), probe: probe(ff, Path::new(i)).await });
        }
        let enc = detect_encoders(ff).await;
        let caps = crate::vidcaps::detect(ff).await;
        let p = plan_with(j, &inputs, &enc, caps.background_blur()).unwrap();
        let (cancel, wake) = (AtomicBool::new(false), Notify::new());
        let seen = Mutex::new(0.0f64);
        let on = |v: f64| *seen.lock().unwrap() = v;
        let mut end = run_ffmpeg_progress(ff, &p.args, p.out_ms, &cancel, &wake, &on).await;
        if let (RunEnd::Failed(_), Some(fb)) = (&end, &p.fallback) {
            end = run_ffmpeg_progress(ff, fb, p.out_ms, &cancel, &wake, &on).await;
        }
        match end {
            RunEnd::Ok => {}
            RunEnd::Failed(m) => panic!("{:?} failed: {m}\nargs: {:?}", j.op, p.args),
            RunEnd::Canceled => panic!("canceled"),
        }
        for t in &p.temp_files {
            let _ = std::fs::remove_file(t);
        }
        (p.output, p.note)
    }

    #[tokio::test]
    async fn every_video_operation_produces_a_valid_file() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("ops");
        let v = dir.join("v.mp4");
        make(&ff, &v, "testsrc", 4, "320x240", true).await;
        let vs = v.to_string_lossy().into_owned();
        let pr = |p: PathBuf| {
            let ff = ff.clone();
            async move { probe(&ff, &p).await }
        };

        // 压缩并限制高度
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Compress { quality: "small".into(), max_height: Some(144) })).await;
        let p = pr(out).await;
        assert!(p.readable() && p.has_audio, "{p:?}");
        assert_eq!(p.height, Some(144));

        // GIF
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Gif { start_ms: 500, duration_ms: 2000, width: 160, fps: 10 })).await;
        assert_eq!(&std::fs::read(&out).unwrap()[..4], b"GIF8");

        // 竖转横：先做一个竖屏视频
        let tall = dir.join("tall.mp4");
        make(&ff, &tall, "testsrc", 2, "180x320", true).await;
        let (out, _) = exec(&ff, &job(&[&tall.to_string_lossy()], ToolOp::Landscape { width: 640, height: 360, mode: "blur".into() })).await;
        let p = pr(out).await;
        assert_eq!((p.width, p.height), (Some(640), Some(360)));
        assert!(p.has_audio);
        let (out, _) = exec(&ff, &job(&[&tall.to_string_lossy()], ToolOp::Landscape { width: 640, height: 360, mode: "black".into() })).await;
        assert_eq!(pr(out).await.width, Some(640));

        // 2 倍速：4 秒 → 约 2 秒
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Speed { factor: 2.0 })).await;
        let d = pr(out).await.duration_ms.unwrap();
        assert!((1700..=2400).contains(&d), "{d}");

        // 响度
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Loudness { target: -16.0 })).await;
        let p = pr(out).await;
        assert!(p.has_video && p.has_audio);

        // 旋转：宽高互换
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Rotate { mode: "cw".into() })).await;
        let p = pr(out).await;
        assert_eq!((p.width, p.height), (Some(240), Some(320)));

        // 转封装（不重新编码）与转音频
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Convert { format: "mkv".into(), reencode: false })).await;
        assert!(out.to_string_lossy().ends_with(".mkv") && pr(out).await.readable());
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Convert { format: "m4a".into(), reencode: false })).await;
        let p = pr(out).await;
        assert!(p.has_audio && !p.has_video);
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Convert { format: "flac".into(), reencode: false })).await;
        assert!(pr(out).await.has_audio);

        // 截图
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Frame { at_ms: 1500, format: "png".into() })).await;
        assert_eq!(&std::fs::read(&out).unwrap()[1..4], b"PNG");
        let (folder, _) = exec(&ff, &job(&[&vs], ToolOp::Frames { every_secs: 1.0, format: "jpg".into() })).await;
        assert!(std::fs::read_dir(&folder).unwrap().count() >= 3);

        // 裁剪：精确与不重新编码
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Trim { start_ms: 1000, end_ms: Some(3000), precise: true })).await;
        let d = pr(out).await.duration_ms.unwrap();
        assert!((1800..=2300).contains(&d), "{d}");
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Trim { start_ms: 1000, end_ms: None, precise: false })).await;
        let d = pr(out).await.duration_ms.unwrap();
        assert!((2800..=3300).contains(&d), "{d}");

        // 去除音轨
        let (out, _) = exec(&ff, &job(&[&vs], ToolOp::Mute)).await;
        let p = pr(out).await;
        assert!(p.has_video && !p.has_audio);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn concat_copy_and_reencode_paths() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("concat");
        let (a, b, c) = (dir.join("a.mp4"), dir.join("b.mp4"), dir.join("c.mp4"));
        make(&ff, &a, "testsrc", 2, "320x240", true).await;
        make(&ff, &b, "testsrc2", 3, "320x240", true).await;
        make(&ff, &c, "testsrc", 2, "160x120", true).await;
        let (sa, sb, sc) = (a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned(), c.to_string_lossy().into_owned());
        // 参数一致：直接拼接
        let (out, note) = exec(&ff, &job(&[&sa, &sb], ToolOp::Concat { reencode: false })).await;
        assert!(note.is_none());
        let d = probe(&ff, &out).await.duration_ms.unwrap();
        assert!((4800..=5300).contains(&d), "{d}");
        // 尺寸不同：自动重新编码并统一尺寸
        let (out, note) = exec(&ff, &job(&[&sa, &sc], ToolOp::Concat { reencode: false })).await;
        assert!(note.as_deref().unwrap().contains("尺寸"));
        let p = probe(&ff, &out).await;
        assert_eq!((p.width, p.height), (Some(320), Some(240)));
        let d = p.duration_ms.unwrap();
        assert!((3600..=4500).contains(&d), "{d}");
        // 强制重新编码
        let (out, _) = exec(&ff, &job(&[&sa, &sb], ToolOp::Concat { reencode: true })).await;
        assert!(probe(&ff, &out).await.readable());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn audio_tags_are_written_and_read_back() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("tags");
        let v = dir.join("s.mp4");
        make(&ff, &v, "testsrc", 2, "64x64", true).await;
        let m4a = dir.join("song.m4a");
        let mut a = base_args();
        a.extend(["-i".to_string(), v.to_string_lossy().into_owned(), "-vn".into(), "-c:a".into(), "aac".into(), m4a.to_string_lossy().into_owned()]);
        crate::postprocess::run_ffmpeg(&ff, &a).await.unwrap();
        let cover = dir.join("cover.jpg");
        let mut a = base_args();
        a.extend([
            "-f".to_string(),
            "lavfi".into(),
            "-i".into(),
            "testsrc=size=200x200".into(),
            "-frames:v".into(),
            "1".into(),
            cover.to_string_lossy().into_owned(),
        ]);
        crate::postprocess::run_ffmpeg(&ff, &a).await.unwrap();
        let j = job(
            &[&m4a.to_string_lossy()],
            ToolOp::Tags {
                title: Some("晴天".into()),
                artist: Some("周杰伦".into()),
                album: Some("叶惠美".into()),
                year: Some("2003".into()),
                genre: None,
                comment: None,
                cover: Some(cover.to_string_lossy().into_owned()),
            },
        );
        let (out, _) = exec(&ff, &j).await;
        let p = probe(&ff, &out).await;
        assert_eq!(p.tags.get("title").map(String::as_str), Some("晴天"));
        assert_eq!(p.tags.get("artist").map(String::as_str), Some("周杰伦"));
        assert_eq!(p.tags.get("album").map(String::as_str), Some("叶惠美"));
        assert!(p.has_audio);
        // 再清除标题，保留其他
        let j2 = job(
            &[&out.to_string_lossy()],
            ToolOp::Tags { title: Some(String::new()), artist: None, album: None, year: None, genre: None, comment: None, cover: None },
        );
        let (out2, _) = exec(&ff, &j2).await;
        let p2 = probe(&ff, &out2).await;
        assert!(!p2.tags.contains_key("title"), "{:?}", p2.tags);
        assert_eq!(p2.tags.get("artist").map(String::as_str), Some("周杰伦"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn canceling_stops_the_process() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("cancel");
        let v = dir.join("long.mp4");
        make(&ff, &v, "testsrc", 30, "640x480", true).await;
        let inputs = vec![Input { path: v.clone(), probe: probe(&ff, &v).await }];
        let p = plan(&job(&[&v.to_string_lossy()], ToolOp::Speed { factor: 0.5 }), &inputs, &detect_encoders(&ff).await).unwrap();
        let (cancel, wake) = (Arc::new(AtomicBool::new(false)), Arc::new(Notify::new()));
        let (c2, w2) = (cancel.clone(), wake.clone());
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            c2.store(true, Ordering::Relaxed);
            w2.notify_waiters();
        });
        let started = std::time::Instant::now();
        let end = run_ffmpeg_progress(&ff, &p.args, p.out_ms, &cancel, &wake, &|_| {}).await;
        assert!(matches!(end, RunEnd::Canceled));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 管理的 ffmpeg 是 LGPL 版（没有 x264）：VP9 和 MPEG-4 两条回退路径都要真的能跑。
    #[tokio::test]
    async fn lgpl_style_encoder_fallbacks_really_encode() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("lgpl");
        let v = dir.join("v.mp4");
        make(&ff, &v, "testsrc", 2, "160x120", true).await;
        let inputs = vec![Input { path: v.clone(), probe: probe(&ff, &v).await }];
        let real = detect_encoders(&ff).await;
        for enc in [Encoders { h264: None, vp9: real.vp9, mp3: false, opus: real.opus }, Encoders::default()] {
            {
                let p = plan(&job(&[&v.to_string_lossy()], ToolOp::Compress { quality: "small".into(), max_height: None }), &inputs, &enc).unwrap();
                let (cancel, wake) = (AtomicBool::new(false), Notify::new());
                let end = run_ffmpeg_progress(&ff, &p.args, p.out_ms, &cancel, &wake, &|_| {}).await;
                assert!(matches!(end, RunEnd::Ok), "{:?} {:?}", p.args, p.note);
                let pr = probe(&ff, &p.output).await;
                assert!(pr.readable() && pr.has_video, "{pr:?}");
                assert!(p.note.is_some());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chapter_metadata_is_escaped_and_closed() {
        use crate::model::Chapter;
        let ch = vec![Chapter { title: "开场=介绍;#1\\".into(), start_ms: 0, end_ms: 0 }, Chapter { title: "正片".into(), start_ms: 3000, end_ms: 0 }];
        let m = ffmetadata(&ch, Some(10_000));
        assert!(m.starts_with(";FFMETADATA1\n"));
        assert!(m.contains("START=0\nEND=3000\ntitle=开场\\=介绍\\;\\#1\\\\\n"), "{m}");
        assert!(m.contains("START=3000\nEND=10000\ntitle=正片"), "{m}");
        assert!(ffmetadata(&ch[1..], None).contains("END=4000"), "last chapter without a known length gets 1 s");
    }

    #[tokio::test]
    async fn chapters_are_written_into_the_file() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("chapters");
        let v = dir.join("v.mp4");
        make(&ff, &v, "testsrc", 6, "160x120", true).await;
        let ch = vec![
            crate::model::Chapter { title: "开场".into(), start_ms: 0, end_ms: 3000 },
            crate::model::Chapter { title: "正片".into(), start_ms: 3000, end_ms: 6000 },
        ];
        let (out, _) = exec(&ff, &job(&[&v.to_string_lossy()], ToolOp::Chapters { chapters: ch })).await;
        let info = std::process::Command::new("ffmpeg").args(["-hide_banner", "-i"]).arg(&out).output().unwrap();
        let text = String::from_utf8_lossy(&info.stderr).into_owned();
        assert!(text.contains("Chapter #0:0") && text.contains("Chapter #0:1"), "{text}");
        assert!(text.contains("开场") && text.contains("正片"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
