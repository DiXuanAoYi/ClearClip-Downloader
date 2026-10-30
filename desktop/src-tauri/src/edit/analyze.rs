//! 剪辑的“分析”命令：场景检测、静音检测、波形图、底噪估计、一键重构画幅。
//! 这些命令只读素材、给界面候选结果，不动工程；是否应用由用户确认。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::library_media::parse_scene_times;
use crate::postprocess::{base_args, run_ffmpeg_capture};
use crate::track::cmds::{begin, decoder, detect_at, end, follow_clip, need_ffmpeg, Candidate2, FollowReq, EVT_TRACK};
use crate::track::decode::Decoder;
use crate::track::NRect;
use crate::AppState;

use super::spec::Region;

type St<'a> = State<'a, Arc<AppState>>;

/// 一次最多分析这么长（毫秒）
const MAX_SPAN_MS: u64 = 6 * 60 * 60 * 1000;
/// 离范围两端太近的切点 / 太近的两个切点不要（毫秒）
const MIN_GAP_MS: u64 = 300;

fn range(path: &str, from_ms: u64, to_ms: u64) -> AppResult<(u64, u64)> {
    if !Path::new(path).is_file() {
        return Err(AppError::not_found("找不到素材文件。"));
    }
    if to_ms <= from_ms {
        return Err(AppError::invalid("范围不对：结束要比开始晚。"));
    }
    if to_ms - from_ms > MAX_SPAN_MS {
        return Err(AppError::invalid("一次最多分析 6 小时。"));
    }
    Ok((from_ms, to_ms))
}

fn seek_args(path: &str, from_ms: u64, to_ms: u64) -> Vec<String> {
    let mut a = vec!["-hide_banner".to_string(), "-nostats".into()];
    a.extend([
        "-ss".into(),
        format!("{:.3}", from_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", (to_ms - from_ms) as f64 / 1000.0),
        "-i".into(),
        path.to_string(),
    ]);
    a
}

// ---------------------------------------------------------------- 场景

/// 把检测到的切点（相对范围起点，秒）换成素材时间，去掉贴着两端和彼此太近的。
pub fn tidy_cuts(rel_secs: &[f64], from_ms: u64, to_ms: u64) -> Vec<u64> {
    let mut out: Vec<u64> = vec![];
    for t in rel_secs {
        let ms = from_ms + (t * 1000.0).round().max(0.0) as u64;
        let last = out.last().copied().unwrap_or(from_ms);
        if ms >= last + MIN_GAP_MS && ms + MIN_GAP_MS <= to_ms {
            out.push(ms);
        }
    }
    out
}

/// 检测镜头切换点（素材毫秒）。`threshold` 越小越敏感，0.1–0.9，常用 0.3–0.5。
pub async fn scenes(ffmpeg: &Path, path: &str, from_ms: u64, to_ms: u64, threshold: f64) -> AppResult<Vec<u64>> {
    let (from_ms, to_ms) = range(path, from_ms, to_ms)?;
    let t = if threshold.is_finite() { threshold.clamp(0.1, 0.9) } else { 0.4 };
    let mut a = seek_args(path, from_ms, to_ms);
    a.extend(["-an".into(), "-vf".into(), format!("scale=320:-2,select='gt(scene,{t:.2})',showinfo"), "-f".into(), "null".into(), "-".into()]);
    let (_, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(3600)).await?;
    Ok(tidy_cuts(&parse_scene_times(&err), from_ms, to_ms))
}

// ---------------------------------------------------------------- 静音

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// 解析 `silencedetect` 的输出（相对范围起点，秒）。到结尾还没结束的静音，用 `end_secs` 收尾。
pub fn parse_silences(stderr: &str, end_secs: f64) -> Vec<(f64, f64)> {
    let mut out = vec![];
    let mut open: Option<f64> = None;
    for line in stderr.lines() {
        if !line.contains("silencedetect") {
            continue;
        }
        if let Some(v) = line.split("silence_start:").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse::<f64>().ok()) {
            open = Some(v.max(0.0));
        } else if let Some(v) = line.split("silence_end:").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|v| v.parse::<f64>().ok()) {
            if let Some(s) = open.take() {
                out.push((s, v));
            }
        }
    }
    if let Some(s) = open {
        out.push((s, end_secs));
    }
    out
}

