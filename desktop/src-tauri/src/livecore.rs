//! 实时屏幕翻译的纯逻辑：文字行分组（横排 / 竖排）、跳过规则、字幕去重与续接、画面变化检测、背景 / 文字取色。
//! 这里不碰系统接口，全部可以在任何平台上测试。

use serde::{Deserialize, Serialize};

/// OCR 得到的一行文字，坐标是图片里的像素。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OLine {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 竖排（从上到下一列一列写）
    pub vertical: bool,
}

impl OLine {
    pub fn new(text: &str, x: f32, y: f32, w: f32, h: f32) -> OLine {
        // 又高又窄、不止一个字的，多半是竖排
        let vertical = text.chars().filter(|c| !c.is_whitespace()).count() >= 2 && h > w * 1.6;
        OLine { text: text.trim().to_string(), x, y, w, h, vertical }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    /// 一行文字的“字高”：横排看高，竖排看宽
    pub fn em(&self) -> f32 {
        if self.vertical {
            self.w
        } else {
            self.h
        }
    }
}

/// 一组连在一起的文字（一段话、一个气泡、一个按钮）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Group {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub vertical: bool,
    /// 包含几行（竖排是几列）
    pub lines: usize,
    /// 单行的字高
    pub em: f32,
    /// 多行时各行的左右端点（横排）；用来判断原文是靠左、居中还是靠右
    #[serde(skip)]
    pub spans: Vec<(f32, f32)>,
    /// 0 靠左 / 1 居中 / 2 靠右。单行认不出来，按靠左
    pub align: u8,
}

/// 各行的左右端点是靠左、居中还是靠右排的：0 靠左 / 1 居中 / 2 靠右。
/// 只有一行没法判断（单行的左边、中线、右边是同一个位置），按靠左。
/// `em` 是字高，端点差在 0.6 个字以内算同一条线。
pub fn align_of(spans: &[(f32, f32)], em: f32) -> u8 {
    if spans.len() < 2 {
        return 0;
    }
    let tol = (em * 0.6).max(1.5);
    let spread = |f: &dyn Fn(&(f32, f32)) -> f32| {
        let (lo, hi) = spans.iter().map(f).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        hi - lo
    };
    let (l, c, r) = (spread(&|s| s.0), spread(&|s| (s.0 + s.1) / 2.0), spread(&|s| s.1));
    // 三条线里最齐的那条决定对齐；左边也齐就是靠左（最常见）
    if l <= tol {
        0
    } else if c <= tol && c <= r {
        1
    } else if r <= tol {
        2
    } else {
        0
    }
}

impl Group {
    fn from_line(l: &OLine) -> Group {
        Group { text: l.text.clone(), x: l.x, y: l.y, w: l.w, h: l.h, vertical: l.vertical, lines: 1, em: l.em(), spans: vec![(l.x, l.right())], align: 0 }
    }
    fn right(&self) -> f32 {
        self.x + self.w
    }
    fn bottom(&self) -> f32 {
        self.y + self.h
    }
    fn grow(&mut self, l: &OLine) {
        self.spans.push((l.x, l.right()));
        let (r, b) = (self.right().max(l.right()), self.bottom().max(l.bottom()));
        self.x = self.x.min(l.x);
        self.y = self.y.min(l.y);
        self.w = r - self.x;
        self.h = b - self.y;
    }
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0xAC00..=0xD7AF | 0x20000..=0x2FA1F)
}

/// 把两行接成一段：中日文直接连，英文加空格，行尾连字符去掉。
pub fn join_text(a: &str, b: &str) -> String {
    let (a, b) = (a.trim_end(), b.trim_start());
    if a.is_empty() {
        return b.to_string();
    }
    if b.is_empty() {
        return a.to_string();
    }
    let (la, fb) = (a.chars().last().unwrap_or(' '), b.chars().next().unwrap_or(' '));
    if is_cjk(la) || is_cjk(fb) {
        format!("{a}{b}")
    } else if la == '-' && a.chars().rev().nth(1).is_some_and(|c| c.is_alphabetic()) && fb.is_lowercase() {
        format!("{}{b}", &a[..a.len() - 1])
    } else {
        format!("{a} {b}")
    }
}

