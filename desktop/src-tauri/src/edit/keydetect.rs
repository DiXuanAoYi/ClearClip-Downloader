//! 自动识别绿幕 / 蓝幕：取一帧，看画面四周一圈里有没有一大片饱和的绿色或蓝色，有就估出键色和合适的相似度。
//! 不用模型。背景不是纯色、或者人物占满画面边缘时会识别不出来，界面据此提示手动选色。

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::postprocess::{base_args, run_ffmpeg_capture};

/// 分析用的帧宽
const WIDTH: usize = 320;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyGuess {
    /// `#RRGGBB`
    pub color: String,
    pub similarity: f64,
    pub blend: f64,
    /// 整个画面里和键色接近的比例 0–1
    pub coverage: f64,
}

fn hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d < 1e-9 {
        0.0
    } else if mx == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if mx < 1e-9 { 0.0 } else { d / mx }, mx)
}

/// 色度平面上的距离（和 ffmpeg `chromakey` 的相似度同一个尺度：U、V 各 0–255，除以 255）
fn chroma_dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() / 255.0
}

fn cbcr(r: u8, g: u8, b: u8) -> (f64, f64) {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    (128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b, 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b)
}

/// 在 RGB24 的一帧里找键色。
pub fn detect(rgb: &[u8], w: usize, h: usize) -> Option<KeyGuess> {
    if w < 16 || h < 16 || rgb.len() < w * h * 3 {
        return None;
    }
    let band_x = (w / 10).max(2);
    let band_y = (h / 10).max(2);
    let at = |x: usize, y: usize| (rgb[(y * w + x) * 3], rgb[(y * w + x) * 3 + 1], rgb[(y * w + x) * 3 + 2]);
    let mut border = 0usize;
    let mut bins = [0usize; 36];
    for y in 0..h {
        for x in 0..w {
            if x >= band_x && x < w - band_x && y >= band_y && y < h - band_y {
                continue;
            }
            border += 1;
            let (r, g, b) = at(x, y);
            let (hu, s, v) = hsv(r, g, b);
            if s >= 0.35 && v >= 0.2 && ((70.0..=170.0).contains(&hu) || (190.0..=260.0).contains(&hu)) {
                bins[(hu / 10.0) as usize % 36] += 1;
            }
        }
    }
    let (best, count) = (0..36).map(|i| (i, bins[i] + bins[(i + 1) % 36] + bins[(i + 35) % 36])).max_by_key(|&(_, n)| n)?;
    if (count as f64) < border as f64 * 0.25 {
        return None;
    }
    let center = best as f64 * 10.0 + 5.0;
    let near = |hu: f64| {
        let d = (hu - center).abs();
        d.min(360.0 - d) <= 20.0
    };
    // 键色：边缘一圈里属于这个色相的像素的平均颜色
    let (mut sr, mut sg, mut sb, mut n) = (0f64, 0f64, 0f64, 0f64);
    for y in 0..h {
        for x in 0..w {
            if x >= band_x && x < w - band_x && y >= band_y && y < h - band_y {
                continue;
            }
            let (r, g, b) = at(x, y);
            let (hu, s, v) = hsv(r, g, b);
            if s >= 0.35 && v >= 0.2 && near(hu) {
                sr += f64::from(r);
                sg += f64::from(g);
                sb += f64::from(b);
                n += 1.0;
            }
        }
    }
    if n < 1.0 {
        return None;
    }
    let key = ((sr / n).round() as u8, (sg / n).round() as u8, (sb / n).round() as u8);
    let kc = cbcr(key.0, key.1, key.2);
    // 相似度：边缘上这类像素离键色的 95 分位距离，再留一点余量
    let mut dists: Vec<f64> = vec![];
    for y in 0..h {
        for x in 0..w {
            if x >= band_x && x < w - band_x && y >= band_y && y < h - band_y {
                continue;
            }
            let (r, g, b) = at(x, y);
            let (hu, s, v) = hsv(r, g, b);
            if s >= 0.35 && v >= 0.2 && near(hu) {
                dists.push(chroma_dist(cbcr(r, g, b), kc));
            }
        }
    }
    dists.sort_by(f64::total_cmp);
    let p95 = dists[(dists.len() as f64 * 0.95) as usize % dists.len()];
    let similarity = ((p95 * 1.25 + 0.06) * 100.0).round() / 100.0;
    let similarity = similarity.clamp(0.08, 0.45);
    let covered = (0..w * h).filter(|&i| chroma_dist(cbcr(rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]), kc) <= similarity).count();
    Some(KeyGuess { color: format!("#{:02X}{:02X}{:02X}", key.0, key.1, key.2), similarity, blend: 0.08, coverage: covered as f64 / (w * h) as f64 })
}

/// 取 `at_ms` 处的一帧并识别。
pub async fn analyze(ffmpeg: &Path, path: &str, at_ms: u64) -> AppResult<Option<KeyGuess>> {
    let mut a = base_args();
    a.extend(["-ss".into(), format!("{:.3}", at_ms as f64 / 1000.0), "-i".into(), path.to_string()]);
    a.extend(["-frames:v".into(), "1".into(), "-vf".into(), format!("scale={WIDTH}:-2")]);
    a.extend(["-f".into(), "rawvideo".into(), "-pix_fmt".into(), "rgb24".into(), "-".into()]);
    let (raw, _) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(30)).await?;
    // 高度由 ffmpeg 决定：长度 / (宽 × 3)
    if raw.len() < WIDTH * 3 * 16 || raw.len() % (WIDTH * 3) != 0 {
        return Err(AppError::msg("取不到这一帧画面。"));
    }
    Ok(detect(&raw, WIDTH, raw.len() / (WIDTH * 3)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bg: (u8, u8, u8), noise: u8) -> Vec<u8> {
        let (w, h) = (160, 120);
        let mut v = Vec::with_capacity(w * h * 3);
        let mut seed = 7u32;
        for y in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let j = ((seed >> 24) as i32 % (i32::from(noise) * 2 + 1)) - i32::from(noise);
                let inside = (50..110).contains(&x) && (30..90).contains(&y);
                let c = if inside { (200u8, 60u8, 50u8) } else { bg };
                for ch in [c.0, c.1, c.2] {
                    v.push((i32::from(ch) + if inside { 0 } else { j }).clamp(0, 255) as u8);
                }
            }
        }
        v
    }

    #[test]
    fn finds_green_and_blue_screens_with_noise() {
        let g = detect(&frame((20, 190, 60), 14), 160, 120).expect("green");
        assert!(g.color.starts_with('#') && u8::from_str_radix(&g.color[3..5], 16).unwrap() > 150, "{g:?}");
        assert!((0.08..=0.45).contains(&g.similarity) && g.coverage > 0.5, "{g:?}");
        let b = detect(&frame((30, 60, 200), 10), 160, 120).expect("blue");
        assert!(u8::from_str_radix(&b.color[5..7], 16).unwrap() > 150, "{b:?}");
    }

    #[test]
    fn no_screen_means_none() {
        assert!(detect(&frame((120, 110, 100), 20), 160, 120).is_none(), "灰褐色背景不是绿幕");
        assert!(detect(&frame((200, 30, 30), 5), 160, 120).is_none(), "红色背景也不是");
        assert!(detect(&[0; 10], 160, 120).is_none());
    }
}
