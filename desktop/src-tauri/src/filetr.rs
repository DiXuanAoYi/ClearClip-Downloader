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
    /// 只能抽出文字，输出为 TXT；扫描版（只有图片）会先用视觉模型识字
    Pdf,
    /// 图片：用视觉模型识字后翻译，输出 TXT
    Img,
}

pub fn kind_of(p: &Path) -> Option<Kind> {
    match p.extension()?.to_string_lossy().to_lowercase().as_str() {
        "txt" | "text" => Some(Kind::Txt),
        "md" | "markdown" => Some(Kind::Md),
        "srt" | "vtt" | "ass" | "ssa" => Some(Kind::Sub),
        "docx" => Some(Kind::Docx),
        "epub" => Some(Kind::Epub),
        "pdf" => Some(Kind::Pdf),
        "png" | "jpg" | "jpeg" | "jfif" | "webp" => Some(Kind::Img),
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

/// 扫描版 PDF（没有可抽的文字）的错误提示；`run` 看到它就改走视觉模型识字。
const SCANNED: &str = "没有读到文字——可能是扫描版 PDF（图片）。";

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
        return Err(AppError::invalid(SCANNED));
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
        Kind::Img => Err(AppError::invalid("图片需要先识字：开始翻译时会用视觉模型识别。")),
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

#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Opts {
    /// true：原文 + 译文对照；false：只留译文
    pub bilingual: bool,
    /// 目标语言；留空用“设置 → AI”里的目标语言
    pub target: String,
    /// PDF 的输出方式：`inplace` 译文写回原处 / `interleave` 原页和译文页交替 / `txt` 只输出文字
    pub pdf_mode: String,
    /// 图片和扫描版 PDF 的识字方式：`vision` 视觉模型 / `enhanced` 本机增强识字（PaddleOCR，开源）/ `api` 在线识字接口（OCR.space、百度高精度、Google）
    pub ocr: String,
    /// 只翻译，不生成文件：先在阅读器里检查、改译文，再点“生成文件”（那一次直接用缓存里改好的译文）
    pub preview: bool,
    /// 只翻译这些页，如 `3-12, 15`、`5-`（从 1 起，留空 = 全部）；没选的页保留原文
    pub pages: String,
    /// 不翻译每页重复出现的页眉、页脚和页码（只对 PDF 版面翻译有效）
    pub skip_headers: bool,
    /// 不翻译公式、代码、网址这类没法翻译的片段
    pub skip_code: bool,
    /// 不翻译参考文献（从“References / 参考文献”那一段起）
    pub skip_refs: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            bilingual: true,
            target: String::new(),
            pdf_mode: "inplace".into(),
            ocr: "vision".into(),
            preview: false,
            pages: String::new(),
            skip_headers: true,
            skip_code: true,
            skip_refs: false,
        }
    }
}

impl Opts {
    /// 识字方式只认四种（增强识字 / Windows 自带 / 识字接口 / 视觉模型），其余当视觉模型。
    pub fn engine(&self) -> &'static str {
        match self.ocr.as_str() {
            "enhanced" => "enhanced",
            "system" => "system",
            "api" => "api",
            _ => "vision",
        }
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
    /// 缓存格式：2 起每页记了页号和处理状态（旧缓存读进来时按“都译完了”处理）
    #[serde(default)]
    pub v: u32,
    /// 片段是按 PDF 版面的块切的（装了排版组件）；否则是按段落切的
    #[serde(default)]
    pub layout: bool,
    /// 文件一共几页（PDF 的页数；其他格式等于 `pages.len()`）
    #[serde(default)]
    pub total_pages: usize,
}

pub(crate) const VIEW_V: u32 = 2;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewPage {
    pub src: Vec<String>,
    pub trans: Vec<Option<String>>,
    /// 对应 PDF 的第几页（从 0 起）；文字版 PDF 里没有文字的页不占位，所以不一定等于序号
    #[serde(default)]
    pub page: Option<usize>,
    /// 这一页处理过了（译完或失败）；还没轮到的页是 false
    #[serde(default)]
    pub done: bool,
    /// 按设置不翻译的片段（页眉页脚、公式代码、参考文献）：保留原文，不算失败
    #[serde(default)]
    pub skip: Vec<usize>,
}

// ---------- 扫描件 / 图片：视觉模型识字 ----------

const OCR_SYSTEM: &str = "You are an OCR engine. Transcribe ALL readable text in the image exactly as written, in reading order, in its original language. \
Separate paragraphs with a blank line; keep headings and list markers. Do not translate, summarize or add commentary. Reply with the plain text only. If there is no text reply with nothing.";
const OCR_MAX_PAGES: usize = 60;
const OCR_MAX_BYTES: usize = 6 * 1024 * 1024;

/// 识过的文字按识字方式分开存：换了更准的引擎要重新识，不能还用上次的结果。
pub(crate) fn ocr_path(data_dir: &Path, input: &Path, engine: &str) -> PathBuf {
    cache_path(data_dir, input, &format!("ocr-{engine}")).with_extension("ocr.txt")
}

/// 图片字节（JPEG / PNG / WebP）→ BGRA 像素。
pub fn decode_bgra(bytes: &[u8]) -> AppResult<(Vec<u8>, usize, usize)> {
    let img = image::load_from_memory(bytes).map_err(|e| AppError::invalid(format!("这张图片读不出来：{e}")))?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut px = img.into_raw();
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    Ok((px, w, h))
}

