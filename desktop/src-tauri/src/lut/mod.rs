//! LUT 工作室：用几项基础调色、已有的 `.cube` 和参考图，生成一张新的 3D LUT（`.cube`），并能把同一个效果套在图片上预览或导出。
//!
//! 处理流程：基底 LUT（可叠加多张，各有强度）→ 基础调色 → 参考图匹配 → 在 N×N×N 的格点上取值写成 `.cube`。
//! 预览用的也是烘焙好的 LUT（四面体插值，和 ffmpeg `lut3d` 的默认做法一致），所以预览里看到的就是 `.cube` 文件的真实效果。

pub mod adjust;
pub mod cmds;
pub mod image;
pub mod matching;

use std::sync::Arc;

use serde::Deserialize;

use adjust::{Adjust, Rgb};

/// 一张 3D LUT：格点数据按“红变化最快，其次绿，最后蓝”排列，和 `.cube` 文件一致。
#[derive(Debug, Clone, PartialEq)]
pub struct Lut3 {
    pub title: String,
    pub size: usize,
    pub domain_min: Rgb,
    pub domain_max: Rgb,
    pub data: Vec<Rgb>,
}

/// 能读的最大格点数（65 是常见的最大值，再大的文件解析很慢，对调色没有意义）。
pub const MAX_SIZE: usize = 129;

