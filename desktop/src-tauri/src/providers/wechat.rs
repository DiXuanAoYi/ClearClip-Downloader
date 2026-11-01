//! 微信公众号文章里的视频（可选组件，默认关闭，设置 → 高级 → 微信组件 里开启）。
//!
//! 做法（参考公开项目 mpwx-video-getter 的思路，自行实现）：抓文章 HTML，读出页面里的 `mp_video_trans_info`
//! （每个视频各清晰度的签名直链和尺寸、大小、时长），没有时退回到页面里直接出现的 `mpvideo.qpic.cn` 地址。
//! 直链带时效签名，所以不缓存；下载时必须带微信的 Referer，否则 CDN 返回 403。
//!
//! 只处理文章里的微信原生视频。腾讯视频嵌入、视频号卡片、需要登录或被“环境异常”拦截的文章会给出明确提示。

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use url::Url;

use super::{Ctx, Provider, DESKTOP_UA};
use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// 组件开关（由设置同步过来）。
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub struct Wechat;

pub const REFERER: &str = "https://mp.weixin.qq.com/";

#[async_trait]
impl Provider for Wechat {
    fn id(&self) -> &'static str {
        "wechat"
    }
    fn name(&self) -> &'static str {
        "微信公众号"
    }
    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| h == "mp.weixin.qq.com") && (url.path() == "/s" || url.path().starts_with("/s/"))
    }
    fn referer(&self) -> &'static str {
        REFERER
    }
    fn login_url(&self) -> &'static str {
        "https://mp.weixin.qq.com/"
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        let resp = ctx
            .get(url)
            .header("User-Agent", DESKTOP_UA)
            .header("Referer", REFERER)
            .header("Accept-Language", "zh-CN,zh;q=0.9")
            .send()
            .await
            .map_err(|e| AppError::new(crate::model::ErrorKind::Network, format!("打不开这篇文章：{e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::msg(format!("微信返回 {}，文章可能已删除或需要在微信里打开。", resp.status().as_u16())));
        }
        let html = resp.text().await?;
        ctx.record("wechat", "article", url, &html);
        parse_article(&html, url)
    }
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// 页面里的 URL 常被 JS 转义（\x26amp;、/ 等），统一还原。
fn normalize(s: &str) -> String {
    s.replace("\\u002F", "/")
        .replace("\\u002f", "/")
        .replace("\\x26amp;", "&")
        .replace("\\u0026amp;", "&")
        .replace("\\x26", "&")
        .replace("\\u0026", "&")
        .replace("\\/", "/")
}

static BLOCKED_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<title>[^<]*(环境异常|操作频繁|访问频繁|系统繁忙|验证)[^<]*</title>").unwrap());
static MPVIDEO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://mpvideo\.qpic\.cn/[^"'\\\s<>)]+"#).unwrap());
static OG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<meta[^>]+property=["'](og:[a-z_:]+)["'][^>]+content=["']([^"']*)["']"#).unwrap());
static FORMAT_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.f(\d{5})\.mp4").unwrap());
static NICKNAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"var\s+nickname\s*=\s*(?:htmlDecode\()?["']([^"']*)["']"#).unwrap());

fn looks_blocked(html: &str) -> bool {
    BLOCKED_TITLE.is_match(html) || (html.len() < 100_000 && ["环境异常", "操作频繁", "访问频繁", "请完成验证"].iter().any(|k| html.contains(k)))
}

fn field(item: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r"{name}:\s*(?:\(\s*)?'([^']*)'")).ok()?;
    re.captures(item).map(|c| c[1].to_string())
}

#[derive(Debug, Clone, PartialEq)]
struct Variant {
    url: String,
    format_id: u32,
    width: u32,
    height: u32,
    size: u64,
    duration_ms: u64,
    label: String,
}

fn quality_label(format_id: u32) -> &'static str {
    match format_id {
        10002 | 10102 => "超清",
        10003 | 10103 => "高清",
        10004 | 10104 => "流畅",
        _ => "标清",
    }
}

