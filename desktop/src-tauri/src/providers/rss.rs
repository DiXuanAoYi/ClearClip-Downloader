//! 播客 / RSS / Atom：把订阅源解析成列表（每期一条，下载音频附件），也可以直接订阅追更。

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use super::listing::{ListResult, SubEntry};
use super::Ctx;
use crate::model::{AppError, AppResult, ErrorKind, MediaInfo, MediaKind, PlaylistEntry};

static ITEM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<(item|entry)[\s>].*?</(?:item|entry)>").unwrap());
static ENCLOSURE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<(?:enclosure|media:content)\s[^>]*>"#).unwrap());
static ATOM_ENCLOSURE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)<link\s[^>]*rel\s*=\s*["']enclosure["'][^>]*>"#).unwrap());
static ATTR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)([a-z:_-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap());

/// 订阅源里一期的内容。
#[derive(Debug, Clone, PartialEq)]
pub struct FeedEntry {
    pub id: String,
    pub title: String,
    /// 音频 / 视频附件地址
    pub url: String,
    /// 附件的扩展名（从地址或类型推断）
    pub ext: String,
    pub published_at: Option<i64>,
    pub duration_ms: Option<u64>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    pub title: String,
    pub author: String,
    pub image: Option<String>,
    pub entries: Vec<FeedEntry>,
}

/// 地址看起来像订阅源（用来决定要不要先试一下 RSS）。
pub fn looks_like_feed_url(url: &str) -> bool {
    let Ok(u) = Url::parse(url) else { return false };
    let path = u.path().to_ascii_lowercase();
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    path.ends_with(".xml")
        || path.ends_with(".rss")
        || path.ends_with(".atom")
        || path.contains("/feed")
        || path.contains("/rss")
        || host.starts_with("feeds.")
        || host.starts_with("rss.")
        || path.contains("/podcast")
}

/// 内容是不是订阅源。
pub fn is_feed_body(body: &str) -> bool {
    let head: String = body.chars().take(600).collect::<String>().to_ascii_lowercase();
    head.contains("<rss") || head.contains("<feed") || head.contains("<rdf:rdf")
}

fn unescape(s: &str) -> String {
    let mut t = s.trim().to_string();
    if let Some(inner) = t.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")) {
        return inner.trim().to_string();
    }
    // 里面可能夹着 CDATA
    while let (Some(a), Some(b)) = (t.find("<![CDATA["), t.find("]]>")) {
        if b < a {
            break;
        }
        let inner = t[a + 9..b].to_string();
        t.replace_range(a..b + 3, &inner);
    }
    t.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// `<name ...>text</name>` 的文字（取第一个）。
fn tag_text(block: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?is)<{}(?:\s[^>]*)?>(.*?)</{}>", regex::escape(name), regex::escape(name))).ok()?;
    re.captures(block).map(|c| unescape(&c[1])).filter(|s| !s.is_empty())
}

fn attrs(tag: &str) -> Vec<(String, String)> {
    ATTR_RE.captures_iter(tag).map(|c| (c[1].to_ascii_lowercase(), unescape(c.get(2).or(c.get(3)).map(|m| m.as_str()).unwrap_or("")))).collect()
}

fn attr_of(tag: &str, name: &str) -> Option<String> {
    attrs(tag).into_iter().find(|(k, _)| k == name).map(|(_, v)| v).filter(|v| !v.is_empty())
}

/// 时长：`3600`、`59:30`、`01:02:03`。
pub fn parse_duration(s: &str) -> Option<u64> {
    let parts: Vec<f64> = s.trim().split(':').map(|p| p.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [s] => *s,
        [m, s] => m * 60.0 + s,
        [h, m, s] => h * 3600.0 + m * 60.0 + s,
        _ => return None,
    };
    (secs >= 0.0).then_some((secs * 1000.0) as u64)
}

fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    chrono::DateTime::parse_from_rfc2822(s).or_else(|_| chrono::DateTime::parse_from_rfc3339(s)).ok().map(|d| d.timestamp())
}

fn ext_from(url: &str, mime: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    for e in ["mp3", "m4a", "aac", "ogg", "opus", "flac", "wav", "mp4", "m4v", "webm", "mov"] {
        if path.ends_with(&format!(".{e}")) {
            return e.to_string();
        }
    }
    let m = mime.to_ascii_lowercase();
    match m.as_str() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => "m4a",
        "audio/aac" => "aac",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/flac" => "flac",
        "audio/wav" | "audio/x-wav" => "wav",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ if m.starts_with("audio/") => "mp3",
        _ if m.starts_with("video/") => "mp4",
        _ => "mp3",
    }
    .to_string()
}

