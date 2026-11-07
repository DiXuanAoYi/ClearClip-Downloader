//! 翻译记忆：所有翻译入口（网页、字幕、文件、屏幕、截图后的重译）共用一份“原文 → 译文”。
//!
//! - 键：档案（网页 / 字幕 / 界面 / 文档）+ 目标语言代码 + 术语版本 + 原文。同一句话在网页里译过，字幕里遇到就直接用。
//! - 人工改过的译文（文件翻译阅读器里改的）记为 edited，不受术语版本影响，优先于机器译文。
//! - 换了术语库或 AI 专家（术语版本变了）就不再命中旧的机器译文；换模型不影响（同一句话不必因为换了模型重译）。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};

/// 翻译档案：决定用哪套提示词，也是翻译记忆的一部分键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// 网页段落、划词、输入框
    Web,
    /// 字幕（短句、口语）
    Subtitle,
    /// 软件 / 游戏界面上的短文字（屏幕翻译）
    Ui,
    /// 文档（TXT、Word、EPUB、PDF）
    Document,
}

impl Profile {
    pub fn key(self) -> &'static str {
        match self {
            Profile::Web => "web",
            Profile::Subtitle => "sub",
            Profile::Ui => "ui",
            Profile::Document => "doc",
        }
    }
}

/// 打开的数据库连接，连同它对应的数据目录（数据目录换了就重新打开；测试里每个用例用自己的目录）。
static CONN: Mutex<Option<(PathBuf, Connection)>> = Mutex::new(None);

struct Guard(MutexGuard<'static, Option<(PathBuf, Connection)>>);

impl std::ops::Deref for Guard {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.0.as_ref().expect("tm").1
    }
}

impl std::ops::DerefMut for Guard {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.0.as_mut().expect("tm").1
    }
}

fn conn(data_dir: &Path) -> AppResult<Guard> {
    let mut g = CONN.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().map_or(true, |(p, _)| p != data_dir) {
        let c = Connection::open(data_dir.join("tm.db")).map_err(|e| AppError::msg(format!("打开翻译记忆失败：{e}")))?;
        c.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS tm (
               key TEXT PRIMARY KEY, profile TEXT NOT NULL, lang TEXT NOT NULL, src TEXT NOT NULL, tgt TEXT NOT NULL,
               terms TEXT NOT NULL, provider TEXT NOT NULL DEFAULT '', edited INTEGER NOT NULL DEFAULT 0, ts INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS tm_ts ON tm(ts);",
        )
        .map_err(|e| AppError::msg(e.to_string()))?;
        *g = Some((data_dir.to_path_buf(), c));
    }
    Ok(Guard(g))
}

