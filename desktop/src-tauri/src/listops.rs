//! 解析结果里的“列表”：创作者主页的分栏、嵌套列表展开、按列表名放进各自文件夹入队。

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use url::Url;

use crate::commands::{BatchProgress, EVT_BATCH};
use crate::download::{self, EnqueueExtra};
use crate::model::{AppError, AppResult, MediaInfo, MediaKind, PlaylistEntry, SeriesInfo};
use crate::providers;
use crate::quality;
use crate::settings::{QualityPreset, Settings};
use crate::AppState;

/// 列表里再套列表时最多展开几层，超过就停下并说明。
const MAX_DEPTH: usize = 3;

// ---------- 创作者主页的分栏 ----------

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatorTab {
    pub key: String,
    pub label: String,
    pub url: String,
    /// 这一栏里每一项是个列表（可展开），而不是视频
    pub lists: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreatorTabs {
    pub tabs: Vec<CreatorTab>,
    pub active: String,
}

fn tab(key: &str, label: &str, url: String, lists: bool) -> CreatorTab {
    CreatorTab { key: key.into(), label: label.into(), url, lists }
}

/// 创作者主页有哪几栏。不是创作者主页（单个视频、列表本身）时返回空。
pub fn creator_tabs(url: &str) -> CreatorTabs {
    let Ok(u) = Url::parse(url.trim()) else { return CreatorTabs::default() };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let segs: Vec<String> = u.path_segments().map(|s| s.filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default();
    if host.ends_with("youtube.com") {
        let (base, rest): (Vec<String>, &[String]) = match segs.as_slice() {
            [h, rest @ ..] if h.starts_with('@') => (vec![h.clone()], rest),
            [k, id, rest @ ..] if matches!(k.as_str(), "channel" | "c" | "user") => (vec![k.clone(), id.clone()], rest),
            _ => return CreatorTabs::default(),
        };
        let active = match rest.first().map(String::as_str) {
            Some("shorts") => "shorts",
            Some("playlists") => "playlists",
            _ => "videos",
        };
        let at = |name: &str| format!("https://www.youtube.com/{}/{name}", base.join("/"));
        return CreatorTabs {
            tabs: vec![
                tab("videos", "视频", at("videos"), false),
                tab("shorts", "Shorts", at("shorts"), false),
                tab("playlists", "播放列表", at("playlists"), true),
            ],
            active: active.into(),
        };
    }
    if host == "space.bilibili.com" {
        let mut it = segs.iter();
        let Some(mid) = it.next().filter(|m| !m.is_empty() && m.chars().all(|c| c.is_ascii_digit())) else { return CreatorTabs::default() };
        let rest: Vec<&str> = it.map(String::as_str).collect();
        let active = match rest.as_slice() {
            [] | ["upload", ..] | ["video"] => "uploads",
            ["dynamic"] => "dynamic",
            ["lists"] | ["channel"] | ["channel", "series" | "collectiondetail_list"] => "lists",
            _ => return CreatorTabs::default(),
        };
        let at = |p: &str| format!("https://space.bilibili.com/{mid}/{p}");
        return CreatorTabs {
            tabs: vec![
                tab("uploads", "投稿", at("upload/video"), false),
                tab("dynamic", "动态", at("dynamic"), false),
                tab("lists", "合集和系列", at("lists"), true),
            ],
            active: active.into(),
        };
    }
    CreatorTabs::default()
}

// ---------- 展开 ----------

/// 解析一个链接：抖音 / 快手 / 小红书 / 微博的主页和合集走隐藏窗口，其余走内置解析器 / yt-dlp。
pub async fn resolve_any(app: &AppHandle, st: &AppState, settings: &Settings, url: &str) -> AppResult<MediaInfo> {
    if let Some(spec) = crate::weblist::classify(url) {
        let c = crate::weblist::collect(app, &spec, crate::weblist::MAX_ENTRIES).await?;
        return Ok(crate::weblist::to_media_info(&spec, c));
    }
    providers::resolve_url(&st.parse_ctx(settings), url).await
}

/// 同一个列表里的一组视频。
#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    pub author: String,
    pub platform_name: String,
    pub entries: Vec<PlaylistEntry>,
}

/// 把一个列表里的条目摊平：是视频的留下，是列表的（最多 3 层）展开成各自的一组。
/// 返回（各组, 说明）。说明里写没能展开的列表及原因，调用方原样展示。
pub async fn flatten(
    app: &AppHandle,
    st: &AppState,
    settings: &Settings,
    top: &MediaInfo,
    picked: Vec<PlaylistEntry>,
    only: &std::collections::HashMap<String, Vec<String>>,
) -> (Vec<Group>, Vec<String>) {
    let mut notes = Vec::new();
    let mut groups = Vec::new();
    let plain: Vec<PlaylistEntry> = picked.iter().filter(|e| !e.is_list).cloned().collect();
    if !plain.is_empty() {
        groups.push(Group { name: top.title.clone(), author: top.author.clone(), platform_name: top.platform_name.clone(), entries: plain });
    }
    // 广度优先：(列表, 层数, 上一级的作者)
    let mut queue: Vec<(PlaylistEntry, usize, String)> = picked.into_iter().filter(|e| e.is_list).map(|e| (e, 1, top.author.clone())).collect();
    let mut seen_lists = HashSet::new();
    while !queue.is_empty() {
        let (e, depth, parent_author) = queue.remove(0);
        if !seen_lists.insert(e.url.clone()) {
            continue;
        }
        if depth > MAX_DEPTH {
            notes.push(format!("{}：列表套得太深（超过 {MAX_DEPTH} 层），没有再展开", e.title));
            continue;
        }
        match resolve_any(app, st, settings, &e.url).await {
            Ok(sub) if sub.kind == MediaKind::Playlist => {
                let author = if sub.author.is_empty() { parent_author.clone() } else { sub.author.clone() };
                let name = if sub.title.is_empty() { e.title.clone() } else { sub.title.clone() };
                let (lists, mut videos): (Vec<_>, Vec<_>) = sub.entries.into_iter().partition(|x| x.is_list);
                if let Some(ids) = only.get(&e.id) {
                    videos.retain(|v| ids.contains(&v.id));
                }
                if videos.is_empty() && lists.is_empty() {
                    notes.push(format!("{}：这个列表里现在没有视频", e.title));
                }
                if !videos.is_empty() {
                    groups.push(Group { name, author: author.clone(), platform_name: sub.platform_name.clone(), entries: videos });
                }
                queue.extend(lists.into_iter().map(|l| (l, depth + 1, author.clone())));
            }
            Ok(_) => notes.push(format!("{}：不是列表", e.title)),
            Err(err) => notes.push(format!("{}：{err}", e.title)),
        }
    }
    (groups, notes)
}

// ---------- 入队选项 ----------

/// 这一批的下载选项。字段为 None 表示按设置里的。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListOptions {
    /// best | 1080 | 720 | audio
    pub quality: Option<String>,
    /// off | file | embed | burn
    pub subtitle: Option<String>,
    /// 只要音频时转成的格式
    pub audio_format: Option<String>,
    /// 按列表放进各自的文件夹（默认开）
    pub folder: Option<bool>,
    /// platform_author_list | author_list | list
    pub layout: Option<String>,
    /// 文件名前面带列表里的序号
    pub numbered: Option<bool>,
    /// 命名模板（没有序号时用；空表示用设置里的）
    pub template: Option<String>,
    /// 一次最多加入多少个视频（0 或没有 = 不限）
    pub limit: Option<usize>,
    /// 展开的列表里只下载其中几个：列表项 id → 视频 id（没有的列表整个下载）
    pub only: std::collections::HashMap<String, Vec<String>>,
}

