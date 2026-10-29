//! 用真实的 ffmpeg 跑一遍：把合成画面（有纹理的背景 + 一个运动的圆形物体）编码成真实的视频文件，
//! 再用解码、感知、追踪的完整流程处理，核对位置和时间有没有对上（差一帧就会出现明显的偏差）。
//! 机器上没有 ffmpeg 时自动跳过；`PATH` 指向哪个版本的 ffmpeg 就测哪个。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::cmds::{detect_at, follow_clip, FollowReq};
use super::decode::Decoder;
use super::gray::Gray;
use super::interpolate;
use super::synth::{Scene, Shot};
use super::NRect;

const W: usize = 640;
const H: usize = 360;
const FPS: u32 = 25;
const FRAMES: usize = 100;
const SIZE: usize = 70;

fn ffmpeg() -> Option<PathBuf> {
    Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
}

async fn decoder(ff: &Path) -> Decoder {
    let caps = crate::vidcaps::detect(ff).await;
    Decoder::new(ff, &caps)
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "clearclip-track-e2e-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 物体中心在第 i 帧的位置（像素）。
fn pos(i: usize) -> (f64, f64) {
    (100.0 + i as f64 * 5.0, 180.0 + 60.0 * (i as f64 * 0.07).sin())
}

fn rect_at(i: usize) -> NRect {
    let (cx, cy) = pos(i);
    NRect { x: (cx - SIZE as f64 / 2.0) / W as f64, y: (cy - SIZE as f64 / 2.0) / H as f64, w: SIZE as f64 / W as f64, h: SIZE as f64 / H as f64 }
}

/// 编码一段视频，用 ffmpeg 里有的编码器（完整版 libx264，精简版 mpeg4）。
fn make_video(ff: &Path, out: &Path, frames: &[Gray], fps: u32) -> bool {
    let has_x264 =
        Command::new(ff).args(["-hide_banner", "-encoders"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("libx264")).unwrap_or(false);
    let codec: &[&str] = if has_x264 { &["-c:v", "libx264", "-crf", "12", "-preset", "ultrafast"] } else { &["-c:v", "mpeg4", "-q:v", "2"] };
    let mut child = match Command::new(ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "gray", "-s", &format!("{W}x{H}"), "-r", &fps.to_string(), "-i", "-"])
        .args(codec)
        .args(["-pix_fmt", "yuv420p"])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    {
        let mut stdin = child.stdin.take().unwrap();
        for f in frames {
            if stdin.write_all(&f.px).is_err() {
                return false;
            }
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false) && std::fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false)
}

fn video(ff: &Path, d: &Path) -> Option<String> {
    let sc = Scene::new(W, H, SIZE);
    let frames: Vec<Gray> = (0..FRAMES).map(|i| sc.frame(&Shot { noise: 2.0, seed: i as u64, ..Shot::at(pos(i).0, pos(i).1) })).collect();
    let out = d.join("moving.mp4");
    make_video(ff, &out, &frames, FPS).then(|| out.to_string_lossy().into_owned())
}

#[tokio::test]
async fn a_region_is_followed_through_a_real_video_in_both_directions() {
    let Some(ff) = ffmpeg() else { return };
    let dec = decoder(&ff).await;
    let d = dir("follow");
    let Some(file) = video(&ff, &d) else { return };
    let ref_i = 50;
    let req = FollowReq { id: 7, path: file, from_ms: 0, to_ms: 3960, ref_ms: ref_i as u64 * 40, rect: rect_at(ref_i) };
    let progress = Arc::new(std::sync::Mutex::new(vec![]));
    let p2 = progress.clone();
    let res = follow_clip(&dec, &req, move |p| p2.lock().unwrap().push(p), Arc::new(AtomicBool::new(false))).await.expect("追踪");
    assert!(res.lost_ms == 0 && res.note.is_none(), "不该有丢失：{} {:?}", res.lost_ms, res.note);
    assert!(!progress.lock().unwrap().is_empty());
    // 参照点就是画的那个框
    let pin = res.points.iter().find(|p| p.pin).expect("参照点");
    assert_eq!(pin.t_ms, 2000);
    let r = rect_at(ref_i);
    assert!((pin.x - r.x).abs() < 1e-9 && (pin.w - r.w).abs() < 1e-9);
    // 每一帧的位置都对得上；差一帧（5 像素）就会超出容差
    let mut worst = 0f64;
    for i in (2..FRAMES - 2).step_by(3) {
        let t = i as f64 * 40.0;
        let got = interpolate(&res.points, t).unwrap();
        let (tx, ty) = pos(i);
        let (gx, gy) = ((got.x + got.w / 2.0) * W as f64, (got.y + got.h / 2.0) * H as f64);
        worst = worst.max(((gx - tx).powi(2) + (gy - ty).powi(2)).sqrt());
    }
    assert!(worst < 3.0, "最大偏差 {worst:.2} 像素（640 宽的画面）");
    // 精简过：不是每个采样点都留着
    assert!(res.points.len() < 80, "{}", res.points.len());
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn moving_objects_are_found_in_a_real_video() {
    let Some(ff) = ffmpeg() else { return };
    let dec = decoder(&ff).await;
    let d = dir("detect");
    let Some(file) = video(&ff, &d) else { return };
    for i in [30usize, 60] {
        let res = detect_at(&dec, &file, i as u64 * 40).await.expect("感知");
        assert!(!res.candidates.is_empty(), "第 {i} 帧：{:?}", res.note);
        let t = rect_at(i);
        let best = res.candidates.iter().map(|c| c.rect.iou(&t)).fold(0.0, f64::max);
        assert!(best > 0.4, "第 {i} 帧：{:?} 和真实位置 {t:?} 重合度 {best:.2}", res.candidates);
    }
    // 开头第一帧附近：前面没有帧，只用后面的
    let first = detect_at(&dec, &file, 0).await.expect("开头");
    assert!(first.candidates.iter().any(|c| c.rect.iou(&rect_at(0)) > 0.3), "{:?} {:?}", first.candidates, first.note);
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn canceling_stops_the_tracking_and_bad_requests_are_explained() {
    let Some(ff) = ffmpeg() else { return };
    let dec = decoder(&ff).await;
    let d = dir("cancel");
    let Some(file) = video(&ff, &d) else { return };
    let req = FollowReq { id: 8, path: file.clone(), from_ms: 0, to_ms: 3960, ref_ms: 2000, rect: rect_at(50) };
    let e = follow_clip(&dec, &req, |_| {}, Arc::new(AtomicBool::new(true))).await.expect_err("取消");
    assert_eq!(e.message, "canceled");
    // 平坦的区域（背景角落之外的纯色块）：说明原因
    let flat = FollowReq { rect: NRect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, ..req.clone() };
    assert!(follow_clip(&dec, &flat, |_| {}, Arc::new(AtomicBool::new(false))).await.err().unwrap().message.contains("框出"));
    let missing = FollowReq { path: d.join("nope.mp4").to_string_lossy().into_owned(), ..req };
    assert!(follow_clip(&dec, &missing, |_| {}, Arc::new(AtomicBool::new(false))).await.err().unwrap().message.contains("找不到"));
    let _ = std::fs::remove_dir_all(d);
}

/// 每一帧的亮度 = 帧序号：解出来的帧带的时间必须和帧序号对得上（25 帧/秒，第 N 帧在 N × 40 毫秒）。
#[tokio::test]
async fn decoded_frames_carry_their_true_times() {
    let Some(ff) = ffmpeg() else { return };
    let dec = decoder(&ff).await;
    let d = dir("marker");
    let file = d.join("marker.mkv");
    let ok = Command::new(&ff)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x36:r=25:d=6,format=gray,geq=lum='N'",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "gray",
        ])
        .arg(&file)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return;
    }
    let path = file.to_string_lossy().into_owned();
    let n_of = |t: &super::tracker::Timed| t.gray.px[100] as f64;
    // 全部帧：时间 = 序号 × 40
    let all = dec.window(&path, 1961.0, 600.0, None, (64, 36)).await.unwrap();
    assert!(all.len() >= 14, "{}", all.len());
    assert_eq!(n_of(&all[0]), 50.0, "从 1961 毫秒起的第一帧是 2000 毫秒的那一帧");
    for f in &all {
        assert!((f.t_ms - n_of(f) * 40.0).abs() < 1.0, "第 {} 帧的时间 {}", n_of(f), f.t_ms);
    }
    // 按最小间隔挑帧：相邻至少相隔 60 毫秒，时间照样对得上
    let some = dec.window(&path, 1961.0, 1200.0, Some(60.0), (64, 36)).await.unwrap();
    assert!(some.len() >= 10 && some.len() <= 16, "{}", some.len());
    assert!(some.windows(2).all(|w| w[1].t_ms - w[0].t_ms >= 59.0), "{:?}", some.iter().map(|f| f.t_ms).collect::<Vec<_>>());
    for f in &some {
        assert!((f.t_ms - n_of(f) * 40.0).abs() < 1.0);
    }
    // 开头
    let head = dec.window(&path, 0.0, 200.0, None, (64, 36)).await.unwrap();
    assert_eq!(n_of(&head[0]), 0.0);
    assert!(head[0].t_ms.abs() < 1.0);
    let _ = std::fs::remove_dir_all(d);
}
