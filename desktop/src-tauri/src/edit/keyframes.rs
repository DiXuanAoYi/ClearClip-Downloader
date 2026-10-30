//! 关键帧：一个属性（位置、缩放、旋转、不透明度、音量）随时间变化的几个“关键点”，中间按缓动曲线补值。
//! 时间都是相对叠加素材 / 文字自己的开始时间（毫秒，按成片时间算，不受素材变速影响）。
//! 前端 `utils/edit.ts` 的 `kfEval` 和这里的 `eval` 要保持一致。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 一个属性的关键点列表，键是属性名（`x`、`y`、`scale`、`rotate`、`opacity`、`volume`）。
pub type KeyframeMap = HashMap<String, Vec<Keyframe>>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Keyframe {
    pub t_ms: u64,
    pub value: f64,
    /// 从这个关键点到下一个关键点之间用的曲线：linear / in / out / inout / hold（保持到下一个点再跳变）
    pub ease: String,
}

impl Default for Keyframe {
    fn default() -> Self {
        Keyframe { t_ms: 0, value: 0.0, ease: "linear".into() }
    }
}

/// 整理一个属性的关键点：去掉不是数的、按时间排序、同一时刻只留最后一个。
pub fn tidy(points: &[Keyframe]) -> Vec<Keyframe> {
    let mut v: Vec<Keyframe> = points.iter().filter(|k| k.value.is_finite()).cloned().collect();
    v.sort_by_key(|k| k.t_ms);
    let mut out: Vec<Keyframe> = Vec::with_capacity(v.len());
    for k in v {
        match out.last_mut() {
            Some(last) if last.t_ms == k.t_ms => *last = k,
            _ => out.push(k),
        }
    }
    out
}

/// 某个属性有没有关键点（有就用关键点，没有就用固定值）。
pub fn track(map: &KeyframeMap, prop: &str) -> Option<Vec<Keyframe>> {
    map.get(prop).map(|p| tidy(p)).filter(|p| !p.is_empty())
}

fn ease(kind: &str, p: f64) -> f64 {
    match kind {
        "in" => p * p,
        "out" => 1.0 - (1.0 - p) * (1.0 - p),
        "inout" => p * p * (3.0 - 2.0 * p),
        "hold" => 0.0,
        _ => p,
    }
}

/// `t_ms` 时刻的值；第一个点之前取第一个点的值，最后一个点之后取最后一个点的值。`points` 要已经整理过。
pub fn eval(points: &[Keyframe], t_ms: f64) -> Option<f64> {
    let first = points.first()?;
    if t_ms <= first.t_ms as f64 {
        return Some(first.value);
    }
    for w in points.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if t_ms < b.t_ms as f64 {
            let p = ((t_ms - a.t_ms as f64) / (b.t_ms - a.t_ms) as f64).clamp(0.0, 1.0);
            return Some(a.value + (b.value - a.value) * ease(&a.ease, p));
        }
    }
    points.last().map(|k| k.value)
}

/// ffmpeg 表达式：`time` 是“距开始多久（秒）”的表达式，例如 `(t-1.500)`。`points` 要已经整理过且不为空。
pub fn expr(points: &[Keyframe], time: &str) -> String {
    let first = &points[0];
    let n = points.len();
    let mut tail = format!("{:.5}", points[n - 1].value);
    for i in (0..n - 1).rev() {
        let (a, b) = (&points[i], &points[i + 1]);
        let (ta, tb) = (a.t_ms as f64 / 1000.0, b.t_ms as f64 / 1000.0);
        let p = format!("clip(({time}-{ta:.3})/{:.3},0,1)", tb - ta);
        let shaped = match a.ease.as_str() {
            "in" => format!("pow({p},2)"),
            "out" => format!("(1-pow(1-{p},2))"),
            "inout" => format!("({p}*{p}*(3-2*{p}))"),
            "hold" => "0".to_string(),
            _ => p,
        };
        tail = format!("if(lt({time},{tb:.3}),{:.5}+({:.5})*{shaped},{tail})", a.value, b.value - a.value);
    }
    format!("if(lt({time},{:.3}),{:.5},{tail})", first.t_ms as f64 / 1000.0, first.value)
}

