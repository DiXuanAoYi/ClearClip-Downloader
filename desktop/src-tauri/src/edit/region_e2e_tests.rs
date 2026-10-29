//! 区域效果的端到端测试：把一个沿已知路径运动的物体编码成真实的视频，加上区域效果导出，
//! 再逐帧核对效果是不是出现在物体所在的位置（差一帧就是 5 个像素，一眼就能看出来）。
//! 机器上没有 ffmpeg（或缺少需要的滤镜 / 编码器）时自动跳过；`PATH` 指向哪个版本的 ffmpeg 就测哪个。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use super::build::Mode;
use super::e2e_tests::{dir, ffmpeg, render, strs};
use super::region::{render_mask, MaskJob};
use super::spec::{Clip, Project, Region};
use crate::postprocess::run_ffmpeg_capture;
use crate::track::synth::{Scene, Shot};
use crate::track::TrackPt;
use crate::vidcaps::{self, Caps};
use crate::vidnorm::facts;

const W: usize = 640;
const H: usize = 360;
const FPS: f64 = 25.0;
const FRAMES: usize = 100;
const SIZE: usize = 70;
/// 区域框的边长（像素）
const BOX: f64 = 90.0;

/// 物体中心在第 i 帧的位置（像素）。
fn pos(i: usize) -> (f64, f64) {
    (100.0 + i as f64 * 5.0, 180.0 + 60.0 * (i as f64 * 0.07).sin())
}

fn make_video(ff: &Path, d: &Path) -> Option<PathBuf> {
    let has_x264 =
        Command::new(ff).args(["-hide_banner", "-encoders"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains("libx264")).unwrap_or(false);
    let codec: &[&str] = if has_x264 { &["-c:v", "libx264", "-crf", "10", "-preset", "ultrafast"] } else { &["-c:v", "mpeg4", "-q:v", "1"] };
    let out = d.join("moving.mp4");
    let mut child = Command::new(ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "gray", "-s", &format!("{W}x{H}"), "-r", "25", "-i", "-"])
        .args(codec)
        .args(["-pix_fmt", "yuv420p"])
        .arg(&out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    {
        let sc = Scene::new(W, H, SIZE);
        let mut stdin = child.stdin.take()?;
        for i in 0..FRAMES {
            let f = sc.frame(&Shot { noise: 2.0, seed: i as u64, ..Shot::at(pos(i).0, pos(i).1) });
            stdin.write_all(&f.px).ok()?;
        }
    }
    (child.wait().ok()?.success() && std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false)).then_some(out)
}

async fn setup(name: &str) -> Option<(PathBuf, PathBuf, Caps, PathBuf)> {
    let ff = ffmpeg()?;
    let caps = vidcaps::detect(&ff).await;
    if caps.encoders.h264.is_none() || !["alphamerge", "overlay", "gblur", "lutyuv", "crop"].iter().all(|f| caps.has_filter(f)) {
        return None;
    }
    let d = dir(name);
    let src = make_video(&ff, &d)?;
    Some((ff, d, caps, src))
}

macro_rules! need {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return,
        }
    };
}

/// 每 5 帧一个点的轨迹，框是以物体为中心的 90×90 像素。
fn pins() -> Vec<TrackPt> {
    (0..FRAMES)
        .step_by(5)
        .chain([FRAMES - 1])
        .map(|i| {
            let (cx, cy) = pos(i);
            TrackPt {
                t_ms: (i as f64 * 40.0) as u64,
                x: (cx - BOX / 2.0) / W as f64,
                y: (cy - BOX / 2.0) / H as f64,
                w: BOX / W as f64,
                h: BOX / H as f64,
                ..Default::default()
            }
        })
        .collect()
}

fn region(effect: &str) -> Region {
    Region { effect: effect.into(), track: pins(), feather: 0.0, ..Default::default() }
}

fn project(src: &Path, regions: Vec<Region>) -> Project {
    let c = Clip { id: 1, path: src.to_string_lossy().into_owned(), in_ms: 0, out_ms: 4000, regions, ..Default::default() };
    Project { clips: vec![c], ..Default::default() }
}

