//! PDF 原处排版翻译：译文写回原来的位置，版面、图片、页码原样保留。
//!
//! 文字版 PDF：用 PDFium 读出每个文字对象的位置、字号、颜色，按位置聚成“段落块”，翻译后删掉原来的文字对象，
//! 在同一个框里按框宽换行、必要时缩小字号写回译文——背景图和矢量图都没动，所以不需要盖白底。
//! 扫描版 PDF：把页面渲染成图，交给视觉模型识字并给出框，再用取样的背景色盖住原文、写上译文。
//!
//! PDFium 是按需下载的“PDF 排版组件”（动态库，放在数据目录），不随安装包发布；译文字体用系统里的中文字体，
//! 找不到时再下载。本文件前半是不依赖 PDFium 的纯逻辑（可单元测试），后半是和 PDFium 打交道的部分。

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

// =====================================================================
// 纯逻辑：把文字对象聚成段落块
// =====================================================================

/// 页面上的一个文字对象（一行或一段连续的文字）。坐标是 PDF 的点，原点在页面左下角。
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// 在页面对象列表里的序号（写回时用它删掉原文字）
    pub idx: usize,
    pub text: String,
    pub l: f32,
    pub b: f32,
    pub r: f32,
    pub t: f32,
    pub size: f32,
    pub rgb: [u8; 3],
}

/// 聚好的一个段落块。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Block {
    /// 组成这个块的文字对象序号
    pub runs: Vec<usize>,
    pub l: f32,
    pub b: f32,
    pub r: f32,
    pub t: f32,
    pub size: f32,
    pub rgb: [u8; 3],
    pub text: String,
    pub lines: usize,
}

/// 中日韩、全角符号这类“一个字占一个字宽”的字符。
pub fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11FF | 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// 估算一个字符的宽度（字号的倍数）。没有字体度量时用它排版；中文字体是等宽的所以准，英文是估算。
pub fn char_w(c: char, size: f32) -> f32 {
    if is_wide(c) {
        size
    } else if c == ' ' {
        size * 0.3
    } else if c.is_ascii_uppercase() || c.is_ascii_digit() || c == 'M' || c == 'W' {
        size * 0.62
    } else if matches!(c, 'i' | 'l' | 'j' | 't' | 'f' | 'r' | '.' | ',' | ';' | ':' | '\'' | '!' | '|' | '(' | ')' | '[' | ']') {
        size * 0.34
    } else {
        size * 0.52
    }
}

pub fn text_w(s: &str, size: f32) -> f32 {
    s.chars().map(|c| char_w(c, size)).sum()
}

fn is_cjk_char(c: char) -> bool {
    is_wide(c) && !matches!(c as u32, 0xFF00..=0xFFEF | 0x3000..=0x303F)
}

/// 一个块里相邻两行怎么连：`Sentence` 行尾是句号（. 。 ! ？ 等）就保留换行、其余接成一段；
/// `All` 每一行都保留换行；`Merge` 全部接成一段（旧行为）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineMode {
    #[default]
    Sentence,
    All,
    Merge,
}

impl LineMode {
    pub fn parse(s: &str) -> LineMode {
        match s {
            "all" => LineMode::All,
            "merge" => LineMode::Merge,
            _ => LineMode::Sentence,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            LineMode::Sentence => "sentence",
            LineMode::All => "all",
            LineMode::Merge => "merge",
        }
    }
}

/// 这一行的结尾是不是真的“一句话到此为止”：以 . 。 ! ！ ? ？ 结尾，
/// 但 Dr. / e.g. / Fig. / 编号 3. / 首字母缩写 J. 这类不算。
pub fn ends_with_stop(line: &str) -> bool {
    let t = line.trim_end();
    let Some(last) = t.chars().last() else { return false };
    if matches!(last, '。' | '！' | '？' | '!' | '?') {
        return true;
    }
    if last != '.' {
        return false;
    }
    // 省略号不算句末
    if t.ends_with("..") {
        return false;
    }
    let word: String = t[..t.len() - 1].chars().rev().take_while(|c| c.is_alphanumeric() || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
    let w = word.to_lowercase();
    const ABBR: [&str; 24] = [
        "e.g", "i.e", "etc", "vs", "fig", "figs", "no", "nos", "dr", "mr", "mrs", "ms", "prof", "inc", "ltd", "co", "al", "cf", "approx", "sec", "eq", "st",
        "jr", "sr",
    ];
    if ABBR.contains(&w.as_str()) {
        return false;
    }
    // 单独的编号（3. / 12.）和单个字母（J.）
    if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if w.chars().count() == 1 && word.chars().next().is_some_and(|c| c.is_alphabetic()) {
        return false;
    }
    true
}

fn join_lines_mode(a: &str, b: &str, mode: LineMode) -> String {
    let (ta, tb) = (a.trim_end(), b.trim_start());
    if ta.is_empty() || tb.is_empty() {
        return join_lines(a, b);
    }
    let brk = match mode {
        LineMode::Merge => false,
        LineMode::All => true,
        // 只看“最后一行”的结尾：块里前面的换行已经在 a 里了
        LineMode::Sentence => ends_with_stop(ta.rsplit('\n').next().unwrap_or(ta)),
    };
    if brk {
        format!("{ta}\n{tb}")
    } else {
        join_lines(a, b)
    }
}

/// 两行文字接在一起时的连接方式：中文直接连，英文加空格，行尾连字符且下一行小写开头则去掉连字符。
fn join_lines(a: &str, b: &str) -> String {
    let (a, b) = (a.trim_end(), b.trim_start());
    if a.is_empty() {
        return b.to_string();
    }
    if b.is_empty() {
        return a.to_string();
    }
    let (la, fb) = (a.chars().last().unwrap_or(' '), b.chars().next().unwrap_or(' '));
    if la == '-' && fb.is_lowercase() && a.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic()) {
        return format!("{}{}", &a[..a.len() - 1], b);
    }
    if is_cjk_char(la) || is_cjk_char(fb) {
        format!("{a}{b}")
    } else {
        format!("{a} {b}")
    }
}

