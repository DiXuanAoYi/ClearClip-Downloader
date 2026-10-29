//! 快手：短链跳转到移动端分享页，读取页面里的 `window.INIT_STATE`；
//! 取不到时回退到 `rest/wd/photo/info` 接口。
//!
//! 优先使用设置里的 Cookie，没有时随机生成 `did` Cookie。

use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use url::Url;

use super::{str_at, u32_at, u64_at, Ctx, Provider};
use crate::model::{AppError, AppResult, Asset, MediaInfo, MediaKind};

pub struct Kuaishou;

static PHOTO_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"photoId=([\w-]+)").unwrap());
static PATH_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/(?:fw/photo|fw/long-video|photo|short-video|long-video)/([\w-]+)").unwrap());

const DEFAULT_CDN: &str = "p2.a.yximgs.com";

#[async_trait]
impl Provider for Kuaishou {
    fn id(&self) -> &'static str {
        "kuaishou"
    }

    fn name(&self) -> &'static str {
        "快手"
    }

    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| {
            h == "kuaishou.com" || h.ends_with(".kuaishou.com") || h.ends_with("chenzhongtech.com") || h.ends_with("gifshow.com") || h.ends_with("kwai.com")
        })
    }

    fn referer(&self) -> &'static str {
        "https://www.kuaishou.com/"
    }

    fn login_url(&self) -> &'static str {
        "https://www.kuaishou.com/"
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        let cookie = ctx.cookie("https://www.kuaishou.com/").unwrap_or_else(random_did_cookie);

        let resp = ctx.get(url).header("Cookie", &cookie).send().await?;
        let final_url = resp.url().to_string();
        let html = resp.text().await.unwrap_or_default();
        ctx.record("kuaishou", "share-page", &final_url, &html);
        let photo_id = photo_id_from_url(&final_url).or_else(|| photo_id_from_url(url));

        if let Some(state) = super::extract_json_after(&html, "window.INIT_STATE") {
            if let Some((photo, atlas)) = find_photo(&state) {
                let id = photo_id.clone().or_else(|| str_at(photo, "/photoId").map(String::from)).unwrap_or_default();
                return parse_photo(photo, atlas, &id, url);
            }
        }

        // 回退：移动端接口
        let id = photo_id.ok_or_else(|| AppError::not_found("没能从链接里识别出快手作品 ID，链接可能已失效。"))?;
        let body = json!({ "photoId": id, "isLongVideo": false });
        let resp = ctx
            .post("https://v.m.chenzhongtech.com/rest/wd/photo/info?kpn=KUAISHOU&captchaToken=")
            .header("Cookie", &cookie)
            .header("Referer", &final_url)
            .json(&body)
            .send()
            .await?;
        let data: Value = resp
            .json()
            .await
            .map_err(|_| AppError::new(crate::error::ErrorKind::RateLimited, "快手接口返回的不是有效数据，可能触发了风控。请在设置中登录快手后重试。"))?;
        if let Some(photo) = data.get("photo").filter(|p| p.is_object()) {
            return parse_photo(photo, data.get("atlas"), &id, url);
        }
        let reason = str_at(&data, "/error_msg").unwrap_or("快手没有返回作品信息，可能需要登录或作品已删除。");
        Err(AppError::msg(reason.to_string()))
    }
}

pub fn photo_id_from_url(url: &str) -> Option<String> {
    PHOTO_ID_RE.captures(url).or_else(|| PATH_ID_RE.captures(url)).map(|c| c[1].to_string())
}

fn random_did_cookie() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    // 32 位十六进制，格式与网页端 did 一致
    let a = nanos.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    format!("did=web_{:032x}; didv={}", a, nanos / 1_000_000)
}

/// 在 INIT_STATE 里递归查找包含作品信息的 `photo` 对象，同时返回同级的 `atlas`（如果有）。
pub fn find_photo(v: &Value) -> Option<(&Value, Option<&Value>)> {
    match v {
        Value::Object(map) => {
            if let Some(photo) = map.get("photo").filter(|p| is_photo(p)) {
                return Some((photo, map.get("atlas").filter(|a| a.is_object())));
            }
            map.values().find_map(find_photo)
        }
        Value::Array(arr) => arr.iter().find_map(find_photo),
        _ => None,
    }
}

fn is_photo(p: &Value) -> bool {
    p.is_object() && (p.get("mainMvUrls").is_some() || p.pointer("/ext_params/atlas").is_some() || p.get("photoUrl").is_some())
}

