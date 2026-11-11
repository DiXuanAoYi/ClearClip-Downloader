//! 抖音、快手、小红书、微博的“主页 / 合集 / 专辑”：这些站的列表接口都带签名或要登录，直接请求很容易被风控，
//! 所以用一个隐藏的浏览器窗口打开页面（和内置登录窗口共用浏览器数据，登录过就能看需要登录的主页），
//! 滚动加载、收集页面上的作品链接，再逐条解析。结果是一个播放列表（kind = playlist）。

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use tauri::AppHandle;
use url::Url;

use crate::error::{AppError, AppResult};
use crate::model::{MediaInfo, MediaKind, PlaylistEntry};
use crate::providers::listing::SubEntry;

/// 一次最多收集这么多条。
pub const MAX_ENTRIES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    Douyin,
    Kuaishou,
    Xiaohongshu,
    Weibo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub site: Site,
    /// 要打开的页面（微博主页会补上“视频”标签）
    pub url: String,
    /// 列表的种类，用在标题里
    pub kind: &'static str,
}

impl Site {
    pub fn id(self) -> &'static str {
        match self {
            Site::Douyin => "douyin",
            Site::Kuaishou => "kuaishou",
            Site::Xiaohongshu => "xiaohongshu",
            Site::Weibo => "weibo",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Site::Douyin => "抖音",
            Site::Kuaishou => "快手",
            Site::Xiaohongshu => "小红书",
            Site::Weibo => "微博",
        }
    }

    /// 页面上哪些链接是作品（在浏览器里和这里各用一遍）。
    fn link_pattern(self) -> &'static str {
        match self {
            Site::Douyin => r"/(?:video|note)/(\d{8,})",
            Site::Kuaishou => r"/(?:short-video|video|photo)/([A-Za-z0-9_\-]{6,})",
            Site::Xiaohongshu => r"/(?:explore|discovery/item|user/profile/[0-9a-f]+)/([0-9a-f]{24})",
            Site::Weibo => r"(?:weibo\.com|weibo\.cn)/(?:\d{6,}|detail)/([A-Za-z0-9]{8,})|/tv/show/(\d+:\d+)",
        }
    }
}

static DOUYIN_USER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/(?:user|collection|mix)/").unwrap());
static KS_PROFILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/(?:profile|playlist|collection)/").unwrap());
static XHS_LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/(?:user/profile/[0-9a-f]+|board/[0-9a-f]+)/?$").unwrap());
static WB_USER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/(?:u/)?\d{6,}/?$").unwrap());

/// 判断链接是不是这四个站的主页 / 合集；单个作品返回 None。
pub fn classify(url: &str) -> Option<Spec> {
    let u = Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let path = u.path();
    let ends = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    if ends("douyin.com") && DOUYIN_USER.is_match(path) {
        let kind = if path.starts_with("/user/") { "主页" } else { "合集" };
        return Some(Spec { site: Site::Douyin, url: url.to_string(), kind });
    }
    if ends("kuaishou.com") && KS_PROFILE.is_match(path) {
        let kind = if path.starts_with("/profile/") { "主页" } else { "合集" };
        return Some(Spec { site: Site::Kuaishou, url: url.to_string(), kind });
    }
    if ends("xiaohongshu.com") && XHS_LIST.is_match(path) {
        let kind = if path.starts_with("/board/") { "专辑" } else { "主页" };
        return Some(Spec { site: Site::Xiaohongshu, url: url.to_string(), kind });
    }
    if (ends("weibo.com") || ends("weibo.cn")) && WB_USER.is_match(path) {
        let mut u2 = u.clone();
        // 只看视频：不然整个时间线里的纯文字微博都会进来
        if !u2.query_pairs().any(|(k, _)| k == "tabtype") {
            u2.query_pairs_mut().append_pair("tabtype", "video");
        }
        return Some(Spec { site: Site::Weibo, url: u2.to_string(), kind: "主页" });
    }
    None
}

