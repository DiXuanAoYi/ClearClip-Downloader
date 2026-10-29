//! 系统托盘、主窗口 / 迷你窗的显示控制。

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, Wry};

use crate::{clipboard, AppState};

/// 迷你窗的尺寸（逻辑像素）
const MINI_W: f64 = 320.0;
const MINI_H: f64 = 196.0;
/// 悬浮拖拽窗的尺寸（逻辑像素）
const FLOAT_W: f64 = 156.0;
const FLOAT_H: f64 = 64.0;

pub struct TrayWatchItem(pub CheckMenuItem<Wry>);

/// 托盘菜单里需要随语言改文字的菜单项
pub struct TrayLabels {
    show: MenuItem<Wry>,
    parse: MenuItem<Wry>,
    lock: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

/// 按界面语言刷新托盘菜单文字。
pub fn apply_language(app: &AppHandle, en: bool) {
    use crate::i18n::tr;
    if let Some(l) = app.try_state::<TrayLabels>() {
        let _ = l.show.set_text(tr(en, "显示主窗口"));
        let _ = l.parse.set_text(tr(en, "解析剪贴板"));
        let _ = l.lock.set_text(tr(en, "锁定"));
        let _ = l.quit.set_text(tr(en, "退出清影"));
    }
    if let Some(w) = app.try_state::<TrayWatchItem>() {
        let _ = w.0.set_text(tr(en, "监听剪贴板"));
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let watching = app.state::<Arc<AppState>>().settings().watch_clipboard;
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let parse = MenuItem::with_id(app, "parse", "解析剪贴板", true, None::<&str>)?;
    let watch = CheckMenuItem::with_id(app, "watch", "监听剪贴板", true, watching, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "锁定", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出清影", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &parse, &watch, &lock, &PredefinedMenuItem::separator(app)?, &quit])?;
    app.manage(TrayWatchItem(watch));
    app.manage(TrayLabels { show: show.clone(), parse: parse.clone(), lock: lock.clone(), quit: quit.clone() });

    let mut builder = TrayIconBuilder::with_id("main").tooltip("清影 ClearClip").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "parse" => clipboard::parse_clipboard_now(app),
            "watch" => {
                let st = app.state::<Arc<AppState>>();
                let paused = st.clipboard_paused.load(Ordering::SeqCst);
                st.clipboard_paused.store(!paused, Ordering::SeqCst);
                sync_watch_item(app);
            }
            "lock" => {
                // 没有设置应用锁时，只是把窗口收起来
                if !crate::security::lock_now(app) {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    apply_language(app, crate::i18n::is_en(&app.state::<Arc<AppState>>().settings_raw().language));
    Ok(())
}

/// 托盘勾选状态 = 设置里开启 且 没有临时暂停。
pub fn sync_watch_item(app: &AppHandle) {
    let st = app.state::<Arc<AppState>>();
    let on = st.settings().watch_clipboard && !st.clipboard_paused.load(Ordering::SeqCst);
    if let Some(item) = app.try_state::<TrayWatchItem>() {
        let _ = item.0.set_checked(on);
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    close_mini(app);
}

/// 托盘提示里的速度，例如 `3.2 MB/s`。
pub fn speed_text(bps: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bps as f64;
    if b >= KB * KB * KB {
        format!("{:.1} GB/s", b / (KB * KB * KB))
    } else if b >= KB * KB {
        format!("{:.1} MB/s", b / (KB * KB))
    } else if b >= KB {
        format!("{:.0} KB/s", b / KB)
    } else {
        format!("{bps} B/s")
    }
}

/// 托盘提示（鼠标悬停时显示）：总下载速度和任务数。只显示数字，不显示任何标题。
pub fn tooltip_for(en: bool, speed: u64, running: usize, queued: usize) -> String {
    let name = if en { "ClearClip" } else { "清影" };
    match (running, queued, en) {
        (0, 0, _) => "清影 ClearClip".to_string(),
        (r, 0, false) => format!("{name} · ↓ {} · {r} 个任务", speed_text(speed)),
        (r, q, false) => format!("{name} · ↓ {} · {r} 个下载中，{q} 个等待", speed_text(speed)),
        (r, 0, true) => format!("{name} · ↓ {} · {r} task{}", speed_text(speed), if r == 1 { "" } else { "s" }),
        (r, q, true) => format!("{name} · ↓ {} · {r} downloading, {q} waiting", speed_text(speed)),
    }
}

/// 每 2 秒刷新托盘提示（macOS 同时在图标旁显示速度）。
pub fn spawn_speed_ticker(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = String::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let st = app.state::<Arc<AppState>>();
            let snaps = st.downloads.snapshots();
            let running: Vec<_> = snaps.iter().filter(|s| s.status == crate::download::TaskStatus::Running).collect();
            let queued = snaps.iter().filter(|s| s.status == crate::download::TaskStatus::Queued).count();
            let speed: u64 = running.iter().map(|s| s.speed).sum();
            let tip = tooltip_for(crate::i18n::is_en(&st.settings_raw().language), speed, running.len(), queued);
            if tip == last {
                continue;
            }
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_tooltip(Some(&tip));
                #[cfg(target_os = "macos")]
                let _ = tray.set_title(if running.is_empty() { None } else { Some(speed_text(speed)) });
            }
            last = tip;
        }
    });
}

pub fn main_is_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main").map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false)).unwrap_or(false)
}

/// 工作区右下角的位置：窗口右边缘、下边缘离工作区边缘 `right`、`bottom`（物理像素）。
/// `area` 是工作区的 (x, y, 宽, 高)，`size` 是窗口的物理尺寸。
fn corner_position(area: (i32, i32, u32, u32), size: (u32, u32), right: i32, bottom: i32) -> (i32, i32) {
    let (x, y, w, h) = area;
    (x + w as i32 - size.0 as i32 - right, y + h as i32 - size.1 as i32 - bottom)
}

/// 小窗（迷你窗、悬浮拖拽窗）的创建参数。
struct SmallWindow {
    label: &'static str,
    title: &'static str,
    url: &'static str,
    width: f64,
    height: f64,
    /// 窗口右边缘、下边缘离工作区边缘的距离（逻辑像素）
    right: f64,
    bottom: f64,
    /// 关掉 Tauri 自己接管的文件拖放，让页面能收到浏览器拖来的链接
    page_handles_drops: bool,
    /// 能不能拖动边缘改大小。Linux 上不可调整大小的窗口会忽略设定的尺寸、按页面的最小尺寸来，
    /// 所以悬浮窗在 Linux / macOS 上要保持“可调整”，并给出最小尺寸；Windows 上不需要
    resizable: bool,
    min_size: Option<(f64, f64)>,
}

const MINI: SmallWindow = SmallWindow {
    label: "mini",
    title: "清影",
    url: "index.html#mini",
    width: MINI_W,
    height: MINI_H,
    right: 16.0,
    bottom: 16.0,
    page_handles_drops: false,
    resizable: false,
    min_size: None,
};
const FLOAT: SmallWindow = SmallWindow {
    label: "float",
    title: "清影拖拽区",
    url: "index.html#float",
    width: FLOAT_W,
    height: FLOAT_H,
    right: 24.0,
    bottom: MINI_H + 40.0,
    page_handles_drops: true,
    resizable: cfg!(not(windows)),
    min_size: Some((120.0, 48.0)),
};

/// 正在创建中的小窗，避免连续触发时建出两个。
static CREATING: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

/// 窗口右下角的位置（逻辑像素）和对应的物理像素位置：在主窗口所在的屏幕，没有就用主屏幕。
fn corner_for(app: &AppHandle, size: LogicalSize<f64>, right: f64, bottom: f64) -> Option<((f64, f64), (i32, i32))> {
    let monitor = app.get_webview_window("main").and_then(|w| w.current_monitor().ok().flatten()).or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let phys = size.to_physical::<u32>(scale);
    let area = monitor.work_area();
    let (x, y) = corner_position(
        (area.position.x, area.position.y, area.size.width, area.size.height),
        (phys.width, phys.height),
        (right * scale) as i32,
        (bottom * scale) as i32,
    );
    Some(((f64::from(x) / scale, f64::from(y) / scale), (x, y)))
}

/// 迷你窗、悬浮拖拽窗不再写在配置里预先建成隐藏窗口，而是用到时才创建、不用了就销毁：
/// 隐藏着的窗口在 Windows 上出现过自己冒出来、带着原生标题栏停在左上角的情况，不存在的窗口不会有这个问题。
/// 创建时就把位置、尺寸、无边框、置顶等参数定好，不依赖“先建好再显示再改位置”。
fn ensure_small_window(app: &AppHandle, spec: &'static SmallWindow) {
    if app.get_webview_window(spec.label).is_some() {
        return;
    }
    {
        let mut busy = CREATING.lock().unwrap_or_else(|e| e.into_inner());
        if busy.contains(&spec.label) {
            return;
        }
        busy.push(spec.label);
    }
    let app = app.clone();
    // 创建窗口要等界面线程响应，放到单独的线程里做，调用方（可能就在界面线程上）不会被卡住
    tauri::async_runtime::spawn_blocking(move || {
        let result = build_small_window(&app, spec);
        CREATING.lock().unwrap_or_else(|e| e.into_inner()).retain(|l| *l != spec.label);
        if let Err(e) = result {
            log::warn!("创建{}失败：{e}", spec.title);
        }
    });
}

fn build_small_window(app: &AppHandle, spec: &SmallWindow) -> tauri::Result<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let size = LogicalSize::new(spec.width, spec.height);
    let corner = corner_for(app, size, spec.right, spec.bottom);
    let protected = app.try_state::<Arc<AppState>>().is_some_and(|st| st.settings_raw().security.content_protection);
    let mut builder = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.url.into()))
        .title(spec.title)
        .inner_size(spec.width, spec.height)
        .resizable(spec.resizable)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .content_protected(protected);
    if spec.page_handles_drops {
        builder = builder.disable_drag_drop_handler();
    }
    if let Some((w, h)) = spec.min_size {
        builder = builder.min_inner_size(w, h);
    }
    if let Some(((x, y), _)) = corner {
        builder = builder.position(x, y);
    }
    let window = builder.build()?;
    crate::winframe::guard(&window);
    if let Some((_, (x, y))) = corner {
        // 创建时给的是逻辑坐标，跨屏、不同缩放比例时可能有偏差，创建好后再按物理像素放一次
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    Ok(())
}

