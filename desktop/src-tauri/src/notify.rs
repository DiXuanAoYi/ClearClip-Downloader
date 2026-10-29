//! 外部通知：把下载完成 / 失败、开播、订阅更新、登录失效推送到手机或聊天工具
//! （Webhook、Telegram、Bark、Server酱、企业微信、钉钉、飞书、ntfy）。令牌保存在加密保险箱里。

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::settings::{NotifyChannel, NotifySettings};
use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Done,
    Failed,
    Live,
    Sub,
    Account,
    /// 自动规则命中时的通知（规则里勾选了才会触发，不再另设开关）
    Rule,
    /// “发送测试”不看事件开关
    Test,
}

impl Event {
    fn enabled(self, s: &NotifySettings) -> bool {
        match self {
            Event::Done => s.on_done,
            Event::Failed => s.on_failed,
            Event::Live => s.on_live,
            Event::Sub => s.on_sub,
            Event::Account => s.on_account,
            Event::Rule | Event::Test => true,
        }
    }
}

/// 一个待发送的 HTTP 请求。
#[derive(Debug, Clone, PartialEq)]
pub struct Req {
    pub url: String,
    pub content_type: &'static str,
    pub body: String,
    pub headers: Vec<(&'static str, String)>,
}

pub const KINDS: &[&str] = &["webhook", "telegram", "bark", "serverchan", "wecom", "dingtalk", "feishu", "ntfy"];

fn need_url(secret: &str) -> AppResult<()> {
    if secret.starts_with("https://") || secret.starts_with("http://") {
        Ok(())
    } else {
        Err(AppError::invalid("请填写完整的地址（以 https:// 开头）。"))
    }
}

/// 各渠道的请求格式。`secret` 是令牌或 Webhook 地址。
pub fn build(kind: &str, target: &str, secret: &str, title: &str, body: &str, now: i64) -> AppResult<Req> {
    let text = if body.is_empty() { title.to_string() } else { format!("{title}\n{body}") };
    let json_req = |url: String, v: serde_json::Value| Req { url, content_type: "application/json", body: v.to_string(), headers: vec![] };
    match kind {
        "webhook" => {
            need_url(secret)?;
            Ok(json_req(secret.to_string(), json!({"app": "ClearClip", "title": title, "body": body, "text": text, "time": now})))
        }
        "telegram" => {
            if secret.is_empty() || target.is_empty() {
                return Err(AppError::invalid("Telegram 需要机器人令牌和 chat id。"));
            }
            Ok(json_req(
                format!("https://api.telegram.org/bot{secret}/sendMessage"),
                json!({"chat_id": target, "text": text, "disable_web_page_preview": true}),
            ))
        }
        "bark" => {
            if secret.is_empty() {
                return Err(AppError::invalid("Bark 需要设备密钥。"));
            }
            let server = if target.is_empty() { "https://api.day.app" } else { target.trim_end_matches('/') };
            Ok(json_req(format!("{server}/{secret}"), json!({"title": title, "body": body, "group": "ClearClip"})))
        }
        "serverchan" => {
            if secret.is_empty() {
                return Err(AppError::invalid("Server酱 需要 SendKey。"));
            }
            let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
            Ok(Req {
                url: format!("https://sctapi.ftqq.com/{secret}.send"),
                content_type: "application/x-www-form-urlencoded",
                body: format!("title={}&desp={}", enc(title), enc(body)),
                headers: vec![],
            })
        }
        "wecom" | "dingtalk" => {
            need_url(secret)?;
            Ok(json_req(secret.to_string(), json!({"msgtype": "text", "text": {"content": text}})))
        }
        "feishu" => {
            need_url(secret)?;
            Ok(json_req(secret.to_string(), json!({"msg_type": "text", "content": {"text": text}})))
        }
        "ntfy" => {
            need_url(secret)?;
            // 标题放在请求头里，只能是 ASCII 以外字符需要编码；ntfy 支持 RFC 2047，这里直接把标题放进正文
            Ok(Req { url: secret.to_string(), content_type: "text/plain; charset=utf-8", body: text, headers: vec![("Tags", "arrow_down".into())] })
        }
        _ => Err(AppError::invalid("不支持的通知渠道。")),
    }
}

/// 各渠道回复里表示失败的情况（有的渠道失败时也返回 200）。
pub fn reply_ok(kind: &str, status: u16, body: &str) -> Result<(), String> {
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let code = |k: &str| v.get(k).and_then(|c| c.as_i64());
    match kind {
        "telegram" if v.get("ok").and_then(|o| o.as_bool()) == Some(false) => {
            Err(v.get("description").and_then(|d| d.as_str()).unwrap_or("Telegram 拒绝了请求").to_string())
        }
        "wecom" | "dingtalk" if code("errcode").is_some_and(|c| c != 0) => Err(v.get("errmsg").and_then(|d| d.as_str()).unwrap_or("接口返回错误").to_string()),
        "feishu" if code("code").is_some_and(|c| c != 0) => Err(v.get("msg").and_then(|d| d.as_str()).unwrap_or("接口返回错误").to_string()),
        "serverchan" if code("code").is_some_and(|c| c != 0) => Err(v.get("message").and_then(|d| d.as_str()).unwrap_or("接口返回错误").to_string()),
        "bark" if code("code").is_some_and(|c| c != 200) => Err(v.get("message").and_then(|d| d.as_str()).unwrap_or("接口返回错误").to_string()),
        _ => Ok(()),
    }
}

/// 发送一条通知到一个渠道（失败重试两次）。
pub async fn deliver(client: &reqwest::Client, kind: &str, req: &Req, retry_ms: u64) -> AppResult<()> {
    let mut last = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(retry_ms * (1 << (attempt - 1)))).await;
        }
        let mut r = client.post(&req.url).header("Content-Type", req.content_type).timeout(Duration::from_secs(15)).body(req.body.clone());
        for (k, v) in &req.headers {
            r = r.header(*k, v);
        }
        match r.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                match reply_ok(kind, status, &text) {
                    Ok(()) => return Ok(()),
                    // 4xx 重试没有意义（令牌错误等）
                    Err(e) if (400..500).contains(&status) => return Err(AppError::new(ErrorKind::Invalid, format!("通知发送失败：{e}"))),
                    Err(e) => last = e,
                }
            }
            Err(e) => last = if e.is_timeout() { "连接超时".into() } else { "无法连接".into() },
        }
    }
    Err(AppError::new(ErrorKind::Network, format!("通知发送失败：{last}")))
}

