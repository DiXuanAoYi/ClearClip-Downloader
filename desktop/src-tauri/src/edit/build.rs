//! 剪辑工程 → ffmpeg 参数（纯函数，不运行 ffmpeg）。
//!
//! 做法：每个片段一个输入（用输入端的 `-ss` / `-t` 直接跳到片段起点，长视频里取一小段也很快），
//! 在滤镜图里把每个片段处理成统一的尺寸、帧率、像素格式（变速、旋转翻转、缩放适配、调色、淡入淡出），
//! 再按时间线拼起来：没有转场的相邻片段用 `concat`，有转场的用 `xfade`（画面）和 `acrossfade`（声音）；
//! 最后叠文字（`drawtext`）、混配乐（`amix`，可选 `sidechaincompress` 在有人声时压低配乐）。

use std::collections::HashMap;
use std::path::Path;

use crate::audiofx::{simple_chain, AudioFxSpec, FxCaps};
use crate::error::{AppError, AppResult};
use crate::media_tools::{atempo_chain, s, target_kbps, video_codec_for, VideoCodec};
use crate::vidcaps::Caps;
use crate::vidnorm::build::{filter_path, fps_fraction, graph_args, hevc_args, nearest_standard_fps};
use crate::vidnorm::facts::{Facts, Hdr};

use super::keyframes;
use super::region::{focus_knots, focus_window, knot_expr, MaskJob};
use super::spec::{layout, overlay_order, tone_changed, total_ms, AudioTrack, Clip, ClipKind, Overlay, Placed, Project, Region, ShapeMask, TextItem};

/// 命令行总长度的上限。Windows 的上限是 32767 个字符，留一点给 ffmpeg 路径和输出文件。
const MAX_ARGS_LEN: usize = 30_000;
/// 预览的画面高度
const PREVIEW_HEIGHT: u32 = 360;

/// 素材的信息（来自 `ffmpeg -i`）。
#[derive(Debug, Clone, Default)]
pub struct Source {
    pub has_video: bool,
    pub has_audio: bool,
    /// 按显示方向（已经计算了旋转标记）的宽高
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub duration_ms: Option<u64>,
    pub hdr: bool,
}

impl Source {
    pub fn from_facts(f: &Facts) -> Source {
        let (width, height) = f.video.as_ref().map_or((0, 0), |v| v.display_size());
        Source {
            has_video: f.video.is_some(),
            has_audio: f.audio.is_some(),
            width,
            height,
            fps: f.video.as_ref().and_then(|v| v.fps),
            duration_ms: f.duration_ms,
            hdr: f.video.as_ref().is_some_and(|v| v.hdr != Hdr::None),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 按工程设置导出
    Export,
    /// 低分辨率的预览：高度 360、帧率不超过 24、编码最快，用来在界面里看转场和文字的真实效果
    Preview,
}

#[derive(Debug, Default)]
pub struct EditPlan {
    /// ffmpeg 参数（不含公共部分和输出文件）
    pub args: Vec<String>,
    pub ext: &'static str,
    /// 输出时长（毫秒），用来算进度
    pub out_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    /// 区域效果用的遮罩视频：跑 ffmpeg 之前要先生成好（见 `region::render_mask`）
    pub masks: Vec<MaskJob>,
}

fn sec(ms: u64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

fn frac_text(f: (u32, u32)) -> String {
    if f.1 == 1 {
        f.0.to_string()
    } else {
        format!("{}/{}", f.0, f.1)
    }
}

/// 输出尺寸：工程里指定了就用指定的；否则跟随第一个片段（计算了旋转），都不行就 1280×720。
fn output_size(p: &Project, src: &HashMap<String, Source>) -> (u32, u32) {
    if p.out.width > 0 && p.out.height > 0 {
        return (even(p.out.width), even(p.out.height));
    }
    let first = &p.clips[0];
    match src.get(&first.path) {
        Some(sr) if sr.width > 0 && sr.height > 0 => {
            let (w, h) = if matches!(first.rotate, 90 | 270) { (sr.height, sr.width) } else { (sr.width, sr.height) };
            (even(w.min(7680)), even(h.min(7680)))
        }
        _ => (1280, 720),
    }
}

/// 输出帧率：工程里指定了就用指定的；否则跟随第一个视频片段（取最接近的标准帧率），都不行就 30。
fn output_fps(p: &Project, src: &HashMap<String, Source>) -> (u32, u32) {
    if p.out.fps > 0.0 {
        return fps_fraction(p.out.fps);
    }
    p.clips
        .iter()
        .filter(|c| c.kind == ClipKind::Video)
        .find_map(|c| src.get(&c.path).and_then(|s| s.fps))
        .map_or((30, 1), |f| nearest_standard_fps(f.clamp(5.0, 120.0)))
}

/// 旋转和翻转（先旋转再翻转）。
fn orient(c: &Clip) -> Vec<&'static str> {
    let mut v = vec![];
    match c.rotate {
        90 => v.push("transpose=1"),
        180 => v.extend(["hflip", "vflip"]),
        270 => v.push("transpose=2"),
        _ => {}
    }
    if c.flip_h {
        v.push("hflip");
    }
    if c.flip_v {
        v.push("vflip");
    }
    v
}

/// 调色。精简版 ffmpeg 没有（需要 GPL 的）`eq`，所以用 `lutyuv`：
/// 对比度以中灰为轴，亮度直接加减（±1 对应 ±128 级），饱和度缩放色度。
fn tone_expr(brightness: f64, contrast: f64, saturation: f64) -> Option<String> {
    if !tone_changed(brightness, contrast, saturation) {
        return None;
    }
    let mut opts: Vec<String> = vec![];
    if brightness.abs() > 1e-6 || (contrast - 1.0).abs() > 1e-6 {
        opts.push(format!("y='clip((val-128)*{contrast:.4}+128+{:.2},0,255)'", brightness * 128.0));
    }
    if (saturation - 1.0).abs() > 1e-6 {
        let e = format!("'clip((val-128)*{saturation:.4}+128,0,255)'");
        opts.push(format!("u={e}"));
        opts.push(format!("v={e}"));
    }
    Some(format!("lutyuv={}", opts.join(":")))
}

fn tone_filter(c: &Clip) -> Option<String> {
    tone_expr(c.brightness, c.contrast, c.saturation)
}

/// 区域效果建图时要用到的环境。
struct Regions<'a> {
    preview: bool,
    tmp: &'a Path,
    /// 遮罩的输入编号从这里开始（排在片段和音频轨后面）
    first_input: usize,
    /// 成片的宽高比（聚焦“按成片比例取景”用）
    out_aspect: f64,
    masks: Vec<MaskJob>,
    /// 额外输入（遮罩视频、背景图片）的参数，按输入编号顺序
    extra: Vec<Vec<String>>,
}

/// 预览时素材长边超过这个值就先缩小再做区域效果（预览本来就只有 360 高，没必要在 4K 上做模糊）
const PREVIEW_WORK_LONG_SIDE: f64 = 1280.0;

/// 区域效果处理时的画面尺寸（宽、高、是否要先缩小）：素材原尺寸，预览时过大的先缩小。
fn region_work(sr: &Source, preview: bool) -> (u32, u32, bool) {
    let (w, h) = (sr.width, sr.height);
    let long = f64::from(w.max(h));
    if preview && long > PREVIEW_WORK_LONG_SIDE {
        let k = PREVIEW_WORK_LONG_SIDE / long;
        (even((f64::from(w) * k).round() as u32), even((f64::from(h) * k).round() as u32), true)
    } else {
        (w, h, false)
    }
}

/// 区域在片段这段时间里框的平均短边（像素），用来决定马赛克色块和模糊半径的大小。
fn box_short_px(r: &Region, w: u32, h: u32, in_ms: u64, out_ms: u64) -> f64 {
    const N: u32 = 16;
    let span = out_ms.saturating_sub(in_ms) as f64;
    let sum: f64 = (0..N)
        .map(|k| {
            let t = in_ms as f64 + span * (f64::from(k) + 0.5) / f64::from(N);
            r.box_at(t).map_or(0.0, |b| (b.w * f64::from(w)).min(b.h * f64::from(h)))
        })
        .sum();
    (sum / f64::from(N)).max(8.0)
}

/// 带效果的那一份画面（整幅都做，由遮罩决定哪里露出来）。
fn region_effect(r: &Region, w: u32, h: u32, short_px: f64) -> String {
    match r.effect.as_str() {
        "blur" => {
            let sigma = (short_px * (0.04 + 0.26 * r.strength)).max(1.0);
            // 半径大的模糊先缩小再模糊再放大：结果几乎一样，速度快很多
            let k = ((sigma / 4.0).floor() as u32).clamp(1, 8);
            if k == 1 {
                format!("gblur=sigma={sigma:.2}:steps=2")
            } else {
                let (sw, sh) = (w.div_ceil(k).max(2), h.div_ceil(k).max(2));
                format!("scale={sw}:{sh}:flags=area,gblur=sigma={:.2}:steps=2,scale={w}:{h}:flags=bilinear", sigma / f64::from(k))
            }
        }
        "tone" => tone_expr(r.brightness, r.contrast, r.saturation).unwrap_or_else(|| "null".into()),
        _ => {
            // 马赛克：框的短边上有 16（细）… 3（粗）个色块
            let blocks = 16.0 - 13.0 * r.strength;
            let b = (short_px / blocks).round().clamp(2.0, f64::from((w.min(h) / 4).max(2))) as u32;
            let (sw, sh) = (w.div_ceil(b).max(2), h.div_ceil(b).max(2));
            format!("scale={sw}:{sh}:flags=area,scale={w}:{h}:flags=neighbor")
        }
    }
}

/// 一个片段的区域效果。输入是片段的原始画面（`setpts` 之后、旋转翻转之前，方向和追踪时一致），返回处理后的画面标签。
/// 效果区域用遮罩视频叠回原画面（`alphamerge` + `overlay`）；聚焦是一个跟着轨迹移动的 `crop`。
fn region_stage(i: usize, c: &Clip, sr: &Source, setpts: String, rg: &mut Regions, stmts: &mut Vec<String>) -> String {
    let (w, h, prescale) = region_work(sr, rg.preview);
    // 统一成 yuv420p 再处理：带透明度的格式只有它这一族才齐全
    let mut first = vec![setpts, "format=yuv420p".to_string()];
    if prescale {
        first.push(format!("scale={w}:{h}"));
    }
    let mut cur = format!("rs{i}_x");
    stmts.push(format!("[{i}:v:0]{}[{cur}]", first.join(",")));
    let dur = c.duration_ms();
    let mut masked = false;
    for (j, r) in c.regions.iter().enumerate().filter(|(_, r)| !r.is_focus()) {
        let eff = region_effect(r, w, h, box_short_px(r, w, h, c.in_ms, c.out_ms));
        if eff == "null" {
            continue; // 没有设置任何调色：不需要这一层
        }
        let m = rg.first_input + rg.extra.len();
        rg.extra.push(vec!["-i".into(), rg.tmp.join(format!("mask-{i}-{j}.nut")).to_string_lossy().into_owned()]);
        rg.masks.push(MaskJob::new(rg.tmp.join(format!("mask-{i}-{j}.nut")), r.clone(), c.in_ms, c.speed, dur, (w, h), sr.fps, rg.preview));
        let (base, branch, fx, mask, alpha, next) =
            (format!("rb{i}_{j}"), format!("re{i}_{j}"), format!("rf{i}_{j}"), format!("rm{i}_{j}"), format!("ra{i}_{j}"), format!("rs{i}_{j}"));
        stmts.push(format!("[{cur}]split[{base}][{branch}]"));
        stmts.push(format!("[{branch}]{eff}[{fx}]"));
        stmts.push(format!("[{m}:v:0]scale={w}:{h}:flags=bilinear,format=gray[{mask}]"));
        stmts.push(format!("[{fx}][{mask}]alphamerge[{alpha}]"));
        stmts.push(format!("[{base}][{alpha}]overlay=format=auto[{next}]"));
        cur = next;
        masked = true;
    }
    if masked {
        // 遮罩视频比片段长一点：叠完之后裁回片段的长度
        let next = format!("rt{i}");
        stmts.push(format!("[{cur}]trim=end={}[{next}]", sec(dur)));
        cur = next;
    }
    if let Some(r) = c.regions.iter().find(|r| r.is_focus()) {
        // 窗口取成片的宽高比时，旋转 90 / 270 度的片段要在旋转之前取横竖相反的比例
        let aspect = r.reframe.then(|| if matches!(c.rotate, 90 | 270) { 1.0 / rg.out_aspect } else { rg.out_aspect });
        let win = focus_window(r.zoom, (w, h), aspect);
        if win != (w, h) {
            let knots = focus_knots(r, c.in_ms, c.speed, dur, (w, h), win);
            let (xe, ye) = (knot_expr(&knots, |k| k.x), knot_expr(&knots, |k| k.y));
            let next = format!("rz{i}");
            stmts.push(format!("[{cur}]crop=w={}:h={}:x='{xe}':y='{ye}':exact=1[{next}]", win.0, win.1));
            cur = next;
        }
    }
    cur
}

struct Frame<'a> {
    w: u32,
    h: u32,
    fps: &'a str,
    fit: &'a str,
    /// 模糊背景用的模糊滤镜（见 `Caps::background_blur`）
    blur: &'static str,
}

