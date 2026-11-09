//! 文档格式保真：一段里有几种格式（粗体、斜体、颜色、字号不同的文字）时，不再把整段译文塞进第一个文字块，
//! 而是给每种格式的文字加上 `<r1>…</r1>` 这样的标记再交给模型翻译，译完按标记把每一段译文放回对应的格式里。
//! 支持 Word（`w:p`）、PowerPoint（`a:p`）、Excel 共享字符串（`si`）。
//!
//! 只在“安全”的段落上用标记：段落里除了普通文字块（只有格式和文字）、批注拼写标记之外没有别的东西
//! （没有超链接、域、制表符、换行、图片夹在中间）。其余段落沿用原来的做法（第一个文字块放译文，其余清空），不会更差。
//! 模型没按要求保留标记时，退回到“整段用第一种格式”，同样不会丢译文。

use std::sync::LazyLock;

use regex::Regex;

use crate::filetr::{worth, xml_escape, xml_unescape, Slot};

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?r\d{1,3}>").unwrap());
static OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<r(\d{1,3})>").unwrap());

/// 去掉所有格式标记，得到纯文字（阅读器显示、检查、导出用）。
pub fn strip(s: &str) -> String {
    TAG.replace_all(s, "").into_owned()
}

pub fn has_marks(s: &str) -> bool {
    TAG.is_match(s)
}

/// 一段同一种格式的文字。`key` 是这种格式在原文里的写法（Word 的 `<w:rPr>…</w:rPr>`），空串 = 没有特别格式。
#[derive(Debug, Clone, PartialEq)]
pub struct Seg {
    pub key: String,
    pub text: String,
}

/// 相邻同格式的合并；只有空白的并进前一段（没有前一段就并进后一段）。
pub fn merge(segs: Vec<Seg>) -> Vec<Seg> {
    let mut out: Vec<Seg> = vec![];
    let mut lead_space = String::new();
    for s in segs {
        if s.text.trim().is_empty() {
            match out.last_mut() {
                Some(p) => p.text.push_str(&s.text),
                None => lead_space.push_str(&s.text),
            }
            continue;
        }
        let mut s = s;
        if !lead_space.is_empty() {
            s.text = format!("{lead_space}{}", s.text);
            lead_space.clear();
        }
        match out.last_mut() {
            Some(p) if p.key == s.key => p.text.push_str(&s.text),
            _ => out.push(s),
        }
    }
    out
}

/// 交给模型的文字：只有一种格式就是纯文字；多种格式时每段包上 `<rN>…</rN>`。首尾空白去掉。
pub fn mark(segs: &[Seg]) -> String {
    if segs.len() == 1 {
        return segs[0].text.trim().to_string();
    }
    let n = segs.len();
    segs.iter()
        .enumerate()
        .map(|(i, s)| {
            let t = if i == 0 { s.text.trim_start() } else { s.text.as_str() };
            let t = if i + 1 == n { t.trim_end() } else { t };
            format!("<r{0}>{t}</r{0}>", i + 1)
        })
        .collect()
}

/// 把译文里的标记拆开，按译文里出现的顺序返回 (第几种格式, 文字)。要求 1..=n 每个编号恰好出现一次、标记外没有别的文字，
/// 否则返回 None（调用方退回“整段用第一种格式”）。
pub fn parse(trans: &str, n: usize) -> Option<Vec<(usize, String)>> {
    let mut out = vec![];
    let mut at = 0usize;
    let mut seen = vec![false; n + 1];
    while let Some(m) = OPEN.captures_at(trans, at) {
        let whole = m.get(0)?;
        if !trans[at..whole.start()].trim().is_empty() {
            return None;
        }
        let id: usize = m[1].parse().ok()?;
        if id == 0 || id > n || seen[id] {
            return None;
        }
        let close = format!("</r{id}>");
        let end = trans[whole.end()..].find(&close)? + whole.end();
        let text = &trans[whole.end()..end];
        if TAG.is_match(text) {
            return None; // 不允许嵌套
        }
        seen[id] = true;
        out.push((id, text.to_string()));
        at = end + close.len();
    }
    if !trans[at..].trim().is_empty() || out.len() != n {
        return None;
    }
    Some(out)
}

