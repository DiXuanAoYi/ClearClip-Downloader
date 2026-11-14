//! 字幕工坊：给没有字幕（或只有没标点的自动字幕）的视频生成、纠错、翻译字幕。
//!
//! 本文件分三块：
//! 1. 纯逻辑（可单元测试）：幻觉过滤、规则纠错、断句、双语合成、导出格式、AI 纠错的“防改错”护栏；
//! 2. AI 纠错：补标点、改同音错字，每一行都过护栏，不合格的行保留原文；
//! 3. 后台任务：`run_subgen`（识别 → 过滤 → 纠错 → 断句 → 翻译 → 导出）和 `run_correct`（对已有字幕纠错）。

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::ai::{self, Endpoint};
use crate::ai_pool::Task;
use crate::error::{AppError, AppResult};
use crate::glossary::Term;
use crate::media_tools::JobCtx;
use crate::subtitle::{self, Cue};
use crate::subtitle_tools;

// ===================== 文字小工具 =====================

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0x3040..=0x30FF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF)
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(
            c,
            '，' | '。'
                | '！'
                | '？'
                | '、'
                | '；'
                | '：'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '（'
                | '）'
                | '《'
                | '》'
                | '…'
                | '—'
                | '·'
                | '「'
                | '」'
                | '『'
                | '』'
                | '【'
                | '】'
        )
}

fn is_sentence_end(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '…')
}

fn is_clause_break(c: char) -> bool {
    matches!(c, '，' | '、' | '；' | '：' | ',' | ';' | ':')
}

pub fn cue_text(c: &Cue) -> String {
    c.lines.join("\n")
}

fn flat(c: &Cue) -> String {
    let mut s = String::new();
    for (i, l) in c.lines.iter().enumerate() {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        if i > 0 && !s.is_empty() {
            let last = s.chars().last().unwrap_or(' ');
            let first = l.chars().next().unwrap_or(' ');
            if !(is_cjk(last) && is_cjk(first)) {
                s.push(' ');
            }
        }
        s.push_str(l);
    }
    s
}

fn cue_of(start: u64, end: u64, text: &str) -> Cue {
    Cue { start, end: end.max(start + 1), lines: text.split('\n').map(str::to_string).collect() }
}

/// 猜一段字幕的语言标记（给文件名用）：有假名 → ja，有谚文 → ko，大量汉字 → zh，否则 en。
pub fn guess_lang(cues: &[Cue]) -> &'static str {
    let (mut kana, mut hangul, mut han, mut latin) = (0usize, 0usize, 0usize, 0usize);
    for c in cues.iter().flat_map(|c| c.lines.iter()).flat_map(|l| l.chars()) {
        match c as u32 {
            0x3040..=0x30FF => kana += 1,
            0xAC00..=0xD7AF => hangul += 1,
            0x3400..=0x9FFF => han += 1,
            _ if c.is_ascii_alphabetic() => latin += 1,
            _ => {}
        }
    }
    if kana * 5 >= han.max(1) && kana > 3 {
        "ja"
    } else if hangul > 3 && hangul >= han {
        "ko"
    } else if han > latin {
        "zh"
    } else {
        "en"
    }
}

// ===================== 幻觉过滤 =====================

/// 语音识别在没有人声的地方常常“编”出来的句子（规范化后比较：去标点空格、转小写）。
const HALLUCINATIONS: &[&str] = &[
    "谢谢观看",
    "感谢观看",
    "谢谢大家观看",
    "谢谢收看",
    "感谢收看",
    "谢谢大家",
    "感谢大家的收看",
    "请不吝点赞订阅转发打赏支持明镜与点点栏目",
    "字幕由amaraorg社区提供",
    "字幕志愿者杨茜茜",
    "字幕组",
    "优优独播剧场yoyotelevisionseries",
    "中文字幕提供",
    "thanksforwatching",
    "thankyouforwatching",
    "thankyou",
    "subtitlesbytheamaraorgcommunity",
    "pleasesubscribe",
    "subscribetomychannel",
    "ご視聴ありがとうございました",
    "ご視聴ありがとうございます",
    "チャンネル登録お願いします",
];

fn norm_key(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && !is_punct(*c)).flat_map(|c| c.to_lowercase()).collect()
}

/// 去掉空行、已知的幻觉句子（只在“整条字幕就是这句话”时），并折叠连续重复的同一条字幕。
/// 返回（结果, 去掉了几条）。
pub fn filter_cues(cues: Vec<Cue>) -> (Vec<Cue>, usize) {
    let total = cues.len();
    let mut out: Vec<Cue> = Vec::with_capacity(total);
    let mut run = 0usize;
    for c in cues {
        let key = norm_key(&flat(&c));
        if key.is_empty() || HALLUCINATIONS.contains(&key.as_str()) {
            continue;
        }
        if let Some(last) = out.last_mut() {
            if norm_key(&flat(last)) == key {
                run += 1;
                // 同一句连续出现：保留第一条，时间延长到最后一条
                if run >= 1 && key.chars().count() >= 2 {
                    last.end = last.end.max(c.end);
                    continue;
                }
            } else {
                run = 0;
            }
        }
        out.push(c);
    }
    let removed = total - out.len();
    (out, removed)
}

// ===================== 标点 =====================

