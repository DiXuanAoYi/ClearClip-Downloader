//! 本地 SQLite：解析历史和已下载文件（媒体库），用于去重。

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::model::{AppResult, MediaInfo, MediaKind};

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: i64,
    pub platform: String,
    pub media_id: String,
    pub title: String,
    pub author: String,
    pub cover: Option<String>,
    pub kind: String,
    pub source_url: String,
    pub created_at: i64,
    pub info: MediaInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: i64,
    pub platform: String,
    pub media_id: String,
    pub asset_id: String,
    pub title: String,
    pub author: String,
    pub cover: Option<String>,
    pub path: String,
    pub size: i64,
    pub finished_at: i64,
    pub exists: bool,
    pub kind: String,
    pub source: String,
    pub source_url: String,
    pub platform_name: String,
    /// 本地缓存的封面
    pub cover_path: Option<String>,
    pub favorite: bool,
    /// 评分 0–5，0 表示未评分
    pub rating: i64,
    pub note: String,
    pub tags: Vec<String>,
    pub duration_ms: Option<i64>,
}

/// 媒体库筛选条件。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LibraryFilter {
    pub query: String,
    pub platform: Option<String>,
    pub kind: Option<String>,
    pub source: Option<String>,
    /// 只看这个时间（Unix 秒）之后完成的
    pub since: Option<i64>,
    /// 只看文件已丢失的
    pub missing_only: bool,
    pub tag: Option<String>,
    pub favorite_only: bool,
    pub min_rating: i64,
    /// finished（默认）/ size / title / rating
    pub sort: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCount {
    pub platform: String,
    pub name: String,
    pub count: i64,
}

/// 一键清除要清哪些记录。
#[derive(Debug, Clone, Copy, Default)]
pub struct DbWipe {
    pub tasks: bool,
    pub history: bool,
    pub library: bool,
    pub inbox: bool,
    pub subscriptions: bool,
}