#[derive(Debug, Clone)]
struct Line {
    runs: Vec<usize>,
    text: String,
    l: f32,
    b: f32,
    r: f32,
    t: f32,
    size: f32,
    rgb: [u8; 3],
}

fn cy(l: &Line) -> f32 {
    (l.b + l.t) / 2.0
}

/// 把同一行的文字对象合成行（按位置，不依赖 PDF 里的写入顺序）。
fn make_lines(runs: &[Run]) -> Vec<Line> {
    let mut order: Vec<&Run> = runs.iter().filter(|r| !r.text.trim().is_empty() && r.size > 0.5 && r.r > r.l).collect();
    order.sort_by(|a, b| ((b.b + b.t) / 2.0).total_cmp(&((a.b + a.t) / 2.0)).then(a.l.total_cmp(&b.l)));
    let mut lines: Vec<Line> = vec![];
    for r in order {
        let mid = (r.b + r.t) / 2.0;
        // 同一行里的两段文字，纵向中心可能差零点几个点：按“先上后下”排序后右边那段会排在左边那段前面，
        // 所以左边相邻的也要接上（接在行首），不然一行会裂成两块。
        let near = |ln: &Line| (cy(ln) - mid).abs() <= ln.size.max(r.size) * 0.45;
        let hit = lines.iter_mut().rev().take(8).find(|ln| {
            near(ln)
                && ((r.l >= ln.l - 1.0 && r.l - ln.r <= ln.size.max(r.size) * 2.5)
                    || (r.l < ln.l - 1.0 && ln.l - r.r <= ln.size.max(r.size) * 2.5 && r.r <= ln.l + ln.size.max(r.size) * 0.5))
        });
        match hit {
            Some(ln) if r.l < ln.l - 1.0 => {
                let gap = ln.l - r.r;
                let sep = if gap > ln.size * 0.15 && !is_cjk_char(r.text.chars().last().unwrap_or(' ')) && !is_cjk_char(ln.text.chars().next().unwrap_or(' ')) {
                    " "
                } else {
                    ""
                };
                ln.text = format!("{}{}{}", r.text.trim_end(), sep, ln.text.trim_start());
                ln.runs.insert(0, r.idx);
                ln.l = r.l;
                ln.b = ln.b.min(r.b);
                ln.t = ln.t.max(r.t);
                ln.size = ln.size.max(r.size);
            }
            Some(ln) => {
                let gap = r.l - ln.r;
                let sep = if gap > ln.size * 0.15 && !is_cjk_char(ln.text.chars().last().unwrap_or(' ')) && !is_cjk_char(r.text.chars().next().unwrap_or(' ')) {
                    " "
                } else {
                    ""
                };
                ln.text = format!("{}{}{}", ln.text.trim_end(), sep, r.text.trim_start());
                ln.runs.push(r.idx);
                ln.r = ln.r.max(r.r);
                ln.b = ln.b.min(r.b);
                ln.t = ln.t.max(r.t);
                ln.size = ln.size.max(r.size);
            }
            None => lines.push(Line { runs: vec![r.idx], text: r.text.trim().to_string(), l: r.l, b: r.b, r: r.r, t: r.t, size: r.size, rgb: r.rgb }),
        }
    }
    lines
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end().chars().last().is_some_and(|c| matches!(c, '.' | '。' | '!' | '！' | '?' | '？' | ':' | '：' | ';' | '；'))
}

/// 值得翻译的块：有字母 / 汉字，不是纯数字页码。
pub fn worth(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    letters >= 2 && letters * 2 >= digits
}

/// 把一页的文字对象聚成段落块，按阅读顺序（先上后下、同高先左后右）返回。
pub fn group_blocks(runs: &[Run]) -> Vec<Block> {
    group_blocks_with(runs, LineMode::Sentence)
}

pub fn group_blocks_with(runs: &[Run], mode: LineMode) -> Vec<Block> {
    let mut lines = make_lines(runs);
    lines.sort_by(|a, b| b.t.total_cmp(&a.t).then(a.l.total_cmp(&b.l)));
    let mut blocks: Vec<(Block, f32, bool)> = vec![]; // (块, 最后一行的右边界, 最后一行是否以句号结尾且偏短)
    for ln in lines {
        let hit = blocks.iter_mut().rev().take(10).find(|(b, last_r, closed)| {
            let size_ok = (b.size - ln.size).abs() <= b.size.max(ln.size) * 0.2;
            let gap = b.b - ln.t; // 上一行底边到这一行顶边
            let below = gap >= -ln.size * 0.3 && gap <= ln.size * 1.15;
            let aligned = (ln.l - b.l).abs() <= ln.size * 2.5 || (ln.l >= b.l && ln.l < *last_r);
            let overlap = ln.r.min(b.r.max(*last_r)) - ln.l.max(b.l) > ln.size * 2.0;
            size_ok && below && aligned && overlap && !*closed
        });
        match hit {
            Some((b, last_r, closed)) => {
                b.text = join_lines_mode(&b.text, &ln.text, mode);
                b.runs.extend(ln.runs);
                b.l = b.l.min(ln.l);
                b.r = b.r.max(ln.r);
                b.b = b.b.min(ln.b);
                b.lines += 1;
                *last_r = ln.r;
                *closed = ends_sentence(&ln.text) && (ln.r - ln.l) < (b.r - b.l) * 0.7;
            }
            None => {
                let closed = ends_sentence(&ln.text) && ln.r - ln.l < ln.size * 12.0;
                blocks.push((
                    Block { runs: ln.runs, l: ln.l, b: ln.b, r: ln.r, t: ln.t, size: ln.size, rgb: ln.rgb, text: ln.text.clone(), lines: 1 },
                    ln.r,
                    closed,
                ));
            }
        }
    }
    let mut out: Vec<Block> = blocks.into_iter().map(|(b, _, _)| b).collect();
    out.retain(|b| worth(&b.text));
    out.sort_by(|a, b| b.t.total_cmp(&a.t).then(a.l.total_cmp(&b.l)));
    out
}

