//! 分段色彩匹配：一个视频里各个镜头来自不同的来源（混剪、拼接、直播切换机位）时，色偏、亮度、对比度、饱和度会不一致。
//! 这里找出和“整体”不一致的镜头，只对这些镜头单独校正，让它们和整体一致；一致的镜头原样不动。
//!
//! 做法：
//! 1. 抽样：只解码关键帧（很快），缩成 64×36 的 YUV 小图；关键帧太稀疏时改为按时间间隔抽样。
//! 2. 分镜头：相邻两个样本画面差别很大、或者色调突然跳变的地方当作镜头分界。
//! 3. 每个镜头取亮度、对比度、饱和度、偏色（用中等亮度像素的色度“中间一半的平均值”估算，少数鲜艳的物体影响不大）的中位数；
//!    “整体”是所有镜头按时长加权的中位数。
//! 4. 偏离整体的幅度超过正常波动范围的镜头才算“不一致”，按强度把它拉向整体：
//!    亮度、对比度调 Y，偏色平移 U / V，饱和度缩放 U / V；用带时间窗口的 `lutyuv` 滤镜只作用在这个镜头上。
//! 5. 只对需要校正的镜头，用场景检测把分界时间精确到帧。
//!
//! 6. 手动调节：每个镜头有“自动校正强度 + 亮度 / 对比度 / 饱和度 / 色温 / 色调”两部分参数。默认所有镜头用同一组“统一参数”
//!    （统一的自动校正强度、统一的手动调节）；检测出的不一致镜头可以单独调节，单独调节的镜头只用自己的参数，不再用统一参数。
//!    手动调节同样换成 Y / U / V 三条曲线，用 `lutyuv` 做；统一调节用“排除窗口”避开单独调节的镜头。
//!
//! 限制：画面内容本来就不同色调的镜头（蓝天和绿草地）可能被误判，所以只处理偏离超出正常范围的，强度可调，也可以单独调节或关掉某个镜头；
//! 没有硬切、渐变过渡的色调变化，以及比关键帧间隔还短的镜头，检测不到。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::postprocess::run_ffmpeg_capture;
use crate::vidcaps::Caps;

use super::analyze::parse_showinfo;
use super::facts::{Facts, Hdr};

pub const W: usize = 64;
pub const H: usize = 36;
const FRAME_BYTES: usize = W * H * 3;

// ---------- 单帧统计 ----------

/// 一帧画面的色彩统计。`cu` / `cv` 是 U / V 平均值减去 128（以 0 为中性）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct FrameStats {
    /// 亮度平均值，以及 5% / 95% 分位（它们的差代表对比度）
    pub y: f64,
    pub lo: f64,
    pub hi: f64,
    pub cu: f64,
    pub cv: f64,
    /// 饱和度：色度偏离偏色中心的平均距离（整体偏色不算在内）
    pub sat: f64,
    /// 偏色：中等亮度像素里、中间一半的 U / V 平均值（以 0 为中性）。不受画面里少数鲜艳物体的影响，也不依赖“灰色”的范围
    pub nu: f64,
    pub nv: f64,
    /// 参与偏色统计的像素（亮度不太暗也不过曝）占的比例，太少时偏色估计不可靠
    pub neutral: f64,
}

/// 直方图（下标 = 值 + 128）里中间一半像素的平均值（去掉最小和最大的各 25%）。
/// 比平均值不容易被少数鲜艳物体拉走，比中位数在颜色分成几块（多峰）的画面里更稳定。
fn hist_middle_mean(hist: &[u32; 256], total: u32) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let (lo, hi) = (f64::from(total) * 0.25, f64::from(total) * 0.75);
    let (mut acc, mut sum, mut weight) = (0.0f64, 0.0f64, 0.0f64);
    for (i, c) in hist.iter().enumerate() {
        let c = f64::from(*c);
        let from = acc.max(lo);
        let to = (acc + c).min(hi);
        if to > from {
            sum += (i as f64 - 128.0) * (to - from);
            weight += to - from;
        }
        acc += c;
    }
    if weight > 0.0 {
        sum / weight
    } else {
        0.0
    }
}

/// 统计一帧 yuv444p 的三个平面（8 位）。
pub fn frame_stats(y: &[u8], u: &[u8], v: &[u8]) -> FrameStats {
    let n = y.len().min(u.len()).min(v.len());
    if n == 0 {
        return FrameStats::default();
    }
    let mut hist = [0u32; 256];
    let (mut hu, mut hv) = ([0u32; 256], [0u32; 256]);
    let (mut ys, mut cu, mut cv) = (0.0, 0.0, 0.0);
    let mut mid = 0u32;
    for i in 0..n {
        let (yy, du, dv) = (f64::from(y[i]), f64::from(u[i]) - 128.0, f64::from(v[i]) - 128.0);
        hist[y[i] as usize] += 1;
        ys += yy;
        cu += du;
        cv += dv;
        // 太暗和太亮的像素（黑边、过曝）色度没有意义，不参与偏色估计
        if (40.0..=225.0).contains(&yy) {
            hu[u[i] as usize] += 1;
            hv[v[i] as usize] += 1;
            mid += 1;
        }
    }
    let nf = n as f64;
    // 偏色的中心：中等亮度像素够多时用中间一半的平均值，否则用整体平均
    let (mu, mv) = (hist_middle_mean(&hu, mid), hist_middle_mean(&hv, mid));
    let valid = f64::from(mid) / nf >= 0.2;
    let (cx, cy) = if valid { (mu, mv) } else { (cu / nf, cv / nf) };
    // 饱和度：色度偏离“中心”的平均距离。这样偏色（整体平移）不会被当成饱和度变化
    let sat: f64 = (0..n).map(|i| (f64::from(u[i]) - 128.0 - cx).hypot(f64::from(v[i]) - 128.0 - cy)).sum::<f64>() / nf;
    let pct = |q: f64| {
        let target = (nf * q).ceil() as u32;
        let mut acc = 0;
        for (i, c) in hist.iter().enumerate() {
            acc += c;
            if acc >= target {
                return i as f64;
            }
        }
        255.0
    };
    FrameStats { y: ys / nf, lo: pct(0.05), hi: pct(0.95), cu: cu / nf, cv: cv / nf, sat, nu: mu, nv: mv, neutral: f64::from(mid) / nf }
}

// ---------- 抽样 ----------

#[derive(Debug, Clone)]
struct Sample {
    t_ms: u64,
    st: FrameStats,
}

/// 把 ffmpeg 输出的连续 yuv444p 小图和时间戳配成样本。
fn parse_samples(raw: &[u8], times: &[f64]) -> Vec<Sample> {
    raw.chunks_exact(FRAME_BYTES)
        .zip(times)
        .map(|(f, t)| {
            let (y, rest) = f.split_at(W * H);
            let (u, v) = rest.split_at(W * H);
            Sample { t_ms: (t.max(0.0) * 1000.0).round() as u64, st: frame_stats(y, u, v) }
        })
        .collect()
}

fn fps_mode(caps: &Caps) -> [&'static str; 2] {
    if caps.has_fps_mode() {
        ["-fps_mode", "passthrough"]
    } else {
        ["-vsync", "0"]
    }
}

/// `keyframes_only`：只解码关键帧；否则全部解码，按 `step_s` 秒取一帧。
async fn sample(ffmpeg: &Path, file: &Path, caps: &Caps, keyframes_only: bool, step_s: f64) -> Option<Vec<Sample>> {
    let mut a: Vec<String> = ["-hide_banner", "-nostats"].iter().map(|s| s.to_string()).collect();
    if keyframes_only {
        a.extend(["-skip_frame".into(), "nokey".into()]);
    }
    a.extend(["-i".into(), file.to_string_lossy().into_owned()]);
    let vf = format!("scale={W}:{H}:flags=area,format=yuv444p,select='isnan(prev_selected_t)+gte(t-prev_selected_t,{step_s:.3})',showinfo");
    a.extend(["-map", "0:v:0", "-an", "-sn", "-vf"].iter().map(|s| s.to_string()));
    a.push(vf);
    a.extend(fps_mode(caps).iter().map(|s| s.to_string()));
    a.extend(["-frames:v", "3000", "-f", "rawvideo", "-pix_fmt", "yuv444p", "-"].iter().map(|s| s.to_string()));
    let (raw, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(300)).await.ok()?;
    let times = parse_showinfo(&err);
    Some(parse_samples(&raw, &times))
}

