//! 剪辑的前端命令：读取素材信息和缩略图、生成预览、保存 / 打开工程文件。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::postprocess::{base_args, run_ffmpeg_capture};
use crate::vidnorm::facts;
use crate::AppState;

use super::fonts;
use super::job;
use super::spec::Project;

type St<'a> = State<'a, Arc<AppState>>;

/// 工程文件的扩展名
pub const PROJECT_EXT: &str = "ccedit";
/// 工程文件最大 20 MB（再大多半不是工程文件）
const MAX_PROJECT_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSource {
    pub path: String,
    pub name: String,
    /// video / audio / image
    pub kind: &'static str,
    pub duration_ms: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
    pub hdr: bool,
    /// 缩略图（视频取开头附近的一帧，图片取缩小的图）
    pub thumb: Option<String>,
    /// 给界面里 `<video>` / `<audio>` 播放用的本机地址
    pub url: String,
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn need_ffmpeg(state: &AppState) -> AppResult<PathBuf> {
    crate::postprocess::find_ffmpeg(state).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试。"))
}

/// 媒体库认识的格式之外，剪辑还接受的扩展名（要和界面 `src/utils/edit.ts` 的 `EDIT_*_EXTS` 保持一致）。
/// 能不能真正解码最后以 ffmpeg 为准，读不出来时 `edit_probe` 会说明原因。
const EXTRA_VIDEO: &[&str] = &["3g2", "m2ts", "mts", "ogv", "vob", "f4v", "asf", "divx", "mxf"];
const EXTRA_IMAGE: &[&str] = &["jfif", "tif", "tiff", "avif", "heif"];
const EXTRA_AUDIO: &[&str] = &["aif", "aiff", "ac3", "mka", "amr"];

/// 只接受视频、音频、图片文件（本机服务不能被用来读取别的文件）。
fn media_kind(p: &Path) -> AppResult<&'static str> {
    if !p.is_file() {
        return Err(AppError::not_found("找不到文件。"));
    }
    let ext = ext_of(p);
    match crate::library::kind_of_ext(&ext) {
        Some(k @ ("video" | "audio" | "image")) => Ok(k),
        _ if EXTRA_VIDEO.contains(&ext.as_str()) => Ok("video"),
        _ if EXTRA_IMAGE.contains(&ext.as_str()) => Ok("image"),
        _ if EXTRA_AUDIO.contains(&ext.as_str()) => Ok("audio"),
        _ => Err(AppError::unsupported("剪辑只能用视频、音频和图片文件。")),
    }
}

fn hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(text.as_bytes())[..8])
}

/// 截一帧缩略图（高度 144）。失败返回 None，界面显示占位。
async fn thumb(ffmpeg: &Path, file: &Path, at_ms: u64, is_image: bool, dir: &Path) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let stamp = std::fs::metadata(file)
        .ok()
        .map(|m| format!("{}-{:?}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs())))
        .unwrap_or_default();
    let out = dir.join(format!("edit-thumb-{}.jpg", hash(&format!("{}|{stamp}|{at_ms}", file.to_string_lossy()))));
    if std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        return Some(out);
    }
    let mut a = base_args();
    if !is_image && at_ms > 0 {
        a.extend(["-ss".into(), format!("{:.3}", at_ms as f64 / 1000.0)]);
    }
    a.extend(["-i".into(), file.to_string_lossy().into_owned()]);
    a.extend(["-frames:v".into(), "1".into(), "-vf".into(), "scale=-2:144".into(), "-q:v".into(), "4".into()]);
    a.push(out.to_string_lossy().into_owned());
    run_ffmpeg_capture(ffmpeg, &a, std::time::Duration::from_secs(30)).await.ok()?;
    std::fs::metadata(&out).ok().filter(|m| m.len() > 0).map(|_| out)
}

