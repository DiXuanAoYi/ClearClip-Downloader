//! 命令行：同一个程序既是桌面应用也是命令行工具。
//!
//! - `clearclip add <链接…>`：交给正在运行的清影（没有运行就启动它）下载，不需要开启本机接口
//! - `clearclip list | status | pause [编号|all] | resume [编号|all] | cancel <编号> | remove <编号> | search <关键词>`：
//!   通过本机接口控制正在运行的清影（需要在“设置 → 手机与浏览器扩展”里开启，令牌从设置文件里读取）

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

const ID: &str = "com.dixuanaoyi.clearclip";

pub const HELP: &str = "清影 ClearClip 命令行

用法：
  clearclip add <链接…>        加入下载（清影没在运行时会先启动）
  clearclip list               列出下载任务
  clearclip status             显示下载概况
  clearclip pause [编号|all]   暂停任务（默认全部）
  clearclip resume [编号|all]  继续任务（默认全部）
  clearclip cancel <编号>      取消任务
  clearclip remove <编号>      从列表移除任务
  clearclip search <关键词>    搜索媒体库
  clearclip health             检查各平台解析是否正常（最多等 5 分钟；有不正常的平台退出码为 2）
  clearclip subtitle generate <文件> [--lang 语言] [--translate zh,en] [--display orig|trans|orig-top|trans-top]
                               [--format srt,vtt,ass,lrc,txt,json] [--correct auto|conservative|aggressive|rules|off] [--no-resegment] [--semantic] [--reflect] [--word-timing]
                               给没有字幕的视频 / 音频生成字幕（识别 → 纠错 → 断句 → 翻译 → 导出）
  clearclip subtitle correct <字幕或视频文件> [--mode conservative|aggressive|rules]
                               对已有字幕做 AI 纠错（补标点、改同音错字），另存为新文件
  clearclip help               显示这份说明

除 add 外的命令需要先在清影的“设置 → 手机与浏览器扩展”里开启服务。加 --json 输出原始 JSON。";

/// 设置文件所在目录（和 Tauri 的 app_config_dir 一致）。
pub fn config_dir() -> Option<PathBuf> {
    if let Some(p) = crate::portable_dir() {
        return Some(p);
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join(ID))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support").join(ID))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join(ID))
    }
}

/// 从命令行参数里取出要添加的链接文字（`add <链接…>` 或 `--add <链接…>`）。
pub fn add_text(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "add" || a == "--add")?;
    // “add” 必须是第一个参数（程序名之后），避免误把别的参数当成命令
    if i > 1 {
        return None;
    }
    let rest: Vec<&str> = args[i + 1..].iter().map(String::as_str).filter(|a| !a.starts_with("--")).collect();
    (!rest.is_empty()).then(|| rest.join("\n"))
}

#[derive(Debug, PartialEq)]
pub enum Cmd {
    Help,
    Status,
    List,
    Pause(Option<i64>),
    Resume(Option<i64>),
    Cancel(i64),
    Remove(i64),
    Search(String),
    /// 平台健康检查：提交以后每 2 秒查一次，最多等 5 分钟；有平台不正常时退出码是 2
    Health,
    /// 字幕工坊：(接口路径, 请求体 JSON)
    Subtitle(&'static str, String),
}

/// 解析命令；`add` 和其他不认识的返回 None（交给桌面应用处理）。
pub fn parse(args: &[String]) -> Option<Result<Cmd, String>> {
    let cmd = args.get(1)?.as_str();
    let rest: Vec<&str> = args[2..].iter().map(String::as_str).filter(|a| *a != "--json").collect();
    let id = |s: Option<&&str>| s.and_then(|v| v.parse::<i64>().ok());
    let all_or_id = |what: &str| -> Result<Option<i64>, String> {
        match rest.first() {
            None | Some(&"all") => Ok(None),
            Some(v) => v.parse().map(Some).map_err(|_| format!("“{v}”不是有效的任务编号，用 clearclip list 查看（{what}）")),
        }
    };
    Some(match cmd {
        "help" | "--help" | "-h" => Ok(Cmd::Help),
        "status" => Ok(Cmd::Status),
        "list" | "ls" => Ok(Cmd::List),
        "health" => Ok(Cmd::Health),
        "pause" => all_or_id("暂停").map(Cmd::Pause),
        "resume" => all_or_id("继续").map(Cmd::Resume),
        "cancel" => id(rest.first()).map(Cmd::Cancel).ok_or_else(|| "用法：clearclip cancel <任务编号>".to_string()),
        "remove" | "rm" => id(rest.first()).map(Cmd::Remove).ok_or_else(|| "用法：clearclip remove <任务编号>".to_string()),
        "subtitle" | "sub" => parse_subtitle(&args[2..]).map(|(p, b)| Cmd::Subtitle(p, b)),
        "search" => {
            if rest.is_empty() {
                Err("用法：clearclip search <关键词>".into())
            } else {
                Ok(Cmd::Search(rest.join(" ")))
            }
        }
        // add 和 --autostart 之类的启动参数交给桌面应用；其他不认识的单词按拼错的命令处理，不要悄悄启动界面
        "add" => return None,
        c if c.starts_with('-') => return None,
        c => Err(format!("不认识的命令“{c}”。用 clearclip help 查看可用命令。")),
    })
}

fn http(port: u16, token: &str, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|_| "连接不上清影。请确认它正在运行，并且在“设置 → 手机与浏览器扩展”里开启了服务。".to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let body = body.unwrap_or("");
    let req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let status = raw.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    Ok((status, raw.split_once("\r\n\r\n").map(|(_, b)| b.to_string()).unwrap_or_default()))
}

fn size(n: u64) -> String {
    let u = ["B", "KB", "MB", "GB"];
    let (mut v, mut i) = (n as f64, 0);
    while v >= 1024.0 && i < 3 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", u[i])
    }
}

