//! 语音翻译（屏幕翻译的第四种模式）的和平台无关的部分：本次会话的记录（回看、导出 SRT / TXT、一键总结）、
//! 翻译时带上的上文、草稿的节流、长时间没有声音的提示。采集和识别在 `liveaudio`（只支持 Windows）。

use std::sync::Mutex;

use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// 这一句在本次会话里的起止（毫秒）
    pub start_ms: u64,
    pub end_ms: u64,
    pub src: String,
    pub text: String,
}

const CAP: usize = 5000;

#[derive(Debug, Default)]
pub struct Transcript {
    entries: Vec<Entry>,
}

static TRANSCRIPT: Mutex<Transcript> = Mutex::new(Transcript { entries: Vec::new() });

fn lock() -> std::sync::MutexGuard<'static, Transcript> {
    TRANSCRIPT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn push(start_ms: u64, end_ms: u64, src: &str, text: &str) {
    lock().push(start_ms, end_ms, src, text);
}

pub fn clear() {
    lock().entries.clear();
    *SESSION_FILE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// 本次会话自动保存到的文件（第一次保存时按时间命名，`clear` 开始新会话时重置）。
static SESSION_FILE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// 把本次会话的字幕实时存到本地：每来一句重写一遍 `.srt`（几千句也只是几百 KB）。`pre` 有值时做字幕预处理（去非语音说明和口水词），
/// 清理前的原文另存为 `.srt.raw`。返回写入的文件；写不进去（文件夹没权限等）返回 `None`，不打断识别。
pub fn autosave(dir: &std::path::Path, pre: Option<&crate::subclean::Opts>, bilingual: bool) -> Option<std::path::PathBuf> {
    let t = lock();
    if t.entries.is_empty() {
        return None;
    }
    let raw_srt = t.to_srt(bilingual);
    let (srt, cleaned) = match pre {
        Some(o) => {
            let mut c = Transcript::default();
            for e in &t.entries {
                let src = crate::subclean::clean_line(&e.src, o);
                if !src.is_empty() {
                    c.entries.push(Entry { src, ..e.clone() });
                }
            }
            let s = c.to_srt(bilingual);
            let changed = s != raw_srt;
            (s, changed)
        }
        None => (raw_srt.clone(), false),
    };
    drop(t);
    std::fs::create_dir_all(dir).ok()?;
    let mut slot = SESSION_FILE.lock().unwrap_or_else(|e| e.into_inner());
    let path = slot.get_or_insert_with(|| dir.join(format!("语音字幕-{}.srt", chrono::Local::now().format("%Y%m%d-%H%M%S")))).clone();
    drop(slot);
    std::fs::write(&path, srt).ok()?;
    if cleaned {
        let _ = std::fs::write(path.with_extension("srt.raw"), raw_srt);
    }
    Some(path)
}

pub fn entries() -> Vec<Entry> {
    lock().entries.clone()
}

pub fn context(n: usize) -> String {
    lock().context(n)
}

impl Transcript {
    pub fn push(&mut self, start_ms: u64, end_ms: u64, src: &str, text: &str) {
        let (src, text) = (src.trim(), text.trim());
        if src.is_empty() {
            return;
        }
        self.entries.push(Entry { start_ms, end_ms: end_ms.max(start_ms + 500), src: src.into(), text: text.into() });
        if self.entries.len() > CAP {
            self.entries.remove(0);
        }
    }

    /// 最近 n 句的“原文 → 译文”，给模型当上文（同一场对话里的称呼、人名、语气保持一致）。
    pub fn context(&self, n: usize) -> String {
        let from = self.entries.len().saturating_sub(n);
        self.entries[from..].iter().map(|e| if e.text.is_empty() { e.src.clone() } else { format!("{} → {}", e.src, e.text) }).collect::<Vec<_>>().join("\n")
    }

    pub fn to_srt(&self, bilingual: bool) -> String {
        let mut out = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            // 下一句开始前结束：不重叠
            let end = self.entries.get(i + 1).map_or(e.end_ms, |n| e.end_ms.min(n.start_ms.max(e.start_ms + 300)));
            let body = if bilingual && !e.text.is_empty() {
                format!("{}\n{}", e.text, e.src)
            } else if e.text.is_empty() {
                e.src.clone()
            } else {
                e.text.clone()
            };
            out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, srt_time(e.start_ms), srt_time(end), body));
        }
        out
    }

    pub fn to_txt(&self, bilingual: bool) -> String {
        self.entries
            .iter()
            .map(|e| {
                let t = format!("[{}] ", clock(e.start_ms));
                if bilingual && !e.text.is_empty() {
                    format!("{t}{}\n{}\n", e.src, e.text)
                } else if e.text.is_empty() {
                    format!("{t}{}\n", e.src)
                } else {
                    format!("{t}{}\n", e.text)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 总结用的全文（带时间，原文和译文都给模型，让它按原意总结）。
    pub fn for_summary(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("[{}] {}{}", clock(e.start_ms), e.src, if e.text.is_empty() { String::new() } else { format!(" ({})", e.text) }))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn srt_time(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

// ---------- 草稿 / 无声提示 ----------

/// 边说边出字：说话过程中隔一阵对“已经说出的部分”识别一次当草稿，整句说完再用完整结果替换。
/// 识别越慢，草稿越稀（不超过大约一半的时间在做草稿），也不会为了多出一两个字就识别。
#[derive(Debug, Default)]
pub struct DraftGate {
    last_at: u64,
    last_len: usize,
    /// 最近一次草稿识别花的时间（毫秒）
    pub cost_ms: u64,
}

impl DraftGate {
    /// `now` 毫秒；`audio_ms` 这一句目前已有多长；返回要不要现在做一次草稿。
    pub fn should(&mut self, now: u64, audio_ms: u64) -> bool {
        let gap = (self.cost_ms * 2).clamp(700, 3000);
        if audio_ms < 1200 || now.saturating_sub(self.last_at) < gap || audio_ms < self.last_len as u64 + 500 {
            return false;
        }
        self.last_at = now;
        self.last_len = audio_ms as usize;
        true
    }

    /// 一句说完（或开始新的一句）：重新计。
    pub fn reset(&mut self) {
        self.last_len = 0;
    }
}

/// 连续没有声音超过一段时间提醒一次（选错了输出设备、视频没在播）。
#[derive(Debug, Default)]
pub struct SilenceWatch {
    quiet_ms: u64,
    warned: bool,
}

impl SilenceWatch {
    /// 喂入一块声音（`ms` 时长、`rms` 音量）；刚好超过 `limit_ms` 时返回 true，之后有声音才会重新开始计。
    pub fn feed(&mut self, ms: u64, rms: f32, limit_ms: u64) -> bool {
        if rms > 0.004 {
            self.quiet_ms = 0;
            self.warned = false;
            return false;
        }
        self.quiet_ms += ms;
        if self.quiet_ms >= limit_ms && !self.warned {
            self.warned = true;
            return true;
        }
        false
    }
}

// ---------- 总结 ----------

pub fn summary_system(lang: &str) -> String {
    format!(
        "You summarise transcripts of live meetings, lectures, videos or conversations. The transcript lines look like `[mm:ss] original (translation)`. \
Write the summary in {lang}. Output Markdown with these sections: ## Overview (2–3 sentences), ## Key points (bullets, each with the time it was said), ## Decisions (bullets, only if any were made), ## Action items (bullets with owner and deadline only if stated, otherwise omit the section), ## Open questions (only if any). \
Use only what is in the transcript; do not invent names, numbers, dates or commitments; when the transcript is unclear, say so. Speech recognition may contain small errors; read through them."
    )
}

#[tauri::command]
pub fn sc_audio_transcript() -> Vec<Entry> {
    entries()
}

#[tauri::command]
pub fn sc_audio_clear() {
    clear();
}

/// 导出本次会话的记录：`srt` / `txt`；`bilingual` 译文和原文一起。
#[tauri::command]
pub fn sc_audio_export(kind: String, bilingual: bool, path: String) -> AppResult<usize> {
    let t = lock();
    if t.entries.is_empty() {
        return Err(AppError::invalid("本次还没有可以导出的内容。"));
    }
    let body = match kind.as_str() {
        "srt" => t.to_srt(bilingual),
        "txt" => t.to_txt(bilingual),
        _ => return Err(AppError::invalid("导出格式只支持 srt / txt。")),
    };
    let n = t.entries.len();
    drop(t);
    std::fs::write(&path, body)?;
    Ok(n)
}

/// 一键总结：把本次会话的记录交给模型，得到会议纪要式的总结（Markdown）。
#[tauri::command]
pub async fn sc_audio_summary(state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> AppResult<String> {
    let text = lock().for_summary();
    if text.trim().is_empty() {
        return Err(AppError::invalid("本次还没有内容可以总结。"));
    }
    let settings = state.settings_raw();
    let lang = crate::livetx::target_lang(&settings.live, &settings.ai.target_lang);
    let ep = crate::ai::chat_endpoint_for(&state, &settings, crate::ai_pool::Task::Summary)?;
    let clipped: String = text.chars().take(60_000).collect();
    crate::ai::chat(&ep, &settings.ai.model, &summary_system(&lang), &clipped).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Transcript {
        let mut t = Transcript::default();
        t.push(1_000, 3_500, "Hello everyone", "大家好");
        t.push(4_000, 5_000, "Let's start", "我们开始吧");
        t.push(3_661_000, 3_662_000, " ", "x"); // 空原文不记
        t
    }

    #[test]
    fn transcript_exports_srt_and_txt_and_gives_context() {
        let t = sample();
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.to_srt(false), "1\n00:00:01,000 --> 00:00:03,500\n大家好\n\n2\n00:00:04,000 --> 00:00:05,000\n我们开始吧\n\n");
        assert!(t.to_srt(true).contains("大家好\nHello everyone"));
        assert_eq!(t.to_txt(true), "[00:01] Hello everyone\n大家好\n\n[00:04] Let's start\n我们开始吧\n");
        assert_eq!(t.context(1), "Let's start → 我们开始吧");
        assert_eq!(t.context(5), "Hello everyone → 大家好\nLet's start → 我们开始吧");
        assert!(t.for_summary().contains("[00:04] Let's start (我们开始吧)"));
        assert_eq!(clock(3_725_000), "1:02:05");
    }

    #[test]
    fn srt_cues_never_overlap() {
        let mut t = Transcript::default();
        t.push(0, 5_000, "a", "甲");
        t.push(2_000, 3_000, "b", "乙");
        assert!(t.to_srt(false).contains("00:00:00,000 --> 00:00:02,000"), "{}", t.to_srt(false));
    }

    #[test]
    fn drafts_are_throttled_by_their_own_cost_and_need_growth() {
        let mut g = DraftGate::default();
        assert!(!g.should(0, 800)); // 太短
        assert!(g.should(1_000, 1_500));
        assert!(!g.should(1_300, 2_200)); // 间隔不够
        g.cost_ms = 1_200; // 识别慢：间隔拉到 2.4 秒
        assert!(!g.should(3_000, 3_000));
        assert!(g.should(3_500, 3_000));
        assert!(!g.should(9_000, 3_200)); // 只长了 200 毫秒，不值得
        g.reset();
        assert!(g.should(12_000, 1_500));
    }

    #[test]
    fn silence_warns_once_until_sound_returns() {
        let mut w = SilenceWatch::default();
        let mut warned = 0;
        for _ in 0..100 {
            warned += usize::from(w.feed(100, 0.0, 8_000));
        }
        assert_eq!(warned, 1);
        assert!(!w.feed(100, 0.2, 8_000));
        let mut again = 0;
        for _ in 0..100 {
            again += usize::from(w.feed(100, 0.0, 8_000));
        }
        assert_eq!(again, 1);
    }

    #[test]
    fn summary_prompt_asks_for_faithful_markdown_in_the_target_language() {
        let s = summary_system("简体中文");
        assert!(s.contains("简体中文") && s.contains("do not invent") && s.contains("## Action items"));
    }

    #[test]
    fn autosave_writes_cleaned_srt_and_keeps_the_raw_one() {
        let dir = std::env::temp_dir().join(format!("cc-autosave-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        clear();
        push(0, 1500, "[Music]", "");
        push(2000, 3500, "Um, hello everyone", "大家好");
        let o = crate::subclean::Opts { hearing: true, filler: crate::subclean::Level::Conservative };
        let p = autosave(&dir, Some(&o), false).expect("saved");
        let srt = std::fs::read_to_string(&p).unwrap();
        assert!(srt.contains("大家好") && !srt.contains("Music") && !srt.contains("Um"), "{srt}");
        let raw = std::fs::read_to_string(p.with_extension("srt.raw")).unwrap();
        assert!(raw.contains("[Music]") && raw.contains("大家好"));
        // 同一会话继续写同一个文件
        push(4000, 5000, "second", "第二句");
        assert_eq!(autosave(&dir, Some(&o), false).unwrap(), p);
        assert!(std::fs::read_to_string(&p).unwrap().contains("第二句"));
        // 新会话换新文件
        clear();
        push(0, 1000, "new", "新的");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_ne!(autosave(&dir, None, false).unwrap(), p);
        clear();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
