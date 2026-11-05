//! 视频缓存：浏览器扩展把某个视频的“摘要会话”和“双语字幕”快照存到清影里，
//! 生词本据此显示每个词出自哪个视频、该视频有哪些缓存，并能在浏览器里恢复。
//! 只存扩展主动同步来的（收藏过词的视频，或用户手动同步）。总量有上限，超出时先删最久没用的。

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 单个视频快照的上限。
pub const MAX_ONE: usize = 2 * 1024 * 1024;
/// 总量默认上限（MB）。
pub const DEFAULT_LIMIT_MB: u64 = 500;

/// 和扩展 vsum-logic.js 的 videoKey 一致：YouTube / B 站按视频编号（B 站带分 P），其他按去掉锚点的网址。
pub fn video_key(href: &str) -> String {
    let Ok(u) = url::Url::parse(href) else { return format!("url:{href}") };
    let host = u.host_str().unwrap_or("").trim_start_matches("www.").trim_start_matches("m.").to_string();
    let q = |k: &str| u.query_pairs().find(|(a, _)| a == k).map(|(_, v)| v.into_owned());
    if host == "youtu.be" {
        let id = u.path().trim_start_matches('/').split('/').next().unwrap_or("");
        if !id.is_empty() {
            return format!("youtube:{id}");
        }
    }
    if host == "youtube.com" || host == "music.youtube.com" {
        if let Some(v) = q("v") {
            return format!("youtube:{v}");
        }
        let segs: Vec<&str> = u.path().split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() >= 2 && matches!(segs[0], "shorts" | "embed" | "live" | "v") && segs[1].len() >= 6 {
            return format!("youtube:{}", segs[1]);
        }
    }
    if host == "bilibili.com" {
        let segs: Vec<&str> = u.path().split('/').filter(|s| !s.is_empty()).collect();
        if let Some(i) = segs.iter().position(|s| *s == "video") {
            if let Some(id) = segs.get(i + 1).filter(|s| s.to_ascii_lowercase().starts_with("bv") || s.to_ascii_lowercase().starts_with("av")) {
                let p = q("p").and_then(|p| p.parse::<u32>().ok()).unwrap_or(1);
                return if p > 1 { format!("bilibili:{id}:p{p}") } else { format!("bilibili:{id}") };
            }
        }
    }
    let mut c = u.clone();
    c.set_fragment(None);
    format!("url:{c}")
}

/// 带时间点的链接：点开就从那一刻播放。
pub fn time_link(href: &str, sec: f64) -> String {
    let t = sec.max(0.0).floor() as u64;
    let Ok(mut u) = url::Url::parse(href) else { return href.to_string() };
    u.set_fragment(None);
    if t == 0 {
        return u.to_string();
    }
    let host = u.host_str().unwrap_or("").trim_start_matches("www.").to_string();
    let mut pairs: Vec<(String, String)> = u.query_pairs().into_owned().filter(|(k, _)| k != "t").collect();
    if host.contains("youtu") {
        pairs.push(("t".into(), format!("{t}s")));
    } else {
        pairs.push(("t".into(), t.to_string()));
    }
    u.query_pairs_mut().clear().extend_pairs(pairs);
    u.to_string()
}

static STORE: OnceLock<Mutex<Connection>> = OnceLock::new();

fn conn(data_dir: &Path) -> AppResult<std::sync::MutexGuard<'static, Connection>> {
    if STORE.get().is_none() {
        std::fs::create_dir_all(data_dir)?;
        let c = Connection::open(data_dir.join("videocache.db"))?;
        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS video_cache (
                video_key TEXT PRIMARY KEY,
                url TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                site TEXT NOT NULL DEFAULT '',
                summary TEXT NOT NULL DEFAULT '',
                subs TEXT NOT NULL DEFAULT '',
                src_lang TEXT NOT NULL DEFAULT '',
                dst_lang TEXT NOT NULL DEFAULT '',
                lines INTEGER NOT NULL DEFAULT 0,
                bytes INTEGER NOT NULL DEFAULT 0,
                updated INTEGER NOT NULL,
                accessed INTEGER NOT NULL
             );",
        )?;
        let _ = STORE.set(Mutex::new(c));
    }
    Ok(STORE.get().ok_or_else(|| AppError::invalid("视频缓存打不开"))?.lock().unwrap_or_else(|e| e.into_inner()))
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Put {
    pub url: String,
    pub title: String,
    pub site: String,
    /// 扩展的摘要会话（vsumSessions 里这个视频的那一条）
    pub summary: Value,
    /// 扩展的双语字幕：{ lines: [{t,text}], trans: [..], key }
    pub subs: Value,
    pub src_lang: String,
    pub dst_lang: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub video_key: String,
    pub url: String,
    pub title: String,
    pub has_summary: bool,
    pub has_subs: bool,
    pub lines: i64,
    pub src_lang: String,
    pub dst_lang: String,
    pub bytes: i64,
    pub updated: i64,
}