// ---------- 分镜头 ----------

/// 两帧的色调差别：亮度、偏色、饱和度里最大的一项，1 表示明显不同。
fn look_jump(a: &FrameStats, b: &FrameStats) -> f64 {
    let (du, dv, _) = cast_delta(a, b);
    ((a.y - b.y).abs() / 30.0).max(du.hypot(dv) / 10.0).max((a.sat - b.sat).abs() / 15.0)
}

/// 相邻两个样本的色调变了就当作换了一段。只看色调、不看画面内容：内容变了但色调没变的，对“色彩是否一致”没有影响。
/// 分界的准确时间之后再用场景检测定位。
fn is_cut(a: &Sample, b: &Sample) -> bool {
    look_jump(&a.st, &b.st) >= 0.45
}

/// 一个镜头：最后一个样本的下标，时间范围由样本时间推出。
#[derive(Debug, Clone)]
struct Shot {
    last: usize,
    start_ms: u64,
    end_ms: u64,
    st: FrameStats,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.0
    }
}

/// 一组帧统计的各项中位数。偏色统计只用有效像素够多的帧。
fn median_stats(frames: &[FrameStats]) -> FrameStats {
    let pick = |f: fn(&FrameStats) -> f64| median(&mut frames.iter().map(f).collect::<Vec<_>>());
    let with_neutral: Vec<&FrameStats> = frames.iter().filter(|s| s.neutral >= 0.2).collect();
    let (nu, nv) = if with_neutral.is_empty() {
        (0.0, 0.0)
    } else {
        (median(&mut with_neutral.iter().map(|s| s.nu).collect::<Vec<_>>()), median(&mut with_neutral.iter().map(|s| s.nv).collect::<Vec<_>>()))
    };
    FrameStats {
        y: pick(|s| s.y),
        lo: pick(|s| s.lo),
        hi: pick(|s| s.hi),
        cu: pick(|s| s.cu),
        cv: pick(|s| s.cv),
        sat: pick(|s| s.sat),
        nu,
        nv,
        neutral: pick(|s| s.neutral),
    }
}

fn split_shots(samples: &[Sample], duration_ms: u64) -> Vec<Shot> {
    let mut shots: Vec<Shot> = vec![];
    let mut first = 0;
    let finish = |first: usize, last: usize, shots: &mut Vec<Shot>| {
        let frames: Vec<FrameStats> = samples[first..=last].iter().map(|s| s.st).collect();
        shots.push(Shot { last, start_ms: samples[first].t_ms, end_ms: 0, st: median_stats(&frames) });
    };
    for i in 1..samples.len() {
        if is_cut(&samples[i - 1], &samples[i]) {
            finish(first, i - 1, &mut shots);
            first = i;
        }
    }
    finish(first, samples.len() - 1, &mut shots);
    // 每个镜头到下一个镜头的第一个样本为止；第一个镜头从头开始，最后一个到视频结束
    let starts: Vec<u64> = shots.iter().map(|s| s.start_ms).collect();
    for (i, s) in shots.iter_mut().enumerate() {
        s.end_ms = starts.get(i + 1).copied().unwrap_or(duration_ms.max(s.start_ms + 1));
    }
    if let Some(s) = shots.first_mut() {
        s.start_ms = 0;
    }
    shots
}

// ---------- 和整体比较 ----------

/// 按权重取中位数。
fn weighted_median(items: &mut [(f64, f64)]) -> f64 {
    if items.is_empty() {
        return 0.0;
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = items.iter().map(|i| i.1).sum();
    let mut acc = 0.0;
    for (v, w) in items.iter() {
        acc += w;
        if acc >= total / 2.0 {
            return *v;
        }
    }
    items.last().map(|i| i.0).unwrap_or(0.0)
}

/// “整体”的色调，以及各项偏离的正常波动范围。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Reference {
    pub st: FrameStats,
    /// 偏离整体的典型幅度（中位数）：亮度、偏色、饱和比例
    pub spread_y: f64,
    pub spread_c: f64,
    pub spread_s: f64,
}

impl FrameStats {
    /// 这一帧（镜头）的色彩中心：偏色估计可靠时用它，否则用整体平均。
    fn center(&self) -> (f64, f64) {
        if self.neutral >= 0.2 {
            (self.nu, self.nv)
        } else {
            (self.cu, self.cv)
        }
    }
}

fn use_neutral(a: &FrameStats, b: &FrameStats) -> bool {
    a.neutral >= 0.2 && b.neutral >= 0.2
}

/// 偏色：优先用中等亮度像素的色度（中间一半的平均值），像素太少（画面几乎全黑或全白）时用整体平均色度。返回 (ΔU, ΔV, 用的是不是前者)。
fn cast_delta(shot: &FrameStats, r: &FrameStats) -> (f64, f64, bool) {
    if use_neutral(shot, r) {
        (shot.nu - r.nu, shot.nv - r.nv, true)
    } else {
        (shot.cu - r.cu, shot.cv - r.cv, false)
    }
}

fn sat_ratio(shot: &FrameStats, r: &FrameStats) -> f64 {
    (shot.sat + 2.0) / (r.sat + 2.0)
}

fn contrast(s: &FrameStats) -> f64 {
    (s.hi - s.lo).max(8.0)
}

fn reference(shots: &[Shot]) -> Reference {
    let w = |s: &Shot| (s.end_ms.saturating_sub(s.start_ms)).max(1) as f64;
    let pick = |f: fn(&FrameStats) -> f64| weighted_median(&mut shots.iter().map(|s| (f(&s.st), w(s))).collect::<Vec<_>>());
    let st = FrameStats {
        y: pick(|s| s.y),
        lo: pick(|s| s.lo),
        hi: pick(|s| s.hi),
        cu: pick(|s| s.cu),
        cv: pick(|s| s.cv),
        sat: pick(|s| s.sat),
        nu: pick(|s| s.nu),
        nv: pick(|s| s.nv),
        neutral: pick(|s| s.neutral),
    };
    let spread_y = weighted_median(&mut shots.iter().map(|s| ((s.st.y - st.y).abs(), w(s))).collect::<Vec<_>>());
    let spread_c = weighted_median(
        &mut shots
            .iter()
            .map(|s| {
                let (du, dv, _) = cast_delta(&s.st, &st);
                (du.hypot(dv), w(s))
            })
            .collect::<Vec<_>>(),
    );
    let spread_s = weighted_median(&mut shots.iter().map(|s| ((sat_ratio(&s.st, &st)).ln().abs(), w(s))).collect::<Vec<_>>());
    Reference { st, spread_y, spread_c, spread_s }
}

/// 一个镜头和整体的哪些地方不一致。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Defects {
    pub brightness: bool,
    pub contrast: bool,
    pub cast: bool,
    pub saturation: bool,
}

impl Defects {
    fn any(&self) -> bool {
        self.brightness || self.contrast || self.cast || self.saturation
    }
}

fn defects(s: &FrameStats, r: &Reference) -> Defects {
    let rs = &r.st;
    let (du, dv, neutral) = cast_delta(s, rs);
    let thr_c = (3.5 * r.spread_c).max(if neutral { 7.0 } else { 11.0 });
    let thr_y = (3.5 * r.spread_y).max(16.0);
    let cr = contrast(s) / contrast(rs);
    let sr = sat_ratio(s, rs);
    Defects {
        brightness: (s.y - rs.y).abs() > thr_y,
        contrast: !(0.65..=1.55).contains(&cr) && (contrast(s) - contrast(rs)).abs() > 20.0,
        cast: du.hypot(dv) > thr_c,
        saturation: !(0.62..=1.6).contains(&sr) && (s.sat - rs.sat).abs() > 4.0 && sr.ln().abs() > 3.5 * r.spread_s,
    }
}

/// 偏色方向的文字：把 U / V 的偏差换算成 RGB 偏移，按色相分成六个方向。
pub fn cast_label(du: f64, dv: f64) -> &'static str {
    let (dr, db) = (1.402 * dv, 1.772 * du);
    let dg = -0.344 * du - 0.714 * dv;
    let hue = (3f64.sqrt() * (dg - db)).atan2(2.0 * dr - dg - db).to_degrees();
    match hue {
        h if (-30.0..30.0).contains(&h) => "偏红",
        h if (30.0..90.0).contains(&h) => "偏黄",
        h if (90.0..150.0).contains(&h) => "偏绿",
        h if !(-150.0..150.0).contains(&h) => "偏青",
        h if (-150.0..-90.0).contains(&h) => "偏蓝",
        _ => "偏品红",
    }
}

