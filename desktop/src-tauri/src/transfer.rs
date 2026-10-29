//! 手机与电脑互传文件（局域网内，不经过任何外部服务器）。
//!
//! - 手机 → 电脑：手机网页把文件原样作为请求正文上传（`POST /api/upload`），电脑边收边写入磁盘
//!   （先写 `.part`，写完改名），任何时候内存里只有一个小缓冲区；文本没有链接时存为文字便签
//! - 电脑 → 手机：电脑上明确添加的文件 / 文字才会出现在手机网页的“电脑发来的”里（`GET /dl/<密钥>`），
//!   按块从磁盘读取发送，支持 Range（断点续传、拖动视频），不会暴露电脑上的其他文件
//! - 两个方向都要求访问令牌和已配对的设备；单文件大小有上限；收到的文件不会被自动运行

use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::db::{now, Db};
use crate::error::{AppError, AppResult};
use crate::phone::Request;
use crate::AppState;

pub const EVT_CHANGED: &str = "transfer://changed";
pub const EVT_PROGRESS: &str = "transfer://progress";
pub const EVT_RECEIVED: &str = "transfer://received";

/// 同时进行的传输数量上限
const MAX_ACTIVE: usize = 8;
/// 一条连接上超过这么久没有数据就放弃
const IDLE: Duration = Duration::from_secs(30);
/// 进度事件的最小间隔
const EMIT_EVERY: Duration = Duration::from_millis(250);
/// 收到文件的系统通知最小间隔
const NOTIFY_EVERY: Duration = Duration::from_secs(8);
/// 写盘前至少要保留的剩余空间
const RESERVE: u64 = 200 * 1024 * 1024;
const CHUNK: usize = 128 * 1024;
const MAX_NOTE: usize = 20_000;
const MAX_ZIP_ENTRIES: usize = 20_000;
const MAX_ZIP_BYTES: u64 = 20 * 1024 * 1024 * 1024;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS transfers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    direction TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL DEFAULT '',
    text TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0,
    done INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL,
    message TEXT,
    extra TEXT,
    key TEXT NOT NULL DEFAULT '',
    device TEXT NOT NULL DEFAULT '',
    device_name TEXT NOT NULL DEFAULT '',
    downloads INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_transfers_dir ON transfers(direction, created_at);
CREATE INDEX IF NOT EXISTS idx_transfers_key ON transfers(key);
";

// ---------- 记录 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferItem {
    pub id: i64,
    /// in：手机发给电脑；out：电脑发给手机
    pub direction: String,
    /// text / image / video / audio / archive / file
    pub kind: String,
    pub name: String,
    pub path: String,
    pub text: String,
    pub size: i64,
    /// 已传输的字节数（传输中为实时值）
    pub done: i64,
    /// in：receiving / done / failed；out：ready
    pub status: String,
    pub message: Option<String>,
    /// 自动解压后的文件夹
    pub extra: Option<String>,
    #[serde(skip)]
    pub key: String,
    pub device: String,
    pub device_name: String,
    /// out：被完整下载的次数
    pub downloads: i64,
    pub created_at: i64,
    pub expires_at: i64,
    /// 正在传输
    pub active: bool,
    /// 文件已被移动或删除
    pub missing: bool,
}

const COLS: &str = "id, direction, kind, name, path, text, size, done, status, message, extra, key, device, device_name, downloads, created_at, expires_at";

fn row(r: &Row) -> rusqlite::Result<TransferItem> {
    Ok(TransferItem {
        id: r.get(0)?,
        direction: r.get(1)?,
        kind: r.get(2)?,
        name: r.get(3)?,
        path: r.get(4)?,
        text: r.get(5)?,
        size: r.get(6)?,
        done: r.get(7)?,
        status: r.get(8)?,
        message: r.get(9)?,
        extra: r.get(10)?,
        key: r.get(11)?,
        device: r.get(12)?,
        device_name: r.get(13)?,
        downloads: r.get(14)?,
        created_at: r.get(15)?,
        expires_at: r.get(16)?,
        active: false,
        missing: false,
    })
}

pub struct NewTransfer<'a> {
    pub direction: &'a str,
    pub kind: &'a str,
    pub name: &'a str,
    pub path: &'a str,
    pub text: &'a str,
    pub size: i64,
    pub status: &'a str,
    pub key: &'a str,
    pub device: &'a str,
    pub device_name: &'a str,
    pub expires_at: i64,
}

