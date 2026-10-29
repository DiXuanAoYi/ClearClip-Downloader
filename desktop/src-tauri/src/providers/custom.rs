//! 自定义站点规则：用户为内置解析器和 yt-dlp 都不支持的网站写一条正则，从网页源码里取视频地址。

use regex::Regex;
use url::Url;

use super::{generic, Ctx, DESKTOP_UA};
use crate::model::{AppError, AppResult, Asset, ErrorKind, MediaInfo, MediaKind};
use crate::settings::SiteRule;

/// 规则是否适用于这个网址。
pub fn matches(rule: &SiteRule, url: &str) -> bool {
    if !rule.enabled || rule.pattern.is_empty() || rule.video_regex.is_empty() {
        return false;
    }
    match rule.pattern.strip_prefix("re:") {
        Some(re) => Regex::new(re).map(|r| r.is_match(url)).unwrap_or(false),
        None => url.to_lowercase().contains(&rule.pattern.to_lowercase()),
    }
}

pub fn find<'a>(rules: &'a [SiteRule], url: &str) -> Option<&'a SiteRule> {
    rules.iter().find(|r| matches(r, url))
}

fn compile(rule: &SiteRule, what: &str, re: &str) -> AppResult<Regex> {
    Regex::new(re).map_err(|e| AppError::invalid(format!("自定义规则“{}”的{what}正则无效：{e}", rule.name)))
}

fn first_capture(re: &Regex, text: &str) -> Option<String> {
    let c = re.captures(text)?;
    c.get(1).or_else(|| c.get(0)).map(|m| m.as_str().to_string())
}

/// 网页源码里的转义：`\/`、`&`、`&amp;`。
fn clean(raw: &str) -> String {
    let t = raw.replace("\\/", "/").replace("\\u002F", "/").replace("\\u0026", "&").replace("\\u003D", "=").replace("\\x26", "&");
    generic::html_unescape(&t)
}

/// 用规则从网页源码里取信息（不联网，便于测试）。
pub fn extract(rule: &SiteRule, html: &str, page: &Url) -> AppResult<MediaInfo> {
    let video_re = compile(rule, "视频地址", &rule.video_regex)?;
    let mut urls: Vec<String> = vec![];
    for c in video_re.captures_iter(html) {
        let raw = c.get(1).or_else(|| c.get(0)).map(|m| m.as_str()).unwrap_or("");
        let Ok(u) = page.join(clean(raw).trim()) else { continue };
        let s = u.to_string();
        if matches!(u.scheme(), "http" | "https") && !urls.contains(&s) {
            urls.push(s);
        }
        if urls.len() >= 12 {
            break;
        }
    }
    if urls.is_empty() {
        return Err(AppError::new(
            ErrorKind::Unsupported,
            format!("规则“{}”没有在网页里找到视频地址。可能需要登录，或地址由脚本动态生成，请检查正则。", rule.name),
        ));
    }
    let title = if rule.title_regex.is_empty() { None } else { first_capture(&compile(rule, "标题", &rule.title_regex)?, html).map(|t| clean(&t)) }
        .or_else(|| generic::TITLE_RE.captures(html).map(|c| clean(c[1].trim())))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| page.host_str().unwrap_or("视频").to_string());
    let cover = if rule.cover_regex.is_empty() {
        generic::meta(html, &["og:image", "twitter:image"]).into_iter().next()
    } else {
        first_capture(&compile(rule, "封面", &rule.cover_regex)?, html).map(|c| clean(&c))
    }
    .and_then(|c| page.join(&c).ok())
    .map(|u| u.to_string());
    let referer = if rule.referer.is_empty() { page.to_string() } else { rule.referer.clone() };
    let ua = if rule.user_agent.is_empty() { DESKTOP_UA.to_string() } else { rule.user_agent.clone() };
    let host = page.host_str().unwrap_or("").to_string();
    let site = crate::cookies::registrable_domain(&host);
    let mut assets: Vec<Asset> = urls.iter().enumerate().map(|(i, u)| generic::asset_for(i, u, &referer)).collect();
    for a in &mut assets {
        a.headers = vec![("Referer".into(), referer.clone()), ("User-Agent".into(), ua.clone())];
    }
    if let Some(c) = &cover {
        assets.push(Asset::cover(c.clone()));
    }
    let id: String = page.path().trim_matches('/').replace('/', "_");
    Ok(MediaInfo {
        platform: site.clone(),
        platform_name: if rule.name.is_empty() { site } else { rule.name.clone() },
        id: if id.is_empty() { host } else { id },
        source_url: page.to_string(),
        title,
        author: String::new(),
        cover,
        duration_ms: None,
        kind: MediaKind::Video,
        width: None,
        height: None,
        published_at: None,
        assets,
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("custom".into()),
    })
}

