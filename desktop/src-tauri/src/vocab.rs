//! 字幕学习：生词本、间隔重复复习、导出，以及离线词频表（给“这个词你可能不熟”标记和查词兜底用）。
//!
//! 查词本身（语境释义）走清影的 AI 引擎（`vsum` 的 `lookup` 任务）；没有可用的模型时，用这里的离线词频表只给“词汇等级”。
//! 词频表来自 wordfreq 的英文数据（MIT 代码，数据 CC-BY-SA 4.0），只用了前 1.2 万个词的顺序。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

// ---------- 离线词频 ----------

static LIST: &str = include_str!("wordlist_en.txt");
static RANKS: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| LIST.lines().enumerate().map(|(i, w)| (w, i + 1)).collect());

/// 词在常用词表里的名次（1 = 最常用）；不在表里返回 `None`。会试着还原常见的词形变化。
pub fn rank(word: &str) -> Option<usize> {
    let w = word.trim().to_lowercase();
    if w.is_empty() {
        return None;
    }
    let mut cands = vec![w.clone()];
    for suf in ["s", "es", "ed", "d", "ing", "ly", "er", "est"] {
        if let Some(stem) = w.strip_suffix(suf) {
            if stem.len() >= 3 {
                cands.push(stem.to_string());
                // running → run, stopped → stop
                let b = stem.as_bytes();
                if b.len() >= 4 && b[b.len() - 1] == b[b.len() - 2] {
                    cands.push(stem[..stem.len() - 1].to_string());
                }
                cands.push(format!("{stem}e"));
            }
        }
    }
    if let Some(stem) = w.strip_suffix("ies") {
        cands.push(format!("{stem}y"));
    }
    cands.iter().filter_map(|c| RANKS.get(c.as_str()).copied()).min()
}

/// 词汇等级：基础 / 常用 / 进阶 / 生僻。
pub fn level(rank: Option<usize>) -> &'static str {
    match rank {
        Some(r) if r <= 2000 => "基础",
        Some(r) if r <= 6000 => "常用",
        Some(_) => "进阶",
        None => "生僻",
    }
}

/// 没有 AI 模型时的查词结果：只有等级，没有释义。
pub fn offline_lookup(word: &str) -> Value {
    let r = rank(word);
    json!({"word": word.trim(), "rank": r, "level": level(r), "offline": true})
}

/// 给扩展：这些词里哪些是“可能不熟”的（排名超过阈值或不在表里）。`min_rank` 为 0 时表示不标记。
pub fn unfamiliar(words: &[String], min_rank: usize) -> Vec<String> {
    if min_rank == 0 {
        return vec![];
    }
    words.iter().filter(|w| w.chars().count() >= 3 && rank(w).map_or(true, |r| r > min_rank)).cloned().collect()
}

// ---------- 存储 ----------

static STORE: OnceLock<Mutex<Connection>> = OnceLock::new();

fn conn(data_dir: &Path) -> AppResult<std::sync::MutexGuard<'static, Connection>> {
    if STORE.get().is_none() {
        std::fs::create_dir_all(data_dir)?;
        let c = Connection::open(data_dir.join("vocab.db"))?;
        init(&c)?;
        let _ = STORE.set(Mutex::new(c));
    }
    Ok(STORE.get().ok_or_else(|| AppError::invalid("生词本打不开"))?.lock().unwrap_or_else(|e| e.into_inner()))
}