/// 显示或隐藏悬浮拖拽窗；显示时放在屏幕右侧、迷你窗上方。
pub fn apply_float(app: &AppHandle, on: bool) {
    if !on {
        close_small_window(app, FLOAT.label);
        return;
    }
    if app.try_state::<Arc<AppState>>().is_some_and(|st| crate::security::status(&st).locked) {
        return;
    }
    ensure_small_window(app, &FLOAT);
}

/// 在屏幕右下角显示迷你窗（不抢焦点）。窗口刚创建时页面还没加载好，它的内容由页面加载后调用 `mini_ready` 取走。
pub fn show_mini(app: &AppHandle) {
    // 锁定时迷你窗会露出任务标题，不显示
    if app.try_state::<Arc<AppState>>().is_some_and(|st| crate::security::status(&st).locked) {
        return;
    }
    ensure_small_window(app, &MINI);
}

fn close_small_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.destroy();
    }
}

/// 收起迷你窗（销毁，下次需要时再创建），同时忘掉上一次给它的内容。
pub fn close_mini(app: &AppHandle) {
    close_small_window(app, MINI.label);
    crate::clipboard::forget_mini_state();
}

/// 锁定时收起所有小窗。
pub fn close_small_windows(app: &AppHandle) {
    close_mini(app);
    close_small_window(app, FLOAT.label);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_units() {
        assert_eq!(speed_text(0), "0 B/s");
        assert_eq!(speed_text(900), "900 B/s");
        assert_eq!(speed_text(2048), "2 KB/s");
        assert_eq!(speed_text(3_355_443), "3.2 MB/s");
        assert_eq!(speed_text(5 * 1024 * 1024 * 1024), "5.0 GB/s");
    }

    #[test]
    fn corner_is_inside_the_work_area() {
        // 1920×1040 的工作区（任务栏在下面），窗口 400×245（125% 缩放下的迷你窗），边距 20
        assert_eq!(corner_position((0, 0, 1920, 1040), (400, 245), 20, 20), (1500, 775));
        // 工作区不从原点开始（副屏在左边）
        assert_eq!(corner_position((-1920, 0, 1920, 1080), (195, 80), 30, 300), (-1920 + 1920 - 195 - 30, 1080 - 80 - 300));
        // 窗口比工作区还大时不会算出溢出的数
        let (x, y) = corner_position((0, 0, 100, 100), (400, 400), 0, 0);
        assert!(x <= 0 && y <= 0);
    }

    #[test]
    fn tooltip_shows_only_numbers() {
        assert_eq!(tooltip_for(false, 0, 0, 0), "清影 ClearClip");
        assert_eq!(tooltip_for(false, 1_048_576, 2, 0), "清影 · ↓ 1.0 MB/s · 2 个任务");
        assert_eq!(tooltip_for(false, 1_048_576, 1, 3), "清影 · ↓ 1.0 MB/s · 1 个下载中，3 个等待");
        assert_eq!(tooltip_for(true, 1_048_576, 1, 0), "ClearClip · ↓ 1.0 MB/s · 1 task");
        assert_eq!(tooltip_for(true, 1_048_576, 3, 2), "ClearClip · ↓ 1.0 MB/s · 3 downloading, 2 waiting");
    }
}
