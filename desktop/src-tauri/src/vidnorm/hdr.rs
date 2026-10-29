//! HDR → SDR 的程序内置做法：生成一张 3D LUT（.cube），用 ffmpeg 自带的 `lut3d` 滤镜应用。
//!
//! 精简版 ffmpeg 没有 zscale（libzimg）时，高质量的色调映射（zscale + tonemap）用不了，但 `lut3d` 是所有构建都有的。
//! 这张表把 “PQ / HLG 编码的 BT.2020 RGB” 映射成 “BT.709 伽马编码的 SDR RGB”：
//! 解码成线性光 → 暗部略抬高 → BT.2020 转 BT.709 色域 → Hable 色调映射（按最大通道，保持色相）→ BT.709 伽马。
//! 常数（`WHITE_NITS`、`PEAK_NITS`、`SHADOW_LIFT`）是拿 zscale + tonemap 的结果在 PQ / HLG 灰阶和纯色上对着调的，中间调相差不到 10/255。

use super::facts::Hdr;

/// 3D LUT 每边的格点数。33 对 PQ 这种感知均匀的编码够用，配合四面体插值没有可见的色带。
pub const LUT_SIZE: usize = 33;
/// 色调映射的峰值（尼特）：线性光里的 12 倍（ffmpeg tonemap 没有元数据时假定峰值是白点的 12 倍）。
const PEAK_NITS: f64 = 900.0;
/// 暗部抬高的量（线性光，以 `WHITE_NITS` 为 1.0）。
const SHADOW_LIFT: f64 = 0.03;
/// 线性光里 1.0 对应的亮度（尼特）。
const WHITE_NITS: f64 = 75.0;

fn pq_to_nits(p: f64) -> f64 {
    const M1: f64 = 2610.0 / 16384.0;
    const M2: f64 = 2523.0 / 4096.0 * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = 2413.0 / 4096.0 * 32.0;
    const C3: f64 = 2392.0 / 4096.0 * 32.0;
    let p = p.clamp(0.0, 1.0).powf(1.0 / M2);
    10000.0 * ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1)
}

/// HLG 逆 OETF：信号 → 场景线性光（0–1）。
fn hlg_inverse_oetf(e: f64) -> f64 {
    const A: f64 = 0.178_832_77;
    const B: f64 = 0.284_668_92;
    const C: f64 = 0.559_910_73;
    let e = e.clamp(0.0, 1.0);
    if e <= 0.5 {
        e * e / 3.0
    } else {
        (((e - C) / A).exp() + B) / 12.0
    }
}

fn hable(x: f64) -> f64 {
    const A: f64 = 0.15;
    const B: f64 = 0.50;
    const C: f64 = 0.10;
    const D: f64 = 0.20;
    const E: f64 = 0.02;
    const F: f64 = 0.30;
    (x * (x * A + B * C) + D * E) / (x * (x * A + B) + D * F) - E / F
}

fn bt709_oetf(l: f64) -> f64 {
    let l = l.clamp(0.0, 1.0);
    if l < 0.018 {
        4.5 * l
    } else {
        1.099 * l.powf(0.45) - 0.099
    }
}

/// 一个 HDR 编码的 BT.2020 RGB（0–1）→ SDR BT.709 编码 RGB（0–1）。
pub fn map_pixel(kind: Hdr, rgb: [f64; 3]) -> [f64; 3] {
    map_pixel_with(kind, rgb, WHITE_NITS, PEAK_NITS)
}

fn map_pixel_with(kind: Hdr, rgb: [f64; 3], white_nits: f64, peak_nits: f64) -> [f64; 3] {
    // 1. 解码成以 100 尼特为 1.0 的线性光
    let lin: [f64; 3] = match kind {
        Hdr::Hlg => {
            let e = [hlg_inverse_oetf(rgb[0]), hlg_inverse_oetf(rgb[1]), hlg_inverse_oetf(rgb[2])];
            let ys = 0.2627 * e[0] + 0.6780 * e[1] + 0.0593 * e[2];
            // OOTF：系统伽马 1.2（1000 尼特峰值）
            let gain = 1000.0 * ys.max(1e-9).powf(0.2) / white_nits;
            [e[0] * gain, e[1] * gain, e[2] * gain]
        }
        _ => [pq_to_nits(rgb[0]) / white_nits, pq_to_nits(rgb[1]) / white_nits, pq_to_nits(rgb[2]) / white_nits],
    };
    // 暗部略微抬高：zscale 的结果里暗部比纯数学映射亮一些（保留暗部层次），这里按同样的观感补上；纯黑不受影响
    let lift = |x: f64| x + SHADOW_LIFT * (1.0 - (-x / 0.01).exp());
    // 2. BT.2020 → BT.709 色域，超出色域的负值截为 0
    let (r, g, b) = (lift(lin[0]), lift(lin[1]), lift(lin[2]));
    let m = [
        (1.660_491 * r - 0.587_641 * g - 0.072_850 * b).max(0.0),
        (-0.124_550 * r + 1.132_900 * g - 0.008_349 * b).max(0.0),
        (-0.018_151 * r - 0.100_579 * g + 1.118_730 * b).max(0.0),
    ];
    // 3. Hable 色调映射：按最大通道算缩放系数，三个通道乘同一个系数，保持色相
    let sig = m[0].max(m[1]).max(m[2]).max(1e-6);
    let peak = peak_nits / white_nits;
    let scale = hable(sig) / hable(peak) / sig;
    let mapped = [m[0] * scale, m[1] * scale, m[2] * scale];
    // 4. BT.709 伽马
    [bt709_oetf(mapped[0]), bt709_oetf(mapped[1]), bt709_oetf(mapped[2])]
}

