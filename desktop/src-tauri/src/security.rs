//! 安全与隐私：应用锁、老板键、隐私模式的清理、一键清除。
//!
//! 应用锁挡的是“坐在屏幕前的人”：锁定后界面被遮住，托盘迷你窗不再显示任务，系统通知不显示标题，
//! 保险箱同时上锁。它不是加密——下载仍在后台继续，手机推送和命令行也照常工作；
//! 要保护文件本身请放进加密保险箱（见 `safebox.rs`）。
//! 密码只保存 Argon2id 的校验值，放在加密的密钥保险箱里（`vault.bin`），不进设置，也不进备份。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::safebox::{derive, random_bytes, Kdf};
use crate::AppState;

pub const APPLOCK: &str = "applock.hash";
pub const EVT_LOCK: &str = "security://lock";
pub const EVT_SAFEBOX: &str = "security://safebox";
pub const MIN_PIN: usize = 4;
/// 保险箱里解密出来临时查看的文件放在这个文件夹
pub const VIEW_DIR: &str = "safebox-view";

/// 输错密码的限速：连续错 5 次后开始等待，30 秒起每次翻倍，最长 15 分钟。
#[derive(Default)]
pub struct Throttle {
    inner: Mutex<(u32, Option<Instant>)>,
}

impl Throttle {
    pub fn remaining_secs(&self) -> u64 {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.1.map(|t| t.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64).unwrap_or(0)
    }

    pub fn check(&self) -> AppResult<()> {
        match self.remaining_secs() {
            0 => Ok(()),
            n => Err(AppError::invalid(format!("输错次数太多，请 {n} 秒后再试。"))),
        }
    }

    pub fn fail(&self) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.0 += 1;
        if g.0 >= 5 {
            let secs = (30u64 << (g.0 - 5).min(5)).min(900);
            g.1 = Some(Instant::now() + std::time::Duration::from_secs(secs));
        }
    }

    pub fn ok(&self) {
        *self.inner.lock().unwrap_or_else(|e| e.into_inner()) = (0, None);
    }
}

pub struct LockState {
    locked: AtomicBool,
    pub throttle: Throttle,
}