fn defect_labels(s: &FrameStats, r: &Reference, d: &Defects) -> Vec<String> {
    let rs = &r.st;
    let mut out = vec![];
    if d.cast {
        let (du, dv, _) = cast_delta(s, rs);
        out.push(cast_label(du, dv).to_string());
    }
    if d.brightness {
        out.push(if s.y > rs.y { "偏亮" } else { "偏暗" }.to_string());
    }
    if d.contrast {
        out.push(if contrast(s) > contrast(rs) { "对比度偏高" } else { "对比度偏低" }.to_string());
    }
    if d.saturation {
        out.push(if s.sat > rs.sat { "饱和度偏高" } else { "饱和度偏低" }.to_string());
    }
    out
}

// ---------- 校正 ----------

/// 一个通道的校正曲线：`v' = (v - pivot) * gain + out`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Curve {
    pub pivot: f64,
    pub gain: f64,
    pub out: f64,
}

impl Curve {
    fn expr(&self) -> String {
        format!("clip((val-{:.2})*{:.4}+{:.2},0,255)", self.pivot, self.gain, self.out)
    }
}

/// 不参与色彩匹配的一段时间：既不用来统计“整体”的色调，也不会被校正（保持原样）。`end_ms` 为空表示一直到结尾。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SkipSpan {
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}

/// 最多排除这么多段，避免滤镜图太长
pub const MAX_SKIPS: usize = 50;

/// 排序、合并重叠或相接的段，去掉空的和反的；最多保留 `MAX_SKIPS` 段。
pub fn normalize_skips(list: &[SkipSpan]) -> Vec<SkipSpan> {
    let mut v: Vec<SkipSpan> = list.iter().copied().filter(|s| s.end_ms.map_or(true, |e| e > s.start_ms)).collect();
    v.sort_by_key(|s| s.start_ms);
    let mut out: Vec<SkipSpan> = vec![];
    for s in v {
        match out.last_mut() {
            Some(l) if l.end_ms.map_or(true, |e| s.start_ms <= e) => {
                l.end_ms = match (l.end_ms, s.end_ms) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    _ => None,
                };
            }
            _ => out.push(s),
        }
    }
    out.truncate(MAX_SKIPS);
    out
}

/// 排除的段换成滤镜窗口（毫秒，`None` 表示到头 / 到尾）。
fn skip_windows(skip: &[SkipSpan]) -> Vec<(Option<i64>, Option<i64>)> {
    skip.iter().map(|s| (Some(s.start_ms as i64), s.end_ms.map(|e| e as i64))).collect()
}

/// 排除的段里和 `[start, end)` 有交集的那些（滤镜窗口）。
fn skip_windows_in(skip: &[SkipSpan], start: Option<i64>, end: Option<i64>) -> Vec<(Option<i64>, Option<i64>)> {
    skip_windows(skip)
        .into_iter()
        .filter(|(a, b)| a.map_or(true, |a| end.map_or(true, |e| a < e)) && b.map_or(true, |b| start.map_or(true, |s| b > s)))
        .collect()
}

/// `[start, end)` 里有多少毫秒落在排除的段里。
fn skipped_ms(skip: &[SkipSpan], start: u64, end: u64) -> u64 {
    skip.iter().map(|s| s.end_ms.unwrap_or(u64::MAX).min(end).saturating_sub(s.start_ms.max(start))).sum()
}

/// 对一个镜头的校正。时间以毫秒计，`None` 表示从视频开头 / 到视频结尾。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorFix {
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub y: Option<Curve>,
    pub u: Option<Curve>,
    pub v: Option<Curve>,
    /// 要避开的时间窗口（单独调节的镜头）：统一调节不作用在这些镜头上
    pub except: Vec<(Option<i64>, Option<i64>)>,
}

impl ColorFix {
    /// `lutyuv` 滤镜（只在这个镜头的时间范围内生效）。
    pub fn filter(&self) -> String {
        let mut opts: Vec<String> = vec![];
        for (name, c) in [("y", &self.y), ("u", &self.u), ("v", &self.v)] {
            if let Some(c) = c {
                opts.push(format!("{name}='{}'", c.expr()));
            }
        }
        // 窗口的两端各让出半毫秒，避免时间戳取整后把分界处的第一帧排除在外
        let window = |a: Option<i64>, b: Option<i64>| -> String {
            match (a, b) {
                (Some(a), Some(b)) => format!("gte(t,{:.4})*lt(t,{:.4})", a as f64 / 1000.0 - 0.0005, b as f64 / 1000.0 - 0.0005),
                (Some(a), None) => format!("gte(t,{:.4})", a as f64 / 1000.0 - 0.0005),
                (None, Some(b)) => format!("lt(t,{:.4})", b as f64 / 1000.0 - 0.0005),
                (None, None) => "1".to_string(),
            }
        };
        let mut enable = window(self.start_ms, self.end_ms);
        if !self.except.is_empty() {
            let skip: Vec<String> = self.except.iter().map(|(a, b)| window(*a, *b)).collect();
            enable = format!("{enable}*not({})", skip.join("+"));
        }
        opts.push(format!("enable='{enable}'"));
        format!("lutyuv={}", opts.join(":"))
    }

    /// 用来在界面里识别“同一个镜头”：开头的时间。
    pub fn id(&self) -> i64 {
        self.start_ms.unwrap_or(0)
    }
}

/// 要校正的镜头集合。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorPlan {
    pub fixes: Vec<ColorFix>,
    /// 按自动校正处理的不一致镜头数、单独调节的镜头数、有没有统一的手动调节（只用来写处理说明）
    pub auto_shots: usize,
    pub own_shots: usize,
    pub unified: bool,
    /// 排除在外（保持原样）的时间段数（只用来写处理说明）
    pub skipped: usize,
}

impl ColorPlan {
    /// 处理说明里的一句话。
    pub fn describe(&self) -> String {
        let mut parts = vec![];
        if self.auto_shots > 0 {
            parts.push(format!("{} 个不一致的镜头自动校正到和整体一致", self.auto_shots));
        }
        if self.own_shots > 0 {
            parts.push(format!("{} 个镜头单独调节", self.own_shots));
        }
        if self.unified {
            parts.push(if self.own_shots > 0 { "其余镜头用统一的手动调节".to_string() } else { "统一的手动调节".to_string() });
        }
        if self.skipped > 0 {
            parts.push(format!("{} 段排除的时间保持原样", self.skipped));
        }
        format!("分段色彩匹配：{}", parts.join("，"))
    }

    /// 预览用：视频从 `offset_ms` 处开始读，滤镜看到的时间都要减去它。预览只会看到开头 `PREVIEW_SPAN_MS` 毫秒，
    /// 和这段时间没有交集的校正（别的镜头）直接去掉，滤镜图不用跟着镜头数一起变长。
    pub fn shifted(&self, offset_ms: i64) -> ColorPlan {
        let hits = |a: Option<i64>, b: Option<i64>| a.map_or(true, |a| a < PREVIEW_SPAN_MS) && b.map_or(true, |b| b > 0);
        let shift = |v: Option<i64>| v.map(|v| v - offset_ms);
        ColorPlan {
            auto_shots: self.auto_shots,
            own_shots: self.own_shots,
            unified: self.unified,
            skipped: self.skipped,
            fixes: self
                .fixes
                .iter()
                .map(|f| ColorFix {
                    start_ms: shift(f.start_ms),
                    end_ms: shift(f.end_ms),
                    except: f.except.iter().map(|(a, b)| (shift(*a), shift(*b))).filter(|(a, b)| hits(*a, *b)).collect(),
                    ..f.clone()
                })
                .filter(|f| hits(f.start_ms, f.end_ms))
                .collect(),
        }
    }
}

/// 预览只读开头这么长的一段（预热加一帧）。
const PREVIEW_SPAN_MS: i64 = 1500;

const MAX_Y_SHIFT: f64 = 40.0;
const MAX_C_SHIFT: f64 = 24.0;

