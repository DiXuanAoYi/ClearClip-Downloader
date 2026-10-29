//! ffmpeg 能力检测：版本、授权（精简版 / 完整版）、可用的滤镜和编码器。
//!
//! 管理的 ffmpeg 默认是 LGPL 精简版（没有 x264、vidstab，滤镜也可能不全），用户也可能装了完整版或系统自带的版本。
//! 视频规整、防抖等功能按这里检测到的能力选择做法，并在做不到时说明原因。
//! 硬件编码器（NVENC、QuickSync、AMF、VideoToolbox）在列表里出现不代表真的能用，要真实试编码一次才算可用。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::media_tools::Encoders;
use crate::postprocess::run_ffmpeg_capture;

#[derive(Debug, Clone, Default)]
pub struct Caps {
    /// 版本号文字，例如 `6.1.1`、`N-118222-gabc`
    pub version: String,
    /// 解析出的主、次版本号；开发版（N-xxxxx）解析不出来时为 0，按“很新”处理
    pub major: u32,
    pub minor: u32,
    /// 编译时启用了 GPL（完整版）
    pub gpl: bool,
    pub filters: BTreeSet<String>,
    pub encoders: Encoders,
    /// 可用的 HEVC 编码器
    pub hevc: Option<&'static str>,
    /// 试编码通过的硬件 H.264 编码器
    pub hw: Vec<&'static str>,
}

impl Caps {
    pub fn has_filter(&self, name: &str) -> bool {
        self.filters.contains(name)
    }

    /// “模糊背景”用的模糊滤镜。`boxblur` 是 GPL 滤镜，精简版（LGPL）ffmpeg 里没有，改用 `gblur`
    /// （σ≈30 和 `boxblur=25:5` 的模糊程度接近）；两个都没探测到时保持 `boxblur`，让 ffmpeg 自己报错。
    pub fn background_blur(&self) -> &'static str {
        if !self.has_filter("boxblur") && self.has_filter("gblur") {
            "gblur=sigma=30"
        } else {
            "boxblur=25:5"
        }
    }

    /// 要求去黑边、但 ffmpeg 没有 `cropdetect` 时的说明。`cropdetect` 是 GPL 滤镜，精简版里没有，
    /// 这时检测不了黑边，去黑边会被跳过（不会让任务失败）。
    pub fn autocrop_warning(&self) -> Option<String> {
        let hint = if self.gpl { "" } else { "在“设置 → 组件”里安装完整版 ffmpeg 后可以使用。" };
        (!self.has_filter("cropdetect")).then(|| format!("没有 cropdetect 滤镜，检测不了黑边，已跳过去黑边。{hint}"))
    }

    /// `-fps_mode` 从 5.1 开始提供，更早的版本只能用 `-vsync`。
    pub fn has_fps_mode(&self) -> bool {
        self.major == 0 || (self.major, self.minor) >= (5, 1)
    }

    /// 能做 HDR → SDR 的高质量色调映射（zscale + tonemap）。
    pub fn can_tonemap(&self) -> bool {
        self.has_filter("zscale") && self.has_filter("tonemap")
    }

    pub fn vidstab(&self) -> bool {
        self.has_filter("vidstabdetect") && self.has_filter("vidstabtransform")
    }
}

/// 给界面看的摘要。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapsSummary {
    pub version: String,
    /// full（完整版）/ lite（精简版）
    pub edition: &'static str,
    pub h264: Option<String>,
    pub hevc: Option<String>,
    pub hardware: Vec<String>,
    pub tonemap: bool,
    pub lut: bool,
    pub levels: bool,
    pub crop_detect: bool,
    pub stabilize: &'static str,
    /// 做不到的事和怎么解决
    pub notes: Vec<String>,
}