impl LockState {
    pub fn new(locked: bool) -> LockState {
        LockState { locked: AtomicBool::new(locked), throttle: Throttle::default() }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    pub fn set(&self, v: bool) {
        self.locked.store(v, Ordering::SeqCst);
    }
}

// ---------- 密码校验值 ----------

/// `v1$内存$轮数$并行$盐$哈希`
pub fn make_hash(password: &str, kdf: Kdf) -> AppResult<String> {
    let salt: [u8; 16] = random_bytes();
    let h = derive(password, &salt, kdf)?;
    Ok(format!("v1${}${}${}${}${}", kdf.m_kib, kdf.t, kdf.p, hex::encode(salt), hex::encode(h)))
}

pub fn check_hash(password: &str, stored: &str) -> AppResult<bool> {
    let parts: Vec<&str> = stored.split('$').collect();
    let bad = || AppError::msg("应用锁的密码记录已损坏，请在设置里重新设置。");
    if parts.len() != 6 || parts[0] != "v1" {
        return Err(bad());
    }
    let kdf = Kdf { m_kib: parts[1].parse().map_err(|_| bad())?, t: parts[2].parse().map_err(|_| bad())?, p: parts[3].parse().map_err(|_| bad())? };
    let salt = hex::decode(parts[4]).map_err(|_| bad())?;
    let want = hex::decode(parts[5]).map_err(|_| bad())?;
    let got = derive(password, &salt, kdf)?;
    // 逐字节比较，不因第一个不同就提前返回
    Ok(want.len() == got.len() && want.iter().zip(got.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0)
}

pub fn lock_enabled(st: &AppState) -> bool {
    st.vault.has(APPLOCK)
}

// ---------- 锁定 / 解锁 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockStatus {
    pub enabled: bool,
    pub locked: bool,
    pub locked_out_secs: u64,
}

pub fn status(st: &AppState) -> LockStatus {
    let enabled = lock_enabled(st);
    LockStatus { enabled, locked: enabled && st.lock.is_locked(), locked_out_secs: st.lock.throttle.remaining_secs() }
}

pub fn view_dir(st: &AppState) -> std::path::PathBuf {
    st.data_dir.join(VIEW_DIR)
}

/// 清掉保险箱解密出来的临时文件，并让保险箱上锁。
pub fn seal_safebox(st: &AppState) {
    st.safebox.lock();
    crate::safebox::shred_dir(&view_dir(st));
}

/// 立即锁定。没有设置密码时什么也不做，返回 false。
pub fn lock_now(app: &AppHandle) -> bool {
    let st = app.state::<Arc<AppState>>();
    if !lock_enabled(&st) {
        return false;
    }
    st.lock.set(true);
    st.media.clear();
    seal_safebox(&st);
    crate::tray::close_small_windows(app);
    let _ = app.emit(EVT_LOCK, serde_json::json!({ "locked": true }));
    true
}

pub fn unlock(app: &AppHandle, st: &AppState, password: &str) -> AppResult<()> {
    let Some(stored) = st.vault.get(APPLOCK) else {
        st.lock.set(false);
        return Ok(());
    };
    st.lock.throttle.check()?;
    if !check_hash(password, &stored)? {
        st.lock.throttle.fail();
        return Err(AppError::invalid(match st.lock.throttle.remaining_secs() {
            0 => "密码不对。".to_string(),
            n => format!("密码不对，输错次数太多，请 {n} 秒后再试。"),
        }));
    }
    st.lock.throttle.ok();
    st.lock.set(false);
    crate::tray::apply_float(app, st.settings_raw().float_ball);
    let _ = app.emit(EVT_LOCK, serde_json::json!({ "locked": false }));
    Ok(())
}

pub fn set_password(st: &AppState, current: Option<&str>, new: &str, kdf: Kdf) -> AppResult<()> {
    if new.chars().count() < MIN_PIN {
        return Err(AppError::invalid(format!("密码至少 {MIN_PIN} 个字符。")));
    }
    verify_current(st, current)?;
    st.vault.set(APPLOCK, &make_hash(new, kdf)?)?;
    Ok(())
}

pub fn remove_password(app: &AppHandle, st: &AppState, current: Option<&str>) -> AppResult<()> {
    verify_current(st, current)?;
    st.vault.remove(APPLOCK)?;
    st.lock.set(false);
    let _ = app.emit(EVT_LOCK, serde_json::json!({ "locked": false }));
    Ok(())
}

/// 已经设置了密码时，修改或删除前要先验证旧密码。
fn verify_current(st: &AppState, current: Option<&str>) -> AppResult<()> {
    let Some(stored) = st.vault.get(APPLOCK) else { return Ok(()) };
    st.lock.throttle.check()?;
    if check_hash(current.unwrap_or(""), &stored)? {
        st.lock.throttle.ok();
        Ok(())
    } else {
        st.lock.throttle.fail();
        Err(AppError::invalid("当前密码不对。"))
    }
}

/// 老板键：立刻隐藏所有窗口，设置了应用锁时同时锁定。
pub fn panic(app: &AppHandle) {
    for (_, w) in app.webview_windows() {
        let _ = w.hide();
    }
    if !lock_now(app) {
        let st = app.state::<Arc<AppState>>();
        seal_safebox(&st);
    }
}

/// 窗口被收进托盘时按设置自动锁定。
pub fn on_hide(app: &AppHandle) {
    let st = app.state::<Arc<AppState>>();
    if st.settings().security.lock_on_hide {
        lock_now(app);
    }
}

pub fn apply_window_protection(app: &AppHandle, on: bool) {
    for (_, w) in app.webview_windows() {
        let _ = w.set_content_protected(on);
    }
}

/// 弹系统通知。隐私模式或已锁定时不显示内容：`title_safe` 为真（标题本身不含作品名、主播名）时标题照常显示。
pub fn system_notification(app: &AppHandle, title: &str, body: &str, title_safe: bool) {
    use crate::i18n::tr;
    use tauri_plugin_notification::NotificationExt;
    let st = app.state::<Arc<AppState>>();
    let settings = st.settings_raw();
    let en = crate::i18n::is_en(&settings.language);
    let hide = settings.security.privacy_mode || (lock_enabled(&st) && st.lock.is_locked());
    let (t, b) = match (hide, title_safe) {
        (false, _) => (tr(en, title), body.to_string()),
        (true, true) => (tr(en, title), tr(en, "详情请打开清影查看")),
        (true, false) => (tr(en, "清影"), tr(en, "有新消息")),
    };
    let _ = app.notification().builder().title(t).body(b).show();
}

// ---------- 保险箱空闲自动上锁 ----------

pub fn spawn_idle_watch(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            let st = app.state::<Arc<AppState>>();
            let mins = st.settings().security.safebox_auto_lock_minutes;
            if mins > 0 && st.safebox.is_unlocked() && st.safebox.idle_secs() >= u64::from(mins) * 60 {
                seal_safebox(&st);
                let _ = app.emit(EVT_SAFEBOX, ());
            }
        }
    });
}

