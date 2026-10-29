//! 区域追踪：在参照帧里圈出一块区域当模板，之后每一帧用“零均值归一化互相关”（ZNCC）找回它。
//!
//! - 金字塔由粗到细：先在缩小的图上大范围找，再逐层在 ±2 像素内细化，最后用抛物线拟合得到亚像素位置。
//! - 位置按匀速预测，只在预测附近的窗口里找；模板在匹配得很好时慢慢融入当前画面（适应光线和角度的缓慢变化）。
//! - 匹配分数太低就判定为“丢失”：位置停在最后一次可靠的地方，之后用参照帧的原始模板在越来越大的范围里重新找回。
//! - 向前、向后各跑一遍（从参照帧出发），再合并、补上丢失的空档、做轻度平滑。
//!
//! 这是模板追踪，不认识“物体是什么”：遮挡、剧烈变形、快速旋转或者尺寸大幅变化时会丢失或漂移。

use super::gray::{Gray, Integral, Pyramid};
use super::TrackPt;

/// 模板最小边长（追踪分辨率下的像素）
pub const MIN_BOX: usize = 8;
/// 粗层模板的像素数上限：决定金字塔用几层
const COARSE_AREA: usize = 1600;
/// 匹配分数低于这个就判定为丢失
const KEEP_SCORE: f32 = 0.40;
/// 丢失之后重新找回的要求更严，避免被相似的东西骗走
const REACQUIRE_SCORE: f32 = 0.62;
/// 匹配得好于这个分数时才更新模板
const UPDATE_SCORE: f32 = 0.75;
const BLEND: f32 = 0.12;

/// 一个模板：每一层金字塔上各有一份零均值的像素。
struct Template {
    levels: Vec<Patch>,
}

struct Patch {
    w: usize,
    h: usize,
    data: Vec<f32>,
    norm: f32,
}

impl Patch {
    fn cut(g: &Gray, x: usize, y: usize, w: usize, h: usize) -> Patch {
        let mut data = Vec::with_capacity(w * h);
        let mut sum = 0f64;
        for r in 0..h {
            let row = &g.px[(y + r) * g.w + x..(y + r) * g.w + x + w];
            for &v in row {
                data.push(f32::from(v));
                sum += f64::from(v);
            }
        }
        let mean = (sum / (w * h) as f64) as f32;
        let mut sq = 0f64;
        for v in &mut data {
            *v -= mean;
            sq += f64::from(*v) * f64::from(*v);
        }
        Patch { w, h, data, norm: sq.sqrt() as f32 }
    }

    fn blend(&mut self, other: &Patch) {
        let mut sq = 0f64;
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            *a = *a * (1.0 - BLEND) + *b * BLEND;
            sq += f64::from(*a) * f64::from(*a);
        }
        self.norm = sq.sqrt() as f32;
    }
}

/// 模板在 `level` 层的位置和尺寸（左上角取整、边长至少 4）。
fn level_rect(x0: usize, y0: usize, tw: usize, th: usize, level: usize, img: &Gray) -> (usize, usize, usize, usize) {
    let w = (tw >> level).max(4).min(img.w);
    let h = (th >> level).max(4).min(img.h);
    let x = (x0 >> level).min(img.w - w);
    let y = (y0 >> level).min(img.h - h);
    (x, y, w, h)
}

fn cut_template(pyr: &Pyramid, x0: usize, y0: usize, tw: usize, th: usize) -> Template {
    let levels = pyr
        .levels
        .iter()
        .enumerate()
        .map(|(l, img)| {
            let (x, y, w, h) = level_rect(x0, y0, tw, th, l, img);
            Patch::cut(img, x, y, w, h)
        })
        .collect();
    Template { levels }
}

/// 模板在 (x, y) 处的互相关分数（-1–1）。图像块几乎没有起伏（纯色）时返回 0。
#[inline]
fn zncc(img: &Gray, ii: &Integral, t: &Patch, x: usize, y: usize) -> f32 {
    let n = (t.w * t.h) as f64;
    let (s1, s2) = ii.rect(x, y, t.w, t.h);
    let var = s2 as f64 - (s1 as f64) * (s1 as f64) / n;
    if var < n * 4.0 || t.norm < 1e-3 {
        return 0.0;
    }
    let mut dot = 0f32;
    for r in 0..t.h {
        let row = &img.px[(y + r) * img.w + x..(y + r) * img.w + x + t.w];
        let trow = &t.data[r * t.w..(r + 1) * t.w];
        let mut acc = 0f32;
        for (a, b) in trow.iter().zip(row) {
            acc += a * f32::from(*b);
        }
        dot += acc;
    }
    (f64::from(dot) / (f64::from(t.norm) * var.sqrt())) as f32
}