/// 识字得到的一行行文字 → 按版面并成段，段与段之间空一行。
pub fn lines_to_text(lines: &[crate::livecore::OLine]) -> String {
    crate::livecore::group_lines(lines).into_iter().map(|g| g.text).filter(|t| !t.trim().is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// 用 `engine`（enhanced / api）识别一张 BGRA 图，返回带位置的行。
async fn recognize_lines(st: &crate::AppState, engine: &str, bgra: Vec<u8>, w: usize, h: usize) -> AppResult<Vec<crate::livecore::OLine>> {
    crate::imagetext::recognize(st, engine, bgra, w, h).await
}

/// 一张图的 (mime, 字节)。
type Pic = (&'static str, Vec<u8>);

/// 把 8 位灰度 / RGB 的原始像素编码成 PNG。
fn png_of(w: u32, h: u32, rgb: bool, raw: &[u8]) -> Option<Vec<u8>> {
    let n = w as usize * h as usize * if rgb { 3 } else { 1 };
    if raw.len() < n || n == 0 {
        return None;
    }
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(if rgb { png::ColorType::Rgb } else { png::ColorType::Grayscale });
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().ok()?.write_image_data(&raw[..n]).ok()?;
    }
    Some(out)
}

/// 取出扫描版 PDF 每一页里的图片：JPEG 原样用，8 位灰度 / RGB 的位图转 PNG，其余跳过。返回 (图片, 跳过的张数)。
fn pdf_pictures(path: &Path) -> AppResult<(Vec<Pic>, usize)> {
    let doc = lopdf::Document::load(path).map_err(|e| AppError::invalid(format!("打不开这个 PDF：{e}")))?;
    let mut pics: Vec<Pic> = vec![];
    let mut skipped = 0usize;
    for (_, page_id) in doc.get_pages() {
        // 一页取最大的一张图（扫描页就是整页一张图）
        let mut best: Option<(i64, Pic)> = None;
        for im in doc.get_page_images(page_id).unwrap_or_default() {
            let filters = im.filters.clone().unwrap_or_default();
            let pic: Option<Pic> = if filters.iter().any(|f| f == "DCTDecode") {
                Some(("image/jpeg", im.content.to_vec()))
            } else if im.bits_per_component == Some(8) && matches!(im.color_space.as_deref(), Some("DeviceGray" | "DeviceRGB")) {
                let raw = doc.get_object(im.id).ok().and_then(|o| o.as_stream().ok()).and_then(|st| st.decompressed_content().ok());
                raw.and_then(|raw| png_of(im.width as u32, im.height as u32, im.color_space.as_deref() == Some("DeviceRGB"), &raw)).map(|b| ("image/png", b))
            } else {
                None
            };
            match pic {
                Some(p) if best.as_ref().map_or(true, |(a, _)| im.width * im.height > *a) => best = Some((im.width * im.height, p)),
                Some(_) => {}
                None => skipped += 1,
            }
        }
        if let Some((_, p)) = best {
            pics.push(p);
        }
    }
    Ok((pics, skipped))
}

/// 一张图识字。
async fn ocr_one(ep: &ai::Endpoint, model: &str, pic: &Pic) -> AppResult<String> {
    use base64::Engine;
    let url = format!("data:{};base64,{}", pic.0, base64::engine::general_purpose::STANDARD.encode(&pic.1));
    let resp = ai::chat_img(ep, model, OCR_SYSTEM, "Transcribe the text in this image.", &url).await.map_err(crate::imgtrans::vision_hint)?;
    Ok(resp.trim().trim_start_matches("```text").trim_start_matches("```").trim_end_matches("```").trim().to_string())
}

/// 图片 / 扫描版 PDF 的识字结果（识过的存在数据目录里，重试翻译不再重识）。
async fn ocr_text(ctx: &JobCtx, ep: &ai::Endpoint, model: &str, input: &Path, kind: Kind, engine: &str) -> AppResult<String> {
    let cp = ocr_path(&ctx.st.data_dir, input, engine);
    if let Ok(t) = std::fs::read_to_string(&cp) {
        if worth(&t) {
            ctx.note("用上次识别出的文字（不再重新识字）");
            return Ok(t);
        }
    }
    let pics: Vec<Pic> = if kind == Kind::Img {
        let mime = crate::media_server::mime_for(input);
        vec![(
            if mime == "image/png" {
                "image/png"
            } else if mime == "image/webp" {
                "image/webp"
            } else {
                "image/jpeg"
            },
            std::fs::read(input)?,
        )]
    } else {
        let (pics, skipped) = pdf_pictures(input)?;
        if pics.is_empty() {
            return Err(AppError::invalid("这个 PDF 里没有能识别的页面图片（只支持 JPEG 和 8 位灰度/彩色位图）。"));
        }
        if skipped > 0 {
            ctx.note(format!("有 {skipped} 张图片格式不支持，已跳过"));
        }
        pics
    };
    if pics.len() > OCR_MAX_PAGES {
        ctx.note(format!("共 {} 页，只识别前 {OCR_MAX_PAGES} 页", pics.len()));
    }
    let total = pics.len().min(OCR_MAX_PAGES);
    let mut out: Vec<String> = vec![];
    for (i, pic) in pics.iter().take(total).enumerate() {
        ctx.check()?;
        if pic.1.len() > OCR_MAX_BYTES {
            ctx.note(format!("第 {} 页图片超过 6 MB，已跳过", i + 1));
            continue;
        }
        ctx.percent(20.0 * i as f64 / total as f64);
        let t = if engine == "vision" {
            ctx.note(format!("视觉模型识字：第 {} / {} 页", i + 1, total));
            ocr_one(ep, model, pic).await?
        } else {
            ctx.note(format!("{}识字：第 {} / {} 页", if engine == "api" { "在线接口" } else { "本机增强" }, i + 1, total));
            let (bgra, w, h) = decode_bgra(&pic.1)?;
            lines_to_text(&recognize_lines(&ctx.st, engine, bgra, w, h).await?)
        };
        if !t.is_empty() {
            out.push(t);
        }
    }
    let text = out.join("\n\n");
    if !worth(&text) {
        return Err(AppError::invalid(if engine == "vision" {
            "视觉模型没有从图片里识别出文字。确认选的模型支持看图，或图片是否清晰。"
        } else {
            "没有从图片里识别出文字。确认图片清晰，识字语言选对了（“屏幕翻译 → 识字”里设置）。"
        }));
    }
    if let Some(dir) = cp.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&cp, &text);
    Ok(text)
}

pub(crate) fn cache_path(data_dir: &Path, input: &Path, target: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let len = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);
    let mut h = Sha256::new();
    h.update(input.to_string_lossy().as_bytes());
    h.update(len.to_le_bytes());
    h.update(target.as_bytes());
    let hex: String = h.finalize().iter().take(10).map(|b| format!("{b:02x}")).collect();
    data_dir.join("filetr").join(format!("{hex}.json"))
}

pub(crate) fn load_cache(path: &Path) -> Option<View> {
    let mut v: View = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    // 旧缓存（没有页状态）：当时每页都是译完才存的，都算处理过了
    if v.v < VIEW_V {
        for p in &mut v.pages {
            p.done = true;
        }
    }
    Some(v)
}

/// 用旧缓存里的译文填新的片段列表：顺序和原文都一样就一一对应（含你改过的译文）；
/// 对不上（换了排版方式、文件内容变了）就按原文一句句找，找到的照用。
pub(crate) fn fill_from_cache(old: &View, texts: &[String]) -> Vec<Option<String>> {
    let flat: Vec<(&String, &Option<String>)> = old.pages.iter().flat_map(|p| p.src.iter().zip(p.trans.iter())).collect();
    let keep = |t: &Option<String>| t.clone().filter(|x| !x.trim().is_empty());
    if flat.len() == texts.len() && flat.iter().zip(texts).all(|((s, _), t)| *s == t) {
        return flat.iter().map(|(_, t)| keep(t)).collect();
    }
    let mut by_src: std::collections::HashMap<&str, Option<String>> = std::collections::HashMap::new();
    for (s, t) in &flat {
        if let Some(t) = keep(t) {
            by_src.entry(s.as_str()).or_insert(Some(t));
        }
    }
    texts.iter().map(|t| by_src.get(t.as_str()).cloned().flatten()).collect()
}

