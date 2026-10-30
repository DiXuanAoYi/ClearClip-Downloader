//! 分析命令和片段声音处理的端到端测试：造真实的素材，跑 ffmpeg，检查结果。机器上没有 ffmpeg 时自动跳过。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::analyze::{reframe, scenes, silences, waveform, ReframeReq};
use super::build::Mode;
use super::e2e_tests::render;
use super::spec::{Clip, Project};
use crate::postprocess::base_args;
use crate::track::decode::Decoder;

fn ffmpeg() -> Option<PathBuf> {
    Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "clearclip-an-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(ff: &Path, args: &[&str]) {
    let mut full = base_args();
    full.extend(args.iter().map(|s| s.to_string()));
    let o = Command::new(ff).args(&full).output().unwrap();
    assert!(o.status.success(), "ffmpeg 失败：{}\n{full:?}", String::from_utf8_lossy(&o.stderr));
}

fn rms_db(ff: &Path, f: &Path, from: f64, secs: f64) -> f64 {
    let o = Command::new(ff)
        .args(["-hide_banner", "-nostats", "-ss", &from.to_string(), "-t", &secs.to_string(), "-i"])
        .arg(f)
        .args(["-af", "astats=measure_overall=RMS_level:measure_perchannel=none", "-f", "null", "-"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    let line = err.lines().rev().find(|l| l.contains("RMS level dB")).unwrap_or_else(|| panic!("{err}"));
    let v = line.rsplit(':').next().unwrap().trim();
    if v == "-inf" {
        -200.0
    } else {
        v.parse().unwrap()
    }
}

#[tokio::test]
async fn scene_cuts_land_on_the_colour_changes() {
    let Some(ff) = ffmpeg() else { return };
    let d = dir("scenes");
    let f = d.join("s.mp4");
    // 黑 2 秒 → 白 3 秒 → 灰 2 秒（场景检测看亮度，所以不用亮度相近的颜色）：切点在 2000 和 5000
    run(
        &ff,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=160x90:r=10:d=2",
            "-f",
            "lavfi",
            "-i",
            "color=c=white:s=160x90:r=10:d=3",
            "-f",
            "lavfi",
            "-i",
            "color=c=gray:s=160x90:r=10:d=2",
            "-filter_complex",
            "[0][1][2]concat=n=3:v=1:a=0",
            "-c:v",
            "mpeg4",
            "-g",
            "5",
            &f.to_string_lossy(),
        ],
    );
    let p = f.to_string_lossy().into_owned();
    let cuts = scenes(&ff, &p, 0, 7000, 0.4).await.unwrap();
    assert_eq!(cuts.len(), 2, "{cuts:?}");
    assert!(cuts[0].abs_diff(2000) <= 150 && cuts[1].abs_diff(5000) <= 150, "{cuts:?}");
    // 只看后半段：时间要换算成素材时间，起点之前的切点不出现
    let late = scenes(&ff, &p, 3000, 7000, 0.4).await.unwrap();
    assert_eq!(late.len(), 1, "{late:?}");
    assert!(late[0].abs_diff(5000) <= 150, "{late:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn silences_are_found_in_source_time() {
    let Some(ff) = ffmpeg() else { return };
    let d = dir("sil");
    let f = d.join("v.wav");
    // 响 1 秒 → 静 1.5 秒 → 响 1 秒 → 静 1 秒（到结尾）
    run(
        &ff,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=1:r=44100",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=mono:d=1.5",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=1:r=44100",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=mono:d=1",
            "-filter_complex",
            "[0][1][2][3]concat=n=4:v=0:a=1",
            "-c:a",
            "pcm_s16le",
            &f.to_string_lossy(),
        ],
    );
    let p = f.to_string_lossy().into_owned();
    let all = silences(&ff, &p, 0, 4500, -35.0, 500).await.unwrap();
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all[0].start_ms.abs_diff(1000) <= 60 && all[0].end_ms.abs_diff(2500) <= 60, "{all:?}");
    assert!(all[1].start_ms.abs_diff(3500) <= 60 && all[1].end_ms >= 4400, "{all:?}");
    // 太短的停顿不算；范围起点之前的不出现，时间按素材算
    let part = silences(&ff, &p, 2000, 4500, -35.0, 500).await.unwrap();
    assert_eq!(part.len(), 2, "{part:?}");
    assert!(part[0].start_ms <= 2100 && part[0].end_ms.abs_diff(2500) <= 60, "{part:?}");
    assert!(silences(&ff, &p, 0, 4500, -35.0, 1800).await.unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn waveform_png_is_drawn_and_cached() {
    let Some(ff) = ffmpeg() else { return };
    let d = dir("wf");
    let f = d.join("a.wav");
    run(&ff, &["-f", "lavfi", "-i", "sine=f=330:d=3:r=44100", "-c:a", "pcm_s16le", &f.to_string_lossy()]);
    let p = f.to_string_lossy().into_owned();
    let out = waveform(&ff, &d.join("pv"), &p, 0, 3000, 600, 80).await.unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let again = waveform(&ff, &d.join("pv"), &p, 0, 3000, 600, 80).await.unwrap();
    assert_eq!(out, again);
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn clip_denoise_and_enhance_reach_the_export() {
    let Some(ff) = ffmpeg() else { return };
    let d = dir("clipfx");
    let f = d.join("n.mp4");
    // 稳定的白噪声底噪 + 间歇的 440 Hz 音
    run(
        &ff,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=s=160x90:r=10:d=6",
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=d=6:c=white:r=48000:a=0.05:s=5",
            "-f",
            "lavfi",
            "-i",
            "sine=f=440:d=6:r=48000",
            "-filter_complex",
            "[2:a]volume='if(lt(mod(t,2),1),0.3,0)':eval=frame[t];[1:a][t]amix=inputs=2:normalize=0[o]",
            "-map",
            "0:v",
            "-map",
            "[o]",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-ac",
            "2",
            &f.to_string_lossy(),
        ],
    );
    let caps = crate::vidcaps::detect(&ff).await;
    if caps.encoders.h264.is_none() || !caps.has_filter("afftdn") {
        return;
    }
    let path = f.to_string_lossy().into_owned();
    let floor = crate::audiofx::estimate_noise_floor(&ff, &f, 0.0, 6).await;
    assert!(floor.is_some());
    let base = Clip { path, out_ms: 6000, ..Default::default() };
    let one = |c: Clip| Project { clips: vec![c], ..Default::default() };
    let (plain, _) = render(&ff, &caps, &d, one(base.clone()), Mode::Export).await;
    let (denoised, plan) = render(&ff, &caps, &d, one(Clip { denoise: "strong".into(), noise_floor_db: floor, ..base.clone() }), Mode::Export).await;
    assert!(plan.args.iter().any(|a| a.contains("afftdn=")), "{:?}", plan.args);
    // 1.2–1.8 秒只有底噪
    let (a, b) = (rms_db(&ff, &plain, 1.2, 0.6), rms_db(&ff, &denoised, 1.2, 0.6));
    // 旧版 ffmpeg（如 4.4）的 afftdn 降幅较小，只要求明显低于原来
    assert!(b < a - 3.0, "底噪应明显下降：{a} → {b} dB");
    // 人声增强：高通 + 均衡 + 压缩都进了滤镜图，导出能跑通
    let (_, plan) = render(&ff, &caps, &d, one(Clip { enhance: true, ..base }), Mode::Export).await;
    assert!(plan.args.iter().any(|a| a.contains("highpass=f=80") && a.contains("acompressor")));
    let _ = std::fs::remove_dir_all(&d);
}

fn moving_box(ff: &Path, out: &Path) {
    // 有纹理的静止背景 + 一个从左向右运动的白方块（drawbox 的 t 是线宽，所以用 overlay 的表达式）
    run(
        ff,
        &[
            "-f",
            "lavfi",
            "-i",
            "nullsrc=s=640x360:r=25:d=6,geq=lum='110+50*sin(X/9)*sin(Y/7)+20*sin(X*Y/900)':cb=128:cr=128",
            "-f",
            "lavfi",
            "-i",
            "color=c=white:s=70x70:r=25:d=6",
            "-filter_complex",
            "[0][1]overlay=x='40+90*t':y=150",
            "-c:v",
            "mpeg4",
            "-q:v",
            "2",
            "-pix_fmt",
            "yuv420p",
            &out.to_string_lossy(),
        ],
    );
}

#[tokio::test]
async fn reframe_follows_the_moving_box() {
    let Some(ff) = ffmpeg() else { return };
    let d = dir("reframe");
    let f = d.join("m.mp4");
    moving_box(&ff, &f);
    let caps = crate::vidcaps::detect(&ff).await;
    let dec = Decoder::new(&ff, &caps);
    let req = ReframeReq { id: 7, path: f.to_string_lossy().into_owned(), from_ms: 0, to_ms: 6000, smooth: Some(0.5) };
    let res = reframe(&dec, &req, |_| {}, Arc::new(AtomicBool::new(false))).await.unwrap();
    assert!(res.tracked, "{:?}", res.note);
    assert!(res.region.is_focus() && res.region.reframe);
    let tr = &res.region.track;
    let first = tr.iter().min_by_key(|p| p.t_ms).unwrap();
    let last = tr.iter().max_by_key(|p| p.t_ms).unwrap();
    // 方块中心从 x≈75 走到 ≈ 40+90*6+35 = 615（占宽度 0.12 → 0.96）
    assert!(first.x + first.w / 2.0 < 0.4, "开头应在左边：{first:?}");
    assert!(last.x + last.w / 2.0 > 0.6, "结尾应在右边：{last:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[tokio::test]
async fn reframe_falls_back_to_the_centre_for_a_still_picture() {
    let Some(ff) = ffmpeg() else { return };
    let d = dir("still");
    let f = d.join("s.mp4");
    run(&ff, &["-f", "lavfi", "-i", "color=c=0x305070:s=320x180:r=25:d=3", "-c:v", "mpeg4", "-pix_fmt", "yuv420p", &f.to_string_lossy()]);
    let caps = crate::vidcaps::detect(&ff).await;
    let dec = Decoder::new(&ff, &caps);
    let req = ReframeReq { id: 8, path: f.to_string_lossy().into_owned(), from_ms: 0, to_ms: 3000, smooth: None };
    let res = reframe(&dec, &req, |_| {}, Arc::new(AtomicBool::new(false))).await.unwrap();
    assert!(!res.tracked && res.note.is_some());
    assert_eq!(res.region.track.len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}