struct Hit {
    /// 模板左上角（第 0 层，亚像素）
    x: f64,
    y: f64,
    score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// 区域中心（追踪分辨率下的像素）
    pub cx: f64,
    pub cy: f64,
    pub score: f32,
    pub lost: bool,
}

pub struct Tracker {
    w: usize,
    h: usize,
    tw: usize,
    th: usize,
    levels: usize,
    anchor: Template,
    cur: Template,
    cx: f64,
    cy: f64,
    vx: f64,
    vy: f64,
    good: (f64, f64),
    lost_run: u32,
    radius: f64,
}

impl Tracker {
    /// `rect` 是参照帧里的区域（x, y, 宽, 高，像素）。区域里几乎没有纹理（纯色、平滑渐变）时没法追踪，返回提示。
    pub fn new(frame: &Gray, rect: (f64, f64, f64, f64)) -> Result<Tracker, String> {
        let (w, h) = (frame.w, frame.h);
        if w < 16 || h < 16 {
            return Err("画面太小，无法追踪。".into());
        }
        let tw = (rect.2.round() as usize).clamp(MIN_BOX, w - 2);
        let th = (rect.3.round() as usize).clamp(MIN_BOX, h - 2);
        let x0 = ((rect.0 + rect.2 / 2.0 - tw as f64 / 2.0).round().max(0.0) as usize).min(w - tw);
        let y0 = ((rect.1 + rect.3 / 2.0 - th as f64 / 2.0).round().max(0.0) as usize).min(h - th);
        let mut levels = 0;
        while levels < 3 && ((tw >> levels) * (th >> levels) > COARSE_AREA) && (tw >> (levels + 1)) >= 6 && (th >> (levels + 1)) >= 6 {
            levels += 1;
        }
        let pyr = Pyramid::new(frame, levels);
        let anchor = cut_template(&pyr, x0, y0, tw, th);
        let base = &anchor.levels[0];
        let n = (base.w * base.h) as f64;
        if f64::from(base.norm) / n.sqrt() < 2.5 {
            return Err("框里几乎是纯色或平滑渐变，没有可以辨认的细节，无法追踪。请把框放在有纹理、有轮廓的物体上，或者把框画大一些。".into());
        }
        let cur = Template { levels: anchor.levels.iter().map(|p| Patch { w: p.w, h: p.h, data: p.data.clone(), norm: p.norm }).collect() };
        let (cx, cy) = (x0 as f64 + tw as f64 / 2.0, y0 as f64 + th as f64 / 2.0);
        Ok(Tracker { w, h, tw, th, levels, anchor, cur, cx, cy, vx: 0.0, vy: 0.0, good: (cx, cy), lost_run: 0, radius: (0.6 * tw.max(th) as f64).max(20.0) })
    }

    /// 模板的实际尺寸（像素）。
    pub fn size(&self) -> (usize, usize) {
        (self.tw, self.th)
    }

    /// 参照帧里区域中心。
    pub fn start(&self) -> (f64, f64) {
        self.good
    }