pub async fn resolve(ctx: &Ctx<'_>, rule: &SiteRule, url: &str) -> AppResult<MediaInfo> {
    Url::parse(url).map_err(|_| AppError::invalid("链接格式不正确。"))?;
    let ua = if rule.user_agent.is_empty() { DESKTOP_UA } else { &rule.user_agent };
    let mut req = ctx.get(url).header("User-Agent", ua);
    if let Some(c) = ctx.cookie(url) {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::from_status(status.as_u16(), "网页打不开"));
    }
    let final_url = resp.url().clone();
    let html = resp.text().await?;
    ctx.record("custom", "page", url, &html);
    extract(rule, &html, &final_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AssetKind;

    fn rule() -> SiteRule {
        SiteRule {
            name: "示例视频站".into(),
            pattern: "example-video.com/play/".into(),
            video_regex: r#"data-src=\"([^\"]+\.m3u8[^\"]*)\""#.replace("\\\"", "\""),
            title_regex: r#"<h1 class="t">(.*?)</h1>"#.into(),
            ..Default::default()
        }
    }

    #[test]
    fn rule_matching() {
        let r = rule();
        assert!(matches(&r, "https://www.Example-Video.com/play/123"));
        assert!(!matches(&r, "https://other.com/play/123"));
        let re = SiteRule { pattern: r"re:^https://v\d+\.site\.com/".into(), video_regex: "x".into(), ..Default::default() };
        assert!(matches(&re, "https://v3.site.com/a") && !matches(&re, "https://www.site.com/a"));
        assert!(!matches(&SiteRule { enabled: false, ..r.clone() }, "https://example-video.com/play/1"));
        assert!(
            !matches(&SiteRule { video_regex: String::new(), ..r.clone() }, "https://example-video.com/play/1"),
            "a rule without a video regex does nothing"
        );
        assert!(!matches(&SiteRule { pattern: "re:(".into(), video_regex: "x".into(), ..Default::default() }, "https://a.com"), "bad regex never matches");
        assert_eq!(find(&[re.clone(), r.clone()], "https://example-video.com/play/9").unwrap().name, "示例视频站");
    }

    #[test]
    fn extracts_video_title_and_cover() {
        let html = r#"<html><head><title>网页标题</title><meta property="og:image" content="/img/c.jpg"></head>
            <body><h1 class="t">第 &amp; 一集</h1>
            <div data-src="https:\/\/cdn.example-video.com\/hls\/1.m3u8?sig=a&b=2"></div>
            <div data-src="/hls/backup.m3u8"></div>
            <div data-src="https://cdn.example-video.com/hls/1.m3u8?sig=a&amp;b=2"></div></body></html>"#;
        let page = Url::parse("https://www.example-video.com/play/123").unwrap();
        let info = extract(&rule(), html, &page).unwrap();
        assert_eq!(info.title, "第 & 一集");
        assert_eq!(info.platform_name, "示例视频站");
        assert_eq!(info.id, "play_123");
        let videos: Vec<&Asset> = info.assets.iter().filter(|a| a.kind == AssetKind::Video).collect();
        assert_eq!(videos.len(), 2, "duplicates are merged: {:?}", videos.iter().map(|a| &a.url).collect::<Vec<_>>());
        assert_eq!(videos[0].url, "https://cdn.example-video.com/hls/1.m3u8?sig=a&b=2");
        assert_eq!(videos[1].url, "https://www.example-video.com/hls/backup.m3u8");
        assert_eq!(videos[0].protocol, crate::model::Protocol::Hls);
        assert!(videos[0].headers.iter().any(|(k, v)| k == "Referer" && v == "https://www.example-video.com/play/123"));
        assert_eq!(info.cover.as_deref(), Some("https://www.example-video.com/img/c.jpg"));
    }

    #[test]
    fn errors_are_explained() {
        let page = Url::parse("https://www.example-video.com/play/1").unwrap();
        let e = extract(&rule(), "<html>nothing</html>", &page).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Unsupported);
        assert!(e.message.contains("示例视频站") && e.message.contains("正则"));
        let bad = SiteRule { video_regex: "(".into(), ..rule() };
        let e = extract(&bad, "x", &page).unwrap_err();
        assert!(e.message.contains("视频地址正则无效"), "{}", e.message);
        // 没有捕获组：取整个匹配
        let whole = SiteRule { video_regex: r"https://cdn\.x\.com/[a-z0-9]+\.mp4".into(), ..rule() };
        let info = extract(&whole, "var u='https://cdn.x.com/abc123.mp4';", &page).unwrap();
        assert_eq!(info.assets[0].url, "https://cdn.x.com/abc123.mp4");
        assert_eq!(info.title, "www.example-video.com", "falls back to the host without any title");
    }
}
