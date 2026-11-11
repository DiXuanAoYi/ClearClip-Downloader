//! B站的“列表类”页面：创作者首页（全部投稿）、合集、视频列表（系列）、收藏夹、稍后再看，以及创作者的“合集和列表”总览。
//! 解析成一个播放列表（kind = playlist），条目是各个稿件，勾选后逐条解析、批量下载。

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use url::Url;

use super::bilibili::{api_data, get_json, wbi_keys, wbi_sign};
use super::listing::DM_PARAMS;
use super::{str_at, u64_at, Ctx};
use crate::model::{AppError, AppResult, MediaInfo, MediaKind, PlaylistEntry};

/// 一次最多取这么多条，免得一个几千条的空间把接口刷到风控。
const MAX_ENTRIES: usize = 1000;

static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/(\d+)(?:/(.*))?$").unwrap());
static ML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/ml(\d+)").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// 创作者首页：全部投稿
    Uploads { mid: String },
    /// “合集和列表”总览：这个创作者的所有合集和系列
    SpaceLists { mid: String },
    /// 合集（season）
    Season { mid: String, id: String },
    /// 视频列表（series）
    Series { mid: String, id: String },
    /// 收藏夹
    Fav { id: String },
    /// 创作者的所有公开收藏夹
    FavFolders { mid: String },
    /// 稍后再看（要登录）
    WatchLater,
}

fn query(u: &Url, key: &str) -> Option<String> {
    u.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

/// 判断链接是不是列表类页面；单个稿件、番剧等返回 None。
pub fn classify(url: &str) -> Option<Target> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let path = u.path().trim_end_matches('/').to_string();
    if host == "space.bilibili.com" {
        let c = SPACE.captures(&path)?;
        let mid = c[1].to_string();
        let rest = c.get(2).map(|m| m.as_str()).unwrap_or("");
        let seg: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        return Some(match seg.as_slice() {
            ["lists", id] => {
                if query(&u, "type").as_deref() == Some("series") {
                    Target::Series { mid, id: (*id).to_string() }
                } else {
                    Target::Season { mid, id: (*id).to_string() }
                }
            }
            ["lists"] => Target::SpaceLists { mid },
            ["channel", "collectiondetail"] => Target::Season { id: query(&u, "sid")?, mid },
            ["channel", "seriesdetail"] => Target::Series { id: query(&u, "sid")?, mid },
            ["channel", ..] => Target::SpaceLists { mid },
            ["favlist"] => match query(&u, "fid") {
                Some(id) => Target::Fav { id },
                None => Target::FavFolders { mid },
            },
            _ => Target::Uploads { mid },
        });
    }
    if host.ends_with("bilibili.com") {
        if path.ends_with("/watchlater") || path.ends_with("/list/watchlater") {
            return Some(Target::WatchLater);
        }
        if path.starts_with("/medialist/play/") || path.starts_with("/list/") {
            if let Some(c) = ML.captures(&path) {
                return Some(Target::Fav { id: c[1].to_string() });
            }
            // /list/{mid}?sid={系列}
            let mid = path.trim_start_matches("/list/").split('/').next().unwrap_or("").to_string();
            if mid.chars().all(|c| c.is_ascii_digit()) && !mid.is_empty() {
                return Some(match query(&u, "sid") {
                    Some(id) => Target::Series { mid, id },
                    None => Target::Uploads { mid },
                });
            }
        }
    }
    None
}

pub struct Page {
    pub entries: Vec<PlaylistEntry>,
    /// 接口说还有下一页
    pub more: bool,
}

fn entry(bvid: &str, title: &str, pic: Option<&str>, secs: Option<u64>, index: usize) -> PlaylistEntry {
    PlaylistEntry {
        id: bvid.to_string(),
        title: title.to_string(),
        url: format!("https://www.bilibili.com/video/{bvid}"),
        duration_ms: secs.map(|s| s * 1000),
        thumbnail: pic.map(|p| p.replace("http://", "https://")),
        index: index as u32,
    }
}

