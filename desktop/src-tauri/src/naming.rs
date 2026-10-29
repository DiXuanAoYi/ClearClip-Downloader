//! 根据命名模板生成文件名和保存路径。

use std::path::{Path, PathBuf};

use crate::model::{Asset, AssetKind, MediaInfo};

const MAX_TITLE_CHARS: usize = 60;
const MAX_NAME_CHARS: usize = 100;

/// 渲染模板，变量：{author} {title} {date} {id} {platform}（{quality} 在这里为空，见 `render_base_q`）
pub fn render_base(template: &str, info: &MediaInfo) -> String {
    render_base_q(template, info, "")
}

/// 同 `render_base`，另外带上清晰度标签（如“1080P”），用于模板里的 {quality}。
pub fn render_base_q(template: &str, info: &MediaInfo, quality: &str) -> String {
    let date = info.published_at.and_then(|ts| chrono::DateTime::from_timestamp(ts, 0)).unwrap_or_else(chrono::Utc::now).format("%Y%m%d").to_string();
    let title: String = clean_title(&info.title).chars().take(MAX_TITLE_CHARS).collect();
    let raw = template
        .replace("{author}", &info.author)
        .replace("{title}", &title)
        .replace("{date}", &date)
        .replace("{id}", &info.id)
        .replace("{platform}", &info.platform_name)
        .replace("{quality}", quality);
    let mut name: String = sanitize(&raw).chars().take(MAX_NAME_CHARS).collect();
    name = name.trim_matches(|c: char| c == '_' || c == '.' || c == ' ' || c == '-').to_string();
    if name.is_empty() {
        name = sanitize(&info.id);
    }
    if name.is_empty() {
        name = "clearclip".into();
    }
    avoid_reserved(name)
}

/// 一个下载任务的文件名主体和额外的子文件夹。优先级：调用方指定的模板（订阅、监视文件夹等）→ 剧集命名（作品是剧集且设置了模板）→ “设置 → 下载 → 保存 → 文件命名”。
/// 手动下载、识别后自动下载、手机发送都走这里，所以文件名一致。`quality` 是这个资源的清晰度标签，供模板里的 {quality} 使用。
pub fn plan_base(extra_template: Option<&str>, series_template: &str, filename_template: &str, info: &MediaInfo, quality: &str) -> (Vec<String>, String) {
    match extra_template {
        Some(t) if !t.trim().is_empty() => return (vec![], render_base_q(t, info, quality)),
        _ => {}
    }
    if let Some((dirs, base)) = render_series(series_template, info) {
        return (dirs, base);
    }
    (vec![], render_base_q(filename_template, info, quality))
}

/// 剧集命名：模板如 `{series}/第{episode}集`，`/` 分隔子文件夹。返回（子文件夹列表，文件名主体）；
/// 不是剧集或模板为空时返回 None。额外变量：{series} {season} {episode}。
pub fn render_series(template: &str, info: &MediaInfo) -> Option<(Vec<String>, String)> {
    let series = info.series.as_ref()?;
    if template.trim().is_empty() || series.name.trim().is_empty() {
        return None;
    }
    let ep = series.episode.map(|e| e.to_string()).unwrap_or_else(|| info.id.clone());
    let season = series.season.map(|s| s.to_string()).unwrap_or_default();
    let name: String = clean_title(&series.name).chars().take(MAX_TITLE_CHARS).collect();
    let mut parts: Vec<String> = template
        .split(['/', '\\'])
        .map(|p| p.replace("{series}", &name).replace("{season}", &season).replace("{episode}", &ep))
        .filter(|p| !p.trim().is_empty())
        .map(|p| render_base(&p, info))
        .collect();
    let base = parts.pop()?;
    Some((parts, base))
}

/// Windows 不允许 CON、NUL、COM1 等作为文件名（不论扩展名），在后面加下划线。
pub fn avoid_reserved(name: String) -> String {
    let stem = name.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit() && stem.as_bytes()[3] != b'0');
    if reserved {
        format!("{name}_")
    } else {
        name
    }
}

