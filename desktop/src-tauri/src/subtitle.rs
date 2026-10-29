//! 字幕处理：格式转换（VTT / B站 CC JSON → SRT，B站弹幕 XML → ASS）、按时间段裁剪、语言匹配。
//!
//! 转换全部在程序内完成，不依赖 ffmpeg。

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::error::{AppError, AppResult};

/// 一条字幕（时间单位毫秒）。
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: u64,
    pub end: u64,
    pub lines: Vec<String>,
}

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]*>").unwrap());
static ROLLING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\d{1,2}:\d{2}:\d{2}\.\d{3}>").unwrap());

fn unescape(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `00:01:02.500` / `01:02.500` / `00:01:02,500` → 毫秒。
fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim().replace(',', ".");
    let (hms, frac) = s.split_once('.').unwrap_or((&s, "0"));
    let parts: Vec<u64> = hms.split(':').map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [m, s] => m * 60 + s,
        _ => return None,
    };
    let frac_ms: u64 = format!("{:0<3}", frac.chars().take(3).collect::<String>()).parse().ok()?;
    Some(secs * 1000 + frac_ms)
}

pub(crate) fn fmt_srt_time(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub(crate) fn fmt_ass_time(cs_total: u64) -> String {
    format!("{}:{:02}:{:02}.{:02}", cs_total / 360_000, cs_total / 6000 % 60, cs_total / 100 % 60, cs_total % 100)
}

pub fn parse_vtt(text: &str) -> Vec<Cue> {
    parse_blocks(text, true)
}

/// VTT 里的标记（`<c>`、`<v>`、时间戳）和 HTML 实体需要清理；SRT 里的 `<i>` `<b>` 保持原样。
fn parse_blocks(text: &str, vtt: bool) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = vec![];
    for block in text.split("\n\n") {
        let mut lines = block.lines().filter(|l| !l.trim().is_empty()).peekable();
        let Some(first) = lines.peek().copied() else { continue };
        if first.starts_with("WEBVTT") || first.starts_with("NOTE") || first.starts_with("STYLE") || first.starts_with("REGION") {
            continue;
        }
        let Some(timing) = lines.by_ref().find(|l| l.contains("-->")) else { continue };
        let (a, b) = timing.split_once("-->").unwrap();
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b.split_whitespace().next().unwrap_or(""))) else { continue };
        let clean = |l: &str| if vtt { unescape(&TAG_RE.replace_all(l, "")) } else { l.to_string() };
        let body: Vec<String> = lines.map(clean).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        cues.push(Cue { start, end, lines: body });
    }
    cues
}

/// YouTube 自动字幕是“滚动”的：后一条字幕会重复前一条的最后几行，还夹着只有 10 毫秒的过渡条。
/// 只保留每条里新出现的行，并把重复的时间并入前一条。
pub fn dedupe_rolling(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out: Vec<Cue> = vec![];
    let mut prev: Vec<String> = vec![];
    for cue in cues {
        let max = prev.len().min(cue.lines.len());
        let k = (0..=max).rev().find(|k| prev[prev.len() - k..] == cue.lines[..*k]).unwrap_or(0);
        let new_lines = cue.lines[k..].to_vec();
        prev = cue.lines.clone();
        if new_lines.is_empty() {
            if let Some(last) = out.last_mut() {
                if cue.start <= last.end + 50 {
                    last.end = last.end.max(cue.end);
                }
            }
            continue;
        }
        // 前一条还没结束就出现新行：把前一条截到这里，避免两条同时显示
        if let Some(last) = out.last_mut() {
            if last.end > cue.start {
                last.end = cue.start;
            }
        }
        out.push(Cue { start: cue.start, end: cue.end, lines: new_lines });
    }
    out.retain(|c| c.end > c.start);
    out
}

pub fn format_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, fmt_srt_time(c.start), fmt_srt_time(c.end), c.lines.join("\n")));
    }
    out
}

