//! 写作浮窗的桌面入口：全局快捷键 → 取选中的文字 → 弹出写作窗 → 改好后写回原来的程序。
//!
//! 取字：Windows 上先等快捷键的修饰键松开，再模拟 Ctrl+C，靠“剪贴板序号变没变”判断有没有选中文字，
//! 取完把原来的剪贴板文字放回去；剪贴板里是图片或文件时不去动它（不取词，让用户自己粘贴）。
//! 写回：把结果放进剪贴板，切回原来的窗口，模拟 Ctrl+V，再还原剪贴板。
//! 其他系统暂时只有“读剪贴板里的文字”这一步。

use crate::settings::WritingSettings;
use crate::AppState;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

pub const LABEL: &str = "write";
const W: f64 = 440.0;
const H: f64 = 560.0;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// 取到的文字（可能为空：没选中、或剪贴板里不是文字）
    pub text: String,
    /// 来自选中的文字（true）还是剪贴板里原有的文字（false）
    pub from_selection: bool,
    /// 来源程序的进程名，如 `winword.exe`
    pub app: String,
    /// 能不能写回（有来源窗口）
    pub can_replace: bool,
    /// 给界面看的提示
    pub note: String,
}

/// 写回要用的现场。
#[derive(Debug, Clone, Default)]
struct Target {
    hwnd: isize,
    /// 取词之前的剪贴板文字，用完要放回去
    old_text: Option<String>,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static TARGET: Mutex<Option<Target>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn take_pending() -> Option<Pending> {
    lock(&PENDING).take()
}

/// 窗口出现的位置：贴着鼠标右下，放不下就往回收，始终在工作区内。
pub fn place(cursor: (i32, i32), size: (i32, i32), area: (i32, i32, i32, i32)) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let x = (cursor.0 + 16).min(ax + aw - size.0 - 8).max(ax + 8);
    let y = (cursor.1 + 16).min(ay + ah - size.1 - 8).max(ay + 8);
    (x, y)
}

/// 取到的文字整理：去掉模拟复制带来的首尾换行，纯空白当作没有。
pub fn clean_selection(raw: &str) -> String {
    let t = raw.trim_matches(|c| c == '\r' || c == '\n');
    if t.trim().is_empty() {
        String::new()
    } else {
        t.to_string()
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn settings(app: &AppHandle) -> WritingSettings {
    app.try_state::<Arc<AppState>>().map(|st| st.settings_raw().writing).unwrap_or_default()
}

/// 快捷键按下：取词并弹窗。
pub fn hotkey(app: &AppHandle) {
    if app.try_state::<Arc<AppState>>().is_some_and(|st| crate::security::status(&st).locked) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pending = capture(&app);
        *lock(&PENDING) = Some(pending);
        show_window(&app);
    });
}

fn capture(app: &AppHandle) -> Pending {
    let cfg = settings(app);
    let (hwnd, exe) = sys::foreground();
    if cfg.app_off(&exe) {
        return Pending { note: "这个程序已在“不在这些程序里出现”列表里".into(), ..Default::default() };
    }
    let old_text = app.clipboard().read_text().ok().filter(|t| !t.is_empty());
    let mut target = Target { hwnd, old_text: old_text.clone() };
    let mut p = Pending { app: exe, can_replace: hwnd != 0, ..Default::default() };
    match sys::copy_selection() {
        Copied::Text(t) => {
            p.text = clean_selection(&t);
            p.from_selection = !p.text.is_empty();
            // 取词时把原来的剪贴板放回去
            match &old_text {
                Some(o) => {
                    let _ = app.clipboard().write_text(o.clone());
                }
                None => {
                    let _ = app.clipboard().clear();
                }
            }
        }
        Copied::Nothing => {
            p.note = "没有选中文字".into();
        }
        Copied::Skipped => {
            p.note = "剪贴板里是图片或文件，为避免覆盖没有自动取词；请先选中文字再复制，或直接在下面粘贴".into();
            target.old_text = None;
        }
        Copied::Unsupported => {
            // 非 Windows：用剪贴板里已有的文字
            p.text = clean_selection(&old_text.unwrap_or_default());
            p.note = if p.text.is_empty() { "请先复制要处理的文字，再按快捷键".into() } else { "使用剪贴板里的文字".into() };
            p.can_replace = false;
        }
    }
    *lock(&TARGET) = Some(target);
    p
}