fn secret_name(channel: &NotifyChannel) -> String {
    format!("notify.{}", channel.id)
}

async fn send_one(st: &AppState, ch: &NotifyChannel, title: &str, body: &str) -> AppResult<()> {
    let secret = st.vault.get(&secret_name(ch)).unwrap_or_default();
    let req = build(&ch.kind, &ch.target, &secret, title, body, crate::db::now())?;
    let settings = st.settings();
    // 通知走程序的网络设置（Telegram 在国内常需要代理）
    let client = st.net.clients_for(&settings.network, &req.url)?.api;
    deliver(&client, &ch.kind, &req, 1500).await
}

/// 触发一个事件：发给所有启用的渠道（在后台发送，不阻塞调用方）。
pub fn emit(app: &AppHandle, event: Event, title: &str, body: &str) {
    let st = app.state::<Arc<AppState>>().inner().clone();
    let settings = st.settings();
    if !event.enabled(&settings.notify) || !settings.notify.channels.iter().any(|c| c.enabled) {
        return;
    }
    let (title, body) = (title.to_string(), body.to_string());
    tauri::async_runtime::spawn(async move {
        for ch in settings.notify.channels.iter().filter(|c| c.enabled) {
            if let Err(e) = send_one(&st, ch, &title, &body).await {
                log::warn!("notify channel {} failed: {e}", ch.name);
            }
        }
    });
}