pub struct NewDownload<'a> {
    pub platform: &'a str,
    pub media_id: &'a str,
    pub asset_id: &'a str,
    pub title: &'a str,
    pub author: &'a str,
    pub cover: Option<&'a str>,
    pub path: &'a str,
    pub size: i64,
    pub kind: &'a str,
    pub source: &'a str,
    pub source_url: &'a str,
    pub platform_name: &'a str,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Db::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> AppResult<Db> {
        Db::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> AppResult<Db> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform TEXT NOT NULL,
                media_id TEXT NOT NULL,
                title TEXT NOT NULL,
                author TEXT NOT NULL,
                cover TEXT,
                kind TEXT NOT NULL,
                source_url TEXT NOT NULL,
                info_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                UNIQUE(platform, media_id)
             );
             CREATE TABLE IF NOT EXISTS downloads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform TEXT NOT NULL,
                media_id TEXT NOT NULL,
                asset_id TEXT NOT NULL,
                title TEXT NOT NULL,
                author TEXT NOT NULL,
                cover TEXT,
                path TEXT NOT NULL,
                size INTEGER NOT NULL,
                finished_at INTEGER NOT NULL,
                UNIQUE(platform, media_id, asset_id)
             );
             CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                media_json TEXT NOT NULL,
                asset_json TEXT NOT NULL,
                file_path TEXT NOT NULL,
                status TEXT NOT NULL,
                received INTEGER NOT NULL DEFAULT 0,
                total INTEGER,
                error TEXT,
                meta_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL,
                finished_at INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at);
             CREATE INDEX IF NOT EXISTS idx_downloads_finished ON downloads(finished_at);",
        )?;
        // 阶段 4 新增的列：旧数据库补上
        let cols: Vec<String> = conn.prepare("PRAGMA table_info(downloads)")?.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<_, _>>()?;
        for (name, def) in [
            ("kind", "TEXT NOT NULL DEFAULT 'video'"),
            ("source", "TEXT NOT NULL DEFAULT 'manual'"),
            ("source_url", "TEXT NOT NULL DEFAULT ''"),
            ("platform_name", "TEXT NOT NULL DEFAULT ''"),
            ("cover_path", "TEXT"),
            ("favorite", "INTEGER NOT NULL DEFAULT 0"),
            ("rating", "INTEGER NOT NULL DEFAULT 0"),
            ("note", "TEXT NOT NULL DEFAULT ''"),
            ("duration_ms", "INTEGER"),
            ("phash", "TEXT"),
            ("quick_hash", "TEXT"),
        ] {
            if !cols.iter().any(|c| c == name) {
                conn.execute_batch(&format!("ALTER TABLE downloads ADD COLUMN {name} {def}"))?;
            }
        }
        conn.execute_batch(crate::subs::SCHEMA)?;
        conn.execute_batch(crate::live::SCHEMA)?;
        conn.execute_batch(crate::inbox::SCHEMA)?;
        conn.execute_batch(crate::library::SCHEMA)?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_downloads_platform ON downloads(platform);
             CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);",
        )?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 某平台最近一次成功解析的原链接（健康检查用作示例）。
    pub fn latest_source_url(&self, platform: &str) -> AppResult<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT source_url FROM history WHERE platform=?1 AND source_url<>'' ORDER BY created_at DESC LIMIT 1", params![platform], |r| r.get(0))
            .optional()?)
    }

    pub fn upsert_history(&self, info: &MediaInfo) -> AppResult<()> {
        let kind = match info.kind {
            MediaKind::Video => "video",
            MediaKind::Images => "images",
            MediaKind::Audio => "audio",
            MediaKind::Playlist => "playlist",
        };
        self.conn().execute(
            "INSERT INTO history (platform, media_id, title, author, cover, kind, source_url, info_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(platform, media_id) DO UPDATE SET
                title=excluded.title, author=excluded.author, cover=excluded.cover, kind=excluded.kind,
                source_url=excluded.source_url, info_json=excluded.info_json, created_at=excluded.created_at",
            params![info.platform, info.id, info.title, info.author, info.cover, kind, info.source_url, serde_json::to_string(info)?, now()],
        )?;
        Ok(())
    }

    pub fn list_history(&self, query: &str, limit: i64) -> AppResult<Vec<HistoryItem>> {
        let like = format!("%{}%", query.trim());
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, platform, media_id, title, author, cover, kind, source_url, created_at, info_json FROM history
             WHERE title LIKE ?1 OR author LIKE ?1 ORDER BY created_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![like, limit], |r| {
            let info_json: String = r.get(9)?;
            Ok((
                HistoryItem {
                    id: r.get(0)?,
                    platform: r.get(1)?,
                    media_id: r.get(2)?,
                    title: r.get(3)?,
                    author: r.get(4)?,
                    cover: r.get(5)?,
                    kind: r.get(6)?,
                    source_url: r.get(7)?,
                    created_at: r.get(8)?,
                    info: placeholder_info(),
                },
                info_json,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut item, json) = row?;
            match serde_json::from_str(&json) {
                Ok(info) => item.info = info,
                Err(_) => continue,
            }
            out.push(item);
        }
        Ok(out)
    }

    pub fn delete_history(&self, id: i64) -> AppResult<()> {
        self.conn().execute("DELETE FROM history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_history(&self) -> AppResult<()> {
        self.conn().execute("DELETE FROM history", [])?;
        Ok(())
    }

    /// 这个路径是不是媒体库里的文件。
    pub fn path_known(&self, path: &str) -> bool {
        self.conn().query_row("SELECT 1 FROM downloads WHERE path = ?1 LIMIT 1", params![path], |_| Ok(())).optional().ok().flatten().is_some()
    }

    /// 一键清除：按选项删除各类记录，并把数据库文件压缩（旧内容不再留在文件里）。
    /// 返回被跳过的项目说明。回收站记录不动，免得里面的文件变成找不回来的孤儿。
    pub fn wipe(&self, w: &DbWipe) -> AppResult<Vec<String>> {
        let mut skipped = vec![];
        {
            let conn = self.conn();
            if w.tasks {
                conn.execute("DELETE FROM jobs", [])?;
            }
            if w.history {
                conn.execute("DELETE FROM history", [])?;
            }
            if w.library {
                for t in ["downloads", "item_tags", "tags", "cues"] {
                    conn.execute(&format!("DELETE FROM {t}"), [])?;
                }
            }
            if w.inbox {
                conn.execute("DELETE FROM inbox", [])?;
            }
            if w.subscriptions {
                conn.execute("DELETE FROM subscription_items", [])?;
                conn.execute("DELETE FROM subscriptions", [])?;
                conn.execute("DELETE FROM live_recordings WHERE status <> 'recording'", [])?;
                let active: i64 = conn.query_row("SELECT COUNT(*) FROM live_recordings WHERE status = 'recording'", [], |r| r.get(0))?;
                if active > 0 {
                    skipped.push("有正在录制的直播，直播间列表已保留".to_string());
                } else {
                    conn.execute("DELETE FROM live_rooms", [])?;
                }
            }
        }
        self.checkpoint()?;
        Ok(skipped)
    }

    /// 隐私模式退出时清理：解析历史、收件箱、已结束的任务记录。
    pub fn purge_private(&self) -> AppResult<()> {
        {
            let conn = self.conn();
            conn.execute("DELETE FROM history", [])?;
            conn.execute("DELETE FROM inbox", [])?;
            conn.execute("DELETE FROM jobs WHERE status IN ('done', 'failed', 'canceled')", [])?;
        }
        self.checkpoint()
    }

    /// 合并 WAL 并压缩数据库文件。
    pub fn checkpoint(&self) -> AppResult<()> {
        let conn = self.conn();
        let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    pub fn record_download(&self, d: &NewDownload<'_>) -> AppResult<()> {
        self.conn().execute(
            "INSERT INTO downloads (platform, media_id, asset_id, title, author, cover, path, size, finished_at, kind, source, source_url, platform_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(platform, media_id, asset_id) DO UPDATE SET
                title=excluded.title, author=excluded.author, cover=excluded.cover, path=excluded.path,
                size=excluded.size, finished_at=excluded.finished_at, kind=excluded.kind, source=excluded.source,
                source_url=excluded.source_url, platform_name=excluded.platform_name",
            params![d.platform, d.media_id, d.asset_id, d.title, d.author, d.cover, d.path, d.size, now(), d.kind, d.source, d.source_url, d.platform_name],
        )?;
        Ok(())
    }

    /// 封面缓存完成后记录本地路径。
    pub fn set_cover_path(&self, platform: &str, media_id: &str, path: &str) -> AppResult<()> {
        self.conn().execute("UPDATE downloads SET cover_path=?3 WHERE platform=?1 AND media_id=?2", params![platform, media_id, path])?;
        Ok(())
    }

    pub fn library_item(&self, id: i64) -> AppResult<Option<LibraryItem>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {LIB_COLS} FROM downloads WHERE id=?1"))?;
        Ok(stmt.query_row(params![id], lib_row).optional()?)
    }

    pub fn library_platforms(&self) -> AppResult<Vec<PlatformCount>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT platform, MAX(platform_name), COUNT(*) FROM downloads GROUP BY platform ORDER BY COUNT(*) DESC")?;
        let rows = stmt.query_map([], |r| {
            let platform: String = r.get(0)?;
            let name: Option<String> = r.get(1)?;
            Ok(PlatformCount { name: name.filter(|n| !n.is_empty()).unwrap_or_else(|| platform.clone()), platform, count: r.get(2)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn search_library(&self, f: &LibraryFilter, limit: i64) -> AppResult<Vec<LibraryItem>> {
        let mut sql = format!(
            "SELECT {LIB_COLS} FROM downloads WHERE (title LIKE ?1 OR author LIKE ?1 OR note LIKE ?1 \
             OR EXISTS (SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE it.item_id = downloads.id AND t.name LIKE ?1))"
        );
        let mut args: Vec<rusqlite::types::Value> = vec![format!("%{}%", f.query.trim()).into()];
        fn push(args: &mut Vec<rusqlite::types::Value>, sql: &mut String, cond: &str, v: rusqlite::types::Value) {
            args.push(v);
            sql.push_str(&format!(" AND {cond}?{}", args.len()));
        }
        if let Some(p) = f.platform.as_ref().filter(|p| !p.is_empty()) {
            push(&mut args, &mut sql, "platform=", p.clone().into());
        }
        if let Some(k) = f.kind.as_ref().filter(|k| !k.is_empty()) {
            push(&mut args, &mut sql, "kind=", k.clone().into());
        }
        if let Some(s) = f.source.as_ref().filter(|s| !s.is_empty()) {
            push(&mut args, &mut sql, "source=", s.clone().into());
        }
        if let Some(t) = f.since {
            push(&mut args, &mut sql, "finished_at>=", t.into());
        }
        if let Some(tag) = f.tag.as_ref().filter(|t| !t.is_empty()) {
            args.push(tag.clone().into());
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE it.item_id = downloads.id AND t.name = ?{} COLLATE NOCASE)",
                args.len()
            ));
        }
        if f.favorite_only {
            sql.push_str(" AND favorite = 1");
        }
        if f.min_rating > 0 {
            push(&mut args, &mut sql, "rating>=", f.min_rating.into());
        }
        let order = match f.sort.as_deref() {
            Some("size") => "size DESC, id DESC",
            Some("title") => "title COLLATE NOCASE ASC, id DESC",
            Some("rating") => "rating DESC, finished_at DESC, id DESC",
            _ => "finished_at DESC, id DESC",
        };
        args.push(if f.missing_only { 20_000i64 } else { limit }.into());
        sql.push_str(&format!(" ORDER BY {order} LIMIT ?{}", args.len()));
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), lib_row)?;
        let mut items: Vec<LibraryItem> = rows.collect::<Result<_, _>>()?;
        if f.missing_only {
            items.retain(|i| !i.exists);
            items.truncate(limit as usize);
        }
        Ok(items)
    }

    /// 已下载且文件仍在磁盘上时返回文件路径。
    pub fn existing_download(&self, platform: &str, media_id: &str, asset_id: &str) -> AppResult<Option<String>> {
        let path: Option<String> = self
            .conn()
            .query_row("SELECT path FROM downloads WHERE platform=?1 AND media_id=?2 AND asset_id=?3", params![platform, media_id, asset_id], |r| r.get(0))
            .optional()?;
        Ok(path.filter(|p| Path::new(p).exists()))
    }

    /// 列表条目里已经下载过的：同一平台、同一作品 ID，或原始链接相同。返回条目 ID。
    /// 只统计视频和音频（字幕、封面不算）。
    pub fn downloaded_entry_ids(&self, platform: &str, entries: &[(String, String)]) -> AppResult<Vec<String>> {
        let conn = self.conn();
        let mut ids = std::collections::HashSet::new();
        let mut urls = std::collections::HashSet::new();
        let mut stmt = conn.prepare("SELECT media_id, source_url FROM downloads WHERE platform=?1 AND kind IN ('video','audio')")?;
        for row in stmt.query_map(params![platform], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, url) = row?;
            ids.insert(id);
            if !url.is_empty() {
                urls.insert(url);
            }
        }
        // 平台标识不同但链接相同的（例如内置解析器和 yt-dlp 解析同一个网站）
        let mut stmt = conn.prepare("SELECT source_url FROM downloads WHERE source_url<>'' AND kind IN ('video','audio')")?;
        for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
            urls.insert(row?);
        }
        Ok(entries.iter().filter(|(id, url)| ids.contains(id) || urls.contains(url)).map(|(id, _)| id.clone()).collect())
    }

    pub fn list_library(&self, query: &str, limit: i64) -> AppResult<Vec<LibraryItem>> {
        self.search_library(&LibraryFilter { query: query.to_string(), ..Default::default() }, limit)
    }

    /// 删除媒体库记录，返回该记录的文件路径。
    pub fn delete_library(&self, id: i64) -> AppResult<Option<String>> {
        let conn = self.conn();
        let path: Option<String> = conn.query_row("SELECT path FROM downloads WHERE id=?1", params![id], |r| r.get(0)).optional()?;
        conn.execute("DELETE FROM downloads WHERE id=?1", params![id])?;
        conn.execute("DELETE FROM item_tags WHERE item_id=?1", params![id])?;
        conn.execute("DELETE FROM cues WHERE sub_item=?1", params![id])?;
        Ok(path)
    }
}