fn show_window(app: &AppHandle) {
    use tauri::{PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window(LABEL) {
        // 已经开着：让页面重新取一次内容
        let _ = tauri::Emitter::emit(app, "write://new", ());
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let cursor = app.cursor_position().ok();
    let monitor = cursor.and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten()).or_else(|| app.primary_monitor().ok().flatten());
    let protected = app.try_state::<Arc<AppState>>().is_some_and(|st| st.settings_raw().security.content_protection);
    let mut b = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#write".into()))
        .title("清影写作")
        .inner_size(W, H)
        .min_inner_size(360.0, 360.0)
        .decorations(false)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(true)
        .content_protected(protected);
    let mut phys = None;
    if let (Some(c), Some(m)) = (cursor, monitor) {
        let s = m.scale_factor();
        let a = m.work_area();
        let pos = place((c.x as i32, c.y as i32), ((W * s) as i32, (H * s) as i32), (a.position.x, a.position.y, a.size.width as i32, a.size.height as i32));
        b = b.position(f64::from(pos.0) / s, f64::from(pos.1) / s);
        phys = Some(pos);
    }
    match b.build() {
        Ok(w) => {
            crate::winframe::guard(&w);
            if let Some((x, y)) = phys {
                let _ = w.set_position(PhysicalPosition::new(x, y));
            }
            let _ = w.set_focus();
        }
        Err(e) => log::warn!("创建写作窗失败：{e}"),
    }
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}

/// 把结果交出去：`replace` 为真时写回原来的程序，否则只放进剪贴板。
pub fn apply(app: &AppHandle, text: &str, replace: bool) -> Result<String, String> {
    let target = lock(&TARGET).take().unwrap_or_default();
    let cb = app.clipboard();
    cb.write_text(text.to_string()).map_err(|e| format!("写剪贴板失败：{e}"))?;
    close(app);
    if !replace || target.hwnd == 0 || !sys::SUPPORTED {
        return Ok("已复制到剪贴板".into());
    }
    let app = app.clone();
    let old = target.old_text.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pasted = sys::paste_into(target.hwnd);
        // 等目标程序读走剪贴板，再放回原来的内容
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let (true, Some(o)) = (pasted, old) {
            let _ = app.clipboard().write_text(o);
        }
    });
    Ok("已替换".into())
}

// ---------- 命令 ----------

#[tauri::command]
pub fn write_take() -> Option<Pending> {
    take_pending()
}

#[tauri::command]
pub fn write_settings(state: tauri::State<'_, Arc<AppState>>) -> WritingSettings {
    state.settings_raw().writing
}

#[tauri::command]
pub fn write_run(app: AppHandle, state: tauri::State<'_, Arc<AppState>>, req: serde_json::Value) -> Result<u64, String> {
    let mut r: crate::vsum::Req = serde_json::from_value(req).map_err(|_| "请求格式不对".to_string())?;
    let cfg = state.settings_raw().writing;
    if r.write.route.is_empty() {
        r.write.route = cfg.route.clone();
    }
    if r.write.candidates == 0 {
        r.write.candidates = cfg.candidates;
    }
    r.write.extra = with_style(&r.write.extra, &cfg.style);
    crate::vsum::start(&app, r).map_err(|e| e.message)
}

/// 个人风格并进额外要求里。
pub fn with_style(extra: &str, style: &str) -> String {
    let (e, s) = (extra.trim(), style.trim());
    match (e.is_empty(), s.is_empty()) {
        (_, true) => e.to_string(),
        (true, false) => format!("我的写作风格：{s}"),
        (false, false) => format!("{e}\n我的写作风格：{s}"),
    }
}

#[tauri::command]
pub fn write_job(id: u64) -> Option<serde_json::Value> {
    crate::vsum::job_json(id)
}

#[tauri::command]
pub fn write_cancel(id: u64) -> bool {
    crate::vsum::cancel(id)
}

#[tauri::command]
pub fn write_apply(app: AppHandle, text: String, replace: bool) -> Result<String, String> {
    apply(&app, &text, replace)
}

#[tauri::command]
pub fn write_close(app: AppHandle) {
    *lock(&TARGET) = None;
    close(&app);
}

#[tauri::command]
pub fn write_pause(app: AppHandle, state: tauri::State<'_, Arc<AppState>>, minutes: i64) -> Result<(), String> {
    let mut s = state.settings_raw();
    s.writing.paused_until = if minutes > 0 { now_secs() + minutes * 60 } else { 0 };
    s.save(&state.settings_path).map_err(|e| e.message)?;
    *state.settings.write().unwrap_or_else(|e| e.into_inner()) = s;
    let _ = app;
    Ok(())
}

// ---------- 系统相关 ----------

pub enum Copied {
    Text(String),
    Nothing,
    Skipped,
    Unsupported,
}

