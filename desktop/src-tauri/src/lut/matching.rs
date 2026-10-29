//! 参考图匹配：让示例图片的明暗分布和偏色、饱和度向一张参考图靠拢，结果是一个“只和颜色有关”的映射，所以能烘焙进 LUT。
//!
//! 在 CIE Lab 里做：
//! - 明暗（L）：直方图匹配——把示例图片的亮度分位数对到参考图的亮度分位数，曲线再平滑，两端沿用斜率 1 延伸；
//! - 色彩（a、b）：均值和标准差迁移（Reinhard 色彩迁移），标准差的缩放限制在 0.4–2.5 倍，高光和暗部的迁移减弱，保持黑白干净。

use super::adjust::{linear_to_srgb, srgb_to_linear, Rgb};

const XN: f32 = 0.950_47;
const ZN: f32 = 1.088_83;

fn f_lab(t: f32) -> f32 {
    if t > 0.008_856 {
        t.cbrt()
    } else {
        7.787 * t + 16.0 / 116.0
    }
}

fn f_lab_inv(t: f32) -> f32 {
    let t3 = t * t * t;
    if t3 > 0.008_856 {
        t3
    } else {
        (t - 16.0 / 116.0) / 7.787
    }
}

/// sRGB（gamma 编码，0–1）→ CIE Lab（D65）。
pub fn rgb_to_lab(v: Rgb) -> [f32; 3] {
    let [r, g, b] = v.map(srgb_to_linear);
    let x = 0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = 0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b;
    let (fx, fy, fz) = (f_lab(x / XN), f_lab(y), f_lab(z / ZN));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE Lab → sRGB（gamma 编码）。超出色域的值不截断。
pub fn lab_to_rgb(lab: [f32; 3]) -> Rgb {
    let fy = (lab[0] + 16.0) / 116.0;
    let (fx, fz) = (lab[1] / 500.0 + fy, fy - lab[2] / 200.0);
    let (x, y, z) = (f_lab_inv(fx) * XN, f_lab_inv(fy), f_lab_inv(fz) * ZN);
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    [linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b)]
}

const TABLE: usize = 256;

/// 一个已经拟合好的匹配。
#[derive(Debug, Clone)]
pub struct Transfer {
    /// L 从 0 到 100 均匀取 `TABLE` 个点，对应的目标 L
    l_curve: Vec<f32>,
    /// a' = a * scale + offset
    a: (f32, f32),
    b: (f32, f32),
    tone: f32,
    color: f32,
}

fn quantile(sorted: &[f32], p: f32) -> f32 {
    let pos = p.clamp(0.0, 1.0) * (sorted.len() - 1) as f32;
    let (i, f) = (pos.floor() as usize, pos - pos.floor());
    let j = (i + 1).min(sorted.len() - 1);
    sorted[i] + (sorted[j] - sorted[i]) * f
}

fn mean_std(v: &[f32]) -> (f32, f32) {
    let n = v.len() as f32;
    let m = v.iter().sum::<f32>() / n;
    let var = v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / n;
    (m, var.sqrt())
}

/// 把多个 (src, ref) 点连成分段线性曲线，两端斜率为 1 向外延伸。
fn piecewise(points: &[(f32, f32)], x: f32) -> f32 {
    let (first, last) = (points[0], points[points.len() - 1]);
    if x <= first.0 {
        return first.1 + (x - first.0);
    }
    if x >= last.0 {
        return last.1 + (x - last.0);
    }
    let i = points.partition_point(|p| p.0 <= x).clamp(1, points.len() - 1);
    let (a, b) = (points[i - 1], points[i]);
    a.1 + (b.1 - a.1) * ((x - a.0) / (b.0 - a.0).max(1e-6))
}