    fn locate(&self, pyr: &Pyramid, iis: &[Integral], t: &Template, pred_tl: (f64, f64), radius: f64) -> Hit {
        let top = self.levels;
        let search = |l: usize, cx: f64, cy: f64, r: i32, penalty: bool| -> (i32, i32, f32) {
            let img = &pyr.levels[l];
            let p = &t.levels[l];
            let (maxx, maxy) = ((img.w - p.w) as i32, (img.h - p.h) as i32);
            let (cx, cy) = (cx.round() as i32, cy.round() as i32);
            let (x0, x1) = ((cx - r).clamp(0, maxx), (cx + r).clamp(0, maxx));
            let (y0, y1) = ((cy - r).clamp(0, maxy), (cy + r).clamp(0, maxy));
            let (mut best, mut bx, mut by, mut braw) = (f32::MIN, x0, y0, f32::MIN);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let raw = zncc(img, &iis[l], p, x as usize, y as usize);
                    // 离预测位置越远越吃亏一点点：纹理重复时选更近的
                    let adj = if penalty {
                        let d = (((x - cx) * (x - cx) + (y - cy) * (y - cy)) as f32).sqrt() / (r.max(1) as f32);
                        raw - 0.04 * d * d
                    } else {
                        raw
                    };
                    if adj > best {
                        (best, bx, by, braw) = (adj, x, y, raw);
                    }
                }
            }
            (bx, by, braw)
        };
        let scale = (1usize << top) as f64;
        let r_top = (radius / scale).ceil() as i32 + 1;
        let (mut bx, mut by, mut score) = search(top, pred_tl.0 / scale, pred_tl.1 / scale, r_top, true);
        for l in (0..top).rev() {
            let (x, y, s) = search(l, f64::from(bx * 2), f64::from(by * 2), 2, false);
            (bx, by, score) = (x, y, s);
        }
        // 亚像素：在最佳位置十字相邻的四个分数上拟合抛物线
        let img = &pyr.levels[0];
        let p = &t.levels[0];
        let (maxx, maxy) = ((img.w - p.w) as i32, (img.h - p.h) as i32);
        let at = |x: i32, y: i32| zncc(img, &iis[0], p, x.clamp(0, maxx) as usize, y.clamp(0, maxy) as usize);
        let vertex = |m: f32, c: f32, pl: f32| -> f64 {
            let den = m - 2.0 * c + pl;
            if den.abs() < 1e-6 {
                0.0
            } else {
                f64::from((0.5 * (m - pl) / den).clamp(-0.5, 0.5))
            }
        };
        let (ox, oy) = if bx > 0 && by > 0 && bx < maxx && by < maxy {
            (vertex(at(bx - 1, by), score, at(bx + 1, by)), vertex(at(bx, by - 1), score, at(bx, by + 1)))
        } else {
            (0.0, 0.0)
        };
        Hit { x: f64::from(bx) + ox, y: f64::from(by) + oy, score }
    }

    /// 处理下一帧。`dt_ms` 是这一帧和上一帧的时间间隔（只用来预测位置，帧的间隔不要求相等）。
    pub fn step(&mut self, frame: &Gray, dt_ms: f64) -> Step {
        debug_assert!(frame.w == self.w && frame.h == self.h);
        let dt = dt_ms.max(1.0);
        let pyr = Pyramid::new(frame, self.levels);
        let iis: Vec<Integral> = pyr.levels.iter().map(Integral::new).collect();
        let lost = self.lost_run > 0;
        let (pcx, pcy) = if lost { self.good } else { (self.cx + self.vx * dt, self.cy + self.vy * dt) };
        let speed = (self.vx * self.vx + self.vy * self.vy).sqrt() * dt;
        let max_r = (self.w.max(self.h) as f64) * 0.5;
        let radius = if lost { (self.radius + 6.0 * f64::from(self.lost_run)).min(max_r) } else { self.radius.max(2.0 * speed + 4.0) };
        let ptl = (pcx - self.tw as f64 / 2.0, pcy - self.th as f64 / 2.0);
        let mut hit = self.locate(&pyr, &iis, &self.cur, ptl, radius);
        if lost || hit.score < 0.6 {
            let alt = self.locate(&pyr, &iis, &self.anchor, ptl, radius);
            if alt.score > hit.score {
                hit = alt;
            }
        }
        let need = if lost { REACQUIRE_SCORE } else { KEEP_SCORE };
        if hit.score >= need {
            let (ncx, ncy) = (hit.x + self.tw as f64 / 2.0, hit.y + self.th as f64 / 2.0);
            if lost {
                (self.vx, self.vy) = (0.0, 0.0);
            } else {
                let cap = self.radius / dt;
                self.vx = (0.5 * self.vx + 0.5 * (ncx - self.cx) / dt).clamp(-cap, cap);
                self.vy = (0.5 * self.vy + 0.5 * (ncy - self.cy) / dt).clamp(-cap, cap);
            }
            (self.cx, self.cy) = (ncx, ncy);
            self.good = (ncx, ncy);
            self.lost_run = 0;
            if hit.score >= UPDATE_SCORE {
                let x0 = (hit.x.round().max(0.0) as usize).min(self.w - self.tw);
                let y0 = (hit.y.round().max(0.0) as usize).min(self.h - self.th);
                let fresh = cut_template(&pyr, x0, y0, self.tw, self.th);
                for (c, f) in self.cur.levels.iter_mut().zip(&fresh.levels) {
                    c.blend(f);
                }
            }
            Step { cx: ncx, cy: ncy, score: hit.score, lost: false }
        } else {
            self.lost_run += 1;
            (self.vx, self.vy) = (0.0, 0.0);
            (self.cx, self.cy) = self.good;
            Step { cx: self.good.0, cy: self.good.1, score: hit.score, lost: true }
        }
    }
}

/// 一个采样：这个时刻区域中心在追踪画面里的位置。
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// 这一帧在素材里的真实时间（毫秒）
    pub t_ms: f64,
    pub cx: f64,
    pub cy: f64,
    pub lost: bool,
    pub score: f32,
    /// 是参照帧
    pub reference: bool,
}

