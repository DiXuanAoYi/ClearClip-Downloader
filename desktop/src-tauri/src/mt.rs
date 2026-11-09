//! 机器翻译服务（Google Cloud Translation / 微软 Azure Translator / DeepL / DeepLX / 百度翻译 / 有道翻译 / 彩云小译 / 小牛翻译）。
//!
//! 它们不是对话接口，不能用“兼容 OpenAI”那一栏填。这里把它们接进模型池：翻译字幕、网页翻译、文件翻译、
//! 屏幕翻译走池子时，如果挑中的是机器翻译服务，就改调它们各自的接口；摘要、问答这类需要“理解”的任务不会用到它们。
//! 机器翻译用不上术语库和 AI 专家（接口不支持提示词），需要术语一致时把对话模型排在它前面。

use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};

/// 池条目的服务类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 兼容 OpenAI 的对话接口（默认）
    Chat,
    Google,
    Microsoft,
    DeepL,
    DeepLX,
    /// 百度翻译开放平台：密钥栏填“应用ID:密钥”
    Baidu,
    /// 有道智云：密钥栏填“应用ID:应用密钥”
    Youdao,
    /// 彩云小译：密钥栏填 token（只支持译成中文 / 英文 / 日文）
    Caiyun,
    /// 小牛翻译：密钥栏填 apikey
    Niutrans,
    /// 腾讯云机器翻译：密钥栏填“SecretId:SecretKey”
    Tencent,
    /// 阿里云机器翻译：密钥栏填“AccessKeyId:AccessKeySecret”
    Aliyun,
    /// 火山引擎机器翻译：密钥栏填“AccessKeyId:SecretAccessKey”
    Volcano,
}

impl Kind {
    pub fn parse(s: &str) -> Kind {
        match s {
            "google" => Kind::Google,
            "microsoft" => Kind::Microsoft,
            "deepl" => Kind::DeepL,
            "deeplx" => Kind::DeepLX,
            "baidu" => Kind::Baidu,
            "youdao" => Kind::Youdao,
            "caiyun" => Kind::Caiyun,
            "niutrans" => Kind::Niutrans,
            "tencent" => Kind::Tencent,
            "aliyun" => Kind::Aliyun,
            "volcano" => Kind::Volcano,
            _ => Kind::Chat,
        }
    }

    pub fn is_mt(self) -> bool {
        self != Kind::Chat
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Chat => "对话模型",
            Kind::Google => "Google 翻译",
            Kind::Microsoft => "微软翻译",
            Kind::DeepL => "DeepL",
            Kind::DeepLX => "DeepLX",
            Kind::Baidu => "百度翻译",
            Kind::Youdao => "有道翻译",
            Kind::Caiyun => "彩云小译",
            Kind::Niutrans => "小牛翻译",
            Kind::Tencent => "腾讯翻译",
            Kind::Aliyun => "阿里翻译",
            Kind::Volcano => "火山翻译",
        }
    }
}

/// 国内几家的目标语言代码：(百度, 有道, 彩云, 小牛)。空串 = 这家不支持。
fn cn_codes(tag: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match tag {
        "zh" => ("zh", "zh-CHS", "auto2zh", "zh"),
        "zh-Hant" => ("cht", "zh-CHT", "", "cht"),
        "en" => ("en", "en", "auto2en", "en"),
        "ja" => ("jp", "ja", "auto2ja", "ja"),
        "ko" => ("kor", "ko", "", "ko"),
        "fr" => ("fra", "fr", "", "fr"),
        "de" => ("de", "de", "", "de"),
        "es" => ("spa", "es", "", "es"),
        "ru" => ("ru", "ru", "", "ru"),
        "pt" => ("pt", "pt", "", "pt"),
        "it" => ("it", "it", "", "it"),
        "ar" => ("ara", "ar", "", "ar"),
        "th" => ("th", "th", "", "th"),
        "vi" => ("vie", "vi", "", "vi"),
        "id" => ("id", "id", "", "id"),
        "tr" => ("tr", "tr", "", "tr"),
        "pl" => ("pl", "pl", "", "pl"),
        "nl" => ("nl", "nl", "", "nl"),
        "uk" => ("ukr", "uk", "", "uk"),
        "hi" => ("hi", "hi", "", "hi"),
        _ => ("", "", "", ""),
    }
}