fn overlap_len(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// 先把同一行里被拆开的几段拼起来，再把相邻的行并成段落。
/// 横排：从上到下；竖排：从右到左一列一列。
pub fn group_lines(lines: &[OLine]) -> Vec<Group> {
    let mut ls: Vec<OLine> = lines.iter().filter(|l| !l.text.trim().is_empty() && l.w > 0.5 && l.h > 0.5).cloned().collect();
    let mut out: Vec<Group> = vec![];
    for vertical in [false, true] {
        let mut part: Vec<OLine> = ls.iter().filter(|l| l.vertical == vertical).cloned().collect();
        if vertical {
            part.sort_by(|a, b| b.cx().total_cmp(&a.cx()).then(a.y.total_cmp(&b.y)));
        } else {
            part.sort_by(|a, b| a.cy().total_cmp(&b.cy()).then(a.x.total_cmp(&b.x)));
        }
        out.extend(if vertical { group_vertical(&part) } else { group_horizontal(&part) });
    }
    ls.clear();
    // 阅读顺序：竖排（日漫）靠右的先读，横排从上到下
    out.sort_by(|a, b| if a.vertical && b.vertical { b.x.total_cmp(&a.x) } else { a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)) });
    out
}

fn group_horizontal(sorted: &[OLine]) -> Vec<Group> {
    // 1) 同一行的拼起来
    let mut rows: Vec<OLine> = vec![];
    for l in sorted {
        if let Some(r) = rows.iter_mut().find(|r| {
            let em = r.h.min(l.h);
            (r.cy() - l.cy()).abs() < em * 0.45
                && l.x >= r.x - em * 0.5
                && l.x - r.right() <= em * 1.1
                && (r.h / l.h).clamp(0.0, 9.0) > 0.6
                && (r.h / l.h) < 1.67
        }) {
            let joined = if l.x < r.x { join_text(&l.text, &r.text) } else { join_text(&r.text, &l.text) };
            let (x, y) = (r.x.min(l.x), r.y.min(l.y));
            let (rr, bb) = (r.right().max(l.right()), r.bottom().max(l.bottom()));
            *r = OLine { text: joined, x, y, w: rr - x, h: bb - y, vertical: false };
        } else {
            rows.push(l.clone());
        }
    }
    rows.sort_by(|a, b| a.cy().total_cmp(&b.cy()).then(a.x.total_cmp(&b.x)));
    // 2) 相邻的行并成段落
    let mut groups: Vec<Group> = vec![];
    for l in &rows {
        let target = groups.iter_mut().rev().take(6).find(|g| {
            let em = g.em.min(l.h);
            let gap = l.y - g.bottom();
            let ratio = l.h / g.em;
            let near = gap >= -em * 0.5 && gap <= em * 0.85;
            let same_size = (0.62..1.6).contains(&ratio);
            let ov = overlap_len(g.x, g.right(), l.x, l.right());
            let aligned = ov >= 0.25 * g.w.min(l.w) || (l.x - g.x).abs() <= em * 2.0 || (l.cx() - (g.x + g.w / 2.0)).abs() <= em * 3.0;
            near && same_size && aligned
        });
        match target {
            Some(g) => {
                g.text = join_text(&g.text, &l.text);
                g.grow(l);
                g.lines += 1;
            }
            None => groups.push(Group::from_line(l)),
        }
    }
    for g in &mut groups {
        g.align = align_of(&g.spans, g.em);
    }
    groups
}

fn group_vertical(sorted: &[OLine]) -> Vec<Group> {
    let mut groups: Vec<Group> = vec![];
    for l in sorted {
        let target = groups.iter_mut().rev().take(6).find(|g| {
            let em = g.em.min(l.w);
            // 新的一列在已有列的左边
            let gap = g.x - l.right();
            let near = gap >= -em * 0.5 && gap <= em * 0.9;
            let ratio = l.w / g.em;
            let ov = overlap_len(g.y, g.bottom(), l.y, l.bottom());
            near && (0.62..1.6).contains(&ratio) && ov >= 0.3 * g.h.min(l.h)
        });
        match target {
            Some(g) => {
                g.text = join_text(&g.text, &l.text);
                g.grow(l);
                g.lines += 1;
            }
            None => groups.push(Group::from_line(l)),
        }
    }
    groups
}

// ---------- 要不要翻 ----------

