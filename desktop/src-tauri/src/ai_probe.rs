//! 添加服务时检测可用模型：读取服务的模型列表，按名称和返回字段标出能力，并能对单个模型做一次极短的请求。

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::ai::{self, Endpoint};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::{ai_pool, AppState};

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    /// chat / vision / stt / embed / image
    pub caps: Vec<String>,
    pub free: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    pub ms: u64,
    /// 成功时是模型的回复，失败时是原因
    pub text: String,
    /// 带图片的请求也通过了
    pub vision: Option<bool>,
}

fn has(id: &str, words: &[&str]) -> bool {
    words.iter().any(|w| id.contains(w))
}

/// 按模型名（和接口返回的附加字段）推断能力。只是提示，最终以“测试”为准。
pub fn classify(id: &str, extra: &Value) -> Vec<String> {
    let l = id.to_lowercase();
    let mut caps = vec![];
    if has(&l, &["embed", "bge-", "rerank", "text-embedding", "gte-"]) {
        caps.push("embed");
    } else if has(&l, &["whisper", "sensevoice", "paraformer", "-asr", "asr-", "speech-to-text", "transcribe"]) {
        caps.push("stt");
    } else if has(&l, &["flux", "stable-diffusion", "sdxl", "dall-e", "kolors", "cogview", "midjourney", "-image", "wan2", "video-gen"])
        || l.starts_with("sd") && l.contains("diffusion")
    {
        caps.push("image");
    } else if has(&l, &["tts", "cosyvoice", "fish-speech", "text-to-speech"]) {
        caps.push("tts");
    } else {
        caps.push("chat");
        let by_field = extra.pointer("/architecture/input_modalities").and_then(Value::as_array).is_some_and(|a| a.iter().any(|m| m.as_str() == Some("image")))
            || extra.pointer("/capabilities/vision").and_then(Value::as_bool) == Some(true);
        if by_field
            || has(
                &l,
                &[
                    "-vl",
                    "vl-",
                    "vision",
                    "gpt-4o",
                    "gpt-4.1",
                    "gpt-5",
                    "gemini",
                    "claude",
                    "llava",
                    "pixtral",
                    "internvl",
                    "glm-4v",
                    "glm-4.5v",
                    "minicpm-v",
                    "omni",
                    "qvq",
                ],
            )
        {
            caps.push("vision");
        }
    }
    caps.into_iter().map(String::from).collect()
}

fn is_free(id: &str, extra: &Value) -> bool {
    let l = id.to_lowercase();
    l.ends_with(":free") || l.contains("-free") || extra.pointer("/pricing/prompt").and_then(Value::as_str) == Some("0")
}

/// 解析各家的模型列表：OpenAI 兼容 `{data:[{id}]}`、Gemini `{models:[{name:"models/x"}]}`、直接数组。
pub fn parse_models(body: &str) -> Vec<ModelInfo> {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return vec![] };
    let list: Vec<&Value> = if let Some(a) = v.get("data").and_then(Value::as_array) {
        a.iter().collect()
    } else if let Some(a) = v.get("models").and_then(Value::as_array) {
        a.iter().collect()
    } else if let Some(a) = v.as_array() {
        a.iter().collect()
    } else {
        vec![]
    };
    let mut out: Vec<ModelInfo> = list
        .into_iter()
        .filter_map(|m| {
            let id = m.get("id").or_else(|| m.get("name")).and_then(Value::as_str).or_else(|| m.as_str())?;
            let id = id.strip_prefix("models/").unwrap_or(id).to_string();
            Some(ModelInfo { caps: classify(&id, m), free: is_free(&id, m), id })
        })
        .collect();
    // Gemini 会声明支持的方法：不支持 generateContent 的（嵌入等）已由名字归类
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

fn endpoint(st: &AppState, base: &str, key: Option<String>) -> AppResult<Endpoint> {
    let base = base.trim().trim_end_matches('/').to_string();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(AppError::invalid("请先填写接口地址（以 http:// 或 https:// 开头）。"));
    }
    let key = key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    if key.is_none() && !ai::is_local_url(&base) {
        return Err(AppError::invalid("请先填写 API 密钥（本机服务可以不填）。"));
    }
    let settings = st.settings();
    let client = st.net.clients_for(&settings.network, &base)?.download;
    Ok(Endpoint { base, key, client, retry_ms: 500, attempts: 1, cands: vec![], pool: None })
}

fn resolve_key(st: &AppState, entry_id: &str, typed: Option<String>) -> Option<String> {
    typed.filter(|k| !k.trim().is_empty()).or_else(|| if entry_id.is_empty() { None } else { st.vault.get(&ai_pool::key_name(entry_id)) })
}