/// 带真实时间的灰度帧。
#[derive(Clone)]
pub struct Timed {
    pub gray: Gray,
    pub t_ms: f64,
}

/// 追踪用的取帧：参照帧，以及它之后 / 之前依次相邻的帧（间隔不要求相等，每帧带自己的真实时间）。
/// 测试里是内存里的一组帧，运行时是 ffmpeg 解码的分块。
pub trait Frames {
    /// 参照帧。
    fn reference(&mut self) -> Result<Option<Timed>, String>;
    /// 参照帧之后的下一帧；到头了返回 None。
    fn next_forward(&mut self) -> Result<Option<Timed>, String>;
    /// 参照帧之前的下一帧（越来越早）；到头了返回 None。
    fn next_backward(&mut self) -> Result<Option<Timed>, String>;
}

/// 追踪没有完成的原因。
#[derive(Debug)]
pub enum Stop {
    Canceled,
    Failed(String),
}

/// 一次追踪的结果（追踪分辨率下）。
pub struct Followed {
    pub samples: Vec<Sample>,
    /// 模板的实际尺寸
    pub size: (usize, usize),
    /// 追踪用的画面尺寸
    pub frame: (usize, usize),
}

/// 从参照帧出发，向前、向后各追踪到取帧来源给不出帧为止。
/// `back_ms` / `fwd_ms` 只用来算进度（0–1）；`stop` 返回 true 时尽快返回 `Err(Stop::Canceled)`。
pub fn follow(
    frames: &mut dyn Frames,
    rect_px: (f64, f64, f64, f64),
    back_ms: f64,
    fwd_ms: f64,
    progress: &mut dyn FnMut(f64),
    stop: &dyn Fn() -> bool,
) -> Result<Followed, Stop> {
    let first = match frames.reference() {
        Ok(Some(f)) => f,
        Ok(None) => return Err(Stop::Failed("读不到参照帧。".into())),
        Err(m) => return Err(Stop::Failed(m)),
    };
    let proto = Tracker::new(&first.gray, rect_px).map_err(Stop::Failed)?;
    let size = proto.size();
    let (w, h) = (first.gray.w, first.gray.h);
    let total = (fwd_ms.max(0.0) + back_ms.max(0.0)).max(1.0);
    let mut done_ms = 0f64;
    let mut out: Vec<Sample> = vec![Sample { t_ms: first.t_ms, cx: proto.start().0, cy: proto.start().1, lost: false, score: 1.0, reference: true }];
    for forward in [true, false] {
        let mut t = Tracker::new(&first.gray, rect_px).map_err(Stop::Failed)?;
        let mut prev_t = first.t_ms;
        let mut n = 0u32;
        loop {
            if stop() {
                return Err(Stop::Canceled);
            }
            let next = if forward { frames.next_forward() } else { frames.next_backward() };
            let f = match next {
                Ok(Some(f)) if f.gray.w == w && f.gray.h == h => f,
                Ok(_) => break,
                Err(m) => return Err(Stop::Failed(m)),
            };
            let s = t.step(&f.gray, (f.t_ms - prev_t).abs());
            out.push(Sample { t_ms: f.t_ms, cx: s.cx, cy: s.cy, lost: s.lost, score: s.score, reference: false });
            done_ms += (f.t_ms - prev_t).abs();
            prev_t = f.t_ms;
            n += 1;
            if n % 8 == 0 {
                progress((done_ms / total).min(1.0));
            }
        }
    }
    out.sort_by(|a, b| a.t_ms.total_cmp(&b.t_ms));
    Ok(Followed { samples: out, size, frame: (w, h) })
}

