//! 视频规整的规格、预设、分析结果，以及根据素材给出的“检测到的问题”和推荐预设。

use serde::{Deserialize, Serialize};

use super::colormatch::{normalize_skips, ShotAdjust, SkipSpan, ToneAdjust};
use super::facts::{Facts, Hdr};

/// 想把视频规整成的样子。字段都有默认值（等于“通用兼容”预设），界面只需要传改动过的。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NormSpec {
    /// 画面尺寸：keep 不改 / limit 只缩小不放大（按短边）/ fit 放进目标框补黑边 / blur 放进目标框用模糊背景补边 / fill 铺满目标框并裁掉多余部分
    pub size: String,
    /// fit / blur / fill 的目标框（横屏写法，宽 × 高）
    pub width: u32,
    pub height: u32,
    /// limit：短边的上限
    pub short_side: u32,
    /// 目标框按素材的方向翻转（竖屏素材用 1080×1920）
    pub follow_orientation: bool,
    /// 帧率：keep 不改 / auto 可变帧率转成最接近的标准固定帧率（本来就是固定的不动）/ fixed 统一成 `fps_value`
    pub fps: String,
    pub fps_value: f64,
    /// 检测到 HDR 时转成 SDR
    pub hdr: bool,
    /// 补全 / 修正色彩标记，并把高清素材的 BT.601 转成 BT.709
    pub fix_color: bool,
    /// 输出电平（色彩范围）：tv 电视范围 16–235（视频的标准，游戏美术、广播等要求）/ pc 全范围 0–255 / keep 沿用素材原来的
    pub out_range: String,
    /// 降噪：off 关闭 / light 弱 / medium 中 / strong 强 / best 高质量（很慢）
    pub denoise: String,
    /// 自动色阶强度（0 关闭，1 完全）
    pub levels: f64,
    /// 分段色彩匹配的强度（0 关闭，1 完全校正）：镜头之间偏色、亮度、对比度、饱和度不一致时，把和整体不一致的镜头单独校正到和整体一致。
    /// 这是“统一的自动校正强度”，没有单独调节的镜头都用它
    pub match_color: f64,
    /// 统一的手动调节：作用在没有单独调节的所有镜头上
    pub match_tone: ToneAdjust,
    /// 单独调节的镜头：用检测结果里镜头开头的毫秒数标识，只用自己的参数，不再用统一的
    pub match_shots: Vec<ShotAdjust>,
    /// 不参与分段色彩匹配的时间段：不统计、不校正，保持原样
    pub match_skip: Vec<SkipSpan>,
    /// `.cube` LUT 文件和强度
    pub lut: Option<String>,
    pub lut_strength: f64,
    /// 自动去黑边
    pub autocrop: bool,
    /// 音频采样率（0 不改）、声道数（0 不改，1 单声道，2 立体声）
    pub audio_rate: u32,
    pub audio_channels: u8,
    /// 响度标准化的目标 LUFS，None 不处理
    pub loudness: Option<f64>,
    /// 修正音画不同步和时间戳跳变（直播录制常见）
    pub fix_sync: bool,
    /// 输出的视频编码：keep 能不重新编码就不动，需要重新编码时用 H.264 / h264 / hevc
    pub codec: String,
    /// small / balanced / high
    pub quality: String,
}

impl Default for NormSpec {
    fn default() -> Self {
        NormSpec {
            size: "limit".into(),
            width: 1920,
            height: 1080,
            short_side: 1080,
            follow_orientation: true,
            fps: "auto".into(),
            fps_value: 30.0,
            hdr: true,
            fix_color: true,
            out_range: "tv".into(),
            denoise: "off".into(),
            levels: 0.0,
            match_color: 0.0,
            match_tone: ToneAdjust::default(),
            match_shots: vec![],
            match_skip: vec![],
            lut: None,
            lut_strength: 1.0,
            autocrop: false,
            audio_rate: 0,
            audio_channels: 0,
            loudness: None,
            fix_sync: true,
            codec: "h264".into(),
            quality: "balanced".into(),
        }
    }
}

