//! 内置播放器：给界面准备播放地址和视频旁边的字幕。
//!
//! 视频本身通过 asset 协议由 `<video>` 直接播放，所以这里只做两件事：
//! 确认这个文件允许播放（在媒体库里，或在下载目录里，避免被拿来读任意文件），
//! 并把它放进 asset 协议的访问范围；再找出同名的字幕文件，解析成条目。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::{library, subtitle, AppState};

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTrack {
    pub label: String,
    pub path: String,
    pub cues: Vec<PlayerCue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSource {
    pub path: String,
    /// 播放地址（本机媒体服务）
    pub url: String,
    pub title: String,
    /// video / audio
    pub kind: String,
    pub ext: String,
    pub tracks: Vec<PlayerTrack>,
}

const SUB_EXTS: [&str; 4] = ["srt", "vtt", "ass", "ssa"];
/// 一条字幕轨最多载入这么多条，避免界面卡顿
const MAX_CUES: usize = 20_000;

/// 视频同一文件夹里、文件名以视频名开头的字幕文件。
pub fn find_subtitles(video: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().map(|s| s.to_string_lossy().into_owned())) else { return vec![] };
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            SUB_EXTS.contains(&ext.as_str()) && p.file_stem().is_some_and(|s| s.to_string_lossy().starts_with(&stem))
        })
        .collect();
    out.sort();
    out
}

/// 字幕轨的名字：取文件名里视频名之后的部分（通常是语言代码），尽量显示成语言名。
pub fn track_label(video: &Path, sub: &Path) -> String {
    let stem = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let sub_stem = sub.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let rest = sub_stem.strip_prefix(&stem).unwrap_or("").trim_matches(|c: char| c == '.' || c == '_' || c == '-' || c == ' ');
    if rest.is_empty() {
        return "字幕".to_string();
    }
    let name = subtitle::lang_name(rest);
    if name.is_empty() || name == rest {
        rest.to_string()
    } else {
        format!("{name}（{rest}）")
    }
}

/// 只允许播放媒体库里的文件，或下载目录里的文件。
pub fn allowed(st: &AppState, path: &Path) -> bool {
    let Ok(canon) = path.canonicalize() else { return false };
    if let Ok(root) = st.settings().download_root().canonicalize() {
        if canon.starts_with(&root) {
            return true;
        }
    }
    st.db.path_known(&path.to_string_lossy()) || st.db.path_known(&canon.to_string_lossy())
}

pub fn load_track(video: &Path, sub: &Path) -> Option<PlayerTrack> {
    let cues = library::read_cues(sub)?;
    if cues.is_empty() {
        return None;
    }
    let cues = cues.into_iter().take(MAX_CUES).map(|c| PlayerCue { start_ms: c.start, end_ms: c.end, text: c.lines.join("\n") }).collect();
    Some(PlayerTrack { label: track_label(video, sub), path: sub.to_string_lossy().into_owned(), cues })
}

#[tauri::command]
pub async fn player_source(state: St<'_>, path: String) -> AppResult<PlayerSource> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(AppError::not_found("文件已不存在"));
    }
    if !allowed(&state, &p) {
        return Err(AppError::invalid("只能播放媒体库或下载目录里的文件。"));
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let kind = match library::kind_of_ext(&ext) {
        Some(k @ ("video" | "audio")) => k.to_string(),
        _ => return Err(AppError::unsupported("播放器只能播放视频和音频文件。")),
    };
    let url = state.media.register(&p.canonicalize().unwrap_or_else(|_| p.clone()));
    let video = p.clone();
    let tracks = tokio::task::spawn_blocking(move || find_subtitles(&video).iter().filter_map(|s| load_track(&video, s)).collect::<Vec<_>>())
        .await
        .map_err(|e| AppError::msg(e.to_string()))?;
    let title = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(PlayerSource { path, url, title, kind, ext, tracks })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("clearclip-player-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn finds_sidecar_subtitles_and_names_them() {
        let d = tmp();
        let v = d.join("演讲.mp4");
        for n in ["演讲.mp4", "演讲.zh-Hans.srt", "演讲.en.vtt", "演讲.srt", "别的视频.srt", "演讲.txt", "演讲.danmaku.xml"] {
            std::fs::write(d.join(n), "x").unwrap();
        }
        let subs = find_subtitles(&v);
        let names: Vec<String> = subs.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["演讲.en.vtt", "演讲.srt", "演讲.zh-Hans.srt"]);
        assert_eq!(track_label(&v, &d.join("演讲.srt")), "字幕");
        assert!(track_label(&v, &d.join("演讲.en.vtt")).contains("en"));
        assert!(track_label(&v, &d.join("演讲.zh-Hans.srt")).contains("zh-Hans"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn tracks_load_cues_and_skip_empty_files() {
        let d = tmp();
        let v = d.join("a.mp4");
        std::fs::write(d.join("a.srt"), "1\n00:00:01,000 --> 00:00:02,500\n第一句\n\n2\n00:00:03,000 --> 00:00:04,000\nsecond\nline\n").unwrap();
        std::fs::write(d.join("a.en.srt"), "").unwrap();
        let t = load_track(&v, &d.join("a.srt")).unwrap();
        assert_eq!(t.cues.len(), 2);
        assert_eq!((t.cues[0].start_ms, t.cues[0].end_ms, t.cues[0].text.as_str()), (1000, 2500, "第一句"));
        assert_eq!(t.cues[1].text, "second\nline");
        assert!(load_track(&v, &d.join("a.en.srt")).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_known_or_downloaded_files_are_playable() {
        let d = tmp();
        let st_db = crate::db::Db::open_in_memory().unwrap();
        let f = d.join("x.mp4");
        std::fs::write(&f, "x").unwrap();
        assert!(!st_db.path_known(&f.to_string_lossy()));
        st_db
            .record_download(&crate::db::NewDownload {
                platform: "p",
                media_id: "1",
                asset_id: "a",
                title: "t",
                author: "",
                cover: None,
                path: &f.to_string_lossy(),
                size: 1,
                kind: "video",
                source: "manual",
                source_url: "",
                platform_name: "p",
            })
            .unwrap();
        assert!(st_db.path_known(&f.to_string_lossy()));
        let _ = std::fs::remove_dir_all(d);
    }
}
