//! 剪辑工程：片段（主轨）、文字、音频轨、输出规格，以及时间线的排布计算。
//!
//! 时间线的约定：主轨上的片段首尾相接；某个片段设了转场时，它的开头和上一个片段的结尾重叠（重叠的长度 = 转场时长），
//! 所以第 i 个片段的开始 = 上一个片段的结束 − 转场时长。文字和音频轨的时间都是时间线上的时间。
//! 前端有一份同样的排布计算（`src/utils/edit.ts` 的 `layout`），改这里的规则时要一起改。

use serde::{Deserialize, Serialize};

use super::keyframes::{self, Keyframe, KeyframeMap};
use crate::track::TrackPt;

pub const MAX_CLIPS: usize = 300;
/// 叠加素材的入场 / 出场动画
pub const ANIMS: [&str; 6] = ["none", "slide-left", "slide-right", "slide-up", "slide-down", "zoom"];
pub const MAX_TEXTS: usize = 500;
pub const MAX_AUDIO: usize = 20;
/// 叠加轨上最多的素材数、最多的轨道数
pub const MAX_OVERLAYS: usize = 100;
pub const MAX_OVERLAY_TRACKS: u32 = 8;
/// 一个片段上最多的区域数
pub const MAX_REGIONS: usize = 8;
/// 一条区域轨迹最多的点数（追踪结果经过精简，正常远少于这个数）
pub const MAX_TRACK_POINTS: usize = 20_000;
/// 片段、文字最短的长度
pub const MIN_MS: u64 = 100;
/// 小于这个长度的转场当作没有（只会闪一下）
pub const MIN_TRANSITION_MS: u64 = 100;
/// 叠加素材、文字、音频最晚从多久开始（10 小时），防止数值溢出
pub const MAX_START_MS: u64 = 36_000_000;
/// 一个属性最多几个关键点
pub const MAX_KEYFRAMES: usize = 64;

/// 可选的转场（xfade 的名字）。用真实的 ffmpeg 逐个试过，见端到端测试。
pub const TRANSITIONS: &[&str] = &[
    "fade",
    "fadeblack",
    "fadewhite",
    "dissolve",
    "wipeleft",
    "wiperight",
    "wipeup",
    "wipedown",
    "slideleft",
    "slideright",
    "slideup",
    "slidedown",
    "circleopen",
    "circleclose",
    "radial",
    "pixelize",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    #[default]
    Video,
    /// 静态图片：`out_ms` 就是显示的时长
    Image,
}

/// 这个片段开头的转场（和上一个片段混合）。第一个片段的转场没有意义，会被忽略。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Transition {
    pub kind: String,
    pub duration_ms: u64,
}

impl Default for Transition {
    fn default() -> Self {
        Transition { kind: "fade".into(), duration_ms: 500 }
    }
}

/// 区域效果：马赛克 / 模糊 / 局部调色作用在区域内（或区域以外），聚焦让画面跟着区域走。
pub const REGION_EFFECTS: &[&str] = &["mosaic", "blur", "tone", "focus"];

/// 跟着物体走的一块画面区域：位置来自追踪（素材时间 + 相对画面的矩形，方向是素材的显示方向，旋转翻转之前），
/// 效果可以作用在区域里，也可以作用在区域以外。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Region {
    pub id: u32,
    pub name: String,
    /// 轨迹；没有点的区域会被丢掉
    pub track: Vec<TrackPt>,
    /// rect / ellipse（框的内切椭圆）
    pub shape: String,
    /// 边缘羽化 0–1（占框短边的一半）
    pub feather: f64,
    /// 区域比追踪的框大多少（-0.5–2，0.2 = 各边放大 20%）
    pub grow: f64,
    /// 效果作用在区域以外
    pub invert: bool,
    /// mosaic / blur / tone / focus
    pub effect: String,
    /// 马赛克、模糊的力度 0–1
    pub strength: f64,
    /// 局部调色（含义同片段的调色）
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    /// 聚焦：放大倍数 1–4
    pub zoom: f64,
    /// 聚焦：窗口取输出画面的比例（竖屏成片里跟着横屏素材里的人走）
    pub reframe: bool,
    /// 聚焦：镜头平滑（秒），越大越稳、跟得越慢
    pub smooth: f64,
    /// 效果只在这段素材时间里生效（毫秒）；留空表示不限。聚焦不受这个限制
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}

impl Default for Region {
    fn default() -> Self {
        Region {
            id: 0,
            name: String::new(),
            track: vec![],
            shape: "rect".into(),
            feather: 0.2,
            grow: 0.0,
            invert: false,
            effect: "mosaic".into(),
            strength: 0.5,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            zoom: 2.0,
            reframe: false,
            smooth: 0.6,
            start_ms: None,
            end_ms: None,
        }
    }
}

impl Region {
    pub fn is_focus(&self) -> bool {
        self.effect == "focus"
    }
}