/// 同一个视频各清晰度共享路径前缀。
fn base_of(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    FORMAT_SUFFIX.replace(path, "").to_string()
}

fn trans_info(html: &str) -> Vec<Variant> {
    let mut out: Vec<Variant> = vec![];
    let mut from = 0;
    while let Some(i) = html[from..].find("mp_video_trans_info") {
        let start = from + i + "mp_video_trans_info".len();
        from = start;
        let Some(open_arr) = html[start..].find('[') else { break };
        let mut pos = start + open_arr + 1;
        while let Some(o) = html[pos..].find('{') {
            let open = pos + o;
            let Some(c) = html[open..].find('}') else { break };
            let close = open + c;
            let item = &html[open..=close];
            pos = close + 1;
            if !item.contains("format_id") {
                break;
            }
            let Some(raw) = field(item, "url") else { continue };
            let url = decode_entities(&normalize(&raw)).replacen("http://", "https://", 1);
            if !url.contains("mpvideo.qpic.cn") || out.iter().any(|v| v.url == url) {
                continue;
            }
            let num = |n: &str| field(item, n).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            let format_id = num("format_id") as u32;
            out.push(Variant {
                url,
                format_id,
                width: num("width") as u32,
                height: num("height") as u32,
                size: num("filesize"),
                duration_ms: num("duration_ms"),
                label: field(item, "video_quality_wording").filter(|s| !s.is_empty()).unwrap_or_else(|| quality_label(format_id).to_string()),
            });
        }
    }
    out
}

/// trans_info 没有时，用页面里出现的 mpvideo 直链兜底（按文件名里的格式号推断清晰度）。
fn bare_urls(html: &str) -> Vec<Variant> {
    let text = normalize(html);
    let mut out: Vec<Variant> = vec![];
    for m in MPVIDEO.find_iter(&text) {
        let url = decode_entities(m.as_str()).replacen("http://", "https://", 1).trim_end_matches('&').to_string();
        if !url.contains(".mp4") || out.iter().any(|v| v.url == url) {
            continue;
        }
        let fid = FORMAT_SUFFIX.captures(&url).and_then(|c| c[1].parse().ok()).unwrap_or(0);
        out.push(Variant { url, format_id: fid, width: 0, height: 0, size: 0, duration_ms: 0, label: quality_label(fid).into() });
    }
    out
}

fn rank(v: &Variant) -> (u8, u32) {
    // 同清晰度优先标准编码（10002/10003/10004 比 101xx 省流变体兼容性好），再按像素
    let std = if matches!(v.format_id, 10002 | 10003 | 10004) { 1 } else { 0 };
    (std, v.width * v.height)
}