impl Db {
    pub fn transfer_add(&self, n: &NewTransfer) -> AppResult<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO transfers (direction, kind, name, path, text, size, status, key, device, device_name, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![n.direction, n.kind, n.name, n.path, n.text, n.size, n.status, n.key, n.device, n.device_name, now(), n.expires_at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn transfer_get(&self, id: i64) -> AppResult<Option<TransferItem>> {
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM transfers WHERE id = ?1"), params![id], row).optional()?)
    }

    pub fn transfer_by_key(&self, key: &str) -> AppResult<Option<TransferItem>> {
        if key.is_empty() {
            return Ok(None);
        }
        Ok(self.conn().query_row(&format!("SELECT {COLS} FROM transfers WHERE key = ?1 AND direction = 'out'"), params![key], row).optional()?)
    }

    pub fn transfer_list(&self, direction: &str, limit: usize) -> AppResult<Vec<TransferItem>> {
        let conn = self.conn();
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM transfers WHERE direction = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2"))?;
        let rows = st.query_map(params![direction, limit as i64], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn transfer_for_device(&self, direction: &str, device: &str, limit: usize) -> AppResult<Vec<TransferItem>> {
        let conn = self.conn();
        let mut st = conn.prepare(&format!(
            "SELECT {COLS} FROM transfers WHERE direction = ?1 AND (device = ?2 OR ?1 = 'out') ORDER BY created_at DESC, id DESC LIMIT ?3"
        ))?;
        let rows = st.query_map(params![direction, device, limit as i64], row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn transfer_finish(&self, id: i64, status: &str, path: Option<&str>, done: i64, message: Option<&str>) -> AppResult<()> {
        let conn = self.conn();
        match path {
            Some(p) => conn.execute("UPDATE transfers SET status=?2, path=?3, done=?4, message=?5 WHERE id=?1", params![id, status, p, done, message])?,
            None => conn.execute("UPDATE transfers SET status=?2, done=?3, message=?4 WHERE id=?1", params![id, status, done, message])?,
        };
        Ok(())
    }

    pub fn transfer_set_extra(&self, id: i64, extra: Option<&str>, message: Option<&str>) -> AppResult<()> {
        self.conn().execute("UPDATE transfers SET extra=?2, message=?3 WHERE id=?1", params![id, extra, message])?;
        Ok(())
    }

    pub fn transfer_count_download(&self, id: i64) -> AppResult<()> {
        self.conn().execute("UPDATE transfers SET downloads = downloads + 1 WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn transfer_remove(&self, id: i64) -> AppResult<()> {
        self.conn().execute("DELETE FROM transfers WHERE id=?1", params![id])?;
        Ok(())
    }

    /// 撤回过期的“电脑发给手机”的项目，返回数量。
    pub fn transfer_expire(&self, at: i64) -> AppResult<usize> {
        Ok(self.conn().execute("DELETE FROM transfers WHERE direction='out' AND expires_at > 0 AND expires_at <= ?1", params![at])?)
    }

    /// 上次退出时还停在“接收中”的记录（程序被关掉）：返回它们的临时文件路径，并标为失败。
    pub fn transfer_fail_stale(&self) -> AppResult<Vec<String>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT path FROM transfers WHERE status = 'receiving'")?;
        let paths = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        conn.execute("UPDATE transfers SET status='failed', message='接收到一半时程序退出了' WHERE status = 'receiving'", [])?;
        Ok(paths)
    }
}

// ---------- 实时进度 ----------

#[derive(Default)]
struct LiveItem {
    done: u64,
    total: u64,
    streams: usize,
    last_emit: Option<Instant>,
}

#[derive(Default)]
pub struct TransferLive {
    items: Mutex<HashMap<i64, LiveItem>>,
    active: AtomicUsize,
    canceled: Mutex<HashSet<i64>>,
    last_notify: Mutex<Option<Instant>>,
}

impl TransferLive {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, LiveItem>> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn try_begin(&self) -> bool {
        // 不用 fetch_update：新版工具链把它改名为 try_update，旧版又没有新名字，手写比较交换两边都能编译
        let mut n = self.active.load(Ordering::SeqCst);
        while n < MAX_ACTIVE {
            match self.active.compare_exchange_weak(n, n + 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return true,
                Err(cur) => n = cur,
            }
        }
        false
    }

    /// (已传输, 总大小, 正在传输的连接数)
    fn snapshot(&self, id: i64) -> Option<(u64, u64, usize)> {
        self.lock().get(&id).map(|l| (l.done, l.total, l.streams))
    }

    fn is_canceled(&self, id: i64) -> bool {
        self.canceled.lock().unwrap_or_else(|e| e.into_inner()).contains(&id)
    }

    /// 一个传输连接的占位：离开作用域时自动登记结束。
    fn guard(self: &Arc<Self>, app: &AppHandle, id: i64, total: u64) -> LiveGuard {
        {
            let mut m = self.lock();
            let l = m.entry(id).or_default();
            l.streams += 1;
            l.total = total;
        }
        LiveGuard { live: self.clone(), app: app.clone(), id }
    }
}

struct LiveGuard {
    live: Arc<TransferLive>,
    app: AppHandle,
    id: i64,
}

impl LiveGuard {
    /// 更新进度（只增不减），按最小间隔发给界面。
    fn advance(&self, done: u64) {
        let emit = {
            let mut m = self.live.lock();
            let Some(l) = m.get_mut(&self.id) else { return };
            l.done = l.done.max(done);
            let due = l.last_emit.map_or(true, |t| t.elapsed() >= EMIT_EVERY);
            if due {
                l.last_emit = Some(Instant::now());
            }
            due.then_some((l.done, l.total))
        };
        if let Some((done, total)) = emit {
            let _ = self.app.emit(EVT_PROGRESS, serde_json::json!({"id": self.id, "done": done, "total": total}));
        }
    }
}

impl Drop for LiveGuard {
    fn drop(&mut self) {
        let mut m = self.live.lock();
        if let Some(l) = m.get_mut(&self.id) {
            l.streams = l.streams.saturating_sub(1);
            if l.streams == 0 {
                m.remove(&self.id);
                self.live.canceled.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
            }
        }
        self.live.active.fetch_sub(1, Ordering::SeqCst);
        let _ = self.app.emit(EVT_CHANGED, ());
    }
}

fn st(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}

fn live(app: &AppHandle) -> Arc<TransferLive> {
    st(app).phone.xfer.clone()
}

// ---------- 文件名和类型 ----------

const ARCHIVE_EXTS: &[&str] = &["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst"];
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7",
    "lpt8", "lpt9",
];
const MAX_NAME_BYTES: usize = 120;

pub fn ext_of(name: &str) -> String {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// 按扩展名归类：image / video / audio / archive / file。
pub fn kind_of(name: &str) -> &'static str {
    let ext = ext_of(name);
    if ARCHIVE_EXTS.contains(&ext.as_str()) {
        return "archive";
    }
    match crate::library::kind_of_ext(&ext) {
        Some("video") => "video",
        Some("audio") => "audio",
        Some("image") => "image",
        _ => "file",
    }
}

/// 手机 / 浏览器给的文件名只当“名字”用：去掉路径、控制字符和 Windows 不允许的字符，避开保留名，限制长度。
pub fn safe_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned = crate::naming::sanitize(base);
    let mut name = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if name.is_empty() {
        name = "file".into();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 12 => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let mut stem = stem;
    if RESERVED.contains(&stem.to_ascii_lowercase().as_str()) {
        stem.insert(0, '_');
    }
    // 按字节截断（保留扩展名），不要切在字符中间
    let room = MAX_NAME_BYTES.saturating_sub(ext.len()).max(8);
    if stem.len() > room {
        let mut end = room;
        while !stem.is_char_boundary(end) {
            end -= 1;
        }
        stem.truncate(end);
    }
    format!("{stem}{ext}")
}

fn mime_of(name: &str) -> &'static str {
    match ext_of(name).as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "zip" => "application/zip",
        "pdf" => "application/pdf",
        "txt" | "log" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 只允许把图片 / 音视频当作网页内容显示（不含 SVG，避免被当成网页脚本执行）。
fn inline_ok(mime: &str) -> bool {
    mime.starts_with("image/") && mime != "image/svg+xml" || mime.starts_with("video/") || mime.starts_with("audio/")
}

fn content_disposition(name: &str, inline: bool) -> String {
    let ascii: String = name.chars().map(|c| if c.is_ascii_graphic() && !matches!(c, '"' | '\\' | '%') || c == ' ' { c } else { '_' }).collect();
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{}; filename=\"{ascii}\"; filename*=UTF-8''{enc}", if inline { "inline" } else { "attachment" })
}

/// 接收文件夹：设置里指定的，否则是“下载目录/接收”。
pub fn receive_dir(settings: &crate::settings::Settings) -> PathBuf {
    let p = settings.phone.receive_dir.trim();
    if p.is_empty() {
        settings.download_root().join("接收")
    } else {
        PathBuf::from(p)
    }
}

/// 在 `dir` 里占一个不重名的文件名（先创建空文件占位，多个同时到达的同名文件不会互相覆盖）。
fn claim_path(dir: &Path, name: &str) -> std::io::Result<PathBuf> {
    let mut candidate = dir.join(name);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1..10_000 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => candidate = dir.join(format!("{stem} ({n}){ext}")),
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("重名文件太多"))
}

fn random_key() -> String {
    use aes_gcm::aead::rand_core::RngCore;
    let mut b = [0u8; 12];
    aes_gcm::aead::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

// ---------- 流式收发（和网络、界面无关，便于测试） ----------

/// 把 `leftover`（读请求头时多读到的正文开头）和 `reader` 里的数据共 `len` 字节写入 `out`。
/// 每写入一块调用一次 `on_chunk(已写入字节数)`，返回 false 表示被取消。读取超过 `idle` 没有数据按失败处理。
pub async fn copy_exact<R, W>(
    reader: &mut R,
    out: &mut W,
    leftover: &[u8],
    len: u64,
    idle: Duration,
    mut on_chunk: impl FnMut(u64) -> bool,
) -> Result<u64, String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut written = 0u64;
    let first = leftover.len().min(len as usize);
    if first > 0 {
        out.write_all(&leftover[..first]).await.map_err(|e| format!("写入失败：{e}"))?;
        written += first as u64;
        if !on_chunk(written) {
            return Err("已取消".into());
        }
    }
    let mut buf = vec![0u8; CHUNK];
    while written < len {
        let want = ((len - written) as usize).min(buf.len());
        let n = match tokio::time::timeout(idle, reader.read(&mut buf[..want])).await {
            Err(_) => return Err("手机长时间没有发送数据，已中断".into()),
            Ok(Err(e)) => return Err(format!("连接中断：{e}")),
            Ok(Ok(0)) => return Err("手机中途断开了连接".into()),
            Ok(Ok(n)) => n,
        };
        out.write_all(&buf[..n]).await.map_err(|e| format!("写入失败：{e}"))?;
        written += n as u64;
        if !on_chunk(written) {
            return Err("已取消".into());
        }
    }
    out.flush().await.map_err(|e| format!("写入失败：{e}"))?;
    Ok(written)
}

/// 把文件 `path` 的 `[start, end]`（含）字节按块发送到 `out`。`on_chunk(下一个字节的位置)` 返回 false 表示取消。
pub async fn send_range<W>(out: &mut W, path: &Path, start: u64, end: u64, idle: Duration, mut on_chunk: impl FnMut(u64) -> bool) -> Result<u64, String>
where
    W: AsyncWrite + Unpin,
{
    let mut f = tokio::fs::File::open(path).await.map_err(|e| format!("打开文件失败：{e}"))?;
    if start > 0 {
        f.seek(std::io::SeekFrom::Start(start)).await.map_err(|e| format!("读取失败：{e}"))?;
    }
    let mut remaining = end + 1 - start;
    let mut sent = 0u64;
    let mut buf = vec![0u8; CHUNK * 2];
    while remaining > 0 {
        let want = (remaining as usize).min(buf.len());
        let n = f.read(&mut buf[..want]).await.map_err(|e| format!("读取失败：{e}"))?;
        if n == 0 {
            return Err("文件在发送中被改动或删除".into());
        }
        match tokio::time::timeout(idle, out.write_all(&buf[..n])).await {
            Err(_) => return Err("手机长时间没有接收，已中断".into()),
            Ok(Err(e)) => return Err(format!("连接中断：{e}")),
            Ok(Ok(())) => {}
        }
        remaining -= n as u64;
        sent += n as u64;
        if !on_chunk(start + sent) {
            return Err("已取消".into());
        }
    }
    Ok(sent)
}

#[derive(Debug, PartialEq)]
pub enum RangeSpec {
    Full,
    Part(u64, u64),
    Unsatisfiable,
}

/// 解析 `Range: bytes=a-b | a- | -n`（只支持单个范围，多个范围当作整个文件）。
pub fn parse_range(header: Option<&str>, total: u64) -> RangeSpec {
    let Some(h) = header else { return RangeSpec::Full };
    let Some(spec) = h.trim().strip_prefix("bytes=") else { return RangeSpec::Full };
    if spec.contains(',') {
        return RangeSpec::Full;
    }
    let Some((a, b)) = spec.split_once('-') else { return RangeSpec::Full };
    let (a, b) = (a.trim(), b.trim());
    if total == 0 {
        return RangeSpec::Unsatisfiable;
    }
    let (start, end) = match (a.is_empty(), b.is_empty()) {
        (true, true) => return RangeSpec::Full,
        (true, false) => {
            let Ok(n) = b.parse::<u64>() else { return RangeSpec::Full };
            if n == 0 {
                return RangeSpec::Unsatisfiable;
            }
            (total.saturating_sub(n), total - 1)
        }
        (false, true) => {
            let Ok(s) = a.parse::<u64>() else { return RangeSpec::Full };
            (s, total - 1)
        }
        (false, false) => {
            let (Ok(s), Ok(e)) = (a.parse::<u64>(), b.parse::<u64>()) else { return RangeSpec::Full };
            (s, e.min(total - 1))
        }
    };
    if start >= total || start > end {
        RangeSpec::Unsatisfiable
    } else {
        RangeSpec::Part(start, end)
    }
}

// ---------- 解压 ----------

/// 把 zip 解压到新建的文件夹。路径穿越（`../`、绝对路径）和符号链接的条目会被跳过。返回 (文件夹, 文件数)。
pub fn extract_zip(zip_path: &Path, into: &Path) -> Result<(PathBuf, usize), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut ar = zip::ZipArchive::new(file).map_err(|e| format!("不是有效的 zip：{e}"))?;
    if ar.len() > MAX_ZIP_ENTRIES {
        return Err(format!("压缩包里的文件太多（超过 {MAX_ZIP_ENTRIES} 个）"));
    }
    let stem = zip_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "解压".into());
    let dest = claim_dir(into, &safe_name(&stem)).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    let mut files = 0usize;
    for i in 0..ar.len() {
        let mut e = ar.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = e.enclosed_name() else { continue };
        if e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            continue;
        }
        let mut out_path = dest.clone();
        for c in rel.components() {
            if let std::path::Component::Normal(p) = c {
                out_path.push(crate::naming::sanitize(&p.to_string_lossy()));
            }
        }
        if out_path == dest {
            continue;
        }
        if e.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        total = total.saturating_add(e.size());
        if total > MAX_ZIP_BYTES {
            return Err("解压后的总大小太大，已停止".into());
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
        // 实际写入的字节数也限制在声明的大小内，防止压缩炸弹
        let declared = e.size();
        let mut limited = (&mut e).take(declared.saturating_add(1));
        let n = std::io::copy(&mut limited, &mut out).map_err(|e| e.to_string())?;
        if n > declared {
            return Err("压缩包里的文件大小和声明的不一致，已停止".into());
        }
        files += 1;
    }
    Ok((dest, files))
}

fn claim_dir(parent: &Path, name: &str) -> std::io::Result<PathBuf> {
    for n in 0..10_000 {
        let candidate = if n == 0 { parent.join(name) } else { parent.join(format!("{name} ({n})")) };
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("重名文件夹太多"))
}

// ---------- 手机 → 电脑 ----------

async fn json(sock: &mut TcpStream, status: &str, v: serde_json::Value) {
    crate::phone::json_response(sock, status, v).await;
}

/// 在读完请求正文之前就拒绝：先发出回应，再把还在路上的正文丢弃一小段时间。
/// 直接关闭连接会让系统向手机发 RST，手机浏览器往往因此收不到回应，只看到“连接中断”。
async fn reject(sock: &mut TcpStream, status: &str, v: serde_json::Value) {
    json(sock, status, v).await;
    let _ = sock.shutdown().await;
    let mut buf = vec![0u8; 16 * 1024];
    let mut left = 8 * 1024 * 1024usize;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while left > 0 {
            match sock.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => left = left.saturating_sub(n),
            }
        }
    })
    .await;
}

