//! 媒体工具箱和字幕工具的前端命令。

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::media_tools::{self, JobSnap, MediaInfoLite, ToolJob};
use crate::subtitle::DanmakuLayout;
use crate::{postprocess, subtitle_tools, AppState};

type St<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn media_job_start(app: AppHandle, job: ToolJob) -> AppResult<u64> {
    media_tools::start(&app, job)
}

#[tauri::command]
pub fn media_jobs(state: St<'_>) -> Vec<JobSnap> {
    state.media_jobs.snapshot()
}

#[tauri::command]
pub fn media_job_cancel(app: AppHandle, state: St<'_>, id: u64) {
    use tauri::Emitter;
    state.media_jobs.cancel(id);
    let _ = app.emit(media_tools::EVT_JOBS, state.media_jobs.snapshot());
}

#[tauri::command]
pub fn media_jobs_clear(app: AppHandle, state: St<'_>) {
    use tauri::Emitter;
    state.media_jobs.clear_finished();
    let _ = app.emit(media_tools::EVT_JOBS, state.media_jobs.snapshot());
}

/// 读取文件的时长、画面尺寸和标签。
#[tauri::command]
pub async fn media_info(state: St<'_>, path: String) -> AppResult<MediaInfoLite> {
    let ff = postprocess::find_ffmpeg(&state).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”中安装 ffmpeg 后重试。"))?;
    if !Path::new(&path).is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let p = postprocess::probe(&ff, Path::new(&path)).await;
    if let Some(e) = &p.error {
        return Err(AppError::invalid(format!("无法读取这个文件：{e}")));
    }
    Ok(p.into())
}

// ---------- 视频规整 ----------

/// 当前 ffmpeg 能做什么、做不到什么。没有安装 ffmpeg 时返回 None。
#[tauri::command]
pub async fn video_caps(state: St<'_>) -> AppResult<Option<crate::vidcaps::CapsSummary>> {
    Ok(crate::vidcaps::current(&state).await.map(|(_, c)| c.summary()))
}

#[tauri::command]
pub fn video_presets() -> Vec<crate::vidnorm::spec::Preset> {
    crate::vidnorm::spec::presets()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoReport {
    pub facts: crate::vidnorm::Facts,
    pub analysis: crate::vidnorm::Analysis,
    pub issues: Vec<crate::vidnorm::spec::Issue>,
    pub recommended: &'static str,
}

/// 检查一个视频：编码、尺寸、帧率、色彩、HDR，以及需要解码才能知道的可变帧率和黑边；给出问题清单和推荐预设。
/// `hint`：来源提示（`live` 直播录制 / `download` 平台下载）。
#[tauri::command]
pub async fn video_analyze(state: St<'_>, path: String, hint: Option<String>) -> AppResult<VideoReport> {
    use crate::vidnorm::{analyze, facts, spec};
    let ff = postprocess::find_ffmpeg(&state).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。"))?;
    let file = Path::new(&path);
    if !file.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let f = facts::read(&ff, file).await.ok_or_else(|| AppError::invalid("无法读取这个文件。"))?;
    if f.video.is_none() {
        return Err(AppError::invalid("这个文件里没有视频画面。"));
    }
    let an = analyze::analyze_quick(&ff, file, &f).await;
    let recommended = spec::recommend(&f, &an, hint.as_deref());
    let issues = spec::issues(&f, &an);
    Ok(VideoReport { facts: f, analysis: an, issues, recommended })
}

/// 检查一个视频里各个镜头的色彩是否一致：哪些镜头和整体的偏色、亮度、对比度、饱和度不一样。要解码一遍关键帧，几秒到几十秒。
#[tauri::command]
pub async fn video_color(
    state: St<'_>,
    path: String,
    skip: Option<Vec<crate::vidnorm::colormatch::SkipSpan>>,
) -> AppResult<crate::vidnorm::colormatch::ColorReport> {
    use crate::vidnorm::{colormatch, facts};
    let (ff, caps) =
        crate::vidcaps::current(&state).await.ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。"))?;
    let file = Path::new(&path);
    if !file.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let f = facts::read(&ff, file).await.ok_or_else(|| AppError::invalid("无法读取这个文件。"))?;
    let skip = skip.unwrap_or_default();
    let a = colormatch::analyze_cached(&ff, file, &f, &caps, &skip).await.map_err(AppError::invalid)?;
    Ok(a.report())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPreview {
    pub before: String,
    pub after: String,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    /// 视频画面是否会被改动（没有改动时“处理后”和“处理前”相同）
    pub changed: bool,
}

pub(crate) fn short_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(text.as_bytes())[..8])
}

