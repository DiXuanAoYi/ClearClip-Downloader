//! 运动区域感知：给定同一个镜头里前后相邻的几帧，找出正在运动的物体的位置。
//!
//! 做法（不认识物体是什么，只看“哪里在动”）：
//! 1. 先估计每个相邻帧相对当前帧的整体平移（镜头平移、手抖），按它对齐后再比较，这样背景不会被当成运动。
//! 2. 三帧差分：当前帧与前、后两帧各比较一次，取两者中较小的——物体在“当前位置”时两次都有差别，
//!    而它在前一帧、后一帧所在的位置只在其中一次有差别，会被去掉，所以得到的是物体当前所在的位置，没有拖影。
//! 3. 用画面本身的噪点水平（中位数 + MAD）定阈值，在 4×4 的小格子上统计，把相邻的格子连成块，每一块给出外框和强度。
//!
//! 局限：静止的物体感知不到；镜头在旋转、变焦，或整个画面在闪烁时结果不可靠（会提示）。

use super::gray::{Gray, Pyramid};
use super::NRect;

/// 小格子的边长（像素）
const CELL: usize = 4;
const MAX_CANDIDATES: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub rect: NRect,
    /// 越大越显眼（运动的面积 × 强度）
    pub score: f64,
}

#[derive(Debug, Default)]
pub struct Motion {
    pub candidates: Vec<Candidate>,
    /// 相邻帧之间整体平移的最大值，占画面宽度的比例（镜头在动的程度）
    pub camera: f64,
    /// 画面里在“动”的面积占比
    pub coverage: f64,
    pub note: Option<String>,
}

/// 估计 `b` 相对 `a` 的整体平移：`b(x+dx, y+dy) ≈ a(x, y)`。
/// 从粗到细搜索；比较时只取差别最小的 70% 的像素，这样运动的物体不会把结果带偏。
pub fn estimate_shift(a: &Gray, b: &Gray) -> (i32, i32) {
    let pa = Pyramid::new(a, 2);
    let pb = Pyramid::new(b, 2);
    let (mut dx, mut dy) = (0i32, 0i32);
    for l in (0..=2).rev() {
        let (ia, ib) = (&pa.levels[l], &pb.levels[l]);
        let r = if l == 2 { 5 } else { 2 };
        let (cx, cy) = (dx * 2, dy * 2);
        let (cx, cy) = if l == 2 { (0, 0) } else { (cx, cy) };
        let (mut best, mut bx, mut by) = (u64::MAX, cx, cy);
        for sy in cy - r..=cy + r {
            for sx in cx - r..=cx + r {
                let s = trimmed_sad(ia, ib, sx, sy);
                // 同样好时倾向于不动
                if s < best || (s == best && sx.abs() + sy.abs() < bx.abs() + by.abs()) {
                    (best, bx, by) = (s, sx, sy);
                }
            }
        }
        (dx, dy) = (bx, by);
    }
    (dx, dy)
}

fn trimmed_sad(a: &Gray, b: &Gray, dx: i32, dy: i32) -> u64 {
    let (w, h) = (a.w as i32, a.h as i32);
    let margin = 2;
    let mut diffs: Vec<u8> = Vec::with_capacity(a.px.len());
    for y in margin..h - margin {
        let by = y + dy;
        if by < 0 || by >= h {
            continue;
        }
        for x in margin..w - margin {
            let bx = x + dx;
            if bx < 0 || bx >= w {
                continue;
            }
            diffs.push(a.px[(y * w + x) as usize].abs_diff(b.px[(by * w + bx) as usize]));
        }
    }
    if diffs.len() < 16 {
        return u64::MAX;
    }
    let keep = diffs.len() * 7 / 10;
    diffs.select_nth_unstable(keep);
    let sum: u64 = diffs[..keep].iter().map(|&v| u64::from(v)).sum();
    // 重叠面积不同的候选位置之间要能比较：用平均值（放大 1000 倍保留精度）
    sum * 1000 / keep as u64
}

/// |center(x, y) − other(x+dx, y+dy)|，对不上的边缘记 0。
fn aligned_diff(center: &Gray, other: &Gray, dx: i32, dy: i32) -> Vec<u8> {
    let (w, h) = (center.w as i32, center.h as i32);
    let mut out = vec![0u8; center.px.len()];
    for y in 0..h {
        let oy = y + dy;
        if oy < 0 || oy >= h {
            continue;
        }
        for x in 0..w {
            let ox = x + dx;
            if ox < 0 || ox >= w {
                continue;
            }
            out[(y * w + x) as usize] = center.px[(y * w + x) as usize].abs_diff(other.px[(oy * w + ox) as usize]);
        }
    }
    out
}