pub fn parse_srt(text: &str) -> Vec<Cue> {
    // SRT 与 VTT 的结构相同（序号行 + 时间行 + 文字），时间分隔符逗号已在 parse_time 里兼容
    parse_blocks(&format!("WEBVTT\n\n{}", text.trim_start_matches('\u{feff}')), false)
}

/// ASS / SSA 的对白行 → 字幕。只取文字和时间：去掉 `{\\an8}` 之类的样式标记，`\\N` 当换行，忽略绘图和注释行。
pub fn parse_ass(text: &str) -> Vec<Cue> {
    static OVERRIDE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^}]*\}").unwrap());
    static DRAWING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^}]*\\p[1-9]").unwrap());
    let mut fmt: Vec<String> = vec![];
    let mut cues = vec![];
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(f) = line.strip_prefix("Format:") {
            fmt = f.split(',').map(|x| x.trim().to_ascii_lowercase()).collect();
        } else if let Some(d) = line.strip_prefix("Dialogue:") {
            let n = if fmt.is_empty() { 10 } else { fmt.len() };
            let parts: Vec<&str> = d.trim_start().splitn(n, ',').collect();
            let at = |name: &str, default: usize| fmt.iter().position(|x| x == name).unwrap_or(default);
            let (Some(s), Some(e), Some(t)) = (parts.get(at("start", 1)), parts.get(at("end", 2)), parts.get(at("text", 9))) else { continue };
            let (Some(start), Some(end)) = (parse_time(s), parse_time(e)) else { continue };
            // {\p1} 之后是矢量绘图指令（形状、卡拉 OK 特效），不是文字
            if DRAWING_RE.is_match(t) {
                continue;
            }
            let t = OVERRIDE_RE.replace_all(t, "").replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " ");
            let lines: Vec<String> = t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            if !lines.is_empty() && end > start {
                cues.push(Cue { start, end, lines });
            }
        }
    }
    cues.sort_by_key(|c| c.start);
    cues
}

pub fn vtt_to_srt(text: &str) -> String {
    let mut cues = parse_vtt(text);
    if ROLLING_RE.is_match(text) {
        cues = dedupe_rolling(cues);
    }
    cues.retain(|c| !c.lines.is_empty());
    format_srt(&cues)
}

/// B站 CC 字幕 JSON：`{"body":[{"from":0.5,"to":2.3,"content":"…"}]}`。
pub fn bilibili_json_to_srt(text: &str) -> AppResult<String> {
    let v: Value = serde_json::from_str(text).map_err(|_| AppError::parser("字幕文件不是有效的 JSON。"))?;
    let body = v.get("body").and_then(Value::as_array).ok_or_else(|| AppError::parser("字幕文件里没有内容。"))?;
    let cues: Vec<Cue> = body
        .iter()
        .filter_map(|e| {
            let from = e.get("from").and_then(Value::as_f64)?;
            let to = e.get("to").and_then(Value::as_f64)?;
            let lines: Vec<String> = e.get("content").and_then(Value::as_str)?.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
            (!lines.is_empty()).then(|| Cue { start: (from * 1000.0).round() as u64, end: (to * 1000.0).round() as u64, lines })
        })
        .collect();
    if cues.is_empty() {
        return Err(AppError::parser("字幕文件里没有内容。"));
    }
    Ok(format_srt(&cues))
}

/// 只保留 `start..end`（毫秒）内的字幕，并把时间平移到从 0 开始（对应裁剪后的视频）。
pub fn cut(cues: &[Cue], start: u64, end: Option<u64>) -> Vec<Cue> {
    cues.iter()
        .filter(|c| c.end > start && end.map_or(true, |e| c.start < e))
        .map(|c| Cue { start: c.start.saturating_sub(start), end: end.map_or(c.end, |e| c.end.min(e)).saturating_sub(start), lines: c.lines.clone() })
        .collect()
}

// ---------- 弹幕 XML → ASS ----------

/// 弹幕布局参数。
#[derive(Debug, Clone, Copy)]
pub struct DanmakuLayout {
    pub width: u32,
    pub height: u32,
    /// 弹幕占屏幕高度的比例（避免挡住底部字幕）
    pub area: f64,
    /// 基础字号
    pub font_size: f64,
    /// 不透明度 0–1
    pub opacity: f64,
    /// 滚动弹幕穿过屏幕的秒数
    pub scroll_secs: f64,
}