/// 截取某一时刻“处理前 / 处理后”的两张图，用来对比规整的效果（只含画面处理，不含帧率和声音）。
#[tauri::command]
pub async fn video_preview(state: St<'_>, path: String, spec: crate::vidnorm::NormSpec, at_ms: u64) -> AppResult<VideoPreview> {
    use crate::vidnorm::{analyze, build, facts};
    let (ff, caps) =
        crate::vidcaps::current(&state).await.ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。"))?;
    let file = Path::new(&path);
    if !file.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let spec = spec.checked().map_err(AppError::invalid)?;
    let f = facts::read(&ff, file).await.ok_or_else(|| AppError::invalid("无法读取这个文件。"))?;
    let mut an = crate::vidnorm::Analysis::default();
    // 精简版 ffmpeg 没有 cropdetect：跳过检测并在预览的提示里说明，而不是悄悄不去黑边
    let crop_warning = if spec.autocrop { caps.autocrop_warning() } else { None };
    if spec.autocrop && crop_warning.is_none() {
        an.crop = analyze::detect_crop(&ff, file, &f).await;
    }
    let mut color_warning = None;
    // 降噪看的是前后几帧：预览从“时间点往前一小段”开始读，再取时间点那一帧，这样画面是降噪稳定之后的样子
    let warm_ms = if spec.denoise == "off" { 0 } else { at_ms.min(500) };
    let start_ms = at_ms - warm_ms;
    if spec.match_color > 0.0 {
        // 预览从 start_ms 处开始读，滤镜看到的时间要减去它
        let analysis = crate::vidnorm::colormatch::analyze_cached(&ff, file, &f, &caps, &spec.match_skip).await;
        let plan = match &analysis {
            Ok(a) => a.plan(spec.match_color, &spec.match_tone, &spec.match_shots),
            Err(_) => crate::vidnorm::colormatch::tone_plan(&spec.match_tone, &spec.match_skip),
        };
        if let Err(e) = &analysis {
            color_warning = Some(format!("分段色彩匹配没有分析镜头之间的色彩：{e}"));
        }
        an.color = Some(plan.shifted(start_ms as i64));
    }
    let pix = if caps.encoders.h264 == Some("h264_qsv") { "nv12" } else { "yuv420p" };
    let tmp = state.data_dir.join("tmp").join("preview");
    let vg = build::video_graph(&spec, &f, &an, &caps, &tmp, pix, true)?;
    let dir = state.data_dir.join("previews");
    // 清理两小时前的预览图
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > std::time::Duration::from_secs(7200));
            if old && e.file_name().to_string_lossy().starts_with("norm-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let key = format!("norm-{}", short_hash(&format!("{path}|{at_ms}|{}", serde_json::to_string(&spec).unwrap_or_default())));
    let (before, after) = analyze::preview(&ff, file, start_ms, warm_ms, vg.graph.as_deref(), &dir, &key).await?;
    for t in vg.temp_files {
        let _ = std::fs::remove_file(t);
    }
    Ok(VideoPreview {
        before: before.to_string_lossy().into_owned(),
        after: after.to_string_lossy().into_owned(),
        changed: vg.graph.is_some(),
        notes: vg.notes,
        warnings: vg.warnings.into_iter().chain(color_warning).chain(crop_warning).collect(),
    })
}

// ---------- 音效 / 混响 ----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioFxPreview {
    pub before: String,
    pub after: String,
    pub notes: Vec<String>,
}

