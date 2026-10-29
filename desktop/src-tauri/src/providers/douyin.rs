//! 抖音：短链跳转拿到作品 ID，再读取分享页内嵌的 `window._ROUTER_DATA`。
//!
//! 读取移动端分享页，不依赖已失效的 `aweme/iteminfo` 接口。

use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use super::{str_at, u32_at, u64_at, Ctx, Provider};
use crate::model::{AppError, AppResult, Asset, MediaInfo, MediaKind};

pub struct Douyin;

static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/(?:video|note|slides|share/video|share/note|share/slides)/(\d{8,})").unwrap());
static QUERY_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:modal_id|aweme_id|item_ids|vid)=(\d{8,})").unwrap());

#[async_trait]
impl Provider for Douyin {
    fn id(&self) -> &'static str {
        "douyin"
    }

    fn name(&self) -> &'static str {
        "抖音"
    }

    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| h == "douyin.com" || h.ends_with(".douyin.com") || h.ends_with("iesdouyin.com"))
    }

    fn referer(&self) -> &'static str {
        "https://www.douyin.com/"
    }

    fn login_url(&self) -> &'static str {
        "https://www.douyin.com/"
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        let id = match aweme_id_from_url(url) {
            Some(id) => id,
            None => {
                // 短链：跟随跳转拿到真实地址
                let resp = ctx.get(url).send().await?;
                let final_url = resp.url().to_string();
                aweme_id_from_url(&final_url).ok_or_else(|| AppError::not_found("没能从链接里识别出抖音作品 ID，链接可能已失效。"))?
            }
        };

        let page = format!("https://www.iesdouyin.com/share/video/{id}/");
        let mut req = ctx.get(&page).header("Referer", "https://www.douyin.com/");
        if let Some(cookie) = ctx.cookie(&page) {
            req = req.header("Cookie", cookie);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(AppError::from_status(resp.status().as_u16(), "抖音分享页").context_suffix("，请稍后再试或在设置中登录抖音。"));
        }
        let html = resp.text().await?;
        ctx.record("douyin", "share-page", &page, &html);
        let item = item_from_share_html(&html)?;
        parse_item(&item, &id, url)
    }
}

pub fn aweme_id_from_url(url: &str) -> Option<String> {
    ID_RE.captures(url).or_else(|| QUERY_ID_RE.captures(url)).map(|c| c[1].to_string())
}

/// 从分享页 HTML 中取出作品 JSON（`item_list[0]`）。
pub fn item_from_share_html(html: &str) -> AppResult<Value> {
    let data = super::extract_json_after(html, "window._ROUTER_DATA")
        .ok_or_else(|| AppError::parser("抖音页面结构已变化，没有找到作品数据。请尝试远程解析模式或等待更新。"))?;
    let loader = data.get("loaderData").and_then(Value::as_object).ok_or_else(|| AppError::parser("抖音页面数据缺少 loaderData。"))?;
    for page in loader.values() {
        let Some(res) = page.get("videoInfoRes") else { continue };
        if let Some(item) = res.pointer("/item_list/0") {
            return Ok(item.clone());
        }
        if let Some(reason) = res.pointer("/filter_list/0/detail_msg").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            return Err(AppError::not_found(format!("作品不可访问：{reason}")));
        }
        return Err(AppError::not_found("作品不存在、已删除或仅作者可见。"));
    }
    Err(AppError::need_login("抖音页面里没有作品信息，可能需要登录。请在设置中登录抖音后重试。"))
}