// ---------- 一键清除 ----------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WipeOptions {
    /// 下载任务（进行中的会被取消）
    pub tasks: bool,
    /// 解析历史
    pub history: bool,
    /// 媒体库记录、标签、字幕索引、回收站记录（不删文件）
    pub library: bool,
    /// 收件箱
    pub inbox: bool,
    /// 订阅、直播间列表和录制记录
    pub subscriptions: bool,
    /// 保存的登录 Cookie 和登录窗口里的浏览数据
    pub cookies: bool,
    /// API 密钥、通知令牌等（应用锁密码保留）
    pub secrets: bool,
    /// 封面、预览图缓存
    pub thumbnails: bool,
    /// 日志和诊断样本
    pub logs: bool,
    /// 加密保险箱里的文件（无法恢复）
    pub safebox: bool,
    /// 清除后退出程序
    pub quit: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WipeReport {
    pub done: Vec<String>,
    pub skipped: Vec<String>,
}

/// 删除目录里的所有文件，目录本身保留。
pub fn wipe_dir_contents(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                n += wipe_dir_contents(&p);
                let _ = std::fs::remove_dir(&p);
            } else if std::fs::remove_file(&p).is_ok() {
                n += 1;
            } else if std::fs::File::create(&p).is_ok() {
                // 正在被日志模块占用的文件：至少清空内容
                n += 1;
            }
        }
    }
    n
}