/// 试听：取 `start_ms` 起的 15 秒，输出“处理前 / 处理后”两个 WAV（在预览目录里，两小时后清理）。
#[tauri::command]
pub async fn audio_fx_preview(state: St<'_>, path: String, spec: crate::audiofx::AudioFxSpec, start_ms: u64) -> AppResult<AudioFxPreview> {
    use crate::audiofx;
    let (ff, caps) =
        crate::vidcaps::current(&state).await.ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。"))?;
    let file = Path::new(&path);
    if !file.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let spec = spec.checked()?;
    let dir = state.data_dir.join("previews");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > std::time::Duration::from_secs(7200));
            if old && e.file_name().to_string_lossy().starts_with("fx-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut fx = audiofx::FxCaps::from_caps(&caps, Some(audiofx::rnn_model_path(&state.data_dir)));
    if spec.denoise != "off" && fx.rnn_model.is_none() {
        fx.noise_floor_db = audiofx::estimate_noise_floor(&ff, file, start_ms as f64 / 1000.0, 15).await;
    }
    let key = short_hash(&format!("{path}|{start_ms}|{}", serde_json::to_string(&spec).unwrap_or_default()));
    let before = dir.join(format!("fx-{}-a.wav", short_hash(&format!("{path}|{start_ms}"))));
    let after = dir.join(format!("fx-{key}-b.wav"));
    let timeout = std::time::Duration::from_secs(120);
    let mut notes = vec![];
    for (out, with) in [(&before, None), (&after, Some((&spec, &fx)))] {
        if out.is_file() && with.is_none() {
            continue;
        }
        let (mut args, n) = audiofx::preview_args(file, start_ms, 15, with, out)?;
        notes.extend(n);
        let mut full = postprocess::base_args();
        full.append(&mut args);
        postprocess::run_ffmpeg_capture(&ff, &full, timeout).await.map_err(|e| AppError::msg(format!("生成试听失败：{e}")))?;
    }
    Ok(AudioFxPreview { before: before.to_string_lossy().into_owned(), after: after.to_string_lossy().into_owned(), notes })
}