impl Lut3 {
    pub fn identity(size: usize) -> Lut3 {
        let size = size.clamp(2, MAX_SIZE);
        let step = 1.0 / (size - 1) as f32;
        let mut data = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    data.push([r as f32 * step, g as f32 * step, b as f32 * step]);
                }
            }
        }
        Lut3 { title: "identity".into(), size, domain_min: [0.0; 3], domain_max: [1.0; 3], data }
    }

    /// 解析 `.cube` 文本。只支持 3D LUT（`LUT_3D_SIZE`）；1D LUT 会明确说明不支持。
    pub fn parse(text: &str) -> Result<Lut3, String> {
        let mut title = String::new();
        let mut size = 0usize;
        let (mut dmin, mut dmax) = ([0.0f32; 3], [1.0f32; 3]);
        let mut data: Vec<Rgb> = Vec::new();
        let triple = |rest: &str| -> Option<Rgb> {
            let mut it = rest.split_whitespace().map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite()));
            Some([it.next()??, it.next()??, it.next()??])
        };
        for (n, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let first = line.split_whitespace().next().unwrap_or("");
            if first.starts_with(|c: char| c.is_ascii_alphabetic()) {
                let rest = line[first.len()..].trim();
                match first {
                    "TITLE" => title = rest.trim_matches('"').to_string(),
                    "LUT_3D_SIZE" => {
                        size = rest.parse().map_err(|_| format!("第 {} 行的 LUT_3D_SIZE 不是整数。", n + 1))?;
                        if !(2..=MAX_SIZE).contains(&size) {
                            return Err(format!("LUT 的格点数 {size} 不在支持的范围（2–{MAX_SIZE}）内。"));
                        }
                        data.reserve(size * size * size);
                    }
                    "LUT_1D_SIZE" => return Err("这是 1D LUT（LUT_1D_SIZE），这里只支持 3D LUT。".into()),
                    "DOMAIN_MIN" => dmin = triple(rest).ok_or_else(|| format!("第 {} 行的 DOMAIN_MIN 不对。", n + 1))?,
                    "DOMAIN_MAX" => dmax = triple(rest).ok_or_else(|| format!("第 {} 行的 DOMAIN_MAX 不对。", n + 1))?,
                    // LUT_3D_INPUT_RANGE、LUT_IN_VIDEO_RANGE 等其他关键字：忽略
                    _ => {}
                }
                continue;
            }
            if size == 0 {
                return Err("数据出现在 LUT_3D_SIZE 之前，文件格式不对。".into());
            }
            data.push(triple(line).ok_or_else(|| format!("第 {} 行不是三个数字：{}", n + 1, line.chars().take(40).collect::<String>()))?);
            if data.len() > size * size * size {
                return Err(format!("数据比 LUT_3D_SIZE {size} 声明的多。"));
            }
        }
        if size == 0 {
            return Err("没有找到 LUT_3D_SIZE，这不是 3D LUT 的 .cube 文件。".into());
        }
        let want = size * size * size;
        if data.len() != want {
            return Err(format!("数据行数不对：LUT_3D_SIZE {size} 需要 {want} 行，实际 {} 行。", data.len()));
        }
        if (0..3).any(|i| dmax[i] <= dmin[i]) {
            return Err("DOMAIN_MAX 必须大于 DOMAIN_MIN。".into());
        }
        Ok(Lut3 { title, size, domain_min: dmin, domain_max: dmax, data })
    }

    /// 写成 `.cube` 文本。
    pub fn to_cube(&self) -> String {
        let mut out = String::with_capacity(self.data.len() * 24 + 160);
        let title: String = self.title.chars().filter(|c| !matches!(c, '"' | '\n' | '\r')).collect();
        out.push_str(&format!("# Created by ClearClip\nTITLE \"{title}\"\nLUT_3D_SIZE {}\n", self.size));
        let f = |v: Rgb| format!("{:.6} {:.6} {:.6}", v[0], v[1], v[2]);
        out.push_str(&format!("DOMAIN_MIN {}\nDOMAIN_MAX {}\n", f(self.domain_min), f(self.domain_max)));
        for v in &self.data {
            out.push_str(&f(*v));
            out.push('\n');
        }
        out
    }

    fn at(&self, r: usize, g: usize, b: usize) -> Rgb {
        self.data[r + g * self.size + b * self.size * self.size]
    }

    /// 四面体插值取值（ffmpeg `lut3d` 的默认做法）。输入按 DOMAIN 归一化并截断到范围内。
    pub fn sample(&self, v: Rgb) -> Rgb {
        let top = (self.size - 1) as f32;
        let mut t = [0.0f32; 3];
        for i in 0..3 {
            t[i] = ((v[i] - self.domain_min[i]) / (self.domain_max[i] - self.domain_min[i])).clamp(0.0, 1.0) * top;
        }
        let i0 = t.map(|x| (x.floor() as usize).min(self.size - 1));
        let i1 = i0.map(|i| (i + 1).min(self.size - 1));
        let (fr, fg, fb) = (t[0] - i0[0] as f32, t[1] - i0[1] as f32, t[2] - i0[2] as f32);
        let c = |dr: bool, dg: bool, db: bool| self.at(if dr { i1[0] } else { i0[0] }, if dg { i1[1] } else { i0[1] }, if db { i1[2] } else { i0[2] });
        let c000 = c(false, false, false);
        let c111 = c(true, true, true);
        // 沿着从 c000 到 c111 的路径，按 r、g、b 小数部分的大小顺序逐段累加
        let path: [(f32, Rgb, Rgb); 3] = if fr > fg {
            if fg > fb {
                [(fr, c(true, false, false), c000), (fg, c(true, true, false), c(true, false, false)), (fb, c111, c(true, true, false))]
            } else if fr > fb {
                [(fr, c(true, false, false), c000), (fb, c(true, false, true), c(true, false, false)), (fg, c111, c(true, false, true))]
            } else {
                [(fb, c(false, false, true), c000), (fr, c(true, false, true), c(false, false, true)), (fg, c111, c(true, false, true))]
            }
        } else if fb > fg {
            [(fb, c(false, false, true), c000), (fg, c(false, true, true), c(false, false, true)), (fr, c111, c(false, true, true))]
        } else if fb > fr {
            [(fg, c(false, true, false), c000), (fb, c(false, true, true), c(false, true, false)), (fr, c111, c(false, true, true))]
        } else {
            [(fg, c(false, true, false), c000), (fr, c(true, true, false), c(false, true, false)), (fb, c111, c(true, true, false))]
        };
        let mut out = c000;
        for (w, hi, lo) in path {
            for k in 0..3 {
                out[k] += w * (hi[k] - lo[k]);
            }
        }
        out
    }
}

