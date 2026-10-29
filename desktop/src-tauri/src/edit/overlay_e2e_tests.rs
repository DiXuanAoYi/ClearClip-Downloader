//! 叠加轨的端到端测试：用真实的 ffmpeg 把素材叠到主轨上，再读回输出，核对位置、时间、层叠顺序、透明度、旋转、淡入、GIF 循环和声音。
//! 机器上没有 ffmpeg（或缺少需要的滤镜 / 编码器）时自动跳过；`PATH` 指向哪个版本的 ffmpeg 就测哪个（完整版和精简版都跑过）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::build::Mode;
use super::e2e_tests::{close, dir, facts_of, ffmpeg, gen, is_blue, is_green, is_red, is_yellow, render, strs};
use super::spec::{Clip, ClipKind, Overlay, Project};
use crate::postprocess::run_ffmpeg_capture;
use crate::vidcaps::{self, Caps};

struct Media {
    /// 绿色，640×360，25 帧，6 秒，有声音（880 Hz）——主轨
    main: PathBuf,
    /// 蓝色，640×360，25 帧，4 秒，没有声音
    silent: PathBuf,
    /// 红色，320×240，30 帧，3 秒，有声音（440 Hz）
    red: PathBuf,
    /// 蓝色，320×240，30 帧，3 秒，没有声音
    blue: PathBuf,
    /// 黄色图片，200×200
    yellow: PathBuf,
    /// 动图：红 0.5 秒 → 蓝 0.5 秒，80×80，循环播放
    gif: PathBuf,
    /// 绿色，1280×720，25 帧，4 秒（预览缩小用）
    big: PathBuf,
}

async fn setup(name: &str) -> Option<(PathBuf, PathBuf, Caps, Media)> {
    let ff = ffmpeg()?;
    let caps = vidcaps::detect(&ff).await;
    if caps.encoders.h264.is_none() || !["overlay", "lutyuv", "rotate", "amix", "concat"].iter().all(|f| caps.has_filter(f)) {
        return None;
    }
    let d = dir(name);
    let vcodec = if caps.encoders.h264 == Some("libx264") { "libx264" } else { "mpeg4" };
    let enc = ["-c:v", vcodec, "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"];
    let main = gen(&ff, &d, "main.mp4", &["-f", "lavfi", "-i", "color=c=green:s=640x360:r=25:d=6", "-f", "lavfi", "-i", "sine=f=880:d=6"], &enc).await?;
    let silent = gen(&ff, &d, "silent.mp4", &["-f", "lavfi", "-i", "color=c=blue:s=640x360:r=25:d=4"], &["-c:v", vcodec, "-pix_fmt", "yuv420p"]).await?;
    let red = gen(&ff, &d, "red.mp4", &["-f", "lavfi", "-i", "color=c=red:s=320x240:r=30:d=3", "-f", "lavfi", "-i", "sine=f=440:d=3"], &enc).await?;
    let blue = gen(&ff, &d, "blue.mp4", &["-f", "lavfi", "-i", "color=c=blue:s=320x240:r=30:d=3"], &["-c:v", vcodec, "-pix_fmt", "yuv420p"]).await?;
    let yellow = gen(&ff, &d, "yellow.png", &["-f", "lavfi", "-i", "color=c=yellow:s=200x200"], &["-frames:v", "1"]).await?;
    let gif = gen(
        &ff,
        &d,
        "loop.gif",
        &["-f", "lavfi", "-i", "color=c=red:s=80x80:r=10:d=0.5", "-f", "lavfi", "-i", "color=c=blue:s=80x80:r=10:d=0.5"],
        &["-filter_complex", "[0][1]concat=n=2:v=1:a=0", "-loop", "0"],
    )
    .await?;
    let big = gen(&ff, &d, "big.mp4", &["-f", "lavfi", "-i", "color=c=green:s=1280x720:r=25:d=4"], &["-c:v", vcodec, "-pix_fmt", "yuv420p"]).await?;
    Some((ff, d, caps, Media { main, silent, red, blue, yellow, gif, big }))
}

macro_rules! need {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return,
        }
    };
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn main_clip(path: &Path, ms: u64) -> Clip {
    Clip { id: 1, path: s(path), out_ms: ms, ..Default::default() }
}

/// 一个视频叠加素材：整段取、放在 (x, y)、宽度占画面的 scale。
fn ov(path: &Path, start_ms: u64, ms: u64, x: f64, y: f64, scale: f64) -> Overlay {
    Overlay { id: 1, path: s(path), out_ms: ms, start_ms, x, y, scale, ..Default::default() }
}

fn ov_image(path: &Path, start_ms: u64, ms: u64, x: f64, y: f64, scale: f64) -> Overlay {
    Overlay { kind: ClipKind::Image, ..ov(path, start_ms, ms, x, y, scale) }
}

