//! 云端语音识别：火山引擎（豆包语音，大模型录音文件极速版）和讯飞（录音文件转写）。
//! 两个都是“上传整段音频 → 返回带时间的句子”，比 Whisper 一类接口快得多、中文更准。
//!
//! 注意：写这个模块时无法访问两家的官方文档站点，接口细节来自公开资料和已知用法，**还没有用真实密钥跑通过**。
//! 解析函数对返回格式做了尽量宽松的处理，并有单元测试；第一次用请先在“设置 → AI → 语音转文字”里点“测试连接”。

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::subtitle::Cue;

pub const VOLC_URL: &str = "https://openspeech.bytedance.com/api/v3/auc/bigmodel/recognize/flash";
pub const VOLC_RESOURCE: &str = "volc.bigasr.auc_turbo";
pub const XFYUN_BASE: &str = "https://raasr.xfyun.cn/v2/api";
pub const KEY_VOLC: &str = "stt.volc_key";
pub const KEY_XFYUN: &str = "stt.xfyun_secret";
/// 讯飞“实时语音转写”（语音翻译用）的 APIKey，和录音文件转写的 SecretKey 不是同一个
pub const KEY_XFYUN_RT: &str = "stt.xfyun_rt_key";

// ---------- 火山引擎（豆包语音） ----------

/// 请求头：填了 AppId 用旧版控制台的 `X-Api-App-Key` + `X-Api-Access-Key`，没填用新版控制台的 `X-Api-Key`。
pub fn volc_headers(app_id: &str, key: &str, request_id: &str) -> Vec<(&'static str, String)> {
    let mut h = vec![("X-Api-Resource-Id", VOLC_RESOURCE.to_string()), ("X-Api-Request-Id", request_id.to_string()), ("X-Api-Sequence", "-1".to_string())];
    if app_id.trim().is_empty() {
        h.push(("X-Api-Key", key.trim().to_string()));
    } else {
        h.push(("X-Api-App-Key", app_id.trim().to_string()));
        h.push(("X-Api-Access-Key", key.trim().to_string()));
    }
    h
}

pub fn volc_body(audio_b64: &str, ddc: bool) -> Value {
    json!({
        "user": {"uid": "clearclip"},
        "audio": {"data": audio_b64},
        "request": {"model_name": "bigmodel", "enable_itn": true, "enable_punc": true, "enable_ddc": ddc, "show_utterances": true}
    })
}

/// 解析极速版返回：`result.utterances[]{start_time,end_time,text}`（毫秒）；没有分句时退回整段文字。
pub fn parse_volc(v: &Value, duration_ms: u64) -> Vec<Cue> {
    let res = &v["result"];
    let mut out: Vec<Cue> = vec![];
    if let Some(us) = res["utterances"].as_array() {
        for u in us {
            let text = u["text"].as_str().unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }
            let (s, e) = (u["start_time"].as_u64().unwrap_or(0), u["end_time"].as_u64().unwrap_or(0));
            out.push(Cue { start: s, end: e.max(s + 300), lines: vec![text.to_string()] });
        }
    }
    if out.is_empty() {
        let text = res["text"].as_str().unwrap_or("").trim();
        if !text.is_empty() {
            out.push(Cue { start: 0, end: duration_ms.max(1000), lines: vec![text.to_string()] });
        }
    }
    out
}

/// `ddc`：用火山自带的“语义顺滑”去口水词（默认关，统一交给本地预处理）。
pub async fn volc_recognize(client: &reqwest::Client, app_id: &str, key: &str, audio: &[u8], duration_ms: u64, ddc: bool) -> AppResult<Vec<Cue>> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(audio);
    let rid = format!("{:x}-{:x}", crate::db::now(), audio.len());
    let mut req = client.post(VOLC_URL).timeout(Duration::from_secs(300)).json(&volc_body(&b64, ddc));
    for (k, v) in volc_headers(app_id, key, &rid) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| AppError::msg(format!("火山引擎语音识别请求失败：{e}")))?;
    let code = resp.headers().get("X-Api-Status-Code").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let msg = resp.headers().get("X-Api-Message").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    match code.as_str() {
        "" | "20000000" => {
            if !status.is_success() {
                return Err(AppError::msg(format!("火山引擎语音识别失败（HTTP {status}）{msg}")));
            }
        }
        // 静音：没有识别出内容，不算失败
        "20000003" => return Ok(vec![]),
        c => return Err(AppError::msg(volc_hint(c, &msg))),
    }
    Ok(parse_volc(&body, duration_ms))
}