// ---------- 配方 ----------

/// 基底 LUT：先套用，强度 0–1。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Layer {
    pub path: String,
    pub strength: f32,
}

impl Default for Layer {
    fn default() -> Self {
        Layer { path: String::new(), strength: 1.0 }
    }
}

/// 参考图：把示例图片的色调、色彩向它靠拢。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reference {
    pub path: String,
    /// 明暗（亮度分布）匹配强度 0–1
    pub tone: f32,
    /// 色彩（偏色、饱和度）匹配强度 0–1
    pub color: f32,
}

impl Default for Reference {
    fn default() -> Self {
        Reference { path: String::new(), tone: 0.8, color: 0.8 }
    }
}

/// 示例图片：预览和参考图匹配用。可以是视频，取 `at_ms` 处的一帧。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SampleRef {
    pub path: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Recipe {
    /// LUT 的名字（写在 .cube 的 TITLE 里）
    pub name: String,
    /// 格点数：17 / 33 / 65
    pub size: usize,
    pub base: Vec<Layer>,
    pub adjust: Adjust,
    pub reference: Option<Reference>,
    pub sample: Option<SampleRef>,
}

impl Default for Recipe {
    fn default() -> Self {
        Recipe { name: "ClearClip LUT".into(), size: 33, base: vec![], adjust: Adjust::default(), reference: None, sample: None }
    }
}

impl Recipe {
    /// 烘焙用的格点数：限制在 9–65。
    pub fn lut_size(&self) -> usize {
        self.size.clamp(9, 65)
    }
}

/// 准备好的计算：基底 LUT 已读入，参考图匹配已经拟合好。
pub struct Prepared {
    pub name: String,
    pub layers: Vec<(Arc<Lut3>, f32)>,
    pub adjust: Adjust,
    pub transfer: Option<matching::Transfer>,
}

/// 参考图匹配的输入：示例图片和参考图的像素（0–1 的 gamma 编码 RGB）。
pub struct MatchInput<'a> {
    pub sample: &'a [Rgb],
    pub reference: &'a [Rgb],
    pub tone: f32,
    pub color: f32,
}

impl Prepared {
    pub fn new(name: &str, layers: Vec<(Arc<Lut3>, f32)>, adjust: Adjust, matching: Option<MatchInput<'_>>) -> Prepared {
        let mut p = Prepared { name: name.into(), layers, adjust: adjust.checked(), transfer: None };
        if let Some(m) = matching {
            // 匹配要以“已经叠好基底 LUT 和调整”的示例图片为起点，让最后的结果和参考图靠近
            let pre: Vec<Rgb> = m.sample.iter().map(|px| p.pre(*px).map(|c| c.clamp(0.0, 1.0))).collect();
            p.transfer = matching::Transfer::fit(&pre, m.reference, m.tone.clamp(0.0, 1.0), m.color.clamp(0.0, 1.0));
        }
        p
    }

    /// 基底 LUT 叠加和基础调色（不含参考图匹配）。
    pub fn pre(&self, v: Rgb) -> Rgb {
        let mut v = v;
        for (lut, strength) in &self.layers {
            let s = strength.clamp(0.0, 1.0);
            if s <= 0.0 {
                continue;
            }
            let o = lut.sample(v);
            for k in 0..3 {
                v[k] += (o[k] - v[k]) * s;
            }
        }
        self.adjust.apply(v)
    }

    /// 完整的颜色映射（输出不截断）。
    pub fn map(&self, v: Rgb) -> Rgb {
        let v = self.pre(v);
        match &self.transfer {
            Some(t) => t.apply(v.map(|c| c.clamp(0.0, 1.0))),
            None => v,
        }
    }

