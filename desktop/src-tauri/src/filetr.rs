//! 文件翻译：把 TXT / Markdown / 字幕 / Word(docx) / EPUB 整份翻译，按原格式输出“双语对照”或“仅译文”。
//!
//! 做法：先把文件切成一个个“片段”（段落、字幕条、Word 段落、EPUB 的 p/h1/li…），只翻译片段里的文字，
//! 再按片段在原文里的位置把译文放回去——所以排版、图片、样式、目录都原样保留。
//! 翻译本身复用网页翻译的提示词、术语库和 AI 专家。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use crate::ai;
use crate::ai_pool::Task;
use crate::error::{AppError, AppResult};
use crate::media_tools::JobCtx;
use crate::subgen::{compose, Display};
use crate::subtitle::{self, Cue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Txt,
    Md,
    Sub,
    Docx,
    Epub,
    /// 只能抽出文字，输出为 TXT
    Pdf,
}

pub fn kind_of(p: &Path) -> Option<Kind> {
    match p.extension()?.to_string_lossy().to_lowercase().as_str() {
        "txt" | "text" => Some(Kind::Txt),
        "md" | "markdown" => Some(Kind::Md),
        "srt" | "vtt" | "ass" | "ssa" => Some(Kind::Sub),
        "docx" => Some(Kind::Docx),
        "epub" => Some(Kind::Epub),
        "pdf" => Some(Kind::Pdf),
        _ => None,
    }
}

/// 原文里一段要翻译的文字。`start..end` 是它在所属文件里的字节范围。
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub start: usize,
    pub end: usize,
    /// 要翻译的纯文字
    pub text: String,
    /// 译文前要补回去的行首标记（Markdown 的 `## `、`- `、`> ` 等）
    pub prefix: String,
}

#[derive(Debug, Clone)]
pub struct Part {
    /// 在压缩包里的路径；TXT / MD 为空
    pub name: String,
    pub raw: String,
    pub slots: Vec<Slot>,
}

/// 没有字母 / 文字的片段（纯数字、符号、分隔线）不用翻译。
pub fn worth(text: &str) -> bool {
    text.chars().any(|c| c.is_alphabetic())
}

// ---------- TXT / Markdown ----------