/// Windows 传统路径上限约 260 字符。完整路径过长时缩短文件名主体（保留后缀）。
pub fn fit_path(dir: &Path, base: &str, asset: &Asset, max_len: usize) -> PathBuf {
    let mut base: String = base.to_string();
    loop {
        let path = dir.join(file_name(&base, asset));
        // 预留 ".part" 和 " (99)" 的长度
        if path.as_os_str().len() + 10 <= max_len || base.chars().count() <= 8 {
            return path;
        }
        let keep = base.chars().count().saturating_sub(4).max(8);
        base = base.chars().take(keep).collect::<String>().trim_end().to_string();
    }
}

/// 资源对应的文件名（不含目录）。
pub fn file_name(base: &str, asset: &Asset) -> String {
    let suffix = match asset.kind {
        AssetKind::Video => String::new(),
        AssetKind::Image => format!("_{:02}", asset.index.unwrap_or(0) + 1),
        AssetKind::Audio => "_music".into(),
        AssetKind::Cover => "_cover".into(),
        AssetKind::Subtitle => format!(".{}", sanitize(asset.quality.as_deref().unwrap_or("sub"))),
    };
    format!("{base}{suffix}.{}", asset.ext)
}

/// 任务文件路径里的文件名（队列、通知里显示的名字，和落盘的名字一致）。路径可能来自别的系统，正反斜杠都当作分隔符。
pub fn display_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

pub fn target_dir(root: &Path, info: &MediaInfo, by_platform: bool) -> PathBuf {
    if by_platform && !info.platform_name.is_empty() {
        root.join(sanitize(&info.platform_name))
    } else {
        root.to_path_buf()
    }
}