/// 检测静音段（素材毫秒）。`noise_db` 是低于多少算静音（-60…-10），`min_ms` 是至少持续多久。
pub async fn silences(ffmpeg: &Path, path: &str, from_ms: u64, to_ms: u64, noise_db: f64, min_ms: u64) -> AppResult<Vec<Span>> {
    let (from_ms, to_ms) = range(path, from_ms, to_ms)?;
    let db = if noise_db.is_finite() { noise_db.clamp(-70.0, -10.0) } else { -35.0 };
    let min_s = (min_ms.clamp(100, 60_000) as f64) / 1000.0;
    let mut a = seek_args(path, from_ms, to_ms);
    a.extend(["-vn".into(), "-af".into(), format!("silencedetect=noise={db:.0}dB:d={min_s:.2}"), "-f".into(), "null".into(), "-".into()]);
    let (_, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(1800)).await?;
    let span_s = (to_ms - from_ms) as f64 / 1000.0;
    Ok(parse_silences(&err, span_s)
        .into_iter()
        .map(|(s, e)| Span { start_ms: from_ms + (s * 1000.0).round() as u64, end_ms: (from_ms + (e.min(span_s) * 1000.0).round() as u64).min(to_ms) })
        .filter(|s| s.end_ms > s.start_ms)
        .collect())
}

// ---------------------------------------------------------------- 波形

/// 波形图：`<预览目录>/wf-<哈希>.png`，同样的参数直接复用。
pub async fn waveform(ffmpeg: &Path, dir: &Path, path: &str, from_ms: u64, to_ms: u64, width: u32, height: u32) -> AppResult<String> {
    let (from_ms, to_ms) = range(path, from_ms, to_ms)?;
    let (w, h) = (width.clamp(100, 4000), height.clamp(20, 400));
    std::fs::create_dir_all(dir)?;
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let out = dir.join(format!("wf-{}.png", crate::media_cmds::short_hash(&format!("{path}|{mtime}|{from_ms}|{to_ms}|{w}x{h}"))));
    if out.is_file() {
        return Ok(out.to_string_lossy().into_owned());
    }
    let mut a = base_args();
    a.extend([
        "-ss".into(),
        format!("{:.3}", from_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", (to_ms - from_ms) as f64 / 1000.0),
        "-i".into(),
        path.to_string(),
    ]);
    a.extend([
        "-vn".into(),
        "-filter_complex".into(),
        format!("aformat=channel_layouts=mono,showwavespic=s={w}x{h}:colors=0x5b9cff:scale=sqrt"),
        "-frames:v".into(),
        "1".into(),
        out.to_string_lossy().into_owned(),
    ]);
    run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(900)).await.map_err(|e| AppError::msg(format!("生成波形失败：{e}")))?;
    Ok(out.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------- 一键重构画幅

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReframeReq {
    /// 界面给的编号：用来取消、区分进度事件
    pub id: u64,
    pub path: String,
    pub from_ms: u64,
    pub to_ms: u64,
    /// 镜头平滑（秒）
    pub smooth: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReframeResult {
    /// 一个聚焦区域（按成片比例取景），编号为 0，由界面分配
    pub region: Region,
    /// 提示（找不到物体、追丢了……）
    pub note: Option<String>,
    /// 找到了运动物体并追踪；否则是居中取景
    pub tracked: bool,
}

/// 从各取样时刻的候选里挑一个要跟着走的：分数高、靠近画面中心的优先。返回（时间，框）。
pub fn pick_target(samples: &[(u64, Vec<Candidate2>)]) -> Option<(u64, NRect)> {
    let mut best: Option<(f64, u64, NRect)> = None;
    for (t, list) in samples {
        for c in list {
            let (cx, cy) = (c.rect.x + c.rect.w / 2.0, c.rect.y + c.rect.h / 2.0);
            let off = ((cx - 0.5).powi(2) + (cy - 0.5).powi(2)).sqrt();
            let score = c.score * (1.0 - 0.5 * off.min(1.0));
            if score.is_finite() && best.as_ref().map_or(true, |b| score > b.0) {
                best = Some((score, *t, c.rect));
            }
        }
    }
    best.map(|(_, t, r)| (t, r))
}

/// 取样时间点：范围内均匀 5 个（避开两端）。
pub fn sample_times(from_ms: u64, to_ms: u64) -> Vec<u64> {
    let span = to_ms.saturating_sub(from_ms);
    (0..5).map(|i| from_ms + span * (2 * i + 1) / 10).collect()
}

fn focus_region(smooth: f64) -> Region {
    Region { name: "自动取景".into(), effect: "focus".into(), reframe: true, zoom: 1.0, smooth: smooth.clamp(0.0, 3.0), ..Default::default() }
}

/// 居中取景：不动的一个点。
fn centered(from_ms: u64, smooth: f64) -> Region {
    let mut r = focus_region(smooth);
    r.track = vec![crate::track::TrackPt { t_ms: from_ms, x: 0.35, y: 0.35, w: 0.3, h: 0.3, lost: false, pin: true }];
    r
}

/// 一键重构画幅：找出画面里最显眼的运动物体，整段追踪，生成一个“按成片比例取景”的聚焦区域。`progress` 收到 0–100。
pub async fn reframe(dec: &Decoder, req: &ReframeReq, progress: impl Fn(f64) + Clone + Send + 'static, cancel: Arc<AtomicBool>) -> AppResult<ReframeResult> {
    let (from_ms, to_ms) = range(&req.path, req.from_ms, req.to_ms)?;
    let smooth = req.smooth.filter(|v| v.is_finite()).unwrap_or(0.8);
    let mut samples = vec![];
    let times = sample_times(from_ms, to_ms);
    for (i, t) in times.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::msg("canceled"));
        }
        // 单个时刻感知失败（解码不了那一帧）不影响整体
        if let Ok(r) = detect_at(dec, &req.path, *t).await {
            samples.push((*t, r.candidates));
        }
        progress(20.0 * (i + 1) as f64 / times.len() as f64);
    }
    let Some((ref_ms, rect)) = pick_target(&samples) else {
        return Ok(ReframeResult {
            region: centered(from_ms, smooth),
            note: Some("没有感知到明显在动的物体，已居中取景。可以在“区域”页手动框选要跟着走的物体。".into()),
            tracked: false,
        });
    };
    let follow = FollowReq { id: req.id, path: req.path.clone(), from_ms, to_ms, ref_ms, rect };
    let p2 = progress.clone();
    let tr = follow_clip(dec, &follow, move |p| p2(20.0 + 80.0 * p), cancel).await?;
    let mut region = focus_region(smooth);
    region.track = tr.points;
    Ok(ReframeResult { region, note: tr.note, tracked: true })
}

