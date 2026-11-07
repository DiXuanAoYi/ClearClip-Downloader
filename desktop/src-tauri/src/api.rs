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

pub fn handle(app: &AppHandle, req: &Request, peer: &SocketAddr) -> Option<Reply> {
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
        // 在这台电脑上显示清影主窗口（浏览器扩展的“在清影里查看”），只接受本机请求
        ("POST", "/api/v1/show") => {
            if !peer.ip().is_loopback() {
                return Some(err("403 Forbidden", "只能在本机打开窗口"));
            }
            crate::tray::show_main(app);
            ok(json!({"ok": true}))
        }
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
                // 打开所在文件夹：会在这台电脑上弹出窗口，只接受本机（浏览器扩展）的请求
                "reveal" => {
                    if !peer.ip().is_loopback() {
                        return Some(err("403 Forbidden", "只能在本机打开文件夹"));
                    }
                    let Some(path) = state.downloads.snapshots().into_iter().find(|t| t.id == id).map(|t| t.file_path) else {
                        return Some(err("404 Not Found", "任务不存在"));
                    };
                    if let Err(e) = reveal_path(app, &path) {
                        return Some(err("400 Bad Request", &e));
                    }
                }
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
        // 最近因“需要登录”失败的网站：浏览器扩展据此询问是否同步登录状态
        ("GET", "/api/v1/login_needed") => ok(json!({"sites": crate::inbox::login_needed(app)})),
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
        // 字幕学习：生词本（浏览器扩展用）。查词本身走 /ai/run 的 kind=lookup
        ("GET", "/api/v1/vocab/list") => {
            let q = req.query.get("q").cloned().unwrap_or_default();
            let f = req.query.get("state").cloned().unwrap_or_else(|| "all".into());
            match crate::vocab::list(&state.data_dir, &q, &f, 2000).and_then(|w| Ok((w, crate::vocab::stats(&state.data_dir)?))) {
                Ok((words, stats)) => ok(json!({"words": words, "stats": stats})),
                Err(e) => err("500 Internal Server Error", &e.message),
            }
        }
        ("GET", "/api/v1/vocab/words") => match crate::vocab::all_words(&state.data_dir) {
            Ok(words) => ok(json!({"words": words})),
            Err(e) => err("500 Internal Server Error", &e.message),
        },
        ("POST", "/api/v1/vocab/add") => match serde_json::from_slice::<crate::vocab::NewWord>(&req.body) {
            Ok(w) => match crate::vocab::add(&state.data_dir, &w) {
                Ok(id) => ok(json!({"id": id})),
                Err(e) => err("400 Bad Request", &e.message),
            },
            Err(_) => err("400 Bad Request", "请求格式不对"),
        },
        ("POST", "/api/v1/vocab/remove") => {
            let id = serde_json::from_slice::<Value>(&req.body).ok().and_then(|v| v["id"].as_i64()).unwrap_or(0);
            match crate::vocab::remove(&state.data_dir, id) {
                Ok(()) => ok(json!({"ok": true})),
                Err(e) => err("500 Internal Server Error", &e.message),
            }
        }
        ("POST", "/api/v1/vocab/review") => {
            let v = serde_json::from_slice::<Value>(&req.body).unwrap_or(Value::Null);
            match crate::vocab::review(&state.data_dir, v["id"].as_i64().unwrap_or(0), v["grade"].as_u64().unwrap_or(1) as u8) {
                Ok(w) => ok(json!({"word": w})),
                Err(e) => err("500 Internal Server Error", &e.message),
            }
        }
        ("POST", "/api/v1/vocab/unfamiliar") => {
            let v = serde_json::from_slice::<Value>(&req.body).unwrap_or(Value::Null);
            let words: Vec<String> =
                v["words"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).take(400).collect()).unwrap_or_default();
            ok(json!({"words": crate::vocab::unfamiliar(&words, v["minRank"].as_u64().unwrap_or(0) as usize)}))
        }
        ("GET", "/api/v1/vocab/export") => {
            let f = req.query.get("format").cloned().unwrap_or_else(|| "csv".into());
            // Anki 而且录过原声：多给一个 zip（卡片 + 原声），扩展直接存下来
            let zip = if f == "anki" { crate::vocab::anki_package(&state.data_dir).ok().flatten() } else { None };
            match crate::vocab::export(&state.data_dir, &f) {
                Ok((text, ext)) => {
                    use base64::Engine;
                    let mut v = json!({"text": text, "ext": ext});
                    if let Some(z) = zip {
                        v["zip"] = json!(base64::engine::general_purpose::STANDARD.encode(z));
                        v["clips"] = json!(crate::vocab::clips(&state.data_dir).len());
                    }
                    ok(v)
                }
                Err(e) => err("500 Internal Server Error", &e.message),
            }
        }
        // 原声：扩展录下的一句，存在清影这边
        ("POST", "/api/v1/vocab/clip") => {
            let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            match crate::vocab::save_clip(&state.data_dir, v["word"].as_str().unwrap_or(""), v["data"].as_str().unwrap_or("")) {
                Ok(name) => ok(json!({"name": name})),
                Err(e) => err("400 Bad Request", &e.message),
            }
        }
        // 浏览器扩展的用户数据（笔记、高亮、本地导入的字幕）：同步过来进备份，换浏览器 / 重装扩展时取回
        ("POST", "/api/v1/extdata") => {
            let v: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            match crate::datareg::ext_save(&state.data_dir, &v["data"], crate::datareg::ext_limit_bytes(state.settings().ext_data_mb)) {
                Ok(ts) => ok(json!({"ts": ts})),
                Err(e) => err("400 Bad Request", &e.message),
            }
        }
        ("GET", "/api/v1/extdata") => ok(crate::datareg::ext_load(&state.data_dir)),
        ("GET", "/api/v1/vocab/clips") => ok(json!({"words": crate::vocab::clips(&state.data_dir).into_iter().map(|(k, _)| k).collect::<Vec<_>>()})),
        // 视频缓存（浏览器扩展用）：同步摘要 / 双语字幕快照，恢复时取回
        ("POST", "/api/v1/videocache/put") => match serde_json::from_slice::<crate::videocache::Put>(&req.body) {
            Ok(p) => match crate::videocache::put(&state.data_dir, &p, state.settings().video_cache_mb) {
                Ok(i) => ok(json!({"info": i})),
                Err(e) => err("400 Bad Request", &e.message),
            },
            Err(_) => err("400 Bad Request", "请求格式不对"),
        },
        ("GET", "/api/v1/videocache/get") => match crate::videocache::get(&state.data_dir, req.query.get("url").map(String::as_str).unwrap_or("")) {
            Ok(Some(f)) => ok(serde_json::to_value(&f).unwrap_or(Value::Null)),
            Ok(None) => err("404 Not Found", "清影里没有这个视频的缓存"),
            Err(e) => err("500 Internal Server Error", &e.message),
        },
        ("GET", "/api/v1/videocache/info") => match crate::videocache::info(&state.data_dir, req.query.get("url").map(String::as_str).unwrap_or("")) {
            Ok(i) => ok(json!({"info": i, "auto": state.settings().video_cache_auto})),
            Err(e) => err("500 Internal Server Error", &e.message),
        },
        // 视频摘要（浏览器扩展用）：提交 → 轮询
        ("GET", "/api/v1/ai/status") => {
            // 浏览器扩展顺带报到：桌面端据此在对应浏览器的窗口里不再弹写作浮标
            if let Some(f) = req.query.get("ext") {
                crate::writebadge::mark_ext(f, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
            }
            ok(crate::vsum::status(&state))
        }
        ("POST", "/api/v1/ai/run") => match serde_json::from_slice::<crate::vsum::Req>(&req.body) {
            Ok(r) => match crate::vsum::start(app, r) {
                Ok(id) => ok(json!({"id": id})),
                Err(e) => err("400 Bad Request", &e.message),
            },
            Err(_) => err("400 Bad Request", "请求格式不对"),
        },
        ("GET", "/api/v1/ai/job") => match req.query.get("id").and_then(|i| i.parse::<u64>().ok()).and_then(crate::vsum::job_json) {
            Some(v) => ok(v),
            None => err("404 Not Found", "任务不存在或已过期"),
        },
        ("POST", "/api/v1/ai/cancel") => {
            let id = req.query.get("id").and_then(|i| i.parse::<u64>().ok()).unwrap_or(0);
            ok(json!({"ok": crate::vsum::cancel(id)}))
        }
        // 字幕工坊：生成字幕 / AI 纠错（路径是清影所在电脑上的文件），返回任务编号，用 /subtitle/job 查进度
        ("POST", "/api/v1/subtitle/generate") => match serde_json::from_slice::<SubReq>(&req.body) {
            Ok(r) => match crate::ai_cmds::subgen_start(app.clone(), r.path, r.opts) {
                Ok(id) => ok(json!({"id": id})),
                Err(e) => err("400 Bad Request", &e.message),
            },
            Err(_) => err("400 Bad Request", "请求格式不对"),
        },
        ("POST", "/api/v1/subtitle/correct") => match serde_json::from_slice::<SubReq>(&req.body) {
            Ok(r) => match crate::ai_cmds::subgen_correct(app.clone(), r.path, r.mode) {
                Ok(id) => ok(json!({"id": id})),
                Err(e) => err("400 Bad Request", &e.message),
            },
            Err(_) => err("400 Bad Request", "请求格式不对"),
        },
        ("GET", "/api/v1/subtitle/job") => {
            let id = req.query.get("id").and_then(|i| i.parse::<u64>().ok()).unwrap_or(0);
            match state.media_jobs.snapshot().into_iter().find(|j| j.id == id) {
                Some(j) => ok(serde_json::to_value(&j).unwrap_or(Value::Null)),
                None => err("404 Not Found", "任务不存在或已过期"),
            }
        }
        _ => err("404 Not Found", "未知接口"),
    })
}

/// 字幕接口的请求体：`path` + 生成选项（字段同 GenOpts）；纠错时用 `mode`。
#[derive(serde::Deserialize)]
struct SubReq {
    path: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(flatten)]
    opts: crate::subgen::GenOpts,
}

/// 在文件管理器里显示文件；文件已不在时打开它所在的文件夹。
fn reveal_path(app: &AppHandle, path: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let p = std::path::PathBuf::from(path);
    if p.exists() {
        return app.opener().reveal_item_in_dir(&p).map_err(|e| format!("无法打开所在文件夹：{e}"));
    }
    match p.parent().filter(|d| d.is_dir()) {
        Some(dir) => app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| format!("无法打开文件夹：{e}")),
        None => Err("文件已被移动或删除".into()),
    }
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
