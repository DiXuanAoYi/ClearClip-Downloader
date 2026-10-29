//! 订阅与追更：定期检查频道、UP 主、画师、合集等列表，有新内容时自动下载。
//!
//! - 检查间隔每个订阅单独设置（最短 1 小时），在设定时间上随机加减，避免固定节奏
//! - 每次只取最新一页；已见过的条目记录在 `subscription_items`，不会重复下载
//! - 首次订阅：只下载以后的新内容（默认）/ 下载最近 N 条 / 下载全部
//! - 过滤：标题包含 / 排除关键词、时长范围、发布时间
//! - 单次检查最多自动下载 N 条，超出的进入“待下载”等用户确认
//! - 连续 3 次检查失败自动暂停并通知
//! - 程序关闭期间错过的检查，启动后补做

use std::sync::Arc;
use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{now, Db};
use crate::download::{self, EnqueueExtra, PostOptions};
use crate::error::{AppError, AppResult};
use crate::providers::listing::{self, ListResult, SubEntry};
use crate::settings::QualityPreset;
use crate::{providers, quality, AppState};

pub const EVT_SUBS: &str = "subs://updated";
/// 每次检查拉取的条目数
const PAGE: usize = 30;
/// “下载全部”时最多拉取的条目数
const ALL_LIMIT: usize = 500;
const MAX_FAILS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubSettings {
    pub interval_hours: u32,
    /// new_only / latest / all
    pub first_run: String,
    pub first_n: usize,
    /// 单次检查最多自动下载的条数
    pub max_auto: usize,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub min_duration_s: Option<u64>,
    pub max_duration_s: Option<u64>,
    /// 只下载最近多少天发布的
    pub max_age_days: Option<u32>,
    /// 清晰度预设（为空时用全局设置）
    pub quality: Option<QualityPreset>,
    /// 保存目录（为空时为“下载目录 / 订阅名”）
    pub dir: String,
    /// 命名模板（为空时用全局模板）
    pub template: String,
    pub notify: bool,
    /// 只保留最近多少个作品，更早的移到回收站（0 表示全部保留）
    pub keep_latest: u32,
    /// 只保留最近多少天下载的，更早的移到回收站（0 表示全部保留）
    pub keep_days: u32,
}

impl Default for SubSettings {
    fn default() -> Self {
        SubSettings {
            interval_hours: 6,
            first_run: "new_only".into(),
            first_n: 5,
            max_auto: 20,
            include: vec![],
            exclude: vec![],
            min_duration_s: None,
            max_duration_s: None,
            max_age_days: None,
            quality: None,
            dir: String::new(),
            template: String::new(),
            notify: true,
            keep_latest: 0,
            keep_days: 0,
        }
    }
}