// ---------- 通用：一段由若干“文字块”拼成 ----------

struct Syn {
    /// 一段（整个 `w:p` / `a:p` / `si`）
    para: Regex,
    /// 一个文字块（`w:r` / `a:r` / `r`）
    run: Regex,
    /// 文字块里的格式
    rpr: Regex,
    /// 文字块里的文字（捕获组 1 = 内容）
    text: Regex,
    /// 文字块里除格式和文字外允许有的东西
    extra: Regex,
    /// 文字块之间允许有的东西（空白、拼写标记）
    gap: Regex,
    /// 文字块里除了开闭标签什么都不剩
    shell: Regex,
    /// 造一个新文字块
    build: fn(&str, &str) -> String,
}

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

static DOCX: LazyLock<Syn> = LazyLock::new(|| Syn {
    para: re(r"(?s)<w:p(?:\s[^>]*)?>.*?</w:p>"),
    run: re(r"(?s)<w:r(?:\s[^>]*)?>.*?</w:r>"),
    rpr: re(r"(?s)<w:rPr>.*?</w:rPr>|<w:rPr/>"),
    text: re(r"(?s)<w:t(?:\s[^>]*)?>(.*?)</w:t>"),
    extra: re(r"<w:lastRenderedPageBreak/>"),
    gap: re(r"^(?:\s|<w:proofErr[^>]*/>)*$"),
    shell: re(r"^\s*<w:r(?:\s[^>]*)?>\s*</w:r>\s*$"),
    build: |key, t| format!("<w:r>{key}<w:t xml:space=\"preserve\">{}</w:t></w:r>", xml_escape(t)),
});

static PPTX: LazyLock<Syn> = LazyLock::new(|| Syn {
    para: re(r"(?s)<a:p(?:\s[^>]*)?>.*?</a:p>"),
    run: re(r"(?s)<a:r>.*?</a:r>"),
    rpr: re(r"(?s)<a:rPr(?:\s[^>]*)?/>|<a:rPr(?:\s[^>]*)?>.*?</a:rPr>"),
    text: re(r"(?s)<a:t(?:\s[^>]*)?>(.*?)</a:t>|<a:t/>"),
    extra: re(r"^$"),
    gap: re(r"^\s*$"),
    shell: re(r"^\s*<a:r>\s*</a:r>\s*$"),
    build: |key, t| format!("<a:r>{key}<a:t>{}</a:t></a:r>", xml_escape(t)),
});

static XLSX: LazyLock<Syn> = LazyLock::new(|| Syn {
    para: re(r"(?s)<si>.*?</si>"),
    run: re(r"(?s)<r>.*?</r>"),
    rpr: re(r"(?s)<rPr>.*?</rPr>|<rPr/>"),
    text: re(r"(?s)<t(?:\s[^>]*)?>(.*?)</t>|<t/>"),
    extra: re(r"^$"),
    gap: re(r"^\s*$"),
    shell: re(r"^\s*<r>\s*</r>\s*$"),
    build: |key, t| format!("<r>{key}<t xml:space=\"preserve\">{}</t></r>", xml_escape(t)),
});

/// 一段的文字块结构：替换范围 + 各种格式的文字。
struct Para {
    /// 要被新文字块替换掉的范围（从第一个文字块开头到最后一个文字块结尾）
    span: (usize, usize),
    segs: Vec<Seg>,
}

