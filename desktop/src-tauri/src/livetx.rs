//! 实时屏幕翻译的非界面部分：翻译（带缓存）、漫画提示词与解析、翻译历史、按程序合并设置。

use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::settings::LiveSettings;
use crate::AppState;

// ---------- 设置合并 ----------

/// 全局设置 + 这个程序单独记的设置（模式、识字、语言）。`mode` 是正在用的模式，优先级最高。
pub fn effective(live: &LiveSettings, exe: &str, mode: Option<&str>) -> LiveSettings {
    let mut l = live.clone();
    if let Some(p) = live.profile(exe) {
        if !p.mode.is_empty() {
            l.mode = p.mode.clone();
        }
        if !p.ocr.is_empty() {
            l.ocr = p.ocr.clone();
        }
        if !p.ocr_lang.is_empty() {
            l.ocr_lang = p.ocr_lang.clone();
        }
        if !p.target_lang.is_empty() {
            l.target_lang = p.target_lang.clone();
        }
    }
    if let Some(m) = mode.filter(|m| !m.is_empty()) {
        l.mode = m.to_string();
    }
    l
}

pub fn target_lang(live: &LiveSettings, ai_target: &str) -> String {
    let t = live.target_lang.trim();
    let t = if t.is_empty() { ai_target.trim() } else { t };
    if t.is_empty() {
        "简体中文".into()
    } else {
        t.into()
    }
}

// ---------- 翻译 ----------

static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
const CACHE_CAP: usize = 4000;

fn ckey(lang: &str, text: &str) -> String {
    format!("{lang}\u{1}{}", crate::livecore::text_key_of(text))
}

fn cache_get(lang: &str, text: &str) -> Option<String> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&ckey(lang, text)).cloned())
}

fn cache_put(lang: &str, text: &str, tr: &str) {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() >= CACHE_CAP {
        m.clear();
    }
    m.insert(ckey(lang, text), tr.to_string());
}

pub fn cache_clear() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// 逐条翻译。缓存里有的不再请求；其余合成一批交给“网页翻译”任务（用术语库、AI 专家和模型池）。
/// 返回和输入等长；某一条失败时那一条是原文。
pub fn translate(app: &AppHandle, texts: &[String], lang: &str, title: &str, stop: &AtomicBool) -> AppResult<Vec<String>> {
    let mut out: Vec<Option<String>> = texts.iter().map(|t| cache_get(lang, t)).collect();
    let todo: Vec<usize> = (0..texts.len()).filter(|i| out[*i].is_none()).collect();
    if !todo.is_empty() {
        let lines: Vec<Value> = todo.iter().enumerate().map(|(k, i)| json!({"t": k, "text": texts[*i]})).collect();
        let req: crate::vsum::Req = serde_json::from_value(json!({
            "kind": "webtrans",
            "lang": lang,
            "title": title,
            "context": "Short user-interface strings, menu items, buttons, dialogue lines or captions captured from the screen of an application or game. Keep them concise; keep names, numbers and key labels intact. OCR may contain small recognition errors; read through them.",
            "lines": lines,
        }))
        .map_err(|e| AppError::msg(e.to_string()))?;
        let id = crate::vsum::start(app, req)?;
        let t0 = Instant::now();
        let res = loop {
            if stop.load(Ordering::Relaxed) {
                crate::vsum::cancel(id);
                return Err(AppError::msg("已停止"));
            }
            match crate::vsum::job_json(id) {
                Some(j) => match j["state"].as_str() {
                    Some("done") => break j["result"].clone(),
                    Some("error") => return Err(AppError::msg(j["error"].as_str().unwrap_or("翻译失败").to_string())),
                    _ => {}
                },
                None => return Err(AppError::msg("翻译任务丢失")),
            }
            if t0.elapsed() > Duration::from_secs(90) {
                crate::vsum::cancel(id);
                return Err(AppError::msg("翻译超时"));
            }
            std::thread::sleep(Duration::from_millis(80));
        };
        let got = res["lines"].as_array().cloned().unwrap_or_default();
        for item in got {
            let (Some(k), Some(tr)) = (item["t"].as_u64(), item["text"].as_str()) else { continue };
            if let Some(i) = todo.get(k as usize) {
                let tr = tr.trim();
                if !tr.is_empty() {
                    cache_put(lang, &texts[*i], tr);
                    out[*i] = Some(tr.to_string());
                }
            }
        }
    }
    Ok(out.into_iter().zip(texts).map(|(o, t)| o.unwrap_or_else(|| t.clone())).collect())
}

// ---------- 漫画：视觉模型 ----------