// ---------- 手动调节 ----------

/// 手动调节：五项都在 -1 到 1 之间，0 是不动。用于“统一调节”和单个镜头的“单独调节”。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToneAdjust {
    /// 亮度：±1 约等于 Y 平移 ±40 级
    pub brightness: f64,
    /// 对比度：±1 约等于以中灰为轴放大 / 缩小 50%
    pub contrast: f64,
    /// 饱和度：-1 变成黑白，+1 色度放大一倍
    pub saturation: f64,
    /// 色温：正数偏暖（黄、橙），负数偏冷（蓝）
    pub temperature: f64,
    /// 色调：正数偏品红，负数偏绿
    pub tint: f64,
}

impl ToneAdjust {
    pub fn is_neutral(&self) -> bool {
        *self == ToneAdjust::default()
    }

    /// 限制在 -1 到 1，不是数字的当作 0。
    pub fn checked(self) -> ToneAdjust {
        let c = |v: f64| if v.is_finite() { (v.clamp(-1.0, 1.0) * 1000.0).round() / 1000.0 } else { 0.0 };
        ToneAdjust {
            brightness: c(self.brightness),
            contrast: c(self.contrast),
            saturation: c(self.saturation),
            temperature: c(self.temperature),
            tint: c(self.tint),
        }
    }

    /// 换成 Y / U / V 三条曲线（在 YUV 里做的近似：亮度平移、对比度缩放，色温和色调平移 U / V，饱和度缩放 U / V）。
    fn curves(&self) -> (Option<Curve>, Option<Curve>, Option<Curve>) {
        let y = (self.brightness != 0.0 || self.contrast != 0.0).then_some(Curve {
            pivot: 126.0,
            gain: 1.0 + 0.5 * self.contrast,
            out: 126.0 + 40.0 * self.brightness,
        });
        // 暖色：U 降（少蓝）、V 略升（多红）；品红：U、V 都升
        let shift_u = -12.0 * self.temperature + 8.0 * self.tint;
        let shift_v = 6.0 * self.temperature + 8.0 * self.tint;
        let chroma = |shift: f64| (self.saturation != 0.0 || shift != 0.0).then_some(Curve { pivot: 128.0, gain: 1.0 + self.saturation, out: 128.0 + shift });
        (y, chroma(shift_u), chroma(shift_v))
    }
}

/// 单独调节的镜头。`id` 是检测结果里这个镜头开头的毫秒数。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShotAdjust {
    pub id: i64,
    /// 这个镜头的自动校正强度（0–1）；不填就和统一的强度一样
    pub strength: Option<f64>,
    pub tone: ToneAdjust,
}

/// 没有镜头分析（HDR、太短、分析失败）时也能做的部分：只有统一的手动调节。
pub fn tone_plan(tone: &ToneAdjust, skip: &[SkipSpan]) -> ColorPlan {
    let (y, u, v) = tone.curves();
    let on = y.is_some() || u.is_some() || v.is_some();
    let skip = normalize_skips(skip);
    ColorPlan {
        fixes: if on { vec![ColorFix { start_ms: None, end_ms: None, y, u, v, except: skip_windows(&skip) }] } else { vec![] },
        unified: on,
        skipped: skip.len(),
        ..Default::default()
    }
}

/// 按强度（0–1）算一个镜头的校正曲线，只改不一致的那几项。
fn fix_for(s: &FrameStats, r: &Reference, d: &Defects, strength: f64) -> (Option<Curve>, Option<Curve>, Option<Curve>) {
    let rs = &r.st;
    let k = strength.clamp(0.0, 1.0);
    let mut y = None;
    if d.brightness || d.contrast {
        let shift = if d.brightness { (k * (rs.y - s.y)).clamp(-MAX_Y_SHIFT, MAX_Y_SHIFT) } else { 0.0 };
        let gain = if d.contrast { (1.0 + k * (contrast(rs) / contrast(s) - 1.0)).clamp(0.7, 1.4) } else { 1.0 };
        y = Some(Curve { pivot: s.y, gain, out: s.y + shift });
    }
    let (du, dv, _) = cast_delta(s, rs);
    let (shift_u, shift_v) = if d.cast { ((-k * du).clamp(-MAX_C_SHIFT, MAX_C_SHIFT), (-k * dv).clamp(-MAX_C_SHIFT, MAX_C_SHIFT)) } else { (0.0, 0.0) };
    let gain = if d.saturation { (1.0 + k * (1.0 / sat_ratio(s, rs) - 1.0)).clamp(0.5, 2.5) } else { 1.0 };
    // 缩放以这个镜头自己的色彩中心为轴，平移再把中心移到整体的位置
    let (cx, cy) = s.center();
    let chroma =
        |center: f64, shift: f64| -> Option<Curve> { (d.cast || d.saturation).then_some(Curve { pivot: 128.0 + center, gain, out: 128.0 + center + shift }) };
    (y, chroma(cx, shift_u), chroma(cy, shift_v))
}

// ---------- 分析结果 ----------

/// 一个需要校正的镜头。
#[derive(Debug, Clone)]
struct Flagged {
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    st: FrameStats,
    defects: Defects,
}

/// 整段视频的分析结果；强度不同时可以直接重新算校正，不用再分析。
#[derive(Debug, Clone)]
pub struct ColorAnalysis {
    duration_ms: u64,
    shots: Vec<(u64, u64, bool)>,
    /// 每个镜头的色彩特征（和 `shots` 一一对应）
    looks: Vec<FrameStats>,
    reference: Reference,
    flagged: Vec<Flagged>,
    /// 排除在外的时间段（已整理）；校正不会作用在这些时间里，落在里面的镜头也不参与统计
    skip: Vec<SkipSpan>,
    /// 因为排除而没有参与统计的镜头数
    excluded: usize,
    /// 没法做或没必要做的原因
    pub note: Option<String>,
}

