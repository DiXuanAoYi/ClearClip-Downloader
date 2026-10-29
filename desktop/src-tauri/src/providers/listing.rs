//! 订阅用的“列出最新条目”：B站 UP 主空间、Pixiv 画师用原生接口，其他交给 yt-dlp 平铺列表。
//! 抖音用户主页需要隐藏浏览器窗口，在 `subs.rs` 里处理。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{bilibili, pixiv, str_at, u64_at, ytdlp, Ctx};
use crate::model::{AppError, AppResult, MediaKind};

/// 列表里的一条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubEntry {
    pub id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub published_at: Option<i64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResult {
    pub title: String,
    pub platform: String,
    pub platform_name: String,
    pub avatar: Option<String>,
    /// 最新的在前
    pub entries: Vec<SubEntry>,
}

static BILI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"space\.bilibili\.com/(\d+)").unwrap());
static PIXIV_USER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"pixiv\.net/(?:en/)?users/(\d+)").unwrap());
static DOUYIN_USER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"douyin\.com/user/([A-Za-z0-9_\-]+)").unwrap());

pub fn is_douyin_user(url: &str) -> bool {
    DOUYIN_USER.is_match(url)
}

/// YouTube 频道首页没有指定标签时，取“视频”标签（否则 yt-dlp 返回的是标签列表）。
pub fn normalize_url(url: &str) -> String {
    let Ok(mut u) = Url::parse(url) else { return url.to_string() };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    if host.ends_with("youtube.com") {
        let segs: Vec<String> = u.path_segments().map(|s| s.filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default();
        let is_channel = match segs.as_slice() {
            [h] if h.starts_with('@') => true,
            [k, _] if matches!(k.as_str(), "channel" | "c" | "user") => true,
            _ => false,
        };
        if is_channel {
            let p = format!("/{}/videos", segs.join("/"));
            u.set_path(&p);
            return u.to_string();
        }
    }
    url.to_string()
}

/// 列出最新的条目（最多 `limit` 条）。
pub async fn list(ctx: &Ctx<'_>, url: &str, limit: usize) -> AppResult<ListResult> {
    if let Some(c) = BILI_SPACE.captures(url) {
        match bili_space(ctx, &c[1], limit).await {
            Ok(r) => return Ok(r),
            Err(e) if ctx.ytdlp.is_some() => log::info!("bilibili space api failed, falling back to yt-dlp: {e}"),
            Err(e) => return Err(e),
        }
    }
    if let Some(c) = PIXIV_USER.captures(url) {
        return pixiv_user(ctx, &c[1], limit).await;
    }
    if super::rss::looks_like_feed_url(url) {
        match super::rss::list(ctx, url, limit).await {
            Ok(r) => return Ok(r),
            Err(e) => log::info!("not a usable feed ({e}), listing with yt-dlp"),
        }
    }
    ytdlp_list(ctx, &normalize_url(url), limit).await
}

async fn ytdlp_list(ctx: &Ctx<'_>, url: &str, limit: usize) -> AppResult<ListResult> {
    let (mut cmd, cookie_file) = ytdlp::command(ctx, url)?;
    cmd.args(["-J", "--flat-playlist", "--playlist-end", &limit.to_string(), "--"])
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = tokio::time::timeout(std::time::Duration::from_secs(180), cmd.output())
        .await
        .map_err(|_| AppError::new(crate::error::ErrorKind::Network, "yt-dlp 获取列表超时"))?
        .map_err(|e| AppError::new(crate::error::ErrorKind::NeedUpdate, format!("无法运行 yt-dlp：{e}")))?;
    if let Some(cf) = &cookie_file {
        cf.merge_back(ctx.cookies);
    }
    if !out.status.success() {
        return Err(ytdlp::classify_stderr(&String::from_utf8_lossy(&out.stderr)));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| AppError::parser(format!("yt-dlp 输出无法解析：{e}")))?;
    from_ytdlp(&v, url)
}

/// yt-dlp 平铺列表 → 订阅条目。
pub fn from_ytdlp(v: &Value, url: &str) -> AppResult<ListResult> {
    let info = ytdlp::from_json(v, url)?;
    if info.kind != MediaKind::Playlist {
        return Err(AppError::invalid("这个链接是单个作品，不是可以订阅的列表。请粘贴频道、UP 主空间、播放列表或合集的链接。"));
    }
    let raw = v.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
    let entries = info
        .entries
        .iter()
        .map(|e| {
            let src = raw.iter().find(|r| str_at(r, "/id") == Some(e.id.as_str()) || str_at(r, "/url").is_some_and(|u| u.starts_with(&e.url)));
            let published_at = src.and_then(|r| u64_at(r, "/timestamp").map(|t| t as i64).or_else(|| str_at(r, "/upload_date").and_then(parse_ymd)));
            SubEntry { id: e.id.clone(), title: e.title.clone(), url: e.url.clone(), thumbnail: e.thumbnail.clone(), published_at, duration_ms: e.duration_ms }
        })
        .collect();
    let avatar = v.get("thumbnails").and_then(Value::as_array).and_then(|a| a.iter().find_map(|t| str_at(t, "/url").map(String::from)));
    let title = if info.author.is_empty() || info.title.contains(&info.author) { info.title.clone() } else { format!("{} - {}", info.author, info.title) };
    Ok(ListResult { title, platform: info.platform, platform_name: info.platform_name, avatar, entries })
}

fn parse_ymd(s: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(s, "%Y%m%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(|d| d.and_utc().timestamp())
}

fn parse_length(s: &str) -> Option<u64> {
    let parts: Vec<u64> = s.split(':').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    Some(parts.iter().fold(0, |acc, p| acc * 60 + p) * 1000)
}

// 部分接口在缺少这些浏览器指纹参数时直接触发风控（-352）
const DM_PARAMS: [(&str, &str); 3] = [
    ("dm_img_list", "[]"),
    ("dm_img_str", "V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ"),
    (
        "dm_cover_img_str",
        "QU5HTEUgKEludGVsLCBJbnRlbChSKSBVSEQgR3JhcGhpY3MgNjMwICgweDAwMDAzRTlCKSBEaXJlY3QzRDExIHZzXzVfMCBwc181XzAsIEQzRDExKUdvb2dsZSBJbmMuIChJbnRlbC",
    ),
];

async fn bili_space(ctx: &Ctx<'_>, mid: &str, limit: usize) -> AppResult<ListResult> {
    let cookie = ctx.cookie("https://api.bilibili.com/");
    let keys = bilibili::wbi_keys(ctx, cookie.as_deref()).await?;
    let mut params: Vec<(String, String)> = vec![
        ("mid".into(), mid.into()),
        ("ps".into(), limit.clamp(1, 50).to_string()),
        ("pn".into(), "1".into()),
        ("order".into(), "pubdate".into()),
        ("platform".into(), "web".into()),
    ];
    params.extend(DM_PARAMS.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    let q = bilibili::wbi_sign(params, &keys, chrono::Utc::now().timestamp());
    let v = bilibili::get_json(ctx, &format!("https://api.bilibili.com/x/space/wbi/arc/search?{q}"), cookie.as_deref()).await?;
    let data = bilibili::api_data(&v)?;
    parse_bili_space(data, mid)
}

pub fn parse_bili_space(data: &Value, mid: &str) -> AppResult<ListResult> {
    let list = data.pointer("/list/vlist").and_then(Value::as_array).cloned().unwrap_or_default();
    let author = list.first().and_then(|x| str_at(x, "/author")).unwrap_or("UP 主").to_string();
    let entries = list
        .iter()
        .filter_map(|x| {
            let bvid = str_at(x, "/bvid")?;
            Some(SubEntry {
                id: bvid.to_string(),
                title: str_at(x, "/title").unwrap_or(bvid).to_string(),
                url: format!("https://www.bilibili.com/video/{bvid}"),
                thumbnail: str_at(x, "/pic").map(|p| p.replace("http://", "https://")),
                published_at: u64_at(x, "/created").map(|t| t as i64),
                duration_ms: str_at(x, "/length").and_then(parse_length),
            })
        })
        .collect();
    Ok(ListResult { title: format!("{author} 的投稿"), platform: "bilibili".into(), platform_name: "B站".into(), avatar: None, entries }).map(|mut r| {
        if r.entries.is_empty() {
            r.title = format!("UP 主 {mid}");
        }
        r
    })
}

async fn pixiv_user(ctx: &Ctx<'_>, uid: &str, limit: usize) -> AppResult<ListResult> {
    let profile = pixiv::get_body(ctx, &format!("https://www.pixiv.net/ajax/user/{uid}/profile/all")).await?;
    let user = pixiv::get_body(ctx, &format!("https://www.pixiv.net/ajax/user/{uid}?full=0")).await.ok();
    Ok(parse_pixiv_profile(&profile, user.as_ref(), uid, limit))
}

pub fn parse_pixiv_profile(profile: &Value, user: Option<&Value>, uid: &str, limit: usize) -> ListResult {
    let mut ids: Vec<u64> = ["illusts", "manga"]
        .iter()
        .filter_map(|k| profile.get(*k).and_then(Value::as_object))
        .flat_map(|m| m.keys().filter_map(|k| k.parse::<u64>().ok()).collect::<Vec<_>>())
        .collect();
    // 作品 ID 递增，按 ID 倒序即最新的在前
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids.dedup();
    let name = user.and_then(|u| str_at(u, "/name")).unwrap_or(uid).to_string();
    ListResult {
        title: format!("{name} 的作品"),
        platform: "pixiv".into(),
        platform_name: "Pixiv".into(),
        avatar: user.and_then(|u| str_at(u, "/imageBig").or_else(|| str_at(u, "/image"))).map(String::from),
        entries: ids
            .into_iter()
            .take(limit)
            .map(|id| SubEntry {
                id: id.to_string(),
                title: format!("作品 {id}"),
                url: format!("https://www.pixiv.net/artworks/{id}"),
                thumbnail: None,
                published_at: None,
                duration_ms: None,
            })
            .collect(),
    }
}

/// 从抖音用户主页（隐藏窗口中渲染后的页面）收集的作品链接。
pub fn douyin_entries(ids: &[String]) -> Vec<SubEntry> {
    ids.iter()
        .map(|id| SubEntry {
            id: id.clone(),
            title: format!("作品 {id}"),
            url: format!("https://www.douyin.com/video/{id}"),
            thumbnail: None,
            published_at: None,
            duration_ms: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_channel_urls() {
        assert_eq!(normalize_url("https://www.youtube.com/@abc"), "https://www.youtube.com/@abc/videos");
        assert_eq!(normalize_url("https://www.youtube.com/channel/UC123"), "https://www.youtube.com/channel/UC123/videos");
        assert_eq!(normalize_url("https://www.youtube.com/@abc/shorts"), "https://www.youtube.com/@abc/shorts");
        assert_eq!(normalize_url("https://www.youtube.com/playlist?list=PL1"), "https://www.youtube.com/playlist?list=PL1");
        assert!(is_douyin_user("https://www.douyin.com/user/MS4wLjABAAAA-x_y?from=1"));
    }

    #[test]
    fn bili_space_list() {
        let data = serde_json::json!({"list": {"vlist": [
            {"bvid": "BV1aa", "title": "新视频", "author": "老王", "created": 1700000000, "length": "12:05", "pic": "http://i0.hdslb.com/a.jpg"},
            {"bvid": "BV1bb", "title": "旧视频", "author": "老王", "created": 1690000000, "length": "1:02:03"}]}});
        let r = parse_bili_space(&data, "1").unwrap();
        assert_eq!(r.title, "老王 的投稿");
        assert_eq!(r.entries.len(), 2);
        assert_eq!(r.entries[0].url, "https://www.bilibili.com/video/BV1aa");
        assert_eq!(r.entries[0].duration_ms, Some(725_000));
        assert_eq!(r.entries[1].duration_ms, Some(3_723_000));
        assert_eq!(r.entries[0].thumbnail.as_deref(), Some("https://i0.hdslb.com/a.jpg"));
    }

    #[test]
    fn pixiv_profile() {
        let profile = serde_json::json!({"illusts": {"100": null, "300": null}, "manga": {"200": null}});
        let user = serde_json::json!({"name": "画师", "imageBig": "https://i.pximg.net/a.png"});
        let r = parse_pixiv_profile(&profile, Some(&user), "9", 2);
        assert_eq!(r.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["300", "200"]);
        assert_eq!(r.title, "画师 的作品");
    }

    #[test]
    fn ytdlp_flat_list() {
        let v = serde_json::json!({"_type": "playlist", "id": "UC1", "title": "Ch - Videos", "uploader": "Ch", "extractor_key": "YoutubeTab",
            "entries": [{"id": "a", "title": "A", "url": "https://www.youtube.com/watch?v=a", "duration": 61, "upload_date": "20240102"},
                        {"id": "b", "title": "B", "url": "https://www.youtube.com/watch?v=b", "timestamp": 1700000000}]});
        let r = from_ytdlp(&v, "https://www.youtube.com/@ch/videos").unwrap();
        assert_eq!(r.title, "Ch - Videos");
        assert_eq!(r.entries[0].published_at, Some(1704153600));
        assert_eq!(r.entries[1].published_at, Some(1700000000));
        assert_eq!(r.entries[0].duration_ms, Some(61_000));
        let single = serde_json::json!({"id": "x", "title": "one", "url": "https://a/b.mp4", "ext": "mp4"});
        assert!(from_ytdlp(&single, "https://a/").is_err());
    }
}