fn volc_hint(code: &str, msg: &str) -> String {
    let why = match code {
        "45000001" | "45000002" => "请求参数有误或音频格式不支持（支持 WAV / MP3 / OGG OPUS）",
        "45000151" => "音频格式不正确",
        "55000031" | "45000292" => "服务器繁忙或请求过快，稍后重试",
        c if c.starts_with("4500") => "鉴权失败或没有开通“大模型录音文件极速版识别”：请检查 AppId / 密钥，并在火山引擎控制台开通该资源",
        _ => "",
    };
    format!("火山引擎语音识别失败（{code}）{msg}{}", if why.is_empty() { String::new() } else { format!("：{why}") })
}

// ---------- 讯飞（录音文件转写） ----------

fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    use sha1::{Digest, Sha1};
    let mut k = key.to_vec();
    if k.len() > 64 {
        k = Sha1::digest(&k).to_vec();
    }
    k.resize(64, 0);
    let mut ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let mut opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    ipad.extend_from_slice(msg);
    let inner = Sha1::digest(&ipad);
    opad.extend_from_slice(&inner);
    Sha1::digest(&opad).into()
}

/// 讯飞签名：`signa = base64( HmacSHA1( md5(appId + ts), secretKey ) )`，`ts` 是秒级时间戳。
pub fn xfyun_signa(app_id: &str, secret: &str, ts: &str) -> String {
    use base64::Engine;
    use md5::{Digest, Md5};
    let md5_hex: String = Md5::digest(format!("{app_id}{ts}").as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    base64::engine::general_purpose::STANDARD.encode(hmac_sha1(secret.as_bytes(), md5_hex.as_bytes()))
}

fn pct(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// 解析讯飞的 `orderResult`（一个 JSON 字符串）：`lattice[].json_1best`（又是一个 JSON 字符串）里 `st.bg/ed`（毫秒，字符串）和 `rt[].ws[].cw[].w`。
/// `drop_smooth`：丢掉讯飞标成“顺滑词”（`wp = s`，口水词）的词。
pub fn parse_xfyun_order_result(order_result: &str, drop_smooth: bool) -> Vec<Cue> {
    let Ok(v) = serde_json::from_str::<Value>(order_result) else { return vec![] };
    let mut out = vec![];
    for item in v["lattice"].as_array().cloned().unwrap_or_default() {
        let Some(one) = item["json_1best"].as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()) else { continue };
        let st = &one["st"];
        let num = |x: &Value| x.as_str().and_then(|s| s.parse::<u64>().ok()).or_else(|| x.as_u64()).unwrap_or(0);
        let (bg, ed) = (num(&st["bg"]), num(&st["ed"]));
        let mut text = String::new();
        for rt in st["rt"].as_array().cloned().unwrap_or_default() {
            for ws in rt["ws"].as_array().cloned().unwrap_or_default() {
                for cw in ws["cw"].as_array().cloned().unwrap_or_default() {
                    // wp：n 普通词、p 标点、s 顺滑（口水词）、g 分段标记
                    if cw["wp"].as_str() == Some("g") || (drop_smooth && cw["wp"].as_str() == Some("s")) {
                        continue;
                    }
                    text.push_str(cw["w"].as_str().unwrap_or(""));
                }
            }
        }
        let text = text.trim().to_string();
        if !text.is_empty() {
            out.push(Cue { start: bg, end: ed.max(bg + 300), lines: vec![text] });
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub async fn xfyun_recognize(
    client: &reqwest::Client,
    app_id: &str,
    secret: &str,
    audio: Vec<u8>,
    file_name: &str,
    duration_ms: u64,
    drop_smooth: bool,
    stop: &(dyn Fn() -> bool + Sync),
) -> AppResult<Vec<Cue>> {
    let ts = crate::db::now().to_string();
    let signa = xfyun_signa(app_id, secret, &ts);
    let url = format!(
        "{XFYUN_BASE}/upload?appId={}&signa={}&ts={}&fileSize={}&fileName={}&duration={}",
        pct(app_id),
        pct(&signa),
        ts,
        audio.len(),
        pct(file_name),
        duration_ms
    );
    let up: Value = client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .timeout(Duration::from_secs(600))
        .body(audio)
        .send()
        .await
        .map_err(|e| AppError::msg(format!("讯飞上传失败：{e}")))?
        .json()
        .await
        .map_err(|e| AppError::msg(format!("讯飞返回的不是预期的格式：{e}")))?;
    if up["code"].as_str() != Some("000000") {
        return Err(AppError::msg(xfyun_hint(&up)));
    }
    let order = up["content"]["orderId"].as_str().ok_or_else(|| AppError::msg("讯飞没有返回订单号"))?.to_string();
    for _ in 0..400 {
        if stop() {
            return Err(AppError::msg("canceled"));
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        let ts = crate::db::now().to_string();
        let signa = xfyun_signa(app_id, secret, &ts);
        let url = format!("{XFYUN_BASE}/getResult?appId={}&signa={}&ts={}&orderId={}&resultType=transfer", pct(app_id), pct(&signa), ts, pct(&order));
        let r: Value = client
            .get(&url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| AppError::msg(format!("讯飞查询失败：{e}")))?
            .json()
            .await
            .map_err(|e| AppError::msg(format!("讯飞返回的不是预期的格式：{e}")))?;
        if r["code"].as_str() != Some("000000") {
            return Err(AppError::msg(xfyun_hint(&r)));
        }
        match r["content"]["orderInfo"]["status"].as_i64().unwrap_or(0) {
            4 => return Ok(parse_xfyun_order_result(r["content"]["orderResult"].as_str().unwrap_or(""), drop_smooth)),
            -1 => return Err(AppError::msg(format!("讯飞转写失败（{}）", r["content"]["orderInfo"]["failType"]))),
            _ => {}
        }
    }
    Err(AppError::msg("讯飞转写等待超时"))
}

fn xfyun_hint(v: &Value) -> String {
    let code = v["code"].as_str().unwrap_or("");
    let desc = v["descInfo"].as_str().unwrap_or("");
    let why = match code {
        "26602" | "26604" | "26605" => "服务繁忙或订单状态异常，稍后重试",
        "26610" | "26611" | "26612" => "AppId 或密钥不对，或没有开通“录音文件转写”",
        "26200" | "26201" | "26202" => "签名或时间戳错误：检查密钥，并确认电脑时间准确",
        _ => "",
    };
    format!("讯飞语音识别失败（{code}）{desc}{}", if why.is_empty() { String::new() } else { format!("：{why}") })
}

// ---------- 讯飞实时语音转写（WebSocket，语音翻译用） ----------

pub const XFYUN_RT_URL: &str = "wss://rtasr.xfyun.cn/v1/ws";
/// 每帧 1280 字节（16 kHz 16 bit 单声道的 40 毫秒）；发送间隔比实时快，整句说完再发，要尽快拿到结果
const RT_FRAME: usize = 1280;
const RT_PACE_MS: u64 = 8;

pub fn xfyun_rt_url(base: &str, app_id: &str, api_key: &str, ts: &str) -> String {
    format!("{base}?appid={}&ts={}&signa={}", pct(app_id), ts, pct(&xfyun_signa(app_id, api_key, ts)))
}

/// 解析一条实时转写返回的 `data`：返回（是不是定稿, 文字）。`type` 为 `0` 是整句定稿，`1` 是中间结果。
pub fn parse_rt_result(data: &str, drop_smooth: bool) -> Option<(bool, String)> {
    let v: Value = serde_json::from_str(data).ok()?;
    let st = &v["cn"]["st"];
    let fin = st["type"].as_str() == Some("0") || st["type"].as_u64() == Some(0);
    let mut text = String::new();
    for rt in st["rt"].as_array()? {
        for ws in rt["ws"].as_array().cloned().unwrap_or_default() {
            for cw in ws["cw"].as_array().cloned().unwrap_or_default() {
                if cw["wp"].as_str() == Some("g") || (drop_smooth && cw["wp"].as_str() == Some("s")) {
                    continue;
                }
                text.push_str(cw["w"].as_str().unwrap_or(""));
            }
        }
    }
    Some((fin, text))
}

/// 连 wss 用 ring 提供的加密实现和系统根证书（程序里同时有别的加密后端，不能依赖“进程默认”）。
fn rt_connector() -> AppResult<tokio_tungstenite::Connector> {
    use std::sync::Arc;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    for c in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(c);
    }
    let cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| AppError::msg(format!("TLS 初始化失败：{e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_tungstenite::Connector::Rustls(Arc::new(cfg)))
}

/// 一句话（16 kHz 16 bit 单声道 PCM）→ 文字：连上、等 `started`、发音频、发结束、收完所有定稿。
pub async fn xfyun_rt_text(app_id: &str, api_key: &str, pcm: &[u8], drop_smooth: bool) -> AppResult<String> {
    let url = xfyun_rt_url(XFYUN_RT_URL, app_id, api_key, &crate::db::now().to_string());
    xfyun_rt_text_at(&url, Some(rt_connector()?), pcm, drop_smooth).await
}

pub async fn xfyun_rt_text_at(url: &str, connector: Option<tokio_tungstenite::Connector>, pcm: &[u8], drop_smooth: bool) -> AppResult<String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async_tls_with_config(url, None, false, connector))
        .await
        .map_err(|_| AppError::msg("连接讯飞实时转写超时"))?
        .map_err(|e| AppError::msg(format!("连接讯飞实时转写失败：{e}")))?;
    // 1) 等握手成功
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.map_err(|_| AppError::msg("讯飞实时转写没有回应"))?;
        let Some(Ok(Message::Text(t))) = m else { return Err(AppError::msg("讯飞实时转写连接被关闭")) };
        let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
        match v["action"].as_str() {
            Some("started") => break,
            Some("error") => return Err(AppError::msg(xfyun_rt_hint(&v))),
            _ => {}
        }
    }
    // 2) 发音频，再发结束
    for frame in pcm.chunks(RT_FRAME) {
        ws.send(Message::Binary(frame.to_vec().into())).await.map_err(|e| AppError::msg(format!("发送音频失败：{e}")))?;
        tokio::time::sleep(Duration::from_millis(RT_PACE_MS)).await;
    }
    ws.send(Message::Text("{\"end\": true}".into())).await.map_err(|e| AppError::msg(format!("发送结束标记失败：{e}")))?;
    // 3) 收结果，直到服务器关闭
    let mut out = String::new();
    while let Ok(Some(m)) = tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
        match m {
            Ok(Message::Text(t)) => {
                let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                match v["action"].as_str() {
                    Some("result") => {
                        if let Some((true, text)) = v["data"].as_str().and_then(|d| parse_rt_result(d, drop_smooth)) {
                            out.push_str(&text);
                        }
                    }
                    Some("error") => return Err(AppError::msg(xfyun_rt_hint(&v))),
                    _ => {}
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    Ok(out.trim().to_string())
}

fn xfyun_rt_hint(v: &Value) -> String {
    let code = v["code"].as_str().unwrap_or("");
    let why = match code {
        "10105" | "10106" | "10107" | "10110" | "10700" => "没有开通“实时语音转写”，或 AppID / APIKey 不对",
        "10202" | "10205" => "网络连接问题，稍后重试",
        "37005" | "37006" => "音频格式不对（需要 16 kHz 16 bit 单声道）",
        _ => "",
    };
    format!("讯飞实时转写失败（{code}）{}{}", v["desc"].as_str().unwrap_or(""), if why.is_empty() { String::new() } else { format!("：{why}") })
}

// ---------- 测试连接 ----------

/// 一秒钟的静音 WAV（16 kHz 16 bit 单声道）：测试连接用，几乎不花钱。
pub fn silent_wav(ms: u32) -> Vec<u8> {
    let samples = 16 * ms;
    let data_len = samples * 2;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&16_000u32.to_le_bytes());
    v.extend_from_slice(&32_000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v.resize(44 + data_len as usize, 0);
    v
}

/// 火山：发一秒静音。返回“静音 / 识别成功”都算连通；鉴权失败、没开通资源会报出原因。
pub async fn test_volc(client: &reqwest::Client, app_id: &str, key: &str) -> AppResult<String> {
    volc_recognize(client, app_id, key, &silent_wav(1000), 1000, false).await?;
    Ok("连通：密钥有效，可以使用“大模型录音文件极速版识别”。".into())
}

/// 讯飞录音文件转写：只做“上传”这一步（鉴权和开通都在这一步检查），不等结果。
pub async fn test_xfyun(client: &reqwest::Client, app_id: &str, secret: &str) -> AppResult<String> {
    let wav = silent_wav(1000);
    let ts = crate::db::now().to_string();
    let url = format!(
        "{XFYUN_BASE}/upload?appId={}&signa={}&ts={}&fileSize={}&fileName=test.wav&duration=1000",
        pct(app_id),
        pct(&xfyun_signa(app_id, secret, &ts)),
        ts,
        wav.len()
    );
    let up: Value = client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .timeout(Duration::from_secs(30))
        .body(wav)
        .send()
        .await
        .map_err(|e| AppError::msg(format!("讯飞上传失败：{e}")))?
        .json()
        .await
        .map_err(|e| AppError::msg(format!("讯飞返回的不是预期的格式：{e}")))?;
    if up["code"].as_str() != Some("000000") {
        return Err(AppError::msg(xfyun_hint(&up)));
    }
    Ok("连通：AppID 和 SecretKey 有效，“录音文件转写”可以使用。".into())
}

/// 讯飞实时转写：连上并收到 `started` 就算通过。
pub async fn test_xfyun_rt(app_id: &str, api_key: &str) -> AppResult<String> {
    xfyun_rt_text(app_id, api_key, &silent_wav(300)[44..], false).await?;
    Ok("连通：AppID 和 APIKey 有效，“实时语音转写”可以使用。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha1_matches_rfc2202() {
        // RFC 2202 测试向量 2：key = "Jefe"，msg = "what do ya want for nothing?"
        let mac: String = hmac_sha1(b"Jefe", b"what do ya want for nothing?").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(mac, "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
    }

    #[test]
    fn volc_headers_follow_the_console_generation() {
        let new = volc_headers("", "k", "r");
        assert!(new.iter().any(|(k, v)| *k == "X-Api-Key" && v == "k") && !new.iter().any(|(k, _)| *k == "X-Api-App-Key"));
        let old = volc_headers("123", "tok", "r");
        assert!(old.iter().any(|(k, v)| *k == "X-Api-App-Key" && v == "123") && old.iter().any(|(k, v)| *k == "X-Api-Access-Key" && v == "tok"));
        assert!(old.iter().any(|(k, v)| *k == "X-Api-Resource-Id" && v == VOLC_RESOURCE));
    }

    #[test]
    fn volc_response_is_parsed_into_cues() {
        let v = json!({"audio_info": {"duration": 5000}, "result": {"text": "你好世界。再见。", "utterances": [
            {"start_time": 100, "end_time": 1800, "text": "你好世界。"}, {"start_time": 2000, "end_time": 3000, "text": "再见。"}, {"start_time": 3000, "end_time": 3500, "text": "  "}]}});
        let c = parse_volc(&v, 5000);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start, c[0].end, c[0].lines[0].as_str()), (100, 1800, "你好世界。"));
        // 没有分句：整段文字一条
        let only = parse_volc(&json!({"result": {"text": "hello"}}), 4000);
        assert_eq!((only.len(), only[0].end), (1, 4000));
        assert!(parse_volc(&json!({"result": {}}), 100).is_empty());
    }

    #[test]
    fn xfyun_signature_has_the_documented_shape() {
        // signa = base64(HmacSHA1(md5(appId+ts), secret))：20 字节摘要 → 28 个字符的 base64
        let s = xfyun_signa("595f23df", "d9f4aa7ea6d94faca62cd88a28fd5234", "1512041814");
        assert_eq!(s.len(), 28);
        assert!(s.ends_with('='));
        assert_eq!(s, xfyun_signa("595f23df", "d9f4aa7ea6d94faca62cd88a28fd5234", "1512041814"));
        assert_ne!(s, xfyun_signa("595f23df", "d9f4aa7ea6d94faca62cd88a28fd5234", "1512041815"));
    }

    #[test]
    fn xfyun_order_result_is_parsed() {
        let one = json!({"st": {"bg": "1200", "ed": "4810", "rt": [{"ws": [
            {"cw": [{"w": "今天", "wp": "n"}]}, {"cw": [{"w": "嗯", "wp": "s"}]}, {"cw": [{"w": "天气", "wp": "n"}]}, {"cw": [{"w": "。", "wp": "p"}]}]}]}});
        let order = json!({"lattice": [{"json_1best": one.to_string()}, {"json_1best": "not json"}]}).to_string();
        let c = parse_xfyun_order_result(&order, false);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].start, c[0].end), (1200, 4810));
        assert_eq!(c[0].lines[0], "今天嗯天气。", "顺滑词先保留，交给预处理决定去不去");
        assert_eq!(parse_xfyun_order_result(&order, true)[0].lines[0], "今天天气。", "开顺滑过滤后“嗯”被去掉");
        assert!(parse_xfyun_order_result("garbage", false).is_empty());
    }

    #[test]
    fn realtime_results_are_parsed() {
        let fin = json!({"cn": {"st": {"bg": "0", "ed": "0", "type": "0", "rt": [{"ws": [{"cw": [{"w": "你好", "wp": "n"}]}, {"cw": [{"w": "呃", "wp": "s"}]}, {"cw": [{"w": "。", "wp": "p"}]}]}]}}}).to_string();
        assert_eq!(parse_rt_result(&fin, false), Some((true, "你好呃。".into())));
        assert_eq!(parse_rt_result(&fin, true), Some((true, "你好。".into())));
        let interim = json!({"cn": {"st": {"type": "1", "rt": [{"ws": [{"cw": [{"w": "你", "wp": "n"}]}]}]}}}).to_string();
        assert_eq!(parse_rt_result(&interim, false), Some((false, "你".into())));
        assert_eq!(parse_rt_result("nope", false), None);
        assert!(xfyun_rt_url("wss://h/v1/ws", "app", "key", "1700000000").contains("appid=app&ts=1700000000&signa="));
    }

    #[test]
    fn silent_wav_has_a_valid_header() {
        let w = silent_wav(1000);
        assert_eq!(w.len(), 44 + 32_000);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 32_000);
    }

    /// 本地起一个假的讯飞实时转写服务器，走完整的“握手 → 发音频 → 发结束 → 收定稿”流程。
    #[tokio::test]
    async fn realtime_client_runs_the_whole_flow_against_a_local_server() {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            ws.send(Message::Text(json!({"action": "started", "code": "0"}).to_string().into())).await.unwrap();
            let mut bytes = 0usize;
            while let Some(Ok(m)) = ws.next().await {
                match m {
                    Message::Binary(b) => bytes += b.len(),
                    Message::Text(t) if t.contains("end") => break,
                    _ => {}
                }
            }
            let part = |t: &str, ty: &str| {
                json!({"action": "result", "code": "0", "data": json!({"cn": {"st": {"type": ty, "rt": [{"ws": [{"cw": [{"w": t, "wp": "n"}]}]}]}}}).to_string()}).to_string()
            };
            ws.send(Message::Text(part("你", "1").into())).await.unwrap();
            ws.send(Message::Text(part("你好", "0").into())).await.unwrap();
            ws.send(Message::Text(part("世界。", "0").into())).await.unwrap();
            ws.send(Message::Close(None)).await.unwrap();
            bytes
        });
        let pcm = vec![0u8; RT_FRAME * 3 + 100];
        let text = xfyun_rt_text_at(&format!("ws://{addr}/v1/ws"), None, &pcm, false).await.unwrap();
        assert_eq!(text, "你好世界。", "中间结果不算，定稿按顺序拼起来");
        assert_eq!(server.await.unwrap(), pcm.len(), "服务器收到了全部音频");
    }
}
