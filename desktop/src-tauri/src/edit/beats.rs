//! 节拍检测：从音频里找出每个拍子的时间和大致的 BPM，用来给配乐打节拍标记、让主轨按节拍切换。
//!
//! 纯 Rust、不用模型：把声音解成单声道 PCM → 分成低频 / 高频两路算短时能量 → 取对数后的正向变化（起拍强度）→
//! 用自相关估计一拍有多长 → 用动态规划（Ellis 2007 的思路）在起拍强度上挑出间隔均匀、又落在强起拍上的一串点。
//! 节奏不清晰的声音（环境音、古典乐）自相关峰很低，`confidence` 会很小，界面据此提示。

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::postprocess::{base_args, run_ffmpeg_capture};

/// 解码用的采样率；起拍检测不需要高频
pub const RATE: u32 = 11025;
/// 每帧间隔（采样点）≈ 23 毫秒
const HOP: usize = 256;
/// 每帧窗口（采样点）
const WIN: usize = 512;
const MIN_BPM: f64 = 60.0;
const MAX_BPM: f64 = 200.0;
/// 速度先验：太偏离这个就乘一个小于 1 的权重（避免把一拍误认成半拍 / 两拍）
const PRIOR_BPM: f64 = 120.0;
/// 一次最多分析这么长（毫秒）
pub const MAX_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Beats {
    pub bpm: f64,
    /// 0–1：越大节奏越清晰。低于 [`LOW_CONFIDENCE`] 时结果不太可信
    pub confidence: f64,
    /// 每一拍在素材里的时间（毫秒，从素材开头算）
    pub beats_ms: Vec<u64>,
}

pub const LOW_CONFIDENCE: f64 = 0.12;

fn frame_secs() -> f64 {
    HOP as f64 / f64::from(RATE)
}

/// 起拍强度曲线：每帧一个值（≥ 0），在强起拍处出现尖峰。
pub fn onset_strength(pcm: &[f32]) -> Vec<f64> {
    if pcm.len() < WIN * 4 {
        return vec![];
    }
    // 低频（贝斯鼓）用原信号的低通；高频（军鼓、镲、人声辅音）用一阶差分
    let mut low = Vec::with_capacity(pcm.len());
    let mut hi = Vec::with_capacity(pcm.len());
    let (mut lp, mut prev) = (0.0f32, 0.0f32);
    for &x in pcm {
        lp += 0.06 * (x - lp);
        low.push(lp);
        hi.push(x - prev);
        prev = x;
    }
    let energy = |sig: &[f32], start: usize| -> f64 {
        let end = (start + WIN).min(sig.len());
        let s: f64 = sig[start..end].iter().map(|&v| f64::from(v) * f64::from(v)).sum();
        (s / (end - start) as f64).sqrt()
    };
    let frames = (pcm.len() - WIN) / HOP;
    let mut out = vec![0.0f64; frames];
    for band in [&low, &hi] {
        let logs: Vec<f64> = (0..frames).map(|i| (1.0 + 100.0 * energy(band, i * HOP)).ln()).collect();
        let mut flux: Vec<f64> = (0..frames).map(|i| if i == 0 { 0.0 } else { (logs[i] - logs[i - 1]).max(0.0) }).collect();
        // 每路各自归一，免得低频总是压过高频
        let mean = flux.iter().sum::<f64>() / frames as f64;
        let sd = (flux.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / frames as f64).sqrt();
        if sd > 1e-9 {
            for v in &mut flux {
                *v = ((*v - mean) / sd).max(0.0);
            }
            for (o, v) in out.iter_mut().zip(&flux) {
                *o += v;
            }
        }
    }
    out
}

