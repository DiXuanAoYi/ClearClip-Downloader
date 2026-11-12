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

/// 读取日志文件末尾若干行（已脱敏）。
pub fn log_tail(log_dir: &Path, max_lines: usize) -> String {
    let Ok(entries) = std::fs::read_dir(log_dir) else { return String::from("(没有日志文件)") };
    let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "log")).collect();
    files.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    let Some(latest) = files.last() else { return String::from("(没有日志文件)") };
    let text = std::fs::read_to_string(latest).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    redact(&lines[start..].join("\n"))
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

    /// tests/fixtures/real/ 里的真实响应样本：不能带 Cookie、令牌、手机号、邮箱，JSON 样本要能解析。
    #[test]
    fn real_fixtures_are_clean() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real");
        let phone = Regex::new(r"(?:^|[^0-9])1[3-9][0-9]{9}(?:$|[^0-9])").unwrap();
        let email = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = e.path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let ext = path.extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if !matches!(ext.as_str(), "json" | "html") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            assert_eq!(redact(&text), text, "{name} 里还有 Cookie 或令牌");
            assert!(!phone.is_match(&text), "{name} 里有手机号");
            let found: Vec<_> = email.find_iter(&text).map(|m| m.as_str()).filter(|m| *m != "user@example.com").collect();
            assert!(found.is_empty(), "{name} 里有邮箱 {found:?}");
            if ext == "json" && !name.ends_with(".meta.json") {
                serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|e| panic!("{name} 不是合法的 JSON：{e}"));
            }
        }
    }
}