impl SubSettings {
    pub fn normalize(&mut self) {
        self.interval_hours = self.interval_hours.clamp(1, 24 * 7);
        if !matches!(self.first_run.as_str(), "new_only" | "latest" | "all") {
            self.first_run = "new_only".into();
        }
        self.first_n = self.first_n.clamp(1, ALL_LIMIT);
        self.max_auto = self.max_auto.clamp(1, 200);
        for v in [&mut self.include, &mut self.exclude] {
            *v = v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        self.dir = self.dir.trim().to_string();
        self.template = self.template.trim().to_string();
        self.keep_latest = self.keep_latest.min(100_000);
        self.keep_days = self.keep_days.min(36_500);
    }

    /// 条目是否通过过滤条件；不通过时返回原因。
    pub fn filter(&self, e: &SubEntry, now: i64) -> Result<(), String> {
        let title = e.title.to_lowercase();
        if !self.include.is_empty() && !self.include.iter().any(|k| title.contains(&k.to_lowercase())) {
            return Err("标题不含指定关键词".into());
        }
        if let Some(k) = self.exclude.iter().find(|k| title.contains(&k.to_lowercase())) {
            return Err(format!("标题包含排除词“{k}”"));
        }
        if let Some(d) = e.duration_ms.map(|d| d / 1000) {
            if self.min_duration_s.is_some_and(|m| d < m) {
                return Err("时长太短".into());
            }
            if self.max_duration_s.is_some_and(|m| m > 0 && d > m) {
                return Err("时长太长".into());
            }
        }
        if let (Some(days), Some(t)) = (self.max_age_days.filter(|d| *d > 0), e.published_at) {
            if now - t > days as i64 * 86400 {
                return Err(format!("发布超过 {days} 天"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub platform: String,
    pub platform_name: String,
    pub avatar: Option<String>,
    pub settings: SubSettings,
    /// active / paused / error
    pub status: String,
    pub last_check: Option<i64>,
    pub next_check: Option<i64>,
    pub last_error: Option<String>,
    pub fail_count: i64,
    /// 上次查看后的新内容数
    pub new_count: i64,
    pub created_at: i64,
    pub downloaded: i64,
    pub pending: i64,
    pub ignored: i64,
    pub checking: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubItem {
    pub item_id: String,
    pub title: String,
    pub url: String,
    pub thumbnail: Option<String>,
    pub published_at: Option<i64>,
    pub duration_ms: Option<u64>,
    /// seen / pending / queued / downloaded / ignored / failed
    pub status: String,
    pub reason: Option<String>,
    pub created_at: i64,
}

#[derive(Default)]
pub struct SubsState {
    checking: std::sync::Mutex<std::collections::HashSet<i64>>,
}

fn st(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

// ---------- 数据库 ----------

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    platform TEXT NOT NULL,
    platform_name TEXT NOT NULL,
    avatar TEXT,
    settings_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    last_check INTEGER,
    next_check INTEGER,
    last_error TEXT,
    fail_count INTEGER NOT NULL DEFAULT 0,
    new_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS subscription_items (
    sub_id INTEGER NOT NULL,
    item_id TEXT NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    thumbnail TEXT,
    published_at INTEGER,
    duration_ms INTEGER,
    status TEXT NOT NULL,
    reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (sub_id, item_id)
);
CREATE INDEX IF NOT EXISTS idx_sub_items_status ON subscription_items(sub_id, status);
";

const SUB_COLS: &str = "s.id, s.url, s.title, s.platform, s.platform_name, s.avatar, s.settings_json, s.status, s.last_check, s.next_check, s.last_error, s.fail_count, s.new_count, s.created_at,
    (SELECT COUNT(*) FROM subscription_items i WHERE i.sub_id=s.id AND i.status='downloaded'),
    (SELECT COUNT(*) FROM subscription_items i WHERE i.sub_id=s.id AND i.status='pending'),
    (SELECT COUNT(*) FROM subscription_items i WHERE i.sub_id=s.id AND i.status='ignored')";

fn sub_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Subscription> {
    let settings_json: String = r.get(6)?;
    Ok(Subscription {
        id: r.get(0)?,
        url: r.get(1)?,
        title: r.get(2)?,
        platform: r.get(3)?,
        platform_name: r.get(4)?,
        avatar: r.get(5)?,
        settings: serde_json::from_str(&settings_json).unwrap_or_default(),
        status: r.get(7)?,
        last_check: r.get(8)?,
        next_check: r.get(9)?,
        last_error: r.get(10)?,
        fail_count: r.get(11)?,
        new_count: r.get(12)?,
        created_at: r.get(13)?,
        downloaded: r.get(14)?,
        pending: r.get(15)?,
        ignored: r.get(16)?,
        checking: false,
    })
}

impl Db {
    pub fn subs_list(&self) -> AppResult<Vec<Subscription>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {SUB_COLS} FROM subscriptions s ORDER BY s.created_at DESC"))?;
        let rows = stmt.query_map([], sub_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn sub_get(&self, id: i64) -> AppResult<Option<Subscription>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!("SELECT {SUB_COLS} FROM subscriptions s WHERE s.id=?1"))?;
        Ok(stmt.query_row(params![id], sub_row).optional()?)
    }

    fn sub_insert(&self, url: &str, list: &ListResult, settings: &SubSettings) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO subscriptions (url, title, platform, platform_name, avatar, settings_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![url, list.title, list.platform, list.platform_name, list.avatar, serde_json::to_string(settings)?, now()],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => AppError::invalid("已经订阅过这个链接了。"),
            e => e.into(),
        })?;
        Ok(conn.last_insert_rowid())
    }

    pub fn sub_update(&self, id: i64, title: &str, settings: &SubSettings) -> AppResult<()> {
        self.conn().execute("UPDATE subscriptions SET title=?2, settings_json=?3 WHERE id=?1", params![id, title, serde_json::to_string(settings)?])?;
        Ok(())
    }

    pub fn sub_set_status(&self, id: i64, status: &str) -> AppResult<()> {
        let fail = if status == "active" { ", fail_count=0, last_error=NULL" } else { "" };
        self.conn().execute(&format!("UPDATE subscriptions SET status=?2{fail} WHERE id=?1"), params![id, status])?;
        Ok(())
    }

    fn sub_checked(&self, id: i64, next: i64, new_items: i64) -> AppResult<()> {
        self.conn().execute(
            "UPDATE subscriptions SET last_check=?2, next_check=?3, fail_count=0, last_error=NULL, new_count=new_count+?4 WHERE id=?1",
            params![id, now(), next, new_items],
        )?;
        Ok(())
    }

    /// 记录一次失败，返回连续失败次数。
    fn sub_failed(&self, id: i64, next: i64, err: &str) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "UPDATE subscriptions SET last_check=?2, next_check=?3, fail_count=fail_count+1, last_error=?4 WHERE id=?1",
            params![id, now(), next, err],
        )?;
        Ok(conn.query_row("SELECT fail_count FROM subscriptions WHERE id=?1", params![id], |r| r.get(0))?)
    }

    pub fn sub_clear_new(&self, id: i64) -> AppResult<()> {
        self.conn().execute("UPDATE subscriptions SET new_count=0 WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn sub_delete(&self, id: i64) -> AppResult<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM subscription_items WHERE sub_id=?1", params![id])?;
        conn.execute("DELETE FROM subscriptions WHERE id=?1", params![id])?;
        Ok(())
    }

    fn sub_known_ids(&self, id: i64) -> AppResult<std::collections::HashSet<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT item_id FROM subscription_items WHERE sub_id=?1")?;
        let rows = stmt.query_map(params![id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn sub_item_insert(&self, id: i64, e: &SubEntry, status: &str, reason: Option<&str>) -> AppResult<()> {
        let t = now();
        self.conn().execute(
            "INSERT OR IGNORE INTO subscription_items (sub_id, item_id, title, url, thumbnail, published_at, duration_ms, status, reason, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![id, e.id, e.title, e.url, e.thumbnail, e.published_at, e.duration_ms.map(|d| d as i64), status, reason, t],
        )?;
        Ok(())
    }

    pub fn sub_item_status(&self, id: i64, item_id: &str, status: &str, title: Option<&str>) -> AppResult<()> {
        self.conn().execute(
            // 只替换占位标题（如“作品 123”），列表里原有的标题通常更准确
            "UPDATE subscription_items SET status=?3, title=CASE WHEN title LIKE '作品 %' THEN COALESCE(?4, title) ELSE title END, updated_at=?5 WHERE sub_id=?1 AND item_id=?2",
            params![id, item_id, status, title, now()],
        )?;
        Ok(())
    }

    pub fn sub_items(&self, id: i64, statuses: &[&str]) -> AppResult<Vec<SubItem>> {
        let conn = self.conn();
        let marks = statuses.iter().map(|s| format!("'{}'", s.replace('\'', ""))).collect::<Vec<_>>().join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT item_id, title, url, thumbnail, published_at, duration_ms, status, reason, created_at FROM subscription_items
             WHERE sub_id=?1 AND status IN ({marks}) ORDER BY COALESCE(published_at, created_at) DESC, created_at DESC LIMIT 1000"
        ))?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(SubItem {
                item_id: r.get(0)?,
                title: r.get(1)?,
                url: r.get(2)?,
                thumbnail: r.get(3)?,
                published_at: r.get(4)?,
                duration_ms: r.get::<_, Option<i64>>(5)?.map(|d| d as u64),
                status: r.get(6)?,
                reason: r.get(7)?,
                created_at: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn sub_due(&self, t: i64) -> AppResult<Vec<i64>> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT id FROM subscriptions WHERE status='active' AND (next_check IS NULL OR next_check<=?1) ORDER BY next_check")?;
        let rows = stmt.query_map(params![t], |r| r.get::<_, i64>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

/// 下载任务结束后更新订阅条目。
pub fn mark_item(st: &AppState, sub_id: i64, item_id: &str, status: &str, title: Option<&str>) {
    if let Err(e) = st.db.sub_item_status(sub_id, item_id, status, title) {
        log::warn!("update subscription item failed: {e}");
    }
}

// ---------- 检查 ----------

/// 下次检查时间：间隔上随机加减最多 10%（不超过 15 分钟）。
pub fn next_check_time(from: i64, interval_hours: u32, jitter_seed: u64) -> i64 {
    let base = interval_hours as i64 * 3600;
    let span = (base / 10).clamp(1, 900);
    let offset = (jitter_seed % (2 * span as u64 + 1)) as i64 - span;
    from + base + offset
}

fn jitter() -> u64 {
    use aes_gcm::aead::rand_core::RngCore;
    aes_gcm::aead::OsRng.next_u64()
}

/// 获取列表：抖音用户主页用隐藏窗口，其他用接口或 yt-dlp。
pub async fn fetch_list(app: &AppHandle, url: &str, limit: usize) -> AppResult<ListResult> {
    if listing::is_douyin_user(url) {
        let ids = douyin_user_ids(app, url).await?;
        return Ok(ListResult {
            title: "抖音用户作品".into(),
            platform: "douyin".into(),
            platform_name: "抖音".into(),
            avatar: None,
            entries: listing::douyin_entries(&ids),
        });
    }
    let state = st(app);
    let settings = state.settings();
    let ctx = state.parse_ctx(&settings);
    if let Ok(u) = url::Url::parse(url) {
        let host = u.host_str().map(crate::cookies::registrable_domain).unwrap_or_default();
        state.net.wait_turn(&host, Duration::from_millis(settings.site_request_interval_ms.max(1000))).await;
    }
    listing::list(&ctx, url, limit).await
}

/// 用隐藏窗口打开抖音用户主页，读取页面上的作品链接（作品列表接口需要签名，直接请求难度和风险都很高）。
/// 与内置登录窗口共用浏览器数据，所以登录过抖音后也能看到需要登录的主页。
async fn douyin_user_ids(app: &AppHandle, url: &str) -> AppResult<Vec<String>> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let label = format!("dysub-{}", jitter() % 1_000_000);
    let parsed = url::Url::parse(url).map_err(|_| AppError::invalid("链接格式不正确"))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    // 页面加载后定时收集作品链接，并滚动加载更多；结果写进标题，由程序读取
    let script = r#"
      (function(){
        var n = 0;
        var timer = setInterval(function(){
          n++;
          var ids = [];
          document.querySelectorAll('a[href*="/video/"], a[href*="/note/"]').forEach(function(a){
            var m = a.href.match(/\/(?:video|note)\/(\d{8,})/);
            if (m && ids.indexOf(m[1]) < 0) ids.push(m[1]);
          });
          if (n % 2 === 0) window.scrollTo(0, document.body.scrollHeight);
          if (ids.length >= 30 || n >= 16) { document.title = 'CCIDS:' + ids.join(','); clearInterval(timer); }
        }, 1200);
      })();
    "#;
    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parsed))
        .title("ClearClip")
        .visible(false)
        .inner_size(1200.0, 900.0)
        .initialization_script(script)
        .on_document_title_changed(move |_w, title| {
            if let Some(ids) = title.strip_prefix("CCIDS:") {
                let _ = tx.send(ids.to_string());
            }
        })
        .build()?;
    let result = tokio::time::timeout(Duration::from_secs(40), rx.recv()).await;
    let _ = win.close();
    let ids: Vec<String> = match result {
        Ok(Some(s)) => s.split(',').filter(|x| !x.is_empty()).map(String::from).collect(),
        _ => vec![],
    };
    if ids.is_empty() {
        return Err(AppError::need_login("没有从抖音主页读到作品。主页可能需要登录：请在“设置 → 账号与 Cookie”中用内置登录窗口登录抖音后重试。"));
    }
    Ok(ids)
}

/// 预览一个链接能否订阅（返回标题和最新条目）。
pub async fn preview(app: &AppHandle, url: &str) -> AppResult<ListResult> {
    let mut r = fetch_list(app, url, 10).await?;
    r.entries.truncate(10);
    Ok(r)
}

pub async fn add(app: &AppHandle, url: &str, title: Option<String>, mut settings: SubSettings) -> AppResult<Subscription> {
    let state = st(app);
    if !state.settings().subscriptions_enabled {
        return Err(AppError::invalid("订阅功能未开启。"));
    }
    settings.normalize();
    let url = listing::normalize_url(url.trim());
    let list = fetch_list(app, &url, 10).await?;
    let mut list2 = list.clone();
    if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
        list2.title = t.trim().to_string();
    }
    let id = state.db.sub_insert(&url, &list2, &settings)?;
    emit(app);
    // 首次检查在后台进行（按“首次订阅”策略处理已有内容）
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = check(&app2, id).await;
    });
    state.db.sub_get(id)?.ok_or_else(|| AppError::msg("订阅保存失败"))
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVT_SUBS, ());
}

pub fn list(app: &AppHandle) -> AppResult<Vec<Subscription>> {
    let state = st(app);
    let checking = state.subs.checking.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut v = state.db.subs_list()?;
    for s in &mut v {
        s.checking = checking.contains(&s.id);
    }
    Ok(v)
}

/// 检查一个订阅。返回本次新发现并加入队列 / 待下载的数量。
pub async fn check(app: &AppHandle, id: i64) -> AppResult<usize> {
    let state = st(app);
    {
        let mut c = state.subs.checking.lock().unwrap_or_else(|e| e.into_inner());
        if !c.insert(id) {
            return Ok(0);
        }
    }
    emit(app);
    let result = check_inner(app, &state, id).await;
    state.subs.checking.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    if let Err(e) = &result {
        log::warn!("subscription {id} check failed: {e}");
        let sub = state.db.sub_get(id).ok().flatten();
        let interval = sub.as_ref().map(|s| s.settings.interval_hours).unwrap_or(6);
        // 失败后按较短间隔重试（不超过正常间隔）
        let retry = next_check_time(now(), interval.min(2), jitter());
        if let Ok(fails) = state.db.sub_failed(id, retry, &e.message) {
            if fails >= MAX_FAILS {
                let _ = state.db.sub_set_status(id, "error");
                notify(app, "订阅已暂停", &format!("“{}”连续 {fails} 次检查失败，已自动暂停：{}", sub.map(|s| s.title).unwrap_or_default(), e.message));
            }
        }
    }
    emit(app);
    result
}

async fn check_inner(app: &AppHandle, state: &Arc<AppState>, id: i64) -> AppResult<usize> {
    let sub = state.db.sub_get(id)?.ok_or_else(|| AppError::not_found("订阅不存在"))?;
    let known = state.db.sub_known_ids(id)?;
    let first = known.is_empty() && sub.last_check.is_none();
    let limit = if first && sub.settings.first_run == "all" { ALL_LIMIT } else { PAGE };
    let list = fetch_list(app, &sub.url, limit).await?;
    let t = now();

    let fresh: Vec<&SubEntry> = list.entries.iter().filter(|e| !known.contains(&e.id)).collect();
    let mut to_download: Vec<SubEntry> = vec![];
    let mut found = 0usize;
    for (i, e) in fresh.iter().enumerate() {
        if first {
            let take = match sub.settings.first_run.as_str() {
                "all" => true,
                "latest" => i < sub.settings.first_n,
                _ => false,
            };
            if !take {
                state.db.sub_item_insert(id, e, "seen", Some("订阅前已发布"))?;
                continue;
            }
        }
        match sub.settings.filter(e, t) {
            Err(reason) => state.db.sub_item_insert(id, e, "ignored", Some(&reason))?,
            Ok(()) if state.settings().metered_mode => {
                state.db.sub_item_insert(id, e, "pending", Some("省流量模式：等待确认"))?;
                found += 1;
            }
            Ok(()) if to_download.len() < sub.settings.max_auto => {
                state.db.sub_item_insert(id, e, "queued", None)?;
                to_download.push((*e).clone());
                found += 1;
            }
            Ok(()) => {
                state.db.sub_item_insert(id, e, "pending", Some("超过单次自动下载数量，等待确认"))?;
                found += 1;
            }
        }
    }
    state.db.sub_checked(id, next_check_time(t, sub.settings.interval_hours, jitter()), found as i64)?;
    if list.avatar.is_some() && sub.avatar.is_none() {
        let _ = state.db.conn().execute("UPDATE subscriptions SET avatar=?2 WHERE id=?1", params![id, list.avatar]);
    }
    emit(app);
    {
        let (state2, sub2) = (state.clone(), sub.clone());
        let _ = tokio::task::spawn_blocking(move || apply_retention(&state2, &sub2)).await;
    }
    if !to_download.is_empty() {
        let n = to_download.len();
        download_entries(app, &sub, to_download).await;
        if sub.settings.notify && !first {
            notify(app, "订阅有更新", &format!("“{}”有 {n} 个新内容，已加入下载队列", sub.title));
        }
    }
    Ok(found)
}

/// 订阅的保存目录：自己设置的，或“下载目录 / 订阅名”。
pub fn sub_dir(download_root: &std::path::Path, sub: &Subscription) -> std::path::PathBuf {
    if sub.settings.dir.is_empty() {
        download_root.join(crate::naming::sanitize(&sub.title))
    } else {
        std::path::PathBuf::from(&sub.settings.dir)
    }
}

/// 按保留规则找出要清理的记录：同一个作品的视频、字幕、封面一起算，按下载时间从新到旧保留。
pub fn retention_victims(items: &[crate::db::LibraryItem], keep_latest: u32, keep_days: u32, now: i64) -> Vec<i64> {
    use std::collections::HashMap;
    if keep_latest == 0 && keep_days == 0 {
        return vec![];
    }
    let mut groups: HashMap<(&str, &str), Vec<&crate::db::LibraryItem>> = HashMap::new();
    for i in items {
        groups.entry((i.platform.as_str(), i.media_id.as_str())).or_default().push(i);
    }
    let mut ordered: Vec<(i64, Vec<&crate::db::LibraryItem>)> = groups.into_values().map(|v| (v.iter().map(|i| i.finished_at).max().unwrap_or(0), v)).collect();
    ordered.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    let mut out = vec![];
    for (idx, (newest, group)) in ordered.iter().enumerate() {
        let too_many = keep_latest > 0 && idx >= keep_latest as usize;
        let too_old = keep_days > 0 && now - newest > keep_days as i64 * 86400;
        if too_many || too_old {
            out.extend(group.iter().map(|i| i.id));
        }
    }
    out
}

/// 按订阅的保留规则清理旧文件（移到回收站，开启回收站时可还原）。返回清理的文件数。
pub fn apply_retention(state: &AppState, sub: &Subscription) -> usize {
    if sub.settings.keep_latest == 0 && sub.settings.keep_days == 0 {
        return 0;
    }
    let dir = sub_dir(&state.settings().download_root(), sub);
    let Ok(all) = state.db.search_library(&crate::db::LibraryFilter { source: Some("subscription".into()), ..Default::default() }, 100_000) else { return 0 };
    let mine: Vec<crate::db::LibraryItem> = all.into_iter().filter(|i| std::path::Path::new(&i.path).starts_with(&dir)).collect();
    let victims = retention_victims(&mine, sub.settings.keep_latest, sub.settings.keep_days, now());
    if victims.is_empty() {
        return 0;
    }
    let rep = crate::library_cmds::delete_items(state, &victims, true);
    log::info!("subscription {} retention: removed {} files ({} to trash)", sub.id, rep.removed, rep.trashed);
    rep.removed
}

/// 逐条解析并加入下载队列（订阅自己的保存目录、命名和清晰度）。
async fn download_entries(app: &AppHandle, sub: &Subscription, entries: Vec<SubEntry>) {
    let state = st(app);
    for e in entries {
        let mut settings = state.settings();
        if let Some(q) = sub.settings.quality {
            settings.quality_preset = q;
        }
        let ctx = state.parse_ctx(&settings);
        let res = async {
            let mut info = providers::resolve_url(&ctx, &e.url).await?;
            if info.kind == crate::model::MediaKind::Playlist {
                return Err(AppError::invalid("条目本身是一个列表，已跳过"));
            }
            quality::sort_videos(&mut info, &settings);
            let (ids, post): (Vec<String>, PostOptions) = quality::auto_selection(&info, &settings);
            let dir = sub_dir(&settings.download_root(), sub);
            let extra = EnqueueExtra {
                dir: Some(dir),
                template: (!sub.settings.template.is_empty()).then(|| sub.settings.template.clone()),
                origin: Some("subscription".into()),
                sub_item: Some((sub.id, e.id.clone())),
                tried: vec![],
            };
            download::enqueue_ext(app, info, &ids, post, extra)
        }
        .await;
        match res {
            Ok(r) if r.tasks.is_empty() => mark_item(&state, sub.id, &e.id, "downloaded", None),
            Ok(_) => {}
            Err(err) => {
                log::info!("subscription {} item {} failed: {err}", sub.id, e.id);
                let _ = state.db.conn().execute(
                    "UPDATE subscription_items SET status='failed', reason=?3, updated_at=?4 WHERE sub_id=?1 AND item_id=?2",
                    params![sub.id, e.id, err.message, now()],
                );
            }
        }
    }
    emit(app);
}

/// 手动下载（待下载 / 已忽略 / 失败的条目）。
pub async fn download_items(app: &AppHandle, id: i64, item_ids: &[String]) -> AppResult<usize> {
    let state = st(app);
    let sub = state.db.sub_get(id)?.ok_or_else(|| AppError::not_found("订阅不存在"))?;
    let items = state.db.sub_items(id, &["pending", "ignored", "failed", "seen", "downloaded"])?;
    let entries: Vec<SubEntry> = items
        .into_iter()
        .filter(|i| item_ids.contains(&i.item_id))
        .map(|i| SubEntry { id: i.item_id, title: i.title, url: i.url, thumbnail: i.thumbnail, published_at: i.published_at, duration_ms: i.duration_ms })
        .collect();
    for e in &entries {
        state.db.sub_item_status(id, &e.id, "queued", None)?;
    }
    let n = entries.len();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move { download_entries(&app2, &sub, entries).await });
    emit(app);
    Ok(n)
}