fn clean_device(id: &str) -> String {
    crate::phone::clean_device_id(id)
}

/// 手机上传一个文件：`POST /api/upload?device=..&dname=..&name=..`，正文就是文件内容。
pub async fn handle_upload(app: &AppHandle, sock: &mut TcpStream, req: &Request, len: Option<u64>, leftover: Vec<u8>, peer: &SocketAddr) {
    let state = st(app);
    let settings = state.settings();
    let device = clean_device(req.query.get("device").map(String::as_str).unwrap_or(""));
    let dname: String = req.query.get("dname").map(|s| s.trim().chars().take(40).collect()).unwrap_or_default();
    if !crate::phone::ensure_paired(app, &device, &dname, peer) {
        reject(sock, "403 Forbidden", serde_json::json!({"state": "pending_pair", "error": "请先在电脑上点“允许”完成配对"})).await;
        return;
    }
    let Some(len) = len else {
        reject(sock, "411 Length Required", serde_json::json!({"error": "缺少文件大小"})).await;
        return;
    };
    let limit = settings.phone.max_upload_mb.saturating_mul(1024 * 1024);
    if limit > 0 && len > limit {
        json(
            sock,
            "413 Payload Too Large",
            serde_json::json!({"error": format!("文件超过上限 {} MB（可在电脑上的“设置 → 手机与浏览器扩展”里调整）", settings.phone.max_upload_mb)}),
        )
        .await;
        return;
    }
    let dir = receive_dir(&settings);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        reject(sock, "500 Internal Server Error", serde_json::json!({"error": format!("无法创建接收文件夹：{e}")})).await;
        return;
    }
    if let Err(e) = crate::engine::ensure_space(&dir, len, RESERVE) {
        reject(sock, "507 Insufficient Storage", serde_json::json!({"error": e.to_string()})).await;
        return;
    }
    let live = live(app);
    if !live.try_begin() {
        reject(sock, "503 Service Unavailable", serde_json::json!({"error": "电脑正在处理太多传输，请稍后再试"})).await;
        return;
    }
    let name = safe_name(req.query.get("name").map(String::as_str).unwrap_or("file"));
    let kind = kind_of(&name);
    let part = dir.join(format!(".{}.{}.part", random_key(), std::process::id()));
    let new = NewTransfer {
        direction: "in",
        kind,
        name: &name,
        path: &part.to_string_lossy(),
        text: "",
        size: len as i64,
        status: "receiving",
        key: "",
        device: &device,
        device_name: &dname,
        expires_at: 0,
    };
    let id = match state.db.transfer_add(&new) {
        Ok(id) => id,
        Err(e) => {
            live.active.fetch_sub(1, Ordering::SeqCst);
            reject(sock, "500 Internal Server Error", serde_json::json!({"error": e.message})).await;
            return;
        }
    };
    let guard = live.guard(app, id, len);
    let _ = app.emit(EVT_CHANGED, ());
    let result = async {
        let mut f = tokio::fs::File::create(&part).await.map_err(|e| format!("无法写入接收文件夹：{e}"))?;
        let live2 = live.clone();
        copy_exact(sock, &mut f, &leftover, len, IDLE, |n| {
            guard.advance(n);
            !live2.is_canceled(id)
        })
        .await
    }
    .await;
    match result {
        Ok(written) => {
            let finished = claim_path(&dir, &name).and_then(|p| std::fs::rename(&part, &p).map(|_| p));
            match finished {
                Ok(final_path) => {
                    let final_name = final_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| name.clone());
                    let _ = state.db.transfer_finish(id, "done", Some(&final_path.to_string_lossy()), written as i64, None);
                    json(sock, "200 OK", serde_json::json!({"id": id, "name": final_name, "size": written})).await;
                    after_received(app, id, &final_name, kind, &final_path, settings.phone.auto_extract);
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    let _ = state.db.transfer_finish(id, "failed", None, 0, Some(&format!("保存失败：{e}")));
                    json(sock, "500 Internal Server Error", serde_json::json!({"error": format!("保存失败：{e}")})).await;
                }
            }
        }
        Err(msg) => {
            let _ = std::fs::remove_file(&part);
            if live.is_canceled(id) {
                let _ = state.db.transfer_remove(id);
            } else {
                let _ = state.db.transfer_finish(id, "failed", None, 0, Some(&msg));
            }
            json(sock, "400 Bad Request", serde_json::json!({"error": msg})).await;
        }
    }
    drop(guard);
}