/// 追踪结果整理：补上丢失的空档（再找回时用直线连接，到头没找回就停在最后可靠的位置）、轻度平滑，
/// 换算成相对整个画面的比例，再按误差精简（匀速的一段只留两头）。
/// `frame` 是追踪分辨率下的画面尺寸，`size` 是模板尺寸。
pub fn finish(samples: &[Sample], frame: (usize, usize), size: (usize, usize)) -> Vec<TrackPt> {
    let n = samples.len();
    if n == 0 {
        return vec![];
    }
    let mut cx: Vec<f64> = samples.iter().map(|s| s.cx).collect();
    let mut cy: Vec<f64> = samples.iter().map(|s| s.cy).collect();
    let lost: Vec<bool> = samples.iter().map(|s| s.lost).collect();
    // 丢失的空档：前后都有可靠的点就按时间直线连接
    let mut i = 0;
    while i < n {
        if !lost[i] {
            i += 1;
            continue;
        }
        let a = i;
        while i < n && lost[i] {
            i += 1;
        }
        let b = i; // 第一个又可靠的点（可能是 n）
        if a > 0 && b < n {
            let (ta, tb) = (samples[a - 1].t_ms, samples[b].t_ms);
            for j in a..b {
                let f = if tb > ta { (samples[j].t_ms - ta) / (tb - ta) } else { 0.0 };
                cx[j] = cx[a - 1] + (cx[b] - cx[a - 1]) * f;
                cy[j] = cy[a - 1] + (cy[b] - cy[a - 1]) * f;
            }
        }
    }
    // 平滑：二项式（最宽 5 点），两边对称取点，所以匀速运动不会被抹偏；碰到两端或者“丢失 / 可靠”的分界就收窄，
    // 丢失的空档里的点不参与可靠的点（避免把停住的位置抹进去）
    let smooth = |v: &[f64]| -> Vec<f64> {
        const KERNELS: [&[f64]; 3] = [&[1.0], &[1.0, 2.0, 1.0], &[1.0, 4.0, 6.0, 4.0, 1.0]];
        (0..n)
            .map(|j| {
                let mut m = 0;
                while m < 2 && j > m && j + m + 1 < n && lost[j - m - 1] == lost[j] && lost[j + m + 1] == lost[j] {
                    m += 1;
                }
                let k = KERNELS[m];
                let total: f64 = k.iter().sum();
                k.iter().enumerate().map(|(o, w)| v[j + o - m] * w).sum::<f64>() / total
            })
            .collect()
    };
    let (sx, sy) = (smooth(&cx), smooth(&cy));
    let (fw, fh) = (frame.0 as f64, frame.1 as f64);
    let mut pts: Vec<TrackPt> = Vec::with_capacity(n);
    for (j, s) in samples.iter().enumerate() {
        let (x, y) = if s.reference { (cx[j], cy[j]) } else { (sx[j], sy[j]) };
        pts.push(TrackPt {
            t_ms: s.t_ms.round().max(0.0) as u64,
            x: round5((x - size.0 as f64 / 2.0) / fw),
            y: round5((y - size.1 as f64 / 2.0) / fh),
            w: round5(size.0 as f64 / fw),
            h: round5(size.1 as f64 / fh),
            lost: lost[j],
            pin: s.reference,
        });
    }
    pts.dedup_by_key(|p| p.t_ms);
    simplify(&pts, 0.0012)
}

fn round5(v: f64) -> f64 {
    (v * 100_000.0).round() / 100_000.0
}

/// 精简轨迹：用 Douglas-Peucker 去掉能被前后两点直线插值还原（误差小于 `eps`，画面比例）的点。
/// 参照点（pin）和丢失状态变化处的点一定保留。
pub fn simplify(pts: &[TrackPt], eps: f64) -> Vec<TrackPt> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    for i in 0..n {
        if pts[i].pin || (i > 0 && pts[i].lost != pts[i - 1].lost) || (i + 1 < n && pts[i].lost != pts[i + 1].lost) {
            keep[i] = true;
        }
    }
    let mut anchors: Vec<usize> = (0..n).filter(|&i| keep[i]).collect();
    anchors.dedup();
    let mut stack: Vec<(usize, usize)> = anchors.windows(2).map(|w| (w[0], w[1])).collect();
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (ta, tb) = (pts[a].t_ms as f64, pts[b].t_ms as f64);
        let (mut worst, mut wi) = (0f64, a);
        for i in a + 1..b {
            let f = if tb > ta { (pts[i].t_ms as f64 - ta) / (tb - ta) } else { 0.0 };
            let lerp = |u: f64, v: f64| u + (v - u) * f;
            let e = [
                (lerp(pts[a].x, pts[b].x) - pts[i].x).abs(),
                (lerp(pts[a].y, pts[b].y) - pts[i].y).abs(),
                (lerp(pts[a].w, pts[b].w) - pts[i].w).abs(),
                (lerp(pts[a].h, pts[b].h) - pts[i].h).abs(),
            ]
            .into_iter()
            .fold(0.0, f64::max);
            if e > worst {
                (worst, wi) = (e, i);
            }
        }
        if worst > eps {
            keep[wi] = true;
            stack.push((a, wi));
            stack.push((wi, b));
        }
    }
    pts.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect()
}

#[cfg(test)]
mod tests {
    use super::super::synth::{Scene, Shot};
    use super::*;