fn is_empty(v: &Value) -> bool {
    v.is_null() || v.as_object().is_some_and(|o| o.is_empty()) || v.as_array().is_some_and(|a| a.is_empty())
}

/// 存（覆盖同一个视频的旧快照）。返回信息；超过单个上限报错。
pub fn put(data_dir: &Path, p: &Put, limit_mb: u64) -> AppResult<Info> {
    if !(p.url.starts_with("http://") || p.url.starts_with("https://")) {
        return Err(AppError::invalid("缺少视频网址。"));
    }
    let key = video_key(&p.url);
    let summary = if is_empty(&p.summary) { String::new() } else { p.summary.to_string() };
    let subs = if is_empty(&p.subs) { String::new() } else { p.subs.to_string() };
    if summary.is_empty() && subs.is_empty() {
        return Err(AppError::invalid("没有可保存的内容。"));
    }
    let bytes = summary.len() + subs.len();
    if bytes > MAX_ONE {
        return Err(AppError::invalid("这个视频的缓存超过 2 MB，没有同步。"));
    }
    let lines = p.subs.get("lines").and_then(Value::as_array).map_or(0, |a| a.len() as i64);
    let c = conn(data_dir)?;
    c.execute(
        "INSERT INTO video_cache (video_key,url,title,site,summary,subs,src_lang,dst_lang,lines,bytes,updated,accessed)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)
         ON CONFLICT(video_key) DO UPDATE SET
           url=excluded.url, title=excluded.title, site=excluded.site,
           summary=CASE WHEN excluded.summary='' THEN video_cache.summary ELSE excluded.summary END,
           subs=CASE WHEN excluded.subs='' THEN video_cache.subs ELSE excluded.subs END,
           src_lang=excluded.src_lang, dst_lang=excluded.dst_lang,
           lines=CASE WHEN excluded.subs='' THEN video_cache.lines ELSE excluded.lines END,
           bytes=length(CASE WHEN excluded.summary='' THEN video_cache.summary ELSE excluded.summary END)+length(CASE WHEN excluded.subs='' THEN video_cache.subs ELSE excluded.subs END),
           updated=excluded.updated, accessed=excluded.accessed",
        params![key, p.url, p.title.chars().take(200).collect::<String>(), p.site, summary, subs, p.src_lang, p.dst_lang, lines, bytes as i64, now()],
    )?;
    evict(&c, limit_mb)?;
    drop(c);
    info(data_dir, &p.url)?.ok_or_else(|| AppError::invalid("保存失败"))
}

/// 总量超过上限时，从最久没用的开始删。
fn evict(c: &Connection, limit_mb: u64) -> AppResult<()> {
    let limit = (limit_mb.max(10) * 1024 * 1024) as i64;
    loop {
        let total: i64 = c.query_row("SELECT COALESCE(SUM(bytes),0) FROM video_cache", [], |r| r.get(0))?;
        if total <= limit {
            return Ok(());
        }
        let n = c.execute("DELETE FROM video_cache WHERE video_key = (SELECT video_key FROM video_cache ORDER BY accessed ASC LIMIT 1)", [])?;
        if n == 0 {
            return Ok(());
        }
    }
}

fn row_info(r: &rusqlite::Row<'_>) -> rusqlite::Result<Info> {
    Ok(Info {
        video_key: r.get(0)?,
        url: r.get(1)?,
        title: r.get(2)?,
        has_summary: r.get::<_, i64>(3)? > 0,
        has_subs: r.get::<_, i64>(4)? > 0,
        lines: r.get(5)?,
        src_lang: r.get(6)?,
        dst_lang: r.get(7)?,
        bytes: r.get(8)?,
        updated: r.get(9)?,
    })
}
const INFO_COLS: &str = "video_key,url,title,length(summary),length(subs),lines,src_lang,dst_lang,bytes,updated";