fn mmss(s: &str) -> Option<u64> {
    let parts: Vec<u64> = s.split(':').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    Some(parts.iter().fold(0, |a, p| a * 60 + p))
}

/// 投稿列表（`x/space/wbi/arc/search`）一页。
pub fn parse_uploads(data: &Value, base: usize) -> Page {
    let list = data.pointer("/list/vlist").and_then(Value::as_array).cloned().unwrap_or_default();
    let total = u64_at(data, "/page/count").unwrap_or(0) as usize;
    let pn = u64_at(data, "/page/pn").unwrap_or(1) as usize;
    let ps = u64_at(data, "/page/ps").unwrap_or(0) as usize;
    let entries: Vec<_> = list
        .iter()
        .filter_map(|x| {
            let bvid = str_at(x, "/bvid")?;
            Some(entry(bvid, str_at(x, "/title").unwrap_or(bvid), str_at(x, "/pic"), str_at(x, "/length").and_then(mmss), 0))
        })
        .enumerate()
        .map(|(i, mut e)| {
            e.index = (base + i + 1) as u32;
            e
        })
        .collect();
    Page { more: !list.is_empty() && pn * ps < total, entries }
}

/// 合集 / 系列的稿件数组（字段名两边一样：bvid、title、pic、duration 秒）。
pub fn parse_archives(arr: &[Value], base: usize) -> Vec<PlaylistEntry> {
    arr.iter()
        .filter_map(|x| {
            let bvid = str_at(x, "/bvid")?;
            Some(entry(bvid, str_at(x, "/title").unwrap_or(bvid), str_at(x, "/pic"), u64_at(x, "/duration"), 0))
        })
        .enumerate()
        .map(|(i, mut e)| {
            e.index = (base + i + 1) as u32;
            e
        })
        .collect()
}

/// 收藏夹一页；失效的稿件（标题“已失效视频”）和非视频（专栏、音频）跳过。
pub fn parse_fav(data: &Value, base: usize) -> Page {
    let medias = data.get("medias").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut n = base;
    let entries = medias
        .iter()
        .filter(|m| u64_at(m, "/type") == Some(2) && u64_at(m, "/attr").unwrap_or(0) & 1 == 0 && str_at(m, "/title") != Some("已失效视频"))
        .filter_map(|m| {
            let bvid = str_at(m, "/bvid").or_else(|| str_at(m, "/bv_id"))?;
            n += 1;
            Some(entry(bvid, str_at(m, "/title").unwrap_or(bvid), str_at(m, "/cover"), u64_at(m, "/duration"), n))
        })
        .collect();
    Page { entries, more: data.get("has_more").and_then(Value::as_bool).unwrap_or(false) }
}

/// “合集和列表”总览里的每个合集 / 系列：`(是合集, id, 名字)`。
pub fn parse_season_series(data: &Value) -> (Vec<(bool, String, String)>, bool) {
    let items = data.get("items_lists");
    let mut out = Vec::new();
    for (key, season) in [("seasons_list", true), ("series_list", false)] {
        for it in items.and_then(|i| i.get(key)).and_then(Value::as_array).into_iter().flatten() {
            let meta = it.get("meta");
            let id = meta.and_then(|m| u64_at(m, if season { "/season_id" } else { "/series_id" }));
            if let Some(id) = id {
                out.push((season, id.to_string(), meta.and_then(|m| str_at(m, "/name")).unwrap_or("").to_string()));
            }
        }
    }
    let page = data.get("page");
    let more = page.map(|p| u64_at(p, "/page_num").unwrap_or(1) * u64_at(p, "/page_size").unwrap_or(0) < u64_at(p, "/total").unwrap_or(0)).unwrap_or(false);
    (out, more)
}

async fn pause() {
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}