/// 给界面看的检测结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorReport {
    pub duration_ms: u64,
    pub shots_total: usize,
    /// 时间轴：每一段的起止和是否不一致（相邻的一致镜头已合并）
    pub timeline: Vec<TimelineSeg>,
    pub flagged: Vec<FlaggedShot>,
    /// 排除在外的时间段（结尾已补成视频时长）和因此没有参与统计的镜头数
    pub skipped: Vec<SkippedSpan>,
    pub shots_excluded: usize,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedSpan {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSeg {
    pub start_ms: u64,
    pub end_ms: u64,
    pub off: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlaggedShot {
    /// 和 `ColorFix::id` 一样，用来取消某个镜头的校正
    pub id: i64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub defects: Vec<String>,
}

impl ColorAnalysis {
    /// 生成要执行的校正。`strength` 和 `tone` 是统一的参数：不一致的镜头按统一强度自动校正，再加上统一的手动调节；
    /// 其他镜头只加统一的手动调节。`shots` 里的镜头单独调节：用自己的自动校正强度和手动调节，不再用统一的手动调节。
    pub fn plan(&self, strength: f64, tone: &ToneAdjust, shots: &[ShotAdjust]) -> ColorPlan {
        let mut fixes = vec![];
        let mut except = vec![];
        let (mut auto_shots, mut own_shots) = (0, 0);
        for f in &self.flagged {
            let own = shots.iter().rev().find(|s| s.id == f.start_ms.unwrap_or(0));
            let k = own.and_then(|s| s.strength).unwrap_or(strength).clamp(0.0, 1.0);
            if k > 0.0 {
                let (y, u, v) = fix_for(&f.st, &self.reference, &f.defects, k);
                if y.is_some() || u.is_some() || v.is_some() {
                    fixes.push(ColorFix { start_ms: f.start_ms, end_ms: f.end_ms, y, u, v, except: skip_windows_in(&self.skip, f.start_ms, f.end_ms) });
                    if own.is_none() {
                        auto_shots += 1;
                    }
                }
            }
            if let Some(own) = own {
                except.push((f.start_ms, f.end_ms));
                let (y, u, v) = own.tone.curves();
                let manual = y.is_some() || u.is_some() || v.is_some();
                if manual {
                    fixes.push(ColorFix { start_ms: f.start_ms, end_ms: f.end_ms, y, u, v, except: skip_windows_in(&self.skip, f.start_ms, f.end_ms) });
                }
                if manual || k > 0.0 {
                    own_shots += 1;
                }
            }
        }
        let (y, u, v) = tone.curves();
        let unified = y.is_some() || u.is_some() || v.is_some();
        if unified {
            except.extend(skip_windows(&self.skip));
            fixes.push(ColorFix { start_ms: None, end_ms: None, y, u, v, except });
        }
        ColorPlan { fixes, auto_shots, own_shots, unified, skipped: self.skip.len() }
    }

    pub fn shots_total(&self) -> usize {
        self.shots.len()
    }

    /// 每个镜头的色彩特征，和整体的色调。
    pub fn looks(&self) -> (&[FrameStats], &Reference) {
        (&self.looks, &self.reference)
    }

    pub fn report(&self) -> ColorReport {
        let mut timeline: Vec<TimelineSeg> = vec![];
        for (a, b, off) in &self.shots {
            match timeline.last_mut() {
                Some(l) if l.off == *off => l.end_ms = *b,
                _ => timeline.push(TimelineSeg { start_ms: *a, end_ms: *b, off: *off }),
            }
        }
        let flagged = self
            .flagged
            .iter()
            .map(|f| FlaggedShot {
                id: f.start_ms.unwrap_or(0),
                start_ms: f.start_ms.unwrap_or(0).max(0) as u64,
                end_ms: f.end_ms.map(|v| v.max(0) as u64).unwrap_or(self.duration_ms),
                defects: defect_labels(&f.st, &self.reference, &f.defects).into_iter().collect(),
            })
            .map(|mut f| {
                f.defects.dedup();
                f
            })
            .collect();
        let skipped = self
            .skip
            .iter()
            .map(|s| SkippedSpan { start_ms: s.start_ms.min(self.duration_ms), end_ms: s.end_ms.unwrap_or(self.duration_ms).min(self.duration_ms) })
            .filter(|s| s.end_ms > s.start_ms)
            .collect();
        ColorReport {
            duration_ms: self.duration_ms,
            shots_total: self.shots.len(),
            timeline,
            flagged,
            skipped,
            shots_excluded: self.excluded,
            note: self.note.clone(),
        }
    }
}

// ---------- 精确定位镜头分界 ----------

/// 在 `(from_ms, to_ms]` 里找镜头切换最明显的那一帧，返回它的时间。没有明显的切换返回 None。
async fn refine_cut(ffmpeg: &Path, file: &Path, from_ms: u64, to_ms: u64) -> Option<u64> {
    if to_ms <= from_ms + 80 {
        return Some(to_ms);
    }
    let mut a: Vec<String> = ["-hide_banner", "-nostats", "-ss"].iter().map(|s| s.to_string()).collect();
    a.push(format!("{:.3}", from_ms as f64 / 1000.0));
    a.extend(["-t".into(), format!("{:.3}", (to_ms - from_ms) as f64 / 1000.0 + 0.2), "-i".into(), file.to_string_lossy().into_owned()]);
    a.extend(
        ["-map", "0:v:0", "-an", "-sn", "-vf", "scale=96:54:flags=area,select='gte(scene,0)',metadata=print:key=lavfi.scene_score:file=-", "-f", "null", "-"]
            .iter()
            .map(|s| s.to_string()),
    );
    let (out, _) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(90)).await.ok()?;
    let text = String::from_utf8_lossy(&out);
    let mut best: Option<(f64, f64)> = None;
    let mut t = None;
    for line in text.lines() {
        if let Some(rest) = line.split("pts_time:").nth(1) {
            t = rest.split_whitespace().next().and_then(|v| v.parse::<f64>().ok());
        } else if let Some(v) = line.strip_prefix("lavfi.scene_score=") {
            if let (Some(t), Ok(score)) = (t, v.trim().parse::<f64>()) {
                if best.map_or(true, |b| score > b.0) {
                    best = Some((score, t));
                }
            }
        }
    }
    let (score, t) = best?;
    (score >= 0.2).then(|| from_ms + (t * 1000.0).round() as u64)
}

// ---------- 分析入口 ----------

/// 抽样和分镜头的结果：只和文件有关，和排除的时间段无关，所以改排除时间段不用重新解码。
#[derive(Debug, Clone)]
struct Scan {
    duration_ms: u64,
    samples: Vec<Sample>,
    shots: Vec<Shot>,
}

/// 抽样并分镜头。HDR、太短、取不到画面时返回说明原因的错误。
async fn scan(ffmpeg: &Path, file: &Path, facts: &Facts, caps: &Caps) -> Result<Scan, String> {
    let v = facts.video.as_ref().ok_or("这个文件里没有视频画面。")?;
    if v.hdr != Hdr::None {
        return Err("HDR 素材要先转成 SDR，目前不支持对 HDR 画面做分段色彩匹配。".into());
    }
    let duration_ms = facts.duration_ms.unwrap_or(0);
    if duration_ms < 4000 {
        return Err("视频太短（不到 4 秒），没有可比较的镜头。".into());
    }
    let dur_s = duration_ms as f64 / 1000.0;
    let mut samples = sample(ffmpeg, file, caps, true, 0.4).await.unwrap_or_default();
    let median_gap = {
        let mut gaps: Vec<f64> = samples.windows(2).map(|w| (w[1].t_ms - w[0].t_ms) as f64).collect();
        median(&mut gaps)
    };
    // 关键帧太少（录屏、直播录制常见）：改成按时间间隔解码抽样，慢一些
    if (samples.len() < 6 || median_gap > 6000.0) && dur_s <= 1800.0 {
        let step = (dur_s / 1000.0).max(0.5);
        if let Some(dense) = sample(ffmpeg, file, caps, false, step).await {
            if dense.len() > samples.len() {
                samples = dense;
            }
        }
    }
    if samples.len() < 4 {
        return Err("取到的画面太少，无法分析色彩。".into());
    }
    let shots = split_shots(&samples, duration_ms);
    Ok(Scan { duration_ms, samples, shots })
}

/// 分析一个视频的镜头之间色彩是否一致（不排除任何时间段）。
pub async fn analyze(ffmpeg: &Path, file: &Path, facts: &Facts, caps: &Caps) -> Result<ColorAnalysis, String> {
    let sc = scan(ffmpeg, file, facts, caps).await?;
    Ok(evaluate(ffmpeg, file, &sc, &[]).await)
}

/// 在抽样结果上找出和整体不一致的镜头。`skip` 里的时间段不参与统计（镜头大半落在里面就整个不算），也不会被校正。
async fn evaluate(ffmpeg: &Path, file: &Path, sc: &Scan, skip: &[SkipSpan]) -> ColorAnalysis {
    let Scan { duration_ms, samples, shots: all } = sc;
    let duration_ms = *duration_ms;
    let skip = normalize_skips(skip);
    // 大半（一半以上）落在排除时间里的镜头：不参与统计，也不标记
    let covered: Vec<bool> = all.iter().map(|s| skipped_ms(&skip, s.start_ms, s.end_ms) * 2 >= (s.end_ms - s.start_ms).max(1)).collect();
    let kept: Vec<usize> = (0..all.len()).filter(|i| !covered[*i]).collect();
    let excluded = all.len() - kept.len();
    let shots: Vec<Shot> = kept.iter().map(|i| all[*i].clone()).collect();
    let timeline: Vec<(u64, u64)> = all.iter().map(|s| (s.start_ms, s.end_ms)).collect();
    let looks: Vec<FrameStats> = all.iter().map(|s| s.st).collect();
    let done = |flag: &[bool], reference: Reference, flagged: Vec<Flagged>, note: Option<String>| ColorAnalysis {
        duration_ms,
        shots: timeline.iter().zip(flag).map(|((a, b), f)| (*a, *b, *f)).collect(),
        looks: looks.clone(),
        reference,
        flagged,
        skip: skip.clone(),
        excluded,
        note,
    };
    if shots.len() < 3 {
        let note = if excluded > 0 {
            "排除掉一些时间之后，剩下的镜头太少（不到 3 个），没有可以和“整体”比较的对象。"
        } else {
            "镜头太少（不到 3 个），没有可以和“整体”比较的对象。"
        };
        return done(&vec![false; all.len()], reference(&shots), vec![], Some(note.into()));
    }
    let r = reference(&shots);
    let marks: Vec<Defects> = shots.iter().map(|s| defects(&s.st, &r)).collect();
    let kept_ms: u64 = shots.iter().map(|s| s.end_ms - s.start_ms).sum();
    let off_ms: u64 = shots.iter().zip(&marks).filter(|(_, d)| d.any()).map(|(s, _)| s.end_ms - s.start_ms).sum();
    let mut note = None;
    let mut flag = vec![false; all.len()];
    if off_ms as f64 > kept_ms as f64 * 0.6 {
        note = Some("超过六成的镜头都和“整体”不一样，没有明确的主体色调可以对齐，没有做处理。".into());
    } else if marks.iter().filter(|d| d.any()).count() > 150 {
        note = Some("有差异的镜头太多（超过 150 个），没有做处理。".into());
    } else {
        for (k, d) in marks.iter().enumerate() {
            flag[kept[k]] = d.any();
        }
        if !flag.iter().any(|f| *f) {
            note = Some("各个镜头的色彩和整体一致，没有需要校正的镜头。".into());
        }
    }

    // 只对需要校正的镜头精确定位分界：开头在“上一个镜头最后一个样本”和“这个镜头第一个样本”之间，结尾同理
    let todo: Vec<usize> = (0..all.len()).filter(|i| flag[*i]).collect();
    let jobs = todo.iter().map(|&i| async move {
        let s = &all[i];
        let start = if i == 0 {
            None
        } else {
            let prev_last = samples[all[i - 1].last].t_ms;
            Some(refine_cut(ffmpeg, file, prev_last, s.start_ms).await.unwrap_or(s.start_ms) as i64)
        };
        let end = if i + 1 >= all.len() {
            None
        } else {
            let last = samples[s.last].t_ms;
            let next_first = all[i + 1].start_ms;
            Some(refine_cut(ffmpeg, file, last, next_first).await.unwrap_or(next_first) as i64)
        };
        (i, start, end)
    });
    let mut refined = vec![];
    let mut pending: Vec<_> = jobs.collect();
    // 同时最多 4 个 ffmpeg
    while !pending.is_empty() {
        let batch: Vec<_> = pending.drain(..pending.len().min(4)).collect();
        refined.extend(futures_util::future::join_all(batch).await);
    }
    refined.sort_by_key(|r| r.0);
    let flagged: Vec<Flagged> = refined
        .iter()
        .map(|(i, start, end)| {
            let k = kept.iter().position(|x| x == i).unwrap_or(0);
            Flagged { start_ms: *start, end_ms: *end, st: all[*i].st, defects: marks[k] }
        })
        .collect();
    done(&flag, r, flagged, note)
}

// ---------- 缓存 ----------

type Entry = (String, Arc<ColorAnalysis>);
static CACHE: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static SCANS: Mutex<Vec<(String, Arc<Scan>)>> = Mutex::new(Vec::new());

fn key_of(file: &Path) -> String {
    let meta = std::fs::metadata(file).ok();
    let mtime = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}|{}|{}", file.display(), meta.map(|m| m.len()).unwrap_or(0), mtime)
}

