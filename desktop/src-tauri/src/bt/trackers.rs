//! BT tracker 列表：定期从开源列表更新，保留上次成功的结果，另可手动添加自己的 tracker。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

/// 默认的列表来源（ngosang/trackerslist 的 best 列表）：先走 jsDelivr，再走 GitHub 原始地址。
pub const DEFAULT_SOURCES: [&str; 2] = [
    "https://cdn.jsdelivr.net/gh/ngosang/trackerslist@master/trackers_best.txt",
    "https://raw.githubusercontent.com/ngosang/trackerslist/master/trackers_best.txt",
];

/// 单个列表最多采用多少个 tracker（太多只会拖慢连接）
const MAX_TRACKERS: usize = 120;
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    /// 上次成功更新的时间（Unix 秒）
    pub updated_at: i64,
    pub source: String,
    pub trackers: Vec<String>,
}

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("bt_trackers.json")
}

pub fn load(data_dir: &Path) -> Option<Stored> {
    serde_json::from_slice(&std::fs::read(file(data_dir)).ok()?).ok()
}

fn save(data_dir: &Path, s: &Stored) {
    if let Ok(json) = serde_json::to_vec(s) {
        let tmp = data_dir.join("bt_trackers.json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, file(data_dir));
        }
    }
}

/// 一个 tracker 地址是否可用：只接受 http / https / udp，且有主机名。
pub fn valid_tracker(s: &str) -> Option<String> {
    let s = s.trim();
    let u = url::Url::parse(s).ok()?;
    if !matches!(u.scheme(), "http" | "https" | "udp") || u.host_str().unwrap_or("").is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// 解析列表文本：每行一个，也容忍逗号 / 空格分隔；去重，丢掉其他协议（ws、wss 等）。
pub fn parse_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for token in text.split(|c: char| c.is_whitespace() || c == ',') {
        if let Some(t) = valid_tracker(token) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        if out.len() >= MAX_TRACKERS {
            break;
        }
    }
    out
}

pub fn is_stale(stored: Option<&Stored>, now: i64, interval_days: u32) -> bool {
    match stored {
        None => true,
        Some(s) => now.saturating_sub(s.updated_at) >= interval_days.max(1) as i64 * 86_400,
    }
}

/// 逐个来源尝试，第一个成功的为准。全部失败时不改动已有的列表，返回最后的错误。
pub async fn update(client: &reqwest::Client, data_dir: &Path, sources: &[String]) -> Result<Stored, String> {
    let defaults: Vec<String> = DEFAULT_SOURCES.iter().map(|s| s.to_string()).collect();
    let sources = if sources.iter().all(|s| s.trim().is_empty()) { &defaults } else { sources };
    let mut last = "没有可用的来源".to_string();
    for src in sources.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if !(src.starts_with("http://") || src.starts_with("https://")) {
            last = format!("{src}：只支持 http / https 地址");
            continue;
        }
        match fetch(client, src).await {
            Ok(list) => {
                let stored = Stored { updated_at: chrono::Utc::now().timestamp(), source: src.to_string(), trackers: list };
                save(data_dir, &stored);
                return Ok(stored);
            }
            Err(e) => last = format!("{src}：{e}"),
        }
    }
    Err(last)
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<String>, String> {
    let r = client.get(url).timeout(Duration::from_secs(20)).send().await.map_err(|e| format!("无法连接（{e}）"))?;
    if !r.status().is_success() {
        return Err(format!("服务器返回 {}", r.status().as_u16()));
    }
    let body = r.bytes().await.map_err(|e| format!("下载中断（{e}）"))?;
    if body.len() > MAX_BODY {
        return Err("列表太大".into());
    }
    let list = parse_list(&String::from_utf8_lossy(&body));
    if list.is_empty() {
        return Err("里面没有可用的 tracker".into());
    }
    Ok(list)
}

/// 到期（或还没有列表）且开启了自动更新时，更新一次。失败只记日志，沿用旧列表。
pub async fn refresh_if_due(client: &reqwest::Client, data_dir: &Path, settings: &Settings) {
    if !settings.bt_tracker_auto_update {
        return;
    }
    let stored = load(data_dir);
    if !is_stale(stored.as_ref(), chrono::Utc::now().timestamp(), settings.bt_tracker_interval_days) {
        return;
    }
    if let Err(e) = update(client, data_dir, &settings.bt_tracker_sources).await {
        log::warn!("tracker list update failed: {e}");
    }
}