/// 标点占文字的比例。自动识别的字幕常常是 0。
pub fn punct_density(cues: &[Cue]) -> f64 {
    let (mut p, mut n) = (0usize, 0usize);
    for c in cues.iter().flat_map(|c| c.lines.iter()).flat_map(|l| l.chars()) {
        if c.is_whitespace() {
            continue;
        }
        n += 1;
        if is_punct(c) && !matches!(c, '\'' | '-' | '_') {
            p += 1;
        }
    }
    if n == 0 {
        1.0
    } else {
        p as f64 / n as f64
    }
}

/// 几乎没有标点，值得补标点吗？（至少 30 个字，标点少于 2%。）
pub fn needs_punct(cues: &[Cue]) -> bool {
    let chars: usize = cues.iter().flat_map(|c| c.lines.iter()).map(|l| l.chars().filter(|c| !c.is_whitespace()).count()).sum();
    chars >= 30 && punct_density(cues) < 0.02
}

static FILLER_CJK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[\s，,。.！!？?、；;])(?:呃|嗯|额|唔|啊这)+(?:[\s，,。.！!？?、；;]|$)").unwrap());
static FILLER_EN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:u+m+|u+h+|e+rm*|h+m+)\b[,.]?\s*").unwrap());

fn half_to_full(c: char) -> char {
    match c {
        ',' => '，',
        '?' => '？',
        '!' => '！',
        ';' => '；',
        ':' => '：',
        '.' => '。',
        _ => c,
    }
}

