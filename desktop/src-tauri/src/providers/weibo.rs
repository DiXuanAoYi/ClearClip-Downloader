//! 微博：从链接取微博 ID（数字 mid 或 base62 的 mblogid），调用移动版
//! `m.weibo.cn/statuses/show` 获取正文、图片和视频。转发微博没有媒体时使用原微博的媒体。
//!
//! 部分微博需要登录才能查看，可在设置中登录微博（移动版）。

use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use super::{str_at, u32_at, Ctx, Provider};
use crate::model::{AppError, AppResult, Asset, MediaInfo, MediaKind};

pub struct Weibo;

static STATUS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"m\.weibo\.cn/(?:status|detail)/(\w+)").unwrap());
static DESKTOP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"weibo\.com/(?:\d+|u/\d+)/([0-9A-Za-z]{8,10})(?:[/?#]|$)").unwrap());
static QUERY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&](?:id|mid|weibo_id)=(\w+)").unwrap());
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

#[async_trait]
impl Provider for Weibo {
    fn id(&self) -> &'static str {
        "weibo"
    }

    fn name(&self) -> &'static str {
        "微博"
    }

    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| h == "weibo.com" || h.ends_with(".weibo.com") || h == "weibo.cn" || h.ends_with(".weibo.cn") || h == "t.cn")
    }

    fn referer(&self) -> &'static str {
        "https://m.weibo.cn/"
    }

    fn login_url(&self) -> &'static str {
        "https://m.weibo.cn/"
    }

    async fn account_status(&self, ctx: &Ctx<'_>) -> AppResult<Option<super::AccountStatus>> {
        let api = "https://m.weibo.cn/api/config";
        let mut req = ctx.get(api).header("Referer", "https://m.weibo.cn/");
        if let Some(c) = ctx.cookie(api) {
            req = req.header("Cookie", c);
        }
        let v: Value = req.send().await?.json().await?;
        let login = v.pointer("/data/login").and_then(Value::as_bool).unwrap_or(false);
        Ok(Some(super::AccountStatus { logged_in: login, user_name: str_at(&v, "/data/uid").map(|u| format!("UID {u}")), vip: None }))
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        let id = match status_id_from_url(url) {
            Some(id) => id,
            None => {
                // t.cn 等短链
                let resp = ctx.get(url).send().await?;
                status_id_from_url(resp.url().as_str())
                    .ok_or_else(|| AppError::msg("没能从链接里识别出微博 ID。目前支持单条微博链接（weibo.com/用户ID/微博ID 或 m.weibo.cn/status/…）。"))?
            }
        };
        let mut req = ctx
            .get(format!("https://m.weibo.cn/statuses/show?id={id}"))
            .header("Referer", "https://m.weibo.cn/")
            .header("MWeibo-Pwa", "1")
            .header("X-Requested-With", "XMLHttpRequest");
        if let Some(cookie) = ctx.cookie("https://m.weibo.cn/") {
            req = req.header("Cookie", cookie);
        }
        let resp = req.send().await?;
        let text = resp.text().await?;
        ctx.record("weibo", "statuses-show", &format!("https://m.weibo.cn/statuses/show?id={id}"), &text);
        let data: Value = serde_json::from_str(&text).map_err(|_| AppError::need_login("微博返回的不是有效数据，可能需要登录。请在设置中登录微博后重试。"))?;
        let status = status_data(&data)?;
        parse_status(status, &id, url)
    }
}

pub fn status_id_from_url(url: &str) -> Option<String> {
    STATUS_RE
        .captures(url)
        .or_else(|| DESKTOP_RE.captures(url))
        .or_else(|| if url.contains("weibo.") { QUERY_RE.captures(url) } else { None })
        .map(|c| c[1].to_string())
}

pub fn status_data(v: &Value) -> AppResult<&Value> {
    if v.get("ok").and_then(Value::as_i64) != Some(1) {
        let msg = str_at(v, "/msg").unwrap_or("微博不存在、已删除或需要登录才能查看。");
        return Err(AppError::classify(format!("微博：{msg}")));
    }
    v.get("data").filter(|d| d.is_object()).ok_or_else(|| AppError::msg("微博没有返回内容。"))
}

pub fn parse_status(status: &Value, id: &str, source_url: &str) -> AppResult<MediaInfo> {
    // 转发且自身无媒体时，取原微博的媒体
    let media_src = if has_media(status) { status } else { status.get("retweeted_status").filter(|r| has_media(r)).unwrap_or(status) };

    let text = clean_text(str_at(status, "/text").unwrap_or(""));
    let title = if text.is_empty() { "微博".to_string() } else { text };
    let author = str_at(status, "/user/screen_name").unwrap_or("未知用户").to_string();
    let published_at = str_at(status, "/created_at").and_then(|s| chrono::DateTime::parse_from_str(s, "%a %b %d %H:%M:%S %z %Y").ok()).map(|d| d.timestamp());

    let mut assets = Vec::new();
    let video = video_url(media_src);
    let pics = media_src.get("pics").and_then(Value::as_array).cloned().unwrap_or_default();

    let (kind, cover) = if let Some(v) = video {
        let cover = str_at(media_src, "/page_info/page_pic/url").map(String::from);
        let w = u32_at(media_src, "/page_info/media_info/width");
        let h = u32_at(media_src, "/page_info/media_info/height");
        assets.push(Asset::video(v, w, h));
        (MediaKind::Video, cover)
    } else if !pics.is_empty() {
        for (i, p) in pics.iter().enumerate() {
            // 实况照片 / 视频类型的 pic 里，图片地址仍在 large.url
            let Some(url) = str_at(p, "/large/url").or_else(|| str_at(p, "/url")) else { continue };
            let w = u32_at(p, "/large/geo/width");
            let h = u32_at(p, "/large/geo/height");
            assets.push(Asset::image(i, to_large(url), w, h));
        }
        let cover = assets.first().map(|a| a.url.clone());
        (MediaKind::Images, cover)
    } else {
        return Err(AppError::msg("这条微博没有图片或视频。"));
    };
    if let Some(c) = &cover {
        if kind == MediaKind::Video {
            assets.push(Asset::cover(c.clone()));
        }
    }

    Ok(MediaInfo {
        platform: "weibo".into(),
        platform_name: "微博".into(),
        id: str_at(status, "/bid").or_else(|| str_at(status, "/mid")).unwrap_or(id).to_string(),
        source_url: source_url.to_string(),
        title,
        author,
        cover,
        duration_ms: status_duration(media_src),
        kind,
        width: assets.first().and_then(|a| a.width),
        height: assets.first().and_then(|a| a.height),
        published_at,
        assets,
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("native".into()),
    })
}

