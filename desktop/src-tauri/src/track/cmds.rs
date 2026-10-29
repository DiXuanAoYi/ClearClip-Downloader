//! 动态区域感知的前端命令：感知运动物体、追踪一个区域、取消。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use super::decode::{track_size, Decoder, FfFrames};
use super::motion::{self, Candidate};
use super::tracker::{self, Stop};
use super::{NRect, TrackPt};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::vidnorm::facts;
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// 追踪进度事件：`{ id, percent }`
pub const EVT_TRACK: &str = "edit://track";
/// 一次最多追踪这么长（毫秒）
const MAX_SPAN_MS: u64 = 30 * 60 * 1000;

fn registry() -> &'static Mutex<HashMap<u64, Arc<AtomicBool>>> {
    static R: OnceLock<Mutex<HashMap<u64, Arc<AtomicBool>>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<u64, Arc<AtomicBool>>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// 登记一个可取消的追踪，返回取消标记。
fn begin(id: u64) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    lock().insert(id, flag.clone());
    flag
}

fn end(id: u64) {
    lock().remove(&id);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub candidates: Vec<Candidate2>,
    /// 镜头在动的程度（相邻帧整体平移占画面宽度的比例）
    pub camera: f64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate2 {
    pub rect: NRect,
    pub score: f64,
}

impl From<Candidate> for Candidate2 {
    fn from(c: Candidate) -> Self {
        Candidate2 { rect: c.rect, score: c.score }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowReq {
    /// 界面给的编号：用来取消、区分进度事件
    pub id: u64,
    pub path: String,
    /// 追踪的时间范围和参照帧（素材里的时间，毫秒）
    pub from_ms: u64,
    pub to_ms: u64,
    pub ref_ms: u64,
    /// 参照帧里要追踪的区域
    pub rect: NRect,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowResult {
    pub points: Vec<TrackPt>,
    /// 追踪不到（丢失）的时间，毫秒
    pub lost_ms: u64,
    pub note: Option<String>,
}

fn need_ffmpeg(state: &AppState) -> AppResult<std::path::PathBuf> {
    crate::postprocess::find_ffmpeg(state).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。"))
}

async fn decoder(state: &AppState) -> AppResult<Decoder> {
    let ff = need_ffmpeg(state)?;
    let caps = state.vcaps.get(&ff).await;
    Ok(Decoder::new(&ff, &caps))
}

struct Video {
    size: (usize, usize),
    frame_ms: f64,
    duration_ms: Option<u64>,
}

async fn probe(ffmpeg: &Path, path: &str) -> AppResult<Video> {
    if !Path::new(path).is_file() {
        return Err(AppError::not_found("找不到素材文件。"));
    }
    let f = facts::read(ffmpeg, Path::new(path)).await.ok_or_else(|| AppError::invalid("无法读取这个文件。"))?;
    let v = f.video.as_ref().ok_or_else(|| AppError::invalid("这个文件里没有画面。"))?;
    let (w, h) = v.display_size();
    if w == 0 || h == 0 {
        return Err(AppError::invalid("读不到画面尺寸。"));
    }
    Ok(Video { size: track_size(w, h), frame_ms: 1000.0 / v.fps.filter(|f| f.is_finite()).unwrap_or(30.0).clamp(5.0, 120.0), duration_ms: f.duration_ms })
}

/// 相邻取样帧的间隔：约 3 个素材帧，限制在 90–200 毫秒。
fn detect_step_ms(frame_ms: f64) -> f64 {
    (frame_ms * 3.0).clamp(90.0, 200.0)
}

/// 感知 `at_ms` 这一帧里正在运动的物体。
pub async fn detect_at(dec: &Decoder, path: &str, at_ms: u64) -> AppResult<DetectResult> {
    let video = probe(&dec.ffmpeg, path).await?;
    let step = detect_step_ms(video.frame_ms);
    // 取 at−2Δ … at+2Δ 附近的所有帧，再按真实时间挑出离 at±Δ、at±2Δ 最近的；开头不够的就只用后面的
    let start = (at_ms as f64 - 2.0 * step - 1.5 * video.frame_ms).max(0.0);
    let span = 4.0 * step + 3.0 * video.frame_ms;
    let frames = dec.window(path, start, span, None, video.size).await?;
    // 界面上暂停时看到的是“时间不晚于 at 的最近一帧”
    let Some(ci) = frames.iter().rposition(|f| f.t_ms <= at_ms as f64 + 1.0).or(if frames.is_empty() { None } else { Some(0) }) else {
        return Err(AppError::invalid("取不到这一时刻的画面。"));
    };
    let tc = frames[ci].t_ms;
    let mut picked: Vec<(i32, usize)> = vec![];
    for o in [-2i32, -1, 1, 2] {
        let target = tc + f64::from(o) * step;
        let best = frames
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != ci && !picked.iter().any(|(_, j)| j == i))
            .min_by(|a, b| (a.1.t_ms - target).abs().total_cmp(&(b.1.t_ms - target).abs()));
        if let Some((i, f)) = best {
            if (f.t_ms - target).abs() <= 0.75 * video.frame_ms {
                picked.push((o, i));
            }
        }
    }
    let res = tokio::task::spawn_blocking(move || {
        let others: Vec<(i32, &super::gray::Gray)> = picked.iter().map(|(o, i)| (*o, &frames[*i].gray)).collect();
        motion::detect(&frames[ci].gray, &others)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?;
    Ok(DetectResult { candidates: res.candidates.into_iter().map(Into::into).collect(), camera: res.camera, note: res.note })
}

/// 追踪用的取样帧率：范围越长越低，避免耗时太久。
fn track_fps(span_ms: u64) -> f64 {
    match span_ms {
        0..=120_000 => 15.0,
        120_001..=400_000 => 10.0,
        _ => 6.0,
    }
}

/// 检查并收紧请求：区域在画面内、时间顺序对、范围不太长。
fn checked(req: &FollowReq, duration_ms: Option<u64>) -> AppResult<FollowReq> {
    let mut r = req.clone();
    let ok = |v: f64| v.is_finite();
    if !(ok(r.rect.x) && ok(r.rect.y) && ok(r.rect.w) && ok(r.rect.h)) || r.rect.w < 0.005 || r.rect.h < 0.005 {
        return Err(AppError::invalid("请先在画面上框出要追踪的区域。"));
    }
    r.rect.x = r.rect.x.clamp(0.0, 0.995);
    r.rect.y = r.rect.y.clamp(0.0, 0.995);
    r.rect.w = r.rect.w.min(1.0 - r.rect.x);
    r.rect.h = r.rect.h.min(1.0 - r.rect.y);
    if let Some(d) = duration_ms {
        r.to_ms = r.to_ms.min(d);
        r.ref_ms = r.ref_ms.min(d);
    }
    if r.from_ms > r.ref_ms {
        r.from_ms = r.ref_ms;
    }
    if r.to_ms < r.ref_ms {
        r.to_ms = r.ref_ms;
    }
    if r.to_ms - r.from_ms > MAX_SPAN_MS {
        return Err(AppError::invalid("一次最多追踪 30 分钟。请缩小追踪的范围。"));
    }
    Ok(r)
}

/// 从参照帧出发向前、向后追踪一个区域，返回整段轨迹。`progress` 收到 0–1。
pub async fn follow_clip(dec: &Decoder, req: &FollowReq, progress: impl Fn(f64) + Send + 'static, cancel: Arc<AtomicBool>) -> AppResult<FollowResult> {
    let video = probe(&dec.ffmpeg, &req.path).await?;
    let req = checked(req, video.duration_ms)?;
    let fps = track_fps(req.to_ms - req.from_ms);
    let (fw, fh) = video.size;
    let rect_px = (req.rect.x * fw as f64, req.rect.y * fh as f64, req.rect.w * fw as f64, req.rect.h * fh as f64);
    let handle = tokio::runtime::Handle::current();
    let (dec, path) = (dec.clone(), req.path.clone());
    let (ref_ms, from_ms, to_ms, frame_ms) = (req.ref_ms, req.from_ms, req.to_ms, video.frame_ms);
    let followed = tokio::task::spawn_blocking(move || {
        let mut frames = FfFrames::new(handle, dec, &path, ref_ms, frame_ms, ref_ms - from_ms, to_ms - ref_ms, fps, (fw, fh));
        tracker::follow(&mut frames, rect_px, (ref_ms - from_ms) as f64, (to_ms - ref_ms) as f64, &mut |p| progress(p), &|| cancel.load(Ordering::Relaxed))
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?;
    let followed = match followed {
        Ok(f) => f,
        Err(Stop::Canceled) => return Err(AppError::msg("canceled")),
        Err(Stop::Failed(m)) => return Err(AppError::invalid(m)),
    };
    // 丢失的时间：每个丢失的采样记它和前一个采样之间的间隔
    let lost_ms: f64 = followed
        .samples
        .iter()
        .enumerate()
        .filter(|(_, s)| s.lost)
        .map(|(i, s)| if i > 0 { s.t_ms - followed.samples[i - 1].t_ms } else { followed.samples.get(1).map_or(0.0, |n| n.t_ms - s.t_ms) })
        .sum();
    let lost_ms = lost_ms.round().max(0.0) as u64;
    let mut points = tracker::finish(&followed.samples, followed.frame, followed.size);
    // 区域大小沿用用户画的（模板有最小尺寸，可能比画的略大）；参照点就是用户画的那个框
    for p in &mut points {
        let (cx, cy) = (p.x + p.w / 2.0, p.y + p.h / 2.0);
        (p.w, p.h) = (req.rect.w, req.rect.h);
        (p.x, p.y) = (cx - p.w / 2.0, cy - p.h / 2.0);
        if p.pin {
            (p.x, p.y) = (req.rect.x, req.rect.y);
        }
    }
    let span = (req.to_ms - req.from_ms).max(1);
    let note = (lost_ms * 100 / span >= 20).then(|| {
        format!("有 {}% 的时间追踪不到这个区域（被挡住、出了画面，或者变化太大）。可以在丢失的位置手动校正框的位置，再从那里重新追踪。", lost_ms * 100 / span)
    });
    Ok(FollowResult { points, lost_ms, note })
}

/// 感知一帧里正在运动的物体，返回候选区域。
#[tauri::command]
pub async fn track_detect(state: St<'_>, path: String, at_ms: u64) -> AppResult<DetectResult> {
    let dec = decoder(&state).await?;
    detect_at(&dec, &path, at_ms).await
}

/// 追踪一个区域。追踪期间界面收到 `edit://track` 事件（`{ id, percent }`）。
#[tauri::command]
pub async fn track_follow(app: AppHandle, state: St<'_>, req: FollowReq) -> AppResult<FollowResult> {
    let dec = decoder(&state).await?;
    let id = req.id;
    let cancel = begin(id);
    let app2 = app.clone();
    let res = follow_clip(
        &dec,
        &req,
        move |p| {
            let _ = app2.emit(EVT_TRACK, serde_json::json!({ "id": id, "percent": (p * 100.0).round() }));
        },
        cancel,
    )
    .await;
    end(id);
    let _ = app.emit(EVT_TRACK, serde_json::json!({ "id": id, "percent": 100 }));
    res
}

/// 取消一次追踪。
#[tauri::command]
pub fn track_cancel(id: u64) {
    if let Some(f) = lock().get(&id) {
        f.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> FollowReq {
        FollowReq { id: 1, path: "/m/a.mp4".into(), from_ms: 1000, to_ms: 9000, ref_ms: 4000, rect: NRect { x: 0.4, y: 0.4, w: 0.2, h: 0.2 } }
    }

    #[test]
    fn requests_are_tightened_or_refused() {
        let r = checked(&req(), Some(6000)).unwrap();
        assert_eq!((r.from_ms, r.ref_ms, r.to_ms), (1000, 4000, 6000), "超出素材长度的收紧到素材结尾");
        // 参照帧不在范围内：范围扩到参照帧
        let mut q = req();
        q.ref_ms = 500;
        let r = checked(&q, None).unwrap();
        assert_eq!((r.from_ms, r.ref_ms), (500, 500));
        q = req();
        q.rect = NRect { x: 0.9, y: 0.95, w: 0.5, h: 0.5 };
        let r = checked(&q, None).unwrap();
        assert!((r.rect.x + r.rect.w - 1.0).abs() < 1e-9 && (r.rect.y + r.rect.h - 1.0).abs() < 1e-9, "{:?}", r.rect);
        q.rect = NRect { x: 0.1, y: 0.1, w: 0.001, h: 0.2 };
        assert!(checked(&q, None).unwrap_err().message.contains("框出"));
        q = req();
        q.rect.w = f64::NAN;
        assert!(checked(&q, None).is_err());
        q = req();
        (q.from_ms, q.to_ms) = (0, MAX_SPAN_MS + 1);
        assert!(checked(&q, None).unwrap_err().message.contains("30 分钟"));
    }

    #[test]
    fn longer_ranges_are_tracked_at_a_lower_rate() {
        assert_eq!((track_fps(60_000), track_fps(300_000), track_fps(900_000)), (15.0, 10.0, 6.0));
    }

    #[test]
    fn sampling_step_follows_the_source_frame_rate() {
        assert_eq!(detect_step_ms(1000.0 / 30.0), 100.0);
        assert_eq!(detect_step_ms(1000.0 / 60.0), 90.0);
        assert_eq!(detect_step_ms(1000.0 / 12.0), 200.0);
    }

    #[test]
    fn cancel_flags_are_per_id() {
        let a = begin(9_001);
        let b = begin(9_002);
        track_cancel(9_001);
        assert!(a.load(Ordering::Relaxed) && !b.load(Ordering::Relaxed));
        end(9_001);
        end(9_002);
        track_cancel(9_001); // 已经结束的：什么也不发生
        assert!(lock().get(&9_001).is_none());
    }
}
