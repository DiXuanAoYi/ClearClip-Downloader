//! 区域效果：把追踪得到的轨迹变成画面效果。
//!
//! - 马赛克 / 模糊 / 局部调色：先在 Rust 里按轨迹逐帧画出“遮罩”（白 = 有效果、黑 = 没有，边缘可羽化），
//!   编码成一个很小的灰度视频；ffmpeg 里把整幅画面做一份带效果的拷贝，用遮罩当透明度叠回原画面。
//!   这样效果的形状、羽化、反选（作用于区域以外）都由遮罩决定，滤镜图本身不随区域的运动变化。
//! - 聚焦：不用遮罩，直接让裁切窗口跟着区域走（`crop` 的位置是随时间变化的表达式，用平滑后的轨迹拟合成不多的折线段）。
//!
//! 时间换算：遮罩的第 k 帧对应片段在时间线上的 `k / fps` 秒，也就是素材里的
//! `in_ms + 那个时间 × 速度`；轨迹都在素材时间里，所以片段被分割、裁剪、变速之后位置仍然对得上。
//! 遮罩帧率和画面帧率相同时，每一帧画面正好用到时间相同的那一帧遮罩。

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::spec::Region;
use crate::error::{AppError, AppResult};
use crate::track::{interpolate, NRect};

/// 遮罩画面的长边（像素）。遮罩在滤镜图里会放大到素材的尺寸，边缘本来就是柔和的，不需要太大
const MASK_LONG_SIDE: f64 = 800.0;
const MASK_LONG_SIDE_PREVIEW: f64 = 480.0;
/// 聚焦窗口的位置最多用多少个折线端点来描述
const MAX_KNOTS: usize = 64;
/// 聚焦轨迹的取样率（次/秒）
const FOCUS_SAMPLE_HZ: f64 = 30.0;

impl Region {
    /// 素材时间 `t_ms` 这一刻区域所在的矩形（已经按“扩大”调整）；没有轨迹时是 `None`。
    pub fn box_at(&self, t_ms: f64) -> Option<NRect> {
        let r = interpolate(&self.track, t_ms)?;
        let k = 1.0 + self.grow;
        let (w, h) = (r.w * k, r.h * k);
        Some(NRect { x: r.x + (r.w - w) / 2.0, y: r.y + (r.h - h) / 2.0, w, h })
    }

    pub fn active_at(&self, t_ms: f64) -> bool {
        self.start_ms.map_or(true, |s| t_ms >= s as f64) && self.end_ms.map_or(true, |e| t_ms < e as f64)
    }
}

/// 一个要生成的遮罩视频。
#[derive(Debug, Clone)]
pub struct MaskJob {
    pub path: PathBuf,
    pub region: Region,
    pub w: usize,
    pub h: usize,
    pub fps: u32,
    pub frames: u32,
    /// 片段取自素材的哪里、多快播放（把遮罩的时间换算成素材时间）
    pub in_ms: u64,
    pub speed: f64,
}

impl MaskJob {
    /// `work` 是区域效果处理时的画面尺寸，`src_fps` 是素材帧率，`dur_ms` 是片段在时间线上的时长。
    #[allow(clippy::too_many_arguments)]
    pub fn new(path: PathBuf, region: Region, in_ms: u64, speed: f64, dur_ms: u64, work: (u32, u32), src_fps: Option<f64>, preview: bool) -> MaskJob {
        let long = if preview { MASK_LONG_SIDE_PREVIEW } else { MASK_LONG_SIDE };
        let s = (long / f64::from(work.0.max(work.1).max(1))).min(1.0);
        let w = ((f64::from(work.0) * s).round() as usize).max(16);
        let h = ((f64::from(work.1) * s).round() as usize).max(16);
        // 遮罩的帧率：不低于变速后画面的帧率（素材帧率 × 速度），这样每一帧画面都有对应的遮罩；太低会一格一格地跳
        let cap = if preview { 24 } else { 60 };
        let fps = ((src_fps.unwrap_or(30.0) * speed).round() as u32).clamp(12, cap);
        let frames = ((dur_ms as f64 / 1000.0 * f64::from(fps)).ceil() as u32).saturating_add(1);
        MaskJob { path, region, w, h, fps, frames, in_ms, speed }
    }