/// 文件收好之后：通知界面、系统通知（限频）、按设置解压 zip。
fn after_received(app: &AppHandle, id: i64, name: &str, kind: &str, path: &Path, auto_extract: bool) {
    let _ = app.emit(EVT_RECEIVED, serde_json::json!({"id": id, "name": name, "kind": kind}));
    let live = live(app);
    let notify = {
        let mut last = live.last_notify.lock().unwrap_or_else(|e| e.into_inner());
        let due = last.map_or(true, |t| t.elapsed() >= NOTIFY_EVERY);
        if due {
            *last = Some(Instant::now());
        }
        due
    };
    if notify {
        let _ = app.notification().builder().title("收到手机发来的文件").body(name).show();
    }
    if auto_extract && ext_of(name) == "zip" {
        let app2 = app.clone();
        let zip = path.to_path_buf();
        tauri::async_runtime::spawn(async move {
            let into = zip.parent().map(Path::to_path_buf).unwrap_or_default();
            let zip2 = zip.clone();
            let res = tokio::task::spawn_blocking(move || extract_zip(&zip2, &into)).await;
            let db = &st(&app2).db;
            match res {
                Ok(Ok((dir, n))) => {
                    let _ = db.transfer_set_extra(id, Some(&dir.to_string_lossy()), Some(&format!("已解压 {n} 个文件")));
                }
                Ok(Err(e)) => {
                    let _ = db.transfer_set_extra(id, None, Some(&format!("解压失败：{e}")));
                }
                Err(e) => {
                    let _ = db.transfer_set_extra(id, None, Some(&format!("解压失败：{e}")));
                }
            }
            let _ = app2.emit(EVT_CHANGED, ());
        });
    }
    let _ = app.emit(EVT_CHANGED, ());
}