static MD_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:(?:#{1,6}|>|[-*+]|\d+[.)])\s+)+").unwrap());

pub fn extract_plain(raw: &str, markdown: bool) -> Vec<Slot> {
    let mut slots = vec![];
    let mut cur: Option<(usize, usize, Vec<&str>)> = None;
    let mut in_fence = false;
    let mut pos = 0usize;
    let flush = |cur: &mut Option<(usize, usize, Vec<&str>)>, slots: &mut Vec<Slot>| {
        if let Some((s, e, lines)) = cur.take() {
            let first = lines[0];
            let skip = markdown
                && (first.trim_start().starts_with('|')
                    || first.trim_start().starts_with('<')
                    || first.trim().chars().all(|c| matches!(c, '-' | '*' | '_' | '=')));
            let (prefix, head) = if markdown {
                let p = MD_PREFIX.find(first).map(|m| m.as_str().to_string()).unwrap_or_default();
                let h = first[p.len()..].to_string();
                (p, h)
            } else {
                (String::new(), first.to_string())
            };
            let mut text = head.trim().to_string();
            for l in &lines[1..] {
                let l = if markdown { l.trim_start_matches(['>', ' ']).trim() } else { l.trim() };
                if !l.is_empty() {
                    text.push(' ');
                    text.push_str(l);
                }
            }
            if !skip && worth(&text) {
                slots.push(Slot { start: s, end: e, text, prefix: prefix.trim_start().to_string() });
            }
        }
    };
    for line in raw.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let (s, e) = (pos, pos + body.len());
        pos += line.len();
        if markdown && (body.trim_start().starts_with("```") || body.trim_start().starts_with("~~~")) {
            flush(&mut cur, &mut slots);
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if body.trim().is_empty() {
            flush(&mut cur, &mut slots);
            continue;
        }
        // Markdown 里标题 / 列表项各自独立成一段
        let own = markdown && MD_PREFIX.is_match(body) && (body.trim_start().starts_with('#') || cur.is_some());
        if own {
            flush(&mut cur, &mut slots);
        }
        match &mut cur {
            Some((_, end, lines)) => {
                *end = e;
                lines.push(body);
            }
            None => cur = Some((s, e, vec![body])),
        }
    }
    flush(&mut cur, &mut slots);
    slots
}

// ---------- PDF ----------

/// PDF 抽出来的文字是按行折断的：行尾不是句末标点就接到下一行，是句末就另起一段。
pub fn reflow_pdf(pages: &[String]) -> String {
    let mut out = String::new();
    for page in pages {
        let mut para = String::new();
        let flush = |para: &mut String, out: &mut String| {
            let t = para.trim();
            if !t.is_empty() {
                out.push_str(t);
                out.push_str("\n\n");
            }
            para.clear();
        };
        for line in page.lines() {
            let l = line.trim();
            if l.is_empty() {
                flush(&mut para, &mut out);
                continue;
            }
            if !para.is_empty() {
                // 英文行之间补空格，中日韩直接相连
                let join_space = !para.chars().last().is_some_and(crate::subgen::is_cjk) || !l.chars().next().is_some_and(crate::subgen::is_cjk);
                if join_space {
                    if para.ends_with('-') && !para.ends_with(" -") {
                        para.pop();
                    } else {
                        para.push(' ');
                    }
                }
            }
            para.push_str(l);
            if l.ends_with(['.', '!', '?', '。', '！', '？', ':', '：', '”', '"', ')', '）']) {
                flush(&mut para, &mut out);
            }
        }
        flush(&mut para, &mut out);
    }
    out
}

fn load_pdf(path: &Path) -> AppResult<String> {
    let doc = lopdf::Document::load(path).map_err(|e| AppError::invalid(format!("打不开这个 PDF：{e}")))?;
    if doc.is_encrypted() {
        return Err(AppError::invalid("这个 PDF 有密码，先用阅读器去掉密码再翻译。"));
    }
    let mut pages = vec![];
    for (no, _) in doc.get_pages() {
        pages.push(doc.extract_text(&[no]).unwrap_or_default());
    }
    let text = reflow_pdf(&pages);
    if !worth(&text) {
        return Err(AppError::invalid("没有读到文字——可能是扫描版 PDF（图片），需要先做文字识别。"));
    }
    Ok(text)
}

// ---------- XML：Word / EPUB ----------

pub fn xml_unescape(s: &str) -> String {
    static ENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&(#x[0-9a-fA-F]+|#\d+|[a-zA-Z]+);").unwrap());
    ENT.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let named = match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "copy" => Some('©'),
            _ => None,
        };
        if let Some(ch) = named {
            return ch.to_string();
        }
        let n = if let Some(h) = e.strip_prefix("#x") { u32::from_str_radix(h, 16).ok() } else { e.strip_prefix('#').and_then(|d| d.parse().ok()) };
        n.and_then(char::from_u32).map(|c| c.to_string()).unwrap_or_else(|| c[0].to_string())
    })
    .into_owned()
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

static W_PARA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<w:p(?:\s[^>]*)?>.*?</w:p>").unwrap());
static W_T: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<w:t(?:\s[^>]*)?>(.*?)</w:t>").unwrap());

pub fn extract_docx(xml: &str) -> Vec<Slot> {
    W_PARA
        .find_iter(xml)
        .filter_map(|m| {
            let text: String = W_T.captures_iter(m.as_str()).map(|c| xml_unescape(&c[1])).collect();
            let text = text.trim().to_string();
            worth(&text).then(|| Slot { start: m.start(), end: m.end(), text, prefix: String::new() })
        })
        .collect()
}

/// 把 Word 段落的文字换成译文：第一个文字块放译文，其余清空（段落格式、编号、对齐都保留）。
fn docx_replace(para: &str, trans: &str) -> String {
    let mut first = true;
    W_T.replace_all(para, |_: &regex::Captures| {
        if first {
            first = false;
            format!("<w:t xml:space=\"preserve\">{}</w:t>", xml_escape(trans))
        } else {
            "<w:t></w:t>".to_string()
        }
    })
    .into_owned()
}