fn init(c: &Connection) -> AppResult<()> {
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS words (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            word TEXT NOT NULL UNIQUE COLLATE NOCASE,
            phonetic TEXT NOT NULL DEFAULT '',
            pos TEXT NOT NULL DEFAULT '',
            meaning TEXT NOT NULL DEFAULT '',
            ctx_meaning TEXT NOT NULL DEFAULT '',
            sentence TEXT NOT NULL DEFAULT '',
            source_title TEXT NOT NULL DEFAULT '',
            source_url TEXT NOT NULL DEFAULT '',
            time_sec REAL NOT NULL DEFAULT 0,
            created INTEGER NOT NULL,
            due INTEGER NOT NULL,
            interval_days REAL NOT NULL DEFAULT 0,
            ease REAL NOT NULL DEFAULT 2.3,
            reps INTEGER NOT NULL DEFAULT 0,
            lapses INTEGER NOT NULL DEFAULT 0,
            last_review INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS words_due ON words(due);",
    )?;
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NewWord {
    pub word: String,
    pub phonetic: String,
    pub pos: String,
    pub meaning: String,
    pub ctx_meaning: String,
    pub sentence: String,
    pub source_title: String,
    pub source_url: String,
    pub time_sec: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub id: i64,
    pub word: String,
    pub phonetic: String,
    pub pos: String,
    pub meaning: String,
    pub ctx_meaning: String,
    pub sentence: String,
    pub source_title: String,
    pub source_url: String,
    pub time_sec: f64,
    pub created: i64,
    pub due: i64,
    pub interval_days: f64,
    pub reps: i64,
    pub lapses: i64,
    pub level: String,
    /// 三个评分按钮各自的“下次复习”提示：不认识 / 模糊 / 认识
    pub next: [String; 3],
}

const COLS: &str =
    "id, word, phonetic, pos, meaning, ctx_meaning, sentence, source_title, source_url, time_sec, created, due, interval_days, ease, reps, lapses";

fn row_word(r: &rusqlite::Row<'_>) -> rusqlite::Result<Word> {
    let word: String = r.get(1)?;
    let interval: f64 = r.get(12)?;
    let ease: f64 = r.get(13)?;
    let reps: i64 = r.get(14)?;
    Ok(Word {
        id: r.get(0)?,
        level: level(rank(&word)).into(),
        word,
        phonetic: r.get(2)?,
        pos: r.get(3)?,
        meaning: r.get(4)?,
        ctx_meaning: r.get(5)?,
        sentence: r.get(6)?,
        source_title: r.get(7)?,
        source_url: r.get(8)?,
        time_sec: r.get(9)?,
        created: r.get(10)?,
        due: r.get(11)?,
        interval_days: interval,
        reps,
        lapses: r.get(15)?,
        next: [0u8, 1, 2].map(|g| human_span(schedule(interval, ease, reps, g).0)),
    })
}

/// 简化的间隔重复。返回（下次间隔，秒；新的间隔天数；新的难度系数；新的连续答对次数）。
/// 0 不认识：10 分钟后再来，难度系数降低；1 模糊：间隔略微拉长；2 认识：1 天 → 3 天 → 之前的间隔 × 难度系数。
pub fn schedule(interval_days: f64, ease: f64, reps: i64, grade: u8) -> (i64, f64, f64, i64) {
    match grade {
        0 => (600, 0.0, (ease - 0.2).max(1.3), 0),
        1 => {
            let d = (interval_days * 1.2).max(1.0);
            ((d * 86_400.0) as i64, d, ease, reps)
        }
        _ => {
            let d = match reps {
                0 => 1.0,
                1 => 3.0,
                _ => (interval_days * ease).max(3.0),
            };
            ((d * 86_400.0) as i64, d, (ease + 0.1).min(3.0), reps + 1)
        }
    }
}

fn human_span(secs: i64) -> String {
    if secs < 3600 {
        format!("{} 分钟", (secs / 60).max(1))
    } else if secs < 86_400 {
        format!("{} 小时", secs / 3600)
    } else {
        format!("{} 天", secs / 86_400)
    }
}

pub fn add(data_dir: &Path, w: &NewWord) -> AppResult<i64> {
    let word = w.word.trim();
    if word.is_empty() || word.chars().count() > 80 {
        return Err(AppError::invalid("词条为空或太长"));
    }
    let c = conn(data_dir)?;
    let t = now();
    // 已经收藏过：补充释义和例句，不动复习进度
    c.execute(
        "INSERT INTO words (word, phonetic, pos, meaning, ctx_meaning, sentence, source_title, source_url, time_sec, created, due)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)
         ON CONFLICT(word) DO UPDATE SET
           phonetic = CASE WHEN excluded.phonetic != '' THEN excluded.phonetic ELSE phonetic END,
           pos = CASE WHEN excluded.pos != '' THEN excluded.pos ELSE pos END,
           meaning = CASE WHEN excluded.meaning != '' THEN excluded.meaning ELSE meaning END,
           ctx_meaning = CASE WHEN excluded.ctx_meaning != '' THEN excluded.ctx_meaning ELSE ctx_meaning END,
           sentence = CASE WHEN excluded.sentence != '' THEN excluded.sentence ELSE sentence END,
           source_title = CASE WHEN excluded.source_title != '' THEN excluded.source_title ELSE source_title END,
           source_url = CASE WHEN excluded.source_url != '' THEN excluded.source_url ELSE source_url END,
           time_sec = CASE WHEN excluded.sentence != '' THEN excluded.time_sec ELSE time_sec END",
        params![
            word,
            w.phonetic.trim(),
            w.pos.trim(),
            w.meaning.trim(),
            w.ctx_meaning.trim(),
            w.sentence.trim().chars().take(600).collect::<String>(),
            w.source_title.trim().chars().take(200).collect::<String>(),
            w.source_url.trim().chars().take(500).collect::<String>(),
            w.time_sec.max(0.0),
            t
        ],
    )?;
    Ok(c.query_row("SELECT id FROM words WHERE word = ?1", params![word], |r| r.get(0))?)
}

pub fn remove(data_dir: &Path, id: i64) -> AppResult<()> {
    conn(data_dir)?.execute("DELETE FROM words WHERE id = ?1", params![id])?;
    Ok(())
}

/// state：all / due（该复习的）/ new（还没复习过）/ learned（已记牢：间隔 ≥ 21 天）
pub fn list(data_dir: &Path, query: &str, state: &str, limit: usize) -> AppResult<Vec<Word>> {
    let c = conn(data_dir)?;
    let cond = match state {
        "due" => "AND due <= ?2",
        "new" => "AND reps = 0 AND last_review = 0 AND ?2 > 0",
        "learned" => "AND interval_days >= 21 AND ?2 > 0",
        _ => "AND ?2 > 0",
    };
    let sql = format!(
        "SELECT {COLS} FROM words WHERE (word LIKE ?1 OR meaning LIKE ?1 OR sentence LIKE ?1 OR source_title LIKE ?1) {cond} ORDER BY created DESC LIMIT {}",
        limit.clamp(1, 5000)
    );
    let like = format!("%{}%", query.trim().replace('%', ""));
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map(params![like, now()], row_word)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 所有已收藏的词（小写），扩展用来在字幕里给收藏过的词加标记。
pub fn all_words(data_dir: &Path) -> AppResult<Vec<String>> {
    let c = conn(data_dir)?;
    let mut st = c.prepare("SELECT word FROM words")?;
    let v = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(v.into_iter().map(|w| w.to_lowercase()).collect())
}

pub fn review(data_dir: &Path, id: i64, grade: u8) -> AppResult<Word> {
    let c = conn(data_dir)?;
    let (interval, ease, reps, lapses): (f64, f64, i64, i64) =
        c.query_row("SELECT interval_days, ease, reps, lapses FROM words WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    let g = grade.min(2);
    let (secs, new_interval, new_ease, new_reps) = schedule(interval, ease, reps, g);
    let t = now();
    c.execute(
        "UPDATE words SET due = ?2, interval_days = ?3, ease = ?4, reps = ?5, lapses = ?6, last_review = ?7 WHERE id = ?1",
        params![id, t + secs, new_interval, new_ease, new_reps, lapses + i64::from(g == 0), t],
    )?;
    Ok(c.query_row(&format!("SELECT {COLS} FROM words WHERE id = ?1"), params![id], row_word)?)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total: i64,
    pub due: i64,
    pub fresh: i64,
    pub learned: i64,
    pub today: i64,
}

pub fn stats(data_dir: &Path) -> AppResult<Stats> {
    let c = conn(data_dir)?;
    let t = now();
    let q = |sql: &str, p: i64| -> rusqlite::Result<i64> { c.query_row(sql, params![p], |r| r.get(0)) };
    Ok(Stats {
        total: q("SELECT COUNT(*) FROM words WHERE ?1 > 0", t)?,
        due: q("SELECT COUNT(*) FROM words WHERE due <= ?1", t)?,
        fresh: q("SELECT COUNT(*) FROM words WHERE reps = 0 AND last_review = 0 AND ?1 > 0", t)?,
        learned: q("SELECT COUNT(*) FROM words WHERE interval_days >= 21 AND ?1 > 0", t)?,
        today: q("SELECT COUNT(*) FROM words WHERE last_review >= ?1", t - t % 86_400)?,
    })
}

fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn clock(sec: f64) -> String {
    let s = sec.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// 导出：csv / md / json / anki（制表符分隔，正面 / 背面，可直接用 Anki 的“导入文件”）。
pub fn export(data_dir: &Path, format: &str) -> AppResult<(String, &'static str)> {
    let words = list(data_dir, "", "all", 5000)?;
    let meaning_of = |w: &Word| if w.ctx_meaning.is_empty() { w.meaning.clone() } else { format!("{}（本句：{}）", w.meaning, w.ctx_meaning) };
    Ok(match format {
        "json" => (serde_json::to_string_pretty(&words)?, "json"),
        "md" => {
            let mut s = String::from("# 生词本\n\n");
            for w in &words {
                s += &format!("## {} {}\n", w.word, if w.phonetic.is_empty() { String::new() } else { format!("`{}`", w.phonetic) });
                if !w.pos.is_empty() || !w.meaning.is_empty() {
                    s += &format!("- {} {}\n", w.pos, meaning_of(w));
                }
                if !w.sentence.is_empty() {
                    s += &format!("- 原句：{}\n", w.sentence);
                }
                if !w.source_title.is_empty() {
                    s += &format!("- 出处：{} {}\n", w.source_title, clock(w.time_sec));
                }
                s += "\n";
            }
            (s, "md")
        }
        "anki" => {
            let mut s = String::new();
            for w in &words {
                let front = if w.sentence.is_empty() { w.word.clone() } else { w.sentence.replace(&w.word, &format!("<b>{}</b>", w.word)) };
                let back = format!("<b>{}</b> {} {}<br>{}", w.word, w.phonetic, w.pos, meaning_of(w));
                s += &format!("{}\t{}\t{}\n", front.replace(['\t', '\n'], " "), back.replace(['\t', '\n'], " "), w.source_title.replace(['\t', '\n'], " "));
            }
            (s, "txt")
        }
        _ => {
            let mut s = String::from("\u{feff}单词,音标,词性,释义,本句释义,原句,出处,时间,等级\n");
            for w in &words {
                let cells = [&w.word, &w.phonetic, &w.pos, &w.meaning, &w.ctx_meaning, &w.sentence, &w.source_title, &clock(w.time_sec), &w.level];
                s += &cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",");
                s += "\n";
            }
            (s, "csv")
        }
    })
}

// ---------- 直接发送到 Anki（AnkiConnect） ----------

/// 一张卡片：正面 / 背面（HTML）。和导出文件里的内容一致。
pub fn anki_cards(words: &[Word]) -> Vec<(String, String)> {
    let meaning_of = |w: &Word| if w.ctx_meaning.is_empty() { w.meaning.clone() } else { format!("{}（本句：{}）", w.meaning, w.ctx_meaning) };
    words
        .iter()
        .map(|w| {
            let front = if w.sentence.is_empty() { w.word.clone() } else { w.sentence.replace(&w.word, &format!("<b>{}</b>", w.word)) };
            let back = format!("<b>{}</b> {} {}<br>{}{}", w.word, w.phonetic, w.pos, meaning_of(w), if w.source_title.is_empty() { String::new() } else { format!("<br><small>{}</small>", w.source_title) });
            (front, back)
        })
        .collect()
}

/// AnkiConnect 的 addNotes 请求体：用内置的“Basic”笔记类型，正面 / 背面两个字段，已有同样正面的卡片不重复添加。
pub fn anki_add_notes(deck: &str, cards: &[(String, String)]) -> Value {
    let notes: Vec<Value> = cards
        .iter()
        .map(|(f, b)| json!({"deckName": deck, "modelName": "Basic", "fields": {"Front": f, "Back": b}, "options": {"allowDuplicate": false, "duplicateScope": "deck"}, "tags": ["clearclip"]}))
        .collect();
    json!({"action": "addNotes", "version": 6, "params": {"notes": notes}})
}

/// 解析 addNotes 的返回：(新加了几张, 已存在 / 没加成功几张)。
pub fn anki_result(resp: &Value) -> AppResult<(usize, usize)> {
    if let Some(e) = resp.get("error").and_then(Value::as_str) {
        return Err(AppError::msg(format!("Anki 说：{e}")));
    }
    let arr = resp.get("result").and_then(Value::as_array).ok_or_else(|| AppError::parser("Anki 没有返回结果。"))?;
    let added = arr.iter().filter(|x| !x.is_null()).count();
    Ok((added, arr.len() - added))
}

/// 把生词本直接发到本机正在运行的 Anki（需要装好 AnkiConnect 插件）。`url` 留空用 http://127.0.0.1:8765。
#[tauri::command]
pub async fn vocab_anki_push(state: St<'_>, deck: String, url: Option<String>) -> AppResult<(usize, usize)> {
    let words = list(&state.data_dir, "", "all", 5000)?;
    if words.is_empty() {
        return Err(AppError::invalid("生词本里还没有单词。"));
    }
    let deck = if deck.trim().is_empty() { "清影生词" } else { deck.trim() };
    let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).unwrap_or_else(|| "http://127.0.0.1:8765".into());
    let client = &state.dl_client;
    let call = |body: Value| {
        let (client, url) = (client.clone(), url.clone());
        async move {
            let r = client
                .post(&url)
                .timeout(std::time::Duration::from_secs(30))
                .json(&body)
                .send()
                .await
                .map_err(|_| AppError::new(crate::error::ErrorKind::Network, "连不上 Anki：请先打开 Anki，并安装 AnkiConnect 插件（插件代码 2055492159）。"))?;
            r.json::<Value>().await.map_err(|_| AppError::parser("Anki 返回的不是有效的 JSON。"))
        }
    };
    // 牌组不存在就建（已存在不会报错）
    anki_result_ok(&call(json!({"action": "createDeck", "version": 6, "params": {"deck": deck}})).await?)?;
    anki_result(&call(anki_add_notes(deck, &anki_cards(&words))).await?)
}

fn anki_result_ok(resp: &Value) -> AppResult<()> {
    match resp.get("error").and_then(Value::as_str) {
        Some(e) => Err(AppError::msg(format!("Anki 说：{e}"))),
        None => Ok(()),
    }
}

// ---------- 命令 ----------

// ---------- 原声（录下的这一句，导出 Anki 时一起打包） ----------

/// 原声文件放在数据目录里，一个词一个文件（存在清影这边，换浏览器、清浏览器数据都不会丢，也进备份）。
pub fn clip_dir(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("vocab-clips")
}

/// 词 → 文件名里用的部分：小写，字母数字之外换成下划线。
pub fn clip_key(word: &str) -> String {
    let k: String = word.trim().to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).take(60).collect();
    k.trim_matches('_').to_string()
}

const CLIP_MAX: usize = 2 * 1024 * 1024;

/// 保存一段原声。`data` 是 `data:audio/webm;base64,...`。
pub fn save_clip(data_dir: &Path, word: &str, data: &str) -> AppResult<String> {
    use base64::Engine;
    let key = clip_key(word);
    if key.is_empty() {
        return Err(AppError::invalid("没有词"));
    }
    let (head, b64) = data.split_once(',').ok_or_else(|| AppError::invalid("录音格式不对"))?;
    let ext = if head.contains("ogg") {
        "ogg"
    } else if head.contains("mp4") || head.contains("m4a") || head.contains("aac") {
        "m4a"
    } else {
        "webm"
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| AppError::invalid("录音数据不对"))?;
    if bytes.is_empty() || bytes.len() > CLIP_MAX {
        return Err(AppError::invalid("这一句太长了，没有保存（最长约 2 MB）"));
    }
    let dir = clip_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    // 同一个词只留最新的一段
    for old in clips(data_dir).into_iter().filter(|(k, _)| *k == key) {
        let _ = std::fs::remove_file(dir.join(old.1));
    }
    let name = format!("clearclip_{key}.{ext}");
    std::fs::write(dir.join(&name), bytes)?;
    Ok(name)
}

/// 已有原声：（词的 key，文件名）。
pub fn clips(data_dir: &Path) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(clip_dir(data_dir)) else { return vec![] };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_prefix("clearclip_")?.rsplit_once('.')?.0.to_string();
            Some((stem, name))
        })
        .collect()
}

