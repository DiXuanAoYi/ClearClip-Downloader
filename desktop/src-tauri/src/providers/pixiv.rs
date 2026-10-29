//! Pixiv：插画 / 漫画（多页原图）和动图（ugoira，下载帧压缩包后用 ffmpeg 合成视频）。
//!
//! 接口：`/ajax/illust/{id}`（作品信息）、`/pages`（每页原图）、`/ugoira_meta`（动图帧）。
//! R-18 作品需要登录（在设置中添加 Pixiv 账号）。图片服务器 i.pximg.net 要求 Referer。

use std::sync::LazyLock;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use super::{str_at, u32_at, Ctx, Provider, DESKTOP_UA};
use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind};

pub struct Pixiv;

const REFERER: &str = "https://www.pixiv.net/";

static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:artworks/|illust_id=|/i/)(\d+)").unwrap());

pub fn illust_id(url: &str) -> Option<String> {
    ID_RE.captures(url).map(|c| c[1].to_string())
}

#[async_trait]
impl Provider for Pixiv {
    fn id(&self) -> &'static str {
        "pixiv"
    }

    fn name(&self) -> &'static str {
        "Pixiv"
    }

    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| h == "pixiv.net" || h.ends_with(".pixiv.net"))
    }

    fn referer(&self) -> &'static str {
        REFERER
    }

    fn login_url(&self) -> &'static str {
        "https://accounts.pixiv.net/login"
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        // 内置解析器只处理单个作品；用户主页、收藏等交给 yt-dlp
        let id = illust_id(url).ok_or_else(|| AppError::parser("内置解析器只支持 Pixiv 作品链接（pixiv.net/artworks/数字）。"))?;
        let body = get_body(ctx, &format!("https://www.pixiv.net/ajax/illust/{id}")).await?;
        let kind = body.get("illustType").and_then(Value::as_u64).unwrap_or(0);
        let pages = if kind == 2 {
            None
        } else if body.get("pageCount").and_then(Value::as_u64).unwrap_or(1) > 1 {
            Some(get_body(ctx, &format!("https://www.pixiv.net/ajax/illust/{id}/pages")).await?)
        } else {
            None
        };
        let ugoira = if kind == 2 { Some(get_body(ctx, &format!("https://www.pixiv.net/ajax/illust/{id}/ugoira_meta")).await?) } else { None };
        parse(&body, pages.as_ref(), ugoira.as_ref(), url)
    }
}

