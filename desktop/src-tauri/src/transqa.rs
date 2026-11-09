//! 译后检查：翻完一段后做几项快速检查，只提示、不阻断、不重翻（问题记在阅读器里，由人决定）。
//! 检查：格式标记有没有丢、数字（含万 / 亿 / million / billion 换算）有没有变、有没有没翻、长度比是不是离谱、术语库里的词有没有按库译。

use std::sync::LazyLock;

use regex::Regex;

use crate::fmtmark::{has_marks, parse, strip};

static QTY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\d[\d,]*(?:\.\d+)?)\s*(thousand|million|billion|trillion|bn|k|m|万亿|百万|千万|万|亿|千|兆)?").unwrap());

fn unit_mult(u: &str) -> f64 {
    match u.to_lowercase().as_str() {
        "thousand" | "k" | "千" => 1e3,
        "million" | "m" | "百万" => 1e6,
        "billion" | "bn" => 1e9,
        "trillion" | "兆" => 1e12,
        "万" => 1e4,
        "亿" => 1e8,
        "千万" => 1e7,
        "万亿" => 1e12,
        _ => 1.0,
    }
}

/// 文字里的数量（数字 × 单位）。单个数字（没有单位）不算：它常常被写成“一”“二”。
pub fn quantities(text: &str) -> Vec<f64> {
    QTY.captures_iter(text)
        .filter_map(|c| {
            let raw = c[1].replace(',', "");
            let n: f64 = raw.parse().ok()?;
            let mult = c.get(2).map_or(1.0, |u| unit_mult(u.as_str()));
            (mult > 1.0 || raw.trim_start_matches('0').chars().filter(char::is_ascii_digit).count() >= 2 || raw.contains('.')).then_some(n * mult)
        })
        .collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= a.abs().max(b.abs()) * 0.005 + 1e-9
}

/// 宽度：中日韩字符算 2。
fn width(s: &str) -> usize {
    s.chars().map(|c| if crate::pdfx::is_wide(c) { 2 } else { 1 }).sum()
}

fn word_in(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.to_lowercase(), needle.to_lowercase());
    if n.chars().any(crate::pdfx::is_wide) {
        return h.contains(&n);
    }
    let mut from = 0;
    while let Some(i) = h[from..].find(&n) {
        let s = from + i;
        let e = s + n.len();
        let before = h[..s].chars().next_back().map_or(true, |c| !c.is_alphanumeric());
        let after = h[e..].chars().next().map_or(true, |c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = e;
    }
    false
}

/// 检查一段的译文，返回要提示的问题（空 = 没问题）。`terms` 是 (原文术语, 译名)。
pub fn check(src: &str, trans: &str, terms: &[(String, String)]) -> Vec<String> {
    let mut out = vec![];
    let src_plain = strip(src);
    let trans_plain = strip(trans);
    let (s, t) = (src_plain.trim(), trans_plain.trim());
    if t.is_empty() {
        return out;
    }
    if has_marks(src) {
        let n = src.matches("<r").count();
        if parse(trans, n).is_none() {
            out.push("译文里的格式标记丢了或乱了，这一段已按第一种格式输出（粗体、斜体等可能变成一样）".into());
        }
    }
    if s == t && s.chars().filter(|c| c.is_alphabetic()).count() >= 6 && !crate::filetr_ext::untranslatable(s) {
        out.push("译文和原文一样，可能没翻".into());
    }
    let sq = quantities(s);
    if !sq.is_empty() {
        let tq = quantities(t);
        let missing: Vec<String> = sq.iter().filter(|a| !tq.iter().any(|b| near(**a, *b))).map(|a| fmt_num(*a)).collect();
        if !missing.is_empty() {
            out.push(format!("数字可能不一致：原文有 {}，译文里没找到", missing.join("、")));
        }
    }
    let (ws, wt) = (width(s), width(t));
    if ws >= 16 {
        let r = wt as f64 / ws as f64;
        if r < 0.25 {
            out.push("译文比原文短太多，可能漏译".into());
        } else if r > 4.0 {
            out.push("译文比原文长太多，可能重复或多译".into());
        }
    }
    for (a, b) in terms {
        if !a.trim().is_empty() && !b.trim().is_empty() && word_in(s, a) && !word_in(t, b) {
            out.push(format!("术语“{a}”没有按术语库译成“{b}”"));
        }
    }
    out
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantities_convert_units_so_billion_matches_yi() {
        assert_eq!(quantities("revenue of 322 billion dollars"), vec![322e9]);
        assert!(quantities("3,220亿美元").iter().any(|v| near(*v, 322e9)));
        assert!(quantities("1,200 units").iter().any(|v| near(*v, 1200.0)));
        assert!(quantities("买了 3 个").is_empty()); // 单个数字不检查
        assert!(quantities("3.5 个").contains(&3.5));
    }

    #[test]
    fn checks_flag_real_problems_and_stay_quiet_otherwise() {
        assert!(check("Revenue grew to 322 billion in 2024.", "收入增长到 3,220 亿，2024 年。", &[]).is_empty());
        let m = check("Revenue grew to 322 billion in 2024.", "收入增长到 322 万，2024 年。", &[]);
        assert!(m.iter().any(|x| x.contains("数字")), "{m:?}");
        assert!(check("This is a perfectly normal English sentence.", "This is a perfectly normal English sentence.", &[]).iter().any(|x| x.contains("没翻")));
        assert!(check("Hello", "Hello", &[]).is_empty()); // 太短不判断
        let long = "This sentence is long enough to measure the length ratio between source and translation.";
        assert!(check(long, "好。", &[]).iter().any(|x| x.contains("太多") || x.contains("太短")));
        let terms = vec![("neural network".to_string(), "神经网络".to_string())];
        assert!(check("A neural network learns.", "一个神经元网路在学习。", &terms).iter().any(|x| x.contains("术语")));
        assert!(check("A neural network learns.", "神经网络会学习。", &terms).is_empty());
        // 格式标记丢了
        let m = check("<r1>Warning: </r1><r2>hot surface</r2>", "警告：表面很烫", &[]);
        assert!(m.iter().any(|x| x.contains("格式标记")), "{m:?}");
        assert!(check("<r1>Warning: </r1><r2>hot surface</r2>", "<r1>警告：</r1><r2>表面很烫</r2>", &[]).is_empty());
    }
}