/// 三家云厂商的目标语言代码：(腾讯, 阿里, 火山)。空串 = 这家不支持。
fn cloud_codes(tag: &str) -> (&'static str, &'static str, &'static str) {
    match tag {
        "zh" => ("zh", "zh", "zh"),
        "zh-Hant" => ("zh-TW", "zh-tw", "zh-Hant"),
        "en" => ("en", "en", "en"),
        "ja" => ("ja", "ja", "ja"),
        "ko" => ("ko", "ko", "ko"),
        "fr" => ("fr", "fr", "fr"),
        "de" => ("de", "de", "de"),
        "es" => ("es", "es", "es"),
        "ru" => ("ru", "ru", "ru"),
        "pt" => ("pt", "pt", "pt"),
        "it" => ("it", "it", "it"),
        "ar" => ("ar", "ar", "ar"),
        "th" => ("th", "th", "th"),
        "vi" => ("vi", "vi", "vi"),
        "id" => ("id", "id", "id"),
        "tr" => ("tr", "tr", "tr"),
        "pl" => ("", "pl", "pl"),
        "nl" => ("", "nl", "nl"),
        "uk" => ("", "uk", "uk"),
        "hi" => ("hi", "hi", "hi"),
        _ => ("", "", ""),
    }
}

/// 各家的目标语言代码：(Google, 微软, DeepL)。空串 = 这家不支持。
fn codes(tag: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match tag {
        "zh" => ("zh-CN", "zh-Hans", "ZH-HANS"),
        "zh-Hant" => ("zh-TW", "zh-Hant", "ZH-HANT"),
        "en" => ("en", "en", "EN-US"),
        "ja" => ("ja", "ja", "JA"),
        "ko" => ("ko", "ko", "KO"),
        "fr" => ("fr", "fr", "FR"),
        "de" => ("de", "de", "DE"),
        "es" => ("es", "es", "ES"),
        "ru" => ("ru", "ru", "RU"),
        "pt" => ("pt", "pt", "PT-BR"),
        "it" => ("it", "it", "IT"),
        "ar" => ("ar", "ar", "AR"),
        "th" => ("th", "th", ""),
        "vi" => ("vi", "vi", ""),
        "id" => ("id", "id", "ID"),
        "tr" => ("tr", "tr", "TR"),
        "pl" => ("pl", "pl", "PL"),
        "nl" => ("nl", "nl", "NL"),
        "uk" => ("uk", "uk", "UK"),
        "hi" => ("hi", "hi", ""),
        _ => return None,
    })
}

/// 目标语言（“简体中文”“English”…）→ 这家服务认的代码。
pub fn target_code(kind: Kind, lang: &str) -> AppResult<&'static str> {
    let tag = crate::ai::lang_tag(lang);
    let (g, m, d) = codes(tag).ok_or_else(|| AppError::invalid(format!("机器翻译服务不认识目标语言“{lang}”，请选常见语言，或把对话模型排在前面。")))?;
    let (bd, yd, cy, nt) = cn_codes(tag);
    let (tx, al, vc) = cloud_codes(tag);
    let c = match kind {
        Kind::Google => g,
        Kind::Microsoft => m,
        Kind::DeepL | Kind::DeepLX => d,
        Kind::Baidu => bd,
        Kind::Youdao => yd,
        Kind::Caiyun => cy,
        Kind::Niutrans => nt,
        Kind::Tencent => tx,
        Kind::Aliyun => al,
        Kind::Volcano => vc,
        Kind::Chat => "",
    };
    if c.is_empty() {
        return Err(AppError::invalid(format!("{} 不支持翻译成“{lang}”。", kind.label())));
    }
    Ok(c)
}