/// 给实时画面用的小文件：系统播放器解不了的编码（H.265、ProRes 等）转成 H.264，高度最多 540，同一个素材只做一次。
/// 只用来在监视器里看，导出永远用原素材。返回播放地址。
#[tauri::command]
pub async fn edit_proxy(state: St<'_>, path: String) -> AppResult<String> {
    let p = PathBuf::from(&path);
    if media_kind(&p)? != "video" {
        return Err(AppError::invalid("只有视频需要生成预览用的小文件。"));
    }
    let ff = need_ffmpeg(&state)?;
    let dir = state.data_dir.join("previews");
    std::fs::create_dir_all(&dir)?;
    let stamp = std::fs::metadata(&p)
        .ok()
        .map(|m| format!("{}-{:?}", m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs())))
        .unwrap_or_default();
    let out = dir.join(format!("edit-proxy-{}.mp4", hash(&format!("{}|{stamp}", p.to_string_lossy()))));
    if !std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        let tmp = out.with_extension("part.mp4");
        let mut a = base_args();
        a.extend(["-i".into(), p.to_string_lossy().into_owned()]);
        a.extend(["-map".into(), "0:v:0".into(), "-map".into(), "0:a:0?".into()]);
        a.extend(["-vf".into(), "scale=-2:'min(540,ih)',format=yuv420p".into()]);
        a.extend(["-c:v".into(), "libx264".into(), "-preset".into(), "ultrafast".into(), "-crf".into(), "30".into(), "-g".into(), "15".into()]);
        a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "96k".into(), "-ac".into(), "2".into(), "-movflags".into(), "+faststart".into(), "-y".into()]);
        a.push(tmp.to_string_lossy().into_owned());
        let r = run_ffmpeg_capture(&ff, &a, std::time::Duration::from_secs(3600)).await;
        if let Err(e) = r {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::fs::rename(&tmp, &out)?;
    }
    Ok(state.media.register(&out))
}

/// 读取一个素材：类型、时长、尺寸、有没有声音，缩略图和播放地址。
#[tauri::command]
pub async fn edit_probe(state: St<'_>, path: String) -> AppResult<EditSource> {
    let p = PathBuf::from(&path);
    let by_ext = media_kind(&p)?;
    let ff = need_ffmpeg(&state)?;
    let f = facts::read(&ff, &p).await.ok_or_else(|| {
        if by_ext == "image" && matches!(ext_of(&p).as_str(), "avif" | "heic" | "heif" | "tif" | "tiff") {
            AppError::invalid("无法读取这张图片：当前的 ffmpeg 解不了这种格式。请先转成 PNG 或 JPG 再用。")
        } else {
            AppError::invalid("无法读取这个文件。")
        }
    })?;
    let (has_video, has_audio) = (f.video.is_some(), f.audio.is_some());
    // 按内容修正扩展名的判断：GIF 有时长就是动图（当视频），否则是图片；mp3 里的封面图不算画面
    let kind = match by_ext {
        "audio" => "audio",
        "image" if ext_of(&p) == "gif" && f.duration_ms.is_some_and(|d| d > 200) => "video",
        "image" => "image",
        _ if !has_video && has_audio => "audio",
        _ if !has_video => return Err(AppError::invalid("这个文件里没有可用的画面或声音。")),
        _ => "video",
    };
    if kind == "audio" && !has_audio {
        return Err(AppError::invalid("这个文件里没有声音。"));
    }
    let v = f.video.as_ref().filter(|_| kind != "audio");
    let (width, height) = v.map_or((0, 0), |v| v.display_size());
    let thumb = if kind == "audio" {
        None
    } else {
        let at = f.duration_ms.map_or(0, |d| (d / 10).min(1000));
        thumb(&ff, &p, if kind == "image" { 0 } else { at }, kind == "image", &state.data_dir.join("previews")).await
    };
    let url = state.media.register(&p.canonicalize().unwrap_or_else(|_| p.clone()));
    Ok(EditSource {
        name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path,
        kind,
        duration_ms: if kind == "image" { None } else { f.duration_ms },
        width,
        height,
        fps: v.and_then(|v| v.fps),
        has_video: v.is_some(),
        has_audio,
        hdr: v.is_some_and(|v| v.hdr != facts::Hdr::None),
        thumb: thumb.map(|t| t.to_string_lossy().into_owned()),
        url,
    })
}