fn has_kana(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF))
}
fn has_hangul(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0xAC00..=0xD7AF | 0x1100..=0x11FF))
}
fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// 目标语言的粗略分类：`zh` / `ja` / `ko` / `en`（其余按拉丁字母处理）。
pub fn lang_class(target: &str) -> &'static str {
    let t = target.to_lowercase();
    if t.contains("中") || t.starts_with("zh") || t.contains("chinese") {
        "zh"
    } else if t.contains("日") || t.starts_with("ja") || t.contains("japan") {
        "ja"
    } else if t.contains("韩") || t.contains("朝鲜") || t.starts_with("ko") || t.contains("korea") {
        "ko"
    } else {
        "en"
    }
}

/// 数字、符号、已经是目标语言的文字，不用翻。
pub fn should_skip(text: &str, target: &str) -> bool {
    let t = text.trim();
    let non_space: Vec<char> = t.chars().filter(|c| !c.is_whitespace()).collect();
    if non_space.is_empty() {
        return true;
    }
    let letters = non_space.iter().filter(|c| c.is_alphabetic()).count();
    if letters == 0 || (letters as f32) / (non_space.len() as f32) < 0.4 {
        return true;
    }
    if letters == 1 && non_space.len() <= 2 && non_space[0].is_ascii() {
        return true;
    }
    match lang_class(target) {
        "zh" => {
            let han = non_space.iter().filter(|c| is_han(**c)).count();
            han as f32 / letters as f32 >= 0.6 && !has_kana(t) && !has_hangul(t)
        }
        "ja" => has_kana(t) && !non_space.iter().any(|c| c.is_ascii_alphabetic() && letters < 3),
        "ko" => has_hangul(t) && !has_kana(t),
        _ => {
            // 全是拉丁字母，多半已经是英文（粗略判断，别的拉丁语系交给模型）
            letters > 0
                && non_space.iter().filter(|c| c.is_alphabetic()).all(|c| c.is_ascii())
                && t.split_whitespace().any(|w| w.len() > 1)
                && is_probably_english(t)
        }
    }
}

fn is_probably_english(t: &str) -> bool {
    const COMMON: &[&str] = &[
        "the", "and", "you", "to", "of", "is", "in", "it", "that", "for", "on", "with", "are", "this", "was", "have", "not", "your", "but", "can", "will",
        "all",
    ];
    let words: Vec<String> = t.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    if words.is_empty() {
        return false;
    }
    words.iter().filter(|w| COMMON.contains(&w.as_str())).count() * 6 >= words.len().min(8)
}

// ---------- 字幕 ----------

fn norm(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).take(240).collect()
}

/// 两段文字有多像（0–1），按编辑距离算。
pub fn similarity(a: &str, b: &str) -> f32 {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    1.0 - prev[b.len()] as f32 / a.len().max(b.len()) as f32
}

pub fn ends_sentence(s: &str) -> bool {
    s.trim_end()
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '…' | '」' | '』' | '”' | '"' | '）' | ')' | '♪' | '～' | '~'))
}

/// 字幕一句话分在两屏时：上一屏没说完、间隔不长，就把新的一屏接在后面再翻译。
/// 返回接好的整句；不需要接就是 None。
pub fn merge_continuation(prev: &str, new: &str, gap_ms: u64, window_ms: u64) -> Option<String> {
    let (p, n) = (prev.trim(), new.trim());
    if p.is_empty() || n.is_empty() || gap_ms > window_ms || ends_sentence(p) {
        return None;
    }
    // 新的一屏已经包含上一屏（逐字出现 / 滚动字幕）：用新的就行
    if similarity(p, n) >= 0.8 || norm(n).windows(norm(p).len().max(1)).any(|w| w == norm(p).as_slice()) {
        return None;
    }
    // 英文新句子以大写开头且上一屏很短，多半是新的一句
    if n.chars().next().is_some_and(|c| c.is_uppercase()) && p.chars().count() < 4 {
        return None;
    }
    Some(join_text(p, n))
}

// ---------- 画面变化 ----------

pub const SIG_W: usize = 64;
pub const SIG_H: usize = 36;

/// 缩成 64×36 的灰度小图。
pub fn signature(bgra: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; SIG_W * SIG_H];
    if w == 0 || h == 0 || bgra.len() < w * h * 4 {
        return out;
    }
    for sy in 0..SIG_H {
        let (y0, y1) = (sy * h / SIG_H, ((sy + 1) * h / SIG_H).max(sy * h / SIG_H + 1).min(h));
        for sx in 0..SIG_W {
            let (x0, x1) = (sx * w / SIG_W, ((sx + 1) * w / SIG_W).max(sx * w / SIG_W + 1).min(w));
            let (mut acc, mut n) = (0u32, 0u32);
            // 每个小格最多抽 4×4 个点
            let (stepy, stepx) = (((y1 - y0) / 4).max(1), ((x1 - x0) / 4).max(1));
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let i = (y * w + x) * 4;
                    acc += (bgra[i] as u32 * 29 + bgra[i + 1] as u32 * 150 + bgra[i + 2] as u32 * 77) >> 8;
                    n += 1;
                    x += stepx;
                }
                y += stepy;
            }
            out[sy * SIG_W + sx] = (acc / n.max(1)) as u8;
        }
    }
    out
}