/// 一个视频（任意指向它的网址）的缓存概况。
pub fn info(data_dir: &Path, href: &str) -> AppResult<Option<Info>> {
    let c = conn(data_dir)?;
    Ok(c.query_row(&format!("SELECT {INFO_COLS} FROM video_cache WHERE video_key=?1"), params![video_key(href)], row_info).optional()?)
}

/// 一批网址的缓存概况（生词本列表用）。
pub fn infos(data_dir: &Path, hrefs: &[String]) -> AppResult<Vec<Info>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    for h in hrefs {
        if seen.insert(video_key(h)) {
            if let Some(i) = info(data_dir, h)? {
                out.push(i);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Full {
    pub info: Info,
    pub summary: Value,
    pub subs: Value,
}

/// 完整快照（给扩展恢复、给主程序查看）。
pub fn get(data_dir: &Path, href: &str) -> AppResult<Option<Full>> {
    let key = video_key(href);
    let c = conn(data_dir)?;
    let row: Option<(String, String)> =
        c.query_row("SELECT summary,subs FROM video_cache WHERE video_key=?1", params![key], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((s, b)) = row else { return Ok(None) };
    let _ = c.execute("UPDATE video_cache SET accessed=?1 WHERE video_key=?2", params![now(), key]);
    let info = c.query_row(&format!("SELECT {INFO_COLS} FROM video_cache WHERE video_key=?1"), params![key], row_info)?;
    let parse = |t: &str| if t.is_empty() { Value::Null } else { serde_json::from_str(t).unwrap_or(Value::Null) };
    Ok(Some(Full { info, summary: parse(&s), subs: parse(&b) }))
}

pub fn remove(data_dir: &Path, href: &str) -> AppResult<()> {
    conn(data_dir)?.execute("DELETE FROM video_cache WHERE video_key=?1", params![video_key(href)])?;
    Ok(())
}

pub fn clear(data_dir: &Path) -> AppResult<()> {
    conn(data_dir)?.execute("DELETE FROM video_cache", [])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub videos: i64,
    pub bytes: i64,
}

pub fn stats(data_dir: &Path) -> AppResult<Stats> {
    let c = conn(data_dir)?;
    Ok(c.query_row("SELECT COUNT(*), COALESCE(SUM(bytes),0) FROM video_cache", [], |r| Ok(Stats { videos: r.get(0)?, bytes: r.get(1)? }))?)
}

// ---------- 命令 ----------

use std::sync::Arc;
use tauri::State;

type St<'a> = State<'a, Arc<crate::AppState>>;

#[tauri::command]
pub fn videocache_infos(state: St<'_>, urls: Vec<String>) -> AppResult<Vec<Info>> {
    infos(&state.data_dir, &urls)
}

#[tauri::command]
pub fn videocache_get(state: St<'_>, url: String) -> AppResult<Option<Full>> {
    get(&state.data_dir, &url)
}

#[tauri::command]
pub fn videocache_remove(state: St<'_>, url: String) -> AppResult<()> {
    remove(&state.data_dir, &url)
}

#[tauri::command]
pub fn videocache_clear(state: St<'_>) -> AppResult<()> {
    clear(&state.data_dir)
}

#[tauri::command]
pub fn videocache_stats(state: St<'_>) -> AppResult<Stats> {
    stats(&state.data_dir)
}

/// 出处链接：从那一秒开始播放；`word` 非空时带上词，装了清影扩展会自动定位到那一句并高亮；`restore` 时扩展会先把清影里的缓存取回来。
pub fn source_link(href: &str, sec: f64, word: &str, restore: bool) -> String {
    let mut link = time_link(href, sec);
    let mut frag = vec![];
    if !word.trim().is_empty() {
        frag.push(format!("cc-word={}", url::form_urlencoded::byte_serialize(word.trim().as_bytes()).collect::<String>()));
        frag.push(format!("cc-t={}", sec.max(0.0).floor() as u64));
    }
    if restore {
        frag.push("cc-restore=1".to_string());
    }
    if !frag.is_empty() {
        link.push('#');
        link.push_str(&frag.join("&"));
    }
    link
}

/// 用默认浏览器打开出处（可带时间、词、恢复缓存）。
#[tauri::command]
pub fn videocache_open(app: tauri::AppHandle, url: String, sec: f64, word: Option<String>, restore: Option<bool>) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::invalid("这条生词没有可打开的出处网址。"));
    }
    let link = source_link(&url, sec, word.as_deref().unwrap_or(""), restore.unwrap_or(false));
    app.opener().open_url(link, None::<&str>).map_err(|e| AppError::msg(format!("无法打开链接：{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_match_the_extension() {
        assert_eq!(video_key("https://www.youtube.com/watch?v=abc123XYZ_-&list=x&t=5"), "youtube:abc123XYZ_-");
        assert_eq!(video_key("https://youtu.be/abc123XYZ_-?t=3"), "youtube:abc123XYZ_-");
        assert_eq!(video_key("https://www.youtube.com/shorts/abcdef12345"), "youtube:abcdef12345");
        assert_eq!(video_key("https://www.bilibili.com/video/BV1xx411c7mD/?p=2&t=61"), "bilibili:BV1xx411c7mD:p2");
        assert_eq!(video_key("https://www.bilibili.com/video/BV1xx411c7mD"), "bilibili:BV1xx411c7mD");
        assert_eq!(video_key("https://example.com/a?b=1#frag"), "url:https://example.com/a?b=1");
    }

    #[test]
    fn time_links_start_from_the_second() {
        assert_eq!(time_link("https://www.youtube.com/watch?v=abc123XYZ_-&list=x", 83.9), "https://www.youtube.com/watch?v=abc123XYZ_-&list=x&t=83s");
        assert_eq!(time_link("https://www.bilibili.com/video/BV1xx411c7mD/?p=2", 61.0), "https://www.bilibili.com/video/BV1xx411c7mD/?p=2&t=61");
        assert_eq!(time_link("https://youtu.be/abc123XYZ_-#x", 0.0), "https://youtu.be/abc123XYZ_-");
    }

    #[test]
    fn source_links_carry_word_and_restore_flags() {
        let u = "https://www.youtube.com/watch?v=abc123XYZ_-";
        assert_eq!(source_link(u, 22.4, "", false), "https://www.youtube.com/watch?v=abc123XYZ_-&t=22s");
        assert_eq!(source_link(u, 22.4, "ephemeral", true), "https://www.youtube.com/watch?v=abc123XYZ_-&t=22s#cc-word=ephemeral&cc-t=22&cc-restore=1");
        assert_eq!(source_link(u, 0.0, "", true), "https://www.youtube.com/watch?v=abc123XYZ_-#cc-restore=1");
    }

    #[test]
    fn put_get_merge_evict_and_remove() {
        let dir = std::env::temp_dir().join(format!("cc-vcache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let url = "https://www.youtube.com/watch?v=aB3xYz12345";
        let a = put(
            &dir,
            &Put {
                url: url.into(),
                title: "How attention works".into(),
                site: "youtube".into(),
                summary: json!({"res": {"summary": "要点"}}),
                ..Default::default()
            },
            500,
        )
        .unwrap();
        assert!(a.has_summary && !a.has_subs);
        // 只补字幕，不会冲掉已有摘要
        let b = put(
            &dir,
            &Put {
                url: format!("{url}&t=9"),
                subs: json!({"lines": [{"t": 0, "text": "hi"}, {"t": 1000, "text": "yo"}], "trans": ["嗨", "哟"]}),
                src_lang: "en".into(),
                dst_lang: "zh".into(),
                ..Default::default()
            },
            500,
        )
        .unwrap();
        assert!(b.has_summary && b.has_subs);
        assert_eq!(b.lines, 2);
        assert_eq!(b.video_key, "youtube:aB3xYz12345");
        let full = get(&dir, url).unwrap().unwrap();
        assert_eq!(full.summary["res"]["summary"], "要点");
        assert_eq!(full.subs["trans"][1], "哟");
        assert_eq!(infos(&dir, &[url.into(), format!("{url}&t=3")]).unwrap().len(), 1, "同一个视频只算一个");
        assert!(put(&dir, &Put { url: url.into(), ..Default::default() }, 500).is_err(), "什么都没有不保存");
        assert!(put(&dir, &Put { url: "ftp://x".into(), summary: json!({"a": 1}), ..Default::default() }, 500).is_err());
        let big = "x".repeat(MAX_ONE + 10);
        assert!(put(&dir, &Put { url: "https://example.com/v".into(), summary: json!({"a": big}), ..Default::default() }, 500).is_err());
        assert_eq!(stats(&dir).unwrap().videos, 1);
        remove(&dir, url).unwrap();
        assert!(get(&dir, url).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
