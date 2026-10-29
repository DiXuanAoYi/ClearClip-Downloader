//! 用真实的 ffmpeg 跑一遍：生成各种“问题素材”，按规格生成参数并执行，再读回输出检查。
//! 机器上没有 ffmpeg（或缺少需要的编码器）时对应的测试自动跳过。

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::analyze;
use super::build::{build, video_graph, NormPlan};
use super::colormatch::{ShotAdjust, ToneAdjust};
use super::facts::{self, Facts, Hdr};
use super::spec::{preset, Analysis, NormSpec};
use crate::media_tools::muxer_args;
use crate::postprocess::{base_args, run_ffmpeg_capture};
use crate::vidcaps::{self, Caps};

fn ffmpeg() -> Option<PathBuf> {
    std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "clearclip-vn-e2e-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// 生成素材：`ins` 是输入部分，`outs` 是输出部分。失败（缺编码器等）返回 None。
async fn gen(ff: &Path, d: &Path, name: &str, ins: &[&str], outs: &[&str]) -> Option<PathBuf> {
    let out = d.join(name);
    let mut a = strs(&["-hide_banner", "-loglevel", "error", "-y"]);
    a.extend(strs(ins));
    a.extend(strs(outs));
    a.push(out.to_string_lossy().into_owned());
    run_ffmpeg_capture(ff, &a, Duration::from_secs(120)).await.ok().map(|_| out)
}

const SRC: &str = "testsrc2=s=640x360:r=30:d=2";

async fn caps_of(ff: &Path) -> Caps {
    vidcaps::detect(ff).await
}

/// 按任务的流程（读取、分析、生成参数、运行）处理一个文件。
async fn normalize(ff: &Path, input: &Path, spec: &NormSpec, caps: &Caps, d: &Path) -> (PathBuf, NormPlan, Facts) {
    let facts = facts::read(ff, input).await.expect("facts");
    let mut an = Analysis::default();
    if spec.fps == "auto" || spec.fix_sync {
        an.vfr = analyze::detect_vfr(ff, input).await;
    }
    if spec.autocrop {
        an.crop = analyze::detect_crop(ff, input, &facts).await;
    }
    if let (Some(t), Some(_)) = (spec.loudness, &facts.audio) {
        an.loudness = analyze::measure_loudness(ff, input, t).await;
    }
    if spec.match_color > 0.0 {
        let plan = match super::colormatch::analyze_cached(ff, input, &facts, caps, &spec.match_skip).await {
            Ok(a) => a.plan(spec.match_color, &spec.match_tone, &spec.match_shots),
            Err(_) => super::colormatch::tone_plan(&spec.match_tone, &spec.match_skip),
        };
        if !plan.fixes.is_empty() {
            an.color = Some(plan);
        }
    }
    let plan = build(input, spec, &facts, &an, caps, &d.join("tmp")).expect("build");
    let out = d.join(format!("out.{}", plan.ext));
    let _ = std::fs::remove_file(&out);
    if !plan.unchanged {
        let mut a = base_args();
        a.extend(plan.args.clone());
        a.extend(muxer_args(plan.ext, &out));
        run_ffmpeg_capture(ff, &a, Duration::from_secs(300)).await.unwrap_or_else(|e| panic!("ffmpeg 失败：{e}\n参数：{a:?}"));
    }
    (out, plan, facts)
}

/// 取第一帧缩成 32×32 的灰度（0–255 全范围）。
async fn gray(ff: &Path, file: &Path, pre: &str) -> Vec<u8> {
    let vf = format!("{pre}scale=32:32:flags=area:in_range=tv:out_range=pc,format=gray");
    let a = strs(&["-hide_banner", "-loglevel", "error", "-i", &file.to_string_lossy(), "-vf", &vf, "-frames:v", "1", "-f", "rawvideo", "-"]);
    run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap().0
}

fn mean(v: &[u8]) -> f64 {
    v.iter().map(|b| f64::from(*b)).sum::<f64>() / v.len().max(1) as f64
}

fn spread(v: &[u8]) -> f64 {
    f64::from(*v.iter().max().unwrap_or(&0)) - f64::from(*v.iter().min().unwrap_or(&0))
}

macro_rules! need {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return,
        }
    };
}

async fn setup(name: &str) -> Option<(PathBuf, PathBuf, Caps)> {
    let ff = ffmpeg()?;
    let caps = caps_of(&ff).await;
    if caps.encoders.h264 != Some("libx264") {
        return None;
    }
    Some((ff, dir(name), caps))
}

async fn hdr_sample(ff: &Path, d: &Path, trc: &str) -> Option<PathBuf> {
    // 用 setparams 写色彩标记：新版 ffmpeg 不再认编码输出上的 -color_primaries / -color_trc
    let vf = format!("setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc={trc}:range=tv");
    gen(
        ff,
        d,
        "hdr.mp4",
        &["-f", "lavfi", "-i", SRC, "-f", "lavfi", "-i", "sine=d=2"],
        &["-vf", &vf, "-c:v", "libx264", "-pix_fmt", "yuv420p10le", "-c:a", "aac", "-shortest"],
    )
    .await
}

#[tokio::test]
async fn hdr_is_tone_mapped_by_zscale_and_by_the_builtin_lut() {
    let (ff, d, caps) = need!(setup("hdr").await);
    for (trc, kind) in [("smpte2084", Hdr::Pq), ("arib-std-b67", Hdr::Hlg)] {
        let src = need!(hdr_sample(&ff, &d, trc).await);
        let f = facts::read(&ff, &src).await.unwrap();
        assert_eq!(f.video.as_ref().unwrap().hdr, kind);
        let spec = preset("compat").unwrap();

        let mut outs = vec![];
        let mut variants = vec![("zscale", caps.clone())];
        let mut lite = caps.clone();
        lite.filters.remove("zscale");
        lite.filters.remove("tonemap");
        variants.push(("lut", lite));
        for (name, c) in variants {
            if name == "zscale" && !c.can_tonemap() {
                continue;
            }
            let sub = d.join(format!("{trc}-{name}"));
            std::fs::create_dir_all(&sub).unwrap();
            let (out, plan, _) = normalize(&ff, &src, &spec, &c, &sub).await;
            assert!(!plan.unchanged && !plan.video_copy, "{name}");
            let o = facts::read(&ff, &out).await.unwrap();
            let v = o.video.unwrap();
            assert_eq!(
                (v.hdr, v.bit_depth, v.transfer.as_deref(), v.matrix.as_deref(), v.range.as_deref()),
                (Hdr::None, 8, Some("bt709"), Some("bt709"), Some("tv")),
                "{name}: {v:?}"
            );
            assert_eq!((v.width, v.height), (640, 360), "不超过 1080p 不缩放");
            assert!(o.audio.is_some(), "音频保留");
            // 用转成 RGB（超出范围的分量被截断）之后的亮度比较，这才是屏幕上看到的样子
            outs.push((name, gray(&ff, &out, "format=rgb24,").await));
        }
        if let [(_, a), (_, b)] = &outs[..] {
            let (ma, mb) = (mean(a), mean(b));
            assert!((ma - mb).abs() < 30.0, "{trc}：zscale 平均亮度 {ma:.0}，内置 LUT {mb:.0}，相差太大");
        }
    }
}

