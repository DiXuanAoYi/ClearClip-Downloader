//! 通用网页嗅探：内置解析器和 yt-dlp 都不支持时，从网页里找视频地址
//! （直链、og:video、`<video>` / `<source>` 标签、脚本里的 mp4 / m3u8 地址）。

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use super::{Ctx, DESKTOP_UA};
use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind, Protocol};

static META_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<meta\s[^>]*>"#).unwrap());
static ATTR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)([a-z:_-]+)\s*=\s*("([^"]*)"|'([^']*)')"#).unwrap());
static TAG_SRC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<(?:video|source)\s[^>]*?src\s*=\s*["']([^"']+)["']"#).unwrap());
static MEDIA_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)https?:(?:\\?/){2}(?:[^\s"'<>()\\]|\\/)+?\.(?:mp4|m3u8|webm|m4v|mov)(?:\?[^\s"'<>\\]*)?"#).unwrap());
pub(super) static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#).unwrap());

fn attrs(tag: &str) -> Vec<(String, String)> {
    ATTR_RE.captures_iter(tag).map(|c| (c[1].to_ascii_lowercase(), c.get(3).or(c.get(4)).map(|m| m.as_str()).unwrap_or("").to_string())).collect()
}

/// `<meta property|name="key" content="...">` 的内容。
pub(super) fn meta(html: &str, keys: &[&str]) -> Vec<String> {
    let mut out = vec![];
    for m in META_RE.find_iter(html) {
        let a = attrs(m.as_str());
        let key = a.iter().find(|(k, _)| k == "property" || k == "name").map(|(_, v)| v.to_ascii_lowercase());
        if key.is_some_and(|k| keys.contains(&k.as_str())) {
            if let Some((_, v)) = a.iter().find(|(k, _)| k == "content") {
                out.push(html_unescape(v));
            }
        }
    }
    out
}

pub(super) fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

pub(super) fn ext_of(url: &str) -> Option<&'static str> {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    ["mp4", "m3u8", "webm", "m4v", "mov", "mp3", "m4a", "aac", "ogg", "opus", "flac", "wav", "jpg", "jpeg", "png", "gif", "webp"]
        .into_iter()
        .find(|e| path.ends_with(&format!(".{e}")))
}

const VIDEO_EXTS: &[&str] = &["mp4", "m3u8", "webm", "m4v", "mov"];
const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "aac", "ogg", "opus", "flac", "wav"];

/// 链接本身就是视频 / 音频文件（地址以媒体扩展名结尾，或带有订阅源附带的 `#cc-ext=` 提示）。
pub fn direct_media_ext(url: &str) -> Option<&'static str> {
    if let Some(e) = ext_of(url).filter(|e| VIDEO_EXTS.contains(e) || AUDIO_EXTS.contains(e)) {
        return Some(e);
    }
    let hint = super::rss::fragment_hints(url).remove("cc-ext")?;
    VIDEO_EXTS.iter().chain(AUDIO_EXTS).copied().find(|e| *e == hint)
}

pub fn is_direct_media(url: &str) -> bool {
    direct_media_ext(url).is_some()
}

fn short_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(s.as_bytes())[..5])
}

fn audio_asset(url: &str, ext: &str, referer: &str) -> Asset {
    let mut a = Asset::base("audio", AssetKind::Audio, url.to_string(), format!("音频 {}", ext.to_uppercase()), ext);
    a.headers = vec![("Referer".into(), referer.to_string()), ("User-Agent".into(), DESKTOP_UA.into())];
    a
}

/// 从网页中找出候选视频地址（按出现顺序去重，已转为绝对地址）。
pub fn sniff(html: &str, base: &Url) -> Vec<String> {
    let mut found: Vec<String> = vec![];
    let mut push = |raw: &str| {
        let raw = html_unescape(&raw.replace("\\/", "/").replace("\\u002F", "/"));
        if raw.starts_with("blob:") || raw.starts_with("data:") {
            return;
        }
        if let Ok(u) = base.join(raw.trim()) {
            let s = u.to_string();
            if matches!(u.scheme(), "http" | "https") && !found.contains(&s) {
                found.push(s);
            }
        }
    };
    for v in meta(html, &["og:video:secure_url", "og:video:url", "og:video", "twitter:player:stream"]) {
        push(&v);
    }
    for c in TAG_SRC_RE.captures_iter(html) {
        push(&c[1]);
    }
    for m in MEDIA_URL_RE.find_iter(html) {
        push(m.as_str());
    }
    // 只保留真正的媒体地址（og:video 有时是播放器页面）
    found.retain(|u| matches!(ext_of(u), Some("mp4" | "m3u8" | "webm" | "m4v" | "mov")));
    found
}