impl NormSpec {
    /// 把各项限制在合理范围内，拒绝无法识别的取值。
    pub fn checked(mut self) -> Result<NormSpec, String> {
        let one_of = |v: &str, set: &[&str], what: &str| -> Result<(), String> {
            if set.contains(&v) {
                Ok(())
            } else {
                Err(format!("不认识的{what}：{v}"))
            }
        };
        one_of(&self.size, &["keep", "limit", "fit", "blur", "fill"], "画面尺寸方式")?;
        one_of(&self.fps, &["keep", "auto", "fixed"], "帧率方式")?;
        one_of(&self.codec, &["keep", "h264", "hevc"], "视频编码")?;
        one_of(&self.quality, &["small", "balanced", "high"], "质量档位")?;
        one_of(&self.out_range, &["tv", "pc", "keep"], "输出电平")?;
        one_of(&self.denoise, &["off", "light", "medium", "strong", "best"], "降噪档位")?;
        self.width = self.width.clamp(320, 7680) & !1;
        self.height = self.height.clamp(240, 4320) & !1;
        self.short_side = self.short_side.clamp(240, 4320) & !1;
        self.fps_value = self.fps_value.clamp(1.0, 240.0);
        self.levels = self.levels.clamp(0.0, 1.0);
        self.match_color = self.match_color.clamp(0.0, 1.0);
        self.match_tone = self.match_tone.checked();
        // 同一个镜头写了几次以后一个为准；最多 100 个
        let mut shots: Vec<ShotAdjust> = vec![];
        for mut s in std::mem::take(&mut self.match_shots) {
            s.strength = s.strength.map(|k| if k.is_finite() { k.clamp(0.0, 1.0) } else { 0.0 });
            s.tone = s.tone.checked();
            shots.retain(|o| o.id != s.id);
            shots.push(s);
        }
        shots.truncate(100);
        self.match_shots = shots;
        self.match_skip = normalize_skips(&self.match_skip);
        self.lut_strength = self.lut_strength.clamp(0.0, 1.0);
        self.lut = self.lut.take().filter(|p| !p.trim().is_empty());
        self.loudness = self.loudness.map(|l| l.clamp(-30.0, -5.0));
        if ![0, 22050, 32000, 44100, 48000, 88200, 96000].contains(&self.audio_rate) {
            return Err(format!("不支持的采样率：{}", self.audio_rate));
        }
        self.audio_channels = self.audio_channels.min(2);
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub spec: NormSpec,
}

/// 预设列表。
pub fn presets() -> Vec<Preset> {
    let base = NormSpec::default;
    vec![
        Preset {
            id: "compat",
            name: "通用兼容",
            desc: "任何设备和软件都能播放：H.264 / 8 位 / BT.709，HDR 转成 SDR，可变帧率转固定帧率，超过 1080p 的缩小到 1080p。",
            spec: base(),
        },
        Preset {
            id: "edit",
            name: "剪辑素材统一",
            desc: "导入剪辑软件前统一规格：1920×1080（竖屏 1080×1920）、固定 30 帧、48 kHz 立体声、响度 -16 LUFS，去黑边。",
            spec: NormSpec {
                size: "fit".into(),
                fps: "fixed".into(),
                fps_value: 30.0,
                autocrop: true,
                audio_rate: 48000,
                audio_channels: 2,
                loudness: Some(-16.0),
                quality: "high".into(),
                ..base()
            },
        },
        Preset {
            id: "screen",
            name: "手机录屏",
            desc: "录屏常见问题：可变帧率（剪辑软件里音画不同步）、横竖屏切换留下的黑边、旋转标记。转成固定帧率，去黑边，音频统一为 48 kHz 立体声。",
            spec: NormSpec { fps: "auto".into(), autocrop: true, audio_rate: 48000, audio_channels: 2, ..base() },
        },
        Preset {
            id: "live",
            name: "直播录制",
            desc: "FLV / TS 录制文件：修正时间戳跳变和音画不同步，转成 MP4，响度统一到 -16 LUFS。视频本身没问题时不重新编码。",
            spec: NormSpec {
                size: "keep".into(),
                fps: "auto".into(),
                fix_sync: true,
                audio_rate: 48000,
                audio_channels: 2,
                loudness: Some(-16.0),
                codec: "keep".into(),
                hdr: false,
                ..base()
            },
        },
        Preset {
            id: "platform",
            name: "平台下载成片",
            desc: "下载来的成片：HDR 转 SDR、去黑边、补全色彩信息；其余保持原样，没有需要改的就不重新编码。",
            spec: NormSpec { size: "keep".into(), fps: "keep".into(), autocrop: true, codec: "keep".into(), quality: "high".into(), ..base() },
        },
        Preset {
            id: "shorts",
            name: "竖屏短视频 1080×1920",
            desc: "抖音 / 快手 / 小红书 / 视频号：统一成 1080×1920（比例不符的用模糊背景补边）、30 帧、48 kHz 立体声、响度 -14 LUFS。",
            spec: NormSpec {
                size: "blur".into(),
                width: 1080,
                height: 1920,
                follow_orientation: false,
                fps: "fixed".into(),
                fps_value: 30.0,
                autocrop: true,
                audio_rate: 48000,
                audio_channels: 2,
                loudness: Some(-14.0),
                quality: "high".into(),
                ..base()
            },
        },
        Preset {
            id: "landscape",
            name: "横屏 1080p",
            desc: "B站 / YouTube：统一成 1920×1080（比例不符的用模糊背景补边）、固定帧率、48 kHz 立体声、响度 -14 LUFS。",
            spec: NormSpec {
                size: "blur".into(),
                follow_orientation: false,
                autocrop: true,
                audio_rate: 48000,
                audio_channels: 2,
                loudness: Some(-14.0),
                quality: "high".into(),
                ..base()
            },
        },
        Preset {
            id: "mashup",
            name: "混剪 / 多来源拼接",
            desc: "几段来源不同的画面拼在一起、各段偏色、亮度或饱和度不一致时：把和整体不一致的镜头单独校正到和整体一致，一致的镜头不动；同时去黑边、补全色彩信息。",
            spec: NormSpec { size: "keep".into(), fps: "keep".into(), autocrop: true, codec: "keep".into(), quality: "high".into(), match_color: 0.8, ..base() },
        },
        Preset {
            id: "archive",
            name: "省空间（HEVC）",
            desc: "重新编码成 HEVC，同样画质体积通常小三成以上，适合长期存放。需要 ffmpeg 有 HEVC 编码器（完整版或硬件编码）。",
            spec: NormSpec { size: "keep".into(), fps: "keep".into(), codec: "hevc".into(), ..base() },
        },
    ]
}

pub fn preset(id: &str) -> Option<NormSpec> {
    presets().into_iter().find(|p| p.id == id).map(|p| p.spec)
}

// ---------- 分析结果 ----------

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VfrInfo {
    /// 帧间隔明显不均匀
    pub variable: bool,
    pub median_fps: f64,
    pub min_fps: f64,
    pub max_fps: f64,
    pub frames: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crop {
    pub w: u32,
    pub h: u32,
    pub x: u32,
    pub y: u32,
    /// 检测时的画面尺寸（按显示方向）
    pub src_w: u32,
    pub src_h: u32,
}

impl Crop {
    /// 上、下、左、右各去掉多少像素。
    pub fn margins(&self) -> (u32, u32, u32, u32) {
        (self.y, self.src_h.saturating_sub(self.y + self.h), self.x, self.src_w.saturating_sub(self.x + self.w))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loudness {
    pub input_i: f64,
    pub input_tp: f64,
    pub input_lra: f64,
    pub input_thresh: f64,
    pub target_offset: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub vfr: Option<VfrInfo>,
    pub crop: Option<Crop>,
    pub loudness: Option<Loudness>,
    /// 分段色彩匹配要校正的镜头（只在选了分段色彩匹配时才分析）
    pub color: Option<super::colormatch::ColorPlan>,
}

// ---------- 问题与推荐 ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: &'static str,
    pub label: String,
    pub detail: String,
    /// info / warn
    pub level: &'static str,
}

fn issue(id: &'static str, level: &'static str, label: impl Into<String>, detail: impl Into<String>) -> Issue {
    Issue { id, level, label: label.into(), detail: detail.into() }
}

/// 素材里值得规整的问题。
pub fn issues(facts: &Facts, an: &Analysis) -> Vec<Issue> {
    let mut out = vec![];
    if let Some(v) = &facts.video {
        if v.hdr != Hdr::None {
            let mut d = format!("{}：在普通播放器和多数剪辑软件里会发灰、发白，转成 SDR 后颜色才正常。", v.hdr.label());
            if v.dolby_profile == Some(5) {
                d.push_str("这是杜比视界 Profile 5，没有 HDR10 兼容层，转换后颜色可能偏紫偏绿。");
            }
            out.push(issue("hdr", "warn", v.hdr.label(), d));
        }
        if let Some(f) = &an.vfr {
            if f.variable {
                out.push(issue(
                    "vfr",
                    "warn",
                    "可变帧率",
                    format!("帧率在 {:.1}–{:.1} 之间变化，导入剪辑软件常会音画不同步，转成固定帧率可解决。", f.min_fps, f.max_fps),
                ));
            }
        }
        if v.rotation != 0 {
            out.push(issue("rotation", "info", format!("旋转标记 {}°", v.rotation), "靠播放器读取旋转标记才能正确显示，处理后会把画面真正转正。"));
        }
        if let Some(c) = &an.crop {
            let (t, b, l, r) = c.margins();
            out.push(issue("black_bars", "info", "有黑边", format!("上 {t}、下 {b}、左 {l}、右 {r} 像素是黑边，可以自动裁掉。")));
        }
        if let Some((a, b)) = v.sar.filter(|(a, b)| a != b) {
            out.push(issue("sar", "info", "非方形像素", format!("像素宽高比 {a}:{b}，部分软件会把画面拉伸，处理后改为方形像素。")));
        }
        if v.bit_depth > 8 && v.hdr == Hdr::None {
            out.push(issue("depth", "info", format!("{} 位色深", v.bit_depth), "部分设备和软件无法播放 10 位视频，可转成 8 位。"));
        }
        if v.full_range() {
            out.push(issue("range", "info", "全范围色彩", "色彩范围是 0–255（JPEG 范围），多数视频软件按 16–235 处理，会造成对比度偏差。"));
        } else if v.untagged() {
            out.push(issue("untagged", "info", "缺少色彩信息", "文件没有写明色彩空间，播放器只能猜测，可能偏色。"));
        }
        if matches!(v.codec.as_str(), "hevc" | "vp9" | "av1") {
            out.push(issue("codec", "info", v.codec.to_ascii_uppercase(), "较新的编码，部分旧设备和剪辑软件无法播放或很卡，需要兼容时可转成 H.264。"));
        }
        if v.fps.is_some_and(|f| f > 60.5) {
            out.push(issue("high_fps", "info", format!("{:.0} 帧", v.fps.unwrap_or(0.0)), "高帧率素材体积大、很多设备播放吃力。"));
        }
    }
    match &facts.audio {
        Some(a) => {
            if a.sample_rate != 0 && a.sample_rate != 44100 && a.sample_rate != 48000 {
                out.push(issue("audio_rate", "info", format!("采样率 {} Hz", a.sample_rate), "不是常见的 44.1 / 48 kHz，部分软件会重采样或报错。"));
            }
            if a.channels > 2 {
                out.push(issue("audio_channels", "info", format!("{} 声道", a.channels), "多声道在手机和耳机上会被混音，音量可能偏小。"));
            }
        }
        None if facts.video.is_some() => out.push(issue("no_audio", "info", "没有音轨", "")),
        None => {}
    }
    out
}

/// 按素材的特征推荐预设。`hint` 是来源提示：`live`（直播录制）、`download`（平台下载）、`tool`（本机已处理过的）。
pub fn recommend(facts: &Facts, an: &Analysis, hint: Option<&str>) -> &'static str {
    if hint == Some("live") || (facts.is_stream_container() && hint != Some("download")) {
        return "live";
    }
    let Some(v) = &facts.video else { return "compat" };
    let vfr = an.vfr.as_ref().is_some_and(|f| f.variable);
    // 可变帧率的 MP4 / MOV：基本是手机录屏或手机拍摄
    if vfr && !facts.is_stream_container() {
        return "screen";
    }
    if hint == Some("download") {
        return "platform";
    }
    if v.hdr != Hdr::None || an.crop.is_some() {
        return "platform";
    }
    "compat"
}

#[cfg(test)]
mod tests {
    use super::super::facts::parse;
    use super::*;

    #[test]
    fn presets_are_valid_and_unique() {
        let list = presets();
        let mut ids: Vec<_> = list.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), list.len());
        for p in list {
            let checked = p.spec.clone().checked().unwrap_or_else(|e| panic!("{}: {e}", p.id));
            assert_eq!(checked, p.spec, "{} 的预设取值应该已经在合法范围内", p.id);
        }
        assert_eq!(preset("compat"), Some(NormSpec::default()));
        assert!(preset("nope").is_none());
    }