pub fn manga_system(lang: &str, rtl: bool, sfx: bool, names: &[(String, String)]) -> String {
    let order = if rtl {
        "List blocks in reading order (for vertical Japanese manga: right to left, top to bottom)."
    } else {
        "List blocks in reading order (left to right, top to bottom)."
    };
    let sfx_rule = if sfx { " Set \"sfx\": true for onomatopoeia / sound effects (then \"translation\" is a very short gloss, 1–4 characters)." } else { "" };
    let known = if names.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = names.iter().map(|(a, b)| format!("{a}={b}")).collect();
        format!(" Keep these names and terms translated exactly as: {}.", list.join("; "))
    };
    format!(
        "You are a manga/comic page translator with OCR. Find every piece of text that matters to the reader on this page: speech bubbles, thought bubbles, narration boxes, captions, signs and important sound effects. \
         Skip page numbers and watermarks. For each text block give: \"text\" (original, line breaks removed), \"translation\" (natural, concise {lang} that fits the bubble; keep the character's tone), \
         \"box\": [x, y, w, h] — a tight box around the TEXT itself (not the whole bubble), each number 0–1000 relative to the whole image with origin at the top-left, \
         and \"vertical\": true if the original text is written top-to-bottom in columns (vertical Japanese/Chinese), false if it is written horizontally. \
         {order}{sfx_rule}{known} \
         Also list character names and forms of address you translated as \"names\": [{{\"src\":\"...\",\"tr\":\"...\"}}]. \
         Reply with ONE JSON object only, no commentary, no code fences: {{\"blocks\":[{{\"text\":\"...\",\"translation\":\"...\",\"box\":[0,0,100,50],\"vertical\":true,\"sfx\":false}}],\"names\":[]}}. If there is no text reply {{\"blocks\":[],\"names\":[]}}."
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct MBlock {
    pub text: String,
    pub translation: String,
    /// 0–1 的 [x, y, w, h]
    pub rect: [f32; 4],
    /// 模型没说就是 None，由框的长宽比推断
    pub vertical: Option<bool>,
    /// 拟声词
    pub sfx: bool,
}

fn num(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn parse_manga(resp: &str) -> Option<Vec<MBlock>> {
    let t = crate::ai::strip_fences(resp);
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    let v: Value = serde_json::from_str(&t[a..=b]).ok()?;
    let arr = v.get("blocks")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|b| {
                let text = b.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
                let translation = b.get("translation").and_then(Value::as_str).unwrap_or("").trim().to_string();
                let bx = b.get("box")?.as_array()?;
                if translation.is_empty() || bx.len() != 4 {
                    return None;
                }
                let n: Vec<f32> = bx.iter().filter_map(num).collect();
                if n.len() != 4 {
                    return None;
                }
                let scale = if n.iter().all(|x| *x <= 1.0) { 1.0 } else { 1000.0 };
                let (x, y) = ((n[0] / scale).clamp(0.0, 1.0), (n[1] / scale).clamp(0.0, 1.0));
                let (w, h) = ((n[2] / scale).clamp(0.0, 1.0 - x), (n[3] / scale).clamp(0.0, 1.0 - y));
                if w < 0.004 || h < 0.004 {
                    return None;
                }
                let vertical = b.get("vertical").and_then(|x| x.as_bool().or_else(|| x.as_str().map(|s| s.eq_ignore_ascii_case("true"))));
                let sfx = b.get("sfx").and_then(|x| x.as_bool().or_else(|| x.as_str().map(|s| s.eq_ignore_ascii_case("true")))).unwrap_or(false);
                Some(MBlock { text, translation, rect: [x, y, w, h], vertical, sfx })
            })
            .take(120)
            .collect(),
    )
}

/// 模型顺带报的人名、称呼对照。
pub fn parse_names(resp: &str) -> Vec<(String, String)> {
    let t = crate::ai::strip_fences(resp);
    let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&t[a..=b]) else { return vec![] };
    v.get("names")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|n| {
                    let s = n.get("src")?.as_str()?.trim();
                    let t = n.get("tr")?.as_str()?.trim();
                    (!s.is_empty() && !t.is_empty() && s.chars().count() <= 24 && t.chars().count() <= 24).then(|| (s.to_string(), t.to_string()))
                })
                .take(40)
                .collect()
        })
        .unwrap_or_default()
}

/// 方向：模型明说了就听它的；没说就看框——又高又窄是竖排。
pub fn is_vertical(b: &MBlock, img_w: f32, img_h: f32) -> bool {
    b.vertical.unwrap_or(b.rect[3] * img_h > b.rect[2] * img_w * 1.8)
}

// ---------- 翻译历史 ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub at: i64,
    pub mode: String,
    pub exe: String,
    pub src: String,
    pub text: String,
}

const HISTORY_FILE: &str = "live_history.jsonl";
const HISTORY_MAX: usize = 1000;