// =====================================================================
// 纯逻辑：排版（换行、缩字号）
// =====================================================================

/// 行首不该出现的标点（避头）。
fn no_line_start(c: char) -> bool {
    matches!(c, '，' | '。' | '、' | '！' | '？' | '；' | '：' | '）' | '》' | '」' | '』' | '”' | '’' | ',' | '.' | '!' | '?' | ';' | ':' | ')' | ']' | '}')
}

/// 按宽度换行：英文在空格处断，中文逐字断，避免标点出现在行首。
pub fn wrap(text: &str, width: f32, size: f32) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for para in text.split('\n') {
        let mut cur = String::new();
        let mut cur_w = 0.0f32;
        let mut word = String::new();
        let mut word_w = 0.0f32;
        let flush_word = |cur: &mut String, cur_w: &mut f32, word: &mut String, word_w: &mut f32, lines: &mut Vec<String>| {
            if word.is_empty() {
                return;
            }
            if *cur_w + *word_w > width && !cur.is_empty() {
                lines.push(cur.trim_end().to_string());
                cur.clear();
                *cur_w = 0.0;
            }
            // 单个词比一行还长：硬切
            if *word_w > width {
                for c in word.chars() {
                    let cw = char_w(c, size);
                    if *cur_w + cw > width && !cur.is_empty() {
                        lines.push(cur.trim_end().to_string());
                        cur.clear();
                        *cur_w = 0.0;
                    }
                    cur.push(c);
                    *cur_w += cw;
                }
            } else {
                cur.push_str(word);
                *cur_w += *word_w;
            }
            word.clear();
            *word_w = 0.0;
        };
        for c in para.chars() {
            if is_wide(c) {
                flush_word(&mut cur, &mut cur_w, &mut word, &mut word_w, &mut lines);
                let cw = char_w(c, size);
                if cur_w + cw > width && !cur.is_empty() && !no_line_start(c) {
                    lines.push(cur.trim_end().to_string());
                    cur.clear();
                    cur_w = 0.0;
                }
                cur.push(c);
                cur_w += cw;
            } else if c == ' ' {
                flush_word(&mut cur, &mut cur_w, &mut word, &mut word_w, &mut lines);
                if !cur.is_empty() {
                    cur.push(' ');
                    cur_w += char_w(' ', size);
                }
            } else {
                word.push(c);
                word_w += char_w(c, size);
            }
        }
        flush_word(&mut cur, &mut cur_w, &mut word, &mut word_w, &mut lines);
        lines.push(cur.trim_end().to_string());
    }
    while lines.last().is_some_and(String::is_empty) && lines.len() > 1 {
        lines.pop();
    }
    lines
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub size: f32,
    pub lines: Vec<String>,
    /// 行距（点）
    pub leading: f32,
}

/// 在宽 `w`、高 `h` 的框里排版：从原字号开始，放不下就逐步缩小，最小 `min`；最小还放不下就允许向下多出一点。
pub fn fit(text: &str, w: f32, h: f32, size0: f32, min: f32) -> Fit {
    let mut size = size0.max(min);
    loop {
        let leading = size * 1.28;
        let lines = wrap(text, w, size);
        let need = lines.len() as f32 * leading;
        if need <= h * 1.08 || size <= min + 0.01 {
            return Fit { size, lines, leading };
        }
        size = (size * 0.93).max(min);
    }
}

// =====================================================================
// 纯逻辑：字体、扫描页背景、组件文件名
// =====================================================================

/// 从 TrueType 字体集合（.ttc，如微软雅黑、苹方、文泉驿）里取出第一个字体，改成单独的 .ttf。
/// PDF 里嵌入字体需要单个字体文件，所以系统里的 .ttc 要先拆开。不是 .ttc 就原样返回。
pub fn ttc_first_face(data: &[u8]) -> AppResult<Vec<u8>> {
    if data.len() < 16 || &data[..4] != b"ttcf" {
        return Ok(data.to_vec());
    }
    let be32 = |o: usize| -> AppResult<u32> {
        data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| AppError::invalid("字体文件不完整"))
    };
    let be16 =
        |o: usize| -> AppResult<u16> { data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]])).ok_or_else(|| AppError::invalid("字体文件不完整")) };
    let off = be32(12)? as usize;
    let n = be16(off + 4)? as usize;
    if n == 0 || n > 64 {
        return Err(AppError::invalid("字体文件不完整"));
    }
    struct Rec {
        tag: [u8; 4],
        sum: u32,
        off: usize,
        len: usize,
    }
    let mut recs = vec![];
    for i in 0..n {
        let o = off + 12 + i * 16;
        let tag = data.get(o..o + 4).ok_or_else(|| AppError::invalid("字体文件不完整"))?;
        recs.push(Rec { tag: [tag[0], tag[1], tag[2], tag[3]], sum: be32(o + 4)?, off: be32(o + 8)? as usize, len: be32(o + 12)? as usize });
    }
    let head = 12 + 16 * n;
    let mut out = Vec::with_capacity(data.len() / 2);
    out.extend_from_slice(&data[off..off + 4]); // sfnt 版本号
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&data[off + 6..off + 12]); // searchRange / entrySelector / rangeShift
    let mut at = head;
    let mut placed = vec![];
    for r in &recs {
        let end = r.off.checked_add(r.len).filter(|e| *e <= data.len()).ok_or_else(|| AppError::invalid("字体文件不完整"))?;
        placed.push((at, &data[r.off..end]));
        at += r.len.div_ceil(4) * 4;
    }
    for (r, (pos, _)) in recs.iter().zip(&placed) {
        out.extend_from_slice(&r.tag);
        out.extend_from_slice(&r.sum.to_be_bytes());
        out.extend_from_slice(&(*pos as u32).to_be_bytes());
        out.extend_from_slice(&(r.len as u32).to_be_bytes());
    }
    for (_, bytes) in &placed {
        out.extend_from_slice(bytes);
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    Ok(out)
}