#[cfg(windows)]
mod sys {
    use super::Copied;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND};
    use windows_sys::Win32::System::DataExchange::{GetClipboardSequenceNumber, IsClipboardFormatAvailable};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow};

    pub const SUPPORTED: bool = true;
    const CF_BITMAP: u32 = 2;
    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const CF_DIB: u32 = 8;

    pub fn foreground() -> (isize, String) {
        unsafe {
            let h = GetForegroundWindow();
            if h.is_null() {
                return (0, String::new());
            }
            (h as isize, exe_of(h))
        }
    }

    unsafe fn exe_of(h: HWND) -> String {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        if pid == 0 {
            return String::new();
        }
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if p.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(p);
        if ok == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
    }

    fn key(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } },
        }
    }

    fn send(keys: &[INPUT]) {
        unsafe {
            SendInput(keys.len() as u32, keys.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// 等快捷键的修饰键都松开，否则模拟出来的 Ctrl+C 会变成 Ctrl+Alt+C。
    fn wait_modifiers_up() {
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(800) {
            let down = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| unsafe { GetAsyncKeyState(i32::from(*k)) } < 0);
            if !down {
                return;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn ctrl_combo(letter: u16) {
        send(&[key(VK_CONTROL, false), key(letter, false), key(letter, true), key(VK_CONTROL, true)]);
    }

    fn clip_has(fmt: u32) -> bool {
        unsafe { IsClipboardFormatAvailable(fmt) != 0 }
    }

    /// 模拟 Ctrl+C 取选中的文字；用剪贴板序号判断有没有复制成功。
    pub fn copy_selection() -> Copied {
        if clip_has(CF_HDROP) || clip_has(CF_BITMAP) || clip_has(CF_DIB) {
            return Copied::Skipped;
        }
        wait_modifiers_up();
        let before = unsafe { GetClipboardSequenceNumber() };
        ctrl_combo(b'C' as u16);
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(450) {
            if unsafe { GetClipboardSequenceNumber() } != before {
                std::thread::sleep(Duration::from_millis(30));
                if !clip_has(CF_UNICODETEXT) {
                    return Copied::Nothing;
                }
                return match read_unicode() {
                    Some(t) => Copied::Text(t),
                    None => Copied::Nothing,
                };
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        Copied::Nothing
    }

    fn read_unicode() -> Option<String> {
        use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
        unsafe {
            for _ in 0..10 {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    let h = GetClipboardData(CF_UNICODETEXT);
                    let mut out = None;
                    if !h.is_null() {
                        let p = GlobalLock(h) as *const u16;
                        if !p.is_null() {
                            let mut n = 0;
                            while *p.add(n) != 0 && n < 1_000_000 {
                                n += 1;
                            }
                            out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                            GlobalUnlock(h);
                        }
                    }
                    CloseClipboard();
                    return out;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            None
        }
    }

    /// 切回原窗口并粘贴；窗口没了返回 false。
    pub fn paste_into(hwnd: isize) -> bool {
        unsafe {
            let h = hwnd as HWND;
            if IsWindow(h) == 0 {
                return false;
            }
            SetForegroundWindow(h);
        }
        std::thread::sleep(Duration::from_millis(150));
        wait_modifiers_up();
        ctrl_combo(b'V' as u16);
        true
    }
}

#[cfg(not(windows))]
mod sys {
    use super::Copied;
    pub const SUPPORTED: bool = false;
    pub fn foreground() -> (isize, String) {
        (0, String::new())
    }
    pub fn copy_selection() -> Copied {
        Copied::Unsupported
    }
    pub fn paste_into(_hwnd: isize) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_stays_inside_the_work_area() {
        let area = (0, 0, 1920, 1040);
        assert_eq!(place((100, 100), (440, 560), area), (116, 116));
        assert_eq!(place((1900, 1000), (440, 560), area), (1472, 472));
        assert_eq!(place((-50, -50), (440, 560), area), (8, 8));
    }

    #[test]
    fn selection_is_cleaned() {
        assert_eq!(clean_selection("你好\r\n"), "你好");
        assert_eq!(clean_selection(" \r\n "), "");
        assert_eq!(clean_selection("  keep indent"), "  keep indent");
    }

    #[test]
    fn style_joins_the_extra_request() {
        assert_eq!(with_style("", ""), "");
        assert_eq!(with_style(" 简短 ", ""), "简短");
        assert_eq!(with_style("", "口语化"), "我的写作风格：口语化");
        assert_eq!(with_style("简短", "口语化"), "简短\n我的写作风格：口语化");
    }

    #[test]
    fn pending_is_taken_once() {
        *lock(&PENDING) = Some(Pending { text: "a".into(), ..Default::default() });
        assert_eq!(take_pending().map(|p| p.text), Some("a".into()));
        assert!(take_pending().is_none());
    }

    #[test]
    fn writing_settings_are_sanitized() {
        let mut s = WritingSettings {
            route: "x".into(),
            candidates: 9,
            off_apps: vec![" Code.EXE ".into(), "code.exe".into(), "".into()],
            style: "  ".into(),
            ..Default::default()
        };
        s.sanitize();
        assert_eq!(s.route, "local_first");
        assert_eq!(s.candidates, 3);
        assert_eq!(s.off_apps, vec!["code.exe"]);
        assert!(s.app_off("CODE.exe") && !s.app_off(""));
        assert!(!s.paused(100) && WritingSettings { paused_until: 200, ..Default::default() }.paused(100));
    }
}