impl Default for DanmakuLayout {
    fn default() -> Self {
        DanmakuLayout { width: 1920, height: 1080, area: 0.75, font_size: 40.0, opacity: 1.0, scroll_secs: 7.0 }
    }
}

impl DanmakuLayout {
    /// 按视频宽高比得到布局（高度固定 1080）。
    pub fn for_video(width: Option<u32>, height: Option<u32>) -> Self {
        match (width, height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => {
                DanmakuLayout { width: ((1080.0 * w as f64 / h as f64).round() as u32).clamp(360, 3840), ..Default::default() }
            }
            _ => Self::default(),
        }
    }

    /// 套用用户设置的弹幕样式。
    pub fn styled(self, s: &crate::settings::DanmakuStyle) -> Self {
        DanmakuLayout {
            area: s.area as f64 / 100.0,
            font_size: s.font_size as f64,
            opacity: s.opacity as f64 / 100.0,
            scroll_secs: s.scroll_secs as f64,
            ..self
        }
    }
}

#[derive(Debug, Clone)]
struct Danmaku {
    time: f64,
    mode: u8,
    size: f64,
    color: u32,
    text: String,
}

fn parse_danmaku(xml: &str) -> Vec<Danmaku> {
    let mut out = vec![];
    let mut rest = xml;
    while let Some(i) = rest.find("<d p=\"") {
        rest = &rest[i + 6..];
        let Some(q) = rest.find('"') else { break };
        let p = &rest[..q];
        let Some(gt) = rest[q..].find('>') else { break };
        let body_start = q + gt + 1;
        let Some(end) = rest[body_start..].find("</d>") else { break };
        let text = unescape(&rest[body_start..body_start + end]);
        rest = &rest[body_start + end + 4..];
        let f: Vec<&str> = p.split(',').collect();
        if f.len() < 4 {
            continue;
        }
        let (Ok(time), Ok(mode), Ok(size), Ok(color)) = (f[0].parse::<f64>(), f[1].parse::<u8>(), f[2].parse::<f64>(), f[3].parse::<u32>()) else { continue };
        let text = text.replace("/n", "\n").replace('\r', "").trim().to_string();
        if !text.is_empty() {
            out.push(Danmaku { time, mode, size, color, text });
        }
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}

/// 文字估算宽度：全角字符占一个字号，半角约 0.55 个。
fn text_width(text: &str, fs: f64) -> f64 {
    text.lines().map(|l| l.chars().map(|c| if c.is_ascii() { fs * 0.55 } else { fs }).sum::<f64>()).fold(0.0, f64::max)
}

fn ass_escape(text: &str) -> String {
    text.replace('\\', "＼").replace('{', "｛").replace('}', "｝").replace('\n', "\\N")
}

/// B站弹幕 XML 转成 ASS：滚动弹幕按行分配、不重叠；顶部 / 底部弹幕各自占行；
/// 没有空行时丢弃该条（与大多数播放器一致）。高级弹幕（mode 7/8）忽略。
///
/// `clip` 为 `(起点毫秒, 终点毫秒)` 时只保留这一段，时间平移到从 0 开始（对应裁剪后的视频）。
pub fn danmaku_xml_to_ass(xml: &str, layout: DanmakuLayout, clip: Option<(u64, Option<u64>)>) -> AppResult<String> {
    danmaku_xml_to_ass_font(xml, layout, clip, "")
}

/// 同 [`danmaku_xml_to_ass`]，并指定字体名（留空使用微软雅黑）。
pub fn danmaku_xml_to_ass_font(xml: &str, layout: DanmakuLayout, clip: Option<(u64, Option<u64>)>, font: &str) -> AppResult<String> {
    let mut list = parse_danmaku(xml);
    if let Some((start, end)) = clip {
        let (start, end) = (start as f64 / 1000.0, end.map(|e| e as f64 / 1000.0));
        list.retain(|d| d.time >= start && end.map_or(true, |e| d.time < e));
        for d in &mut list {
            d.time -= start;
        }
    }
    if list.is_empty() {
        return Err(AppError::not_found("这个视频没有弹幕。"));
    }
    let (w, h) = (layout.width as f64, layout.height as f64);
    let base_fs = layout.font_size.clamp(18.0, 80.0);
    let row_h = base_fs * 1.2;
    let rows = ((h * layout.area) / row_h).floor().max(1.0) as usize;
    // 滚动速度（像素 / 秒）：所有弹幕同速，后面的不会追上前面的
    let speed = w / layout.scroll_secs.max(1.0);
    let fixed_secs = 4.0;

    // 每行最后一条弹幕“尾部离开右边缘”的时间 / 固定弹幕结束的时间
    let mut scroll_free = vec![f64::MIN; rows];
    let mut top_free = vec![f64::MIN; rows];
    let mut bottom_free = vec![f64::MIN; rows];

    let mut events = String::new();
    let mut kept = 0usize;
    for d in list.iter().take(60_000) {
        // 弹幕自带的字号按基础字号等比缩放（B站默认 25 对应基础字号）
        let fs = (d.size * 1.6 * base_fs / 40.0).clamp(base_fs * 0.6, base_fs * 1.8);
        let fs = if (fs - base_fs).abs() < base_fs * 0.1 { base_fs } else { fs };
        let tw = text_width(&d.text, fs);
        let lines = d.text.lines().count().max(1);
        let span = ((fs * 1.2 * lines as f64) / row_h).ceil().max(1.0) as usize;
        let color = format!("&H{:02X}{:02X}{:02X}&", d.color & 0xff, (d.color >> 8) & 0xff, (d.color >> 16) & 0xff);
        let style = format!("\\fs{}{}", fs.round() as u32, if d.color == 0xffffff { String::new() } else { format!("\\c{color}") });
        let text = ass_escape(&d.text);
        let t = d.time.max(0.0);
        match d.mode {
            1..=3 | 6 => {
                let Some(row) = (0..=rows.saturating_sub(span)).find(|r| (*r..*r + span).all(|k| scroll_free[k] <= t)) else { continue };
                let dur = (w + tw) / speed;
                scroll_free[row..row + span].fill(t + (tw + 40.0) / speed);
                let y = row as f64 * row_h;
                let (x1, x2) = if d.mode == 6 { (-tw, w) } else { (w, -tw) };
                events.push_str(&format!(
                    "Dialogue: 0,{},{},Danmaku,,0,0,0,,{{\\an7\\move({:.0},{:.0},{:.0},{:.0}){style}}}{text}\n",
                    fmt_ass_time((t * 100.0) as u64),
                    fmt_ass_time(((t + dur) * 100.0) as u64),
                    x1,
                    y,
                    x2,
                    y
                ));
            }
            4 => {
                let Some(row) = (0..=rows.saturating_sub(span)).find(|r| (*r..*r + span).all(|k| bottom_free[k] <= t)) else { continue };
                bottom_free[row..row + span].fill(t + fixed_secs);
                let y = h - row as f64 * row_h;
                events.push_str(&format!(
                    "Dialogue: 1,{},{},Danmaku,,0,0,0,,{{\\an2\\pos({:.0},{:.0}){style}}}{text}\n",
                    fmt_ass_time((t * 100.0) as u64),
                    fmt_ass_time(((t + fixed_secs) * 100.0) as u64),
                    w / 2.0,
                    y
                ));
            }
            5 => {
                let Some(row) = (0..=rows.saturating_sub(span)).find(|r| (*r..*r + span).all(|k| top_free[k] <= t)) else { continue };
                top_free[row..row + span].fill(t + fixed_secs);
                let y = row as f64 * row_h;
                events.push_str(&format!(
                    "Dialogue: 1,{},{},Danmaku,,0,0,0,,{{\\an8\\pos({:.0},{:.0}){style}}}{text}\n",
                    fmt_ass_time((t * 100.0) as u64),
                    fmt_ass_time(((t + fixed_secs) * 100.0) as u64),
                    w / 2.0,
                    y
                ));
            }
            _ => continue,
        }
        kept += 1;
    }
    if kept == 0 {
        return Err(AppError::not_found("这个视频没有可显示的弹幕。"));
    }
    let font = if font.trim().is_empty() { "Microsoft YaHei" } else { font.trim() };
    // ASS 的透明度是反的：00 不透明，FF 全透明
    let alpha = ((1.0 - layout.opacity.clamp(0.0, 1.0)) * 255.0).round() as u8;
    Ok(format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {}\nPlayResY: {}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Danmaku,{font},{},&H{alpha:02X}FFFFFF,&H{alpha:02X}FFFFFF,&H{alpha:02X}000000,&H64000000,0,0,0,0,100,100,0,0,1,1.8,0,7,0,0,0,1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n{events}",
        layout.width,
        layout.height,
        base_fs.round() as u32
    ))
}