fn entry_from(block: &str, base: &Url, fallback_image: &Option<String>) -> Option<FeedEntry> {
    let enclosure = ENCLOSURE_RE.find_iter(block).chain(ATOM_ENCLOSURE_RE.find_iter(block)).find_map(|m| {
        let tag = m.as_str();
        let url = attr_of(tag, "url").or_else(|| attr_of(tag, "href"))?;
        let mime = attr_of(tag, "type").unwrap_or_default();
        let medium = attr_of(tag, "medium").unwrap_or_default();
        // 只要音频和视频附件（有的订阅源把图片也放在 enclosure 里）
        let ok = mime.is_empty() && medium.is_empty()
            || mime.starts_with("audio/")
            || mime.starts_with("video/")
            || matches!(medium.as_str(), "audio" | "video")
            || mime == "application/octet-stream";
        ok.then_some((url, mime))
    })?;
    let url = base.join(&enclosure.0).ok()?.to_string();
    let title = tag_text(block, "title").unwrap_or_else(|| "未命名".into());
    let id = tag_text(block, "guid").or_else(|| tag_text(block, "id")).unwrap_or_else(|| url.clone());
    let published_at = tag_text(block, "pubDate").or_else(|| tag_text(block, "published")).or_else(|| tag_text(block, "updated")).and_then(|d| parse_date(&d));
    let duration_ms = tag_text(block, "itunes:duration").and_then(|d| parse_duration(&d));
    let thumbnail = Regex::new(r#"(?is)<itunes:image\s[^>]*>"#)
        .ok()
        .and_then(|re| re.find(block).and_then(|m| attr_of(m.as_str(), "href")))
        .or_else(|| fallback_image.clone());
    Some(FeedEntry { id, ext: ext_from(&url, &enclosure.1), url, title, published_at, duration_ms, thumbnail })
}

/// 解析 RSS 2.0 / Atom 订阅源。没有任何带音视频附件的条目时返回 None。
pub fn parse_feed(xml: &str, base: &Url) -> Option<Feed> {
    if !is_feed_body(xml) {
        return None;
    }
    // 频道信息在第一个 <item> 之前
    let head_end = ITEM_RE.find(xml).map(|m| m.start()).unwrap_or(xml.len());
    let head = &xml[..head_end];
    let title = tag_text(head, "title").unwrap_or_default();
    let author = tag_text(head, "itunes:author").or_else(|| tag_text(head, "managingEditor")).or_else(|| tag_text(head, "author")).unwrap_or_default();
    let image = Regex::new(r#"(?is)<itunes:image\s[^>]*>"#)
        .ok()
        .and_then(|re| re.find(head).and_then(|m| attr_of(m.as_str(), "href")))
        .or_else(|| Regex::new(r"(?is)<image>.*?<url>(.*?)</url>").ok().and_then(|re| re.captures(head).map(|c| unescape(&c[1]))))
        .and_then(|u| base.join(&u).ok())
        .map(|u| u.to_string());
    let mut entries: Vec<FeedEntry> = ITEM_RE.find_iter(xml).filter_map(|m| entry_from(m.as_str(), base, &image)).collect();
    if entries.is_empty() {
        return None;
    }
    // 去重（同一个 id 只留第一条），按发布时间从新到旧
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.id.clone()));
    if entries.iter().all(|e| e.published_at.is_some()) {
        entries.sort_by_key(|e| std::cmp::Reverse(e.published_at));
    }
    Some(Feed { title, author, image, entries })
}

/// 把附件地址包装成下载时能还原标题、作者的地址（`#cc-…` 片段不会发给服务器）。
pub fn entry_url(e: &FeedEntry, author: &str, feed_title: &str) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let sep = if e.url.contains('#') { '&' } else { '#' };
    let mut out = format!("{}{sep}cc-pod=1&cc-title={}&cc-ext={}", e.url, enc(&e.title), e.ext);
    let who = if author.is_empty() { feed_title } else { author };
    if !who.is_empty() {
        out.push_str(&format!("&cc-author={}", enc(who)));
    }
    if let Some(t) = &e.thumbnail {
        out.push_str(&format!("&cc-cover={}", enc(t)));
    }
    if let Some(p) = e.published_at {
        out.push_str(&format!("&cc-date={p}"));
    }
    out
}

/// 读出 `#cc-…` 片段里的信息。
pub fn fragment_hints(url: &str) -> std::collections::HashMap<String, String> {
    Url::parse(url)
        .ok()
        .and_then(|u| {
            u.fragment().map(|f| {
                url::form_urlencoded::parse(f.as_bytes()).filter(|(k, _)| k.starts_with("cc-")).map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
            })
        })
        .unwrap_or_default()
}