pub(crate) fn save_cache(path: &Path, v: &View) {
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
    let kind = kind_of(path).ok_or_else(|| AppError::invalid("支持 TXT、Markdown、PDF、SRT/VTT/ASS 字幕、Word(docx)、EPUB，以及图片（PNG / JPG / WebP）。"))?;
    let mut v = View { input: path.to_string_lossy().into_owned(), v: VIEW_V, ..Default::default() };
    if kind == Kind::Sub {
        let cues = crate::subtitle_tools::read(path)?;
        let texts: Vec<String> = cues.iter().map(|c| c.lines.join(" ")).collect();
        let mut at = 0;
        for n in page_sizes(&[], texts.len(), true) {
            v.pages.push(ViewPage { src: texts[at..at + n].to_vec(), trans: vec![None; n], ..Default::default() });
            at += n;
        }
    } else {
        let loaded = match load(path, kind) {
            Ok(l) => l,
            // 图片和扫描版 PDF 要先识字，识字在开始翻译时做；这里只给空的阅读页
            Err(e) if kind == Kind::Img || (kind == Kind::Pdf && e.to_string().contains(SCANNED)) => vec![],
            Err(e) => return Err(e),
        };
        for p in loaded {
            let src: Vec<String> = p.slots.iter().map(|s| s.text.clone()).collect();
            v.pages.push(ViewPage { trans: vec![None; src.len()], src, ..Default::default() });
        }
    }
    v.pages.retain(|p| !p.src.is_empty());
    Ok(v)
}

/// 翻译一份文件，返回输出文件路径。
/// 文字版 PDF：把译文按块写回原位置，存成新的 PDF。`trans` 和各页的块按顺序一一对应。
async fn write_pdf_layout(
    ctx: &JobCtx,
    input: &Path,
    lib: &Path,
    pages: &[crate::pdfx::PageInfo],
    trans: &[Option<String>],
    mode: crate::pdfx::Mode,
    base: &str,
) -> AppResult<PathBuf> {
    use crate::pdfx::{Item, PagePlan};
    let mut plans = vec![PagePlan::default(); pages.len()];
    let mut at = 0usize;
    for (pi, p) in pages.iter().enumerate() {
        for b in &p.blocks {
            if let Some(Some(t)) = trans.get(at) {
                plans[pi].items.push(Item { remove: b.runs.clone(), rect: (b.l, b.b, b.r, b.t), size: b.size, rgb: b.rgb, bg: None, text: t.clone() });
            }
            at += 1;
        }
    }
    // 译文先用原文所在处的背景色盖一层再写（和版面画布里看到的一样）：原文就算有一部分没被删干净（图片里的字、描边字）也不会透出来
    {
        let sizes: Vec<(f32, f32)> = pages.iter().map(|p| (p.w, p.h)).collect();
        let (lib2, inp) = (lib.to_path_buf(), input.to_path_buf());
        let mut taken = std::mem::take(&mut plans);
        plans = tokio::task::spawn_blocking(move || {
            for (pi, plan) in taken.iter_mut().enumerate() {
                if plan.items.is_empty() {
                    continue;
                }
                if let Ok((w, h, rgba)) = crate::pdfx::render_page(&lib2, &inp, pi, 900) {
                    for it in &mut plan.items {
                        it.bg = Some(sample_rect_bg(&rgba, w, h, sizes[pi], it.rect));
                    }
                }
            }
            taken
        })
        .await
        .map_err(|e| AppError::msg(e.to_string()))?;
    }
    let font = crate::pdfx::load_font(&ctx.st.data_dir)?;
    let tag = if mode == crate::pdfx::Mode::Interleave { "双语" } else { "译文" };
    let out = ai::out_beside(input, base, tag, "pdf");
    ctx.note("正在把译文写回 PDF…");
    let (lib2, inp, out2) = (lib.to_path_buf(), input.to_path_buf(), out.clone());
    tokio::task::spawn_blocking(move || crate::pdfx::apply(&lib2, &font, &inp, &out2, &plans, mode, |_, _| {}))
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
    Ok(out)
}

/// 扫描版 PDF：逐页渲染成图，让视觉模型识字、翻译并给出位置，再用背景色盖住原文写上译文。
/// 扫描页上的一块：原文、译文和它在页面上的位置（PDF 坐标：左、下、右、上）。存成旁路文件，
/// 这样“先预览、改译文、再生成”时不用重新识字和翻译，改过的译文也能用上。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ScanBlock {
    src: String,
    trans: String,
    rect: Option<(f32, f32, f32, f32)>,
    size: f32,
    rgb: [u8; 3],
    bg: Option<[u8; 3]>,
}

pub(crate) fn scan_path(data_dir: &Path, input: &Path, target: &str, engine: &str) -> PathBuf {
    cache_path(data_dir, input, &format!("scan-{target}-{engine}")).with_extension("scan.json")
}

/// 一页里的像素框（左、上、右、下）换成 PDF 坐标（左、下、右、上）。
fn pdf_rect(px: (f32, f32, f32, f32), img: (usize, usize), page: (f32, f32)) -> (f32, f32, f32, f32) {
    let (iw, ih) = (img.0.max(1) as f32, img.1.max(1) as f32);
    let (left, top, right, bottom) = (px.0 / iw * page.0, px.1 / ih * page.1, px.2 / iw * page.0, px.3 / ih * page.1);
    (left, page.1 - bottom, right, page.1 - top)
}

