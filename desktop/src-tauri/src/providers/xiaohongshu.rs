//! 小红书：短链跳转到笔记页，读取页面里的 `window.__INITIAL_STATE__`。
//!
//! 图片用 `urlDefault` 中的图片 token 拼出无水印原图地址；视频优先用 `originVideoKey`。
//! 网页版笔记链接通常带 `xsec_token`，缺少时可能需要在设置中登录小红书。

use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use super::{str_at, u32_at, u64_at, Ctx, Provider, DESKTOP_UA};
use crate::model::{AppError, AppResult, Asset, MediaInfo, MediaKind};

pub struct Xiaohongshu;

static NOTE_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/(?:explore|discovery/item|item)/([0-9a-f]{24})").unwrap());

#[async_trait]
impl Provider for Xiaohongshu {
    fn id(&self) -> &'static str {
        "xiaohongshu"
    }

    fn name(&self) -> &'static str {
        "小红书"
    }

    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| h == "xhslink.com" || h.ends_with(".xhslink.com") || h == "xiaohongshu.com" || h.ends_with(".xiaohongshu.com"))
    }

    fn referer(&self) -> &'static str {
        "https://www.xiaohongshu.com/"
    }

    fn login_url(&self) -> &'static str {
        "https://www.xiaohongshu.com/"
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        let mut req = ctx.get(url).header("User-Agent", DESKTOP_UA).header("Referer", "https://www.xiaohongshu.com/");
        if let Some(cookie) = ctx.cookie("https://www.xiaohongshu.com/") {
            req = req.header("Cookie", cookie);
        }
        let resp = req.send().await?;
        let final_url = resp.url().to_string();
        if final_url.contains("/404") || final_url.contains("website-login") {
            return Err(AppError::need_login("小红书要求登录或笔记不可见。请在设置中登录小红书，或使用带 xsec_token 的完整分享链接。"));
        }
        let html = resp.text().await?;
        ctx.record("xiaohongshu", "note-page", &final_url, &html);
        let id = note_id_from_url(&final_url).or_else(|| note_id_from_url(url));
        let note = note_from_html(&html, id.as_deref())?;
        let id = id.or_else(|| str_at(&note, "/noteId").map(String::from)).unwrap_or_default();
        parse_note(&note, &id, url)
    }
}

pub fn note_id_from_url(url: &str) -> Option<String> {
    NOTE_ID_RE.captures(url).map(|c| c[1].to_string())
}

/// 从页面 HTML 中取出笔记 JSON（`note.noteDetailMap[id].note`）。
pub fn note_from_html(html: &str, id: Option<&str>) -> AppResult<Value> {
    let state = super::extract_json_after(html, "window.__INITIAL_STATE__")
        .ok_or_else(|| AppError::parser("小红书页面结构已变化或需要登录，没有找到笔记数据。请在设置中登录小红书后重试。"))?;
    let map =
        state.pointer("/note/noteDetailMap").and_then(Value::as_object).ok_or_else(|| AppError::need_login("小红书页面里没有笔记信息，可能需要登录。"))?;
    let entry = id.and_then(|id| map.get(id)).or_else(|| map.values().find(|v| v.pointer("/note/noteId").is_some()));
    entry
        .and_then(|e| e.get("note"))
        .filter(|n| n.get("noteId").is_some() || n.get("imageList").is_some())
        .cloned()
        .ok_or_else(|| AppError::need_login("笔记不存在、已删除或需要登录才能查看。"))
}

