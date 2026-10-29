//! 用真实的 ffmpeg 跑一遍：生成几段纯色素材，按工程生成参数并执行，再读回输出检查时长、颜色、声音。
//! 机器上没有 ffmpeg（或缺少需要的编码器 / 滤镜）时对应的测试自动跳过。
//! 完整版（libx264）和精简版（libopenh264 之类）的 ffmpeg 都能跑；用 `PATH` 指向哪个版本就测哪个。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::build::{build, EditPlan, Mode, Source};
use super::fonts;
use super::job::fit_to_sources;
use super::region::render_mask;
use super::spec::{AudioTrack, Clip, ClipKind, Project, TextItem, Transition, TRANSITIONS};
use crate::media_tools::muxer_args;
use crate::postprocess::{base_args, run_ffmpeg_capture};
use crate::vidcaps::{self, Caps};
use crate::vidnorm::facts;

pub(super) fn ffmpeg() -> Option<PathBuf> {
    std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
}

pub(super) fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "clearclip-edit-e2e-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(super) fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

pub(super) async fn gen(ff: &Path, d: &Path, name: &str, ins: &[&str], outs: &[&str]) -> Option<PathBuf> {
    let out = d.join(name);
    let mut a = strs(&["-hide_banner", "-loglevel", "error", "-y"]);
    a.extend(strs(ins));
    a.extend(strs(outs));
    a.push(out.to_string_lossy().into_owned());
    run_ffmpeg_capture(ff, &a, Duration::from_secs(120)).await.ok().map(|_| out)
}

struct Media {
    /// 红色，320×240，30 帧，3 秒，有声音（440 Hz）
    red: PathBuf,
    /// 绿色，640×360，25 帧，3 秒，有声音（880 Hz）
    green: PathBuf,
    /// 蓝色，320×240，30 帧，3 秒，没有声音
    blue: PathBuf,
    /// 黄色图片，200×200
    yellow: PathBuf,
    /// 220 Hz 的音乐，5 秒
    music: PathBuf,
}

async fn setup(name: &str) -> Option<(PathBuf, PathBuf, Caps, Media)> {
    let ff = ffmpeg()?;
    let caps = vidcaps::detect(&ff).await;
    if caps.encoders.h264.is_none() || !["xfade", "acrossfade", "concat", "amix"].iter().all(|f| caps.has_filter(f)) {
        return None;
    }
    let d = dir(name);
    // 素材用什么编码器生成不重要（精简版 ffmpeg 没有 libx264，用 mpeg4）；被测的是后面剪辑生成的参数
    let vcodec = if caps.encoders.h264 == Some("libx264") { "libx264" } else { "mpeg4" };
    let enc = ["-c:v", vcodec, "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"];
    let red = gen(&ff, &d, "red.mp4", &["-f", "lavfi", "-i", "color=c=red:s=320x240:r=30:d=3", "-f", "lavfi", "-i", "sine=f=440:d=3"], &enc).await?;
    let green = gen(&ff, &d, "green.mp4", &["-f", "lavfi", "-i", "color=c=green:s=640x360:r=25:d=3", "-f", "lavfi", "-i", "sine=f=880:d=3"], &enc).await?;
    let blue = gen(&ff, &d, "blue.mp4", &["-f", "lavfi", "-i", "color=c=blue:s=320x240:r=30:d=3"], &["-c:v", vcodec, "-pix_fmt", "yuv420p"]).await?;
    let yellow = gen(&ff, &d, "yellow.png", &["-f", "lavfi", "-i", "color=c=yellow:s=200x200"], &["-frames:v", "1"]).await?;
    let music = gen(&ff, &d, "music.wav", &["-f", "lavfi", "-i", "sine=f=220:d=5"], &["-c:a", "pcm_s16le"]).await?;
    Some((ff, d, caps, Media { red, green, blue, yellow, music }))
}

fn clip(path: &Path, from: u64, to: u64) -> Clip {
    Clip { id: 1, path: path.to_string_lossy().into_owned(), in_ms: from, out_ms: to, ..Default::default() }
}

