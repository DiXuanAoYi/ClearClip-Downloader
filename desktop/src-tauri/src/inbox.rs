//! 收到的链接（收件箱）：剪贴板识别、手机发送、浏览器扩展发送的链接，以及手动解析失败的链接，
//! 全部记录下来，重启后仍在。可以按来源 / 状态筛选、重试、忽略；需要登录的链接在保存账号后自动重试。
//!
//! 状态：
//! - `pending_pair` 新设备等待电脑确认配对；`rejected` 拒绝了配对
//! - `confirm` 等待在电脑上确认（“识别后自动下载”关闭时）
//! - `playlist` 是一个列表，需要在电脑上选择条目
//! - `resolving` 解析中；`queued` / `downloading` / `done` 跟随下载任务
//! - `failed` 失败（带错误分类）；`ignored` 已忽略

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use url::Url;

use crate::db::{now, Db};
use crate::download::TaskStatus;
use crate::error::{AppError, AppResult, ErrorKind};
use crate::model::MediaInfo;
use crate::{clipboard, providers, AppState};

pub const EVT_INBOX: &str = "inbox://changed";

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS inbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source TEXT NOT NULL,
    device TEXT NOT NULL DEFAULT '',
    device_name TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL,
    url TEXT NOT NULL,
    site TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    message TEXT,
    error_kind TEXT,
    title TEXT,
    platform TEXT,
    media_id TEXT,
    task_ids TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_inbox_created ON inbox(created_at);
CREATE INDEX IF NOT EXISTS idx_inbox_status ON inbox(status);
CREATE INDEX IF NOT EXISTS idx_inbox_device ON inbox(device);
";

/// 需要用户处理的状态（侧栏角标计数）。
const UNHANDLED: &str = "('confirm','playlist','failed')";
/// 还在进行中、跟随下载任务更新的状态。
const ACTIVE: &str = "('queued','downloading')";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: i64,
    /// clipboard / phone / extension / manual
    pub source: String,
    pub device: String,
    pub device_name: String,
    pub text: String,
    pub url: String,
    /// 用于登录：内置平台 ID 或网站域名
    pub site: String,
    pub status: String,
    pub message: Option<String>,
    pub error_kind: Option<String>,
    pub title: Option<String>,
    pub platform: Option<String>,
    pub media_id: Option<String>,
    pub task_ids: Vec<i64>,
    /// 同一链接之前收到的次数
    pub seen_before: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InboxFilter {
    pub source: Option<String>,
    /// all / unhandled / failed / done / active
    pub status: Option<String>,
    pub query: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxCounts {
    pub unhandled: i64,
    pub total: i64,
}

pub fn kind_str(k: ErrorKind) -> String {
    serde_json::to_value(k).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "other".into())
}

/// 链接对应的登录网站：内置平台用平台 ID，其他用可注册域名（youtu.be 归到 youtube.com）。
pub fn site_for(url: &str) -> String {
    let Ok(u) = Url::parse(url) else { return String::new() };
    if let Some(p) = providers::all().iter().find(|p| p.matches(&u)) {
        return p.id().to_string();
    }
    match crate::cookies::site_for_url(&u).unwrap_or_default().as_str() {
        "youtu.be" => "youtube.com".into(),
        "twitter.com" => "x.com".into(),
        s => s.to_string(),
    }
}

/// 文本里用于记录的主链接：优先内置平台的链接。
pub fn primary_url(text: &str) -> Option<String> {
    let urls = providers::extract_urls(text);
    let supported = urls.iter().find(|u| Url::parse(u).map(|p| providers::all().iter().any(|pr| pr.matches(&p))).unwrap_or(false));
    supported.or(urls.first()).cloned()
}