async fn api(ctx: &Ctx<'_>, url: &str) -> AppResult<Value> {
    let cookie = ctx.cookie("https://api.bilibili.com/");
    let v = get_json(ctx, url, cookie.as_deref()).await?;
    api_data(&v).cloned()
}

async fn uploads(ctx: &Ctx<'_>, mid: &str) -> AppResult<(String, Vec<PlaylistEntry>)> {
    let cookie = ctx.cookie("https://api.bilibili.com/");
    let keys = wbi_keys(ctx, cookie.as_deref()).await?;
    let mut all: Vec<PlaylistEntry> = Vec::new();
    let mut author = String::new();
    let mut pn = 1;
    loop {
        let mut params: Vec<(String, String)> = vec![
            ("mid".into(), mid.into()),
            ("ps".into(), "50".into()),
            ("pn".into(), pn.to_string()),
            ("order".into(), "pubdate".into()),
            ("platform".into(), "web".into()),
        ];
        params.extend(DM_PARAMS.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let q = wbi_sign(params, &keys, chrono::Utc::now().timestamp());
        let v = get_json(ctx, &format!("https://api.bilibili.com/x/space/wbi/arc/search?{q}"), cookie.as_deref()).await?;
        let data = api_data(&v)?;
        if author.is_empty() {
            author = data.pointer("/list/vlist/0/author").and_then(Value::as_str).unwrap_or("").to_string();
        }
        let page = parse_uploads(data, all.len());
        all.extend(page.entries);
        if !page.more || all.len() >= MAX_ENTRIES {
            break;
        }
        pn += 1;
        pause().await;
    }
    Ok((author, all))
}

async fn season(ctx: &Ctx<'_>, mid: &str, id: &str) -> AppResult<(String, Vec<PlaylistEntry>)> {
    let mut all: Vec<PlaylistEntry> = Vec::new();
    let mut name = String::new();
    let mut pn = 1;
    loop {
        let url = format!(
            "https://api.bilibili.com/x/polymer/web-space/seasons_archives_list?mid={mid}&season_id={id}&sort_reverse=false&page_num={pn}&page_size=100"
        );
        let data = api(ctx, &url).await?;
        if name.is_empty() {
            name = data.pointer("/meta/name").and_then(Value::as_str).unwrap_or("").to_string();
        }
        let arr = data.get("archives").and_then(Value::as_array).cloned().unwrap_or_default();
        let n = arr.len();
        all.extend(parse_archives(&arr, all.len()));
        let total = u64_at(&data, "/page/total").unwrap_or(0) as usize;
        if n == 0 || all.len() >= total || all.len() >= MAX_ENTRIES {
            break;
        }
        pn += 1;
        pause().await;
    }
    Ok((name, all))
}

async fn series(ctx: &Ctx<'_>, mid: &str, id: &str) -> AppResult<(String, Vec<PlaylistEntry>)> {
    let name = api(ctx, &format!("https://api.bilibili.com/x/series/series?series_id={id}"))
        .await
        .ok()
        .and_then(|d| d.pointer("/meta/name").and_then(Value::as_str).map(String::from))
        .unwrap_or_default();
    let mut all: Vec<PlaylistEntry> = Vec::new();
    let mut pn = 1;
    loop {
        let url = format!("https://api.bilibili.com/x/series/archives?mid={mid}&series_id={id}&only_normal=true&sort=asc&pn={pn}&ps=30");
        let data = api(ctx, &url).await?;
        let arr = data.get("archives").and_then(Value::as_array).cloned().unwrap_or_default();
        let n = arr.len();
        all.extend(parse_archives(&arr, all.len()));
        let total = u64_at(&data, "/page/total").unwrap_or(0) as usize;
        if n == 0 || (total > 0 && all.len() >= total) || all.len() >= MAX_ENTRIES {
            break;
        }
        pn += 1;
        pause().await;
    }
    Ok((name, all))
}

async fn fav(ctx: &Ctx<'_>, id: &str) -> AppResult<(String, String, Vec<PlaylistEntry>)> {
    let mut all: Vec<PlaylistEntry> = Vec::new();
    let (mut title, mut owner) = (String::new(), String::new());
    let mut pn = 1;
    loop {
        let url = format!("https://api.bilibili.com/x/v3/fav/resource/list?media_id={id}&pn={pn}&ps=20&order=mtime&type=0&tid=0&platform=web");
        let data = api(ctx, &url).await?;
        if title.is_empty() {
            title = data.pointer("/info/title").and_then(Value::as_str).unwrap_or("").to_string();
            owner = data.pointer("/info/upper/name").and_then(Value::as_str).unwrap_or("").to_string();
        }
        let page = parse_fav(&data, all.len());
        all.extend(page.entries);
        if !page.more || all.len() >= MAX_ENTRIES {
            break;
        }
        pn += 1;
        pause().await;
    }
    Ok((title, owner, all))
}

async fn space_lists(ctx: &Ctx<'_>, mid: &str) -> AppResult<Vec<(bool, String, String)>> {
    let mut out = Vec::new();
    let mut pn = 1;
    loop {
        let data = api(ctx, &format!("https://api.bilibili.com/x/polymer/web-space/seasons_series_list?mid={mid}&page_num={pn}&page_size=20")).await?;
        let (items, more) = parse_season_series(&data);
        out.extend(items);
        if !more || pn >= 10 {
            break;
        }
        pn += 1;
        pause().await;
    }
    Ok(out)
}

fn playlist(id: &str, title: String, author: String, source: &str, entries: Vec<PlaylistEntry>) -> AppResult<MediaInfo> {
    if entries.is_empty() {
        return Err(AppError::not_found("这个列表是空的，或者需要登录才能查看（收藏夹、稍后再看、私密合集要先在设置里登录 B站）。"));
    }
    let cover = entries.first().and_then(|e| e.thumbnail.clone());
    Ok(MediaInfo {
        platform: "bilibili".into(),
        platform_name: "B站".into(),
        id: id.to_string(),
        source_url: source.to_string(),
        title,
        author,
        cover,
        duration_ms: None,
        kind: MediaKind::Playlist,
        width: None,
        height: None,
        published_at: None,
        assets: vec![],
        entries,
        series: None,
        extractor: Some("native".into()),
        chapters: vec![],
    })
}

/// 把多个子列表的条目拼到一起，标题前加上子列表的名字，序号重排，同一个稿件只留一条。
fn merge(groups: Vec<(String, Vec<PlaylistEntry>)>) -> Vec<PlaylistEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<PlaylistEntry> = Vec::new();
    for (name, list) in groups {
        for mut e in list {
            if !seen.insert(e.id.clone()) {
                continue;
            }
            if !name.is_empty() {
                e.title = format!("【{name}】{}", e.title);
            }
            e.index = out.len() as u32 + 1;
            out.push(e);
            if out.len() >= MAX_ENTRIES {
                return out;
            }
        }
    }
    out
}