fn median(v: &mut [u8]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mid = v.len() / 2;
    f64::from(*v.select_nth_unstable(mid).1)
}

/// `others` 是（相对当前帧的序号，画面）：-1 = 前一帧，+1 = 后一帧，±2 = 隔一帧。
pub fn detect(center: &Gray, others: &[(i32, &Gray)]) -> Motion {
    let mut out = Motion::default();
    let (w, h) = (center.w, center.h);
    let c = center.blur3();
    // 对齐并求差
    let mut diffs: Vec<(i32, Vec<u8>)> = vec![];
    for (off, g) in others {
        if g.w != w || g.h != h {
            continue;
        }
        let gb = g.blur3();
        let (dx, dy) = estimate_shift(&c, &gb);
        out.camera = out.camera.max(f64::from(dx).hypot(f64::from(dy)) / w as f64);
        diffs.push((*off, aligned_diff(&c, &gb, dx, dy)));
    }
    let get = |o: i32| diffs.iter().find(|(k, _)| *k == o).map(|(_, d)| d);
    // 组合：前后各一帧最好；缺一边时用同一边的两帧
    let mut pairs: Vec<(&Vec<u8>, &Vec<u8>)> = vec![];
    for (a, b) in [(-1, 1), (-2, 2), (-1, -2), (1, 2)] {
        if let (Some(da), Some(db)) = (get(a), get(b)) {
            // (-1,-2)/(1,2) 只在没有前后配对时才用
            if (a, b) == (-1, 1) || (a, b) == (-2, 2) || pairs.is_empty() {
                pairs.push((da, db));
            }
        }
    }
    if pairs.is_empty() {
        out.note = Some("相邻的画面不够（太靠近视频开头或结尾），感知不出运动。".into());
        return out;
    }
    let mut d = vec![0u8; w * h];
    for (da, db) in &pairs {
        for i in 0..d.len() {
            d[i] = d[i].max(da[i].min(db[i]));
        }
    }
    // 阈值：画面本身的噪点水平
    let mut sample = d.clone();
    let med = median(&mut sample);
    let mut dev: Vec<u8> = d.iter().map(|&v| (f64::from(v) - med).abs() as u8).collect();
    let mad = median(&mut dev);
    let thr = (med + 6.0 * 1.4826 * mad).clamp(14.0, 70.0);

    // 4×4 的格子：有足够多的像素超过阈值就算“在动”
    let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut active = vec![false; gw * gh];
    let mut strength = vec![0f64; gw * gh];
    for gy in 0..gh {
        for gx in 0..gw {
            let (mut n, mut cnt, mut sum) = (0usize, 0usize, 0f64);
            for y in gy * CELL..((gy + 1) * CELL).min(h) {
                for x in gx * CELL..((gx + 1) * CELL).min(w) {
                    n += 1;
                    let v = d[y * w + x];
                    if f64::from(v) > thr {
                        cnt += 1;
                        sum += f64::from(v);
                    }
                }
            }
            if cnt * 4 >= n {
                active[gy * gw + gx] = true;
                strength[gy * gw + gx] = sum;
            }
        }
    }
    out.coverage = active.iter().filter(|&&a| a).count() as f64 / active.len() as f64;
    if out.coverage > 0.5 {
        out.note = Some("整个画面几乎都在变化（镜头在大幅移动、变焦，或者画面在闪烁），感知不到单独的运动物体。".into());
        return out;
    }
    // 膨胀 2 格，把物体内部没有纹理的空洞和相邻的碎块连起来
    let dil = dilate(&active, gw, gh, 2);
    // 连通块（8 邻域）
    let mut seen = vec![false; gw * gh];
    let min_cells = ((gw * gh) as f64 * 0.0015).ceil().max(2.0) as usize;
    let mut found: Vec<(f64, usize, usize, usize, usize)> = vec![];
    for start in 0..gw * gh {
        if !dil[start] || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let (mut x0, mut y0, mut x1, mut y1) = (gw, gh, 0usize, 0usize);
        let (mut n_active, mut power) = (0usize, 0f64);
        while let Some(i) = stack.pop() {
            let (cx, cy) = (i % gw, i / gw);
            x0 = x0.min(cx);
            x1 = x1.max(cx);
            y0 = y0.min(cy);
            y1 = y1.max(cy);
            if active[i] {
                n_active += 1;
                power += strength[i];
            }
            for ny in cy.saturating_sub(1)..=(cy + 1).min(gh - 1) {
                for nx in cx.saturating_sub(1)..=(cx + 1).min(gw - 1) {
                    let j = ny * gw + nx;
                    if dil[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if n_active >= min_cells {
            found.push((power, x0, y0, x1, y1));
        }
    }
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (power, x0, y0, x1, y1) in found.into_iter().take(MAX_CANDIDATES) {
        // 外框：取这一块里“动”的像素的范围（比膨胀后的格子更贴近物体）
        let (px0, py0, px1, py1) =
            ((x0 * CELL).saturating_sub(CELL), (y0 * CELL).saturating_sub(CELL), ((x1 + 1) * CELL + CELL).min(w), ((y1 + 1) * CELL + CELL).min(h));
        let (mut lx, mut ly, mut rx, mut ry) = (px1, py1, px0, py0);
        let mut any = false;
        let col_hits: Vec<usize> = (px0..px1).map(|x| (py0..py1).filter(|&y| f64::from(d[y * w + x]) > thr).count()).collect();
        let row_hits: Vec<usize> = (py0..py1).map(|y| (px0..px1).filter(|&x| f64::from(d[y * w + x]) > thr).count()).collect();
        for (i, &c) in col_hits.iter().enumerate() {
            if c >= 2 {
                lx = lx.min(px0 + i);
                rx = rx.max(px0 + i + 1);
                any = true;
            }
        }
        for (i, &r) in row_hits.iter().enumerate() {
            if r >= 2 {
                ly = ly.min(py0 + i);
                ry = ry.max(py0 + i + 1);
            }
        }
        if !any || rx <= lx || ry <= ly {
            continue;
        }
        // 向外留一点余量（差分只抓得到有纹理的部分）
        let pad_x = ((rx - lx) as f64 * 0.06).max(1.0);
        let pad_y = ((ry - ly) as f64 * 0.06).max(1.0);
        let r = NRect { x: ((lx as f64 - pad_x) / w as f64).max(0.0), y: ((ly as f64 - pad_y) / h as f64).max(0.0), w: 0.0, h: 0.0 };
        let r = NRect { w: (((rx as f64 + pad_x) / w as f64).min(1.0) - r.x).max(0.0), h: (((ry as f64 + pad_y) / h as f64).min(1.0) - r.y).max(0.0), ..r };
        // 几乎占满整个画面的不是物体
        if r.area() > 0.7 {
            continue;
        }
        out.candidates.push(Candidate { rect: r, score: power / 255.0 });
    }
    if out.candidates.is_empty() && out.note.is_none() {
        out.note = Some("这一刻没有感知到明显的运动物体。静止不动的物体感知不到，可以直接在画面上框选。".into());
    }
    if out.camera > 0.03 && out.note.is_none() {
        out.note = Some("镜头在移动，感知结果可能包含背景，请核对后再用。".into());
    }
    out
}

fn dilate(m: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    // 先横向再纵向
    let mut tmp = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if m[y * w + x] {
                for nx in x.saturating_sub(r)..=(x + r).min(w - 1) {
                    tmp[y * w + nx] = true;
                }
            }
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if tmp[y * w + x] {
                for ny in y.saturating_sub(r)..=(y + r).min(h - 1) {
                    out[ny * w + x] = true;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::synth::{Scene, Shot};
    use super::*;

    const W: usize = 160;
    const H: usize = 90;

    fn truth(cx: f64, cy: f64) -> NRect {
        NRect { x: (cx - 11.0) / W as f64, y: (cy - 11.0) / H as f64, w: 22.0 / W as f64, h: 22.0 / H as f64 }
    }

    fn shots(sc: &Scene, mk: impl Fn(i32) -> Shot) -> Vec<Gray> {
        (-2..=2).map(|i| sc.frame(&mk(i))).collect()
    }

    fn run(frames: &[Gray]) -> Motion {
        let others: Vec<(i32, &Gray)> = [(-2, 0), (-1, 1), (1, 3), (2, 4)].iter().map(|&(o, i)| (o, &frames[i])).collect();
        detect(&frames[2], &others)
    }

    #[test]
    fn finds_the_moving_object_without_ghosts() {
        let sc = Scene::new(W, H, 22);
        let f = shots(&sc, |i| Shot { noise: 3.0, seed: (i + 5) as u64, ..Shot::at(70.0 + f64::from(i) * 7.0, 40.0) });
        let m = run(&f);
        assert!(!m.candidates.is_empty(), "{:?}", m.note);
        let t = truth(70.0, 40.0);
        let best = &m.candidates[0];
        assert!(best.rect.iou(&t) > 0.5, "候选 {:?} 和真实位置 {t:?} 重合度 {:.2}", best.rect, best.rect.iou(&t));
        // 没有跑到前一帧、后一帧物体所在的位置（x = 56 和 84 附近）
        assert!(m.candidates.iter().all(|c| c.rect.iou(&truth(56.0, 40.0)) < 0.3 || c.rect.iou(&t) > 0.5), "{:?}", m.candidates);
        assert!(m.camera < 0.01);
    }

    #[test]
    fn a_static_scene_has_no_candidates() {
        let sc = Scene::new(W, H, 22);
        let f = shots(&sc, |i| Shot { noise: 4.0, seed: (i + 9) as u64, no_object: true, ..Shot::at(0.0, 0.0) });
        let m = run(&f);
        assert!(m.candidates.is_empty(), "{:?}", m.candidates);
        assert!(m.note.as_deref().is_some_and(|n| n.contains("没有感知到")));
    }

    #[test]
    fn camera_pan_is_compensated_so_the_background_is_not_reported() {
        let sc = Scene::new(W, H, 22);
        // 镜头每帧向右平移 3、向下 1 像素；物体在世界里也在动（画面里的位置 = 世界位置 − 镜头）
        let f = shots(&sc, |i| {
            let cam = (3 * i, i);
            Shot { cam, noise: 3.0, seed: (i + 20) as u64, ..Shot::at(70.0 + f64::from(i) * 6.0 - f64::from(cam.0), 40.0 - f64::from(cam.1)) }
        });
        let m = run(&f);
        assert!(m.camera > 0.01, "应该测出镜头在动：{}", m.camera);
        assert!(!m.candidates.is_empty(), "{:?}", m.note);
        assert!(m.candidates[0].rect.iou(&truth(70.0, 40.0)) > 0.4, "{:?}", m.candidates[0]);
        assert!(m.candidates.len() <= 2, "背景不该被当成运动：{:?}", m.candidates);
    }

    #[test]
    fn shift_estimation_recovers_integer_translations() {
        let sc = Scene::new(W, H, 22);
        let a = sc.frame(&Shot { no_object: true, ..Shot::at(0.0, 0.0) });
        let b = sc.frame(&Shot { cam: (4, -3), no_object: true, ..Shot::at(0.0, 0.0) });
        // b(x, y) = a(x + 4, y − 3)，所以 b(x − 4, y + 3) = a(x, y)
        assert_eq!(estimate_shift(&a, &b), (-4, 3));
        assert_eq!(estimate_shift(&a, &a), (0, 0));
    }

    #[test]
    fn at_the_start_of_a_video_only_later_frames_are_used() {
        let sc = Scene::new(W, H, 22);
        let frames: Vec<Gray> = (0..3).map(|i| sc.frame(&Shot { noise: 2.0, seed: i + 40, ..Shot::at(60.0 + i as f64 * 7.0, 40.0) })).collect();
        let others = [(1, &frames[1]), (2, &frames[2])];
        let m = detect(&frames[0], &others);
        assert!(!m.candidates.is_empty(), "{:?}", m.note);
        assert!(m.candidates[0].rect.iou(&truth(60.0, 40.0)) > 0.4, "{:?}", m.candidates[0]);
        // 只有一边的一帧：感知不出
        let only = [(1, &frames[1])];
        assert!(detect(&frames[0], &only).note.is_some_and(|n| n.contains("不够")));
    }

    #[test]
    fn a_flashing_screen_is_reported_as_unreliable() {
        let sc = Scene::new(W, H, 22);
        let mut f = shots(&sc, |i| Shot { no_object: true, seed: i as u64, ..Shot::at(0.0, 0.0) });
        for (i, g) in f.iter_mut().enumerate() {
            if i % 2 == 1 {
                for v in &mut g.px {
                    *v = v.saturating_add(80);
                }
            }
        }
        let m = run(&f);
        assert!(m.candidates.is_empty());
    }
}