fn row(r: &Row) -> rusqlite::Result<InboxItem> {
    let ids: String = r.get("task_ids")?;
    Ok(InboxItem {
        id: r.get("id")?,
        source: r.get("source")?,
        device: r.get("device")?,
        device_name: r.get("device_name")?,
        text: r.get("text")?,
        url: r.get("url")?,
        site: r.get("site")?,
        status: r.get("status")?,
        message: r.get("message")?,
        error_kind: r.get("error_kind")?,
        title: r.get("title")?,
        platform: r.get("platform")?,
        media_id: r.get("media_id")?,
        task_ids: serde_json::from_str(&ids).unwrap_or_default(),
        seen_before: r.get("seen_before")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

const SELECT: &str = "SELECT i.*, (SELECT COUNT(*) FROM inbox p WHERE p.url = i.url AND p.id < i.id) AS seen_before FROM inbox i";

/// 一条记录的更新内容；为 None 的字段保持不变。
#[derive(Debug, Default)]
pub struct Patch<'a> {
    pub status: Option<&'a str>,
    pub message: Option<Option<String>>,
    pub error_kind: Option<Option<String>>,
    pub title: Option<String>,
    pub media: Option<(&'a str, &'a str)>,
    pub task_ids: Option<&'a [i64]>,
}

impl Db {
    pub fn inbox_add(&self, source: &str, device: &str, device_name: &str, text: &str, status: &str) -> AppResult<i64> {
        let url = primary_url(text).unwrap_or_else(|| text.chars().take(500).collect());
        let site = site_for(&url);
        let t = now();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO inbox (source, device, device_name, text, url, site, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![source, device, device_name, text, url, site, status, t],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn inbox_get(&self, id: i64) -> AppResult<Option<InboxItem>> {
        Ok(self.conn().query_row(&format!("{SELECT} WHERE i.id = ?1"), params![id], row).optional()?)
    }

    pub fn inbox_patch(&self, id: i64, p: Patch<'_>) -> AppResult<()> {
        let conn = self.conn();
        let t = now();
        if let Some(s) = p.status {
            conn.execute("UPDATE inbox SET status=?2, updated_at=?3 WHERE id=?1", params![id, s, t])?;
        }
        if let Some(m) = p.message {
            conn.execute("UPDATE inbox SET message=?2, updated_at=?3 WHERE id=?1", params![id, m, t])?;
        }
        if let Some(k) = p.error_kind {
            conn.execute("UPDATE inbox SET error_kind=?2, updated_at=?3 WHERE id=?1", params![id, k, t])?;
        }
        if let Some(title) = p.title {
            conn.execute("UPDATE inbox SET title=?2, updated_at=?3 WHERE id=?1", params![id, title, t])?;
        }
        if let Some((platform, media_id)) = p.media {
            conn.execute("UPDATE inbox SET platform=?2, media_id=?3, updated_at=?4 WHERE id=?1", params![id, platform, media_id, t])?;
        }
        if let Some(ids) = p.task_ids {
            conn.execute("UPDATE inbox SET task_ids=?2, updated_at=?3 WHERE id=?1", params![id, serde_json::to_string(ids)?, t])?;
        }
        Ok(())
    }

    pub fn inbox_list(&self, f: &InboxFilter) -> AppResult<Vec<InboxItem>> {
        let mut sql = format!("{SELECT} WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![];
        if let Some(s) = f.source.as_deref().filter(|s| !s.is_empty() && *s != "all") {
            args.push(Box::new(s.to_string()));
            sql.push_str(&format!(" AND i.source = ?{}", args.len()));
        }
        match f.status.as_deref().unwrap_or("all") {
            "unhandled" => sql.push_str(&format!(" AND i.status IN {UNHANDLED}")),
            "failed" => sql.push_str(" AND i.status IN ('failed','rejected')"),
            "done" => sql.push_str(" AND i.status = 'done'"),
            "active" => sql.push_str(" AND i.status IN ('pending_pair','resolving','queued','downloading')"),
            "ignored" => sql.push_str(" AND i.status = 'ignored'"),
            _ => {}
        }
        let q = f.query.trim();
        if !q.is_empty() {
            args.push(Box::new(format!("%{q}%")));
            let n = args.len();
            sql.push_str(&format!(" AND (i.text LIKE ?{n} OR i.url LIKE ?{n} OR IFNULL(i.title,'') LIKE ?{n} OR i.device_name LIKE ?{n})"));
        }
        args.push(Box::new(f.limit.unwrap_or(500).clamp(1, 2000)));
        sql.push_str(&format!(" ORDER BY i.id DESC LIMIT ?{}", args.len()));
        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let list = stmt.query_map(refs.as_slice(), row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(list)
    }

    /// 某个设备最近发送的记录（手机网页显示）。
    pub fn inbox_for_device(&self, device: &str, limit: i64) -> AppResult<Vec<InboxItem>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("{SELECT} WHERE i.device = ?1 ORDER BY i.id DESC LIMIT ?2"))?;
        let list = stmt.query_map(params![device, limit], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(list)
    }

    pub fn inbox_with_status(&self, statuses: &str) -> AppResult<Vec<InboxItem>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("{SELECT} WHERE i.status IN {statuses} ORDER BY i.id"))?;
        let list = stmt.query_map([], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(list)
    }

    pub fn inbox_counts(&self) -> AppResult<InboxCounts> {
        let conn = self.conn();
        let unhandled = conn.query_row(&format!("SELECT COUNT(*) FROM inbox WHERE status IN {UNHANDLED}"), [], |r| r.get(0))?;
        let total = conn.query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get(0))?;
        Ok(InboxCounts { unhandled, total })
    }

    pub fn inbox_delete(&self, ids: &[i64]) -> AppResult<()> {
        let conn = self.conn();
        for id in ids {
            conn.execute("DELETE FROM inbox WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    /// 清空：`scope` 为 done（已完成和已忽略）/ all。
    pub fn inbox_clear(&self, scope: &str) -> AppResult<usize> {
        let sql = match scope {
            "all" => "DELETE FROM inbox",
            _ => "DELETE FROM inbox WHERE status IN ('done','ignored','rejected')",
        };
        Ok(self.conn().execute(sql, [])?)
    }

    /// 按保留期限和条数上限清理旧记录（进行中的不删）。
    pub fn inbox_prune(&self, keep_days: u32, max_items: usize) -> AppResult<usize> {
        let conn = self.conn();
        let mut n = 0;
        if keep_days > 0 {
            let before = now() - keep_days as i64 * 86400;
            n += conn.execute(&format!("DELETE FROM inbox WHERE created_at < ?1 AND status NOT IN {ACTIVE}"), params![before])?;
        }
        if max_items > 0 {
            n += conn.execute(
                &format!("DELETE FROM inbox WHERE status NOT IN {ACTIVE} AND id NOT IN (SELECT id FROM inbox ORDER BY id DESC LIMIT ?1)"),
                params![max_items as i64],
            )?;
        }
        Ok(n)
    }

    /// 解析成功后，把还在等待处理（确认、列表、失败）的同一链接记上作品信息，之后加入下载时会关联任务。
    pub fn inbox_mark_parsed(&self, text: &str, info: &MediaInfo) -> AppResult<()> {
        let mut urls = providers::extract_urls(text);
        urls.push(info.source_url.clone());
        let since = now() - 7 * 86400;
        let conn = self.conn();
        for u in urls.iter().filter(|u| !u.is_empty()) {
            conn.execute(
                "UPDATE inbox SET platform=?2, media_id=?3, title=?4, updated_at=?5
                 WHERE url=?1 AND created_at>?6 AND status IN ('confirm','playlist','failed','resolving')",
                params![u, info.platform, info.id, info.title, now(), since],
            )?;
        }
        Ok(())
    }

    /// 列表选好条目加入下载后，对应的“需要选择”记录标记为完成。
    pub fn inbox_playlist_chosen(&self, platform: &str, media_id: &str, count: usize) -> AppResult<bool> {
        let n = self.conn().execute(
            "UPDATE inbox SET status='done', message=?3, error_kind=NULL, updated_at=?4
             WHERE platform=?1 AND media_id=?2 AND status IN ('playlist','confirm','failed')",
            params![platform, media_id, format!("已选择 {count} 条加入下载"), now()],
        )?;
        Ok(n > 0)
    }

    /// 作品加入下载队列：关联到等待处理的同一作品的记录。返回是否有记录变化。
    pub fn inbox_link_tasks(&self, platform: &str, media_id: &str, task_ids: &[i64]) -> AppResult<bool> {
        if task_ids.is_empty() {
            return Ok(false);
        }
        let n = self.conn().execute(
            "UPDATE inbox SET status='queued', message=NULL, error_kind=NULL, task_ids=?3, updated_at=?4
             WHERE platform=?1 AND media_id=?2 AND status IN ('confirm','playlist','failed','resolving')",
            params![platform, media_id, serde_json::to_string(task_ids)?, now()],
        )?;
        Ok(n > 0)
    }
}

fn st(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

pub fn emit_changed(app: &AppHandle) {
    if let Ok(c) = st(app).db.inbox_counts() {
        let _ = app.emit(EVT_INBOX, c);
    }
}

/// 记录一条收到的链接。
pub fn record(app: &AppHandle, source: &str, device: &str, device_name: &str, text: &str, status: &str) -> Option<i64> {
    let state = st(app);
    match state.db.inbox_add(source, device, device_name, text, status) {
        Ok(id) => {
            let s = state.settings().inbox;
            // 偶尔清理一次，避免每条都扫表
            if id % 50 == 0 {
                let _ = state.db.inbox_prune(s.keep_days, s.max_items);
            }
            emit_changed(app);
            Some(id)
        }
        Err(e) => {
            log::warn!("record inbox item failed: {e}");
            None
        }
    }
}

pub fn patch(app: &AppHandle, id: i64, p: Patch<'_>) {
    if let Err(e) = st(app).db.inbox_patch(id, p) {
        log::warn!("update inbox item {id} failed: {e}");
    }
    emit_changed(app);
}

/// 解析并按默认选项加入下载，结果写回记录。
pub async fn process(app: &AppHandle, id: i64) {
    let Ok(Some(item)) = st(app).db.inbox_get(id) else { return };
    patch(app, id, Patch { status: Some("resolving"), message: Some(None), error_kind: Some(None), ..Default::default() });
    match clipboard::resolve_and_enqueue_full(app, &item.text).await {
        Ok(r) if r.playlist.is_some() => patch(
            app,
            id,
            Patch {
                status: Some("playlist"),
                title: Some(r.title.clone()),
                message: Some(Some(format!("是一个列表（{} 条），请在电脑上选择要下载的条目", r.playlist.unwrap_or(0)))),
                ..Default::default()
            },
        ),
        Ok(r) => {
            let (status, message) = if !r.task_ids.is_empty() {
                ("queued", None)
            } else if r.already_queued > 0 {
                ("done", Some("已在下载队列中".to_string()))
            } else {
                ("done", Some("之前已下载过".to_string()))
            };
            patch(
                app,
                id,
                Patch { status: Some(status), title: Some(r.title.clone()), message: Some(message), task_ids: Some(&r.task_ids), ..Default::default() },
            );
        }
        Err(e) => patch(
            app,
            id,
            Patch { status: Some("failed"), message: Some(Some(e.message.clone())), error_kind: Some(Some(kind_str(e.kind))), ..Default::default() },
        ),
    }
}

/// 重试一条记录。
pub fn retry(app: &AppHandle, id: i64) -> AppResult<()> {
    let item = st(app).db.inbox_get(id)?.ok_or_else(|| AppError::not_found("记录不存在。"))?;
    if item.status == "pending_pair" {
        return Err(AppError::invalid("这台设备还没有配对，请先在弹窗中允许配对。"));
    }
    // 失败的下载任务直接继续，不用重新解析
    let state = st(app);
    let tasks = state.downloads.snapshots();
    let failed: Vec<i64> =
        tasks.iter().filter(|t| item.task_ids.contains(&t.id) && matches!(t.status, TaskStatus::Failed | TaskStatus::Canceled)).map(|t| t.id).collect();
    if !failed.is_empty() {
        for t in &failed {
            crate::download::resume(app, *t);
        }
        patch(app, id, Patch { status: Some("queued"), message: Some(None), error_kind: Some(None), ..Default::default() });
        return Ok(());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { process(&app, id).await });
    Ok(())
}

pub fn ignore(app: &AppHandle, ids: &[i64]) -> AppResult<()> {
    for id in ids {
        st(app).db.inbox_patch(*id, Patch { status: Some("ignored"), ..Default::default() })?;
    }
    emit_changed(app);
    Ok(())
}

/// 跟随下载任务更新进行中的记录（任务完成、失败时调用；查询列表前也会调用）。
pub fn sync_tasks(app: &AppHandle) {
    let state = st(app);
    let Ok(rows) = state.db.inbox_with_status(ACTIVE) else { return };
    if rows.is_empty() {
        return;
    }
    let tasks = state.downloads.snapshots();
    let mut changed = false;
    for r in rows {
        let mine: Vec<_> = tasks.iter().filter(|t| r.task_ids.contains(&t.id)).collect();
        if mine.is_empty() {
            continue;
        }
        let (status, msg, kind) = if mine.iter().all(|t| t.status == TaskStatus::Done) {
            ("done", None, None)
        } else if let Some(t) = mine.iter().find(|t| matches!(t.status, TaskStatus::Failed | TaskStatus::Canceled)) {
            let msg = t.error.clone().or_else(|| (t.status == TaskStatus::Canceled).then(|| "下载已取消".to_string()));
            ("failed", msg, t.error_kind.map(kind_str))
        } else if mine.iter().any(|t| t.status == TaskStatus::Running) {
            let (got, total) = mine.iter().fold((0u64, 0u64), |(g, tt), t| (g + t.received, tt + t.total.unwrap_or(0)));
            ("downloading", (total > 0).then(|| format!("{}%", got * 100 / total)), None)
        } else {
            ("queued", None, None)
        };
        if status != r.status || msg != r.message {
            let _ = state.db.inbox_patch(r.id, Patch { status: Some(status), message: Some(msg), error_kind: Some(kind), ..Default::default() });
            changed |= status != r.status;
        }
    }
    if changed {
        emit_changed(app);
    }
}

/// 保存了某个网站的账号后：重试这个网站因需要登录（或限流）而失败的链接和下载任务。返回重试的数量。
/// 浏览器扩展自动同步时（`automatic`）跳过 10 分钟内刚失败过的，避免反复重试（例如需要大会员的内容）。
pub fn retry_after_login(app: &AppHandle, site: &str, automatic: bool) -> usize {
    let state = st(app);
    let t = now();
    let since = t - 3 * 86400;
    let settled = if automatic { t - 600 } else { i64::MAX };
    let mut n = 0;
    if let Ok(rows) = state.db.inbox_with_status("('failed')") {
        for r in rows.into_iter().filter(|r| r.created_at > since && r.updated_at < settled && same_site(&r.site, site)) {
            if matches!(r.error_kind.as_deref(), Some("need_login") | Some("rate_limited")) && retry(app, r.id).is_ok() {
                n += 1;
            }
        }
    }
    // 队列里因登录失败的任务（不一定来自收件箱）
    for t in state.downloads.snapshots() {
        if t.status == TaskStatus::Failed
            && matches!(t.error_kind, Some(ErrorKind::NeedLogin))
            && same_site(&t.site, site)
            && t.finished_at.map_or(true, |f| f < settled)
        {
            crate::download::resume(app, t.id);
            n += 1;
        }
    }
    if n > 0 {
        log::info!("retrying {n} items after saving login for {site}");
    }
    n
}

/// 网站标识比较：平台 ID 或域名，`www.` 与子域名视为同一网站。
fn same_site(a: &str, b: &str) -> bool {
    let a = a.trim_start_matches("www.");
    let b = b.trim_start_matches("www.");
    !a.is_empty() && (a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}")))
}

/// 手动解析失败：更新刚收到的同一链接的记录，没有时新记一条（来源为手动）。
pub fn record_manual_failure(app: &AppHandle, text: &str, err: &AppError) {
    let Some(url) = primary_url(text) else { return };
    let state = st(app);
    let recent: Option<i64> = state
        .db
        .conn()
        .query_row(
            "SELECT id FROM inbox WHERE url=?1 AND created_at>?2 AND status IN ('confirm','playlist','failed','resolving') ORDER BY id DESC LIMIT 1",
            params![url, now() - 86400],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let id = match recent {
        Some(id) => id,
        None => match state.db.inbox_add("manual", "", "", text, "failed") {
            Ok(id) => id,
            Err(_) => return,
        },
    };
    patch(
        app,
        id,
        Patch { status: Some("failed"), message: Some(Some(err.message.clone())), error_kind: Some(Some(kind_str(err.kind))), ..Default::default() },
    );
}

/// 剪贴板识别到链接：记录下来。返回各链接对应的记录编号。
pub fn record_clipboard(app: &AppHandle, urls: &[String], auto: bool) -> Vec<Option<i64>> {
    if !st(app).settings().inbox.record_clipboard {
        return urls.iter().map(|_| None).collect();
    }
    urls.iter().map(|u| record(app, "clipboard", "", "", u, if auto { "resolving" } else { "confirm" })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(id: &str, url: &str) -> MediaInfo {
        serde_json::from_value(serde_json::json!({
            "platform": "bilibili", "platformName": "B站", "id": id, "title": "标题", "author": "作者",
            "kind": "video", "sourceUrl": url, "assets": []
        }))
        .unwrap()
    }

    #[test]
    fn add_list_and_counts() {
        let db = Db::open_in_memory().unwrap();
        let a = db.inbox_add("clipboard", "", "", "看看 https://www.bilibili.com/video/BV1xx411c7mD 这个", "confirm").unwrap();
        let b = db.inbox_add("phone", "dev1", "我的手机", "https://www.bilibili.com/video/BV1xx411c7mD", "failed").unwrap();
        let item = db.inbox_get(a).unwrap().unwrap();
        assert_eq!(item.url, "https://www.bilibili.com/video/BV1xx411c7mD");
        assert_eq!(item.site, "bilibili");
        assert_eq!(db.inbox_get(b).unwrap().unwrap().seen_before, 1);
        assert_eq!(db.inbox_counts().unwrap().unhandled, 2);
        let phone = db.inbox_list(&InboxFilter { source: Some("phone".into()), ..Default::default() }).unwrap();
        assert_eq!(phone.len(), 1);
        let q = db.inbox_list(&InboxFilter { query: "我的手机".into(), ..Default::default() }).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(db.inbox_for_device("dev1", 20).unwrap().len(), 1);
    }

    #[test]
    fn parsed_then_linked_to_tasks() {
        let db = Db::open_in_memory().unwrap();
        let url = "https://www.bilibili.com/video/BV1xx411c7mD";
        let id = db.inbox_add("clipboard", "", "", url, "confirm").unwrap();
        db.inbox_mark_parsed(url, &media("BV1xx411c7mD", url)).unwrap();
        assert_eq!(db.inbox_get(id).unwrap().unwrap().media_id.as_deref(), Some("BV1xx411c7mD"));
        assert!(db.inbox_link_tasks("bilibili", "BV1xx411c7mD", &[7, 8]).unwrap());
        let item = db.inbox_get(id).unwrap().unwrap();
        assert_eq!(item.status, "queued");
        assert_eq!(item.task_ids, vec![7, 8]);
        // 已完成的不再被关联
        db.inbox_patch(id, Patch { status: Some("done"), ..Default::default() }).unwrap();
        assert!(!db.inbox_link_tasks("bilibili", "BV1xx411c7mD", &[9]).unwrap());
    }

    #[test]
    fn prune_keeps_active() {
        let db = Db::open_in_memory().unwrap();
        for i in 0..5 {
            db.inbox_add("clipboard", "", "", &format!("https://example.com/{i}"), if i == 0 { "queued" } else { "done" }).unwrap();
        }
        db.inbox_prune(0, 2).unwrap();
        let left = db.inbox_list(&InboxFilter::default()).unwrap();
        assert_eq!(left.len(), 3, "two newest plus the active one");
        assert!(left.iter().any(|i| i.status == "queued"));
    }

    #[test]
    fn site_mapping() {
        assert_eq!(site_for("https://youtu.be/abc"), "youtube.com");
        assert_eq!(site_for("https://twitter.com/a/status/1"), "x.com");
        assert_eq!(site_for("https://www.bilibili.com/video/BV1xx411c7mD"), "bilibili");
        assert!(same_site("www.youtube.com", "youtube.com"));
        assert!(same_site("m.youtube.com", "youtube.com"));
        assert!(!same_site("bilibili", "youtube.com"));
        assert!(!same_site("", "x.com"));
    }
}
