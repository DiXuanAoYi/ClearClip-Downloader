//! 远程解析：调用用户自己部署的解析接口（`?url=` 返回 `{code, message}` JSON），作为本地解析的备用通道。

use serde_json::Value;
use url::Url;

use super::Ctx;
use crate::model::{AppError, AppResult, Asset, MediaInfo, MediaKind};

pub async fn resolve(ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
    let endpoint = ctx.settings.remote_endpoint.trim();
    let mut api = Url::parse(endpoint).map_err(|_| AppError::invalid("远程 API 地址格式不正确，应类似 https://example.com/api"))?;
    api.query_pairs_mut().append_pair("url", url);
    let resp = ctx.get(api).send().await?;
    let text = resp.text().await?;
    let data: Value = serde_json::from_str(&text).map_err(|_| AppError::msg("远程 API 返回的不是 JSON，请检查接口地址是否正确。"))?;
    parse_response(&data, url)
}

/// 解析远程接口的 `{code, message}` 结构。
pub fn parse_response(data: &Value, source_url: &str) -> AppResult<MediaInfo> {
    let code = data.get("code").and_then(Value::as_i64).unwrap_or(0);
    let message = data.get("message").cloned().unwrap_or(Value::Null);
    if code != 200 {
        let reason = message.as_str().unwrap_or("远程 API 解析失败。");
        return Err(AppError::classify(format!("远程 API：{reason}")));
    }

    let title = message.get("nickname").and_then(Value::as_str).unwrap_or("作品").trim().to_string();
    let is_photo = message.get("type").and_then(Value::as_str) == Some("photo");
    let music = message.get("music").and_then(Value::as_str).filter(|s| s.starts_with("http")).map(String::from);

    // 远程接口的图集可能是以数字为键的对象而不是数组。
    let urls: Vec<(usize, String)> = match message.get("video_url") {
        Some(Value::String(s)) => vec![(0, s.clone())],
        Some(Value::Array(a)) => a.iter().enumerate().filter_map(|(i, v)| v.as_str().map(|s| (i, s.to_string()))).collect(),
        Some(Value::Object(m)) => {
            let mut v: Vec<(usize, String)> = m.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))).collect();
            v.sort_by_key(|(i, _)| *i);
            v
        }
        _ => vec![],
    };
    if urls.is_empty() {
        return Err(AppError::msg("远程 API 没有返回下载地址。"));
    }

    let mut assets = Vec::new();
    let kind = if is_photo {
        for (n, (_, u)) in urls.iter().enumerate() {
            assets.push(Asset::image(n, u.clone(), None, None));
        }
        MediaKind::Images
    } else {
        assets.push(Asset::video(urls[0].1.clone(), None, None));
        MediaKind::Video
    };
    if let Some(m) = music {
        assets.push(Asset::audio(m));
    }

    let platform =
        Url::parse(source_url).ok().and_then(|u| super::all().iter().find(|p| p.matches(&u)).map(|p| (p.id(), p.name()))).unwrap_or(("remote", "远程"));

    Ok(MediaInfo {
        platform: platform.0.into(),
        platform_name: platform.1.into(),
        id: stable_id(source_url),
        source_url: source_url.to_string(),
        title,
        author: String::new(),
        cover: if is_photo { urls.first().map(|(_, u)| u.clone()) } else { None },
        duration_ms: None,
        kind,
        width: None,
        height: None,
        published_at: None,
        assets,
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("remote".into()),
    })
}

/// 远程接口不返回作品 ID，用链接生成一个稳定的 ID 供去重。
fn stable_id(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("r{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_video_response() {
        let data = json!({"code":200,"message":{"nickname":"标题","video_url":"https://cdn/x.mp4","music":"https://cdn/m.mp3","type":"movie"}});
        let info = parse_response(&data, "https://v.douyin.com/abc/").unwrap();
        assert_eq!(info.kind, MediaKind::Video);
        assert_eq!(info.platform, "douyin");
        assert_eq!(info.assets.len(), 2);
    }

    #[test]
    fn parses_legacy_object_shaped_image_list_in_order() {
        let data = json!({"code":200,"message":{"nickname":"图集","video_url":{"1":"https://i/1.jpg","2":"https://i/2.jpg","0":"https://i/0.jpg"},"music":"v0200","type":"photo"}});
        let info = parse_response(&data, "https://v.douyin.com/abc/").unwrap();
        assert_eq!(info.kind, MediaKind::Images);
        let urls: Vec<_> = info.assets.iter().map(|a| a.url.as_str()).collect();
        assert_eq!(urls, vec!["https://i/0.jpg", "https://i/1.jpg", "https://i/2.jpg"]);
    }

    #[test]
    fn surfaces_error_message() {
        let data = json!({"code":500,"message":"抱歉，此url暂不支持！"});
        let err = parse_response(&data, "x").unwrap_err();
        assert!(err.to_string().contains("暂不支持"));
    }
}