/// 输出里第 n 帧（按帧序号）的灰度画面，缩放到 `w × h`。
async fn gray_frame(ff: &Path, file: &Path, n: usize, fps: f64, w: usize, h: usize) -> Vec<u8> {
    // 输入端的 -ss 会丢掉时间早于它的帧：略早于这一帧的开始
    let t = (n as f64 / fps - 0.01).max(0.0);
    let a = strs(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-ss",
        &format!("{t:.3}"),
        "-i",
        &file.to_string_lossy(),
        "-frames:v",
        "1",
        "-vf",
        &format!("scale={w}:{h}:flags=area,format=gray"),
        "-f",
        "rawvideo",
        "-pix_fmt",
        "gray",
        "-",
    ]);
    let o = run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap().0;
    assert_eq!(o.len(), w * h, "第 {n} 帧取不到");
    o
}

/// 两幅画面不一样的地方（差别超过 `thr`）的外框：行和列上至少有 `max` 的四分之一个不一样的像素才算，避免被零星的噪点带偏。返回 [x0, x1) × [y0, y1)。
fn changed_box(a: &[u8], b: &[u8], w: usize, h: usize, thr: i32) -> Option<(f64, f64, f64, f64)> {
    let mut rows = vec![0usize; h];
    let mut cols = vec![0usize; w];
    for y in 0..h {
        for x in 0..w {
            if (i32::from(a[y * w + x]) - i32::from(b[y * w + x])).abs() > thr {
                rows[y] += 1;
                cols[x] += 1;
            }
        }
    }
    let span = |v: &[usize]| {
        let m = *v.iter().max()?;
        if m < 8 {
            return None;
        }
        let ok = |c: &usize| *c * 4 >= m;
        Some((v.iter().position(ok)? as f64, (v.iter().rposition(ok)? + 1) as f64))
    };
    let (x0, x1) = span(&cols)?;
    let (y0, y1) = span(&rows)?;
    Some((x0, x1, y0, y1))
}

/// 平均绝对差，只算 `inside`（含）为真的像素。
fn mad(a: &[u8], b: &[u8], w: usize, keep: impl Fn(usize, usize) -> bool) -> f64 {
    let (mut s, mut n) = (0u64, 0u64);
    for (i, (p, q)) in a.iter().zip(b).enumerate() {
        if keep(i % w, i / w) {
            s += u64::from(p.abs_diff(*q));
            n += 1;
        }
    }
    s as f64 / n.max(1) as f64
}

/// 横向相邻像素差的平均（细节多少的度量），只算框里面。
fn detail(a: &[u8], w: usize, x0: usize, x1: usize, y0: usize, y1: usize) -> f64 {
    let (mut s, mut n) = (0u64, 0u64);
    for y in y0..y1 {
        for x in x0..x1 - 1 {
            s += u64::from(a[y * w + x].abs_diff(a[y * w + x + 1]));
            n += 1;
        }
    }
    s as f64 / n.max(1) as f64
}

fn near(v: f64, want: f64, tol: f64) -> bool {
    (v - want).abs() <= tol
}