#[tokio::test]
async fn variable_frame_rate_becomes_constant() {
    let (ff, d, caps) = need!(setup("vfr").await);
    let src = need!(
        gen(
            &ff,
            &d,
            "vfr.mp4",
            &["-f", "lavfi", "-i", "testsrc2=s=320x240:r=30:d=6", "-f", "lavfi", "-i", "sine=d=6"],
            &["-vf", "select='lt(mod(n,10),3)'", "-fps_mode", "vfr", "-c:v", "libx264", "-c:a", "aac", "-shortest"]
        )
        .await
    );
    let before = analyze::detect_vfr(&ff, &src).await.unwrap();
    assert!(before.variable, "{before:?}");
    let spec = preset("screen").unwrap();
    let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    assert!(plan.notes.iter().any(|n| n.contains("可变帧率转成固定")), "{:?}", plan.notes);
    let after = analyze::detect_vfr(&ff, &out).await.unwrap();
    assert!(!after.variable, "{after:?}");
    let fps = facts::read(&ff, &out).await.unwrap().video.unwrap().fps.unwrap();
    assert!([24.0, 25.0, 30.0, 50.0, 60.0, 23.976, 29.97, 59.94].iter().any(|s| (s - fps).abs() < 0.1), "{fps}");
}

#[tokio::test]
async fn black_bars_are_cropped() {
    let (ff, d, caps) = need!(setup("bars").await);
    let src = need!(gen(&ff, &d, "bars.mp4", &["-f", "lavfi", "-i", SRC], &["-vf", "pad=640:480:0:60:black", "-c:v", "libx264", "-pix_fmt", "yuv420p"]).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let c = analyze::detect_crop(&ff, &src, &facts).await.expect("检测到黑边");
    assert_eq!((c.w, c.x), (640, 0));
    assert!((i64::from(c.h) - 360).abs() <= 4 && (i64::from(c.y) - 60).abs() <= 4, "{c:?}");
    let (out, plan, _) = normalize(&ff, &src, &preset("platform").unwrap(), &caps, &d).await;
    assert!(plan.notes.iter().any(|n| n.contains("去黑边")));
    let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
    assert!((i64::from(v.height) - 360).abs() <= 4 && v.width == 640, "{}×{}", v.width, v.height);
}

#[tokio::test]
async fn live_recording_is_remuxed_synced_and_loudness_normalized() {
    let (ff, d, caps) = need!(setup("live").await);
    let src = need!(
        gen(
            &ff,
            &d,
            "live.flv",
            &["-f", "lavfi", "-i", SRC, "-f", "lavfi", "-i", "sine=f=440:d=2,volume=0.05"],
            &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-ar", "44100", "-shortest", "-f", "flv"]
        )
        .await
    );
    let (out, plan, f) = normalize(&ff, &src, &preset("live").unwrap(), &caps, &d).await;
    assert!(f.is_stream_container() && plan.video_copy && plan.ext == "mp4");
    let o = facts::read(&ff, &out).await.unwrap();
    assert!(o.container.contains("mp4"), "{}", o.container);
    assert_eq!(o.video.as_ref().unwrap().codec, "h264");
    let a = o.audio.unwrap();
    assert_eq!((a.codec.as_str(), a.sample_rate, a.channels), ("aac", 48000, 2));
    // 两遍响度标准化：输出的响度应接近 -16 LUFS
    let m = analyze::measure_loudness(&ff, &out, -16.0).await.expect("测得响度");
    assert!((m.input_i + 16.0).abs() < 2.0, "输出响度 {} LUFS", m.input_i);
}

#[tokio::test]
async fn rotation_is_kept_when_copying_and_baked_when_reencoding() {
    let (ff, d, caps) = need!(setup("rot").await);
    let plain = need!(
        gen(
            &ff,
            &d,
            "plain.mp4",
            &["-f", "lavfi", "-i", SRC, "-f", "lavfi", "-i", "sine=d=2"],
            &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]
        )
        .await
    );
    let rot = need!(gen(&ff, &d, "rot.mp4", &["-display_rotation", "90", "-i", &plain.to_string_lossy()], &["-c", "copy"]).await);
    let f = facts::read(&ff, &rot).await.unwrap();
    assert_eq!(f.video.as_ref().unwrap().rotation, 90, "{:?}", f.video);
    // 普通预设：已经符合规格，不动
    let (_, plan, _) = normalize(&ff, &rot, &preset("compat").unwrap(), &caps, &d).await;
    assert!(plan.unchanged || plan.video_copy, "{:?}", plan.notes);
    // 统一成竖屏 1080×1920：画面转正后缩放
    let sub = d.join("edit");
    std::fs::create_dir_all(&sub).unwrap();
    let (out, plan, _) = normalize(&ff, &rot, &preset("edit").unwrap(), &caps, &sub).await;
    assert!(plan.notes.iter().any(|n| n.contains("旋转标记")), "{:?}", plan.notes);
    let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
    assert_eq!((v.width, v.height, v.rotation), (1080, 1920, 0), "{v:?}");
}

#[tokio::test]
async fn full_range_is_converted_to_limited_bt709() {
    let (ff, d, caps) = need!(setup("range").await);
    let src = need!(
        gen(
            &ff,
            &d,
            "full.mp4",
            &["-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30:d=1"],
            &["-vf", "format=yuvj420p", "-c:v", "libx264", "-pix_fmt", "yuvj420p"]
        )
        .await
    );
    let f = facts::read(&ff, &src).await.unwrap();
    assert!(f.video.as_ref().unwrap().full_range(), "{:?}", f.video);
    let (out, _, _) = normalize(&ff, &src, &NormSpec { size: "keep".into(), ..Default::default() }, &caps, &d).await;
    let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
    assert_eq!((v.pix_fmt.as_str(), v.range.as_deref(), v.matrix.as_deref()), ("yuv420p", Some("tv"), Some("bt709")), "{v:?}");
}

#[tokio::test]
async fn landscape_becomes_vertical_with_blurred_background() {
    let (ff, d, caps) = need!(setup("blur").await);
    let src = need!(gen(&ff, &d, "land.mp4", &["-f", "lavfi", "-i", SRC], &["-c:v", "libx264", "-pix_fmt", "yuv420p"]).await);
    let (out, _, _) = normalize(&ff, &src, &preset("shorts").unwrap(), &caps, &d).await;
    let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
    assert_eq!((v.width, v.height), (1080, 1920));
    // 上方补边区域不是纯黑（模糊背景）
    let top = gray(&ff, &out, "crop=1080:300:0:0,").await;
    assert!(mean(&top) > 12.0, "上方应是模糊背景，平均亮度 {}", mean(&top));
}

const INVERT_CUBE: &str = "LUT_3D_SIZE 2\n1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n";

#[tokio::test]
async fn lut_is_applied_with_strength_and_awkward_paths() {
    let (ff, d, caps) = need!(setup("lut").await);
    let src = need!(gen(&ff, &d, "src.mp4", &["-f", "lavfi", "-i", SRC], &["-c:v", "libx264", "-pix_fmt", "yuv420p"]).await);
    // 路径里有空格、逗号、冒号、单引号、方括号：滤镜参数的转义要对
    let odd = d.join("a b,c:d'e [f]");
    std::fs::create_dir_all(&odd).unwrap();
    let lut = odd.join("invert.cube");
    std::fs::write(&lut, INVERT_CUBE).unwrap();
    let base = mean(&gray(&ff, &src, "").await);
    let mut got = vec![];
    for strength in [1.0, 0.5] {
        let spec = NormSpec { size: "keep".into(), lut: Some(lut.to_string_lossy().into_owned()), lut_strength: strength, ..Default::default() };
        let sub = d.join(format!("s{}", (strength * 10.0) as u32));
        std::fs::create_dir_all(&sub).unwrap();
        let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &sub).await;
        assert!(plan.notes.iter().any(|n| n.contains("应用 LUT")));
        got.push(mean(&gray(&ff, &out, "").await));
    }
    assert!((got[0] - (255.0 - base)).abs() < 40.0, "反相后的平均亮度应接近 255-{base:.0}，实际 {:.0}", got[0]);
    let mid = (base + got[0]) / 2.0;
    assert!((got[1] - mid).abs() < 25.0, "一半强度应在原图和反相之间：{base:.0} / {:.0} / {:.0}", got[1], got[0]);
}