/// 一拍多长（帧数，可以是小数）和把握：在 `MIN_BPM`–`MAX_BPM` 里找自相关最高的。
fn tempo(onset: &[f64]) -> Option<(f64, f64)> {
    let n = onset.len();
    let fs = frame_secs();
    let lo = (60.0 / MAX_BPM / fs).floor() as usize;
    let hi = ((60.0 / MIN_BPM / fs).ceil() as usize).min(n / 2);
    if hi <= lo + 2 {
        return None;
    }
    let mean = onset.iter().sum::<f64>() / n as f64;
    let x: Vec<f64> = onset.iter().map(|v| v - mean).collect();
    let r0: f64 = x.iter().map(|v| v * v).sum();
    if r0 < 1e-9 {
        return None;
    }
    let acf: Vec<f64> = (0..=hi).map(|lag| if lag < lo { 0.0 } else { x[lag..].iter().zip(&x).map(|(a, b)| a * b).sum::<f64>() / r0 }).collect();
    // 一拍的自相关峰，加上两拍、四拍处的峰一起算（周期性强的节奏三个都高），再乘速度先验
    let score = |lag: usize| -> f64 {
        let at = |m: usize| -> f64 {
            let l = lag * m;
            if l > hi {
                0.0
            } else {
                acf[l.saturating_sub(1)..=(l + 1).min(hi)].iter().copied().fold(f64::MIN, f64::max)
            }
        };
        let bpm = 60.0 / (lag as f64 * fs);
        let prior = (-0.5 * (bpm / PRIOR_BPM).log2().powi(2) / 0.8f64.powi(2)).exp();
        (acf[lag] + 0.5 * at(2) + 0.25 * at(4)) * prior
    };
    let best = (lo.max(1)..=hi).max_by(|&a, &b| score(a).total_cmp(&score(b)))?;
    // 抛物线插值，拿到小数帧
    let (a, b, c) = (acf[best.saturating_sub(1)], acf[best], acf[(best + 1).min(hi)]);
    let denom = a - 2.0 * b + c;
    let off = if denom.abs() > 1e-12 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
    Some((best as f64 + off, acf[best].clamp(0.0, 1.0)))
}

/// 在起拍强度上挑出一串间隔接近 `period`（帧）的点，使“强度之和 − 间隔偏差惩罚”最大。
fn track(onset: &[f64], period: f64) -> Vec<usize> {
    let n = onset.len();
    let tight = 100.0;
    let mut score = onset.to_vec();
    let mut back = vec![usize::MAX; n];
    let (lo, hi) = ((period * 0.5).floor() as usize, (period * 2.0).ceil() as usize);
    for i in 0..n {
        let mut best = (f64::MIN, usize::MAX);
        for gap in lo.max(1)..=hi {
            if gap > i {
                break;
            }
            let j = i - gap;
            let pen = tight * (gap as f64 / period).ln().powi(2);
            let v = score[j] - pen;
            if v > best.0 {
                best = (v, j);
            }
        }
        // 比“从头开始”划算才接上前一个
        if best.1 != usize::MAX && best.0 > 0.0 {
            score[i] = onset[i] + best.0;
            back[i] = best.1;
        }
    }
    // 从最后一段里分最高的位置回溯
    let tail = n.saturating_sub(period as usize * 2);
    let Some(end) = (tail..n).max_by(|&a, &b| score[a].total_cmp(&score[b])) else { return vec![] };
    let mut v = vec![end];
    while back[*v.last().unwrap()] != usize::MAX {
        v.push(back[*v.last().unwrap()]);
    }
    v.reverse();
    v
}

/// 从单声道 PCM（[`RATE`] 采样率）里找节拍；`offset_ms` 是这段 PCM 在素材里的起点。
pub fn detect(pcm: &[f32], offset_ms: u64) -> Option<Beats> {
    let onset = onset_strength(pcm);
    if onset.len() < 200 {
        return None;
    }
    let (period, conf) = tempo(&onset)?;
    let fs = frame_secs();
    let picks = track(&onset, period);
    if picks.len() < 4 {
        return None;
    }
    let bpm = 60.0 / (period * fs);
    // 起拍强度是“能量开始上升的那一帧”，窗口中心比它晚半个窗口
    let delay = (WIN as f64 / 2.0) / f64::from(RATE) * 0.5;
    let beats_ms = picks.iter().map(|&i| offset_ms + (((i as f64 * fs) + delay) * 1000.0).round() as u64).collect();
    Some(Beats { bpm: (bpm * 10.0).round() / 10.0, confidence: (conf * 100.0).round() / 100.0, beats_ms })
}