pub fn apply_options(settings: &Settings, o: &ListOptions) -> Settings {
    let mut s = settings.clone();
    s.quality_preset = match o.quality.as_deref() {
        Some("best") => QualityPreset::Best,
        Some("1080") => QualityPreset::Max1080,
        Some("720") => QualityPreset::Small,
        Some("audio") => QualityPreset::Audio,
        _ => s.quality_preset,
    };
    if let Some(m) = o.subtitle.as_deref().filter(|m| matches!(*m, "off" | "file" | "embed" | "burn")) {
        s.subtitle_mode = m.to_string();
    }
    if let Some(f) = o.audio_format.as_deref().filter(|f| matches!(*f, "mp3" | "m4a" | "opus" | "flac")) {
        s.audio_format = f.to_string();
    }
    s
}

/// 这一组视频放进哪个文件夹（相对下载目录，一层一层）。
pub fn folder_parts(layout: &str, platform: &str, author: &str, list: &str) -> Vec<String> {
    use crate::naming::sanitize;
    let list = list.trim();
    let author = author.trim();
    // 列表名里已经带了作者名（如“彼德 - Videos”）就不再多套一层
    let show_author = !author.is_empty() && !list.contains(author);
    let mut parts = Vec::new();
    match layout {
        "list" => {}
        "author_list" => {
            if show_author {
                parts.push(sanitize(author));
            }
        }
        _ => {
            if !platform.is_empty() {
                parts.push(sanitize(platform));
            }
            if show_author {
                parts.push(sanitize(author));
            }
        }
    }
    parts.push(sanitize(if list.is_empty() { "未命名列表" } else { list }));
    parts
}