    /// 第 k 帧对应的素材时间（毫秒）。合成时每一帧画面用“不晚于它的最近一帧遮罩”，帧率相同时时间正好重合
    pub fn src_time_ms(&self, k: u32) -> f64 {
        self.in_ms as f64 + f64::from(k) * 1000.0 / f64::from(self.fps) * self.speed
    }

    /// 画出第 k 帧遮罩（灰度，行优先，`w × h` 字节）。
    pub fn fill(&self, k: u32, buf: &mut Vec<u8>) {
        let (w, h) = (self.w, self.h);
        let r = &self.region;
        let t = self.src_time_ms(k);
        let active = r.active_at(t);
        buf.clear();
        // 生效时间之外：没有效果；之内：区域里有效果（反选时相反），先填底色再画区域附近
        buf.resize(w * h, if active && r.invert { 255 } else { 0 });
        if !active {
            return;
        }
        let Some(b) = r.box_at(t).filter(|b| b.w > 1e-6 && b.h > 1e-6) else { return };
        let (cx, cy) = ((b.x + b.w / 2.0) * w as f64, (b.y + b.h / 2.0) * h as f64);
        let (hw, hh) = (b.w * w as f64 / 2.0, b.h * h as f64 / 2.0);
        // 羽化：过渡带的半宽。框的边缘是 50% 的位置；最少 0.75 像素，当作抗锯齿
        let fw = (r.feather * hw.min(hh)).max(0.75);
        let pad = fw + 1.0;
        let x0 = (cx - hw - pad).floor().max(0.0) as usize;
        let x1 = ((cx + hw + pad).ceil().max(0.0) as usize).min(w);
        let y0 = (cy - hh - pad).floor().max(0.0) as usize;
        let y1 = ((cy + hh + pad).ceil().max(0.0) as usize).min(h);
        let ellipse = r.shape == "ellipse";
        for y in y0..y1 {
            let py = y as f64 + 0.5;
            for x in x0..x1 {
                let px = x as f64 + 0.5;
                // 到形状边缘的距离（正 = 在外面）
                let d = if ellipse {
                    let k = (((px - cx) / hw).powi(2) + ((py - cy) / hh).powi(2)).sqrt();
                    (k - 1.0) * hw.min(hh)
                } else {
                    let (qx, qy) = ((px - cx).abs() - hw, (py - cy).abs() - hh);
                    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)
                };
                let m = (0.5 - d / (2.0 * fw)).clamp(0.0, 1.0);
                let v = if r.invert { 1.0 - m } else { m };
                buf[y * w + x] = (v * 255.0 + 0.5) as u8;
            }
        }
    }
}

/// 把遮罩逐帧画出来，通过标准输入送给 ffmpeg，编码成无损的灰度视频（FFV1，装进 NUT）。`check` 在每一帧之间调用，用来响应取消。
pub async fn render_mask(ffmpeg: &Path, job: &MaskJob, check: impl Fn() -> AppResult<()>) -> AppResult<()> {
    if let Some(dir) = job.path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "gray",
        "-s",
        &format!("{}x{}", job.w, job.h),
        "-framerate",
        &job.fps.to_string(),
        "-i",
        "-",
    ])
    .args(["-an", "-c:v", "ffv1", "-pix_fmt", "gray", "-f", "nut"])
    .arg(&job.path)
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| AppError::msg(format!("无法运行 ffmpeg：{e}")))?;
    let mut stdin = child.stdin.take().ok_or_else(|| AppError::msg("无法写入 ffmpeg。"))?;
    let mut stderr = child.stderr.take().ok_or_else(|| AppError::msg("无法读取 ffmpeg 的输出。"))?;
    let log = tokio::spawn(async move {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s).await;
        s
    });
    let mut buf = Vec::with_capacity(job.w * job.h);
    for k in 0..job.frames {
        check()?;
        job.fill(k, &mut buf);
        if stdin.write_all(&buf).await.is_err() {
            break; // ffmpeg 提前结束了：下面读它的报错
        }
    }
    drop(stdin);
    let status = child.wait().await.map_err(|e| AppError::msg(format!("ffmpeg 运行失败：{e}")))?;
    let log = log.await.unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        let tail: String = log.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        let _ = std::fs::remove_file(&job.path);
        Err(AppError::msg(format!("生成区域遮罩失败：{tail}")))
    }
}