static HTML_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<(p|h[1-6]|li|dt|dd|figcaption|caption|th|td)(?:\s[^>]*)?>").unwrap());
static HTML_NESTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<(p|div|h[1-6]|li|ul|ol|table|tr|blockquote|pre)[\s>/]").unwrap());
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]*>").unwrap());
static HTML_BR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").unwrap());

/// 找与 `name` 开标签配对的闭标签，返回闭标签起点。同名标签嵌套时按层数配对。
fn find_close(lower: &str, from: usize, hi: usize, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut depth = 1usize;
    let mut i = from;
    while i < hi {
        let rest = &lower[i..hi];
        let nc = rest.find(&close)?;
        // close 之前有没有同名开标签（后面必须是空白、> 或 /，避免 <p 匹配 <pre）
        let mut opens = 0usize;
        let mut k = 0usize;
        while let Some(o) = rest[k..nc].find(&open) {
            let after = rest[k + o + open.len()..].chars().next();
            if matches!(after, Some(c) if c.is_whitespace() || c == '>' || c == '/') {
                opens += 1;
            }
            k += o + open.len();
        }
        depth += opens;
        depth -= 1;
        i += nc + close.len();
        if depth == 0 {
            return Some(i - close.len());
        }
    }
    None
}

fn scan_html(raw: &str, lower: &str, lo: usize, hi: usize, out: &mut Vec<Slot>) {
    let mut i = lo;
    while i < hi {
        let Some(m) = HTML_OPEN.find_at(&raw[..hi], i) else { break };
        let name = lower[m.start() + 1..m.end()].split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string();
        let Some(close) = find_close(lower, m.end(), hi, &name) else {
            i = m.end();
            continue;
        };
        let end = close + name.len() + 3;
        let inner = &raw[m.end()..close];
        if HTML_NESTED.is_match(inner) {
            // 外层只是容器（li 里套 p、blockquote 里套 p）：翻译里面那一层
            scan_html(raw, lower, m.end(), close, out);
        } else {
            let text = xml_unescape(HTML_TAG.replace_all(&HTML_BR.replace_all(inner, " "), "").trim());
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if worth(&text) {
                out.push(Slot { start: m.start(), end, text, prefix: String::new() });
            }
        }
        i = end;
    }
}

pub fn extract_html(raw: &str) -> Vec<Slot> {
    let mut out = vec![];
    scan_html(raw, &raw.to_ascii_lowercase(), 0, raw.len(), &mut out);
    out
}

fn html_replace(block: &str, trans: &str) -> String {
    match (block.find('>'), block.rfind("</")) {
        (Some(a), Some(b)) if a < b => format!("{}{}{}", &block[..=a], xml_escape(trans), &block[b..]),
        _ => block.to_string(),
    }
}

// ---------- 按片段位置写回 ----------

/// 把译文放回原文。`trans[i]` 对应所有 Part 的片段依次编号；没有译文的片段保持原样。
pub fn render_part(kind: Kind, part: &Part, trans: &[Option<String>], offset: usize, bilingual: bool) -> String {
    let mut out = String::with_capacity(part.raw.len() * 2);
    let mut at = 0usize;
    for (k, s) in part.slots.iter().enumerate() {
        out.push_str(&part.raw[at..s.start]);
        let orig = &part.raw[s.start..s.end];
        let t = trans.get(offset + k).cloned().flatten().filter(|t| !t.trim().is_empty());
        match t {
            None => out.push_str(orig),
            Some(t) => {
                let t = t.trim();
                let line = match kind {
                    Kind::Docx => docx_replace(orig, t),
                    Kind::Epub => html_replace(orig, t),
                    _ => format!("{}{}", s.prefix, t),
                };
                if bilingual {
                    out.push_str(orig);
                    out.push_str(match kind {
                        Kind::Md => "\n\n",
                        Kind::Pdf => "\n\n",
                        Kind::Txt => "\n",
                        _ => "\n",
                    });
                }
                out.push_str(&line);
            }
        }
        at = s.end;
    }
    out.push_str(&part.raw[at..]);
    out
}

// ---------- 读取 / 输出 ----------