fn status_name(s: &str) -> &str {
    match s {
        "running" => "下载中",
        "queued" => "等待中",
        "paused" => "已暂停",
        "failed" => "失败",
        "done" => "已完成",
        "canceled" => "已取消",
        other => other,
    }
}

/// 按终端显示宽度补空格（中文字符占两格）。
fn pad(s: &str, width: usize) -> String {
    let w: usize = s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
    format!("{s}{}", " ".repeat(width.saturating_sub(w)))
}

/// 把接口返回的内容整理成适合终端阅读的文字。
/// `subtitle generate|correct <文件> [选项]` → (接口路径, JSON)。文件转成绝对路径（命令行的当前目录和清影不同）。
fn parse_subtitle(args: &[String]) -> Result<(&'static str, String), String> {
    const USAGE: &str = "用法：clearclip subtitle generate|correct <文件> [选项]，clearclip help 查看全部选项";
    let sub = args.first().map(String::as_str).ok_or(USAGE)?;
    let path = match sub {
        "generate" | "gen" => "/api/v1/subtitle/generate",
        "correct" | "fix" => "/api/v1/subtitle/correct",
        _ => return Err(USAGE.into()),
    };
    let mut file: Option<String> = None;
    let mut body = serde_json::Map::new();
    let list = |v: &str| Value::Array(v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| Value::String(s.into())).collect());
    let mut it = args[1..].iter().filter(|a| a.as_str() != "--json");
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} 后面需要一个值"));
        match a.as_str() {
            "--lang" => {
                body.insert("language".into(), Value::String(val("--lang")?));
            }
            "--translate" => {
                body.insert("targets".into(), list(&val("--translate")?));
            }
            "--display" => {
                body.insert("display".into(), Value::String(val("--display")?));
            }
            "--format" => {
                body.insert("formats".into(), list(&val("--format")?));
            }
            "--correct" => {
                body.insert("correct".into(), Value::String(val("--correct")?));
            }
            "--mode" => {
                body.insert("mode".into(), Value::String(val("--mode")?));
            }
            "--no-resegment" => {
                body.insert("resegment".into(), Value::Bool(false));
            }
            "--semantic" => {
                body.insert("semantic".into(), Value::Bool(true));
            }
            "--word-timing" => {
                body.insert("wordTiming".into(), Value::Bool(true));
            }
            "--reflect" => {
                body.insert("reflect".into(), Value::Bool(true));
            }
            f if f.starts_with("--") => return Err(format!("不认识的选项“{f}”")),
            f => file = Some(f.to_string()),
        }
    }
    let file = file.ok_or(USAGE)?;
    let abs = std::fs::canonicalize(&file).map_err(|_| format!("找不到文件“{file}”"))?;
    body.insert("path".into(), Value::String(abs.to_string_lossy().trim_start_matches(r"\\?\").to_string()));
    Ok((path, Value::Object(body).to_string()))
}

pub fn render(cmd: &Cmd, body: &Value) -> String {
    match cmd {
        Cmd::Status => {
            let t = &body["tasks"];
            format!(
                "清影 {}\n下载中 {} · 等待 {} · 暂停 {} · 失败 {} · 已完成 {} · 速度 {}/s{}",
                body["version"].as_str().unwrap_or(""),
                t["running"],
                t["queued"],
                t["paused"],
                t["failed"],
                t["done"],
                size(t["speed"].as_u64().unwrap_or(0)),
                if body["recording"].as_bool() == Some(true) { " · 正在录制直播" } else { "" }
            )
        }
        Cmd::List => {
            let tasks = body["tasks"].as_array().cloned().unwrap_or_default();
            if tasks.is_empty() {
                return "没有任务".into();
            }
            tasks
                .iter()
                .map(|t| {
                    let (rec, total) = (t["received"].as_u64().unwrap_or(0), t["total"].as_u64());
                    let pct = total.filter(|t| *t > 0).map(|t| format!(" {}%", (rec * 100 / t).min(100))).unwrap_or_default();
                    format!(
                        "{:>4}  {} {}{}  {}",
                        t["id"].as_i64().unwrap_or(0),
                        pad(status_name(t["status"].as_str().unwrap_or("")), 8),
                        size(rec),
                        pct,
                        t["fileName"].as_str().filter(|n| !n.is_empty()).or_else(|| t["title"].as_str()).unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Cmd::Subtitle(..) => {
            format!("已开始，任务编号 {}。在清影的“工具箱”任务列表里查看进度，或用 GET /api/v1/subtitle/job?id={} 查询。", body["id"], body["id"])
        }
        Cmd::Search(_) => {
            let items = body["items"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                return "没有结果".into();
            }
            items
                .iter()
                .map(|i| {
                    let tags =
                        i["tags"].as_array().map(|t| t.iter().filter_map(|x| x.as_str()).map(|x| format!(" #{x}")).collect::<String>()).unwrap_or_default();
                    format!(
                        "{}{}  [{}] {} · {}{}",
                        if i["favorite"].as_bool() == Some(true) { "★ " } else { "" },
                        i["title"].as_str().unwrap_or(""),
                        i["platform"].as_str().unwrap_or(""),
                        i["kind"].as_str().unwrap_or(""),
                        size(i["size"].as_u64().unwrap_or(0)),
                        tags
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => "完成".into(),
    }
}

/// 提交一次平台健康检查并等结果。返回的文字里每个平台一行；有不正常的平台时返回 `Err("…")` 不合适（那样会丢掉输出），
/// 所以全部通过与否写在最后一行，退出码由 `try_run` 看 `HEALTH_FAILED` 决定。
static HEALTH_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn run_health_cmd(port: u16, token: &str, json: bool) -> Result<String, String> {
    let call = |method: &str, path: &str| -> Result<Value, String> {
        let (code, body) = http(port, token, method, path, None)?;
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if code != 200 {
            return Err(v["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("接口返回 {code}")));
        }
        Ok(v)
    };
    let id = call("POST", "/api/v1/health/run")?["id"].as_u64().ok_or("接口没有返回任务编号")?;
    let started = std::time::Instant::now();
    loop {
        let v = call("GET", &format!("/api/v1/health/job?id={id}"))?;
        match v["state"].as_str() {
            Some("done") => {
                if v["allOk"].as_bool() != Some(true) {
                    HEALTH_FAILED.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                return Ok(if json { serde_json::to_string_pretty(&v).unwrap_or_default() } else { render_health(&v) });
            }
            Some("failed") => return Err(v["error"].as_str().unwrap_or("体检失败").to_string()),
            _ => {}
        }
        if started.elapsed() > Duration::from_secs(300) {
            return Err("等了 5 分钟还没有结果，体检仍在后台进行；稍后再运行 clearclip health".into());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// 体检结果的文字版：每个平台一行，最后一行是汇总。
pub fn render_health(v: &Value) -> String {
    let rows = v["results"].as_array().cloned().unwrap_or_default();
    let mut lines: Vec<String> = rows
        .iter()
        .map(|r| {
            let ok = r["ok"].as_bool() == Some(true);
            format!(
                "{} {}  {} 毫秒  {}",
                if ok { "✓" } else { "✗" },
                r["name"].as_str().unwrap_or(""),
                r["millis"].as_u64().unwrap_or(0),
                r["message"].as_str().unwrap_or("")
            )
        })
        .collect();
    let bad = rows.iter().filter(|r| r["ok"].as_bool() != Some(true)).count();
    lines.push(if bad == 0 { format!("全部正常（{} 项）", rows.len()) } else { format!("{bad} 项不正常（共 {} 项）", rows.len()) });
    lines.join("\n")
}

fn run_cmd(cmd: &Cmd, json: bool) -> Result<String, String> {
    let dir = config_dir().ok_or("找不到清影的设置目录")?;
    let text = std::fs::read_to_string(dir.join("settings.json")).map_err(|_| "没有找到清影的设置文件，请先启动一次清影。".to_string())?;
    let settings: Value = serde_json::from_str(&text).map_err(|_| "设置文件已损坏".to_string())?;
    let phone = &settings["phone"];
    let (port, token) = (phone["port"].as_u64().unwrap_or(0) as u16, phone["token"].as_str().unwrap_or(""));
    if phone["enabled"].as_bool() != Some(true) || port == 0 || token.is_empty() {
        return Err("还没有开启本机接口：请在清影的“设置 → 手机与浏览器扩展”里打开服务。".into());
    }
    let mut post_body: Option<&str> = None;
    let (method, path): (&str, String) = match cmd {
        Cmd::Subtitle(p, b) => {
            post_body = Some(b.as_str());
            ("POST", (*p).to_string())
        }
        Cmd::Help => return Ok(HELP.into()),
        Cmd::Health => return run_health_cmd(port, token, json),
        Cmd::Status => ("GET", "/api/v1/status".into()),
        Cmd::List => ("GET", "/api/v1/tasks".into()),
        Cmd::Pause(None) => ("POST", "/api/v1/tasks/pause_all".into()),
        Cmd::Resume(None) => ("POST", "/api/v1/tasks/resume_all".into()),
        Cmd::Pause(Some(id)) => ("POST", format!("/api/v1/tasks/{id}/pause")),
        Cmd::Resume(Some(id)) => ("POST", format!("/api/v1/tasks/{id}/resume")),
        Cmd::Cancel(id) => ("POST", format!("/api/v1/tasks/{id}/cancel")),
        Cmd::Remove(id) => ("POST", format!("/api/v1/tasks/{id}/remove")),
        Cmd::Search(q) => ("GET", format!("/api/v1/library?limit=30&q={}", url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>())),
    };
    let (code, body) = http(port, token, method, &path, post_body)?;
    let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    if code != 200 {
        return Err(v["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("接口返回 {code}")));
    }
    Ok(if json { serde_json::to_string_pretty(&v).unwrap_or(body) } else { render(cmd, &v) })
}

#[cfg(windows)]
fn attach_console() {
    // 发布版是窗口程序，没有控制台；命令行模式下接到启动它的终端上，才能输出文字
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// 命令行模式：是命令就执行并返回退出码；不是命令（正常启动桌面应用）返回 None。
pub fn try_run() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    let parsed = parse(&args)?;
    #[cfg(windows)]
    attach_console();
    let json = args.iter().any(|a| a == "--json");
    Some(match parsed.and_then(|cmd| run_cmd(&cmd, json)) {
        Ok(out) => {
            println!("{out}");
            if HEALTH_FAILED.load(std::sync::atomic::Ordering::SeqCst) {
                2
            } else {
                0
            }
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        std::iter::once("clearclip").chain(v.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn commands_are_parsed() {
        assert_eq!(parse(&a(&["list"])), Some(Ok(Cmd::List)));
        assert_eq!(parse(&a(&["pause"])), Some(Ok(Cmd::Pause(None))));
        assert_eq!(parse(&a(&["pause", "all"])), Some(Ok(Cmd::Pause(None))));
        assert_eq!(parse(&a(&["resume", "7"])), Some(Ok(Cmd::Resume(Some(7)))));
        assert!(matches!(parse(&a(&["pause", "x"])), Some(Err(_))));
        assert_eq!(parse(&a(&["cancel", "3", "--json"])), Some(Ok(Cmd::Cancel(3))));
        assert!(matches!(parse(&a(&["cancel"])), Some(Err(_))));
        assert_eq!(parse(&a(&["search", "rust", "入门"])), Some(Ok(Cmd::Search("rust 入门".into()))));
        assert_eq!(parse(&a(&["health"])), Some(Ok(Cmd::Health)));
        assert_eq!(parse(&a(&["health", "--json"])), Some(Ok(Cmd::Health)));
        assert_eq!(parse(&a(&["--help"])), Some(Ok(Cmd::Help)));
        // 字幕命令：文件转绝对路径，选项进 JSON
        let f = std::env::temp_dir().join("cli-sub-test.mp4");
        std::fs::write(&f, b"x").unwrap();
        let fs = f.to_string_lossy().to_string();
        match parse(&a(&["subtitle", "generate", &fs, "--translate", "zh,en", "--format", "vtt", "--no-resegment", "--semantic", "--reflect"])) {
            Some(Ok(Cmd::Subtitle(p, b))) => {
                assert_eq!(p, "/api/v1/subtitle/generate");
                let v: Value = serde_json::from_str(&b).unwrap();
                assert_eq!(v["targets"], serde_json::json!(["zh", "en"]));
                assert_eq!(v["formats"], serde_json::json!(["vtt"]));
                assert_eq!(v["resegment"], false);
                assert_eq!(v["semantic"], true);
                assert_eq!(v["reflect"], true);
                assert!(v["path"].as_str().unwrap().ends_with("cli-sub-test.mp4"));
            }
            o => panic!("{o:?}"),
        }
        assert!(matches!(parse(&a(&["subtitle", "generate"])), Some(Err(_))));
        assert!(matches!(parse(&a(&["subtitle", "correct", &fs, "--bogus"])), Some(Err(_))));
        std::fs::remove_file(&f).ok();
        // add 和启动参数交给桌面应用
        assert_eq!(parse(&a(&["add", "https://x.com/1"])), None);
        assert_eq!(parse(&a(&["--autostart"])), None);
        assert_eq!(parse(&a(&[])), None);
        assert!(matches!(parse(&a(&["lst"])), Some(Err(e)) if e.contains("lst")), "typos print an error instead of opening the window");
    }

    #[test]
    fn add_urls_are_collected() {
        assert_eq!(add_text(&a(&["add", "https://a.com/1", "https://b.com/2"])).as_deref(), Some("https://a.com/1\nhttps://b.com/2"));
        assert_eq!(add_text(&a(&["--add", "https://a.com/1"])).as_deref(), Some("https://a.com/1"));
        assert_eq!(add_text(&a(&["add"])), None);
        assert_eq!(add_text(&a(&["--autostart"])), None);
        assert_eq!(add_text(&a(&["--autostart", "x", "add", "y"])), None, "add must come first");
    }

    #[test]
    fn output_is_readable() {
        let body = serde_json::json!({"tasks": [
            {"id": 5, "status": "running", "received": 5_242_880, "total": 10_485_760, "title": "视频 A"},
            {"id": 6, "status": "failed", "received": 0, "total": null, "title": "视频 B", "fileName": "作者_视频 B_20241002.mp4"}]});
        let out = render(&Cmd::List, &body);
        assert!(out.contains("   5  下载中   5.0 MB 50%  视频 A"), "{out}");
        assert!(out.contains("   6  失败     0 B  作者_视频 B_20241002.mp4"), "{out}");
        assert_eq!(render(&Cmd::List, &serde_json::json!({"tasks": []})), "没有任务");
        let st = render(
            &Cmd::Status,
            &serde_json::json!({"version": "0.2.0", "tasks": {"running": 1, "queued": 2, "paused": 0, "failed": 0, "done": 9, "speed": 2_097_152}, "recording": true}),
        );
        assert!(st.contains("下载中 1 · 等待 2") && st.contains("2.0 MB/s") && st.contains("正在录制直播"), "{st}");
        let s = render(
            &Cmd::Search("x".into()),
            &serde_json::json!({"items": [{"title": "T", "platform": "B站", "kind": "video", "size": 1024, "favorite": true, "tags": ["学习"]}]}),
        );
        assert_eq!(s, "★ T  [B站] video · 1.0 KB #学习");
    }

    #[test]
    fn health_results_are_rendered_one_line_per_platform_with_a_summary() {
        let v = serde_json::json!({"state": "done", "allOk": false, "results": [
            {"name": "B站", "ok": true, "millis": 820, "message": "正常：某视频（3 个资源）"},
            {"name": "抖音", "ok": false, "millis": 30000, "message": "超时"}]});
        let t = render_health(&v);
        assert_eq!(t.lines().count(), 3);
        assert!(t.contains("✓ B站  820 毫秒") && t.contains("✗ 抖音  30000 毫秒  超时"), "{t}");
        assert!(t.ends_with("1 项不正常（共 2 项）"), "{t}");
        let ok = serde_json::json!({"results": [{"name": "A", "ok": true, "millis": 1, "message": "好"}]});
        assert!(render_health(&ok).ends_with("全部正常（1 项）"));
    }
}