#[tokio::test]
async fn a_tone_region_sticks_to_the_moving_object_to_the_pixel() {
    let (ff, d, caps, src) = need!(setup("tone").await);
    let (base, _) = render(&ff, &caps, &d, project(&src, vec![]), Mode::Export).await;
    let mut r = region("tone");
    r.brightness = 0.5;
    let (out, plan) = render(&ff, &caps, &d, project(&src, vec![r]), Mode::Export).await;
    assert_eq!(plan.masks.len(), 1);
    // 时长没有被遮罩拖长或缩短
    let f = facts::read(&ff, &out).await.unwrap();
    assert!(f.duration_ms.is_some_and(|m| m.abs_diff(4000) <= 120), "{:?}", f.duration_ms);
    for n in [3usize, 21, 47, 66, 90] {
        let (a, b) = (gray_frame(&ff, &base, n, FPS, W, H).await, gray_frame(&ff, &out, n, FPS, W, H).await);
        let (x0, x1, y0, y1) = changed_box(&a, &b, W, H, 20).unwrap_or_else(|| panic!("第 {n} 帧没有变化"));
        let (cx, cy) = pos(n);
        let ok = near(x0, cx - BOX / 2.0, 2.0) && near(x1, cx + BOX / 2.0, 2.0) && near(y0, cy - BOX / 2.0, 2.0) && near(y1, cy + BOX / 2.0, 2.0);
        assert!(
            ok,
            "第 {n} 帧：变化的范围 x {x0}–{x1} y {y0}–{y1}，物体框 x {:.1}–{:.1} y {:.1}–{:.1}",
            cx - BOX / 2.0,
            cx + BOX / 2.0,
            cy - BOX / 2.0,
            cy + BOX / 2.0
        );
        // 框外面一点变化都没有
        let m = mad(&a, &b, W, |x, y| {
            (x as f64) < cx - BOX / 2.0 - 4.0 || (x as f64) > cx + BOX / 2.0 + 4.0 || (y as f64) < cy - BOX / 2.0 - 4.0 || (y as f64) > cy + BOX / 2.0 + 4.0
        });
        assert!(m < 1.5, "第 {n} 帧框外的平均差别 {m:.2}");
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn mosaic_and_blur_hide_the_detail_inside_the_box_only() {
    let (ff, d, caps, src) = need!(setup("hide").await);
    let (base, _) = render(&ff, &caps, &d, project(&src, vec![]), Mode::Export).await;
    for (effect, strength, at_most) in [("mosaic", 0.5, 0.6), ("blur", 0.8, 0.3)] {
        let r = Region { strength, ..region(effect) };
        let (out, _) = render(&ff, &caps, &d, project(&src, vec![r]), Mode::Export).await;
        for n in [10usize, 40, 75] {
            let (a, b) = (gray_frame(&ff, &base, n, FPS, W, H).await, gray_frame(&ff, &out, n, FPS, W, H).await);
            let (cx, cy) = pos(n);
            let (x0, x1, y0, y1) =
                ((cx - BOX / 2.0 + 10.0) as usize, (cx + BOX / 2.0 - 10.0) as usize, (cy - BOX / 2.0 + 10.0) as usize, (cy + BOX / 2.0 - 10.0) as usize);
            let (before, after) = (detail(&a, W, x0, x1, y0, y1), detail(&b, W, x0, x1, y0, y1));
            assert!(after < before * at_most, "{effect} 第 {n} 帧：框里的细节 {before:.2} → {after:.2}");
            let m = mad(&a, &b, W, |x, y| {
                (x as f64) < cx - BOX / 2.0 - 4.0 || (x as f64) > cx + BOX / 2.0 + 4.0 || (y as f64) < cy - BOX / 2.0 - 4.0 || (y as f64) > cy + BOX / 2.0 + 4.0
            });
            assert!(m < 1.5, "{effect} 第 {n} 帧框外的平均差别 {m:.2}");
        }
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn inverted_ellipse_regions_act_outside_and_the_active_range_limits_the_time() {
    let (ff, d, caps, src) = need!(setup("invert").await);
    let (base, _) = render(&ff, &caps, &d, project(&src, vec![]), Mode::Export).await;
    // 作用于区域以外：椭圆里面保持原样，远处被调亮
    let r = Region { brightness: 0.5, shape: "ellipse".into(), invert: true, feather: 0.3, ..region("tone") };
    let (out, _) = render(&ff, &caps, &d, project(&src, vec![r]), Mode::Export).await;
    let n = 50;
    let (a, b) = (gray_frame(&ff, &base, n, FPS, W, H).await, gray_frame(&ff, &out, n, FPS, W, H).await);
    let (cx, cy) = pos(n);
    let inside = mad(&a, &b, W, |x, y| ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() < 16.0);
    let far = mad(&a, &b, W, |x, y| ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt() > 120.0);
    assert!(inside < 8.0, "椭圆中心附近应该保持原样：{inside:.1}");
    assert!(far > 35.0, "远处应该被调亮：{far:.1}");
    // 只在 1.0–2.0 秒生效
    let r = Region { brightness: 0.5, start_ms: Some(1000), end_ms: Some(2000), ..region("tone") };
    let (out, _) = render(&ff, &caps, &d, project(&src, vec![r]), Mode::Export).await;
    for (n, on) in [(10usize, false), (37, true), (60, false), (90, false)] {
        let (a, b) = (gray_frame(&ff, &base, n, FPS, W, H).await, gray_frame(&ff, &out, n, FPS, W, H).await);
        let whole = mad(&a, &b, W, |_, _| true);
        if on {
            assert!(changed_box(&a, &b, W, H, 20).is_some(), "第 {n} 帧应该有效果");
        } else {
            assert!(whole < 2.5, "第 {n} 帧不该有效果：{whole:.2}");
        }
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn trimmed_sped_up_and_rotated_clips_keep_the_effect_on_the_object() {
    let (ff, d, caps, src) = need!(setup("speed").await);
    // 取素材 1.0–3.0 秒、两倍速、顺时针旋转 90 度：成片 1 秒，360×640
    let make = |regions: Vec<Region>| {
        let mut p = project(&src, regions);
        let c = &mut p.clips[0];
        (c.in_ms, c.out_ms, c.speed, c.rotate) = (1000, 3000, 2.0, 90);
        p
    };
    let (base, _) = render(&ff, &caps, &d, make(vec![]), Mode::Export).await;
    let mut r = region("tone");
    r.brightness = 0.5;
    let (out, _) = render(&ff, &caps, &d, make(vec![r]), Mode::Export).await;
    let f = facts::read(&ff, &out).await.unwrap();
    assert!(f.duration_ms.is_some_and(|m| m.abs_diff(1000) <= 120), "{:?}", f.duration_ms);
    for m in [3usize, 11, 20] {
        // 成片第 m 帧 = 素材 1000 + 80m 毫秒 = 素材第 25 + 2m 帧
        let (cx, cy) = pos(25 + 2 * m);
        let (a, b) = (gray_frame(&ff, &base, m, FPS, 360, 640).await, gray_frame(&ff, &out, m, FPS, 360, 640).await);
        let (x0, x1, y0, y1) = changed_box(&a, &b, 360, 640, 20).unwrap_or_else(|| panic!("成片第 {m} 帧没有变化"));
        // 顺时针旋转 90 度：原来的 (x, y) 到了 (H - y, x)
        let want = (H as f64 - cy - BOX / 2.0, H as f64 - cy + BOX / 2.0, cx - BOX / 2.0, cx + BOX / 2.0);
        assert!(
            near(x0, want.0, 2.5) && near(x1, want.1, 2.5) && near(y0, want.2, 2.5) && near(y1, want.3, 2.5),
            "成片第 {m} 帧：变化的范围 x {x0}–{x1} y {y0}–{y1}，应该在 {want:?}"
        );
    }
    let _ = std::fs::remove_dir_all(d);
}

/// 参照：从素材的第 n 帧里按窗口裁出来，放大到 `w × h`。
async fn expected_crop(ff: &Path, src: &Path, n: usize, win: (usize, usize), at: (f64, f64), scale: (usize, usize)) -> Vec<u8> {
    let t = (n as f64 / FPS - 0.01).max(0.0);
    let vf = format!("crop={}:{}:{:.0}:{:.0},scale={}:{}:flags=bicubic,format=gray", win.0, win.1, at.0, at.1, scale.0, scale.1);
    let a = strs(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-ss",
        &format!("{t:.3}"),
        "-i",
        &src.to_string_lossy(),
        "-frames:v",
        "1",
        "-vf",
        &vf,
        "-f",
        "rawvideo",
        "-pix_fmt",
        "gray",
        "-",
    ]);
    run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap().0
}

#[tokio::test]
async fn focus_keeps_the_object_in_the_middle_of_the_picture() {
    let (ff, d, caps, src) = need!(setup("focus").await);
    // 放大 2 倍、不平滑：窗口 320×180 的中心就是物体
    let r = Region { zoom: 2.0, smooth: 0.0, ..region("focus") };
    let (out, plan) = render(&ff, &caps, &d, project(&src, vec![r]), Mode::Export).await;
    assert!(plan.masks.is_empty());
    for n in [8usize, 30, 52, 77] {
        let (cx, cy) = pos(n);
        let at = ((cx - 160.0).clamp(0.0, 320.0), (cy - 90.0).clamp(0.0, 180.0));
        let got = gray_frame(&ff, &out, n, FPS, W, H).await;
        let want = expected_crop(&ff, &src, n, (320, 180), at, (W, H)).await;
        let right = mad(&got, &want, W, |_, _| true);
        // 窗口晚了一帧（物体移动 5 个像素，放大后 10 个像素）会差得多
        let (cx1, cy1) = pos(n - 1);
        let late = expected_crop(&ff, &src, n, (320, 180), ((cx1 - 160.0).clamp(0.0, 320.0), (cy1 - 90.0).clamp(0.0, 180.0)), (W, H)).await;
        let wrong = mad(&got, &late, W, |_, _| true);
        assert!(right < 12.0 && right * 1.6 < wrong, "第 {n} 帧：和正确的取景相差 {right:.2}，和晚一帧的取景相差 {wrong:.2}");
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn focus_can_cut_a_portrait_window_out_of_a_landscape_clip() {
    let (ff, d, caps, src) = need!(setup("reframe").await);
    // 成片 360×640（竖屏）：窗口取 9:16（202×360），跟着物体左右移动
    let r = Region { zoom: 1.0, reframe: true, smooth: 0.0, ..region("focus") };
    let mut p = project(&src, vec![r]);
    (p.out.width, p.out.height) = (360, 640);
    let (out, _) = render(&ff, &caps, &d, p, Mode::Export).await;
    let f = facts::read(&ff, &out).await.unwrap();
    assert_eq!(f.video.as_ref().map(|v| (v.width, v.height)), Some((360, 640)));
    for n in [10usize, 45, 80] {
        let (cx, _) = pos(n);
        let x = (cx - 101.0).clamp(0.0, (W - 202) as f64);
        let got = gray_frame(&ff, &out, n, FPS, 360, 640).await;
        // 202×360 的窗口放进 360×640：按高度放大到 358×640，左右各留 1 像素黑边——只比较中间
        let want = expected_crop(&ff, &src, n, (202, 360), (x, 0.0), (358, 640)).await;
        let (g, w): (Vec<u8>, Vec<u8>) =
            (0..640).flat_map(|y| (8..350).map(move |xx| (y, xx))).map(|(y, xx)| (got[y * 360 + xx + 1], want[y * 358 + xx])).unzip();
        let right = mad(&g, &w, 342, |_, _| true);
        let (cx2, _) = pos(n.saturating_sub(2));
        let x2 = (cx2 - 101.0).clamp(0.0, (W - 202) as f64);
        let off = expected_crop(&ff, &src, n, (202, 360), (x2, 0.0), (358, 640)).await;
        let o: Vec<u8> = (0..640).flat_map(|y| (8..350).map(move |xx| (y, xx))).map(|(y, xx)| off[y * 358 + xx]).collect();
        let wrong = mad(&g, &o, 342, |_, _| true);
        assert!(right < 12.0 && right * 1.6 < wrong, "第 {n} 帧：和正确的取景相差 {right:.2}，和晚两帧的取景相差 {wrong:.2}");
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn previews_carry_the_region_effects_too() {
    let (ff, d, caps, src) = need!(setup("preview").await);
    let mut r = region("tone");
    r.brightness = 0.5;
    let (base, _) = render(&ff, &caps, &d, project(&src, vec![]), Mode::Preview).await;
    let (out, plan) = render(&ff, &caps, &d, project(&src, vec![r]), Mode::Preview).await;
    assert_eq!((plan.width, plan.height), (640, 360));
    let n = 40;
    let (a, b) = (gray_frame(&ff, &base, n, 24.0, 640, 360).await, gray_frame(&ff, &out, n, 24.0, 640, 360).await);
    assert!(changed_box(&a, &b, 640, 360, 20).is_some());
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn rendering_masks_can_be_canceled_and_failures_are_explained() {
    let ff = need!(ffmpeg());
    let d = dir("mask");
    let job = MaskJob::new(d.join("m.nut"), region("tone"), 0, 1.0, 4000, (640, 360), Some(25.0), false);
    // 正常生成
    render_mask(&ff, &job, || Ok(())).await.expect("生成遮罩");
    assert!(std::fs::metadata(&job.path).unwrap().len() > 0);
    // 中途取消
    let n = std::cell::Cell::new(0);
    let e = render_mask(&ff, &job, || {
        n.set(n.get() + 1);
        if n.get() > 10 {
            Err(crate::error::AppError::msg("canceled"))
        } else {
            Ok(())
        }
    })
    .await
    .unwrap_err();
    assert_eq!(e.message, "canceled");
    // 目录没法写：ffmpeg 报错，我们转述
    let bad = MaskJob { path: d.join("no-such-dir-file").join("x").join("m.nut"), ..job.clone() };
    std::fs::write(d.join("no-such-dir-file"), "x").unwrap();
    assert!(render_mask(&ff, &bad, || Ok(())).await.is_err());
    let _ = std::fs::remove_dir_all(d);
}