/// Anki 包：卡片文本（有原声的词在背面加 `[sound:文件名]`）+ 原声文件，打成 zip。没有原声返回 None。
pub fn anki_package(data_dir: &Path) -> AppResult<Option<Vec<u8>>> {
    use std::io::Write;
    let have: std::collections::HashMap<String, String> = clips(data_dir).into_iter().collect();
    if have.is_empty() {
        return Ok(None);
    }
    let words = list(data_dir, "", "all", 5000)?;
    let (text, _) = export(data_dir, "anki")?;
    let mut used: Vec<String> = vec![];
    let lines: Vec<String> = text
        .lines()
        .zip(words.iter())
        .map(|(ln, w)| match have.get(&clip_key(&w.word)) {
            Some(name) => {
                let mut cols: Vec<String> = ln.split('\t').map(str::to_string).collect();
                if cols.len() > 1 {
                    cols[1] += &format!(" [sound:{name}]");
                }
                used.push(name.clone());
                cols.join("\t")
            }
            None => ln.to_string(),
        })
        .collect();
    if used.is_empty() {
        return Ok(None);
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("生词本.txt", opts).map_err(|e| AppError::msg(e.to_string()))?;
        zip.write_all(lines.join("\n").as_bytes())?;
        for name in used {
            let bytes = std::fs::read(clip_dir(data_dir).join(&name))?;
            zip.start_file(name.as_str(), opts).map_err(|e| AppError::msg(e.to_string()))?;
            zip.write_all(&bytes)?;
        }
        zip.finish().map_err(|e| AppError::msg(e.to_string()))?;
    }
    Ok(Some(buf.into_inner()))
}