/// 把快手作品 JSON 转成统一结构。
pub fn parse_photo(photo: &Value, atlas: Option<&Value>, id: &str, source_url: &str) -> AppResult<MediaInfo> {
    let title = str_at(photo, "/caption").unwrap_or("快手作品").trim().to_string();
    let author = str_at(photo, "/userName").unwrap_or("未知作者").to_string();
    let cover = str_at(photo, "/coverUrls/0/url").or_else(|| str_at(photo, "/webpCoverUrls/0/url")).map(String::from);
    let published_at = u64_at(photo, "/timestamp").map(|ms| (ms / 1000) as i64);

    let atlas = photo.pointer("/ext_params/atlas").filter(|a| a.get("list").is_some()).or(atlas.filter(|a| a.get("list").is_some()));

    let mut assets = Vec::new();
    let (kind, width, height, duration_ms) = if let Some(atlas) = atlas {
        let cdn = str_at(atlas, "/cdn/0").or_else(|| str_at(atlas, "/cdnList/0/cdn")).unwrap_or(DEFAULT_CDN);
        let list = atlas.get("list").and_then(Value::as_array).cloned().unwrap_or_default();
        let sizes = atlas.get("size").and_then(Value::as_array);
        for (i, path) in list.iter().filter_map(Value::as_str).enumerate() {
            let size = sizes.and_then(|s| s.get(i));
            let w = size.and_then(|s| u32_at(s, "/w"));
            let h = size.and_then(|s| u32_at(s, "/h"));
            assets.push(Asset::image(i, join_cdn(cdn, path), w, h));
        }
        if assets.is_empty() {
            return Err(AppError::msg("图集里没有可下载的图片。"));
        }
        if let Some(music) = str_at(atlas, "/music") {
            assets.push(Asset::audio(join_cdn(cdn, music)));
        }
        (MediaKind::Images, None, None, None)
    } else {
        let url = str_at(photo, "/mainMvUrls/0/url")
            .or_else(|| str_at(photo, "/photoUrl"))
            .ok_or_else(|| AppError::not_found("没有找到视频地址，作品可能已删除。"))?;
        let w = u32_at(photo, "/width");
        let h = u32_at(photo, "/height");
        assets.push(Asset::video(url.to_string(), w, h));
        if let Some(m) = str_at(photo, "/soundTrack/audioUrls/0/url").or_else(|| str_at(photo, "/music/audioUrls/0/url")) {
            assets.push(Asset::audio(m.to_string()));
        }
        (MediaKind::Video, w, h, u64_at(photo, "/duration").filter(|d| *d > 0))
    };

    if let Some(c) = &cover {
        assets.push(Asset::cover(c.clone()));
    }

    Ok(MediaInfo {
        platform: "kuaishou".into(),
        platform_name: "快手".into(),
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

fn join_cdn(cdn: &str, path: &str) -> String {
    if path.starts_with("http") {
        return path.to_string();
    }
    let cdn = cdn.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    format!("https://{cdn}/{}", path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO_HTML: &str = include_str!("../../tests/fixtures/kuaishou_video.html");
    const ATLAS_HTML: &str = include_str!("../../tests/fixtures/kuaishou_atlas.html");
    const REST_ATLAS: &str = include_str!("../../tests/fixtures/kuaishou_rest_atlas.json");

    #[test]
    fn photo_id_from_urls() {
        assert_eq!(photo_id_from_url("https://v.m.chenzhongtech.com/fw/photo/3xk2abcdef?fid=1&photoId=3xk2abcdef&cc=share").as_deref(), Some("3xk2abcdef"));
        assert_eq!(photo_id_from_url("https://www.kuaishou.com/short-video/3x9ab4cd").as_deref(), Some("3x9ab4cd"));
        assert_eq!(photo_id_from_url("https://v.kuaishou.com/abc"), None);
    }

    #[test]
    fn parses_video_from_init_state() {
        let state = super::super::extract_json_after(VIDEO_HTML, "window.INIT_STATE").unwrap();
        let (photo, atlas) = find_photo(&state).unwrap();
        let info = parse_photo(photo, atlas, "3xk2abcdef", "https://v.kuaishou.com/abc").unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        assert_eq!(info.author, "城市漫游者");
        assert_eq!(info.duration_ms, Some(31_500));
        assert_eq!(info.published_at, Some(1_727_856_000));
        assert!(info.assets[0].url.starts_with("https://v2.kwaicdn.com/"));
        assert!(info.assets.iter().any(|a| a.id == "cover"));
    }

    #[test]
    fn parses_atlas_from_init_state() {
        let state = super::super::extract_json_after(ATLAS_HTML, "window.INIT_STATE").unwrap();
        let (photo, atlas) = find_photo(&state).unwrap();
        let info = parse_photo(photo, atlas, "3xatlas", "x").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        let imgs: Vec<_> = info.assets.iter().filter(|a| a.id.starts_with("image-")).collect();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].url, "https://tx2.a.yximgs.com/ufile/atlas/a1.jpg");
        assert_eq!(imgs[0].width, Some(1080));
        let music = info.assets.iter().find(|a| a.id == "music").unwrap();
        assert_eq!(music.url, "https://tx2.a.yximgs.com/ufile/atlas/bgm.m4a");
        assert_eq!(music.ext, "m4a");
    }

    #[test]
    fn parses_rest_api_atlas() {
        let data: Value = serde_json::from_str(REST_ATLAS).unwrap();
        let info = parse_photo(&data["photo"], data.get("atlas"), "3xrest", "x").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        // 默认 CDN
        assert!(info.assets[0].url.starts_with("https://p2.a.yximgs.com/"), "{}", info.assets[0].url);
        assert_eq!(info.assets.iter().filter(|a| a.id.starts_with("image-")).count(), 3);
    }

    #[test]
    fn random_cookie_has_did() {
        let c = random_did_cookie();
        assert!(c.starts_with("did=web_") && c.contains("didv="));
    }
}
