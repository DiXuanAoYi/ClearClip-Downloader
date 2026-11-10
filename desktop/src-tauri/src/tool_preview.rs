//! 工具箱的预览：旋转 / 翻转、竖屏转横屏的“实时”一帧对比，压缩 / 倍速 / 转 GIF 的“试一小段”。
//!
//! 预览文件放在数据目录的 `previews` 里，两小时后清理；不进媒体库、不改原文件。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult, ErrorKind};
use crate::media_cmds::short_hash;
use crate::media_tools::{landscape_graph, plan_with, rotate_filter, Input, ToolJob, ToolOp};
use crate::postprocess::{probe, run_ffmpeg_capture};
use crate::vidnorm::analyze;
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

fn need_ffmpeg() -> AppError {
    AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。")
}

/// 清理预览目录里两小时前、以 `prefix` 开头的文件。
fn sweep(dir: &Path, prefix: &str) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(7200));
        if old && e.file_name().to_string_lossy().starts_with(prefix) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum FrameOp {
    /// 只取这一帧（截图、裁剪的起止点）
    None,
    Rotate {
        mode: String,
    },
    Landscape {
        width: u32,
        height: u32,
        mode: String,
    },
}

/// 把竖屏转横屏的画布缩到宽度不超过 960，预览更快，构图和成片一样。
pub(crate) fn preview_canvas(width: u32, height: u32) -> (u32, u32) {
    let (w, h) = (width.clamp(320, 3840), height.clamp(240, 2160));
    let k = (960.0 / w as f64).min(1.0);
    ((((w as f64 * k) as u32) & !1).max(2), (((h as f64 * k) as u32) & !1).max(2))
}