const LIB_COLS: &str = "id, platform, media_id, asset_id, title, author, cover, path, size, finished_at, kind, source, source_url, platform_name, cover_path, favorite, rating, note, duration_ms, \
    (SELECT GROUP_CONCAT(t.name, char(31)) FROM item_tags it JOIN tags t ON t.id = it.tag_id WHERE it.item_id = downloads.id)";

fn lib_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryItem> {
    let path: String = r.get(7)?;
    Ok(LibraryItem {
        id: r.get(0)?,
        platform: r.get(1)?,
        media_id: r.get(2)?,
        asset_id: r.get(3)?,
        title: r.get(4)?,
        author: r.get(5)?,
        cover: r.get(6)?,
        exists: Path::new(&path).exists(),
        path,
        size: r.get(8)?,
        finished_at: r.get(9)?,
        kind: r.get(10)?,
        source: r.get(11)?,
        source_url: r.get(12)?,
        platform_name: r.get(13)?,
        cover_path: r.get::<_, Option<String>>(14)?.filter(|p| Path::new(p).exists()),
        favorite: r.get::<_, i64>(15)? != 0,
        rating: r.get(16)?,
        note: r.get(17)?,
        duration_ms: r.get(18)?,
        tags: r.get::<_, Option<String>>(19)?.map(|t| t.split('\u{1f}').map(String::from).collect()).unwrap_or_default(),
    })
}