/// 手机发来没有链接的文字：存为文字便签。
pub fn receive_note(app: &AppHandle, device: &str, device_name: &str, text: &str) -> AppResult<i64> {
    let text: String = text.trim().chars().take(MAX_NOTE).collect();
    if text.is_empty() {
        return Err(AppError::invalid("内容是空的。"));
    }
    let title: String = text.lines().next().unwrap_or("").chars().take(40).collect();
    let size = text.len() as i64;
    let id = st(app).db.transfer_add(&NewTransfer {
        direction: "in",
        kind: "text",
        name: &title,
        path: "",
        text: &text,
        size,
        status: "done",
        key: "",
        device,
        device_name,
        expires_at: 0,
    })?;
    after_received(app, id, &title, "text", Path::new(""), false);
    Ok(id)
}

// ---------- 电脑 → 手机 ----------

/// 电脑上添加要发给手机的文件。返回 (成功数, 被跳过的原因)。
pub fn send_files(app: &AppHandle, paths: &[String]) -> AppResult<(usize, Vec<String>)> {
    let state = st(app);
    let hours = state.settings().phone.outbox_hours;
    let expires = if hours == 0 { 0 } else { now() + hours as i64 * 3600 };
    let mut added = 0;
    let mut skipped = vec![];
    for p in paths.iter().take(500) {
        let path = PathBuf::from(p);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                skipped.push(format!("{name}：{e}"));
                continue;
            }
        };
        if meta.is_dir() {
            skipped.push(format!("{name}：文件夹请先压缩成压缩包再发送"));
            continue;
        }
        if !meta.is_file() {
            skipped.push(format!("{name}：不是普通文件"));
            continue;
        }
        let name = safe_name(&name);
        state.db.transfer_add(&NewTransfer {
            direction: "out",
            kind: kind_of(&name),
            name: &name,
            path: &path.to_string_lossy(),
            text: "",
            size: meta.len() as i64,
            status: "ready",
            key: &random_key(),
            device: "",
            device_name: "",
            expires_at: expires,
        })?;
        added += 1;
    }
    let _ = app.emit(EVT_CHANGED, ());
    Ok((added, skipped))
}

