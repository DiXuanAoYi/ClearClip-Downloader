//! 诊断：脱敏、调试样本保存、“复制诊断信息”。

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

static SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(cookie|set-cookie|sessdata|bili_jct|sessionid|session_id|sid|hsid|ssid|sapisid|phpsessid|token|access_token|xsec_token|a1|web_session|sub|subp|authorization|password|passwd)(["']?\s*[:=]\s*["']?)([^;&\s"',}]+)"#,
    )
    .unwrap()
});

/// 把 Cookie、令牌等敏感值替换为 `***`。
pub fn redact(text: &str) -> String {
    SECRET_RE.replace_all(text, "$1$2***").into_owned()
}

/// 去掉地址里的查询参数（常含签名和令牌）。
pub fn strip_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

pub fn save_sample(dir: &Path, platform: &str, label: &str, url: &str, body: &str) {
    let dir = dir.join(platform);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S%.3f");
    let file = dir.join(format!("{ts}-{label}.txt"));
    let content = format!("URL: {}\nTIME: {}\n\n{}", strip_query(url), chrono::Utc::now().to_rfc3339(), redact(body));
    match std::fs::write(&file, content) {
        Ok(()) => log::info!("saved parser sample {}", file.display()),
        Err(e) => log::warn!("failed to save sample: {e}"),
    }
}

pub fn samples_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("samples")
}

/// 一行日志去掉开头“[日期][时间]”以后的部分：这部分相同就算同一条。
fn line_key(line: &str) -> &str {
    let mut rest = line;
    for _ in 0..2 {
        match rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((_, after)) => rest = after,
            None => return line,
        }
    }
    rest
}

/// 一行日志里的时间（第二个方括号里的内容）。
fn line_time(line: &str) -> Option<&str> {
    let (_, after_date) = line.strip_prefix('[')?.split_once(']')?;
    let (time, _) = after_date.strip_prefix('[')?.split_once(']')?;
    Some(time)
}

/// 把连续重复的日志行（只有时间不同）合成一行，后面写上又出现了几次、最后一次的时间。
/// 同一条警告每隔几秒写一次时，诊断信息里不会被它刷满。
pub fn fold_repeats(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let key = line_key(lines[i]);
        let mut j = i + 1;
        while j < lines.len() && line_key(lines[j]) == key {
            j += 1;
        }
        let n = j - i;
        if n == 1 {
            out.push(lines[i].to_string());
        } else {
            let last = line_time(lines[j - 1]).map(|t| format!("，最后一次 {t}")).unwrap_or_default();
            // 前面留一个空格：整份诊断信息最后还要再脱敏一遍，令牌的值遇到空格才算结束，说明不会被一起隐去
            out.push(format!("{} （又重复了 {} 次{last}）", lines[i], n - 1));
        }
        i = j;
    }
    out
}

/// 是不是警告或错误（tauri-plugin-log 的格式：`[日期][时间][模块][级别] 内容`）。
fn is_problem(line: &str) -> bool {
    line.contains("][WARN]") || line.contains("][ERROR]")
}