// ---------- 语言 ----------

/// 统一写法：小写、下划线换连字符、去掉 B站 AI 字幕的 `ai-` 前缀和 YouTube 的 `-orig` 后缀。
pub fn normalize_lang(code: &str) -> String {
    let c = code.trim().to_ascii_lowercase().replace('_', "-");
    let c = c.strip_prefix("ai-").unwrap_or(&c);
    c.strip_suffix("-orig").unwrap_or(c).to_string()
}

/// `code` 是否属于偏好语言 `pref`：相同，或是它的变体（`zh` 匹配 `zh-hans`、`zh-cn`、`zh-tw`）。
pub fn lang_matches(code: &str, pref: &str) -> bool {
    let (c, p) = (normalize_lang(code), normalize_lang(pref));
    !p.is_empty() && (c == p || c.starts_with(&format!("{p}-")))
}

/// 在偏好列表里的位置（越小越优先）；不在列表里返回 None。
pub fn lang_rank(code: &str, prefs: &[String]) -> Option<usize> {
    prefs.iter().position(|p| lang_matches(code, p))
}

/// 常见语言的名称（用于显示）。
pub fn lang_name(code: &str) -> String {
    let n = normalize_lang(code);
    let name = match n.as_str() {
        "zh" | "zh-cn" | "zh-hans" | "zh-sg" => "中文（简体）",
        "zh-tw" | "zh-hant" | "zh-hk" => "中文（繁体）",
        "en" | "en-us" | "en-gb" => "英语",
        "ja" => "日语",
        "ko" => "韩语",
        "fr" => "法语",
        "de" => "德语",
        "es" => "西班牙语",
        "pt" | "pt-br" => "葡萄牙语",
        "ru" => "俄语",
        "it" => "意大利语",
        "ar" => "阿拉伯语",
        "th" => "泰语",
        "vi" => "越南语",
        "id" => "印尼语",
        "hi" => "印地语",
        "tr" => "土耳其语",
        _ => return code.to_string(),
    };
    name.to_string()
}