// ---------- 字幕工具 ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SubOp {
    Shift {
        offset_ms: i64,
    },
    Rescale {
        factor: f64,
    },
    Align {
        a_from: u64,
        a_to: u64,
        b_from: u64,
        b_to: u64,
    },
    /// 双语合并：`second` 是另一个字幕文件，文字放在下面
    Merge {
        second: String,
    },
    Clean {
        drop_hearing: bool,
    },
    Convert,
    /// B站弹幕 XML 转 ASS（使用“设置”里的弹幕样式）。宽高留空按 16:9
    Danmaku {
        width: Option<u32>,
        height: Option<u32>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubJob {
    pub input: String,
    #[serde(flatten)]
    pub op: SubOp,
    /// 输出格式 srt / vtt / ass，默认 srt（输入是 vtt 时为 vtt）
    pub format: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubResult {
    pub output: String,
    pub count: usize,
}

fn run_sub(job: &SubJob, danmaku: &crate::settings::DanmakuStyle) -> AppResult<SubResult> {
    let input = Path::new(&job.input);
    if !input.is_file() {
        return Err(AppError::not_found("找不到字幕文件。"));
    }
    let in_ext = input.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if let SubOp::Danmaku { width, height } = &job.op {
        if in_ext != "xml" {
            return Err(AppError::invalid("请选择 B站弹幕的 XML 文件。"));
        }
        let xml = std::fs::read_to_string(input)?;
        let layout = DanmakuLayout::for_video(*width, *height).styled(danmaku);
        let ass = crate::subtitle::danmaku_xml_to_ass_font(&xml, layout, None, &danmaku.font)?;
        let out = subtitle_tools::output_path(input, "弹幕", "ass");
        std::fs::write(&out, ass)?;
        return Ok(SubResult { output: out.to_string_lossy().into_owned(), count: 0 });
    }
    let cues = subtitle_tools::read(input)?;
    let (cues, tag) = match &job.op {
        SubOp::Shift { offset_ms } => (subtitle_tools::shift(&cues, *offset_ms), "平移"),
        SubOp::Rescale { factor } => (subtitle_tools::rescale(&cues, *factor), "缩放"),
        SubOp::Align { a_from, a_to, b_from, b_to } => (subtitle_tools::align(&cues, *a_from, *a_to, *b_from, *b_to)?, "校准"),
        SubOp::Merge { second } => (subtitle_tools::merge_bilingual(&cues, &subtitle_tools::read(Path::new(second))?), "双语"),
        SubOp::Clean { drop_hearing } => (subtitle_tools::clean(&cues, *drop_hearing), "清理"),
        SubOp::Convert => (cues, "转换"),
        SubOp::Danmaku { .. } => unreachable!(),
    };
    if cues.is_empty() {
        return Err(AppError::invalid("处理后没有剩下任何字幕。"));
    }
    let fmt = job.format.clone().filter(|f| !f.is_empty()).unwrap_or_else(|| if in_ext == "vtt" { "vtt".into() } else { "srt".into() });
    let text = subtitle_tools::write_as(&cues, &fmt)?;
    let out = subtitle_tools::output_path(input, tag, &fmt);
    std::fs::write(&out, text)?;
    Ok(SubResult { output: out.to_string_lossy().into_owned(), count: cues.len() })
}

#[tauri::command]
pub async fn subtitle_tool(state: St<'_>, job: SubJob) -> AppResult<SubResult> {
    let style = state.settings().danmaku;
    tokio::task::spawn_blocking(move || run_sub(&job, &style)).await.map_err(|e| AppError::msg(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-mc-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn job(json: &str) -> SubJob {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn subtitle_jobs_write_new_files_next_to_the_input() {
        let dir = temp("sub");
        let zh = dir.join("课.zh.srt");
        let en = dir.join("课.en.srt");
        std::fs::write(&zh, "1\n00:00:01,000 --> 00:00:03,000\n你好\n\n2\n00:00:05,000 --> 00:00:06,000\n再见\n\n").unwrap();
        std::fs::write(&en, "1\n00:00:01,200 --> 00:00:03,100\nHello\n\n").unwrap();
        let style = crate::settings::DanmakuStyle::default();
        let zs = zh.to_string_lossy().replace('\\', "\\\\");
        let es = en.to_string_lossy().replace('\\', "\\\\");
        let r = run_sub(&job(&format!(r#"{{"input":"{zs}","op":"shift","offsetMs":-500}}"#)), &style).unwrap();
        assert!(r.output.ends_with("课.zh.平移.srt"), "{}", r.output);
        let shifted = std::fs::read_to_string(&r.output).unwrap();
        assert!(shifted.contains("00:00:00,500 --> 00:00:02,500"), "{shifted}");
        let r = run_sub(&job(&format!(r#"{{"input":"{zs}","op":"merge","second":"{es}","format":"ass"}}"#)), &style).unwrap();
        let ass = std::fs::read_to_string(&r.output).unwrap();
        assert!(ass.contains("你好\\NHello"), "{ass}");
        assert_eq!(r.count, 2);
        let r = run_sub(&job(&format!(r#"{{"input":"{zs}","op":"convert","format":"vtt"}}"#)), &style).unwrap();
        assert!(std::fs::read_to_string(&r.output).unwrap().starts_with("WEBVTT"));
        // 弹幕
        let xml = dir.join("d.xml");
        std::fs::write(&xml, r#"<i><d p="1,1,25,16777215,0,0,0,0">hi</d></i>"#).unwrap();
        let xs = xml.to_string_lossy().replace('\\', "\\\\");
        let r = run_sub(
            &job(&format!(r#"{{"input":"{xs}","op":"danmaku","width":1080,"height":1920}}"#)),
            &crate::settings::DanmakuStyle { font_size: 50, ..style.clone() },
        )
        .unwrap();
        let ass = std::fs::read_to_string(&r.output).unwrap();
        assert!(ass.contains("PlayResX: 608") && ass.contains("Danmaku,Microsoft YaHei,50"), "{ass}");
        // 错误情况
        assert!(run_sub(&job(&format!(r#"{{"input":"{zs}","op":"danmaku"}}"#)), &style).is_err(), "not an xml");
        assert!(run_sub(&job(r#"{"input":"/nope.srt","op":"convert"}"#), &style).is_err());
        assert!(run_sub(&job(&format!(r#"{{"input":"{zs}","op":"shift","offsetMs":-99999999}}"#)), &style).is_err(), "nothing left");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