fn has_media(s: &Value) -> bool {
    video_url(s).is_some() || s.get("pics").and_then(Value::as_array).is_some_and(|p| !p.is_empty())
}

/// 依次尝试高清到标清的视频地址。
fn video_url(s: &Value) -> Option<String> {
    if str_at(s, "/page_info/type") != Some("video") {
        return None;
    }
    let urls = s.pointer("/page_info/urls");
    for key in ["mp4_1080p_mp4", "mp4_720p_mp4", "mp4_hd_mp4", "mp4_ld_mp4"] {
        if let Some(u) = urls.and_then(|u| u.get(key)).and_then(Value::as_str).filter(|u| !u.is_empty()) {
            return Some(u.to_string());
        }
    }
    ["/page_info/media_info/mp4_720p_mp4", "/page_info/media_info/stream_url_hd", "/page_info/media_info/mp4_hd_url", "/page_info/media_info/stream_url"]
        .iter()
        .find_map(|p| str_at(s, p))
        .map(String::from)
}

fn status_duration(s: &Value) -> Option<u64> {
    s.pointer("/page_info/media_info/duration")
        .and_then(|d| d.as_f64().or_else(|| d.as_str().and_then(|x| x.parse().ok())))
        .map(|sec| (sec * 1000.0) as u64)
        .filter(|d| *d > 0)
}

/// 缩略图地址换成原图：`/orj360/`、`/mw690/` 等 → `/large/`。
fn to_large(url: &str) -> String {
    static SIZE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.sinaimg\.cn/[a-z0-9_]+/").unwrap());
    SIZE_RE.replace(url, ".sinaimg.cn/large/").into_owned()
}

/// 去掉 HTML 标签、话题和表情图片，保留纯文本。
fn clean_text(html: &str) -> String {
    let text = html.replace("<br />", " ").replace("<br/>", " ");
    let text = TAG_RE.replace_all(&text, "");
    let text = text.replace("&quot;", "\"").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ");
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    text.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PICS: &str = include_str!("../../tests/fixtures/weibo_pics.json");
    const VIDEO: &str = include_str!("../../tests/fixtures/weibo_video.json");

    #[test]
    fn status_ids() {
        assert_eq!(status_id_from_url("https://weibo.com/1234567890/OabcD1234").as_deref(), Some("OabcD1234"));
        assert_eq!(status_id_from_url("https://m.weibo.cn/status/5084123456789012").as_deref(), Some("5084123456789012"));
        assert_eq!(status_id_from_url("https://m.weibo.cn/detail/OabcD1234?from=x").as_deref(), Some("OabcD1234"));
        assert_eq!(status_id_from_url("https://weibo.com/u/1234567890"), None);
        assert_eq!(status_id_from_url("http://t.cn/A6abcdE"), None);
    }

    #[test]
    fn parses_picture_status() {
        let v: Value = serde_json::from_str(PICS).unwrap();
        let info = parse_status(status_data(&v).unwrap(), "x", "u").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        assert_eq!(info.id, "OabcD1234");
        assert_eq!(info.title, "秋天的武功山 #徒步#");
        assert_eq!(info.author, "走走停停");
        let imgs: Vec<_> = info.assets.iter().filter(|a| a.id.starts_with("image-")).collect();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].url, "https://wx1.sinaimg.cn/large/abc001.jpg");
        assert_eq!(imgs[1].ext, "gif");
        assert_eq!(info.published_at, Some(1_727_856_000));
    }

    #[test]
    fn uses_retweeted_video_when_repost_has_no_media() {
        let v: Value = serde_json::from_str(VIDEO).unwrap();
        let info = parse_status(status_data(&v).unwrap(), "x", "u").unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        assert_eq!(info.title, "转发微博");
        assert!(info.assets[0].url.contains("720p"), "{}", info.assets[0].url);
        assert_eq!(info.duration_ms, Some(31_500));
        assert!(info.assets.iter().any(|a| a.id == "cover"));
    }

    #[test]
    fn not_ok_surfaces_message() {
        let v: Value = serde_json::from_str(r#"{"ok":0,"msg":"这条微博已经被删除"}"#).unwrap();
        assert!(status_data(&v).unwrap_err().to_string().contains("已经被删除"));
    }
}