/// 在 RGBA 图片里取框外一圈像素的中位色，当作这块文字的背景色。`(x0, y0, x1, y1)` 是像素框。
pub fn sample_bg(rgba: &[u8], w: usize, h: usize, bx: (usize, usize, usize, usize)) -> [u8; 3] {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return [255, 255, 255];
    }
    let (x0, y0, x1, y1) = (bx.0.saturating_sub(2), bx.1.saturating_sub(2), (bx.2 + 2).min(w - 1), (bx.3 + 2).min(h - 1));
    let mut px: Vec<[u8; 3]> = vec![];
    let mut take = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        px.push([rgba[i], rgba[i + 1], rgba[i + 2]]);
    };
    for x in x0..=x1 {
        take(x, y0);
        take(x, y1);
    }
    for y in y0..=y1 {
        take(x0, y);
        take(x1, y);
    }
    let med = |k: usize| {
        let mut v: Vec<u8> = px.iter().map(|p| p[k]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    [med(0), med(1), med(2)]
}

/// 取一块文字的背景色：看框里（外扩 2 像素）“不是文字颜色”的像素，取出现最多的那种颜色。
///
/// 以前只取框外一圈像素的中位数，旁边有色块、描边或别的元素时会取错。这里先去掉和文字色接近的像素
/// （文字本身和它的抗锯齿边），剩下的里面数哪种颜色最多；剩得太少（几乎全是文字）才退回框外一圈。
pub fn sample_bg_smart(rgba: &[u8], w: usize, h: usize, bx: (usize, usize, usize, usize), text: [u8; 3]) -> [u8; 3] {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return [255, 255, 255];
    }
    let (x0, y0, x1, y1) = (bx.0.saturating_sub(2), bx.1.saturating_sub(2), (bx.2 + 2).min(w - 1), (bx.3 + 2).min(h - 1));
    if x1 <= x0 || y1 <= y0 {
        return sample_bg(rgba, w, h, bx);
    }
    let dist = |p: [u8; 3], q: [u8; 3]| p.iter().zip(&q).map(|(a, b)| (i32::from(*a) - i32::from(*b)).unsigned_abs()).sum::<u32>();
    // 间隔取点，最多看 6000 个像素
    let step = (((x1 - x0 + 1) * (y1 - y0 + 1)) as f32 / 6000.0).sqrt().ceil().max(1.0) as usize;
    let mut bins: std::collections::HashMap<u16, (u32, [u32; 3])> = std::collections::HashMap::new();
    let mut total = 0u32;
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            let i = (y * w + x) * 4;
            let px = [rgba[i], rgba[i + 1], rgba[i + 2]];
            total += 1;
            // 文字本身和它的抗锯齿边：离文字色近的像素不算背景
            if dist(px, text) > 96 {
                let key = (u16::from(px[0] >> 4) << 8) | (u16::from(px[1] >> 4) << 4) | u16::from(px[2] >> 4);
                let e = bins.entry(key).or_insert((0, [0; 3]));
                e.0 += 1;
                for (acc, v) in e.1.iter_mut().zip(px) {
                    *acc += u32::from(v);
                }
            }
            x += step;
        }
        y += step;
    }
    let kept: u32 = bins.values().map(|e| e.0).sum();
    if total == 0 || kept * 8 < total {
        return sample_bg(rgba, w, h, bx);
    }
    // 最多的那一格，再把相邻格（颜色差一级）合进来，取平均，渐变和轻微噪点才不会让结果乱跳
    let best = *bins.iter().max_by_key(|(k, e)| (e.0, **k)).map(|(k, _)| k).unwrap_or(&0);
    let (br, bgc, bb) = (i32::from(best >> 8), i32::from((best >> 4) & 15), i32::from(best & 15));
    let (mut n, mut sum) = (0u32, [0u32; 3]);
    for (k, e) in &bins {
        let (r, g, b) = (i32::from(*k >> 8), i32::from((*k >> 4) & 15), i32::from(*k & 15));
        if (r - br).abs() <= 1 && (g - bgc).abs() <= 1 && (b - bb).abs() <= 1 {
            n += e.0;
            for (acc, v) in sum.iter_mut().zip(e.1) {
                *acc += v;
            }
        }
    }
    if n == 0 {
        return sample_bg(rgba, w, h, bx);
    }
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
}

/// 深色背景用浅色字，浅色背景用深色字。
pub fn contrast(bg: [u8; 3]) -> [u8; 3] {
    let lum = 0.299 * f32::from(bg[0]) + 0.587 * f32::from(bg[1]) + 0.114 * f32::from(bg[2]);
    if lum < 140.0 {
        [245, 245, 245]
    } else {
        [20, 20, 20]
    }
}

/// 当前系统对应的 PDFium 预编译包名（bblanchon/pdfium-binaries 的发布文件）。
pub fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64") => "pdfium-win-x64.tgz",
        ("windows", "aarch64") => "pdfium-win-arm64.tgz",
        ("macos", _) => "pdfium-mac-univ.tgz",
        ("linux", "x86_64") => "pdfium-linux-x64.tgz",
        ("linux", "aarch64") => "pdfium-linux-arm64.tgz",
        _ => return None,
    })
}

pub fn lib_file(os: &str) -> &'static str {
    match os {
        "windows" => "pdfium.dll",
        "macos" => "libpdfium.dylib",
        _ => "libpdfium.so",
    }
}

const RELEASE_BASE: &str = "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/";

pub fn dir_of(data_dir: &Path) -> PathBuf {
    data_dir.join("components").join("pdfium")
}

pub fn find_lib(data_dir: &Path) -> Option<PathBuf> {
    let p = dir_of(data_dir).join(lib_file(std::env::consts::OS));
    p.is_file().then_some(p)
}

/// 系统里常见的中文字体（优先单文件 .ttf，其次 .ttc）。
fn system_font_candidates() -> Vec<PathBuf> {
    let mut v: Vec<&str> = vec![];
    if cfg!(windows) {
        v.extend(["C:\\Windows\\Fonts\\simhei.ttf", "C:\\Windows\\Fonts\\msyh.ttc", "C:\\Windows\\Fonts\\simsun.ttc", "C:\\Windows\\Fonts\\msyh.ttf"]);
    } else if cfg!(target_os = "macos") {
        v.extend([
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ]);
    } else {
        v.extend([
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-zenhei/wqy-zenhei.ttc",
        ]);
    }
    v.into_iter().map(PathBuf::from).collect()
}