/// 解码一段音频成单声道 PCM 再找节拍。
pub async fn analyze(ffmpeg: &Path, path: &str, from_ms: u64, to_ms: u64) -> AppResult<Beats> {
    if !Path::new(path).is_file() {
        return Err(AppError::not_found("找不到素材文件。"));
    }
    if to_ms <= from_ms {
        return Err(AppError::invalid("范围不对：结束要比开始晚。"));
    }
    let to = to_ms.min(from_ms + MAX_MS);
    let mut a = base_args();
    a.extend([
        "-ss".into(),
        format!("{:.3}", from_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", (to - from_ms) as f64 / 1000.0),
        "-i".into(),
        path.to_string(),
    ]);
    a.extend(["-vn".into(), "-ac".into(), "1".into(), "-ar".into(), RATE.to_string(), "-f".into(), "f32le".into(), "-".into()]);
    let (raw, _) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(600)).await.map_err(|e| AppError::msg(format!("读取声音失败：{e}")))?;
    let pcm: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    detect(&pcm, from_ms).ok_or_else(|| AppError::msg("没有找到节拍：声音太短、太安静，或者没有明显的节奏。"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每 `period` 秒一声短促的“咚”（衰减的 80 Hz 正弦），其余是很轻的底噪
    fn clicks(period: f64, secs: f64, offset: f64) -> Vec<f32> {
        let n = (secs * f64::from(RATE)) as usize;
        let mut v = vec![0.0f32; n];
        let mut seed = 12345u32;
        for s in v.iter_mut() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *s = ((seed >> 16) as f32 / 65536.0 - 0.5) * 0.002;
        }
        let mut t = offset;
        while t < secs {
            let start = (t * f64::from(RATE)) as usize;
            for k in 0..(0.08 * f64::from(RATE)) as usize {
                if start + k < n {
                    let tt = k as f32 / RATE as f32;
                    v[start + k] += (tt * 80.0 * std::f32::consts::TAU).sin() * (-tt * 40.0).exp() * 0.8;
                }
            }
            t += period;
        }
        v
    }

    #[test]
    fn finds_the_tempo_and_places_beats_on_the_clicks() {
        let b = detect(&clicks(0.5, 16.0, 0.3), 0).expect("检测到节拍");
        assert!((b.bpm - 120.0).abs() < 3.0, "bpm = {}", b.bpm);
        assert!(b.confidence > LOW_CONFIDENCE, "confidence = {}", b.confidence);
        // 每一拍都离最近的真实“咚”不超过 60 毫秒
        let worst =
            b.beats_ms.iter().map(|&t| ((t as f64 / 1000.0 - 0.3).rem_euclid(0.5)).min(0.5 - (t as f64 / 1000.0 - 0.3).rem_euclid(0.5))).fold(0.0, f64::max);
        assert!(worst < 0.06, "最大偏差 {worst} 秒，beats = {:?}", b.beats_ms);
        assert!(b.beats_ms.len() >= 28 && b.beats_ms.len() <= 33, "拍数 {}", b.beats_ms.len());
        // 起点偏移被加上
        let shifted = detect(&clicks(0.5, 16.0, 0.3), 10_000).unwrap();
        assert!(shifted.beats_ms[0] >= 10_000);
    }

    #[test]
    fn other_tempos_work_too() {
        let b = detect(&clicks(0.4, 16.0, 0.1), 0).unwrap();
        assert!((b.bpm - 150.0).abs() < 4.0, "bpm = {}", b.bpm);
        let b = detect(&clicks(0.75, 20.0, 0.2), 0).unwrap();
        assert!((b.bpm - 80.0).abs() < 3.0, "bpm = {}", b.bpm);
    }

    #[test]
    fn noise_has_low_confidence_and_silence_has_none() {
        let mut seed = 99u32;
        let noise: Vec<f32> = (0..(RATE as usize * 12))
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                ((seed >> 16) as f32 / 65536.0 - 0.5) * 0.3
            })
            .collect();
        if let Some(b) = detect(&noise, 0) {
            assert!(b.confidence < 0.2, "噪声的把握应该很低：{}", b.confidence);
        }
        assert!(detect(&vec![0.0; RATE as usize * 10], 0).is_none());
        assert!(detect(&[0.0; 100], 0).is_none());
    }
}