fn image(path: &Path, ms: u64) -> Clip {
    Clip { id: 1, path: path.to_string_lossy().into_owned(), kind: ClipKind::Image, out_ms: ms, ..Default::default() }
}

fn with_t(mut c: Clip, kind: &str, ms: u64) -> Clip {
    c.transition = Some(Transition { kind: kind.into(), duration_ms: ms });
    c
}

/// 按任务的流程（读素材、核对、生成参数、运行）渲染一个工程。
pub(super) async fn render(ff: &Path, caps: &Caps, d: &Path, p: Project, mode: Mode) -> (PathBuf, EditPlan) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let mut p = p.checked().expect("工程校验");
    let mut src: HashMap<String, Source> = HashMap::new();
    for path in p.clips.iter().map(|c| c.path.clone()).chain(p.overlays.iter().map(|o| o.path.clone())).chain(p.audio.iter().map(|a| a.path.clone())) {
        if let std::collections::hash_map::Entry::Vacant(e) = src.entry(path) {
            let f = facts::read(ff, Path::new(e.key())).await.expect("facts");
            e.insert(Source::from_facts(&f));
        }
    }
    fit_to_sources(&mut p, &src).expect("fit");
    let font = fonts::default_font();
    let plan = build(&p, &src, caps, &d.join("tmp"), mode, font.as_deref()).expect("build");
    let out = d.join(format!("out-{}.{}", N.fetch_add(1, Ordering::SeqCst), plan.ext));
    for m in &plan.masks {
        render_mask(ff, m, || Ok(())).await.expect("遮罩");
    }
    let mut a = base_args();
    a.extend(plan.args.clone());
    a.extend(muxer_args(plan.ext, &out));
    run_ffmpeg_capture(ff, &a, Duration::from_secs(300)).await.unwrap_or_else(|e| panic!("ffmpeg 失败：{e}\n参数：{a:?}"));
    (out, plan)
}

/// 某一时刻整个画面的平均颜色。
async fn rgb_at(ff: &Path, file: &Path, t: f64) -> (u8, u8, u8) {
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
        "scale=1:1:flags=area",
        "-pix_fmt",
        "rgb24",
        "-f",
        "rawvideo",
        "-",
    ]);
    let o = run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap().0;
    assert_eq!(o.len(), 3, "{t} 秒处取不到画面");
    (o[0], o[1], o[2])
}

pub(super) fn is_red(c: (u8, u8, u8)) -> bool {
    c.0 > 180 && c.1 < 70 && c.2 < 70
}
pub(super) fn is_green(c: (u8, u8, u8)) -> bool {
    c.1 > 90 && c.0 < 70 && c.2 < 70
}
pub(super) fn is_yellow(c: (u8, u8, u8)) -> bool {
    c.0 > 180 && c.1 > 180 && c.2 < 80
}
pub(super) fn is_blue(c: (u8, u8, u8)) -> bool {
    c.2 > 150 && c.0 < 70 && c.1 < 70
}

pub(super) async fn facts_of(ff: &Path, f: &Path) -> facts::Facts {
    facts::read(ff, f).await.expect("facts")
}

pub(super) fn close(ms: Option<u64>, want: u64, tol: u64) -> bool {
    ms.is_some_and(|m| m.abs_diff(want) <= tol)
}

macro_rules! need {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return,
        }
    };
}