fn analyse(syn: &Syn, para: &str) -> Option<Para> {
    let runs: Vec<_> = syn.run.find_iter(para).collect();
    if runs.is_empty() {
        return None;
    }
    for w in runs.windows(2) {
        if !syn.gap.is_match(&para[w[0].end()..w[1].start()]) {
            return None;
        }
    }
    let mut segs = vec![];
    for r in &runs {
        let run = r.as_str();
        let key = syn.rpr.find(run).map(|m| m.as_str().to_string()).unwrap_or_default();
        let mut text = String::new();
        for c in syn.text.captures_iter(run) {
            text.push_str(&xml_unescape(c.get(1).map_or("", |m| m.as_str())));
        }
        // 文字块里只许有格式、文字（和换页标记）：有制表符、换行、图片、域……整段退回老办法
        let no_rpr = syn.rpr.replace_all(run, "").into_owned();
        let no_text = syn.text.replace_all(&no_rpr, "").into_owned();
        let rest = syn.extra.replace_all(&no_text, "");
        if !syn.shell.is_match(&rest) {
            return None;
        }
        segs.push(Seg { key, text });
    }
    let segs = merge(segs);
    if segs.is_empty() {
        return None;
    }
    Some(Para { span: (runs[0].start(), runs[runs.len() - 1].end()), segs })
}

/// 一段要翻译的文字（多种格式时带标记）。
fn text_of(syn: &Syn, para: &str) -> String {
    if let Some(p) = analyse(syn, para) {
        return mark(&p.segs);
    }
    let all: String = syn.text.captures_iter(para).map(|c| xml_unescape(c.get(1).map_or("", |m| m.as_str()))).collect();
    all.trim().to_string()
}

fn extract(syn: &Syn, xml: &str) -> Vec<Slot> {
    syn.para
        .find_iter(xml)
        .filter_map(|m| {
            let text = text_of(syn, m.as_str());
            worth(&strip(&text)).then(|| Slot { start: m.start(), end: m.end(), text, prefix: String::new() })
        })
        .collect()
}

/// 老办法：第一个文字块放译文，其余清空（段落格式、编号、对齐都保留）。
fn replace_first(syn: &Syn, para: &str, trans: &str) -> String {
    let mut first = true;
    syn.text
        .replace_all(para, |c: &regex::Captures| {
            let whole = c.get(0).map_or("", |m| m.as_str());
            // 保留原来的开标签（含 xml:space 等属性），只换里面的文字
            let open_end = whole.find('>').map_or(0, |i| i + 1);
            let open = if whole.ends_with("/>") { whole.trim_end_matches("/>").to_string() + ">" } else { whole[..open_end].to_string() };
            let close = open.replacen('<', "</", 1);
            let name_end = close.find(|ch: char| ch.is_whitespace() || ch == '>').unwrap_or(close.len());
            let close = format!("{}>", &close[..name_end]);
            if first {
                first = false;
                let open = if open.contains("xml:space") || open.starts_with("<a:") { open } else { open.replacen('>', " xml:space=\"preserve\">", 1) };
                format!("{open}{}{close}", xml_escape(trans))
            } else {
                format!("{open}{close}")
            }
        })
        .into_owned()
}

fn replace(syn: &Syn, para: &str, trans: &str) -> String {
    let plain_first = |t: &str| replace_first(syn, para, &strip(t));
    let Some(p) = analyse(syn, para) else { return plain_first(trans) };
    let n = p.segs.len();
    let pieces: Vec<(usize, String)> = if n > 1 { parse(trans, n).unwrap_or_else(|| vec![(1, strip(trans))]) } else { vec![(1, strip(trans))] };
    let runs: String = pieces.iter().map(|(id, t)| (syn.build)(&p.segs[id - 1].key, t)).collect();
    format!("{}{}{}", &para[..p.span.0], runs, &para[p.span.1..])
}

// ---------- Word ----------

pub fn docx_slots(xml: &str) -> Vec<Slot> {
    extract(&DOCX, xml)
}

pub fn docx_replace(para: &str, trans: &str) -> String {
    replace(&DOCX, para, trans)
}

// ---------- PowerPoint ----------

pub fn pptx_slots(xml: &str) -> Vec<Slot> {
    extract(&PPTX, xml)
}

pub fn pptx_replace(para: &str, trans: &str) -> String {
    replace(&PPTX, para, trans)
}

// ---------- Excel 共享字符串 ----------

static RPH: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<rPh\b.*?</rPh>"));

