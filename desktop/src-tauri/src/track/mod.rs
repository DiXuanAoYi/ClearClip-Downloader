//! 动态区域感知：在一个镜头里找出正在运动的物体、并持续追踪它的位置。
//!
//! - [`gray`]：灰度图、金字塔、积分图
//! - [`motion`]：运动区域感知（补偿镜头平移后的三帧差分）→ 候选区域
//! - [`tracker`]：区域追踪（ZNCC 模板匹配，向前向后，丢失后重新找回）和结果整理
//! - [`decode`]：用 ffmpeg 把视频解成低分辨率灰度帧
//! - [`cmds`]：给界面用的命令（感知、追踪、取消）
//!
//! 位置都用“相对整个画面的比例”（0–1，左上角为原点，方向是素材的显示方向），和分辨率无关。
//! 时间是素材自己的时间（毫秒），所以剪辑里分割、调速、裁剪片段之后轨迹仍然对得上。

use serde::{Deserialize, Serialize};

pub mod cmds;
pub mod decode;
#[cfg(test)]
mod e2e_tests;
pub mod gray;
pub mod motion;
#[cfg(test)]
pub mod synth;
pub mod tracker;

fn is_false(b: &bool) -> bool {
    !*b
}

/// 轨迹上的一个点：这个时刻区域所在的矩形（左上角和尺寸）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackPt {
    /// 素材里的时间（毫秒）
    pub t_ms: u64,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// 追踪器在这里没有把握（物体被挡住、出了画面……），位置是推测的
    #[serde(skip_serializing_if = "is_false")]
    pub lost: bool,
    /// 用户手动指定（或校正）的点：重新追踪时不会被改动
    #[serde(skip_serializing_if = "is_false")]
    pub pin: bool,
}

/// 画面里的一个矩形（比例）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl NRect {
    pub fn area(&self) -> f64 {
        self.w.max(0.0) * self.h.max(0.0)
    }

    pub fn iou(&self, o: &NRect) -> f64 {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = ((self.x + self.w).min(o.x + o.w), (self.y + self.h).min(o.y + o.h));
        let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
        let union = self.area() + o.area() - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }
}

/// 轨迹在 `t_ms`（素材时间）处的矩形：相邻两点之间直线插值，两头停在第一个 / 最后一个点。
/// 前端有一份同样的计算（`src/utils/edit.ts` 的 `trackAt`），改这里要一起改。
pub fn interpolate(pts: &[TrackPt], t_ms: f64) -> Option<NRect> {
    let first = pts.first()?;
    let last = pts.last()?;
    let r = |p: &TrackPt| NRect { x: p.x, y: p.y, w: p.w, h: p.h };
    if t_ms <= first.t_ms as f64 {
        return Some(r(first));
    }
    if t_ms >= last.t_ms as f64 {
        return Some(r(last));
    }
    // 二分找到 t_ms 所在的区间
    let i = pts.partition_point(|p| (p.t_ms as f64) <= t_ms);
    let (a, b) = (&pts[i - 1], &pts[i]);
    let span = b.t_ms as f64 - a.t_ms as f64;
    let f = if span > 0.0 { (t_ms - a.t_ms as f64) / span } else { 0.0 };
    let l = |u: f64, v: f64| u + (v - u) * f;
    Some(NRect { x: l(a.x, b.x), y: l(a.y, b.y), w: l(a.w, b.w), h: l(a.h, b.h) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(t: u64, x: f64) -> TrackPt {
        TrackPt { t_ms: t, x, y: 0.5, w: 0.2, h: 0.1, ..Default::default() }
    }

    #[test]
    fn interpolation_is_linear_and_holds_at_both_ends() {
        let pts = [pt(1000, 0.1), pt(2000, 0.3), pt(4000, 0.7)];
        assert!(interpolate(&[], 5.0).is_none());
        assert_eq!(interpolate(&pts, 0.0).unwrap().x, 0.1);
        assert_eq!(interpolate(&pts, 1000.0).unwrap().x, 0.1);
        assert!((interpolate(&pts, 1500.0).unwrap().x - 0.2).abs() < 1e-12);
        assert!((interpolate(&pts, 3000.0).unwrap().x - 0.5).abs() < 1e-12);
        assert_eq!(interpolate(&pts, 9999.0).unwrap().x, 0.7);
        // 两个点时间相同：不除以零
        let same = [pt(1000, 0.1), pt(1000, 0.9)];
        assert!(interpolate(&same, 1000.0).unwrap().x.is_finite());
    }

    #[test]
    fn iou_of_overlapping_rects() {
        let a = NRect { x: 0.0, y: 0.0, w: 0.5, h: 0.5 };
        assert!((a.iou(&a) - 1.0).abs() < 1e-12);
        assert_eq!(a.iou(&NRect { x: 0.6, y: 0.6, w: 0.2, h: 0.2 }), 0.0);
        let b = NRect { x: 0.25, y: 0.0, w: 0.5, h: 0.5 };
        assert!((a.iou(&b) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn track_points_serialize_compactly() {
        let j = serde_json::to_string(&pt(1500, 0.25)).unwrap();
        assert_eq!(j, r#"{"tMs":1500,"x":0.25,"y":0.5,"w":0.2,"h":0.1}"#);
        let p = TrackPt { lost: true, pin: true, ..pt(1, 0.0) };
        let back: TrackPt = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let min: TrackPt = serde_json::from_str(r#"{"tMs":5}"#).unwrap();
        assert_eq!((min.t_ms, min.lost), (5, false));
    }
}