/// 池子发来的请求如果是“把一个 JSON 数组里的文字译成某种语言”，取出目标语言；其他请求（术语提取、摘要、看图）返回 None。
/// 翻译字幕 / 网页翻译 / 屏幕翻译的系统提示词都含 “into {lang}” 和 “JSON array”。
pub fn translate_request(system: &str, user: &Value) -> Option<(String, Vec<String>)> {
    if !system.contains("JSON array") {
        return None;
    }
    let lang = system.split(" into ").nth(1)?;
    let end = lang.find([';', '.', '\n']).unwrap_or(lang.len());
    let lang = lang[..end].trim().trim_end_matches(',').to_string();
    if lang.is_empty() || lang.chars().count() > 40 {
        return None;
    }
    let texts: Vec<String> = serde_json::from_str(user.as_str()?).ok()?;
    if texts.is_empty() {
        return None;
    }
    Some((lang, texts))
}

/// 从提示词里取出术语表（`glossary::prompt_block` 的格式：`- 原文 => 译法`，`(keep as is)` 表示保留原文）。
pub fn glossary_of(system: &str) -> Vec<(String, String)> {
    let Some(block) = system.split("\nGlossary").nth(1) else { return vec![] };
    block
        .lines()
        .filter_map(|l| l.strip_prefix("- "))
        .filter_map(|l| l.split_once(" => "))
        .map(|(s, t)| {
            let t = t.split("  (").next().unwrap_or(t).trim();
            let t = if t == "(keep as is)" { s.trim() } else { t };
            (s.trim().to_string(), t.to_string())
        })
        .filter(|(s, _)| !s.is_empty())
        .collect()
}

/// 机器翻译不认提示词：把术语换成占位符送去翻译，回来再换成规定的译法。返回处理后的文字和每段用到的（占位符, 译法）。
pub fn protect_terms(texts: &[String], terms: &[(String, String)]) -> (Vec<String>, Vec<Vec<(String, String)>>) {
    let mut out = vec![];
    let mut maps = vec![];
    for t in texts {
        let mut s = t.clone();
        let mut m = vec![];
        // 长的术语先换，免得短的把长的拆开
        let mut sorted: Vec<&(String, String)> = terms.iter().collect();
        sorted.sort_by_key(|(src, _)| std::cmp::Reverse(src.chars().count()));
        for (src, tgt) in sorted {
            let lower = s.to_lowercase();
            let needle = src.to_lowercase();
            if needle.is_empty() || !lower.contains(&needle) || lower.len() != s.len() {
                continue;
            }
            let ph = format!("{{T{}}}", m.len());
            let mut res = String::new();
            let mut rest = s.as_str();
            let mut rest_l = lower.as_str();
            while let Some(i) = rest_l.find(&needle) {
                res.push_str(&rest[..i]);
                res.push_str(&ph);
                rest = &rest[i + needle.len()..];
                rest_l = &rest_l[i + needle.len()..];
            }
            res.push_str(rest);
            s = res;
            m.push((ph, tgt.clone()));
        }
        out.push(s);
        maps.push(m);
    }
    (out, maps)
}

pub fn restore_terms(texts: Vec<String>, maps: &[Vec<(String, String)>]) -> Vec<String> {
    texts
        .into_iter()
        .zip(maps)
        .map(|(mut t, m)| {
            for (ph, tgt) in m {
                t = t.replace(ph.as_str(), tgt);
            }
            t
        })
        .collect()
}

fn strip_slash(s: &str) -> &str {
    s.trim_end_matches('/')
}

/// 填了完整地址就用，没填用官方地址。
fn base_or<'a>(base: &'a str, default: &'a str) -> &'a str {
    if base.trim().is_empty() {
        default
    } else {
        strip_slash(base.trim())
    }
}

pub(crate) fn http_error(kind: Kind, status: u16, body: &str) -> AppError {
    let mut e = crate::ai::api_error(status, body);
    if kind == Kind::DeepL && status == 456 {
        e = AppError::new(crate::error::ErrorKind::RateLimited, "DeepL 本月免费字数用完了（456）。");
    }
    if status == 403 && kind == Kind::Google {
        e = AppError::new(
            crate::error::ErrorKind::NeedLogin,
            format!("Google 翻译密钥无效，或没有开通 Cloud Translation API（403）：{}", body.chars().take(160).collect::<String>()),
        );
    }
    e
}