/// 如果文件已存在或已被其他任务占用，在文件名后追加 (1)、(2)…
pub fn unique_path(path: PathBuf, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    if !taken(&path) {
        return path;
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().into_owned());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for n in 1.. {
        let name = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = dir.join(name);
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

/// 去掉话题标签和多余空白，让文件名更短。
fn clean_title(title: &str) -> String {
    let no_tags: Vec<&str> = title.split_whitespace().filter(|w| !w.starts_with('#') && !w.starts_with('@')).collect();
    let joined = no_tags.join(" ");
    if joined.trim().is_empty() {
        title.trim().to_string()
    } else {
        joined
    }
}

/// 替换各系统文件名中不允许的字符。
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaKind;

    fn info() -> MediaInfo {
        MediaInfo {
            platform: "douyin".into(),
            platform_name: "抖音".into(),
            id: "742".into(),
            source_url: String::new(),
            title: "秋天第一锅/板栗焖鸡?\n #家常菜 @山野".into(),
            author: "山野厨房".into(),
            cover: None,
            duration_ms: None,
            kind: MediaKind::Video,
            width: None,
            height: None,
            published_at: Some(1_727_856_000),
            assets: vec![],
            entries: vec![],
            series: None,
            chapters: vec![],
            extractor: None,
        }
    }

    #[test]
    fn renders_template_and_sanitizes() {
        assert_eq!(render_base("{author}_{title}_{date}", &info()), "山野厨房_秋天第一锅_板栗焖鸡__20241002");
        assert_eq!(render_base("{platform}-{id}", &info()), "抖音-742");
    }

    #[test]
    fn quality_variable() {
        assert_eq!(render_base_q("{id}_{quality}", &info(), "1080P"), "742_1080P");
        // 没有清晰度（图片、音频等）时变量留空，首尾的分隔符会被去掉
        assert_eq!(render_base_q("{id}_{quality}", &info(), ""), "742");
        assert_eq!(render_base("{id}-{quality}", &info()), "742");
    }

    #[test]
    fn plan_base_precedence() {
        let mut i = info();
        // 只有“文件命名”：手动下载和自动下载一样
        assert_eq!(plan_base(None, "{series}/第{episode}集", "{platform}-{id}-{quality}", &i, "720P"), (vec![], "抖音-742-720P".to_string()));
        // 调用方给了模板，优先于其它
        assert_eq!(plan_base(Some("{id}"), "{series}/第{episode}集", "{platform}", &i, "720P").1, "742");
        // 空白的调用方模板当作没有
        assert_eq!(plan_base(Some("  "), "", "{id}", &i, "").1, "742");
        // 剧集命名只在作品是剧集时生效，并且优先于“文件命名”
        i.series = Some(crate::model::SeriesInfo { name: "山野".into(), season: Some(1), episode: Some(3) });
        let (dirs, base) = plan_base(None, "{series}/第{episode}集", "{id}", &i, "");
        assert_eq!((dirs, base.as_str()), (vec!["山野".to_string()], "第3集"));
        // 没设置剧集模板时回到“文件命名”
        assert_eq!(plan_base(None, "", "{id}", &i, "").1, "742");
    }

    #[test]
    fn display_name_takes_last_component() {
        assert_eq!(display_name("/d/抖音/山野厨房_秋天_20241002.mp4"), "山野厨房_秋天_20241002.mp4");
        assert_eq!(display_name(r"C:\Users\a\Downloads\x (1).mp4"), "x (1).mp4");
        assert_eq!(display_name("plain.mp4"), "plain.mp4");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn empty_result_falls_back_to_id() {
        let mut i = info();
        i.author.clear();
        assert_eq!(render_base("{author}", &i), "742");
    }

    #[test]
    fn file_names_per_asset_kind() {
        assert_eq!(file_name("a", &Asset::video("u".into(), None, None)), "a.mp4");
        assert_eq!(file_name("a", &Asset::image(0, "u.jpeg".into(), None, None)), "a_01.jpg");
        assert_eq!(file_name("a", &Asset::audio("u.mp3".into())), "a_music.mp3");
        assert_eq!(file_name("a", &Asset::cover("u.webp".into())), "a_cover.webp");
    }

    #[test]
    fn reserved_names_are_escaped() {
        assert_eq!(avoid_reserved("CON".into()), "CON_");
        assert_eq!(avoid_reserved("nul.txt".into()), "nul.txt_");
        assert_eq!(avoid_reserved("COM1".into()), "COM1_");
        assert_eq!(avoid_reserved("COM0".into()), "COM0");
        assert_eq!(avoid_reserved("CONSOLE".into()), "CONSOLE");
        let mut i = info();
        i.title = "AUX".into();
        assert_eq!(render_base("{title}", &i), "AUX_");
    }

    #[test]
    fn long_paths_are_shortened() {
        let dir = PathBuf::from("/d".repeat(60));
        let base = "很长的标题".repeat(30);
        let p = fit_path(&dir, &base, &Asset::video("u".into(), None, None), 240);
        assert!(p.as_os_str().len() + 10 <= 240, "{}", p.as_os_str().len());
        assert!(p.to_string_lossy().ends_with(".mp4"));
    }

    #[test]
    fn unique_path_appends_counter() {
        let taken = |p: &Path| p.ends_with("a.mp4") || p.ends_with("a (1).mp4");
        assert_eq!(unique_path(PathBuf::from("/x/a.mp4"), &taken), PathBuf::from("/x/a (2).mp4"));
    }

    #[test]
    fn series_template() {
        let mut i = info();
        assert!(render_series("{series}/第{episode}集", &i).is_none());
        i.series = Some(crate::model::SeriesInfo { name: "山野/日常".into(), season: Some(2), episode: Some(7) });
        let (dirs, base) = render_series("{series}/S{season}/第{episode}集 {title}", &i).unwrap();
        assert_eq!(dirs, vec!["山野_日常", "S2"]);
        assert!(base.starts_with("第7集 秋天第一锅"), "{base}");
        assert!(render_series("", &i).is_none());
    }
}