/// 汉字旁边的半角标点换成全角；`...` 换成 `…`。数字之间的 `.` `,` `:` 不动。
fn normalize_punct(s: &str) -> String {
    let s = s.replace("...", "…").replace("。。。", "…").replace("……", "…");
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        let prev = if i > 0 { Some(chars[i - 1]) } else { None };
        let next = chars.get(i + 1).copied();
        let after_cjk = prev.is_some_and(is_cjk) || (prev == Some(' ') && i > 1 && is_cjk(chars[i - 2]) && next.is_some_and(is_cjk));
        let digits = prev.is_some_and(|p| p.is_ascii_digit()) && next.is_some_and(|n| n.is_ascii_digit());
        if matches!(c, ',' | '?' | '!' | ';' | ':' | '.') && after_cjk && !digits && !next.is_some_and(|n| n.is_ascii_alphabetic() && c == '.') {
            if out.ends_with(' ') {
                out.pop();
            }
            out.push(half_to_full(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// 中文和英文 / 数字之间加一个空格（“用 iPhone 拍”）。
fn cjk_spacing(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if let Some(p) = prev {
            let latin = |x: char| x.is_ascii_alphanumeric();
            if (is_cjk(p) && latin(c)) || (latin(p) && is_cjk(c)) {
                out.push(' ');
            }
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// 去掉连续重复的英文单词（“the the”→“the”），保留大小写不同的写法。
fn dedupe_words(s: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for w in s.split(' ') {
        let k = |x: &str| x.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        if let Some(last) = out.last() {
            let (a, b) = (k(last), k(w));
            if !a.is_empty() && a == b && a.chars().all(|c| c.is_ascii_alphabetic()) && a.len() <= 12 {
                continue;
            }
        }
        out.push(w);
    }
    out.join(" ")
}

/// 规则层纠错（不用大模型，离线、瞬时）：去口头禅和重复词、统一标点、中英文间加空格。
/// 不增删有意义的字，不改时间。
pub fn fix_rules(cues: &[Cue]) -> Vec<Cue> {
    cues.iter()
        .map(|c| {
            let lines = c
                .lines
                .iter()
                .map(|l| {
                    let mut t = l.trim().to_string();
                    t = FILLER_CJK.replace_all(&t, "$1").into_owned();
                    t = FILLER_EN.replace_all(&t, "").into_owned();
                    t = dedupe_words(&t);
                    t = normalize_punct(&t);
                    t = cjk_spacing(&t);
                    t.trim().trim_start_matches(['，', ',', '、']).trim().to_string()
                })
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>();
            Cue { start: c.start, end: c.end, lines }
        })
        .filter(|c| !c.lines.is_empty())
        .collect()
}

// ===================== 断句 =====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegOpts {
    /// 中日韩每条字幕最多多少字
    pub max_cjk: usize,
    /// 英文等每条字幕最多多少字符
    pub max_latin: usize,
}

impl SegOpts {
    /// 设置里的“字幕句长”：短适合视频字幕层，标准兼顾阅读，长接近完整句子。
    pub fn for_length(name: &str) -> SegOpts {
        match name {
            "short" => SegOpts { max_cjk: 16, max_latin: 36 },
            "long" => SegOpts { max_cjk: 32, max_latin: 72 },
            _ => SegOpts::default(),
        }
    }
}

impl Default for SegOpts {
    fn default() -> Self {
        SegOpts { max_cjk: 22, max_latin: 48 }
    }
}

fn weight(s: &str) -> usize {
    s.chars().filter(|c| !c.is_whitespace()).count().max(1)
}

fn limit_for(s: &str, o: SegOpts) -> usize {
    let cjk = s.chars().filter(|c| is_cjk(*c)).count();
    if cjk * 2 >= s.chars().count() {
        o.max_cjk
    } else {
        o.max_latin
    }
}

/// 在 `s` 里找最靠近中间的切分位置（字节下标）：优先句末标点，其次逗号，再其次空格；都没有时硬切在中间。
fn split_point(s: &str) -> usize {
    let idx: Vec<(usize, char)> = s.char_indices().collect();
    let n = idx.len();
    if n < 2 {
        return s.len();
    }
    let mid = n / 2;
    let pick = |pred: &dyn Fn(char) -> bool| -> Option<usize> {
        // 切在标点之后
        (1..n.saturating_sub(1))
            .filter(|&i| pred(idx[i - 1].1) && (idx[i - 1].1 != '.' || idx[i].1 == ' '))
            .min_by_key(|&i| (i as i64 - mid as i64).abs())
            .filter(|&i| (n / 5..=n - n / 5).contains(&i))
    };
    if let Some(i) = pick(&is_sentence_end).or_else(|| pick(&is_clause_break)) {
        return idx[i].0;
    }
    // 空格：切在空格处（空格不保留）
    if let Some(i) = (1..n - 1).filter(|&i| idx[i].1 == ' ').min_by_key(|&i| (i as i64 - mid as i64).abs()).filter(|&i| i >= n / 5 && i <= n - n / 5) {
        return idx[i].0;
    }
    idx[mid].0
}

fn split_text(s: &str, o: SegOpts, out: &mut Vec<String>) {
    let s = s.trim();
    if s.is_empty() {
        return;
    }
    // 先按句末标点切成句子
    let mut sentences: Vec<String> = vec![];
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        let next = chars.get(i + 1).copied();
        // 英文句号后面必须是空格才算句末（避免 3.5、e.g.）
        let end = is_sentence_end(c) && (c == '。' || c == '！' || c == '？' || c == '…' || next.is_none_or(|n| n == ' '));
        if end && cur.chars().filter(|c| !c.is_whitespace()).count() >= 2 {
            sentences.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur);
    }
    for sent in sentences {
        let sent = sent.trim().to_string();
        if sent.is_empty() {
            continue;
        }
        let mut stack = vec![sent];
        let mut guard = 0;
        while let Some(piece) = stack.pop() {
            guard += 1;
            if guard > 200 {
                out.push(piece);
                continue;
            }
            if piece.chars().count() <= limit_for(&piece, o) {
                out.push(piece.trim().to_string());
            } else {
                let at = split_point(&piece);
                let (a, b) = piece.split_at(at);
                if a.trim().is_empty() || b.trim().is_empty() {
                    out.push(piece.trim().to_string());
                } else {
                    stack.push(b.trim().to_string());
                    stack.push(a.trim().to_string());
                }
            }
        }
    }
}

/// 逐词（token）时间，毫秒。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub start: u64,
    pub end: u64,
}

/// 从 whisper.cpp 的完整 JSON（`-ojf`）里取出每个词 / 字的时间；特殊标记（`[_BEG_]`、`<|...|>`）和空白去掉。
pub fn parse_whisper_words(json: &str) -> Vec<Word> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let mut out = vec![];
    for seg in v.get("transcription").and_then(|t| t.as_array()).into_iter().flatten() {
        for tok in seg.get("tokens").and_then(|t| t.as_array()).into_iter().flatten() {
            let text = tok.get("text").and_then(|t| t.as_str()).unwrap_or("").trim();
            if text.is_empty() || text.starts_with("[_") || text.starts_with("<|") {
                continue;
            }
            let (Some(a), Some(b)) = (tok.pointer("/offsets/from").and_then(|x| x.as_u64()), tok.pointer("/offsets/to").and_then(|x| x.as_u64())) else {
                continue;
            };
            if b >= a {
                out.push(Word { start: a, end: b });
            }
        }
    }
    out.sort_by_key(|w| w.start);
    out
}

/// 把“按字数比例拆开”产生的分界（相邻两条首尾相接）挪到最近的真实词间停顿上（±1.5 秒内），
/// 这样拆开的字幕不会在词的中间切换。不动文字，也不动整体起止。
pub fn snap_to_words(cues: &[Cue], words: &[Word]) -> Vec<Cue> {
    let mut out = cues.to_vec();
    if words.len() < 2 {
        return out;
    }
    for i in 0..out.len().saturating_sub(1) {
        let b = out[i].end;
        if b != out[i + 1].start {
            continue;
        }
        // 候选分界：相邻两个词之间的空档中点
        let best = words
            .windows(2)
            .map(|w| (w[0].end + w[1].start.max(w[0].end)) / 2)
            .filter(|m| m.abs_diff(b) <= 1500 && *m > out[i].start && *m < out[i + 1].end)
            .min_by_key(|m| m.abs_diff(b));
        if let Some(m) = best {
            out[i].end = m;
            out[i + 1].start = m;
        }
    }
    out
}

/// 把一条字幕的时间按字数比例分给拆出来的几段。
fn spread(c: &Cue, pieces: &[String], out: &mut Vec<Cue>) {
    let total: usize = pieces.iter().map(|p| weight(p)).sum();
    let span = c.end.saturating_sub(c.start).max(pieces.len() as u64);
    let mut acc = 0usize;
    for (i, p) in pieces.iter().enumerate() {
        let a = c.start + span * acc as u64 / total as u64;
        acc += weight(p);
        let b = if i + 1 == pieces.len() { c.start + span } else { c.start + span * acc as u64 / total as u64 };
        out.push(cue_of(a, b, p));
    }
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn needs_split(c: &Cue, o: SegOpts) -> bool {
    let t = flat(c);
    t.chars().count() > limit_for(&t, o) || c.lines.len() > 2
}

pub fn segment_system() -> String {
    "You split subtitle lines at natural semantic breaks. The user sends a JSON array of subtitle texts. For each text, return the SAME text \
     with a line break (\\n) inserted at every natural break point: between clauses, before conjunctions, between a subject phrase and its verb \
     phrase. Each resulting piece should be short enough for one subtitle (about 15-22 Chinese characters or 35-48 Latin characters) and must \
     never cut a word, name, number or fixed phrase. NEVER add, remove, reorder or change any character, word or punctuation - only insert \\n. \
     Reply with ONE JSON array of the same length, no commentary and no code fences."
        .into()
}

/// 采用 AI 给的断点：去掉空白后必须和原文一模一样（一个字都不能变），否则返回 None。
/// 断出来仍然过长的片段再用规则继续拆。
pub fn split_by_breaks(c: &Cue, reply: &str, o: SegOpts) -> Option<Vec<Cue>> {
    let pieces: Vec<String> = reply.split('\n').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    if pieces.len() <= 1 || squash(&pieces.join("")) != squash(&flat(c)) {
        return None;
    }
    let mut fine = vec![];
    for p in &pieces {
        if p.chars().count() > limit_for(p, o) {
            split_text(p, o, &mut fine);
        } else {
            fine.push(p.clone());
        }
    }
    let mut out = vec![];
    spread(c, &fine, &mut out);
    Some(out)
}

/// 语义断句：只有过长的字幕才请 AI 断；AI 改了字、数量不对、请求失败的，退回规则断句。
/// 返回 (结果, 被拒绝或失败而退回规则的条数)。
pub async fn resegment_ai(
    ep: &Endpoint,
    model: &str,
    cues: &[Cue],
    o: SegOpts,
    batch: usize,
    prog: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<(Vec<Cue>, usize)> {
    let idx: Vec<usize> = (0..cues.len()).filter(|&i| needs_split(&cues[i], o)).collect();
    if idx.is_empty() {
        return Ok((cues.to_vec(), 0));
    }
    let subset: Vec<Cue> = idx.iter().map(|&i| cues[i].clone()).collect();
    let got = ai::map_cues_g(ep, model, &segment_system(), &subset, batch, &[], prog).await?;
    let mut replace: std::collections::HashMap<usize, Vec<Cue>> = std::collections::HashMap::new();
    let mut fallback = 0usize;
    for (k, &i) in idx.iter().enumerate() {
        match got.texts.get(k).and_then(|t| t.as_deref()).and_then(|t| split_by_breaks(&cues[i], t, o)) {
            Some(v) => {
                replace.insert(i, v);
            }
            None => fallback += 1,
        }
    }
    let mut out = Vec::with_capacity(cues.len());
    for (i, c) in cues.iter().enumerate() {
        match replace.remove(&i) {
            Some(v) => out.extend(v),
            None => out.extend(resegment(std::slice::from_ref(c), o)),
        }
    }
    Ok((out, fallback))
}

/// 重新断句：过长的字幕按标点 / 空格拆开，时间按字数比例分配；不会跨条合并，也不会改字。
/// 已经够短的字幕原样保留。
pub fn resegment(cues: &[Cue], o: SegOpts) -> Vec<Cue> {
    let mut out = Vec::with_capacity(cues.len());
    for c in cues {
        let text = flat(c);
        let mut pieces = vec![];
        if text.chars().count() <= limit_for(&text, o) && c.lines.len() <= 2 {
            out.push(c.clone());
            continue;
        }
        split_text(&text, o, &mut pieces);
        if pieces.len() <= 1 {
            out.push(c.clone());
            continue;
        }
        spread(c, &pieces, &mut out);
    }
    out
}

// ===================== 双语与导出 =====================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Display {
    /// 只显示原文
    Orig,
    /// 只显示译文
    Trans,
    /// 双语，原文在上
    OrigTop,
    /// 双语，译文在上
    TransTop,
}

impl Display {
    pub fn parse(s: &str) -> Display {
        match s {
            "orig" => Display::Orig,
            "trans" => Display::Trans,
            "trans-top" => Display::TransTop,
            _ => Display::OrigTop,
        }
    }
    pub fn is_bilingual(self) -> bool {
        matches!(self, Display::OrigTop | Display::TransTop)
    }
}

/// 译文轨与原文轨逐行对应；按显示方式合成。某一行没有译文时显示原文。
pub fn compose(orig: &[Cue], trans: &[Option<String>], d: Display) -> Vec<Cue> {
    orig.iter()
        .enumerate()
        .map(|(i, c)| {
            let t: Option<Vec<String>> = trans.get(i).cloned().flatten().filter(|t| !t.trim().is_empty()).map(|t| t.split('\n').map(str::to_string).collect());
            let lines = match (d, t) {
                (Display::Orig, _) | (_, None) => c.lines.clone(),
                (Display::Trans, Some(t)) => t,
                (Display::OrigTop, Some(t)) => c.lines.iter().cloned().chain(t).collect(),
                (Display::TransTop, Some(t)) => t.into_iter().chain(c.lines.iter().cloned()).collect(),
            };
            Cue { start: c.start, end: c.end, lines }
        })
        .collect()
}

fn fmt_lrc_time(ms: u64) -> String {
    format!("{:02}:{:02}.{:02}", ms / 60_000, ms / 1000 % 60, ms % 1000 / 10)
}

pub fn to_lrc(cues: &[Cue]) -> String {
    cues.iter().map(|c| format!("[{}]{}\n", fmt_lrc_time(c.start), c.lines.join(" "))).collect()
}

pub fn to_txt(cues: &[Cue], with_time: bool) -> String {
    cues.iter()
        .map(|c| {
            if with_time {
                format!("[{}] {}\n", subtitle::fmt_srt_time(c.start).split(',').next().unwrap_or(""), c.lines.join(" "))
            } else {
                format!("{}\n", c.lines.join(" "))
            }
        })
        .collect()
}

pub fn to_json(cues: &[Cue]) -> String {
    let rows: Vec<serde_json::Value> =
        cues.iter().map(|c| serde_json::json!({"start": c.start, "end": c.end, "text": c.lines.join("\n"), "lines": c.lines})).collect();
    serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into())
}

/// 按格式名输出：srt / vtt / ass / lrc / txt / json。
pub fn export(cues: &[Cue], fmt: &str) -> AppResult<String> {
    match fmt {
        "srt" | "vtt" | "ass" => subtitle_tools::write_as(cues, fmt),
        "lrc" => Ok(to_lrc(cues)),
        "txt" => Ok(to_txt(cues, false)),
        "json" => Ok(to_json(cues)),
        _ => Err(AppError::invalid("输出格式只能是 SRT、VTT、ASS、LRC、TXT 或 JSON。")),
    }
}

// ===================== AI 纠错：护栏 =====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 只用规则
    Rules,
    /// 补标点 + 明显的同音错字
    Conservative,
    /// 再加语法 / 书面化
    Aggressive,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        match s {
            "rules" => Mode::Rules,
            "aggressive" => Mode::Aggressive,
            _ => Mode::Conservative,
        }
    }
    /// 一行里“字”最多允许改动多少（不计标点和空格）
    fn max_ratio(self) -> f64 {
        match self {
            Mode::Aggressive => 0.5,
            _ => 0.25,
        }
    }
}