impl Caps {
    pub fn summary(&self) -> CapsSummary {
        let mut notes = vec![];
        let h264 = self.encoders.h264;
        match h264 {
            Some("libx264") => {}
            Some(hw) if hw != "libopenh264" => notes.push(format!("没有 x264，视频编码使用硬件编码器 {hw}，同样码率下画质略低于 x264。")),
            Some(_) => notes.push("没有 x264，视频编码使用 openh264，画质和速度都不如 x264。在“设置 → 组件”里安装完整版 ffmpeg 可以解决。".into()),
            None => notes.push("没有 H.264 编码器，输出会退回到 VP9 或 MPEG-4。在“设置 → 组件”里安装完整版 ffmpeg 可以解决。".into()),
        }
        if !self.can_tonemap() {
            notes.push("没有 zscale / tonemap 滤镜，HDR 转 SDR 会使用程序内置的色调映射（3D LUT），效果接近但不如 zscale。".into());
        }
        if !self.vidstab() {
            let hint = if self.gpl { "" } else { "安装完整版 ffmpeg 后可以使用 vidstab 两遍防抖。" };
            notes.push(if self.has_filter("deshake") {
                format!("没有 vidstab，防抖会使用 deshake，效果较弱。{hint}")
            } else {
                format!("没有防抖滤镜（vidstab / deshake），无法防抖。{hint}")
            });
        }
        if !self.has_filter("lut3d") {
            notes.push("没有 lut3d 滤镜，无法应用 LUT。".into());
        }
        if !self.has_filter("normalize") {
            notes.push("没有 normalize 滤镜，无法自动色阶。".into());
        }
        notes.extend(self.autocrop_warning());
        CapsSummary {
            version: self.version.clone(),
            edition: if self.gpl { "full" } else { "lite" },
            h264: h264.map(String::from),
            hevc: self.hevc.map(String::from),
            hardware: self.hw.iter().map(|s| s.to_string()).collect(),
            tonemap: self.can_tonemap(),
            lut: self.has_filter("lut3d"),
            levels: self.has_filter("normalize"),
            crop_detect: self.has_filter("cropdetect"),
            stabilize: if self.vidstab() {
                "vidstab"
            } else if self.has_filter("deshake") {
                "deshake"
            } else {
                "none"
            },
            notes,
        }
    }
}

// ---------- 解析 ----------

/// `ffmpeg -version` 的第一行 → (版本文字, 主版本, 次版本)。
pub fn parse_version(first_line: &str) -> (String, u32, u32) {
    let v = first_line.split_whitespace().skip_while(|w| *w != "version").nth(1).unwrap_or("").to_string();
    let trimmed = v.trim_start_matches(['n', 'N']);
    let mut nums = trimmed.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
    // 开发版形如 N-118222-gabcdef：开头不是版本号
    let (major, minor) = if v.starts_with("N-") || v.starts_with("git-") {
        (0, 0)
    } else {
        (nums.next().and_then(|s| s.parse().ok()).unwrap_or(0), nums.next().and_then(|s| s.parse().ok()).unwrap_or(0))
    };
    (v, major, minor)
}

/// 解析 `ffmpeg -filters`。每行形如 ` TSC lut3d  V->V  说明`：先是标志列，再是滤镜名，再是输入输出类型（`V->V`、`N->A`……）。
/// 标志列旧版是 3 个字符（`T..`、`TSC`），新版（2026 年的开发版）只剩 2 个字符（`TS`、`..`），所以不能写死长度，
/// 改用“第三列带 `->`”来认行，图例行（`T.. = Timeline support`）的第三列没有 `->`，不会混进来。
pub fn parse_filters(out: &str) -> BTreeSet<String> {
    out.lines()
        .filter(|l| l.starts_with(' '))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let flags = it.next()?;
            let name = it.next()?;
            let io = it.next()?;
            ((1..=4).contains(&flags.len())
                && flags.chars().all(|c| matches!(c, '.' | 'T' | 'S' | 'C'))
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && io.contains("->"))
            .then(|| name.to_string())
        })
        .collect()
}

fn has_encoder(listing: &str, name: &str) -> bool {
    listing.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

/// 各系统上值得试一试的硬件编码器，按优先顺序。
fn hw_candidates(os: &str) -> &'static [&'static str] {
    match os {
        "macos" => &["h264_videotoolbox"],
        "windows" => &["h264_nvenc", "h264_qsv", "h264_amf"],
        _ => &["h264_nvenc", "h264_qsv"],
    }
}

fn hevc_candidates(os: &str) -> &'static [&'static str] {
    match os {
        "macos" => &["libx265", "hevc_videotoolbox"],
        "windows" => &["libx265", "hevc_nvenc", "hevc_qsv", "hevc_amf"],
        _ => &["libx265", "hevc_nvenc", "hevc_qsv"],
    }
}

/// 用 0.3 秒的纯色画面试编码，成功才算能用。
async fn works(ffmpeg: &Path, enc: &str) -> bool {
    let args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "color=c=gray:s=640x360:r=30:d=0.3",
        "-frames:v",
        "6",
        "-c:v",
        enc,
        "-pix_fmt",
        "yuv420p",
        "-f",
        "null",
        "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    run_ffmpeg_capture(ffmpeg, &args, Duration::from_secs(20)).await.is_ok()
}