#[allow(clippy::too_many_arguments)]
async fn run_pdf_scanned(
    ctx: &JobCtx,
    input: &Path,
    opts: &Opts,
    lib: &Path,
    ep: &ai::Endpoint,
    settings: &crate::settings::Settings,
    target: &str,
    pages: Vec<crate::pdfx::PageInfo>,
) -> AppResult<PathBuf> {
    use crate::pdfx::{contrast, png_of_rgba, sample_bg, Item, PagePlan};
    use base64::Engine;
    let engine = opts.engine();
    let data_dir = ctx.st.data_dir.clone();
    let (sp, vp) = (scan_path(&data_dir, input, target, engine), cache_path(&data_dir, input, target));

    // 1. 上次识别、翻译好的（含在阅读器里改过的译文）直接用
    let mut scan: Option<Vec<Vec<ScanBlock>>> = std::fs::read(&sp).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(sc) = &mut scan {
        match load_cache(&vp) {
            Some(v) if v.failed == 0 && v.pages.len() == sc.len() && v.pages.iter().zip(sc.iter()).all(|(a, b)| a.trans.len() == b.len()) => {
                for (page, vpage) in sc.iter_mut().zip(&v.pages) {
                    for (b, t) in page.iter_mut().zip(&vpage.trans) {
                        b.trans = t.clone().unwrap_or_default();
                    }
                }
                ctx.note("用上次识别、翻译好的内容（含你在阅读器里改过的译文），不再重新识字");
            }
            _ => scan = None,
        }
    }

    // 2. 没有就识字 + 翻译
    let scan = match scan {
        Some(s) => s,
        None => {
            let total = pages.len().min(OCR_MAX_PAGES);
            if pages.len() > OCR_MAX_PAGES {
                ctx.note(format!("共 {} 页，只处理前 {OCR_MAX_PAGES} 页", pages.len()));
            }
            let mut out: Vec<Vec<ScanBlock>> = vec![];
            let mut failed = 0usize;
            for (i, page) in pages.iter().enumerate().take(total) {
                ctx.check()?;
                ctx.percent(90.0 * i as f64 / total as f64);
                let (l2, in2) = (lib.to_path_buf(), input.to_path_buf());
                let width = if engine == "vision" { 1500 } else { 2000 };
                let (w, h, rgba) =
                    tokio::task::spawn_blocking(move || crate::pdfx::render_page(&l2, &in2, i, width)).await.map_err(|e| AppError::msg(e.to_string()))??;
                let mut blocks: Vec<ScanBlock> = vec![];
                if engine == "vision" {
                    ctx.note(format!("扫描页：视觉模型识字并翻译 第 {} / {} 页", i + 1, total));
                    let png = png_of_rgba(w, h, &rgba)?;
                    let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png));
                    let resp = match ai::chat_img(ep, &settings.ai.model, &crate::imgtrans::system(target), "Translate the text in this image.", &url).await {
                        Ok(r) => r,
                        Err(e) => {
                            let e = crate::imgtrans::vision_hint(e);
                            if i == 0 {
                                return Err(e);
                            }
                            ctx.note(format!("第 {} 页识别失败，已跳过：{}", i + 1, e.message));
                            failed += 1;
                            out.push(vec![]);
                            continue;
                        }
                    };
                    for b in crate::imgtrans::parse_blocks(&resp).unwrap_or_default() {
                        if b.translation.trim().is_empty() {
                            continue;
                        }
                        let Some([x, y, bw, bh]) = b.rect else {
                            blocks.push(ScanBlock { src: b.text, trans: b.translation, rect: None, size: 10.0, rgb: [0, 0, 0], bg: None });
                            continue;
                        };
                        let px = (x as f32 * w as f32, y as f32 * h as f32, (x + bw) as f32 * w as f32, (y + bh) as f32 * h as f32);
                        let rect = pdf_rect(px, (w, h), (page.w, page.h));
                        let bgbox = (px.0 as usize, px.1 as usize, (px.2 as usize).min(w - 1), (px.3 as usize).min(h - 1));
                        let bg = sample_bg(&rgba, w, h, bgbox);
                        let n = b.translation.chars().count().max(1) as f32;
                        let size = ((rect.2 - rect.0) * (rect.3 - rect.1) / (n * 1.4)).sqrt().clamp(6.5, 16.0);
                        blocks.push(ScanBlock { src: b.text, trans: b.translation, rect: Some(rect), size, rgb: contrast(bg), bg: Some(bg) });
                    }
                } else {
                    ctx.note(format!("扫描页：{}识字 第 {} / {} 页", if engine == "api" { "在线接口" } else { "本机增强" }, i + 1, total));
                    let mut bgra = rgba.clone();
                    for p in bgra.chunks_exact_mut(4) {
                        p.swap(0, 2);
                    }
                    let lines = match recognize_lines(&ctx.st, engine, bgra, w, h).await {
                        Ok(l) => l,
                        Err(e) => {
                            if i == 0 {
                                return Err(e);
                            }
                            ctx.note(format!("第 {} 页识字失败，已跳过：{}", i + 1, e.message));
                            failed += 1;
                            out.push(vec![]);
                            continue;
                        }
                    };
                    let groups: Vec<_> = crate::livecore::group_lines(&lines).into_iter().filter(|g| worth(&g.text)).collect();
                    if !groups.is_empty() {
                        ctx.note(format!("扫描页：翻译 第 {} / {} 页（{} 块）", i + 1, total, groups.len()));
                        let texts: Vec<String> = groups.iter().map(|g| g.text.clone()).collect();
                        let tr = translate_texts(&ctx.st, settings, ep, target, &texts, &|_, _| ctx.check()).await?;
                        for (g, t) in groups.iter().zip(tr.texts) {
                            let Some(t) = t.filter(|t| !t.trim().is_empty()) else { continue };
                            let px = (g.x, g.y, g.x + g.w, g.y + g.h);
                            let rect = pdf_rect(px, (w, h), (page.w, page.h));
                            let bgbox = (px.0.max(0.0) as usize, px.1.max(0.0) as usize, (px.2 as usize).min(w - 1), (px.3 as usize).min(h - 1));
                            let bg = sample_bg(&rgba, w, h, bgbox);
                            let size = (g.em / h as f32 * page.h * 0.9).clamp(6.5, 28.0);
                            blocks.push(ScanBlock { src: g.text.clone(), trans: t, rect: Some(rect), size, rgb: contrast(bg), bg: Some(bg) });
                        }
                    }
                }
                out.push(blocks);
            }
            if out.iter().all(|b| b.iter().all(|x| x.rect.is_none())) {
                return Err(AppError::invalid(if engine == "vision" {
                    "视觉模型没有从页面里识别出带位置的文字。确认选的模型支持看图，或页面是否清晰。"
                } else {
                    "没有从页面里识别出文字。确认页面清晰，识字语言选对了（“屏幕翻译 → 识字”里设置）。"
                }));
            }
            let view = View {
                input: input.to_string_lossy().into_owned(),
                target: target.to_string(),
                bilingual: false,
                pages: out
                    .iter()
                    .enumerate()
                    .map(|(i, p)| ViewPage {
                        src: p.iter().map(|b| b.src.clone()).collect(),
                        trans: p.iter().map(|b| Some(b.trans.clone())).collect(),
                        page: Some(i),
                        done: true,
                        skip: vec![],
                    })
                    .collect(),
                failed,
                v: VIEW_V,
                layout: false,
                total_pages: pages.len(),
            };
            save_cache(&vp, &view);
            if let Ok(b) = serde_json::to_vec(&out) {
                let _ = std::fs::write(&sp, b);
            }
            out
        }
    };

    if opts.preview {
        ctx.percent(100.0);
        ctx.note("已识字、翻译完。在右边检查、直接改译文，满意后点“生成文件”。");
        return Ok(input.to_path_buf());
    }

    // 3. 写回 PDF（译文被改成空的块不写，保留原样）
    let mut plans = vec![PagePlan::default(); pages.len()];
    let mut any = 0usize;
    for (pi, page) in scan.iter().enumerate().take(plans.len()) {
        for b in page {
            let (Some(rect), false) = (b.rect, b.trans.trim().is_empty()) else { continue };
            plans[pi].items.push(Item { remove: vec![], rect, size: b.size, rgb: b.rgb, bg: b.bg, text: b.trans.clone() });
            any += 1;
        }
    }
    if any == 0 {
        return Err(AppError::invalid("没有可以写回页面的译文。"));
    }
    let font = crate::pdfx::load_font(&data_dir)?;
    let mode = crate::pdfx::Mode::parse(&opts.pdf_mode);
    let base = ai::strip_lang_suffix(&crate::ai::stem_of(input)).to_string();
    let out = ai::out_beside(input, &base, if mode == crate::pdfx::Mode::Interleave { "双语" } else { "译文" }, "pdf");
    ctx.note("正在把译文写回 PDF…");
    let (l2, in2, out2) = (lib.to_path_buf(), input.to_path_buf(), out.clone());
    tokio::task::spawn_blocking(move || crate::pdfx::apply(&l2, &font, &in2, &out2, &plans, mode, |_, _| {}))
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
    ctx.percent(100.0);
    Ok(out)
}

