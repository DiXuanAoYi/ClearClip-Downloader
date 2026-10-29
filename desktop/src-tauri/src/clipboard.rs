//! 剪贴板监听：复制到受支持的链接时通知前端，主窗口不在前台时弹出迷你窗。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::providers::{self, DetectedLink};
use crate::{download, tray, AppState};

pub const EVT_CLIPBOARD: &str = "clipboard://link";
pub const EVT_PARSE_REQUEST: &str = "app://parse-request";
pub const EVT_AUTO_RESULT: &str = "clipboard://auto-result";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardLink {
    pub text: String,
    pub links: Vec<DetectedLink>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoResult {
    pub url: String,
    pub ok: bool,
    pub message: String,
}

/// 迷你窗是用到时才创建的，页面加载好之前发出的事件它收不到；最近一次的链接和自动下载结果记在这里，
/// 迷你窗页面准备好后用 `mini_ready` 取走。
#[derive(Default)]
struct MiniMemory {
    link: Option<ClipboardLink>,
    result: Option<AutoResult>,
}

static MINI_MEMORY: std::sync::Mutex<MiniMemory> = std::sync::Mutex::new(MiniMemory { link: None, result: None });

/// 迷你窗页面刚加载好时需要的内容。
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MiniState {
    pub link: Option<ClipboardLink>,
    pub result: Option<AutoResult>,
}

pub fn take_mini_state() -> MiniState {
    let m = MINI_MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    MiniState { link: m.link.clone(), result: m.result.clone() }
}

pub fn forget_mini_state() {
    let mut m = MINI_MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    m.link = None;
    m.result = None;
}

fn remember_result(r: &AutoResult) {
    MINI_MEMORY.lock().unwrap_or_else(|e| e.into_inner()).result = Some(r.clone());
}

pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        // 启动时剪贴板里已有的内容不触发
        let mut last = app.clipboard().read_text().unwrap_or_default();
        loop {
            std::thread::sleep(Duration::from_millis(800));
            let Ok(text) = app.clipboard().read_text() else { continue };
            if text == last {
                continue;
            }
            last = text.clone();
            let st = app.state::<Arc<AppState>>().inner().clone();
            {
                let mut ignore = st.clipboard_ignore.lock().unwrap_or_else(|e| e.into_inner());
                if ignore.as_deref() == Some(text.as_str()) {
                    *ignore = None;
                    continue;
                }
            }
            let settings = st.settings();
            if !settings.watch_clipboard || st.clipboard_paused.load(Ordering::SeqCst) || !settings.disclaimer_accepted {
                continue;
            }
            let links = providers::detect_links_with(&text, &settings);
            if links.is_empty() {
                continue;
            }
            on_links(&app, text, links, settings.auto_download);
        }
    });
}

pub(crate) fn on_links(app: &AppHandle, text: String, links: Vec<DetectedLink>, auto_download: bool) {
    notify_links(app, text, links.clone());
    let urls: Vec<String> = links.iter().map(|l| l.url.clone()).collect();
    let ids = crate::inbox::record_clipboard(app, &urls, auto_download);
    if auto_download {
        for (link, id) in links.into_iter().zip(ids) {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let payload = match id {
                    // 记录下来时由收件箱处理，结果写回记录
                    Some(id) => {
                        crate::inbox::process(&app, id).await;
                        let item = app.state::<Arc<AppState>>().db.inbox_get(id).ok().flatten();
                        match item {
                            Some(i) if i.status == "failed" => AutoResult { url: link.url, ok: false, message: i.message.unwrap_or_default() },
                            Some(i) if i.status == "playlist" => AutoResult { url: link.url, ok: false, message: i.message.unwrap_or_default() },
                            Some(i) => {
                                let name = added_name(&app, &i.task_ids, &i.title.unwrap_or_default());
                                AutoResult {
                                    url: link.url,
                                    ok: true,
                                    message: format!("已加入下载：{}", i.message.map(|m| format!("{name}（{m}）")).unwrap_or(name)),
                                }
                            }
                            None => return,
                        }
                    }
                    None => match resolve_and_enqueue(&app, &link.url).await {
                        Ok(title) => AutoResult { url: link.url, ok: true, message: format!("已加入下载：{title}") },
                        Err(e) => AutoResult { url: link.url, ok: false, message: e.to_string() },
                    },
                };
                remember_result(&payload);
                let _ = app.emit(EVT_AUTO_RESULT, payload);
            });
        }
    }
}