pub fn wipe(app: &AppHandle, o: &WipeOptions) -> WipeReport {
    let st = app.state::<Arc<AppState>>().inner().clone();
    let mut rep = WipeReport::default();
    if o.tasks {
        for s in st.downloads.snapshots() {
            crate::download::remove(app, s.id);
        }
        st.media_jobs.clear_finished();
        let _ = app.emit(crate::media_tools::EVT_JOBS, st.media_jobs.snapshot());
        rep.done.push("下载任务".into());
    }
    let db_part = crate::db::DbWipe { tasks: o.tasks, history: o.history, library: o.library, inbox: o.inbox, subscriptions: o.subscriptions };
    if o.tasks || o.history || o.library || o.inbox || o.subscriptions {
        match st.db.wipe(&db_part) {
            Ok(skipped) => {
                for (flag, name) in [(o.history, "解析历史"), (o.library, "媒体库记录"), (o.inbox, "收件箱"), (o.subscriptions, "订阅与直播间")]
                {
                    if flag {
                        rep.done.push(name.into());
                    }
                }
                rep.skipped.extend(skipped);
            }
            Err(e) => rep.skipped.push(format!("数据库清理失败：{e}")),
        }
    }
    if o.cookies {
        match st.cookies.clear_all() {
            Ok(()) => rep.done.push("登录 Cookie".into()),
            Err(e) => rep.skipped.push(format!("Cookie 清理失败：{e}")),
        }
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.clear_all_browsing_data();
        }
        for (label, w) in app.webview_windows() {
            if label.starts_with("login-") {
                let _ = w.close();
            }
        }
    }
    if o.secrets {
        match st.vault.clear_except(&["applock."]) {
            Ok(()) => rep.done.push("API 密钥和通知令牌".into()),
            Err(e) => rep.skipped.push(format!("密钥清理失败：{e}")),
        }
    }
    if o.thumbnails {
        for d in ["covers", "previews"] {
            wipe_dir_contents(&st.data_dir.join(d));
        }
        rep.done.push("封面和预览图缓存".into());
    }
    if o.logs {
        wipe_dir_contents(&st.log_dir);
        wipe_dir_contents(&st.samples_dir);
        rep.done.push("日志和诊断样本".into());
    }
    if o.safebox {
        st.safebox.destroy();
        crate::safebox::shred_dir(&view_dir(&st));
        let _ = app.emit(EVT_SAFEBOX, ());
        rep.done.push("加密保险箱".into());
    } else {
        seal_safebox(&st);
    }
    let _ = app.emit(crate::download::EVT_TASKS, st.downloads.snapshots());
    let _ = app.emit(crate::inbox::EVT_INBOX, ());
    let _ = app.emit("library://changed", ());
    let _ = app.emit(crate::subs::EVT_SUBS, ());
    let _ = app.emit(crate::live::EVT_LIVE, ());
    let _ = app.emit("accounts://updated", ());
    let _ = st.db.checkpoint();
    if o.quit {
        let h = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            h.exit(0);
        });
    }
    rep
}

/// 隐私模式：退出时（以及启动时，防止上次异常退出）清掉不该留下的记录。
pub fn purge_private(db: &crate::db::Db) {
    if let Err(e) = db.purge_private() {
        log::warn!("privacy purge failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_roundtrip() {
        let h = make_hash("1234 abcd", Kdf::LIGHT).unwrap();
        assert!(h.starts_with("v1$"));
        assert!(!h.contains("1234"));
        assert!(check_hash("1234 abcd", &h).unwrap());
        assert!(!check_hash("1234 abce", &h).unwrap());
        assert!(!check_hash("", &h).unwrap());
        // 每次的盐不同
        assert_ne!(h, make_hash("1234 abcd", Kdf::LIGHT).unwrap());
    }

    #[test]
    fn broken_hash_records_are_errors_not_panics() {
        for bad in ["", "v1$1$2$3", "v2$64$1$1$00$00", "v1$x$1$1$00$00", "v1$64$1$1$zz$00", "v1$99999999$1$1$00$00"] {
            assert!(check_hash("pw", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn throttle_starts_after_five_failures_and_resets() {
        let t = Throttle::default();
        for _ in 0..4 {
            t.fail();
            assert!(t.check().is_ok());
        }
        t.fail();
        let r = t.remaining_secs();
        assert!((29..=30).contains(&r), "{r}");
        assert!(t.check().unwrap_err().message.contains("秒"));
        t.fail();
        assert!(t.remaining_secs() > 55, "doubles");
        t.ok();
        assert!(t.check().is_ok());
    }

    #[test]
    fn wipe_dir_contents_keeps_the_directory() {
        let d = std::env::temp_dir().join(format!("clearclip-wipe-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.jpg"), b"1").unwrap();
        std::fs::write(d.join("sub").join("b.jpg"), b"2").unwrap();
        assert_eq!(wipe_dir_contents(&d), 2);
        assert!(d.is_dir());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(d);
    }
}
