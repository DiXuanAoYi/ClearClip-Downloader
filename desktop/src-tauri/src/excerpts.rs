//! 摘录：在任意软件里选中文字后“存入摘录”，记下文字、来源程序和时间。桌面端没法给别的软件里的文字画高亮，用摘录代替。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Excerpt {
    pub id: String,
    pub text: String,
    /// 来源程序（进程名）或网页标题
    pub app: String,
    pub note: String,
    pub created: i64,
}

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
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::invalid("没有可以摘录的文字。"));
    }
    let mut all = list(dir);
    if let Some(e) = all.iter().find(|e| e.text == text) {
        return Ok(e.clone());
    }
    let created = now();
    let e = Excerpt {
        id: format!("{created:x}{:03x}", all.len() % 4096),
        text: text.chars().take(20_000).collect(),
        app: app.chars().take(120).collect(),
        note: String::new(),
        created,
    };
    all.push(e.clone());
    if all.len() > 2000 {
        all.drain(..all.len() - 2000);
    }
    save(dir, &all)?;
    Ok(e)
}

pub fn remove(dir: &Path, id: &str) -> AppResult<()> {
    let mut all = list(dir);
    all.retain(|e| e.id != id);
    save(dir, &all)
}

pub fn set_note(dir: &Path, id: &str, note: &str) -> AppResult<()> {
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

#[tauri::command]
pub fn excerpt_markdown(state: St<'_>) -> String {
    to_markdown(&list(&state.data_dir))
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
}