/// 换背景：`input` 是已经套好 `head`（变速、方向、帧率）的画面，输出合成后的标签。
#[allow(clippy::too_many_arguments)]
fn bg_stage(i: usize, c: &Clip, sr: &Source, f: &Frame, input: &str, head: &str, rg: &mut Regions, stmts: &mut Vec<String>) -> String {
    let bg = &c.bg;
    let hex = |s: &str| s.trim_start_matches('#').to_string();
    let swap = matches!(c.rotate, 90 | 270);
    let (sw, sh) = if swap { (sr.height, sr.width) } else { (sr.width, sr.height) };
    let needs_copy = bg.mode != "image";
    let (fg_in, bg_in) = (format!("bk{i}"), format!("bs{i}"));
    if needs_copy {
        stmts.push(format!("{input}{head},split[{bg_in}][{fg_in}]"));
    } else {
        stmts.push(format!("{input}{head}[{fg_in}]"));
    }
    let key = &bg.chroma;
    let mut k = vec!["format=yuva420p".to_string(), format!("chromakey=color=0x{}:similarity={:.3}:blend={:.3}", hex(&key.color), key.similarity, key.blend)];
    if key.despill {
        k.extend(["format=gbrap".into(), format!("despill=type={}:mix=0.5:expand=0.1", key.spill_type()), "format=yuva420p".into()]);
    }
    stmts.push(format!("[{fg_in}]{}[bf{i}]", k.join(",")));
    match bg.mode.as_str() {
        "blur" => stmts.push(format!("[{bg_in}]{}[bb{i}]", f.blur)),
        "image" => {
            let m = rg.first_input + rg.extra.len();
            rg.extra.push(vec![
                "-loop".into(),
                "1".into(),
                "-framerate".into(),
                f.fps.into(),
                "-t".into(),
                sec(c.duration_ms() + 500),
                "-i".into(),
                bg.image.clone(),
            ]);
            let (w, h) = (sw.max(2) & !1, sh.max(2) & !1);
            stmts.push(format!("[{m}:v:0]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h},setsar=1,format=yuv420p[bb{i}]"));
        }
        _ => stmts.push(format!("[{bg_in}]drawbox=x=0:y=0:w=iw:h=ih:color=0x{}:t=fill[bb{i}]", hex(&bg.color))),
    }
    stmts.push(format!("[bb{i}][bf{i}]overlay=format=auto:shortest=1,format=yuv420p[bgv{i}]"));
    format!("bgv{i}")
}

/// 一个片段的画面处理，输出 `[v{i}]`。
fn clip_video(i: usize, c: &Clip, sr: &Source, f: &Frame, rg: &mut Regions, stmts: &mut Vec<String>) {
    let (w, h) = (f.w, f.h);
    let mut head: Vec<String> = vec![];
    let setpts = if c.kind == ClipKind::Video && !c.freeze && (c.speed - 1.0).abs() > 1e-9 {
        format!("setpts=(PTS-STARTPTS)/{:.6}", c.speed)
    } else {
        "setpts=PTS-STARTPTS".into()
    };
    // 有区域效果时，变速之后先做区域（还在素材的方向上，和追踪的坐标一致），再旋转翻转。区域追踪的是正放的素材时间，倒放 / 定格的片段不做区域
    let input = if c.kind == ClipKind::Video && !c.regions.is_empty() && !c.reverse && !c.freeze {
        let label = region_stage(i, c, sr, setpts, rg, stmts);
        format!("[{label}]")
    } else {
        if c.kind == ClipKind::Video && c.freeze {
            // 定格：取入点处的第一帧，停住片段那么长
            let hold = sec(c.duration_ms());
            head.extend(["trim=end_frame=1".into(), setpts, format!("tpad=stop_mode=clone:stop_duration={hold}"), format!("trim=duration={hold}")]);
        } else {
            if c.kind == ClipKind::Video && c.reverse {
                head.push("reverse".into());
            }
            head.push(setpts);
        }
        format!("[{i}:v:0]")
    };
    head.extend(orient(c).into_iter().map(String::from));
    head.push(format!("fps={}", f.fps));
    // 色彩：统一转成 BT.709 电视范围。图片（RGB）转 YUV 时不指定的话 ffmpeg 默认用 BT.601，颜色会偏
    let conv = "out_color_matrix=bt709:out_range=tv";
    let mut tail: Vec<String> = vec![];
    tail.extend(tone_filter(c));
    let d = c.duration_ms();
    if c.fade_in_ms > 0 {
        tail.push(format!("fade=t=in:st=0:d={}", sec(c.fade_in_ms)));
    }
    if c.fade_out_ms > 0 {
        tail.push(format!("fade=t=out:st={}:d={}", sec(d.saturating_sub(c.fade_out_ms)), sec(c.fade_out_ms)));
    }
    tail.extend(["format=yuv420p".into(), "setsar=1".into(), "settb=AVTB".into()]);
    let mut head = head.join(",");
    let mut input = input;
    if c.bg.on && c.bg.matte == "chroma" {
        input = format!("[{}]", bg_stage(i, c, sr, f, &input, &head, rg, stmts));
        head = "null".into();
    }
    let keyed = ["zoom", "panX", "panY"].iter().any(|p| keyframes::track(&c.keyframes, p).is_some());
    // 有缩放 / 平移关键帧时，适配画面这一步先输出到 `[kz{i}]`，缩放平移之后才接淡入淡出、调色
    let tail = tail.join(",");
    let (tail, out) = if keyed { (String::new(), format!("kz{i}")) } else { (tail, format!("v{i}")) };
    let end = if tail.is_empty() { format!("[{out}]") } else { format!(",{tail}[{out}]") };
    match f.fit {
        "cover" => stmts.push(format!("{input}{head},scale={w}:{h}:force_original_aspect_ratio=increase:{conv},format=yuv420p,crop={w}:{h}{end}")),
        "blur" => {
            stmts.push(format!("{input}{head},split[bg{i}][fg{i}]"));
            stmts.push(format!(
                "[bg{i}]scale={w}:{h}:force_original_aspect_ratio=increase:{conv},format=yuv420p,crop={w}:{h},{blur}[bb{i}]",
                blur = f.blur
            ));
            stmts.push(format!("[fg{i}]scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2:{conv},format=yuv420p[ff{i}]"));
            stmts.push(format!("[bb{i}][ff{i}]overlay=(W-w)/2:(H-h)/2{end}"));
        }
        _ => stmts.push(format!(
            "{input}{head},scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2:{conv},format=yuv420p,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black{end}"
        )),
    }
    if keyed {
        clip_keys(i, c, f, stmts);
    }
}

/// 缩放 / 平移关键帧：整幅画面按 `zoom` 缩放，放在黑底上，位置 = 居中 + 平移。
fn clip_keys(i: usize, c: &Clip, f: &Frame, stmts: &mut Vec<String>) {
    let (w, h) = (f.w, f.h);
    let at = |prop: &str, fixed: &str| keyframes::track(&c.keyframes, prop).map_or_else(|| fixed.to_string(), |k| keyframes::expr(&k, "t"));
    let (z, px, py) = (at("zoom", "1"), at("panX", "0"), at("panY", "0"));
    let mut tail: Vec<String> = vec![];
    tail.extend(tone_filter(c));
    let d = c.duration_ms();
    if c.fade_in_ms > 0 {
        tail.push(format!("fade=t=in:st=0:d={}", sec(c.fade_in_ms)));
    }
    if c.fade_out_ms > 0 {
        tail.push(format!("fade=t=out:st={}:d={}", sec(d.saturating_sub(c.fade_out_ms)), sec(c.fade_out_ms)));
    }
    tail.extend(["format=yuv420p".into(), "setsar=1".into(), "settb=AVTB".into()]);
    stmts.push(format!("color=c=black:s={w}x{h}:r={}:d={}[kb{i}]", f.fps, sec(d + 500)));
    stmts.push(format!("[kz{i}]scale=w='max(2,trunc({w}*({z})/2)*2)':h='max(2,trunc({h}*({z})/2)*2)':eval=frame[ks{i}]"));
    stmts.push(format!("[kb{i}][ks{i}]overlay=x='(W-w)/2+({px})*W':y='(H-h)/2+({py})*H':eval=frame:shortest=1:format=auto,{}[v{i}]", tail.join(",")));
}

/// 淡入淡出（声音）。
fn audio_fades(f: &mut Vec<String>, len_ms: u64, fade_in: u64, fade_out: u64) {
    if fade_in > 0 {
        f.push(format!("afade=t=in:st=0:d={}", sec(fade_in)));
    }
    if fade_out > 0 {
        f.push(format!("afade=t=out:st={}:d={}", sec(len_ms.saturating_sub(fade_out)), sec(fade_out)));
    }
}

const AFORMAT: &str = "aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo";

/// 一个片段的声音，输出 `[a{i}]`，长度严格等于片段在时间线上的长度；没有声音（图片、静音、素材没有音轨）时用静音补上。
fn clip_audio(i: usize, c: &Clip, has_audio: bool, fx: &FxCaps, stmts: &mut Vec<String>, warnings: &mut Vec<String>) {
    let d = c.duration_ms();
    if c.kind == ClipKind::Video && has_audio && !c.mute && !c.freeze {
        let mut f: Vec<String> = vec![];
        if c.reverse {
            f.push("areverse".into());
        }
        f.push("asetpts=PTS-STARTPTS".into());
        if (c.speed - 1.0).abs() > 1e-9 {
            f.push(atempo_chain(c.speed));
        }
        f.push(AFORMAT.into());
        if c.denoise != "off" || c.enhance {
            let spec = AudioFxSpec { denoise: c.denoise.clone(), enhance: c.enhance, ..Default::default() };
            let (chain, notes) = simple_chain(&spec, &FxCaps { noise_floor_db: c.noise_floor_db, ..fx.clone() });
            f.extend(chain);
            for n in notes.into_iter().filter(|n| n.contains("没有降噪滤镜")) {
                if !warnings.contains(&n) {
                    warnings.push(n);
                }
            }
        }
        // 素材的声音比画面短一点点很常见，补齐再裁到片段长度，拼接时才不会错位
        f.push(format!("apad=whole_dur={}", sec(d)));
        f.push(format!("atrim=duration={}", sec(d)));
        audio_fades(&mut f, d, c.fade_in_ms, c.fade_out_ms);
        if (c.volume - 1.0).abs() > 1e-9 {
            f.push(format!("volume={:.4}", c.volume));
        }
        stmts.push(format!("[{i}:a:0]{}[a{i}]", f.join(",")));
    } else {
        stmts.push(format!("anullsrc=r=48000:cl=stereo:d={},aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]", sec(d)));
    }
}

/// 叠加素材放进画面前的尺寸（宽、高，偶数）：宽度 = 画面宽度 × 缩放，高度按素材的比例（90 / 270 度旋转后宽高互换）。
fn overlay_size(o: &Overlay, sr: &Source, w: u32) -> (u32, u32) {
    let quarter = quarter_turn(o);
    let (sw, sh) = if matches!(quarter, Some(90 | 270)) { (sr.height, sr.width) } else { (sr.width, sr.height) };
    let ow = (f64::from(w) * o.scale_max()).round().clamp(2.0, 16384.0) as u32;
    let oh = (f64::from(ow) * f64::from(sh.max(1)) / f64::from(sw.max(1))).round().clamp(2.0, 16384.0) as u32;
    (even(ow), even(oh))
}

/// 恰好是 90 / 180 / 270 度的旋转用转置 / 翻转做（不重采样，边缘干净）；其他角度返回 None，用 `rotate` 滤镜。
fn quarter_turn(o: &Overlay) -> Option<u32> {
    if o.keys("rotate").is_some() {
        return None;
    }
    let a = o.angle();
    [90.0, 180.0, 270.0].into_iter().find(|q| (a - q).abs() < 1e-6).map(|q| q as u32)
}