pub fn xlsx_slots(xml: &str) -> Vec<Slot> {
    XLSX.para
        .find_iter(xml)
        .filter_map(|m| {
            let si = RPH.replace_all(m.as_str(), "");
            let text = if si.contains("<r>") {
                text_of(&XLSX, &si)
            } else {
                XLSX.text.captures_iter(&si).map(|c| xml_unescape(c.get(1).map_or("", |m| m.as_str()))).collect::<String>().trim().to_string()
            };
            let plain = strip(&text);
            (worth(&plain) && !crate::filetr_ext::untranslatable(&plain)).then(|| Slot { start: m.start(), end: m.end(), text, prefix: String::new() })
        })
        .collect()
}

pub fn xlsx_replace(si: &str, trans: &str) -> String {
    let si = RPH.replace_all(si, "");
    if si.contains("<r>") {
        return replace(&XLSX, &si, trans);
    }
    replace_first(&XLSX, &si, &strip(trans))
}

// ---------- EPUB / HTML：行内标签 ----------

static HTML_TOKEN: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)<[^>]*>"));
static TAG_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"^<([A-Za-z][A-Za-z0-9]*)"));
const INLINE: &[&str] = &[
    "a", "b", "i", "em", "strong", "u", "span", "code", "sup", "sub", "small", "big", "mark", "cite", "abbr", "s", "strike", "q", "ruby", "rt", "rp", "font",
    "kbd", "var", "dfn", "bdi",
];

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut sp = false;
    for c in s.chars() {
        if c.is_whitespace() {
            sp = true;
        } else {
            if sp {
                out.push(' ');
            }
            sp = false;
            out.push(c);
        }
    }
    if sp {
        out.push(' ');
    }
    out
}

/// 一个 HTML 块里面的文字按行内标签分段；`key` 是这段外面套着的开标签（按外到内的顺序连在一起）。
/// 里面有换行、图片、注释、块级标签、没配上的标签时返回 None（退回老办法：整块换成纯文字）。
fn html_segs(inner: &str) -> Option<Vec<Seg>> {
    let mut stack: Vec<(String, String)> = vec![]; // (标签名, 开标签原文)
    let mut segs: Vec<Seg> = vec![];
    let mut at = 0usize;
    let key = |st: &[(String, String)]| st.iter().map(|(_, o)| o.as_str()).collect::<String>();
    let push = |segs: &mut Vec<Seg>, st: &[(String, String)], raw: &str| {
        if !raw.is_empty() {
            segs.push(Seg { key: key(st), text: collapse_ws(&xml_unescape(raw)) });
        }
    };
    for m in HTML_TOKEN.find_iter(inner) {
        push(&mut segs, &stack, &inner[at..m.start()]);
        at = m.end();
        let tag = m.as_str();
        if tag.starts_with("<!") || tag.starts_with("<?") || tag.ends_with("/>") {
            return None;
        }
        if let Some(close) = tag.strip_prefix("</") {
            let name = close.trim_end_matches('>').trim().to_ascii_lowercase();
            if stack.pop().map(|(n, _)| n) != Some(name) {
                return None;
            }
        } else {
            let name = TAG_NAME.captures(tag)?[1].to_ascii_lowercase();
            if !INLINE.contains(&name.as_str()) {
                return None;
            }
            stack.push((name, tag.to_string()));
        }
    }
    push(&mut segs, &stack, &inner[at..]);
    if !stack.is_empty() {
        return None;
    }
    let segs = merge(segs);
    (!segs.is_empty()).then_some(segs)
}

fn closers(key: &str) -> String {
    let names: Vec<String> = HTML_TOKEN.find_iter(key).filter_map(|m| TAG_NAME.captures(m.as_str()).map(|c| c[1].to_string())).collect();
    names.iter().rev().map(|n| format!("</{n}>")).collect()
}

/// EPUB 里一个块（`<p>…</p>`）要翻译的文字：里面有几种行内格式（强调、链接、上下标……）时带标记，否则 None（调用方用纯文字）。
pub fn html_text(inner: &str) -> Option<String> {
    let segs = html_segs(inner)?;
    (segs.len() > 1).then(|| mark(&segs))
}