/// 找一个能写中文的 TrueType 字体，返回可以直接嵌入 PDF 的字体数据。
pub fn load_font(data_dir: &Path) -> AppResult<Vec<u8>> {
    let own = dir_of(data_dir).join("font.ttf");
    let mut cands = vec![own];
    cands.extend(system_font_candidates());
    for p in cands {
        if let Ok(b) = std::fs::read(&p) {
            if let Ok(f) = ttc_first_face(&b) {
                // 是 TrueType 轮廓（sfnt 版本 0x00010000 或 true）；CFF 轮廓的 OpenType（OTTO）PDFium 嵌入不了
                if f.len() > 1000 && (f.starts_with(&[0, 1, 0, 0]) || f.starts_with(b"true")) {
                    return Ok(f);
                }
            }
        }
    }
    Err(AppError::invalid("没找到能写中文的 TrueType 字体。请在“文件翻译”里点“下载译文字体”，或把一个 .ttf 中文字体放到数据目录 components/pdfium/font.ttf。"))
}

// =====================================================================
// 与 PDFium 打交道
// =====================================================================

use pdfium_render::prelude::*;

/// PDFium 的动态库绑定在一个进程里只能初始化一次（再绑会报 BindingsAlreadyInitialized），所以绑一次，之后全进程复用。
static PDFIUM: std::sync::OnceLock<Pdfium> = std::sync::OnceLock::new();
static BIND_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn bind(lib: &Path) -> AppResult<&'static Pdfium> {
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let _g = BIND_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let b = Pdfium::bind_to_library(lib).map_err(|e| AppError::msg(format!("PDF 排版组件打不开：{e}。请在“文件翻译”里重新安装。")))?;
    Ok(PDFIUM.get_or_init(|| Pdfium::new(b)))
}

fn perr(e: PdfiumError) -> AppError {
    AppError::msg(format!("PDF 处理失败：{e}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    pub w: f32,
    pub h: f32,
    pub blocks: Vec<Block>,
    /// 页面上有没有任何文字对象（没有 = 扫描页）
    pub has_text: bool,
}

/// 读出每一页的段落块（行尾是句号的行保留换行）。
pub fn extract(lib: &Path, pdf: &Path) -> AppResult<Vec<PageInfo>> {
    extract_with(lib, pdf, LineMode::Sentence)
}

/// 读出每一页的段落块；`mode` 决定块里相邻两行怎么连。
pub fn extract_with(lib: &Path, pdf: &Path, mode: LineMode) -> AppResult<Vec<PageInfo>> {
    let pdfium = bind(lib)?;
    let doc = pdfium.load_pdf_from_file(pdf, None).map_err(|e| match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => AppError::invalid("这个 PDF 有密码，先用阅读器去掉密码再翻译。"),
        other => perr(other),
    })?;
    let mut out = vec![];
    for page in doc.pages().iter() {
        let (w, h) = (page.width().value, page.height().value);
        let mut runs = vec![];
        let mut any = false;
        for (i, obj) in page.objects().iter().enumerate() {
            let Some(t) = obj.as_text_object() else { continue };
            any = true;
            let text = t.text();
            let Ok(bb) = obj.bounds() else { continue };
            let rgb = t.fill_color().map(|c| [c.red(), c.green(), c.blue()]).unwrap_or([0, 0, 0]);
            runs.push(Run {
                idx: i,
                text,
                l: bb.left().value,
                b: bb.bottom().value,
                r: bb.right().value,
                t: bb.top().value,
                size: t.scaled_font_size().value,
                rgb,
            });
        }
        out.push(PageInfo { w, h, blocks: group_blocks_with(&runs, mode), has_text: any });
    }
    Ok(out)
}

/// 渲染出来的位图 → RGBA 像素（不透明）。
///
/// `PdfRenderConfig::new()` 默认会让 PDFium 颠倒字节序（`FPDF_REVERSE_BYTE_ORDER`），拿到的已经是 R、G、B 的顺序；
/// `as_rgba_bytes()` 会按“有没有颠倒过”换算，不能再自己对调 R 和 B（之前对调了一次，页面里红蓝是反的）。
fn rgba_of(bmp: &PdfBitmap) -> Vec<u8> {
    let mut v = bmp.as_rgba_bytes();
    for px in v.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    v
}

/// 渲染一页成 RGBA 像素（扫描页识字用）。返回 (宽, 高, RGBA)。
pub fn render_page(lib: &Path, pdf: &Path, index: usize, target_w: i32) -> AppResult<(usize, usize, Vec<u8>)> {
    let pdfium = bind(lib)?;
    let doc = pdfium.load_pdf_from_file(pdf, None).map_err(perr)?;
    let page = doc.pages().get(index as PdfPageIndex).map_err(perr)?;
    let bmp = page.render_with_config(&PdfRenderConfig::new().set_target_width(target_w)).map_err(perr)?;
    let (w, h) = (bmp.width() as usize, bmp.height() as usize);
    Ok((w, h, rgba_of(&bmp)))
}

/// RGBA → PNG。
pub fn png_of_rgba(w: usize, h: usize, rgba: &[u8]) -> AppResult<Vec<u8>> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().map_err(|e| AppError::msg(e.to_string()))?.write_image_data(rgba).map_err(|e| AppError::msg(e.to_string()))?;
    }
    Ok(out)
}