/// 一个叠加素材的画面：处理成带透明度的小画面，时间平移到它在时间线上的位置，输出 `[ov{k}]`。
/// 处理顺序：变速 → 帧率 → 翻转 → 90 度旋转 → 缩放 → 调色 → 任意角度旋转 → 不透明度 → 淡入淡出 → 平移时间。
fn overlay_video(k: usize, idx: usize, o: &Overlay, sr: &Source, frame: (u32, &str), stmts: &mut Vec<String>) {
    let (w, fps) = frame;
    let (ow, oh) = overlay_size(o, sr, w);
    let mut f: Vec<String> = vec![];
    f.push(if o.kind == ClipKind::Video && (o.speed - 1.0).abs() > 1e-9 {
        format!("setpts=(PTS-STARTPTS)/{:.6}", o.speed)
    } else {
        "setpts=PTS-STARTPTS".into()
    });
    f.push(format!("fps={fps}"));
    if o.flip_h {
        f.push("hflip".into());
    }
    if o.flip_v {
        f.push("vflip".into());
    }
    match quarter_turn(o) {
        Some(90) => f.push("transpose=1".into()),
        Some(180) => f.extend(["hflip".into(), "vflip".into()]),
        Some(270) => f.push("transpose=2".into()),
        _ => {}
    }
    // 先转成 BT.709 电视范围的 YUV 再带上透明通道（图片是 RGB，不指定的话 ffmpeg 默认用 BT.601，颜色会偏）
    f.push(format!("scale={ow}:{oh}:flags=bicubic:out_color_matrix=bt709:out_range=tv"));
    f.push("format=yuva420p".into());
    if o.chroma.on {
        // 先按颜色抠出透明通道，再（可选）在 RGB 里去掉留下部分边缘的溢色
        f.push(format!("chromakey=color=0x{}:similarity={:.3}:blend={:.3}", o.chroma.color.trim_start_matches('#'), o.chroma.similarity, o.chroma.blend));
        if o.chroma.despill {
            f.extend(["format=gbrap".into(), format!("despill=type={}:mix=0.5:expand=0.1", o.chroma.spill_type()), "format=yuva420p".into()]);
        }
    }
    if o.mask.on {
        f.push(mask_filter(&o.mask));
    }
    f.extend(tone_expr(o.brightness, o.contrast, o.saturation));
    if let Some(k) = o.keys("rotate") {
        // 角度随时间变：输出做成能装下任意角度的正方形（对角线），转的时候中心不动
        let e = keyframes::expr(&k, "t");
        f.push(format!("rotate=a='({e})*PI/180':ow='trunc(hypot(iw,ih)/2)*2+2':oh='trunc(hypot(iw,ih)/2)*2+2':c=black@0"));
    } else if quarter_turn(o).is_none() && o.angle() > 1e-6 {
        let rad = o.angle().to_radians();
        // 旋转后的外框变大，空出来的四角是透明的
        f.push(format!("rotate={rad:.6}:ow='rotw({rad:.6})':oh='roth({rad:.6})':c=none"));
    }
    if let Some(k) = o.keys("opacity") {
        // 不透明度随时间变：lutyuv 不认时间，用 geq（逐像素算，叠加素材一般不大）
        f.push(format!("geq=lum='lum(X,Y)':cb='cb(X,Y)':cr='cr(X,Y)':a='alpha(X,Y)*clip({},0,1)'", keyframes::expr(&k, "T")));
    } else if o.opacity < 0.999 {
        f.push(format!("lutyuv=a='val*{:.4}'", o.opacity));
    }
    let d = o.duration_ms();
    if o.fade_in_ms > 0 {
        f.push(format!("fade=t=in:st=0:d={}:alpha=1", sec(o.fade_in_ms)));
    }
    if o.fade_out_ms > 0 {
        f.push(format!("fade=t=out:st={}:d={}:alpha=1", sec(d.saturating_sub(o.fade_out_ms)), sec(o.fade_out_ms)));
    }
    if let Some(z) = zoom_expr(o) {
        f.push(format!("scale=w='max(2,trunc(iw*({z})/2)*2)':h='max(2,trunc(ih*({z})/2)*2)':eval=frame"));
    }
    f.push(format!("setpts=PTS+{}/TB", sec(o.start_ms)));
    stmts.push(format!("[{idx}:v:0]{}[ov{k}]", f.join(",")));
}

/// 形状蒙版：逐像素算一个 0–1 的系数乘到透明通道上（形状里 1、外面 0，边上按羽化过渡）。
fn mask_filter(m: &ShapeMask) -> String {
    let (hw, hh) = (m.w / 2.0, m.h / 2.0);
    let (u, v) = (format!("(X/W-{:.5})/{hw:.5}", m.x), format!("(Y/H-{:.5})/{hh:.5}", m.y));
    // d：到形状边缘的“归一化距离”，边缘上 = 1，中心 = 0
    let d = if m.shape == "rect" { format!("max(abs({u}),abs({v}))") } else { format!("hypot({u},{v})") };
    let soft = m.feather.max(0.002);
    let inside = format!("clip(0.5+(1-{d})/{:.5},0,1)", 2.0 * soft);
    let k = if m.invert { format!("(1-{inside})") } else { inside };
    format!("geq=lum='lum(X,Y)':cb='cb(X,Y)':cr='cr(X,Y)':a='alpha(X,Y)*{k}'")
}

/// 入场 / 出场动画里“还没走完的比例”（1 = 刚开始入场 / 已经离场，0 = 在正常位置），`t0` 是动画段的开始时间（秒）。
/// 入场先快后慢，出场先慢后快。
fn anim_left(enter: bool, t0: f64, d: f64) -> String {
    if enter {
        format!("pow(1-clip((t-{t0:.3})/{d:.3},0,1),2)")
    } else {
        format!("pow(clip((t-{t0:.3})/{d:.3},0,1),2)")
    }
}

/// 入场、出场动画的 (时间起点, 持续, 是入场, 动画名) 列表，只含用到的。`base` 是素材在时间线上的开始时间（毫秒）。
fn anim_phases(o: &Overlay) -> Vec<(f64, f64, bool, &str)> {
    let d = o.anim_ms as f64 / 1000.0;
    let mut v = vec![];
    if o.anim_in != "none" {
        v.push((o.start_ms as f64 / 1000.0, d, true, o.anim_in.as_str()));
    }
    if o.anim_out != "none" {
        v.push(((o.end_ms() as f64 - o.anim_ms as f64) / 1000.0, d, false, o.anim_out.as_str()));
    }
    v
}

/// 缩放动画的系数表达式（素材自己的时间，0 起算）；没有缩放动画返回 None。
fn zoom_expr(o: &Overlay) -> Option<String> {
    let dur = o.duration_ms() as f64 / 1000.0;
    let d = o.anim_ms as f64 / 1000.0;
    let keyed = o.keys("scale").map(|k| format!("({})/{:.5}", keyframes::expr(&k, "t"), o.scale_max()));
    let mut terms = String::new();
    if o.anim_in == "zoom" {
        terms.push_str(&format!("-{}", anim_left(true, 0.0, d)));
    }
    if o.anim_out == "zoom" {
        terms.push_str(&format!("-{}", anim_left(false, dur - d, d)));
    }
    match (terms.is_empty(), keyed) {
        (true, None) => None,
        (true, Some(k)) => Some(format!("max(0.02,{k})")),
        (false, None) => Some(format!("max(0.02,1{terms})")),
        (false, Some(k)) => Some(format!("max(0.02,{k})*max(0.02,1{terms})")),
    }
}

/// 把 `[ov{k}]` 盖到 `base` 上，返回新的画面标签。滑入 / 滑出动画写在 x、y 的表达式里。
fn overlay_on(k: usize, base: &str, o: &Overlay, stmts: &mut Vec<String>) -> String {
    let next = format!("ob{k}");
    let (mut dx, mut dy) = (String::new(), String::new());
    for (t0, d, enter, name) in anim_phases(o) {
        let r = anim_left(enter, t0, d);
        match name {
            "slide-left" => dx.push_str(&format!("-{r}*(W*({:.5})+w/2)", o.x)),
            "slide-right" => dx.push_str(&format!("+{r}*(W*(1-({:.5}))+w/2)", o.x)),
            "slide-up" => dy.push_str(&format!("-{r}*(H*({:.5})+h/2)", o.y)),
            "slide-down" => dy.push_str(&format!("+{r}*(H*(1-({:.5}))+h/2)", o.y)),
            _ => {}
        }
    }
    let time = format!("(t-{})", sec(o.start_ms));
    let pos = |prop: &str, fixed: f64| o.keys(prop).map_or_else(|| format!("{fixed:.5}"), |k| keyframes::expr(&k, &time));
    stmts.push(format!(
        "[{base}][ov{k}]overlay=x='W*({})-w/2{dx}':y='H*({})-h/2{dy}':enable='between(t,{},{})':eof_action=pass:format=auto[{next}]",
        pos("x", o.x),
        pos("y", o.y),
        sec(o.start_ms),
        sec(o.end_ms())
    ));
    next
}

/// 叠加素材自带的声音 → `[oa{k}]`（对齐到它在时间线上的起点）；图片、静音、素材没有声音时返回 false。
fn overlay_audio(k: usize, idx: usize, o: &Overlay, has_audio: bool, stmts: &mut Vec<String>) -> bool {
    let keyed = o.keys("volume");
    if o.kind != ClipKind::Video || o.mute || !has_audio || (keyed.is_none() && o.volume < 1e-6) {
        return false;
    }
    let d = o.duration_ms();
    let mut f: Vec<String> = vec!["asetpts=PTS-STARTPTS".into()];
    if (o.speed - 1.0).abs() > 1e-9 {
        f.push(atempo_chain(o.speed));
    }
    f.push(AFORMAT.into());
    f.push(format!("apad=whole_dur={}", sec(d)));
    f.push(format!("atrim=duration={}", sec(d)));
    audio_fades(&mut f, d, o.fade_in_ms, o.fade_out_ms);
    if let Some(k) = keyed {
        f.push(format!("volume=volume='max(0,{})':eval=frame", keyframes::expr(&k, "t")));
    } else if (o.volume - 1.0).abs() > 1e-9 {
        f.push(format!("volume={:.4}", o.volume));
    }
    if o.start_ms > 0 {
        f.push(format!("adelay={0}|{0}", o.start_ms));
    }
    stmts.push(format!("[{idx}:a:0]{}[oa{k}]", f.join(",")));
    true
}

fn color_arg(hex: &str, alpha: f64) -> String {
    format!("0x{}@{:.2}", hex.trim_start_matches('#'), alpha)
}

/// 一条文字 → `drawtext`。文字内容写进文件再让 ffmpeg 去读，这样引号、冒号、百分号、换行都不用转义。
fn drawtext(k: usize, t: &TextItem, h: u32, font: Option<&Path>, tmp: &Path) -> AppResult<String> {
    std::fs::create_dir_all(tmp)?;
    let file = tmp.join(format!("text-{k}.txt"));
    std::fs::write(&file, t.text.replace("\r\n", "\n").replace('\r', "\n"))?;
    let px = (f64::from(h) * t.size / 100.0).round().max(8.0) as u32;
    let mut o: Vec<String> = vec![format!("textfile={}", filter_path(&file)), "expansion=none".into()];
    if let Some(f) = font {
        o.push(format!("fontfile={}", filter_path(f)));
    }
    o.push(format!("fontsize={px}"));
    o.push(format!("fontcolor={}", color_arg(&t.color, t.opacity)));
    let time = format!("(t-{})", sec(t.start_ms));
    let key = |prop: &str| keyframes::track(&t.keyframes, prop).map(|k| keyframes::expr(&k, &time));
    let opacity = key("opacity").map_or_else(|| format!("{:.2}", t.opacity), |e| format!("clip({e},0,1)"));
    if t.fade_ms > 0 {
        // 淡入淡出：只作用在字上，描边和底色块不跟着变
        let (s0, s1, f) = (sec(t.start_ms), sec(t.end_ms), sec(t.fade_ms));
        o.push(format!("alpha='{opacity}*min(1,min((t-{s0})/{f},({s1}-t)/{f}))'"));
    } else if opacity.starts_with("clip(") {
        o.push(format!("alpha='{opacity}'"));
    }
    o.push(format!("x=(w*{})-(text_w/2)", key("x").map_or_else(|| format!("{:.4}", t.x), |e| format!("({e})"))));
    o.push(format!("y=(h*{})-(text_h/2)", key("y").map_or_else(|| format!("{:.4}", t.y), |e| format!("({e})"))));
    if t.outline {
        o.push(format!("borderw={}", ((f64::from(px) * 0.06).round() as u32).max(2)));
        o.push(format!("bordercolor={}", color_arg(&t.outline_color, t.opacity)));
    }
    if t.boxed {
        o.push("box=1".into());
        o.push(format!("boxcolor={}", color_arg(&t.box_color, t.box_opacity * t.opacity)));
        o.push(format!("boxborderw={}", px / 4));
    }
    o.push(format!("enable='between(t,{},{})'", sec(t.start_ms), sec(t.end_ms)));
    Ok(format!("drawtext={}", o.join(":")))
}

