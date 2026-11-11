//! 窗口组件（Windows 上的 WebView2）断开时的处理。
//!
//! 系统在后台更新 WebView2 运行时、或它的浏览器进程被结束以后，已经在跑的程序再新开任何窗口都会失败，
//! 错误是 0x80010108（“被调用的对象已与其客户端断开连接”）；已经开着的窗口还能用，只有重启才能恢复。
//! 屏幕翻译的取景框和贴字层、写作浮标、悬浮窗都要新开窗口，所以会一起“没反应”。
//! 这里负责：认出这个错误、不再刷屏、告诉界面弹出“重启清影”的提示、屏幕翻译开始前给出明确的说法。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

/// 界面上看到的说明（也用作屏幕翻译等功能开始失败时的报错）。
pub const HINT: &str = "清影的窗口组件（WebView2）已经断开，新窗口打不开，所以屏幕翻译、写作浮标这类要新开窗口的功能暂时用不了。最常见的原因是系统刚更新了 WebView2 运行时。完全退出清影（托盘图标右键 → 退出）再重新打开就能恢复；也可以点设置里的“重启清影”。";

/// 事件名：主窗口收到后弹出“重启清影”的提示。
pub const EVT_BROKEN: &str = "webview://broken";

static BROKEN: AtomicBool = AtomicBool::new(false);
static LAST_FAIL: Mutex<Option<Instant>> = Mutex::new(None);
static LAST_NOTE: Mutex<Option<Instant>> = Mutex::new(None);

/// 日志和提示最多这么久出现一次，免得后台每隔几秒重试就刷屏。
const QUIET: Duration = Duration::from_secs(600);
/// 断开以后，后台自动开窗口的功能（写作浮标）这么久才再试一次。
const RETRY_AFTER: Duration = Duration::from_secs(300);

/// 错误文字是不是“窗口组件断开”。
pub fn is_disconnected(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("0x80010108") || m.contains("rpc_e_disconnected") || msg.contains("已与其客户端断开连接")
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 开窗口失败时调用。是“窗口组件断开”就记下来、限频地写日志并通知界面，返回 true（调用方不用再自己写日志）；
/// 不是就返回 false，由调用方按原来的方式处理。
pub fn note_failure(app: &AppHandle, what: &str, err: &dyn std::fmt::Display) -> bool {
    if !is_disconnected(&err.to_string()) {
        return false;
    }
    BROKEN.store(true, Ordering::SeqCst);
    let now = Instant::now();
    *lock(&LAST_FAIL) = Some(now);
    let mut last = lock(&LAST_NOTE);
    if last.map_or(true, |t| now.duration_since(t) >= QUIET) {
        *last = Some(now);
        log::warn!("窗口组件（WebView2）已断开，{what}打不开：{err}。需要重启清影才能恢复");
        let _ = app.emit(EVT_BROKEN, HINT);
    } else {
        log::debug!("窗口组件仍然断开，{what}打不开");
    }
    true
}

/// 窗口开成功了：说明组件是好的。
pub fn note_ok() {
    BROKEN.store(false, Ordering::SeqCst);
}

pub fn is_broken() -> bool {
    BROKEN.load(Ordering::SeqCst)
}

/// 后台自动开窗口的功能（写作浮标）要不要现在就试：没断开就试；断开了隔几分钟才试一次。
pub fn should_try() -> bool {
    if !is_broken() {
        return true;
    }
    lock(&LAST_FAIL).map_or(true, |t| t.elapsed() >= RETRY_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_webview2_disconnect_error() {
        let log = r#"runtime error: failed to create webview: WebView2 error: WindowsError(Error { code: HRESULT(0x80010108), message: "被调用的对象已与其客户端断开连接。" })"#;
        assert!(is_disconnected(log));
        assert!(is_disconnected("RPC_E_DISCONNECTED"));
        assert!(is_disconnected("被调用的对象已与其客户端断开连接。"));
        assert!(!is_disconnected("WebView2 error: WindowsError(Error { code: HRESULT(0x8007139F) })"));
        assert!(!is_disconnected("窗口创建失败"));
    }

    #[test]
    fn background_retries_back_off_while_broken() {
        note_ok();
        assert!(should_try());
        BROKEN.store(true, Ordering::SeqCst);
        *lock(&LAST_FAIL) = Some(Instant::now());
        assert!(!should_try(), "刚失败过，先不要再试");
        *lock(&LAST_FAIL) = Instant::now().checked_sub(RETRY_AFTER + Duration::from_secs(1));
        assert!(should_try(), "隔了几分钟可以再试一次");
        note_ok();
        assert!(!is_broken());
    }
}