/// 电脑上输入一段文字发给手机（手机网页里可以一键复制）。
pub fn send_text(app: &AppHandle, text: &str) -> AppResult<i64> {
    let text: String = text.trim().chars().take(MAX_NOTE).collect();
    if text.is_empty() {
        return Err(AppError::invalid("请先输入要发送的文字。"));
    }
    let state = st(app);
    let hours = state.settings().phone.outbox_hours;
    let expires = if hours == 0 { 0 } else { now() + hours as i64 * 3600 };
    let title: String = text.lines().next().unwrap_or("").chars().take(40).collect();
    let id = state.db.transfer_add(&NewTransfer {
        direction: "out",
        kind: "text",
        name: &title,
        path: "",
        text: &text,
        size: text.len() as i64,
        status: "ready",
        key: &random_key(),
        device: "",
        device_name: "",
        expires_at: expires,
    })?;
    let _ = app.emit(EVT_CHANGED, ());
    Ok(id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferList {
    pub incoming: Vec<TransferItem>,
    pub outgoing: Vec<TransferItem>,
    pub receive_dir: String,
}

fn merge_live(app: &AppHandle, mut it: TransferItem) -> TransferItem {
    if let Some((done, _, streams)) = live(app).snapshot(it.id) {
        it.done = done as i64;
        it.active = streams > 0;
    }
    if !it.path.is_empty() && it.status != "receiving" {
        it.missing = !Path::new(&it.path).is_file();
    }
    it
}

pub fn list(app: &AppHandle) -> AppResult<TransferList> {
    let state = st(app);
    let _ = state.db.transfer_expire(now());
    let incoming = state.db.transfer_list("in", 200)?.into_iter().map(|i| merge_live(app, i)).collect();
    let outgoing = state.db.transfer_list("out", 200)?.into_iter().map(|i| merge_live(app, i)).collect();
    Ok(TransferList { incoming, outgoing, receive_dir: receive_dir(&state.settings()).to_string_lossy().into_owned() })
}

/// 删除一条记录。接收中的会中断；`delete_file` 为真时同时删除收到的文件（只对“手机 → 电脑”有效，
/// 电脑上被发送的原文件永远不会被删除）。
pub fn remove(app: &AppHandle, id: i64, delete_file: bool) -> AppResult<()> {
    let state = st(app);
    let Some(it) = state.db.transfer_get(id)? else { return Ok(()) };
    if live(app).snapshot(id).is_some() {
        live(app).canceled.lock().unwrap_or_else(|e| e.into_inner()).insert(id);
    }
    if delete_file && it.direction == "in" && !it.path.is_empty() && it.status == "done" {
        let _ = std::fs::remove_file(&it.path);
    }
    state.db.transfer_remove(id)?;
    let _ = app.emit(EVT_CHANGED, ());
    Ok(())
}

/// 清掉已经结束的记录（不删文件）。
pub fn clear(app: &AppHandle, direction: &str) -> AppResult<()> {
    let state = st(app);
    for it in state.db.transfer_list(direction, 1000)? {
        if live(app).snapshot(it.id).is_none() && it.status != "receiving" {
            state.db.transfer_remove(it.id)?;
        }
    }
    let _ = app.emit(EVT_CHANGED, ());
    Ok(())
}

/// 启动时清理：撤回过期项，清理上次中断留下的 .part 文件。
pub fn restore(app: &AppHandle) {
    let state = st(app);
    let _ = state.db.transfer_expire(now());
    if let Ok(parts) = state.db.transfer_fail_stale() {
        for p in parts.into_iter().filter(|p| p.ends_with(".part")) {
            let _ = std::fs::remove_file(p);
        }
    }
}

// ---------- 手机网页用的接口 ----------

fn out_json(app: &AppHandle, it: &TransferItem) -> serde_json::Value {
    let live = live(app).snapshot(it.id);
    serde_json::json!({
        "key": it.key,
        "name": it.name,
        "kind": it.kind,
        "size": it.size,
        "text": if it.kind == "text" { it.text.as_str() } else { "" },
        "sent": live.map(|l| l.0).unwrap_or(0),
        "active": live.is_some_and(|l| l.2 > 0),
        "downloads": it.downloads,
        "missing": !it.path.is_empty() && !Path::new(&it.path).is_file(),
        "at": it.created_at,
    })
}

/// `GET /api/box?device=`：电脑发给手机的项目，和这台手机最近上传的文件。
pub fn box_json(app: &AppHandle, device: &str) -> serde_json::Value {
    let state = st(app);
    let _ = state.db.transfer_expire(now());
    let out: Vec<_> =
        state.db.transfer_for_device("out", device, 100).unwrap_or_default().iter().filter(|i| !i.key.is_empty()).map(|i| out_json(app, i)).collect();
    let inc: Vec<_> = state
        .db
        .transfer_for_device("in", device, 20)
        .unwrap_or_default()
        .into_iter()
        .map(|i| {
            serde_json::json!({
                "id": i.id, "name": i.name, "kind": i.kind, "size": i.size, "state": i.status, "message": i.message, "at": i.created_at,
            })
        })
        .collect();
    serde_json::json!({"out": out, "in": inc, "maxMb": state.settings().phone.max_upload_mb})
}

/// `GET /dl/<密钥>?t=令牌&d=设备`：把电脑发给手机的文件发出去。
pub async fn handle_download(app: &AppHandle, sock: &mut TcpStream, req: &Request, peer: &SocketAddr) {
    let state = st(app);
    let device = clean_device(req.query.get("d").map(String::as_str).unwrap_or(""));
    if !state.settings().phone.devices.iter().any(|d| d.id == device) {
        let _ = peer;
        json(sock, "403 Forbidden", serde_json::json!({"error": "这台设备还没有配对，请回到发送页面等电脑确认"})).await;
        return;
    }
    let key = req.path.trim_start_matches("/dl/");
    let _ = state.db.transfer_expire(now());
    let item = match state.db.transfer_by_key(key) {
        Ok(Some(i)) => i,
        _ => {
            json(sock, "404 Not Found", serde_json::json!({"error": "这个文件已经被撤回或过期了"})).await;
            return;
        }
    };
    let inline = req.query.get("inline").is_some_and(|v| v == "1");
    // 文字：直接作为文本下载
    if item.kind == "text" {
        let body = item.text.as_bytes();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nContent-Disposition: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",
            body.len(),
            content_disposition(&format!("{}.txt", safe_name(&item.name)), false)
        );
        let _ = sock.write_all(head.as_bytes()).await;
        if req.method != "HEAD" {
            let _ = sock.write_all(body).await;
        }
        return;
    }
    let path = PathBuf::from(&item.path);
    let total = match tokio::fs::metadata(&path).await {
        Ok(m) if m.is_file() => m.len(),
        _ => {
            json(sock, "404 Not Found", serde_json::json!({"error": "电脑上的这个文件已经被移动或删除了"})).await;
            return;
        }
    };
    let (status, start, end) = match parse_range(req.headers.get("range").map(String::as_str), total) {
        RangeSpec::Full => ("200 OK", 0, total.saturating_sub(1)),
        RangeSpec::Part(s, e) => ("206 Partial Content", s, e),
        RangeSpec::Unsatisfiable => {
            let head = format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = sock.write_all(head.as_bytes()).await;
            return;
        }
    };
    let mime = mime_of(&item.name);
    let inline = inline && inline_ok(mime);
    let body_len = if total == 0 { 0 } else { end + 1 - start };
    let streaming = req.method != "HEAD" && total > 0;
    let live = live(app);
    if streaming && !live.try_begin() {
        json(sock, "503 Service Unavailable", serde_json::json!({"error": "电脑正在处理太多传输，请稍后再试"})).await;
        return;
    }
    // 占位：离开作用域时自动登记结束（没有真正开始传输时不占）
    let guard = streaming.then(|| live.guard(app, item.id, total));
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {body_len}\r\nAccept-Ranges: bytes\r\nContent-Disposition: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n",
        content_disposition(&item.name, inline)
    );
    if status.starts_with("206") {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
    }
    head.push_str("\r\n");
    if sock.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    let Some(guard) = guard else { return };
    let live2 = live.clone();
    let id = item.id;
    let res = send_range(sock, &path, start, end, IDLE, |pos| {
        guard.advance(pos);
        !live2.is_canceled(id)
    })
    .await;
    if res.is_ok() && end + 1 == total {
        let _ = state.db.transfer_count_download(item.id);
    }
    drop(guard);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\Windows\\system32\\evil.exe"), "evil.exe");
        assert_eq!(safe_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(safe_name("  ..hidden. "), "hidden");
        assert_eq!(safe_name(""), "file");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
        assert_eq!(safe_name("nul"), "_nul");
        assert_eq!(safe_name("IMG_0001.HEIC"), "IMG_0001.HEIC");
        let long = format!("{}.mp4", "视".repeat(100));
        let n = safe_name(&long);
        assert!(n.len() <= MAX_NAME_BYTES + 4 && n.ends_with(".mp4"), "{n}");
        assert!(n.is_char_boundary(n.len()));
    }

    #[test]
    fn kinds_follow_the_extension() {
        assert_eq!(kind_of("a.MP4"), "video");
        assert_eq!(kind_of("a.jpg"), "image");
        assert_eq!(kind_of("a.m4a"), "audio");
        assert_eq!(kind_of("a.zip"), "archive");
        assert_eq!(kind_of("a.tar"), "archive");
        assert_eq!(kind_of("a.docx"), "file");
        assert_eq!(kind_of("noext"), "file");
    }

    #[test]
    fn content_disposition_is_safe_for_odd_names() {
        let h = content_disposition("视频 \"a\"\r\nX.mp4", false);
        assert!(!h.contains('\r') && !h.contains('\n'), "{h}");
        assert!(h.starts_with("attachment; filename=\""));
        assert!(h.contains("filename*=UTF-8''%E8%A7%86%E9%A2%91%20%22a%22%0D%0AX.mp4"), "{h}");
        assert!(inline_ok("image/png") && inline_ok("video/mp4") && !inline_ok("image/svg+xml") && !inline_ok("text/html") && !inline_ok("application/zip"));
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("bytes=0-9"), 100), RangeSpec::Part(0, 9));
        assert_eq!(parse_range(Some("bytes=10-"), 100), RangeSpec::Part(10, 99));
        assert_eq!(parse_range(Some("bytes=-20"), 100), RangeSpec::Part(80, 99));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), RangeSpec::Part(90, 99));
        assert_eq!(parse_range(Some("bytes=100-"), 100), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=5-2"), 100), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-0"), 0), RangeSpec::Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-4,10-20"), 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("items=0-4"), 100), RangeSpec::Full);
        assert_eq!(parse_range(Some("bytes=abc-"), 100), RangeSpec::Full);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-xfer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn claim_path_never_overwrites() {
        let d = tmp("claim");
        let a = claim_path(&d, "a.jpg").unwrap();
        let b = claim_path(&d, "a.jpg").unwrap();
        let c = claim_path(&d, "noext").unwrap();
        let c2 = claim_path(&d, "noext").unwrap();
        assert_eq!(a.file_name().unwrap(), "a.jpg");
        assert_eq!(b.file_name().unwrap(), "a (1).jpg");
        assert_eq!(c2.file_name().unwrap(), "noext (1)");
        assert!(c.exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn receiving_streams_leftover_and_body_to_disk() {
        let d = tmp("recv");
        let data: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let payload = data.clone();
        let writer = tokio::spawn(async move {
            // 前 100 字节假装是读请求头时多读到的，其余从连接里来
            client.write_all(&payload[100..]).await.unwrap();
        });
        let mut f = tokio::fs::File::create(d.join("x.bin")).await.unwrap();
        let mut ticks = 0u64;
        let n = copy_exact(&mut server, &mut f, &data[..100], data.len() as u64, Duration::from_secs(5), |w| {
            ticks = w;
            true
        })
        .await
        .unwrap();
        writer.await.unwrap();
        drop(f);
        assert_eq!(n, data.len() as u64);
        assert_eq!(ticks, n);
        assert_eq!(std::fs::read(d.join("x.bin")).unwrap(), data);
        let _ = std::fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn receiving_reports_a_dropped_connection_and_cancel() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_all(&[1u8; 10]).await.unwrap();
        drop(client);
        let mut sink = tokio::io::sink();
        let err = copy_exact(&mut server, &mut sink, &[], 100, Duration::from_secs(2), |_| true).await.unwrap_err();
        assert!(err.contains("断开"), "{err}");

        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_all(&[1u8; 600]).await.unwrap();
        let mut sink = tokio::io::sink();
        let err = copy_exact(&mut server, &mut sink, &[], 1000, Duration::from_secs(2), |w| w < 500).await.unwrap_err();
        assert_eq!(err, "已取消");
    }

    #[tokio::test]
    async fn receiving_times_out_when_the_phone_goes_quiet() {
        let (_client, mut server) = tokio::io::duplex(1024);
        let mut sink = tokio::io::sink();
        let err = copy_exact(&mut server, &mut sink, &[], 10, Duration::from_millis(100), |_| true).await.unwrap_err();
        assert!(err.contains("长时间"), "{err}");
    }

    #[tokio::test]
    async fn sending_a_range_reads_only_that_part() {
        let d = tmp("send");
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 253) as u8).collect();
        std::fs::write(d.join("f.bin"), &data).unwrap();
        let mut out: Vec<u8> = vec![];
        let mut last = 0;
        let n = send_range(&mut out, &d.join("f.bin"), 1000, 600_000, Duration::from_secs(5), |p| {
            last = p;
            true
        })
        .await
        .unwrap();
        assert_eq!(n, 599_001);
        assert_eq!(last, 600_001);
        assert_eq!(out, data[1000..=600_000]);
        let mut all: Vec<u8> = vec![];
        send_range(&mut all, &d.join("f.bin"), 0, data.len() as u64 - 1, Duration::from_secs(5), |_| true).await.unwrap();
        assert_eq!(all, data);
        // 发送中文件变短：报错而不是挂住
        std::fs::write(d.join("short.bin"), [1u8; 10]).unwrap();
        let mut o2: Vec<u8> = vec![];
        let err = send_range(&mut o2, &d.join("short.bin"), 0, 99, Duration::from_secs(1), |_| true).await.unwrap_err();
        assert!(err.contains("改动"), "{err}");
        let _ = std::fs::remove_dir_all(d);
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        use std::io::Write as _;
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file(*name, opts).unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn extraction_skips_path_traversal() {
        let d = tmp("zip");
        let z = d.join("pack.zip");
        make_zip(&z, &[("ok/a.txt", b"hello"), ("../escape.txt", b"bad"), ("/abs.txt", b"bad"), ("b.txt", b"b")]);
        let (dest, n) = extract_zip(&z, &d).unwrap();
        assert_eq!(n, 2);
        assert_eq!(dest.file_name().unwrap(), "pack");
        assert_eq!(std::fs::read(dest.join("ok").join("a.txt")).unwrap(), b"hello");
        assert!(!d.join("escape.txt").exists() && !d.parent().unwrap().join("escape.txt").exists());
        assert!(!dest.join("abs.txt").exists());
        // 再解一次：放进新的同名文件夹，不覆盖
        let (dest2, _) = extract_zip(&z, &d).unwrap();
        assert_eq!(dest2.file_name().unwrap(), "pack (1)");
        // 不是 zip
        std::fs::write(d.join("fake.zip"), b"not a zip").unwrap();
        assert!(extract_zip(&d.join("fake.zip"), &d).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn database_round_trip_and_expiry() {
        let db = Db::open_in_memory().unwrap();
        fn mk<'a>(dir: &'a str, key: &'a str, expires: i64) -> NewTransfer<'a> {
            NewTransfer {
                direction: dir,
                kind: "file",
                name: "a.bin",
                path: "/tmp/a.bin",
                text: "",
                size: 5,
                status: if dir == "out" { "ready" } else { "receiving" },
                key,
                device: "d1",
                device_name: "手机",
                expires_at: expires,
            }
        }
        let out_old = db.transfer_add(&mk("out", "k1", now() - 10)).unwrap();
        let out_new = db.transfer_add(&mk("out", "k2", now() + 1000)).unwrap();
        let out_forever = db.transfer_add(&mk("out", "k3", 0)).unwrap();
        let inc = db.transfer_add(&mk("in", "", 0)).unwrap();
        assert_eq!(db.transfer_expire(now()).unwrap(), 1);
        assert!(db.transfer_get(out_old).unwrap().is_none());
        assert!(db.transfer_by_key("k2").unwrap().is_some());
        assert!(db.transfer_by_key("").unwrap().is_none());
        assert!(db.transfer_by_key("nope").unwrap().is_none());
        db.transfer_count_download(out_new).unwrap();
        assert_eq!(db.transfer_get(out_new).unwrap().unwrap().downloads, 1);
        assert_eq!(db.transfer_list("out", 10).unwrap().len(), 2);
        assert!(db.transfer_get(out_forever).unwrap().is_some());
        // 同一台手机只看到自己上传的，但能看到电脑发出的全部
        assert_eq!(db.transfer_for_device("in", "d1", 10).unwrap().len(), 1);
        assert_eq!(db.transfer_for_device("in", "other", 10).unwrap().len(), 0);
        assert_eq!(db.transfer_for_device("out", "other", 10).unwrap().len(), 2);
        // 上次中断留下的“接收中”变成失败
        let parts = db.transfer_fail_stale().unwrap();
        assert_eq!(parts, vec!["/tmp/a.bin".to_string()]);
        assert_eq!(db.transfer_get(inc).unwrap().unwrap().status, "failed");
        db.transfer_finish(inc, "done", Some("/x/y.bin"), 5, None).unwrap();
        let it = db.transfer_get(inc).unwrap().unwrap();
        assert_eq!((it.status.as_str(), it.path.as_str(), it.done), ("done", "/x/y.bin", 5));
        db.transfer_remove(inc).unwrap();
        assert!(db.transfer_get(inc).unwrap().is_none());
    }
}