/// 一条配乐 → `[m{k}]`。`idx` 是它的输入编号，`total` 是时间线总长。返回 false 表示这条不会被听到（起点在视频结束之后）。
fn music_track(k: usize, idx: usize, a: &AudioTrack, src: &Source, total: u64, stmts: &mut Vec<String>) -> bool {
    if a.start_ms >= total {
        return false;
    }
    let avail = total - a.start_ms;
    let seg = a.out_ms.map(|o| o - a.in_ms).or_else(|| src.duration_ms.map(|d| d.saturating_sub(a.in_ms)));
    let len = if a.looped { avail } else { seg.map_or(avail, |s| s.min(avail)) };
    let mut f: Vec<String> = vec![AFORMAT.into()];
    if a.in_ms > 0 || a.out_ms.is_some() {
        let mut t = format!("atrim=start={}", sec(a.in_ms));
        if let Some(o) = a.out_ms {
            t.push_str(&format!(":end={}", sec(o)));
        }
        f.push(t);
    }
    f.push("asetpts=PTS-STARTPTS".into());
    if a.looped {
        // aloop 要把循环的这一段全部放进内存：按这一段的长度算样本数，不知道长度时按 10 分钟封顶
        let samples = seg.map_or(28_800_000u64, |s| s * 48 + 48);
        f.push(format!("aloop=loop=-1:size={samples}"));
    }
    f.push(format!("atrim=duration={}", sec(len)));
    audio_fades(&mut f, len, a.fade_in_ms, a.fade_out_ms);
    if (a.volume - 1.0).abs() > 1e-9 {
        f.push(format!("volume={:.4}", a.volume));
    }
    if a.start_ms > 0 {
        f.push(format!("adelay={0}|{0}", a.start_ms));
    }
    stmts.push(format!("[{idx}:a:0]{}[m{k}]", f.join(",")));
    true
}

/// 画面按时间线拼接：相邻且没有转场的片段分成一组，用 `concat` 拼；组与组之间用 `xfade` 和 `acrossfade` 叠起来。
fn assemble(placed: &[Placed], clips: &[Clip], audio: bool, stmts: &mut Vec<String>, caps: &Caps) -> AppResult<(String, Option<String>)> {
    // 没有转场的相邻片段归为一组
    let mut groups: Vec<Vec<usize>> = vec![];
    for (i, pl) in placed.iter().enumerate() {
        match groups.last_mut() {
            Some(g) if i > 0 && pl.overlap_ms == 0 => g.push(i),
            _ => groups.push(vec![i]),
        }
    }
    // 每组：一个片段直接用，多个片段 concat
    let mut gv: Vec<String> = vec![];
    let mut ga: Vec<String> = vec![];
    for (g, members) in groups.iter().enumerate() {
        if members.len() == 1 {
            gv.push(format!("v{}", members[0]));
            ga.push(format!("a{}", members[0]));
            continue;
        }
        let ins: String = members.iter().map(|i| if audio { format!("[v{i}][a{i}]") } else { format!("[v{i}]") }).collect();
        if audio {
            stmts.push(format!("{ins}concat=n={}:v=1:a=1[gx{g}][ga{g}]", members.len()));
        } else {
            stmts.push(format!("{ins}concat=n={}:v=1:a=0[gx{g}]", members.len()));
        }
        stmts.push(format!("[gx{g}]settb=AVTB[gv{g}]"));
        gv.push(format!("gv{g}"));
        ga.push(format!("ga{g}"));
    }
    let (mut cv, mut ca) = (gv[0].clone(), ga[0].clone());
    for g in 1..groups.len() {
        let first = groups[g][0];
        let pl = placed[first];
        let t = clips[first].transition.as_ref().ok_or_else(|| AppError::msg("内部错误：转场丢失。"))?;
        if !caps.has_filter("xfade") {
            return Err(AppError::invalid("当前的 ffmpeg 没有转场滤镜（xfade）。请在“设置 → 组件”里更新或安装完整版 ffmpeg。"));
        }
        stmts.push(format!("[{cv}][{}]xfade=transition={}:duration={}:offset={}[xy{g}]", gv[g], t.kind, sec(pl.overlap_ms), sec(pl.start_ms)));
        stmts.push(format!("[xy{g}]settb=AVTB[xv{g}]"));
        cv = format!("xv{g}");
        if audio {
            stmts.push(format!("[{ca}][{}]acrossfade=d={}:c1=tri:c2=tri[xa{g}]", ga[g], sec(pl.overlap_ms)));
            ca = format!("xa{g}");
        }
    }
    Ok((cv, audio.then_some(ca)))
}

