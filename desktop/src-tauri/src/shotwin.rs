//! 桌面全局截图翻译：快捷键 → 整屏截图冻结 → 铺满所有显示器的选框窗口（前端 `ShotApp.vue`）。

use crate::AppState;
use base64::Engine;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

const LABEL: &str = "shot";

static PENDING: Mutex<Option<crate::scrcap::Grab>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Option<crate::scrcap::Grab>> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

/// 快捷键按下。
pub fn hotkey(app: &AppHandle) {
    if app.try_state::<Arc<AppState>>().is_some_and(|st| crate::security::status(&st).locked) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 已经开着：先关掉，再重新截（这时屏幕上没有它自己）
        close(&app);
        std::thread::sleep(std::time::Duration::from_millis(80));
        match crate::scrcap::grab_screen() {
            Ok(g) => {
                *lock() = Some(g);
                show(&app);
            }
            Err(e) => {
                log::warn!("截图翻译：{e}");
                let _ = tauri::Emitter::emit(&app, "toast://error", e);
            }
        }
    });
}

fn show(app: &AppHandle) {
    use tauri::{PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
    let Some((x, y, w, h)) = lock().as_ref().map(|g| (g.x, g.y, g.w, g.h)) else { return };
    let protected = app.try_state::<Arc<AppState>>().is_some_and(|st| st.settings_raw().security.content_protection);
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#shot".into()))
        .title("清影截图翻译")
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(true)
        .content_protected(protected)
        .build();
    match built {
        Ok(win) => {
            crate::winframe::guard(&win);
            let _ = win.set_position(PhysicalPosition::new(x, y));
            let _ = win.set_size(PhysicalSize::new(w, h));
            let _ = win.show();
            let _ = win.set_focus();
        }
        Err(e) => log::warn!("创建截图翻译窗口失败：{e}"),
    }
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
    *lock() = None;
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotInfo {
    pub data_url: String,
    pub w: u32,
    pub h: u32,
}

/// 窗口打开后取走那张冻结的截图。
#[tauri::command]
pub fn shot_take() -> Option<ShotInfo> {
    let g = lock().take()?;
    let data_url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&g.png));
    Some(ShotInfo { data_url, w: g.w, h: g.h })
}

#[tauri::command]
pub fn shot_run(app: AppHandle, req: serde_json::Value) -> Result<u64, String> {
    let r: crate::vsum::Req = serde_json::from_value(req).map_err(|_| "请求格式不对".to_string())?;
    if r.kind != "imgtrans" {
        return Err("请求格式不对".into());
    }
    crate::vsum::start(&app, r).map_err(|e| e.message)
}

#[tauri::command]
pub fn shot_close(app: AppHandle) {
    close(&app);
}