/// 把整块 `<p …>…</p>` 的内容换成译文，行内标签按标记放回；不能按标记放时整块换成纯文字。
pub fn html_replace(block: &str, trans: &str) -> String {
    let (Some(a), Some(b)) = (block.find('>'), block.rfind("</")) else { return block.to_string() };
    if a >= b {
        return block.to_string();
    }
    let inner = &block[a + 1..b];
    let plain = || format!("{}{}{}", &block[..=a], xml_escape(&strip(trans)), &block[b..]);
    let Some(segs) = html_segs(inner) else { return plain() };
    let n = segs.len();
    let pieces = if n > 1 { parse(trans, n).unwrap_or_else(|| vec![(1, strip(trans))]) } else { vec![(1, strip(trans))] };
    let body: String = pieces.iter().map(|(id, t)| format!("{}{}{}", segs[id - 1].key, xml_escape(t), closers(&segs[id - 1].key))).collect();
    format!("{}{}{}", &block[..=a], body, &block[b..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(k: &str, t: &str) -> Seg {
        Seg { key: k.into(), text: t.into() }
    }

    #[test]
    fn marks_round_trip_and_reject_damaged_translations() {
        let segs = merge(vec![seg("", "Click "), seg("<b/>", "here"), seg("<b/>", " now"), seg("", " ")]);
        assert_eq!(segs, vec![seg("", "Click "), seg("<b/>", "here now ")]);
        assert_eq!(mark(&segs), "<r1>Click </r1><r2>here now</r2>");
        assert_eq!(strip("<r1>点击</r1><r2>这里</r2>"), "点击这里");
        // 顺序可以调整
        assert_eq!(parse("<r2>现在</r2><r1>点击这里</r1>", 2), Some(vec![(2, "现在".into()), (1, "点击这里".into())]));
        // 丢了一个、重复、嵌套、标记外有字、编号越界：都不行
        assert_eq!(parse("<r1>点击</r1>", 2), None);
        assert_eq!(parse("<r1>a</r1><r1>b</r1>", 2), None);
        assert_eq!(parse("<r1>a<r2>b</r2></r1>", 2), None);
        assert_eq!(parse("前言<r1>a</r1><r2>b</r2>", 2), None);
        assert_eq!(parse("<r1>a</r1><r3>b</r3>", 2), None);
        // 单一格式不加标记
        assert_eq!(mark(&[seg("", "  Hello ")]), "Hello");
    }

    const P: &str = r#"<w:p w:rsidR="1"><w:pPr><w:jc w:val="center"/></w:pPr><w:r w:rsidR="1"><w:rPr><w:b/></w:rPr><w:t>Hello </w:t></w:r><w:proofErr w:type="spellStart"/><w:r><w:t xml:space="preserve">big </w:t></w:r><w:r><w:t>wor</w:t></w:r><w:r><w:t>ld</w:t></w:r></w:p>"#;

    #[test]
    fn word_paragraph_with_two_formats_is_marked_and_rebuilt_in_translated_order() {
        let slots = docx_slots(P);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].text, "<r1>Hello </r1><r2>big world</r2>");
        let out = docx_replace(P, "<r2>大世界</r2><r1>你好，</r1>");
        assert!(out.starts_with(r#"<w:p w:rsidR="1"><w:pPr><w:jc w:val="center"/></w:pPr>"#), "{out}");
        assert!(
            out.contains(r#"<w:r><w:t xml:space="preserve">大世界</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">你好，</w:t></w:r></w:p>"#),
            "{out}"
        );
        assert!(!out.contains("proofErr"));
    }

    #[test]
    fn damaged_marks_fall_back_to_the_first_format_and_never_lose_text() {
        let out = docx_replace(P, "你好，大世界");
        assert!(out.contains(r#"<w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve">你好，大世界</w:t></w:r></w:p>"#), "{out}");
        let out = docx_replace(P, "<r1>你好</r1>大世界<r2>x</r2>");
        assert!(out.contains("你好大世界x"), "{out}");
    }

    #[test]
    fn paragraphs_with_tabs_links_or_breaks_keep_the_old_behaviour() {
        let p = r#"<w:p><w:r><w:t>Name</w:t></w:r><w:r><w:tab/></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>Value</w:t></w:r></w:p>"#;
        let s = docx_slots(p);
        assert_eq!(s[0].text, "NameValue"); // 没有标记
        let out = docx_replace(p, "名称值");
        assert!(out.contains(r#"<w:t xml:space="preserve">名称值</w:t>"#) && out.contains("<w:t></w:t>") && out.contains("<w:tab/>"), "{out}");
        let link = r#"<w:p><w:r><w:t>See </w:t></w:r><w:hyperlink r:id="r1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>this page</w:t></w:r></w:hyperlink></w:p>"#;
        assert_eq!(docx_slots(link)[0].text, "See this page");
    }

    #[test]
    fn powerpoint_runs_keep_their_formats_and_end_of_paragraph_properties() {
        let p = r#"<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" b="1"/><a:t>Big </a:t></a:r><a:r><a:rPr lang="en-US" i="1"/><a:t>idea</a:t></a:r><a:endParaRPr lang="en-US"/></a:p>"#;
        let s = pptx_slots(p);
        assert_eq!(s[0].text, "<r1>Big </r1><r2>idea</r2>");
        let out = pptx_replace(p, "<r1>大</r1><r2>想法</r2>");
        assert_eq!(
            out,
            r#"<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" b="1"/><a:t>大</a:t></a:r><a:r><a:rPr lang="en-US" i="1"/><a:t>想法</a:t></a:r><a:endParaRPr lang="en-US"/></a:p>"#
        );
        // 软换行：退回老办法，换行保留
        let br = r#"<a:p><a:r><a:rPr/><a:t>Line one</a:t></a:r><a:br/><a:r><a:rPr/><a:t>line two</a:t></a:r></a:p>"#;
        assert_eq!(pptx_slots(br)[0].text, "Line oneline two");
        assert!(pptx_replace(br, "译文").contains("<a:br/>"));
    }

    #[test]
    fn html_inline_formats_are_marked_and_restored() {
        let inner = r#"Read the <em>full</em> <a href="x.html">guide <b>now</b></a>."#;
        let segs = html_segs(inner).unwrap();
        assert_eq!(segs.len(), 5, "{segs:?}");
        assert_eq!(html_text(inner).unwrap(), r#"<r1>Read the </r1><r2>full </r2><r3>guide </r3><r4>now</r4><r5>.</r5>"#);
        let block = format!("<p class=\"c\">{inner}</p>");
        let out = html_replace(&block, "<r1>请阅读</r1><r2>完整</r2><r3>指南</r3><r4>现在</r4><r5>。</r5>");
        assert!(out.starts_with("<p class=\"c\">请阅读<em>完整</em>"), "{out}");
        assert!(out.contains(r#"<a href="x.html">指南</a><a href="x.html"><b>现在</b></a>。</p>"#), "{out}");
        // 单一格式：外面的标签保留
        assert_eq!(html_replace("<p><em>Only</em></p>", "仅此"), "<p><em>仅此</em></p>");
        // 里面有换行 / 图片：整块换成纯文字
        assert_eq!(html_replace("<p>a<br/>b</p>", "译文"), "<p>译文</p>");
        assert_eq!(html_text("a<img src=\"x\"/>b"), None);
    }

    #[test]
    fn excel_shared_strings_plain_and_rich_and_skip_untranslatable() {
        let xml = r#"<sst><si><t>Total revenue</t></si><si><r><rPr><b/></rPr><t>Net</t></r><r><t xml:space="preserve"> profit</t></r></si><si><t>https://example.com/a</t></si><si><t>2024</t></si></sst>"#;
        let s = xlsx_slots(xml);
        assert_eq!(s.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["Total revenue", "<r1>Net</r1><r2> profit</r2>"]);
        assert_eq!(xlsx_replace("<si><t>Total revenue</t></si>", "总收入"), "<si><t xml:space=\"preserve\">总收入</t></si>");
        let rich = &xml[s[1].start..s[1].end];
        assert_eq!(
            xlsx_replace(rich, "<r1>净</r1><r2>利润</r2>"),
            "<si><r><rPr><b/></rPr><t xml:space=\"preserve\">净</t></r><r><t xml:space=\"preserve\">利润</t></r></si>"
        );
    }
}