#[tokio::test]
async fn auto_levels_stretch_a_flat_picture() {
    let (ff, d, caps) = need!(setup("levels").await);
    let src = need!(
        gen(
            &ff,
            &d,
            "flat.mp4",
            &["-f", "lavfi", "-i", SRC],
            &["-vf", "format=gray,eq=contrast=0.35,format=yuv420p", "-c:v", "libx264", "-pix_fmt", "yuv420p"]
        )
        .await
    );
    let before = spread(&gray(&ff, &src, "").await);
    let spec = NormSpec { size: "keep".into(), levels: 1.0, ..Default::default() };
    let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    assert!(plan.notes.iter().any(|n| n.contains("自动色阶")));
    let after = spread(&gray(&ff, &out, "").await);
    assert!(after > before * 1.25, "对比度应被拉开：{before} → {after}");
}

#[tokio::test]
async fn hevc_output_when_an_encoder_exists() {
    let (ff, d, caps) = need!(setup("hevc").await);
    need!(caps.hevc);
    let src = need!(
        gen(
            &ff,
            &d,
            "src.mp4",
            &["-f", "lavfi", "-i", SRC, "-f", "lavfi", "-i", "sine=d=2"],
            &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]
        )
        .await
    );
    let (out, _, _) = normalize(&ff, &src, &preset("archive").unwrap(), &caps, &d).await;
    assert_eq!(facts::read(&ff, &out).await.unwrap().video.unwrap().codec, "hevc");
}

#[tokio::test]
async fn untagged_video_is_tagged_without_reencoding() {
    let (ff, d, caps) = need!(setup("tags").await);
    let src = need!(gen(&ff, &d, "untagged.mp4", &["-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30:d=1"], &["-c:v", "libx264", "-pix_fmt", "yuv420p"]).await);
    let before = facts::read(&ff, &src).await.unwrap().video.unwrap();
    assert!(before.untagged());
    let (out, plan, _) = normalize(&ff, &src, &preset("platform").unwrap(), &caps, &d).await;
    assert!(plan.video_copy && plan.notes.iter().any(|n| n.contains("补写了色彩标记")), "{:?}", plan.notes);
    let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
    assert_eq!((v.matrix.as_deref(), v.transfer.as_deref()), (Some("bt709"), Some("bt709")), "{v:?}");
    assert_eq!(v.codec, "h264");
}

#[tokio::test]
async fn preview_frames_are_written() {
    let (ff, d, caps) = need!(setup("preview").await);
    let src = need!(hdr_sample(&ff, &d, "smpte2084").await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let vg = video_graph(&preset("compat").unwrap(), &facts, &Analysis::default(), &caps, &d.join("tmp"), "yuv420p", true).unwrap();
    let (before, after) = analyze::preview(&ff, &src, 500, 0, vg.graph.as_deref(), &d, "k1").await.unwrap();
    for p in [&before, &after] {
        assert!(std::fs::metadata(p).unwrap().len() > 500, "{p:?}");
    }
    // 超出视频长度
    assert!(analyze::preview(&ff, &src, 600_000, 0, vg.graph.as_deref(), &d, "k2").await.is_err());
}

#[tokio::test]
async fn stabilization_runs_with_vidstab_and_with_deshake() {
    use super::job::stabilize_plan;
    let (ff, d, caps) = need!(setup("stab").await);
    let src = need!(
        gen(
            &ff,
            &d,
            "shaky.mp4",
            &["-f", "lavfi", "-i", "testsrc2=s=640x360:r=30:d=3"],
            &["-vf", "crop=580:300:30+12*sin(n/2.5):30+12*cos(n/3.1)", "-c:v", "libx264", "-pix_fmt", "yuv420p"]
        )
        .await
    );
    let facts = facts::read(&ff, &src).await.unwrap();
    let mut variants = vec![("vidstab", caps.clone())];
    let mut no_vidstab = caps.clone();
    no_vidstab.filters.remove("vidstabdetect");
    no_vidstab.filters.remove("vidstabtransform");
    variants.push(("deshake", no_vidstab));
    for (name, c) in variants {
        if name == "vidstab" && !c.vidstab() {
            continue;
        }
        let sub = d.join(name);
        std::fs::create_dir_all(&sub).unwrap();
        let trf = sub.join("s.trf");
        let plan = stabilize_plan(&src, "normal", &trf, &c, &facts).unwrap();
        assert_eq!(plan.pass1.is_some(), name == "vidstab");
        if let Some(p1) = &plan.pass1 {
            let mut a = base_args();
            a.extend(p1.clone());
            run_ffmpeg_capture(&ff, &a, Duration::from_secs(120)).await.unwrap_or_else(|e| panic!("{e}\n{a:?}"));
            assert!(std::fs::metadata(&trf).unwrap().len() > 0);
        }
        let out = sub.join("out.mp4");
        let mut a = base_args();
        a.extend(plan.pass2.clone());
        a.extend(muxer_args(plan.ext, &out));
        run_ffmpeg_capture(&ff, &a, Duration::from_secs(120)).await.unwrap_or_else(|e| panic!("{e}\n{a:?}"));
        let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
        assert_eq!((v.width, v.height), (580, 300), "{name}");
    }
    // 两种滤镜都没有：明确报错
    let mut none = caps.clone();
    for f in ["vidstabdetect", "vidstabtransform", "deshake"] {
        none.filters.remove(f);
    }
    assert!(stabilize_plan(&src, "normal", &d.join("x.trf"), &none, &facts).is_err());
}

#[tokio::test]
async fn already_compliant_files_are_reported_unchanged() {
    let (ff, d, caps) = need!(setup("unchanged").await);
    let src = need!(
        gen(
            &ff,
            &d,
            "ok.mp4",
            &["-f", "lavfi", "-i", SRC, "-f", "lavfi", "-i", "sine=d=2"],
            &[
                "-vf",
                "setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest"
            ]
        )
        .await
    );
    let (_, plan, _) = normalize(&ff, &src, &preset("compat").unwrap(), &caps, &d).await;
    assert!(plan.unchanged, "{:?}", plan.notes);
}

// ---------- 分段色彩匹配 ----------

use super::colormatch::{self, FrameStats};

/// 五个镜头（每个 3 秒、内容不同）拼成一个视频：第 2 个偏黄，第 4 个又亮又发灰，其余三个是正常的中性色调。
/// `sparse_keys` 为真时只在开头有一个关键帧（模拟录屏、直播录制）。
async fn mashup(ff: &Path, d: &Path, name: &str, sparse_keys: bool) -> Option<PathBuf> {
    // (亮度基准, 亮度花纹频率, U 偏移, V 偏移, 色度花纹幅度)
    mashup_of(ff, d, name, sparse_keys, [(110, 18, 0, 0, 10), (110, 26, -16, 7, 10), (108, 14, 0, 0, 10), (155, 22, 0, 0, 3), (112, 30, 0, 0, 10)]).await
}

async fn mashup_of(ff: &Path, d: &Path, name: &str, sparse_keys: bool, shots: [(i32, i32, i32, i32, i32); 5]) -> Option<PathBuf> {
    let srcs: Vec<String> = shots
        .iter()
        .enumerate()
        .map(|(i, (y, f, du, dv, amp))| {
            format!(
                // 色度花纹取整数个周期（零均值），这样“偏色”才只来自 du / dv
                "nullsrc=s=320x180:r=25:d=3,format=yuv420p,geq=lum='{y}+45*sin(X/{f}+T+{i})*cos(Y/{}-T/2)':cb='{}+{amp}*sin(2*PI*2*X/160+{i})':cr='{}+{amp}*cos(2*PI*2*Y/90+{i})'",
                f + 7,
                128 + du,
                128 + dv
            )
        })
        .collect();
    let mut a = strs(&["-hide_banner", "-loglevel", "error", "-y"]);
    for s in &srcs {
        a.extend(strs(&["-f", "lavfi", "-i", s]));
    }
    a.extend(strs(&[
        "-filter_complex",
        "[0:v][1:v][2:v][3:v][4:v]concat=n=5:v=1:a=0[v]",
        "-map",
        "[v]",
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "14",
        "-pix_fmt",
        "yuv420p",
    ]));
    if sparse_keys {
        a.extend(strs(&["-g", "9999", "-sc_threshold", "0"]));
    } else {
        a.extend(strs(&["-g", "25", "-force_key_frames", "expr:gte(t,n_forced*3)"]));
    }
    let out = d.join(name);
    a.push(out.to_string_lossy().into_owned());
    run_ffmpeg_capture(ff, &a, Duration::from_secs(120)).await.ok().map(|_| out)
}

fn ms_close(a: u64, b: u64, tol: u64) -> bool {
    a.abs_diff(b) <= tol
}

/// 取视频某一时刻的一帧，统计色彩。
async fn stats_at(ff: &Path, file: &Path, t: f64) -> FrameStats {
    let a = strs(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-ss",
        &format!("{t:.3}"),
        "-i",
        &file.to_string_lossy(),
        "-vf",
        "scale=64:36:flags=area,format=yuv444p",
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-",
    ]);
    let raw = run_ffmpeg_capture(ff, &a, Duration::from_secs(30)).await.unwrap().0;
    let n = 64 * 36;
    colormatch::frame_stats(&raw[..n], &raw[n..2 * n], &raw[2 * n..3 * n])
}