/// 写回的一项：删掉 `remove` 里的文字对象，在 `rect` 里写 `text`；`bg` 有值时先用这个颜色盖住（扫描页）。
#[derive(Debug, Clone)]
pub struct Item {
    pub remove: Vec<usize>,
    /// (左, 下, 右, 上)
    pub rect: (f32, f32, f32, f32),
    pub size: f32,
    pub rgb: [u8; 3],
    pub bg: Option<[u8; 3]>,
    pub text: String,
    /// 背景是用户手动指定的（不要再用页面取样覆盖）
    pub bg_user: bool,
    pub bg_mode: BgMode,
    /// 渐变的色标（至少两个，等距排列）；空 = 用 `bg` 单色
    pub bg_stops: Vec<[u8; 3]>,
    pub align: Align,
    /// 遮盖和文字区域向外扩多少（点）；None = 字号的 18%，至少 2 点
    pub pad: Option<f32>,
    /// 这一块往下最多能占多高（点，从上边算起）；None = 只用原来的框高
    pub room: Option<f32>,
}

impl Default for Item {
    fn default() -> Self {
        Item {
            remove: vec![],
            rect: (0.0, 0.0, 0.0, 0.0),
            size: 10.0,
            rgb: [0, 0, 0],
            bg: None,
            text: String::new(),
            bg_user: false,
            bg_mode: BgMode::Solid,
            bg_stops: vec![],
            align: Align::Left,
            pad: None,
            room: None,
        }
    }
}

/// 译文底下的遮盖：纯色 / 横向渐变 / 竖向渐变 / 不遮盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BgMode {
    #[default]
    Solid,
    HGrad,
    VGrad,
    None,
}

