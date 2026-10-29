//! 基础调色：曝光、黑白场、中间调、对比度、阴影 / 高光、白平衡、饱和度、自然饱和度、色相、分色调。
//!
//! 所有计算都在“显示用”的 gamma 编码 RGB（0–1，普通 sRGB / Rec.709 画面）上进行，和 `.cube` 在 ffmpeg `lut3d` 里的用法一致；
//! 曝光和白平衡先转到线性光再算。计算顺序固定：曝光 → 黑白场 → 中间调 → 对比度 → 阴影 / 高光 → 白平衡 → 饱和度 / 自然饱和度 → 色相 → 分色调 → 整体强度。

use serde::{Deserialize, Serialize};

pub type Rgb = [f32; 3];

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(l: f32) -> f32 {
    if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Rec.709 亮度（对 gamma 编码的值直接加权，足够用来做明暗分区和饱和度）。
pub fn luma(v: Rgb) -> f32 {
    0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb {
    let h = h.rem_euclid(360.0) / 60.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// 两端固定在 0 和 1、以 0.5 为中心对称的 S 形曲线：`a > 1` 增加对比度，`a < 1` 降低；反函数是 `a → 1/a`。
fn s_curve(x: f32, a: f32) -> f32 {
    if x <= 0.0 || x >= 1.0 {
        return x;
    }
    let (p, q) = (x.powf(a), (1.0 - x).powf(a));
    p / (p + q)
}

/// 分色调：给阴影或高光染上一种颜色。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SplitTone {
    /// 色相 0–360（0 红、120 绿、240 蓝）
    pub hue: f32,
    /// 强度 0–1，0 为不染色
    pub amount: f32,
}

impl Default for SplitTone {
    fn default() -> Self {
        SplitTone { hue: 0.0, amount: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Adjust {
    /// 曝光，单位“档”（每档亮一倍），-3 到 3
    pub exposure: f32,
    /// 黑场 -0.2 到 0.4：大于 0 把更多暗部压成纯黑，小于 0 抬高黑位（褪色感）
    pub black: f32,
    /// 白场 0.6 到 1.2：小于 1 把更多亮部推成纯白，大于 1 压低最亮处
    pub white: f32,
    /// 中间调 -1 到 1：正数提亮中间调，不动黑白
    pub gamma: f32,
    /// 对比度 -1 到 1
    pub contrast: f32,
    /// 阴影 -1 到 1：正数提亮暗部
    pub shadows: f32,
    /// 高光 -1 到 1：负数压低亮部
    pub highlights: f32,
    /// 色温 -1 到 1：负数偏蓝，正数偏黄
    pub temperature: f32,
    /// 色调 -1 到 1：负数偏绿，正数偏品红
    pub tint: f32,
    /// 饱和度 -1 到 1：-1 变黑白，1 约两倍
    pub saturation: f32,
    /// 自然饱和度 -1 到 1：优先作用在不够鲜艳的颜色上
    pub vibrance: f32,
    /// 色相偏移，度，-180 到 180
    pub hue: f32,
    pub shadow_tone: SplitTone,
    pub highlight_tone: SplitTone,
    /// 整体强度 0–1：和未调整的画面混合
    pub strength: f32,
}

impl Default for Adjust {
    fn default() -> Self {
        Adjust {
            exposure: 0.0,
            black: 0.0,
            white: 1.0,
            gamma: 0.0,
            contrast: 0.0,
            shadows: 0.0,
            highlights: 0.0,
            temperature: 0.0,
            tint: 0.0,
            saturation: 0.0,
            vibrance: 0.0,
            hue: 0.0,
            shadow_tone: SplitTone { hue: 220.0, amount: 0.0 },
            highlight_tone: SplitTone { hue: 40.0, amount: 0.0 },
            strength: 1.0,
        }
    }
}

const EPS: f32 = 1e-6;

impl Adjust {
    /// 把各项限制在允许的范围内（非数值当作默认值）。
    pub fn checked(self) -> Adjust {
        let d = Adjust::default();
        let f = |v: f32, lo: f32, hi: f32, def: f32| if v.is_finite() { v.clamp(lo, hi) } else { def };
        let tone = |t: SplitTone, def: SplitTone| SplitTone {
            hue: if t.hue.is_finite() { t.hue.rem_euclid(360.0) } else { def.hue },
            amount: f(t.amount, 0.0, 1.0, 0.0),
        };
        let black = f(self.black, -0.2, 0.4, 0.0);
        // 黑白场至少隔开 0.2，否则画面会被拉成一团
        let white = f(self.white, 0.6, 1.2, 1.0).max(black + 0.2);
        Adjust {
            exposure: f(self.exposure, -3.0, 3.0, 0.0),
            black,
            white,
            gamma: f(self.gamma, -1.0, 1.0, 0.0),
            contrast: f(self.contrast, -1.0, 1.0, 0.0),
            shadows: f(self.shadows, -1.0, 1.0, 0.0),
            highlights: f(self.highlights, -1.0, 1.0, 0.0),
            temperature: f(self.temperature, -1.0, 1.0, 0.0),
            tint: f(self.tint, -1.0, 1.0, 0.0),
            saturation: f(self.saturation, -1.0, 1.0, 0.0),
            vibrance: f(self.vibrance, -1.0, 1.0, 0.0),
            hue: f(self.hue, -180.0, 180.0, 0.0),
            shadow_tone: tone(self.shadow_tone, d.shadow_tone),
            highlight_tone: tone(self.highlight_tone, d.highlight_tone),
            strength: f(self.strength, 0.0, 1.0, 1.0),
        }
    }

    /// 没有任何调整（输出等于输入）。
    pub fn is_neutral(&self) -> bool {
        let d = Adjust::default();
        let s = self;
        [s.exposure, s.black, s.gamma, s.contrast, s.shadows, s.highlights, s.temperature, s.tint, s.saturation, s.vibrance, s.hue]
            .iter()
            .all(|v| v.abs() < EPS)
            && (s.white - d.white).abs() < EPS
            && s.shadow_tone.amount < EPS
            && s.highlight_tone.amount < EPS
            || s.strength < EPS
    }

    /// 对一个颜色做调整。输入输出都是 gamma 编码的 RGB，输出不做 0–1 截断。
    pub fn apply(&self, input: Rgb) -> Rgb {
        let mut v = input;
        // 1. 曝光（线性光里乘 2^档）
        if self.exposure.abs() > EPS {
            let g = 2f32.powf(self.exposure);
            for c in &mut v {
                *c = linear_to_srgb(srgb_to_linear(*c) * g);
            }
        }
        // 2. 黑白场
        if self.black.abs() > EPS || (self.white - 1.0).abs() > EPS {
            let span = (self.white - self.black).max(0.05);
            for c in &mut v {
                *c = (*c - self.black) / span;
            }
        }
        // 3. 中间调
        if self.gamma.abs() > EPS {
            let inv = 1.0 / 2f32.powf(self.gamma);
            for c in &mut v {
                if *c > 0.0 {
                    *c = c.powf(inv);
                }
            }
        }
        // 4. 对比度
        if self.contrast.abs() > EPS {
            let a = 3f32.powf(self.contrast);
            for c in &mut v {
                *c = s_curve(*c, a);
            }
        }
        // 5. 阴影 / 高光：按亮度分区，整体乘一个增益，不改变色相
        if self.shadows.abs() > EPS || self.highlights.abs() > EPS {
            let y = luma(v).clamp(0.0, 1.0);
            let gain = 1.0 + self.shadows * 0.8 * (1.0 - y) * (1.0 - y) + self.highlights * 0.3 * y * y;
            for c in &mut v {
                *c *= gain;
            }
        }
        // 6. 白平衡：线性光里按通道乘增益，再把亮度补回去
        if self.temperature.abs() > EPS || self.tint.abs() > EPS {
            let (rg, gg, bg) = (2f32.powf(self.temperature * 0.4), 2f32.powf(-self.tint * 0.3), 2f32.powf(-self.temperature * 0.4));
            let k = 1.0 / (0.2126 * rg + 0.7152 * gg + 0.0722 * bg);
            let gains = [rg * k, gg * k, bg * k];
            for (c, g) in v.iter_mut().zip(gains) {
                *c = linear_to_srgb(srgb_to_linear(*c) * g);
            }
        }
        // 7. 饱和度和自然饱和度：围绕亮度缩放色度
        if self.saturation.abs() > EPS || self.vibrance.abs() > EPS {
            let y = luma(v);
            let (mx, mn) = (v.iter().copied().fold(f32::MIN, f32::max), v.iter().copied().fold(f32::MAX, f32::min));
            let cur = if mx > 1e-4 { ((mx - mn) / mx).clamp(0.0, 1.0) } else { 0.0 };
            let k = (1.0 + self.saturation) * (1.0 + self.vibrance * (1.0 - cur));
            for c in &mut v {
                *c = y + (*c - y) * k;
            }
        }
        // 8. 色相偏移：保持亮度的旋转矩阵（和 SVG feColorMatrix hueRotate 相同）
        if self.hue.abs() > EPS {
            let (s, c) = self.hue.to_radians().sin_cos();
            let m = [
                [0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928],
                [0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140, 0.072 - c * 0.072 - s * 0.283],
                [0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072],
            ];
            v = [
                m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
                m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
                m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
            ];
        }
        // 9. 分色调：给阴影、高光加上一种颜色（只加色度，不改亮度）
        if self.shadow_tone.amount > EPS || self.highlight_tone.amount > EPS {
            let y = luma(v).clamp(0.0, 1.0);
            for (tone, mask) in [(self.shadow_tone, (1.0 - y) * (1.0 - y)), (self.highlight_tone, y * y)] {
                if tone.amount > EPS {
                    let col = hsv_to_rgb(tone.hue, 1.0, 1.0);
                    let cy = luma(col);
                    for (c, t) in v.iter_mut().zip(col) {
                        *c += (t - cy) * tone.amount * mask * 0.4;
                    }
                }
            }
        }
        // 10. 整体强度
        if self.strength < 1.0 - EPS {
            for (c, o) in v.iter_mut().zip(input) {
                *c = lerp(o, *c, self.strength);
            }
        }
        v
    }
}

/// 一键风格：只是一组调整参数。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Look {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub adjust: Adjust,
}

pub fn looks() -> Vec<Look> {
    let base = Adjust::default();
    let tone = |hue, amount| SplitTone { hue, amount };
    vec![
        Look {
            id: "warm",
            name: "暖调",
            desc: "偏黄偏暖，对比度略增",
            adjust: Adjust { temperature: 0.35, tint: 0.05, saturation: 0.1, contrast: 0.1, ..base },
        },
        Look {
            id: "cool", name: "冷调", desc: "偏蓝偏冷，对比度略增", adjust: Adjust { temperature: -0.35, saturation: 0.05, contrast: 0.1, ..base }
        },
        Look {
            id: "teal_orange",
            name: "青橙电影感",
            desc: "暗部偏青、亮部偏橙，对比度加强",
            adjust: Adjust { contrast: 0.25, saturation: 0.1, shadow_tone: tone(190.0, 0.5), highlight_tone: tone(35.0, 0.45), ..base },
        },
        Look {
            id: "faded",
            name: "褪色胶片",
            desc: "抬高黑位、压低饱和度，暗部偏青、亮部偏黄",
            adjust: Adjust {
                black: -0.08,
                white: 0.96,
                saturation: -0.2,
                contrast: -0.1,
                shadow_tone: tone(200.0, 0.25),
                highlight_tone: tone(45.0, 0.2),
                ..base
            },
        },
        Look {
            id: "vivid",
            name: "鲜艳通透",
            desc: "提亮暗部、加强对比和自然饱和度",
            adjust: Adjust { vibrance: 0.45, saturation: 0.1, contrast: 0.15, shadows: 0.15, ..base },
        },
        Look {
            id: "soft",
            name: "清新明亮",
            desc: "提亮、降低对比，偏冷一点",
            adjust: Adjust { exposure: 0.3, contrast: -0.15, highlights: -0.2, shadows: 0.25, saturation: -0.1, temperature: -0.1, ..base },
        },
        Look { id: "bw", name: "黑白高反差", desc: "去掉颜色，加强对比", adjust: Adjust { saturation: -1.0, contrast: 0.35, ..base } },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb, tol: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    fn colors() -> Vec<Rgb> {
        let mut out = vec![];
        for r in [0.0, 0.2, 0.5, 0.8, 1.0] {
            for g in [0.0, 0.3, 0.6, 1.0] {
                for b in [0.0, 0.1, 0.5, 0.9, 1.0] {
                    out.push([r, g, b]);
                }
            }
        }
        out
    }

    #[test]
    fn transfer_functions_round_trip() {
        for i in 0..=100 {
            let c = i as f32 / 100.0;
            assert!((linear_to_srgb(srgb_to_linear(c)) - c).abs() < 1e-5, "{c}");
        }
        // 负值不产生 NaN
        assert!(linear_to_srgb(srgb_to_linear(-0.1)).is_finite());
    }

    #[test]
    fn default_is_identity() {
        let a = Adjust::default();
        assert!(a.is_neutral());
        for c in colors() {
            assert!(close(a.apply(c), c, 1e-6), "{c:?}");
        }
        // 分色调的色相不影响“没有调整”的判断
        let b = Adjust { shadow_tone: SplitTone { hue: 100.0, amount: 0.0 }, ..Default::default() };
        assert!(b.is_neutral());
        assert!(!Adjust { exposure: 0.1, ..Default::default() }.is_neutral());
        assert!(!Adjust { white: 0.9, ..Default::default() }.is_neutral());
        // 强度为 0 等于没调整
        assert!(Adjust { exposure: 1.0, strength: 0.0, ..Default::default() }.is_neutral());
    }

    #[test]
    fn checked_clamps_and_repairs() {
        let a = Adjust { exposure: 9.0, saturation: f32::NAN, black: 0.4, white: 0.6, strength: 3.0, hue: 400.0, ..Default::default() }.checked();
        assert_eq!(a.exposure, 3.0);
        assert_eq!(a.saturation, 0.0);
        assert_eq!(a.strength, 1.0);
        assert_eq!(a.hue, 180.0);
        assert!(a.white >= a.black + 0.2 - 1e-6, "{a:?}");
        let t = Adjust { shadow_tone: SplitTone { hue: -90.0, amount: 2.0 }, ..Default::default() }.checked();
        assert_eq!(t.shadow_tone.hue, 270.0);
        assert_eq!(t.shadow_tone.amount, 1.0);
        // 合法的值原样保留
        let ok = Adjust { exposure: 0.5, contrast: -0.3, ..Default::default() };
        assert_eq!(ok.checked(), ok);
    }

    #[test]
    fn exposure_doubles_linear_light() {
        let a = Adjust { exposure: 1.0, ..Default::default() };
        let mid = 0.3;
        let out = a.apply([mid; 3]);
        let ratio = srgb_to_linear(out[0]) / srgb_to_linear(mid);
        assert!((ratio - 2.0).abs() < 1e-3, "{ratio}");
        assert!(close(out, [out[0]; 3], 1e-6), "灰色保持灰色");
        // 减一档是一半
        let down = Adjust { exposure: -1.0, ..Default::default() }.apply([mid; 3]);
        assert!((srgb_to_linear(down[0]) / srgb_to_linear(mid) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn contrast_is_symmetric_monotonic_and_pinned() {
        for c in [-1.0, -0.5, 0.5, 1.0] {
            let a = Adjust { contrast: c, ..Default::default() };
            assert!((a.apply([0.0; 3])[0]).abs() < 1e-6 && (a.apply([1.0; 3])[0] - 1.0).abs() < 1e-6, "两端不动");
            assert!((a.apply([0.5; 3])[0] - 0.5).abs() < 1e-6, "中点不动");
            let mut prev = -1.0;
            for i in 0..=100 {
                let o = a.apply([i as f32 / 100.0; 3])[0];
                assert!(o >= prev - 1e-6, "对比度 {c} 在 {i} 处不单调");
                prev = o;
            }
        }
        // 正数拉开，负数收拢
        assert!(Adjust { contrast: 0.5, ..Default::default() }.apply([0.25; 3])[0] < 0.25);
        assert!(Adjust { contrast: 0.5, ..Default::default() }.apply([0.75; 3])[0] > 0.75);
        assert!(Adjust { contrast: -0.5, ..Default::default() }.apply([0.25; 3])[0] > 0.25);
        // 正负相反的两个值互为反函数
        let up = Adjust { contrast: 0.6, ..Default::default() };
        let down = Adjust { contrast: -0.6, ..Default::default() };
        for i in 1..20 {
            let x = i as f32 / 20.0;
            assert!((down.apply(up.apply([x; 3]))[0] - x).abs() < 1e-4, "{x}");
        }
    }

    #[test]
    fn shadows_and_highlights_act_on_their_own_range() {
        let sh = Adjust { shadows: 1.0, ..Default::default() };
        let dark_gain = sh.apply([0.1; 3])[0] / 0.1;
        let bright_gain = sh.apply([0.9; 3])[0] / 0.9;
        assert!(dark_gain > 1.5 && bright_gain < 1.02, "阴影提亮暗部不动亮部：{dark_gain} {bright_gain}");
        let hi = Adjust { highlights: -1.0, ..Default::default() };
        let dark = hi.apply([0.1; 3])[0] / 0.1;
        let bright = hi.apply([0.95; 3])[0] / 0.95;
        assert!(dark > 0.99 && bright < 0.8, "高光压低亮部不动暗部：{dark} {bright}");
        // 灰阶保持单调，不会出现反转
        for adj in [Adjust { shadows: -1.0, highlights: -1.0, ..Default::default() }, Adjust { shadows: 1.0, highlights: 1.0, ..Default::default() }] {
            let mut prev = -1.0;
            for i in 0..=200 {
                let o = adj.apply([i as f32 / 200.0; 3])[0];
                assert!(o > prev - 1e-6, "{adj:?} 在 {i} 处不单调");
                prev = o;
            }
        }
        // 色相不变：各通道乘同一个增益
        let o = sh.apply([0.2, 0.1, 0.05]);
        assert!((o[0] / 0.2 - o[1] / 0.1).abs() < 1e-5 && (o[1] / 0.1 - o[2] / 0.05).abs() < 1e-5);
    }

    #[test]
    fn levels_gamma_and_fade() {
        let lift = Adjust { black: -0.1, ..Default::default() };
        assert!(lift.apply([0.0; 3])[0] > 0.08, "负黑场抬高黑位");
        let crush = Adjust { black: 0.2, ..Default::default() };
        assert!(crush.apply([0.2; 3])[0].abs() < 1e-6 && crush.apply([0.1; 3])[0] < 0.0, "黑场以下压成黑（负值会在最后截断）");
        let white = Adjust { white: 0.8, ..Default::default() };
        assert!((white.apply([0.8; 3])[0] - 1.0).abs() < 1e-6);
        let g = Adjust { gamma: 0.5, ..Default::default() };
        assert!(g.apply([0.5; 3])[0] > 0.5 && g.apply([0.0; 3])[0] == 0.0 && (g.apply([1.0; 3])[0] - 1.0).abs() < 1e-6);
        assert!(Adjust { gamma: -0.5, ..Default::default() }.apply([0.5; 3])[0] < 0.5);
    }

    #[test]
    fn white_balance_moves_the_right_channels_and_keeps_luma() {
        let warm = Adjust { temperature: 0.6, ..Default::default() }.apply([0.5; 3]);
        assert!(warm[0] > 0.5 && warm[2] < 0.5, "{warm:?}");
        let cool = Adjust { temperature: -0.6, ..Default::default() }.apply([0.5; 3]);
        assert!(cool[2] > 0.5 && cool[0] < 0.5, "{cool:?}");
        assert!((luma(warm) - 0.5).abs() < 0.02 && (luma(cool) - 0.5).abs() < 0.02, "亮度基本不变");
        let magenta = Adjust { tint: 0.6, ..Default::default() }.apply([0.5; 3]);
        assert!(magenta[1] < magenta[0] && magenta[1] < magenta[2], "{magenta:?}");
        let green = Adjust { tint: -0.6, ..Default::default() }.apply([0.5; 3]);
        assert!(green[1] > green[0] && green[1] > green[2], "{green:?}");
    }

    #[test]
    fn saturation_and_vibrance() {
        let c = [0.8, 0.4, 0.2];
        let gray = Adjust { saturation: -1.0, ..Default::default() }.apply(c);
        assert!(close(gray, [luma(c); 3], 1e-5), "{gray:?}");
        let more = Adjust { saturation: 0.5, ..Default::default() }.apply(c);
        assert!(more[0] - more[2] > c[0] - c[2]);
        assert!((luma(more) - luma(c)).abs() < 1e-5, "亮度不变");
        // 自然饱和度对已经很鲜艳的颜色影响小
        let v = Adjust { vibrance: 1.0, ..Default::default() };
        let dull = [0.55, 0.5, 0.45];
        let vivid = [0.9, 0.2, 0.1];
        let (d, w) = (v.apply(dull), v.apply(vivid));
        let spread = |x: Rgb| x[0] - x[2];
        assert!(spread(d) / spread(dull) > 1.7, "淡色被大幅提升：{}", spread(d) / spread(dull));
        assert!(spread(w) / spread(vivid) < 1.15, "鲜艳色几乎不动：{}", spread(w) / spread(vivid));
    }

    #[test]
    fn hue_rotation_keeps_gray_and_wraps() {
        for h in [-120.0, 30.0, 90.0, 180.0] {
            let a = Adjust { hue: h, ..Default::default() };
            for g in [0.0, 0.3, 0.7, 1.0] {
                assert!(close(a.apply([g; 3]), [g; 3], 1e-3), "灰色在色相 {h} 下保持灰色");
            }
            let c = [0.8, 0.3, 0.2];
            assert!((luma(a.apply(c)) - luma(c)).abs() < 0.01, "亮度基本不变 {h}");
        }
        // 红色往正方向转会靠向绿
        let o = Adjust { hue: 120.0, ..Default::default() }.apply([1.0, 0.0, 0.0]);
        assert!(o[1] > o[0] && o[1] > o[2], "{o:?}");
        // 转 180 度再转 -180 度回来（在不截断的范围内）
        let c = [0.6, 0.4, 0.3];
        let back = Adjust { hue: -90.0, ..Default::default() }.apply(Adjust { hue: 90.0, ..Default::default() }.apply(c));
        assert!(close(back, c, 2e-3), "{back:?}");
    }

    #[test]
    fn split_toning_tints_the_right_zone() {
        let blue_shadows = Adjust { shadow_tone: SplitTone { hue: 240.0, amount: 1.0 }, ..Default::default() };
        let d = blue_shadows.apply([0.1; 3]);
        assert!(d[2] > d[0] + 0.05, "暗部变蓝 {d:?}");
        let b = blue_shadows.apply([0.95; 3]);
        assert!((b[2] - b[0]).abs() < 0.02, "亮部基本不受影响 {b:?}");
        let orange_highlights = Adjust { highlight_tone: SplitTone { hue: 30.0, amount: 1.0 }, ..Default::default() };
        let h = orange_highlights.apply([0.9; 3]);
        assert!(h[0] > h[2] + 0.05, "亮部偏橙 {h:?}");
        assert!((luma(h) - 0.9).abs() < 0.01, "只加色度、亮度不变");
    }

    #[test]
    fn strength_blends_with_the_original() {
        let a = Adjust { exposure: 1.0, strength: 0.5, ..Default::default() };
        let full = Adjust { exposure: 1.0, ..Default::default() };
        let c = [0.3, 0.4, 0.5];
        let (h, f) = (a.apply(c), full.apply(c));
        for i in 0..3 {
            assert!((h[i] - (c[i] + f[i]) / 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn looks_are_valid_and_actually_change_something() {
        let ls = looks();
        assert!(ls.len() >= 6);
        let mut ids: Vec<_> = ls.iter().map(|l| l.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ls.len(), "编号不能重复");
        for l in &ls {
            assert_eq!(l.adjust, l.adjust.checked(), "{} 的参数超出范围", l.id);
            assert!(!l.adjust.is_neutral(), "{} 没有任何调整", l.id);
            let out = l.adjust.apply([0.5, 0.4, 0.3]);
            assert!(out.iter().all(|c| c.is_finite()), "{}", l.id);
        }
        // 黑白风格的输出没有颜色
        let bw = ls.iter().find(|l| l.id == "bw").unwrap().adjust.apply([0.8, 0.3, 0.2]);
        assert!((bw[0] - bw[1]).abs() < 0.02 && (bw[1] - bw[2]).abs() < 0.02, "{bw:?}");
    }
}