/// 翻译用的系统提示词和术语（术语库 + AI 专家）。字幕文件用字幕档案（和工具箱、摘要面板一样），其余用文档档案。
fn trans_setup(st: &crate::AppState, settings: &crate::settings::Settings, target: &str, profile: crate::tm::Profile) -> (String, Vec<crate::glossary::Term>) {
    let terms = if settings.ai.glossary_on { st.glossary.terms_for("", ai::lang_tag(target)) } else { vec![] };
    let guide = st.experts.resolve(&settings.ai.expert, settings.ai.expert_auto, "").map(|e| crate::experts::guide_block(&e)).unwrap_or_default();
    let base = if profile == crate::tm::Profile::Subtitle { ai::translate_system(target) } else { crate::vsum::webtrans_system(target) };
    (format!("{base}{guide}"), terms)
}

fn profile_of(kind: Kind) -> crate::tm::Profile {
    if kind == Kind::Sub {
        crate::tm::Profile::Subtitle
    } else {
        crate::tm::Profile::Document
    }
}

/// 翻译一批文字；返回和输入等长，失败的那条是 None。
async fn translate_texts(
    st: &crate::AppState,
    settings: &crate::settings::Settings,
    ep: &ai::Endpoint,
    target: &str,
    texts: &[String],
    check: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<ai::Translated> {
    let (system, terms) = trans_setup(st, settings, target, crate::tm::Profile::Document);
    let work: Vec<Cue> = texts.iter().enumerate().map(|(k, t)| Cue { start: k as u64, end: k as u64 + 1, lines: vec![t.clone()] }).collect();
    ai::map_cues_g(ep, &settings.ai.model, &system, &work, settings.ai.batch_size.clamp(8, 30), &terms, check).await
}

/// 版面里的一块：原文、译文和它在页面上的位置（PDF 点，左 / 下 / 右 / 上）。
struct LBlock {
    /// 这一块在阅读器那一页里的序号（编辑译文用）
    index: usize,
    rect: (f32, f32, f32, f32),
    size: f32,
    rgb: [u8; 3],
    bg: Option<[u8; 3]>,
    remove: Vec<usize>,
    src: String,
    trans: String,
    /// 按设置不翻译的块（页眉页脚、公式代码、参考文献）
    skipped: bool,
}

struct Collected {
    lib: PathBuf,
    pages: usize,
    /// 阅读器里对应的页序号（文字版 PDF 里没有文字的页不占位）
    view_page: usize,
    size: (f32, f32),
    blocks: Vec<LBlock>,
    /// 这一页处理过了（译完或失败）；还没轮到时是 false
    done: bool,
    /// 要告诉用户的情况（没译、重新匹配过、扫描页没识字……）；没有就是空
    note: String,
}

/// 版面里一页的块和缓存里的译文对上之后的结果。
#[derive(Debug, Default, PartialEq)]
pub(crate) struct PageMatch {
    pub trans: Vec<Option<String>>,
    pub skip: Vec<usize>,
    /// 这一页在缓存（阅读器）里的序号
    pub view_page: usize,
    pub done: bool,
    pub note: String,
    /// 缓存被改动了（重新匹配过、给还没轮到的页占了位），要存回去
    pub dirty: bool,
}

/// 把 PDF 某一页（有文字块的）和缓存里的译文对上。只看这一页：别的页没译完、失败、取消都不影响；
/// 这一页的缓存和当前的块对不上时，按原文一句句找，找到的照用，把缓存改成当前的块，没找到的当“未译”。
pub(crate) fn match_cached_page(view: &mut Option<View>, pages: &[crate::pdfx::PageInfo], page: usize) -> PageMatch {
    let texts: Vec<String> = pages[page].blocks.iter().map(|b| b.text.clone()).collect();
    let mut m = PageMatch { trans: vec![None; texts.len()], view_page: pages[..page].iter().filter(|p| !p.blocks.is_empty()).count(), ..Default::default() };
    let Some(v) = view.as_mut() else {
        m.note = "这份文件还没有译文：先点“翻译”（或任务里的“继续”）。".into();
        return m;
    };
    if !v.layout && v.pages.iter().all(|p| p.page.is_none()) {
        // 旧缓存（没记页号）：只有总数对得上才能用；对不上说明当时是按段落切的
        let flat: Vec<&Option<String>> = v.pages.iter().flat_map(|p| p.trans.iter()).collect();
        let total: usize = pages.iter().map(|p| p.blocks.len()).sum();
        if flat.len() == total {
            let before: usize = pages[..page].iter().map(|p| p.blocks.len()).sum();
            for (k, t) in m.trans.iter_mut().enumerate() {
                *t = flat.get(before + k).and_then(|x| x.as_deref()).map(str::to_string);
            }
            m.done = true;
        } else {
            m.note = "这份译文是按段落切的（装 PDF 排版组件之前译的），对不上页面上的块。点任务里的“重新翻译”，按版面重译一次。".into();
        }
        return m;
    }
    match v.pages.iter().position(|p| p.page == Some(page)) {
        Some(i) => {
            let vp = &mut v.pages[i];
            m.view_page = i;
            m.done = vp.done;
            if vp.src == texts {
                m.trans = vp.trans.clone();
                m.skip = vp.skip.clone();
            } else {
                let skipped_src: std::collections::HashSet<&str> = vp.skip.iter().filter_map(|k| vp.src.get(*k)).map(String::as_str).collect();
                let by_src: std::collections::HashMap<&str, String> =
                    vp.src.iter().zip(&vp.trans).filter_map(|(s, t)| t.clone().filter(|x| !x.trim().is_empty()).map(|t| (s.as_str(), t))).collect();
                m.trans = texts.iter().map(|t| by_src.get(t.as_str()).cloned()).collect();
                m.skip = texts.iter().enumerate().filter(|(_, t)| skipped_src.contains(t.as_str())).map(|(k, _)| k).collect();
                let missing = (0..texts.len()).filter(|k| m.trans[*k].is_none() && !m.skip.contains(k)).count();
                vp.src = texts;
                vp.trans = m.trans.clone();
                vp.skip = m.skip.clone();
                m.note = format!("这一页的文字和译文缓存对不上（可能换了文件或排版方式），已按原文重新匹配，{missing} 段没有匹配上，显示为“未译”。");
                m.dirty = true;
            }
        }
        None => {
            // 这一页还没轮到：先在缓存里占个位，在页面上直接改译文也能存下来
            let at = v.pages.iter().position(|p| p.page.is_some_and(|x| x > page)).unwrap_or(v.pages.len());
            v.pages.insert(at, ViewPage { trans: vec![None; texts.len()], src: texts, page: Some(page), done: false, skip: vec![] });
            m.view_page = at;
            m.dirty = true;
        }
    }
    m
}

/// 取出 PDF 某一页的版面块和缓存里的译文（含阅读器里改过的、扫描页的位置）。`page` 从 0 起。
/// 只看这一页：别的页没译完、失败、取消都不影响；这一页的缓存和当前 PDF 对不上时按原文重新匹配，匹配不上的当“未译”。
async fn collect_page(st: &crate::AppState, input: &Path, target: &str, ocr: &str, page: usize) -> AppResult<Collected> {
    let lib = crate::pdfx::find_lib(&st.data_dir).ok_or_else(|| AppError::invalid("版面预览需要 PDF 排版组件：在“文件翻译”页点“安装 PDF 排版组件”。"))?;
    let (l2, in2) = (lib.clone(), input.to_path_buf());
    let pages = tokio::task::spawn_blocking(move || crate::pdfx::extract(&l2, &in2)).await.map_err(|e| AppError::msg(e.to_string()))??;
    if page >= pages.len() {
        return Err(AppError::invalid("没有这一页。"));
    }
    let cp = cache_path(&st.data_dir, input, target);
    let mut view = load_cache(&cp);
    let size = (pages[page].w, pages[page].h);
    let mut blocks = vec![];
    let mut view_page = page;
    let mut done = false;
    let mut note = String::new();
    if !pages[page].blocks.is_empty() {
        let m = match_cached_page(&mut view, &pages, page);
        if m.dirty {
            if let Some(v) = &view {
                save_cache(&cp, v);
            }
        }
        let (trans, skip) = (m.trans, m.skip);
        view_page = m.view_page;
        done = m.done;
        note = m.note;
        for (k, b) in pages[page].blocks.iter().enumerate() {
            blocks.push(LBlock {
                index: k,
                rect: (b.l, b.b, b.r, b.t),
                size: b.size,
                rgb: b.rgb,
                bg: None,
                remove: b.runs.clone(),
                src: b.text.clone(),
                trans: trans[k].clone().unwrap_or_default(),
                skipped: skip.contains(&k),
            });
        }
    } else {
        // 扫描页：旁路文件里有位置，阅读器里改过的译文优先
        let engine = Opts { ocr: ocr.to_string(), ..Opts::default() }.engine();
        let sc: Option<Vec<Vec<ScanBlock>>> = std::fs::read(scan_path(&st.data_dir, input, target, engine)).ok().and_then(|b| serde_json::from_slice(&b).ok());
        match sc {
            None => note = "这一页是扫描页（没有文字层），还没有识字、翻译结果。".into(),
            Some(sc) => {
                if let Some(list) = sc.get(page) {
                    let edited = view.as_ref().and_then(|v| v.pages.get(page)).filter(|v| v.trans.len() == list.len());
                    for (k, b) in list.iter().enumerate() {
                        let Some(rect) = b.rect else { continue };
                        let trans = edited.map(|v| v.trans[k].clone().unwrap_or_default()).unwrap_or_else(|| b.trans.clone());
                        blocks.push(LBlock { index: k, rect, size: b.size, rgb: b.rgb, bg: b.bg, remove: vec![], src: b.src.clone(), trans, skipped: false });
                    }
                    done = true;
                }
            }
        }
    }
    Ok(Collected { lib, pages: pages.len(), view_page, size, blocks, done, note })
}

/// 在渲染好的整页图上取一块文字的背景色（PDF 点 → 像素；PDF 的 y 轴朝上）。
pub(crate) fn sample_rect_bg(rgba: &[u8], w: usize, h: usize, page: (f32, f32), rect: (f32, f32, f32, f32)) -> [u8; 3] {
    let (sx, sy) = (w as f32 / page.0.max(1.0), h as f32 / page.1.max(1.0));
    let px = |v: f32, max: usize| (v.max(0.0) as usize).min(max.saturating_sub(1));
    crate::pdfx::sample_bg(rgba, w, h, (px(rect.0 * sx, w), px((page.1 - rect.3) * sy, h), px(rect.2 * sx, w), px((page.1 - rect.1) * sy, h)))
}

/// 版面预览 / 画布用的一页数据：原页图 + 每块的位置、背景色、原文和译文。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutBlock {
    pub index: usize,
    /// 左、下、右、上（PDF 点）
    pub rect: [f32; 4],
    pub size: f32,
    pub rgb: [u8; 3],
    pub bg: [u8; 3],
    pub src: String,
    pub trans: String,
    /// 按设置不翻译（页眉页脚、公式代码、参考文献）：保留原文，不算“未译”
    pub skipped: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayout {
    /// PDF 总页数
    pub pages: usize,
    /// 阅读器里这一页的序号（编辑译文时用）
    pub view_page: usize,
    pub w: f32,
    pub h: f32,
    /// 原页图（data URL）
    pub image: String,
    pub blocks: Vec<LayoutBlock>,
    /// 这一页处理过了（译完或失败）；还没轮到的页是 false
    pub done: bool,
    /// 要告诉用户的情况：没译、重新匹配过、扫描页没识字……（没有就是空）
    pub note: String,
}

/// 画布：把某一页渲染成图，连同每一块的位置、背景色和译文一起交给界面，界面里可以切换原文 / 译文、缩放平移、直接改译文。
pub async fn page_layout(st: &crate::AppState, input: &Path, target: &str, ocr: &str, page: usize) -> AppResult<PageLayout> {
    use base64::Engine;
    let c = collect_page(st, input, target, ocr, page).await?;
    let (pages, view_page, size, done, note) = (c.pages, c.view_page, c.size, c.done, c.note);
    let (lib, inp, mut blocks) = (c.lib, input.to_path_buf(), c.blocks);
    let (img, blocks) = tokio::task::spawn_blocking(move || -> AppResult<(Vec<u8>, Vec<LBlock>)> {
        let (w, h, rgba) = crate::pdfx::render_page(&lib, &inp, page, 1400)?;
        for b in &mut blocks {
            b.bg.get_or_insert_with(|| sample_rect_bg(&rgba, w, h, size, b.rect));
        }
        Ok((crate::pdfx::png_of_rgba(w, h, &rgba)?, blocks))
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    let image = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&img));
    let blocks = blocks
        .into_iter()
        .map(|b| LayoutBlock {
            index: b.index,
            rect: [b.rect.0, b.rect.1, b.rect.2, b.rect.3],
            size: b.size,
            rgb: b.rgb,
            bg: b.bg.unwrap_or([255, 255, 255]),
            src: b.src,
            trans: b.trans,
            skipped: b.skipped,
        })
        .collect();
    Ok(PageLayout { pages, view_page, w: size.0, h: size.1, image, blocks, done, note })
}

/// “导出效果”：把译文真的写到原页上（和生成的文件一样的做法：删旧文字、盖背景色、写译文）再渲染，不生成文件。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagePreview {
    pub orig: String,
    pub trans: String,
    pub pages: usize,
    pub items: usize,
}