pub async fn resolve(ctx: &Ctx<'_>, target: &Target, source: &str) -> AppResult<MediaInfo> {
    match target {
        Target::Uploads { mid } => {
            let (author, entries) = uploads(ctx, mid).await?;
            let who = if author.is_empty() { format!("UP 主 {mid}") } else { author.clone() };
            playlist(&format!("space-{mid}"), format!("{who} 的全部投稿"), author, source, entries)
        }
        Target::Season { mid, id } => {
            let (name, entries) = season(ctx, mid, id).await?;
            playlist(&format!("season-{id}"), if name.is_empty() { format!("合集 {id}") } else { name }, String::new(), source, entries)
        }
        Target::Series { mid, id } => {
            let (name, entries) = series(ctx, mid, id).await?;
            playlist(&format!("series-{id}"), if name.is_empty() { format!("视频列表 {id}") } else { name }, String::new(), source, entries)
        }
        Target::Fav { id } => {
            let (title, owner, entries) = fav(ctx, id).await?;
            playlist(&format!("fav-{id}"), if title.is_empty() { format!("收藏夹 {id}") } else { title }, owner, source, entries)
        }
        Target::FavFolders { mid } => {
            let data = api(ctx, &format!("https://api.bilibili.com/x/v3/fav/folder/created/list-all?up_mid={mid}")).await?;
            let folders: Vec<(String, String)> = data
                .get("list")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|f| Some((u64_at(f, "/id")?.to_string(), str_at(f, "/title").unwrap_or("").to_string())))
                .collect();
            let mut groups = Vec::new();
            for (id, name) in folders {
                if let Ok((_, _, e)) = fav(ctx, &id).await {
                    groups.push((name, e));
                }
            }
            playlist(&format!("favs-{mid}"), format!("UP 主 {mid} 的收藏夹"), String::new(), source, merge(groups))
        }
        Target::SpaceLists { mid } => {
            let lists = space_lists(ctx, mid).await?;
            let mut groups = Vec::new();
            for (is_season, id, name) in lists {
                let r = if is_season { season(ctx, mid, &id).await } else { series(ctx, mid, &id).await };
                if let Ok((n, e)) = r {
                    groups.push((if name.is_empty() { n } else { name }, e));
                }
            }
            playlist(&format!("lists-{mid}"), format!("UP 主 {mid} 的合集和列表"), String::new(), source, merge(groups))
        }
        Target::WatchLater => {
            let data = api(ctx, "https://api.bilibili.com/x/v2/history/toview").await?;
            let arr = data.get("list").and_then(Value::as_array).cloned().unwrap_or_default();
            let entries = parse_archives(&arr, 0);
            playlist("watchlater", "稍后再看".into(), String::new(), source, entries)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(u: &str) -> Option<Target> {
        classify(u)
    }

    #[test]
    fn classifies_creator_and_list_pages() {
        let m = || "3546747185924773".to_string();
        assert_eq!(t("https://space.bilibili.com/3546747185924773/lists"), Some(Target::SpaceLists { mid: m() }));
        assert_eq!(t("https://space.bilibili.com/3546747185924773"), Some(Target::Uploads { mid: m() }));
        assert_eq!(t("https://space.bilibili.com/3546747185924773/video?tid=0"), Some(Target::Uploads { mid: m() }));
        assert_eq!(t("https://space.bilibili.com/3546747185924773/upload/video"), Some(Target::Uploads { mid: m() }));
        assert_eq!(t("https://space.bilibili.com/3546747185924773/lists/123?type=season"), Some(Target::Season { mid: m(), id: "123".into() }));
        assert_eq!(t("https://space.bilibili.com/3546747185924773/lists/456?type=series"), Some(Target::Series { mid: m(), id: "456".into() }));
        assert_eq!(t("https://space.bilibili.com/1/channel/collectiondetail?sid=77"), Some(Target::Season { mid: "1".into(), id: "77".into() }));
        assert_eq!(t("https://space.bilibili.com/1/channel/seriesdetail?sid=88"), Some(Target::Series { mid: "1".into(), id: "88".into() }));
        assert_eq!(t("https://space.bilibili.com/1/channel/series"), Some(Target::SpaceLists { mid: "1".into() }));
        assert_eq!(t("https://space.bilibili.com/1/favlist?fid=999&ftype=create"), Some(Target::Fav { id: "999".into() }));
        assert_eq!(t("https://space.bilibili.com/1/favlist"), Some(Target::FavFolders { mid: "1".into() }));
        assert_eq!(t("https://www.bilibili.com/medialist/play/ml999"), Some(Target::Fav { id: "999".into() }));
        assert_eq!(t("https://www.bilibili.com/list/ml999?oid=1"), Some(Target::Fav { id: "999".into() }));
        assert_eq!(t("https://www.bilibili.com/list/watchlater"), Some(Target::WatchLater));
        assert_eq!(t("https://www.bilibili.com/list/55?sid=9"), Some(Target::Series { mid: "55".into(), id: "9".into() }));
    }

    #[test]
    fn single_videos_are_not_lists() {
        assert_eq!(t("https://www.bilibili.com/video/BV1aa411c7mD"), None);
        assert_eq!(t("https://www.bilibili.com/bangumi/play/ep1"), None);
        assert_eq!(t("https://b23.tv/abc"), None);
    }

    #[test]
    fn uploads_page_and_paging() {
        let data = json!({"page": {"pn": 1, "ps": 2, "count": 3}, "list": {"vlist": [
            {"bvid": "BV1aa", "title": "新", "length": "12:05", "pic": "http://x/a.jpg", "author": "老王"},
            {"bvid": "BV1bb", "title": "旧", "length": "1:02:03"}]}});
        let p = parse_uploads(&data, 0);
        assert!(p.more);
        assert_eq!(p.entries[0].duration_ms, Some(725_000));
        assert_eq!(p.entries[1].duration_ms, Some(3_723_000));
        assert_eq!(p.entries[0].thumbnail.as_deref(), Some("https://x/a.jpg"));
        assert_eq!(p.entries[1].index, 2);
        let last = json!({"page": {"pn": 2, "ps": 2, "count": 3}, "list": {"vlist": [{"bvid": "BV1cc", "title": "c"}]}});
        let p2 = parse_uploads(&last, 2);
        assert!(!p2.more);
        assert_eq!(p2.entries[0].index, 3);
    }

    #[test]
    fn archives_of_season_and_series() {
        let arr = json!([{"bvid": "BV1a", "title": "第一集", "duration": 61, "pic": "https://x/1.jpg"}, {"title": "没有 bvid"}]);
        let e = parse_archives(arr.as_array().unwrap(), 10);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].index, 11);
        assert_eq!(e[0].duration_ms, Some(61_000));
        assert_eq!(e[0].url, "https://www.bilibili.com/video/BV1a");
    }

    #[test]
    fn favorites_skip_invalid_and_non_video() {
        let data = json!({"has_more": true, "medias": [
            {"type": 2, "bvid": "BV1ok", "title": "好", "duration": 30, "cover": "https://x/c.jpg", "attr": 0},
            {"type": 2, "bvid": "BV1bad", "title": "已失效视频", "attr": 9},
            {"type": 12, "bvid": "", "title": "音频"},
            {"type": 2, "bvid": "BV1dead", "title": "x", "attr": 1}]});
        let p = parse_fav(&data, 0);
        assert!(p.more);
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].id, "BV1ok");
    }

    #[test]
    fn season_series_overview() {
        let data = json!({"items_lists": {
            "seasons_list": [{"meta": {"season_id": 11, "name": "合集甲"}}],
            "series_list": [{"meta": {"series_id": 22, "name": "列表乙"}}]},
            "page": {"page_num": 1, "page_size": 20, "total": 45}});
        let (items, more) = parse_season_series(&data);
        assert_eq!(items, vec![(true, "11".into(), "合集甲".into()), (false, "22".into(), "列表乙".into())]);
        assert!(more);
    }

    #[test]
    fn merge_dedups_and_prefixes() {
        let a = vec![entry("BV1a", "一", None, None, 1), entry("BV1b", "二", None, None, 2)];
        let b = vec![entry("BV1b", "二", None, None, 1), entry("BV1c", "三", None, None, 2)];
        let m = merge(vec![("甲".into(), a), ("乙".into(), b)]);
        assert_eq!(m.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["BV1a", "BV1b", "BV1c"]);
        assert_eq!(m[2].title, "【乙】三");
        assert_eq!(m[2].index, 3);
    }
}