/// 有多少比例的小格变了（0–1）。
pub fn change_ratio(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    a.iter().zip(b).filter(|(x, y)| x.abs_diff(**y) > 14).count() as f32 / a.len() as f32
}

// ---------- 取色 ----------

fn px(bgra: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
    let i = (y * w + x) * 4;
    [bgra[i + 2], bgra[i + 1], bgra[i]]
}

/// 邻近修补：取框上、下各一条紧贴外侧的像素平均色，做成上下渐变（网点、渐变底比纯色自然）。
/// 两端颜色很接近时返回 None，调用方就用纯色。
pub fn edge_gradient(bgra: &[u8], w: usize, h: usize, x: f32, y: f32, rw: f32, rh: f32) -> Option<([u8; 3], [u8; 3])> {
    if w < 4 || h < 4 || bgra.len() < w * h * 4 {
        return None;
    }
    let x0 = (x.max(0.0) as usize).min(w - 1);
    let x1 = ((x + rw).max(0.0) as usize).min(w - 1).max(x0);
    let strip = |yy: i64| -> Option<[u8; 3]> {
        if yy < 0 || yy as usize >= h {
            return None;
        }
        let step = ((x1 - x0) / 24).max(1);
        let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
        let mut xx = x0;
        while xx <= x1 {
            let c = px(bgra, w, xx, yy as usize);
            r += u32::from(c[0]);
            g += u32::from(c[1]);
            b += u32::from(c[2]);
            n += 1;
            xx += step;
        }
        (n > 0).then(|| [(r / n) as u8, (g / n) as u8, (b / n) as u8])
    };
    let top = strip(y as i64 - 2);
    let bottom = strip((y + rh) as i64 + 2);
    let (a, b) = match (top, bottom) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) | (None, Some(a)) => (a, a),
        _ => return None,
    };
    let d = a.iter().zip(b.iter()).map(|(p, q)| (i32::from(*p) - i32::from(*q)).abs()).sum::<i32>();
    (d >= 24).then_some((a, b))
}

