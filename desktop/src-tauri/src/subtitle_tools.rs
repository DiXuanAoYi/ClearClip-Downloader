//! 字幕小工具：时间轴平移 / 缩放、双语合并、清理标记、格式转换（SRT / VTT / ASS）。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::subtitle::{self, Cue};

/// 读取字幕文件（SRT / VTT / ASS）。
pub fn read(path: &Path) -> AppResult<Vec<Cue>> {
    let cues = crate::library::read_cues(path).ok_or_else(|| AppError::invalid("只支持 SRT、VTT、ASS 字幕文件。"))?;
    if cues.is_empty() {
        return Err(AppError::invalid("字幕文件里没有内容。"));
    }
    Ok(cues)
}

/// 整体平移（毫秒，可以为负）。移到 0 之前的部分被截掉，完全在 0 之前的整条丢弃。
pub fn shift(cues: &[Cue], offset_ms: i64) -> Vec<Cue> {
    cues.iter()
        .filter_map(|c| {
            let (s, e) = (c.start as i64 + offset_ms, c.end as i64 + offset_ms);
            (e > 0).then(|| Cue { start: s.max(0) as u64, end: e as u64, lines: c.lines.clone() })
        })
        .collect()
}

/// 按比例缩放时间轴（字幕和视频帧率不一致、越到后面越对不上时用）：`factor` = 新时长 / 旧时长。
pub fn rescale(cues: &[Cue], factor: f64) -> Vec<Cue> {
    let f = factor.clamp(0.5, 2.0);
    cues.iter().map(|c| Cue { start: (c.start as f64 * f).round() as u64, end: (c.end as f64 * f).round() as u64, lines: c.lines.clone() }).collect()
}

/// 用两个时间点校准：字幕里 `a_from` 处应该在 `a_to`，`b_from` 处应该在 `b_to`，求出线性变换并应用。
pub fn align(cues: &[Cue], a_from: u64, a_to: u64, b_from: u64, b_to: u64) -> AppResult<Vec<Cue>> {
    if b_from <= a_from || b_to <= a_to {
        return Err(AppError::invalid("第二个时间点必须晚于第一个。"));
    }
    let factor = (b_to - a_to) as f64 / (b_from - a_from) as f64;
    if !(0.5..=2.0).contains(&factor) {
        return Err(AppError::invalid("两个时间点差别太大，请检查输入。"));
    }
    let map = |t: u64| ((t as f64 - a_from as f64) * factor + a_to as f64).round() as i64;
    Ok(cues
        .iter()
        .filter_map(|c| {
            let (s, e) = (map(c.start), map(c.end));
            (e > 0).then(|| Cue { start: s.max(0) as u64, end: e as u64, lines: c.lines.clone() })
        })
        .collect())
}

/// 双语合并：以 `main` 为准，每条加上时间上重叠最多的 `second` 的文字（放在下面）。
pub fn merge_bilingual(main: &[Cue], second: &[Cue]) -> Vec<Cue> {
    main.iter()
        .map(|c| {
            let best = second
                .iter()
                .map(|o| (o, c.end.min(o.end) as i64 - c.start.max(o.start) as i64))
                .filter(|(_, overlap)| *overlap > 0)
                .max_by_key(|(_, overlap)| *overlap);
            let mut lines = c.lines.clone();
            if let Some((o, _)) = best {
                lines.extend(o.lines.iter().cloned());
            }
            Cue { start: c.start, end: c.end, lines }
        })
        .collect()
}

/// 清理：去掉 HTML 标记和 ASS 样式标记、`[音乐]` 这类听障说明、首尾空白，丢弃空条目。
pub fn clean(cues: &[Cue], drop_hearing: bool) -> Vec<Cue> {
    use std::sync::LazyLock;
    static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]*>|\{\\[^}]*\}").unwrap());
    static HEARING: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[\[\(（【][^\]\)）】]{1,30}[\]\)）】]").unwrap());
    cues.iter()
        .filter_map(|c| {
            let lines: Vec<String> = c
                .lines
                .iter()
                .map(|l| {
                    let t = TAGS.replace_all(l, "");
                    let t = if drop_hearing { HEARING.replace_all(&t, "").into_owned() } else { t.into_owned() };
                    t.split_whitespace().collect::<Vec<_>>().join(" ")
                })
                .filter(|l| !l.is_empty())
                .collect();
            (!lines.is_empty()).then_some(Cue { start: c.start, end: c.end, lines })
        })
        .collect()
}

fn fmt_vtt_time(ms: u64) -> String {
    format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn format_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for c in cues {
        out.push_str(&format!("{} --> {}\n{}\n\n", fmt_vtt_time(c.start), fmt_vtt_time(c.end), c.lines.join("\n")));
    }
    out
}