#[tauri::command]
pub async fn edit_scenes(state: St<'_>, path: String, from_ms: u64, to_ms: u64, threshold: f64) -> AppResult<Vec<u64>> {
    let ff = need_ffmpeg(&state)?;
    scenes(&ff, &path, from_ms, to_ms, threshold).await
}

#[tauri::command]
pub async fn edit_silences(state: St<'_>, path: String, from_ms: u64, to_ms: u64, noise_db: f64, min_ms: u64) -> AppResult<Vec<Span>> {
    let ff = need_ffmpeg(&state)?;
    silences(&ff, &path, from_ms, to_ms, noise_db, min_ms).await
}

/// 找出一段声音的节拍（配乐打节拍标记、让主轨按节拍切换用）。
#[tauri::command]
pub async fn edit_beats(state: St<'_>, path: String, from_ms: u64, to_ms: u64) -> AppResult<super::beats::Beats> {
    let ff = need_ffmpeg(&state)?;
    super::beats::analyze(&ff, &path, from_ms, to_ms).await
}

/// 自动识别绿幕 / 蓝幕：取 `at_ms` 处一帧，找画面四周的大片纯色。识别不出来返回空。
#[tauri::command]
pub async fn edit_detect_key(state: St<'_>, path: String, at_ms: u64) -> AppResult<Option<super::keydetect::KeyGuess>> {
    let ff = need_ffmpeg(&state)?;
    super::keydetect::analyze(&ff, &path, at_ms).await
}