#[tokio::test]
async fn timeline_has_the_right_length_colours_and_sound() {
    let (ff, d, caps, m) = need!(setup("basic").await);
    // 红色取 0.5–2.0 秒（1.5 秒）→ 绿色取 1–2 秒 → 图片 1 秒（硬切），再接蓝色 1 秒（无声）：共 4.5 秒
    let p = Project { clips: vec![clip(&m.red, 500, 2000), clip(&m.green, 1000, 2000), image(&m.yellow, 1000), clip(&m.blue, 0, 1000)], ..Default::default() };
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(close(f.duration_ms, 4500, 150), "时长 {:?}", f.duration_ms);
    let v = f.video.as_ref().expect("有画面");
    assert_eq!((v.width, v.height), (320, 240), "跟随第一个片段的尺寸");
    assert!(f.audio.is_some(), "有的片段有声音，输出就有音轨");
    assert_eq!(plan.out_ms, 4500);
    assert!(is_red(rgb_at(&ff, &out, 0.7).await), "{:?}", rgb_at(&ff, &out, 0.7).await);
    assert!(is_green(rgb_at(&ff, &out, 2.0).await), "{:?}", rgb_at(&ff, &out, 2.0).await);
    assert!(is_yellow(rgb_at(&ff, &out, 3.0).await), "{:?}", rgb_at(&ff, &out, 3.0).await);
    assert!(is_blue(rgb_at(&ff, &out, 4.0).await), "{:?}", rgb_at(&ff, &out, 4.0).await);
    // 声音和画面一样长（相差不超过 100 毫秒）
    let a = a_duration(&ff, &out).await;
    assert!(a.abs_diff(f.duration_ms.unwrap()) <= 100, "声音 {a} 毫秒，画面 {:?}", f.duration_ms);
    let _ = std::fs::remove_dir_all(d);
}

/// 音轨的实际时长（毫秒）：把音频解码一遍数样本。
pub(super) async fn a_duration(ff: &Path, file: &Path) -> u64 {
    let a = strs(&["-hide_banner", "-i", &file.to_string_lossy(), "-vn", "-f", "null", "-"]);
    let log = match run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await {
        Ok((_, e)) => e,
        Err(e) => panic!("{e}"),
    };
    // 最后一行进度里的 time=00:00:04.50
    let t = log.rsplit("time=").next().unwrap_or("").split_whitespace().next().unwrap_or("").to_string();
    let p: Vec<f64> = t.split(':').filter_map(|x| x.parse().ok()).collect();
    assert_eq!(p.len(), 3, "读不出音频时长：{log}");
    ((p[0] * 3600.0 + p[1] * 60.0 + p[2]) * 1000.0) as u64
}