impl BgMode {
    pub fn parse(s: &str) -> BgMode {
        match s {
            "hgrad" => BgMode::HGrad,
            "vgrad" => BgMode::VGrad,
            "none" => BgMode::None,
            _ => BgMode::Solid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    pub fn parse(s: &str) -> Align {
        match s {
            "center" => Align::Center,
            "right" => Align::Right,
            _ => Align::Left,
        }
    }
}

/// 遮盖 / 文字区域默认向外扩的量：字号的 18%，至少 2 点。
pub fn default_pad(size: f32) -> f32 {
    (size * 0.18).max(2.0)
}

/// 两个色标之间按位置取色（`stops` 等距排列，`t` 在 0..=1）。
pub fn gradient_at(stops: &[[u8; 3]], t: f32) -> [u8; 3] {
    match stops.len() {
        0 => [255, 255, 255],
        1 => stops[0],
        n => {
            let x = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let i = (x.floor() as usize).min(n - 2);
            let f = x - i as f32;
            let (a, b) = (stops[i], stops[i + 1]);
            [0, 1, 2].map(|k| (f32::from(a[k]) + (f32::from(b[k]) - f32::from(a[k])) * f).round() as u8)
        }
    }
}

/// 给一页里的每块算“往下还能占多高”：下面有别的块（水平方向有重叠）就到它的上边为止，没有就到页面下边距。
/// 译文变长时框往下长，不用缩小字号；碰到别的块才缩。
pub fn assign_room(items: &mut [Item], page_h: f32) {
    let rects: Vec<(f32, f32, f32, f32)> = items.iter().map(|it| it.rect).collect();
    for (i, it) in items.iter_mut().enumerate() {
        let (l, b, r, t) = it.rect;
        let w = (r - l).max(1.0);
        let mut floor = 8.0_f32.min(b); // 页面下边距
        for (j, o) in rects.iter().enumerate() {
            if j == i {
                continue;
            }
            let overlap = r.min(o.2) - l.max(o.0);
            // 在这一块下面（它的上边不高于这一块的下边附近）、水平方向有明显重叠
            if overlap > w * 0.2 && o.3 <= b + it.size * 0.5 && o.3 < t {
                floor = floor.max(o.3 + 2.0);
            }
        }
        it.room = Some((t - floor).clamp(t - b, page_h.max(t - b)));
    }
}

/// 在宽 `w` 里排版，高度上限 `room`：先保持原字号；放不下才缩，先缩到原字号的 85%，
/// 再不行才继续缩到 `min`（和以前一样，最后仍放不下就向下多出一点）。
pub fn fit_keep(text: &str, w: f32, room: f32, size0: f32, min: f32) -> Fit {
    let size0 = size0.max(min);
    let try_at = |size: f32| {
        let leading = size * 1.28;
        let lines = wrap(text, w, size);
        let need = lines.len() as f32 * leading;
        (need <= room * 1.02, Fit { size, lines, leading })
    };
    let (ok, f) = try_at(size0);
    if ok {
        return f;
    }
    let mut size = size0;
    let soft = (size0 * 0.85).max(min);
    while size > soft + 0.01 {
        size = (size * 0.96).max(soft);
        let (ok, f) = try_at(size);
        if ok {
            return f;
        }
    }
    let mut size = soft;
    loop {
        let (ok, f) = try_at(size);
        if ok || size <= min + 0.01 {
            return f;
        }
        size = (size * 0.94).max(min);
    }
}

#[derive(Debug, Clone, Default)]
pub struct PagePlan {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 译文写回原位
    InPlace,
    /// 原页、译文页交替排列
    Interleave,
}

impl Mode {
    pub fn parse(s: &str) -> Mode {
        if s == "interleave" {
            Mode::Interleave
        } else {
            Mode::InPlace
        }
    }
}

/// 把一页的译文写上去（删旧文字、盖底色、按框换行写译文）。
fn edit_page(page: &mut PdfPage, font: PdfFontToken, plan: &PagePlan) {
    // 先删旧文字（序号大的先删，前面的序号才不会错位）
    let mut gone: Vec<usize> = plan.items.iter().flat_map(|it| it.remove.iter().copied()).collect();
    gone.sort_unstable_by(|a, b| b.cmp(a));
    gone.dedup();
    for i in gone {
        let _ = page.objects_mut().remove_object_at_index(i);
    }
    for it in &plan.items {
        let (l, b, r, t) = it.rect;
        let pad = it.pad.unwrap_or_else(|| default_pad(it.size)).max(0.0);
        let width = (r - l).max(it.size * 2.0);
        let room = it.room.unwrap_or(t - b).max(t - b);
        let f = fit_keep(&it.text, width, room, it.size, 6.0);
        let need = f.lines.len() as f32 * f.leading;
        // 单行：在原来的框里上下居中；多行：从上往下排，框往下长
        let top = if f.lines.len() == 1 && t - b > f.leading { t - ((t - b) - f.leading) / 2.0 } else { t };
        // 遮盖：原来的框外扩 pad，译文比原框高时往下一起长
        let cover = (l - pad, (b.min(top - need)) - pad, r + pad, t + pad);
        draw_cover(page, it, cover);
        let color = PdfColor::new(it.rgb[0], it.rgb[1], it.rgb[2], 255);
        for (n, line) in f.lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let y = top - f.size * 0.95 - n as f32 * f.leading;
            let x = match it.align {
                Align::Left => l,
                Align::Center => l + (width - text_w(line, f.size)).max(0.0) / 2.0,
                Align::Right => l + (width - text_w(line, f.size)).max(0.0),
            };
            if let Ok(mut o) = page.objects_mut().create_text_object(PdfPoints::new(x), PdfPoints::new(y), line, font, PdfPoints::new(f.size)) {
                let _ = o.set_fill_color(color);
            }
        }
    }
}

/// 画译文底下的遮盖。`cover` 是 (左, 下, 右, 上)。
fn draw_cover(page: &mut PdfPage, it: &Item, cover: (f32, f32, f32, f32)) {
    let (l, b, r, t) = cover;
    let fill = |page: &mut PdfPage, l: f32, b: f32, r: f32, t: f32, c: [u8; 3]| {
        let rect = PdfRect::new(PdfPoints::new(b), PdfPoints::new(l), PdfPoints::new(t), PdfPoints::new(r));
        let _ = page.objects_mut().create_path_object_rect(rect, None, None, Some(PdfColor::new(c[0], c[1], c[2], 255)));
    };
    let Some(bg) = it.bg else { return };
    match it.bg_mode {
        BgMode::None => {}
        BgMode::Solid => fill(page, l, b, r, t, bg),
        BgMode::HGrad | BgMode::VGrad => {
            let stops = if it.bg_stops.len() >= 2 { it.bg_stops.clone() } else { vec![bg, bg] };
            const N: usize = 24;
            for i in 0..N {
                let c = gradient_at(&stops, (i as f32 + 0.5) / N as f32);
                // 相邻两条多叠 0.3 点，避免缝
                if it.bg_mode == BgMode::HGrad {
                    let w = (r - l) / N as f32;
                    fill(page, l + w * i as f32, b, l + w * (i as f32 + 1.0) + 0.3, t, c);
                } else {
                    let h = (t - b) / N as f32;
                    fill(page, l, t - h * (i as f32 + 1.0) - 0.3, r, t - h * i as f32, c);
                }
            }
        }
    }
}

/// 只渲染一页的译文效果（不存文件）：给“版面预览”用。
pub fn render_translated(lib: &Path, font_bytes: &[u8], input: &Path, index: usize, plan: &PagePlan, target_w: i32) -> AppResult<(usize, usize, Vec<u8>)> {
    let pdfium = bind(lib)?;
    let mut doc = pdfium.load_pdf_from_file(input, None).map_err(perr)?;
    let font = doc.fonts_mut().load_true_type_from_bytes(font_bytes, true).map_err(|e| AppError::msg(format!("译文字体嵌入失败：{e}")))?;
    let mut page = doc.pages().get(index as PdfPageIndex).map_err(perr)?;
    edit_page(&mut page, font, plan);
    let bmp = page.render_with_config(&PdfRenderConfig::new().set_target_width(target_w)).map_err(perr)?;
    let (w, h) = (bmp.width() as usize, bmp.height() as usize);
    Ok((w, h, rgba_of(&bmp)))
}

/// 按计划改写 PDF 并存到 `out`。`plans` 和页一一对应（可以比页数少）。
pub fn apply(lib: &Path, font_bytes: &[u8], input: &Path, out: &Path, plans: &[PagePlan], mode: Mode, mut progress: impl FnMut(usize, usize)) -> AppResult<()> {
    let pdfium = bind(lib)?;
    let mut doc = pdfium.load_pdf_from_file(input, None).map_err(perr)?;
    let font = doc.fonts_mut().load_true_type_from_bytes(font_bytes, true).map_err(|e| AppError::msg(format!("译文字体嵌入失败：{e}")))?;
    let total = plans.len().max(1);
    for (pi, plan) in plans.iter().enumerate() {
        progress(pi, total);
        if plan.items.is_empty() {
            continue;
        }
        let mut page = doc.pages().get(pi as PdfPageIndex).map_err(perr)?;
        edit_page(&mut page, font, plan);
    }
    progress(total, total);
    match mode {
        Mode::InPlace => doc.save_to_file(out).map_err(perr),
        Mode::Interleave => {
            // 先存译文版，再把“原页、译文页”一对一对拼起来
            let tmp = out.with_extension("tr.tmp.pdf");
            doc.save_to_file(&tmp).map_err(perr)?;
            let orig = pdfium.load_pdf_from_file(input, None).map_err(perr)?;
            let tr = pdfium.load_pdf_from_file(&tmp, None).map_err(perr)?;
            let mut merged = pdfium.create_new_pdf().map_err(perr)?;
            let n = orig.pages().len();
            for i in 0..n {
                let at = merged.pages().len();
                merged.pages_mut().copy_page_range_from_document(&orig, i..=i, at).map_err(perr)?;
                let at = merged.pages().len();
                merged.pages_mut().copy_page_range_from_document(&tr, i..=i, at).map_err(perr)?;
            }
            let r = merged.save_to_file(out).map_err(perr);
            let _ = std::fs::remove_file(&tmp);
            r
        }
    }
}

// =====================================================================
// 组件：状态、安装、卸载
// =====================================================================

use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

type St<'a> = State<'a, Arc<crate::AppState>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// 这个系统有没有现成的预编译包
    pub supported: bool,
    pub installed: bool,
    pub path: String,
    /// 能不能找到中文字体
    pub font_ok: bool,
    pub note: String,
}