/// 实际使用的 tracker：自己添加的在前，在线列表的在后，去重。
pub fn effective(data_dir: &Path, settings: &Settings) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let listed = load(data_dir).map(|s| s.trackers).unwrap_or_default();
    for t in settings.bt_custom_trackers.iter().filter_map(|t| valid_tracker(t)).chain(listed) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-trackers-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 按路径返回固定内容的 HTTP 服务器
    async fn server(routes: Vec<(&'static str, u16, &'static str)>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let (code, body) = routes.iter().find(|r| r.0 == path).map(|r| (r.1, r.2)).unwrap_or((404, ""));
                    let _ = s.write_all(format!("HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                });
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    #[test]
    fn parses_and_filters() {
        let text = "udp://tracker.opentrackr.org:1337/announce\n\nhttps://a.example/announce\nws://x.example/announce\nudp://tracker.opentrackr.org:1337/announce\nnot a url\nhttp://b.example:6969/announce, udp://c.example:80\nmagnet:?xt=urn:btih:abc\nudp://";
        assert_eq!(
            parse_list(text),
            vec!["udp://tracker.opentrackr.org:1337/announce", "https://a.example/announce", "http://b.example:6969/announce", "udp://c.example:80"]
        );
        assert!(parse_list("").is_empty());
        let many: String = (0..300).map(|i| format!("udp://t{i}.example:1\n")).collect();
        assert_eq!(parse_list(&many).len(), MAX_TRACKERS);
    }

    #[test]
    fn staleness() {
        let s = Stored { updated_at: 1_000_000, source: String::new(), trackers: vec![] };
        assert!(is_stale(None, 0, 7));
        assert!(!is_stale(Some(&s), 1_000_000 + 6 * 86_400, 7));
        assert!(is_stale(Some(&s), 1_000_000 + 7 * 86_400, 7));
        assert!(is_stale(Some(&s), 1_000_000 + 86_400, 0), "间隔至少一天");
    }

    #[tokio::test]
    async fn falls_back_to_the_next_source_and_persists() {
        let base =
            server(vec![("/bad", 500, ""), ("/empty", 200, "ws://x\n"), ("/good", 200, "udp://a.example:1/announce\nhttps://b.example/announce\n")]).await;
        let dir = tmp("fallback");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let got = update(&client, &dir, &[format!("{base}/bad"), format!("{base}/empty"), format!("{base}/good")]).await.unwrap();
        assert_eq!(got.trackers.len(), 2);
        assert_eq!(got.source, format!("{base}/good"));
        assert_eq!(load(&dir).unwrap(), got);
    }

    #[tokio::test]
    async fn failure_keeps_the_previous_list() {
        let base = server(vec![("/bad", 503, "")]).await;
        let dir = tmp("keep");
        let old = Stored { updated_at: 5, source: "old".into(), trackers: vec!["udp://old.example:1".into()] };
        save(&dir, &old);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let err = update(&client, &dir, &[format!("{base}/bad"), "ftp://nope".into()]).await.unwrap_err();
        assert!(err.contains("只支持 http"), "{err}");
        assert_eq!(load(&dir).unwrap(), old);
    }

    #[tokio::test]
    async fn auto_update_respects_the_setting_and_the_interval() {
        let base = server(vec![("/l", 200, "udp://new.example:1\n")]).await;
        let dir = tmp("due");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut s = Settings { bt_tracker_sources: vec![format!("{base}/l")], bt_tracker_auto_update: false, ..Default::default() };
        refresh_if_due(&client, &dir, &s).await;
        assert!(load(&dir).is_none(), "关闭自动更新时不联网");
        s.bt_tracker_auto_update = true;
        refresh_if_due(&client, &dir, &s).await;
        let first = load(&dir).unwrap();
        assert_eq!(first.trackers, vec!["udp://new.example:1"]);
        // 没到期：不再更新
        save(&dir, &Stored { source: "marker".into(), ..first.clone() });
        refresh_if_due(&client, &dir, &s).await;
        assert_eq!(load(&dir).unwrap().source, "marker");
    }

    #[test]
    fn effective_merges_custom_first_without_duplicates() {
        let dir = tmp("effective");
        save(&dir, &Stored { updated_at: 1, source: String::new(), trackers: vec!["udp://a.example:1".into(), "udp://b.example:1".into()] });
        let s = Settings {
            bt_custom_trackers: vec!["udp://b.example:1".into(), "bogus".into(), "wss://z".into(), "http://c.example/announce".into()],
            ..Default::default()
        };
        assert_eq!(effective(&dir, &s), vec!["udp://b.example:1", "http://c.example/announce", "udp://a.example:1"]);
    }
}