/// 读取最新的日志文件，连续重复的行合成一行，返回最后 `max_lines` 行（已脱敏）。
/// `only_problems` 为 true 时只保留警告和错误（“复制摘要”用）。
pub fn log_tail(log_dir: &Path, max_lines: usize, only_problems: bool) -> String {
    let Ok(entries) = std::fs::read_dir(log_dir) else { return String::from("(没有日志文件)") };
    let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "log")).collect();
    files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let Some(latest) = files.last() else { return String::from("(没有日志文件)") };
    // 先脱敏再合并：合并时加在行尾的说明不能被当成令牌的一部分隐去
    let text = redact(&std::fs::read_to_string(latest).unwrap_or_default());
    let lines: Vec<&str> = text.lines().filter(|l| !only_problems || is_problem(l)).collect();
    let folded = fold_repeats(&lines);
    let start = folded.len().saturating_sub(max_lines);
    if folded.is_empty() {
        return String::from(if only_problems { "(没有警告或错误)" } else { "(日志是空的)" });
    }
    folded[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_cookies_and_tokens() {
        let s = "Cookie: SESSDATA=abc123; bili_jct=zz\nurl?xsec_token=AB12&x=1 \"token\":\"secret\" sid=9";
        let r = redact(s);
        assert!(!r.contains("abc123") && !r.contains("zz") && !r.contains("AB12") && !r.contains("secret") && !r.contains("sid=9"), "{r}");
        assert!(r.contains("x=1"));
    }

    #[test]
    fn folds_repeated_lines() {
        let w = "[clearclip_lib::writebadge::win][WARN] 创建写作浮标失败：WebView2 error 0x80010108";
        let lines = [
            "[2026-11-11][15:05:34][clearclip_lib::tray][INFO] 启动".to_string(),
            format!("[2026-11-11][15:05:47]{w}"),
            format!("[2026-11-11][15:06:00]{w}"),
            format!("[2026-11-11][15:06:13]{w}"),
            "[2026-11-11][15:06:20][clearclip_lib::livetr][WARN] 屏幕翻译：已停止".to_string(),
            format!("[2026-11-11][15:07:00]{w}"),
            "不带时间的续行".to_string(),
            "不带时间的续行".to_string(),
        ];
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let out = fold_repeats(&refs);
        assert_eq!(out.len(), 5, "{out:#?}");
        assert_eq!(out[1], format!("[2026-11-11][15:05:47]{w} （又重复了 2 次，最后一次 15:06:13）"));
        assert_eq!(out[3], format!("[2026-11-11][15:07:00]{w}"), "中间隔了别的行，不合并");
        assert_eq!(out[4], "不带时间的续行 （又重复了 1 次）");
        assert!(fold_repeats(&[]).is_empty());
    }

    #[test]
    fn log_tail_folds_and_filters() {
        let dir = std::env::temp_dir().join(format!("clearclip-logs-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut text = String::from("[2026-11-11][15:00:00][a][INFO] 开始\n");
        for i in 0..50 {
            text.push_str(&format!("[2026-11-11][15:{:02}:00][b][WARN] 同一条 token=abc\n", i + 1));
        }
        text.push_str("[2026-11-11][16:00:00][c][ERROR] 出错了\n");
        std::fs::write(dir.join("clearclip.log"), text).unwrap();
        let all = log_tail(&dir, 200, false);
        assert_eq!(all.lines().count(), 3, "{all}");
        assert!(all.contains("又重复了 49 次，最后一次 15:50:00"), "{all}");
        assert!(all.contains("token=***") && !all.contains("abc"), "脱敏");
        assert_eq!(redact(&all), all, "整份诊断最后再脱敏一遍，合并的说明也不受影响");
        let problems = log_tail(&dir, 200, true);
        assert!(!problems.contains("开始") && problems.contains("出错了"), "{problems}");
        assert_eq!(log_tail(&dir, 1, false).lines().count(), 1, "只要最后几行");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(log_tail(&dir, 10, false), "(没有日志文件)");
    }

    #[test]
    fn strips_query() {
        assert_eq!(strip_query("https://a.com/x?sign=1#f"), "https://a.com/x");
    }

    #[test]
    fn saves_redacted_sample() {
        let dir = std::env::temp_dir().join(format!("clearclip-samples-{}", std::process::id()));
        save_sample(&dir, "douyin", "share-page", "https://www.iesdouyin.com/share/video/1/?sign=s", "token=abc body");
        let f = std::fs::read_dir(dir.join("douyin")).unwrap().next().unwrap().unwrap().path();
        let text = std::fs::read_to_string(f).unwrap();
        assert!(text.starts_with("URL: https://www.iesdouyin.com/share/video/1/\n"));
        assert!(text.contains("token=***") && !text.contains("sign=s"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