pub fn status_of(data_dir: &Path) -> Status {
    let supported = asset_name(std::env::consts::OS, std::env::consts::ARCH).is_some();
    let lib = find_lib(data_dir);
    let font_ok = load_font(data_dir).is_ok();
    let note = if !supported {
        "这个系统暂时没有现成的 PDF 排版组件。".to_string()
    } else if lib.is_none() {
        "未安装（约 4 MB，一键下载）".to_string()
    } else if !font_ok {
        "已安装；但没找到可用的中文字体，译文可能显示成方框。".to_string()
    } else {
        "已就绪".to_string()
    };
    Status { supported, installed: lib.is_some(), path: lib.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), font_ok, note }
}

#[tauri::command]
pub fn pdfx_status(state: St<'_>) -> Status {
    status_of(&state.data_dir)
}

/// 解开 `.tgz`，只取名字叫 `wanted` 的文件（不带目录）放进 `dest`。
fn untar_gz_pick(archive: &Path, dest: &Path, wanted: &str) -> AppResult<bool> {
    std::fs::create_dir_all(dest)?;
    let f = std::fs::File::open(archive)?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    for e in ar.entries().map_err(|e| AppError::msg(format!("压缩包打不开：{e}")))? {
        let mut e = e.map_err(|e| AppError::msg(format!("压缩包损坏：{e}")))?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let path = e.path().map_err(|e| AppError::msg(e.to_string()))?.into_owned();
        if path.file_name().and_then(|n| n.to_str()) != Some(wanted) {
            continue;
        }
        let mut buf = Vec::with_capacity(e.size() as usize);
        std::io::Read::read_to_end(&mut e, &mut buf)?;
        let tmp = dest.join(format!("{wanted}.part"));
        std::fs::write(&tmp, buf)?;
        std::fs::rename(&tmp, dest.join(wanted))?;
        return Ok(true);
    }
    Ok(false)
}

const EVT: &str = "ml://download";
const ID: &str = "pdfium";

#[tauri::command]
pub async fn pdfx_install(app: AppHandle, state: St<'_>) -> AppResult<Status> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let name = asset_name(std::env::consts::OS, std::env::consts::ARCH).ok_or_else(|| AppError::invalid("这个系统暂时没有现成的 PDF 排版组件。"))?;
    let dir = dir_of(&state.data_dir);
    std::fs::create_dir_all(&dir)?;
    let url = format!("{RELEASE_BASE}{name}");
    let part = dir.join(format!("{name}.part"));
    let _ = app.emit(EVT, serde_json::json!({ "id": ID, "got": 0, "total": 0 }));
    let mut last = AppError::msg("下载失败");
    let mut ok = false;
    for u in crate::tools::candidates(&url, &state.settings().component_mirrors) {
        let r: AppResult<()> = async {
            let resp = state.dl_client.get(&u).send().await.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
            if !resp.status().is_success() {
                return Err(AppError::from_status(resp.status().as_u16(), "PDF 排版组件"));
            }
            let total = resp.content_length().unwrap_or(0);
            let mut stream = resp.bytes_stream();
            let mut file = tokio::fs::File::create(&part).await?;
            let mut got = 0u64;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
                file.write_all(&chunk).await?;
                got += chunk.len() as u64;
                let _ = app.emit(EVT, serde_json::json!({ "id": ID, "got": got, "total": total.max(got) }));
            }
            file.flush().await?;
            if got < 500_000 {
                return Err(AppError::msg("下载到的文件太小，不是有效的组件包"));
            }
            Ok(())
        }
        .await;
        match r {
            Ok(()) => {
                ok = true;
                break;
            }
            Err(e) => last = e,
        }
    }
    if !ok {
        let _ = std::fs::remove_file(&part);
        return Err(last.context("下载 PDF 排版组件失败："));
    }
    let (p2, d2, want) = (part.clone(), dir.clone(), lib_file(std::env::consts::OS));
    let found = tokio::task::spawn_blocking(move || untar_gz_pick(&p2, &d2, want)).await.map_err(|e| AppError::msg(e.to_string()))??;
    let _ = std::fs::remove_file(&part);
    if !found {
        return Err(AppError::msg(format!("压缩包里没有 {}。可能官方改了打包方式。", lib_file(std::env::consts::OS))));
    }
    let _ = app.emit(EVT, serde_json::json!({ "id": ID, "got": 1, "total": 1 }));
    Ok(status_of(&state.data_dir))
}

/// 下载译文字体（系统里没有中文字体时用）：Noto Sans SC 的可变字体，约 10 MB。
#[tauri::command]
pub async fn pdfx_install_font(state: St<'_>) -> AppResult<Status> {
    const URL: &str = "https://github.com/google/fonts/raw/main/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf";
    let dir = dir_of(&state.data_dir);
    std::fs::create_dir_all(&dir)?;
    let mut last = AppError::msg("下载失败");
    for u in crate::tools::candidates(URL, &state.settings().component_mirrors) {
        match state.dl_client.get(&u).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(b) if b.len() > 1_000_000 && b.starts_with(&[0, 1, 0, 0]) => {
                    let part = dir.join("font.ttf.part");
                    std::fs::write(&part, &b)?;
                    std::fs::rename(&part, dir.join("font.ttf"))?;
                    return Ok(status_of(&state.data_dir));
                }
                Ok(_) => last = AppError::msg("下载到的不是有效的字体文件"),
                Err(e) => last = AppError::classify(crate::error::describe_reqwest(&e)),
            },
            Ok(resp) => last = AppError::from_status(resp.status().as_u16(), "译文字体"),
            Err(e) => last = AppError::classify(crate::error::describe_reqwest(&e)),
        }
    }
    Err(last.context("下载译文字体失败："))
}

#[tauri::command]
pub fn pdfx_remove(state: St<'_>) -> Status {
    let d = dir_of(&state.data_dir);
    let _ = std::fs::remove_file(d.join(lib_file(std::env::consts::OS)));
    status_of(&state.data_dir)
}

#[cfg(test)]
#[path = "pdfx_tests.rs"]
mod tests;