/// MP4 / MKV 字幕轨的语言标记需要 ISO 639-2 三字母代码。
pub fn iso639_2(code: &str) -> &'static str {
    let n = normalize_lang(code);
    match n.split('-').next().unwrap_or("") {
        "zh" => "chi",
        "en" => "eng",
        "ja" => "jpn",
        "ko" => "kor",
        "fr" => "fra",
        "de" => "deu",
        "es" => "spa",
        "pt" => "por",
        "ru" => "rus",
        "it" => "ita",
        "ar" => "ara",
        "th" => "tha",
        "vi" => "vie",
        "id" => "ind",
        "hi" => "hin",
        "tr" => "tur",
        _ => "und",
    }
}

/// 下载下来的字幕文件内容转换为目标格式；返回新内容。`to` 为 `srt` 或 `ass`。
/// 已经是目标格式、或没有可做的转换时返回 None。
pub fn convert(ext: &str, to: &str, content: &str, layout: DanmakuLayout, clip: Option<(u64, Option<u64>)>) -> AppResult<Option<String>> {
    convert_with_font(ext, to, content, layout, clip, "")
}

pub fn convert_with_font(ext: &str, to: &str, content: &str, layout: DanmakuLayout, clip: Option<(u64, Option<u64>)>, font: &str) -> AppResult<Option<String>> {
    match (ext, to) {
        ("vtt", "srt") => Ok(Some(vtt_to_srt(content))),
        ("json", "srt") => bilibili_json_to_srt(content).map(Some),
        ("xml", "ass") => danmaku_xml_to_ass_font(content, layout, clip, font).map(Some),
        _ => Ok(None),
    }
}