/// 把抖音作品 JSON 转成统一结构。
pub fn parse_item(item: &Value, id: &str, source_url: &str) -> AppResult<MediaInfo> {
    let title = str_at(item, "/desc").unwrap_or("抖音作品").trim().to_string();
    let author = str_at(item, "/author/nickname").unwrap_or("未知作者").to_string();
    let cover = str_at(item, "/video/cover/url_list/0").or_else(|| str_at(item, "/video/origin_cover/url_list/0")).map(String::from);
    let published_at = u64_at(item, "/create_time").map(|n| n as i64);
    let music = str_at(item, "/music/play_url/url_list/0").or_else(|| str_at(item, "/music/play_url/uri").filter(|u| u.starts_with("http"))).map(String::from);

    let images = item.get("images").and_then(Value::as_array).filter(|a| !a.is_empty());
    let mut assets = Vec::new();

    let (kind, width, height, duration_ms) = if let Some(images) = images {
        // 循环从 0 开始，不用封面顶替第一张图。
        for (i, img) in images.iter().enumerate() {
            if let Some(url) = pick_image_url(img) {
                assets.push(Asset::image(i, url, u32_at(img, "/width"), u32_at(img, "/height")));
            }
        }
        if assets.is_empty() {
            return Err(AppError::msg("图集里没有可下载的图片。"));
        }
        (MediaKind::Images, None, None, None)
    } else {
        let w = u32_at(item, "/video/width");
        let h = u32_at(item, "/video/height");
        let play = video_play_url(item).ok_or_else(|| AppError::not_found("没有找到视频地址，作品可能已删除。"))?;
        let mut default = Asset::video(play, w, h);
        default.label = format!("视频 {}", default.quality.clone().unwrap_or_else(|| "MP4".into()));
        assets.push(default);
        assets.extend(bitrate_variants(item));
        (MediaKind::Video, w, h, u64_at(item, "/video/duration").filter(|d| *d > 0))
    };

    if let Some(m) = music {
        assets.push(Asset::audio(m));
    }
    if let Some(c) = &cover {
        assets.push(Asset::cover(c.clone()));
    }

    Ok(MediaInfo {
        platform: "douyin".into(),
        platform_name: "抖音".into(),
        id: id.to_string(),
        source_url: source_url.to_string(),
        title,
        author,
        cover,
        duration_ms,
        kind,
        width,
        height,
        published_at,
        assets,
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("native".into()),
    })
}

/// 无水印播放地址：把 `playwm` 换成 `play`，或用 `uri` 拼接。
fn video_play_url(item: &Value) -> Option<String> {
    if let Some(u) = str_at(item, "/video/play_addr/url_list/0") {
        return Some(u.replace("/playwm/", "/play/").replace("playwm", "play"));
    }
    let uri = str_at(item, "/video/play_addr/uri")?;
    if uri.starts_with("http") {
        return Some(uri.to_string());
    }
    Some(format!("https://www.iesdouyin.com/aweme/v1/play/?video_id={uri}&ratio=1080p&line=0"))
}