fn project(main: Clip, overlays: Vec<Overlay>) -> Project {
    Project { clips: vec![main], overlays, ..Default::default() }
}

/// 某一时刻、某个位置（像素）附近的颜色。
async fn px_at(ff: &Path, file: &Path, t: f64, x: u32, y: u32) -> (u8, u8, u8) {
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
        &format!("crop=4:4:{}:{},scale=1:1:flags=area", x.saturating_sub(2), y.saturating_sub(2)),
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

/// 一段声音的平均音量（dB）；静音返回 -91。
async fn mean_db(ff: &Path, file: &Path, from: f64, len: f64) -> f64 {
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
    log.split("mean_volume:").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse().ok()).unwrap_or(-91.0)
}

#[tokio::test]
async fn an_overlay_sits_at_its_position_and_time() {
    let (ff, d, caps, m) = need!(setup("pos").await);
    // 红色 320×240 缩成画面宽度的 25%（160×120），中心在 (75%, 25%) → 像素 (480, 90)；从 1 秒起显示 3 秒
    let p = project(main_clip(&m.main, 6000), vec![ov(&m.red, 1000, 3000, 0.75, 0.25, 0.25)]);
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(close(f.duration_ms, 6000, 150), "叠加素材不改变成片长度：{:?}", f.duration_ms);
    assert_eq!(f.video.as_ref().map(|v| (v.width, v.height)), Some((640, 360)));
    assert_eq!(plan.out_ms, 6000);
    let before = px_at(&ff, &out, 0.5, 480, 90).await;
    assert!(is_green(before), "还没到起点：{before:?}");
    let inside = px_at(&ff, &out, 2.0, 480, 90).await;
    assert!(is_red(inside), "叠加素材中心：{inside:?}");
    // 框的四边：480±80、90±60；外侧 10 像素还是主轨画面，内侧 10 像素是叠加素材
    for (x, y, red) in
        [(405, 90, true), (390, 90, false), (555, 90, true), (570, 90, false), (480, 35, true), (480, 20, false), (480, 145, true), (480, 160, false)]
    {
        let c = px_at(&ff, &out, 2.0, x, y).await;
        assert!(if red { is_red(c) } else { is_green(c) }, "({x},{y}) 应是{}：{c:?}", if red { "红色（叠加素材）" } else { "绿色（主轨）" });
    }
    let other = px_at(&ff, &out, 2.0, 100, 300).await;
    assert!(is_green(other), "别处不受影响：{other:?}");
    let after = px_at(&ff, &out, 4.6, 480, 90).await;
    assert!(is_green(after), "结束之后：{after:?}");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn a_higher_track_covers_a_lower_one_and_later_starts_cover_earlier_ones() {
    let (ff, d, caps, m) = need!(setup("order").await);
    // 红在 1 号轨、蓝在 2 号轨，位置完全重叠：蓝在上
    let mut red = ov(&m.red, 0, 3000, 0.5, 0.5, 0.5);
    let mut blue = ov(&m.blue, 0, 3000, 0.5, 0.5, 0.5);
    red.track = 1;
    blue.track = 2;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 4000), vec![red.clone(), blue.clone()]), Mode::Export).await;
    let (x, y, t) = (320u32, 180u32, 1.0);
    assert!(is_blue(px_at(&ff, &out, t, x, y).await), "2 号轨在上");
    // 换一下轨道号，再换一下列表里的顺序（顺序不影响层叠，轨道号才算）
    red.track = 3;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 4000), vec![blue.clone(), red.clone()]), Mode::Export).await;
    assert!(is_red(px_at(&ff, &out, t, x, y).await), "3 号轨在上，和列表顺序无关");
    // 同一条轨道：后开始的盖在先开始的上面
    red.track = 1;
    blue.track = 1;
    red.start_ms = 0;
    blue.start_ms = 500;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 4000), vec![blue.clone(), red.clone()]), Mode::Export).await;
    assert!(is_red(px_at(&ff, &out, 0.3, x, y).await), "蓝还没开始");
    assert!(is_blue(px_at(&ff, &out, 1.0, x, y).await), "后开始的在上");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn opacity_rotation_and_fades_work_on_overlays() {
    let (ff, d, caps, m) = need!(setup("look").await);
    let mid = (320u32, 180u32);
    // 半透明：黄色（255,255,0）以 50% 盖在绿色（0,128,0）上 ≈ (128,191,0)
    let mut half = ov_image(&m.yellow, 0, 3000, 0.5, 0.5, 0.5);
    half.opacity = 0.5;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 3000), vec![half]), Mode::Export).await;
    let c = px_at(&ff, &out, 1.0, mid.0, mid.1).await;
    assert!((100..160).contains(&c.0) && (165..215).contains(&c.1) && c.2 < 60, "半透明的混色：{c:?}");

    // 旋转 45 度：中心还是黄色，原来方块的角落（离中心 0.45 个边长）露出主轨的绿色
    let corner = (mid.0 + 144, mid.1 + 144);
    let plain = ov_image(&m.yellow, 0, 3000, 0.5, 0.5, 0.5);
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 3000), vec![plain.clone()]), Mode::Export).await;
    assert!(is_yellow(px_at(&ff, &out, 1.0, corner.0, corner.1).await), "没旋转时角落是黄色");
    let mut turned = plain;
    turned.rotate = 45.0;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 3000), vec![turned]), Mode::Export).await;
    assert!(is_yellow(px_at(&ff, &out, 1.0, mid.0, mid.1).await), "旋转后中心不变");
    let c = px_at(&ff, &out, 1.0, corner.0, corner.1).await;
    assert!(is_green(c), "旋转 45 度后角落露出主轨：{c:?}");

    // 淡入 1 秒：刚开始几乎看不到，淡入结束后是完整的红色
    let mut fade = ov(&m.red, 0, 3000, 0.5, 0.5, 0.5);
    fade.fade_in_ms = 1000;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 3000), vec![fade]), Mode::Export).await;
    let early = px_at(&ff, &out, 0.1, mid.0, mid.1).await;
    assert!(early.0 < 90 && early.1 > 80, "淡入刚开始应该还是主轨的绿色为主：{early:?}");
    assert!(is_red(px_at(&ff, &out, 1.6, mid.0, mid.1).await), "淡入结束后完整显示");

    // 旋转 90 度：320×240 的红色变成竖的（显示宽度仍按 scale 算：160 宽、213 高），中心上下 ±100 在内、左右 ±100 在外
    let mut quarter = ov(&m.red, 0, 3000, 0.5, 0.5, 0.25);
    quarter.rotate = 90.0;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 3000), vec![quarter]), Mode::Export).await;
    assert!(is_red(px_at(&ff, &out, 1.0, 320, 180 + 95).await), "竖直方向在框内");
    assert!(is_green(px_at(&ff, &out, 1.0, 320 + 100, 180).await), "水平方向在框外");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn an_animated_gif_loops_to_fill_the_time_it_is_given() {
    let (ff, d, caps, m) = need!(setup("gif").await);
    let facts = facts_of(&ff, &m.gif).await;
    let gif_ms = facts.duration_ms.expect("GIF 有时长");
    assert!((800..=1300).contains(&gif_ms), "测试素材应该约 1 秒：{gif_ms}");
    // 0.5 秒红 → 0.5 秒蓝，从主轨 0.5 秒处开始，循环到 3.5 秒（总共 3 秒 = 3 遍）
    let mut g = ov(&m.gif, 500, 3000, 0.5, 0.5, 0.2);
    g.looped = true;
    let (out, plan) = render(&ff, &caps, &d, project(main_clip(&m.main, 5000), vec![g]), Mode::Export).await;
    assert!(plan.args.windows(2).any(|w| w == ["-stream_loop", "-1"]), "{:?}", plan.args);
    let (x, y) = (320, 180);
    assert!(is_green(px_at(&ff, &out, 0.3, x, y).await), "开始之前");
    // 第一遍 0.5–1.0 红、1.0–1.5 蓝；第二遍 1.5–2.0 红、2.0–2.5 蓝；第三遍 2.5–3.0 红、3.0–3.5 蓝
    for (t, red) in [(0.75, true), (1.25, false), (1.75, true), (2.25, false), (2.75, true), (3.25, false)] {
        let c = px_at(&ff, &out, t, x, y).await;
        assert!(if red { is_red(c) } else { is_blue(c) }, "{t} 秒应该是{}：{c:?}", if red { "红" } else { "蓝" });
    }
    assert!(is_green(px_at(&ff, &out, 3.8, x, y).await), "3.5 秒之后结束");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn overlay_sound_is_mixed_in_at_its_start_time() {
    let (ff, d, caps, m) = need!(setup("sound").await);
    // 主轨没有声音，叠加的红色素材（440 Hz）从 2 秒开始
    let p = project(main_clip(&m.silent, 4000), vec![ov(&m.red, 2000, 2000, 0.5, 0.5, 0.3)]);
    let (out, _) = render(&ff, &caps, &d, p, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(f.audio.is_some(), "叠加素材有声音，成片就有音轨");
    assert!(close(f.duration_ms, 4000, 150), "{:?}", f.duration_ms);
    let quiet = mean_db(&ff, &out, 0.2, 1.4).await;
    let loud = mean_db(&ff, &out, 2.3, 1.4).await;
    assert!(quiet < -60.0, "2 秒之前应该是静音：{quiet} dB");
    assert!(loud > -35.0, "2 秒之后能听到叠加素材：{loud} dB");

    // 静音的叠加素材不出声；主轨有声音时叠加素材的声音是叠加上去的（比单独主轨更响）
    let mut muted = ov(&m.red, 2000, 2000, 0.5, 0.5, 0.3);
    muted.mute = true;
    let (out, _) = render(&ff, &caps, &d, project(main_clip(&m.silent, 4000), vec![muted]), Mode::Export).await;
    assert!(facts_of(&ff, &out).await.audio.is_none(), "静音的叠加素材 + 没有声音的主轨 = 没有音轨");
    let alone = {
        let (o, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 4000), vec![]), Mode::Export).await;
        mean_db(&ff, &o, 2.3, 1.4).await
    };
    let mixed = {
        let (o, _) = render(&ff, &caps, &d, project(main_clip(&m.main, 4000), vec![ov(&m.red, 2000, 2000, 0.5, 0.5, 0.3)]), Mode::Export).await;
        mean_db(&ff, &o, 2.3, 1.4).await
    };
    assert!(mixed > alone + 1.0, "两路声音叠在一起更响：单独 {alone} dB，混合 {mixed} dB");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn overlays_that_run_past_the_end_are_cut_and_late_ones_are_dropped() {
    let (ff, d, caps, m) = need!(setup("end").await);
    let p = project(
        main_clip(&m.silent, 4000),
        vec![
            // 3–6 秒，主轨只有 4 秒：1 秒会出现
            ov(&m.red, 3000, 3000, 0.5, 0.5, 0.5),
            // 从 5 秒开始：根本不会出现
            ov(&m.blue, 5000, 3000, 0.5, 0.5, 0.5),
        ],
    );
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(close(f.duration_ms, 4000, 150), "成片长度由主轨决定：{:?}", f.duration_ms);
    assert!(plan.warnings.iter().any(|w| w.contains("第 2 个叠加素材") && w.contains("不会出现")), "{:?}", plan.warnings);
    assert!(plan.notes.iter().any(|n| n.contains("第 1 个叠加素材超出成片结尾")), "{:?}", plan.notes);
    assert!(is_red(px_at(&ff, &out, 3.5, 320, 180).await));
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn previews_scale_overlays_with_the_frame() {
    let (ff, d, caps, m) = need!(setup("prev").await);
    // 1280×720 的主轨，预览缩到 640×360；红色叠加素材宽度 25%、中心 (75%, 25%)：预览里是 (480, 90)
    let p = project(main_clip(&m.big, 3000), vec![ov(&m.red, 0, 3000, 0.75, 0.25, 0.25)]);
    let (out, plan) = render(&ff, &caps, &d, p.clone(), Mode::Preview).await;
    assert_eq!((plan.width, plan.height), (640, 360));
    assert!(is_red(px_at(&ff, &out, 1.0, 480, 90).await), "预览");
    assert!(is_green(px_at(&ff, &out, 1.0, 100, 300).await));
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    assert_eq!((plan.width, plan.height), (1280, 720));
    assert!(is_red(px_at(&ff, &out, 1.0, 960, 180).await), "导出");
    let _ = std::fs::remove_dir_all(d);
}

#[tokio::test]
async fn overlays_work_with_transitions_texts_and_speed() {
    let (ff, d, caps, m) = need!(setup("combo").await);
    use super::spec::Transition;
    // 主轨：绿 3 秒 → （1 秒淡入淡出）蓝 3 秒；叠加素材 2 倍速的红色（3 秒素材 → 1.5 秒）放在 2–3.5 秒
    let mut blue = Clip { id: 2, path: s(&m.blue), out_ms: 3000, ..Default::default() };
    blue.transition = Some(Transition { kind: "fade".into(), duration_ms: 1000 });
    let green = Clip { id: 1, path: s(&m.main), out_ms: 3000, ..Default::default() };
    let mut fast = ov(&m.red, 2000, 3000, 0.5, 0.5, 0.2);
    fast.speed = 2.0;
    let p = Project { clips: vec![green, blue], overlays: vec![fast], ..Default::default() };
    let (out, plan) = render(&ff, &caps, &d, p, Mode::Export).await;
    let f = facts_of(&ff, &out).await;
    assert!(close(f.duration_ms, 5000, 150), "主轨 3 + 3 − 1 秒转场：{:?}", f.duration_ms);
    // 红色素材 2 倍速只占 1.5 秒：2.0–3.5 秒内可见，之后消失
    let (x, y) = (320, 180);
    assert!(is_red(px_at(&ff, &out, 2.8, x, y).await), "{:?}", px_at(&ff, &out, 2.8, x, y).await);
    let after = px_at(&ff, &out, 4.0, x, y).await;
    assert!(!is_red(after), "2 倍速播完之后消失：{after:?}");
    assert!(plan.out_ms == 5000);
    let _ = std::fs::remove_dir_all(d);
}