fn norm(src: &str) -> String {
    src.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn key(profile: Profile, lang: &str, terms: &str, src: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(format!("{}\u{1}{}\u{1}{}\u{1}{}", profile.key(), lang, terms, norm(src)).as_bytes());
    hex::encode(&h.finalize()[..12])
}

/// 目标语言（“简体中文”“English”…）→ 统一的代码（zh / en / ja …）。认不出时用原样（小写），不至于和别的语言混在一起。
pub fn lang_code(target: &str) -> String {
    match crate::ai::lang_tag(target) {
        "ai" => target.trim().to_lowercase(),
        c => c.to_string(),
    }
}

/// 查一批原文：返回和输入等长，命中的是 Some。人工改过的优先。
pub fn lookup(data_dir: &Path, profile: Profile, target: &str, terms: &str, texts: &[String]) -> Vec<Option<String>> {
    let lang = lang_code(target);
    let Ok(c) = conn(data_dir) else { return vec![None; texts.len()] };
    texts
        .iter()
        .map(|t| {
            if t.trim().is_empty() {
                return None;
            }
            let edited: Option<String> = c.query_row("SELECT tgt FROM tm WHERE key = ?1", [key(profile, &lang, "*", t)], |r| r.get(0)).ok();
            edited.or_else(|| c.query_row("SELECT tgt FROM tm WHERE key = ?1", [key(profile, &lang, terms, t)], |r| r.get(0)).ok())
        })
        .collect()
}

/// 记下一批机器译文（空的、和原文一样的不记）。
pub fn store(data_dir: &Path, profile: Profile, target: &str, terms: &str, provider: &str, pairs: &[(String, String)]) {
    let lang = lang_code(target);
    let Ok(mut c) = conn(data_dir) else { return };
    let now = crate::db::now();
    let Ok(tx) = c.transaction() else { return };
    for (s, t) in pairs {
        if s.trim().is_empty() || t.trim().is_empty() || norm(s) == norm(t) {
            continue;
        }
        let _ = tx.execute(
            "INSERT INTO tm(key, profile, lang, src, tgt, terms, provider, edited, ts) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)
             ON CONFLICT(key) DO UPDATE SET tgt = excluded.tgt, provider = excluded.provider, ts = excluded.ts",
            params![key(profile, &lang, terms, s), profile.key(), lang, s, t, terms, provider, now],
        );
    }
    let _ = tx.commit();
}

/// 人工改过的译文：任何术语版本下都优先用它。`tgt` 为空表示撤销人工译文。
pub fn store_edited(data_dir: &Path, profile: Profile, target: &str, src: &str, tgt: &str) {
    let lang = lang_code(target);
    let Ok(c) = conn(data_dir) else { return };
    let k = key(profile, &lang, "*", src);
    if tgt.trim().is_empty() {
        let _ = c.execute("DELETE FROM tm WHERE key = ?1", [k]);
        return;
    }
    let _ = c.execute(
        "INSERT INTO tm(key, profile, lang, src, tgt, terms, provider, edited, ts) VALUES(?1, ?2, ?3, ?4, ?5, '*', 'user', 1, ?6)
         ON CONFLICT(key) DO UPDATE SET tgt = excluded.tgt, ts = excluded.ts",
        params![k, profile.key(), lang, src, tgt.trim(), crate::db::now()],
    );
}

/// 术语版本：启用的术语库、AI 专家和开关的指纹。它们变了，旧的机器译文就不再命中。
pub fn terms_version(st: &crate::AppState) -> String {
    use sha2::{Digest, Sha256};
    let s = st.settings();
    let glossaries: Vec<_> = st.glossary.snapshot().glossaries.into_iter().filter(|g| g.enabled).collect();
    let material = serde_json::json!({
        "on": s.ai.glossary_on, "expert": s.ai.expert, "auto": s.ai.expert_auto, "experts": st.experts.all(), "glossaries": glossaries,
    });
    hex::encode(&Sha256::digest(material.to_string().as_bytes())[..6])
}

/// 统计（设置页显示）。
pub fn count(data_dir: &Path) -> (u64, u64) {
    let Ok(c) = conn(data_dir) else { return (0, 0) };
    let all: u64 = c.query_row("SELECT COUNT(*) FROM tm", [], |r| r.get(0)).unwrap_or(0);
    let edited: u64 = c.query_row("SELECT COUNT(*) FROM tm WHERE edited = 1", [], |r| r.get(0)).unwrap_or(0);
    (all, edited)
}

/// 清空机器译文（人工改过的保留）。
pub fn clear_machine(data_dir: &Path) -> AppResult<u64> {
    let c = conn(data_dir)?;
    c.execute("DELETE FROM tm WHERE edited = 0", []).map(|n| n as u64).map_err(|e| AppError::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_translations_per_profile_language_and_terms_and_prefers_human_edits() {
        let d = std::env::temp_dir().join(format!("cc-tm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let texts = vec!["Hello  world".to_string(), "Bye".to_string(), "".to_string()];
        assert_eq!(lookup(&d, Profile::Web, "简体中文", "v1", &texts), vec![None, None, None]);
        store(&d, Profile::Web, "简体中文", "v1", "deepl", &[("Hello world".into(), "你好世界".into()), ("Bye".into(), "Bye".into())]);
        // 空白不同也算同一句；和原文一样的不记
        assert_eq!(lookup(&d, Profile::Web, "Chinese", "v1", &texts), vec![Some("你好世界".into()), None, None], "简体中文和 Chinese 是同一个语言代码");
        // 换了档案、语言或术语版本都不命中
        assert_eq!(lookup(&d, Profile::Subtitle, "简体中文", "v1", &texts)[0], None);
        assert_eq!(lookup(&d, Profile::Web, "日语", "v1", &texts)[0], None);
        assert_eq!(lookup(&d, Profile::Web, "简体中文", "v2", &texts)[0], None);
        // 人工改过的：任何术语版本都优先
        store_edited(&d, Profile::Web, "简体中文", "Hello world", "哈喽，世界");
        assert_eq!(lookup(&d, Profile::Web, "简体中文", "v2", &texts)[0].as_deref(), Some("哈喽，世界"));
        assert_eq!(count(&d), (2, 1));
        assert_eq!(clear_machine(&d).unwrap(), 1);
        assert_eq!(lookup(&d, Profile::Web, "简体中文", "v1", &texts)[0].as_deref(), Some("哈喽，世界"), "清空机器译文不动人工的");
        store_edited(&d, Profile::Web, "简体中文", "Hello world", "");
        assert_eq!(lookup(&d, Profile::Web, "简体中文", "v1", &texts)[0], None);
    }
}
