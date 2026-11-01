//! Supadata 在线字幕通道（https://supadata.ai）：给一个视频网址，直接返回带时间戳的字幕，
//! 比自己用 yt-dlp 抓快得多，也不容易被 YouTube 限流。有免费额度，密钥保存在保险箱 `ai.supadata`。
//!
//! 只取视频**已有**的字幕（`mode=native`），没有字幕时返回 `None`，由调用方继续走 yt-dlp / 音频转写。

use std::time::Duration;

use serde_json::Value;

use crate::error::{AppError, AppResult, ErrorKind};
use crate::subtitle::Cue;

pub const API: &str = "https://api.supadata.ai/v1";
/// 申请密钥的页面
pub const KEY_URL: &str = "https://dash.supadata.ai/";

/// 把接口返回的 `content` 数组（`text` / `offset` / `duration`，单位毫秒）转成字幕条目。
pub fn parse_transcript(v: &Value) -> Option<(String, Vec<Cue>)> {
    let items = v.get("content")?.as_array()?;
    let mut cues = Vec::with_capacity(items.len());
    for it in items {
        let text = it.get("text").and_then(Value::as_str).map(|t| t.replace('\n', " ").trim().to_string()).unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let start = it.get("offset").and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64;
        let dur = it.get("duration").and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64;
        cues.push(Cue { start, end: start + dur.max(500), lines: vec![text] });
    }
    // 接口给的是“开始 + 时长”，相邻两条可能有重叠，把前一条截到下一条开始
    for i in 0..cues.len().saturating_sub(1) {
        let next = cues[i + 1].start;
        if cues[i].end > next && next > cues[i].start {
            cues[i].end = next;
        }
    }
    if cues.is_empty() {
        return None;
    }
    let lang = v.get("lang").and_then(Value::as_str).unwrap_or("").to_string();
    Some((lang, cues))
}

fn status_error(status: u16, body: &str) -> AppError {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").or_else(|| v.get("error")).and_then(|m| m.as_str().map(str::to_string)))
        .unwrap_or_else(|| body.chars().take(160).collect());
    match status {
        401 | 403 => AppError::new(ErrorKind::NeedLogin, format!("Supadata 密钥无效（{status}）：{msg}")),
        402 => AppError::new(ErrorKind::RateLimited, format!("Supadata 额度用完了（402）：{msg}")),
        429 => AppError::new(ErrorKind::RateLimited, format!("Supadata 请求太频繁（429）：{msg}")),
        s if s >= 500 => AppError::new(ErrorKind::Network, format!("Supadata 暂时不可用（{s}）：{msg}")),
        s => AppError::msg(format!("Supadata 返回 {s}：{msg}")),
    }
}

/// 取视频已有的字幕。没有字幕（接口说不可用 / 404 / 206）返回 `Ok(None)`。
pub async fn fetch(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    url: &str,
    lang: &str,
    canceled: impl Fn() -> bool,
) -> AppResult<Option<(String, Vec<Cue>)>> {
    let mut q = vec![("url", url.to_string()), ("text", "false".into()), ("mode", "native".into())];
    if !lang.is_empty() {
        q.push(("lang", lang.to_string()));
    }
    let net = |e: reqwest::Error| AppError::new(ErrorKind::Network, format!("无法连接 Supadata：{e}"));
    let resp = client.get(format!("{base}/transcript")).header("x-api-key", key).query(&q).timeout(Duration::from_secs(40)).send().await.map_err(net)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(net)?;
    match status {
        200 => Ok(serde_json::from_str::<Value>(&text).ok().and_then(|v| parse_transcript(&v))),
        // 长视频走异步任务：返回 jobId，轮询到完成
        202 => {
            let job = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("jobId").and_then(Value::as_str).map(str::to_string));
            let Some(job) = job else { return Ok(None) };
            for _ in 0..30 {
                if canceled() {
                    return Err(AppError::msg("canceled"));
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                let r = client.get(format!("{base}/transcript/{job}")).header("x-api-key", key).timeout(Duration::from_secs(30)).send().await.map_err(net)?;
                let st = r.status().as_u16();
                let body = r.text().await.map_err(net)?;
                if st != 200 {
                    return if st == 404 { Ok(None) } else { Err(status_error(st, &body)) };
                }
                let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                match v.get("status").and_then(Value::as_str) {
                    Some("completed") => return Ok(parse_transcript(&v)),
                    Some("failed") => return Ok(None),
                    _ => {}
                }
            }
            Err(AppError::new(ErrorKind::Network, "Supadata 处理太久，改用其他方式取字幕。"))
        }
        // 视频没有字幕
        206 | 404 => Ok(None),
        400 if text.contains("unavailable") || text.contains("not-found") || text.contains("transcript") => Ok(None),
        s => Err(status_error(s, &text)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_timed_segments_and_trims_overlaps() {
        let v = json!({"lang": "en", "content": [
            {"text": "Hello\nworld", "offset": 0, "duration": 4000, "lang": "en"},
            {"text": "  ", "offset": 3000, "duration": 1000},
            {"text": "Second line", "offset": 2500, "duration": 2000},
        ]});
        let (lang, cues) = parse_transcript(&v).unwrap();
        assert_eq!(lang, "en");
        assert_eq!(cues.len(), 2, "空白条目被丢掉");
        assert_eq!(cues[0].lines, vec!["Hello world"]);
        assert_eq!((cues[0].start, cues[0].end), (0, 2500), "与下一条重叠的部分被截掉");
        assert_eq!((cues[1].start, cues[1].end), (2500, 4500));
    }

    #[test]
    fn plain_text_or_empty_content_is_not_a_transcript() {
        assert!(parse_transcript(&json!({"content": "just text"})).is_none());
        assert!(parse_transcript(&json!({"content": []})).is_none());
        assert!(parse_transcript(&json!({})).is_none());
    }

    #[test]
    fn quota_and_key_errors_are_classified() {
        assert_eq!(status_error(402, "{}").kind, ErrorKind::RateLimited);
        assert_eq!(status_error(401, "{\"message\":\"bad\"}").kind, ErrorKind::NeedLogin);
        assert_eq!(status_error(503, "x").kind, ErrorKind::Network);
    }
}
