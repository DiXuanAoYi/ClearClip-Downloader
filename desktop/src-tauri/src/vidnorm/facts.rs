//! 从 `ffmpeg -i` 的输出里读出视频、音频流的细节：编码、尺寸、帧率、色彩信息、旋转、HDR 类型等。
//!
//! 管理的组件里只有 ffmpeg 没有 ffprobe，所以解析 ffmpeg 打印的流信息行：
//! `Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 1080x1920 [SAR 1:1 DAR 9:16], 5123 kb/s, 30 fps, 30 tbr, 600 tbn (default)`

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hdr {
    #[default]
    None,
    /// HDR10 / PQ（SMPTE ST 2084）
    Pq,
    /// HLG（ARIB STD-B67）
    Hlg,
}

impl Hdr {
    pub fn label(self) -> &'static str {
        match self {
            Hdr::None => "SDR",
            Hdr::Pq => "HDR10",
            Hdr::Hlg => "HLG",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFacts {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub pix_fmt: String,
    pub bit_depth: u8,
    /// tv / pc
    pub range: Option<String>,
    pub matrix: Option<String>,
    pub primaries: Option<String>,
    pub transfer: Option<String>,
    /// 平均帧率（ffmpeg 打印的 fps）
    pub fps: Option<f64>,
    /// ffmpeg 估计的基础帧率（tbr），可变帧率的视频这个数常常和 fps 差很多
    pub tbr: Option<f64>,
    /// 显示时要旋转的角度：0 / 90 / 180 / 270
    pub rotation: u16,
    /// 像素宽高比（SAR），None 表示没写
    pub sar: Option<(u32, u32)>,
    pub bitrate_kbps: Option<u32>,
    pub hdr: Hdr,
    pub dolby_vision: bool,
    /// 杜比视界 Profile（5 没有 HDR10 兼容层，转换的颜色会不对）
    pub dolby_profile: Option<u32>,
}

impl VideoFacts {
    /// 按显示方向（计算了旋转）的宽高。
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation == 90 || self.rotation == 270 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    pub fn is_portrait(&self) -> bool {
        let (w, h) = self.display_size();
        h > w
    }

    /// 色彩信息是否完全没写。
    pub fn untagged(&self) -> bool {
        self.matrix.is_none() && self.primaries.is_none() && self.transfer.is_none()
    }

    /// 全范围（JPEG 范围）色彩。
    pub fn full_range(&self) -> bool {
        self.range.as_deref() == Some("pc") || self.pix_fmt.starts_with("yuvj")
    }

    pub fn is_bt601(&self) -> bool {
        matches!(self.matrix.as_deref(), Some("bt470bg" | "smpte170m" | "fcc"))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFacts {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate_kbps: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub duration_ms: Option<u64>,
    /// 封装格式（ffmpeg 的写法），例如 `mov,mp4,m4a,3gp,3g2,mj2`、`flv`、`mpegts`
    pub container: String,
    pub video: Option<VideoFacts>,
    pub audio: Option<AudioFacts>,
}

impl Facts {
    /// 直播录制常见的封装：FLV、TS。
    pub fn is_stream_container(&self) -> bool {
        let c = &self.container;
        c == "flv" || c == "mpegts" || c.contains("mpegts")
    }
}

/// 在顶层逗号处拆分（括号、方括号里的逗号不算）。
fn split_top(s: &str) -> Vec<String> {
    let mut out = vec![];
    let (mut depth, mut cur) = (0i32, String::new());
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn num_before(piece: &str, unit: &str) -> Option<f64> {
    let p = piece.trim().strip_suffix(unit)?.trim();
    // 24k tbn、1k tbr 这类写法
    if let Some(k) = p.strip_suffix('k') {
        return k.parse::<f64>().ok().map(|v| v * 1000.0);
    }
    p.parse().ok()
}

fn bit_depth_of(pix: &str) -> u8 {
    for (suffix, d) in
        [("p10", 10), ("10le", 10), ("10be", 10), ("p12", 12), ("12le", 12), ("12be", 12), ("p16", 16), ("16le", 16), ("16be", 16), ("p9", 9), ("9le", 9)]
    {
        if pix.contains(suffix) {
            return d;
        }
    }
    8
}

fn known(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s != "unknown" && s != "unspecified").then(|| s.to_string())
}

/// 解析像素格式后面括号里的内容：`tv, bt2020nc/bt2020/smpte2084, progressive`。
fn parse_color(inner: &str, v: &mut VideoFacts) {
    for tok in inner.split(',').map(str::trim) {
        match tok {
            "tv" | "pc" => v.range = Some(tok.to_string()),
            "progressive" | "top first" | "bottom first" | "tt" | "bb" | "tb" | "bt" | "unknown" => {}
            t if t.contains('/') => {
                let mut it = t.split('/');
                v.matrix = it.next().and_then(known);
                v.primaries = it.next().and_then(known);
                v.transfer = it.next().and_then(known);
            }
            // 矩阵、原色、传输特性相同时只打印一个名字
            t => {
                if let Some(n) = known(t) {
                    v.matrix = Some(n.clone());
                    v.primaries = Some(n.clone());
                    v.transfer = Some(n);
                }
            }
        }
    }
}

fn parse_video_line(line: &str) -> Option<VideoFacts> {
    let rest = line.split_once("Video:")?.1;
    let parts = split_top(rest);
    let mut v = VideoFacts { codec: parts.first()?.split_whitespace().next()?.to_string(), ..Default::default() };
    for (i, p) in parts.iter().enumerate().skip(1) {
        if let Some((w, h)) =
            p.split_whitespace().next().and_then(|d| d.split_once('x')).and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        {
            v.width = w;
            v.height = h;
            if let Some(sar) = p.split("SAR ").nth(1).and_then(|s| s.split_whitespace().next()) {
                if let Some((a, b)) = sar.split_once(':') {
                    v.sar = Some((a.parse().unwrap_or(1), b.parse().unwrap_or(1))).filter(|(a, b)| *a > 0 && *b > 0);
                }
            }
        } else if let Some(k) = p.strip_suffix("kb/s").and_then(|s| s.trim().parse::<f64>().ok()) {
            v.bitrate_kbps = Some(k as u32);
        } else if let Some(f) = num_before(p, "fps") {
            v.fps = Some(f);
        } else if let Some(f) = num_before(p, "tbr") {
            v.tbr = Some(f);
        } else if i == 1 {
            // 像素格式：yuv420p(tv, bt709, progressive)
            let (name, inner) = match p.split_once('(') {
                Some((n, rest)) => (n.trim(), rest.trim_end_matches(')')),
                None => (p.trim(), ""),
            };
            v.pix_fmt = name.to_string();
            v.bit_depth = bit_depth_of(name);
            parse_color(inner, &mut v);
        }
    }
    v.hdr = match v.transfer.as_deref() {
        Some("smpte2084") => Hdr::Pq,
        Some("arib-std-b67") => Hdr::Hlg,
        _ => Hdr::None,
    };
    Some(v)
}

fn parse_audio_line(line: &str) -> Option<AudioFacts> {
    let rest = line.split_once("Audio:")?.1;
    let parts = split_top(rest);
    let mut a = AudioFacts { codec: parts.first()?.split_whitespace().next()?.to_string(), ..Default::default() };
    for p in parts.iter().skip(1) {
        if let Some(hz) = p.strip_suffix("Hz").and_then(|s| s.trim().parse::<u32>().ok()) {
            a.sample_rate = hz;
        } else if let Some(k) = p.contains("kb/s").then(|| p.split_whitespace().next().unwrap_or("")).and_then(|s| s.parse::<f64>().ok()) {
            a.bitrate_kbps = Some(k as u32);
        } else if a.channels == 0 {
            a.channels = channels_of(p);
        }
    }
    Some(a)
}

/// 声道布局名 → 声道数。
fn channels_of(layout: &str) -> u8 {
    let l = layout.trim();
    match l {
        "mono" => 1,
        "stereo" => 2,
        "2.1" => 3,
        "quad" | "4.0" => 4,
        "5.0" | "5.0(side)" => 5,
        "5.1" | "5.1(side)" => 6,
        "6.1" => 7,
        "7.1" => 8,
        _ => l.split_whitespace().next().and_then(|n| n.parse().ok()).filter(|_| l.contains("channel")).unwrap_or(0),
    }
}

fn parse_duration(line: &str) -> Option<u64> {
    let t = line.trim().strip_prefix("Duration:")?.trim().split(',').next()?.trim();
    if t == "N/A" {
        return None;
    }
    let mut it = t.split(':');
    let (h, m, s): (f64, f64, f64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    Some(((h * 3600.0 + m * 60.0 + s) * 1000.0) as u64)
}

/// 解析 `ffmpeg -i file` 的输出。
pub fn parse(stderr: &str) -> Facts {
    let mut f = Facts::default();
    let lines: Vec<&str> = stderr.lines().collect();
    // 当前所在的流：用来把后面的 Side data / rotate 行归到它
    let mut in_video = false;
    for line in &lines {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Input #0,") {
            f.container = rest.split(", from").next().unwrap_or("").trim().to_string();
        } else if t.starts_with("Duration:") {
            f.duration_ms = parse_duration(t);
        } else if t.starts_with("Stream #") {
            in_video = false;
            if t.contains(": Video:") && f.video.is_none() && !t.contains("attached pic") {
                f.video = parse_video_line(t);
                in_video = f.video.is_some();
            } else if t.contains(": Audio:") && f.audio.is_none() {
                f.audio = parse_audio_line(t);
            }
        } else if in_video {
            let rot = if let Some(r) = t.split("rotation of ").nth(1) {
                r.split_whitespace().next().and_then(|n| n.parse::<f64>().ok())
            } else if t.starts_with("rotate") {
                t.split_once(':').and_then(|(_, v)| v.trim().parse::<f64>().ok())
            } else {
                None
            };
            if let (Some(deg), Some(v)) = (rot, f.video.as_mut()) {
                v.rotation = (((deg.round() as i32 % 360) + 360) % 360) as u16;
            }
            if t.starts_with("DOVI configuration record") {
                if let Some(v) = f.video.as_mut() {
                    v.dolby_vision = true;
                    v.dolby_profile = t.split("profile: ").nth(1).and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next()).and_then(|s| s.parse().ok());
                }
            }
        }
    }
    f
}

/// 读取文件的详细信息（运行一次 `ffmpeg -i`）。
pub async fn read(ffmpeg: &std::path::Path, file: &std::path::Path) -> Option<Facts> {
    use std::process::Stdio;
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-i"]).arg(file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output()).await.ok()?.ok()?;
    Some(parse(&String::from_utf8_lossy(&out.stderr)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mov':\n  Metadata:\n    major_brand     : qt  \n  Duration: 00:01:23.45, start: 0.000000, bitrate: 9000 kb/s\n  Stream #0:0[0x1](und): Video: hevc (Main 10) (hvc1 / 0x31637668), yuv420p10le(tv, bt2020nc/bt2020/arib-std-b67), 3840x2160, 8800 kb/s, 29.97 fps, 30 tbr, 600 tbn (default)\n    Metadata:\n      handler_name    : Core Media Video\n    Side data:\n      displaymatrix: rotation of -90.00 degrees\n  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 123 kb/s (default)\n";

    #[test]
    fn parses_hlg_phone_video() {
        let f = parse(PHONE);
        assert_eq!(f.container, "mov,mp4,m4a,3gp,3g2,mj2");
        assert_eq!(f.duration_ms, Some(83_450));
        let v = f.video.unwrap();
        assert_eq!((v.codec.as_str(), v.width, v.height, v.pix_fmt.as_str(), v.bit_depth), ("hevc", 3840, 2160, "yuv420p10le", 10));
        assert_eq!(
            (v.range.as_deref(), v.matrix.as_deref(), v.primaries.as_deref(), v.transfer.as_deref()),
            (Some("tv"), Some("bt2020nc"), Some("bt2020"), Some("arib-std-b67"))
        );
        assert_eq!(v.hdr, Hdr::Hlg);
        assert_eq!((v.fps, v.tbr, v.bitrate_kbps), (Some(29.97), Some(30.0), Some(8800)));
        assert_eq!(v.rotation, 270, "rotation of -90 → 270");
        assert_eq!(v.display_size(), (2160, 3840));
        assert!(v.is_portrait());
        let a = f.audio.unwrap();
        assert_eq!((a.codec.as_str(), a.sample_rate, a.channels, a.bitrate_kbps), ("aac", 44100, 2, Some(123)));
    }

    #[test]
    fn parses_plain_and_sd_videos() {
        let plain = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  Duration: 00:00:02.00, start: 0.000000, bitrate: 2154 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(progressive), 320x240 [SAR 1:1 DAR 4:3], 2050 kb/s, 25 fps, 25 tbr, 12800 tbn (default)\n  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, mono, fltp, 69 kb/s (default)\n";
        let f = parse(plain);
        let v = f.video.unwrap();
        assert!(v.untagged() && v.range.is_none());
        assert_eq!((v.sar, v.hdr, v.rotation, v.bit_depth), (Some((1, 1)), Hdr::None, 0, 8));
        assert_eq!(f.audio.unwrap().channels, 1);

        let sd = "Input #0, mpegts, from 'b.ts':\n  Duration: N/A, start: 1.4, bitrate: N/A\n  Stream #0:0[0x100]: Video: mpeg2video (Main), yuv420p(tv, bt470bg/bt470bg/smpte170m, top first), 720x576 [SAR 16:15 DAR 4:3], 25 fps, 25 tbr, 90k tbn\n  Stream #0:1[0x101]: Audio: mp2 ([3][0][0][0] / 0x0003), 48000 Hz, 5.1(side), fltp, 384 kb/s\n";
        let f = parse(sd);
        assert!(f.is_stream_container() && f.duration_ms.is_none());
        let v = f.video.unwrap();
        assert!(v.is_bt601() && !v.untagged());
        assert_eq!((v.sar, v.tbr), (Some((16, 15)), Some(25.0)));
        assert_eq!(f.audio.unwrap().channels, 6);
    }

    #[test]
    fn parses_pq_full_range_and_dolby_vision() {
        let pq = "Input #0, matroska,webm, from 'c.mkv':\n  Stream #0:0: Video: hevc (Main 10), yuv420p10le(tv, bt2020nc/bt2020/smpte2084), 1920x1080, 24 fps, 24 tbr, 1k tbn\n    Side data:\n      DOVI configuration record: version: 1.0, profile: 8, level: 6, rpu flag: 1, el flag: 0, bl flag: 1, compatibility id: 1\n";
        let v = parse(pq).video.unwrap();
        assert_eq!(v.hdr, Hdr::Pq);
        assert!(v.dolby_vision);
        assert_eq!(v.dolby_profile, Some(8));
        let jpeg = "Stream #0:0: Video: mjpeg (Baseline), yuvj420p(pc, bt470bg/unknown/unknown), 640x480 [SAR 1:1 DAR 4:3], 30 fps, 30 tbr, 1200k tbn\n";
        let v = parse(jpeg).video.unwrap();
        assert!(v.full_range() && v.is_bt601());
        assert_eq!(v.primaries, None);
    }

    #[test]
    fn old_style_rotate_tag_and_cover_art() {
        let s = "Input #0, mov,mp4, from 'd.mp4':\n  Stream #0:0: Video: h264 (High), yuv420p, 1920x1080, 30 fps, 30 tbr, 90k tbn\n    Metadata:\n      rotate          : 90\n  Stream #0:1: Audio: aac (LC), 44100 Hz, stereo, fltp\n  Stream #0:2: Video: mjpeg, yuvj420p, 500x500 (attached pic)\n";
        let f = parse(s);
        let v = f.video.unwrap();
        assert_eq!((v.rotation, v.width), (90, 1920), "封面图不算视频轨");
        assert_eq!(f.audio.unwrap().channels, 2);
    }

    #[test]
    fn garbage_gives_empty_facts() {
        let f = parse("Error opening input: Invalid data found when processing input\n");
        assert!(f.video.is_none() && f.audio.is_none() && f.container.is_empty());
    }
}