fn read_zip_entry(z: &mut zip::ZipArchive<std::fs::File>, name: &str) -> AppResult<String> {
    let mut f = z.by_name(name).map_err(|_| AppError::invalid(format!("文件里没有 {name}，可能不是有效的文档。")))?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| AppError::invalid(format!("读取 {name} 失败：{e}")))?;
    Ok(s)
}

pub fn load(path: &Path, kind: Kind) -> AppResult<Vec<Part>> {
    match kind {
        Kind::Txt | Kind::Md => {
            let bytes = std::fs::read(path)?;
            let raw = String::from_utf8(bytes.clone()).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
            let slots = extract_plain(&raw, kind == Kind::Md);
            Ok(vec![Part { name: String::new(), raw, slots }])
        }
        Kind::Docx => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(|_| AppError::invalid("这不是有效的 docx 文件。"))?;
            let raw = read_zip_entry(&mut z, "word/document.xml")?;
            let slots = extract_docx(&raw);
            Ok(vec![Part { name: "word/document.xml".into(), raw, slots }])
        }
        Kind::Epub => {
            let mut z = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(|_| AppError::invalid("这不是有效的 epub 文件。"))?;
            let names: Vec<String> = (0..z.len())
                .filter_map(|i| z.by_index(i).ok().map(|f| f.name().to_string()))
                .filter(|n| {
                    let l = n.to_lowercase();
                    l.ends_with(".xhtml") || l.ends_with(".html") || l.ends_with(".htm")
                })
                .collect();
            let mut parts = vec![];
            for name in names {
                let raw = read_zip_entry(&mut z, &name)?;
                let slots = extract_html(&raw);
                if !slots.is_empty() {
                    parts.push(Part { name, raw, slots });
                }
            }
            Ok(parts)
        }
        Kind::Pdf => {
            let raw = load_pdf(path)?;
            let slots = extract_plain(&raw, false);
            Ok(vec![Part { name: String::new(), raw, slots }])
        }
        Kind::Sub => Err(AppError::invalid("字幕文件走字幕通道。")),
    }
}

/// 把改过的几个条目写回压缩包，其余条目原样复制（EPUB 的 mimetype 必须第一个且不压缩）。
fn write_zip(src: &Path, dst: &Path, replaced: &std::collections::HashMap<String, String>) -> AppResult<()> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(src)?).map_err(|_| AppError::invalid("压缩包损坏。"))?;
    let mut w = zip::ZipWriter::new(std::fs::File::create(dst)?);
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| AppError::msg(e.to_string()))?;
        let name = f.name().to_string();
        let method = if name == "mimetype" { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
        let opts = zip::write::SimpleFileOptions::default().compression_method(method);
        if f.is_dir() {
            w.add_directory(name, opts).map_err(|e| AppError::msg(e.to_string()))?;
            continue;
        }
        w.start_file(name.clone(), opts).map_err(|e| AppError::msg(e.to_string()))?;
        match replaced.get(&name) {
            Some(text) => w.write_all(text.as_bytes())?,
            None => {
                let mut buf = vec![];
                f.read_to_end(&mut buf)?;
                w.write_all(&buf)?;
            }
        }
    }
    w.finish().map_err(|e| AppError::msg(e.to_string()))?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Opts {
    /// true：原文 + 译文对照；false：只留译文
    pub bilingual: bool,
    /// 目标语言；留空用“设置 → AI”里的目标语言
    pub target: String,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { bilingual: true, target: String::new() }
    }
}

/// 阅读器 / 缓存用的旁路数据：每个片段的原文和译文，按页（部分）分组。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub input: String,
    pub target: String,
    pub bilingual: bool,
    /// 每一页（EPUB 的一章、PDF 的一页、字幕按 40 条分页）的片段
    pub pages: Vec<ViewPage>,
    pub failed: usize,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewPage {
    pub src: Vec<String>,
    pub trans: Vec<Option<String>>,
}

fn cache_path(data_dir: &Path, input: &Path, target: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let len = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);
    let mut h = Sha256::new();
    h.update(input.to_string_lossy().as_bytes());
    h.update(len.to_le_bytes());
    h.update(target.as_bytes());
    let hex: String = h.finalize().iter().take(10).map(|b| format!("{b:02x}")).collect();
    data_dir.join("filetr").join(format!("{hex}.json"))
}