async fn fetch(ctx: &Ctx<'_>, url: &str) -> AppResult<(String, Url)> {
    let mut req =
        ctx.get(url).header("User-Agent", super::DESKTOP_UA).header("Accept", "application/rss+xml, application/atom+xml, application/xml, text/xml, */*");
    if let Some(c) = ctx.cookie(url) {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16(), "订阅源打不开"));
    }
    let final_url = resp.url().clone();
    let bytes = resp.bytes().await?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(AppError::unsupported("订阅源太大。"));
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), final_url))
}

fn not_a_feed() -> AppError {
    AppError::new(ErrorKind::Unsupported, "这个地址不是播客 / RSS 订阅源。")
}

/// 从已经取到的内容解析成可选择的列表。
pub fn to_media_info(feed: &Feed, source_url: &str, host: &str) -> MediaInfo {
    let id: String =
        source_url.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().chars().rev().take(40).collect::<String>().chars().rev().collect();
    MediaInfo {
        platform: "rss".into(),
        platform_name: if feed.title.is_empty() { host.to_string() } else { format!("播客 · {}", feed.title) },
        id,
        source_url: source_url.to_string(),
        title: if feed.title.is_empty() { host.to_string() } else { feed.title.clone() },
        author: feed.author.clone(),
        cover: feed.image.clone(),
        duration_ms: None,
        kind: MediaKind::Playlist,
        width: None,
        height: None,
        published_at: None,
        assets: vec![],
        entries: feed
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| PlaylistEntry {
                id: e.id.clone(),
                title: e.title.clone(),
                url: entry_url(e, &feed.author, &feed.title),
                duration_ms: e.duration_ms,
                thumbnail: e.thumbnail.clone(),
                index: i as u32 + 1,
            })
            .collect(),
        series: None,
        chapters: vec![],
        extractor: Some("rss".into()),
    }
}

pub async fn resolve(ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
    let (body, final_url) = fetch(ctx, url).await?;
    ctx.record("rss", "feed", url, &body);
    let feed = parse_feed(&body, &final_url).ok_or_else(not_a_feed)?;
    Ok(to_media_info(&feed, url, final_url.host_str().unwrap_or("")))
}