pub async fn page_preview(st: &crate::AppState, input: &Path, target: &str, ocr: &str, page: usize) -> AppResult<PagePreview> {
    use crate::pdfx::{Item, PagePlan};
    use base64::Engine;
    let c = collect_page(st, input, target, ocr, page).await?;
    let (pages, size) = (c.pages, c.size);
    let font = crate::pdfx::load_font(&st.data_dir)?;
    let (lib, inp, blocks) = (c.lib, input.to_path_buf(), c.blocks);
    let (a, b, items) = tokio::task::spawn_blocking(move || -> AppResult<(Vec<u8>, Vec<u8>, usize)> {
        let (w, h, rgba) = crate::pdfx::render_page(&lib, &inp, page, 900)?;
        let mut plan = PagePlan::default();
        for b in blocks.iter().filter(|b| !b.trans.trim().is_empty()) {
            let bg = b.bg.unwrap_or_else(|| sample_rect_bg(&rgba, w, h, size, b.rect));
            plan.items.push(Item { remove: b.remove.clone(), rect: b.rect, size: b.size, rgb: b.rgb, bg: Some(bg), text: b.trans.clone() });
        }
        let n = plan.items.len();
        let orig = crate::pdfx::png_of_rgba(w, h, &rgba)?;
        let (tw, th, trgba) = crate::pdfx::render_translated(&lib, &font, &inp, page, &plan, 900)?;
        Ok((orig, crate::pdfx::png_of_rgba(tw, th, &trgba)?, n))
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    let url = |p: &[u8]| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(p));
    Ok(PagePreview { orig: url(&a), trans: url(&b), pages, items })
}

/// 阅读器里改了一段译文：写回缓存。空白 = 去掉这段译文（下次生成时保留原文，或重新翻译）。
pub fn edit_seg(data_dir: &Path, input: &Path, target: &str, page: usize, index: usize, text: &str) -> AppResult<()> {
    let cp = cache_path(data_dir, input, target);
    let mut v = load_cache(&cp).ok_or_else(|| AppError::not_found("这个文件还没有译文，先翻译。"))?;
    let src = v.pages.get(page).and_then(|p| p.src.get(index)).cloned().unwrap_or_default();
    let slot = v.pages.get_mut(page).and_then(|p| p.trans.get_mut(index)).ok_or_else(|| AppError::not_found("找不到这一段。"))?;
    let t = text.trim();
    // 人工改过的译文进翻译记忆：以后在网页、字幕、别的文件里遇到同一句也用它
    if let Some(kind) = kind_of(input) {
        crate::tm::store_edited(data_dir, profile_of(kind), target, &src, t);
    }
    *slot = if t.is_empty() { None } else { Some(t.to_string()) };
    v.failed = v.pages.iter().map(|p| p.trans.iter().filter(|t| t.is_none()).count()).sum();
    save_cache(&cp, &v);
    Ok(())
}

/// 阅读器里点“重译”：把这一段原文重新翻一遍，结果写进缓存并返回。
pub async fn retranslate_seg(st: &crate::AppState, input: &Path, target: &str, page: usize, index: usize) -> AppResult<String> {
    let settings = st.settings();
    let cp = cache_path(&st.data_dir, input, target);
    let v = load_cache(&cp).ok_or_else(|| AppError::not_found("这个文件还没有译文，先翻译。"))?;
    let src = v.pages.get(page).and_then(|p| p.src.get(index)).cloned().ok_or_else(|| AppError::not_found("找不到这一段。"))?;
    let ep = ai::chat_endpoint_for(st, &settings, Task::Translate)?;
    let t = translate_texts(st, &settings, &ep, target, &[src], &|_, _| Ok(())).await?;
    let out = t.texts.into_iter().next().flatten().filter(|x| !x.trim().is_empty()).ok_or_else(|| AppError::msg("模型没有返回译文，换个说法或稍后再试。"))?;
    edit_seg(&st.data_dir, input, target, page, index, &out)?;
    Ok(out)
}

pub async fn run(ctx: &JobCtx, input: &Path, opts: Opts) -> AppResult<PathBuf> {
    let kind =
        kind_of(input).ok_or_else(|| AppError::invalid("支持 TXT、Markdown、PDF、SRT/VTT/ASS 字幕、Word(docx)、EPUB，以及图片（PNG / JPG / WebP）。"))?;
    let settings = ctx.st.settings();
    let target = if opts.target.trim().is_empty() { settings.ai.target_lang.clone() } else { opts.target.trim().to_string() };
    let ep = ai::chat_endpoint_for(&ctx.st, &settings, Task::Translate)?;

    // PDF 原处排版需要 PDFium 组件；没装就退回只输出 TXT
    let layout_lib = if kind == Kind::Pdf && opts.pdf_mode != "txt" { crate::pdfx::find_lib(&ctx.st.data_dir) } else { None };
    if kind == Kind::Pdf && opts.pdf_mode != "txt" && layout_lib.is_none() {
        ctx.note("还没安装 PDF 排版组件，这次只输出 TXT；在“文件翻译”页点“安装 PDF 排版组件”后可以原处排版。");
    }
    let mut layout_pages: Option<Vec<crate::pdfx::PageInfo>> = None;

    // 1. 切片段
    let mut cues: Vec<Cue> = vec![];
    let mut parts: Vec<Part> = vec![];
    // 版面翻译时：每个 part 对应 PDF 的第几页（没有文字的页不占位）
    let mut part_pdf: Vec<usize> = vec![];
    if kind == Kind::Sub {
        cues = crate::subtitle_tools::read(input)?;
    } else {
        if let Some(lib) = &layout_lib {
            let (lib2, inp) = (lib.clone(), input.to_path_buf());
            ctx.note("正在读取 PDF 的版面…");
            let pages = tokio::task::spawn_blocking(move || crate::pdfx::extract(&lib2, &inp)).await.map_err(|e| AppError::msg(e.to_string()))??;
            if pages.iter().any(|p| !p.blocks.is_empty()) {
                parts = pages
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| !p.blocks.is_empty())
                    .map(|(i, p)| Part {
                        name: format!("page:{i}"),
                        raw: String::new(),
                        slots: p.blocks.iter().map(|b| Slot { start: 0, end: 0, text: b.text.clone(), prefix: String::new() }).collect(),
                    })
                    .collect();
                part_pdf = pages.iter().enumerate().filter(|(_, p)| !p.blocks.is_empty()).map(|(i, _)| i).collect();
                layout_pages = Some(pages);
            } else if pages.iter().all(|p| !p.has_text) {
                ctx.note("这份 PDF 是扫描版（没有文字层），改用视觉模型识字并按位置翻译");
                return run_pdf_scanned(ctx, input, &opts, lib, &ep, &settings, &target, pages).await;
            }
        }
        if layout_pages.is_some() {
            // 已经按页面版面切好了
        } else {
            parts = match load(input, kind) {
                Ok(p) => p,
                Err(e) if kind == Kind::Img || (kind == Kind::Pdf && e.to_string().contains(SCANNED)) => {
                    ctx.note("没有可抽取的文字，改用视觉模型识字");
                    let raw = ocr_text(ctx, &ep, &settings.ai.model, input, kind, opts.engine()).await?;
                    let slots = extract_plain(&raw, false);
                    vec![Part { name: String::new(), raw, slots }]
                }
                Err(e) => return Err(e),
            };
        }
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
    // 每一页对应 PDF 的第几页，每个片段在第几页
    let page_no: Vec<Option<usize>> = (0..sizes.len()).map(|pi| if layout_pages.is_some() { part_pdf.get(pi).copied() } else { None }).collect();
    let seg_page: Vec<usize> = sizes.iter().enumerate().flat_map(|(pi, n)| std::iter::repeat(pi).take(*n)).collect();
    // 页范围、不翻译的内容（页眉页脚 / 公式代码 / 参考文献）
    let (selected, skip) = crate::filetr_ext::select_and_skip(&texts, &seg_page, &page_no, &opts, layout_pages.is_some(), kind == Kind::Sub);
    if !selected.iter().any(|s| *s) {
        return Err(AppError::invalid("页范围里没有可以翻译的页。写法如 3-12, 15。"));
    }

    // 缓存：同一个文件、同一目标语言，已经译好的片段不再重翻（也就是“重试失败段落”和“继续”）
    let cpath = cache_path(&ctx.st.data_dir, input, &target);
    let trans_cached = load_cache(&cpath).map(|old| fill_from_cache(&old, &texts)).unwrap_or_else(|| vec![None; texts.len()]);
    let mut trans: Vec<Option<String>> = trans_cached;
    let cached = trans.iter().filter(|t| t.is_some()).count();
    ctx.note(format!("共 {} 段、{} 页，译成{}；已有 {} 段译文可直接用", texts.len(), sizes.len(), target, cached));

    // 2. 翻译（术语库 + AI 专家 + 网页翻译同款提示词），一页一页地译，每页译完就存缓存。
    // 缓存一开始就写好所有页（没轮到的页 done = false）：中途停止、失败、版面预览都能按页对上。
    let profile = profile_of(kind);
    let (system, terms) = trans_setup(&ctx.st, &settings, &target, profile);
    let tv = crate::tm::terms_version(&ctx.st);
    let mut view = View {
        input: input.to_string_lossy().into_owned(),
        target: target.clone(),
        bilingual: opts.bilingual,
        pages: vec![],
        failed: 0,
        v: VIEW_V,
        layout: layout_pages.is_some(),
        total_pages: layout_pages.as_ref().map_or(sizes.len(), |p| p.len()),
    };
    {
        let mut at = 0usize;
        for (pi, n) in sizes.iter().enumerate() {
            view.pages.push(ViewPage {
                src: texts[at..at + n].to_vec(),
                trans: trans[at..at + n].to_vec(),
                page: page_no[pi],
                done: false,
                skip: (at..at + n).filter(|i| skip.contains(i)).map(|i| i - at).collect(),
            });
            at += n;
        }
        save_cache(&cpath, &view);
    }
    let work_total: usize = sizes.iter().enumerate().filter(|(pi, _)| selected[*pi]).map(|(_, n)| *n).sum();
    let mut work_done = 0usize;
    let mut at = 0usize;
    for (pi, n) in sizes.iter().enumerate() {
        if !selected[pi] {
            at += n;
            continue;
        }
        ctx.check()?;
        let mut todo: Vec<usize> = (at..at + n).filter(|i| trans[*i].is_none() && !skip.contains(i)).collect();
        // 翻译记忆：别处（网页、字幕、别的文件）译过的句子、人工改过的译文直接用
        if !todo.is_empty() {
            let src: Vec<String> = todo.iter().map(|i| texts[*i].clone()).collect();
            for (i, hit) in todo.iter().zip(crate::tm::lookup(&ctx.st.data_dir, profile, &target, &tv, &src)) {
                if hit.is_some() {
                    trans[*i] = hit;
                }
            }
            todo.retain(|i| trans[*i].is_none());
        }
        if !todo.is_empty() {
            let have = trans.iter().filter(|t| t.is_some()).count();
            ctx.note(format!("共 {} 页 · 正在译第 {} 页 · 已缓存 {} 段", sizes.len(), pi + 1, have));
            let work: Vec<Cue> = todo.iter().enumerate().map(|(k, i)| Cue { start: k as u64, end: k as u64 + 1, lines: vec![texts[*i].clone()] }).collect();
            let prog = |_: usize, _: usize| -> AppResult<()> { ctx.check() };
            let t = ai::map_cues_g(&ep, &settings.ai.model, &system, &work, settings.ai.batch_size.clamp(8, 30), &terms, &prog).await?;
            for (k, i) in todo.iter().enumerate() {
                trans[*i] = t.texts.get(k).cloned().flatten().filter(|x| !x.trim().is_empty());
            }
            let pairs: Vec<(String, String)> = todo.iter().filter_map(|i| trans[*i].clone().map(|t| (texts[*i].clone(), t))).collect();
            crate::tm::store(&ctx.st.data_dir, profile, &target, &tv, "pool", &pairs);
        }
        view.pages[pi].trans = trans[at..at + n].to_vec();
        view.pages[pi].done = true;
        view.failed =
            view.pages.iter().filter(|p| p.done).map(|p| p.trans.iter().enumerate().filter(|(k, t)| t.is_none() && !p.skip.contains(k)).count()).sum();
        save_cache(&cpath, &view);
        at += n;
        work_done += n;
        ctx.percent(95.0 * work_done as f64 / work_total.max(1) as f64);
    }
    let failed = view.failed;
    if opts.preview {
        ctx.percent(100.0);
        ctx.note("已译好。在右边检查、直接改译文，满意后点“生成文件”。");
        return Ok(input.to_path_buf());
    }
    let t = ai::Translated { texts: trans, failed };
    let tag = if opts.bilingual { "双语" } else { "译文" };
    let base = ai::strip_lang_suffix(&crate::ai::stem_of(input)).to_string();

    // 3. 写回
    let out = if let (Some(pages), Some(lib)) = (&layout_pages, &layout_lib) {
        write_pdf_layout(ctx, input, lib, pages, &t.texts, crate::pdfx::Mode::parse(&opts.pdf_mode), &base).await?
    } else if kind == Kind::Sub {
        let d = if opts.bilingual { Display::OrigTop } else { Display::Trans };
        let out = ai::out_beside(input, &base, tag, "srt");
        std::fs::write(&out, subtitle::format_srt(&compose(&cues, &t.texts, d)))?;
        ai::register_subtitle(&ctx.st, input, &out, ai::lang_tag(&target));
        out
    } else {
        let ext = if matches!(kind, Kind::Pdf | Kind::Img) {
            "txt".to_string()
        } else {
            input.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
        };
        let out = ai::out_beside(input, &base, tag, &ext);
        let mut offset = 0usize;
        let mut rendered = std::collections::HashMap::new();
        for p in &parts {
            rendered.insert(p.name.clone(), render_part(kind, p, &t.texts, offset, opts.bilingual));
            offset += p.slots.len();
        }
        match kind {
            Kind::Txt | Kind::Md | Kind::Pdf | Kind::Img => std::fs::write(&out, rendered.remove("").unwrap_or_default())?,
            _ => write_zip(input, &out, &rendered)?,
        }
        out
    };
    ctx.percent(100.0);
    if t.failed > 0 {
        ctx.note(format!("有 {} 段没有译文（模型没返回），已保留原文：{}。", t.failed, failed_pages_text(&view)));
    }
    Ok(out)
}

/// “第 9 页 2 段、第 12 页 1 段”：哪几页有没译出来的段（最多列 6 页）。
pub(crate) fn failed_pages_text(v: &View) -> String {
    let list: Vec<String> = v
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.done)
        .filter_map(|(i, p)| {
            let n = p.trans.iter().enumerate().filter(|(k, t)| t.is_none() && !p.skip.contains(k)).count();
            (n > 0).then(|| format!("第 {} 页 {n} 段", p.page.unwrap_or(i) + 1))
        })
        .collect();
    let more = list.len().saturating_sub(6);
    let mut text = list.into_iter().take(6).collect::<Vec<_>>().join("、");
    if more > 0 {
        text.push_str(&format!(" 等 {} 页", more + 6));
    }
    text
}

#[cfg(test)]
#[path = "filetr_tests.rs"]
mod tests;