/// 检测一个 ffmpeg 的能力。运行不了时返回空能力（调用方会在真正运行时得到明确的错误）。
pub async fn detect(ffmpeg: &Path) -> Caps {
    let run = |a: &[&str]| {
        let args: Vec<String> = ["-hide_banner"].iter().chain(a.iter()).map(|s| s.to_string()).collect();
        async move {
            run_ffmpeg_capture(ffmpeg, &args, Duration::from_secs(20)).await.map(|(o, e)| format!("{}{}", String::from_utf8_lossy(&o), e)).unwrap_or_default()
        }
    };
    let (ver, filters, encs) = tokio::join!(run(&["-version"]), run(&["-filters"]), run(&["-encoders"]));
    let (version, major, minor) = parse_version(ver.lines().next().unwrap_or(""));
    let gpl = ver.lines().any(|l| l.starts_with("configuration:") && l.split_whitespace().any(|w| w == "--enable-gpl"));
    let mut encoders = crate::media_tools::parse_encoders(&encs);
    let os = std::env::consts::OS;

    // 硬件编码器：逐个试，并发进行
    let hw_names: Vec<&'static str> = hw_candidates(os).iter().copied().filter(|n| has_encoder(&encs, n)).collect();
    let hevc_names: Vec<&'static str> = hevc_candidates(os).iter().copied().filter(|n| has_encoder(&encs, n)).collect();
    let (hw_ok, hevc_ok) = tokio::join!(
        futures_util::future::join_all(hw_names.iter().map(|n| works(ffmpeg, n))),
        futures_util::future::join_all(hevc_names.iter().map(|n| works(ffmpeg, n)))
    );
    let hw: Vec<&'static str> = hw_names.into_iter().zip(hw_ok).filter(|(_, ok)| *ok).map(|(n, _)| n).collect();
    let hevc = hevc_names.into_iter().zip(hevc_ok).find(|(_, ok)| *ok).map(|(n, _)| n);
    encoders = encoders.with_hardware(hw.first().copied());
    Caps { version, major, minor, gpl, filters: parse_filters(&filters), encoders, hevc, hw }
}

// ---------- 缓存 ----------

/// 按 ffmpeg 文件（路径、修改时间、大小）缓存检测结果，换了 ffmpeg 会自动重新检测。
#[derive(Default)]
pub struct CapsCache {
    inner: Mutex<Option<(String, Arc<Caps>)>>,
}

fn key_of(p: &Path) -> String {
    let meta = std::fs::metadata(p).ok();
    let mtime = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}|{}|{}", p.display(), mtime, meta.map(|m| m.len()).unwrap_or(0))
}

impl CapsCache {
    pub async fn get(&self, ffmpeg: &Path) -> Arc<Caps> {
        let key = key_of(ffmpeg);
        if let Some((k, c)) = self.inner.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            if *k == key {
                return c.clone();
            }
        }
        let caps = Arc::new(detect(ffmpeg).await);
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, caps.clone()));
        caps
    }

    pub fn clear(&self) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// 当前使用的 ffmpeg 的能力。