/// 取视频里某一时刻的缩略图（时间线上每个片段显示它的开头）。
#[tauri::command]
pub async fn edit_thumb(state: St<'_>, path: String, at_ms: u64) -> AppResult<Option<String>> {
    let p = PathBuf::from(&path);
    let kind = media_kind(&p)?;
    if kind == "audio" {
        return Ok(None);
    }
    let ff = need_ffmpeg(&state)?;
    // 时间取到 0.1 秒，同一个位置不重复截
    let at = at_ms / 100 * 100;
    Ok(thumb(&ff, &p, at, kind == "image", &state.data_dir.join("previews")).await.map(|t| t.to_string_lossy().into_owned()))
}

/// 预览文件的播放地址（只限应用生成的预览）。
#[tauri::command]
pub fn edit_preview_url(state: St<'_>, path: String) -> AppResult<String> {
    let p = PathBuf::from(&path).canonicalize().map_err(|_| AppError::not_found("预览文件已经不存在。"))?;
    let root = state.data_dir.join("previews").canonicalize().map_err(|_| AppError::not_found("预览文件已经不存在。"))?;
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !p.starts_with(&root) || !name.starts_with("edit-prev-") {
        return Err(AppError::invalid("只能播放应用生成的预览。"));
    }
    Ok(state.media.register(&p))
}

/// 生成预览（后台任务，和其他工具箱任务一样出现在任务列表里）。返回任务编号。
#[tauri::command]
pub fn edit_preview_start(app: AppHandle, project: Project) -> AppResult<u64> {
    let project = project.checked().map_err(AppError::invalid)?;
    let title = if project.title.trim().is_empty() { "剪辑预览".to_string() } else { format!("{} 预览", project.title.trim()) };
    Ok(crate::media_tools::spawn_job(&app, title, "剪辑预览", move |ctx| async move { job::run_preview(&ctx, &project).await }))
}

fn check_project_path(path: &str) -> AppResult<PathBuf> {
    let p = PathBuf::from(path);
    if ext_of(&p) != PROJECT_EXT {
        return Err(AppError::invalid(format!("工程文件的扩展名是 .{PROJECT_EXT}。")));
    }
    Ok(p)
}

/// 保存工程文件（.ccedit，JSON）。
#[tauri::command]
pub fn edit_save(path: String, project: Project) -> AppResult<()> {
    let p = check_project_path(&path)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("ccedit.part");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&project)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedProject {
    pub project: Project,
    /// 工程里用到、但现在找不到的文件
    pub missing: Vec<String>,
}