/// 生成 `.cube` 文件内容。红色变化最快，其次绿、蓝。
pub fn cube(kind: Hdr) -> String {
    cube_with(kind, WHITE_NITS, PEAK_NITS)
}

fn cube_with(kind: Hdr, white_nits: f64, peak_nits: f64) -> String {
    let n = LUT_SIZE;
    let mut out = String::with_capacity(n * n * n * 24 + 128);
    out.push_str(&format!("TITLE \"ClearClip HDR to SDR ({})\"\nLUT_3D_SIZE {n}\nDOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\n", kind.label()));
    let step = 1.0 / (n - 1) as f64;
    for bi in 0..n {
        for gi in 0..n {
            for ri in 0..n {
                let o = map_pixel_with(kind, [ri as f64 * step, gi as f64 * step, bi as f64 * step], white_nits, peak_nits);
                out.push_str(&format!("{:.6} {:.6} {:.6}\n", o[0], o[1], o[2]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_stays_black_and_white_does_not_clip_badly() {
        for kind in [Hdr::Pq, Hdr::Hlg] {
            assert_eq!(map_pixel(kind, [0.0; 3]), [0.0; 3]);
            let w = map_pixel(kind, [1.0; 3]);
            for c in w {
                assert!((0.9..=1.0001).contains(&c), "{kind:?} {c}");
            }
        }
    }

    #[test]
    fn mapping_is_monotonic_and_keeps_neutrals_neutral() {
        for kind in [Hdr::Pq, Hdr::Hlg] {
            let mut prev = -1.0;
            for i in 0..=100 {
                let v = i as f64 / 100.0;
                let o = map_pixel(kind, [v, v, v]);
                assert!(o[0] >= prev - 1e-9, "{kind:?} at {v}: {} < {prev}", o[0]);
                assert!((o[0] - o[1]).abs() < 1e-6 && (o[1] - o[2]).abs() < 1e-6, "灰色应保持灰色");
                prev = o[0];
            }
        }
    }

    #[test]
    fn pq_reference_points() {
        // PQ 0.5081 ≈ 100 尼特；0.7518 ≈ 1000 尼特；1.0 = 10000 尼特
        assert!((pq_to_nits(0.5081) - 100.0).abs() < 2.0, "{}", pq_to_nits(0.5081));
        assert!((pq_to_nits(0.7518) - 1000.0).abs() < 15.0, "{}", pq_to_nits(0.7518));
        assert!((pq_to_nits(1.0) - 10000.0).abs() < 1.0);
        // HLG 信号 0.5 对应场景线性光 1/12
        assert!((hlg_inverse_oetf(0.5) - 1.0 / 12.0).abs() < 1e-9);
        assert!((hlg_inverse_oetf(1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn saturated_colors_stay_in_range() {
        for kind in [Hdr::Pq, Hdr::Hlg] {
            for rgb in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.9, 0.2, 0.7]] {
                let o = map_pixel(kind, rgb);
                assert!(o.iter().all(|c| (0.0..=1.0001).contains(c)), "{kind:?} {rgb:?} → {o:?}");
            }
        }
    }

    #[test]
    fn cube_has_the_right_shape() {
        let c = cube(Hdr::Pq);
        let lines: Vec<&str> = c.lines().collect();
        assert!(lines[0].starts_with("TITLE") && lines[1] == format!("LUT_3D_SIZE {LUT_SIZE}"));
        let data = &lines[4..];
        assert_eq!(data.len(), LUT_SIZE.pow(3));
        // 第二行数据是红色走了一格
        let first: Vec<f64> = data[1].split(' ').map(|v| v.parse().unwrap()).collect();
        assert!(first[0] > 0.0 && first[1] == 0.0 && first[2] == 0.0);
        // 最后一行是白色
        let last: Vec<f64> = data[data.len() - 1].split(' ').map(|v| v.parse().unwrap()).collect();
        assert!(last.iter().all(|v| *v > 0.9));
    }
}