fn dist_to_ref(s: &FrameStats, r: &FrameStats) -> f64 {
    ((s.nu - r.nu).hypot(s.nv - r.nv)).max((s.y - r.y).abs() / 4.0)
}

#[tokio::test]
async fn color_outliers_between_shots_are_found_and_corrected() {
    let (ff, d, caps) = need!(setup("colormatch").await);
    let src = need!(mashup(&ff, &d, "m.mp4", false).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let a = colormatch::analyze(&ff, &src, &facts, &caps).await.expect("analyze");
    let rep = a.report();
    assert_eq!(rep.shots_total, 5, "{rep:?}");
    let ids: Vec<(u64, u64)> = rep.flagged.iter().map(|f| (f.start_ms, f.end_ms)).collect();
    assert_eq!(ids.len(), 2, "只有第 2、4 个镜头不一致：{rep:?}");
    // 分界精确到帧（25 帧 = 40 毫秒）
    assert!(ms_close(ids[0].0, 3000, 45) && ms_close(ids[0].1, 6000, 45), "{ids:?}");
    assert!(ms_close(ids[1].0, 9000, 45) && ms_close(ids[1].1, 12000, 45), "{ids:?}");
    assert!(rep.flagged[0].defects.contains(&"偏黄".to_string()), "{:?}", rep.flagged[0]);
    let second = &rep.flagged[1].defects;
    assert!(second.contains(&"偏亮".to_string()) && second.contains(&"饱和度偏低".to_string()), "{second:?}");

    // 处理：校正后再分析，应该没有不一致的镜头；一致的镜头不动
    let mut spec = preset("mashup").unwrap();
    spec.match_color = 1.0;
    let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    assert!(plan.notes.iter().any(|n| n.contains("分段色彩匹配：2 个不一致的镜头自动校正")), "{:?}", plan.notes);
    let f2 = facts::read(&ff, &out).await.unwrap();
    let after = colormatch::analyze(&ff, &out, &f2, &caps).await.expect("analyze after");
    assert!(after.report().flagged.is_empty(), "校正后不应再有不一致的镜头：{:?}", after.report());

    // 逐个镜头的中间时刻各取一帧比较：一致的镜头（1、3、5）不该被改动，不一致的（2、4）要向整体靠拢
    let mid = [1.5, 4.5, 7.5, 10.5, 13.5];
    let mut before = vec![];
    let mut now = vec![];
    for t in mid {
        before.push(stats_at(&ff, &src, t).await);
        now.push(stats_at(&ff, &out, t).await);
    }
    for i in [0, 2, 4] {
        let (b, n) = (&before[i], &now[i]);
        assert!(
            (b.y - n.y).abs() < 3.0 && (b.nu - n.nu).abs() < 2.5 && (b.nv - n.nv).abs() < 2.5 && (b.sat - n.sat).abs() < 1.5,
            "镜头 {i} 本来一致，不该被改动：{b:?} → {n:?}"
        );
    }
    // 整体的色调：三个一致镜头的平均
    let r = FrameStats {
        y: (before[0].y + before[2].y + before[4].y) / 3.0,
        nu: (before[0].nu + before[2].nu + before[4].nu) / 3.0,
        nv: (before[0].nv + before[2].nv + before[4].nv) / 3.0,
        sat: (before[0].sat + before[2].sat + before[4].sat) / 3.0,
        ..Default::default()
    };
    for i in [1, 3] {
        let (b, n) = (dist_to_ref(&before[i], &r), dist_to_ref(&now[i], &r));
        assert!(n < b * 0.35, "镜头 {i} 和整体的差距应该大幅缩小：{b:.1} → {n:.1}（{:?} → {:?}）", before[i], now[i]);
    }
    // 饱和度偏低的那个镜头：饱和度向整体靠拢
    assert!((now[3].sat - r.sat).abs() < (before[3].sat - r.sat).abs() * 0.6, "{:?} → {:?}，整体 {r:?}", before[3], now[3]);
}

#[tokio::test]
async fn sparse_keyframes_fall_back_to_timed_sampling() {
    let (ff, d, caps) = need!(setup("colormatch-sparse").await);
    let src = need!(mashup(&ff, &d, "s.mp4", true).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let a = colormatch::analyze(&ff, &src, &facts, &caps).await.expect("analyze");
    let rep = a.report();
    let ids: Vec<(u64, u64)> = rep.flagged.iter().map(|f| (f.start_ms, f.end_ms)).collect();
    assert_eq!(ids.len(), 2, "{rep:?}");
    // 没有关键帧也能靠场景检测定位到帧
    assert!(ms_close(ids[0].0, 3000, 45) && ms_close(ids[0].1, 6000, 45), "{ids:?}");
    assert!(ms_close(ids[1].0, 9000, 45) && ms_close(ids[1].1, 12000, 45), "{ids:?}");
}

#[tokio::test]
async fn consistent_footage_is_left_alone() {
    let (ff, d, caps) = need!(setup("colormatch-same").await);
    // 内容各不相同、但色调一致的五个镜头（自然的亮度、饱和度差别）：一个都不该被改
    let natural = need!(
        mashup_of(&ff, &d, "n.mp4", false, [(100, 18, 0, 0, 10), (118, 26, 1, -1, 12), (108, 14, 0, 1, 9), (96, 22, -1, 0, 11), (122, 30, 0, 0, 10)]).await
    );
    let nf = facts::read(&ff, &natural).await.unwrap();
    let na = colormatch::analyze(&ff, &natural, &nf, &caps).await.expect("analyze");
    assert!(na.report().flagged.is_empty(), "{:?}", na.report());
    // 色彩很浓、画面一直在变的测试图：同样不该被当成不一致
    let src =
        need!(gen(&ff, &d, "same.mp4", &["-f", "lavfi", "-i", "testsrc2=s=320x180:r=25:d=12"], &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-g", "25"]).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let a = colormatch::analyze(&ff, &src, &facts, &caps).await.expect("analyze");
    assert!(a.report().flagged.is_empty(), "{:?}", a.report());
    let mut spec = preset("mashup").unwrap();
    spec.size = "keep".into();
    let (_, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    assert!(!plan.notes.iter().any(|n| n.starts_with("分段色彩匹配")), "{:?}", plan.notes);
}

#[tokio::test]
async fn excluded_shots_and_preview_use_the_same_plan() {
    let (ff, d, caps) = need!(setup("colormatch-preview").await);
    let src = need!(mashup(&ff, &d, "p.mp4", false).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let a = colormatch::analyze(&ff, &src, &facts, &caps).await.expect("analyze");
    let full = a.plan(1.0, &ToneAdjust::default(), &[]);
    assert_eq!(full.fixes.len(), 2);
    // 取消第 2 个镜头（开头约 3000 毫秒）：只剩第 4 个
    let skip = a.report().flagged[0].id;
    let skipped = [ShotAdjust { id: skip, strength: Some(0.0), ..Default::default() }];
    assert_eq!(a.plan(1.0, &ToneAdjust::default(), &skipped).fixes.len(), 1);

    // 预览：在第 2 个镜头里（4 秒处）截一帧，处理后的偏色应该比处理前小得多
    let spec = NormSpec { match_color: 1.0, ..Default::default() };
    let an = Analysis { color: Some(full.shifted(4000)), ..Default::default() };
    let vg = video_graph(&spec, &facts, &an, &caps, &d.join("tmp"), "yuv420p", true).unwrap();
    let (before, after) = analyze::preview(&ff, &src, 4000, 0, vg.graph.as_deref(), &d, "pv").await.unwrap();
    let stat = |f: &Path| {
        let ff = ff.clone();
        let f = f.to_path_buf();
        async move {
            let a = strs(&[
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                &f.to_string_lossy(),
                "-vf",
                "scale=64:36:flags=area,format=yuv444p",
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-",
            ]);
            let raw = run_ffmpeg_capture(&ff, &a, Duration::from_secs(30)).await.unwrap().0;
            let n = 64 * 36;
            colormatch::frame_stats(&raw[..n], &raw[n..2 * n], &raw[2 * n..3 * n])
        }
    };
    let (b, n) = (stat(&before).await, stat(&after).await);
    let (_, r) = a.looks();
    assert!(dist_to_ref(&n, &r.st) < dist_to_ref(&b, &r.st) * 0.5, "预览应该已经校正：{b:?} → {n:?}");
    // 同一时刻之外（第 3 个镜头，7 秒处）预览不应有任何改动
    let an2 = Analysis { color: Some(full.shifted(7000)), ..Default::default() };
    let vg2 = video_graph(&spec, &facts, &an2, &caps, &d.join("tmp"), "yuv420p", true).unwrap();
    let (b2, a2) = analyze::preview(&ff, &src, 7000, 0, vg2.graph.as_deref(), &d, "pv2").await.unwrap();
    let (sb, sa) = (stat(&b2).await, stat(&a2).await);
    assert!((sb.y - sa.y).abs() < 2.0 && (sb.nu - sa.nu).abs() < 2.0, "{sb:?} → {sa:?}");
}

#[tokio::test]
async fn skipped_time_is_neither_counted_nor_corrected() {
    let (ff, d, caps) = need!(setup("colormatch-skip").await);
    let src = need!(mashup(&ff, &d, "k.mp4", false).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let span = |a: u64, b: u64| colormatch::SkipSpan { start_ms: a, end_ms: Some(b) };

    // 1. 整个镜头排除：偏黄的第 2 个镜头不再被标记，也不统计；偏亮的第 4 个镜头照常
    let a = colormatch::analyze_cached(&ff, &src, &facts, &caps, &[span(3000, 6000)]).await.expect("analyze");
    let rep = a.report();
    assert_eq!((rep.shots_total, rep.shots_excluded), (5, 1), "{rep:?}");
    assert_eq!(rep.flagged.len(), 1, "{rep:?}");
    assert!(ms_close(rep.flagged[0].start_ms, 9000, 45), "{rep:?}");
    assert_eq!(rep.skipped.iter().map(|s| (s.start_ms, s.end_ms)).collect::<Vec<_>>(), vec![(3000, 6000)]);
    // 不排除时两个都标记；换了排除的时间段不用重新分析整个文件（结果各自缓存）
    let none = colormatch::analyze_cached(&ff, &src, &facts, &caps, &[]).await.expect("analyze");
    assert_eq!(none.report().flagged.len(), 2);
    assert_eq!(none.report().shots_excluded, 0);

    // 处理：被排除的时间原样不动，其余的不一致镜头照常校正
    let mut spec = preset("mashup").unwrap();
    spec.match_color = 1.0;
    spec.match_skip = vec![span(3000, 6000)];
    let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    assert!(plan.notes.iter().any(|n| n.contains("1 个不一致的镜头自动校正") && n.contains("1 段排除的时间保持原样")), "{:?}", plan.notes);
    let (b_skip, n_skip) = (stats_at(&ff, &src, 4.5).await, stats_at(&ff, &out, 4.5).await);
    assert!(
        (b_skip.y - n_skip.y).abs() < 3.0 && (b_skip.nu - n_skip.nu).abs() < 2.5 && (b_skip.nv - n_skip.nv).abs() < 2.5,
        "排除的镜头不该被改：{b_skip:?} → {n_skip:?}"
    );
    let (b_bad, n_bad) = (stats_at(&ff, &src, 10.5).await, stats_at(&ff, &out, 10.5).await);
    assert!((b_bad.y - n_bad.y).abs() > 15.0, "第 4 个镜头应该被校正：{b_bad:?} → {n_bad:?}");

    // 2. 只排除镜头中间的一小段：镜头照样校正，排除的那一段保持原样
    spec.match_skip = vec![span(3600, 4600)];
    let (out, _, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    let (b_in, n_in) = (stats_at(&ff, &src, 4.1).await, stats_at(&ff, &out, 4.1).await);
    assert!((b_in.nu - n_in.nu).abs() < 2.5 && (b_in.nv - n_in.nv).abs() < 2.5, "排除的一段不该被改：{b_in:?} → {n_in:?}");
    let (b_out, n_out) = (stats_at(&ff, &src, 5.5).await, stats_at(&ff, &out, 5.5).await);
    assert!((b_out.nu - n_out.nu).abs() > 6.0, "同一个镜头排除之外的部分应该被校正：{b_out:?} → {n_out:?}");

    // 3. 统一的手动调节同样避开排除的时间
    spec.match_color = 1.0;
    spec.match_tone = ToneAdjust { brightness: 0.5, ..Default::default() };
    spec.match_skip = vec![span(3000, 6000)];
    let (out, _, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    let (b1, n1) = (stats_at(&ff, &src, 1.5).await, stats_at(&ff, &out, 1.5).await);
    assert!(n1.y - b1.y > 12.0, "排除之外的时间应该变亮：{b1:?} → {n1:?}");
    let (b2, n2) = (stats_at(&ff, &src, 4.5).await, stats_at(&ff, &out, 4.5).await);
    assert!((n2.y - b2.y).abs() < 3.0, "排除的时间不该变亮：{b2:?} → {n2:?}");
    // 结尾留空 = 一直到视频结尾
    spec.match_skip = vec![colormatch::SkipSpan { start_ms: 12_000, end_ms: None }];
    let (out, _, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    let (b3, n3) = (stats_at(&ff, &src, 13.5).await, stats_at(&ff, &out, 13.5).await);
    assert!((n3.y - b3.y).abs() < 3.0, "{b3:?} → {n3:?}");
}

// ---------- 输出电平 ----------

/// 一条从暗到亮的斜坡（亮度 16→235 或 0→255），用来看电平被拉开还是压缩。
async fn ramp(ff: &Path, d: &Path, name: &str, full: bool) -> Option<PathBuf> {
    let (lo, span, tag, pix) = if full { (0, 255, "pc", "yuvj420p") } else { (16, 219, "tv", "yuv420p") };
    let vf = format!("format=yuv420p,geq=lum='{lo}+{span}*X/319':cb=128:cr=128,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range={tag}");
    gen(ff, d, name, &["-f", "lavfi", "-i", "nullsrc=s=320x180:r=25:d=2"], &["-vf", &vf, "-c:v", "libx264", "-crf", "8", "-pix_fmt", pix]).await
}

/// 第一帧亮度的最小值、最大值（signalstats 直接读编码里的数值，不做 yuvj / 电平转换）。
async fn luma_extremes(ff: &Path, file: &Path) -> (u32, u32) {
    let a = strs(&[
        "-hide_banner",
        "-loglevel",
        "error",
        "-i",
        &file.to_string_lossy(),
        "-vf",
        "signalstats,metadata=print:file=-",
        "-frames:v",
        "1",
        "-f",
        "null",
        "-",
    ]);
    let out = String::from_utf8_lossy(&run_ffmpeg_capture(ff, &a, Duration::from_secs(30)).await.unwrap().0).into_owned();
    let get = |k: &str| -> u32 {
        out.lines()
            .find_map(|l| l.split_once(&format!("lavfi.signalstats.{k}=")).map(|(_, v)| v.trim().parse().unwrap()))
            .unwrap_or_else(|| panic!("没有 {k}：{out}"))
    };
    (get("YMIN"), get("YMAX"))
}

#[tokio::test]
async fn output_range_16_235_and_0_255_are_both_honoured() {
    let (ff, d, caps) = need!(setup("range").await);
    let limited = need!(ramp(&ff, &d, "tv.mp4", false).await);
    let full = need!(ramp(&ff, &d, "pc.mp4", true).await);
    let spec = |r: &str| NormSpec { size: "keep".into(), out_range: r.into(), ..Default::default() };
    let tags = |f: &Facts| f.video.as_ref().unwrap().full_range();

    // 默认 16–235：电视范围的素材不动；全范围的素材压回 16–235，并写成电视范围
    let (_, plan, _) = normalize(&ff, &limited, &spec("tv"), &caps, &d).await;
    assert!(plan.unchanged, "{:?}", plan.notes);
    let (out, plan, _) = normalize(&ff, &full, &spec("tv"), &caps, &d).await;
    assert!(!plan.unchanged);
    let (lo, hi) = luma_extremes(&ff, &out).await;
    assert!(lo >= 14 && (225..=237).contains(&hi), "全范围素材应压进 16–235：{lo}–{hi}");
    assert!(!tags(&facts::read(&ff, &out).await.unwrap()));

    // 0–255：电视范围的素材拉开，输出标成全范围
    let (out, plan, _) = normalize(&ff, &limited, &spec("pc"), &caps, &d).await;
    assert!(!plan.unchanged && plan.notes.iter().any(|n| n.contains("0–255")), "{:?}", plan.notes);
    let (lo, hi) = luma_extremes(&ff, &out).await;
    assert!(lo <= 6 && hi >= 249, "电视范围的素材应拉开到接近 0–255：{lo}–{hi}");
    assert!(tags(&facts::read(&ff, &out).await.unwrap()), "输出应标成全范围");

    // 全范围的素材选 0–255：数值原样保留
    let (out, plan, _) = normalize(&ff, &full, &spec("pc"), &caps, &d).await;
    if !plan.unchanged {
        let (lo, hi) = luma_extremes(&ff, &out).await;
        assert!(lo <= 6 && hi >= 249, "{lo}–{hi}");
    }

    // 沿用素材：电视范围还是电视范围
    let (out, plan, _) = normalize(&ff, &limited, &spec("keep"), &caps, &d).await;
    assert!(plan.unchanged || !tags(&facts::read(&ff, &out).await.unwrap()));
}

#[tokio::test]
async fn full_range_output_survives_lut_levels_and_hdr() {
    let (ff, d, caps) = need!(setup("range-chain").await);
    let limited = need!(ramp(&ff, &d, "tv.mp4", false).await);
    // 一个什么都不改的 LUT（单位 LUT）：电平仍然要落在 0–255
    let lut = d.join("identity.cube");
    let mut cube = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                cube.push_str(&format!("{r} {g} {b}\n"));
            }
        }
    }
    std::fs::write(&lut, cube).unwrap();
    let spec = NormSpec { size: "keep".into(), out_range: "pc".into(), lut: Some(lut.to_string_lossy().into_owned()), levels: 0.0, ..Default::default() };
    let (out, _, _) = normalize(&ff, &limited, &spec, &caps, &d).await;
    let (lo, hi) = luma_extremes(&ff, &out).await;
    assert!(lo <= 8 && hi >= 247, "LUT 之后仍应是 0–255：{lo}–{hi}");
    assert!(facts::read(&ff, &out).await.unwrap().video.unwrap().full_range());

    // HDR 转 SDR 后同样按所选电平输出（用一条不太亮的 PQ 灰阶：真实素材的高光不会超出范围）
    let hdr = need!(
        gen(
            &ff,
            &d,
            "hdrramp.mp4",
            &["-f", "lavfi", "-i", "nullsrc=s=320x180:r=25:d=2"],
            &[
                "-vf",
                "format=yuv420p10le,geq=lum='64+576*X/319':cb=512:cr=512,setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc=smpte2084:range=tv",
                "-c:v",
                "libx264",
                "-crf",
                "6",
                "-pix_fmt",
                "yuv420p10le",
            ],
        )
        .await
    );
    let mut got = vec![];
    for (r, want_full) in [("tv", false), ("pc", true)] {
        let (out, _, _) = normalize(&ff, &hdr, &NormSpec { out_range: r.into(), ..preset("compat").unwrap() }, &caps, &d).await;
        let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
        assert_eq!(v.full_range(), want_full, "{r}: {v:?}");
        got.push(luma_extremes(&ff, &out).await);
    }
    let ((tv_lo, tv_hi), (pc_lo, pc_hi)) = (got[0], got[1]);
    assert!(tv_lo >= 14 && tv_hi <= 237, "16–235 输出：{tv_lo}–{tv_hi}");
    assert!(pc_lo <= 4 && pc_hi >= tv_hi + 8, "0–255 输出应该比 16–235 的更宽：{pc_lo}–{pc_hi} 对 {tv_lo}–{tv_hi}");
}

// ---------- 降噪 ----------

/// 干净画面和加了噪点的画面（同一份内容），都用接近无损的方式编码，再返回两者的路径。
async fn noisy_pair(ff: &Path, d: &Path) -> Option<(PathBuf, PathBuf)> {
    let clean = gen(
        ff,
        d,
        "clean.mkv",
        &["-f", "lavfi", "-i", "testsrc2=s=640x360:r=25:d=2"],
        &["-vf", "format=yuv420p", "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv420p"],
    )
    .await?;
    let noisy = gen(
        ff,
        d,
        "noisy.mkv",
        &["-i", &clean.to_string_lossy()],
        &["-vf", "noise=alls=16:allf=t+u:all_seed=7,format=yuv420p", "-c:v", "libx264", "-qp", "4", "-pix_fmt", "yuv420p"],
    )
    .await?;
    Some((clean, noisy))
}

/// 两个视频之间的平均 PSNR（dB，越高越接近）。
async fn psnr(ff: &Path, a: &Path, b: &Path) -> f64 {
    let args = strs(&[
        "-hide_banner",
        "-i",
        &a.to_string_lossy(),
        "-i",
        &b.to_string_lossy(),
        "-lavfi",
        "[0:v]format=yuv420p[a];[1:v]format=yuv420p[b];[a][b]psnr",
        "-f",
        "null",
        "-",
    ]);
    let log = run_ffmpeg_capture(ff, &args, Duration::from_secs(120)).await.unwrap().1;
    let at = log.rfind("average:").unwrap_or_else(|| panic!("没有 PSNR 结果：{log}"));
    log[at + 8..].split_whitespace().next().unwrap().parse().unwrap()
}

#[tokio::test]
async fn every_denoise_level_brings_the_picture_closer_to_the_clean_original() {
    let (ff, d, caps) = need!(setup("denoise").await);
    let (clean, noisy) = need!(noisy_pair(&ff, &d).await);
    let base = NormSpec { size: "keep".into(), ..Default::default() };
    let (off, _, _) = normalize(&ff, &noisy, &base, &caps, &d).await;
    let off = {
        let keep = d.join("off.mkv");
        std::fs::copy(&off, &keep).ok();
        if keep.exists() {
            keep
        } else {
            noisy.clone()
        }
    };
    let baseline = psnr(&ff, &off, &clean).await;
    for level in ["light", "medium", "strong", "best"] {
        let spec = NormSpec { denoise: level.into(), ..base.clone() };
        let (out, plan, _) = normalize(&ff, &noisy, &spec, &caps, &d).await;
        if plan.notes.iter().all(|n| !n.starts_with("降噪")) {
            // 这个 ffmpeg 一个降噪滤镜都没有：应该有警告而不是失败
            assert!(plan.warnings.iter().any(|w| w.contains("降噪没有执行")), "{:?}", plan);
            continue;
        }
        let p = psnr(&ff, &out, &clean).await;
        assert!(p > baseline + 1.0, "{level}：降噪后 PSNR {p:.2} dB，没降噪 {baseline:.2} dB，{:?}", plan.notes);
        // 同一个 ffmpeg 里降噪不应该把画面糊成一团：和干净原片的差距至少比噪点小
        assert!(p > 25.0, "{level}：{p:.2} dB");
    }
}

#[tokio::test]
async fn denoise_falls_back_to_the_filters_this_ffmpeg_does_have() {
    let (ff, d, caps) = need!(setup("denoise-fallback").await);
    let (clean, noisy) = need!(noisy_pair(&ff, &d).await);
    let base = NormSpec { size: "keep".into(), ..Default::default() };
    let (off, _, _) = normalize(&ff, &noisy, &base, &caps, &d).await;
    let off_copy = d.join("off.mkv");
    std::fs::copy(&off, &off_copy).unwrap();
    let baseline = psnr(&ff, &off_copy, &clean).await;
    // 依次去掉首选滤镜，剩下的备选在真实的 ffmpeg 上也要能跑，并且有效果
    for (drop, level, want) in [
        (vec!["nlmeans"], "best", "fftdnoiz"),
        (vec!["nlmeans", "fftdnoiz"], "medium", "hqdn3d"),
        (vec!["nlmeans", "fftdnoiz", "hqdn3d"], "medium", "atadenoise"),
    ] {
        if !caps.has_filter(want) {
            continue;
        }
        let mut c = caps.clone();
        for f in &drop {
            c.filters.remove(*f);
        }
        let spec = NormSpec { denoise: level.into(), ..base.clone() };
        let (out, plan, _) = normalize(&ff, &noisy, &spec, &c, &d).await;
        assert!(plan.args.iter().any(|a| a.contains(want)), "{want}：{:?}", plan.args);
        assert!(plan.warnings.iter().any(|w| w.contains("替代")), "{:?}", plan.warnings);
        let p = psnr(&ff, &out, &clean).await;
        assert!(p > baseline + 0.5, "{want}：{p:.2} dB，没降噪 {baseline:.2} dB");
    }
}

#[tokio::test]
async fn denoise_works_together_with_resizing_and_other_steps() {
    let (ff, d, caps) = need!(setup("denoise-chain").await);
    let (_, noisy) = need!(noisy_pair(&ff, &d).await);
    if !caps.has_filter("fftdnoiz") {
        return;
    }
    // 缩小、放大、竖屏模糊填充，每种尺寸方式都和降噪一起跑一遍
    for (size, w, h) in [("limit", 640, 360), ("fit", 1280, 720), ("blur", 360, 640)] {
        let spec =
            NormSpec { size: size.into(), short_side: 240, width: w, height: h, follow_orientation: false, denoise: "medium".into(), ..Default::default() };
        let (out, plan, _) = normalize(&ff, &noisy, &spec, &caps, &d).await;
        assert!(plan.notes.iter().any(|n| n.starts_with("降噪")), "{:?}", plan.notes);
        let v = facts::read(&ff, &out).await.unwrap().video.unwrap();
        match size {
            "limit" => assert_eq!(v.height, 240, "{v:?}"),
            _ => assert_eq!((v.width, v.height), (w, h), "{v:?}"),
        }
    }
}

#[tokio::test]
async fn preview_with_denoise_is_warmed_up_and_cleaner_than_the_original() {
    let (ff, d, caps) = need!(setup("denoise-preview").await);
    let (_, noisy) = need!(noisy_pair(&ff, &d).await);
    if !caps.has_filter("fftdnoiz") {
        return;
    }
    let facts = facts::read(&ff, &noisy).await.unwrap();
    let spec = NormSpec { size: "keep".into(), denoise: "strong".into(), ..Default::default() };
    let vg = video_graph(&spec, &facts, &Analysis::default(), &caps, &d.join("tmp"), "yuv420p", true).unwrap();
    // 预览点在 1 秒处，预热 500 毫秒；也试一个靠近开头的时间点（预热被截到 200 毫秒）
    for (at, warm) in [(1000, 500), (200, 200), (0, 0)] {
        let (before, after) = analyze::preview(&ff, &noisy, at, warm, vg.graph.as_deref(), &d, &format!("w{at}")).await.unwrap();
        let (b, a) = (std::fs::metadata(&before).unwrap().len(), std::fs::metadata(&after).unwrap().len());
        // 噪点去掉以后画面更“干净”，压缩出来的图片也小得多
        assert!(a * 10 < b * 8, "时间点 {at}：处理后 {a} 字节，处理前 {b} 字节");
    }
}

// ---------- 分段色彩匹配：统一调节 / 单独调节 ----------

#[tokio::test]
async fn selected_shots_use_their_own_adjustment_and_the_rest_use_the_unified_one() {
    let (ff, d, caps) = need!(setup("colormatch-tone").await);
    let src = need!(mashup(&ff, &d, "t.mp4", false).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    let a = colormatch::analyze(&ff, &src, &facts, &caps).await.expect("analyze");
    let rep = a.report();
    assert_eq!(rep.flagged.len(), 2, "{rep:?}");
    let (second, fourth) = (rep.flagged[0].id, rep.flagged[1].id);

    // 各个镜头中间时刻的亮度、色彩：处理前
    let mid = [1.5, 4.5, 7.5, 10.5, 13.5];
    let mut before = vec![];
    for t in mid {
        before.push(stats_at(&ff, &src, t).await);
    }
    let run = |tone: ToneAdjust, shots: Vec<ShotAdjust>| {
        let (ff, d, caps, src) = (ff.clone(), d.clone(), caps.clone(), src.clone());
        async move {
            let spec = NormSpec { size: "keep".into(), match_color: 1.0, match_tone: tone, match_shots: shots, ..Default::default() };
            let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
            let mut now = vec![];
            for t in mid {
                now.push(stats_at(&ff, &out, t).await);
            }
            (now, plan)
        }
    };

    // 1. 只有自动校正：一致的镜头不动
    let (base, _) = run(ToneAdjust::default(), vec![]).await;
    for i in [0, 2, 4] {
        assert!((base[i].y - before[i].y).abs() < 3.0, "镜头 {i}：{:?} → {:?}", before[i], base[i]);
    }

    // 2. 统一提亮；第 2 个镜头单独调节（自动校正，自己的调节为 0）：其余镜头提亮，它不提亮
    let bright = ToneAdjust { brightness: 0.5, ..Default::default() };
    let (now, plan) = run(bright, vec![ShotAdjust { id: second, strength: Some(1.0), tone: ToneAdjust::default() }]).await;
    assert!(plan.notes.iter().any(|n| n.contains("1 个镜头单独调节") && n.contains("其余镜头用统一的手动调节")), "{:?}", plan.notes);
    for i in [0, 2, 4] {
        assert!(now[i].y > before[i].y + 12.0, "镜头 {i} 应按统一参数提亮：{:?} → {:?}", before[i], now[i]);
    }
    assert!(now[1].y < base[1].y + 3.0, "单独调节的镜头不该再被统一提亮：自动校正 {:?}，现在 {:?}", base[1], now[1]);
    // 没被选中的不一致镜头（第 4 个）：自动校正之外也要统一提亮
    assert!(now[3].y > base[3].y + 12.0, "第 4 个镜头用统一参数：{:?} → {:?}", base[3], now[3]);

    // 3. 单独调节有自己的参数：第 2 个镜头调暗，和统一的提亮方向相反
    let dark = ToneAdjust { brightness: -0.5, ..Default::default() };
    let (now, _) = run(bright, vec![ShotAdjust { id: second, strength: Some(1.0), tone: dark }]).await;
    assert!(now[1].y < base[1].y - 12.0, "单独调暗：自动校正 {:?}，现在 {:?}", base[1], now[1]);
    for i in [0, 2, 4] {
        assert!(now[i].y > before[i].y + 12.0, "其余镜头仍按统一参数提亮：{:?} → {:?}", before[i], now[i]);
    }

    // 4. 单独调节里把自动校正强度设为 0：这个镜头保持原样
    let (now, _) = run(ToneAdjust::default(), vec![ShotAdjust { id: fourth, strength: Some(0.0), tone: ToneAdjust::default() }]).await;
    assert!((now[3].y - before[3].y).abs() < 3.0, "第 4 个镜头不处理：{:?} → {:?}", before[3], now[3]);
    assert!(dist_to_ref(&now[1], &a.looks().1.st) < dist_to_ref(&before[1], &a.looks().1.st) * 0.5, "第 2 个镜头仍按统一强度自动校正");
}

#[tokio::test]
async fn unified_adjustment_still_works_when_shots_cannot_be_analysed() {
    let (ff, d, caps) = need!(setup("colormatch-tone-short").await);
    // 太短（分析不了镜头）的视频：统一的手动调节照样应用
    let src = need!(gen(&ff, &d, "short.mp4", &["-f", "lavfi", "-i", "testsrc2=s=320x180:r=25:d=1"], &["-c:v", "libx264", "-pix_fmt", "yuv420p"]).await);
    let before = stats_at(&ff, &src, 0.5).await;
    let spec = NormSpec { size: "keep".into(), match_color: 1.0, match_tone: ToneAdjust { saturation: -1.0, ..Default::default() }, ..Default::default() };
    let (out, plan, _) = normalize(&ff, &src, &spec, &caps, &d).await;
    let now = stats_at(&ff, &out, 0.5).await;
    assert!(now.sat < before.sat * 0.3, "饱和度 -1 应该接近黑白：{:?} → {:?}（{:?}）", before, now, plan.notes);
}

#[tokio::test]
async fn a_graph_too_long_for_the_command_line_still_runs_from_a_file() {
    use super::colormatch::{ColorFix, ColorPlan, Curve};
    let (ff, d, caps) = need!(setup("long-graph").await);
    let src = need!(gen(&ff, &d, "g.mp4", &["-f", "lavfi", "-i", "testsrc2=s=320x180:r=25:d=2"], &["-c:v", "libx264", "-pix_fmt", "yuv420p"]).await);
    let facts = facts::read(&ff, &src).await.unwrap();
    // 250 个只管 8 毫秒的校正，合起来把整段视频提亮 30 级；滤镜图有四万多个字符
    let fixes: Vec<ColorFix> = (0..250)
        .map(|i| ColorFix {
            start_ms: Some(i * 8),
            end_ms: Some(i * 8 + 8),
            y: Some(Curve { pivot: 126.0, gain: 1.0, out: 156.0 }),
            u: None,
            v: None,
            except: vec![],
        })
        .collect();
    let an = Analysis { color: Some(ColorPlan { fixes, auto_shots: 250, own_shots: 0, unified: false, skipped: 0 }), ..Default::default() };
    let spec = NormSpec { size: "keep".into(), match_color: 1.0, ..Default::default() };
    let plan = build(&src, &spec, &facts, &an, &caps, &d.join("tmp")).unwrap();
    assert!(plan.args.iter().all(|a| a.len() < 1000), "滤镜图应该在文件里，不在命令行上");
    let out = d.join("out.mp4");
    let mut a = base_args();
    a.extend(plan.args.clone());
    a.extend(muxer_args(plan.ext, &out));
    run_ffmpeg_capture(&ff, &a, Duration::from_secs(120)).await.unwrap_or_else(|e| panic!("ffmpeg 失败：{e}\n参数：{a:?}"));
    let (before, now) = (stats_at(&ff, &src, 1.0).await, stats_at(&ff, &out, 1.0).await);
    assert!(now.y > before.y + 20.0, "{before:?} → {now:?}");
}