/// 持久化的下载任务。
#[derive(Debug, Clone)]
pub struct JobRow {
    pub id: i64,
    pub media_json: String,
    pub asset_json: String,
    pub file_path: String,
    pub status: String,
    pub received: i64,
    pub total: Option<i64>,
    pub error: Option<String>,
    pub meta_json: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

impl Db {
    pub fn insert_job(&self, r: &JobRow) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO jobs (media_json, asset_json, file_path, status, received, total, error, meta_json, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![r.media_json, r.asset_json, r.file_path, r.status, r.received, r.total, r.error, r.meta_json, r.created_at, r.finished_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_job(
        &self,
        id: i64,
        status: &str,
        received: i64,
        total: Option<i64>,
        error: Option<&str>,
        file_path: &str,
        meta_json: &str,
        finished_at: Option<i64>,
    ) -> AppResult<()> {
        self.conn().execute(
            "UPDATE jobs SET status=?2, received=?3, total=?4, error=?5, file_path=?6, meta_json=?7, finished_at=?8 WHERE id=?1",
            params![id, status, received, total, error, file_path, meta_json, finished_at],
        )?;
        Ok(())
    }

    pub fn update_job_asset(&self, id: i64, asset_json: &str) -> AppResult<()> {
        self.conn().execute("UPDATE jobs SET asset_json=?2 WHERE id=?1", params![id, asset_json])?;
        Ok(())
    }

    pub fn delete_job(&self, id: i64) -> AppResult<()> {
        self.conn().execute("DELETE FROM jobs WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn delete_jobs_with_status(&self, statuses: &[&str]) -> AppResult<()> {
        let conn = self.conn();
        for s in statuses {
            conn.execute("DELETE FROM jobs WHERE status=?1", params![s])?;
        }
        Ok(())
    }

    pub fn load_jobs(&self) -> AppResult<Vec<JobRow>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, media_json, asset_json, file_path, status, received, total, error, meta_json, created_at, finished_at FROM jobs ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(JobRow {
                id: r.get(0)?,
                media_json: r.get(1)?,
                asset_json: r.get(2)?,
                file_path: r.get(3)?,
                status: r.get(4)?,
                received: r.get(5)?,
                total: r.get(6)?,
                error: r.get(7)?,
                meta_json: r.get(8)?,
                created_at: r.get(9)?,
                finished_at: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn placeholder_info() -> MediaInfo {
    MediaInfo {
        platform: String::new(),
        platform_name: String::new(),
        id: String::new(),
        source_url: String::new(),
        title: String::new(),
        author: String::new(),
        cover: None,
        duration_ms: None,
        kind: MediaKind::Video,
        width: None,
        height: None,
        published_at: None,
        assets: vec![],
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Asset;

    fn sample(id: &str, title: &str) -> MediaInfo {
        MediaInfo {
            platform: "douyin".into(),
            platform_name: "抖音".into(),
            id: id.into(),
            title: title.into(),
            author: "作者".into(),
            assets: vec![Asset::video("https://x/v.mp4".into(), None, None)],
            ..placeholder_info()
        }
    }

    #[test]
    fn jobs_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let row = JobRow {
            id: 0,
            media_json: "{}".into(),
            asset_json: "{}".into(),
            file_path: "/x.mp4".into(),
            status: "queued".into(),
            received: 0,
            total: None,
            error: None,
            meta_json: "{}".into(),
            created_at: 1,
            finished_at: None,
        };
        let id = db.insert_job(&row).unwrap();
        db.update_job(id, "paused", 10, Some(20), None, "/x.mp4", r#"{"etag":"e"}"#, None).unwrap();
        let jobs = db.load_jobs().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!((jobs[0].status.as_str(), jobs[0].received, jobs[0].total), ("paused", 10, Some(20)));
        db.delete_jobs_with_status(&["paused"]).unwrap();
        assert!(db.load_jobs().unwrap().is_empty());
    }

    #[test]
    fn history_upsert_dedupes_and_searches() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_history(&sample("1", "板栗焖鸡")).unwrap();
        db.upsert_history(&sample("2", "手冲咖啡")).unwrap();
        db.upsert_history(&sample("1", "板栗焖鸡（更新）")).unwrap();
        let all = db.list_history("", 50).unwrap();
        assert_eq!(all.len(), 2);
        let hit = db.list_history("板栗", 50).unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].title, "板栗焖鸡（更新）");
        assert_eq!(hit[0].info.assets.len(), 1);
    }

    #[test]
    fn downloaded_entries_by_id_or_url() {
        let db = Db::open_in_memory().unwrap();
        let rec = |platform: &str, media_id: &str, kind: &str, source_url: &str| {
            db.record_download(&NewDownload {
                platform,
                media_id,
                asset_id: &format!("a-{kind}"),
                title: "t",
                author: "",
                cover: None,
                path: "/x",
                size: 1,
                kind,
                source: "manual",
                source_url,
                platform_name: platform,
            })
            .unwrap();
        };
        rec("youtube", "aaa", "video", "https://www.youtube.com/watch?v=aaa");
        rec("youtube", "sub-only", "subtitle", "https://www.youtube.com/watch?v=sub-only");
        rec("other", "zzz", "video", "https://v.example.com/p/9");
        let entries: Vec<(String, String)> = [
            ("aaa", "https://www.youtube.com/watch?v=aaa"),
            ("bbb", "https://www.youtube.com/watch?v=bbb"),
            ("sub-only", "https://www.youtube.com/watch?v=sub-only"),
            ("n9", "https://v.example.com/p/9"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let got = db.downloaded_entry_ids("youtube", &entries).unwrap();
        assert_eq!(got, vec!["aaa", "n9"], "subtitle-only downloads do not count; same URL under another platform does");
    }

    #[test]
    fn existing_download_requires_file_on_disk() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("clearclip-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.mp4");
        std::fs::write(&file, b"x").unwrap();
        let path = file.to_string_lossy().to_string();
        db.record_download(&NewDownload {
            platform: "douyin",
            media_id: "1",
            asset_id: "video",
            title: "t",
            author: "a",
            cover: None,
            path: &path,
            size: 1,
            kind: "video",
            source: "manual",
            source_url: "",
            platform_name: "抖音",
        })
        .unwrap();
        assert_eq!(db.existing_download("douyin", "1", "video").unwrap(), Some(path.clone()));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(db.existing_download("douyin", "1", "video").unwrap(), None);
        assert_eq!(db.list_library("", 10).unwrap().len(), 1);
        assert!(!db.list_library("", 10).unwrap()[0].exists);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn seed_everything(db: &Db) {
        db.upsert_history(&sample("1", "私密作品")).unwrap();
        db.record_download(&NewDownload {
            platform: "douyin",
            media_id: "1",
            asset_id: "a",
            title: "私密作品",
            author: "",
            cover: None,
            path: "/x.mp4",
            size: 1,
            kind: "video",
            source: "manual",
            source_url: "https://v.douyin.com/abc",
            platform_name: "抖音",
        })
        .unwrap();
        db.inbox_add("phone", "d1", "手机", "https://v.douyin.com/abc", "done").unwrap();
        for status in ["done", "failed", "canceled", "paused", "running"] {
            db.insert_job(&JobRow {
                id: 0,
                media_json: "{}".into(),
                asset_json: "{}".into(),
                file_path: "/x.mp4".into(),
                status: status.into(),
                received: 0,
                total: None,
                error: None,
                meta_json: "{}".into(),
                created_at: 1,
                finished_at: None,
            })
            .unwrap();
        }
    }

    #[test]
    fn purge_private_clears_history_inbox_and_finished_jobs_only() {
        let db = Db::open_in_memory().unwrap();
        seed_everything(&db);
        db.purge_private().unwrap();
        assert!(db.list_history("", 10).unwrap().is_empty());
        assert!(db.inbox_list(&crate::inbox::InboxFilter::default()).unwrap().is_empty());
        let left: Vec<String> = db.load_jobs().unwrap().into_iter().map(|j| j.status).collect();
        assert_eq!(left, vec!["paused", "running"], "unfinished tasks are kept so they can resume");
        assert_eq!(db.list_library("", 10).unwrap().len(), 1, "the library is not touched");
    }

    #[test]
    fn wipe_only_removes_what_was_selected() {
        let db = Db::open_in_memory().unwrap();
        seed_everything(&db);
        db.wipe(&DbWipe { history: true, ..Default::default() }).unwrap();
        assert!(db.list_history("", 10).unwrap().is_empty());
        assert_eq!(db.list_library("", 10).unwrap().len(), 1);
        assert_eq!(db.load_jobs().unwrap().len(), 5);
        db.wipe(&DbWipe { library: true, tasks: true, inbox: true, subscriptions: true, ..Default::default() }).unwrap();
        assert!(db.list_library("", 10).unwrap().is_empty());
        assert!(db.load_jobs().unwrap().is_empty());
        assert!(db.inbox_list(&crate::inbox::InboxFilter::default()).unwrap().is_empty());
    }
}