pub(crate) fn frame_graph(op: &FrameOp, blur: &str) -> AppResult<Option<String>> {
    Ok(match op {
        FrameOp::None => None,
        FrameOp::Rotate { mode } => {
            let vf = rotate_filter(mode).ok_or_else(|| AppError::invalid("不支持的旋转方式。"))?;
            Some(format!("[0:v:0]{vf}[v]"))
        }
        FrameOp::Landscape { width, height, mode } => {
            let (w, h) = preview_canvas(*width, *height);
            Some(landscape_graph(w, h, mode, blur))
        }
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePreview {
    pub before: String,
    pub after: String,
    pub changed: bool,
}

/// 取 `at_ms` 那一帧的“处理前 / 处理后”两张图（宽度不超过 960）。
#[tauri::command]
pub async fn tool_frame_preview(state: St<'_>, path: String, at_ms: u64, op: FrameOp) -> AppResult<FramePreview> {
    let (ff, caps) = crate::vidcaps::current(&state).await.ok_or_else(need_ffmpeg)?;
    frame_preview(&ff, caps.background_blur(), &state.data_dir.join("previews"), &path, at_ms, &op).await
}

pub(crate) async fn frame_preview(ff: &Path, blur: &str, dir: &Path, path: &str, at_ms: u64, op: &FrameOp) -> AppResult<FramePreview> {
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let graph = frame_graph(op, blur)?;
    sweep(dir, "frm-");
    let key = format!("frm-{}", short_hash(&format!("{path}|{at_ms}|{graph:?}")));
    let (before, after) = analyze::preview(ff, file, at_ms, 0, graph.as_deref(), dir, &key).await?;
    Ok(FramePreview { before: before.to_string_lossy().into_owned(), after: after.to_string_lossy().into_owned(), changed: graph.is_some() })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Trial {
    /// 原片这一小段（直接截取，不重新编码）
    pub before: String,
    /// 处理后这一小段（转 GIF 时是一张动图）
    pub after: String,
    pub after_is_image: bool,
    pub before_bytes: u64,
    pub after_bytes: u64,
    /// 试的这一小段多长（秒）
    pub secs: f64,
    /// 整段视频多长（秒），用来粗略估算整段的体积
    pub total_secs: Option<f64>,
    pub note: Option<String>,
}

/// 只对 `ops` 里这几样做“试一小段”：其余的没有意义（转换格式、去音轨）或另有预览。
pub(crate) fn trial_allowed(op: &ToolOp) -> bool {
    matches!(op, ToolOp::Compress { .. } | ToolOp::Speed { .. } | ToolOp::Gif { .. })
}

/// 试一小段：从 `start_ms` 起取 `dur_ms`（默认 5 秒，1–10 秒），按同样的参数真的处理一遍，对比前后。
#[tauri::command]
pub async fn tool_trial(state: St<'_>, path: String, op: ToolOp, start_ms: u64, dur_ms: Option<u64>) -> AppResult<Trial> {
    let (ff, caps) = crate::vidcaps::current(&state).await.ok_or_else(need_ffmpeg)?;
    trial(&ff, &caps, &state.data_dir.join("previews"), &path, op, start_ms, dur_ms).await
}

pub(crate) async fn trial(ff: &Path, caps: &crate::vidcaps::Caps, dir: &Path, path: &str, op: ToolOp, start_ms: u64, dur_ms: Option<u64>) -> AppResult<Trial> {
    if !trial_allowed(&op) {
        return Err(AppError::invalid("这个工具不支持试一小段。"));
    }
    let file = Path::new(path);
    if !file.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let ff = ff.to_path_buf();
    let src = probe(&ff, file).await;
    if let Some(e) = &src.error {
        return Err(AppError::invalid(format!("无法读取这个文件：{e}")));
    }
    if !src.has_video {
        return Err(AppError::invalid("文件里没有视频，没法试。"));
    }
    let secs_ms = dur_ms.unwrap_or(5_000).clamp(1_000, 10_000);
    // 起点不能让这一小段超出视频结尾
    let start = match src.duration_ms {
        Some(total) => start_ms.min(total.saturating_sub(secs_ms.min(total))),
        None => start_ms,
    };
    std::fs::create_dir_all(dir)?;
    sweep(dir, "trial-");
    let key = format!("trial-{}", short_hash(&format!("{path}|{start}|{secs_ms}|{}", serde_json::to_string(&op).unwrap_or_default())));

    // 1) 原片这一小段：直接截取，不重新编码
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "mp4" | "mov" | "m4v" | "webm"))
        .unwrap_or_else(|| "mkv".into());
    let cut: PathBuf = dir.join(format!("{key}-src.{ext}"));
    let mut cut_args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-ss"].iter().map(|s| s.to_string()).collect();
    cut_args.push(format!("{:.3}", start as f64 / 1000.0));
    cut_args.extend(["-t".to_string(), format!("{:.3}", secs_ms as f64 / 1000.0), "-i".to_string(), file.to_string_lossy().into_owned()]);
    cut_args.extend(["-map", "0:v:0", "-map", "0:a:0?", "-c", "copy"].iter().map(|s| s.to_string()));
    cut_args.push(cut.to_string_lossy().into_owned());
    run_ffmpeg_capture(&ff, &cut_args, Duration::from_secs(120)).await?;

    // 2) 同样的参数处理这一小段。转 GIF 的起点在截下来的小段里是 0
    let op = match op {
        ToolOp::Gif { duration_ms, width, fps, .. } => ToolOp::Gif { start_ms: 0, duration_ms: duration_ms.min(secs_ms), width, fps },
        other => other,
    };
    let cut_probe = probe(&ff, &cut).await;
    if let Some(e) = &cut_probe.error {
        return Err(AppError::invalid(format!("截取的小段无法读取：{e}")));
    }
    let job = ToolJob { inputs: vec![cut.to_string_lossy().into_owned()], op, output_dir: Some(dir.to_string_lossy().into_owned()) };
    let plan = plan_with(&job, &[Input { path: cut.clone(), probe: cut_probe }], &caps.encoders, caps.background_blur())?;
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"].iter().map(|s| s.to_string()).collect();
    args.extend(plan.args.iter().cloned());
    let ran = run_ffmpeg_capture(&ff, &args, Duration::from_secs(300)).await;
    for t in &plan.temp_files {
        let _ = std::fs::remove_file(t);
    }
    ran?;
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let after_bytes = size(&plan.output);
    if after_bytes == 0 {
        return Err(AppError::msg("没有生成预览，可能是这一小段处理失败。"));
    }
    Ok(Trial {
        before: cut.to_string_lossy().into_owned(),
        after: plan.output.to_string_lossy().into_owned(),
        after_is_image: plan.kind == "image",
        before_bytes: size(&cut),
        after_bytes,
        secs: secs_ms as f64 / 1000.0,
        total_secs: src.duration_ms.map(|d| d as f64 / 1000.0),
        note: plan.note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-tp-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn make(ff: &Path, out: &Path, size: &str, secs: u32) {
        let a: Vec<String> = [
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size={size}:rate=25"),
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440",
            "-t",
            &secs.to_string(),
            "-c:v",
            "mpeg4",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain([out.to_string_lossy().into_owned()])
        .collect();
        run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap();
    }

    #[tokio::test]
    async fn trial_and_frame_previews_really_run() {
        let Some(ff) = ffmpeg() else { return };
        let d = temp("e2e");
        let v = d.join("v.mp4");
        make(&ff, &v, "320x240", 8).await;
        let vs = v.to_string_lossy().into_owned();
        let caps = crate::vidcaps::detect(&ff).await;
        let pv = d.join("previews");

        // 压缩：试 3 秒；原片这一小段和处理后都能读，体积都不为 0
        let t = trial(&ff, &caps, &pv, &vs, ToolOp::Compress { quality: "small".into(), max_height: Some(144) }, 2_000, Some(3_000)).await.unwrap();
        assert_eq!(t.secs, 3.0);
        assert!(t.before_bytes > 0 && t.after_bytes > 0 && !t.after_is_image);
        let p = probe(&ff, Path::new(&t.after)).await;
        assert_eq!(p.height, Some(144), "{p:?}");
        assert!(p.duration_ms.is_some_and(|d| (2_500..=3_500).contains(&d)), "{p:?}");

        // 起点太靠后：这一小段仍在视频里
        let t = trial(&ff, &caps, &pv, &vs, ToolOp::Speed { factor: 2.0 }, 99_000, Some(4_000)).await.unwrap();
        let p = probe(&ff, Path::new(&t.after)).await;
        assert!(p.duration_ms.is_some_and(|d| (1_500..=2_500).contains(&d)), "2 倍速的 4 秒应约 2 秒：{p:?}");

        // 转 GIF：预览是一张动图
        let t = trial(&ff, &caps, &pv, &vs, ToolOp::Gif { start_ms: 0, duration_ms: 2_000, width: 160, fps: 10 }, 1_000, None).await.unwrap();
        assert!(t.after_is_image);
        assert_eq!(&std::fs::read(&t.after).unwrap()[..4], b"GIF8");

        // 不支持的工具、不存在的文件
        assert!(trial(&ff, &caps, &pv, &vs, ToolOp::Mute, 0, None).await.is_err());
        assert!(trial(&ff, &caps, &pv, "/no/such.mp4", ToolOp::Speed { factor: 2.0 }, 0, None).await.is_err());

        // 旋转：横屏 320×240 顺时针转 90° 后是 240 宽 320 高；竖屏转横屏的画面是 16:9
        let r = frame_preview(&ff, caps.background_blur(), &pv, &vs, 1_000, &FrameOp::Rotate { mode: "cw".into() }).await.unwrap();
        assert!(r.changed);
        let (b, a) = (probe(&ff, Path::new(&r.before)).await, probe(&ff, Path::new(&r.after)).await);
        assert_eq!((b.width, b.height), (Some(320), Some(240)));
        assert_eq!((a.width, a.height), (Some(240), Some(320)));
        let tall = d.join("tall.mp4");
        make(&ff, &tall, "180x320", 2).await;
        let l = frame_preview(
            &ff,
            caps.background_blur(),
            &pv,
            &tall.to_string_lossy(),
            500,
            &FrameOp::Landscape { width: 1920, height: 1080, mode: "blur".into() },
        )
        .await
        .unwrap();
        let a = probe(&ff, Path::new(&l.after)).await;
        assert_eq!((a.width, a.height), (Some(960), Some(540)));
        // 不变的操作：前后同一帧
        let n = frame_preview(&ff, caps.background_blur(), &pv, &vs, 1_000, &FrameOp::None).await.unwrap();
        assert!(!n.changed);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rotate_graph_uses_the_same_filter_as_the_job() {
        let g = frame_graph(&FrameOp::Rotate { mode: "cw".into() }, "boxblur=25:5").unwrap().unwrap();
        assert_eq!(g, "[0:v:0]transpose=1[v]");
        assert!(frame_graph(&FrameOp::Rotate { mode: "bogus".into() }, "x").is_err());
        assert!(frame_graph(&FrameOp::None, "x").unwrap().is_none());
    }

    #[test]
    fn landscape_preview_is_scaled_down_but_keeps_the_shape() {
        assert_eq!(preview_canvas(1920, 1080), (960, 540));
        assert_eq!(preview_canvas(3840, 2160), (960, 540));
        // 已经比 960 小的不放大
        assert_eq!(preview_canvas(640, 360), (640, 360));
        let g = frame_graph(&FrameOp::Landscape { width: 1920, height: 1080, mode: "black".into() }, "x").unwrap().unwrap();
        assert!(g.contains("scale=960:540") && g.contains("pad=960:540"), "{g}");
        let g = frame_graph(&FrameOp::Landscape { width: 1920, height: 1080, mode: "blur".into() }, "gblur=sigma=20").unwrap().unwrap();
        assert!(g.contains("gblur=sigma=20") && g.contains("crop=960:540"), "{g}");
    }

    #[test]
    fn only_slow_encodes_can_be_trialed() {
        assert!(trial_allowed(&ToolOp::Compress { quality: "balanced".into(), max_height: None }));
        assert!(trial_allowed(&ToolOp::Speed { factor: 2.0 }));
        assert!(trial_allowed(&ToolOp::Gif { start_ms: 0, duration_ms: 3000, width: 480, fps: 12 }));
        assert!(!trial_allowed(&ToolOp::Rotate { mode: "cw".into() }));
        assert!(!trial_allowed(&ToolOp::Mute));
    }
}