/// 自动加入下载后提示里显示的名字：新建了任务就用任务的文件名（按“文件命名”生成，和队列里一致），
/// 没有新任务（之前下载过、已在队列中）时用作品标题。
fn added_name(app: &AppHandle, task_ids: &[i64], title: &str) -> String {
    names_text(&download::file_names(app, task_ids), title)
}

fn names_text(names: &[String], fallback: &str) -> String {
    match names {
        [] => fallback.to_string(),
        [one] => one.clone(),
        [first, rest @ ..] => format!("{first} 等 {} 个文件", rest.len() + 1),
    }
}

/// 通知前端识别到链接；主窗口不在前台时弹出迷你窗。
pub(crate) fn notify_links(app: &AppHandle, text: String, links: Vec<DetectedLink>) {
    let payload = ClipboardLink { text, links };
    {
        let mut m = MINI_MEMORY.lock().unwrap_or_else(|e| e.into_inner());
        m.link = Some(payload.clone());
        m.result = None;
    }
    let _ = app.emit(EVT_CLIPBOARD, payload);
    if !tray::main_is_focused(app) {
        tray::show_mini(app);
    }
}

/// 解析并把默认资源加入下载队列，返回作品标题。
pub async fn resolve_and_enqueue(app: &AppHandle, text: &str) -> crate::model::AppResult<String> {
    let r = resolve_and_enqueue_full(app, text).await?;
    if let Some(n) = r.playlist {
        return Err(crate::model::AppError::invalid(format!("“{}”是一个列表（{n} 条），请在电脑上选择要下载的条目。", r.title)));
    }
    Ok(r.message())
}

/// 自动加入队列的结果。
pub struct AutoEnqueued {
    pub title: String,
    pub task_ids: Vec<i64>,
    /// 新建任务的文件名（和下载队列里显示的一致）
    pub file_names: Vec<String>,
    pub already_downloaded: usize,
    pub already_queued: usize,
    /// 是列表（合集、播放列表）时的条目数：不自动下载，需要用户选择
    pub playlist: Option<usize>,
}

impl AutoEnqueued {
    pub fn message(&self) -> String {
        if self.task_ids.is_empty() && self.already_downloaded > 0 {
            format!("{}（之前已下载）", self.title)
        } else if self.task_ids.is_empty() && self.already_queued > 0 {
            format!("{}（已在队列中）", self.title)
        } else {
            names_text(&self.file_names, &self.title)
        }
    }
}

/// 解析并按默认选项加入队列（剪贴板自动下载、手机发送）。
pub async fn resolve_and_enqueue_full(app: &AppHandle, text: &str) -> crate::model::AppResult<AutoEnqueued> {
    let st = app.state::<Arc<AppState>>().inner().clone();
    let settings = st.settings();
    let info = providers::resolve_text(&st.parse_ctx(&settings), text).await?;
    if !settings.security.privacy_mode {
        let _ = st.db.upsert_history(&info);
    }
    let _ = st.db.inbox_mark_parsed(text, &info);
    let (ids, post) = crate::quality::auto_selection(&info, &settings);
    let title = info.title.clone();
    if info.kind == crate::model::MediaKind::Playlist {
        return Ok(AutoEnqueued { title, task_ids: vec![], file_names: vec![], already_downloaded: 0, already_queued: 0, playlist: Some(info.entries.len()) });
    }
    let r = download::enqueue_with(app, info, &ids, post)?;
    Ok(AutoEnqueued {
        title,
        task_ids: r.tasks.iter().map(|t| t.id).collect(),
        file_names: r.tasks.iter().map(|t| crate::naming::display_name(&t.file_path)).collect(),
        already_downloaded: r.already_downloaded,
        already_queued: r.already_queued,
        playlist: None,
    })
}

/// 全局快捷键 / 托盘菜单：打开主窗口并解析当前剪贴板。
pub fn parse_clipboard_now(app: &AppHandle) {
    let text = app.clipboard().read_text().unwrap_or_default();
    tray::show_main(app);
    let _ = app.emit(EVT_PARSE_REQUEST, text);
}