    /// 内存里的一组帧，相邻两帧相隔 `STEP` 毫秒。
    struct Seq {
        frames: Vec<Gray>,
        ref_idx: usize,
        fwd: usize,
        bwd: usize,
    }

    const STEP: f64 = 66.0;

    impl Seq {
        fn new(frames: Vec<Gray>, ref_idx: usize) -> Seq {
            Seq { frames, ref_idx, fwd: 0, bwd: 0 }
        }
        fn at(&self, i: usize) -> Timed {
            Timed { gray: self.frames[i].clone(), t_ms: 1000.0 + i as f64 * STEP }
        }
    }

    impl Frames for Seq {
        fn reference(&mut self) -> Result<Option<Timed>, String> {
            Ok(Some(self.at(self.ref_idx)))
        }
        fn next_forward(&mut self) -> Result<Option<Timed>, String> {
            self.fwd += 1;
            let i = self.ref_idx + self.fwd;
            Ok((i < self.frames.len()).then(|| self.at(i)))
        }
        fn next_backward(&mut self) -> Result<Option<Timed>, String> {
            self.bwd += 1;
            Ok((self.bwd <= self.ref_idx).then(|| self.at(self.ref_idx - self.bwd)))
        }
    }

    const W: usize = 160;
    const H: usize = 90;

    /// 圆形物体（直径 22）从左向右、略微起伏地穿过画面。
    fn path(i: usize) -> (f64, f64) {
        (30.0 + i as f64 * 2.4, 45.0 + 12.0 * (i as f64 * 0.12).sin())
    }

    /// 第 `i` 帧在序列里的下标：`(t − 1000) / STEP`
    fn idx(s: &Sample) -> usize {
        ((s.t_ms - 1000.0) / STEP).round() as usize
    }

    fn run(shots: &[Shot], ref_idx: usize, rect: (f64, f64, f64, f64)) -> (Vec<Sample>, (usize, usize)) {
        let sc = Scene::new(W, H, 22);
        let mut seq = Seq::new(shots.iter().map(|s| sc.frame(s)).collect(), ref_idx);
        let Ok(f) = follow(&mut seq, rect, 1e4, 1e4, &mut |_| {}, &|| false) else { panic!("追踪失败") };
        (f.samples, f.size)
    }

    #[test]
    fn follows_a_moving_object_forward_and_backward_from_the_middle() {
        let shots: Vec<Shot> = (0..41).map(|i| Shot { noise: 3.0, seed: i as u64, ..Shot::at(path(i).0, path(i).1) }).collect();
        let ref_idx = 20;
        let (cx, cy) = path(ref_idx);
        let (samples, size) = run(&shots, ref_idx, (cx - 11.0, cy - 11.0, 22.0, 22.0));
        assert_eq!(samples.len(), 41);
        assert!(samples.windows(2).all(|w| w[1].t_ms > w[0].t_ms), "按时间排好");
        assert_eq!(samples.iter().filter(|s| s.reference).count(), 1);
        assert_eq!(idx(&samples[20]), 20);
        assert_eq!(size, (22, 22));
        let mut worst = 0f64;
        for s in &samples {
            let (tx, ty) = path(idx(s));
            worst = worst.max(((s.cx - tx).powi(2) + (s.cy - ty).powi(2)).sqrt());
            assert!(!s.lost, "第 {} 帧不该丢失（分数 {}）", idx(s), s.score);
        }
        assert!(worst < 1.5, "最大误差 {worst:.2} 像素");
    }

    #[test]
    fn a_large_object_is_tracked_through_the_pyramid() {
        // 直径 56 的物体：模板 56×56 超过粗层的像素上限，会用到金字塔
        let (w, h) = (320usize, 180usize);
        let sc = Scene::new(w, h, 56);
        let pos = |i: usize| (70.0 + i as f64 * 4.0, 90.0 + 25.0 * (i as f64 * 0.15).sin());
        let frames: Vec<Gray> = (0..36).map(|i| sc.frame(&Shot { noise: 4.0, seed: i as u64, ..Shot::at(pos(i).0, pos(i).1) })).collect();
        let (cx, cy) = pos(0);
        let t = Tracker::new(&frames[0], (cx - 28.0, cy - 28.0, 56.0, 56.0)).unwrap();
        assert_eq!(t.levels, 1);
        let mut seq = Seq::new(frames, 0);
        let f = follow(&mut seq, (cx - 28.0, cy - 28.0, 56.0, 56.0), 0.0, 1e4, &mut |_| {}, &|| false).ok().unwrap();
        assert_eq!(f.samples.len(), 36);
        for s in &f.samples {
            let (tx, ty) = pos(idx(s));
            assert!(
                !s.lost && ((s.cx - tx).powi(2) + (s.cy - ty).powi(2)).sqrt() < 2.0,
                "第 {} 帧：({:.1},{:.1}) vs ({tx:.1},{ty:.1}) 分数 {}",
                idx(s),
                s.cx,
                s.cy,
                s.score
            );
        }
    }