pub async fn current(st: &crate::AppState) -> Option<(PathBuf, Arc<Caps>)> {
    let ff = crate::postprocess::find_ffmpeg(st)?;
    let caps = st.vcaps.get(&ff).await;
    Some((ff, caps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blurred_backgrounds_fall_back_to_gblur_when_boxblur_is_missing() {
        let with = |names: &[&str]| Caps { filters: names.iter().map(|n| n.to_string()).collect(), ..Default::default() };
        assert_eq!(with(&["boxblur", "gblur"]).background_blur(), "boxblur=25:5");
        assert_eq!(with(&["gblur", "avgblur"]).background_blur(), "gblur=sigma=30", "精简版 ffmpeg 没有 boxblur");
        assert_eq!(with(&[]).background_blur(), "boxblur=25:5", "没探测到任何滤镜时保持原来的写法");
    }

    #[test]
    fn autocrop_needs_cropdetect() {
        let with = |names: &[&str], gpl| Caps { gpl, filters: names.iter().map(|n| n.to_string()).collect(), ..Default::default() };
        assert_eq!(with(&["cropdetect"], false).autocrop_warning(), None);
        let lite = with(&["scale"], false).autocrop_warning().unwrap();
        assert!(lite.contains("cropdetect") && lite.contains("安装完整版"), "{lite}");
        let full = with(&["scale"], true).autocrop_warning().unwrap();
        assert!(!full.contains("安装完整版"), "完整版没有它时不再劝人安装完整版：{full}");
        assert!(with(&["scale"], false).summary().notes.iter().any(|n| n.contains("cropdetect")), "界面上的限制清单里要有");
        assert!(!with(&["cropdetect"], false).summary().notes.iter().any(|n| n.contains("cropdetect")));
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("ffmpeg version 6.1.1-3ubuntu5 Copyright (c) 2000-2023"), ("6.1.1-3ubuntu5".into(), 6, 1));
        assert_eq!(parse_version("ffmpeg version n7.1-30-gabc-20250101 Copyright"), ("n7.1-30-gabc-20250101".into(), 7, 1));
        let (v, a, b) = parse_version("ffmpeg version N-118222-g1234567-20250101 Copyright (c)");
        assert_eq!((v.as_str(), a, b), ("N-118222-g1234567-20250101", 0, 0));
        assert_eq!(parse_version(""), (String::new(), 0, 0));
        let c = Caps { major: 4, minor: 4, ..Default::default() };
        assert!(!c.has_fps_mode());
        assert!(Caps { major: 5, minor: 1, ..Default::default() }.has_fps_mode());
        assert!(Caps { major: 0, ..Default::default() }.has_fps_mode(), "开发版按新版本处理");
    }

    #[test]
    fn filter_listing() {
        let out = "Filters:\n  T.. = Timeline support\n  .S. = Slice threading\n  ..C = Command support\n  A = Audio input/output\n  V = Video input/output\n  N = Dynamic number and/or type of input/output\n  | = Source or sink filter\n ... aresample         A->A       Resample audio data.\n .S. tonemap           V->V       Conversion to/from different dynamic ranges.\n TSC lut3d             V->V       Adjust colors using a 3D LUT.\n ..C zscale            V->V       Apply resizing.\n";
        let f = parse_filters(out);
        for n in ["aresample", "tonemap", "lut3d", "zscale"] {
            assert!(f.contains(n), "{n}");
        }
        assert!(!f.contains("Timeline") && !f.contains("="), "{f:?}");
    }

    /// ffmpeg 开发版（2026）把标志列从 3 个字符改成了 2 个，没有“Command support”那一列。
    #[test]
    fn filter_listing_with_two_flag_columns() {
        let out = "Filters:\n  T.. = Timeline support\n  .S. = Slice threading\n  A = Audio input/output\n  V = Video input/output\n  N = Dynamic number and/or type of input/output\n  | = Source or sink filter\n  ------\n TS aap               AA->A      Apply Affine Projection algorithm to first audio stream.\n .. abench            A->A       Benchmark part of a filtergraph.\n .S acrossover        A->N       Split audio into per-bands streams.\n T. acrusher          A->A       Reduce audio bit resolution.\n .. anullsrc          |->A       Null audio source, return empty audio frames.\n .. nullsink          V->|       Do absolutely nothing with the input video.\n T. lut3d             V->V       Adjust colors using a 3D LUT.\n .. zscale            V->V       Apply resizing, colorspace and bit depth conversion.\n";
        let f = parse_filters(out);
        for n in ["aap", "abench", "acrossover", "acrusher", "anullsrc", "nullsink", "lut3d", "zscale"] {
            assert!(f.contains(n), "{n} {f:?}");
        }
        assert_eq!(f.len(), 8, "{f:?}");
        // 带 \r\n 的输出（Windows）也一样
        assert_eq!(parse_filters(&out.replace('\n', "\r\n")), f);
    }

    #[test]
    fn summary_explains_limits() {
        let lite = Caps { encoders: Encoders { h264: None, ..Default::default() }, ..Default::default() };
        let s = lite.summary();
        assert_eq!(s.edition, "lite");
        assert_eq!(s.stabilize, "none");
        assert!(s.notes.iter().any(|n| n.contains("H.264")) && s.notes.iter().any(|n| n.contains("zscale")));
        let full = Caps {
            gpl: true,
            encoders: Encoders { h264: Some("libx264"), ..Default::default() },
            filters: ["zscale", "tonemap", "lut3d", "normalize", "cropdetect", "vidstabdetect", "vidstabtransform"].iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        let s = full.summary();
        assert!(s.notes.is_empty(), "{:?}", s.notes);
        assert_eq!((s.edition, s.stabilize, s.tonemap), ("full", "vidstab", true));
        let hw = Caps { encoders: Encoders { h264: Some("h264_nvenc"), ..Default::default() }, ..full.clone() };
        assert!(hw.summary().notes.iter().any(|n| n.contains("h264_nvenc")));
    }

    #[tokio::test]
    async fn detects_the_local_ffmpeg() {
        let Some(ff) = std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
        else {
            return;
        };
        let c = detect(&ff).await;
        assert!(c.has_filter("scale") && c.has_filter("fps"), "{:?}", c.filters.len());
        // 这些滤镜每个构建都有；解析不出来说明列表格式变了
        assert!(c.has_filter("lut3d") && c.has_filter("loudnorm"), "{:?}", c.filters.len());
        // cropdetect 和 boxblur 是 GPL 滤镜，精简版里没有：以 `ffmpeg -filters` 的实际输出为准，不能假定它们存在
        let listing = String::from_utf8_lossy(&std::process::Command::new("ffmpeg").args(["-hide_banner", "-filters"]).output().unwrap().stdout).into_owned();
        for name in ["cropdetect", "boxblur"] {
            let listed = listing.lines().any(|l| l.split_whitespace().nth(1) == Some(name));
            assert_eq!(c.has_filter(name), listed, "{name}");
        }
        assert!(c.filters.len() > 100, "{}", c.filters.len());
        assert!(!c.version.is_empty());
    }
}