/// 调色参数是否有改动（亮度 / 对比度 / 饱和度不是原样）。
pub fn tone_changed(brightness: f64, contrast: f64, saturation: f64) -> bool {
    brightness.abs() > 1e-6 || (contrast - 1.0).abs() > 1e-6 || (saturation - 1.0).abs() > 1e-6
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Clip {
    /// 界面里的编号，只用来在错误提示里区分
    pub id: u32,
    pub path: String,
    pub kind: ClipKind,
    /// 取素材的哪一段（毫秒）。图片：`in_ms` 为 0，`out_ms` 是显示时长
    pub in_ms: u64,
    pub out_ms: u64,
    /// 播放速度 0.25–4
    pub speed: f64,
    /// 音量 0–4（1 = 原样）
    pub volume: f64,
    pub mute: bool,
    /// 画面淡入 / 淡出（从黑色），声音同步淡入 / 淡出
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    /// 顺时针旋转 0 / 90 / 180 / 270
    pub rotate: u16,
    pub flip_h: bool,
    pub flip_v: bool,
    /// 调色：亮度 -1–1（0 = 原样），对比度 0–3（1 = 原样），饱和度 0–3（1 = 原样）。
    /// 用 `lutyuv` 实现（精简版 ffmpeg 没有需要 GPL 的 `eq`）
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    /// 声音处理：降噪 off / light / medium / strong；人声增强。这两项都不改变片段长度
    pub denoise: String,
    pub enhance: bool,
    /// 实测的底噪电平（dBFS），降噪按它设置；没有就让 afftdn 自己跟踪
    pub noise_floor_db: Option<f64>,
    /// 倒放（整段缓冲后反着播，片段别太长）。声音也倒过来
    pub reverse: bool,
    /// 定格：只取 `in_ms` 处的一帧，停住 `out_ms - in_ms` 毫秒；没有声音。变速和倒放在定格片段上不起作用
    pub freeze: bool,
    /// 跟着物体走的区域（只用于视频片段，图片会被忽略）
    pub regions: Vec<Region>,
    pub transition: Option<Transition>,
}

impl Default for Clip {
    fn default() -> Self {
        Clip {
            id: 0,
            path: String::new(),
            kind: ClipKind::Video,
            in_ms: 0,
            out_ms: 0,
            speed: 1.0,
            volume: 1.0,
            mute: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            rotate: 0,
            flip_h: false,
            flip_v: false,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            denoise: "off".into(),
            enhance: false,
            noise_floor_db: None,
            reverse: false,
            freeze: false,
            regions: vec![],
            transition: None,
        }
    }
}

impl Clip {
    /// 在时间线上占的时长（毫秒）。
    pub fn duration_ms(&self) -> u64 {
        let span = self.out_ms.saturating_sub(self.in_ms);
        match self.kind {
            ClipKind::Image => span,
            ClipKind::Video if self.freeze => span,
            ClipKind::Video => (span as f64 / self.speed.max(0.01)).round() as u64,
        }
    }

    pub fn needs_tone(&self) -> bool {
        tone_changed(self.brightness, self.contrast, self.saturation)
    }
}

/// 叠在画面上的文字。位置是文字中心在画面里的比例（0–1）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextItem {
    pub id: u32,
    pub text: String,
    /// 时间线上的出现和消失时间
    pub start_ms: u64,
    pub end_ms: u64,
    pub x: f64,
    pub y: f64,
    /// 字号，占输出画面高度的百分比
    pub size: f64,
    /// `#RRGGBB`
    pub color: String,
    pub opacity: f64,
    pub outline: bool,
    pub outline_color: String,
    /// 文字后面的底色块
    pub boxed: bool,
    pub box_color: String,
    pub box_opacity: f64,
    /// 字体文件（ttf / otf / ttc）；留空用系统里找到的中文字体
    pub font: Option<String>,
    /// 逐字高亮用：每个词在这条文字里的出现时间（相对 `start_ms`），词接起来就是 `text`
    pub words: Vec<TextWord>,
    /// 说到哪个词，哪个词就换成这个颜色（`#RRGGBB`）；留空不高亮
    pub highlight: String,
    /// 淡入淡出的时长（毫秒），0 为没有
    pub fade_ms: u64,
    /// 关键帧：`x`、`y`、`opacity`（时间相对 `start_ms`）。有关键帧的属性不再用上面的固定值；逐字高亮的文字不支持
    pub keyframes: KeyframeMap,
}

/// 逐字高亮里的一个词。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextWord {
    pub t: String,
    /// 开始说的时刻，相对这条文字的出现时间（毫秒）
    pub a: u64,
    /// 说完的时刻
    pub b: u64,
}

impl TextItem {
    /// 要不要用逐字高亮来画（有高亮色，并且词和文字对得上）
    pub fn karaoke(&self) -> bool {
        !self.highlight.is_empty() && !self.words.is_empty()
    }
}

impl Default for TextItem {
    fn default() -> Self {
        TextItem {
            id: 0,
            text: String::new(),
            start_ms: 0,
            end_ms: 3000,
            x: 0.5,
            y: 0.85,
            size: 6.0,
            color: "#FFFFFF".into(),
            opacity: 1.0,
            outline: true,
            outline_color: "#000000".into(),
            boxed: false,
            box_color: "#000000".into(),
            box_opacity: 0.5,
            font: None,
            words: vec![],
            highlight: String::new(),
            fade_ms: 0,
            keyframes: KeyframeMap::new(),
        }
    }
}

/// 配乐、旁白、音效等：混在主轨声音上面。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioTrack {
    pub id: u32,
    pub path: String,
    /// 在时间线上从哪里开始
    pub start_ms: u64,
    /// 取素材的哪一段；`out_ms` 为空表示到素材结尾
    pub in_ms: u64,
    pub out_ms: Option<u64>,
    pub volume: f64,
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    /// 不够长时重复播放，直到视频结束
    pub looped: bool,
    /// 主轨有声音（人声）时自动把这条压低
    pub duck: bool,
}

impl Default for AudioTrack {
    fn default() -> Self {
        AudioTrack { id: 0, path: String::new(), start_ms: 0, in_ms: 0, out_ms: None, volume: 1.0, fade_in_ms: 0, fade_out_ms: 0, looped: false, duck: false }
    }
}

/// 叠加轨上的素材（画中画、贴纸、GIF、水印）：盖在主轨画面上面，位置、大小和时间都自由。
/// 轨道编号越大越在上面；同一条轨道上后开始的盖在先开始的上面。叠加素材不会撑长成片：超出主轨结尾的部分被截掉。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Overlay {
    pub id: u32,
    pub path: String,
    pub kind: ClipKind,
    /// 叠加轨编号 1–`MAX_OVERLAY_TRACKS`
    pub track: u32,
    /// 在时间线上从哪里开始（毫秒）
    pub start_ms: u64,
    /// 取素材的哪一段（毫秒）。图片：`in_ms` 为 0，`out_ms` 是显示时长
    pub in_ms: u64,
    pub out_ms: u64,
    /// 播放速度 0.25–4（视频）
    pub speed: f64,
    /// 素材不够长时重复播放（GIF、短视频）：这时 `out_ms` 可以超过素材的长度
    pub looped: bool,
    pub volume: f64,
    pub mute: bool,
    /// 淡入 / 淡出：画面变透明，声音同步
    pub fade_in_ms: u64,
    pub fade_out_ms: u64,
    /// 入场 / 出场动画：none、slide-left / slide-right / slide-up / slide-down（从画面外滑入 / 滑出）、zoom（从小变大 / 变小消失）
    pub anim_in: String,
    pub anim_out: String,
    /// 每段动画的时长
    pub anim_ms: u64,
    /// 中心点在画面里的位置（占画面宽、高的比例，0.5 = 正中）
    pub x: f64,
    pub y: f64,
    /// 素材的宽度占画面宽度的比例（1 = 和画面一样宽）
    pub scale: f64,
    /// 顺时针旋转的角度（度）
    pub rotate: f64,
    /// 不透明度 0–1
    pub opacity: f64,
    pub flip_h: bool,
    pub flip_v: bool,
    /// 调色，含义同片段
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    /// 关键帧：`x`、`y`、`scale`、`rotate`、`opacity`、`volume`（时间相对 `start_ms`）。有关键帧的属性不再用上面的固定值
    pub keyframes: KeyframeMap,
}