fn letters(s: &str) -> Vec<char> {
    s.chars().filter(|c| !c.is_whitespace() && !is_punct(*c)).flat_map(|c| c.to_lowercase()).collect()
}

/// 两段文字的差异比例：去掉标点、空格、大小写后的编辑距离 ÷ 较长者。只加标点时恒为 0。
pub fn edit_ratio(a: &str, b: &str) -> f64 {
    let (a, b) = (letters(a), letters(b));
    let n = a.len().max(b.len());
    if n == 0 {
        return 0.0;
    }
    if a.len().abs_diff(b.len()) * 2 > n {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()] as f64 / n as f64
}

fn digits_of(s: &str) -> Vec<char> {
    let mut d: Vec<char> = s.chars().filter(char::is_ascii_digit).collect();
    d.sort_unstable();
    d
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject(&'static str),
}

/// 一行改动能不能采用。`terms` 是术语库里的原文写法：原文里有、改后没了，就拒绝。
pub fn guard_line(before: &str, after: &str, mode: Mode, terms: &[Term]) -> Verdict {
    let after_t = after.trim();
    if after_t.is_empty() {
        return Verdict::Reject("改后为空");
    }
    if before.split('\n').count() != after_t.split('\n').count() && before.split('\n').count() == 1 {
        // 单行字幕不允许被改成多行
        if after_t.contains('\n') {
            return Verdict::Reject("行数变了");
        }
    }
    if mode != Mode::Aggressive && digits_of(before) != digits_of(after_t) {
        return Verdict::Reject("数字被改动");
    }
    let (lb, la) = (before.to_lowercase(), after_t.to_lowercase());
    for t in terms {
        let s = t.source.trim().to_lowercase();
        if !s.is_empty() && lb.contains(&s) && !la.contains(&s) {
            return Verdict::Reject("术语被改动");
        }
    }
    if edit_ratio(before, after_t) > mode.max_ratio() {
        return Verdict::Reject("改动过大");
    }
    Verdict::Accept
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Change {
    /// 第几条（从 0 起）
    pub idx: usize,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Default)]
pub struct Corrected {
    pub cues: Vec<Cue>,
    pub changes: Vec<Change>,
    /// 模型改得太多、被拒绝而保留原文的条数
    pub rejected: usize,
    /// 模型没有给出结果、保留原文的条数
    pub failed: usize,
}

/// 把模型给的每一行过护栏，合格的采用，不合格的保留原文。时间始终取自原字幕。
pub fn apply_corrections(orig: &[Cue], texts: &[Option<String>], mode: Mode, terms: &[Term]) -> Corrected {
    let mut r = Corrected::default();
    for (i, c) in orig.iter().enumerate() {
        let before = cue_text(c);
        match texts.get(i).cloned().flatten() {
            None => {
                r.failed += 1;
                r.cues.push(c.clone());
            }
            Some(after) => {
                let after = after.trim().to_string();
                if after == before {
                    r.cues.push(c.clone());
                    continue;
                }
                match guard_line(&before, &after, mode, terms) {
                    Verdict::Accept => {
                        r.changes.push(Change { idx: i, before: before.clone(), after: after.clone() });
                        r.cues.push(cue_of(c.start, c.end, &after));
                    }
                    Verdict::Reject(_) => {
                        r.rejected += 1;
                        r.cues.push(c.clone());
                    }
                }
            }
        }
    }
    r
}

pub fn correct_system(mode: Mode, title: &str, terms: &[&Term]) -> String {
    let scope = if mode == Mode::Aggressive {
        "You may also fix clear grammar slips and make spoken text read more naturally, but never change the meaning."
    } else {
        "Make the smallest changes that fix the text. Do NOT paraphrase, reword, summarize, translate or polish."
    };
    let mut s = format!(
        "You proofread subtitles produced by automatic speech recognition (ASR). Each element of the user's JSON array is one subtitle line, in the same language as the speech. \
         Your job: (1) add proper punctuation (commas, full stops, question marks; use full-width punctuation for Chinese/Japanese, and capitalize sentence starts in English); \
         (2) fix words the recognizer clearly got wrong — homophones and near-sounds (for example 权利/权力, 的/得/地, 在/再), misspelled names and technical terms — using the surrounding lines and the video title as context. {scope} \
         Rules: reply with ONE JSON array only, no commentary and no code fences; the array must have exactly the same number of strings in the same order; never merge, split, add or drop lines; \
         keep numbers exactly as they are; keep a line break inside a subtitle as \\n; if a line is already correct return it unchanged."
    );
    let title = title.trim();
    if !title.is_empty() {
        s.push_str(&format!("\nVideo title (context): {}", title.chars().take(120).collect::<String>()));
    }
    if !terms.is_empty() {
        s.push_str("\nSpell these terms exactly as written when they occur (never replace them with similar common words): ");
        s.push_str(&terms.iter().map(|t| t.source.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" | "));
    }
    s
}

/// AI 纠错：补标点、改同音错字。`Mode::Rules` 只走规则层。
#[allow(clippy::too_many_arguments)]
pub async fn correct_cues(
    ep: Option<&Endpoint>,
    model: &str,
    cues: &[Cue],
    mode: Mode,
    title: &str,
    terms: &[Term],
    batch: usize,
    progress: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Corrected> {
    let ruled = fix_rules(cues);
    // 规则层可能去掉整条字幕（只剩口头禅）；为了与原条数对齐，只在条数不变时采用规则层的结果
    let base: Vec<Cue> = if ruled.len() == cues.len() { ruled } else { cues.to_vec() };
    let Some(ep) = ep.filter(|_| mode != Mode::Rules) else {
        let mut changes = vec![];
        for (i, (a, b)) in cues.iter().zip(&base).enumerate() {
            if a.lines != b.lines {
                changes.push(Change { idx: i, before: cue_text(a), after: cue_text(b) });
            }
        }
        return Ok(Corrected { cues: base, changes, rejected: 0, failed: 0 });
    };
    let all: Vec<String> = base.iter().map(cue_text).collect();
    let hit = crate::glossary::relevant(terms, &all, 40);
    let system = correct_system(mode, title, &hit);
    let t = ai::map_cues(ep, model, &system, &base, batch, progress).await?;
    let mut r = apply_corrections(&base, &t.texts, mode, terms);
    // 变化相对“原始识别结果”记录（规则层的改动也算）
    r.changes = base_changes(cues, &r.cues);
    Ok(r)
}

fn base_changes(orig: &[Cue], now: &[Cue]) -> Vec<Change> {
    orig.iter()
        .zip(now)
        .enumerate()
        .filter(|(_, (a, b))| a.lines != b.lines)
        .map(|(i, (a, b))| Change { idx: i, before: cue_text(a), after: cue_text(b) })
        .collect()
}

// ===================== 后台任务 =====================

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GenOpts {
    /// 音频语言（zh / en…），空 = 用设置里的，再空 = 自动
    pub language: Option<String>,
    /// `off` / `rules` / `conservative` / `aggressive`；空 = 看识别结果有没有标点，没有就用设置里的模式
    pub correct: Option<String>,
    /// 是否重新断句（默认是）
    pub resegment: Option<bool>,
    /// 语义断句：过长的字幕请 AI 在语义停顿处断开（默认关）。一个字都不能改，改了就退回规则断句
    pub semantic: Option<bool>,
    /// 逐词时间：拆开的字幕对到真实的词间停顿上（仅本机 whisper.cpp；默认关）
    pub word_timing: Option<bool>,
    /// 反思翻译：翻完再让 AI 对照原文检查并润色一遍（默认关；多一倍请求）
    pub reflect: Option<bool>,
    /// 要翻译成的语言（如“简体中文”），可以多个
    pub targets: Vec<String>,
    /// `orig` / `trans` / `orig-top` / `trans-top`，决定主输出文件
    pub display: Option<String>,
    /// 除 SRT 外，主输出文件再导出哪些格式：vtt / ass / lrc / txt / json
    pub formats: Vec<String>,
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
}

fn write_track(ctx: &JobCtx, input: &Path, base: &str, tag: &str, cues: &[Cue], extra: &[String]) -> AppResult<PathBuf> {
    let out = ai::out_beside(input, base, tag, "srt");
    std::fs::write(&out, subtitle::format_srt(cues))?;
    ai::register_subtitle(&ctx.st, input, &out, tag);
    for f in extra {
        if f == "srt" {
            continue;
        }
        let text = export(cues, f)?;
        std::fs::write(out.with_extension(f), text)?;
    }
    Ok(out)
}

fn write_changes(path: &Path, changes: &[Change]) {
    if changes.is_empty() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(changes) {
        let _ = std::fs::write(path.with_extension("改动.json"), json);
    }
}

/// 要用的纠错模式：调用方指定的优先；没指定时，识别结果几乎没有标点就用设置里的模式，否则只走规则层。
pub fn pick_mode(requested: Option<&str>, setting: &str, cues: &[Cue]) -> Mode {
    match requested.filter(|m| !m.is_empty()) {
        Some("off") => Mode::Rules,
        Some(m) => Mode::parse(m),
        None if needs_punct(cues) => Mode::parse(setting),
        None => Mode::Rules,
    }
}

fn progress_fn<'a>(ctx: &'a JobCtx, from: f64, to: f64) -> impl Fn(usize, usize) -> AppResult<()> + Sync + 'a {
    move |done, total| {
        ctx.check()?;
        ctx.percent(from + (to - from) * done as f64 / total.max(1) as f64);
        Ok(())
    }
}