// ---------------------------------------------------------------- 聚焦

/// 聚焦窗口在某个时刻的左上角（像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Knot {
    /// 片段在时间线上的时间（秒）
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

fn even(v: f64) -> u32 {
    ((v.max(2.0) as u32) & !1).max(2)
}

/// 聚焦窗口的尺寸。`aspect` 有值时窗口取这个宽高比（在素材方向上）。
pub fn focus_window(zoom: f64, work: (u32, u32), aspect: Option<f64>) -> (u32, u32) {
    let (w, h) = (f64::from(work.0), f64::from(work.1));
    let (bw, bh) = match aspect.filter(|a| a.is_finite() && *a > 0.0) {
        Some(a) if a >= w / h => (w, w / a),
        Some(a) => (h * a, h),
        None => (w, h),
    };
    let z = zoom.max(1.0);
    (even(bw / z).min(work.0), even(bh / z).min(work.1))
}

/// 聚焦窗口的运动：区域中心按时间取样 → 高斯平滑 → 窗口放在中心周围并限制在画面内 → 用不多的折线端点逼近。
/// `dur_ms` 是片段在时间线上的时长，`work` 是处理时的画面尺寸，`win` 是窗口尺寸。
pub fn focus_knots(r: &Region, in_ms: u64, speed: f64, dur_ms: u64, work: (u32, u32), win: (u32, u32)) -> Vec<Knot> {
    let dur = (dur_ms as f64 / 1000.0).max(0.001);
    let n = ((dur * FOCUS_SAMPLE_HZ).ceil() as usize).max(1) + 1;
    let (fw, fh) = (f64::from(work.0), f64::from(work.1));
    let times: Vec<f64> = (0..n).map(|i| (i as f64 / FOCUS_SAMPLE_HZ).min(dur)).collect();
    let centers: Vec<(f64, f64)> = times
        .iter()
        .map(|t| {
            let b = interpolate(&r.track, in_ms as f64 + t * 1000.0 * speed).unwrap_or_default();
            ((b.x + b.w / 2.0) * fw, (b.y + b.h / 2.0) * fh)
        })
        .collect();
    // 高斯平滑；两端只用存在的邻居，权重重新归一
    let sigma = r.smooth * FOCUS_SAMPLE_HZ;
    let smooth: Vec<(f64, f64)> = if sigma < 0.5 {
        centers.clone()
    } else {
        let rad = (3.0 * sigma).ceil() as i64;
        let wts: Vec<f64> = (-rad..=rad).map(|i| (-((i * i) as f64) / (2.0 * sigma * sigma)).exp()).collect();
        (0..n as i64)
            .map(|i| {
                let (mut sx, mut sy, mut sw) = (0.0, 0.0, 0.0);
                for (j, wt) in (i - rad..=i + rad).zip(&wts) {
                    if (0..n as i64).contains(&j) {
                        sx += centers[j as usize].0 * wt;
                        sy += centers[j as usize].1 * wt;
                        sw += wt;
                    }
                }
                (sx / sw, sy / sw)
            })
            .collect()
    };
    let (cw, ch) = (f64::from(win.0), f64::from(win.1));
    let pts: Vec<Knot> = times
        .iter()
        .zip(&smooth)
        .map(|(t, (cx, cy))| Knot { t: *t, x: (cx - cw / 2.0).clamp(0.0, (fw - cw).max(0.0)), y: (cy - ch / 2.0).clamp(0.0, (fh - ch).max(0.0)) })
        .collect();
    let mut eps = 0.4;
    loop {
        let k = simplify(&pts, eps);
        if k.len() <= MAX_KNOTS {
            return k;
        }
        eps *= 1.6;
    }
}

