//! 本机 / 局域网 HTTP 接口（v1）：查看和控制下载任务、添加链接、搜索媒体库。
//! 和“手机发链接”共用同一个服务和访问令牌（设置 → 手机与浏览器扩展里开启）。
//! 命令行（`clearclip list` 等）和网页控制台（`/console`）都是它的客户端。
//! 接口不返回本机文件路径；不能删除文件。

use std::net::SocketAddr;
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::db::LibraryFilter;
use crate::download::{self, TaskStatus};
use crate::phone::Request;
use crate::{providers, AppState};

pub type Reply = (&'static str, Value);

fn ok(v: Value) -> Reply {
    ("200 OK", v)
}

fn err(status: &'static str, msg: &str) -> Reply {
    (status, json!({"error": msg}))
}

fn st(app: &AppHandle) -> Arc<AppState> {
    use tauri::Manager;
    app.state::<Arc<AppState>>().inner().clone()
}

/// 任务列表里给外部看的字段（不含本机路径）。
pub fn task_json(t: &download::TaskSnapshot) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("filePath");
        // 只给文件名（按“文件命名”生成），不暴露保存路径
        o.insert("fileName".into(), Value::String(crate::naming::display_name(&t.file_path)));
    }
    v
}

/// 汇总：各状态数量和总速度。
pub fn summary(tasks: &[download::TaskSnapshot]) -> Value {
    let count = |s: TaskStatus| tasks.iter().filter(|t| t.status == s).count();
    json!({
        "running": count(TaskStatus::Running),
        "queued": count(TaskStatus::Queued),
        "paused": count(TaskStatus::Paused),
        "failed": count(TaskStatus::Failed),
        "done": count(TaskStatus::Done),
        "speed": tasks.iter().filter(|t| t.status == TaskStatus::Running).map(|t| t.speed).sum::<u64>(),
    })
}

/// `/api/v1/tasks/{id}/{action}` → (id, action)
fn task_action(path: &str) -> Option<(i64, &str)> {
    let rest = path.strip_prefix("/api/v1/tasks/")?;
    let (id, action) = rest.split_once('/')?;
    Some((id.parse().ok()?, action))
}

pub fn handle(app: &AppHandle, req: &Request, _peer: &SocketAddr) -> Option<Reply> {
    let path = req.path.as_str();
    if !path.starts_with("/api/v1/") {
        return None;
    }
    let state = st(app);
    Some(match (req.method.as_str(), path) {
        ("GET", "/api/v1/status") => {
            let tasks = state.downloads.snapshots();
            ok(
                json!({"app": "ClearClip", "version": app.package_info().version.to_string(), "tasks": summary(&tasks), "recording": state.live.any_recording()}),
            )
        }
        ("GET", "/api/v1/tasks") => ok(json!({"tasks": state.downloads.snapshots().iter().map(task_json).collect::<Vec<_>>()})),
        ("POST", "/api/v1/tasks/pause_all") => {
            download::pause_all(app);
            ok(json!({"ok": true}))
        }
        ("POST", "/api/v1/tasks/resume_all") => {
            download::resume_all(app);
            ok(json!({"ok": true}))
        }
        ("POST", "/api/v1/tasks/clear_finished") => {
            download::clear_finished(app);
            ok(json!({"ok": true}))
        }
        ("POST", _) if task_action(path).is_some() => {
            let (id, action) = task_action(path)?;
            if !state.downloads.snapshots().iter().any(|t| t.id == id) {
                return Some(err("404 Not Found", "任务不存在"));
            }
            match action {
                "pause" => download::pause(app, id),
                "resume" => download::resume(app, id),
                "cancel" => download::cancel(app, id),
                "remove" => download::remove(app, id),
                _ => return Some(err("404 Not Found", "未知操作")),
            }
            ok(json!({"ok": true}))
        }
        ("POST", "/api/v1/add") => {
            #[derive(serde::Deserialize)]
            struct Body {
                #[serde(default)]
                text: String,
                #[serde(default)]
                url: String,
            }
            let text = match serde_json::from_slice::<Body>(&req.body) {
                Ok(b) if !b.text.is_empty() => b.text,
                Ok(b) => b.url,
                Err(_) => String::from_utf8_lossy(&req.body).into_owned(),
            };
            let text: String = text.trim().chars().take(4000).collect();
            if providers::extract_urls(&text).is_empty() {
                return Some(err("400 Bad Request", "没有找到链接"));
            }
            match crate::phone::receive_trusted(app, "api", "api", "API", text) {
                Some(id) => ok(json!({"id": id, "state": "resolving"})),
                None => err("500 Internal Server Error", "保存记录失败"),
            }
        }
        ("GET", "/api/v1/library") => {
            let limit = req.query.get("limit").and_then(|l| l.parse::<i64>().ok()).unwrap_or(50).clamp(1, 500);
            let filter = LibraryFilter {
                query: req.query.get("q").cloned().unwrap_or_default(),
                kind: req.query.get("kind").cloned().filter(|k| !k.is_empty()),
                tag: req.query.get("tag").cloned().filter(|k| !k.is_empty()),
                ..Default::default()
            };
            match state.db.search_library(&filter, limit) {
                Ok(items) => ok(json!({"items": items.iter().map(|i| json!({
                    "id": i.id, "title": i.title, "author": i.author, "platform": i.platform_name, "kind": i.kind, "size": i.size,
                    "finishedAt": i.finished_at, "exists": i.exists, "favorite": i.favorite, "rating": i.rating, "tags": i.tags, "durationMs": i.duration_ms,
                    "sourceUrl": i.source_url,
                })).collect::<Vec<_>>()})),
                Err(e) => err("500 Internal Server Error", &e.message),
            }
        }
        _ => err("404 Not Found", "未知接口"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_action_paths() {
        assert_eq!(task_action("/api/v1/tasks/12/pause"), Some((12, "pause")));
        assert_eq!(task_action("/api/v1/tasks/x/pause"), None);
        assert_eq!(task_action("/api/v1/tasks/12"), None);
        assert_eq!(task_action("/api/v1/other/1/pause"), None);
    }
}