impl Default for Overlay {
    fn default() -> Self {
        Overlay {
            id: 0,
            path: String::new(),
            kind: ClipKind::Video,
            track: 1,
            start_ms: 0,
            in_ms: 0,
            out_ms: 0,
            speed: 1.0,
            looped: false,
            volume: 1.0,
            mute: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            anim_in: "none".into(),
            anim_out: "none".into(),
            anim_ms: 500,
            x: 0.5,
            y: 0.5,
            scale: 0.4,
            rotate: 0.0,
            opacity: 1.0,
            flip_h: false,
            flip_v: false,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            keyframes: KeyframeMap::new(),
        }
    }
}

impl Overlay {
    /// 在时间线上占的时长（毫秒）。
    pub fn duration_ms(&self) -> u64 {
        let span = self.out_ms.saturating_sub(self.in_ms);
        match self.kind {
            ClipKind::Image => span,
            ClipKind::Video => (span as f64 / self.speed.max(0.01)).round() as u64,
        }
    }

    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.duration_ms()
    }

    pub fn needs_tone(&self) -> bool {
        tone_changed(self.brightness, self.contrast, self.saturation)
    }

    /// 某个属性的关键点（整理过）；没有关键点返回 None
    pub fn keys(&self, prop: &str) -> Option<Vec<Keyframe>> {
        keyframes::track(&self.keyframes, prop)
    }

    /// 缩放的最大值：有关键帧时素材先按最大的那一档缩放，再逐帧缩小，画质才够
    pub fn scale_max(&self) -> f64 {
        self.keys("scale").map_or(self.scale, |k| k.iter().map(|p| p.value).fold(0.01, f64::max))
    }

    /// 旋转角度换算到 0–360 度
    pub fn angle(&self) -> f64 {
        self.rotate.rem_euclid(360.0)
    }
}

/// 叠加素材从下到上的顺序（下标）：轨道编号小的在下，同一轨道上开始早的在下。
pub fn overlay_order(list: &[Overlay]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..list.len()).collect();
    v.sort_by_key(|&i| (list[i].track, list[i].start_ms, i));
    v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OutputSpec {
    /// 0 = 跟随第一个视频片段（取偶数）
    pub width: u32,
    pub height: u32,
    /// 0 = 跟随第一个视频片段
    pub fps: f64,
    /// 片段和输出画面比例不同时：contain 留黑边 / cover 裁切填满 / blur 模糊背景
    pub fit: String,
    /// small / balanced / high
    pub quality: String,
    /// h264 / hevc
    pub codec: String,
    /// mp4 / mkv
    pub format: String,
}

impl Default for OutputSpec {
    fn default() -> Self {
        OutputSpec { width: 0, height: 0, fps: 0.0, fit: "contain".into(), quality: "balanced".into(), codec: "h264".into(), format: "mp4".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    /// 输出文件名（不含扩展名）；留空沿用第一个片段的名字
    pub title: String,
    pub clips: Vec<Clip>,
    /// 叠加轨上的素材（画中画、贴纸、GIF、水印）
    pub overlays: Vec<Overlay>,
    pub texts: Vec<TextItem>,
    pub audio: Vec<AudioTrack>,
    pub out: OutputSpec,
}

/// 一个片段在时间线上的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub start_ms: u64,
    pub dur_ms: u64,
    /// 开头和上一个片段重叠多少（转场时长，已经按能放下的长度收紧）
    pub overlap_ms: u64,
}

impl Placed {
    pub fn end_ms(&self) -> u64 {
        self.start_ms + self.dur_ms
    }
}

/// 排布主轨。转场时长按“放得下”收紧：不超过前后两个片段，并且一个片段开头和结尾的转场加起来不超过它自己的长度。
pub fn layout(clips: &[Clip]) -> Vec<Placed> {
    let d: Vec<u64> = clips.iter().map(Clip::duration_ms).collect();
    let mut out: Vec<Placed> = Vec::with_capacity(clips.len());
    let mut prev_overlap = 0u64;
    for (i, c) in clips.iter().enumerate() {
        let overlap = if i == 0 {
            0
        } else {
            let want = c.transition.as_ref().map_or(0, |t| t.duration_ms);
            let fit = want.min(d[i - 1].saturating_sub(prev_overlap)).min(d[i]);
            if fit < MIN_TRANSITION_MS {
                0
            } else {
                fit
            }
        };
        let start = if i == 0 { 0 } else { out[i - 1].end_ms() - overlap };
        out.push(Placed { start_ms: start, dur_ms: d[i], overlap_ms: overlap });
        prev_overlap = overlap;
    }
    out
}

/// 时间线总长（毫秒）。
pub fn total_ms(clips: &[Clip]) -> u64 {
    layout(clips).last().map_or(0, Placed::end_ms)
}

fn hex_color(s: &str, fallback: &str) -> String {
    let t = s.trim().trim_start_matches('#');
    if t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", t.to_ascii_uppercase())
    } else {
        fallback.to_string()
    }
}