pub(super) fn asset_for(i: usize, url: &str, referer: &str) -> Asset {
    let ext = ext_of(url).unwrap_or("mp4");
    let hls = ext == "m3u8";
    let mut a = Asset::base(format!("video-{i}"), AssetKind::Video, url.to_string(), "", if hls { "mp4" } else { ext });
    a.protocol = if hls { Protocol::Hls } else { Protocol::Http };
    a.headers = vec![("Referer".into(), referer.to_string()), ("User-Agent".into(), DESKTOP_UA.into())];
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
    a.label = format!("{} {}（{host}）", if hls { "视频流" } else { "视频" }, if hls { "M3U8".to_string() } else { ext.to_uppercase() });
    a
}

pub async fn resolve(ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
    let base = Url::parse(url).map_err(|_| AppError::invalid("链接格式不正确。"))?;
    let host = base.host_str().unwrap_or("").to_string();
    let site = crate::cookies::registrable_domain(&host);
    let mut info = MediaInfo {
        platform: site.clone(),
        platform_name: if site.is_empty() { "网页".into() } else { site.clone() },
        id: String::new(),
        source_url: url.to_string(),
        title: String::new(),
        author: String::new(),
        cover: None,
        duration_ms: None,
        kind: MediaKind::Video,
        width: None,
        height: None,
        published_at: None,
        assets: vec![],
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("generic".into()),
    };
    let name_from_path = || base.path_segments().and_then(|mut s| s.rfind(|x| !x.is_empty())).map(|s| s.to_string()).unwrap_or_else(|| host.clone());

    // 链接本身就是媒体文件
    if let Some(e) = direct_media_ext(url) {
        let hints = super::rss::fragment_hints(url);
        let mut clean = base.clone();
        clean.set_fragment(None);
        let clean = clean.to_string();
        // 浏览器扩展嗅探到的媒体地址会带上所在网页，作为 Referer（很多 CDN 要求）
        let page_referer = hints.get("cc-referer").filter(|r| Url::parse(r).is_ok()).cloned();
        let referer = page_referer.clone().unwrap_or_else(|| format!("{}://{host}/", base.scheme()));
        info.title = hints.get("cc-title").cloned().unwrap_or_else(|| name_from_path().trim_end_matches(&format!(".{e}")).to_string());
        info.id = if hints.is_empty() {
            info.title.clone()
        } else {
            format!("{}-{}", short_hash(&clean), crate::naming::sanitize(&info.title).chars().take(20).collect::<String>())
        };
        if let Some(a) = hints.get("cc-author") {
            info.author = a.clone();
        }
        info.published_at = hints.get("cc-date").and_then(|d| d.parse().ok());
        if hints.contains_key("cc-pod") {
            // 播客附件：按“播客”归类，避免被当成某个 CDN 站点
            info.platform = "rss".into();
            info.platform_name = "播客".into();
        } else if let Some(page_host) = page_referer.as_deref().and_then(|r| Url::parse(r).ok()).and_then(|u| u.host_str().map(String::from)) {
            // 扩展嗅探到的：按视频所在的网站归类，而不是 CDN
            let site = crate::cookies::registrable_domain(&page_host);
            info.platform = site.clone();
            info.platform_name = site;
        }
        if AUDIO_EXTS.contains(&e) {
            info.kind = MediaKind::Audio;
            info.assets.push(audio_asset(&clean, e, &referer));
        } else {
            info.assets.push(asset_for(0, &clean, &referer));
        }
        if let Some(c) = hints.get("cc-cover") {
            info.cover = Some(c.clone());
            info.assets.push(Asset::cover(c.clone()));
        }
        return Ok(info);
    }

    let mut req = ctx.get(url).header("User-Agent", DESKTOP_UA);
    if let Some(c) = ctx.cookie(url) {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16(), "网页打不开"));
    }
    let ctype = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    let final_url = resp.url().clone();
    if ctype.starts_with("video/") || ctype.contains("mpegurl") {
        info.title = name_from_path();
        info.id = info.title.clone();
        let mut a = asset_for(0, final_url.as_str(), url);
        if ctype.contains("mpegurl") {
            a.protocol = Protocol::Hls;
            a.ext = "mp4".into();
        }
        info.assets.push(a);
        return Ok(info);
    }
    if ctype.contains("xml") || ctype.contains("rss") {
        let body = resp.text().await?;
        let feed = super::rss::parse_feed(&body, &final_url).ok_or_else(|| AppError::unsupported("这个地址不是播客 / RSS 订阅源。"))?;
        return Ok(super::rss::to_media_info(&feed, url, &host));
    }
    if !ctype.is_empty() && !ctype.contains("html") && !ctype.contains("text") {
        return Err(AppError::unsupported("这个链接不是网页也不是视频文件。"));
    }
    let html = resp.text().await?;
    if super::rss::is_feed_body(&html) {
        if let Some(feed) = super::rss::parse_feed(&html, &final_url) {
            return Ok(super::rss::to_media_info(&feed, url, &host));
        }
    }
    ctx.record("generic", "page", url, &html);

    info.title = meta(&html, &["og:title", "twitter:title"])
        .into_iter()
        .next()
        .or_else(|| TITLE_RE.captures(&html).map(|c| html_unescape(c[1].trim())))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(name_from_path);
    info.id = final_url.path().trim_matches('/').replace('/', "_");
    info.cover =
        meta(&html, &["og:image", "og:image:secure_url", "twitter:image"]).into_iter().next().and_then(|c| final_url.join(&c).ok()).map(|u| u.to_string());

    let urls = sniff(&html, &final_url);
    if urls.is_empty() {
        return Err(AppError::unsupported("暂不支持这个网站：网页里没有找到视频地址。可能需要登录，或视频由脚本动态加载。"));
    }
    info.assets = urls.iter().take(12).enumerate().map(|(i, u)| asset_for(i, u, final_url.as_str())).collect();
    if let Some(c) = info.cover.clone() {
        info.assets.push(Asset::cover(c));
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_common_patterns() {
        let html = r#"<html><head><title>Clip &amp; more</title>
            <meta property="og:video" content="https://cdn.example.com/og.mp4?sig=1&amp;t=2">
            <meta property="og:video:url" content="https://www.example.com/player">
            </head><body>
            <video controls src="/media/inline.webm"></video>
            <video><source src='rel/stream.m3u8' type="application/x-mpegURL"></video>
            <video src="blob:https://x/1"></video>
            <script>var p = {"url":"https:\/\/cdn2.example.com\/a\/b.mp4","dup":"https://cdn.example.com/og.mp4?sig=1&t=2"};</script>
            </body></html>"#;
        let base = Url::parse("https://www.example.com/watch/1").unwrap();
        let urls = sniff(html, &base);
        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/og.mp4?sig=1&t=2",
                "https://www.example.com/media/inline.webm",
                "https://www.example.com/watch/rel/stream.m3u8",
                "https://cdn2.example.com/a/b.mp4",
            ]
        );
        let a = asset_for(2, &urls[2], base.as_str());
        assert_eq!(a.protocol, Protocol::Hls);
        assert_eq!(a.ext, "mp4");
    }

    #[tokio::test]
    async fn podcast_attachment_links_resolve_without_network() {
        let client = reqwest::Client::new();
        let settings = crate::settings::Settings::default();
        let store = crate::cookies::CookieStore::in_memory();
        let ctx = Ctx::new(&client, &settings, &store);
        let url = "https://media.example.com/dl?id=9#cc-pod=1&cc-title=%E7%AC%AC1%E6%9C%9F&cc-ext=m4a&cc-author=%E8%80%81%E7%8E%8B&cc-cover=https%3A%2F%2Fc.example.com%2Fx.jpg&cc-date=1759824000";
        assert_eq!(direct_media_ext(url), Some("m4a"));
        let info = resolve(&ctx, url).await.unwrap();
        assert_eq!((info.title.as_str(), info.author.as_str(), info.platform.as_str()), ("第1期", "老王", "rss"));
        assert_eq!(info.kind, MediaKind::Audio);
        assert_eq!(info.published_at, Some(1_759_824_000));
        let audio = info.assets.iter().find(|a| a.kind == AssetKind::Audio).unwrap();
        assert_eq!((audio.ext.as_str(), audio.url.as_str()), ("m4a", "https://media.example.com/dl?id=9"), "the fragment is not part of the download address");
        assert!(info.assets.iter().any(|a| a.kind == AssetKind::Cover));
        // 两个不同的附件不会得到同一个作品 ID
        let other = resolve(&ctx, "https://media.example.com/dl?id=10#cc-title=%E7%AC%AC1%E6%9C%9F&cc-ext=m4a").await.unwrap();
        assert_ne!(info.id, other.id);
        // 普通音频直链
        let plain = resolve(&ctx, "https://x.example.com/a/song.mp3").await.unwrap();
        assert_eq!((plain.title.as_str(), plain.kind), ("song", MediaKind::Audio));
        assert!(!is_direct_media("https://x.example.com/watch?v=1"));
        // 浏览器扩展嗅探到的 m3u8：带网页地址和标题
        let sniffed = "https://cdn.video-site.net/hls/abc/index.m3u8?sig=1#cc-referer=https%3A%2F%2Fwww.video-site.com%2Fplay%2F9&cc-title=%E4%B8%80%E4%B8%AA%E8%A7%86%E9%A2%91";
        let info = resolve(&ctx, sniffed).await.unwrap();
        assert_eq!((info.title.as_str(), info.platform.as_str()), ("一个视频", "video-site.com"));
        let v = &info.assets[0];
        assert_eq!(v.protocol, Protocol::Hls);
        assert_eq!(v.url, "https://cdn.video-site.net/hls/abc/index.m3u8?sig=1");
        assert!(v.headers.iter().any(|(k, val)| k == "Referer" && val == "https://www.video-site.com/play/9"), "{:?}", v.headers);
    }

    #[test]
    fn meta_reads_name_and_property() {
        let html = r#"<meta name="twitter:title" content="T1"><meta content="I" property="og:image">"#;
        assert_eq!(meta(html, &["twitter:title"]), vec!["T1"]);
        assert_eq!(meta(html, &["og:image"]), vec!["I"]);
    }
}
