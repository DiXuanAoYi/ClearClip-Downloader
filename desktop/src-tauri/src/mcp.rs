//! MCP（Model Context Protocol）服务：`clearclip mcp`。
//!
//! 通过标准输入输出（一行一条 JSON-RPC 消息）让 Claude、Codex、Cursor 等智能体调用清影。
//! 它不是独立的一套功能，只是本机接口（`/api/v1/…`）的一层包装：
//! 连接端口和令牌从清影的设置文件里读取，所以清影要在运行，并且开启了
//! “设置 → 手机与浏览器扩展”里的服务。不监听任何端口，也不暴露到局域网。
//!
//! 安全边界：
//! - 标准输出只写协议消息，日志写标准错误；
//! - 不提供删除文件、移除任务、读写设置的工具；接口本来就不返回本机文件路径；
//! - `--read-only`：只开放查询类工具（状态、任务列表、媒体库搜索、字幕任务进度）。

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{json, Value};

const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// 调用本机接口的方式；测试里换成假的。
pub trait Api {
    fn call(&self, method: &str, path: &str, body: Option<&str>) -> Result<Value, String>;
}

struct Live;

impl Api for Live {
    fn call(&self, method: &str, path: &str, body: Option<&str>) -> Result<Value, String> {
        crate::cli::call(method, path, body)
    }
}

/// 工具定义：(是否会改变状态, 定义)。
fn defs() -> Vec<(bool, Value)> {
    let path = json!({"type": "string", "description": "本机上的绝对路径（清影和这个 MCP 服务在同一台电脑上）"});
    vec![
        (
            false,
            json!({
                "name": "get_status",
                "description": "清影的运行概况：版本、各状态的下载任务数量、总速度、是否正在录制直播。",
                "inputSchema": {"type": "object", "properties": {}},
                "annotations": {"readOnlyHint": true},
            }),
        ),
        (
            false,
            json!({
                "name": "list_tasks",
                "description": "列出下载任务（编号、状态、进度、文件名）。",
                "inputSchema": {"type": "object", "properties": {
                    "status": {"type": "string", "enum": ["running", "queued", "paused", "failed", "done", "canceled"], "description": "只看某个状态"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "最多返回几条，默认 50"},
                }},
                "annotations": {"readOnlyHint": true},
            }),
        ),
        (
            false,
            json!({
                "name": "search_library",
                "description": "在清影的媒体库（已下载的内容）里搜索，返回标题、作者、平台、类型、大小、标签和来源链接。不返回本机文件路径。",
                "inputSchema": {"type": "object", "properties": {
                    "query": {"type": "string", "description": "关键词，留空则按最近完成排序"},
                    "kind": {"type": "string", "description": "按类型筛选，如 video / audio / image"},
                    "tag": {"type": "string", "description": "按标签筛选"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "默认 30"},
                }},
                "annotations": {"readOnlyHint": true},
            }),
        ),
        (
            false,
            json!({
                "name": "get_subtitle_job",
                "description": "查询字幕任务（generate_subtitle / correct_subtitle 返回的编号）的进度和结果。",
                "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]},
                "annotations": {"readOnlyHint": true},
            }),
        ),
        (
            true,
            json!({
                "name": "add_download",
                "description": "把一个或多个视频 / 图片 / 音频页面链接交给清影下载。清影会自己解析并按设置选择清晰度、保存位置；需要登录的网站要先在清影里同步登录状态。",
                "inputSchema": {"type": "object", "properties": {
                    "urls": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 50, "description": "链接列表"},
                }, "required": ["urls"]},
                "annotations": {"readOnlyHint": false, "destructiveHint": false, "openWorldHint": true},
            }),
        ),
        (
            true,
            json!({
                "name": "pause_tasks",
                "description": "暂停下载任务；不传 id 就暂停全部。",
                "inputSchema": {"type": "object", "properties": {"id": {"type": "integer", "description": "任务编号"}}},
                "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true},
            }),
        ),
        (
            true,
            json!({
                "name": "resume_tasks",
                "description": "继续下载任务；不传 id 就继续全部。",
                "inputSchema": {"type": "object", "properties": {"id": {"type": "integer", "description": "任务编号"}}},
                "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true},
            }),
        ),
        (
            true,
            json!({
                "name": "cancel_task",
                "description": "取消一个下载任务（任务停止，已下载的内容按清影的设置处理；不会删除媒体库里已完成的文件）。",
                "inputSchema": {"type": "object", "properties": {"id": {"type": "integer", "description": "任务编号"}}, "required": ["id"]},
                "annotations": {"readOnlyHint": false, "destructiveHint": true, "idempotentHint": true},
            }),
        ),
        (
            true,
            json!({
                "name": "generate_subtitle",
                "description": "给没有字幕的视频 / 音频生成字幕（语音识别 → 纠错 → 断句 → 可选翻译 → 导出）。异步执行：返回任务编号，用 get_subtitle_job 查进度。字幕文件保存在原文件旁边。",
                "inputSchema": {"type": "object", "properties": {
                    "path": path,
                    "language": {"type": "string", "description": "音频语言，如 en / zh；不填则自动判断"},
                    "translate_to": {"type": "array", "items": {"type": "string"}, "description": "要翻译成的语言，如 [\"zh\"]"},
                    "display": {"type": "string", "enum": ["orig", "trans", "orig-top", "trans-top"], "description": "双语时的排列"},
                    "formats": {"type": "array", "items": {"type": "string", "enum": ["srt", "vtt", "ass", "lrc", "txt", "json"]}, "description": "导出格式，默认 srt"},
                }, "required": ["path"]},
                "annotations": {"readOnlyHint": false, "destructiveHint": false},
            }),
        ),
        (
            true,
            json!({
                "name": "correct_subtitle",
                "description": "对已有字幕（或带字幕的视频）做 AI 纠错：补标点、改同音错字，另存为新文件，不覆盖原文件。异步执行，用 get_subtitle_job 查进度。",
                "inputSchema": {"type": "object", "properties": {
                    "path": path,
                    "mode": {"type": "string", "enum": ["conservative", "aggressive", "rules"], "description": "保守 / 激进 / 只用规则，默认保守"},
                }, "required": ["path"]},
                "annotations": {"readOnlyHint": false, "destructiveHint": false},
            }),
        ),
    ]
}

