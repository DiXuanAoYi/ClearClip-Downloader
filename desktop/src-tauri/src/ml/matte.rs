//! 人像抠图：把素材逐帧交给抠图模型，得到每帧一张灰度“前景把握”图（白 = 前景），编码成 NUT 灰度视频，
//! 剪辑导出时当作透明通道合成。时间都在素材自己的时间里，变速、旋转由导出的滤镜图统一处理。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::onnx::{declared_shape, Net};
use super::store::Norm;
use crate::error::{AppError, AppResult};

/// 抠图帧率上限：模型在 CPU 上一帧要几百毫秒，抠图用的是低帧率，合成时按画面帧率补帧
pub const MAX_FPS: u32 = 12;
const LONG_SIDE: usize = 512;
/// 同一段素材最多抠这么多帧，防止几小时的素材把电脑跑死
pub const MAX_FRAMES: u32 = 6000;

#[derive(Debug, Clone)]
pub struct MatteJob {
    pub path: PathBuf,
    pub src: String,
    pub image: bool,
    /// 取素材的哪一段（毫秒，素材时间）
    pub in_ms: u64,
    pub span_ms: u64,
    /// 模型编号（见 `store::MODELS`）；`model` 是它在本机的文件，运行前才填
    pub model_id: String,
    pub model: PathBuf,
    pub norm: Norm,
    /// 模型要求固定边长（0 = 按画面比例）
    pub fixed_side: usize,
    pub src_size: (u32, u32),
    pub src_fps: Option<f64>,
}

/// 模型输入的 (高, 宽)：模型自己写死的尺寸优先；否则长边 512，按画面比例，边长取 32 的倍数。
pub fn model_shape(declared: &[Option<usize>], fixed_side: usize, src: (u32, u32)) -> (usize, usize) {
    if declared.len() == 4 {
        if let (Some(h), Some(w)) = (declared[2], declared[3]) {
            return (h, w);
        }
    }
    if fixed_side > 0 {
        return (fixed_side, fixed_side);
    }
    let (sw, sh) = (f64::from(src.0.max(1)), f64::from(src.1.max(1)));
    let s = LONG_SIDE as f64 / sw.max(sh);
    let r = |v: f64| ((v * s / 32.0).round() as usize).max(1) * 32;
    (r(sh), r(sw))
}

/// 抠图用的帧率：不超过素材帧率，也不超过 `MAX_FPS`。
pub fn matte_fps(src_fps: Option<f64>) -> u32 {
    (src_fps.unwrap_or(25.0).round() as u32).clamp(1, MAX_FPS)
}

impl MatteJob {
    pub fn fps(&self) -> u32 {
        if self.image {
            1
        } else {
            matte_fps(self.src_fps)
        }
    }

    pub fn frames(&self) -> u32 {
        if self.image {
            1
        } else {
            ((self.span_ms as f64 / 1000.0 * f64::from(self.fps())).ceil() as u32).saturating_add(1).min(MAX_FRAMES)
        }
    }
}

/// RGB（行优先、每像素 3 字节）→ 模型输入（CHW，f32，按模型的归一化）。
pub fn preprocess(rgb: &[u8], w: usize, h: usize, norm: Norm) -> Vec<f32> {
    let n = w * h;
    let mut out = vec![0f32; 3 * n];
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    for (i, px) in rgb.chunks_exact(3).take(n).enumerate() {
        for c in 0..3 {
            let v = f32::from(px[c]) / 255.0;
            out[c * n + i] = match norm {
                Norm::Half => (v - 0.5) / 0.5,
                Norm::ImageNet => (v - MEAN[c]) / STD[c],
            };
        }
    }
    out
}

/// 模型输出（一张 `h×w` 的浮点图）→ 灰度。ImageNet 类（U²-Net）输出范围不固定，按这一帧的最小最大值拉伸；其他的直接夹到 0–1。
pub fn postprocess(out: &[f32], norm: Norm) -> Vec<u8> {
    let (lo, hi) = if norm == Norm::ImageNet { out.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v))) } else { (0.0, 1.0) };
    let span = (hi - lo).max(1e-6);
    out.iter().map(|&v| (((v - lo) / span).clamp(0.0, 1.0) * 255.0).round() as u8).collect()
}