fn seq_width(total: usize) -> usize {
    total.to_string().len().max(2)
}

/// 勾选的条目（视频或列表）逐个解析并加入队列：列表自动展开，每个列表放进自己的文件夹。
pub async fn enqueue(app: AppHandle, st: std::sync::Arc<AppState>, playlist: MediaInfo, picked: Vec<PlaylistEntry>, opts: ListOptions) {
    let settings = apply_options(&st.settings(), &opts);
    let n = picked.len();
    let mut p = BatchProgress { title: playlist.title.clone(), done: 0, total: n, queued: 0, skipped: 0, failed: vec![], finished: false, lists: 0 };
    let _ = app.emit(EVT_BATCH, p.clone());

    let (groups, notes) = flatten(&app, &st, &settings, &playlist, picked, &opts.only).await;
    p.failed.extend(notes);
    let mut seen: HashSet<String> = HashSet::new();
    let limit = opts.limit.filter(|l| *l > 0).unwrap_or(usize::MAX);
    let mut groups: Vec<Group> = groups
        .into_iter()
        .map(|mut g| {
            // 同一个视频出现在两个列表里只下一次
            g.entries.retain(|e| seen.insert(e.id.clone()));
            g
        })
        .filter(|g| !g.entries.is_empty())
        .collect();
    let mut left = limit;
    for g in &mut groups {
        g.entries.truncate(left);
        left -= g.entries.len();
    }
    p.lists = groups.len();
    p.total = groups.iter().map(|g| g.entries.len()).sum::<usize>() + p.failed.len();
    let _ = app.emit(EVT_BATCH, p.clone());

    let folder = opts.folder.unwrap_or(true);
    let layout = opts.layout.clone().unwrap_or_else(|| "platform_author_list".into());
    let numbered = opts.numbered.unwrap_or(true);
    for g in groups {
        let width = seq_width(g.entries.iter().map(|e| e.index as usize).max().unwrap_or(0));
        let dir: Option<PathBuf> = folder.then(|| {
            let mut d = settings.download_root();
            d.extend(folder_parts(&layout, &g.platform_name, &g.author, &g.name));
            d
        });
        for e in g.entries {
            let ctx = st.parse_ctx(&settings);
            match providers::resolve_url(&ctx, &e.url).await {
                Ok(mut info) if info.kind != MediaKind::Playlist => {
                    if info.series.is_none() {
                        info.series = Some(SeriesInfo { name: g.name.clone(), season: None, episode: Some(e.index) });
                    }
                    let (ids, post) = quality::auto_selection(&info, &settings);
                    let extra = EnqueueExtra {
                        dir: dir.clone(),
                        template: dir.is_some().then(|| match (numbered, opts.template.as_deref().map(str::trim).filter(|t| !t.is_empty())) {
                            (true, _) => "{index} - {title}".to_string(),
                            (false, Some(t)) => t.to_string(),
                            (false, None) => settings.filename_template.clone(),
                        }),
                        seq_label: Some(format!("{:0width$}", e.index)),
                        origin: Some("list".into()),
                        list: Some(g.name.clone()),
                        ..Default::default()
                    };
                    match download::enqueue_ext(&app, info, &ids, post, extra) {
                        Ok(r) => {
                            p.queued += r.tasks.len();
                            p.skipped += r.already_downloaded + r.already_queued;
                        }
                        Err(err) => p.failed.push(format!("{}：{err}", e.title)),
                    }
                }
                Ok(_) => p.failed.push(format!("{}：这一项是个列表，没有展开成视频", e.title)),
                Err(err) => p.failed.push(format!("{}：{err}", e.title)),
            }
            p.done += 1;
            let _ = app.emit(EVT_BATCH, p.clone());
        }
    }
    p.done = p.total;
    p.finished = true;
    let _ = app.emit(EVT_BATCH, p);
}

