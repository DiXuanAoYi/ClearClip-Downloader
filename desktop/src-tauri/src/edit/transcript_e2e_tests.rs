//! 文稿相关的端到端测试：逐字高亮的字幕真的会随时间变色。机器上没有 ffmpeg / 字体 / ass 滤镜时自动跳过。

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::build::Mode;
use super::e2e_tests::{dir, ffmpeg, render, strs};
use super::fonts;
use super::spec::{Clip, Project, TextItem, TextWord};
use crate::postprocess::run_ffmpeg_capture;

/// 某一时刻画面里“红色”和“白色”像素的个数
async fn count_red_white(ff: &Path, file: &Path, t: f64) -> (usize, usize) {
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
        "-pix_fmt",
        "rgb24",
        "-f",
        "rawvideo",
        "-",
    ]);
    let o = run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap().0;
    let (mut red, mut white) = (0, 0);
    for px in o.chunks_exact(3) {
        if px[0] > 200 && px[1] < 70 && px[2] < 70 {
            red += 1;
        }
        if px.iter().all(|&c| c > 215) {
            white += 1;
        }
    }
    (red, white)
}

#[tokio::test]
async fn karaoke_text_changes_colour_word_by_word() {
    let Some(ff) = ffmpeg() else { return };
    let caps = crate::vidcaps::detect(&ff).await;
    if caps.encoders.h264.is_none() || !caps.has_filter("ass") || !caps.has_filter("drawtext") || fonts::default_font().is_none() {
        return;
    }
    let d = dir("karaoke");
    let src = d.join("black.mp4");
    let o = Command::new(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=black:s=640x360:r=25:d=4", "-c:v", "mpeg4", "-pix_fmt", "yuv420p"])
        .arg(&src)
        .output()
        .unwrap();
    assert!(o.status.success());
    let text = TextItem {
        text: "AAAA BBBB".into(),
        start_ms: 0,
        end_ms: 4000,
        size: 14.0,
        y: 0.5,
        highlight: "#FF0000".into(),
        words: vec![TextWord { t: "AAAA".into(), a: 500, b: 1500 }, TextWord { t: " BBBB".into(), a: 1500, b: 2500 }],
        ..Default::default()
    };
    let p = Project {
        clips: vec![Clip { path: src.to_string_lossy().into_owned(), out_ms: 4000, ..Default::default() }],
        texts: vec![text.clone()],
        ..Default::default()
    };
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    assert!(plan.args.iter().any(|a| a.contains("ass=filename=")), "{:?}", plan.args);

    let (r0, w0) = count_red_white(&ff, &out, 0.2).await;
    let (r1, w1) = count_red_white(&ff, &out, 1.0).await;
    let (r2, w2) = count_red_white(&ff, &out, 3.5).await;
    assert!(w0 > 300 && r0 == 0, "还没开始说：全是白字（白 {w0} 红 {r0}）");
    assert!(r1 > 100 && w1 > 100, "第一个词开始说、第二个还没：一半红一半白（白 {w1} 红 {r1}）");
    assert!(r2 > 300 && w2 < w0 / 4, "全说完：几乎全红（白 {w2} 红 {r2}）");
    assert!(r1 < r2, "红色越来越多");

    // 同样的文字去掉词（被改过）：按普通文字画，不走 ass
    let plain = TextItem { words: vec![], highlight: String::new(), ..text };
    let p = Project {
        clips: vec![Clip { path: src.to_string_lossy().into_owned(), out_ms: 4000, ..Default::default() }],
        texts: vec![plain],
        ..Default::default()
    };
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    assert!(plan.args.iter().all(|a| !a.contains("ass=filename=")));
    let (r, w) = count_red_white(&ff, &out, 3.5).await;
    assert!(r == 0 && w > 300);
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn fade_makes_the_text_dim_at_the_edges() {
    let Some(ff) = ffmpeg() else { return };
    let caps = crate::vidcaps::detect(&ff).await;
    if caps.encoders.h264.is_none() || !caps.has_filter("drawtext") || fonts::default_font().is_none() {
        return;
    }
    let d = dir("fade");
    let src = d.join("black.mp4");
    let o = Command::new(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=black:s=640x360:r=25:d=4", "-c:v", "mpeg4", "-pix_fmt", "yuv420p"])
        .arg(&src)
        .output()
        .unwrap();
    assert!(o.status.success());
    let text = TextItem { text: "AAAA".into(), start_ms: 0, end_ms: 4000, size: 14.0, y: 0.5, outline: false, fade_ms: 1000, ..Default::default() };
    let p =
        Project { clips: vec![Clip { path: src.to_string_lossy().into_owned(), out_ms: 4000, ..Default::default() }], texts: vec![text], ..Default::default() };
    let (out, _) = render(&ff, &caps, &d, p, Mode::Export).await;
    let (_, edge) = count_red_white(&ff, &out, 0.1).await;
    let (_, mid) = count_red_white(&ff, &out, 2.0).await;
    assert!(mid > 200 && edge * 4 < mid, "淡入刚开始时字几乎看不见（{edge} / {mid}）");
    let _ = std::fs::remove_dir_all(&d);
}