fn lum(c: [u8; 3]) -> f32 {
    fn f(v: u8) -> f32 {
        let s = v as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
}

pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn dist(a: [u8; 3], b: [u8; 3]) -> i32 {
    (a[0] as i32 - b[0] as i32).abs() + (a[1] as i32 - b[1] as i32).abs() + (a[2] as i32 - b[2] as i32).abs()
}

/// 文字框周围的底色（出现最多的颜色）和框内的文字颜色（离底色最远的那批点的平均）。
/// 文字颜色对比度不够 4.5 时，改用黑 / 白。
pub fn sample_colors(bgra: &[u8], w: usize, h: usize, x: f32, y: f32, rw: f32, rh: f32) -> ([u8; 3], [u8; 3]) {
    if w == 0 || h == 0 || bgra.len() < w * h * 4 {
        return ([0, 0, 0], [255, 255, 255]);
    }
    let cl = |v: f32, hi: usize| -> usize { (v.round().max(0.0) as usize).min(hi.saturating_sub(1)) };
    let (x0, y0) = (cl(x, w), cl(y, h));
    let (x1, y1) = (cl(x + rw, w).max(x0), cl(y + rh, h).max(y0));
    // 外圈：框往外 3 像素的一圈
    let pad = 3usize;
    let (ox0, oy0) = (x0.saturating_sub(pad), y0.saturating_sub(pad));
    let (ox1, oy1) = ((x1 + pad).min(w - 1), (y1 + pad).min(h - 1));
    let mut ring: Vec<[u8; 3]> = vec![];
    let step = (((ox1 - ox0) + (oy1 - oy0)) / 80).max(1);
    let mut xx = ox0;
    while xx <= ox1 {
        ring.push(px(bgra, w, xx, oy0));
        ring.push(px(bgra, w, xx, oy1));
        xx += step;
    }
    let mut yy = oy0;
    while yy <= oy1 {
        ring.push(px(bgra, w, ox0, yy));
        ring.push(px(bgra, w, ox1, yy));
        yy += step;
    }
    let mut buckets: std::collections::HashMap<u16, (u32, [u32; 3])> = std::collections::HashMap::new();
    for c in &ring {
        let k = ((c[0] as u16 >> 4) << 8) | ((c[1] as u16 >> 4) << 4) | (c[2] as u16 >> 4);
        let e = buckets.entry(k).or_insert((0, [0; 3]));
        e.0 += 1;
        for (acc, v) in e.1.iter_mut().zip(c.iter()) {
            *acc += *v as u32;
        }
    }
    let bg = buckets.values().max_by_key(|(n, _)| *n).map(|(n, s)| [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]).unwrap_or([0, 0, 0]);
    // 框内：离底色最远的 8%
    let mut inner: Vec<([u8; 3], i32)> = vec![];
    let (sy, sx) = (((y1 - y0) / 24).max(1), ((x1 - x0) / 48).max(1));
    let mut yy = y0;
    while yy <= y1 {
        let mut xx = x0;
        while xx <= x1 {
            let c = px(bgra, w, xx, yy);
            inner.push((c, dist(c, bg)));
            xx += sx;
        }
        yy += sy;
    }
    inner.sort_by_key(|(_, d)| -*d);
    let take = (inner.len() / 12).max(1).min(inner.len());
    let mut fg = if inner.is_empty() {
        [255, 255, 255]
    } else {
        let mut s = [0u32; 3];
        for (c, _) in &inner[..take] {
            for (acc, v) in s.iter_mut().zip(c.iter()) {
                *acc += *v as u32;
            }
        }
        [(s[0] / take as u32) as u8, (s[1] / take as u32) as u8, (s[2] / take as u32) as u8]
    };
    if contrast(bg, fg) < 4.5 {
        fg = if contrast(bg, [0, 0, 0]) >= contrast(bg, [255, 255, 255]) { [0, 0, 0] } else { [255, 255, 255] };
    }
    (bg, fg)
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// 双线性放大（给识别很小的字用）。
pub fn upscale(bgra: &[u8], w: usize, h: usize, k: usize) -> (Vec<u8>, usize, usize) {
    if k <= 1 || w == 0 || h == 0 || bgra.len() < w * h * 4 {
        return (bgra.to_vec(), w, h);
    }
    let (nw, nh) = (w * k, h * k);
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) / k as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) / k as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let g = |xx: usize, yy: usize| bgra[(yy * w + xx) * 4 + c] as f32;
                let top = g(x0, y0) * (1.0 - tx) + g(x1, y0) * tx;
                let bot = g(x0, y1) * (1.0 - tx) + g(x1, y1) * tx;
                out[(y * nw + x) * 4 + c] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
    (out, nw, nh)
}

/// 双线性缩放到任意大小（缩小时先按整数倍取平均，再插值，减少锯齿）。
pub fn resize_bgra(bgra: &[u8], w: usize, h: usize, nw: usize, nh: usize) -> Vec<u8> {
    if w == 0 || h == 0 || nw == 0 || nh == 0 || bgra.len() < w * h * 4 {
        return vec![0; nw * nh * 4];
    }
    if (w, h) == (nw, nh) {
        return bgra[..w * h * 4].to_vec();
    }
    let mut out = vec![0u8; nw * nh * 4];
    let (sx, sy) = (w as f32 / nw as f32, h as f32 / nh as f32);
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy.floor() as usize, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx.floor() as usize, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let g = |xx: usize, yy: usize| bgra[(yy * w + xx) * 4 + c] as f32;
                let top = g(x0, y0) * (1.0 - tx) + g(x1, y0) * tx;
                let bot = g(x0, y1) * (1.0 - tx) + g(x1, y1) * tx;
                out[(y * nw + x) * 4 + c] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
    out
}

/// 取出一块区域（BGRA）。
pub fn crop_bgra(bgra: &[u8], w: usize, h: usize, x: usize, y: usize, cw: usize, ch: usize) -> Vec<u8> {
    let (x, y) = (x.min(w), y.min(h));
    let (cw, ch) = (cw.min(w - x), ch.min(h - y));
    let mut out = Vec::with_capacity(cw * ch * 4);
    for row in y..y + ch {
        out.extend_from_slice(&bgra[(row * w + x) * 4..(row * w + x + cw) * 4]);
    }
    out
}

