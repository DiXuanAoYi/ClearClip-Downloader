//! 摘录：清影里所有“记下这段文字”的地方共用这一份——
//! 桌面写作窗的“存入摘录”、截图翻译历史的“存进摘录”、浏览器扩展的“摘录选中文字到清影”和各处“存为笔记”。
//! 记下文字、来源（程序名或网页标题）、网址和时间；主窗口“生词与摘录 → 摘录”里看。
//! 以前扩展存的是“收到的文件”里的文字便签，第一次启动时搬过来（migrate_browser_notes）。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

/// 摘录变了（浏览器扩展存进来一条）：主窗口刷新列表。
pub const EVT_CHANGED: &str = "excerpts://changed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Excerpt {
    pub id: String,
    pub text: String,
    /// 来源程序（进程名）或网页标题
    pub app: String,
    pub note: String,
    pub created: i64,
    /// 网页地址（浏览器里存的才有）
    #[serde(default)]
    pub url: String,
}

/// 读写 excerpts.json 时加锁（写作窗、主窗口、浏览器扩展可能同时存）。
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// 一条最长多少字（扩展可以把整份学习稿存进来）
const MAX_CHARS: usize = 200_000;

fn file(dir: &Path) -> PathBuf {
    dir.join("excerpts.json")
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn list(dir: &Path) -> Vec<Excerpt> {
    std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, v: &[Excerpt]) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(file(dir), serde_json::to_vec_pretty(v)?)?;
    Ok(())
}

/// 存入一条；同样的文字已经存过就不重复。最多留 2000 条。
pub fn add(dir: &Path, text: &str, app: &str) -> AppResult<Excerpt> {
    add_full(dir, text, app, "", None)
}

/// 浏览器扩展存的文字末尾常带“来源：标题\n网址”：拆成正文、标题、网址。
pub fn split_source(text: &str) -> (String, String, String) {
    let t = text.trim();
    let lines: Vec<&str> = t.lines().collect();
    let n = lines.len();
    if n >= 2 {
        let url = lines[n - 1].trim();
        let src = lines[n - 2].trim();
        if url.starts_with("http") && !url.contains(char::is_whitespace) {
            if let Some(title) = src.strip_prefix("来源：").or_else(|| src.strip_prefix("—— ")).or_else(|| (src == "——").then_some("")) {
                let body = lines[..n - 2].join("\n").trim().to_string();
                return (body, title.trim().to_string(), url.to_string());
            }
        }
    }
    (t.to_string(), String::new(), String::new())
}

/// 存入一条（带网址、可指定时间）。
pub fn add_full(dir: &Path, text: &str, app: &str, url: &str, created: Option<i64>) -> AppResult<Excerpt> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::invalid("没有可以摘录的文字。"));
    }
    let mut all = list(dir);
    if let Some(e) = all.iter().find(|e| e.text == text) {
        return Ok(e.clone());
    }
    let created = created.unwrap_or_else(now);
    let e = Excerpt {
        id: format!("{created:x}{:03x}", all.len() % 4096),
        text: text.chars().take(MAX_CHARS).collect(),
        app: app.chars().take(120).collect(),
        note: String::new(),
        created,
        url: url.chars().take(2000).collect(),
    };
    all.push(e.clone());
    all.sort_by_key(|x| x.created);
    if all.len() > 2000 {
        all.drain(..all.len() - 2000);
    }
    save(dir, &all)?;
    Ok(e)
}

pub fn remove(dir: &Path, id: &str) -> AppResult<()> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = list(dir);
    all.retain(|e| e.id != id);
    save(dir, &all)
}

pub fn set_note(dir: &Path, id: &str, note: &str) -> AppResult<()> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = list(dir);
    for e in &mut all {
        if e.id == id {
            e.note = note.trim().chars().take(500).collect();
        }
    }
    save(dir, &all)
}

pub fn to_markdown(all: &[Excerpt]) -> String {
    let mut s = String::from("# 摘录\n\n");
    for e in all.iter().rev() {
        s += &format!("- {}", e.text.split_whitespace().collect::<Vec<_>>().join(" "));
        if !e.app.is_empty() {
            s += &format!("（{}）", e.app);
        }
        if !e.url.is_empty() {
            s += &format!(" <{}>", e.url);
        }
        s.push('\n');
        if !e.note.is_empty() {
            s += &format!("  - 笔记：{}\n", e.note);
        }
    }
    s
}

#[tauri::command]
pub fn excerpt_list(state: St<'_>) -> Vec<Excerpt> {
    let mut v = list(&state.data_dir);
    v.reverse();
    v
}

#[tauri::command]
pub fn excerpt_add(state: St<'_>, text: String, app: Option<String>) -> AppResult<Excerpt> {
    add(&state.data_dir, &text, app.as_deref().unwrap_or(""))
}

#[tauri::command]
pub fn excerpt_remove(state: St<'_>, id: String) -> AppResult<()> {
    remove(&state.data_dir, &id)
}

#[tauri::command]
pub fn excerpt_note(state: St<'_>, id: String, note: String) -> AppResult<()> {
    set_note(&state.data_dir, &id, &note)
}