pub fn history_push(dir: &Path, e: &Entry) {
    let p = dir.join(HISTORY_FILE);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        if let Ok(line) = serde_json::to_string(e) {
            let _ = writeln!(f, "{line}");
        }
    }
    // 超过上限时只留最后一部分（偶尔整理一次，不是每次）
    if let Ok(m) = std::fs::metadata(&p) {
        if m.len() > 512 * 1024 {
            let all = history_read(dir, usize::MAX);
            let keep = &all[all.len().saturating_sub(HISTORY_MAX)..];
            let body: String = keep.iter().filter_map(|e| serde_json::to_string(e).ok()).map(|l| l + "\n").collect();
            let _ = std::fs::write(&p, body);
        }
    }
}

/// 最近的 `n` 条，新的在前。
pub fn history_read(dir: &Path, n: usize) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(dir.join(HISTORY_FILE)) else { return vec![] };
    let mut v: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let skip = v.len().saturating_sub(n);
    v.drain(..skip);
    v.reverse();
    v
}

pub fn history_clear(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(HISTORY_FILE));
}

pub fn state_of(app: &AppHandle) -> Option<Arc<AppState>> {
    app.try_state::<Arc<AppState>>().map(|s| s.inner().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LiveProfile;

    #[test]
    fn profile_overrides_global_and_mode_wins() {
        let mut l = LiveSettings::default();
        l.profiles.push(LiveProfile { exe: "game.exe".into(), mode: "manga".into(), ocr: "enhanced".into(), ocr_lang: "ja".into(), ..Default::default() });
        let e = effective(&l, "Game.exe", None);
        assert_eq!((e.mode.as_str(), e.ocr.as_str(), e.ocr_lang.as_str()), ("manga", "enhanced", "ja"));
        assert_eq!(effective(&l, "Game.exe", Some("subtitle")).mode, "subtitle");
        assert_eq!(effective(&l, "other.exe", None).mode, "standard");
        assert_eq!(target_lang(&l, ""), "简体中文");
        assert_eq!(target_lang(&l, "English"), "English");
    }

    #[test]
    fn manga_names_sfx_and_prompt_rules() {
        let r = r#"{"blocks":[{"text":"ドン","translation":"咚","box":[10,10,50,50],"sfx":true}],"names":[{"src":"太郎","tr":"太郎"},{"src":"","tr":"x"}]}"#;
        let b = parse_manga(r).unwrap();
        assert!(b[0].sfx);
        assert_eq!(parse_names(r), vec![("太郎".to_string(), "太郎".to_string())]);
        let p = manga_system("简体中文", false, true, &[("先輩".into(), "前辈".into())]);
        assert!(p.contains("left to right") && p.contains("先輩=前辈") && p.contains("sfx"));
        assert!(manga_system("简体中文", true, false, &[]).contains("right to left"));
    }

    #[test]
    fn parses_manga_blocks_with_orientation() {
        let r = r#"```json
{"blocks":[{"text":"こんにちは","translation":"你好","box":[800,100,60,300],"vertical":true},
{"text":"Hi","translation":"嗨","box":[100,100,300,60]},
{"text":"x","translation":"","box":[1,1,1,1]},
{"text":"bad","translation":"坏","box":[1,2,3]}]}
```"#;
        let b = parse_manga(r).unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].vertical, Some(true));
        assert!((b[0].rect[0] - 0.8).abs() < 1e-5);
        // 没写方向：按长宽比
        assert!(!is_vertical(&b[1], 1000.0, 1400.0));
        let tall = MBlock { text: "a".into(), translation: "b".into(), rect: [0.1, 0.1, 0.05, 0.3], vertical: None, sfx: false };
        assert!(is_vertical(&tall, 1000.0, 1000.0));
        // 明确说横排就是横排，即使框又高又窄
        let said = MBlock { vertical: Some(false), ..tall };
        assert!(!is_vertical(&said, 1000.0, 1000.0));
        assert!(parse_manga("not json").is_none());
    }

    #[test]
    fn history_roundtrip_and_clear() {
        let dir = std::env::temp_dir().join(format!("cc-live-hist-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        for i in 0..5 {
            history_push(&dir, &Entry { at: i, mode: "standard".into(), exe: "a.exe".into(), src: format!("s{i}"), text: format!("t{i}") });
        }
        let h = history_read(&dir, 3);
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].src, "s4");
        assert_eq!(h[2].src, "s2");
        history_clear(&dir);
        assert!(history_read(&dir, 10).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cache_ignores_case_and_punctuation() {
        cache_clear();
        cache_put("zh", "Start Game!", "开始游戏");
        assert_eq!(cache_get("zh", "start game").as_deref(), Some("开始游戏"));
        assert_eq!(cache_get("en", "start game"), None);
    }
}