async fn send_json(client: &reqwest::Client, kind: Kind, req: reqwest::RequestBuilder) -> AppResult<Value> {
    let resp = req
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("无法连接{}：{e}", kind.label())))?;
    let _ = client;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("读取响应失败：{e}")))?;
    if !(200..300).contains(&status) {
        return Err(http_error(kind, status, &text));
    }
    serde_json::from_str(&text).map_err(|_| AppError::parser(format!("{} 返回的不是有效的 JSON。", kind.label())))
}

/// 一批文字译成目标语言；返回条数和输入一致。`base` 空 = 官方地址，`region` 只有微软用（Azure 资源所在区域）。
pub async fn translate(
    client: &reqwest::Client,
    kind: Kind,
    base: &str,
    key: Option<&str>,
    region: &str,
    lang: &str,
    texts: &[String],
) -> AppResult<Vec<String>> {
    let to = target_code(kind, lang)?;
    let need_key = || key.filter(|k| !k.is_empty()).ok_or_else(|| AppError::invalid(format!("请先填写 {} 的密钥。", kind.label())));
    let out: Vec<String> = match kind {
        Kind::Google => {
            let key = need_key()?;
            let url = format!("{}/language/translate/v2", base_or(base, "https://translation.googleapis.com"));
            let v = send_json(client, kind, client.post(url).query(&[("key", key)]).json(&json!({"q": texts, "target": to, "format": "text"}))).await?;
            v.pointer("/data/translations")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|t| html_unescape(t.get("translatedText").and_then(Value::as_str).unwrap_or(""))).collect())
                .ok_or_else(|| AppError::parser("Google 翻译没有返回译文。"))?
        }
        Kind::Microsoft => {
            let key = need_key()?;
            let url = format!("{}/translate", base_or(base, "https://api.cognitive.microsofttranslator.com"));
            let body: Vec<Value> = texts.iter().map(|t| json!({"Text": t})).collect();
            let mut req = client.post(url).query(&[("api-version", "3.0"), ("to", to)]).header("Ocp-Apim-Subscription-Key", key).json(&body);
            if !region.trim().is_empty() {
                req = req.header("Ocp-Apim-Subscription-Region", region.trim());
            }
            let v = send_json(client, kind, req).await?;
            v.as_array()
                .map(|a| a.iter().map(|t| t.pointer("/translations/0/text").and_then(Value::as_str).unwrap_or("").to_string()).collect())
                .ok_or_else(|| AppError::parser("微软翻译没有返回译文。"))?
        }
        Kind::DeepL => {
            let key = need_key()?;
            // 免费版密钥以 :fx 结尾，走 api-free；付费版走 api
            let default = if key.ends_with(":fx") { "https://api-free.deepl.com" } else { "https://api.deepl.com" };
            let url = format!("{}/v2/translate", base_or(base, default));
            let v = send_json(
                client,
                kind,
                client.post(url).header("Authorization", format!("DeepL-Auth-Key {key}")).json(&json!({"text": texts, "target_lang": to})),
            )
            .await?;
            v.get("translations")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|t| t.get("text").and_then(Value::as_str).unwrap_or("").to_string()).collect())
                .ok_or_else(|| AppError::parser("DeepL 没有返回译文。"))?
        }
        Kind::DeepLX => {
            // DeepLX 一次只译一段：地址填完整接口，如 http://127.0.0.1:1188/translate
            if base.trim().is_empty() {
                return Err(AppError::invalid("DeepLX 需要填接口地址，例如 http://127.0.0.1:1188/translate。"));
            }
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                let mut req = client.post(base.trim()).json(&json!({"text": t, "source_lang": "auto", "target_lang": to.split('-').next().unwrap_or(to)}));
                if let Some(k) = key.filter(|k| !k.is_empty()) {
                    req = req.header("Authorization", format!("Bearer {k}"));
                }
                let v = send_json(client, kind, req).await?;
                let tr = v.get("data").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| {
                    AppError::parser(format!("DeepLX 没有返回译文：{}", v.get("message").and_then(Value::as_str).unwrap_or("请检查接口地址")))
                })?;
                out.push(tr.to_string());
            }
            out
        }
        Kind::Baidu => {
            let (appid, secret) = need_pair(key, kind)?;
            let url = format!("{}/api/trans/vip/translate", base_or(base, "https://fanyi-api.baidu.com"));
            let mut out = Vec::with_capacity(texts.len());
            // 能用换行拼在一起的就一批译（省请求；免费版每秒 1 次）：一批不超过 1500 个字，且每段里没有换行
            for batch in baidu_batches(texts) {
                let q = batch.iter().map(|i| texts[*i].as_str()).collect::<Vec<_>>().join("\n");
                let salt = salt();
                let sign = md5_hex(&format!("{appid}{q}{salt}{secret}"));
                let v = send_json(
                    client,
                    kind,
                    client.post(&url).form(&[
                        ("q", q.as_str()),
                        ("from", "auto"),
                        ("to", to),
                        ("appid", appid),
                        ("salt", salt.as_str()),
                        ("sign", sign.as_str()),
                    ]),
                )
                .await?;
                if let Some(code) = v.get("error_code").and_then(Value::as_str) {
                    return Err(baidu_error(code, v.get("error_msg").and_then(Value::as_str).unwrap_or("")));
                }
                let got: Vec<String> = v
                    .get("trans_result")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|t| t.get("dst").and_then(Value::as_str).unwrap_or("").to_string()).collect())
                    .unwrap_or_default();
                if got.len() != batch.len() {
                    return Err(AppError::parser("百度翻译返回的条数不对。"));
                }
                out.extend(got);
            }
            out
        }
        Kind::Youdao => {
            let (appid, secret) = need_pair(key, kind)?;
            let url = format!("{}/api", base_or(base, "https://openapi.youdao.com"));
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                let salt = salt();
                let curtime = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()).to_string();
                let sign = youdao_sign(appid, t, &salt, &curtime, secret);
                let v = send_json(
                    client,
                    kind,
                    client.post(&url).form(&[
                        ("q", t.as_str()),
                        ("from", "auto"),
                        ("to", to),
                        ("appKey", appid),
                        ("salt", salt.as_str()),
                        ("sign", sign.as_str()),
                        ("signType", "v3"),
                        ("curtime", curtime.as_str()),
                    ]),
                )
                .await?;
                let code = v.get("errorCode").and_then(Value::as_str).unwrap_or("");
                if code != "0" {
                    return Err(AppError::msg(format!("有道翻译出错（{code}）：{}", youdao_error(code))));
                }
                let tr = v.pointer("/translation/0").and_then(Value::as_str).ok_or_else(|| AppError::parser("有道翻译没有返回译文。"))?;
                out.push(tr.to_string());
            }
            out
        }
        Kind::Caiyun => {
            let token = need_key()?;
            let url = format!("{}/v1/translator", base_or(base, "https://api.interpreter.caiyunai.com"));
            let v = send_json(
                client,
                kind,
                client
                    .post(url)
                    .header("X-Authorization", format!("token {token}"))
                    .json(&json!({"source": texts, "trans_type": to, "request_id": "clearclip", "detect": true})),
            )
            .await?;
            v.get("target")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect())
                .ok_or_else(|| AppError::parser(format!("彩云小译没有返回译文：{}", v.get("message").and_then(Value::as_str).unwrap_or(""))))?
        }
        Kind::Niutrans => {
            let apikey = need_key()?;
            let url = format!("{}/NiuTransServer/translation", base_or(base, "https://api.niutrans.com"));
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                let v = send_json(client, kind, client.post(&url).form(&[("from", "auto"), ("to", to), ("apikey", apikey), ("src_text", t.as_str())])).await?;
                if let Some(code) = v.get("error_code").and_then(Value::as_str) {
                    return Err(AppError::msg(format!("小牛翻译出错（{code}）：{}", v.get("error_msg").and_then(Value::as_str).unwrap_or(""))));
                }
                let tr = v.get("tgt_text").and_then(Value::as_str).ok_or_else(|| AppError::parser("小牛翻译没有返回译文。"))?;
                out.push(tr.to_string());
            }
            out
        }
        Kind::Tencent => {
            let (id, secret) = need_pair(key, kind)?;
            let (host, url) = cloud_host(base, "https://tmt.tencentcloudapi.com");
            let mut out = Vec::with_capacity(texts.len());
            for batch in char_batches(texts, 1500, 40) {
                let payload =
                    json!({"Source": "auto", "Target": to, "ProjectId": 0, "SourceTextList": batch.iter().map(|i| texts[*i].as_str()).collect::<Vec<_>>()})
                        .to_string();
                let headers =
                    crate::mt_cloud::tencent_headers(id, secret, unix_now(), &host, "tmt", "TextTranslateBatch", "2018-03-21", "ap-guangzhou", &payload);
                let v = send_json(client, kind, with_headers(client.post(&url), &headers).body(payload)).await?;
                if let Some(e) = v.pointer("/Response/Error") {
                    return Err(AppError::msg(format!("腾讯翻译出错（{}）：{}", e["Code"].as_str().unwrap_or(""), e["Message"].as_str().unwrap_or(""))));
                }
                let got: Vec<String> = v
                    .pointer("/Response/TargetTextList")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect())
                    .ok_or_else(|| AppError::parser("腾讯翻译没有返回译文。"))?;
                if got.len() != batch.len() {
                    return Err(AppError::parser("腾讯翻译返回的条数不对。"));
                }
                out.extend(got);
            }
            out
        }
        Kind::Aliyun => {
            let (id, secret) = need_pair(key, kind)?;
            let (host, url) = cloud_host(base, "https://mt.cn-hangzhou.aliyuncs.com");
            let mut out = Vec::with_capacity(texts.len());
            for t in texts {
                // 一次译一段（上限 5000 字，超过的截断）
                let text: String = t.chars().take(5000).collect();
                let q = [("FormatType", "text"), ("Scene", "general"), ("SourceLanguage", "auto"), ("TargetLanguage", to), ("SourceText", text.as_str())];
                let nonce = format!("{}{}", unix_now(), salt());
                let headers = crate::mt_cloud::aliyun_headers(id, secret, unix_now(), &nonce, &host, "TranslateGeneral", "2018-10-12", &q);
                let v = send_json(client, kind, with_headers(client.post(&url).query(&q), &headers)).await?;
                let code = v.get("Code").map(|c| c.as_str().map(str::to_string).unwrap_or_else(|| c.to_string())).unwrap_or_default();
                if code != "200" {
                    return Err(AppError::msg(format!("阿里翻译出错（{code}）：{}", v.get("Message").and_then(Value::as_str).unwrap_or(""))));
                }
                let tr = v.pointer("/Data/Translated").and_then(Value::as_str).ok_or_else(|| AppError::parser("阿里翻译没有返回译文。"))?;
                out.push(tr.to_string());
            }
            out
        }
        Kind::Volcano => {
            let (ak, sk) = need_pair(key, kind)?;
            let (host, url) = cloud_host(base, "https://translate.volcengineapi.com");
            let query = "Action=TranslateText&Version=2020-06-01";
            let mut out = Vec::with_capacity(texts.len());
            for batch in char_batches(texts, 4000, 16) {
                let payload = json!({"TargetLanguage": to, "TextList": batch.iter().map(|i| texts[*i].as_str()).collect::<Vec<_>>()}).to_string();
                let headers = crate::mt_cloud::volc_headers(ak, sk, unix_now(), &host, "cn-north-1", "translate", query, &payload);
                let v = send_json(client, kind, with_headers(client.post(format!("{url}/?{query}")), &headers).body(payload)).await?;
                if let Some(e) = v.pointer("/ResponseMetadata/Error") {
                    return Err(AppError::msg(format!("火山翻译出错（{}）：{}", e["Code"].as_str().unwrap_or(""), e["Message"].as_str().unwrap_or(""))));
                }
                let got: Vec<String> = v
                    .get("TranslationList")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|t| t.get("Translation").and_then(Value::as_str).unwrap_or("").to_string()).collect())
                    .ok_or_else(|| AppError::parser("火山翻译没有返回译文。"))?;
                if got.len() != batch.len() {
                    return Err(AppError::parser("火山翻译返回的条数不对。"));
                }
                out.extend(got);
            }
            out
        }
        Kind::Chat => return Err(AppError::invalid("不是机器翻译服务。")),
    };
    if out.len() != texts.len() {
        return Err(AppError::parser(format!("{} 返回的条数不对（要 {}，给了 {}）。", kind.label(), texts.len(), out.len())));
    }
    Ok(out)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// 给请求加上一组请求头。