/// 读取服务的模型列表。`api_key` 留空时用这个条目已保存的密钥。
#[tauri::command]
pub async fn ai_probe_models(state: St<'_>, entry_id: Option<String>, base_url: String, api_key: Option<String>) -> AppResult<Vec<ModelInfo>> {
    let key = resolve_key(&state, entry_id.as_deref().unwrap_or(""), api_key);
    let ep = endpoint(&state, &base_url, key)?;
    let mut req = ep.client.get(format!("{}/models", ep.base)).timeout(Duration::from_secs(20));
    if let Some(k) = &ep.key {
        req = req.header("Authorization", format!("Bearer {k}")).header("x-goog-api-key", k.as_str());
    }
    let resp = req.send().await.map_err(|e| AppError::new(ErrorKind::Network, format!("无法连接接口：{e}")))?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(|e| AppError::new(ErrorKind::Network, format!("读取响应失败：{e}")))?;
    if !(200..300).contains(&status) {
        if status == 404 {
            return Err(AppError::invalid("这个服务没有提供模型列表接口（404）。请手动填写模型名，然后用“测试”确认。"));
        }
        return Err(ai::api_error(status, &text));
    }
    let list = parse_models(&text);
    if list.is_empty() {
        return Err(AppError::parser("服务返回了内容，但里面没有模型列表。请手动填写模型名。"));
    }
    Ok(list)
}

/// 1×1 的 PNG，用来试探模型能不能接受图片。
const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/// 用指定模型发一条极短的请求；`check_vision` 时再带一张图试一次。
#[tauri::command]
pub async fn ai_probe_test(
    state: St<'_>,
    entry_id: Option<String>,
    base_url: String,
    api_key: Option<String>,
    model: String,
    check_vision: Option<bool>,
) -> AppResult<ProbeResult> {
    let key = resolve_key(&state, entry_id.as_deref().unwrap_or(""), api_key);
    let ep = endpoint(&state, &base_url, key)?;
    let started = Instant::now();
    let r = ai::chat(&ep, model.trim(), "You are a connectivity test. Answer with the single word: OK", "ping").await;
    let ms = started.elapsed().as_millis() as u64;
    match r {
        Err(e) => Ok(ProbeResult { ok: false, ms, text: e.to_string(), vision: None }),
        Ok(reply) => {
            let vision = if check_vision.unwrap_or(false) {
                Some(ai::chat_img(&ep, model.trim(), "Reply with the single word: OK", "What is this?", PIXEL).await.is_ok())
            } else {
                None
            };
            Ok(ProbeResult { ok: true, ms, text: reply.chars().take(40).collect(), vision })
        }
    }
}

/// 把一个条目已保存的密钥复制给另一个条目（同一个服务下加多个模型时用）。
#[tauri::command]
pub fn ai_pool_copy_key(state: St<'_>, from: String, to: String) -> AppResult<()> {
    let key = state.vault.get(&ai_pool::key_name(&from)).unwrap_or_default();
    state.vault.set(&ai_pool::key_name(&to), &key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openai_list_is_parsed_and_classified() {
        let body = r#"{"object":"list","data":[{"id":"Qwen/Qwen2.5-7B-Instruct"},{"id":"Qwen/Qwen2.5-VL-7B-Instruct"},{"id":"BAAI/bge-m3"},{"id":"FunAudioLLM/SenseVoiceSmall"},{"id":"black-forest-labs/FLUX.1-dev"},{"id":"Qwen/Qwen2.5-7B-Instruct"},{"id":"meta-llama/llama-3.3-70b-instruct:free"}]}"#;
        let m = parse_models(body);
        assert_eq!(m.len(), 6, "重复的只留一个");
        let get = |id: &str| m.iter().find(|x| x.id == id).unwrap();
        assert_eq!(get("Qwen/Qwen2.5-7B-Instruct").caps, ["chat"]);
        assert_eq!(get("Qwen/Qwen2.5-VL-7B-Instruct").caps, ["chat", "vision"]);
        assert_eq!(get("BAAI/bge-m3").caps, ["embed"]);
        assert_eq!(get("FunAudioLLM/SenseVoiceSmall").caps, ["stt"]);
        assert_eq!(get("black-forest-labs/FLUX.1-dev").caps, ["image"]);
        assert!(get("meta-llama/llama-3.3-70b-instruct:free").free);
    }

    #[test]
    fn gemini_and_plain_array_shapes() {
        let g = parse_models(r#"{"models":[{"name":"models/gemini-2.0-flash"},{"name":"models/text-embedding-004"}]}"#);
        assert_eq!(g[0].id, "gemini-2.0-flash");
        assert!(g[0].caps.contains(&"vision".to_string()));
        assert_eq!(g[1].caps, ["embed"]);
        let a = parse_models(r#"["a-model","b-model"]"#);
        assert_eq!(a.len(), 2);
        assert!(parse_models("not json").is_empty());
        assert!(parse_models(r#"{"error":"x"}"#).is_empty());
    }

    #[test]
    fn modalities_field_marks_vision() {
        let v = json!({"id":"x/some-model","architecture":{"input_modalities":["text","image"]}});
        assert!(classify("x/some-model", &v).contains(&"vision".to_string()));
        assert!(!classify("x/some-model", &json!({})).contains(&"vision".to_string()));
    }
}