    /// 在 `size`³ 的格点上取值，得到一张 LUT。输出截断到 0–1。
    pub fn bake(&self, size: usize) -> Lut3 {
        let size = size.clamp(2, MAX_SIZE);
        let step = 1.0 / (size - 1) as f32;
        let mut data = vec![[0.0f32; 3]; size * size * size];
        let layer = size * size;
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
        // 按蓝色切片分给几个线程
        let per = size.div_ceil(threads);
        std::thread::scope(|s| {
            for (ti, chunk) in data.chunks_mut(per * layer).enumerate() {
                s.spawn(move || {
                    for (i, out) in chunk.iter_mut().enumerate() {
                        let idx = ti * per * layer + i;
                        let (r, g, b) = (idx % size, (idx / size) % size, idx / layer);
                        let m = self.map([r as f32 * step, g as f32 * step, b as f32 * step]);
                        *out = m.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 });
                    }
                });
            }
        });
        Lut3 { title: self.name.clone(), size, domain_min: [0.0; 3], domain_max: [1.0; 3], data }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb, tol: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    #[test]
    fn identity_samples_back_the_input() {
        for size in [2, 5, 17, 33] {
            let lut = Lut3::identity(size);
            assert_eq!(lut.data.len(), size * size * size);
            for r in [0.0, 0.13, 0.5, 0.77, 1.0] {
                for g in [0.0, 0.31, 0.5, 0.9, 1.0] {
                    for b in [0.0, 0.2, 0.5, 0.64, 1.0] {
                        assert!(close(lut.sample([r, g, b]), [r, g, b], 1e-5), "size {size} {r} {g} {b}");
                    }
                }
            }
        }
        // 超出范围的输入被截断
        assert!(close(Lut3::identity(9).sample([-0.5, 2.0, 0.5]), [0.0, 1.0, 0.5], 1e-5));
    }

    #[test]
    fn red_varies_fastest_in_the_data() {
        let l = Lut3::identity(3);
        assert_eq!(l.data[1], [0.5, 0.0, 0.0]);
        assert_eq!(l.data[3], [0.0, 0.5, 0.0]);
        assert_eq!(l.data[9], [0.0, 0.0, 0.5]);
    }

    #[test]
    fn tetrahedral_interpolation_is_exact_for_linear_maps_and_all_six_orders() {
        // 线性映射在四面体插值下应该完全准确，覆盖 r/g/b 小数部分的六种大小顺序
        let size = 5;
        let mut lut = Lut3::identity(size);
        for v in &mut lut.data {
            *v = [0.2 + 0.5 * v[0] + 0.1 * v[1], 0.1 * v[0] + 0.7 * v[1] + 0.05 * v[2], 0.3 * v[2] + 0.2 * v[0]];
        }
        let expect = |p: Rgb| [0.2 + 0.5 * p[0] + 0.1 * p[1], 0.1 * p[0] + 0.7 * p[1] + 0.05 * p[2], 0.3 * p[2] + 0.2 * p[0]];
        let fracs = [[0.13, 0.41, 0.77], [0.13, 0.77, 0.41], [0.41, 0.13, 0.77], [0.41, 0.77, 0.13], [0.77, 0.13, 0.41], [0.77, 0.41, 0.13]];
        for f in fracs {
            let p = [(1.0 + f[0]) / 4.0, (2.0 + f[1]) / 4.0, (0.0 + f[2]) / 4.0];
            assert!(close(lut.sample(p), expect(p), 1e-5), "{f:?}");
        }
    }

    #[test]
    fn cube_round_trip() {
        let mut lut = Lut3::identity(5);
        lut.title = "我的\"测试\"".into();
        for v in &mut lut.data {
            *v = [v[0] * 0.9 + 0.05, v[1], (v[2] * 1.1).min(1.0)];
        }
        let text = lut.to_cube();
        assert!(text.contains("LUT_3D_SIZE 5") && text.contains("DOMAIN_MIN 0.000000 0.000000 0.000000"));
        let back = Lut3::parse(&text).unwrap();
        assert_eq!(back.size, 5);
        assert_eq!(back.title, "我的测试", "标题里的引号被去掉");
        for (a, b) in lut.data.iter().zip(&back.data) {
            assert!(close(*a, *b, 1e-6));
        }
    }

    #[test]
    fn parse_is_tolerant_and_strict_where_it_matters() {
        // 注释、BOM、CRLF、多余的关键字、数据行后面的注释、科学计数法
        let text = "\u{feff}# comment\r\nTITLE \"x\"\r\nLUT_3D_INPUT_RANGE 0.0 1.0\r\nLUT_3D_SIZE 2\r\nDOMAIN_MIN 0 0 0\r\nDOMAIN_MAX 1 1 1\r\n0 0 0 # first\r\n1.0 0 0\r\n0 1e0 0\r\n1 1 0\r\n0 0 1\r\n1 0 1\r\n0 1 1\r\n1 1 1\r\n";
        let lut = Lut3::parse(text).unwrap();
        assert_eq!(lut.size, 2);
        assert!(close(lut.sample([0.3, 0.6, 0.9]), [0.3, 0.6, 0.9], 1e-6));
        // 非 0–1 的定义域
        let dom = "LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        let d = Lut3::parse(dom).unwrap();
        assert!(close(d.sample([1.0, 0.0, 2.0]), [0.5, 0.0, 1.0], 1e-6));
        // 错误都有明确的说明
        for (bad, hint) in [
            ("LUT_1D_SIZE 4\n0 0 0\n", "1D"),
            ("0 0 0\n", "LUT_3D_SIZE"),
            ("TITLE \"x\"\n", "LUT_3D_SIZE"),
            ("LUT_3D_SIZE 2\n0 0 0\n", "数据行数"),
            ("LUT_3D_SIZE 1\n", "范围"),
            ("LUT_3D_SIZE 2\n0 0\n", "不是三个数字"),
            ("LUT_3D_SIZE 2\n0 0 nan\n", "不是三个数字"),
            ("LUT_3D_SIZE 2\nDOMAIN_MAX 0 0 0\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n", "DOMAIN_MAX"),
        ] {
            let e = Lut3::parse(bad).unwrap_err();
            assert!(e.contains(hint), "{bad:?} → {e}");
        }
        // 数据比声明的多
        let many = "LUT_3D_SIZE 2\n".to_string() + &"0 0 0\n".repeat(9);
        assert!(Lut3::parse(&many).unwrap_err().contains("多"));
    }

    #[test]
    fn built_in_hdr_lut_parses() {
        // 程序内置的 HDR → SDR LUT 也是标准的 .cube，能被这里读进来
        let lut = Lut3::parse(&crate::vidnorm::hdr::cube(crate::vidnorm::facts::Hdr::Pq)).unwrap();
        assert_eq!(lut.size, 33);
        assert!(close(lut.sample([0.0; 3]), [0.0; 3], 1e-4));
    }

    #[test]
    fn prepared_with_nothing_is_identity_and_layers_compose() {
        let p = Prepared::new("t", vec![], Adjust::default(), None);
        let lut = p.bake(17);
        for (a, b) in lut.data.iter().zip(&Lut3::identity(17).data) {
            assert!(close(*a, *b, 1e-6));
        }
        // 基底 LUT 叠加：两张提亮的 LUT 叠在一起比一张更亮；强度 0 等于没有
        let mut bright = Lut3::identity(9);
        for v in &mut bright.data {
            *v = v.map(|c| (c * 1.2).min(1.0));
        }
        let bright = Arc::new(bright);
        let one = Prepared::new("t", vec![(bright.clone(), 1.0)], Adjust::default(), None).map([0.4; 3])[0];
        let two = Prepared::new("t", vec![(bright.clone(), 1.0), (bright.clone(), 1.0)], Adjust::default(), None).map([0.4; 3])[0];
        let zero = Prepared::new("t", vec![(bright.clone(), 0.0)], Adjust::default(), None).map([0.4; 3])[0];
        let half = Prepared::new("t", vec![(bright, 0.5)], Adjust::default(), None).map([0.4; 3])[0];
        assert!(
            (one - 0.48).abs() < 1e-4 && (two - 0.576).abs() < 1e-4 && (zero - 0.4).abs() < 1e-6 && (half - 0.44).abs() < 1e-4,
            "{one} {two} {zero} {half}"
        );
    }

    #[test]
    fn baked_lut_matches_direct_evaluation_for_a_gentle_look() {
        let adj = Adjust { contrast: 0.2, temperature: 0.2, saturation: 0.15, exposure: 0.2, ..Default::default() };
        let p = Prepared::new("look", vec![], adj, None);
        let lut = p.bake(33);
        let mut worst = 0.0f32;
        for r in 0..9 {
            for g in 0..9 {
                for b in 0..9 {
                    let c = [r as f32 / 8.5 + 0.01, g as f32 / 8.5 + 0.01, b as f32 / 8.5 + 0.01].map(|x| x.min(1.0));
                    let want = p.map(c).map(|x| x.clamp(0.0, 1.0));
                    let got = lut.sample(c);
                    for k in 0..3 {
                        worst = worst.max((want[k] - got[k]).abs());
                    }
                }
            }
        }
        assert!(worst < 0.01, "33 格点的 LUT 和直接计算最多差 {worst}");
    }

    #[test]
    fn bake_output_is_finite_and_clamped_and_threads_do_not_reorder() {
        let adj = Adjust { exposure: 3.0, contrast: 1.0, saturation: 1.0, hue: 90.0, ..Default::default() };
        let lut = Prepared::new("x", vec![], adj, None).bake(21);
        assert!(lut.data.iter().all(|v| v.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c))));
        // 逐点对照，确认多线程切片没有错位
        let p = Prepared::new("x", vec![], adj, None);
        for (i, v) in lut.data.iter().enumerate().step_by(37) {
            let (r, g, b) = (i % 21, (i / 21) % 21, i / 441);
            let want = p.map([r as f32 / 20.0, g as f32 / 20.0, b as f32 / 20.0]).map(|c| c.clamp(0.0, 1.0));
            assert!(close(*v, want, 1e-6), "{i}");
        }
    }

    #[test]
    fn recipe_deserializes_from_the_frontend_shape() {
        let r: Recipe = serde_json::from_str(
            r#"{"name":"mine","size":17,"base":[{"path":"/a.cube","strength":0.5}],"adjust":{"exposure":0.5,"shadowTone":{"hue":200,"amount":0.3}},
                "reference":{"path":"/r.jpg","tone":0.5,"color":1},"sample":{"path":"/s.jpg","atMs":1500}}"#,
        )
        .unwrap();
        assert_eq!(r.size, 17);
        assert_eq!(r.base[0].strength, 0.5);
        assert_eq!(r.adjust.exposure, 0.5);
        assert_eq!(r.adjust.white, 1.0, "没写的项用默认值");
        assert_eq!(r.adjust.shadow_tone.hue, 200.0);
        assert_eq!(r.reference.unwrap().color, 1.0);
        assert_eq!(r.sample.unwrap().at_ms, 1500);
        let empty: Recipe = serde_json::from_str("{}").unwrap();
        assert_eq!((empty.size, empty.lut_size()), (33, 33));
        assert_eq!(Recipe { size: 3, ..Default::default() }.lut_size(), 9);
        assert_eq!(Recipe { size: 500, ..Default::default() }.lut_size(), 65);
    }
}