#[tokio::test]
async fn every_transition_kind_renders_with_the_overlap_length() {
    let (ff, d, caps, m) = need!(setup("transitions").await);
    for kind in TRANSITIONS {
        // 红 3 秒 + 绿 3 秒，1 秒转场：总长 5 秒；转场之前是红色，之后是绿色
        let p = Project { clips: vec![clip(&m.red, 0, 3000), with_t(clip(&m.green, 0, 3000), kind, 1000)], ..Default::default() };
        let (out, _) = render(&ff, &caps, &d, p, Mode::Export).await;
        let f = facts_of(&ff, &out).await;
        assert!(close(f.duration_ms, 5000, 150), "{kind}：时长 {:?}", f.duration_ms);
        assert_eq!(f.video.as_ref().map(|v| (v.width, v.height)), Some((320, 240)), "{kind}");
        assert!(f.audio.is_some(), "{kind}");
        let (before, after) = (rgb_at(&ff, &out, 1.0).await, rgb_at(&ff, &out, 4.3).await);
        assert!(is_red(before), "{kind}：转场前应是红色，实际 {before:?}");
        assert!(is_green(after), "{kind}：转场后应是绿色，实际 {after:?}");
        // 声音用 acrossfade 叠在一起，长度也是 5 秒
        let a = a_duration(&ff, &out).await;
        assert!(a.abs_diff(5000) <= 120, "{kind}：声音 {a} 毫秒");
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn a_transition_really_blends_the_two_pictures() {
    let (ff, d, caps, m) = need!(setup("blend").await);
    let p = Project { clips: vec![clip(&m.red, 0, 3000), with_t(clip(&m.green, 0, 3000), "fade", 2000)], ..Default::default() };
    let (out, _) = render(&ff, &caps, &d, p, Mode::Export).await;
    // 转场在 1–3 秒，中点 2 秒处红绿各一半
    let mid = rgb_at(&ff, &out, 2.0).await;
    assert!(mid.0 > 60 && mid.1 > 30 && mid.0 < 200, "中点应该是红绿混合，实际 {mid:?}");
    assert!(close(facts_of(&ff, &out).await.duration_ms, 4000, 150));
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn speed_changes_length_and_keeps_sound_in_step() {
    let (ff, d, caps, m) = need!(setup("speed").await);
    let mut fast = clip(&m.red, 0, 3000);
    fast.speed = 2.0;
    let mut slow = clip(&m.green, 0, 1000);
    slow.speed = 0.5;
    let (out, plan) = render(&ff, &caps, &d, Project { clips: vec![fast, slow], ..Default::default() }, Mode::Export).await;
    // 3 秒 ÷ 2 = 1.5 秒；1 秒 ÷ 0.5 = 2 秒
    assert_eq!(plan.out_ms, 3500);
    assert!(close(facts_of(&ff, &out).await.duration_ms, 3500, 150));
    assert!(a_duration(&ff, &out).await.abs_diff(3500) <= 120);
    assert!(is_red(rgb_at(&ff, &out, 1.0).await) && is_green(rgb_at(&ff, &out, 2.5).await));
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn images_only_projects_have_no_audio_and_muting_silences_a_clip() {
    let (ff, d, caps, m) = need!(setup("silent").await);
    let (out, _) = render(&ff, &caps, &d, Project { clips: vec![image(&m.yellow, 1500), image(&m.yellow, 1000)], ..Default::default() }, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(f.audio.is_none(), "只有图片：没有音轨");
    assert!(close(f.duration_ms, 2500, 150), "{:?}", f.duration_ms);
    assert_eq!(f.video.as_ref().map(|v| (v.width, v.height)), Some((200, 200)));
    let mut c = clip(&m.red, 0, 2000);
    c.mute = true;
    let (out, _) = render(&ff, &caps, &d, Project { clips: vec![c], ..Default::default() }, Mode::Export).await;
    assert!(facts_of(&ff, &out).await.audio.is_none(), "唯一的片段静音、又没有配乐：没有音轨");
    // 静音的片段和有声音的片段拼在一起：输出有音轨，静音的那段是无声的
    let mut c = clip(&m.red, 0, 1000);
    c.mute = true;
    let (out, _) = render(&ff, &caps, &d, Project { clips: vec![c, clip(&m.green, 0, 1000)], ..Default::default() }, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(f.audio.is_some() && close(f.duration_ms, 2000, 120));
    assert!(mean_volume(&ff, &out, 0.1, 0.8).await < -60.0, "静音的那一秒应该没有声音");
    assert!(mean_volume(&ff, &out, 1.2, 0.7).await > -30.0, "后面的片段有声音");
    let _ = std::fs::remove_dir_all(d);
}

/// 一段声音的平均音量（dB）。
async fn mean_volume(ff: &Path, file: &Path, from: f64, len: f64) -> f64 {
    let a = strs(&[
        "-hide_banner",
        "-ss",
        &format!("{from:.3}"),
        "-t",
        &format!("{len:.3}"),
        "-i",
        &file.to_string_lossy(),
        "-vn",
        "-af",
        "volumedetect",
        "-f",
        "null",
        "-",
    ]);
    let log = run_ffmpeg_capture(ff, &a, Duration::from_secs(60)).await.unwrap().1;
    let after = log.split("mean_volume:").nth(1).unwrap_or_else(|| panic!("没有音量信息：{log}"));
    after.split_whitespace().next().unwrap().parse().unwrap()
}

#[tokio::test]
async fn volume_and_fades_shape_the_sound_and_picture() {
    let (ff, d, caps, m) = need!(setup("fades").await);
    let mut c = clip(&m.red, 0, 3000);
    c.volume = 0.25;
    c.fade_in_ms = 1000;
    c.fade_out_ms = 1000;
    let loud = clip(&m.red, 0, 3000);
    let (quiet_out, _) = render(&ff, &caps, &d, Project { clips: vec![c], ..Default::default() }, Mode::Export).await;
    let (loud_out, _) = render(&ff, &caps, &d, Project { clips: vec![loud], ..Default::default() }, Mode::Export).await;
    let (q, l) = (mean_volume(&ff, &quiet_out, 1.0, 1.0).await, mean_volume(&ff, &loud_out, 1.0, 1.0).await);
    assert!((l - q - 12.0).abs() < 1.5, "音量 0.25 约低 12 dB：{q} / {l}");
    // 淡入：开头第一帧接近黑，一秒后恢复；淡出：结尾接近黑
    let (start, mid, end) = (rgb_at(&ff, &quiet_out, 0.0).await, rgb_at(&ff, &quiet_out, 1.5).await, rgb_at(&ff, &quiet_out, 2.95).await);
    assert!(start.0 < 60, "开头应接近黑：{start:?}");
    assert!(is_red(mid), "{mid:?}");
    assert!(end.0 < 80, "结尾应接近黑：{end:?}");
    assert!(mean_volume(&ff, &quiet_out, 0.0, 0.2).await < q - 6.0, "声音也淡入");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn rotate_flip_and_tone_change_the_picture() {
    let (ff, d, caps, m) = need!(setup("looks").await);
    let mut c = clip(&m.red, 0, 1500);
    c.rotate = 90;
    let (out, _) = render(&ff, &caps, &d, Project { clips: vec![c], ..Default::default() }, Mode::Export).await;
    let v = facts_of(&ff, &out).await.video.unwrap();
    assert_eq!((v.width, v.height), (240, 320), "旋转 90° 后输出变成竖屏");
    assert!(is_red(rgb_at(&ff, &out, 0.5).await));
    // 饱和度 0：红色变灰；亮度 +：变亮
    let mut gray = clip(&m.red, 0, 1000);
    gray.saturation = 0.0;
    let (out, _) = render(&ff, &caps, &d, Project { clips: vec![gray], ..Default::default() }, Mode::Export).await;
    let c = rgb_at(&ff, &out, 0.5).await;
    assert!(c.0.abs_diff(c.1) < 30 && c.1.abs_diff(c.2) < 30, "饱和度 0 应该是灰色：{c:?}");
    let mut bright = clip(&m.blue, 0, 1000);
    bright.brightness = 0.5;
    let (out_b, _) = render(&ff, &caps, &d, Project { clips: vec![bright], ..Default::default() }, Mode::Export).await;
    let (out_n, _) = render(&ff, &caps, &d, Project { clips: vec![clip(&m.blue, 0, 1000)], ..Default::default() }, Mode::Export).await;
    let (b, n) = (rgb_at(&ff, &out_b, 0.5).await, rgb_at(&ff, &out_n, 0.5).await);
    assert!(u32::from(b.0) + u32::from(b.1) > u32::from(n.0) + u32::from(n.1) + 40, "亮度 +0.5 应该更亮：{b:?} / {n:?}");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn fit_modes_fill_a_different_shaped_frame() {
    let (ff, d, caps, m) = need!(setup("fit").await);
    // 绿色 640×360（16:9）放进 360×640 的竖屏
    let mut res = vec![];
    for fit in ["contain", "cover", "blur"] {
        let mut p = Project { clips: vec![clip(&m.green, 0, 1500)], ..Default::default() };
        p.out.width = 360;
        p.out.height = 640;
        p.out.fit = fit.into();
        let (out, _) = render(&ff, &caps, &d, p, Mode::Export).await;
        let v = facts_of(&ff, &out).await.video.unwrap();
        assert_eq!((v.width, v.height), (360, 640), "{fit}");
        // 画面最上面一条
        let a = strs(&[
            "-hide_banner",
            "-loglevel",
            "error",
            "-ss",
            "0.5",
            "-i",
            &out.to_string_lossy(),
            "-frames:v",
            "1",
            "-vf",
            "crop=360:40:0:0,scale=1:1:flags=area",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ]);
        let o = run_ffmpeg_capture(&ff, &a, Duration::from_secs(30)).await.unwrap().0;
        res.push((o[0], o[1], o[2]));
    }
    assert!(res[0].1 < 30, "contain：上下是黑边 {:?}", res[0]);
    assert!(is_green(res[1]), "cover：裁切填满，上方是绿色 {:?}", res[1]);
    assert!(res[2].1 > 60 && res[2].0 < 60, "blur：上方是模糊的绿色背景 {:?}", res[2]);
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn text_is_drawn_for_its_time_window_only() {
    let (ff, d, caps, m) = need!(setup("text").await);
    if !caps.has_filter("drawtext") || fonts::default_font().is_none() {
        eprintln!("跳过：没有 drawtext 或找不到字体");
        let _ = std::fs::remove_dir_all(d);
        return;
    }
    // 黑底（蓝色素材压暗不如直接用黑场：用亮度 -1 调黑），白字
    let mut black = clip(&m.blue, 0, 3000);
    black.brightness = -1.0;
    black.saturation = 0.0;
    black.contrast = 0.0;
    let mut p = Project { clips: vec![black], ..Default::default() };
    p.texts = vec![TextItem {
        text: "Hello 你好 100% it's: ok".into(),
        start_ms: 1000,
        end_ms: 2000,
        x: 0.5,
        y: 0.5,
        size: 20.0,
        outline: false,
        ..Default::default()
    }];
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let peak = |t: f64| {
        let ff = ff.clone();
        let out = out.clone();
        async move {
            let a = strs(&[
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                &format!("{t:.3}"),
                "-i",
                &out.to_string_lossy(),
                "-frames:v",
                "1",
                "-vf",
                "scale=64:48:flags=area,format=gray",
                "-f",
                "rawvideo",
                "-",
            ]);
            let o = run_ffmpeg_capture(&ff, &a, Duration::from_secs(30)).await.unwrap().0;
            *o.iter().max().unwrap()
        }
    };
    assert!(peak(0.3).await < 40, "文字出现之前是黑的");
    assert!(peak(1.5).await > 90, "文字出现期间有亮的像素");
    assert!(peak(2.6).await < 40, "文字消失之后又是黑的");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn music_is_mixed_in_delayed_looped_and_ducked() {
    let (ff, d, caps, m) = need!(setup("music").await);
    // 画面 4 秒，原声（880 Hz）静音，只听配乐：从 1 秒开始、音量 0.5
    let mut v = clip(&m.green, 0, 3000);
    v.mute = true;
    let v2 = clip(&m.blue, 0, 1000);
    let mut p = Project { clips: vec![v, v2], ..Default::default() };
    p.audio = vec![AudioTrack { path: m.music.to_string_lossy().into_owned(), start_ms: 1000, volume: 0.5, ..Default::default() }];
    let (out, _) = render(&ff, &caps, &d, p.clone(), Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(close(f.duration_ms, 4000, 150), "{:?}", f.duration_ms);
    assert!(a_duration(&ff, &out).await.abs_diff(4000) <= 150, "配乐不会把输出拉长");
    assert!(mean_volume(&ff, &out, 0.1, 0.7).await < -60.0, "配乐开始之前是静音");
    let with = mean_volume(&ff, &out, 1.5, 1.0).await;
    // 单声道的 220 Hz 正弦波（-21 dB）× 0.5 = -27 dB，单声道摆到左右两个声道各降 3 dB，约 -30 dB
    assert!(with > -35.0 && with < -22.0, "配乐 0.5 倍：{with}");

    // 配乐只有 5 秒、视频 8 秒：循环补满；不循环时 5 秒之后没有声音
    let long = |looped: bool| {
        let mut p = Project { clips: vec![clip(&m.red, 0, 3000), clip(&m.green, 0, 3000), clip(&m.blue, 0, 2000)], ..Default::default() };
        for c in &mut p.clips {
            c.mute = true;
        }
        p.audio = vec![AudioTrack { path: m.music.to_string_lossy().into_owned(), looped, ..Default::default() }];
        p
    };
    let (once, _) = render(&ff, &caps, &d, long(false), Mode::Export).await;
    let (looped, _) = render(&ff, &caps, &d, long(true), Mode::Export).await;
    assert!(mean_volume(&ff, &once, 6.0, 1.5).await < -60.0, "不循环：5 秒之后没有声音");
    assert!(mean_volume(&ff, &looped, 6.0, 1.5).await > -35.0, "循环：一直有声音");
    assert!(close(facts_of(&ff, &looped).await.duration_ms, 8000, 200));

    // 自动压低：原声（红色片段，440 Hz）响着的时候配乐被压低
    if caps.has_filter("sidechaincompress") {
        let build = |duck: bool| {
            let mut p = Project { clips: vec![clip(&m.red, 0, 3000)], ..Default::default() };
            p.audio = vec![AudioTrack { path: m.music.to_string_lossy().into_owned(), duck, ..Default::default() }];
            p
        };
        let (plain, _) = render(&ff, &caps, &d, build(false), Mode::Export).await;
        let (ducked, _) = render(&ff, &caps, &d, build(true), Mode::Export).await;
        let (a, b) = (mean_volume(&ff, &plain, 1.0, 1.5).await, mean_volume(&ff, &ducked, 1.0, 1.5).await);
        assert!(b < a - 2.0, "压低之后应该更轻：{a} → {b}");
        assert!(close(facts_of(&ff, &ducked).await.duration_ms, 3000, 150));
    }
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn preview_is_small_and_still_the_same_length() {
    let (ff, d, caps, _) = need!(setup("preview").await);
    let vcodec = if caps.encoders.h264 == Some("libx264") { "libx264" } else { "mpeg4" };
    let Some(big) = gen(
        &ff,
        &d,
        "big.mp4",
        &["-f", "lavfi", "-i", "testsrc2=s=1280x720:r=60:d=2", "-f", "lavfi", "-i", "sine=f=330:d=2"],
        &["-c:v", vcodec, "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"],
    )
    .await
    else {
        return;
    };
    let p = Project { clips: vec![clip(&big, 0, 2000), with_t(clip(&big, 0, 2000), "slideleft", 500)], ..Default::default() };
    let (out, plan) = render(&ff, &caps, &d, p.clone(), Mode::Preview).await;
    let f = facts_of(&ff, &out).await;
    let v = f.video.unwrap();
    assert_eq!((v.width, v.height), (640, 360));
    assert!(v.fps.is_some_and(|x| (x - 24.0).abs() < 0.5), "{:?}", v.fps);
    assert!(close(f.duration_ms, 3500, 150));
    assert_eq!(plan.out_ms, 3500);
    let (full, _) = render(&ff, &caps, &d, p, Mode::Export).await;
    let v = facts_of(&ff, &full).await.video.unwrap();
    assert_eq!((v.width, v.height), (1280, 720));
    assert!(v.fps.is_some_and(|x| (x - 60.0).abs() < 0.5), "{:?}", v.fps);
    assert!(std::fs::metadata(&out).unwrap().len() < std::fs::metadata(&full).unwrap().len());
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn many_clips_use_the_graph_file_and_stay_in_order() {
    let (ff, d, caps, m) = need!(setup("many").await);
    // 同一个红色素材 + 绿色素材交替取 0.3 秒，共 40 段：滤镜图超过 12000 个字符，走文件
    let clips: Vec<Clip> = (0..40).map(|i| if i % 2 == 0 { clip(&m.red, 0, 300) } else { clip(&m.green, 100, 400) }).collect();
    let (out, plan) = render(&ff, &caps, &d, Project { clips, ..Default::default() }, Mode::Export).await;
    assert!(d.join("tmp").join("filtergraph.txt").exists(), "{:?}", plan.args.iter().take(4).collect::<Vec<_>>());
    assert!(close(facts_of(&ff, &out).await.duration_ms, 12_000, 300));
    assert!(is_red(rgb_at(&ff, &out, 0.15).await));
    assert!(is_green(rgb_at(&ff, &out, 0.45).await));
    assert!(is_red(rgb_at(&ff, &out, 11.1).await) || is_green(rgb_at(&ff, &out, 11.1).await));
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn hevc_and_mkv_outputs_when_the_encoder_exists() {
    let (ff, d, caps, m) = need!(setup("codec").await);
    if caps.hevc.is_none() {
        let _ = std::fs::remove_dir_all(d);
        return;
    }
    let mut p = Project { clips: vec![clip(&m.red, 0, 1500)], ..Default::default() };
    p.out.codec = "hevc".into();
    p.out.format = "mkv".into();
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    assert_eq!(plan.ext, "mkv");
    assert_eq!(facts_of(&ff, &out).await.video.unwrap().codec, "hevc");
    let _ = std::fs::remove_dir_all(d);
}