    #[test]
    fn checked_rejects_and_clamps() {
        assert!(NormSpec { size: "weird".into(), ..Default::default() }.checked().is_err());
        assert!(NormSpec { audio_rate: 12345, ..Default::default() }.checked().is_err());
        let c = NormSpec {
            width: 99999,
            height: 1,
            levels: 4.0,
            lut_strength: -1.0,
            loudness: Some(5.0),
            audio_channels: 6,
            lut: Some("  ".into()),
            ..Default::default()
        }
        .checked()
        .unwrap();
        assert_eq!((c.width, c.height, c.levels, c.lut_strength, c.loudness, c.audio_channels, c.lut), (7680, 240, 1.0, 0.0, Some(-5.0), 2, None));
    }

    #[test]
    fn range_denoise_and_shot_adjustments_are_validated() {
        assert!(NormSpec { out_range: "limited".into(), ..Default::default() }.checked().is_err());
        assert!(NormSpec { denoise: "max".into(), ..Default::default() }.checked().is_err());
        for r in ["tv", "pc", "keep"] {
            assert!(NormSpec { out_range: r.into(), ..Default::default() }.checked().is_ok());
        }
        for d in ["off", "light", "medium", "strong", "best"] {
            assert!(NormSpec { denoise: d.into(), ..Default::default() }.checked().is_ok());
        }
        // 没写的字段：电平默认 16–235（游戏、美术素材的要求），降噪默认关
        let s: NormSpec = serde_json::from_str("{}").unwrap();
        assert_eq!((s.out_range.as_str(), s.denoise.as_str()), ("tv", "off"));
        // 统一调节限制在 -1 到 1；单独调节的镜头去重（后写的生效）、强度限制在 0 到 1
        let c = NormSpec {
            match_tone: ToneAdjust { brightness: 3.0, contrast: f64::NAN, saturation: -7.0, ..Default::default() },
            match_shots: vec![
                ShotAdjust { id: 3000, strength: Some(2.0), ..Default::default() },
                ShotAdjust { id: 3000, strength: Some(0.5), tone: ToneAdjust { tint: 9.0, ..Default::default() } },
            ],
            ..Default::default()
        }
        .checked()
        .unwrap();
        assert_eq!((c.match_tone.brightness, c.match_tone.contrast, c.match_tone.saturation), (1.0, 0.0, -1.0));
        assert_eq!(c.match_shots.len(), 1, "{:?}", c.match_shots);
        assert_eq!((c.match_shots[0].strength, c.match_shots[0].tone.tint), (Some(0.5), 1.0));
        // 排除的时间段：去掉空的和反的、排序、合并重叠的
        let skip = |a: u64, b: Option<u64>| SkipSpan { start_ms: a, end_ms: b };
        let c = NormSpec {
            match_skip: vec![skip(9000, Some(9000)), skip(5000, Some(7000)), skip(1000, Some(3000)), skip(6000, Some(8000)), skip(20000, None)],
            ..Default::default()
        }
        .checked()
        .unwrap();
        assert_eq!(c.match_skip, vec![skip(1000, Some(3000)), skip(5000, Some(8000)), skip(20000, None)]);
    }

