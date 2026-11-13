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

pub fn volc_body(audio_b64: &str) -> Value {
    json!({
        "user": {"uid": "clearclip"},
        "audio": {"data": audio_b64},
        "request": {"model_name": "bigmodel", "enable_itn": true, "enable_punc": true, "enable_ddc": false, "show_utterances": true}
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

pub async fn volc_recognize(client: &reqwest::Client, app_id: &str, key: &str, audio: &[u8], duration_ms: u64) -> AppResult<Vec<Cue>> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(audio);
    let rid = format!("{:x}-{:x}", crate::db::now(), audio.len());
    let mut req = client.post(VOLC_URL).timeout(Duration::from_secs(300)).json(&volc_body(&b64));
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
pub fn parse_xfyun_order_result(order_result: &str) -> Vec<Cue> {
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
                    if cw["wp"].as_str() == Some("g") {
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

pub async fn xfyun_recognize(client: &reqwest::Client, app_id: &str, secret: &str, audio: Vec<u8>, file_name: &str, duration_ms: u64, stop: &(dyn Fn() -> bool + Sync)) -> AppResult<Vec<Cue>> {
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
            4 => return Ok(parse_xfyun_order_result(r["content"]["orderResult"].as_str().unwrap_or(""))),
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
        let c = parse_xfyun_order_result(&order);
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].start, c[0].end), (1200, 4810));
        assert_eq!(c[0].lines[0], "今天嗯天气。", "顺滑词先保留，交给预处理决定去不去");
        assert!(parse_xfyun_order_result("garbage").is_empty());
    }
}