/// 把 SRT 内容裁剪到时间段内（其他格式原样返回）。
pub fn clip_srt(content: &str, clip: (u64, Option<u64>)) -> String {
    format_srt(&cut(&parse_srt(content), clip.0, clip.1))
}

/// 转换后的扩展名。
pub fn target_ext(ext: &str, convert_subs: bool, danmaku_ass: bool) -> String {
    match ext {
        "vtt" if convert_subs => "srt",
        "json" => "srt",
        "xml" if danmaku_ass => "ass",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ass_dialogue_is_parsed() {
        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.50,0:00:03.00,Default,,0,0,0,,{\\an8}你好,世界\\N第二行\nComment: 0,0:00:05.00,0:00:06.00,Default,,0,0,0,,x\n";
        let c = super::parse_ass(ass);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].start, c[0].end), (1500, 3000));
        assert_eq!(c[0].lines, vec!["你好,世界".to_string(), "第二行".to_string()]);
    }

    #[test]
    fn ass_drawing_commands_are_not_text() {
        let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\p1}m 0 0 l 100 0 100 100{\\p0}\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,{\\pos(10,10)}路径 p1\n";
        let c = super::parse_ass(ass);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].lines, vec!["路径 p1".to_string()]);
    }

    use super::*;

    const ROLLING: &str = "WEBVTT\nKind: captions\nLanguage: en\n\n\
00:00:01.000 --> 00:00:03.000 align:start position:0%\nhello<00:00:01.500><c> world</c>\n\n\
00:00:03.000 --> 00:00:03.010 align:start position:0%\nhello world\n\n\
00:00:03.010 --> 00:00:05.000 align:start position:0%\nhello world\nthis<00:00:03.500><c> is</c><00:00:04.000><c> next</c>\n\n\
00:00:05.000 --> 00:00:05.010 align:start position:0%\nthis is next\n\n\
00:00:05.010 --> 00:00:07.000 align:start position:0%\nthis is next\nand the end\n";

    #[test]
    fn danmaku_style_changes_font_speed_and_opacity() {
        let xml = r#"<i><d p="0,1,25,16777215,0,0,0,0">hello</d></i>"#;
        let style = crate::settings::DanmakuStyle { font_size: 60, opacity: 50, scroll_secs: 14, area: 50, font: "Noto Sans CJK SC".into() };
        let ass = danmaku_xml_to_ass_font(xml, DanmakuLayout::default().styled(&style), None, &style.font).unwrap();
        assert!(ass.contains("Style: Danmaku,Noto Sans CJK SC,60,&H80FFFFFF"), "{ass}");
        let slow = ass.lines().find(|l| l.starts_with("Dialogue")).unwrap().to_string();
        let fast = danmaku_xml_to_ass(xml, DanmakuLayout::default(), None).unwrap();
        let fast = fast.lines().find(|l| l.starts_with("Dialogue")).unwrap().to_string();
        // 滚动更慢，所以显示时间更长
        let end = |l: &str| l.split(',').nth(2).unwrap().to_string();
        assert_ne!(end(&slow), end(&fast));
        let mut s = crate::settings::DanmakuStyle { font_size: 1, opacity: 0, scroll_secs: 0, area: 0, font: "a,b\n".into() };
        s.normalize();
        assert_eq!((s.font_size, s.opacity, s.scroll_secs, s.area, s.font.as_str()), (18, 20, 3, 10, "ab"));
    }

    #[test]
    fn rolling_auto_captions_are_deduplicated() {
        let srt = vtt_to_srt(ROLLING);
        let cues = parse_srt(&srt);
        let texts: Vec<String> = cues.iter().map(|c| c.lines.join(" ")).collect();
        assert_eq!(texts, vec!["hello world", "this is next", "and the end"]);
        assert_eq!((cues[0].start, cues[0].end), (1000, 3010), "filler cue time is merged");
        assert_eq!(cues[1].start, 3010);
        assert!(cues[0].end <= cues[1].start, "no overlap");
        assert!(srt.starts_with("1\n00:00:01,000 --> 00:00:03,010\nhello world\n"));
    }

    #[test]
    fn manual_vtt_keeps_repeated_lines() {
        let vtt = "WEBVTT\n\n00:01.000 --> 00:02.000\nYes.\n\n00:02.500 --> 00:03.000\nYes.\n\n00:03.000 --> 00:04.000 line:90%\n&lt;b&gt;ok&amp;go\n";
        let cues = parse_srt(&vtt_to_srt(vtt));
        assert_eq!(cues.len(), 3, "without rolling markers nothing is merged");
        assert_eq!(cues[0].start, 1000);
        assert_eq!(cues[2].lines, vec!["<b>ok&go"]);
    }

    #[test]
    fn srt_roundtrip_and_cut() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nA\n\n2\n00:00:05,000 --> 00:00:08,000\nB\nline2\n\n3\n00:00:10,000 --> 00:00:12,000\nC\n\n";
        let cues = parse_srt(srt);
        assert_eq!(cues.len(), 3);
        assert_eq!(format_srt(&cues), srt);
        let cut = cut(&cues, 4000, Some(9000));
        assert_eq!(cut.len(), 1);
        assert_eq!((cut[0].start, cut[0].end), (1000, 4000));
        let tail = super::cut(&cues, 2000, None);
        assert_eq!(tail.len(), 3);
        assert_eq!((tail[0].start, tail[0].end), (0, 1000), "cue that began before the cut is clipped");
    }

    #[test]
    fn bilibili_cc() {
        let json = r#"{"font_size":0.4,"body":[{"from":0.5,"to":2.25,"content":"你好"},{"from":3,"to":4,"content":"第一行\n第二行"},{"from":5,"to":6,"content":"  "}]}"#;
        let srt = bilibili_json_to_srt(json).unwrap();
        assert!(srt.contains("00:00:00,500 --> 00:00:02,250\n你好"));
        assert!(srt.contains("第一行\n第二行"));
        assert_eq!(parse_srt(&srt).len(), 2);
        assert!(bilibili_json_to_srt("{\"body\":[]}").is_err());
        assert!(bilibili_json_to_srt("nope").is_err());
    }

    #[test]
    fn danmaku_layout() {
        let xml = r#"<?xml version="1.0"?><i><chatserver>x</chatserver>
<d p="1.0,1,25,16777215,1,0,abc,1">第一条</d>
<d p="1.2,1,25,16711680,1,0,abc,2">第二条 &amp; more</d>
<d p="1.5,5,25,255,1,0,abc,3">顶部</d>
<d p="2.0,4,25,16777215,1,0,abc,4">底部</d>
<d p="2.0,7,25,16777215,1,0,abc,5">[0,0,"1-1",1,"高级",0,0,0,0,1,0,0,0,0]</d>
<d p="bad">x</d></i>"#;
        let ass = danmaku_xml_to_ass(xml, DanmakuLayout::default(), None).unwrap();
        assert!(ass.contains("PlayResX: 1920"));
        let events: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue")).collect();
        assert_eq!(events.len(), 4, "mode 7 and malformed entries are skipped: {events:?}");
        // 两条滚动弹幕同时出现，必须在不同的行
        let y = |s: &str| s.split("\\move(").nth(1).unwrap().split(',').nth(1).unwrap().to_string();
        assert_ne!(y(events[0]), y(events[1]));
        assert!(events[1].contains("\\c&H0000FF&"), "red is BGR 0000FF: {}", events[1]);
        assert!(events[1].contains("第二条 & more"));
        assert!(events[2].contains("\\an8"));
        assert!(events[3].contains("\\an2"));
        assert!(danmaku_xml_to_ass("<i></i>", DanmakuLayout::default(), None).is_err());
    }

    #[test]
    fn scroll_lane_is_reused_after_it_clears() {
        let layout = DanmakuLayout { area: 0.0, ..DanmakuLayout::default() }; // 只有一行
        let xml = r#"<i><d p="0,1,25,16777215,1,0,a,1">短</d><d p="0.1,1,25,16777215,1,0,a,2">被丢弃</d><d p="9,1,25,16777215,1,0,a,3">后面的</d></i>"#;
        let ass = danmaku_xml_to_ass(xml, layout, None).unwrap();
        let n = ass.lines().filter(|l| l.starts_with("Dialogue")).count();
        assert_eq!(n, 2, "second is dropped while the lane is busy, third reuses it");
    }

    #[test]
    fn danmaku_and_srt_clip() {
        let xml = r#"<i><d p="1,1,25,16777215,1,0,a,1">早</d><d p="12.5,1,25,16777215,1,0,a,2">中间</d><d p="30,1,25,16777215,1,0,a,3">晚</d></i>"#;
        let ass = danmaku_xml_to_ass(xml, DanmakuLayout::default(), Some((10_000, Some(20_000)))).unwrap();
        let events: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue")).collect();
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("Dialogue: 0,0:00:02.50,"), "shifted to start at 2.5s: {}", events[0]);
        assert!(events[0].contains("中间"));
        assert!(danmaku_xml_to_ass(xml, DanmakuLayout::default(), Some((100_000, None))).is_err(), "nothing left after the cut");
        let srt = "1\n00:00:05,000 --> 00:00:08,000\nA\n\n2\n00:00:12,000 --> 00:00:14,000\nB\n\n";
        assert_eq!(clip_srt(srt, (10_000, None)), "1\n00:00:02,000 --> 00:00:04,000\nB\n\n");
    }

    #[test]
    fn videos_get_matching_layout() {
        assert_eq!(DanmakuLayout::for_video(Some(1080), Some(1920)).width, 608);
        assert_eq!(DanmakuLayout::for_video(None, None).width, 1920);
    }

    #[test]
    fn languages() {
        assert!(lang_matches("zh-Hans", "zh"));
        assert!(lang_matches("zh_CN", "zh-cn"));
        assert!(lang_matches("ai-zh", "zh"));
        assert!(lang_matches("en-orig", "en"));
        assert!(lang_matches("en-US", "en"));
        assert!(!lang_matches("zh", "zh-Hans"), "pref is more specific than the track");
        assert!(!lang_matches("eng", "en"));
        assert!(!lang_matches("en", ""));
        let prefs = vec!["zh".to_string(), "en".to_string()];
        assert_eq!(lang_rank("en-US", &prefs), Some(1));
        assert_eq!(lang_rank("zh-Hant", &prefs), Some(0));
        assert_eq!(lang_rank("ja", &prefs), None);
        assert_eq!(iso639_2("zh-Hans"), "chi");
        assert_eq!(iso639_2("xx"), "und");
        assert_eq!(lang_name("zh-TW"), "中文（繁体）");
        assert_eq!(lang_name("sw"), "sw");
    }

    #[test]
    fn conversions_and_target_ext() {
        assert!(convert("srt", "srt", "x", DanmakuLayout::default(), None).unwrap().is_none());
        assert!(convert("vtt", "srt", "WEBVTT\n\n00:01.000 --> 00:02.000\nhi\n", DanmakuLayout::default(), None).unwrap().unwrap().contains("hi"));
        assert_eq!(target_ext("vtt", true, true), "srt");
        assert_eq!(target_ext("vtt", false, true), "vtt");
        assert_eq!(target_ext("json", false, true), "srt");
        assert_eq!(target_ext("xml", true, false), "xml");
        assert_eq!(target_ext("xml", true, true), "ass");
        assert_eq!(target_ext("ass", true, true), "ass");
    }
}