    #[test]
    fn spec_json_uses_defaults_for_missing_fields() {
        let s: NormSpec = serde_json::from_str(r#"{"size":"blur","width":1080,"height":1920,"loudness":-14}"#).unwrap();
        assert_eq!((s.size.as_str(), s.fps.as_str(), s.codec.as_str(), s.loudness), ("blur", "auto", "h264", Some(-14.0)));
    }

    const HLG: &str = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mov':\n  Stream #0:0: Video: hevc (Main 10), yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67), 3840x2160, 29.97 fps, 30 tbr, 600 tbn\n    Side data:\n      displaymatrix: rotation of -90.00 degrees\n  Stream #0:1: Audio: aac (LC), 44100 Hz, 5.1, fltp\n";

    #[test]
    fn issues_describe_the_footage() {
        let f = parse(HLG);
        let an = Analysis {
            vfr: Some(VfrInfo { variable: true, median_fps: 30.0, min_fps: 12.0, max_fps: 60.0, frames: 200 }),
            crop: Some(Crop { w: 2160, h: 3400, x: 0, y: 220, src_w: 2160, src_h: 3840 }),
            ..Default::default()
        };
        let ids: Vec<_> = issues(&f, &an).iter().map(|i| i.id).collect();
        for want in ["hdr", "vfr", "rotation", "black_bars", "codec", "audio_channels"] {
            assert!(ids.contains(&want), "{want} in {ids:?}");
        }
        let bars = issues(&f, &an).into_iter().find(|i| i.id == "black_bars").unwrap();
        assert!(bars.detail.contains("上 220") && bars.detail.contains("下 220"), "{}", bars.detail);
        // 普通的 H.264 SDR 没有问题
        let ok = parse("Input #0, mov,mp4, from 'b.mp4':\n  Stream #0:0: Video: h264 (High), yuv420p(tv, bt709, progressive), 1920x1080, 30 fps, 30 tbr, 15360 tbn\n  Stream #0:1: Audio: aac (LC), 48000 Hz, stereo, fltp\n");
        assert!(issues(&ok, &Analysis::default()).is_empty());
    }

    #[test]
    fn recommendations() {
        let phone = parse(HLG);
        let vfr = Analysis { vfr: Some(VfrInfo { variable: true, ..Default::default() }), ..Default::default() };
        assert_eq!(recommend(&phone, &vfr, None), "screen");
        assert_eq!(recommend(&phone, &Analysis::default(), None), "platform", "HDR 成片");
        assert_eq!(recommend(&phone, &Analysis::default(), Some("live")), "live");
        let flv = parse("Input #0, flv, from 'x.flv':\n  Stream #0:0: Video: h264 (High), yuv420p(progressive), 1280x720, 30 fps, 30 tbr, 1k tbn\n  Stream #0:1: Audio: aac (LC), 44100 Hz, stereo, fltp\n");
        assert_eq!(recommend(&flv, &Analysis::default(), None), "live");
        let plain = parse(
            "Input #0, mov,mp4, from 'b.mp4':\n  Stream #0:0: Video: h264 (High), yuv420p(tv, bt709, progressive), 1920x1080, 30 fps, 30 tbr, 15360 tbn\n",
        );
        assert_eq!(recommend(&plain, &Analysis::default(), None), "compat");
        assert_eq!(recommend(&plain, &Analysis::default(), Some("download")), "platform");
        assert_eq!(recommend(&Facts::default(), &Analysis::default(), None), "compat");
    }
}