pub fn build(p: &Project, src: &HashMap<String, Source>, caps: &Caps, tmp: &Path, mode: Mode, default_font: Option<&Path>) -> AppResult<EditPlan> {
    let mut plan = EditPlan::default();
    let preview = mode == Mode::Preview;

    // 素材检查
    for (n, c) in p.clips.iter().enumerate() {
        let sr = src.get(&c.path).ok_or_else(|| AppError::invalid(format!("没有读取到第 {} 个片段的素材信息。", n + 1)))?;
        if !sr.has_video {
            return Err(AppError::invalid(format!("第 {} 个片段的素材里没有画面。", n + 1)));
        }
        if sr.hdr && !plan.warnings.iter().any(|w| w.contains("HDR")) {
            plan.warnings.push("有 HDR 素材：导出时不做 HDR → SDR 转换，画面会发灰。在检查器里点“转成普通画面并替换”，或先用“视频规整”转成 SDR。".into());
        }
    }
    for (n, o) in p.overlays.iter().enumerate() {
        let sr = src.get(&o.path).ok_or_else(|| AppError::invalid(format!("没有读取到第 {} 个叠加素材的信息。", n + 1)))?;
        if !sr.has_video || sr.width == 0 || sr.height == 0 {
            return Err(AppError::invalid(format!("第 {} 个叠加素材里没有画面。", n + 1)));
        }
    }
    for (n, a) in p.audio.iter().enumerate() {
        let sr = src.get(&a.path).ok_or_else(|| AppError::invalid(format!("没有读取到第 {} 条音频轨的素材信息。", n + 1)))?;
        if !sr.has_audio {
            return Err(AppError::invalid(format!("第 {} 条音频轨的素材里没有声音。", n + 1)));
        }
    }
    let need = |name: &str, what: &str| -> AppResult<()> {
        if caps.has_filter(name) {
            Ok(())
        } else {
            Err(AppError::invalid(format!("当前的 ffmpeg 没有“{name}”滤镜，{what}。请在“设置 → 组件”里更新或安装完整版 ffmpeg。")))
        }
    };
    if !p.texts.is_empty() {
        need("drawtext", "无法添加文字")?;
    }
    if p.clips.iter().any(Clip::needs_tone) {
        need("lutyuv", "无法调色")?;
    }
    if p.clips.iter().any(|c| c.freeze) {
        need("tpad", "无法定格画面")?;
    }
    if p.clips.iter().any(|c| c.reverse && !c.freeze) {
        need("reverse", "无法倒放")?;
    }
    let regions = || p.clips.iter().filter(|c| !c.reverse && !c.freeze).flat_map(|c| c.regions.iter());
    if regions().any(|r| !r.is_focus()) {
        need("alphamerge", "无法使用区域效果")?;
        need("overlay", "无法使用区域效果")?;
    }
    if regions().any(|r| r.effect == "blur") {
        need("gblur", "无法使用区域模糊")?;
    }
    if regions().any(|r| r.effect == "tone") {
        need("lutyuv", "无法使用区域调色")?;
    }
    if regions().any(Region::is_focus) {
        need("crop", "无法使用跟随聚焦")?;
    }
    if p.audio.iter().any(|a| a.duck) {
        need("sidechaincompress", "无法自动压低配乐")?;
    }
    if !p.overlays.is_empty() {
        need("overlay", "无法使用叠加轨")?;
    }
    if p.overlays.iter().any(|o| o.needs_tone() || (o.opacity < 0.999 && o.keys("opacity").is_none())) {
        need("lutyuv", "无法调整叠加素材的颜色和透明度")?;
    }
    if p.overlays.iter().any(|o| o.keys("opacity").is_some()) {
        need("geq", "无法给叠加素材做不透明度关键帧")?;
    }
    if p.clips.iter().any(|c| c.bg.on && c.bg.matte == "chroma") {
        need("chromakey", "无法换背景")?;
        if p.clips.iter().any(|c| c.bg.on && c.bg.chroma.despill) {
            need("despill", "无法去溢色")?;
        }
    }
    if p.overlays.iter().any(|o| o.chroma.on) {
        need("chromakey", "无法抠像")?;
        if p.overlays.iter().any(|o| o.chroma.on && o.chroma.despill) {
            need("despill", "无法去溢色")?;
        }
    }
    if p.overlays.iter().any(|o| o.keys("rotate").is_some() || (quarter_turn(o).is_none() && o.angle() > 1e-6)) {
        need("rotate", "无法旋转叠加素材")?;
    }

    // 输出规格
    let (mut w, mut h) = output_size(p, src);
    let mut fps = output_fps(p, src);
    if preview {
        if h > PREVIEW_HEIGHT {
            w = even((f64::from(w) * f64::from(PREVIEW_HEIGHT) / f64::from(h)).round() as u32);
            h = PREVIEW_HEIGHT;
        }
        if frac_f(fps) > 24.5 {
            fps = (24, 1);
        }
    }
    let fps_txt = frac_text(fps);
    let total = total_ms(&p.clips);
    let placed = layout(&p.clips);
    plan.width = w;
    plan.height = h;
    plan.fps = frac_f(fps);
    plan.out_ms = total;

    // 输入
    let mut args: Vec<String> = vec![];
    for c in &p.clips {
        match c.kind {
            ClipKind::Video => {
                if c.in_ms > 0 {
                    args.extend(["-ss".into(), sec(c.in_ms)]);
                }
                // 定格只要入点那一帧，多读一点点就够
                let span = if c.freeze { (c.out_ms - c.in_ms).min(500) } else { c.out_ms - c.in_ms };
                args.extend(["-t".into(), sec(span), "-i".into(), c.path.clone()]);
            }
            ClipKind::Image => {
                args.extend(["-loop".into(), "1".into(), "-framerate".into(), fps_txt.clone(), "-t".into(), sec(c.out_ms), "-i".into(), c.path.clone()]);
            }
        }
    }
    for a in &p.audio {
        args.extend(["-i".into(), a.path.clone()]);
    }
    // 叠加素材：起点在成片结束之后的不会出现，不放进输入
    let mut ovs: Vec<(usize, &Overlay)> = vec![];
    for (n, o) in p.overlays.iter().enumerate() {
        if o.start_ms >= total {
            plan.warnings.push(format!("第 {} 个叠加素材的起点在成片结束之后，不会出现。", n + 1));
        } else {
            ovs.push((n, o));
        }
    }
    let ov_first = p.clips.len() + p.audio.len();
    for (_, o) in &ovs {
        match o.kind {
            ClipKind::Video => {
                if o.looped {
                    args.extend(["-stream_loop".into(), "-1".into()]);
                }
                if o.in_ms > 0 {
                    args.extend(["-ss".into(), sec(o.in_ms)]);
                }
                args.extend(["-t".into(), sec(o.out_ms - o.in_ms), "-i".into(), o.path.clone()]);
            }
            ClipKind::Image => {
                args.extend(["-loop".into(), "1".into(), "-framerate".into(), fps_txt.clone(), "-t".into(), sec(o.out_ms), "-i".into(), o.path.clone()]);
            }
        }
    }

    // 滤镜图
    let ov_audio = |o: &Overlay| o.kind == ClipKind::Video && !o.mute && o.volume > 1e-6 && src.get(&o.path).is_some_and(|s| s.has_audio);
    let has_audio = !p.audio.is_empty()
        || ovs.iter().any(|(_, o)| ov_audio(o))
        || p.clips.iter().any(|c| c.kind == ClipKind::Video && !c.mute && src.get(&c.path).is_some_and(|s| s.has_audio));
    let frame = Frame { w, h, fps: &fps_txt, fit: &p.out.fit, blur: caps.background_blur() };
    let mut rg = Regions { preview, tmp, first_input: ov_first + ovs.len(), out_aspect: f64::from(w) / f64::from(h), masks: vec![], extra: vec![] };
    let mut stmts: Vec<String> = vec![];
    let fx = FxCaps::from_caps(caps, None);
    for (i, c) in p.clips.iter().enumerate() {
        clip_video(i, c, src.get(&c.path).expect("checked above"), &frame, &mut rg, &mut stmts);
        if has_audio {
            clip_audio(i, c, src.get(&c.path).is_some_and(|s| s.has_audio), &fx, &mut stmts, &mut plan.warnings);
        }
    }
    let (mut video, main_audio) = assemble(&placed, &p.clips, has_audio, &mut stmts, caps)?;

    // 叠加轨：从下到上依次盖到主轨画面上
    let mut ov_mix: Vec<String> = vec![];
    if !ovs.is_empty() {
        let mut slot: HashMap<usize, usize> = HashMap::new();
        for (j, (n, _)) in ovs.iter().enumerate() {
            slot.insert(*n, j);
        }
        for n in overlay_order(&p.overlays) {
            let Some(&j) = slot.get(&n) else { continue };
            let o = &p.overlays[n];
            let sr = src.get(&o.path).expect("checked above");
            let idx = ov_first + j;
            overlay_video(j, idx, o, sr, (w, &fps_txt), &mut stmts);
            video = overlay_on(j, &video, o, &mut stmts);
            if has_audio && overlay_audio(j, idx, o, sr.has_audio, &mut stmts) {
                ov_mix.push(format!("oa{j}"));
            }
            if o.end_ms() > total {
                plan.notes.push(format!("第 {} 个叠加素材超出成片结尾，超出的部分被截掉", n + 1));
            }
        }
        plan.notes.push(format!("叠加轨上有 {} 个素材", ovs.len()));
    }

    // 文字
    if !p.texts.is_empty() {
        let font = p.texts.iter().find_map(|t| t.font.as_deref().map(Path::new)).or(default_font);
        if font.is_none() {
            plan.warnings.push("没有找到可用的字体，文字可能显示不出来（特别是中文）。请在文字属性里选择一个字体文件。".into());
        }
        // 逐字高亮的文字交给 libass（ASS 滤镜）画；没有这个滤镜时按普通文字画
        let karaoke_ok = caps.has_filter("ass");
        let (ass_idx, plain_idx): (Vec<usize>, Vec<usize>) = (0..p.texts.len()).partition(|&k| karaoke_ok && p.texts[k].karaoke());
        if !karaoke_ok && p.texts.iter().any(TextItem::karaoke) {
            plan.warnings.push("当前的 ffmpeg 没有“ass”滤镜，逐字高亮的字幕按普通文字显示。".into());
        }
        let mut chain: Vec<String> = vec![];
        for &k in &plain_idx {
            let t = &p.texts[k];
            let own = t.font.as_deref().map(Path::new).or(font);
            chain.push(drawtext(k, t, h, own, tmp)?);
        }
        if !chain.is_empty() {
            stmts.push(format!("[{video}]{}[vtx]", chain.join(",")));
            video = "vtx".into();
        }
        if !ass_idx.is_empty() {
            std::fs::create_dir_all(tmp)?;
            let items: Vec<&TextItem> = ass_idx.iter().map(|&k| &p.texts[k]).collect();
            let file = tmp.join("captions.ass");
            let family = font.and_then(super::fonts::family_name);
            std::fs::write(&file, super::captions::document(&items, w, h, family.as_deref()))?;
            let dir = font.and_then(Path::parent).map(|d| format!(":fontsdir={}", filter_path(d))).unwrap_or_default();
            stmts.push(format!("[{video}]ass=filename={}{dir}[vass]", filter_path(&file)));
            video = "vass".into();
        }
    }

    // 配乐和叠加素材的声音：混在主轨声音上面
    let mut audio_out: Option<String> = main_audio;
    if has_audio {
        let main = audio_out.clone().expect("main audio exists when anything has sound");
        // (标签, 要不要在主轨有声音时压低)
        let mut heard: Vec<(String, bool)> = ov_mix.iter().map(|l| (l.clone(), false)).collect();
        for (k, a) in p.audio.iter().enumerate() {
            let idx = p.clips.len() + k;
            let sr = src.get(&a.path).expect("checked above");
            if music_track(k, idx, a, sr, total, &mut stmts) {
                heard.push((format!("m{k}"), a.duck));
            } else {
                plan.warnings.push(format!("第 {} 条音频轨的起点在视频结束之后，不会被听到。", k + 1));
            }
        }
        if !heard.is_empty() {
            let ducked = heard.iter().filter(|(_, d)| *d).count();
            let mut mix_main = main.clone();
            if ducked > 0 {
                let outs: String = (0..ducked).map(|j| format!("[sc{j}]")).collect();
                stmts.push(format!("[{main}]asplit={}[mm]{outs}", ducked + 1));
                mix_main = "mm".into();
            }
            let mut ins = format!("[{mix_main}]");
            let mut j = 0;
            for (label, duck) in &heard {
                if *duck {
                    // 压低后的标签沿用配乐的编号：[m0] → [d0]
                    let d = format!("d{}", label.trim_start_matches('m'));
                    stmts.push(format!("[{label}][sc{j}]sidechaincompress=threshold=0.02:ratio=8:attack=30:release=500[{d}]"));
                    ins.push_str(&format!("[{d}]"));
                    j += 1;
                } else {
                    ins.push_str(&format!("[{label}]"));
                }
            }
            need("amix", "无法混入配乐")?;
            stmts.push(format!("{ins}amix=inputs={}:duration=first:dropout_transition=0:normalize=0[amx]", heard.len() + 1));
            audio_out = Some("amx".into());
        }
    }

    // 编码
    let level = match p.out.quality.as_str() {
        "small" => 0,
        "high" => 2,
        _ => 1,
    };
    let pps = f64::from(w) * f64::from(h) * frac_f(fps);
    let want_hevc = p.out.codec == "hevc" && !preview;
    let hevc = if want_hevc { hevc_args(caps, level, target_kbps(pps, level)) } else { None };
    if want_hevc && hevc.is_none() {
        plan.warnings.push(format!(
            "当前的 ffmpeg 没有 HEVC 编码器，已改用 H.264。{}",
            if caps.gpl { "" } else { "在“设置 → 组件”里安装完整版 ffmpeg 可以使用 libx265。" }
        ));
    }
    let want_ext = if p.out.format == "mkv" && !preview { "mkv" } else { "mp4" };
    let vc: VideoCodec = if preview && caps.encoders.h264 == Some("libx264") {
        VideoCodec {
            args: s(&["-c:v", "libx264", "-preset", "ultrafast", "-crf", "32", "-pix_fmt", "yuv420p"]),
            ext: "mp4",
            audio: s(&["-c:a", "aac", "-b:a", "96k"]),
            note: None,
            pix: "yuv420p",
        }
    } else {
        let mut vc = video_codec_for(&caps.encoders, if preview { "small" } else { &p.out.quality }, want_ext, Some(pps));
        // 编辑后的成片画质比“压缩”高一档
        if !preview && vc.args.iter().any(|a| a == "libx264") {
            if let Some(i) = vc.args.iter().position(|a| a == "-crf") {
                vc.args[i + 1] = ["26", "22", "18"][level].into();
            }
        }
        vc
    };
    let (vargs, pix, ext, acodec): (Vec<String>, &str, &'static str, Vec<String>) = match hevc {
        Some((a, pix)) => {
            plan.notes.push("视频编码为 HEVC".into());
            (a, pix, want_ext, s(&["-c:a", "aac", "-b:a", "192k"]))
        }
        None => {
            if let Some(n) = vc.note.clone() {
                plan.warnings.push(n);
            }
            let ext = if preview && vc.ext == "mkv" && vc.args.iter().any(|a| a == "libvpx-vp9") { "webm" } else { vc.ext };
            (vc.args.clone(), vc.pix, ext, vc.audio.clone())
        }
    };
    // 收尾：像素格式（编码器要求的）和色彩标记（BT.709、电视范围）
    stmts.push(format!("[{video}]format={pix},setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv[vout]"));

    // 区域效果的遮罩视频：作为额外的输入，排在片段和音频轨之后
    for a in &rg.extra {
        args.extend(a.iter().cloned());
    }
    if !rg.masks.is_empty() {
        plan.notes.push(format!("有 {} 个区域效果，导出前会先生成遮罩", rg.masks.len()));
    }
    plan.masks = rg.masks;

    let graph = stmts.join(";");
    args.extend(graph_args(&graph, caps, tmp)?);
    args.extend(s(&["-map", "[vout]"]));
    if let Some(a) = &audio_out {
        args.extend(["-map".into(), format!("[{a}]")]);
    }
    args.extend(vargs);
    if audio_out.is_some() {
        args.extend(acodec);
        args.extend(s(&["-ar", "48000", "-ac", "2"]));
    }
    args.extend(["-t".into(), sec(total)]);
    args.extend(s(&["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv", "-max_muxing_queue_size", "4096"]));

    let len: usize = args.iter().map(|a| a.len() + 3).sum();
    if len > MAX_ARGS_LEN {
        return Err(AppError::invalid("片段太多或素材的路径太长，超出了系统命令行的长度限制。请减少片段，或把素材放到路径更短的文件夹里。"));
    }
    if !p.clips.iter().any(|c| c.kind == ClipKind::Video) {
        plan.notes.push("时间线上只有图片".into());
    }
    plan.args = args;
    plan.ext = ext;
    Ok(plan)
}

fn frac_f(f: (u32, u32)) -> f64 {
    f64::from(f.0) / f64::from(f.1)
}

#[cfg(test)]
mod tests {
    use super::super::spec::{TextItem, Transition};
    use super::*;
    use crate::media_tools::Encoders;

    fn caps() -> Caps {
        let names = [
            "scale",
            "pad",
            "crop",
            "fps",
            "boxblur",
            "gblur",
            "xfade",
            "acrossfade",
            "drawtext",
            "lutyuv",
            "amix",
            "sidechaincompress",
            "aloop",
            "atempo",
            "concat",
            "alphamerge",
            "overlay",
            "rotate",
        ];
        Caps {
            gpl: true,
            filters: names.iter().map(|n| n.to_string()).collect(),
            encoders: Encoders { h264: Some("libx264"), vp9: true, mp3: true, opus: true },
            hevc: Some("libx265"),
            ..Default::default()
        }
    }

    fn sources(paths: &[&str]) -> HashMap<String, Source> {
        paths
            .iter()
            .map(|p| {
                (
                    p.to_string(),
                    Source {
                        has_video: true,
                        has_audio: !p.contains("silent"),
                        width: 1920,
                        height: 1080,
                        fps: Some(29.97),
                        duration_ms: Some(60_000),
                        hdr: false,
                    },
                )
            })
            .collect()
    }

    fn clip(path: &str, from: u64, to: u64) -> Clip {
        Clip { id: 1, path: path.into(), in_ms: from, out_ms: to, ..Default::default() }
    }

    fn with_t(mut c: Clip, kind: &str, ms: u64) -> Clip {
        c.transition = Some(Transition { kind: kind.into(), duration_ms: ms });
        c
    }

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("clearclip-editb-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn graph_of(plan: &EditPlan) -> String {
        let i = plan.args.iter().position(|a| a == "-filter_complex").expect("graph inline");
        plan.args[i + 1].clone()
    }