pub(crate) async fn get_body(ctx: &Ctx<'_>, api: &str) -> AppResult<Value> {
    let mut req = ctx.get(api).header("User-Agent", DESKTOP_UA).header("Referer", REFERER).header("Accept", "application/json");
    if let Some(c) = ctx.cookie(api) {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let text = resp.text().await?;
    ctx.record("pixiv", "ajax", api, &text);
    let v: Value = serde_json::from_str(&text).map_err(|_| AppError::from_status(status, "Pixiv 接口"))?;
    body_of(v, ctx.cookie(api).is_some())
}

/// Pixiv 接口格式 `{error, message, body}`。
pub fn body_of(v: Value, logged_in: bool) -> AppResult<Value> {
    if v.get("error").and_then(Value::as_bool).unwrap_or(true) {
        let msg = str_at(&v, "/message").unwrap_or("未知错误").to_string();
        let text = format!("Pixiv：{msg}");
        let lower = msg.to_lowercase();
        // R-18 作品未登录时返回“尚无权限浏览该作品”
        if !logged_in && (msg.contains("权限") || msg.contains("権限") || lower.contains("permission") || msg.contains("登录")) {
            return Err(AppError::need_login(format!("{text}。R-18 作品需要登录，请在“设置 → 账号与 Cookie”中添加 Pixiv 账号。")));
        }
        if msg.contains("删除") || msg.contains("削除") || lower.contains("deleted") || msg.contains("不存在") {
            return Err(AppError::not_found(text));
        }
        return Err(AppError::classify(text));
    }
    v.get("body").cloned().ok_or_else(|| AppError::parser("Pixiv 接口没有返回数据"))
}

fn pximg_ext(url: &str) -> String {
    url.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("jpg").to_ascii_lowercase()
}

/// 合并作品信息、分页和动图信息。
pub fn parse(body: &Value, pages: Option<&Value>, ugoira: Option<&Value>, source_url: &str) -> AppResult<MediaInfo> {
    let id = str_at(body, "/illustId").or_else(|| str_at(body, "/id")).unwrap_or_default().to_string();
    let title = str_at(body, "/illustTitle").or_else(|| str_at(body, "/title")).unwrap_or("Pixiv 作品").to_string();
    let author = str_at(body, "/userName").unwrap_or_default().to_string();
    let published_at = str_at(body, "/createDate").and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()).map(|d| d.timestamp());
    let (width, height) = (u32_at(body, "/width"), u32_at(body, "/height"));
    let cover = str_at(body, "/urls/regular").or_else(|| str_at(body, "/urls/small")).map(String::from);
    let headers = vec![("Referer".to_string(), REFERER.to_string())];

    let mut assets: Vec<Asset> = vec![];
    let kind;
    if let Some(u) = ugoira {
        kind = MediaKind::Video;
        let zip = str_at(u, "/originalSrc").or_else(|| str_at(u, "/src")).ok_or_else(|| AppError::parser("Pixiv 动图缺少帧压缩包地址"))?;
        let frames =
            u.get("frames").cloned().filter(|f| f.as_array().is_some_and(|a| !a.is_empty())).ok_or_else(|| AppError::parser("Pixiv 动图缺少帧信息"))?;
        let mut a = Asset::base("ugoira", AssetKind::Video, zip.to_string(), "动图（合成为 MP4）", "mp4");
        a.width = width;
        a.height = height;
        a.headers = headers.clone();
        a.extra = Some(serde_json::json!({ "ugoira": frames }));
        assets.push(a);
        let mut z = Asset::base("ugoira-zip", AssetKind::Image, zip.to_string(), "原始帧压缩包 ZIP", "zip");
        z.index = Some(0);
        z.headers = headers.clone();
        assets.push(z);
    } else {
        kind = MediaKind::Images;
        let list: Vec<(String, Option<u32>, Option<u32>)> = match pages.and_then(Value::as_array) {
            Some(p) if !p.is_empty() => {
                p.iter().filter_map(|pg| Some((str_at(pg, "/urls/original")?.to_string(), u32_at(pg, "/width"), u32_at(pg, "/height")))).collect()
            }
            _ => str_at(body, "/urls/original").map(|u| vec![(u.to_string(), width, height)]).unwrap_or_default(),
        };
        if list.is_empty() {
            return Err(AppError::need_login("Pixiv 没有返回原图地址，可能需要登录。"));
        }
        for (i, (url, w, h)) in list.into_iter().enumerate() {
            let mut a = Asset::image(i, url.clone(), w, h);
            a.ext = pximg_ext(&url);
            a.headers = headers.clone();
            assets.push(a);
        }
    }

    Ok(MediaInfo {
        platform: "pixiv".into(),
        platform_name: "Pixiv".into(),
        id,
        source_url: source_url.to_string(),
        title,
        author,
        cover,
        duration_ms: None,
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

/// 动图的帧列表（文件名、毫秒）。
pub fn ugoira_frames(asset: &Asset) -> Option<Vec<(String, u64)>> {
    let frames = asset.extra.as_ref()?.get("ugoira")?.as_array()?;
    let list: Vec<(String, u64)> =
        frames.iter().filter_map(|f| Some((f.get("file")?.as_str()?.to_string(), f.get("delay").and_then(Value::as_u64).unwrap_or(100)))).collect();
    (!list.is_empty()).then_some(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(illust_id("https://www.pixiv.net/artworks/123456").as_deref(), Some("123456"));
        assert_eq!(illust_id("https://www.pixiv.net/en/artworks/42?x=1").as_deref(), Some("42"));
        assert_eq!(illust_id("https://www.pixiv.net/member_illust.php?mode=medium&illust_id=77").as_deref(), Some("77"));
        assert_eq!(illust_id("https://www.pixiv.net/users/1"), None);
    }

    #[test]
    fn manga_pages() {
        let body = serde_json::json!({"illustId": "100", "illustTitle": "漫画", "userName": "画师", "illustType": 1, "pageCount": 2,
            "createDate": "2024-01-02T03:04:05+00:00", "width": 1000, "height": 1400,
            "urls": {"original": "https://i.pximg.net/img-original/img/2024/01/02/12/00/00/100_p0.png", "regular": "https://i.pximg.net/r.jpg"}});
        let pages = serde_json::json!([
            {"urls": {"original": "https://i.pximg.net/img-original/img/x/100_p0.png"}, "width": 1000, "height": 1400},
            {"urls": {"original": "https://i.pximg.net/img-original/img/x/100_p1.jpg"}, "width": 1000, "height": 1400}]);
        let info = parse(&body, Some(&pages), None, "u").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        assert_eq!(info.assets.len(), 2);
        assert_eq!(info.assets[0].ext, "png");
        assert_eq!(info.assets[1].ext, "jpg");
        assert!(info.assets[0].headers.iter().any(|(k, v)| k == "Referer" && v == REFERER));
        assert_eq!(info.published_at, Some(1704164645));
        assert_eq!(info.default_asset_ids(), vec!["image-0", "image-1"]);
    }

    #[test]
    fn ugoira() {
        let body = serde_json::json!({"illustId": "200", "illustTitle": "动图", "illustType": 2, "width": 600, "height": 600, "urls": {}});
        let meta = serde_json::json!({"originalSrc": "https://i.pximg.net/img-zip-ugoira/img/200_ugoira1920x1080.zip",
            "frames": [{"file": "000000.jpg", "delay": 80}, {"file": "000001.jpg", "delay": 120}]});
        let info = parse(&body, None, Some(&meta), "u").unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        assert_eq!(info.default_asset_ids(), vec!["ugoira"]);
        assert_eq!(ugoira_frames(&info.assets[0]).unwrap(), vec![("000000.jpg".to_string(), 80), ("000001.jpg".to_string(), 120)]);
    }

    #[test]
    fn errors() {
        let v = serde_json::json!({"error": true, "message": "尚无权限浏览该作品", "body": []});
        assert_eq!(body_of(v.clone(), false).unwrap_err().kind, crate::error::ErrorKind::NeedLogin);
        let v = serde_json::json!({"error": true, "message": "该作品已被删除，或作品ID不存在。", "body": []});
        assert_eq!(body_of(v, true).unwrap_err().kind, crate::error::ErrorKind::NotFound);
    }
}