/// 和上一帧的抠图轻微混合，压住逐帧抖动的边缘。
pub fn smooth(prev: &[u8], cur: &mut [u8]) {
    for (c, p) in cur.iter_mut().zip(prev) {
        *c = ((u16::from(*c) * 7 + u16::from(*p) * 3) / 10) as u8;
    }
}

fn ffmpeg_cmd(ff: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(ff);
    c.kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    c
}

/// 生成抠图视频。`progress` 收到 0–1；`cancel` 被置位就停下。
pub async fn render_matte(ffmpeg: &Path, job: &MatteJob, cancel: &Arc<AtomicBool>, progress: impl Fn(f64)) -> AppResult<()> {
    if let Some(dir) = job.path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let declared = declared_shape(&job.model)?;
    let (h, w) = model_shape(&declared, job.fixed_side, job.src_size);
    let shape = [1usize, 3, h, w];
    let model = job.model.clone();
    let net = Arc::new(tokio::task::spawn_blocking(move || Net::load(&model, &shape)).await.map_err(|e| AppError::msg(e.to_string()))??);

    let fps = job.fps();
    let frames = job.frames();
    let mut dec = ffmpeg_cmd(ffmpeg);
    dec.args(["-hide_banner", "-loglevel", "error"]);
    if !job.image {
        dec.args(["-ss", &format!("{:.3}", job.in_ms as f64 / 1000.0), "-t", &format!("{:.3}", job.span_ms as f64 / 1000.0)]);
    }
    dec.args([
        "-i",
        &job.src,
        "-an",
        "-vf",
        &format!("fps={fps},scale={w}:{h}:flags=area,format=rgb24"),
        "-frames:v",
        &frames.to_string(),
        "-f",
        "rawvideo",
        "-",
    ]);
    dec.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut enc = ffmpeg_cmd(ffmpeg);
    enc.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "gray",
        "-s",
        &format!("{w}x{h}"),
        "-framerate",
        &fps.to_string(),
        "-i",
        "-",
    ])
    .args(["-an", "-c:v", "ffv1", "-pix_fmt", "gray", "-f", "nut"])
    .arg(&job.path)
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    let mut dchild = dec.spawn().map_err(|e| AppError::msg(format!("无法运行 ffmpeg：{e}")))?;
    let mut echild = enc.spawn().map_err(|e| AppError::msg(format!("无法运行 ffmpeg：{e}")))?;
    let mut din = dchild.stdout.take().ok_or_else(|| AppError::msg("无法读取画面。"))?;
    let mut eout = echild.stdin.take().ok_or_else(|| AppError::msg("无法写入 ffmpeg。"))?;
    let mut derr = dchild.stderr.take();
    let mut eerr = echild.stderr.take();

    let mut rgb = vec![0u8; w * h * 3];
    let mut prev: Option<Vec<u8>> = None;
    let mut done = 0u32;
    let mut failure: Option<AppError> = None;
    while done < frames {
        if cancel.load(Ordering::Relaxed) {
            failure = Some(AppError::msg("canceled"));
            break;
        }
        if din.read_exact(&mut rgb).await.is_err() {
            break; // 素材比预想的短：已经抠了多少算多少
        }
        let input = preprocess(&rgb, w, h, job.norm);
        let n = Arc::clone(&net);
        let res = tokio::task::spawn_blocking(move || n.run(&shape, input)).await.map_err(|e| AppError::msg(e.to_string()));
        let (_, data) = match res.and_then(|r| r) {
            Ok(v) => v,
            Err(e) => {
                failure = Some(e);
                break;
            }
        };
        if data.len() != w * h {
            failure = Some(AppError::msg(format!("模型输出的大小（{}）和输入（{}×{}）对不上，这个模型不是抠图模型。", data.len(), w, h)));
            break;
        }
        let mut gray = postprocess(&data, job.norm);
        if let Some(p) = &prev {
            smooth(p, &mut gray);
        }
        if eout.write_all(&gray).await.is_err() {
            break;
        }
        prev = Some(gray);
        done += 1;
        progress(f64::from(done) / f64::from(frames));
    }
    drop(eout);
    drop(din);
    let mut dlog = String::new();
    if let Some(e) = derr.as_mut() {
        let _ = e.read_to_string(&mut dlog).await;
    }
    let mut elog = String::new();
    if let Some(e) = eerr.as_mut() {
        let _ = e.read_to_string(&mut elog).await;
    }
    let _ = dchild.kill().await;
    let estatus = echild.wait().await.map_err(|e| AppError::msg(format!("ffmpeg 运行失败：{e}")))?;
    if let Some(e) = failure {
        let _ = std::fs::remove_file(&job.path);
        return Err(e);
    }
    if done == 0 || !estatus.success() {
        let _ = std::fs::remove_file(&job.path);
        let tail = |s: &str| s.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        return Err(AppError::msg(format!("生成抠图失败：{}", if dlog.trim().is_empty() { tail(&elog) } else { tail(&dlog) })));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_follows_the_model_then_the_aspect() {
        // 模型写死尺寸：照它的
        assert_eq!(model_shape(&[Some(1), Some(3), Some(320), Some(320)], 0, (1920, 1080)), (320, 320));
        // 动态尺寸：长边 512、32 的倍数、保持比例
        assert_eq!(model_shape(&[Some(1), Some(3), None, None], 0, (1920, 1080)), (288, 512));
        assert_eq!(model_shape(&[], 0, (1080, 1920)), (512, 288));
        // 登记里要求固定边长
        assert_eq!(model_shape(&[Some(1), Some(3), None, None], 320, (1920, 1080)), (320, 320));
        // 极端比例也不会出 0
        let (h, w) = model_shape(&[], 0, (4000, 10));
        assert!(h >= 32 && w >= 32 && h % 32 == 0 && w % 32 == 0);
    }

    #[test]
    fn frame_counts_and_fps_are_capped() {
        let mut j = MatteJob {
            path: "m.nut".into(),
            src: "a.mp4".into(),
            image: false,
            in_ms: 0,
            span_ms: 10_000,
            model_id: "modnet".into(),
            model: "m.onnx".into(),
            norm: Norm::Half,
            fixed_side: 0,
            src_size: (640, 360),
            src_fps: Some(30.0),
        };
        assert_eq!((j.fps(), j.frames()), (12, 121));
        j.src_fps = Some(8.0);
        assert_eq!(j.fps(), 8);
        j.span_ms = 100 * 3_600_000;
        assert_eq!(j.frames(), MAX_FRAMES);
        j.image = true;
        assert_eq!((j.fps(), j.frames()), (1, 1));
    }

    #[test]
    fn pre_and_post_processing() {
        let rgb = [255u8, 0, 128, 0, 255, 128];
        let v = preprocess(&rgb, 2, 1, Norm::Half);
        assert_eq!(v.len(), 6);
        assert!((v[0] - 1.0).abs() < 1e-6 && (v[1] + 1.0).abs() < 1e-6, "R 通道：{v:?}");
        let im = preprocess(&rgb, 2, 1, Norm::ImageNet);
        assert!((im[0] - (1.0 - 0.485) / 0.229).abs() < 1e-5);
        assert_eq!(postprocess(&[-1.0, 0.0, 0.5, 2.0], Norm::Half), vec![0, 0, 128, 255]);
        // ImageNet 类：按这一帧拉伸
        assert_eq!(postprocess(&[2.0, 4.0, 6.0], Norm::ImageNet), vec![0, 128, 255]);
        let mut cur = vec![200u8, 0];
        smooth(&[0, 100], &mut cur);
        assert_eq!(cur, vec![140, 30]);
    }
}