/// `video.bit_rate` 里的其他清晰度 / 编码，按清晰度从高到低排列。
fn bitrate_variants(item: &Value) -> Vec<Asset> {
    let Some(list) = item.pointer("/video/bit_rate").and_then(Value::as_array) else { return vec![] };
    let mut out: Vec<Asset> = list
        .iter()
        .filter_map(|b| {
            let url = str_at(b, "/play_addr/url_list/0")?.replace("playwm", "play");
            let gear = str_at(b, "/gear_name").map(String::from).unwrap_or_else(|| format!("q{}", u64_at(b, "/quality_type").unwrap_or(0)));
            let h265 = u64_at(b, "/is_h265") == Some(1) || str_at(b, "/format").is_some_and(|f| f.contains("265"));
            let mut a = Asset::video(url, u32_at(b, "/play_addr/width"), u32_at(b, "/play_addr/height"));
            a.id = format!("video-{gear}");
            a.vcodec = Some(if h265 { "h265".into() } else { "h264".into() });
            a.bitrate = u64_at(b, "/bit_rate").map(|n| n / 1000);
            a.filesize = u64_at(b, "/play_addr/data_size");
            a.fps = u64_at(b, "/FPS").map(|f| f as f32);
            a.label = format!("视频 {} {}", a.quality.clone().unwrap_or_else(|| gear.clone()), if h265 { "H.265" } else { "H.264" });
            Some(a)
        })
        .collect();
    out.sort_by(|a, b| b.short_side().cmp(&a.short_side()).then(b.bitrate.cmp(&a.bitrate)));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// 图集图片优先选 jpeg 格式的地址，方便直接打开。
fn pick_image_url(img: &Value) -> Option<String> {
    let list: Vec<&str> = img.get("url_list")?.as_array()?.iter().filter_map(Value::as_str).collect();
    list.iter().find(|u| u.contains(".jpeg") || u.contains(".jpg")).or_else(|| list.first()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO_HTML: &str = include_str!("../../tests/fixtures/douyin_video.html");
    const NOTE_HTML: &str = include_str!("../../tests/fixtures/douyin_note.html");
    const DELETED_HTML: &str = include_str!("../../tests/fixtures/douyin_deleted.html");

    #[test]
    fn id_from_various_urls() {
        assert_eq!(aweme_id_from_url("https://www.iesdouyin.com/share/video/7421234567890123456/?region=CN").as_deref(), Some("7421234567890123456"));
        assert_eq!(aweme_id_from_url("https://www.douyin.com/note/7421234567890123457").as_deref(), Some("7421234567890123457"));
        assert_eq!(aweme_id_from_url("https://www.douyin.com/discover?modal_id=7421234567890123458").as_deref(), Some("7421234567890123458"));
        assert_eq!(aweme_id_from_url("https://v.douyin.com/ehHpu7V/"), None);
    }

    #[test]
    fn parses_video_share_page() {
        let item = item_from_share_html(VIDEO_HTML).unwrap();
        let info = parse_item(&item, "7421234567890123456", "https://v.douyin.com/ehHpu7V/").unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        assert_eq!(info.author, "山野厨房");
        assert_eq!(info.title, "秋天第一锅板栗焖鸡 #家常菜");
        assert_eq!(info.duration_ms, Some(47_000));
        assert_eq!(info.width, Some(1080));
        let video = info.assets.iter().find(|a| a.id == "video").unwrap();
        let variants: Vec<_> = info.assets.iter().filter(|a| a.id.starts_with("video-")).collect();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].quality.as_deref(), Some("1080P"));
        assert_eq!(variants[0].vcodec.as_deref(), Some("h265"));
        assert_eq!(variants[0].filesize, Some(12_000_000));
        assert_eq!(variants[1].quality.as_deref(), Some("720P"));
        assert!(video.url.contains("/play/") && !video.url.contains("playwm"), "{}", video.url);
        assert!(info.assets.iter().any(|a| a.id == "music"));
        assert!(info.assets.iter().any(|a| a.id == "cover"));
        assert_eq!(info.default_asset_ids(), vec!["video"]);
    }

    #[test]
    fn parses_note_without_dropping_first_image() {
        let item = item_from_share_html(NOTE_HTML).unwrap();
        let info = parse_item(&item, "7421234567890123457", "x").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        let images: Vec<_> = info.assets.iter().filter(|a| a.id.starts_with("image-")).collect();
        assert_eq!(images.len(), 3);
        assert!(images[0].url.contains("img-1"), "first image must be kept: {}", images[0].url);
        assert!(images[0].url.contains(".jpeg"), "prefers jpeg over webp: {}", images[0].url);
        assert_eq!(info.default_asset_ids(), vec!["image-0", "image-1", "image-2"]);
    }

    #[test]
    fn deleted_item_reports_reason() {
        let err = item_from_share_html(DELETED_HTML).unwrap_err();
        assert!(err.to_string().contains("作品已删除"), "{err}");
    }

    #[test]
    fn missing_router_data_is_a_clear_error() {
        let err = item_from_share_html("<html></html>").unwrap_err();
        assert!(err.to_string().contains("页面结构已变化"));
    }
}