pub fn parse_article(html: &str, source_url: &str) -> AppResult<MediaInfo> {
    if looks_blocked(html) {
        return Err(AppError::msg("微信拦截了这次访问（环境异常 / 访问频繁）。稍后再试，或换一个网络。"));
    }
    let mut variants = trans_info(html);
    if variants.is_empty() {
        variants = bare_urls(html);
    }
    if variants.is_empty() {
        let hint = if html.contains("v.qq.com") {
            "这篇文章里的视频是腾讯视频嵌入，请直接复制腾讯视频链接来下载。"
        } else if html.contains("mp-common-videosnap") || html.contains("finder") {
            "这篇文章里是视频号卡片，需要视频号组件，暂不支持从文章里直接下载。"
        } else {
            "没有在这篇文章里找到微信原生视频。"
        };
        return Err(AppError::msg(hint));
    }
    // 按视频分组，组内最好的排前面
    let mut groups: BTreeMap<String, Vec<Variant>> = BTreeMap::new();
    let mut order: Vec<String> = vec![];
    for v in variants {
        let b = base_of(&v.url);
        if !groups.contains_key(&b) {
            order.push(b.clone());
        }
        groups.entry(b).or_default().push(v);
    }
    let multi = order.len() > 1;
    let mut assets = vec![];
    let mut first_dur = None;
    let (mut w0, mut h0) = (None, None);
    for (gi, b) in order.iter().enumerate() {
        let mut list = groups.remove(b).unwrap_or_default();
        list.sort_by(|a, b| rank(b).cmp(&rank(a)));
        // 同清晰度档位只留最好的一个，避免列表里出现一堆重复
        let mut seen = std::collections::HashSet::new();
        for (qi, v) in list.into_iter().enumerate() {
            let tier = (v.label.clone(), v.height);
            if !seen.insert(tier) {
                continue;
            }
            let id = if gi == 0 && qi == 0 { "video".to_string() } else { format!("video-{}-{}", gi + 1, v.format_id) };
            let mut a = Asset::base(
                id,
                AssetKind::Video,
                v.url.clone(),
                if multi { format!("视频 {} · {}", gi + 1, v.label) } else { format!("视频 · {}", v.label) },
                "mp4",
            );
            a.width = (v.width > 0).then_some(v.width);
            a.height = (v.height > 0).then_some(v.height);
            a.quality = Some(v.label.clone());
            a.filesize = (v.size > 0).then_some(v.size);
            a.headers = vec![("Referer".into(), REFERER.into()), ("User-Agent".into(), DESKTOP_UA.into())];
            if gi == 0 && qi == 0 {
                first_dur = (v.duration_ms > 0).then_some(v.duration_ms);
                w0 = a.width;
                h0 = a.height;
            }
            assets.push(a);
        }
    }
    let og: BTreeMap<String, String> = OG.captures_iter(html).map(|c| (c[1].to_lowercase(), decode_entities(&c[2]).trim().to_string())).collect();
    let title = og.get("og:title").cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| "微信公众号视频".into());
    let author = NICKNAME
        .captures(html)
        .map(|c| decode_entities(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| og.get("og:article:author").cloned())
        .unwrap_or_else(|| "公众号".into());
    let cover = og.get("og:image").cloned().filter(|s| !s.is_empty()).map(|u| u.replacen("http://", "https://", 1));
    if let Some(c) = &cover {
        let mut a = Asset::cover(c.clone());
        a.headers = vec![("Referer".into(), REFERER.into())];
        assets.push(a);
    }
    let id = Url::parse(source_url)
        .ok()
        .map(|u| u.path().trim_start_matches("/s/").trim_start_matches("/s").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "article".into());
    Ok(MediaInfo {
        platform: "wechat".into(),
        platform_name: "微信公众号".into(),
        id,
        source_url: source_url.to_string(),
        title,
        author,
        cover,
        duration_ms: first_dur,
        kind: MediaKind::Video,
        width: w0,
        height: h0,
        published_at: None,
        assets,
        entries: vec![],
        series: None,
        extractor: Some("native".into()),
        chapters: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"<html><head><title>一篇文章</title>
<meta property="og:title" content="春日 &amp; 花开">
<meta property="og:image" content="http://mmbiz.qpic.cn/mmbiz_jpg/abc/0?wx_fmt=jpeg">
</head><body><script>var nickname = "示例号";
window.__mpVideoData = { mp_video_trans_info: [
 { url: ('http://mpvideo.qpic.cn/0bf2/v1.f10002.mp4?dis_k=aa\x26amp;dis_t=1').replace(/&amp;/g,'&'), format_id: '10002', width: '1080', height: '1920', filesize: '5000000', duration_ms: '61000', video_quality_wording: '超清' },
 { url: 'http://mpvideo.qpic.cn/0bf2/v1.f10102.mp4?dis_k=bb&dis_t=1', format_id: '10102', width: '1080', height: '1920', filesize: '3000000', duration_ms: '61000', video_quality_wording: '超清' },
 { url: 'http://mpvideo.qpic.cn/0bf2/v1.f10004.mp4?dis_k=cc&dis_t=1', format_id: '10004', width: '540', height: '960', filesize: '1000000', duration_ms: '61000' }
], other: 1 };
</script></body></html>"#;

    #[test]
    fn reads_variants_and_picks_the_standard_encode_per_tier() {
        let m = parse_article(ARTICLE, "https://mp.weixin.qq.com/s/yCLxi7nbHZ6fh_L6q5ll_A").unwrap();
        assert_eq!((m.title.as_str(), m.author.as_str(), m.platform.as_str()), ("春日 & 花开", "示例号", "wechat"));
        assert_eq!(m.id, "yCLxi7nbHZ6fh_L6q5ll_A");
        assert_eq!(m.duration_ms, Some(61000));
        let vids: Vec<&Asset> = m.assets.iter().filter(|a| a.kind == AssetKind::Video).collect();
        assert_eq!(vids.len(), 2, "同为超清的 10002 与 10102 只留标准编码: {vids:?}");
        assert_eq!(vids[0].id, "video");
        assert!(vids[0].url.starts_with("https://mpvideo.qpic.cn/0bf2/v1.f10002.mp4?dis_k=aa&dis_t=1"), "{}", vids[0].url);
        assert_eq!((vids[0].width, vids[0].height, vids[0].filesize), (Some(1080), Some(1920), Some(5_000_000)));
        assert_eq!(vids[1].quality.as_deref(), Some("流畅"));
        assert!(vids.iter().all(|v| v.headers.iter().any(|(k, v)| k == "Referer" && v == REFERER)), "下载必须带微信 Referer");
        assert!(m.cover.as_deref().unwrap().starts_with("https://mmbiz.qpic.cn"));
    }

    #[test]
    fn falls_back_to_bare_links_and_explains_failures() {
        let html = r#"<script>var u = "http:\/\/mpvideo.qpic.cn\/x\/y.f10003.mp4?dis_k=1\x26amp;dis_t=2";</script>"#;
        let m = parse_article(html, "https://mp.weixin.qq.com/s?__biz=1").unwrap();
        let v = m.assets.iter().find(|a| a.kind == AssetKind::Video).unwrap();
        assert_eq!(v.url, "https://mpvideo.qpic.cn/x/y.f10003.mp4?dis_k=1&dis_t=2");
        assert_eq!(v.quality.as_deref(), Some("高清"));
        assert!(parse_article("<title>环境异常</title>", "https://mp.weixin.qq.com/s/x").unwrap_err().to_string().contains("拦截"));
        assert!(parse_article("<p>纯文字</p><a href='https://v.qq.com/x/page/a.html'>x</a>", "https://mp.weixin.qq.com/s/x")
            .unwrap_err()
            .to_string()
            .contains("腾讯视频"));
        assert!(parse_article("<p>纯文字</p>", "https://mp.weixin.qq.com/s/x").unwrap_err().to_string().contains("没有在这篇文章"));
    }

    #[test]
    fn two_videos_are_kept_apart_and_url_matching_is_strict() {
        let html = r#"<script>mp_video_trans_info: [ { url: 'http://mpvideo.qpic.cn/a/1.f10002.mp4?k=1', format_id: '10002', width: '720', height: '1280' },
 { url: 'http://mpvideo.qpic.cn/b/2.f10002.mp4?k=2', format_id: '10002', width: '720', height: '1280' } ]</script>"#;
        let m = parse_article(html, "https://mp.weixin.qq.com/s/x").unwrap();
        let ids: Vec<_> = m.assets.iter().filter(|a| a.kind == AssetKind::Video).map(|a| a.label.clone()).collect();
        assert_eq!(ids, vec!["视频 1 · 超清", "视频 2 · 超清"]);
        let p = |u: &str| Wechat.matches(&Url::parse(u).unwrap());
        assert!(p("https://mp.weixin.qq.com/s/abc") && p("https://mp.weixin.qq.com/s?__biz=1&mid=2"));
        assert!(!p("https://mp.weixin.qq.com/") && !p("https://weixin.qq.com/s/abc") && !p("https://evil.com/mp.weixin.qq.com/s/a"));
    }
}