pub fn tools(read_only: bool) -> Vec<Value> {
    defs().into_iter().filter(|(write, _)| !(read_only && *write)).map(|(_, d)| d).collect()
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn id_arg(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_i64().map(Some).ok_or_else(|| format!("{key} 需要是整数")),
    }
}

fn list_arg(args: &Value, key: &str) -> Vec<Value> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(|s| Value::String(s.into())).collect())
        .unwrap_or_default()
}

/// 字幕工具的文件路径：必须是本机已存在的绝对路径（智能体常给相对路径，直接拒绝并说明）。
fn file_arg(args: &Value) -> Result<String, String> {
    let p = text_arg(args, "path").ok_or("缺少 path")?;
    let path = Path::new(p);
    if !path.is_absolute() {
        return Err("path 需要是绝对路径".into());
    }
    if !path.is_file() {
        return Err(format!("找不到文件：{p}"));
    }
    Ok(p.to_string())
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// 执行一个工具，返回给模型看的文字。
pub fn call_tool(api: &dyn Api, name: &str, args: &Value, read_only: bool) -> Result<String, String> {
    if read_only && defs().iter().any(|(w, d)| *w && d["name"] == name) {
        return Err("这个 MCP 服务以只读模式运行（--read-only），不能执行会改变状态的操作".into());
    }
    match name {
        "get_status" => api.call("GET", "/api/v1/status", None).map(|v| pretty(&v)),
        "list_tasks" => {
            let v = api.call("GET", "/api/v1/tasks", None)?;
            let status = text_arg(args, "status");
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).clamp(1, 200) as usize;
            let all = v["tasks"].as_array().cloned().unwrap_or_default();
            let matched: Vec<Value> = all
                .into_iter()
                .filter(|t| match status {
                    None => true,
                    Some(s) => t["status"].as_str() == Some(s),
                })
                .collect();
            let total = matched.len();
            Ok(pretty(&json!({"total": total, "returned": total.min(limit), "tasks": matched.into_iter().take(limit).collect::<Vec<_>>()})))
        }
        "search_library" => {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(30).clamp(1, 100);
            let mut path = format!("/api/v1/library?limit={limit}&q={}", enc(text_arg(args, "query").unwrap_or("")));
            for (key, param) in [("kind", "kind"), ("tag", "tag")] {
                if let Some(v) = text_arg(args, key) {
                    path.push_str(&format!("&{param}={}", enc(v)));
                }
            }
            api.call("GET", &path, None).map(|v| pretty(&v))
        }
        "get_subtitle_job" => {
            let id = id_arg(args, "id")?.ok_or("缺少 id")?;
            api.call("GET", &format!("/api/v1/subtitle/job?id={id}"), None).map(|v| pretty(&v))
        }
        "add_download" => {
            let urls = list_arg(args, "urls");
            if urls.is_empty() {
                return Err("urls 里至少要有一个链接".into());
            }
            let text = urls.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n");
            let v = api.call("POST", "/api/v1/add", Some(&json!({"text": text}).to_string()))?;
            Ok(format!("已交给清影解析（记录 {}）。用 list_tasks 查看下载进度。", v["id"]))
        }
        "pause_tasks" | "resume_tasks" => {
            let verb = if name == "pause_tasks" { "pause" } else { "resume" };
            let path = match id_arg(args, "id")? {
                Some(id) => format!("/api/v1/tasks/{id}/{verb}"),
                None => format!("/api/v1/tasks/{verb}_all"),
            };
            api.call("POST", &path, Some("{}")).map(|_| "完成".into())
        }
        "cancel_task" => {
            let id = id_arg(args, "id")?.ok_or("缺少 id")?;
            api.call("POST", &format!("/api/v1/tasks/{id}/cancel"), Some("{}")).map(|_| "已取消".into())
        }
        "generate_subtitle" => {
            let mut body = serde_json::Map::new();
            body.insert("path".into(), Value::String(file_arg(args)?));
            if let Some(l) = text_arg(args, "language") {
                body.insert("language".into(), l.into());
            }
            if let Some(d) = text_arg(args, "display") {
                body.insert("display".into(), d.into());
            }
            for (key, field) in [("translate_to", "targets"), ("formats", "formats")] {
                let l = list_arg(args, key);
                if !l.is_empty() {
                    body.insert(field.into(), Value::Array(l));
                }
            }
            let v = api.call("POST", "/api/v1/subtitle/generate", Some(&Value::Object(body).to_string()))?;
            Ok(format!("字幕任务已开始，编号 {}。用 get_subtitle_job 查询进度。", v["id"]))
        }
        "correct_subtitle" => {
            let mut body = serde_json::Map::new();
            body.insert("path".into(), Value::String(file_arg(args)?));
            if let Some(m) = text_arg(args, "mode") {
                body.insert("mode".into(), m.into());
            }
            let v = api.call("POST", "/api/v1/subtitle/correct", Some(&Value::Object(body).to_string()))?;
            Ok(format!("纠错任务已开始，编号 {}。用 get_subtitle_job 查询进度。", v["id"]))
        }
        other => Err(format!("没有这个工具：{other}")),
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// 处理一条消息；通知（没有 id）不回复。
pub fn handle(api: &dyn Api, msg: &Value, read_only: bool) -> Option<Value> {
    let id = msg.get("id")?; // 通知（含 notifications/initialized）没有 id
    let method = msg["method"].as_str().unwrap_or("");
    Some(match method {
        "initialize" => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = SUPPORTED.iter().find(|v| **v == asked).unwrap_or(&SUPPORTED[0]);
            reply(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "clearclip", "title": "清影 ClearClip", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "清影是一个下载器：add_download 提交链接，list_tasks 看进度，search_library 搜已下载的内容，generate_subtitle 给音视频生成字幕。清影桌面程序需要在运行，并在“设置 → 手机与浏览器扩展”里开启服务。",
                }),
            )
        }
        "ping" => reply(id, json!({})),
        "tools/list" => reply(id, json!({"tools": tools(read_only)})),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or(Value::Null);
            let args = if args.is_null() { json!({}) } else { args };
            if !args.is_object() {
                return Some(failure(id, -32602, "arguments 需要是对象"));
            }
            // 工具自己的失败（清影没开、参数不对）按协议放在结果里，让模型看到并能改正
            let (text, is_error) = match call_tool(api, name, &args, read_only) {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            reply(id, json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        _ => failure(id, -32601, &format!("不支持的方法：{method}")),
    })
}

/// 在标准输入输出上跑到输入结束，返回退出码。
pub fn serve(args: &[String]) -> i32 {
    let read_only = args.iter().any(|a| a == "--read-only");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(line) {
            Ok(msg) if msg.is_object() => handle(&Live, &msg, read_only),
            Ok(_) => Some(failure(&Value::Null, -32600, "只支持单条 JSON-RPC 消息")),
            Err(_) => Some(failure(&Value::Null, -32700, "不是有效的 JSON")),
        };
        if let Some(r) = resp {
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break; // 对端关闭
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<(String, String, Option<String>)>>,
        reply: Value,
    }

    impl Api for Fake {
        fn call(&self, method: &str, path: &str, body: Option<&str>) -> Result<Value, String> {
            self.calls.borrow_mut().push((method.into(), path.into(), body.map(String::from)));
            if self.reply.get("error").is_some() {
                return Err(self.reply["error"].as_str().unwrap().into());
            }
            Ok(self.reply.clone())
        }
    }

    fn rpc(api: &Fake, msg: Value, ro: bool) -> Value {
        handle(api, &msg, ro).expect("a reply")
    }

    #[test]
    fn handshake_and_notifications() {
        let api = Fake::default();
        let r = rpc(&api, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}), false);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "clearclip");
        // 不认识的版本回最新的
        let r = rpc(&api, json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}), false);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert!(handle(&api, &json!({"jsonrpc":"2.0","method":"notifications/initialized"}), false).is_none());
        assert_eq!(rpc(&api, json!({"jsonrpc":"2.0","id":3,"method":"ping"}), false)["result"], json!({}));
        assert_eq!(rpc(&api, json!({"jsonrpc":"2.0","id":4,"method":"nope"}), false)["error"]["code"], -32601);
        assert!(api.calls.borrow().is_empty(), "the handshake never touches the app");
    }

    #[test]
    fn read_only_hides_and_blocks_writes() {
        let api = Fake { reply: json!({"id": 5}), ..Default::default() };
        let names = |ro: bool| -> Vec<String> {
            rpc(&api, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}), ro)["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names(true), ["get_status", "list_tasks", "search_library", "get_subtitle_job"]);
        assert!(names(false).contains(&"add_download".to_string()));
        assert!(!names(false).iter().any(|n| n.contains("delete") || n.contains("remove")), "no destructive file tools");
        let r =
            rpc(&api, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_download","arguments":{"urls":["https://a.com/1"]}}}), true);
        assert_eq!(r["result"]["isError"], true);
        assert!(api.calls.borrow().is_empty(), "a blocked call never reaches the app");
    }

    #[test]
    fn every_tool_has_a_schema_and_unique_name() {
        let t = tools(false);
        let mut names: Vec<&str> = t.iter().map(|d| d["name"].as_str().unwrap()).collect();
        for d in &t {
            assert_eq!(d["inputSchema"]["type"], "object");
            assert!(!d["description"].as_str().unwrap().is_empty());
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), t.len());
    }

    #[test]
    fn tools_map_to_the_local_api() {
        let api = Fake {
            reply: json!({"id": 9, "tasks": [
                {"id": 1, "status": "done", "fileName": "a.mp4"},
                {"id": 2, "status": "running", "fileName": "b.mp4"},
                {"id": 3, "status": "running", "fileName": "c.mp4"},
            ]}),
            ..Default::default()
        };
        let call = |name: &str, args: Value| call_tool(&api, name, &args, false);
        let out = call("list_tasks", json!({"status": "running", "limit": 1})).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!((v["total"].as_u64(), v["returned"].as_u64(), v["tasks"][0]["id"].as_i64()), (Some(2), Some(1), Some(2)));

        call("search_library", json!({"query": "rust 入门", "tag": "学习", "limit": 500})).unwrap();
        call("add_download", json!({"urls": ["https://a.com/1", " https://b.com/2 "]})).unwrap();
        call("pause_tasks", json!({})).unwrap();
        call("resume_tasks", json!({"id": 7})).unwrap();
        call("cancel_task", json!({"id": 3})).unwrap();
        call("get_subtitle_job", json!({"id": 4})).unwrap();
        let calls = api.calls.borrow();
        let seen: Vec<(&str, &str)> = calls.iter().map(|(m, p, _)| (m.as_str(), p.as_str())).collect();
        assert_eq!(seen[1].1, "/api/v1/library?limit=100&q=rust+%E5%85%A5%E9%97%A8&tag=%E5%AD%A6%E4%B9%A0");
        assert_eq!(seen[2], ("POST", "/api/v1/add"));
        assert_eq!(calls[2].2.as_deref(), Some(r#"{"text":"https://a.com/1\nhttps://b.com/2"}"#));
        assert_eq!(seen[3], ("POST", "/api/v1/tasks/pause_all"));
        assert_eq!(seen[4], ("POST", "/api/v1/tasks/7/resume"));
        assert_eq!(seen[5], ("POST", "/api/v1/tasks/3/cancel"));
        assert_eq!(seen[6], ("GET", "/api/v1/subtitle/job?id=4"));
    }

    #[test]
    fn subtitle_tools_check_the_file_first() {
        let api = Fake { reply: json!({"id": 12}), ..Default::default() };
        assert!(call_tool(&api, "generate_subtitle", &json!({"path": "relative/a.mp4"}), false).unwrap_err().contains("绝对路径"));
        assert!(call_tool(&api, "generate_subtitle", &json!({}), false).is_err());
        let f = std::env::temp_dir().join("mcp-sub-test.mp4");
        std::fs::write(&f, b"x").unwrap();
        let p = f.to_string_lossy().to_string();
        assert!(call_tool(&api, "generate_subtitle", &json!({"path": p + ".missing"}), false).unwrap_err().contains("找不到"));
        assert!(api.calls.borrow().is_empty(), "bad paths never reach the app");
        let out = call_tool(
            &api,
            "generate_subtitle",
            &json!({"path": f.to_string_lossy(), "language": "en", "translate_to": ["zh"], "formats": ["srt", "vtt"]}),
            false,
        )
        .unwrap();
        assert!(out.contains("12"));
        let body: Value = serde_json::from_str(api.calls.borrow()[0].2.as_deref().unwrap()).unwrap();
        assert_eq!((body["language"].as_str(), body["targets"].clone(), body["formats"].clone()), (Some("en"), json!(["zh"]), json!(["srt", "vtt"])));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn app_errors_come_back_as_tool_errors() {
        let api = Fake { reply: json!({"error": "还没有开启本机接口"}), ..Default::default() };
        let r = rpc(&api, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_status"}}), false);
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("本机接口"));
        let r = rpc(&api, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_status","arguments":[1]}}), false);
        assert_eq!(r["error"]["code"], -32602);
        let r = rpc(&api, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"bogus"}}), false);
        assert_eq!(r["result"]["isError"], true);
    }
}