/// 打开工程文件。
#[tauri::command]
pub fn edit_open(path: String) -> AppResult<OpenedProject> {
    let p = check_project_path(&path)?;
    let meta = std::fs::metadata(&p).map_err(|_| AppError::not_found("找不到工程文件。"))?;
    if meta.len() > MAX_PROJECT_BYTES {
        return Err(AppError::invalid("这个文件太大了，不像是剪辑工程。"));
    }
    let project: Project = serde_json::from_slice(&std::fs::read(&p)?).map_err(|e| AppError::invalid(format!("工程文件已损坏或不是剪辑工程：{e}")))?;
    let missing = project.media_paths().into_iter().filter(|m| !Path::new(m).is_file()).collect();
    Ok(OpenedProject { project, missing })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// 字幕文件最大 5 MB
const MAX_SUBTITLE_BYTES: u64 = 5 * 1024 * 1024;

/// 读取 SRT / VTT / ASS / SSA 字幕文件，按时间返回每一条字幕。
#[tauri::command]
pub fn edit_subtitle_cues(path: String) -> AppResult<Vec<SubtitleCue>> {
    use crate::subtitle;
    let p = Path::new(&path);
    let ext = ext_of(p);
    if !matches!(ext.as_str(), "srt" | "vtt" | "ass" | "ssa") {
        return Err(AppError::unsupported("只支持 SRT、VTT、ASS、SSA 字幕文件。"));
    }
    let meta = std::fs::metadata(p).map_err(|_| AppError::not_found("找不到字幕文件。"))?;
    if meta.len() > MAX_SUBTITLE_BYTES {
        return Err(AppError::invalid("这个字幕文件太大了。"));
    }
    let text = String::from_utf8(std::fs::read(p)?).map_err(|_| AppError::invalid("字幕文件不是 UTF-8 编码，请先用文本编辑器另存为 UTF-8。"))?;
    let cues = match ext.as_str() {
        "srt" => subtitle::parse_srt(&text),
        "vtt" => subtitle::parse_vtt(&text),
        _ => subtitle::parse_ass(&text),
    };
    Ok(cues.into_iter().map(|c| SubtitleCue { start_ms: c.start, end_ms: c.end, text: c.lines.join("\n") }).collect())
}

/// 检查这些文件还在不在，返回找不到的。
#[tauri::command]
pub fn edit_missing(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().filter(|m| !Path::new(m).is_file()).collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFonts {
    /// 默认字体（系统里找到的中文字体）
    pub default: Option<String>,
}

#[tauri::command]
pub async fn edit_fonts() -> EditFonts {
    let default = tokio::task::spawn_blocking(fonts::default_font).await.ok().flatten();
    EditFonts { default: default.map(|p| p.to_string_lossy().into_owned()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-editcmd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn project_files_round_trip_and_report_missing_media() {
        let d = dir("proj");
        let real = d.join("a.mp4");
        std::fs::write(&real, "x").unwrap();
        let mut p = Project { title: "旅行".into(), ..Default::default() };
        p.clips = vec![
            super::super::spec::Clip { path: real.to_string_lossy().into_owned(), out_ms: 3000, ..Default::default() },
            super::super::spec::Clip { path: d.join("gone.mp4").to_string_lossy().into_owned(), out_ms: 2000, ..Default::default() },
        ];
        let file = d.join("trip.ccedit");
        edit_save(file.to_string_lossy().into_owned(), p.clone()).unwrap();
        assert!(!d.join("trip.ccedit.part").exists());
        let back = edit_open(file.to_string_lossy().into_owned()).unwrap();
        assert_eq!(back.project, p);
        assert_eq!(back.missing, vec![d.join("gone.mp4").to_string_lossy().into_owned()]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_ccedit_files_can_be_saved_or_opened() {
        let d = dir("ext");
        assert!(edit_save(d.join("x.json").to_string_lossy().into_owned(), Project::default()).is_err());
        assert!(edit_open(d.join("x.txt").to_string_lossy().into_owned()).is_err());
        assert!(edit_open(d.join("nope.ccedit").to_string_lossy().into_owned()).is_err());
        std::fs::write(d.join("bad.ccedit"), "not json").unwrap();
        assert!(edit_open(d.join("bad.ccedit").to_string_lossy().into_owned()).unwrap_err().message.contains("已损坏"));
        // 新版本多出来的字段、旧版本缺的字段都不影响打开
        std::fs::write(d.join("old.ccedit"), r#"{"clips":[{"path":"/a.mp4","outMs":1000,"futureField":1}],"unknown":true}"#).unwrap();
        assert_eq!(edit_open(d.join("old.ccedit").to_string_lossy().into_owned()).unwrap().project.clips.len(), 1);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_media_files_are_accepted() {
        let d = dir("kinds");
        for (n, k) in [
            ("a.mp4", "video"),
            ("a.MP3", "audio"),
            ("a.png", "image"),
            ("a.gif", "image"),
            ("a.m2ts", "video"),
            ("a.Tiff", "image"),
            ("a.avif", "image"),
            ("a.aiff", "audio"),
        ] {
            std::fs::write(d.join(n), "x").unwrap();
            assert_eq!(media_kind(&d.join(n)).unwrap(), k);
        }
        std::fs::write(d.join("secret.txt"), "x").unwrap();
        assert!(media_kind(&d.join("secret.txt")).is_err());
        assert!(media_kind(&d.join("missing.mp4")).is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