/// 给指定渠道发一条测试通知。
pub async fn test(st: &AppState, id: &str) -> AppResult<()> {
    let ch = st.settings().notify.channels.into_iter().find(|c| c.id == id).ok_or_else(|| AppError::not_found("渠道不存在"))?;
    send_one(st, &ch, "清影 ClearClip", "这是一条测试通知，收到说明配置正确。").await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(kind: &str, target: &str, secret: &str) -> AppResult<Req> {
        build(kind, target, secret, "下载完成", "视频 A", 1_700_000_000)
    }

    #[test]
    fn request_shapes() {
        let r = b("webhook", "", "https://hook.example.com/x").unwrap();
        assert_eq!(r.url, "https://hook.example.com/x");
        let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!((v["title"].as_str(), v["body"].as_str(), v["app"].as_str()), (Some("下载完成"), Some("视频 A"), Some("ClearClip")));
        assert_eq!(v["time"], 1_700_000_000);
        let r = b("telegram", "12345", "123:ABC").unwrap();
        assert_eq!(r.url, "https://api.telegram.org/bot123:ABC/sendMessage");
        let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
        assert_eq!((v["chat_id"].as_str(), v["text"].as_str()), (Some("12345"), Some("下载完成\n视频 A")));
        assert_eq!(b("bark", "", "KEY").unwrap().url, "https://api.day.app/KEY");
        assert_eq!(b("bark", "https://bark.me/", "KEY").unwrap().url, "https://bark.me/KEY");
        let r = b("serverchan", "", "SCT1").unwrap();
        assert_eq!(r.url, "https://sctapi.ftqq.com/SCT1.send");
        assert!(r.body.starts_with("title=%E4%B8%8B%E8%BD%BD") && r.content_type.contains("urlencoded"), "{}", r.body);
        let v: serde_json::Value = serde_json::from_str(&b("wecom", "", "https://qyapi.weixin.qq.com/k").unwrap().body).unwrap();
        assert_eq!(v["msgtype"], "text");
        assert_eq!(v["text"]["content"], "下载完成\n视频 A");
        let v: serde_json::Value = serde_json::from_str(&b("feishu", "", "https://open.feishu.cn/h").unwrap().body).unwrap();
        assert_eq!(v["msg_type"], "text");
        let r = b("ntfy", "", "https://ntfy.sh/mytopic").unwrap();
        assert_eq!((r.content_type, r.body.as_str()), ("text/plain; charset=utf-8", "下载完成\n视频 A"));
        // title only
        assert_eq!(build("wecom", "", "https://x.com/h", "只有标题", "", 0).map(|r| r.body).unwrap(), r#"{"msgtype":"text","text":{"content":"只有标题"}}"#);
    }

    #[test]
    fn bad_config_is_rejected() {
        assert!(b("webhook", "", "not a url").is_err());
        assert!(b("telegram", "", "tok").is_err() && b("telegram", "1", "").is_err());
        assert!(b("bark", "", "").is_err() && b("serverchan", "", "").is_err());
        assert!(b("sms", "", "x").is_err());
        assert!(KINDS.iter().all(|k| *k != "sms"));
    }

    #[test]
    fn some_services_fail_with_http_200() {
        assert!(reply_ok("telegram", 200, r#"{"ok":true}"#).is_ok());
        assert_eq!(reply_ok("telegram", 200, r#"{"ok":false,"description":"chat not found"}"#).unwrap_err(), "chat not found");
        assert_eq!(reply_ok("wecom", 200, r#"{"errcode":93000,"errmsg":"invalid webhook url"}"#).unwrap_err(), "invalid webhook url");
        assert!(reply_ok("wecom", 200, r#"{"errcode":0,"errmsg":"ok"}"#).is_ok());
        assert_eq!(reply_ok("feishu", 200, r#"{"code":19001,"msg":"param invalid"}"#).unwrap_err(), "param invalid");
        assert!(reply_ok("feishu", 200, r#"{"code":0}"#).is_ok());
        assert!(reply_ok("bark", 200, r#"{"code":200,"message":"success"}"#).is_ok());
        assert!(reply_ok("bark", 200, r#"{"code":400,"message":"bad key"}"#).is_err());
        assert!(reply_ok("webhook", 200, "plain text").is_ok());
        assert_eq!(reply_ok("webhook", 500, "").unwrap_err(), "HTTP 500");
    }

    #[test]
    fn events_follow_their_switches() {
        let mut s = NotifySettings::default();
        assert!(Event::Done.enabled(&s) && Event::Failed.enabled(&s));
        s.on_done = false;
        assert!(!Event::Done.enabled(&s));
        assert!(Event::Test.enabled(&s), "tests ignore the switches");
    }

    #[tokio::test]
    async fn delivery_retries_server_errors_but_not_client_errors() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let hits = Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let n = h2.fetch_add(1, Ordering::SeqCst) + 1;
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = sock.read(&mut buf).await;
                    let req = String::from_utf8_lossy(&buf).into_owned();
                    let status = if req.contains("/client-error") {
                        400
                    } else if n < 3 {
                        503
                    } else {
                        200
                    };
                    let body = "{}";
                    let _ =
                        sock.write_all(format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
                });
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let ok = Req { url: format!("http://{addr}/ok"), content_type: "application/json", body: "{}".into(), headers: vec![] };
        deliver(&client, "webhook", &ok, 5).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 3, "two 503s then success");
        hits.store(0, Ordering::SeqCst);
        let bad = Req { url: format!("http://{addr}/client-error"), ..ok.clone() };
        let e = deliver(&client, "webhook", &bad, 5).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Invalid);
        assert_eq!(hits.load(Ordering::SeqCst), 1, "no retry on 4xx");
        let dead = Req { url: "http://127.0.0.1:1/x".into(), ..ok };
        assert_eq!(deliver(&client, "webhook", &dead, 5).await.unwrap_err().kind, ErrorKind::Network);
    }
}