pub fn parse_note(note: &Value, id: &str, source_url: &str) -> AppResult<MediaInfo> {
    let title = str_at(note, "/title").or_else(|| str_at(note, "/desc")).unwrap_or("小红书笔记").trim().to_string();
    let author = str_at(note, "/user/nickname").or_else(|| str_at(note, "/user/nickName")).unwrap_or("未知作者").to_string();
    let published_at = u64_at(note, "/time").map(|ms| (ms / 1000) as i64);
    let images = note.get("imageList").and_then(Value::as_array).cloned().unwrap_or_default();
    let cover = images.first().and_then(image_url);

    let mut assets = Vec::new();
    let is_video = str_at(note, "/type") == Some("video");
    let (kind, width, height, duration_ms) = if is_video {
        let url = video_url(note).ok_or_else(|| AppError::not_found("没有找到视频地址，笔记可能已删除。"))?;
        let stream = note.pointer("/video/media/stream/h264/0");
        let w = stream.and_then(|s| u32_at(s, "/width"));
        let h = stream.and_then(|s| u32_at(s, "/height"));
        let d = note.pointer("/video/capa/duration").and_then(Value::as_u64).map(|s| s * 1000);
        assets.push(Asset::video(url, w, h));
        (MediaKind::Video, w, h, d)
    } else {
        for (i, img) in images.iter().enumerate() {
            if let Some(u) = image_url(img) {
                assets.push(Asset::image(i, u, u32_at(img, "/width"), u32_at(img, "/height")));
            }
        }
        if assets.is_empty() {
            return Err(AppError::msg("笔记里没有可下载的图片。"));
        }
        (MediaKind::Images, None, None, None)
    };
    if let Some(c) = &cover {
        assets.push(Asset::cover(c.clone()));
    }

    Ok(MediaInfo {
        platform: "xiaohongshu".into(),
        platform_name: "小红书".into(),
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

fn video_url(note: &Value) -> Option<String> {
    if let Some(key) = str_at(note, "/video/consumer/originVideoKey") {
        return Some(format!("https://sns-video-bd.xhscdn.com/{key}"));
    }
    ["/video/media/stream/h264/0/masterUrl", "/video/media/stream/h265/0/masterUrl", "/video/media/stream/av1/0/masterUrl"]
        .iter()
        .find_map(|p| str_at(note, p))
        .map(String::from)
}

/// 无水印原图：取 `urlDefault` 路径中的图片 token（去掉日期、签名段和 `!` 后的样式参数）。
fn image_url(img: &Value) -> Option<String> {
    let raw = str_at(img, "/urlDefault")
        .or_else(|| str_at(img, "/url"))
        .or_else(|| img.get("infoList").and_then(Value::as_array).and_then(|l| l.iter().rev().find_map(|i| str_at(i, "/url"))))?;
    match image_token(raw) {
        Some(token) => Some(format!("https://ci.xiaohongshu.com/{token}?imageView2/format/png")),
        None => Some(raw.replace("http://", "https://")),
    }
}

pub fn image_token(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let segments: Vec<&str> = parsed.path_segments()?.collect();
    // 形如 /202410021234/0123abcd.../1040g2sg31...!nd_dft_wlteh_webp_3
    let tail = if segments.len() >= 3 && segments[0].len() >= 10 && segments[0].chars().all(|c| c.is_ascii_digit()) { &segments[2..] } else { &segments[..] };
    let token = tail.join("/");
    let token = token.split('!').next()?.trim_matches('/');
    (!token.is_empty()).then(|| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_HTML: &str = include_str!("../../tests/fixtures/xhs_note.html");
    const VIDEO_HTML: &str = include_str!("../../tests/fixtures/xhs_video.html");

    #[test]
    fn note_id_from_urls() {
        assert_eq!(note_id_from_url("https://www.xiaohongshu.com/explore/66fc0a1b000000002c02a1b2?xsec_token=AB").as_deref(), Some("66fc0a1b000000002c02a1b2"));
        assert_eq!(note_id_from_url("https://www.xiaohongshu.com/discovery/item/66fc0a1b000000002c02a1b2").as_deref(), Some("66fc0a1b000000002c02a1b2"));
        assert_eq!(note_id_from_url("http://xhslink.com/a/AbCdEf"), None);
    }

    #[test]
    fn image_token_strips_date_sign_and_style() {
        assert_eq!(
            image_token("http://sns-webpic-qc.xhscdn.com/202410021234/0a1b2c3d4e5f/1040g2sg31abc!nd_dft_wlteh_webp_3").as_deref(),
            Some("1040g2sg31abc")
        );
        assert_eq!(
            image_token("https://sns-webpic-qc.xhscdn.com/202410021234/0a1b/spectrum/1040g0k0abc!nd_dft_wgth_webp_3").as_deref(),
            Some("spectrum/1040g0k0abc")
        );
    }

    #[test]
    fn parses_image_note() {
        let note = note_from_html(IMAGE_HTML, Some("66fc0a1b000000002c02a1b2")).unwrap();
        let info = parse_note(&note, "66fc0a1b000000002c02a1b2", "x").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        assert_eq!(info.author, "走走停停");
        assert_eq!(info.published_at, Some(1_727_856_000));
        let imgs: Vec<_> = info.assets.iter().filter(|a| a.id.starts_with("image-")).collect();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].url, "https://ci.xiaohongshu.com/1040g2sg31first?imageView2/format/png");
        assert_eq!(imgs[0].ext, "png");
        assert_eq!(imgs[0].width, Some(1080));
    }

    #[test]
    fn parses_video_note_with_origin_key() {
        let note = note_from_html(VIDEO_HTML, None).unwrap();
        let info = parse_note(&note, "66fc0a1b000000002c02a1b3", "x").unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        assert_eq!(info.assets[0].url, "https://sns-video-bd.xhscdn.com/pre_post/1040g2t0origin");
        assert_eq!(info.duration_ms, Some(25_000));
        assert_eq!(info.width, Some(1080));
    }

    #[test]
    fn missing_state_is_clear_error() {
        assert!(note_from_html("<html></html>", None).unwrap_err().to_string().contains("登录"));
    }
}
