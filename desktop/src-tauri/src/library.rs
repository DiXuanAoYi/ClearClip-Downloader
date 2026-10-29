//! 媒体库扩展：标签 / 收藏 / 评分 / 备注、导入已有文件、磁盘统计、回收站、按规则整理目录、
//! 重复文件检测（完全相同 + 画面相似）、字幕全文搜索。

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::{now, Db, LibraryItem};
use crate::error::{AppError, AppResult};
use crate::naming::sanitize;
use crate::subtitle;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);
CREATE TABLE IF NOT EXISTS item_tags (
    item_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (item_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_item_tags_tag ON item_tags(tag_id);
CREATE TABLE IF NOT EXISTS cues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sub_item INTEGER NOT NULL,
    platform TEXT NOT NULL,
    media_id TEXT NOT NULL,
    lang TEXT NOT NULL DEFAULT '',
    start_ms INTEGER NOT NULL,
    end_ms INTEGER NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cues_sub ON cues(sub_item);
CREATE INDEX IF NOT EXISTS idx_cues_media ON cues(platform, media_id);
CREATE TABLE IF NOT EXISTS trash (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_json TEXT NOT NULL,
    original_path TEXT NOT NULL,
    trash_path TEXT NOT NULL,
    title TEXT NOT NULL,
    size INTEGER NOT NULL,
    deleted_at INTEGER NOT NULL
);
";

/// 回收站目录名（放在下载目录里，和文件在同一个磁盘上，移动时不用复制）。
pub const TRASH_DIR: &str = ".ClearClip-Trash";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    pub key: String,
    pub name: String,
    pub count: i64,
    pub size: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BigFile {
    pub id: i64,
    pub title: String,
    pub path: String,
    pub size: i64,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub count: i64,
    pub total_size: i64,
    pub missing: i64,
    pub by_kind: Vec<Bucket>,
    pub by_platform: Vec<Bucket>,
    pub by_author: Vec<Bucket>,
    pub by_month: Vec<Bucket>,
    pub largest: Vec<BigFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub id: i64,
    pub title: String,
    pub original_path: String,
    pub size: i64,
    pub deleted_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CueHit {
    /// 视频 / 音频在媒体库里的记录（没有时为 None）
    pub item_id: Option<i64>,
    pub title: String,
    pub path: String,
    pub lang: String,
    pub start_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePlan {
    pub id: i64,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MoveReport {
    pub moved: usize,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: usize,
    pub subtitles: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    /// exact（内容完全相同）/ similar（画面相似）
    pub kind: String,
    pub items: Vec<LibraryItem>,
}

pub const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "flv", "ts", "m4v", "wmv", "mpg", "mpeg", "3gp"];
pub const AUDIO_EXTS: &[&str] = &["mp3", "m4a", "flac", "wav", "ogg", "opus", "aac", "wma"];
pub const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "heic", "bmp"];
pub const SUB_EXTS: &[&str] = &["srt", "vtt", "ass", "ssa"];

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub fn kind_of_ext(ext: &str) -> Option<&'static str> {
    if VIDEO_EXTS.contains(&ext) {
        Some("video")
    } else if AUDIO_EXTS.contains(&ext) {
        Some("audio")
    } else if IMAGE_EXTS.contains(&ext) {
        Some("image")
    } else if SUB_EXTS.contains(&ext) {
        Some("subtitle")
    } else {
        None
    }
}

/// 标签规范化：去首尾空白和控制字符，最长 30 个字符；忽略大小写去重，最多 20 个。
pub fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in tags {
        let t: String = t.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(30).collect();
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&t) && o.to_lowercase() == t.to_lowercase()) {
            out.push(t);
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

impl Db {
    // ---------- 收藏、评分、备注、标签 ----------

    pub fn set_item_meta(&self, id: i64, favorite: Option<bool>, rating: Option<i64>, note: Option<&str>) -> AppResult<()> {
        let conn = self.conn();
        if let Some(f) = favorite {
            conn.execute("UPDATE downloads SET favorite=?2 WHERE id=?1", params![id, f as i64])?;
        }
        if let Some(r) = rating {
            conn.execute("UPDATE downloads SET rating=?2 WHERE id=?1", params![id, r.clamp(0, 5)])?;
        }
        if let Some(n) = note {
            let n: String = n.chars().take(2000).collect();
            conn.execute("UPDATE downloads SET note=?2 WHERE id=?1", params![id, n])?;
        }
        Ok(())
    }

    fn tag_id(conn: &rusqlite::Connection, name: &str) -> AppResult<i64> {
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![name])?;
        Ok(conn.query_row("SELECT id FROM tags WHERE name=?1 COLLATE NOCASE", params![name], |r| r.get(0))?)
    }

    pub fn set_item_tags_by_key(&self, platform: &str, media_id: &str, asset_id: &str, tags: &[String]) -> AppResult<()> {
        match self.id_by_key(platform, media_id, asset_id)? {
            Some(id) => self.bulk_tags(&[id], tags, false),
            None => Ok(()),
        }
    }

    pub fn set_item_tags(&self, id: i64, tags: &[String]) -> AppResult<()> {
        let tags = clean_tags(tags);
        let conn = self.conn();
        conn.execute("DELETE FROM item_tags WHERE item_id=?1", params![id])?;
        for t in &tags {
            let tid = Self::tag_id(&conn, t)?;
            conn.execute("INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)", params![id, tid])?;
        }
        conn.execute("DELETE FROM tags WHERE id NOT IN (SELECT DISTINCT tag_id FROM item_tags)", [])?;
        Ok(())
    }

    /// 给多个项目批量加（或去掉）标签。
    pub fn bulk_tags(&self, ids: &[i64], tags: &[String], remove: bool) -> AppResult<()> {
        let tags = clean_tags(tags);
        let conn = self.conn();
        for t in &tags {
            let tid = Self::tag_id(&conn, t)?;
            for id in ids {
                if remove {
                    conn.execute("DELETE FROM item_tags WHERE item_id=?1 AND tag_id=?2", params![id, tid])?;
                } else {
                    conn.execute("INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)", params![id, tid])?;
                }
            }
        }
        conn.execute("DELETE FROM tags WHERE id NOT IN (SELECT DISTINCT tag_id FROM item_tags)", [])?;
        Ok(())
    }

    pub fn all_tags(&self) -> AppResult<Vec<TagCount>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT t.name, COUNT(it.item_id) FROM tags t JOIN item_tags it ON it.tag_id = t.id GROUP BY t.id ORDER BY COUNT(it.item_id) DESC, t.name",
        )?;
        let rows = stmt.query_map([], |r| Ok(TagCount { name: r.get(0)?, count: r.get(1)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn library_items(&self, ids: &[i64]) -> AppResult<Vec<LibraryItem>> {
        let mut out = vec![];
        for id in ids {
            if let Some(i) = self.library_item(*id)? {
                out.push(i);
            }
        }
        Ok(out)
    }

    pub fn set_duration(&self, id: i64, ms: i64) -> AppResult<()> {
        self.conn().execute("UPDATE downloads SET duration_ms=?2 WHERE id=?1", params![id, ms])?;
        Ok(())
    }

    /// 文件已经不存在的记录（返回 ID 和路径）。
    pub fn missing_ids(&self) -> AppResult<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id, path FROM downloads")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = vec![];
        for row in rows {
            let (id, path) = row?;
            if !Path::new(&path).exists() {
                out.push(id);
            }
        }
        Ok(out)
    }

    // ---------- 统计 ----------

    pub fn library_stats(&self) -> AppResult<Stats> {
        let conn = self.conn();
        let mut st = Stats::default();
        let (count, size): (i64, i64) = conn.query_row("SELECT COUNT(*), COALESCE(SUM(size),0) FROM downloads", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
        st.count = count;
        st.total_size = size;
        let buckets = |sql: &str| -> AppResult<Vec<Bucket>> {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map([], |r| {
                let key: String = r.get(0)?;
                let name: Option<String> = r.get(1)?;
                Ok(Bucket { name: name.filter(|n| !n.is_empty()).unwrap_or_else(|| key.clone()), key, count: r.get(2)?, size: r.get(3)? })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        };
        st.by_kind = buckets("SELECT kind, kind, COUNT(*), COALESCE(SUM(size),0) FROM downloads GROUP BY kind ORDER BY SUM(size) DESC")?;
        st.by_platform =
            buckets("SELECT platform, MAX(platform_name), COUNT(*), COALESCE(SUM(size),0) FROM downloads GROUP BY platform ORDER BY SUM(size) DESC")?;
        st.by_author =
            buckets("SELECT author, author, COUNT(*), COALESCE(SUM(size),0) FROM downloads WHERE author<>'' GROUP BY author ORDER BY SUM(size) DESC LIMIT 15")?;
        st.by_month = buckets(
            "SELECT strftime('%Y-%m', finished_at, 'unixepoch'), strftime('%Y-%m', finished_at, 'unixepoch'), COUNT(*), COALESCE(SUM(size),0)
             FROM downloads GROUP BY 1 ORDER BY 1 DESC LIMIT 12",
        )?;
        let mut stmt = conn.prepare("SELECT id, title, path, size FROM downloads ORDER BY size DESC LIMIT 20")?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(2)?;
            Ok(BigFile { id: r.get(0)?, title: r.get(1)?, exists: Path::new(&path).exists(), path, size: r.get(3)? })
        })?;
        st.largest = rows.collect::<Result<_, _>>()?;
        drop(stmt);
        let mut stmt = conn.prepare("SELECT path FROM downloads")?;
        let paths = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for p in paths {
            if !Path::new(&p?).exists() {
                st.missing += 1;
            }
        }
        Ok(st)
    }

    // ---------- 回收站 ----------

    pub fn trash_insert(&self, item_json: &str, original: &str, trash_path: &str, title: &str, size: i64) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO trash (item_json, original_path, trash_path, title, size, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![item_json, original, trash_path, title, size, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn trash_list(&self) -> AppResult<Vec<TrashItem>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id, title, original_path, size, deleted_at FROM trash ORDER BY deleted_at DESC, id DESC")?;
        let rows =
            stmt.query_map([], |r| Ok(TrashItem { id: r.get(0)?, title: r.get(1)?, original_path: r.get(2)?, size: r.get(3)?, deleted_at: r.get(4)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn trash_get(&self, id: i64) -> AppResult<Option<(String, String, String)>> {
        Ok(self
            .conn()
            .query_row("SELECT item_json, original_path, trash_path FROM trash WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?)
    }

    pub fn trash_remove(&self, id: i64) -> AppResult<()> {
        self.conn().execute("DELETE FROM trash WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn trash_older_than(&self, secs: i64) -> AppResult<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM trash WHERE deleted_at <= ?1")?;
        let rows = stmt.query_map(params![now() - secs], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // ---------- 字幕全文索引 ----------

    pub fn clear_cues(&self, sub_item: Option<i64>) -> AppResult<()> {
        match sub_item {
            Some(id) => self.conn().execute("DELETE FROM cues WHERE sub_item=?1", params![id])?,
            None => self.conn().execute("DELETE FROM cues", [])?,
        };
        Ok(())
    }

    pub fn insert_cues(&self, sub_item: i64, platform: &str, media_id: &str, lang: &str, cues: &[subtitle::Cue]) -> AppResult<usize> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM cues WHERE sub_item=?1", params![sub_item])?;
        let mut n = 0;
        {
            let mut stmt = tx.prepare("INSERT INTO cues (sub_item, platform, media_id, lang, start_ms, end_ms, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
            for c in cues {
                let text = c.lines.join(" ");
                if text.trim().is_empty() {
                    continue;
                }
                stmt.execute(params![sub_item, platform, media_id, lang, c.start as i64, c.end as i64, text])?;
                n += 1;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// 全文搜索字幕。每个作品最多返回 `per_item` 条命中。
    pub fn search_cues(&self, query: &str, limit: usize, per_item: usize) -> AppResult<Vec<CueHit>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let pat = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.platform, c.media_id, c.lang, c.start_ms, c.text,
                    (SELECT v.id FROM downloads v WHERE v.platform=c.platform AND v.media_id=c.media_id AND v.kind IN ('video','audio') ORDER BY v.id LIMIT 1),
                    (SELECT v.title FROM downloads v WHERE v.platform=c.platform AND v.media_id=c.media_id AND v.kind IN ('video','audio') ORDER BY v.id LIMIT 1),
                    (SELECT v.path FROM downloads v WHERE v.platform=c.platform AND v.media_id=c.media_id AND v.kind IN ('video','audio') ORDER BY v.id LIMIT 1)
             FROM cues c WHERE c.text LIKE ?1 ESCAPE '\\' ORDER BY c.id DESC LIMIT 5000",
        )?;
        let rows = stmt.query_map(params![pat], |r| {
            Ok((
                format!("{}\u{1f}{}", r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                CueHit {
                    lang: r.get(2)?,
                    start_ms: r.get(3)?,
                    text: r.get(4)?,
                    item_id: r.get(5)?,
                    title: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    path: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                },
            ))
        })?;
        let mut per: HashMap<String, usize> = HashMap::new();
        let mut out = vec![];
        for row in rows {
            let (key, hit) = row?;
            let n = per.entry(key).or_insert(0);
            if *n >= per_item {
                continue;
            }
            *n += 1;
            out.push(hit);
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    pub fn subtitle_items(&self) -> AppResult<Vec<LibraryItem>> {
        let all = self.search_library(&crate::db::LibraryFilter { kind: Some("subtitle".into()), ..Default::default() }, 100_000)?;
        Ok(all)
    }

    pub fn paths_set(&self) -> AppResult<HashSet<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path FROM downloads")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 同一个作品下已有的字幕资源 ID（下载的、导入的、AI 生成的）。
    pub fn subtitle_assets_for(&self, platform: &str, media_id: &str) -> AppResult<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT asset_id FROM downloads WHERE platform=?1 AND media_id=?2 AND kind='subtitle'")?;
        let rows = stmt.query_map(params![platform, media_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn item_by_path(&self, path: &str) -> AppResult<Option<LibraryItem>> {
        let id: Option<i64> = self.conn().query_row("SELECT id FROM downloads WHERE path=?1 ORDER BY id LIMIT 1", params![path], |r| r.get(0)).optional()?;
        match id {
            Some(id) => self.library_item(id),
            None => Ok(None),
        }
    }

    pub fn phash_of(&self, id: i64) -> AppResult<Option<String>> {
        Ok(self
            .conn()
            .query_row("SELECT phash FROM downloads WHERE id=?1", params![id], |r| r.get::<_, Option<String>>(0))
            .optional()?
            .flatten()
            .filter(|s| !s.is_empty()))
    }

    pub fn set_phash(&self, id: i64, phash: &str) -> AppResult<()> {
        self.conn().execute("UPDATE downloads SET phash=?2 WHERE id=?1", params![id, phash])?;
        Ok(())
    }

    pub fn id_by_key(&self, platform: &str, media_id: &str, asset_id: &str) -> AppResult<Option<i64>> {
        Ok(self
            .conn()
            .query_row("SELECT id FROM downloads WHERE platform=?1 AND media_id=?2 AND asset_id=?3", params![platform, media_id, asset_id], |r| r.get(0))
            .optional()?)
    }

    /// 还没有建立全文索引的字幕记录。
    pub fn unindexed_subtitles(&self) -> AppResult<Vec<LibraryItem>> {
        let ids: Vec<i64> = {
            let conn = self.conn();
            let mut stmt = conn.prepare("SELECT id FROM downloads WHERE kind='subtitle' AND id NOT IN (SELECT DISTINCT sub_item FROM cues)")?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        self.library_items(&ids)
    }

    pub fn cue_count(&self) -> AppResult<i64> {
        Ok(self.conn().query_row("SELECT COUNT(*) FROM cues", [], |r| r.get(0))?)
    }

    /// 把整个数据库做成一份一致的快照（备份用）。
    pub fn snapshot_to(&self, dest: &Path) -> AppResult<()> {
        let _ = std::fs::remove_file(dest);
        self.conn().execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
        Ok(())
    }

    fn insert_local(&self, r: &LocalRow<'_>) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO downloads (platform, media_id, asset_id, title, author, cover, path, size, finished_at, kind, source, source_url, platform_name)
             VALUES ('local', ?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, '', ?10)",
            params![r.media_id, r.asset_id, r.title, r.author, r.path, r.size, r.finished_at, r.kind, "import", "本地导入"],
        )?;
        Ok(conn.query_row("SELECT id FROM downloads WHERE platform='local' AND media_id=?1 AND asset_id=?2", params![r.media_id, r.asset_id], |r| r.get(0))?)
    }
}

/// 导入本地文件时登记的一行。
struct LocalRow<'a> {
    media_id: &'a str,
    asset_id: &'a str,
    title: &'a str,
    author: &'a str,
    path: &'a str,
    size: i64,
    kind: &'a str,
    finished_at: i64,
}

// ---------- 回收站（文件操作） ----------

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

fn unique_path(path: PathBuf) -> PathBuf {
    crate::naming::unique_path(path, &|p: &Path| p.exists())
}

/// 把媒体库里的一项（连文件）放进回收站。文件已经不在时只删除记录。返回回收站记录的 ID。
pub fn move_to_trash(db: &Db, root: &Path, item: &LibraryItem) -> AppResult<Option<i64>> {
    let path = Path::new(&item.path);
    let json = serde_json::to_string(&TrashedItem::from(item))?;
    let trash_id = if path.exists() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let dest = unique_path(root.join(TRASH_DIR).join(format!("{}-{}-{name}", now(), item.id)));
        move_file(path, &dest).map_err(|e| AppError::new(crate::error::ErrorKind::Disk, format!("无法移到回收站：{e}")))?;
        Some(db.trash_insert(&json, &item.path, &dest.to_string_lossy(), &item.title, item.size)?)
    } else {
        None
    };
    db.delete_library(item.id)?;
    Ok(trash_id)
}

/// 回收站里还原：文件放回原来的位置（已被占用时加序号），记录重新出现在媒体库。
pub fn restore_from_trash(db: &Db, trash_id: i64) -> AppResult<String> {
    let (json, original, trash_path) = db.trash_get(trash_id)?.ok_or_else(|| AppError::not_found("回收站里没有这一项。"))?;
    let t: TrashedItem = serde_json::from_str(&json).map_err(|e| AppError::msg(format!("回收站记录损坏：{e}")))?;
    let src = Path::new(&trash_path);
    if !src.exists() {
        db.trash_remove(trash_id)?;
        return Err(AppError::not_found("回收站里的文件已经不存在。"));
    }
    let dest = unique_path(PathBuf::from(&original));
    move_file(src, &dest).map_err(|e| AppError::new(crate::error::ErrorKind::Disk, format!("无法还原：{e}")))?;
    let conn = db.conn();
    conn.execute(
        "INSERT OR REPLACE INTO downloads (platform, media_id, asset_id, title, author, cover, path, size, finished_at, kind, source, source_url, platform_name, cover_path, favorite, rating, note, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            t.platform, t.media_id, t.asset_id, t.title, t.author, t.cover, dest.to_string_lossy(), t.size, t.finished_at, t.kind, t.source, t.source_url, t.platform_name, t.cover_path,
            t.favorite as i64, t.rating, t.note, t.duration_ms
        ],
    )?;
    let id: i64 =
        conn.query_row("SELECT id FROM downloads WHERE platform=?1 AND media_id=?2 AND asset_id=?3", params![t.platform, t.media_id, t.asset_id], |r| {
            r.get(0)
        })?;
    drop(conn);
    db.set_item_tags(id, &t.tags)?;
    db.trash_remove(trash_id)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// 永久删除回收站里的一项。
pub fn purge_trash(db: &Db, trash_id: i64) -> AppResult<()> {
    if let Some((_, _, trash_path)) = db.trash_get(trash_id)? {
        let _ = std::fs::remove_file(trash_path);
    }
    db.trash_remove(trash_id)
}

#[derive(Debug, Serialize, Deserialize)]
struct TrashedItem {
    platform: String,
    media_id: String,
    asset_id: String,
    title: String,
    author: String,
    cover: Option<String>,
    size: i64,
    finished_at: i64,
    kind: String,
    source: String,
    source_url: String,
    platform_name: String,
    cover_path: Option<String>,
    favorite: bool,
    rating: i64,
    note: String,
    tags: Vec<String>,
    duration_ms: Option<i64>,
}

impl From<&LibraryItem> for TrashedItem {
    fn from(i: &LibraryItem) -> Self {
        TrashedItem {
            platform: i.platform.clone(),
            media_id: i.media_id.clone(),
            asset_id: i.asset_id.clone(),
            title: i.title.clone(),
            author: i.author.clone(),
            cover: i.cover.clone(),
            size: i.size,
            finished_at: i.finished_at,
            kind: i.kind.clone(),
            source: i.source.clone(),
            source_url: i.source_url.clone(),
            platform_name: i.platform_name.clone(),
            cover_path: i.cover_path.clone(),
            favorite: i.favorite,
            rating: i.rating,
            note: i.note.clone(),
            tags: i.tags.clone(),
            duration_ms: i.duration_ms,
        }
    }
}

// ---------- 按规则整理目录 ----------

/// 渲染目录模板：`{platform}` `{author}` `{year}` `{month}` `{day}` `{date}` `{kind}`，用 `/` 分级。
pub fn render_dir(template: &str, primary: &LibraryItem) -> PathBuf {
    let dt = chrono::DateTime::from_timestamp(primary.finished_at, 0).unwrap_or_default();
    let platform = if primary.platform_name.is_empty() { primary.platform.clone() } else { primary.platform_name.clone() };
    let author = if primary.author.trim().is_empty() { "未知作者".to_string() } else { primary.author.clone() };
    let mut out = PathBuf::new();
    for part in template.split(['/', '\\']) {
        let s = part
            .replace("{platform}", &platform)
            .replace("{author}", &author)
            .replace("{year}", &dt.format("%Y").to_string())
            .replace("{month}", &dt.format("%m").to_string())
            .replace("{day}", &dt.format("%d").to_string())
            .replace("{date}", &dt.format("%Y-%m-%d").to_string())
            .replace("{kind}", &primary.kind);
        let s = sanitize(&s);
        let s = s.trim().trim_matches('.').to_string();
        if !s.is_empty() {
            out.push(s);
        }
    }
    out
}

/// 计算整理计划：同一个作品（字幕、封面等）跟着视频或音频走，放进同一个目录。
pub fn plan_reorganize(db: &Db, root: &Path, template: &str) -> AppResult<Vec<MovePlan>> {
    let all = db.search_library(&crate::db::LibraryFilter::default(), 200_000)?;
    let mut groups: HashMap<(String, String), Vec<&LibraryItem>> = HashMap::new();
    for i in &all {
        groups.entry((i.platform.clone(), i.media_id.clone())).or_default().push(i);
    }
    let mut plans = vec![];
    for items in groups.values() {
        let primary = items.iter().find(|i| i.kind == "video").or_else(|| items.iter().find(|i| i.kind == "audio")).or_else(|| items.first());
        let Some(primary) = primary else { continue };
        let dir = root.join(render_dir(template, primary));
        for i in items {
            let from = Path::new(&i.path);
            if !from.exists() {
                continue;
            }
            let Some(name) = from.file_name() else { continue };
            let to = dir.join(name);
            if to != from {
                plans.push(MovePlan { id: i.id, from: i.path.clone(), to: to.to_string_lossy().into_owned() });
            }
        }
    }
    plans.sort_by(|a, b| a.to.cmp(&b.to));
    Ok(plans)
}

/// 同一个目录里、和主文件同名前缀的附属文件（信息 JSON、NFO、没有登记在媒体库里的字幕等）。
fn sidecars(file: &Path, tracked: &HashSet<String>) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (file.parent(), file.file_stem().and_then(|s| s.to_str())) else { return vec![] };
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| p != file && p.is_file() && !tracked.contains(&p.to_string_lossy().into_owned()))
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            n.starts_with(&format!("{stem}.")) && !VIDEO_EXTS.contains(&ext_of(p).as_str()) && !AUDIO_EXTS.contains(&ext_of(p).as_str())
        })
        .collect()
}

pub fn apply_reorganize(db: &Db, plans: &[MovePlan]) -> AppResult<MoveReport> {
    let tracked = db.paths_set()?;
    let mut rep = MoveReport::default();
    for p in plans {
        let (from, to) = (Path::new(&p.from), Path::new(&p.to));
        // 只移动媒体库里登记的文件，且起点必须和记录一致
        if db.library_item(p.id)?.map(|i| i.path) != Some(p.from.clone()) {
            rep.skipped.push(format!("{}：和媒体库里的记录不一致", p.from));
            continue;
        }
        if !from.exists() {
            rep.skipped.push(format!("{}：文件已不存在", p.from));
            continue;
        }
        if to.exists() {
            rep.skipped.push(format!("{}：目标已有同名文件", p.to));
            continue;
        }
        let side = sidecars(from, &tracked);
        // 附属文件的目标冲突时整组跳过，避免主文件和附属文件分家
        let to_dir = to.parent().unwrap_or(Path::new("."));
        if side.iter().any(|s| s.file_name().is_some_and(|n| to_dir.join(n).exists())) {
            rep.skipped.push(format!("{}：目标目录里已有同名的附属文件", p.to));
            continue;
        }
        if let Err(e) = move_file(from, to) {
            rep.skipped.push(format!("{}：{e}", p.from));
            continue;
        }
        for s in side {
            if let Some(n) = s.file_name() {
                let _ = move_file(&s, &to_dir.join(n));
            }
        }
        db.conn().execute("UPDATE downloads SET path=?2 WHERE id=?1", params![p.id, p.to])?;
        rep.moved += 1;
    }
    Ok(rep)
}

// ---------- 导入已有文件 ----------

fn walk(dir: &Path, recursive: bool, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            if recursive && depth < 8 {
                walk(&p, recursive, depth + 1, out);
            }
        } else if p.is_file() {
            out.push(p);
        }
    }
}

pub fn path_key(path: &Path) -> String {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    hex::encode(&Sha256::digest(canon.to_string_lossy().as_bytes())[..8])
}

fn lang_from_sub_name(video_stem: &str, sub: &Path) -> String {
    let stem = sub.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    stem.strip_prefix(video_stem)
        .map(|r| r.trim_start_matches(['.', '_', ' ', '-']).to_string())
        .filter(|s| !s.is_empty() && s.len() <= 16)
        .unwrap_or_else(|| "und".into())
}

/// 把文件夹里已有的视频、音频、图片登记进媒体库（已经登记过的路径跳过）；
/// 和视频同名的字幕文件作为这个视频的字幕一起登记。
pub fn import_folder(db: &Db, dir: &Path, recursive: bool) -> AppResult<(ImportReport, Vec<i64>)> {
    if !dir.is_dir() {
        return Err(AppError::invalid("请选择一个文件夹。"));
    }
    let mut files = vec![];
    walk(dir, recursive, 0, &mut files);
    files.sort();
    let known = db.paths_set()?;
    let mut rep = ImportReport::default();
    let mut new_media: Vec<i64> = vec![];
    // 先登记媒体文件，再处理字幕（需要找到对应的视频）
    let mut stems: HashMap<(PathBuf, String), String> = HashMap::new();
    for f in &files {
        let ext = ext_of(f);
        let Some(kind) = kind_of_ext(&ext).filter(|k| *k != "subtitle") else { continue };
        let path = f.to_string_lossy().into_owned();
        if known.contains(&path) {
            rep.skipped += 1;
            continue;
        }
        let meta = std::fs::metadata(f)?;
        let finished = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or_else(now);
        let title = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let author = f.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let media_id = format!("local-{}", path_key(f));
        let id = db.insert_local(&LocalRow {
            media_id: &media_id,
            asset_id: "file",
            title: &title,
            author: &author,
            path: &path,
            size: meta.len() as i64,
            kind,
            finished_at: finished,
        })?;
        rep.added += 1;
        if kind == "video" || kind == "audio" {
            new_media.push(id);
        }
        stems.insert((f.parent().unwrap_or(Path::new("")).to_path_buf(), title), media_id);
    }
    for f in &files {
        let ext = ext_of(f);
        if !SUB_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let path = f.to_string_lossy().into_owned();
        if known.contains(&path) {
            continue;
        }
        let parent = f.parent().unwrap_or(Path::new("")).to_path_buf();
        let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // 找到名字是它的前缀的视频：`视频.zh.srt` 属于 `视频.mp4`
        let owner =
            stems.iter().filter(|((p, vs), _)| *p == parent && (stem == *vs || stem.starts_with(&format!("{vs}.")))).max_by_key(|((_, vs), _)| vs.len());
        let Some(((_, vstem), media_id)) = owner else { continue };
        let lang = lang_from_sub_name(vstem, f);
        let meta = std::fs::metadata(f)?;
        db.insert_local(&LocalRow {
            media_id,
            asset_id: &format!("sub-{lang}"),
            title: &stem,
            author: "",
            path: &path,
            size: meta.len() as i64,
            kind: "subtitle",
            finished_at: now(),
        })?;
        rep.subtitles += 1;
    }
    Ok((rep, new_media))
}

// ---------- 字幕索引 ----------

fn parse_ass_text(content: &str) -> Vec<subtitle::Cue> {
    let tag = regex::Regex::new(r"\{[^}]*\}").unwrap();
    let mut out = vec![];
    for line in content.lines() {
        let Some(rest) = line.strip_prefix("Dialogue:") else { continue };
        let f: Vec<&str> = rest.splitn(10, ',').collect();
        if f.len() < 10 {
            continue;
        }
        let t = |s: &str| -> Option<u64> {
            let (h, r) = s.trim().split_once(':')?;
            let (m, r) = r.split_once(':')?;
            let (sec, cs) = r.split_once('.')?;
            Some(((h.parse::<u64>().ok()? * 3600 + m.parse::<u64>().ok()? * 60 + sec.parse::<u64>().ok()?) * 1000) + cs.parse::<u64>().ok()? * 10)
        };
        let (Some(start), Some(end)) = (t(f[1]), t(f[2])) else { continue };
        let text = tag.replace_all(f[9], "").replace("\\N", " ").replace("\\n", " ");
        if !text.trim().is_empty() {
            out.push(subtitle::Cue { start, end, lines: vec![text.trim().to_string()] });
        }
    }
    out
}

/// 读取字幕文件里的所有条目（SRT / VTT / ASS）。
pub fn read_cues(path: &Path) -> Option<Vec<subtitle::Cue>> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Some(match ext_of(path).as_str() {
        "srt" => subtitle::parse_srt(&text),
        "vtt" => {
            let mut cues = subtitle::parse_vtt(&text);
            if text.contains("<c>") || text.contains("<00:") {
                cues = subtitle::dedupe_rolling(cues);
            }
            cues
        }
        "ass" | "ssa" => parse_ass_text(&text),
        _ => return None,
    })
}

/// 给一条字幕记录建立全文索引。弹幕不建立索引。
pub fn index_subtitle_item(db: &Db, item: &LibraryItem) -> AppResult<usize> {
    if item.kind != "subtitle" || item.asset_id == "danmaku" || item.path.ends_with(".xml") {
        return Ok(0);
    }
    let Some(cues) = read_cues(Path::new(&item.path)) else { return Ok(0) };
    let lang = item.asset_id.split_once('-').map(|(_, l)| l.to_string()).unwrap_or_else(|| item.asset_id.clone());
    db.insert_cues(item.id, &item.platform, &item.media_id, &lang, &cues)
}

/// 重建全部字幕索引，返回索引的条数。
pub fn reindex_all(db: &Db) -> AppResult<usize> {
    db.clear_cues(None)?;
    let mut n = 0;
    for item in db.subtitle_items()? {
        if Path::new(&item.path).exists() {
            n += index_subtitle_item(db, &item)?;
        }
    }
    Ok(n)
}

// ---------- 重复文件 ----------

/// 内容指纹：文件大小 + 头尾各 256 KB 的 SHA-256（不读完整个大文件）。
pub fn quick_hash(path: &Path) -> std::io::Result<String> {
    const CHUNK: u64 = 256 * 1024;
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut h = Sha256::new();
    h.update(len.to_le_bytes());
    let mut buf = vec![0u8; CHUNK.min(len) as usize];
    f.read_exact(&mut buf)?;
    h.update(&buf);
    if len > CHUNK {
        let tail = CHUNK.min(len - CHUNK);
        f.seek(SeekFrom::Start(len - tail))?;
        let mut buf = vec![0u8; tail as usize];
        f.read_exact(&mut buf)?;
        h.update(&buf);
    }
    Ok(hex::encode(h.finalize()))
}

/// 完全相同的文件（大小相同且内容指纹相同）。
pub fn exact_duplicates(db: &Db) -> AppResult<Vec<DupGroup>> {
    let items = db.search_library(&crate::db::LibraryFilter::default(), 200_000)?;
    let mut by_size: HashMap<i64, Vec<LibraryItem>> = HashMap::new();
    for i in items.into_iter().filter(|i| i.exists && matches!(i.kind.as_str(), "video" | "audio" | "image") && i.size > 0) {
        by_size.entry(i.size).or_default().push(i);
    }
    let mut groups = vec![];
    for (_, same) in by_size.into_iter().filter(|(_, v)| v.len() > 1) {
        let mut by_hash: HashMap<String, Vec<LibraryItem>> = HashMap::new();
        for i in same {
            if let Ok(h) = quick_hash(Path::new(&i.path)) {
                by_hash.entry(h).or_default().push(i);
            }
        }
        groups.extend(by_hash.into_values().filter(|v| v.len() > 1).map(|items| DupGroup { kind: "exact".into(), items }));
    }
    groups.sort_by(|a, b| b.items[0].size.cmp(&a.items[0].size));
    Ok(groups)
}

/// 差异哈希（dHash）：把图像缩成 9×8 的灰度图，比较相邻像素的亮暗，得到 64 位指纹。
pub fn dhash(gray_9x8: &[u8]) -> Option<u64> {
    if gray_9x8.len() < 72 {
        return None;
    }
    let mut h = 0u64;
    for row in 0..8 {
        for col in 0..8 {
            h <<= 1;
            if gray_9x8[row * 9 + col] > gray_9x8[row * 9 + col + 1] {
                h |= 1;
            }
        }
    }
    Some(h)
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// 画面指纹的字符串形式：每帧 16 位十六进制，帧之间用 `-` 连接。
pub fn parse_phash(s: &str) -> Vec<u64> {
    s.split('-').filter_map(|p| u64::from_str_radix(p, 16).ok()).collect()
}

/// 两个视频是否相似：同样位置的几帧都很接近，并且时长相差不大。
pub fn similar(a: &[u64], b: &[u64], dur_a: Option<i64>, dur_b: Option<i64>) -> bool {
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    if let (Some(x), Some(y)) = (dur_a, dur_b) {
        let diff = (x - y).abs();
        if diff > 3000 && diff * 100 > x.max(y) * 8 {
            return false;
        }
    }
    let dists: Vec<u32> = a.iter().zip(b).map(|(x, y)| hamming(*x, *y)).collect();
    dists.iter().all(|d| *d <= 10) && dists.iter().sum::<u32>() <= 24
}

/// 把相似的视频分组（并查集）。`hashes` 是（记录, 画面指纹）。
pub fn group_similar(items: Vec<(LibraryItem, Vec<u64>)>) -> Vec<DupGroup> {
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            if similar(&items[i].1, &items[j].1, items[i].0.duration_ms, items[j].0.duration_ms) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<LibraryItem>> = HashMap::new();
    for (i, (item, _)) in items.into_iter().enumerate() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(item);
    }
    let mut out: Vec<DupGroup> = groups.into_values().filter(|v| v.len() > 1).map(|items| DupGroup { kind: "similar".into(), items }).collect();
    out.sort_by(|a, b| b.items[0].size.cmp(&a.items[0].size));
    out
}

// ---------- 导出 ----------

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 媒体库清单导出为 CSV（带 BOM，Excel 打开不乱码）。
pub fn library_csv(items: &[LibraryItem]) -> String {
    let mut out = String::from("\u{feff}ID,平台,标题,作者,类型,大小(字节),文件路径,原链接,完成时间,收藏,评分,标签,备注\n");
    for i in items {
        let t = chrono::DateTime::from_timestamp(i.finished_at, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
        let row = [
            i.id.to_string(),
            i.platform_name.clone(),
            i.title.clone(),
            i.author.clone(),
            i.kind.clone(),
            i.size.to_string(),
            i.path.clone(),
            i.source_url.clone(),
            t,
            if i.favorite { "是".into() } else { String::new() },
            if i.rating > 0 { i.rating.to_string() } else { String::new() },
            i.tags.join(";"),
            i.note.clone(),
        ];
        out.push_str(&row.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{LibraryFilter, NewDownload};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-lib-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[allow(clippy::too_many_arguments)]
    fn add(db: &Db, platform: &str, media_id: &str, asset: &str, title: &str, author: &str, path: &Path, kind: &str) -> i64 {
        db.record_download(&NewDownload {
            platform,
            media_id,
            asset_id: asset,
            title,
            author,
            cover: None,
            path: &path.to_string_lossy(),
            size: std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0),
            kind,
            source: "manual",
            source_url: "",
            platform_name: platform,
        })
        .unwrap();
        db.conn()
            .query_row("SELECT id FROM downloads WHERE platform=?1 AND media_id=?2 AND asset_id=?3", params![platform, media_id, asset], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn tags_meta_and_filters() {
        let db = Db::open_in_memory().unwrap();
        let dir = temp("tags");
        let f1 = dir.join("a.mp4");
        std::fs::write(&f1, b"aaaa").unwrap();
        let f2 = dir.join("b.mp4");
        std::fs::write(&f2, b"bbbbbb").unwrap();
        let a = add(&db, "x", "1", "video", "猫猫视频", "甲", &f1, "video");
        let b = add(&db, "x", "2", "video", "狗狗视频", "乙", &f2, "video");
        db.set_item_tags(a, &["萌宠".into(), " 猫 ".into(), "萌宠".into(), "".into()]).unwrap();
        db.bulk_tags(&[a, b], &["待看".into()], false).unwrap();
        db.set_item_meta(a, Some(true), Some(9), Some("备注内容")).unwrap();
        let item = db.library_item(a).unwrap().unwrap();
        assert_eq!(item.tags.len(), 3, "{:?}", item.tags);
        assert!(item.favorite);
        assert_eq!(item.rating, 5, "rating is clamped");
        assert_eq!(item.note, "备注内容");
        let tags = db.all_tags().unwrap();
        assert_eq!(tags[0].name, "待看");
        assert_eq!(tags[0].count, 2);
        let f = |q: &str| LibraryFilter { query: q.into(), ..Default::default() };
        assert_eq!(db.search_library(&f("萌宠"), 10).unwrap().len(), 1, "query matches tag names");
        assert_eq!(db.search_library(&f("备注"), 10).unwrap().len(), 1, "query matches notes");
        assert_eq!(db.search_library(&LibraryFilter { tag: Some("待看".into()), ..Default::default() }, 10).unwrap().len(), 2);
        assert_eq!(db.search_library(&LibraryFilter { favorite_only: true, ..Default::default() }, 10).unwrap().len(), 1);
        assert_eq!(db.search_library(&LibraryFilter { min_rating: 4, ..Default::default() }, 10).unwrap().len(), 1);
        let by_size = db.search_library(&LibraryFilter { sort: Some("size".into()), ..Default::default() }, 10).unwrap();
        assert_eq!(by_size[0].id, b);
        db.bulk_tags(&[a, b], &["待看".into()], true).unwrap();
        assert!(db.all_tags().unwrap().iter().all(|t| t.name != "待看"), "unused tags are dropped");
        db.delete_library(a).unwrap();
        assert!(db.all_tags().unwrap().iter().all(|t| t.name != "猫"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_tags_rules() {
        let many: Vec<String> = (0..30).map(|i| format!("t{i}")).collect();
        assert_eq!(clean_tags(&many).len(), 20);
        assert_eq!(clean_tags(&["A".into(), "a".into(), "B\n".into()]), vec!["A", "B"]);
        assert_eq!(clean_tags(&["x".repeat(50)])[0].chars().count(), 30);
    }

    #[test]
    fn trash_and_restore_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let root = temp("trash");
        let f = root.join("子目录").join("视频.mp4");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"hello world").unwrap();
        let id = add(&db, "x", "1", "video", "视频", "甲", &f, "video");
        db.set_item_tags(id, &["收藏".into()]).unwrap();
        db.set_item_meta(id, Some(true), Some(4), Some("note")).unwrap();
        let item = db.library_item(id).unwrap().unwrap();
        let tid = move_to_trash(&db, &root, &item).unwrap().unwrap();
        assert!(!f.exists());
        assert!(db.library_item(id).unwrap().is_none());
        assert_eq!(db.trash_list().unwrap().len(), 1);
        assert!(root.join(TRASH_DIR).is_dir());
        // 原位置被占用时还原到带序号的名字
        std::fs::write(&f, b"other").unwrap();
        let restored = restore_from_trash(&db, tid).unwrap();
        assert!(restored.ends_with("视频 (1).mp4"), "{restored}");
        assert_eq!(std::fs::read(&restored).unwrap(), b"hello world");
        let back = db.search_library(&LibraryFilter::default(), 10).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].favorite, back[0].rating, back[0].note.as_str(), back[0].tags.clone()), (true, 4, "note", vec!["收藏".to_string()]));
        assert!(db.trash_list().unwrap().is_empty());
        // 永久删除
        let item = back[0].clone();
        let tid = move_to_trash(&db, &root, &item).unwrap().unwrap();
        let (_, _, tp) = db.trash_get(tid).unwrap().unwrap();
        purge_trash(&db, tid).unwrap();
        assert!(!Path::new(&tp).exists());
        // 文件已经不在：只删记录，不进回收站
        let g = root.join("gone.mp4");
        std::fs::write(&g, b"x").unwrap();
        let gid = add(&db, "x", "9", "video", "gone", "", &g, "video");
        std::fs::remove_file(&g).unwrap();
        let gi = db.library_item(gid).unwrap().unwrap();
        assert_eq!(move_to_trash(&db, &root, &gi).unwrap(), None);
        assert!(db.library_item(gid).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stats_report() {
        let db = Db::open_in_memory().unwrap();
        let dir = temp("stats");
        let f = dir.join("a.mp4");
        std::fs::write(&f, vec![0u8; 1000]).unwrap();
        let g = dir.join("b.mp3");
        std::fs::write(&g, vec![0u8; 300]).unwrap();
        add(&db, "p1", "1", "video", "A", "甲", &f, "video");
        let gone = add(&db, "p2", "2", "audio", "B", "甲", &g, "audio");
        std::fs::remove_file(&g).unwrap();
        let _ = gone;
        let s = db.library_stats().unwrap();
        assert_eq!((s.count, s.total_size, s.missing), (2, 1300, 1));
        assert_eq!(s.by_platform[0].key, "p1");
        assert_eq!(s.by_author[0].count, 2);
        assert_eq!(s.largest[0].size, 1000);
        assert_eq!(s.by_month.len(), 1);
        assert_eq!(db.missing_ids().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reorganize_moves_groups_and_sidecars() {
        let db = Db::open_in_memory().unwrap();
        let root = temp("reorg");
        let v = root.join("视频.mp4");
        let s = root.join("视频.zh.srt");
        let nfo = root.join("视频.nfo");
        let other = root.join("视频 二.mp4");
        for p in [&v, &s, &nfo, &other] {
            std::fs::write(p, b"data").unwrap();
        }
        let vid = add(&db, "抖音", "1", "video", "视频", "作者/甲", &v, "video");
        add(&db, "抖音", "1", "sub-zh", "视频.zh", "", &s, "subtitle");
        add(&db, "抖音", "2", "video", "视频 二", "", &other, "video");
        db.conn().execute("UPDATE downloads SET finished_at=1700000000", []).unwrap();
        let plans = plan_reorganize(&db, &root, "{platform}/{author}/{year}-{month}").unwrap();
        assert_eq!(plans.len(), 3);
        let rel: Vec<String> = plans.iter().map(|p| Path::new(&p.to).strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert!(rel.contains(&"抖音/作者_甲/2023-11/视频.mp4".to_string()), "{rel:?}");
        assert!(rel.contains(&"抖音/作者_甲/2023-11/视频.zh.srt".to_string()), "subtitle follows its video: {rel:?}");
        assert!(rel.contains(&"抖音/未知作者/2023-11/视频 二.mp4".to_string()), "{rel:?}");
        let rep = apply_reorganize(&db, &plans).unwrap();
        assert_eq!((rep.moved, rep.skipped.len()), (3, 0), "{rep:?}");
        let moved = db.library_item(vid).unwrap().unwrap();
        assert!(Path::new(&moved.path).exists());
        assert!(!v.exists());
        assert!(moved.path.contains("2023-11"));
        // 没有登记的附属文件 .nfo 跟着走
        assert!(Path::new(&moved.path).with_extension("nfo").exists());
        assert!(!nfo.exists());
        // 再整理一次：已经在位置上，没有计划
        assert!(plan_reorganize(&db, &root, "{platform}/{author}/{year}-{month}").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reorganize_skips_conflicts() {
        let db = Db::open_in_memory().unwrap();
        let root = temp("reorg2");
        let v = root.join("a.mp4");
        std::fs::write(&v, b"a").unwrap();
        let id = add(&db, "p", "1", "video", "a", "x", &v, "video");
        let dest = root.join("x");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("a.mp4"), b"existing").unwrap();
        let plans = plan_reorganize(&db, &root, "{author}").unwrap();
        let rep = apply_reorganize(&db, &plans).unwrap();
        assert_eq!(rep.moved, 0);
        assert_eq!(rep.skipped.len(), 1);
        assert!(v.exists(), "source stays when the target is taken");
        assert_eq!(db.library_item(id).unwrap().unwrap().path, v.to_string_lossy());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_registers_media_and_subtitles() {
        let db = Db::open_in_memory().unwrap();
        let root = temp("import");
        std::fs::create_dir_all(root.join("作者甲")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("作者甲").join("电影.mp4"), b"video").unwrap();
        std::fs::write(root.join("作者甲").join("电影.zh-Hans.srt"), "1\n00:00:01,000 --> 00:00:02,000\n你好\n\n").unwrap();
        std::fs::write(root.join("作者甲").join("电影.en.srt"), "1\n00:00:01,000 --> 00:00:02,000\nhello\n\n").unwrap();
        std::fs::write(root.join("作者甲").join("歌.mp3"), b"mp3").unwrap();
        std::fs::write(root.join("作者甲").join("说明.txt"), b"ignore").unwrap();
        std::fs::write(root.join(".hidden").join("x.mp4"), b"hidden").unwrap();
        std::fs::write(root.join("孤儿.srt"), "1\n00:00:01,000 --> 00:00:02,000\nx\n\n").unwrap();
        let (rep, media) = import_folder(&db, &root, true).unwrap();
        assert_eq!((rep.added, rep.subtitles), (2, 2), "{rep:?}");
        assert_eq!(media.len(), 2);
        let items = db.search_library(&LibraryFilter::default(), 100).unwrap();
        assert_eq!(items.len(), 4);
        let video = items.iter().find(|i| i.kind == "video").unwrap();
        assert_eq!((video.title.as_str(), video.author.as_str(), video.platform.as_str()), ("电影", "作者甲", "local"));
        let subs: Vec<&LibraryItem> = items.iter().filter(|i| i.kind == "subtitle").collect();
        assert!(subs.iter().all(|s| s.media_id == video.media_id), "subtitles belong to the video");
        assert!(subs.iter().any(|s| s.asset_id == "sub-zh-Hans") && subs.iter().any(|s| s.asset_id == "sub-en"));
        // 再导入一次：全部跳过
        let (rep2, _) = import_folder(&db, &root, true).unwrap();
        assert_eq!((rep2.added, rep2.subtitles, rep2.skipped), (0, 0, 2), "{rep2:?}");
        // 不递归只看当前目录
        let db2 = Db::open_in_memory().unwrap();
        assert_eq!(import_folder(&db2, &root, false).unwrap().0.added, 0);
        assert!(import_folder(&db2, &root.join("不存在"), false).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn subtitle_search_with_jump_positions() {
        let db = Db::open_in_memory().unwrap();
        let root = temp("search");
        let v = root.join("课程.mp4");
        std::fs::write(&v, b"v").unwrap();
        let vid = add(&db, "local", "m1", "file", "课程", "", &v, "video");
        let s1 = root.join("课程.zh.srt");
        std::fs::write(&s1, "1\n00:00:05,000 --> 00:00:07,000\n今天讲 100% 的覆盖率\n\n2\n00:01:00,000 --> 00:01:02,000\n下课\n\n").unwrap();
        let sid = add(&db, "local", "m1", "sub-zh", "课程.zh", "", &s1, "subtitle");
        let s2 = root.join("课程.vtt");
        std::fs::write(&s2, "WEBVTT\n\n00:00:03.000 --> 00:00:04.000\nHello coverage\n").unwrap();
        add(&db, "local", "m1", "sub-en", "课程.en", "", &s2, "subtitle");
        let ass = root.join("字幕.ass");
        std::fs::write(&ass, "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:02:00.00,0:02:03.50,D,,0,0,0,,{\\an8}第一行\\N第二行, 带逗号\n").unwrap();
        add(&db, "local", "m1", "sub-ass", "字幕", "", &ass, "subtitle");
        add(&db, "local", "m1", "danmaku", "弹幕", "", &root.join("d.xml"), "subtitle");
        assert_eq!(reindex_all(&db).unwrap(), 4);
        let hits = db.search_cues("覆盖率", 10, 3).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].item_id, hits[0].start_ms, hits[0].title.as_str(), hits[0].lang.as_str()), (Some(vid), 5000, "课程", "zh"));
        assert_eq!(db.search_cues("100%", 10, 3).unwrap().len(), 1, "percent sign is literal");
        assert_eq!(db.search_cues("100_", 10, 3).unwrap().len(), 0, "underscore is literal");
        assert_eq!(db.search_cues("coverage", 10, 3).unwrap()[0].lang, "en");
        let ass_hit = db.search_cues("带逗号", 10, 3).unwrap();
        assert_eq!(ass_hit[0].start_ms, 120_000);
        assert!(ass_hit[0].text.contains("第一行 第二行"), "{}", ass_hit[0].text);
        assert!(db.search_cues("", 10, 3).unwrap().is_empty());
        // 每个作品最多返回 per_item 条
        assert_eq!(db.search_cues("课", 10, 1).unwrap().len(), 1);
        // 删除字幕记录后索引也没了
        db.delete_library(sid).unwrap();
        assert!(db.search_cues("覆盖率", 10, 3).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exact_duplicates_by_content() {
        let db = Db::open_in_memory().unwrap();
        let root = temp("dup");
        let big: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        let a = root.join("a.mp4");
        let b = root.join("b.mp4");
        let c = root.join("c.mp4");
        std::fs::write(&a, &big).unwrap();
        std::fs::write(&b, &big).unwrap();
        let mut changed = big.clone();
        changed[350_000] ^= 0xff; // 中间改一个字节：头尾相同，大小相同
        std::fs::write(&c, &changed).unwrap();
        add(&db, "x", "1", "video", "a", "", &a, "video");
        add(&db, "x", "2", "video", "b", "", &b, "video");
        add(&db, "x", "3", "video", "c", "", &c, "video");
        let groups = exact_duplicates(&db).unwrap();
        // 快速指纹只看头尾：c 与 a、b 会被判为同组，这是已知的取舍（只做“可能重复”的提示）
        assert_eq!(groups.len(), 1);
        assert!(groups[0].items.len() >= 2);
        assert!(groups[0].items.iter().any(|i| i.title == "a") && groups[0].items.iter().any(|i| i.title == "b"));
        let d = root.join("d.mp4");
        std::fs::write(&d, b"short").unwrap();
        add(&db, "x", "4", "video", "d", "", &d, "video");
        assert_eq!(exact_duplicates(&db).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dhash_and_similarity() {
        // 渐变：左亮右暗 → 所有位为 1；反过来全为 0
        let left_bright: Vec<u8> = (0..72).map(|i| 255 - (i % 9) as u8 * 20).collect();
        let right_bright: Vec<u8> = (0..72).map(|i| (i % 9) as u8 * 20).collect();
        assert_eq!(dhash(&left_bright), Some(u64::MAX));
        assert_eq!(dhash(&right_bright), Some(0));
        assert_eq!(dhash(&[0; 10]), None);
        assert_eq!(hamming(0b1011, 0b0010), 2);
        let h = |v: u64| vec![v, v, v];
        assert!(similar(&h(0xff00), &h(0xff01), Some(10_000), Some(10_100)));
        assert!(!similar(&h(0), &h(u64::MAX), None, None));
        assert!(!similar(&h(0xff00), &h(0xff01), Some(10_000), Some(60_000)), "very different durations");
        assert!(!similar(&[], &[], None, None));
        assert_eq!(parse_phash("00000000000000ff-ff00000000000000"), vec![0xff, 0xff00000000000000]);
        let mk = |id: i64| LibraryItem {
            id,
            platform: "p".into(),
            media_id: id.to_string(),
            asset_id: "video".into(),
            title: format!("t{id}"),
            author: String::new(),
            cover: None,
            path: String::new(),
            size: 10 - id,
            finished_at: 0,
            exists: true,
            kind: "video".into(),
            source: String::new(),
            source_url: String::new(),
            platform_name: String::new(),
            cover_path: None,
            favorite: false,
            rating: 0,
            note: String::new(),
            tags: vec![],
            duration_ms: Some(10_000),
        };
        let groups = group_similar(vec![(mk(1), h(0xf0f0)), (mk(2), h(0xf0f1)), (mk(3), h(0x0f0f_0f0f_0f0f_0f0f)), (mk(4), h(0xf0f3))]);
        assert_eq!(groups.len(), 1);
        let mut ids: Vec<i64> = groups[0].items.iter().map(|i| i.id).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2, 4]);
    }

    #[test]
    fn csv_export_escapes() {
        let db = Db::open_in_memory().unwrap();
        let dir = temp("csv");
        let f = dir.join("a,b.mp4");
        std::fs::write(&f, b"x").unwrap();
        let id = add(&db, "x", "1", "video", "含\"引号\"，和逗号", "作者", &f, "video");
        db.set_item_tags(id, &["a".into(), "b".into()]).unwrap();
        db.set_item_meta(id, Some(true), Some(3), Some("多行\n备注")).unwrap();
        let items = db.search_library(&LibraryFilter::default(), 10).unwrap();
        let csv = library_csv(&items);
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("\"含\"\"引号\"\"，和逗号\""), "{csv}");
        assert!(csv.contains("\"多行\n备注\""));
        assert!(csv.contains(",是,3,a;b,"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