impl Transfer {
    /// 由示例图片（已经过前面的调整）和参考图的像素拟合。两边都至少要有 64 个像素；强度都是 0 时返回 None。
    pub fn fit(sample: &[Rgb], reference: &[Rgb], tone: f32, color: f32) -> Option<Transfer> {
        if (tone <= 0.0 && color <= 0.0) || sample.len() < 64 || reference.len() < 64 {
            return None;
        }
        let ls: Vec<[f32; 3]> = sample.iter().map(|p| rgb_to_lab(*p)).collect();
        let lr: Vec<[f32; 3]> = reference.iter().map(|p| rgb_to_lab(*p)).collect();
        let col = |v: &[[f32; 3]], k: usize| v.iter().map(|p| p[k]).collect::<Vec<f32>>();

        // 明暗：分位数对分位数，去掉两头 0.5% 的离群值
        let sorted = |mut v: Vec<f32>| {
            v.sort_by(f32::total_cmp);
            v
        };
        let (s_l, r_l) = (sorted(col(&ls, 0)), sorted(col(&lr, 0)));
        const K: usize = 65;
        let mut points: Vec<(f32, f32)> = Vec::with_capacity(K);
        let mut group: Vec<f32> = vec![];
        let mut group_src = f32::MIN;
        for i in 0..K {
            let p = 0.005 + 0.99 * i as f32 / (K - 1) as f32;
            let (sq, rq) = (quantile(&s_l, p), quantile(&r_l, p));
            // 源分位数几乎相同（大片同亮度的区域）：目标取这一组的平均，避免曲线出现竖直的台阶
            if sq - group_src > 0.25 {
                if !group.is_empty() {
                    points.push((group_src, group.iter().sum::<f32>() / group.len() as f32));
                }
                group_src = sq;
                group.clear();
            }
            group.push(rq);
        }
        points.push((group_src, group.iter().sum::<f32>() / group.len() as f32));
        let mut curve: Vec<f32> = (0..TABLE).map(|i| piecewise(&points, i as f32 * 100.0 / (TABLE - 1) as f32).clamp(0.0, 100.0)).collect();
        // 平滑：三次方框滤波（相当于高斯），再保证不递减
        for _ in 0..3 {
            let src = curve.clone();
            for (i, c) in curve.iter_mut().enumerate() {
                let (lo, hi) = (i.saturating_sub(6), (i + 6).min(TABLE - 1));
                *c = src[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32;
            }
        }
        for i in 1..TABLE {
            curve[i] = curve[i].max(curve[i - 1]);
        }

        // 色彩：a、b 的均值和标准差
        let chan = |k: usize| {
            let (ms, ss) = mean_std(&col(&ls, k));
            let (mr, sr) = mean_std(&col(&lr, k));
            // 示例图片几乎没有色彩（标准差很小）时只平移，不缩放
            let scale = if ss > 1.0 { (sr / ss).clamp(0.4, 2.5) } else { 1.0 };
            (scale, mr - ms * scale)
        };
        Some(Transfer { l_curve: curve, a: chan(1), b: chan(2), tone, color })
    }

    fn curve_at(&self, l: f32) -> f32 {
        let pos = (l / 100.0).clamp(0.0, 1.0) * (TABLE - 1) as f32;
        let (i, f) = (pos.floor() as usize, pos - pos.floor());
        let j = (i + 1).min(TABLE - 1);
        let base = self.l_curve[i] + (self.l_curve[j] - self.l_curve[i]) * f;
        // 超出 0–100 的 L（超色域）按斜率 1 延伸
        base + (l - l.clamp(0.0, 100.0))
    }

    pub fn apply(&self, v: Rgb) -> Rgb {
        let [l, a, b] = rgb_to_lab(v);
        let l2 = l + (self.curve_at(l) - l) * self.tone;
        // 高光和暗部的色彩迁移减弱，纯黑纯白不被染色
        let w = 1.0 - 0.6 * ((l - 50.0) / 50.0).powi(2);
        let k = self.color * w;
        let (a2, b2) = (a + (a * self.a.0 + self.a.1 - a) * k, b + (b * self.b.0 + self.b.1 - b) * k);
        lab_to_rgb([l2, a2, b2])
    }

    #[cfg(test)]
    pub fn l_curve_at(&self, l: f32) -> f32 {
        self.curve_at(l)
    }
}

/// 从一堆像素里均匀取出不超过 `max` 个。
pub fn thin(pixels: &[Rgb], max: usize) -> Vec<Rgb> {
    if pixels.len() <= max {
        return pixels.to_vec();
    }
    let step = pixels.len() as f64 / max as f64;
    (0..max).map(|i| pixels[(i as f64 * step) as usize]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 简单的确定性伪随机数，保证测试可重复。
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32) / (1u64 << 31) as f32
        }
    }

    /// 造一批“照片般”的像素：亮度在 [lo, hi] 里均匀分布，色彩带一个偏色。
    fn photo(n: usize, lo: f32, hi: f32, tint: Rgb, seed: u64) -> Vec<Rgb> {
        let mut r = Rng(seed);
        (0..n)
            .map(|_| {
                let y = lo + (hi - lo) * r.next();
                let jitter = [r.next() - 0.5, r.next() - 0.5, r.next() - 0.5];
                [0, 1, 2].map(|k| (y + tint[k] * (0.4 + y) + jitter[k] * 0.1).clamp(0.0, 1.0))
            })
            .collect()
    }

    fn lab_stats(px: &[Rgb]) -> ([f32; 3], [f32; 3]) {
        let labs: Vec<[f32; 3]> = px.iter().map(|p| rgb_to_lab(*p)).collect();
        let mut m = [0.0; 3];
        let mut s = [0.0; 3];
        for k in 0..3 {
            let col: Vec<f32> = labs.iter().map(|l| l[k]).collect();
            let (mm, ss) = mean_std(&col);
            m[k] = mm;
            s[k] = ss;
        }
        (m, s)
    }

    #[test]
    fn lab_round_trip_and_known_values() {
        for r in [0.0, 0.2, 0.5, 1.0] {
            for g in [0.0, 0.3, 0.9] {
                for b in [0.0, 0.4, 1.0] {
                    let back = lab_to_rgb(rgb_to_lab([r, g, b]));
                    assert!(back.iter().zip([r, g, b]).all(|(x, y)| (x - y).abs() < 2e-3), "{r} {g} {b} → {back:?}");
                }
            }
        }
        let white = rgb_to_lab([1.0; 3]);
        assert!((white[0] - 100.0).abs() < 0.05 && white[1].abs() < 0.1 && white[2].abs() < 0.1, "{white:?}");
        let red = rgb_to_lab([1.0, 0.0, 0.0]);
        assert!((red[0] - 53.24).abs() < 0.1 && (red[1] - 80.09).abs() < 0.2 && (red[2] - 67.2).abs() < 0.2, "{red:?}");
        assert!(rgb_to_lab([0.0; 3])[0].abs() < 1e-3);
    }

    #[test]
    fn no_strength_or_too_few_pixels_means_no_transfer() {
        let a = photo(500, 0.2, 0.8, [0.0; 3], 1);
        assert!(Transfer::fit(&a, &a, 0.0, 0.0).is_none());
        assert!(Transfer::fit(&a[..10], &a, 1.0, 1.0).is_none());
        assert!(Transfer::fit(&a, &a[..10], 1.0, 1.0).is_none());
    }

    #[test]
    fn matching_an_image_to_itself_changes_almost_nothing() {
        let a = photo(4000, 0.1, 0.9, [0.05, 0.0, -0.05], 7);
        let t = Transfer::fit(&a, &a, 1.0, 1.0).unwrap();
        let mut worst = 0.0f32;
        for p in a.iter().step_by(13) {
            let o = t.apply(*p);
            worst = worst.max(o.iter().zip(p).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max));
        }
        assert!(worst < 0.03, "自己对自己最多差 {worst}");
    }

    #[test]
    fn the_sample_ends_up_with_the_references_tone_and_color_statistics() {
        // 示例：低对比、偏暖、偏暗；参考：高对比、偏冷、偏亮
        let sample = photo(6000, 0.25, 0.55, [0.10, 0.02, -0.10], 11);
        let reference = photo(6000, 0.05, 0.95, [-0.08, 0.0, 0.10], 23);
        let t = Transfer::fit(&sample, &reference, 1.0, 1.0).unwrap();
        let out: Vec<Rgb> = sample.iter().map(|p| t.apply(*p).map(|c| c.clamp(0.0, 1.0))).collect();
        let (ms, ss) = lab_stats(&sample);
        let (mr, sr) = lab_stats(&reference);
        let (mo, so) = lab_stats(&out);
        // 亮度的均值和对比度向参考靠拢
        assert!((mo[0] - mr[0]).abs() < 4.0, "L 均值 {} 目标 {} 原来 {}", mo[0], mr[0], ms[0]);
        assert!((so[0] - sr[0]).abs() < 0.25 * sr[0], "L 标准差 {} 目标 {} 原来 {}", so[0], sr[0], ss[0]);
        // 偏色（a、b 均值）移向参考：偏暖 → 偏冷
        for k in 1..3 {
            assert!((mo[k] - mr[k]).abs() < (ms[k] - mr[k]).abs() * 0.35 + 1.5, "通道 {k}：输出 {} 参考 {} 原来 {}", mo[k], mr[k], ms[k]);
        }
        assert!(out.iter().flatten().all(|c| c.is_finite()));
    }

    #[test]
    fn strengths_are_independent_and_zero_is_identity() {
        let sample = photo(3000, 0.25, 0.55, [0.10, 0.0, -0.10], 3);
        let reference = photo(3000, 0.05, 0.95, [-0.10, 0.0, 0.10], 5);
        let probe = [0.45, 0.4, 0.3];
        // 只匹配明暗：色相不变（a、b 不动），亮度变
        let tone_only = Transfer::fit(&sample, &reference, 1.0, 0.0).unwrap().apply(probe);
        let (l0, l1) = (rgb_to_lab(probe), rgb_to_lab(tone_only));
        assert!((l0[1] - l1[1]).abs() < 0.5 && (l0[2] - l1[2]).abs() < 0.5, "{l0:?} {l1:?}");
        assert!((l0[0] - l1[0]).abs() > 1.0, "亮度应该有变化");
        // 只匹配色彩：亮度不变，偏色变
        let color_only = Transfer::fit(&sample, &reference, 0.0, 1.0).unwrap().apply(probe);
        let c1 = rgb_to_lab(color_only);
        assert!((l0[0] - c1[0]).abs() < 0.5, "{l0:?} {c1:?}");
        assert!((l0[1] - c1[1]).abs() + (l0[2] - c1[2]).abs() > 2.0, "偏色应该有变化");
        // 一半强度大致落在中间
        let half = Transfer::fit(&sample, &reference, 0.5, 0.5).unwrap().apply(probe);
        let full = Transfer::fit(&sample, &reference, 1.0, 1.0).unwrap().apply(probe);
        let (h, f) = (rgb_to_lab(half), rgb_to_lab(full));
        assert!(h[0] > l0[0].min(f[0]) - 0.5 && h[0] < l0[0].max(f[0]) + 0.5, "L: {} {} {}", l0[0], h[0], f[0]);
    }

    #[test]
    fn l_curve_is_monotonic_smooth_and_keeps_black_and_white_reasonable() {
        // 参考图是“褪色”的：黑位抬高、白位压低
        let sample = photo(5000, 0.02, 0.98, [0.0; 3], 31);
        let reference = photo(5000, 0.15, 0.80, [0.0; 3], 37);
        let t = Transfer::fit(&sample, &reference, 1.0, 0.0).unwrap();
        let mut prev = t.l_curve_at(0.0);
        let mut max_step = 0.0f32;
        for i in 1..=100 {
            let v = t.l_curve_at(i as f32);
            assert!(v >= prev - 1e-4, "曲线在 {i} 处下降");
            max_step = max_step.max(v - prev);
            prev = v;
        }
        assert!(max_step < 3.0, "相邻两个 L 之间曲线不应该有陡坎：{max_step}");
        let (black, white) = (t.apply([0.0; 3]), t.apply([1.0; 3]));
        assert!(rgb_to_lab(black)[0] > 8.0, "褪色的参考把黑位抬高：{black:?}");
        assert!(rgb_to_lab(white)[0] < 92.0, "白位被压低：{white:?}");
    }

    #[test]
    fn neutral_extremes_stay_clean_when_matching_color() {
        let sample = photo(4000, 0.2, 0.8, [0.0; 3], 41);
        let reference = photo(4000, 0.2, 0.8, [0.15, 0.0, -0.15], 43);
        let t = Transfer::fit(&sample, &reference, 0.0, 1.0).unwrap();
        let mid = t.apply([0.5; 3]);
        let white = t.apply([1.0; 3]);
        let tint = |c: Rgb| (rgb_to_lab(c)[1].powi(2) + rgb_to_lab(c)[2].powi(2)).sqrt();
        assert!(tint(mid) > 3.0, "中间调被染上颜色：{}", tint(mid));
        assert!(tint(white) < tint(mid) * 0.6, "纯白染得更轻：{} vs {}", tint(white), tint(mid));
    }

    #[test]
    fn grayscale_sample_does_not_blow_up() {
        let gray: Vec<Rgb> = (0..1000).map(|i| [(i % 100) as f32 / 100.0; 3]).collect();
        let reference = photo(1000, 0.1, 0.9, [0.2, 0.0, -0.2], 53);
        let t = Transfer::fit(&gray, &reference, 1.0, 1.0).unwrap();
        for p in [[0.0; 3], [0.5; 3], [1.0; 3]] {
            assert!(t.apply(p).iter().all(|c| c.is_finite()), "{p:?}");
        }
    }

    #[test]
    fn thin_keeps_order_and_count() {
        let v: Vec<Rgb> = (0..1000).map(|i| [i as f32; 3]).collect();
        let t = thin(&v, 100);
        assert_eq!(t.len(), 100);
        assert!(t.windows(2).all(|w| w[0][0] < w[1][0]));
        assert_eq!(thin(&v, 5000).len(), 1000);
    }
}