/// 同一个文件、同样的排除时间段分析过就直接用（界面检测、预览、真正处理共用一份结果）；
/// 抽样那一步只和文件有关，改排除时间段时不用重新解码。
pub async fn analyze_cached(ffmpeg: &Path, file: &Path, facts: &Facts, caps: &Caps, skip: &[SkipSpan]) -> Result<Arc<ColorAnalysis>, String> {
    let skip = normalize_skips(skip);
    let file_key = key_of(file);
    let key = format!("{file_key}|{}", serde_json::to_string(&skip).unwrap_or_default());
    if let Some(hit) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == key) {
        return Ok(hit.1.clone());
    }
    let cached_scan = SCANS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == file_key).map(|(_, s)| s.clone());
    let sc = match cached_scan {
        Some(sc) => sc,
        None => {
            let sc = Arc::new(scan(ffmpeg, file, facts, caps).await?);
            let mut c = SCANS.lock().unwrap_or_else(|e| e.into_inner());
            c.retain(|(k, _)| *k != file_key);
            c.push((file_key, sc.clone()));
            if c.len() > 4 {
                c.remove(0);
            }
            sc
        }
    };
    let a = Arc::new(evaluate(ffmpeg, file, &sc, &skip).await);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.retain(|(k, _)| *k != key);
    c.push((key, a.clone()));
    if c.len() > 8 {
        c.remove(0);
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一帧：亮度、U、V 各一个常数加一点纹理。
    fn frame(y: u8, u: u8, v: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let n = W * H;
        ((0..n).map(|i| y.saturating_add((i % 7) as u8)).collect(), vec![u; n], vec![v; n])
    }

    fn stats(y: u8, u: u8, v: u8) -> FrameStats {
        let (a, b, c) = frame(y, u, v);
        frame_stats(&a, &b, &c)
    }

    #[test]
    fn frame_stats_basics() {
        let s = stats(100, 128, 128);
        assert!((s.y - 103.0).abs() < 1.0 && s.cu.abs() < 1e-9 && s.cv.abs() < 1e-9 && s.sat < 1e-9, "{s:?}");
        assert!(s.neutral > 0.99 && s.hi - s.lo <= 6.0, "{s:?}");
        // 偏蓝偏红：U 高、V 高；中间一半的平均值和整体平均值一致
        let w = stats(100, 150, 140);
        assert!((w.cu - 22.0).abs() < 1e-9 && (w.cv - 12.0).abs() < 1e-9, "{w:?}");
        assert!((w.nu - 22.0).abs() < 1e-9 && (w.nv - 12.0).abs() < 1e-9 && w.neutral > 0.99, "{w:?}");
        // 黑边（很暗的像素）不参与偏色估计
        let black = stats(16, 128, 128);
        assert_eq!(black.neutral, 0.0);
        // 少数鲜艳的物体不影响偏色估计：1/5 的像素是纯红，整体平均色度被拉走，中间一半的平均值还在原处
        let (y, mut u, mut v) = frame(100, 128, 128);
        for i in 0..(W * H / 5) {
            u[i] = 90;
            v[i] = 240;
        }
        let m = frame_stats(&y, &u, &v);
        assert!(m.cv > 20.0 && m.nu.abs() < 1e-9 && m.nv.abs() < 1e-9, "{m:?}");
        assert_eq!(frame_stats(&[], &[], &[]), FrameStats::default());
    }

    #[test]
    fn cast_directions() {
        assert_eq!(cast_label(0.0, 20.0), "偏红");
        assert_eq!(cast_label(20.0, 0.0), "偏蓝");
        assert_eq!(cast_label(-20.0, 5.0), "偏黄");
        assert_eq!(cast_label(-8.0, -20.0), "偏绿");
        assert_eq!(cast_label(5.0, -20.0), "偏青");
        assert_eq!(cast_label(20.0, 20.0), "偏品红");
    }

    fn shot(start: u64, end: u64, st: FrameStats) -> Shot {
        Shot { last: 0, start_ms: start, end_ms: end, st }
    }

    fn base() -> FrameStats {
        FrameStats { y: 110.0, lo: 40.0, hi: 190.0, cu: 0.0, cv: 0.0, sat: 14.0, nu: 0.0, nv: 0.0, neutral: 0.5 }
    }

    #[test]
    fn outliers_are_found_and_the_rest_is_left_alone() {
        let mut shots: Vec<Shot> = (0..6).map(|i| shot(i * 3000, i * 3000 + 3000, FrameStats { y: 108.0 + (i % 3) as f64 * 2.0, ..base() })).collect();
        // 第 2 个偏黄（U 低、V 略高），第 4 个偏亮又发灰
        shots[2].st = FrameStats { cu: -12.0, cv: 5.0, nu: -14.0, nv: 6.0, ..base() };
        shots[4].st = FrameStats { y: 160.0, lo: 115.0, hi: 190.0, sat: 5.0, ..base() };
        let r = reference(&shots);
        let marks: Vec<Defects> = shots.iter().map(|s| defects(&s.st, &r)).collect();
        assert!(marks[2].cast && !marks[2].brightness, "{:?}", marks[2]);
        assert!(marks[4].brightness && marks[4].contrast && marks[4].saturation, "{:?}", marks[4]);
        for i in [0, 1, 3, 5] {
            assert!(!marks[i].any(), "镜头 {i} 本来就和整体一致：{:?}", marks[i]);
        }
        assert!(defect_labels(&shots[2].st, &r, &marks[2]).contains(&"偏黄".to_string()));
        let l = defect_labels(&shots[4].st, &r, &marks[4]);
        assert!(l.contains(&"偏亮".into()) && l.contains(&"饱和度偏低".into()), "{l:?}");
    }

    #[test]
    fn natural_variation_is_not_flagged() {
        // 每个镜头亮度、饱和度都有一些自然差别
        let ys = [95.0, 118.0, 104.0, 125.0, 99.0, 112.0];
        let shots: Vec<Shot> = ys
            .iter()
            .enumerate()
            .map(|(i, y)| shot(i as u64 * 2000, i as u64 * 2000 + 2000, FrameStats { y: *y, sat: 12.0 + (i as f64) * 1.5, ..base() }))
            .collect();
        let r = reference(&shots);
        for s in &shots {
            assert!(!defects(&s.st, &r).any(), "{:?}", s.st);
        }
    }

    #[test]
    fn fixes_pull_towards_the_reference_by_strength() {
        let shots: Vec<Shot> = (0..5).map(|i| shot(i * 2000, i * 2000 + 2000, base())).collect();
        let r = reference(&shots);
        let bad = FrameStats { y: 150.0, cu: -16.0, cv: 8.0, nu: -18.0, nv: 9.0, sat: 30.0, ..base() };
        let d = defects(&bad, &r);
        assert!(d.brightness && d.cast && d.saturation, "{d:?}");
        let (y, u, v) = fix_for(&bad, &r, &d, 1.0);
        let (y, u, v) = (y.unwrap(), u.unwrap(), v.unwrap());
        // 亮度整体下移到整体的 110；偏色平移 +18 / -9；饱和度变小
        assert!((y.out - (150.0 - 40.0)).abs() < 1e-6, "{y:?}");
        assert!((u.out - u.pivot - 18.0).abs() < 1e-6 && (v.out - v.pivot + 9.0).abs() < 1e-6, "{u:?} {v:?}");
        assert!(u.gain < 1.0 && (u.gain - v.gain).abs() < 1e-12, "{u:?}");
        // 强度 0.5：一半
        let (y5, ..) = fix_for(&bad, &r, &d, 0.5);
        assert!((y5.unwrap().out - (150.0 - 20.0)).abs() < 1e-6);
        // 只有亮度不一致时不碰色度
        let only_y = FrameStats { y: 150.0, ..base() };
        let (y, u, v) = fix_for(&only_y, &r, &defects(&only_y, &r), 1.0);
        assert!(y.is_some() && u.is_none() && v.is_none());
    }

    #[test]
    fn filter_text_has_a_time_window() {
        let c = Curve { pivot: 100.0, gain: 1.1, out: 90.0 };
        let fix = ColorFix { start_ms: Some(3000), end_ms: Some(6000), y: Some(c), u: None, v: None, except: vec![] };
        let f = fix.filter();
        assert!(f.starts_with("lutyuv=y='clip((val-100.00)*1.1000+90.00,0,255)':enable='gte(t,2.9995)*lt(t,5.9995)'"), "{f}");
        let first = ColorFix { start_ms: None, end_ms: Some(2000), ..fix.clone() };
        assert!(first.filter().ends_with("enable='lt(t,1.9995)'"), "{}", first.filter());
        let last = ColorFix { start_ms: Some(2000), end_ms: None, ..fix.clone() };
        assert!(last.filter().ends_with("enable='gte(t,1.9995)'"));
        // 预览从 2 秒处开始读：窗口往前移 2 秒
        let plan = ColorPlan { fixes: vec![fix], ..Default::default() }.shifted(2000);
        assert_eq!((plan.fixes[0].start_ms, plan.fixes[0].end_ms), (Some(1000), Some(4000)));
    }

    fn flagged_analysis() -> ColorAnalysis {
        let r = Reference { st: base(), spread_y: 2.0, spread_c: 1.0, spread_s: 0.05 };
        let off = FrameStats { y: 160.0, ..base() };
        ColorAnalysis {
            duration_ms: 20_000,
            shots: vec![(0, 5000, true), (5000, 20_000, false)],
            looks: vec![off, base()],
            reference: r,
            flagged: vec![Flagged { start_ms: None, end_ms: Some(5000), st: off, defects: defects(&off, &r) }],
            skip: vec![],
            excluded: 0,
            note: None,
        }
    }

    fn span(a: u64, b: Option<u64>) -> SkipSpan {
        SkipSpan { start_ms: a, end_ms: b }
    }

    #[test]
    fn skip_spans_are_tidied_and_measured() {
        // 空的、反的去掉；重叠、相接的合并；"到结尾"吃掉后面所有的
        let v = normalize_skips(&[
            span(9000, Some(9000)),
            span(8000, Some(7000)),
            span(5000, Some(7000)),
            span(1000, Some(3000)),
            span(7000, Some(8000)),
            span(20_000, None),
            span(30_000, Some(31_000)),
        ]);
        assert_eq!(v, vec![span(1000, Some(3000)), span(5000, Some(8000)), span(20_000, None)]);
        assert_eq!(normalize_skips(&[span(0, None), span(5, Some(9))]), vec![span(0, None)]);
        let many: Vec<SkipSpan> = (0..80).map(|i| span(i * 100, Some(i * 100 + 50))).collect();
        assert_eq!(normalize_skips(&many).len(), MAX_SKIPS);
        // 一个镜头有多少毫秒在排除的时间里
        let v = [span(1000, Some(3000)), span(5000, None)];
        assert_eq!(skipped_ms(&v, 0, 2000), 1000);
        assert_eq!(skipped_ms(&v, 2000, 5000), 1000);
        assert_eq!(skipped_ms(&v, 4000, 9000), 4000);
        assert_eq!(skipped_ms(&v, 3000, 5000), 0);
        // 只留和这段有交集的窗口
        assert_eq!(skip_windows_in(&v, Some(2500), Some(4000)), vec![(Some(1000), Some(3000))]);
        assert_eq!(skip_windows_in(&v, None, Some(900)), vec![]);
        assert_eq!(skip_windows_in(&v, Some(3000), Some(5000)), vec![]);
        assert_eq!(skip_windows_in(&v, Some(4000), None), vec![(Some(5000), None)]);
    }

    #[test]
    fn plans_keep_clear_of_the_skipped_time() {
        let mut a = flagged_analysis();
        a.skip = vec![span(2000, Some(3000)), span(9000, Some(10_000))];
        let warm = ToneAdjust { temperature: 0.5, ..Default::default() };
        let p = a.plan(0.8, &warm, &[]);
        assert_eq!(p.skipped, 2);
        // 自动校正的镜头（0–5 秒）只避开和它有交集的那一段
        assert_eq!(p.fixes[0].except, vec![(Some(2000), Some(3000))]);
        assert!(p.fixes[0].filter().ends_with("enable='lt(t,4.9995)*not(gte(t,1.9995)*lt(t,2.9995))'"), "{}", p.fixes[0].filter());
        // 统一的手动调节避开全部排除的时间
        assert_eq!(p.fixes[1].except, vec![(Some(2000), Some(3000)), (Some(9000), Some(10_000))]);
        assert!(p.describe().contains("2 段排除的时间保持原样"), "{}", p.describe());
        // 分析失败时统一调节照样避开
        let t = tone_plan(&warm, &a.skip);
        assert_eq!((t.fixes[0].except.len(), t.skipped), (2, 2));
        assert!(tone_plan(&ToneAdjust::default(), &a.skip).fixes.is_empty());
        // 单独调节的镜头：自己的手动调节也避开排除的时间
        let own = ShotAdjust { id: 0, strength: Some(0.0), tone: ToneAdjust { brightness: 0.4, ..Default::default() } };
        let p = a.plan(0.8, &ToneAdjust::default(), &[own]);
        assert_eq!(p.fixes[0].except, vec![(Some(2000), Some(3000))]);
        // 预览从 1 秒处读起：窗口往前移 1 秒
        let sh = a.plan(0.8, &ToneAdjust::default(), &[]).shifted(1000);
        assert_eq!(sh.fixes[0].except, vec![(Some(1000), Some(2000))]);
        // 报告里有排除的时间段，结尾补成视频时长
        a.skip = vec![span(18_000, None)];
        assert_eq!(a.report().skipped.iter().map(|s| (s.start_ms, s.end_ms)).collect::<Vec<_>>(), vec![(18_000, 20_000)]);
    }

    #[test]
    fn plan_follows_the_unified_strength() {
        let a = flagged_analysis();
        let none = ToneAdjust::default();
        assert_eq!(a.plan(0.0, &none, &[]).fixes.len(), 0);
        assert_eq!(a.plan(0.8, &none, &[]).fixes.len(), 1);
        let rep = a.report();
        assert_eq!((rep.shots_total, rep.flagged.len(), rep.timeline.len()), (2, 1, 2));
        assert_eq!(rep.flagged[0].defects, vec!["偏亮".to_string()]);
    }

    #[test]
    fn unified_tone_skips_shots_that_are_adjusted_on_their_own() {
        let a = flagged_analysis();
        let warm = ToneAdjust { temperature: 0.5, ..Default::default() };
        // 统一的手动调节作用在整段视频上：不一致的镜头先自动校正，再加统一调节
        let p = a.plan(0.8, &warm, &[]);
        assert_eq!(p.fixes.len(), 2);
        let uni = p.fixes.last().unwrap();
        assert!(uni.start_ms.is_none() && uni.end_ms.is_none() && uni.except.is_empty() && uni.y.is_none() && uni.u.is_some() && uni.v.is_some());
        // 单独调节且强度 0、不加手动调节：这个镜头什么都不做，统一调节也避开它
        let skip = ShotAdjust { id: 0, strength: Some(0.0), tone: ToneAdjust::default() };
        let p = a.plan(0.8, &warm, &[skip]);
        assert_eq!(p.fixes.len(), 1, "只剩统一调节");
        assert_eq!(p.fixes[0].except, vec![(None, Some(5000))]);
        assert!(p.fixes[0].filter().ends_with("enable='1*not(lt(t,4.9995))'"), "{}", p.fixes[0].filter());
        // 单独调节的镜头：自己的自动校正强度（不填跟统一一样）+ 自己的手动调节
        let own = ShotAdjust { id: 0, strength: None, tone: ToneAdjust { brightness: -0.3, ..Default::default() } };
        let p = a.plan(0.8, &ToneAdjust::default(), std::slice::from_ref(&own));
        assert_eq!(p.fixes.len(), 2);
        assert_eq!((p.fixes[1].start_ms, p.fixes[1].end_ms), (None, Some(5000)));
        let y = p.fixes[1].y.unwrap();
        assert!((y.out - (126.0 - 12.0)).abs() < 1e-9 && y.gain == 1.0, "{y:?}");
        // 没有对应镜头的 id 被忽略；同一个 id 写了两次，以后一个为准
        let stray = ShotAdjust { id: 12345, strength: Some(0.0), tone: ToneAdjust { tint: 1.0, ..Default::default() } };
        assert_eq!(a.plan(0.8, &ToneAdjust::default(), &[stray]).fixes.len(), 1);
        let off = ShotAdjust { id: 0, strength: Some(0.0), tone: ToneAdjust::default() };
        assert_eq!(a.plan(0.8, &ToneAdjust::default(), &[own, off]).fixes.len(), 0);
        // 预览从 2 秒处开始读时，排除窗口也一起往前移
        let shifted = a.plan(0.8, &warm, &[ShotAdjust { id: 0, strength: Some(0.0), ..Default::default() }]).shifted(2000);
        assert_eq!(shifted.fixes[0].except, vec![(None, Some(3000))]);
    }

    #[test]
    fn preview_plan_only_keeps_what_the_preview_can_see() {
        let fix = |a: i64, b: i64| ColorFix { start_ms: Some(a), end_ms: Some(b), y: None, u: None, v: None, except: vec![] };
        let plan = ColorPlan {
            fixes: vec![
                fix(0, 3000),
                fix(3000, 6000),
                fix(6000, 9000),
                ColorFix { start_ms: None, end_ms: None, except: vec![(Some(0), Some(3000)), (Some(6000), Some(9000))], ..fix(0, 0) },
            ],
            auto_shots: 3,
            own_shots: 0,
            unified: true,
            skipped: 0,
        };
        // 预览从 3.2 秒处读起：只剩第 2 个镜头和统一调节，统一调节只留下和预览有交集的排除窗口
        let p = plan.shifted(3200);
        assert_eq!(p.fixes.len(), 2, "{:?}", p.fixes);
        assert_eq!((p.fixes[0].start_ms, p.fixes[0].end_ms), (Some(-200), Some(2800)));
        assert!(p.fixes[1].except.is_empty(), "{:?}", p.fixes[1].except);
        // 预览从第 3 个镜头开头读起：第 3 个镜头的排除窗口要留着
        let p = plan.shifted(6000);
        assert_eq!(p.fixes.len(), 2);
        assert_eq!(p.fixes[1].except, vec![(Some(0), Some(3000))]);
    }

    #[test]
    fn tone_adjust_maps_to_curves() {
        assert!(ToneAdjust::default().is_neutral());
        assert!(tone_plan(&ToneAdjust::default(), &[]).fixes.is_empty());
        let (y, u, v) = ToneAdjust::default().curves();
        assert!(y.is_none() && u.is_none() && v.is_none());
        // 亮度、对比度只动 Y
        let (y, u, v) = ToneAdjust { brightness: 0.5, contrast: -1.0, ..Default::default() }.curves();
        assert_eq!(y, Some(Curve { pivot: 126.0, gain: 0.5, out: 146.0 }));
        assert!(u.is_none() && v.is_none());
        // 饱和度只动 U / V（以 128 为轴缩放）
        let (y, u, v) = ToneAdjust { saturation: -1.0, ..Default::default() }.curves();
        assert!(y.is_none());
        assert_eq!((u, v), (Some(Curve { pivot: 128.0, gain: 0.0, out: 128.0 }), Some(Curve { pivot: 128.0, gain: 0.0, out: 128.0 })));
        // 偏暖：U 下降（少蓝）、V 上升（多红）；偏冷反过来；品红两个都升
        let (_, u, v) = ToneAdjust { temperature: 1.0, ..Default::default() }.curves();
        assert!(u.unwrap().out < 128.0 && v.unwrap().out > 128.0);
        let (_, u, v) = ToneAdjust { temperature: -1.0, ..Default::default() }.curves();
        assert!(u.unwrap().out > 128.0 && v.unwrap().out < 128.0);
        let (_, u, v) = ToneAdjust { tint: 1.0, ..Default::default() }.curves();
        assert!(u.unwrap().out > 128.0 && v.unwrap().out > 128.0);
        // 越界和非数字
        let c = ToneAdjust { brightness: 7.0, contrast: f64::NAN, saturation: -3.0, temperature: f64::INFINITY, tint: 0.12345 }.checked();
        assert_eq!((c.brightness, c.contrast, c.saturation, c.temperature, c.tint), (1.0, 0.0, -1.0, 0.0, 0.123));
        // 前端传来的 JSON：缺的项按 0
        let t: ToneAdjust = serde_json::from_str(r#"{"brightness":0.2,"tint":-0.4}"#).unwrap();
        assert_eq!((t.brightness, t.contrast, t.tint), (0.2, 0.0, -0.4));
        let s: ShotAdjust = serde_json::from_str(r#"{"id":4200,"tone":{"saturation":0.3}}"#).unwrap();
        assert_eq!((s.id, s.strength, s.tone.saturation), (4200, None, 0.3));
    }

    #[test]
    fn samples_pair_frames_with_times() {
        let (y, u, v) = frame(90, 128, 128);
        let mut raw = vec![];
        for _ in 0..3 {
            raw.extend(&y);
            raw.extend(&u);
            raw.extend(&v);
        }
        let s = parse_samples(&raw, &[0.0, 1.5, 3.04]);
        assert_eq!(s.len(), 3);
        assert_eq!((s[1].t_ms, s[2].t_ms), (1500, 3040));
        // 多出来的半帧丢掉；时间戳不够就少配
        assert_eq!(parse_samples(&raw[..FRAME_BYTES * 2 + 10], &[0.0, 1.0, 2.0]).len(), 2);
        assert_eq!(parse_samples(&raw, &[0.0]).len(), 1);
    }
}
