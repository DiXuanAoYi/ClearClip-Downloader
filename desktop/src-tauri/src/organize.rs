//! 整理：把标题、作者、封面写入文件；保存作品信息 JSON；生成 Jellyfin / Plex 能识别的 NFO。

use std::path::{Path, PathBuf};

use crate::model::{Asset, MediaInfo};
use crate::postprocess::{find_ffmpeg, muxer_for, run_ffmpeg};
use crate::settings::Settings;
use crate::AppState;

/// 能写入元数据的格式。
pub fn supports_metadata(ext: &str) -> bool {
    matches!(ext, "mp4" | "m4a" | "mov" | "mp3" | "mkv" | "webm")
}

/// 能内嵌封面的格式（mkv / webm 的封面是附件，不处理）。
fn supports_cover(ext: &str) -> bool {
    matches!(ext, "mp4" | "m4a" | "mov" | "mp3")
}

/// 判断图片格式（只用 JPEG / PNG 作封面，兼容性最好）。
pub fn image_kind(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else {
        None
    }
}

/// 下载封面到临时文件；不是 JPEG / PNG 时放弃。
pub async fn fetch_cover(st: &AppState, settings: &Settings, media: &MediaInfo, near: &Path) -> Option<PathBuf> {
    let url = media.cover.as_deref()?;
    let client = st.net.clients_for(&settings.network, url).ok()?.api;
    let mut req = client.get(url).header("Referer", crate::providers::referer_for(&media.platform));
    if let Some(c) = st.cookies.header_for(url) {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data = resp.bytes().await.ok()?;
    if data.len() > 10 * 1024 * 1024 {
        return None;
    }
    let ext = image_kind(&data)?;
    let mut p = near.as_os_str().to_owned();
    p.push(format!(".cover.{ext}"));
    let p = PathBuf::from(p);
    std::fs::write(&p, &data).ok()?;
    Some(p)
}

/// ffmpeg 元数据参数。
pub fn metadata_args(media: &MediaInfo) -> Vec<String> {
    let mut args = vec![];
    let mut put = |k: &str, v: &str| {
        let v = v.trim();
        if !v.is_empty() {
            args.push("-metadata".to_string());
            args.push(format!("{k}={v}"));
        }
    };
    put("title", &media.title);
    put("artist", &media.author);
    put("album_artist", &media.author);
    if let Some(s) = &media.series {
        put("album", &s.name);
        if let Some(e) = s.episode {
            put("track", &e.to_string());
        }
    }
    if let Some(d) = media.published_at.and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
        put("date", &d.format("%Y-%m-%d").to_string());
    }
    put("comment", &media.source_url);
    put("publisher", &media.platform_name);
    args
}