#[tauri::command]
pub fn vocab_list(state: St<'_>, query: Option<String>, filter: Option<String>) -> AppResult<Vec<Word>> {
    list(&state.data_dir, query.as_deref().unwrap_or(""), filter.as_deref().unwrap_or("all"), 2000)
}

#[tauri::command]
pub fn vocab_stats(state: St<'_>) -> AppResult<Stats> {
    stats(&state.data_dir)
}

#[tauri::command]
pub fn vocab_add(state: St<'_>, word: NewWord) -> AppResult<i64> {
    add(&state.data_dir, &word)
}

#[tauri::command]
pub fn vocab_remove(state: St<'_>, id: i64) -> AppResult<()> {
    remove(&state.data_dir, id)
}

#[tauri::command]
pub fn vocab_review(state: St<'_>, id: i64, grade: u8) -> AppResult<Word> {
    review(&state.data_dir, id, grade)
}

/// 导出到文件，返回写入的路径。
#[tauri::command]
pub fn vocab_export(state: St<'_>, format: String, path: String) -> AppResult<String> {
    // Anki 而且录过原声：导出成 zip（卡片 + 原声）
    if format == "anki" {
        if let Some(zip) = anki_package(&state.data_dir)? {
            let p = std::path::Path::new(&path).with_extension("zip");
            std::fs::write(&p, zip)?;
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    let (text, _) = export(&state.data_dir, &format)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anki_connect_payload_and_result() {
        let w = Word { word: "serendipity".into(), phonetic: "/ˌserənˈdɪpəti/".into(), pos: "n.".into(), meaning: "意外发现".into(), sentence: "It was pure serendipity.".into(), source_title: "Demo".into(), ..Default::default() };
        let cards = anki_cards(&[w.clone(), Word { word: "x".into(), meaning: "叉".into(), ..Default::default() }]);
        assert_eq!(cards[0].0, "It was pure <b>serendipity</b>.");
        assert!(cards[0].1.starts_with("<b>serendipity</b> /ˌserənˈdɪpəti/ n.<br>意外发现") && cards[0].1.contains("<small>Demo</small>"));
        assert_eq!(cards[1].0, "x", "没有原句时正面就是单词");
        let body = anki_add_notes("清影生词", &cards);
        assert_eq!(body["action"], "addNotes");
        assert_eq!(body["version"], 6);
        assert_eq!(body["params"]["notes"][0]["deckName"], "清影生词");
        assert_eq!(body["params"]["notes"][0]["modelName"], "Basic");
        assert_eq!(body["params"]["notes"][0]["options"]["allowDuplicate"], false);
        // 返回：null 表示没加成（重复）
        assert_eq!(anki_result(&json!({"result": [1496198395707i64, null, 1496198395708i64], "error": null})).unwrap(), (2, 1));
        assert!(anki_result(&json!({"result": null, "error": "deck was not found"})).unwrap_err().message.contains("deck was not found"));
        assert!(anki_result(&json!({})).is_err());
    }

    #[test]
    fn clips_are_saved_per_word_and_packed_into_the_anki_zip() {
        use base64::Engine;
        let d = std::env::temp_dir().join(format!("cc-clip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(clip_key(" Hello-World "), "hello_world");
        let data = format!("data:audio/webm;codecs=opus;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"RIFFfake"));
        assert_eq!(save_clip(&d, "Hello", &data).unwrap(), "clearclip_hello.webm");
        // 同一个词再录一次：只留最新的
        let ogg = format!("data:audio/ogg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b"OggS"));
        assert_eq!(save_clip(&d, "hello", &ogg).unwrap(), "clearclip_hello.ogg");
        assert_eq!(clips(&d), vec![("hello".to_string(), "clearclip_hello.ogg".to_string())]);
        assert!(save_clip(&d, "x", "not a data url").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tmp() -> std::path::PathBuf {
        // 存储是进程内全局的，测试共用一个目录
        static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
        DIR.get_or_init(|| {
            let d = std::env::temp_dir().join(format!("cc-vocab-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            d
        })
        .clone()
    }

    #[test]
    fn ranks_and_levels() {
        assert_eq!(rank("the"), Some(1));
        assert_eq!(level(rank("The")), "基础");
        assert!(rank("running").is_some());
        assert_eq!(level(rank("zxqvbn")), "生僻");
        assert!(rank("").is_none());
    }

    #[test]
    fn unfamiliar_respects_threshold() {
        let ws: Vec<String> = ["the", "zxqvbn", "an"].iter().map(|s| s.to_string()).collect();
        assert_eq!(unfamiliar(&ws, 3000), vec!["zxqvbn".to_string()]);
        assert!(unfamiliar(&ws, 0).is_empty());
    }

    #[test]
    fn schedule_grows_and_resets() {
        let (s0, i0, e0, r0) = schedule(0.0, 2.3, 0, 2);
        assert_eq!((s0, i0, r0), (86_400, 1.0, 1));
        let (_, i1, _, r1) = schedule(i0, e0, r0, 2);
        assert_eq!((i1, r1), (3.0, 2));
        let (_, i2, _, _) = schedule(i1, 2.4, r1, 2);
        assert!(i2 > 3.0);
        let (s, i, e, r) = schedule(10.0, 2.5, 4, 0);
        assert_eq!((s, i, r), (600, 0.0, 0));
        assert!(e < 2.5);
        let (_, vague, _, _) = schedule(10.0, 2.5, 4, 1);
        assert!(vague > 10.0 && vague < 25.0);
    }

    #[test]
    fn add_list_review_export_remove() {
        let d = tmp();
        let id = add(
            &d,
            &NewWord {
                word: "Ephemeral".into(),
                meaning: "短暂的".into(),
                sentence: "An ephemeral joy.".into(),
                source_title: "Talk".into(),
                time_sec: 75.0,
                ..Default::default()
            },
        )
        .unwrap();
        // 再收藏同一个词（大小写不同）不会重复
        let again = add(&d, &NewWord { word: "ephemeral".into(), phonetic: "/ɪˈfem(ə)rəl/".into(), ..Default::default() }).unwrap();
        assert_eq!(id, again);
        let all = list(&d, "", "all", 50).unwrap();
        assert_eq!(all.iter().filter(|w| w.word.eq_ignore_ascii_case("ephemeral")).count(), 1);
        assert!(all.iter().any(|w| w.phonetic.starts_with('/') && w.meaning == "短暂的"));
        assert!(list(&d, "", "due", 50).unwrap().iter().any(|w| w.id == id));
        let w = review(&d, id, 2).unwrap();
        assert_eq!(w.reps, 1);
        assert!(!list(&d, "", "due", 50).unwrap().iter().any(|x| x.id == id));
        assert!(all_words(&d).unwrap().contains(&"ephemeral".to_string()));
        let (csv, ext) = export(&d, "csv").unwrap();
        assert_eq!(ext, "csv");
        assert!(csv.contains("Ephemeral") && csv.contains("1:15"));
        let (anki, _) = export(&d, "anki").unwrap();
        assert!(anki.contains("<b>Ephemeral</b>") || anki.contains("ephemeral"));
        assert!(stats(&d).unwrap().total >= 1);
        remove(&d, id).unwrap();
        assert!(!list(&d, "", "all", 50).unwrap().iter().any(|x| x.id == id));
    }

    #[test]
    fn csv_cells_are_quoted() {
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("plain"), "plain");
    }
}