/// 一屏文字的指纹：只看识别出的文字，用来判断“这一屏的文字有没有变”。
pub fn text_key(groups: &[Group]) -> String {
    groups.iter().map(|g| norm(&g.text).into_iter().collect::<String>()).collect::<Vec<_>>().join("|")
}

/// 一段文字的比较用键（去标点和大小写）。
pub fn text_key_of(s: &str) -> String {
    norm(s).into_iter().collect()
}

/// 漫画：视觉模型给的框只是个大概。在框向外扩 `grow` 倍的范围里找和底色差别大的点（字），返回它们的外接框；
/// 找到的点太少（框里其实没字）或外接框比原框大太多，就返回 None，沿用模型的框。
#[allow(clippy::too_many_arguments)]
pub fn ink_bbox(bgra: &[u8], w: usize, h: usize, x: f32, y: f32, rw: f32, rh: f32, bg: [u8; 3], grow: f32) -> Option<[f32; 4]> {
    if w == 0 || h == 0 || bgra.len() < w * h * 4 || rw < 4.0 || rh < 4.0 {
        return None;
    }
    let (gx, gy) = (rw * grow, rh * grow);
    let x0 = (x - gx).floor().max(0.0) as usize;
    let y0 = (y - gy).floor().max(0.0) as usize;
    let x1 = ((x + rw + gx).ceil() as usize).min(w);
    let y1 = ((y + rh + gy).ceil() as usize).min(h);
    let (mut bx0, mut by0, mut bx1, mut by1, mut n) = (usize::MAX, usize::MAX, 0usize, 0usize, 0usize);
    for yy in y0..y1 {
        for xx in x0..x1 {
            let i = (yy * w + xx) * 4;
            if dist([bgra[i + 2], bgra[i + 1], bgra[i]], bg) > 150 {
                bx0 = bx0.min(xx);
                by0 = by0.min(yy);
                bx1 = bx1.max(xx + 1);
                by1 = by1.max(yy + 1);
                n += 1;
            }
        }
    }
    if n < 8 || bx1 <= bx0 || by1 <= by0 {
        return None;
    }
    let (ow, oh) = ((bx1 - bx0) as f32, (by1 - by0) as f32);
    // 外接框不该比模型的框大出一倍以上（多半把气泡的描边也算进去了）
    if ow > rw * (1.0 + 2.0 * grow) * 1.05 || oh > rh * (1.0 + 2.0 * grow) * 1.05 || ow < rw * 0.3 || oh < rh * 0.3 {
        return None;
    }
    Some([bx0 as f32, by0 as f32, ow, oh])
}

#[cfg(test)]
mod tests {
    #[test]
    fn edge_gradient_only_when_ends_differ() {
        let (w, h) = (20usize, 40usize);
        let mut img = vec![255u8; w * h * 4];
        // 上半黑、下半白
        for y in 0..20 {
            for x in 0..w {
                let i = (y * w + x) * 4;
                img[i] = 0;
                img[i + 1] = 0;
                img[i + 2] = 0;
            }
        }
        assert!(edge_gradient(&img, w, h, 2.0, 10.0, 10.0, 20.0).is_some());
        let flat = vec![200u8; w * h * 4];
        assert!(edge_gradient(&flat, w, h, 2.0, 10.0, 10.0, 20.0).is_none());
    }

    use super::*;

    fn l(t: &str, x: f32, y: f32, w: f32, h: f32) -> OLine {
        OLine::new(t, x, y, w, h)
    }

    #[test]
    fn alignment_is_read_from_how_the_lines_end() {
        let em = 20.0;
        // 靠左：左边一条线，右边参差
        assert_eq!(align_of(&[(10.0, 200.0), (10.0, 150.0), (11.0, 180.0)], em), 0);
        // 居中：中线一条线，左右都不齐
        assert_eq!(align_of(&[(60.0, 160.0), (40.0, 180.0), (80.0, 140.0)], em), 1);
        // 靠右：右边一条线
        assert_eq!(align_of(&[(10.0, 200.0), (60.0, 200.0), (100.0, 201.0)], em), 2);
        // 单行没法判断，按靠左
        assert_eq!(align_of(&[(50.0, 150.0)], em), 0);
        assert_eq!(align_of(&[], em), 0);
        // 什么都不齐：按靠左
        assert_eq!(align_of(&[(10.0, 100.0), (50.0, 300.0), (90.0, 150.0)], em), 0);
    }