/// 浏览器扩展发来的“摘录 / 存为笔记”：存进摘录（以前是“收到的文件”里的文字便签）。
pub fn add_from_browser(dir: &Path, text: &str, browser: &str) -> AppResult<Excerpt> {
    let (body, title, url) = split_source(text);
    let app = if title.is_empty() { browser.to_string() } else { title };
    add_full(dir, if body.is_empty() { text } else { &body }, &app, &url, None)
}

/// 一次性：把以前扩展存成“收到的文件”文字便签的那些搬进摘录（浏览器扩展发来的文字，设备名以“浏览器扩展”结尾）。
/// 搬完删掉原来那条，做个标记，以后不再搬。返回搬了几条。
pub fn migrate_browser_notes(dir: &Path, db: &crate::db::Db) -> usize {
    let mark = dir.join("excerpts-migrated");
    if mark.exists() {
        return 0;
    }
    let mut n = 0;
    if let Ok(items) = db.transfer_list("in", 100_000) {
        for t in items.iter().filter(|t| t.kind == "text" && t.device_name.ends_with("浏览器扩展") && !t.text.trim().is_empty()) {
            let (body, title, url) = split_source(&t.text);
            let app = if title.is_empty() { t.device_name.clone() } else { title };
            if add_full(dir, if body.is_empty() { &t.text } else { &body }, &app, &url, Some(t.created_at)).is_ok() {
                let _ = db.transfer_remove(t.id);
                n += 1;
            }
        }
    }
    let _ = std::fs::write(&mark, n.to_string());
    n
}

#[tauri::command]
pub fn excerpt_markdown(state: St<'_>) -> String {
    to_markdown(&list(&state.data_dir))
}

/// 把全部摘录导出成 Markdown 文件。
#[tauri::command]
pub fn excerpt_export(state: St<'_>, dest: String) -> AppResult<()> {
    std::fs::write(&dest, to_markdown(&list(&state.data_dir)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedupes_notes_and_exports() {
        let dir = std::env::temp_dir().join(format!("cc-excerpt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(add(&dir, "   ", "x").is_err());
        let a = add(&dir, "该协议要求双方交换证书", "winword.exe").unwrap();
        let again = add(&dir, "  该协议要求双方交换证书 ", "other").unwrap();
        assert_eq!(a.id, again.id, "同样的文字不重复存");
        let b = add(&dir, "第二条\n换行", "").unwrap();
        set_note(&dir, &b.id, " 要核实 ").unwrap();
        let all = list(&dir);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].note, "要核实");
        let md = to_markdown(&all);
        assert!(md.starts_with("# 摘录\n\n- 第二条 换行\n  - 笔记：要核实\n- 该协议要求双方交换证书（winword.exe）\n"), "{md}");
        remove(&dir, &a.id).unwrap();
        assert_eq!(list(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn browser_notes_go_into_excerpts_and_old_ones_are_moved_once() {
        let dir = std::env::temp_dir().join(format!("cc-excerpt-br-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 扩展存的文字末尾带“来源：标题 + 网址”：拆出来
        let (body, title, url) = split_source("resilient\n\n翻译：\n有韧性的\n\n来源：Demo 页面\nhttps://a.test/x?y=1");
        assert_eq!((body.as_str(), title.as_str(), url.as_str()), ("resilient\n\n翻译：\n有韧性的", "Demo 页面", "https://a.test/x?y=1"));
        assert_eq!(split_source("没有来源的一段").1, "");
        // 右键“摘录选中文字到清影”的格式：原文 + “—— 标题” + 网址
        assert_eq!(split_source("原文一句\n\n—— 标题\nhttps://c.test/"), ("原文一句".to_string(), "标题".to_string(), "https://c.test/".to_string()));
        let e = add_from_browser(&dir, "一段话\n\n来源：Some Title\nhttps://b.test/", "Chrome 浏览器扩展").unwrap();
        assert_eq!((e.app.as_str(), e.url.as_str(), e.text.as_str()), ("Some Title", "https://b.test/", "一段话"));
        // 以前存成“收到的文件”便签的：浏览器扩展发来的搬进摘录（保留原来的时间），手机发来的不动；只搬一次
        let db = crate::db::Db::open_in_memory().unwrap();
        fn mk<'a>(text: &'a str, dev: &'a str) -> crate::transfer::NewTransfer<'a> {
            crate::transfer::NewTransfer {
                direction: "in",
                kind: "text",
                name: "n",
                path: "",
                text,
                size: 1,
                status: "done",
                key: "",
                device: "d",
                device_name: dev,
                expires_at: 0,
            }
        }
        db.transfer_add(&mk("旧摘录\n\n来源：Old\nhttps://old.test/", "Edge 浏览器扩展")).unwrap();
        db.transfer_add(&mk("手机发来的文字", "我的手机")).unwrap();
        assert_eq!(migrate_browser_notes(&dir, &db), 1);
        let all = list(&dir);
        assert!(all.iter().any(|x| x.text == "旧摘录" && x.app == "Old" && x.url == "https://old.test/"));
        let left = db.transfer_list("in", 10).unwrap();
        assert_eq!(left.len(), 1, "手机发来的便签留在收到的文件里");
        assert_eq!(left[0].device_name, "我的手机");
        db.transfer_add(&mk("再来一条", "Edge 浏览器扩展")).unwrap();
        assert_eq!(migrate_browser_notes(&dir, &db), 0, "只搬一次");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