/// 生成字幕：识别 → 过滤幻觉 → 纠错（补标点 / 改错字）→ 断句 → 翻译 → 导出。返回主输出文件。
pub async fn run_subgen(ctx: &JobCtx, input: &Path, opts: GenOpts) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ffmpeg = crate::postprocess::find_ffmpeg(&ctx.st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let lang = opts.language.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| settings.ai.stt_language.clone());
    let work = std::env::temp_dir().join(format!("clearclip-subgen-{}-{}", std::process::id(), ctx.id));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let result = async {
        // 1) 识别
        ctx.note("正在识别语音…");
        let (raw, words_json) = ai::transcribe_cues_words(ctx, &settings, &ffmpeg, input, &lang, &work, opts.word_timing.unwrap_or(false)).await?;
        let words = words_json.as_deref().map(parse_whisper_words).unwrap_or_default();
        if opts.word_timing.unwrap_or(false) && words.is_empty() {
            ctx.note("没有拿到逐词时间（只有本机 whisper.cpp 支持），断句位置按字数比例估算。");
        }
        ctx.check()?;
        // 2) 过滤幻觉
        let (cues, removed) = filter_cues(raw);
        if cues.is_empty() {
            return Err(AppError::msg("没有识别出任何语音。"));
        }
        if removed > 0 {
            ctx.note(format!("已去掉 {removed} 条疑似误识别（空白、重复、“谢谢观看”之类）。"));
        }
        let lang_tag: String = if lang.is_empty() { guess_lang(&cues).to_string() } else { lang.clone() };
        let title = stem_of(input);
        let base = ai::strip_lang_suffix(&title).to_string();
        // 预处理：去非语音说明和口水词（设置里可关）；清理前的原文另存一份 `.srt.raw`，需要原话时还在
        let cues = if settings.ai.sub_clean_output {
            let o = crate::subclean::Opts { hearing: settings.ai.sub_clean_hearing, filler: crate::subclean::parse_level(&settings.ai.sub_clean_filler) };
            let (cleaned, changed) = crate::subclean::clean_cues(&cues, &o);
            if changed > 0 && !cleaned.is_empty() {
                let raw_path = ai::out_beside(input, &base, &lang_tag, "srt");
                let _ = std::fs::write(raw_path.with_extension("srt.raw"), subtitle::format_srt(&cues));
                ctx.note(format!("预处理：{changed} 条字幕去掉了非语音说明 / 口水词（清理前的原文另存为 .srt.raw）。"));
                cleaned
            } else {
                cues
            }
        } else {
            cues
        };
        let terms = if settings.ai.glossary_on { ctx.st.glossary.terms_for("", &lang_tag) } else { vec![] };

        // 3) 纠错
        let mode = pick_mode(opts.correct.as_deref(), &settings.ai.correct_mode, &cues);
        let ep = if mode == Mode::Rules { None } else { ai::chat_endpoint_for(&ctx.st, &settings, Task::Edit).ok() };
        if mode != Mode::Rules && ep.is_none() {
            ctx.note("没有配置对话模型，AI 纠错已跳过，只做了规则整理（去口头禅、统一标点）。");
        } else if needs_punct(&cues) && mode != Mode::Rules {
            ctx.note("识别结果几乎没有标点，正在用 AI 补标点、改错字…");
        }
        let prog = progress_fn(ctx, 40.0, 70.0);
        let fixed = correct_cues(ep.as_ref(), &settings.ai.model, &cues, mode, &title, &terms, settings.ai.batch_size, &prog).await?;
        if fixed.rejected > 0 || fixed.failed > 0 {
            ctx.note(format!("AI 纠错：{} 条改动过大被拒绝，{} 条没有结果，这些保留了识别原文。", fixed.rejected, fixed.failed));
        }
        // 4) 断句
        let mut cues = fixed.cues;
        if opts.resegment.unwrap_or(true) {
            let so = SegOpts::for_length(&settings.ai.cue_length);
            cues = match (opts.semantic.unwrap_or(false), mode != Mode::Rules) {
                (true, true) => match ai::chat_endpoint_for(&ctx.st, &settings, Task::Edit) {
                    Ok(sep) => {
                        ctx.note("正在用 AI 按语义断句…");
                        let prog = progress_fn(ctx, 70.0, 74.0);
                        let (v, fb) = resegment_ai(&sep, &settings.ai.model, &cues, so, settings.ai.batch_size, &prog).await?;
                        if fb > 0 {
                            ctx.note(format!("有 {fb} 条 AI 断句结果不可用（改了字或没有结果），改用规则断句。"));
                        }
                        v
                    }
                    Err(_) => resegment(&cues, so),
                },
                _ => resegment(&cues, so),
            };
        }
        if !words.is_empty() {
            cues = snap_to_words(&cues, &words);
        }
        let display = Display::parse(opts.display.as_deref().unwrap_or("orig-top"));
        let mut primary = write_track(ctx, input, &base, &lang_tag, &cues, &if opts.targets.is_empty() { opts.formats.clone() } else { vec![] })?;
        write_changes(&primary, &fixed.changes);
        ctx.percent(75.0);

        // 5) 翻译
        if !opts.targets.is_empty() {
            let ep = ai::chat_endpoint_for(&ctx.st, &settings, Task::Translate)?;
            let n = opts.targets.len();
            for (k, target) in opts.targets.iter().enumerate() {
                ctx.check()?;
                ctx.note(format!("正在翻译成{target}…"));
                let tag = ai::lang_tag(target);
                let tt = if settings.ai.glossary_on { ctx.st.glossary.terms_for("", tag) } else { vec![] };
                let from = 75.0 + 24.0 * k as f64 / n as f64;
                let prog = progress_fn(ctx, from, from + 24.0 / n as f64);
                let t = ai::translate_cues_g(&ep, &settings.ai.model, target, &cues, settings.ai.batch_size, &tt, &prog).await?;
                if t.failed > 0 {
                    ctx.note(format!("有 {} 条没有翻译成功，保留了原文。", t.failed));
                }
                let mut t = t;
                if opts.reflect.unwrap_or(false) {
                    ctx.note(format!("正在对照原文检查并润色{target}译文…"));
                    let r = ai::reflect_translations(&ep, &settings.ai.model, target, &cues, &t.texts, settings.ai.batch_size, &tt, &prog).await?;
                    ctx.note(format!("反思翻译：润色了 {} 条，{} 条保持不变。", r.changed, r.kept));
                    t.texts = r.texts;
                }
                let only = compose(&cues, &t.texts, Display::Trans);
                let both = compose(&cues, &t.texts, if display.is_bilingual() { display } else { Display::OrigTop });
                let trans_path = write_track(ctx, input, &base, tag, &only, &[])?;
                let both_path = write_track(ctx, input, &base, &format!("{tag}.双语"), &both, &[])?;
                let main = match display {
                    Display::Orig => primary.clone(),
                    Display::Trans => trans_path,
                    _ => both_path,
                };
                if k == 0 {
                    // 额外格式导出到主输出（第一个目标语言的合成结果）
                    let chosen = match display {
                        Display::Orig => cues.clone(),
                        Display::Trans => only,
                        _ => both,
                    };
                    for f in &opts.formats {
                        if f != "srt" {
                            std::fs::write(main.with_extension(f), export(&chosen, f)?)?;
                        }
                    }
                    primary = main;
                }
            }
        }
        ctx.percent(100.0);
        Ok(primary)
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// 对已有字幕做 AI 纠错：保存为新文件（原字幕不动），并把每一处改动记在 `.改动.json` 里。
pub async fn run_correct(ctx: &JobCtx, input: &Path, requested: Option<String>) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let cues = subtitle_tools::read(input)?;
    if cues.is_empty() {
        return Err(AppError::msg("字幕是空的。"));
    }
    let mode = match requested.as_deref().filter(|m| !m.is_empty()) {
        Some(m) => Mode::parse(m),
        None => Mode::parse(&settings.ai.correct_mode),
    };
    let ep = if mode == Mode::Rules { None } else { Some(ai::chat_endpoint_for(&ctx.st, &settings, Task::Edit)?) };
    let title = stem_of(input);
    let base = ai::strip_lang_suffix(&title).to_string();
    let lang_tag = guess_lang(&cues);
    let terms = if settings.ai.glossary_on { ctx.st.glossary.terms_for("", lang_tag) } else { vec![] };
    ctx.note(if needs_punct(&cues) { "字幕几乎没有标点，正在补标点、改错字…" } else { "正在检查错字和标点…" });
    let prog = progress_fn(ctx, 0.0, 95.0);
    let fixed = correct_cues(ep.as_ref(), &settings.ai.model, &cues, mode, &title, &terms, settings.ai.batch_size, &prog).await?;
    let out = ai::out_beside(input, &base, "已修正", "srt");
    std::fs::write(&out, subtitle::format_srt(&fixed.cues))?;
    ai::register_subtitle(&ctx.st, input, &out, "fixed");
    write_changes(&out, &fixed.changes);
    ctx.note(format!(
        "改了 {} 条；{} 条改动过大被拒绝，{} 条没有结果（这些保留原文）。改动明细在 .改动.json 里。",
        fixed.changes.len(),
        fixed.rejected,
        fixed.failed
    ));
    Ok(out)
}

#[cfg(test)]
#[path = "subgen_tests.rs"]
mod tests;