/// 订阅用：最新的在前，最多 `limit` 期。
pub async fn list(ctx: &Ctx<'_>, url: &str, limit: usize) -> AppResult<ListResult> {
    let (body, final_url) = fetch(ctx, url).await?;
    let feed = parse_feed(&body, &final_url).ok_or_else(not_a_feed)?;
    let host = final_url.host_str().unwrap_or("").to_string();
    Ok(ListResult {
        title: if feed.title.is_empty() { host } else { feed.title.clone() },
        platform: "rss".into(),
        platform_name: "播客 / RSS".into(),
        avatar: feed.image.clone(),
        entries: feed
            .entries
            .iter()
            .take(limit)
            .map(|e| SubEntry {
                id: e.id.clone(),
                title: e.title.clone(),
                url: entry_url(e, &feed.author, &feed.title),
                thumbnail: e.thumbnail.clone(),
                published_at: e.published_at,
                duration_ms: e.duration_ms,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel>
  <title><![CDATA[科技 & 生活 周刊]]></title>
  <itunes:author>老王</itunes:author>
  <itunes:image href="https://cdn.example.com/cover.jpg"/>
  <item>
    <title>第 2 期：&quot;聊聊 AI&quot;</title>
    <guid isPermaLink="false">ep-2</guid>
    <pubDate>Tue, 07 Oct 2025 08:00:00 +0000</pubDate>
    <itunes:duration>01:02:03</itunes:duration>
    <enclosure url="https://media.example.com/ep2.mp3?token=a&amp;b=1" length="123" type="audio/mpeg"/>
  </item>
  <item>
    <title>第 1 期</title>
    <guid>ep-1</guid>
    <pubDate>Tue, 30 Sep 2025 08:00:00 +0000</pubDate>
    <itunes:duration>2400</itunes:duration>
    <itunes:image href="https://cdn.example.com/ep1.jpg"/>
    <enclosure url="/audio/ep1" type="audio/x-m4a"/>
  </item>
  <item><title>只有文字的一期</title><guid>text</guid></item>
  <item><title>封面当附件</title><guid>img</guid><enclosure url="https://x.example.com/a.jpg" type="image/jpeg"/></item>
</channel></rss>"#;

    #[test]
    fn parses_podcast_feed() {
        let base = Url::parse("https://pod.example.com/feed.xml").unwrap();
        let f = parse_feed(RSS, &base).unwrap();
        assert_eq!((f.title.as_str(), f.author.as_str()), ("科技 & 生活 周刊", "老王"));
        assert_eq!(f.image.as_deref(), Some("https://cdn.example.com/cover.jpg"));
        assert_eq!(f.entries.len(), 2, "text-only and image-only items are skipped");
        let e = &f.entries[0];
        assert_eq!(e.id, "ep-2");
        assert_eq!(e.title, "第 2 期：\"聊聊 AI\"");
        assert_eq!(e.url, "https://media.example.com/ep2.mp3?token=a&b=1");
        assert_eq!(e.ext, "mp3");
        assert_eq!(e.duration_ms, Some(3_723_000));
        assert_eq!(e.published_at, Some(1_759_824_000));
        assert_eq!(e.thumbnail.as_deref(), Some("https://cdn.example.com/cover.jpg"), "falls back to the show cover");
        let e1 = &f.entries[1];
        assert_eq!(e1.url, "https://pod.example.com/audio/ep1", "relative enclosure");
        assert_eq!((e1.ext.as_str(), e1.duration_ms), ("m4a", Some(2_400_000)));
        assert_eq!(e1.thumbnail.as_deref(), Some("https://cdn.example.com/ep1.jpg"));
    }

    #[test]
    fn parses_atom_feed_with_enclosure_links() {
        let atom = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom 播客</title><author><name>Bob</name></author>
          <entry><title>E1</title><id>tag:x,1</id><updated>2025-10-01T10:00:00Z</updated>
            <link rel="alternate" href="https://x.example.com/e1"/>
            <link rel="enclosure" type="audio/mpeg" href="https://x.example.com/e1.mp3"/></entry></feed>"#;
        let f = parse_feed(atom, &Url::parse("https://x.example.com/atom").unwrap()).unwrap();
        assert_eq!(f.title, "Atom 播客");
        assert_eq!(f.entries[0].url, "https://x.example.com/e1.mp3");
        assert_eq!(f.entries[0].published_at, Some(1_759_312_800));
    }

    #[test]
    fn non_feeds_are_rejected() {
        let base = Url::parse("https://x.example.com/").unwrap();
        assert!(parse_feed("<html><body>hello</body></html>", &base).is_none());
        assert!(parse_feed("<rss><channel><title>t</title></channel></rss>", &base).is_none(), "no playable items");
        assert!(is_feed_body("<?xml version=\"1.0\"?><rss version=\"2.0\">") && !is_feed_body("<!doctype html><html>"));
        assert!(
            looks_like_feed_url("https://feeds.example.com/show")
                && looks_like_feed_url("https://x.com/podcast/rss")
                && looks_like_feed_url("https://x.com/a.xml")
        );
        assert!(!looks_like_feed_url("https://www.youtube.com/watch?v=1") && !looks_like_feed_url("not a url"));
    }

    #[test]
    fn entry_urls_carry_title_and_ext_in_the_fragment() {
        let base = Url::parse("https://pod.example.com/feed.xml").unwrap();
        let f = parse_feed(RSS, &base).unwrap();
        let u = entry_url(&f.entries[0], &f.author, &f.title);
        assert!(u.starts_with("https://media.example.com/ep2.mp3?token=a&b=1#cc-pod=1&cc-title="), "{u}");
        let h = fragment_hints(&u);
        assert_eq!(h["cc-title"], "第 2 期：\"聊聊 AI\"");
        assert_eq!(h["cc-author"], "老王");
        assert_eq!(h["cc-ext"], "mp3");
        assert_eq!(h["cc-date"], "1759824000");
        assert!(h["cc-cover"].ends_with("cover.jpg"));
        // 原地址已经带片段
        let e = FeedEntry { url: "https://a.example.com/x#t=1".into(), ..f.entries[0].clone() };
        assert!(entry_url(&e, "", "").contains("#t=1&cc-pod=1&cc-title="));
        assert_eq!(parse_duration("59:30"), Some(3_570_000));
        assert_eq!(parse_duration("abc"), None);
    }

    #[test]
    fn playlist_info_has_selectable_entries() {
        let base = Url::parse("https://pod.example.com/feed.xml").unwrap();
        let f = parse_feed(RSS, &base).unwrap();
        let info = to_media_info(&f, "https://pod.example.com/feed.xml", "pod.example.com");
        assert_eq!(info.kind, MediaKind::Playlist);
        assert_eq!(info.entries.len(), 2);
        assert_eq!(info.entries[0].index, 1);
        assert!(info.platform_name.contains("科技"));
    }
}