    #[test]
    fn fast_motion_and_heavy_noise_do_not_lose_the_object() {
        // 每帧移动约 9 个像素（接近物体直径的 40%），噪点幅度 ±10
        let pos = |i: usize| (20.0 + i as f64 * 9.0, 30.0 + (i % 4) as f64 * 5.0);
        let shots: Vec<Shot> = (0..14).map(|i| Shot { noise: 10.0, seed: 100 + i as u64, ..Shot::at(pos(i).0, pos(i).1) }).collect();
        let (samples, _) = run(&shots, 0, (20.0 - 11.0, 30.0 - 11.0, 22.0, 22.0));
        for s in &samples {
            let (tx, ty) = pos(idx(s));
            assert!(
                !s.lost && ((s.cx - tx).powi(2) + (s.cy - ty).powi(2)).sqrt() < 2.5,
                "第 {} 帧：({:.1},{:.1}) vs ({tx:.1},{ty:.1}) 分数 {}",
                idx(s),
                s.cx,
                s.cy,
                s.score
            );
        }
    }

    #[test]
    fn frames_that_are_unevenly_spaced_still_predict_well() {
        // 物体匀速（每毫秒 0.1 像素），但帧的间隔忽长忽短：按时间预测，不是按帧数
        struct Uneven {
            sc: Scene,
            times: Vec<f64>,
            next: usize,
        }
        impl Uneven {
            fn shot(&self, t: f64) -> Timed {
                Timed { gray: self.sc.frame(&Shot::at(20.0 + t * 0.1, 45.0)), t_ms: 1000.0 + t }
            }
        }
        impl Frames for Uneven {
            fn reference(&mut self) -> Result<Option<Timed>, String> {
                Ok(Some(self.shot(0.0)))
            }
            fn next_forward(&mut self) -> Result<Option<Timed>, String> {
                self.next += 1;
                Ok(self.times.get(self.next - 1).map(|t| self.shot(*t)))
            }
            fn next_backward(&mut self) -> Result<Option<Timed>, String> {
                Ok(None)
            }
        }
        let times: Vec<f64> = vec![60.0, 160.0, 220.0, 340.0, 400.0, 520.0, 580.0, 700.0, 760.0, 880.0, 940.0, 1060.0];
        let mut u = Uneven { sc: Scene::new(W, H, 22), times, next: 0 };
        let f = follow(&mut u, (9.0, 34.0, 22.0, 22.0), 0.0, 1100.0, &mut |_| {}, &|| false).ok().unwrap();
        for s in &f.samples {
            let tx = 20.0 + (s.t_ms - 1000.0) * 0.1;
            assert!(!s.lost && (s.cx - tx).abs() < 1.2, "t={} cx={:.1} 应为 {tx:.1}", s.t_ms, s.cx);
        }
    }

    #[test]
    fn a_flat_box_cannot_be_tracked_and_says_why() {
        let sc = Scene::new(W, H, 22);
        let mut f = sc.frame(&Shot::at(40.0, 40.0));
        for y in 30..60 {
            for x in 100..130 {
                f.px[y * W + x] = 90;
            }
        }
        let e = Tracker::new(&f, (104.0, 34.0, 20.0, 20.0)).err().expect("flat");
        assert!(e.contains("纯色"), "{e}");
    }

    #[test]
    fn occlusion_marks_the_gap_as_lost_then_finds_the_object_again() {
        // 物体匀速向右，第 14–22 帧被一堵墙挡住，之后重新出现
        let shots: Vec<Shot> = (0..40)
            .map(|i| {
                let (x, y) = path(i);
                let wall = (14..=22).contains(&i).then_some((x as usize - 16, 20, 40, 60));
                Shot { wall, noise: 2.0, seed: i as u64, ..Shot::at(x, y) }
            })
            .collect();
        let (cx, cy) = path(4);
        let (samples, _) = run(&shots, 4, (cx - 11.0, cy - 11.0, 22.0, 22.0));
        let lost: Vec<usize> = samples.iter().filter(|s| s.lost).map(idx).collect();
        assert!(lost.len() >= 5 && lost.iter().all(|i| (13..=24).contains(i)), "丢失的帧：{lost:?}");
        // 被挡住期间停在最后可靠的位置；重新出现之后回到物体上
        for s in samples.iter().filter(|s| idx(s) >= 28) {
            let (tx, ty) = path(idx(s));
            assert!(!s.lost, "第 {} 帧应该已经找回", idx(s));
            assert!(((s.cx - tx).powi(2) + (s.cy - ty).powi(2)).sqrt() < 2.0, "第 {} 帧偏了：({:.1},{:.1}) vs ({tx:.1},{ty:.1})", idx(s), s.cx, s.cy);
        }
    }