    #[test]
    fn grouped_paragraph_remembers_its_alignment() {
        // 三行居中的标题文字上下相连
        let ls = vec![l("Welcome to the", 100.0, 10.0, 200.0, 20.0), l("ClearClip", 140.0, 34.0, 120.0, 20.0), l("translator app", 120.0, 58.0, 160.0, 20.0)];
        let g = group_lines(&ls);
        assert_eq!(g.len(), 1);
        assert_eq!((g[0].lines, g[0].align), (3, 1));
        // 左对齐的两行
        let ls = vec![l("First line of text", 10.0, 10.0, 200.0, 20.0), l("second", 10.0, 34.0, 90.0, 20.0)];
        let g = group_lines(&ls);
        assert_eq!((g[0].lines, g[0].align), (2, 0));
    }

    #[test]
    fn merges_wrapped_lines_into_one_paragraph() {
        let g = group_lines(&[
            l("The ruins to the north are guarded", 14.0, 10.0, 300.0, 20.0),
            l("by an ancient spirit.", 14.0, 32.0, 150.0, 20.0),
            l("Inventory", 400.0, 200.0, 80.0, 20.0),
        ]);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].text, "The ruins to the north are guarded by an ancient spirit.");
        assert_eq!(g[0].lines, 2);
        assert_eq!(g[1].text, "Inventory");
    }

    #[test]
    fn far_apart_or_different_sized_lines_stay_separate() {
        let g = group_lines(&[l("Title", 10.0, 10.0, 200.0, 40.0), l("small print here", 10.0, 55.0, 150.0, 14.0), l("Another", 10.0, 300.0, 100.0, 20.0)]);
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn words_on_the_same_row_are_joined() {
        let g = group_lines(&[l("Save", 10.0, 10.0, 40.0, 20.0), l("Game", 58.0, 11.0, 44.0, 20.0)]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].text, "Save Game");
    }

    #[test]
    fn vertical_columns_read_right_to_left() {
        // 竖排：右边一列先读
        let a = l("まさか、ここまで", 200.0, 20.0, 24.0, 160.0);
        let b = l("来るとは…", 172.0, 20.0, 24.0, 100.0);
        assert!(a.vertical && b.vertical);
        let g = group_lines(&[b.clone(), a.clone()]);
        assert_eq!(g.len(), 1);
        assert!(g[0].vertical);
        assert_eq!(g[0].text, "まさか、ここまで来るとは…");
        assert_eq!(g[0].lines, 2);
    }

    #[test]
    fn vertical_and_horizontal_are_grouped_separately() {
        let g = group_lines(&[l("逃げろ！", 300.0, 20.0, 26.0, 110.0), l("Hello world", 20.0, 400.0, 120.0, 20.0)]);
        assert_eq!(g.len(), 2);
        assert!(g.iter().any(|x| x.vertical) && g.iter().any(|x| !x.vertical));
    }

    #[test]
    fn joins_text_by_script() {
        assert_eq!(join_text("你好", "世界"), "你好世界");
        assert_eq!(join_text("hello", "world"), "hello world");
        assert_eq!(join_text("exam-", "ple"), "example");
    }

    #[test]
    fn skip_rules() {
        assert!(should_skip("HP 128/200", "简体中文"));
        assert!(should_skip("12:30", "简体中文"));
        assert!(should_skip("你好，世界", "简体中文"));
        assert!(!should_skip("Inventory", "简体中文"));
        assert!(!should_skip("日本語のテキスト", "简体中文"));
        assert!(!should_skip("これは漢字です", "简体中文"));
        assert!(should_skip("This is what you can do for the team", "English"));
        assert!(!should_skip("你好", "English"));
        assert!(should_skip("ひらがな", "日本語"));
        assert!(should_skip("%", "简体中文"));
    }

    #[test]
    fn similarity_and_continuation() {
        assert!(similarity("Hello, world!", "hello world") > 0.95);
        assert!(similarity("abc", "xyz") < 0.2);
        // 上一屏没说完 → 接上
        assert_eq!(merge_continuation("I went to the", "store yesterday.", 800, 3000).unwrap(), "I went to the store yesterday.");
        // 上一屏已是整句 / 间隔太久 → 不接
        assert!(merge_continuation("I went home.", "Then I slept.", 800, 3000).is_none());
        assert!(merge_continuation("I went to the", "store yesterday.", 5000, 3000).is_none());
        // 逐字出现：新的包含旧的 → 用新的
        assert!(merge_continuation("I went to", "I went to the store", 300, 3000).is_none());
        assert_eq!(merge_continuation("今日は天気が", "いいですね", 500, 3000).unwrap(), "今日は天気がいいですね");
    }

    fn solid(w: usize, h: usize, bgra: [u8; 4]) -> Vec<u8> {
        std::iter::repeat(bgra).take(w * h).flatten().collect()
    }

    #[test]
    fn change_detection() {
        let a = solid(200, 100, [10, 10, 10, 255]);
        let mut b = a.clone();
        let (sa, sb) = (signature(&a, 200, 100), signature(&b, 200, 100));
        assert_eq!(change_ratio(&sa, &sb), 0.0);
        // 画一块亮的
        for y in 10..60 {
            for x in 20..120 {
                let i = (y * 200 + x) * 4;
                b[i..i + 3].copy_from_slice(&[240, 240, 240]);
            }
        }
        let r = change_ratio(&sa, &signature(&b, 200, 100));
        assert!(r > 0.2 && r < 0.6, "{r}");
        assert_eq!(change_ratio(&sa, &sa[..10]), 1.0);
    }

    #[test]
    fn samples_background_and_text_colors() {
        // 深蓝底、白字
        let (w, h) = (120, 60);
        let mut img = solid(w, h, [100, 40, 20, 255]); // B G R → 红 20 绿 40 蓝 100
        for y in 20..34 {
            for x in 30..90 {
                if (x / 3) % 2 == 0 {
                    let i = (y * w + x) * 4;
                    img[i..i + 3].copy_from_slice(&[255, 255, 255]);
                }
            }
        }
        let (bg, fg) = sample_colors(&img, w, h, 28.0, 18.0, 64.0, 18.0);
        assert_eq!(bg, [20, 40, 100]);
        assert!(fg[0] > 200 && fg[1] > 200 && fg[2] > 200);
        assert!(contrast(bg, fg) >= 4.5);
        // 文字颜色和底色太接近时改用黑白
        let flat = solid(w, h, [120, 120, 120, 255]);
        let (bg2, fg2) = sample_colors(&flat, w, h, 10.0, 10.0, 50.0, 20.0);
        assert_eq!(bg2, [120, 120, 120]);
        assert!(contrast(bg2, fg2) >= 4.5, "{fg2:?}");
        assert_eq!(hex([1, 2, 255]), "#0102ff");
    }

    #[test]
    fn resize_keeps_flat_color() {
        let img = solid(10, 8, [7, 99, 200, 255]);
        let r = resize_bgra(&img, 10, 8, 5, 4);
        assert_eq!(r.len(), 5 * 4 * 4);
        assert!(r.chunks_exact(4).all(|p| p == [7, 99, 200, 255]));
        assert_eq!(resize_bgra(&img, 10, 8, 10, 8), img);
    }

    #[test]
    fn upscale_and_crop() {
        let img = solid(4, 3, [0, 128, 255, 255]);
        let (big, nw, nh) = upscale(&img, 4, 3, 2);
        assert_eq!((nw, nh, big.len()), (8, 6, 8 * 6 * 4));
        assert_eq!(&big[..4], &[0, 128, 255, 255]);
        let c = crop_bgra(&img, 4, 3, 1, 1, 10, 10);
        assert_eq!(c.len(), 3 * 2 * 4);
    }

    #[test]
    fn text_key_ignores_punctuation_and_case() {
        let a = vec![Group { text: "Hello, World!".into(), ..Default::default() }];
        let b = vec![Group { text: "hello world".into(), ..Default::default() }];
        assert_eq!(text_key(&a), text_key(&b));
    }

    #[test]
    fn ink_box_tightens_to_text() {
        let (w, h) = (100, 60);
        let mut img = vec![255u8; w * h * 4];
        for y in 20..30 {
            for x in 30..60 {
                let i = (y * w + x) * 4;
                img[i] = 10;
                img[i + 1] = 10;
                img[i + 2] = 10;
            }
        }
        // 模型的框偏大一点
        let b = ink_bbox(&img, w, h, 25.0, 15.0, 40.0, 20.0, [255, 255, 255], 0.1).unwrap();
        assert_eq!(b, [30.0, 20.0, 30.0, 10.0]);
        // 框里没有字
        assert!(ink_bbox(&img, w, h, 0.0, 40.0, 20.0, 15.0, [255, 255, 255], 0.1).is_none());
    }
}
