//! 逐字高亮的文字 → ASS 字幕文件，由 ffmpeg 的 `ass` 滤镜烧进画面。
//! `drawtext` 一条文字只能有一种颜色，而逐字高亮要“说到哪个词，哪个词变色”，所以这类文字改用 libass 来画。

use super::spec::TextItem;

fn ass_time(ms: u64) -> String {
    let cs = ms / 10;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

/// `#RRGGBB` → ASS 的 `&HBBGGRR&`
fn ass_color(hex: &str) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 {
        return "&HFFFFFF&".into();
    }
    format!("&H{}{}{}&", &h[4..6], &h[2..4], &h[0..2])
}

/// ASS 里的透明度：`00` 不透明，`FF` 全透明
fn ass_alpha(opacity: f64) -> String {
    format!("&H{:02X}&", ((1.0 - opacity.clamp(0.0, 1.0)) * 255.0).round() as u32)
}

fn escape(t: &str) -> String {
    t.replace('\\', "＼").replace('{', "｛").replace('}', "｝").replace("\r\n", "\\N").replace(['\n', '\r'], "\\N")
}

/// 一条文字的 `Dialogue` 行。`family` 是字体的家族名（找不到时用 libass 的默认字体）。
fn dialogue(t: &TextItem, w: u32, h: u32, family: Option<&str>) -> String {
    let px = (f64::from(h) * t.size / 100.0).round().max(8.0) as u32;
    let mut tags = format!(
        "\\an5\\pos({},{})\\q2\\fs{px}\\1c{}\\2c{}\\1a{a}\\2a{a}",
        (f64::from(w) * t.x).round(),
        (f64::from(h) * t.y).round(),
        ass_color(&t.highlight),
        ass_color(&t.color),
        a = ass_alpha(t.opacity)
    );
    if let Some(f) = family {
        tags.push_str(&format!("\\fn{f}"));
    }
    if t.boxed {
        tags.push_str(&format!("\\bord{}\\3c{}\\3a{}\\shad0", px / 4, ass_color(&t.box_color), ass_alpha(t.box_opacity * t.opacity)));
    } else if t.outline {
        tags.push_str(&format!(
            "\\bord{}\\3c{}\\3a{}\\shad0",
            ((f64::from(px) * 0.06).round() as u32).max(2),
            ass_color(&t.outline_color),
            ass_alpha(t.opacity)
        ));
    } else {
        tags.push_str("\\bord0\\shad0");
    }
    if t.fade_ms > 0 {
        tags.push_str(&format!("\\fad({0},{0})", t.fade_ms));
    }
    // 卡拉 OK：\k 的单位是 1/100 秒，从这一个词开始说，到下一个词开始说为止
    let mut body = String::new();
    let mut cursor = 0u64;
    for (i, wd) in t.words.iter().enumerate() {
        let start = wd.a.max(cursor);
        let next = t.words.get(i + 1).map_or(wd.b.max(start), |n| n.a.max(start));
        let gap = start.saturating_sub(cursor);
        if gap >= 10 {
            body.push_str(&format!("{{\\k{}}}", gap / 10));
            cursor = start;
        }
        let dur = next.saturating_sub(cursor).max(1);
        body.push_str(&format!("{{\\k{}}}{}", (dur / 10).max(1), escape(&wd.t)));
        cursor += dur / 10 * 10;
    }
    format!("Dialogue: 0,{},{},Default,,0,0,0,,{{{tags}}}{body}", ass_time(t.start_ms), ass_time(t.end_ms))
}

/// 整份 ASS 文件。画布大小就是输出画面的大小，所以字号和位置按像素写。
pub fn document(items: &[&TextItem], w: u32, h: u32, family: Option<&str>) -> String {
    let mut s = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,{},40,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        family.unwrap_or("Arial")
    );
    for t in items {
        s.push_str(&dialogue(t, w, h, family));
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::spec::TextWord;

    fn item() -> TextItem {
        TextItem {
            text: "你好 world".into(),
            start_ms: 1000,
            end_ms: 3500,
            highlight: "#FFD400".into(),
            words: vec![
                TextWord { t: "你".into(), a: 100, b: 400 },
                TextWord { t: "好".into(), a: 400, b: 900 },
                TextWord { t: " world".into(), a: 1000, b: 2000 },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn times_and_colors_use_ass_notation() {
        assert_eq!(ass_time(3_723_450), "1:02:03.45");
        assert_eq!(ass_color("#FFD400"), "&H00D4FF&");
        assert_eq!(ass_alpha(1.0), "&H00&");
        assert_eq!(ass_alpha(0.0), "&HFF&");
    }

    #[test]
    fn words_become_consecutive_karaoke_durations() {
        let l = dialogue(&item(), 1080, 1920, Some("Noto Sans CJK SC"));
        // 先空 10 厘秒，再 你(30) 好(60：到下一个词开始) 最后一个词 100
        assert!(l.contains("{\\k10}{\\k30}你{\\k60}好{\\k100} world"), "{l}");
        assert!(l.starts_with("Dialogue: 0,0:00:01.00,0:00:03.50,Default"), "{l}");
        assert!(l.contains("\\pos(540,1632)") && l.contains("\\fnNoto Sans CJK SC") && l.contains("\\1c&H00D4FF&") && l.contains("\\2c&HFFFFFF&"), "{l}");
        assert!(l.contains("\\bord"), "{l}");
    }

    #[test]
    fn special_characters_cannot_break_the_override_block() {
        let mut t = item();
        t.words = vec![TextWord { t: "a{b}\nc".into(), a: 0, b: 500 }];
        let l = dialogue(&t, 1280, 720, None);
        assert!(l.contains("a｛b｝\\Nc"), "{l}");
        let doc = document(&[&t], 1280, 720, None);
        assert!(doc.contains("PlayResX: 1280") && doc.contains("Style: Default,Arial,"));
    }

    #[test]
    fn fade_and_box_are_written_when_asked() {
        let mut t = item();
        t.fade_ms = 200;
        t.boxed = true;
        let l = dialogue(&t, 1280, 720, None);
        assert!(l.contains("\\fad(200,200)") && l.contains("\\3a&H80&"), "{l}");
    }
}