#[tauri::command]
pub async fn edit_waveform(state: St<'_>, path: String, from_ms: u64, to_ms: u64, width: u32, height: u32) -> AppResult<String> {
    let ff = need_ffmpeg(&state)?;
    waveform(&ff, &state.data_dir.join("previews"), &path, from_ms, to_ms, width, height).await
}

/// 测一段声音的底噪电平（dBFS），降噪按它设置；测不出来返回空。
#[tauri::command]
pub async fn edit_noise_floor(state: St<'_>, path: String, from_ms: u64, to_ms: u64) -> AppResult<Option<f64>> {
    let ff = need_ffmpeg(&state)?;
    let (from_ms, to_ms) = range(&path, from_ms, to_ms)?;
    let secs = ((to_ms - from_ms) / 1000).clamp(1, 60) as u32;
    Ok(crate::audiofx::estimate_noise_floor(&ff, Path::new(&path), from_ms as f64 / 1000.0, secs).await)
}

#[tauri::command]
pub async fn edit_auto_reframe(app: AppHandle, state: St<'_>, req: ReframeReq) -> AppResult<ReframeResult> {
    let dec = decoder(&state).await?;
    let id = req.id;
    let cancel = begin(id);
    let emit = move |app: &AppHandle, p: f64| {
        let _ = app.emit(EVT_TRACK, serde_json::json!({ "id": id, "percent": p.round() }));
    };
    let app2 = app.clone();
    let res = reframe(&dec, &req, move |p| emit(&app2, p), cancel).await;
    end(id);
    emit(&app, 100.0);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(x: f64, y: f64, w: f64, h: f64, score: f64) -> Candidate2 {
        Candidate2 { rect: NRect { x, y, w, h }, score }
    }

    #[test]
    fn cuts_are_shifted_and_filtered() {
        // 范围 10 s–30 s：0.1 s 贴着起点、5.0 和 5.1 太近、19.9 贴着终点
        assert_eq!(tidy_cuts(&[0.1, 5.0, 5.1, 12.34, 19.9], 10_000, 30_000), vec![15_000, 22_340]);
        assert!(tidy_cuts(&[], 0, 1000).is_empty());
    }

    #[test]
    fn silences_are_paired_and_the_open_one_is_closed() {
        let err = "[silencedetect @ 0x1] silence_start: 1.5\n[silencedetect @ 0x1] silence_end: 3.25 | silence_duration: 1.75\nnoise\n[silencedetect @ 0x1] silence_start: 8.0\n";
        assert_eq!(parse_silences(err, 10.0), vec![(1.5, 3.25), (8.0, 10.0)]);
        assert!(parse_silences("nothing", 5.0).is_empty());
    }

    #[test]
    fn target_prefers_strong_and_central_candidates() {
        let samples = vec![(1000, vec![cand(0.0, 0.0, 0.1, 0.1, 10.0)]), (2000, vec![cand(0.45, 0.45, 0.1, 0.1, 9.0), cand(0.9, 0.9, 0.1, 0.1, 9.5)])];
        let (t, r) = pick_target(&samples).unwrap();
        assert_eq!(t, 2000);
        assert!((r.x - 0.45).abs() < 1e-9);
        assert!(pick_target(&[(0, vec![])]).is_none());
    }

    #[test]
    fn sample_times_avoid_the_ends() {
        assert_eq!(sample_times(0, 10_000), vec![1000, 3000, 5000, 7000, 9000]);
        assert_eq!(sample_times(2000, 2000), vec![2000; 5]);
    }

    #[test]
    fn centered_region_is_a_focus_with_one_pin() {
        let r = centered(500, 0.8);
        assert!(r.is_focus() && r.reframe);
        assert_eq!(r.track.len(), 1);
        assert_eq!(r.zoom, 1.0);
    }

    #[test]
    fn ranges_are_validated() {
        assert!(range("/definitely/not/here.mp4", 0, 1000).is_err());
        let me = std::env::current_exe().unwrap();
        let p = me.to_string_lossy();
        assert!(range(&p, 1000, 1000).is_err());
        assert!(range(&p, 0, MAX_SPAN_MS + 1).is_err());
        assert_eq!(range(&p, 0, 1000).unwrap(), (0, 1000));
    }
}