/// 把元数据（和封面）写入 `input`，输出到 `output`。`final_path` 决定输出格式。
pub async fn embed(st: &AppState, media: &MediaInfo, input: &Path, output: &Path, final_path: &Path, cover: Option<&Path>) -> crate::error::AppResult<()> {
    let ffmpeg = find_ffmpeg(st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let ext = final_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let cover = cover.filter(|_| supports_cover(&ext));
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(), "-i".into(), input.to_string_lossy().into_owned()];
    if let Some(c) = cover {
        args.extend([
            "-i".into(),
            c.to_string_lossy().into_owned(),
            "-map".into(),
            "0:v?".into(),
            "-map".into(),
            "0:a?".into(),
            "-map".into(),
            "0:s?".into(),
            "-map".into(),
            "1".into(),
        ]);
        args.extend(["-c".into(), "copy".into()]);
        // 封面是最后一路视频流
        let cover_idx = if ext == "mp3" || ext == "m4a" { 0 } else { 1 };
        args.extend([format!("-disposition:v:{cover_idx}"), "attached_pic".into()]);
    } else {
        args.extend(["-map".into(), "0".into(), "-c".into(), "copy".into()]);
    }
    args.extend(metadata_args(media));
    let muxer = muxer_for(final_path);
    if muxer == "mp3" {
        args.extend(["-id3v2_version".into(), "3".into()]);
    }
    if muxer == "mp4" || muxer == "ipod" {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-f".into(), muxer.into(), output.to_string_lossy().into_owned()]);
    run_ffmpeg(&ffmpeg, &args).await
}

/// 作品信息 JSON。
pub fn info_json(media: &MediaInfo, asset: &Asset, file: &Path) -> serde_json::Value {
    serde_json::json!({
        "title": media.title,
        "author": media.author,
        "platform": media.platform_name,
        "id": media.id,
        "url": media.source_url,
        "publishedAt": media.published_at,
        "durationMs": media.duration_ms,
        "cover": media.cover,
        "series": media.series,
        "format": { "id": asset.id, "label": asset.label, "width": asset.width, "height": asset.height, "vcodec": asset.vcodec, "acodec": asset.acodec },
        "file": file.file_name().map(|n| n.to_string_lossy().into_owned()),
        "downloadedBy": "ClearClip",
    })
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Jellyfin / Kodi / Plex 识别的 NFO：剧集用 episodedetails，其他用 movie。
pub fn nfo(media: &MediaInfo) -> String {
    let root = if media.series.is_some() { "episodedetails" } else { "movie" };
    let mut x = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<{root}>\n");
    let mut tag = |k: &str, v: &str| {
        if !v.trim().is_empty() {
            x.push_str(&format!("  <{k}>{}</{k}>\n", xml_escape(v.trim())));
        }
    };
    tag("title", &media.title);
    tag("studio", &media.platform_name);
    tag("director", &media.author);
    tag("credits", &media.author);
    if let Some(s) = &media.series {
        tag("showtitle", &s.name);
        if let Some(n) = s.season {
            tag("season", &n.to_string());
        }
        if let Some(n) = s.episode {
            tag("episode", &n.to_string());
        }
    }
    if let Some(d) = media.published_at.and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
        tag("premiered", &d.format("%Y-%m-%d").to_string());
        tag("aired", &d.format("%Y-%m-%d").to_string());
    }
    if let Some(ms) = media.duration_ms {
        tag("runtime", &(ms / 60_000).max(1).to_string());
    }
    tag("plot", &media.source_url);
    x.push_str(&format!("  <uniqueid type=\"{}\" default=\"true\">{}</uniqueid>\n", xml_escape(&media.platform), xml_escape(&media.id)));
    x.push_str(&format!("</{root}>\n"));
    x
}

/// 在文件旁写入信息 JSON 和 NFO（按设置）。
pub fn write_sidecars(settings: &Settings, media: &MediaInfo, asset: &Asset, file: &Path) {
    if settings.write_info_json {
        let p = file.with_extension("info.json");
        if let Ok(s) = serde_json::to_string_pretty(&info_json(media, asset, file)) {
            let _ = std::fs::write(p, s);
        }
    }
    if settings.write_nfo {
        let _ = std::fs::write(file.with_extension("nfo"), nfo(media));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AssetKind, MediaKind, SeriesInfo};

    fn media() -> MediaInfo {
        MediaInfo {
            platform: "bilibili".into(),
            platform_name: "B站".into(),
            id: "BV1".into(),
            source_url: "https://b23.tv/x".into(),
            title: "猫 & <狗>".into(),
            author: "UP".into(),
            cover: None,
            duration_ms: Some(125_000),
            kind: MediaKind::Video,
            width: None,
            height: None,
            published_at: Some(1_700_000_000),
            assets: vec![],
            entries: vec![],
            series: Some(SeriesInfo { name: "合集".into(), season: None, episode: Some(3) }),
            chapters: vec![],
            extractor: None,
        }
    }

    #[test]
    fn nfo_and_metadata() {
        let x = nfo(&media());
        assert!(x.contains("<episodedetails>"));
        assert!(x.contains("<title>猫 &amp; &lt;狗&gt;</title>"));
        assert!(x.contains("<episode>3</episode>"));
        assert!(x.contains("<premiered>2023-11-14</premiered>"));
        assert!(x.contains("<runtime>2</runtime>"));
        let args = metadata_args(&media());
        assert!(args.contains(&"artist=UP".to_string()));
        assert!(args.contains(&"track=3".to_string()));
        let a = Asset::base("video", AssetKind::Video, String::new(), "1080P", "mp4");
        let j = info_json(&media(), &a, Path::new("/x/猫.mp4"));
        assert_eq!(j["file"], "猫.mp4");
        assert_eq!(j["series"]["episode"], 3);
    }

    #[test]
    fn image_sniffing() {
        assert_eq!(image_kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_kind(b"\x89PNG\r\n"), Some("png"));
        assert_eq!(image_kind(b"RIFF....WEBP"), None);
        assert!(supports_metadata("mp4") && !supports_metadata("jpg"));
    }
}