fn with_headers(mut req: reqwest::RequestBuilder, headers: &[(String, String)]) -> reqwest::RequestBuilder {
    for (k, v) in headers {
        req = req.header(k.as_str(), v.as_str());
    }
    req
}

/// 接口地址 → (主机名, 去掉结尾斜杠的地址)。地址栏留空用官方的。
fn cloud_host(base: &str, default: &str) -> (String, String) {
    let url = base_or(base, default).trim_end_matches('/').to_string();
    let host = reqwest::Url::parse(&url).ok().and_then(|u| u.host_str().map(|h| h.to_string())).unwrap_or_default();
    (host, url)
}

/// 把一批文字按“总字数不超过 max_chars、条数不超过 max_items”分成几组（返回每组里的下标）；单条超长的自己一组。
pub fn char_batches(texts: &[String], max_chars: usize, max_items: usize) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = vec![];
    let mut cur: Vec<usize> = vec![];
    let mut size = 0usize;
    for (i, t) in texts.iter().enumerate() {
        let n = t.chars().count();
        if !cur.is_empty() && (size + n > max_chars || cur.len() >= max_items) {
            out.push(std::mem::take(&mut cur));
            size = 0;
        }
        cur.push(i);
        size += n;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// “应用ID:密钥”拆成两段（百度、有道、腾讯、阿里、火山）。
fn need_pair(key: Option<&str>, kind: Kind) -> AppResult<(&str, &str)> {
    key.and_then(|k| k.split_once(':'))
        .map(|(a, b)| (a.trim(), b.trim()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .ok_or_else(|| AppError::invalid(format!("{} 的密钥栏要填“应用ID:密钥”（用英文冒号连起来）。", kind.label())))
}

fn salt() -> String {
    format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos()))
}

fn md5_hex(s: &str) -> String {
    use md5::{Digest, Md5};
    hex::encode(Md5::digest(s.as_bytes()))
}

/// 有道 v3 签名：sha256(应用ID + input + salt + curtime + 应用密钥)，input 是原文（超过 20 个字取前 10 个 + 总长度 + 后 10 个）。
pub fn youdao_sign(appid: &str, q: &str, salt: &str, curtime: &str, secret: &str) -> String {
    use sha2::{Digest, Sha256};
    let chars: Vec<char> = q.chars().collect();
    let input: String = if chars.len() <= 20 {
        q.to_string()
    } else {
        format!("{}{}{}", chars[..10].iter().collect::<String>(), chars.len(), chars[chars.len() - 10..].iter().collect::<String>())
    };
    hex::encode(Sha256::digest(format!("{appid}{input}{salt}{curtime}{secret}").as_bytes()))
}

/// 百度一批最多 1500 个字、每段不含换行；含换行的段单独一批。返回每批里文字的序号。
pub fn baidu_batches(texts: &[String]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = vec![];
    let mut cur: Vec<usize> = vec![];
    let mut n = 0usize;
    for (i, t) in texts.iter().enumerate() {
        let len = t.chars().count();
        if t.contains('\n') || len > 1500 {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                n = 0;
            }
            out.push(vec![i]);
            continue;
        }
        if n + len > 1500 && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            n = 0;
        }
        cur.push(i);
        n += len + 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn baidu_error(code: &str, msg: &str) -> AppError {
    let hint = match code {
        "52003" => "应用ID 或密钥不对",
        "54003" => "请求太频繁（免费版每秒 1 次），稍后再试",
        "54004" => "账户余额不足",
        "58001" => "不支持这个目标语言",
        "54001" => "签名错误，请检查密钥",
        _ => msg,
    };
    AppError::msg(format!("百度翻译出错（{code}）：{hint}"))
}

fn youdao_error(code: &str) -> &'static str {
    match code {
        "108" => "应用ID 不对",
        "202" => "签名失败，请检查应用密钥",
        "206" => "时间戳不对，请校准电脑时间",
        "207" => "请求重复",
        "401" => "账户欠费",
        "411" => "访问太频繁",
        "102" => "不支持的语言",
        _ => "见有道智云文档",
    }
}