pub fn format_ass(cues: &[Cue], font: &str, size: u32) -> String {
    let font = if font.trim().is_empty() { "Microsoft YaHei" } else { font.trim() };
    let mut out = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,{font},{size},&H00FFFFFF,&H00FFFFFF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,0,2,60,60,50,1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    );
    for c in cues {
        let text = c.lines.iter().map(|l| l.replace('{', "｛").replace('}', "｝")).collect::<Vec<_>>().join("\\N");
        out.push_str(&format!("Dialogue: 0,{},{},Default,,0,0,0,,{text}\n", subtitle::fmt_ass_time(c.start / 10), subtitle::fmt_ass_time(c.end / 10)));
    }
    out
}

pub fn write_as(cues: &[Cue], fmt: &str) -> AppResult<String> {
    match fmt {
        "srt" => Ok(subtitle::format_srt(cues)),
        "vtt" => Ok(format_vtt(cues)),
        "ass" => Ok(format_ass(cues, "", 56)),
        _ => Err(AppError::invalid("输出格式只能是 SRT、VTT 或 ASS。")),
    }
}

/// 输出文件放在原文件旁边，名字加上说明，已存在时加序号。
pub fn output_path(input: &Path, tag: &str, ext: &str) -> PathBuf {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "subtitle".into());
    let p = input.with_file_name(format!("{stem}.{tag}.{ext}"));
    crate::naming::unique_path(p, &|p: &Path| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(s: u64, e: u64, t: &str) -> Cue {
        Cue { start: s, end: e, lines: vec![t.into()] }
    }

    #[test]
    fn shift_clamps_at_zero() {
        let v = vec![cue(500, 1500, "a"), cue(3000, 4000, "b")];
        let s = shift(&v, -1000);
        assert_eq!(s, vec![cue(0, 500, "a"), cue(2000, 3000, "b")]);
        assert_eq!(shift(&v, -1600).len(), 1, "a cue that ends before zero is dropped");
        assert_eq!(shift(&v, 2000)[0].start, 2500);
    }

    #[test]
    fn rescale_and_align() {
        let v = vec![cue(10_000, 12_000, "a"), cue(100_000, 102_000, "b")];
        let r = rescale(&v, 1.001);
        assert_eq!(r[1].start, 100_100);
        // 第一条应该在 12 s，最后一条应该在 104 s
        let a = align(&v, 10_000, 12_000, 100_000, 104_000).unwrap();
        assert_eq!((a[0].start, a[1].start), (12_000, 104_000));
        assert!(align(&v, 100, 100, 50, 60).is_err());
        assert!(align(&v, 0, 0, 10, 5000).is_err(), "absurd ratio");
    }

    #[test]
    fn bilingual_uses_largest_overlap() {
        let zh = vec![cue(1000, 3000, "你好"), cue(3500, 5000, "再见"), cue(9000, 9500, "没有对应")];
        let en = vec![cue(900, 3100, "Hello"), cue(3400, 4000, "Bye"), cue(4000, 5100, "bye bye")];
        let m = merge_bilingual(&zh, &en);
        assert_eq!(m[0].lines, vec!["你好", "Hello"]);
        assert_eq!(m[1].lines, vec!["再见", "bye bye"], "1000 ms overlap beats 500 ms");
        assert_eq!(m[2].lines, vec!["没有对应"]);
    }

    #[test]
    fn clean_strips_markup() {
        let v = vec![
            Cue { start: 0, end: 1000, lines: vec!["<i>hello</i>  world".into(), "{\\an8}top".into()] },
            cue(1000, 2000, "[Music]"),
            cue(2000, 3000, "（笑）好的"),
        ];
        let c = clean(&v, true);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].lines, vec!["hello world", "top"]);
        assert_eq!(c[1].lines, vec!["好的"]);
        assert_eq!(clean(&v, false).len(), 3, "hearing notes stay unless asked");
    }

    #[test]
    fn writers_roundtrip_through_parsers() {
        let v = vec![cue(1500, 3250, "第一行"), Cue { start: 61_000, end: 62_000, lines: vec!["a".into(), "b".into()] }];
        let srt = write_as(&v, "srt").unwrap();
        assert_eq!(subtitle::parse_srt(&srt), v);
        let vtt = write_as(&v, "vtt").unwrap();
        assert!(vtt.starts_with("WEBVTT"));
        assert_eq!(subtitle::parse_vtt(&vtt), v);
        let ass = write_as(&v, "ass").unwrap();
        assert!(ass.contains("Dialogue: 0,0:00:01.50,0:00:03.25,Default,,0,0,0,,第一行"), "{ass}");
        assert!(ass.contains("a\\Nb"));
        assert!(write_as(&v, "doc").is_err());
    }

    #[test]
    fn output_names_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("clearclip-st-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("课程.zh.srt");
        let first = output_path(&input, "shift", "srt");
        assert_eq!(first.file_name().unwrap(), "课程.zh.shift.srt");
        std::fs::write(&first, "x").unwrap();
        assert_ne!(output_path(&input, "shift", "srt"), first);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