fn load_cache(path: &Path) -> Option<View> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn save_cache(path: &Path, v: &View) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(b) = serde_json::to_vec(v) {
        let _ = std::fs::write(path, b);
    }
}

/// 把一长串片段切成页：每页的片段数。
pub fn page_sizes(parts_slots: &[usize], total: usize, is_sub: bool) -> Vec<usize> {
    if is_sub || parts_slots.is_empty() {
        let n = total.div_ceil(40).max(1);
        return (0..n).map(|i| (total - i * 40).min(40)).collect();
    }
    parts_slots.to_vec()
}

/// 读出某个任务的阅读器数据（任务的输出文件旁边没有数据时返回 None）。
pub fn view_for(data_dir: &Path, input: &Path, target: &str) -> Option<View> {
    load_cache(&cache_path(data_dir, input, target))
}

/// 不翻译，只把文件切成页，给阅读器看原文。
pub fn preview(path: &Path) -> AppResult<View> {
    let kind = kind_of(path).ok_or_else(|| AppError::invalid("支持 TXT、Markdown、PDF、SRT/VTT/ASS 字幕、Word(docx) 和 EPUB。"))?;
    let mut v = View { input: path.to_string_lossy().into_owned(), ..Default::default() };
    if kind == Kind::Sub {
        let cues = crate::subtitle_tools::read(path)?;
        let texts: Vec<String> = cues.iter().map(|c| c.lines.join(" ")).collect();
        let mut at = 0;
        for n in page_sizes(&[], texts.len(), true) {
            v.pages.push(ViewPage { src: texts[at..at + n].to_vec(), trans: vec![None; n] });
            at += n;
        }
    } else {
        for p in load(path, kind)? {
            let src: Vec<String> = p.slots.iter().map(|s| s.text.clone()).collect();
            v.pages.push(ViewPage { trans: vec![None; src.len()], src });
        }
    }
    v.pages.retain(|p| !p.src.is_empty());
    Ok(v)
}