    #[test]
    fn finish_fills_gaps_smooths_and_converts_to_fractions() {
        let mk = |t: f64, x: f64, lost: bool, reference: bool| Sample { t_ms: t, cx: x, cy: 40.0, lost, score: 0.9, reference };
        let samples = [
            mk(800.0, 20.0, false, false),
            mk(900.0, 24.0, false, false),
            mk(1000.0, 28.0, false, true),
            mk(1100.0, 28.0, true, false),
            mk(1200.0, 28.0, true, false),
            mk(1300.0, 40.0, false, false),
            mk(1400.0, 44.0, false, false),
        ];
        let pts = finish(&samples, (160, 90), (20, 20));
        let find = |t: u64| pts.iter().find(|p| p.t_ms == t);
        assert!(find(1000).is_some_and(|p| p.pin), "参照点一定保留");
        let first = &pts[0];
        assert_eq!(first.t_ms, 800);
        assert!((first.w - 0.125).abs() < 1e-9 && (first.h - 20.0 / 90.0).abs() < 1e-4, "{first:?}");
        // 位置是左上角：中心 x=20 → (20-10)/160，两端不被平滑拉偏
        assert!((first.x - 10.0 / 160.0).abs() < 0.002, "{first:?}");
        // 空档按时间直线连接：1100 → 32，1200 → 36（再减去模板半宽）
        let gap = pts.iter().filter(|p| p.lost).count();
        assert!(gap >= 1, "丢失的标记保留");
        assert!(pts.windows(2).all(|w| w[0].t_ms < w[1].t_ms));
        let mid = interpolate_at(&pts, 1150.0);
        assert!((mid - (34.0 - 10.0) / 160.0).abs() < 0.004, "{mid}");
    }

    fn interpolate_at(pts: &[TrackPt], t: f64) -> f64 {
        super::super::interpolate(pts, t).unwrap().x
    }

    #[test]
    fn simplify_drops_points_on_straight_runs_but_keeps_corners_and_pins() {
        let mk = |t: u64, x: f64, pin: bool| TrackPt { t_ms: t, x, y: 0.5, w: 0.1, h: 0.1, lost: false, pin };
        let straight: Vec<TrackPt> = (0..30).map(|i| mk(i * 100, i as f64 * 0.01, false)).collect();
        assert_eq!(simplify(&straight, 0.001).len(), 2);
        let mut bent: Vec<TrackPt> = (0..30).map(|i| mk(i * 100, if i < 15 { i as f64 * 0.01 } else { 0.15 - (i - 15) as f64 * 0.01 }, false)).collect();
        let kept = simplify(&bent, 0.001);
        assert_eq!(kept.len(), 3, "折返处保留");
        assert_eq!(kept[1].t_ms, 1500);
        bent[7].pin = true;
        assert!(simplify(&bent, 0.001).iter().any(|p| p.t_ms == 700), "参照点保留");
    }

    #[test]
    fn progress_and_cancel_are_honoured() {
        let shots: Vec<Shot> = (0..40).map(|i| Shot::at(path(i).0, path(i).1)).collect();
        let sc = Scene::new(W, H, 22);
        let frames: Vec<Gray> = shots.iter().map(|s| sc.frame(s)).collect();
        let (cx, cy) = path(0);
        let rect = (cx - 11.0, cy - 11.0, 22.0, 22.0);
        let mut seen = vec![];
        let r = follow(&mut Seq::new(frames.clone(), 0), rect, 0.0, 39.0 * STEP, &mut |p| seen.push(p), &|| false);
        assert!(r.is_ok() && !seen.is_empty() && seen.windows(2).all(|w| w[1] >= w[0]) && *seen.last().unwrap() <= 1.0);
        let calls = std::cell::Cell::new(0);
        let r = follow(&mut Seq::new(frames, 0), rect, 0.0, 1e4, &mut |_| {}, &|| {
            calls.set(calls.get() + 1);
            calls.get() > 5
        });
        assert!(matches!(r, Err(Stop::Canceled)), "取消后返回");
    }
}