    fn arg_after<'a>(a: &'a [String], k: &str) -> Option<&'a str> {
        a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).map(String::as_str)
    }

    #[test]
    fn clips_are_trimmed_at_the_input_and_joined_with_concat() {
        let p = Project { clips: vec![clip("/m/a.mp4", 2000, 5000), clip("/m/b.mp4", 0, 1500)], ..Default::default() }.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4", "/m/b.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let a = &plan.args;
        // 第一个片段：-ss 2 -t 3；第二个从头开始，没有 -ss
        assert_eq!(&a[0..6], &["-ss", "2.000", "-t", "3.000", "-i", "/m/a.mp4"]);
        assert_eq!(&a[6..10], &["-t", "1.500", "-i", "/m/b.mp4"]);
        let g = graph_of(&plan);
        assert!(g.contains("[v0][a0][v1][a1]concat=n=2:v=1:a=1"), "{g}");
        assert!(!g.contains("xfade"), "没有转场就不用 xfade：{g}");
        assert_eq!((plan.out_ms, plan.width, plan.height), (4500, 1920, 1080));
        assert_eq!(arg_after(a, "-t"), Some("3.000"));
        assert_eq!(a.iter().filter(|x| *x == "-t").count(), 3, "两个输入各一个，输出一个");
        assert!(
            a.windows(2).any(|w| w == ["-map", "[vout]"]) && a.windows(2).any(|w| w == ["-map", "[amx]"] || w == ["-map", "[xa1]"] || w == ["-map", "[ga0]"]),
            "{a:?}"
        );
        assert_eq!((plan.ext, plan.fps > 29.9 && plan.fps < 30.0), ("mp4", true), "跟随第一个片段的 29.97 帧");
    }

    #[test]
    fn transitions_become_xfade_and_acrossfade_at_the_layout_offset() {
        let p = Project {
            clips: vec![clip("/m/a.mp4", 0, 4000), with_t(clip("/m/b.mp4", 0, 3000), "wipeleft", 1000), clip("/m/c.mp4", 0, 2000)],
            ..Default::default()
        }
        .checked()
        .unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4", "/m/b.mp4", "/m/c.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let g = graph_of(&plan);
        // 第 2、3 个片段没有转场、相邻，并成一组
        assert!(g.contains("[v1][a1][v2][a2]concat=n=2"), "{g}");
        assert!(g.contains("[v0][gv1]xfade=transition=wipeleft:duration=1.000:offset=3.000"), "{g}");
        assert!(g.contains("[a0][ga1]acrossfade=d=1.000:c1=tri:c2=tri"), "{g}");
        assert_eq!(plan.out_ms, 8000);
        assert_eq!(arg_after(&plan.args, "-map"), Some("[vout]"));
    }

    #[test]
    fn missing_filters_are_reported_in_plain_words() {
        let mut c = caps();
        c.filters.remove("xfade");
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 4000), with_t(clip("/m/b.mp4", 0, 3000), "fade", 500)], ..Default::default() }.checked().unwrap();
        let e = build(&p, &sources(&["/m/a.mp4", "/m/b.mp4"]), &c, &tmp(), Mode::Export, None).unwrap_err();
        assert!(e.message.contains("xfade") && e.message.contains("完整版"), "{}", e.message);

        let mut c = caps();
        c.filters.remove("drawtext");
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 4000)], ..Default::default() }.checked().unwrap();
        p.texts = vec![TextItem { text: "你好".into(), ..Default::default() }];
        let e = build(&p, &sources(&["/m/a.mp4"]), &c, &tmp(), Mode::Export, None).unwrap_err();
        assert!(e.message.contains("drawtext"), "{}", e.message);
    }

    #[test]
    fn speed_rotation_fit_and_tone_go_into_the_clip_chain() {
        let mut c = clip("/m/a.mp4", 0, 6000);
        c.speed = 2.0;
        c.rotate = 90;
        c.flip_h = true;
        c.brightness = 0.25;
        c.saturation = 0.0;
        c.fade_in_ms = 500;
        c.fade_out_ms = 500;
        c.volume = 0.5;
        let mut p = Project { clips: vec![c], ..Default::default() };
        p.out.width = 1080;
        p.out.height = 1920;
        p.out.fit = "blur".into();
        let p = p.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let g = graph_of(&plan);
        assert!(g.contains("setpts=(PTS-STARTPTS)/2.000000,transpose=1,hflip,fps=30000/1001"), "{g}");
        assert!(g.contains("split[bg0][fg0]") && g.contains("boxblur=25:5") && g.contains("overlay=(W-w)/2:(H-h)/2"), "{g}");
        assert!(g.contains("lutyuv=y='clip((val-128)*1.0000+128+32.00,0,255)':u='clip((val-128)*0.0000+128,0,255)'"), "{g}");
        assert!(g.contains("fade=t=in:st=0:d=0.500") && g.contains("fade=t=out:st=2.500:d=0.500"), "{g}");
        assert!(g.contains("atempo=2.0000") && g.contains("volume=0.5000") && g.contains("afade=t=out:st=2.500:d=0.500"), "{g}");
        assert!(g.contains("apad=whole_dur=3.000,atrim=duration=3.000"), "声音补齐并裁到片段长度：{g}");
        assert_eq!((plan.width, plan.height, plan.out_ms), (1080, 1920, 3000));
    }

    #[test]
    fn images_hold_for_their_duration_and_get_silence() {
        let img = Clip { kind: ClipKind::Image, path: "/m/p.png".into(), out_ms: 2500, ..Default::default() };
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 3000), img], ..Default::default() }.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4", "/m/p.png"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let a = &plan.args;
        assert!(a.windows(8).any(|w| w == ["-loop", "1", "-framerate", "30000/1001", "-t", "2.500", "-i", "/m/p.png"]), "{a:?}");
        let g = graph_of(&plan);
        assert!(g.contains("anullsrc=r=48000:cl=stereo:d=2.500"), "图片没有声音，用静音补上：{g}");
        assert!(!g.contains("[1:a:0]"), "{g}");
    }

    #[test]
    fn a_project_without_any_sound_has_no_audio_stream() {
        let img = Clip { kind: ClipKind::Image, path: "/m/p.png".into(), out_ms: 2500, ..Default::default() };
        let mut srcs = sources(&["/m/p.png"]);
        srcs.get_mut("/m/p.png").unwrap().has_audio = false;
        let p = Project { clips: vec![img], ..Default::default() }.checked().unwrap();
        let plan = build(&p, &srcs, &caps(), &tmp(), Mode::Export, None).unwrap();
        assert!(!plan.args.iter().any(|a| a == "-c:a" || a == "[aout]" || a.contains("anullsrc")), "{:?}", plan.args);
        // 静音的视频片段 + 无配乐：同样没有音轨
        let mut c = clip("/m/a.mp4", 0, 3000);
        c.mute = true;
        let p = Project { clips: vec![c], ..Default::default() }.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        assert!(!plan.args.iter().any(|a| a == "-c:a"));
    }

    #[test]
    fn texts_are_written_to_files_and_timed() {
        let tmp = tmp();
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 5000)], ..Default::default() };
        p.texts = vec![TextItem {
            text: "100% 好看: it's \"ok\"\n第二行".into(),
            start_ms: 1000,
            end_ms: 3500,
            x: 0.5,
            y: 0.9,
            size: 5.0,
            boxed: true,
            ..Default::default()
        }];
        let p = p.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp, Mode::Export, Some(Path::new("/fonts/msyh.ttc"))).unwrap();
        let g = graph_of(&plan);
        assert!(g.contains("drawtext=textfile=") && g.contains("expansion=none") && g.contains("fontfile=/fonts/msyh.ttc"), "{g}");
        assert!(g.contains("fontsize=54") && g.contains("fontcolor=0xFFFFFF@1.00"), "{g}");
        assert!(g.contains("x=(w*0.5000)-(text_w/2)") && g.contains("y=(h*0.9000)-(text_h/2)"), "{g}");
        assert!(g.contains("box=1:boxcolor=0x000000@0.50:boxborderw=13") && g.contains("borderw=3"), "{g}");
        assert!(g.contains("enable='between(t,1.000,3.500)'"), "{g}");
        // 文字内容原样写进文件，不用转义
        assert_eq!(std::fs::read_to_string(tmp.join("text-0.txt")).unwrap(), "100% 好看: it's \"ok\"\n第二行");
        assert!(plan.warnings.is_empty());
        let plan = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp, Mode::Export, None).unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("字体")), "{:?}", plan.warnings);
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn music_is_trimmed_delayed_looped_and_mixed() {
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 10_000)], ..Default::default() };
        p.audio = vec![
            AudioTrack {
                path: "/m/bgm.mp3".into(),
                start_ms: 2000,
                in_ms: 5000,
                out_ms: Some(9000),
                volume: 0.4,
                fade_in_ms: 1000,
                fade_out_ms: 1000,
                duck: true,
                ..Default::default()
            },
            AudioTrack { path: "/m/loop.mp3".into(), looped: true, ..Default::default() },
            AudioTrack { path: "/m/late.mp3".into(), start_ms: 20_000, ..Default::default() },
        ];
        let p = p.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4", "/m/bgm.mp3", "/m/loop.mp3", "/m/late.mp3"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let g = graph_of(&plan);
        // 第一条：取 5–9 秒（4 秒），延后 2 秒，音量 0.4，淡入淡出
        assert!(g.contains("[1:a:0]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,atrim=start=5.000:end=9.000,asetpts=PTS-STARTPTS,atrim=duration=4.000,afade=t=in:st=0:d=1.000,afade=t=out:st=3.000:d=1.000,volume=0.4000,adelay=2000|2000[m0]"), "{g}");
        // 第二条循环到视频结束（10 秒），素材 60 秒 → 循环的一段是整个素材
        assert!(g.contains("aloop=loop=-1:size=2880048,atrim=duration=10.000"), "{g}");
        // 起点在结束之后的不会进混音，并给出提示
        assert!(!g.contains("[m2]") && plan.warnings.iter().any(|w| w.contains("第 3 条音频轨")), "{g} {:?}", plan.warnings);
        assert!(g.contains("asplit=2[mm][sc0]"), "{g}");
        assert!(g.contains("[m0][sc0]sidechaincompress=threshold=0.02"), "{g}");
        assert!(g.contains("[mm][d0][m1]amix=inputs=3:duration=first:dropout_transition=0:normalize=0[amx]"), "{g}");
        assert!(plan.args.windows(2).any(|w| w == ["-map", "[amx]"]));
    }

    #[test]
    fn preview_is_small_fast_and_capped_at_24_fps() {
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 4000)], ..Default::default() };
        p.out.fps = 60.0;
        p.out.quality = "high".into();
        p.out.codec = "hevc".into();
        p.out.format = "mkv".into();
        let p = p.checked().unwrap();
        let plan = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp(), Mode::Preview, None).unwrap();
        assert_eq!((plan.width, plan.height, plan.fps, plan.ext), (640, 360, 24.0, "mp4"));
        assert_eq!(arg_after(&plan.args, "-preset"), Some("ultrafast"));
        let g = graph_of(&plan);
        assert!(g.contains("fps=24,") && g.contains("scale=640:360"), "{g}");
        // 导出：按工程设置，HEVC + MKV
        let plan = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        assert_eq!((plan.width, plan.height, plan.ext), (1920, 1080, "mkv"));
        assert_eq!(arg_after(&plan.args, "-c:v"), Some("libx265"));
        assert!(graph_of(&plan).contains("fps=60,"));
    }

    #[test]
    fn long_graphs_go_to_a_file_and_huge_command_lines_are_refused() {
        let clips: Vec<Clip> = (0..120).map(|i| clip(&format!("/m/{i}.mp4"), 0, 3000)).collect();
        let paths: Vec<String> = clips.iter().map(|c| c.path.clone()).collect();
        let srcs = sources(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let p = Project { clips, ..Default::default() }.checked().unwrap();
        let tmp = tmp();
        let plan = build(&p, &srcs, &caps(), &tmp, Mode::Export, None).unwrap();
        assert!(plan.args.iter().any(|a| a == "-/filter_complex" || a == "-filter_complex_script"), "长的滤镜图放进文件");
        assert!(std::fs::read_to_string(tmp.join("filtergraph.txt")).unwrap().contains("concat=n=120"));
        let _ = std::fs::remove_dir_all(&tmp);
        // 路径特别长：输入参数就超过了命令行上限
        let long = "x".repeat(300);
        let clips: Vec<Clip> = (0..120).map(|i| clip(&format!("/m/{long}{i}.mp4"), 0, 3000)).collect();
        let paths: Vec<String> = clips.iter().map(|c| c.path.clone()).collect();
        let srcs = sources(&paths.iter().map(String::as_str).collect::<Vec<_>>());
        let p = Project { clips, ..Default::default() }.checked().unwrap();
        let e = build(&p, &srcs, &caps(), &tmp, Mode::Export, None).unwrap_err();
        assert!(e.message.contains("命令行"), "{}", e.message);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_blurred_background_uses_gblur_on_builds_without_boxblur() {
        let mut c = Clip { path: "/m/a.mp4".into(), out_ms: 3000, ..Default::default() };
        c.id = 1;
        let mut p = Project { clips: vec![c], ..Default::default() };
        (p.out.width, p.out.height, p.out.fit) = (1080, 1920, "blur".into());
        let p = p.checked().unwrap();
        let mut lite = caps();
        lite.filters.remove("boxblur");
        let g = graph_of(&build(&p, &sources(&["/m/a.mp4"]), &lite, &tmp(), Mode::Export, None).unwrap());
        assert!(g.contains("crop=1080:1920,gblur=sigma=30[bb0]") && !g.contains("boxblur"), "{g}");
    }

    #[test]
    fn sources_without_picture_or_sound_are_refused() {
        let mut srcs = sources(&["/m/a.mp4", "/m/bgm.mp3"]);
        srcs.get_mut("/m/a.mp4").unwrap().has_video = false;
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 4000)], ..Default::default() }.checked().unwrap();
        assert!(build(&p, &srcs, &caps(), &tmp(), Mode::Export, None).unwrap_err().message.contains("没有画面"));
        let mut srcs = sources(&["/m/a.mp4", "/m/silent.mp3"]);
        srcs.get_mut("/m/silent.mp3").unwrap().has_audio = false;
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 4000)], ..Default::default() };
        p.audio = vec![AudioTrack { path: "/m/silent.mp3".into(), ..Default::default() }];
        let p = p.checked().unwrap();
        assert!(build(&p, &srcs, &caps(), &tmp(), Mode::Export, None).unwrap_err().message.contains("没有声音"));
    }

    #[test]
    fn hdr_sources_get_a_warning() {
        let mut srcs = sources(&["/m/a.mp4"]);
        srcs.get_mut("/m/a.mp4").unwrap().hdr = true;
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 4000)], ..Default::default() }.checked().unwrap();
        let plan = build(&p, &srcs, &caps(), &tmp(), Mode::Export, None).unwrap();
        assert!(plan.warnings.iter().any(|w| w.contains("HDR")));
    }

    // ---- 区域效果 ----

    use super::super::spec::Region;
    use crate::track::TrackPt;

    fn track(x: f64) -> Vec<TrackPt> {
        vec![
            TrackPt { t_ms: 0, x, y: 0.4, w: 0.2, h: 0.2, ..Default::default() },
            TrackPt { t_ms: 3000, x: x + 0.3, y: 0.4, w: 0.2, h: 0.2, ..Default::default() },
        ]
    }

    fn with_region(mut c: Clip, r: Region) -> Clip {
        c.regions.push(Region { track: track(0.1), ..r });
        c
    }

    fn effect(kind: &str) -> Region {
        Region { effect: kind.into(), ..Default::default() }
    }

    fn built(p: Project, caps: &Caps, mode: Mode) -> EditPlan {
        let p = p.checked().unwrap();
        build(&p, &sources(&["/m/a.mp4", "/m/b.mp4"]), caps, &tmp(), mode, None).unwrap()
    }

    #[test]
    fn a_mosaic_region_is_merged_back_through_a_mask_before_rotation() {
        let mut c = with_region(clip("/m/a.mp4", 1000, 7000), effect("mosaic"));
        c.speed = 2.0;
        c.rotate = 90;
        let plan = built(Project { clips: vec![c], ..Default::default() }, &caps(), Mode::Export);
        let g = graph_of(&plan);
        // 变速 → 统一格式 → 区域（还在素材的方向上）→ 裁回片段长度 → 旋转、帧率……
        assert!(g.contains("[0:v:0]setpts=(PTS-STARTPTS)/2.000000,format=yuv420p[rs0_x]"), "{g}");
        assert!(g.contains("[rs0_x]split[rb0_0][re0_0]"), "{g}");
        // 框的短边 = 0.2×1080 = 216 像素，强度 0.5 → 9.5 个色块 → 约 23 像素一块
        assert!(g.contains("[re0_0]scale=84:47:flags=area,scale=1920:1080:flags=neighbor[rf0_0]"), "{g}");
        assert!(g.contains("[1:v:0]scale=1920:1080:flags=bilinear,format=gray[rm0_0]"), "遮罩是第 2 个输入：{g}");
        assert!(g.contains("[rf0_0][rm0_0]alphamerge[ra0_0]") && g.contains("[rb0_0][ra0_0]overlay=format=auto[rs0_0]"), "{g}");
        assert!(g.contains("[rs0_0]trim=end=3.000[rt0]"), "{g}");
        assert!(g.contains("[rt0]transpose=1,fps=30000/1001"), "{g}");
        // 遮罩作为额外的输入排在片段后面；任务里先生成它
        assert_eq!(plan.masks.len(), 1);
        let m = &plan.masks[0];
        assert_eq!((m.w, m.h, m.fps, m.in_ms, m.speed), (800, 450, 60, 1000, 2.0));
        assert!(plan.args.windows(2).any(|w| w[0] == "-i" && w[1] == m.path.to_string_lossy()), "{:?}", plan.args);
        let clip_in = plan.args.iter().position(|a| a == "/m/a.mp4").unwrap();
        let mask_in = plan.args.iter().position(|a| a.ends_with("mask-0-0.nut")).unwrap();
        assert!(clip_in < mask_in);
        assert!(plan.notes.iter().any(|n| n.contains("1 个区域效果")), "{:?}", plan.notes);
    }

    #[test]
    fn blur_and_tone_regions_and_stacking_order() {
        let mut c = with_region(clip("/m/a.mp4", 0, 3000), Region { effect: "blur".into(), strength: 1.0, ..Default::default() });
        c = with_region(c, Region { effect: "tone".into(), brightness: 0.3, saturation: 0.0, invert: true, ..Default::default() });
        // 没有设置任何调色的调色区域：不产生遮罩
        c = with_region(c, Region { effect: "tone".into(), ..Default::default() });
        let plan = built(Project { clips: vec![c], ..Default::default() }, &caps(), Mode::Export);
        let g = graph_of(&plan);
        // 模糊：短边 216 像素，强度 1 → sigma 64.8，先缩小 8 倍再模糊
        assert!(g.contains("[re0_0]scale=240:135:flags=area,gblur=sigma=8.10:steps=2,scale=1920:1080:flags=bilinear[rf0_0]"), "{g}");
        assert!(
            g.contains(
                "[re0_1]lutyuv=y='clip((val-128)*1.0000+128+38.40,0,255)':u='clip((val-128)*0.0000+128,0,255)':v='clip((val-128)*0.0000+128,0,255)'[rf0_1]"
            ),
            "{g}"
        );
        // 第二层叠在第一层的结果上
        assert!(g.contains("[rs0_0]split[rb0_1][re0_1]"), "{g}");
        assert!(!g.contains("re0_2"), "{g}");
        assert_eq!(plan.masks.len(), 2);
        assert!(plan.masks[1].region.invert);
        assert!(g.contains("[1:v:0]scale") && g.contains("[2:v:0]scale"), "{g}");
    }

    #[test]
    fn focus_is_a_moving_crop_and_can_match_the_output_shape() {
        // 横屏素材做竖屏成片：窗口取 9:16
        let r = Region { effect: "focus".into(), zoom: 1.5, reframe: true, smooth: 0.0, ..Default::default() };
        let mut p = Project { clips: vec![with_region(clip("/m/a.mp4", 0, 3000), r.clone())], ..Default::default() };
        (p.out.width, p.out.height) = (1080, 1920);
        let plan = built(p, &caps(), Mode::Export);
        let g = graph_of(&plan);
        assert!(g.contains("[rs0_x]crop=w=404:h=720:x='if(lt(t,"), "{g}");
        assert!(g.contains(":y='") && g.contains(":exact=1[rz0]"), "{g}");
        assert!(g.contains("[rz0]fps="), "旋转之前：{g}");
        assert!(plan.masks.is_empty() && !g.contains("alphamerge") && !g.contains("trim=end"), "聚焦不用遮罩：{g}");
        // 片段旋转 90 度：在旋转之前取横竖相反的窗口（旋转后正好是 9:16）
        let mut c = with_region(clip("/m/a.mp4", 0, 3000), Region { zoom: 1.0, ..r.clone() });
        c.rotate = 90;
        let mut p = Project { clips: vec![c], ..Default::default() };
        (p.out.width, p.out.height) = (1080, 1920);
        let g = graph_of(&built(p, &caps(), Mode::Export));
        assert!(!g.contains("crop=w="), "整幅画面旋转后就是 9:16，不用裁：{g}");
        // 放大 1 倍且不改比例：什么都不做
        let flat = Region { zoom: 1.0, reframe: false, ..r };
        let g = graph_of(&built(Project { clips: vec![with_region(clip("/m/a.mp4", 0, 3000), flat)], ..Default::default() }, &caps(), Mode::Export));
        assert!(!g.contains("crop=w="), "{g}");
    }

    #[test]
    fn previews_shrink_big_sources_before_the_region_effect_and_use_small_masks() {
        let mut srcs = sources(&["/m/a.mp4"]);
        let s = srcs.get_mut("/m/a.mp4").unwrap();
        (s.width, s.height) = (3840, 2160);
        let p = Project { clips: vec![with_region(clip("/m/a.mp4", 0, 3000), effect("blur"))], ..Default::default() }.checked().unwrap();
        let plan = build(&p, &srcs, &caps(), &tmp(), Mode::Preview, None).unwrap();
        let g = graph_of(&plan);
        assert!(g.contains("format=yuv420p,scale=1280:720[rs0_x]") && g.contains("scale=1280:720:flags=bilinear,format=gray"), "{g}");
        assert_eq!((plan.masks[0].w, plan.masks[0].h), (480, 270));
        // 导出：原尺寸
        let g = graph_of(&build(&p, &srcs, &caps(), &tmp(), Mode::Export, None).unwrap());
        assert!(!g.contains("scale=1280:720") && g.contains("scale=3840:2160:flags=bilinear,format=gray"), "{g}");
    }

    #[test]
    fn region_effects_need_their_filters_and_images_ignore_them() {
        let p = |kind: &str| Project { clips: vec![with_region(clip("/m/a.mp4", 0, 3000), effect(kind))], ..Default::default() }.checked().unwrap();
        let srcs = sources(&["/m/a.mp4"]);
        for (kind, missing) in [("mosaic", "alphamerge"), ("blur", "gblur"), ("tone", "lutyuv"), ("focus", "crop")] {
            let mut c = caps();
            c.filters.insert("alphamerge".into());
            c.filters.insert("overlay".into());
            c.filters.remove(missing);
            let mut proj = p(kind);
            if kind == "tone" {
                proj.clips[0].regions[0].brightness = 0.2;
            }
            if kind == "focus" {
                proj.clips[0].regions[0].zoom = 2.0;
            }
            let e = build(&proj, &srcs, &c, &tmp(), Mode::Export, None).unwrap_err();
            assert!(e.message.contains(missing) && e.message.contains("完整版"), "{kind}: {}", e.message);
        }
        // 图片没有区域效果（校验时就丢掉了）
        let img = Clip {
            kind: ClipKind::Image,
            path: "/m/p.png".into(),
            out_ms: 2000,
            regions: vec![Region { track: track(0.1), ..Default::default() }],
            ..Default::default()
        };
        let q = Project { clips: vec![img], ..Default::default() }.checked().unwrap();
        assert!(q.clips[0].regions.is_empty());
    }

    #[test]
    fn clips_without_regions_keep_their_exact_chain() {
        // 没有区域效果时滤镜图和以前完全一样：变速、旋转、帧率连在一条链上，没有额外的输入
        let mut c = clip("/m/a.mp4", 0, 3000);
        c.speed = 2.0;
        c.rotate = 90;
        let plan = built(Project { clips: vec![c], ..Default::default() }, &caps(), Mode::Export);
        let g = graph_of(&plan);
        assert!(g.contains("[0:v:0]setpts=(PTS-STARTPTS)/2.000000,transpose=1,fps="), "{g}");
        assert!(plan.masks.is_empty() && !g.contains("rs0_"));
    }

    fn ov(path: &str, track: u32, start: u64, from: u64, to: u64) -> Overlay {
        Overlay { id: 1, path: path.into(), track, start_ms: start, in_ms: from, out_ms: to, ..Default::default() }
    }

    fn built_ov(overlays: Vec<Overlay>, paths: &[&str]) -> EditPlan {
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 6000)], overlays, ..Default::default() }.checked().unwrap();
        let mut all = vec!["/m/a.mp4"];
        all.extend_from_slice(paths);
        build(&p, &sources(&all), &caps(), &tmp(), Mode::Export, None).unwrap()
    }

    #[test]
    fn overlay_animations_are_expressions_on_position_and_size() {
        let mut o = ov("/o/b.mp4", 1, 1000, 0, 3000);
        o.x = 0.8;
        o.anim_in = "slide-left".into();
        o.anim_out = "zoom".into();
        o.anim_ms = 400;
        let g = graph_of(&built_ov(vec![o], &["/o/b.mp4"]));
        assert!(g.contains("x='W*(0.80000)-w/2-pow(1-clip((t-1.000)/0.400,0,1),2)*(W*(0.80000)+w/2)'"), "{g}");
        assert!(g.contains("y='H*(0.50000)-h/2'"), "{g}");
        assert!(g.contains("scale=w='max(2,trunc(iw*(max(0.02,1-pow(clip((t-2.600)/0.400,0,1),2)))/2)*2)'"), "{g}");
        let plain = graph_of(&built_ov(vec![ov("/o/b.mp4", 1, 1000, 0, 3000)], &["/o/b.mp4"]));
        assert!(!plain.contains("pow(") && !plain.contains("eval=frame"), "{plain}");
    }

    #[test]
    fn overlays_become_inputs_after_the_clips_and_music() {
        let mut gif = ov("/o/loop.gif", 1, 500, 0, 3000);
        gif.looped = true;
        let png = Overlay { kind: ClipKind::Image, ..ov("/o/p.png", 1, 1000, 0, 2000) };
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 6000)], overlays: vec![ov("/o/b.mp4", 2, 2000, 1000, 4000), gif, png], ..Default::default() }
            .checked()
            .unwrap();
        p.audio = vec![AudioTrack { path: "/m/music.mp3".into(), ..Default::default() }];
        let plan = build(&p, &sources(&["/m/a.mp4", "/m/music.mp3", "/o/b.mp4", "/o/loop.gif", "/o/p.png"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let a = &plan.args;
        let joined = a.join(" ");
        // 输入顺序：主轨片段 → 音频轨 → 叠加素材（按列表顺序）
        let order: Vec<usize> =
            ["/m/a.mp4", "/m/music.mp3", "/o/b.mp4", "/o/loop.gif", "/o/p.png"].iter().map(|n| a.iter().position(|x| x == n).unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{joined}");
        assert!(joined.contains("-ss 1.000 -t 3.000 -i /o/b.mp4"), "视频叠加素材在输入端裁剪：{joined}");
        assert!(joined.contains("-stream_loop -1 -t 3.000 -i /o/loop.gif"), "循环播放用 -stream_loop：{joined}");
        assert!(joined.contains("-loop 1 -framerate 30000/1001 -t 2.000 -i /o/p.png"), "{joined}");
        assert_eq!(a.iter().filter(|x| *x == "-stream_loop").count(), 1);
        let g = graph_of(&plan);
        // 层叠顺序：1 号轨的 gif（0.5 秒起）→ png（1 秒起）→ 2 号轨的 b.mp4
        let pos = |s: &str| g.find(s).unwrap_or_else(|| panic!("{s} 不在图里：{g}"));
        let (gif_at, png_at, mp4_at) = (pos("[ov1]overlay"), pos("[ov2]overlay"), pos("[ov0]overlay"));
        assert!(gif_at < png_at && png_at < mp4_at, "{g}");
        assert!(g.contains("setpts=PTS+2.000/TB[ov0]") && g.contains("setpts=PTS+0.500/TB[ov1]"), "{g}");
        assert!(g.contains("enable='between(t,2.000,5.000)':eof_action=pass"), "{g}");
        assert_eq!(plan.out_ms, 6000);
    }

    #[test]
    fn an_overlay_is_scaled_rotated_and_faded_in_a_fixed_order() {
        let mut o = ov("/o/b.mp4", 1, 1000, 0, 4000);
        o.scale = 0.25;
        o.x = 0.75;
        o.y = 0.2;
        o.rotate = 30.0;
        o.opacity = 0.5;
        o.flip_h = true;
        o.brightness = 0.1;
        o.fade_in_ms = 500;
        o.fade_out_ms = 500;
        o.speed = 2.0;
        let plan = built_ov(vec![o], &["/o/b.mp4"]);
        let g = graph_of(&plan);
        // 1920 × 25% = 480 宽；素材 1920×1080 → 480×270
        let chain = g.split(';').find(|s| s.starts_with("[1:v:0]")).expect(&g);
        let order = [
            "setpts=(PTS-STARTPTS)/2.000000",
            "fps=30000/1001",
            "hflip",
            "scale=480:270",
            "format=yuva420p",
            "lutyuv=y=",
            "rotate=0.523599",
            "lutyuv=a='val*0.5000'",
            "fade=t=in:st=0:d=0.500:alpha=1",
            "fade=t=out:st=1.500:d=0.500:alpha=1",
            "setpts=PTS+1.000/TB[ov0]",
        ];
        let mut at = 0;
        for part in order {
            let i = chain[at..].find(part).unwrap_or_else(|| panic!("{part} 不在 {chain} 里（或顺序不对）"));
            at += i + part.len();
        }
        assert!(g.contains("overlay=x='W*(0.75000)-w/2':y='H*(0.20000)-h/2'"), "{g}");
        assert!(g.contains("ow='rotw(0.523599)':oh='roth(0.523599)':c=none"), "{g}");
        // 2 倍速：4 秒素材占 2 秒，从 1 秒到 3 秒
        assert!(g.contains("between(t,1.000,3.000)"), "{g}");
    }

    #[test]
    fn quarter_turns_use_transpose_and_swap_the_size() {
        let mut o = ov("/o/b.mp4", 1, 0, 0, 2000);
        o.scale = 0.25;
        o.rotate = 90.0;
        let g = graph_of(&built_ov(vec![o.clone()], &["/o/b.mp4"]));
        // 90 度：先转置，宽 480、高按转置后的比例 1080:1920 → 853.3（取偶数 852）
        assert!(g.contains("transpose=1,scale=480:852"), "{g}");
        assert!(!g.contains("rotate="), "{g}");
        o.rotate = -90.0;
        let g = graph_of(&built_ov(vec![o.clone()], &["/o/b.mp4"]));
        assert!(g.contains("transpose=2,scale=480:852"), "{g}");
        o.rotate = 180.0;
        let g = graph_of(&built_ov(vec![o], &["/o/b.mp4"]));
        assert!(g.contains("hflip,vflip,scale=480:270"), "{g}");
    }

    #[test]
    fn an_overlay_with_sound_joins_the_mix_even_without_music() {
        let plan = built_ov(vec![ov("/o/b.mp4", 1, 2000, 0, 3000)], &["/o/b.mp4"]);
        let g = graph_of(&plan);
        assert!(g.contains("[1:a:0]asetpts=PTS-STARTPTS,aresample=48000"), "{g}");
        assert!(g.contains("adelay=2000|2000[oa0]"), "{g}");
        assert!(g.contains("[a0][oa0]amix=inputs=2:duration=first"), "{g}");
        // 静音 / 图片 / 素材没有声音：不混
        let mut muted = ov("/o/b.mp4", 1, 2000, 0, 3000);
        muted.mute = true;
        let g = graph_of(&built_ov(vec![muted], &["/o/b.mp4"]));
        assert!(!g.contains("oa0") && !g.contains("amix"), "{g}");
        let g = graph_of(&built_ov(vec![ov("/o/silent.mp4", 1, 0, 0, 3000)], &["/o/silent.mp4"]));
        assert!(!g.contains("oa0"), "{g}");
        // 和配乐一起混
        let mut p = Project { clips: vec![clip("/m/a.mp4", 0, 6000)], overlays: vec![ov("/o/b.mp4", 1, 0, 0, 3000)], ..Default::default() }.checked().unwrap();
        p.audio = vec![AudioTrack { path: "/m/music.mp3".into(), duck: true, ..Default::default() }];
        let plan = build(&p, &sources(&["/m/a.mp4", "/m/music.mp3", "/o/b.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap();
        let g = graph_of(&plan);
        assert!(g.contains("[m0][sc0]sidechaincompress") && g.contains("[mm][oa0][d0]amix=inputs=3"), "{g}");
    }

    #[test]
    fn overlays_that_start_after_the_end_are_dropped_with_a_warning() {
        let plan = built_ov(vec![ov("/o/b.mp4", 1, 6000, 0, 2000), ov("/o/c.mp4", 1, 5000, 0, 3000)], &["/o/b.mp4", "/o/c.mp4"]);
        assert!(plan.warnings.iter().any(|w| w.contains("第 1 个叠加素材") && w.contains("不会出现")), "{:?}", plan.warnings);
        assert!(plan.notes.iter().any(|n| n.contains("第 2 个叠加素材超出成片结尾")), "{:?}", plan.notes);
        assert!(!plan.args.contains(&"/o/b.mp4".to_string()), "起点在结尾之后的不放进输入");
        let g = graph_of(&plan);
        assert!(!g.contains("[ov1]"), "{g}");
    }

    #[test]
    fn overlay_problems_are_reported_in_plain_words() {
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 3000)], overlays: vec![ov("/o/b.mp4", 1, 0, 0, 2000)], ..Default::default() }.checked().unwrap();
        let mut c = caps();
        c.filters.remove("overlay");
        let e = build(&p, &sources(&["/m/a.mp4", "/o/b.mp4"]), &c, &tmp(), Mode::Export, None).unwrap_err();
        assert!(e.message.contains("overlay") && e.message.contains("完整版"), "{}", e.message);
        // 读不到素材信息
        let e = build(&p, &sources(&["/m/a.mp4"]), &caps(), &tmp(), Mode::Export, None).unwrap_err();
        assert!(e.message.contains("第 1 个叠加素材"), "{}", e.message);
        // 素材里没有画面
        let mut srcs = sources(&["/m/a.mp4", "/o/b.mp4"]);
        srcs.get_mut("/o/b.mp4").unwrap().has_video = false;
        let e = build(&p, &srcs, &caps(), &tmp(), Mode::Export, None).unwrap_err();
        assert!(e.message.contains("没有画面"), "{}", e.message);
        // 透明度 / 旋转需要的滤镜
        let mut o = ov("/o/b.mp4", 1, 0, 0, 2000);
        o.opacity = 0.5;
        let p = Project { clips: vec![clip("/m/a.mp4", 0, 3000)], overlays: vec![o], ..Default::default() }.checked().unwrap();
        let mut c = caps();
        c.filters.remove("lutyuv");
        assert!(build(&p, &sources(&["/m/a.mp4", "/o/b.mp4"]), &c, &tmp(), Mode::Export, None).unwrap_err().message.contains("lutyuv"));
    }

    #[test]
    fn projects_without_overlays_build_exactly_as_before() {
        let plan = built_ov(vec![], &[]);
        let g = graph_of(&plan);
        assert!(!g.contains("overlay") && !g.contains("[ov") && !g.contains("[ob"), "{g}");
        assert!(!plan.notes.iter().any(|n| n.contains("叠加")));
    }
    #[test]
    fn text_keyframes_become_drawtext_expressions() {
        use super::super::keyframes::Keyframe;
        let mut t = TextItem { text: "hi".into(), start_ms: 1000, end_ms: 4000, ..Default::default() };
        let plain = drawtext(0, &t, 720, None, &std::env::temp_dir().join("clearclip-kf-text")).unwrap();
        assert!(plain.contains("x=(w*0.5000)-(text_w/2)"), "{plain}");
        let k = |t: u64, v: f64| Keyframe { t_ms: t, value: v, ease: "linear".into() };
        t.keyframes.insert("x".into(), vec![k(0, 0.2), k(1000, 0.8)]);
        t.keyframes.insert("opacity".into(), vec![k(0, 0.0), k(500, 1.0)]);
        let keyed = drawtext(0, &t, 720, None, &std::env::temp_dir().join("clearclip-kf-text")).unwrap();
        assert!(keyed.contains("x=(w*(if(lt((t-1.000),0.000)"), "{keyed}");
        assert!(keyed.contains("alpha='clip(if(lt((t-1.000)"), "{keyed}");
        assert!(keyed.contains("y=(h*0.8500)-(text_h/2)"), "没有关键帧的属性仍用固定值：{keyed}");
    }
    #[test]
    fn chroma_key_goes_into_the_overlay_chain_before_tone_and_rotation() {
        let mut o = Overlay { path: "o.mp4".into(), out_ms: 2000, brightness: 0.2, rotate: 10.0, ..Default::default() };
        let sr = Source { has_video: true, has_audio: false, width: 640, height: 360, fps: Some(25.0), duration_ms: Some(2000), hdr: false };
        let graph = |o: &Overlay| {
            let mut stmts = vec![];
            overlay_video(0, 1, o, &sr, (1280, "25"), &mut stmts);
            stmts.join(";")
        };
        assert!(!graph(&o).contains("chromakey"));
        o.chroma.on = true;
        o.chroma.color = "#00B140".into();
        let g = graph(&o);
        let key = g.find("chromakey=color=0x00B140:similarity=0.180:blend=0.080").expect(&g);
        assert!(g.find("despill=type=green").unwrap() > key, "{g}");
        assert!(g.find("rotate=").unwrap() > key, "{g}");
        o.chroma.despill = false;
        assert!(!graph(&o).contains("despill"));
    }
}