pub fn ignore_items(app: &AppHandle, id: i64, item_ids: &[String]) -> AppResult<()> {
    let state = st(app);
    for i in item_ids {
        state.db.sub_item_status(id, i, "ignored", None)?;
    }
    emit(app);
    Ok(())
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    crate::security::system_notification(app, title, body, false);
    crate::notify::emit(app, crate::notify::Event::Sub, title, body);
}

/// 后台调度：每分钟检查一次到期的订阅；启动后补做错过的检查。
pub fn spawn_scheduler(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            let state = st(&app);
            if state.settings().subscriptions_enabled {
                match state.db.sub_due(now()) {
                    Ok(ids) => {
                        for id in ids {
                            let _ = check(&app, id).await;
                            // 不同订阅之间也留出间隔
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                    Err(e) => log::warn!("load due subscriptions failed: {e}"),
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, dur: Option<u64>, published: Option<i64>) -> SubEntry {
        SubEntry { id: title.into(), title: title.into(), url: String::new(), thumbnail: None, published_at: published, duration_ms: dur.map(|d| d * 1000) }
    }

    #[test]
    fn filters() {
        let s = SubSettings {
            include: vec!["教程".into()],
            exclude: vec!["预告".into()],
            min_duration_s: Some(60),
            max_duration_s: Some(3600),
            max_age_days: Some(7),
            ..Default::default()
        };
        let now = 1_700_000_000;
        assert!(s.filter(&entry("Rust 教程 01", Some(600), Some(now - 3600)), now).is_ok());
        assert!(s.filter(&entry("Vlog", Some(600), None), now).unwrap_err().contains("关键词"));
        assert!(s.filter(&entry("教程 预告", Some(600), None), now).unwrap_err().contains("预告"));
        assert!(s.filter(&entry("教程 短", Some(30), None), now).unwrap_err().contains("太短"));
        assert!(s.filter(&entry("教程 长", Some(7200), None), now).unwrap_err().contains("太长"));
        assert!(s.filter(&entry("教程 旧", None, Some(now - 30 * 86400)), now).unwrap_err().contains("7 天"));
        // 不知道时长和发布时间时不过滤
        assert!(s.filter(&entry("教程 未知", None, None), now).is_ok());
    }

    #[test]
    fn jittered_schedule() {
        for seed in [0u64, 1, 899, 900, 1799, 123_456_789] {
            let n = next_check_time(1000, 6, seed);
            assert!((1000 + 6 * 3600 - 900..=1000 + 6 * 3600 + 900).contains(&n), "{n}");
        }
        let n = next_check_time(0, 1, 0);
        assert!((3600 - 360..=3600 + 360).contains(&n));
    }

    #[test]
    fn settings_normalize() {
        let mut s = SubSettings { interval_hours: 0, first_run: "x".into(), max_auto: 0, include: vec![" a ".into(), "".into()], ..Default::default() };
        s.normalize();
        assert_eq!(s.interval_hours, 1);
        assert_eq!(s.first_run, "new_only");
        assert_eq!(s.max_auto, 1);
        assert_eq!(s.include, vec!["a"]);
    }

    fn li(id: i64, media: &str, kind: &str, finished_at: i64) -> crate::db::LibraryItem {
        crate::db::LibraryItem {
            id,
            platform: "bilibili".into(),
            media_id: media.into(),
            asset_id: kind.into(),
            title: media.into(),
            author: String::new(),
            cover: None,
            path: format!("/dl/{media}.{kind}"),
            size: 1,
            finished_at,
            exists: true,
            kind: kind.into(),
            source: "subscription".into(),
            source_url: String::new(),
            platform_name: String::new(),
            cover_path: None,
            favorite: false,
            rating: 0,
            note: String::new(),
            tags: vec![],
            duration_ms: None,
        }
    }

    #[test]
    fn retention_removes_whole_works_oldest_first() {
        let day = 86400;
        let now = 100 * day;
        // 四个作品：A 最新（有视频和字幕）、B、C、D 最旧
        let items = vec![
            li(1, "A", "video", now - day),
            li(2, "A", "subtitle", now - day + 10),
            li(3, "B", "video", now - 5 * day),
            li(4, "C", "video", now - 20 * day),
            li(5, "C", "cover", now - 20 * day),
            li(6, "D", "video", now - 60 * day),
        ];
        assert!(retention_victims(&items, 0, 0, now).is_empty(), "no rule keeps everything");
        let mut v = retention_victims(&items, 2, 0, now);
        v.sort();
        assert_eq!(v, vec![4, 5, 6], "keep the 2 newest works, C's cover goes with its video");
        let mut v = retention_victims(&items, 0, 30, now);
        v.sort();
        assert_eq!(v, vec![6], "only D is older than 30 days");
        let mut v = retention_victims(&items, 3, 30, now);
        v.sort();
        assert_eq!(v, vec![6], "3 newest are kept by count, D is too old");
        let mut v = retention_victims(&items, 2, 30, now);
        v.sort();
        assert_eq!(v, vec![4, 5, 6], "both rules apply");
        assert!(retention_victims(&items, 10, 0, now).is_empty());
    }

    #[test]
    fn db_items_and_counts() {
        let db = Db::open_in_memory().unwrap();
        let list = ListResult { title: "频道".into(), platform: "youtube".into(), platform_name: "YouTube".into(), avatar: None, entries: vec![] };
        let id = db.sub_insert("https://y/c", &list, &SubSettings::default()).unwrap();
        assert!(db.sub_insert("https://y/c", &list, &SubSettings::default()).unwrap_err().to_string().contains("已经订阅"));
        db.sub_item_insert(id, &entry("a", None, Some(10)), "queued", None).unwrap();
        db.sub_item_insert(id, &entry("b", None, Some(20)), "pending", None).unwrap();
        db.sub_item_insert(id, &entry("c", None, Some(5)), "ignored", Some("x")).unwrap();
        db.sub_item_insert(id, &entry("a", None, None), "pending", None).unwrap(); // 已存在，忽略
        db.sub_item_status(id, "a", "downloaded", Some("A!")).unwrap();
        db.sub_item_insert(id, &entry("作品 9", None, None), "queued", None).unwrap();
        db.sub_item_status(id, "作品 9", "downloaded", Some("真正的标题")).unwrap();
        let s = db.sub_get(id).unwrap().unwrap();
        assert_eq!((s.downloaded, s.pending, s.ignored), (2, 1, 1));
        assert_eq!(db.sub_known_ids(id).unwrap().len(), 4);
        let items = db.sub_items(id, &["downloaded", "pending"]).unwrap();
        assert_eq!(
            items.iter().map(|i| i.item_id.as_str()).collect::<Vec<_>>(),
            vec!["作品 9", "b", "a"],
            "items without a publish time sort by when they were found"
        );
        assert_eq!(items[2].title, "a", "real titles are kept");
        assert_eq!(items[0].title, "真正的标题", "placeholder titles are replaced");
        assert_eq!(db.sub_due(now()).unwrap(), vec![id]);
        db.sub_checked(id, now() + 100, 2).unwrap();
        assert!(db.sub_due(now()).unwrap().is_empty());
        assert_eq!(db.sub_failed(id, 0, "boom").unwrap(), 1);
        assert_eq!(db.sub_failed(id, 0, "boom").unwrap(), 2);
        db.sub_set_status(id, "active").unwrap();
        assert_eq!(db.sub_get(id).unwrap().unwrap().fail_count, 0);
        db.sub_delete(id).unwrap();
        assert!(db.subs_list().unwrap().is_empty());
    }
}