/// 在 `shift_ms` 处把一个属性的关键点切成前后两半（切开处补一个等值的点，两边曲线才对得上）；后半的时间从 0 重新算。
pub fn split_at(points: &[Keyframe], at_ms: u64) -> (Vec<Keyframe>, Vec<Keyframe>) {
    let pts = tidy(points);
    let Some(v) = eval(&pts, at_ms as f64) else { return (vec![], vec![]) };
    let mut a: Vec<Keyframe> = pts.iter().filter(|k| k.t_ms < at_ms).cloned().collect();
    let prev_ease = pts.iter().rfind(|k| k.t_ms < at_ms).map_or("linear".to_string(), |k| k.ease.clone());
    a.push(Keyframe { t_ms: at_ms, value: v, ease: prev_ease.clone() });
    let mut b: Vec<Keyframe> = vec![Keyframe { t_ms: 0, value: v, ease: prev_ease }];
    b.extend(pts.iter().filter(|k| k.t_ms > at_ms).map(|k| Keyframe { t_ms: k.t_ms - at_ms, ..k.clone() }));
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(t: u64, v: f64, e: &str) -> Keyframe {
        Keyframe { t_ms: t, value: v, ease: e.into() }
    }

    #[test]
    fn eval_interpolates_and_clamps() {
        let p = tidy(&[k(2000, 1.0, "linear"), k(0, 0.0, "linear")]);
        assert_eq!(p[0].t_ms, 0);
        assert_eq!(eval(&p, -5.0), Some(0.0));
        assert_eq!(eval(&p, 1000.0), Some(0.5));
        assert_eq!(eval(&p, 9000.0), Some(1.0));
        assert_eq!(eval(&[], 1.0), None);
    }

    #[test]
    fn easing_shapes() {
        let p = vec![k(0, 0.0, "in"), k(1000, 1.0, "linear")];
        assert!((eval(&p, 500.0).unwrap() - 0.25).abs() < 1e-9);
        let p = vec![k(0, 0.0, "out"), k(1000, 1.0, "linear")];
        assert!((eval(&p, 500.0).unwrap() - 0.75).abs() < 1e-9);
        let p = vec![k(0, 0.0, "hold"), k(1000, 1.0, "linear")];
        assert_eq!(eval(&p, 999.0), Some(0.0));
        assert_eq!(eval(&p, 1000.0), Some(1.0));
    }

    #[test]
    fn tidy_drops_duplicates_and_nan() {
        let p = tidy(&[k(100, 1.0, "linear"), k(100, 2.0, "linear"), k(50, f64::NAN, "linear")]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].value, 2.0);
    }

    #[test]
    fn expr_has_one_branch_per_segment() {
        let p = vec![k(0, 0.0, "linear"), k(1000, 1.0, "inout"), k(2000, 0.5, "hold")];
        let e = expr(&p, "(t-1.000)");
        assert_eq!(e.matches("if(").count(), 3);
        assert!(e.contains("3-2*"));
        assert!(!e.contains('\''));
    }

    #[test]
    fn split_keeps_value_continuous() {
        let p = vec![k(0, 0.0, "linear"), k(2000, 1.0, "linear")];
        let (a, b) = split_at(&p, 500);
        assert_eq!(a.last().unwrap().t_ms, 500);
        assert!((a.last().unwrap().value - 0.25).abs() < 1e-9);
        assert_eq!(b[0].t_ms, 0);
        assert!((eval(&b, 0.0).unwrap() - 0.25).abs() < 1e-9);
        assert!((eval(&b, 1500.0).unwrap() - 1.0).abs() < 1e-9);
    }
}