/// 翻译一份文件，返回输出文件路径。
pub async fn run(ctx: &JobCtx, input: &Path, opts: Opts) -> AppResult<PathBuf> {
    let kind = kind_of(input).ok_or_else(|| AppError::invalid("支持 TXT、Markdown、PDF、SRT/VTT/ASS 字幕、Word(docx) 和 EPUB。"))?;
    let settings = ctx.st.settings();
    let target = if opts.target.trim().is_empty() { settings.ai.target_lang.clone() } else { opts.target.trim().to_string() };
    let ep = ai::chat_endpoint_for(&ctx.st, &settings, Task::Translate)?;

    // 1. 切片段
    let mut cues: Vec<Cue> = vec![];
    let mut parts: Vec<Part> = vec![];
    if kind == Kind::Sub {
        cues = crate::subtitle_tools::read(input)?;
    } else {
        parts = load(input, kind)?;
    }
    let texts: Vec<String> = if kind == Kind::Sub {
        cues.iter().map(|c| c.lines.join(" ")).collect()
    } else {
        parts.iter().flat_map(|p| p.slots.iter().map(|s| s.text.clone())).collect()
    };
    if texts.is_empty() {
        return Err(AppError::invalid("没有找到需要翻译的文字。"));
    }
    // 页（部分）的划分：与阅读器一致
    let slot_counts: Vec<usize> = parts.iter().map(|p| p.slots.len()).collect();
    let sizes = page_sizes(&slot_counts, texts.len(), kind == Kind::Sub);
    let sizes: Vec<usize> = sizes.into_iter().filter(|n| *n > 0).collect();

    // 缓存：同一个文件、同一目标语言，已经译好的片段不再重翻（也就是“重试失败段落”）
    let cpath = cache_path(&ctx.st.data_dir, input, &target);
    let mut trans: Vec<Option<String>> = vec![None; texts.len()];
    if let Some(old) = load_cache(&cpath) {
        let flat: Vec<(&String, &Option<String>)> = old.pages.iter().flat_map(|p| p.src.iter().zip(p.trans.iter())).collect();
        if flat.len() == texts.len() && flat.iter().zip(&texts).all(|((s, _), t)| *s == t) {
            for (i, (_, t)) in flat.iter().enumerate() {
                trans[i] = (*t).clone().filter(|x| !x.trim().is_empty());
            }
        }
    }
    let cached = trans.iter().filter(|t| t.is_some()).count();
    ctx.note(format!("共 {} 段、{} 页，译成{}；已有 {} 段译文可直接用", texts.len(), sizes.len(), target, cached));

    // 2. 翻译（术语库 + AI 专家 + 网页翻译同款提示词），一页一页地译，每页译完就存缓存
    let terms = if settings.ai.glossary_on { ctx.st.glossary.terms_for("", ai::lang_tag(&target)) } else { vec![] };
    let guide = ctx.st.experts.resolve(&settings.ai.expert, settings.ai.expert_auto, "").map(|e| crate::experts::guide_block(&e)).unwrap_or_default();
    let system = format!("{}{}", crate::vsum::webtrans_system(&target), guide);
    let mut view = View { input: input.to_string_lossy().into_owned(), target: target.clone(), bilingual: opts.bilingual, pages: vec![], failed: 0 };
    let mut at = 0usize;
    let mut failed = 0usize;
    for (pi, n) in sizes.iter().enumerate() {
        ctx.check()?;
        let todo: Vec<usize> = (at..at + n).filter(|i| trans[*i].is_none()).collect();
        if !todo.is_empty() {
            let have = trans.iter().filter(|t| t.is_some()).count();
            ctx.note(format!("共 {} 页 · 正在译第 {} 页 · 已缓存 {} 段", sizes.len(), pi + 1, have));
            let work: Vec<Cue> = todo.iter().enumerate().map(|(k, i)| Cue { start: k as u64, end: k as u64 + 1, lines: vec![texts[*i].clone()] }).collect();
            let prog = |_: usize, _: usize| -> AppResult<()> { ctx.check() };
            let t = ai::map_cues_g(&ep, &settings.ai.model, &system, &work, settings.ai.batch_size.clamp(8, 30), &terms, &prog).await?;
            for (k, i) in todo.iter().enumerate() {
                trans[*i] = t.texts.get(k).cloned().flatten().filter(|x| !x.trim().is_empty());
            }
        }
        failed += (at..at + n).filter(|i| trans[*i].is_none()).count();
        view.pages.push(ViewPage { src: texts[at..at + n].to_vec(), trans: trans[at..at + n].to_vec() });
        view.failed = failed;
        save_cache(&cpath, &view);
        at += n;
        ctx.percent(95.0 * at as f64 / texts.len().max(1) as f64);
    }
    let t = ai::Translated { texts: trans, failed };
    let tag = if opts.bilingual { "双语" } else { "译文" };
    let base = ai::strip_lang_suffix(&crate::ai::stem_of(input)).to_string();

    // 3. 写回
    let out = if kind == Kind::Sub {
        let d = if opts.bilingual { Display::OrigTop } else { Display::Trans };
        let out = ai::out_beside(input, &base, tag, "srt");
        std::fs::write(&out, subtitle::format_srt(&compose(&cues, &t.texts, d)))?;
        ai::register_subtitle(&ctx.st, input, &out, ai::lang_tag(&target));
        out
    } else {
        let ext = if kind == Kind::Pdf { "txt".to_string() } else { input.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default() };
        let out = ai::out_beside(input, &base, tag, &ext);
        let mut offset = 0usize;
        let mut rendered = std::collections::HashMap::new();
        for p in &parts {
            rendered.insert(p.name.clone(), render_part(kind, p, &t.texts, offset, opts.bilingual));
            offset += p.slots.len();
        }
        match kind {
            Kind::Txt | Kind::Md | Kind::Pdf => std::fs::write(&out, rendered.remove("").unwrap_or_default())?,
            _ => write_zip(input, &out, &rendered)?,
        }
        out
    };
    ctx.percent(100.0);
    if t.failed > 0 {
        ctx.note(format!("有 {} 段没有译文（模型没返回），已保留原文。", t.failed));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "filetr_tests.rs"]
mod tests;