/// Google v2 即使 `format=text` 也会把 `'` `&` 等写成 HTML 实体。
fn html_unescape(s: &str) -> String {
    s.replace("&#39;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_target_language_out_of_our_translation_prompts() {
        let sys = crate::ai::translate_system("简体中文");
        let (lang, texts) = translate_request(&sys, &json!("[\"Hello\",\"World\"]")).unwrap();
        assert_eq!(lang, "简体中文");
        assert_eq!(texts, ["Hello", "World"]);
        let web = crate::vsum::webtrans_system("English");
        assert_eq!(translate_request(&web, &json!("[\"你好\"]")).unwrap().0, "English");
        // 术语提取、摘要、看图都不是“译一批文字”，机器翻译不接
        assert!(translate_request("You extract terms. Reply with a JSON array.", &json!("some page text")).is_none());
        assert!(translate_request(&sys, &json!([{"type": "image_url"}])).is_none());
        assert!(translate_request("Summarize the video", &json!("[\"x\"]")).is_none());
    }

    #[test]
    fn maps_language_names_to_each_providers_code() {
        assert_eq!(target_code(Kind::Google, "简体中文").unwrap(), "zh-CN");
        assert_eq!(target_code(Kind::Microsoft, "简体中文").unwrap(), "zh-Hans");
        assert_eq!(target_code(Kind::DeepL, "简体中文").unwrap(), "ZH-HANS");
        assert_eq!(target_code(Kind::Microsoft, "繁体中文").unwrap(), "zh-Hant");
        assert_eq!(target_code(Kind::DeepL, "English").unwrap(), "EN-US");
        assert_eq!(target_code(Kind::Google, "日语").unwrap(), "ja");
        assert!(target_code(Kind::DeepL, "泰语").is_err(), "DeepL 不支持泰语");
        assert!(target_code(Kind::Google, "克林贡语").is_err());
    }

    #[test]
    fn parses_kinds() {
        assert_eq!(Kind::parse("deepl"), Kind::DeepL);
        assert_eq!(Kind::parse(""), Kind::Chat);
        assert!(Kind::parse("google").is_mt());
        assert!(!Kind::parse("openai").is_mt());
    }

    #[test]
    fn glossary_terms_survive_machine_translation_via_placeholders() {
        let terms = [crate::glossary::Term { source: "Rust".into(), target: "锈语言".into(), ..Default::default() }];
        let refs: Vec<&crate::glossary::Term> = terms.iter().collect();
        let sys = format!("Translate into 简体中文. JSON array{}", crate::glossary::prompt_block(&refs));
        let g = glossary_of(&sys);
        assert_eq!(g, vec![("Rust".to_string(), "锈语言".to_string())]);
        let (texts, maps) = protect_terms(&["I love rust and RUST.".to_string(), "nothing".to_string()], &g);
        assert_eq!(texts, ["I love {T0} and {T0}.", "nothing"]);
        // 机器翻译把占位符原样带回来
        let back = restore_terms(vec!["我爱 {T0} 和 {T0}。".into(), "无".into()], &maps);
        assert_eq!(back, ["我爱 锈语言 和 锈语言。", "无"]);
    }

    #[test]
    fn unescapes_google_entities() {
        assert_eq!(html_unescape("It&#39;s &quot;ok&quot; &amp; fine"), "It's \"ok\" & fine");
    }
}
