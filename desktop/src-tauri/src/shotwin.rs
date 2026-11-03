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

// ---------- 钉在桌面 ----------

static PINS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static PIN_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn pins() -> std::sync::MutexGuard<'static, Vec<(String, String)>> {
    PINS.lock().unwrap_or_else(|e| e.into_inner())
}

/// 把译图钉成一个置顶小窗。`x/y/w/h` 是截图窗口里的 CSS 像素。
#[tauri::command]
pub fn shot_pin(app: AppHandle, data_url: String, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    use tauri::{PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
    if !data_url.starts_with("data:image/") || data_url.len() > 40_000_000 {
        return Err("图片不对".into());
    }
    let (ox, oy, k) = match app.get_webview_window(LABEL) {
        Some(win) => {
            let p = win.outer_position().map_err(|e| e.to_string())?;
            (p.x, p.y, win.scale_factor().unwrap_or(1.0))
        }
        None => (0, 0, 1.0),
    };
    let label = format!("shotpin{}", PIN_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    pins().push((label.clone(), data_url));
    let protected = app.try_state::<Arc<AppState>>().is_some_and(|st| st.settings_raw().security.content_protection);
    let win = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html#shotpin".into()))
        .title("清影译图")
        .decorations(false)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .content_protected(protected)
        .build()
        .map_err(|e| e.to_string())?;
    crate::winframe::guard(&win);
    let _ = win.set_position(PhysicalPosition::new(ox + (x * k) as i32, oy + (y * k) as i32));
    let _ = win.set_size(PhysicalSize::new((w * k).max(60.0) as u32, (h * k).max(40.0) as u32));
    let _ = win.show();
    Ok(())
}

#[tauri::command]
pub fn shot_pin_take(window: tauri::WebviewWindow) -> Option<String> {
    let label = window.label().to_string();
    let mut g = pins();
    let i = g.iter().position(|(l, _)| *l == label)?;
    Some(g.remove(i).1)
}

#[tauri::command]
pub fn shot_pin_close(window: tauri::WebviewWindow) {
    let _ = window.destroy();
}

// ---------- 对比服务 ----------

/// 用“通用翻译”（Google 网页翻译接口）把原文再译一遍，和清影大模型的结果并排看。
#[tauri::command]
pub async fn shot_compare(state: tauri::State<'_, Arc<AppState>>, text: String, lang: String) -> Result<String, String> {
    let text: String = text.chars().take(4000).collect();
    if text.trim().is_empty() {
        return Err("没有可译的文字".into());
    }
    let tl = google_lang(&lang);
    let url = format!("https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={tl}&dt=t&q={}", urlencoding_min(&text));
    let resp = state.dl_client.get(&url).timeout(std::time::Duration::from_secs(15)).send().await.map_err(|e| format!("通用翻译连不上：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("通用翻译返回 {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|_| "通用翻译返回的内容看不懂".to_string())?;
    let out: String = v.get(0).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|p| p.get(0).and_then(|s| s.as_str())).collect()).unwrap_or_default();
    if out.trim().is_empty() {
        return Err("通用翻译没有返回结果".into());
    }
    Ok(out)
}

fn google_lang(l: &str) -> &'static str {
    let l = l.to_lowercase();
    if l.contains("繁") || l.contains("traditional") {
        "zh-TW"
    } else if l.contains("英") || l.contains("english") {
        "en"
    } else if l.contains("日") || l.contains("japan") {
        "ja"
    } else if l.contains("韩") || l.contains("korea") {
        "ko"
    } else if l.contains("法") || l.contains("french") {
        "fr"
    } else if l.contains("德") || l.contains("german") {
        "de"
    } else if l.contains("西班牙") || l.contains("spanish") {
        "es"
    } else if l.contains("俄") || l.contains("russian") {
        "ru"
    } else {
        "zh-CN"
    }
}

fn urlencoding_min(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

// ---------- 最近的截图翻译（只存文字） ----------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShotEntry {
    pub at: i64,
    pub src: String,
    pub text: String,
    /// 存为笔记
    pub saved: bool,
}

const HIST: &str = "shot_history.json";
const HIST_MAX: usize = 200;

fn hist_load(dir: &std::path::Path) -> Vec<ShotEntry> {
    std::fs::read_to_string(dir.join(HIST)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn hist_store(dir: &std::path::Path, v: &[ShotEntry]) {
    if let Ok(t) = serde_json::to_string(v) {
        let _ = std::fs::write(dir.join(HIST), t);
    }
}

/// 加一条；返回它的时间戳（用来之后“存为笔记”）。设置里关了记录就不存，返回 0。
pub fn hist_push(dir: &std::path::Path, enabled: bool, src: &str, text: &str, now: i64) -> i64 {
    if !enabled || text.trim().is_empty() {
        return 0;
    }
    let mut v = hist_load(dir);
    let at = v.iter().map(|e| e.at).max().map_or(now, |m| now.max(m + 1));
    v.push(ShotEntry { at, src: src.chars().take(2000).collect(), text: text.chars().take(2000).collect(), saved: false });
    let skip = v.len().saturating_sub(HIST_MAX);
    // 已存为笔记的不被挤掉
    let mut keep: Vec<ShotEntry> = v.iter().take(skip).filter(|e| e.saved).cloned().collect();
    keep.extend(v.into_iter().skip(skip));
    hist_store(dir, &keep);
    at
}

#[tauri::command]
pub fn shot_hist_add(state: tauri::State<'_, Arc<AppState>>, src: String, text: String) -> i64 {
    let on = state.settings_raw().shot_history;
    hist_push(&state.data_dir, on, &src, &text, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64))
}

#[tauri::command]
pub fn shot_hist_list(state: tauri::State<'_, Arc<AppState>>, saved_only: Option<bool>) -> Vec<ShotEntry> {
    let mut v = hist_load(&state.data_dir);
    if saved_only.unwrap_or(false) {
        v.retain(|e| e.saved);
    }
    v.reverse();
    v
}

#[tauri::command]
pub fn shot_hist_save(state: tauri::State<'_, Arc<AppState>>, at: i64, saved: bool) {
    let mut v = hist_load(&state.data_dir);
    if let Some(e) = v.iter_mut().find(|e| e.at == at) {
        e.saved = saved;
    }
    hist_store(&state.data_dir, &v);
}

#[tauri::command]
pub fn shot_hist_clear(state: tauri::State<'_, Arc<AppState>>, keep_saved: Option<bool>) {
    let mut v = hist_load(&state.data_dir);
    if keep_saved.unwrap_or(true) {
        v.retain(|e| e.saved);
    } else {
        v.clear();
    }
    hist_store(&state.data_dir, &v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_saved_and_respects_switch() {
        let d = std::env::temp_dir().join(format!("cc-shothist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(hist_push(&d, false, "a", "b", 10), 0);
        let a = hist_push(&d, true, "hello", "你好", 10);
        let b = hist_push(&d, true, "bye", "再见", 10);
        assert!(b > a);
        let mut v = hist_load(&d);
        v[0].saved = true;
        hist_store(&d, &v);
        for i in 0..(HIST_MAX + 5) {
            hist_push(&d, true, "x", &format!("t{i}"), 100 + i as i64);
        }
        let v = hist_load(&d);
        assert!(v.len() <= HIST_MAX + 1);
        assert!(v.iter().any(|e| e.saved && e.src == "hello"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn encodes_query_and_maps_lang() {
        assert_eq!(urlencoding_min("a b&中"), "a%20b%26%E4%B8%AD");
        assert_eq!(google_lang("简体中文"), "zh-CN");
        assert_eq!(google_lang("English"), "en");
    }
}