/// 从页面上收集到的链接里挑出作品，去重、保持页面上的顺序。返回 `(id, 作品链接)`。
pub fn pick_entries(site: Site, hrefs: &[String], limit: usize) -> Vec<(String, String)> {
    let re = Regex::new(site.link_pattern()).expect("link pattern");
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for h in hrefs {
        let Some(c) = re.captures(h) else { continue };
        let Some(id) = c.iter().skip(1).flatten().next().map(|m| m.as_str().to_string()) else { continue };
        if !seen.insert(id.clone()) {
            continue;
        }
        out.push((id.clone(), work_url(site, &id, h)));
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn work_url(site: Site, id: &str, href: &str) -> String {
    match site {
        Site::Douyin => format!("https://www.douyin.com/{}/{id}", if href.contains("/note/") { "note" } else { "video" }),
        Site::Kuaishou => format!("https://www.kuaishou.com/short-video/{id}"),
        // 小红书的作品链接带 xsec_token，没有它常常打不开：原样保留页面上的链接
        Site::Xiaohongshu => href.to_string(),
        Site::Weibo => {
            if id.contains(':') {
                format!("https://weibo.com/tv/show/{id}")
            } else {
                href.to_string()
            }
        }
    }
}

/// 隐藏窗口里跑的脚本：滚动加载，收集链接，连续几次没有新增就结束；结果放在窗口标题里（`CCLINKS:` + JSON）。
pub fn collector_script(site: Site, limit: usize) -> String {
    let pat = serde_json::to_string(site.link_pattern()).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function(){{
  var re = new RegExp({pat}); var title0 = ''; var found = []; var seen = {{}}; var idle = 0; var n = 0;
  var timer = setInterval(function(){{
    n++; if (!title0) title0 = document.title || '';
    var before = found.length;
    var links = document.querySelectorAll('a[href]');
    for (var i = 0; i < links.length; i++) {{
      var h = links[i].href; if (!h || !re.test(h) || seen[h]) continue; seen[h] = 1; found.push(h);
    }}
    idle = found.length === before ? idle + 1 : 0;
    window.scrollTo(0, document.body.scrollHeight);
    if (found.length >= {limit} || idle >= 4 || n >= 70) {{
      clearInterval(timer);
      document.title = 'CCLINKS:' + JSON.stringify({{ t: title0, l: found.slice(0, {limit}) }});
    }}
  }}, 1200);
}})();"#
    )
}

/// 解析窗口标题里的结果：`(页面标题, 链接们)`。
pub fn parse_result(title: &str) -> Option<(String, Vec<String>)> {
    let json = title.strip_prefix("CCLINKS:")?;
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let t = v.get("t").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let l = v.get("l").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    Some((t, l))
}

/// 页面标题常带站名后缀，去掉。
pub fn clean_title(raw: &str, spec: &Spec) -> String {
    let t = raw.split(" - ").next().unwrap_or(raw).split('_').next().unwrap_or(raw).trim();
    let t = t.trim_end_matches("的个人主页").trim_end_matches("的主页").trim();
    if t.is_empty() || t.chars().count() > 40 {
        format!("{}{}", spec.site.name(), spec.kind)
    } else {
        format!("{}的{}", t, spec.kind)
    }
}

pub struct Collected {
    pub title: String,
    pub entries: Vec<(String, String)>,
}

/// 用隐藏窗口打开页面并收集作品链接。
pub async fn collect(app: &AppHandle, spec: &Spec, limit: usize) -> AppResult<Collected> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let limit = limit.clamp(1, MAX_ENTRIES);
    let label = format!("wl-{}-{}", spec.site.id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0));
    let parsed = Url::parse(&spec.url).map_err(|_| AppError::invalid("链接格式不正确"))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed))
        .title("ClearClip")
        .visible(false)
        .inner_size(1200.0, 900.0)
        .initialization_script(collector_script(spec.site, limit))
        .on_document_title_changed(move |_w, title| {
            if title.starts_with("CCLINKS:") {
                let _ = tx.send(title);
            }
        })
        .build()?;
    let got = tokio::time::timeout(Duration::from_secs(100), rx.recv()).await;
    let _ = win.close();
    let (raw_title, hrefs) = got.ok().flatten().and_then(|t| parse_result(&t)).unwrap_or_default();
    let entries = pick_entries(spec.site, &hrefs, limit);
    if entries.is_empty() {
        return Err(AppError::need_login(format!(
            "没有从{}{}读到作品。可能需要登录：请在“设置 → 账号与 Cookie”里用内置登录窗口登录{}后重试；也可能是页面改版或这个主页没有公开作品。",
            spec.site.name(),
            spec.kind,
            spec.site.name()
        )));
    }
    Ok(Collected { title: clean_title(&raw_title, spec), entries })
}

pub fn to_media_info(spec: &Spec, c: Collected) -> MediaInfo {
    let entries: Vec<PlaylistEntry> = c
        .entries
        .iter()
        .enumerate()
        .map(|(i, (id, url))| PlaylistEntry {
            id: id.clone(),
            title: format!("{}作品 {}", spec.site.name(), i + 1),
            url: url.clone(),
            duration_ms: None,
            thumbnail: None,
            index: i as u32 + 1,
        })
        .collect();
    MediaInfo {
        platform: spec.site.id().into(),
        platform_name: spec.site.name().into(),
        id: format!("{}-list", spec.site.id()),
        source_url: spec.url.clone(),
        title: c.title,
        author: String::new(),
        cover: None,
        duration_ms: None,
        kind: MediaKind::Playlist,
        width: None,
        height: None,
        published_at: None,
        assets: vec![],
        entries,
        series: None,
        extractor: Some("webview".into()),
        chapters: vec![],
    }
}