fn clamp(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

/// 整理一个片段的区域：没有轨迹的丢掉，数值收紧，只留一个聚焦。
fn check_regions(regions: &mut Vec<Region>, clip_no: usize) -> Result<(), String> {
    regions.retain(|r| !r.track.is_empty());
    if regions.len() > MAX_REGIONS {
        return Err(format!("第 {clip_no} 个片段的区域太多了（最多 {MAX_REGIONS} 个）。"));
    }
    for r in regions.iter_mut() {
        if r.track.len() > MAX_TRACK_POINTS {
            return Err(format!("第 {clip_no} 个片段的区域轨迹点太多了（最多 {MAX_TRACK_POINTS} 个）。"));
        }
        r.track.retain(|p| [p.x, p.y, p.w, p.h].iter().all(|v| v.is_finite()));
        for p in &mut r.track {
            p.w = p.w.clamp(0.002, 1.0);
            p.h = p.h.clamp(0.002, 1.0);
            p.x = p.x.clamp(-1.0, 2.0);
            p.y = p.y.clamp(-1.0, 2.0);
        }
        r.track.sort_by_key(|p| p.t_ms);
        if !matches!(r.shape.as_str(), "rect" | "ellipse") {
            r.shape = "rect".into();
        }
        if !REGION_EFFECTS.contains(&r.effect.as_str()) {
            r.effect = "mosaic".into();
        }
        r.feather = clamp(r.feather, 0.0, 1.0, 0.2);
        r.grow = clamp(r.grow, -0.5, 2.0, 0.0);
        r.strength = clamp(r.strength, 0.0, 1.0, 0.5);
        r.brightness = clamp(r.brightness, -1.0, 1.0, 0.0);
        r.contrast = clamp(r.contrast, 0.0, 3.0, 1.0);
        r.saturation = clamp(r.saturation, 0.0, 3.0, 1.0);
        r.zoom = clamp(r.zoom, 1.0, 4.0, 2.0);
        r.smooth = clamp(r.smooth, 0.0, 5.0, 0.6);
        if r.end_ms.zip(r.start_ms).is_some_and(|(e, s)| e <= s) {
            r.end_ms = None;
        }
    }
    // 聚焦是改变整个镜头的取景，一个片段只能有一个：多的丢掉
    let mut seen_focus = false;
    regions.retain(|r| !r.is_focus() || !std::mem::replace(&mut seen_focus, true));
    Ok(())
}

/// 整理关键帧：只留认识的属性、值收紧到范围里、点排序去重、曲线名不对的改成线性。
fn check_keys(map: &mut KeyframeMap, allowed: &[(&str, f64, f64)]) {
    map.retain(|k, _| allowed.iter().any(|(n, _, _)| n == k));
    for (name, lo, hi) in allowed {
        let Some(points) = map.get_mut(*name) else { continue };
        let mut v = keyframes::tidy(points);
        v.truncate(MAX_KEYFRAMES);
        for k in &mut v {
            k.t_ms = k.t_ms.min(MAX_START_MS);
            k.value = k.value.clamp(*lo, *hi);
            if !matches!(k.ease.as_str(), "linear" | "in" | "out" | "inout" | "hold") {
                k.ease = "linear".into();
            }
        }
        if v.is_empty() {
            map.remove(*name);
        } else {
            *points = v;
        }
    }
}

impl Project {
    /// 校验并整理：超出范围的数值收紧，结构性的错误（没有片段、素材路径为空、起止颠倒）返回提示。
    pub fn checked(mut self) -> Result<Project, String> {
        if self.clips.is_empty() {
            return Err("时间线上还没有片段，请先添加视频或图片。".into());
        }
        if self.clips.len() > MAX_CLIPS {
            return Err(format!("片段太多了（最多 {MAX_CLIPS} 个）。"));
        }
        if self.overlays.len() > MAX_OVERLAYS {
            return Err(format!("叠加素材太多了（最多 {MAX_OVERLAYS} 个）。"));
        }
        if self.texts.len() > MAX_TEXTS {
            return Err(format!("文字太多了（最多 {MAX_TEXTS} 条）。"));
        }
        if self.audio.len() > MAX_AUDIO {
            return Err(format!("音频轨太多了（最多 {MAX_AUDIO} 条）。"));
        }
        for (i, c) in self.clips.iter_mut().enumerate() {
            let n = i + 1;
            if c.path.trim().is_empty() {
                return Err(format!("第 {n} 个片段没有素材文件。"));
            }
            match c.kind {
                ClipKind::Image => {
                    c.in_ms = 0;
                    c.speed = 1.0;
                    c.out_ms = c.out_ms.max(MIN_MS);
                }
                ClipKind::Video => {
                    if c.out_ms <= c.in_ms || c.out_ms - c.in_ms < MIN_MS {
                        return Err(format!("第 {n} 个片段的结束时间要比开始时间晚（至少 {MIN_MS} 毫秒）。"));
                    }
                    c.speed = clamp(c.speed, 0.25, 4.0, 1.0);
                }
            }
            if c.kind == ClipKind::Image {
                c.regions.clear();
            } else {
                check_regions(&mut c.regions, n)?;
            }
            if c.kind == ClipKind::Image {
                c.reverse = false;
                c.freeze = false;
            }
            c.volume = clamp(c.volume, 0.0, 4.0, 1.0);
            c.brightness = clamp(c.brightness, -1.0, 1.0, 0.0);
            c.contrast = clamp(c.contrast, 0.0, 3.0, 1.0);
            c.saturation = clamp(c.saturation, 0.0, 3.0, 1.0);
            if !matches!(c.denoise.as_str(), "off" | "light" | "medium" | "strong") {
                c.denoise = "off".into();
            }
            c.noise_floor_db = c.noise_floor_db.filter(|v| v.is_finite()).map(|v| v.clamp(-80.0, -20.0));
            if !matches!(c.rotate, 0 | 90 | 180 | 270) {
                c.rotate = (c.rotate % 360) / 90 * 90;
            }
            let d = c.duration_ms();
            c.fade_in_ms = c.fade_in_ms.min(d);
            c.fade_out_ms = c.fade_out_ms.min(d.saturating_sub(c.fade_in_ms));
            if let Some(t) = &mut c.transition {
                if !TRANSITIONS.contains(&t.kind.as_str()) {
                    t.kind = "fade".into();
                }
                if i == 0 || t.duration_ms < MIN_TRANSITION_MS {
                    c.transition = None;
                }
            }
        }
        for (i, o) in self.overlays.iter_mut().enumerate() {
            let n = i + 1;
            if o.path.trim().is_empty() {
                return Err(format!("第 {n} 个叠加素材没有素材文件。"));
            }
            match o.kind {
                ClipKind::Image => {
                    o.in_ms = 0;
                    o.speed = 1.0;
                    o.looped = false;
                    o.out_ms = o.out_ms.max(MIN_MS);
                }
                ClipKind::Video => {
                    if o.out_ms <= o.in_ms || o.out_ms - o.in_ms < MIN_MS {
                        return Err(format!("第 {n} 个叠加素材的结束时间要比开始时间晚（至少 {MIN_MS} 毫秒）。"));
                    }
                    o.speed = clamp(o.speed, 0.25, 4.0, 1.0);
                }
            }
            o.track = o.track.clamp(1, MAX_OVERLAY_TRACKS);
            o.start_ms = o.start_ms.min(MAX_START_MS);
            o.x = clamp(o.x, -1.0, 2.0, 0.5);
            o.y = clamp(o.y, -1.0, 2.0, 0.5);
            o.scale = clamp(o.scale, 0.02, 3.0, 0.4);
            o.rotate = clamp(o.rotate, -3600.0, 3600.0, 0.0);
            o.opacity = clamp(o.opacity, 0.0, 1.0, 1.0);
            o.volume = clamp(o.volume, 0.0, 4.0, 1.0);
            check_keys(
                &mut o.keyframes,
                &[("x", -1.0, 2.0), ("y", -1.0, 2.0), ("scale", 0.02, 3.0), ("rotate", -3600.0, 3600.0), ("opacity", 0.0, 1.0), ("volume", 0.0, 4.0)],
            );
            o.brightness = clamp(o.brightness, -1.0, 1.0, 0.0);
            o.contrast = clamp(o.contrast, 0.0, 3.0, 1.0);
            o.saturation = clamp(o.saturation, 0.0, 3.0, 1.0);
            let d = o.duration_ms();
            o.fade_in_ms = o.fade_in_ms.min(d);
            o.fade_out_ms = o.fade_out_ms.min(d.saturating_sub(o.fade_in_ms));
            for a in [&mut o.anim_in, &mut o.anim_out] {
                if !ANIMS.contains(&a.as_str()) {
                    *a = "none".into();
                }
            }
            o.anim_ms = o.anim_ms.clamp(MIN_MS, 3000).min((d / 2).max(MIN_MS));
        }
        self.texts.retain(|t| !t.text.trim().is_empty());
        for t in &mut self.texts {
            if t.end_ms < t.start_ms + MIN_MS {
                t.end_ms = t.start_ms + MIN_MS;
            }
            t.x = clamp(t.x, 0.0, 1.0, 0.5);
            t.y = clamp(t.y, 0.0, 1.0, 0.85);
            t.size = clamp(t.size, 1.0, 40.0, 6.0);
            t.opacity = clamp(t.opacity, 0.0, 1.0, 1.0);
            check_keys(&mut t.keyframes, &[("x", -1.0, 2.0), ("y", -1.0, 2.0), ("opacity", 0.0, 1.0)]);
            t.box_opacity = clamp(t.box_opacity, 0.0, 1.0, 0.5);
            t.color = hex_color(&t.color, "#FFFFFF");
            t.outline_color = hex_color(&t.outline_color, "#000000");
            t.box_color = hex_color(&t.box_color, "#000000");
            t.font = t.font.take().filter(|f| !f.trim().is_empty());
            t.fade_ms = t.fade_ms.min((t.end_ms - t.start_ms) / 2);
            // 词和文字对不上（文字被改过）时，逐字高亮作废，按普通文字画
            let joined: String = t.words.iter().map(|w| w.t.as_str()).collect();
            if joined != t.text || t.words.len() > 400 {
                t.words.clear();
            }
            for w in &mut t.words {
                w.b = w.b.max(w.a);
            }
            t.highlight = if t.words.is_empty() { String::new() } else { hex_color(&t.highlight, "") };
        }
        for (i, a) in self.audio.iter_mut().enumerate() {
            if a.path.trim().is_empty() {
                return Err(format!("第 {} 条音频轨没有素材文件。", i + 1));
            }
            if let Some(o) = a.out_ms {
                if o <= a.in_ms {
                    return Err(format!("第 {} 条音频轨的结束时间要比开始时间晚。", i + 1));
                }
            }
            a.volume = clamp(a.volume, 0.0, 4.0, 1.0);
        }
        let o = &mut self.out;
        if (o.width != 0 && !(16..=7680).contains(&o.width)) || (o.height != 0 && !(16..=7680).contains(&o.height)) || (o.width == 0) != (o.height == 0) {
            return Err("输出尺寸不对：宽和高都要在 16–7680 之间（或者都留空，跟随第一个片段）。".into());
        }
        o.fps = clamp(o.fps, 0.0, 120.0, 0.0);
        if o.fps != 0.0 && o.fps < 5.0 {
            return Err("输出帧率太低了（至少 5 帧；留空则跟随第一个片段）。".into());
        }
        if !matches!(o.fit.as_str(), "contain" | "cover" | "blur") {
            o.fit = "contain".into();
        }
        if !matches!(o.quality.as_str(), "small" | "balanced" | "high") {
            o.quality = "balanced".into();
        }
        if !matches!(o.codec.as_str(), "h264" | "hevc") {
            o.codec = "h264".into();
        }
        if !matches!(o.format.as_str(), "mp4" | "mkv") {
            o.format = "mp4".into();
        }
        Ok(self)
    }

    /// 所有用到的素材文件（去重，保持顺序）。
    pub fn media_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let paths = self
            .clips
            .iter()
            .map(|c| &c.path)
            .chain(self.overlays.iter().map(|o| &o.path))
            .chain(self.audio.iter().map(|a| &a.path))
            .chain(self.texts.iter().filter_map(|t| t.font.as_ref()));
        for p in paths {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: u32, dur: u64) -> Clip {
        Clip { id, path: format!("/m/{id}.mp4"), in_ms: 0, out_ms: dur, ..Default::default() }
    }

    fn with_t(mut c: Clip, ms: u64) -> Clip {
        c.transition = Some(Transition { kind: "fade".into(), duration_ms: ms });
        c
    }

    #[test]
    fn clips_follow_each_other_and_transitions_overlap() {
        let clips = vec![clip(1, 4000), with_t(clip(2, 3000), 1000), clip(3, 2000)];
        let l = layout(&clips);
        assert_eq!((l[0].start_ms, l[0].end_ms()), (0, 4000));
        assert_eq!((l[1].start_ms, l[1].end_ms(), l[1].overlap_ms), (3000, 6000, 1000));
        assert_eq!((l[2].start_ms, l[2].end_ms(), l[2].overlap_ms), (6000, 8000, 0));
        assert_eq!(total_ms(&clips), 8000);
        assert_eq!(total_ms(&[]), 0);
    }

    #[test]
    fn speed_changes_how_long_a_clip_plays() {
        let mut c = clip(1, 6000);
        c.speed = 2.0;
        assert_eq!(c.duration_ms(), 3000);
        c.speed = 0.5;
        assert_eq!(c.duration_ms(), 12000);
        // 图片的时长就是 out_ms，不受速度影响
        let img = Clip { kind: ClipKind::Image, out_ms: 2500, speed: 4.0, ..Default::default() };
        assert_eq!(img.duration_ms(), 2500);
    }

    #[test]
    fn transitions_shrink_to_what_fits() {
        // 第 2 个片段只有 1 秒，请求 3 秒的转场：缩到 1 秒；第 3 个片段开头的转场再占掉第 2 个片段剩下的部分
        let clips = vec![clip(1, 5000), with_t(clip(2, 1000), 3000), with_t(clip(3, 5000), 2000)];
        let l = layout(&clips);
        assert_eq!(l[1].overlap_ms, 1000);
        // 第 2 个片段 1 秒已经全被开头的转场占了，第 3 个片段的转场放不下，当作硬切
        assert_eq!(l[2].overlap_ms, 0);
        assert_eq!(l[2].start_ms, l[1].end_ms());
        // 一个片段开头和结尾的转场加起来不超过它自己
        let clips = vec![clip(1, 5000), with_t(clip(2, 2000), 1500), with_t(clip(3, 5000), 1500)];
        let l = layout(&clips);
        assert_eq!((l[1].overlap_ms, l[2].overlap_ms), (1500, 500));
        assert!(l[1].overlap_ms + l[2].overlap_ms <= l[1].dur_ms);
        // 第一个片段的转场没有意义；太短的转场当作没有
        let clips = vec![with_t(clip(1, 3000), 800), with_t(clip(2, 3000), 50)];
        let l = layout(&clips);
        assert_eq!((l[0].overlap_ms, l[1].overlap_ms, l[1].start_ms), (0, 0, 3000));
    }

    #[test]
    fn checked_tightens_numbers_and_rejects_structural_mistakes() {
        assert!(Project::default().checked().unwrap_err().contains("还没有片段"));
        let mut p = Project { clips: vec![clip(1, 3000)], ..Default::default() };
        p.clips[0].path.clear();
        assert!(p.clone().checked().unwrap_err().contains("第 1 个片段没有素材"));
        p.clips[0].path = "/m/1.mp4".into();
        p.clips[0].out_ms = 0;
        assert!(p.clone().checked().unwrap_err().contains("结束时间要比开始时间晚"));

        let mut p = Project { clips: vec![clip(1, 3000), with_t(clip(2, 3000), 800)], ..Default::default() };
        p.clips[0].speed = 99.0;
        p.clips[0].volume = f64::NAN;
        p.clips[0].fade_in_ms = 10_000;
        p.clips[0].fade_out_ms = 10_000;
        p.clips[0].rotate = 450;
        p.clips[1].transition.as_mut().unwrap().kind = "nonsense".into();
        p.texts = vec![
            TextItem { text: "  ".into(), ..Default::default() },
            TextItem { text: "你好".into(), start_ms: 1000, end_ms: 900, color: "red".into(), x: 7.0, size: 500.0, ..Default::default() },
        ];
        p.out.fit = "weird".into();
        p.out.format = "avi".into();
        let q = p.checked().unwrap();
        assert_eq!(q.clips[0].speed, 4.0);
        assert_eq!(q.clips[0].volume, 1.0);
        // 淡入 + 淡出不超过片段自己的长度（速度 4 倍，750 毫秒）
        assert!(q.clips[0].fade_in_ms + q.clips[0].fade_out_ms <= q.clips[0].duration_ms());
        assert_eq!(q.clips[0].rotate, 90);
        assert_eq!(q.clips[1].transition.as_ref().unwrap().kind, "fade");
        assert_eq!(q.texts.len(), 1, "空文字被丢掉");
        let t = &q.texts[0];
        assert_eq!((t.end_ms, t.color.as_str(), t.x, t.size), (1100, "#FFFFFF", 1.0, 40.0));
        assert_eq!((q.out.fit.as_str(), q.out.format.as_str()), ("contain", "mp4"));
    }

    #[test]
    fn output_size_must_be_both_or_neither() {
        let mut p = Project { clips: vec![clip(1, 3000)], ..Default::default() };
        p.out.width = 1280;
        assert!(p.clone().checked().unwrap_err().contains("输出尺寸"));
        p.out.height = 720;
        assert!(p.clone().checked().is_ok());
        p.out.fps = 2.0;
        assert!(p.checked().unwrap_err().contains("帧率"));
    }

    #[test]
    fn media_paths_are_unique_and_include_fonts_and_music() {
        let mut p = Project { clips: vec![clip(1, 3000), clip(1, 3000)], ..Default::default() };
        p.audio = vec![AudioTrack { path: "/m/a.mp3".into(), ..Default::default() }];
        p.texts = vec![TextItem { text: "x".into(), font: Some("/f/x.ttf".into()), ..Default::default() }];
        assert_eq!(p.media_paths(), vec!["/m/1.mp4", "/m/a.mp3", "/f/x.ttf"]);
    }

    #[test]
    fn projects_round_trip_through_json_with_camel_case() {
        let mut p = Project { title: "旅行".into(), clips: vec![with_t(clip(2, 3000), 400)], ..Default::default() };
        p.clips[0].fade_in_ms = 200;
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"inMs\"") && j.contains("\"fadeInMs\"") && j.contains("\"durationMs\""), "{j}");
        let back: Project = serde_json::from_str(&j).unwrap();
        assert_eq!(back, p);
        // 缺少的字段用默认值（旧版本存的工程文件也能打开）
        let min: Project = serde_json::from_str(r#"{"clips":[{"path":"/a.mp4","outMs":1000}]}"#).unwrap();
        assert_eq!((min.clips[0].speed, min.clips[0].volume, min.out.fit.as_str()), (1.0, 1.0, "contain"));
    }

    /// 前端（界面测试里导出的真实请求）发来的 JSON 能被完整读进来并通过校验。
    #[test]
    fn a_job_sent_by_the_editor_ui_is_understood() {
        use crate::media_tools::{ToolJob, ToolOp};
        let job: ToolJob = serde_json::from_str(include_str!("fixtures/frontend-job.json")).unwrap();
        assert!(job.inputs.is_empty() && job.output_dir.is_none());
        let ToolOp::Edit { project } = job.op else { panic!("应该是剪辑任务") };
        let p = project.checked().unwrap();
        assert_eq!((p.clips.len(), p.texts.len(), p.audio.len()), (3, 1, 1));
        assert_eq!(p.clips[1].transition.as_ref().map(|t| (t.kind.as_str(), t.duration_ms)), Some(("fade", 500)));
        assert_eq!((p.clips[1].in_ms, p.clips[1].out_ms), (1500, 4000));
        assert_eq!(p.audio[0].start_ms, 5000);
        assert_eq!(p.texts[0].text, "你好 Hello");
        // 1.5 + (2.5 - 0.5 转场重叠) + 5.003 = 8.503 秒，和界面显示的总长一致
        assert_eq!(total_ms(&p.clips), 8503);
        assert!((p.clips[0].volume - 1.05).abs() < 1e-9);
        assert_eq!(
            p.media_paths(),
            vec!["/media/red.webm", "/media/green.webm", "/media/sticker.png", "/media/anim.gif", "/media/blue.webm", "/media/music.ogg"]
        );
        // 叠加轨：界面导出的字段名后端都认得（轨号、开始时间、位置 / 大小 / 旋转、GIF 的循环和静音）
        assert_eq!(p.overlays.len(), 3);
        let by = |name: &str| p.overlays.iter().find(|o| o.path.ends_with(name)).unwrap();
        let (st, gif, blue) = (by("sticker.png"), by("anim.gif"), by("blue.webm"));
        assert_eq!((st.kind, st.track, st.start_ms, st.in_ms, st.out_ms), (ClipKind::Image, 1, 1000, 0, 5000));
        assert!((st.scale - 0.281).abs() < 1e-9 && (st.x - 0.5).abs() < 1e-9 && st.opacity == 1.0 && st.rotate == 0.0);
        assert_eq!((gif.kind, gif.track, gif.start_ms, gif.out_ms, gif.looped, gif.mute), (ClipKind::Video, 3, 2400, 3600, true, true));
        assert_eq!((blue.track, blue.start_ms, blue.out_ms), (2, 3000, 4000));
        assert!((blue.x - 0.64).abs() < 1e-9 && (blue.y - 0.64).abs() < 1e-9);
        // 区域：界面导出的字段名（含手动点 pin、追踪不到 lost）后端都认得；没有 regions 字段的旧片段读出来是空的
        assert_eq!(p.clips.iter().map(|c| c.regions.len()).collect::<Vec<_>>(), vec![1, 1, 0]);
        let (a, b) = (&p.clips[0].regions[0], &p.clips[1].regions[0]);
        assert_eq!((a.effect.as_str(), a.shape.as_str(), a.zoom, a.smooth, a.start_ms, a.end_ms), ("focus", "ellipse", 2.0, 0.6, Some(1500), Some(3000)));
        assert_eq!(
            a.track.iter().map(|p| (p.t_ms, p.pin)).collect::<Vec<_>>(),
            vec![(0, false), (400, false), (1000, true), (1500, false), (2000, true), (2100, false)]
        );
        assert_eq!(b.track.iter().filter(|p| p.lost).count(), 7);
        assert_eq!(b.track.iter().filter(|p| p.pin).map(|p| p.t_ms).collect::<Vec<_>>(), vec![2000]);
        assert!((a.track[2].x - 0.45).abs() < 1e-9 && (a.track[2].w - 0.15).abs() < 1e-9);
    }

    #[test]
    fn regions_are_cleaned_up_when_the_project_is_checked() {
        use crate::track::TrackPt;
        let tp = |t: u64, x: f64| TrackPt { t_ms: t, x, y: 0.5, w: 0.2, h: 0.2, ..Default::default() };
        let focus = |zoom: f64| Region { effect: "focus".into(), zoom, track: vec![tp(0, 0.1)], ..Default::default() };
        let mut c = clip(1, 3000);
        c.regions = vec![
            Region { track: vec![], ..Default::default() },
            Region {
                track: vec![tp(2000, 0.5), tp(0, f64::NAN), tp(0, 0.1), TrackPt { w: 0.0, h: 9.0, ..tp(1000, 5.0) }],
                shape: "star".into(),
                effect: "sparkle".into(),
                feather: 7.0,
                grow: -3.0,
                strength: f64::NAN,
                ..Default::default()
            },
            focus(9.0),
            focus(2.0),
            Region { track: vec![tp(0, 0.1)], start_ms: Some(500), end_ms: Some(400), brightness: 9.0, ..Default::default() },
        ];
        let p = Project { clips: vec![c], ..Default::default() }.checked().unwrap();
        let rs = &p.clips[0].regions;
        assert_eq!(rs.len(), 3, "空轨迹丢掉，多余的聚焦丢掉：{rs:?}");
        let r = &rs[0];
        assert_eq!((r.shape.as_str(), r.effect.as_str(), r.feather, r.grow, r.strength), ("rect", "mosaic", 1.0, -0.5, 0.5));
        assert_eq!(r.track.iter().map(|p| p.t_ms).collect::<Vec<_>>(), vec![0, 1000, 2000], "按时间排序，丢掉不是数字的点");
        assert_eq!((r.track[1].w, r.track[1].h, r.track[1].x), (0.002, 1.0, 2.0), "尺寸和位置收紧到合理范围");
        assert_eq!(rs[1].zoom, 4.0, "留下的是第一个聚焦");
        assert_eq!((rs[2].start_ms, rs[2].end_ms, rs[2].brightness), (Some(500), None, 1.0), "结束比开始早：当作不限结束");
        // 区域太多
        let mut c = clip(1, 3000);
        c.regions = (0..=MAX_REGIONS).map(|_| Region { track: vec![tp(0, 0.1)], ..Default::default() }).collect();
        assert!(Project { clips: vec![c], ..Default::default() }.checked().unwrap_err().contains("区域太多"));
    }

    #[test]
    fn regions_round_trip_and_old_projects_without_them_still_open() {
        let mut c = clip(1, 3000);
        c.regions = vec![Region {
            id: 4,
            name: "球".into(),
            track: vec![crate::track::TrackPt { t_ms: 100, x: 0.25, y: 0.5, w: 0.1, h: 0.2, pin: true, ..Default::default() }],
            invert: true,
            start_ms: Some(500),
            ..Default::default()
        }];
        let p = Project { clips: vec![c], ..Default::default() };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"regions\"") && j.contains("\"tMs\":100") && j.contains("\"startMs\":500") && j.contains("\"endMs\":null"), "{j}");
        assert_eq!(serde_json::from_str::<Project>(&j).unwrap(), p);
        let old: Project = serde_json::from_str(r#"{"clips":[{"path":"/a.mp4","outMs":1000}]}"#).unwrap();
        assert!(old.clips[0].regions.is_empty());
        // 界面可以只发要改的几个字段
        let r: Region = serde_json::from_str(r#"{"track":[{"tMs":0,"x":0.1,"y":0.1,"w":0.2,"h":0.2}],"effect":"blur"}"#).unwrap();
        assert_eq!((r.shape.as_str(), r.zoom, r.feather, r.start_ms), ("rect", 2.0, 0.2, None));
    }

    fn overlay(id: u32, track: u32, start: u64) -> Overlay {
        Overlay { id, path: format!("/o/{id}.mp4"), track, start_ms: start, out_ms: 2000, ..Default::default() }
    }

    #[test]
    fn overlays_are_checked_and_clamped() {
        let wild = Overlay {
            id: 1,
            path: "/o/a.png".into(),
            kind: ClipKind::Image,
            track: 99,
            start_ms: u64::MAX,
            in_ms: 777,
            out_ms: 10,
            speed: 3.0,
            looped: true,
            x: 9.0,
            y: f64::NAN,
            scale: 0.0,
            rotate: f64::INFINITY,
            opacity: 7.0,
            volume: -1.0,
            brightness: 5.0,
            contrast: 9.0,
            saturation: -2.0,
            fade_in_ms: 99_999,
            fade_out_ms: 99_999,
            ..Default::default()
        };
        let p = Project { clips: vec![clip(1, 3000)], overlays: vec![wild], ..Default::default() }.checked().unwrap();
        let o = &p.overlays[0];
        assert_eq!(
            (o.track, o.start_ms, o.in_ms, o.out_ms, o.speed, o.looped),
            (MAX_OVERLAY_TRACKS, MAX_START_MS, 0, MIN_MS, 1.0, false),
            "图片：从头开始、不变速、不循环、至少 {MIN_MS} 毫秒"
        );
        assert_eq!((o.x, o.y, o.scale, o.rotate, o.opacity, o.volume), (2.0, 0.5, 0.02, 0.0, 1.0, 0.0));
        assert_eq!((o.brightness, o.contrast, o.saturation), (1.0, 3.0, 0.0));
        assert_eq!((o.fade_in_ms, o.fade_out_ms), (MIN_MS, 0), "淡入淡出加起来不超过自己的长度");

        let mut v = overlay(2, 0, 0);
        v.out_ms = v.in_ms;
        let e = Project { clips: vec![clip(1, 3000)], overlays: vec![v], ..Default::default() }.checked().unwrap_err();
        assert!(e.contains("第 1 个叠加素材") && e.contains("结束时间"), "{e}");
        let mut v = overlay(2, 1, 0);
        v.path = " ".into();
        assert!(Project { clips: vec![clip(1, 3000)], overlays: vec![v], ..Default::default() }.checked().unwrap_err().contains("没有素材文件"));
        let many = (0..=MAX_OVERLAYS as u32).map(|i| overlay(i, 1, 0)).collect();
        assert!(Project { clips: vec![clip(1, 3000)], overlays: many, ..Default::default() }.checked().unwrap_err().contains("叠加素材太多"));
        // 轨道编号 0 当作 1
        let p = Project { clips: vec![clip(1, 3000)], overlays: vec![overlay(1, 0, 0)], ..Default::default() }.checked().unwrap();
        assert_eq!(p.overlays[0].track, 1);
    }

    #[test]
    fn overlays_stack_by_track_then_start_time() {
        let list = vec![overlay(1, 2, 0), overlay(2, 1, 500), overlay(3, 1, 0), overlay(4, 3, 0), overlay(5, 1, 0)];
        // 1 号轨：开始早的在下（3、5 同时开始，列表里靠前的在下），再 2 号轨、3 号轨
        assert_eq!(overlay_order(&list), vec![2, 4, 1, 0, 3]);
        assert!(overlay_order(&[]).is_empty());
    }

    #[test]
    fn overlay_durations_follow_speed_and_kind() {
        let mut o = overlay(1, 1, 1000);
        o.in_ms = 500;
        o.out_ms = 4500;
        assert_eq!((o.duration_ms(), o.end_ms()), (4000, 5000));
        o.speed = 2.0;
        assert_eq!((o.duration_ms(), o.end_ms()), (2000, 3000));
        o.kind = ClipKind::Image;
        o.in_ms = 0;
        o.out_ms = 2500;
        assert_eq!(o.duration_ms(), 2500, "图片不变速");
        o.rotate = -90.0;
        assert_eq!(o.angle(), 270.0);
        o.rotate = 725.0;
        assert_eq!(o.angle(), 5.0);
    }

    #[test]
    fn overlays_round_trip_and_old_projects_without_them_still_open() {
        let mut o = overlay(3, 2, 1500);
        o.rotate = 12.5;
        o.looped = true;
        let p = Project { clips: vec![clip(1, 3000)], overlays: vec![o], ..Default::default() };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"overlays\"") && j.contains("\"startMs\":1500") && j.contains("\"track\":2") && j.contains("\"looped\":true"), "{j}");
        assert_eq!(serde_json::from_str::<Project>(&j).unwrap(), p);
        let old: Project = serde_json::from_str(r#"{"clips":[{"path":"/a.mp4","outMs":1000}]}"#).unwrap();
        assert!(old.overlays.is_empty());
        // 界面可以只发要改的几个字段
        let o: Overlay = serde_json::from_str(r#"{"path":"/a.gif","outMs":2000,"startMs":300}"#).unwrap();
        assert_eq!((o.track, o.scale, o.opacity, o.x, o.y), (1, 0.4, 1.0, 0.5, 0.5));
    }

    #[test]
    fn media_paths_include_overlays_once() {
        let mut p = Project { clips: vec![clip(1, 1000)], overlays: vec![overlay(2, 1, 0), overlay(3, 2, 0)], ..Default::default() };
        p.overlays[1].path = p.overlays[0].path.clone();
        p.audio = vec![AudioTrack { path: "/m/x.mp3".into(), ..Default::default() }];
        assert_eq!(p.media_paths(), vec!["/m/1.mp4", "/o/2.mp4", "/m/x.mp3"]);
    }
    #[test]
    fn keyframes_are_cleaned_up_and_old_projects_load() {
        // 旧项目没有关键帧 / 倒放 / 定格这些字段，照样能读
        let old = r#"{"clips":[{"path":"a.mp4","outMs":2000}],"overlays":[{"path":"b.png","kind":"image","outMs":1000}]}"#;
        let p: Project = serde_json::from_str(old).unwrap();
        assert!(p.overlays[0].keyframes.is_empty() && !p.clips[0].reverse && !p.clips[0].freeze);
        let mut p = p;
        let k = |t: u64, v: f64, e: &str| Keyframe { t_ms: t, value: v, ease: e.into() };
        p.overlays[0].keyframes.insert("x".into(), vec![k(500, 9.0, "bogus"), k(0, 0.1, "linear"), k(500, 0.9, "in")]);
        p.overlays[0].keyframes.insert("nonsense".into(), vec![k(0, 1.0, "linear")]);
        p.overlays[0].keyframes.insert("opacity".into(), vec![]);
        let p = p.checked().unwrap();
        let o = &p.overlays[0];
        assert_eq!(o.keyframes.len(), 1, "只留认识的、非空的属性");
        let x = &o.keyframes["x"];
        assert_eq!((x.len(), x[0].t_ms, x[1].t_ms), (2, 0, 500));
        assert_eq!(x[1].value, 0.9, "同一时刻只留最后一个");
        // 图片没有倒放和定格
        let mut q = Project {
            clips: vec![Clip { path: "i.png".into(), kind: ClipKind::Image, out_ms: 1000, reverse: true, freeze: true, ..Default::default() }],
            ..Default::default()
        };
        q = q.checked().unwrap();
        assert!(!q.clips[0].reverse && !q.clips[0].freeze);
        // 定格的时长不受变速影响
        let f = Clip { out_ms: 3000, in_ms: 500, speed: 2.0, freeze: true, ..Default::default() };
        assert_eq!(f.duration_ms(), 2500);
    }
}