pub fn check_picked(picked: &[PlaylistEntry]) -> AppResult<()> {
    if picked.is_empty() {
        return Err(AppError::invalid("请至少选择一个条目。"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_tabs() {
        let t = creator_tabs("https://www.youtube.com/@abc");
        assert_eq!(t.active, "videos");
        assert_eq!(
            t.tabs.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(),
            vec!["https://www.youtube.com/@abc/videos", "https://www.youtube.com/@abc/shorts", "https://www.youtube.com/@abc/playlists"]
        );
        assert!(t.tabs[2].lists && !t.tabs[0].lists);
        assert_eq!(creator_tabs("https://www.youtube.com/@abc/playlists").active, "playlists");
        assert_eq!(creator_tabs("https://www.youtube.com/channel/UC1/shorts").active, "shorts");
        assert!(creator_tabs("https://www.youtube.com/playlist?list=PL1").tabs.is_empty());
        assert!(creator_tabs("https://www.youtube.com/watch?v=x").tabs.is_empty());
    }

    #[test]
    fn bilibili_tabs() {
        let t = creator_tabs("https://space.bilibili.com/123");
        assert_eq!(t.active, "uploads");
        assert_eq!(t.tabs.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), vec!["投稿", "动态", "合集和系列"]);
        assert_eq!(creator_tabs("https://space.bilibili.com/123/lists").active, "lists");
        assert_eq!(creator_tabs("https://space.bilibili.com/123/dynamic").active, "dynamic");
        assert!(creator_tabs("https://space.bilibili.com/123/lists/45?type=season").tabs.is_empty());
        assert!(creator_tabs("https://space.bilibili.com/123/favlist?fid=9").tabs.is_empty());
        assert!(creator_tabs("https://www.bilibili.com/video/BV1x").tabs.is_empty());
    }

    #[test]
    fn folder_layouts() {
        assert_eq!(folder_parts("platform_author_list", "YouTube", "彼德", "AI新聞"), vec!["YouTube", "彼德", "AI新聞"]);
        assert_eq!(folder_parts("author_list", "YouTube", "彼德", "AI新聞"), vec!["彼德", "AI新聞"]);
        assert_eq!(folder_parts("list", "YouTube", "彼德", "AI新聞"), vec!["AI新聞"]);
        // 列表名里已经有作者名：不重复
        assert_eq!(folder_parts("platform_author_list", "YouTube", "彼德", "彼德 - Videos"), vec!["YouTube", "彼德 - Videos"]);
        // 列表名里的非法字符被替换
        assert_eq!(folder_parts("list", "", "", "a/b:c"), vec![crate::naming::sanitize("a/b:c")]);
        assert_eq!(folder_parts("list", "", "", "  "), vec!["未命名列表"]);
    }

    #[test]
    fn options_override_settings() {
        let base = Settings::default();
        let o = ListOptions { quality: Some("720".into()), subtitle: Some("file".into()), audio_format: Some("flac".into()), ..Default::default() };
        let s = apply_options(&base, &o);
        assert_eq!(s.quality_preset, QualityPreset::Small);
        assert_eq!(s.subtitle_mode, "file");
        assert_eq!(s.audio_format, "flac");
        // 没选的保持设置里的
        let same = apply_options(&base, &ListOptions::default());
        assert_eq!(same.quality_preset, base.quality_preset);
        assert_eq!(same.subtitle_mode, base.subtitle_mode);
        // 乱填的值被忽略
        assert_eq!(apply_options(&base, &ListOptions { subtitle: Some("x".into()), ..Default::default() }).subtitle_mode, base.subtitle_mode);
    }
}