pub fn to_sub_entries(c: &Collected) -> Vec<SubEntry> {
    c.entries
        .iter()
        .enumerate()
        .map(|(i, (id, url))| SubEntry {
            id: id.clone(),
            title: format!("作品 {}", i + 1),
            url: url.clone(),
            thumbnail: None,
            published_at: None,
            duration_ms: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(u: &str) -> Option<Site> {
        classify(u).map(|s| s.site)
    }

    #[test]
    fn classifies_profiles_and_collections() {
        assert_eq!(site("https://www.douyin.com/user/MS4wLjABAAAA-x_y?from_tab_name=main"), Some(Site::Douyin));
        assert_eq!(site("https://www.douyin.com/collection/7123456789012345678"), Some(Site::Douyin));
        assert_eq!(classify("https://www.douyin.com/collection/1").unwrap().kind, "合集");
        assert_eq!(site("https://www.kuaishou.com/profile/3xabc123"), Some(Site::Kuaishou));
        assert_eq!(site("https://www.xiaohongshu.com/user/profile/5f1234567890abcdef123456"), Some(Site::Xiaohongshu));
        assert_eq!(site("https://www.xiaohongshu.com/board/5f1234567890abcdef123456"), Some(Site::Xiaohongshu));
        assert_eq!(site("https://weibo.com/u/1234567890"), Some(Site::Weibo));
        assert_eq!(site("https://weibo.com/1234567890"), Some(Site::Weibo));
    }

    #[test]
    fn single_works_are_not_lists() {
        assert_eq!(site("https://www.douyin.com/video/7123456789012345678"), None);
        assert_eq!(site("https://www.kuaishou.com/short-video/3xabcdefg"), None);
        assert_eq!(site("https://www.xiaohongshu.com/explore/5f1234567890abcdef123456"), None);
        assert_eq!(site("https://weibo.com/1234567890/N1abcDEFg"), None);
        assert_eq!(site("https://www.bilibili.com/video/BV1aa411c7mD"), None);
    }

    #[test]
    fn weibo_profile_gets_the_video_tab() {
        let s = classify("https://weibo.com/u/1234567890").unwrap();
        assert!(s.url.ends_with("?tabtype=video"), "{}", s.url);
        let s2 = classify("https://weibo.com/u/1234567890?tabtype=album").unwrap();
        assert_eq!(s2.url, "https://weibo.com/u/1234567890?tabtype=album");
    }

    #[test]
    fn picks_works_in_page_order_without_duplicates() {
        let h: Vec<String> = [
            "https://www.douyin.com/video/7111111111111111111",
            "https://www.douyin.com/user/self",
            "https://www.douyin.com/note/7222222222222222222",
            "https://www.douyin.com/video/7111111111111111111?x=1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let e = pick_entries(Site::Douyin, &h, 10);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0], ("7111111111111111111".into(), "https://www.douyin.com/video/7111111111111111111".into()));
        assert_eq!(e[1].1, "https://www.douyin.com/note/7222222222222222222");
        assert_eq!(pick_entries(Site::Douyin, &h, 1).len(), 1);
    }

    #[test]
    fn xiaohongshu_keeps_the_token_link() {
        let h =
            vec!["https://www.xiaohongshu.com/user/profile/5f1234567890abcdef123456/64a1b2c3d4e5f60718293a4b?xsec_token=AB&xsec_source=pc_user".to_string()];
        let e = pick_entries(Site::Xiaohongshu, &h, 5);
        assert_eq!(e[0].0, "64a1b2c3d4e5f60718293a4b");
        assert!(e[0].1.contains("xsec_token=AB"));
    }

    #[test]
    fn kuaishou_and_weibo_links() {
        let k = vec!["https://www.kuaishou.com/short-video/3xabcdefg123?authorId=1".to_string()];
        assert_eq!(pick_entries(Site::Kuaishou, &k, 5)[0].1, "https://www.kuaishou.com/short-video/3xabcdefg123");
        let w = vec!["https://weibo.com/1234567890/N1abcDEFgh".to_string(), "https://weibo.com/tv/show/1034:4912345678901234".to_string()];
        let e = pick_entries(Site::Weibo, &w, 5);
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].1, "https://weibo.com/tv/show/1034:4912345678901234");
    }

    #[test]
    fn result_round_trip_and_title() {
        let t = "CCLINKS:{\"t\":\"老王的个人主页 - 抖音\",\"l\":[\"https://www.douyin.com/video/7111111111111111111\"]}";
        let (title, links) = parse_result(t).unwrap();
        assert_eq!(links.len(), 1);
        let spec = classify("https://www.douyin.com/user/abc").unwrap();
        assert_eq!(clean_title(&title, &spec), "老王的主页");
        assert_eq!(clean_title("", &spec), "抖音主页");
        assert!(parse_result("普通标题").is_none());
        assert!(collector_script(Site::Douyin, 50).contains("CCLINKS:"));
    }

    #[test]
    fn media_info_is_a_playlist() {
        let spec = classify("https://www.kuaishou.com/profile/3xabc123").unwrap();
        let info = to_media_info(&spec, Collected { title: "t".into(), entries: vec![("a".into(), "https://u/a".into()), ("b".into(), "https://u/b".into())] });
        assert_eq!(info.kind, MediaKind::Playlist);
        assert_eq!(info.entries.len(), 2);
        assert_eq!(info.entries[1].index, 2);
        assert_eq!(info.platform, "kuaishou");
    }
}