/// Douglas–Peucker：保留首尾，其余点的位置（相对同一时刻的线性插值）偏差超过 `eps` 像素才留下。
fn simplify(pts: &[Knot], eps: f64) -> Vec<Knot> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (pts[a], pts[b]);
        let span = (pb.t - pa.t).max(1e-9);
        let (mut worst, mut at) = (0.0, 0);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let f = (p.t - pa.t) / span;
            let e = (p.x - (pa.x + (pb.x - pa.x) * f)).abs().max((p.y - (pa.y + (pb.y - pa.y) * f)).abs());
            if e > worst {
                (worst, at) = (e, i);
            }
        }
        if worst > eps {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// 折线 → ffmpeg 表达式（变量 `t` 是画面在片段里的时间，秒）。不动的轴直接写常数。
pub fn knot_expr(knots: &[Knot], pick: impl Fn(&Knot) -> f64) -> String {
    let v: Vec<f64> = knots.iter().map(&pick).collect();
    let Some(&last) = v.last() else { return "0".into() };
    if v.iter().all(|a| (a - v[0]).abs() < 0.01) {
        return format!("{:.2}", v[0]);
    }
    let mut e = format!("{last:.2}");
    for i in (0..v.len() - 1).rev() {
        let (t0, t1) = (knots[i].t, knots[i + 1].t);
        let slope = (v[i + 1] - v[i]) / (t1 - t0).max(1e-3);
        e = format!("if(lt(t,{t1:.3}),{:.2}+({slope:.4})*(t-{t0:.3}),{e})", v[i]);
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::TrackPt;

    fn pt(t: u64, x: f64, y: f64) -> TrackPt {
        TrackPt { t_ms: t, x, y, w: 0.2, h: 0.2, ..Default::default() }
    }

    fn region(track: Vec<TrackPt>) -> Region {
        Region { track, ..Default::default() }
    }

    fn job(r: Region) -> MaskJob {
        // 100×100 的遮罩，25 帧/秒，从素材 0 毫秒开始，原速
        MaskJob { path: PathBuf::from("m.nut"), region: r, w: 100, h: 100, fps: 25, frames: 50, in_ms: 0, speed: 1.0 }
    }

    fn at(buf: &[u8], x: usize, y: usize) -> u8 {
        buf[y * 100 + x]
    }

    #[test]
    fn a_rectangle_mask_covers_the_box_and_nothing_else() {
        // 框 (0.3,0.4)–(0.5,0.6)，不羽化
        let mut j = job(region(vec![pt(0, 0.3, 0.4)]));
        j.region.feather = 0.0;
        let mut b = vec![];
        j.fill(0, &mut b);
        assert_eq!(b.len(), 10_000);
        assert_eq!(at(&b, 40, 50), 255, "框里面");
        assert_eq!(at(&b, 31, 41), 255);
        assert_eq!(at(&b, 10, 50), 0, "框外面");
        assert_eq!(at(&b, 40, 70), 0);
        assert_eq!(at(&b, 55, 50), 0);
        // 边缘上抗锯齿，不会是硬边的两端
        assert!(at(&b, 29, 50) < 80 && at(&b, 30, 50) > 120, "{} {}", at(&b, 29, 50), at(&b, 30, 50));
        // 被覆盖的像素数 ≈ 框的面积（20×20）
        let lit = b.iter().filter(|v| **v > 127).count();
        assert!((390..=410).contains(&lit), "{lit}");
    }

    #[test]
    fn ellipse_feather_and_invert() {
        let mut r = region(vec![pt(0, 0.3, 0.3)]);
        r.shape = "ellipse".into();
        r.feather = 0.0;
        let mut j = job(r);
        let mut b = vec![];
        j.fill(0, &mut b);
        // 椭圆的圆心 (40,40)，半径 10：四个角不在里面，正中间在里面
        assert_eq!(at(&b, 40, 40), 255);
        assert_eq!(at(&b, 31, 31), 0, "框的角落在椭圆外面");
        assert_eq!(at(&b, 48, 40), 255);
        let ellipse_lit = b.iter().filter(|v| **v > 127).count();
        assert!((300..=330).contains(&ellipse_lit), "π×10² ≈ 314：{ellipse_lit}");
        // 羽化：边缘是渐变，中间仍然是满的
        j.region.shape = "rect".into();
        j.region.feather = 1.0;
        j.fill(0, &mut b);
        let row: Vec<u8> = (20..60).map(|x| at(&b, x, 40)).collect();
        assert!(at(&b, 40, 40) >= 245, "最中间基本不受羽化影响：{}", at(&b, 40, 40));
        assert!(row[..20].windows(2).all(|w| w[0] <= w[1]) && row[20..].windows(2).all(|w| w[0] >= w[1]), "左半边递增、右半边递减：{row:?}");
        assert!((60..=200).contains(&at(&b, 28, 40)) || (60..=200).contains(&at(&b, 32, 40)), "{row:?}");
        assert_eq!(at(&b, 5, 5), 0);
        // 反选：框里没有、框外有，且羽化处互补
        let before = b.clone();
        j.region.invert = true;
        j.fill(0, &mut b);
        assert!(before.iter().zip(&b).all(|(a, c)| (u16::from(*a) + u16::from(*c)).abs_diff(255) <= 1));
        assert_eq!(at(&b, 5, 5), 255);
        assert!(at(&b, 40, 40) <= 10);
    }

    #[test]
    fn the_box_moves_with_the_track_and_follows_clip_time_and_speed() {
        // 素材时间 0 → 2000 毫秒，框从 x=0.1 移到 0.5
        let mut j = job(region(vec![pt(0, 0.1, 0.4), pt(2000, 0.5, 0.4)]));
        j.region.feather = 0.0;
        let centre = |j: &MaskJob, k: u32| {
            let mut b = vec![];
            j.fill(k, &mut b);
            let xs: Vec<usize> = (0..100).filter(|x| at(&b, *x, 50) > 127).collect();
            (xs[0] + xs[xs.len() - 1]) as f64 / 2.0
        };
        // 第 0 帧在 0 毫秒（框中心 20），第 25 帧在 1000 毫秒（40）
        assert!((centre(&j, 0) - 20.0).abs() <= 1.5, "{}", centre(&j, 0));
        assert!((centre(&j, 25) - 40.0).abs() <= 1.5, "{}", centre(&j, 25));
        // 片段从素材 1000 毫秒开始、两倍速：时间线上的第 25 帧（1 秒）对应素材 3000 毫秒，轨迹停在最后一点
        j.in_ms = 1000;
        j.speed = 2.0;
        assert!((j.src_time_ms(25) - 3000.0).abs() < 1e-6);
        assert!((centre(&j, 25) - 60.0).abs() <= 1.5);
    }

    #[test]
    fn the_effect_only_exists_inside_the_active_range() {
        let mut r = region(vec![pt(0, 0.3, 0.3)]);
        r.start_ms = Some(500);
        r.end_ms = Some(1000);
        let j = job(r);
        let mut b = vec![];
        for (k, on) in [(0u32, false), (12, false), (13, true), (24, true), (25, false), (40, false)] {
            j.fill(k, &mut b);
            assert_eq!(b.iter().any(|v| *v > 0), on, "第 {k} 帧（素材 {} 毫秒）", j.src_time_ms(k));
        }
        // 反选 + 生效范围之外：整幅都没有效果（不是整幅都有）
        let mut j2 = j.clone();
        j2.region.invert = true;
        j2.fill(0, &mut b);
        assert!(b.iter().all(|v| *v == 0));
        j2.fill(13, &mut b);
        assert_eq!(at(&b, 5, 5), 255);
    }

    #[test]
    fn boxes_partly_outside_the_frame_and_grown_boxes_are_handled() {
        let mut b = vec![];
        let j = job(region(vec![pt(0, 0.9, 0.9)]));
        j.fill(0, &mut b);
        assert_eq!(b.len(), 10_000);
        assert_eq!(at(&b, 99, 99), 255, "只画在画面里的部分");
        assert_eq!(at(&b, 50, 50), 0);
        let far = job(region(vec![pt(0, 3.0, 3.0)]));
        far.fill(0, &mut b);
        assert!(b.iter().all(|v| *v == 0));
        // 扩大 50%：宽高各 ×1.5，中心不变
        let mut r = region(vec![pt(0, 0.4, 0.4)]);
        r.grow = 0.5;
        let g = r.box_at(0.0).unwrap();
        assert!((g.w - 0.3).abs() < 1e-12 && (g.x + g.w / 2.0 - 0.5).abs() < 1e-12);
        r.grow = -0.5;
        assert!((r.box_at(0.0).unwrap().w - 0.1).abs() < 1e-12);
        assert!(region(vec![]).box_at(0.0).is_none());
    }

    #[test]
    fn mask_sizes_and_rates_follow_the_clip() {
        let new = |work: (u32, u32), fps: Option<f64>, speed: f64, dur: u64, preview: bool| {
            MaskJob::new(PathBuf::from("m"), Region::default(), 0, speed, dur, work, fps, preview)
        };
        let j = new((1920, 1080), Some(29.97), 1.0, 2000, false);
        assert_eq!((j.w, j.h, j.fps, j.frames), (800, 450, 30, 61));
        let j = new((1080, 1920), Some(59.94), 1.0, 1000, false);
        assert_eq!((j.w, j.h, j.fps), (450, 800, 60));
        // 4 倍速：画面在时间线上的帧率是 4 倍，遮罩跟着提高（有上限）；慢放时不低于 12
        assert_eq!(new((640, 360), Some(30.0), 4.0, 1000, false).fps, 60);
        assert_eq!(new((640, 360), Some(30.0), 4.0, 1000, true).fps, 24);
        assert_eq!(new((640, 360), Some(30.0), 0.25, 1000, false).fps, 12);
        // 小画面不放大；预览的遮罩更小；不知道帧率按 30 算
        let j = new((320, 240), None, 1.0, 1000, true);
        assert_eq!((j.w, j.h, j.fps), (320, 240, 24));
        assert_eq!(new((1920, 1080), Some(30.0), 1.0, 1000, true).w, 480);
    }

    #[test]
    fn focus_window_sizes() {
        // 原比例放大 2 倍
        assert_eq!(focus_window(2.0, (1920, 1080), None), (960, 540));
        assert_eq!(focus_window(1.0, (1920, 1080), None), (1920, 1080));
        // 竖屏成片（9:16）从横屏素材里取窗口：高取满，宽 = 高 × 9/16；再放大 1.5 倍
        assert_eq!(focus_window(1.0, (1920, 1080), Some(9.0 / 16.0)), (606, 1080));
        assert_eq!(focus_window(1.5, (1920, 1080), Some(9.0 / 16.0)), (404, 720));
        // 宽的成片比例从竖屏素材里取：宽取满
        assert_eq!(focus_window(1.0, (1080, 1920), Some(16.0 / 9.0)), (1080, 606));
        // 偶数；不会比画面还大
        assert!(focus_window(3.3, (1001, 777), None).0 % 2 == 0);
        assert_eq!(focus_window(1.0, (100, 100), Some(100.0)), (100, 2));
    }

    #[test]
    fn the_focus_window_follows_a_smoothed_path_and_stays_inside_the_frame() {
        // 区域中心从画面左边（x=0.1）匀速走到右边（x=0.9），4 秒
        let r = Region { track: vec![pt(0, 0.0, 0.4), pt(4000, 0.8, 0.4)], smooth: 0.5, effect: "focus".into(), ..Default::default() };
        let work = (1000, 500);
        let win = (500, 250);
        let k = focus_knots(&r, 0, 1.0, 4000, work, win);
        assert!(k.len() >= 2 && k.len() <= MAX_KNOTS, "{}", k.len());
        assert_eq!((k[0].t, k.last().unwrap().t), (0.0, 4.0));
        assert!(k.windows(2).all(|w| w[0].t < w[1].t));
        // 窗口始终在画面里
        assert!(k.iter().all(|p| p.x >= 0.0 && p.x <= 500.0 && p.y >= 0.0 && p.y <= 250.0), "{k:?}");
        // 起点：中心在 100 像素，窗口会被挡在左边界（x=0）；终点：中心 900，窗口贴右边界（x=500）
        assert!(k[0].x.abs() < 1e-6 && (k.last().unwrap().x - 500.0).abs() < 1e-6, "{k:?}");
        // 中间：2 秒时中心在 500，窗口左上角 250
        let x = knot_value(&k, 2.0, |p| p.x);
        assert!((x - 250.0).abs() < 8.0, "{x}");
        // 竖直方向没有移动：y 恒定 = 中心 (0.4+0.1)×500=250 − 125
        assert!(k.iter().all(|p| (p.y - 125.0).abs() < 1e-6));
        assert_eq!(knot_expr(&k, |p| p.y), "125.00");
        // 速度 2 倍、从素材 1000 毫秒开始：时间线 1 秒 ≈ 素材 3000 毫秒（窗口左上角 = 3000/4000×800... 中心 0.1+0.6=0.7→700−250=450）
        let k2 = focus_knots(&Region { smooth: 0.0, ..r.clone() }, 1000, 2.0, 1000, work, win);
        let x2 = knot_value(&k2, 1.0, |p| p.x);
        assert!((x2 - 450.0).abs() < 1.0, "{x2}");
    }

    fn knot_value(k: &[Knot], t: f64, pick: impl Fn(&Knot) -> f64) -> f64 {
        let i = k.partition_point(|p| p.t <= t).clamp(1, k.len() - 1);
        let (a, b) = (&k[i - 1], &k[i]);
        pick(a) + (pick(b) - pick(a)) * ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0)
    }

    #[test]
    fn a_wandering_path_is_fitted_with_few_knots() {
        // 来回晃动 60 秒、振幅占半个画面宽（比真实的镜头剧烈得多）：折线端点不超过上限，而且和原路径相差不大
        let track: Vec<TrackPt> = (0..=120).map(|i| pt(i * 500, 0.4 + 0.3 * (f64::from(i as u32) * 0.37).sin(), 0.3)).collect();
        let r = Region { track, smooth: 0.3, effect: "focus".into(), ..Default::default() };
        let k = focus_knots(&r, 0, 1.0, 60_000, (1920, 1080), (960, 540));
        assert!(k.len() <= MAX_KNOTS, "{}", k.len());
        assert!(k.len() > 10);
        let exact = focus_knots_dense(&r);
        let worst = (0..600).map(|i| i as f64 * 0.1).map(|t| (knot_value(&k, t, |p| p.x) - exact(t)).abs()).fold(0.0, f64::max);
        assert!(worst < 40.0, "最大偏差 {worst:.1} 像素");
    }

    /// 同样的计算但不精简（每个取样点都保留），用来比对。
    fn focus_knots_dense(r: &Region) -> impl Fn(f64) -> f64 {
        let r = r.clone();
        move |t| {
            // 平滑后的中心：对 ±3σ 的原始中心加权平均
            let sigma = r.smooth;
            let (mut s, mut w) = (0.0, 0.0);
            let mut u = (t - 3.0 * sigma).max(0.0);
            while u <= (t + 3.0 * sigma).min(60.0) {
                let b = interpolate(&r.track, u * 1000.0).unwrap();
                let wt = (-((u - t) * (u - t)) / (2.0 * sigma * sigma)).exp();
                s += (b.x + b.w / 2.0) * 1920.0 * wt;
                w += wt;
                u += 1.0 / 30.0;
            }
            (s / w - 480.0).clamp(0.0, 960.0)
        }
    }

    #[test]
    fn expressions_are_nested_ifs_with_constant_axes_collapsed() {
        let k = vec![Knot { t: 0.0, x: 10.0, y: 5.0 }, Knot { t: 1.0, x: 30.0, y: 5.0 }, Knot { t: 3.0, x: 30.0, y: 5.0 }];
        assert_eq!(knot_expr(&k, |p| p.y), "5.00");
        let e = knot_expr(&k, |p| p.x);
        assert_eq!(e, "if(lt(t,1.000),10.00+(20.0000)*(t-0.000),if(lt(t,3.000),30.00+(0.0000)*(t-1.000),30.00))");
        // 负的斜率写在括号里（ffmpeg 的表达式里 `+-` 不稳妥）
        let down = vec![Knot { t: 0.0, x: 30.0, y: 0.0 }, Knot { t: 2.0, x: 10.0, y: 0.0 }];
        assert_eq!(knot_expr(&down, |p| p.x), "if(lt(t,2.000),30.00+(-10.0000)*(t-0.000),10.00)");
        assert_eq!(knot_expr(&[], |p| p.x), "0");
    }
}
