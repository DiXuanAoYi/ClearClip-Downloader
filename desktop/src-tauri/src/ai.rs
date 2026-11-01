//! AI 功能：字幕翻译、摘要与章节（兼容 OpenAI 的对话接口），语音转文字（兼容 OpenAI 的
//! `/audio/transcriptions`，或本机的 whisper.cpp）。API 密钥保存在加密的保险箱里。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use crate::ai_pool::{self, AiPool, Task};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::media_tools::JobCtx;
use crate::model::Chapter;
use crate::postprocess::{base_args, find_ffmpeg, run_ffmpeg_in};
use crate::settings::Settings;
use crate::subtitle::{self, Cue};
use crate::{subtitle_tools, AppState};

pub const KEY_CHAT: &str = "ai.api_key";
pub const KEY_STT: &str = "stt.api_key";

// ---------- 接口 ----------

pub struct Endpoint {
    pub base: String,
    pub key: Option<String>,
    pub client: reqwest::Client,
    /// 429 / 5xx / 网络错误时的重试间隔基数（毫秒）
    pub retry_ms: u64,
    /// 单个服务内最多请求几次（模型池里有备选时只试 1 次，失败马上换下一家）
    pub attempts: u32,
    /// 模型池挑出的候选服务，按顺序尝试；空 = 只用上面的 base / key
    pub cands: Vec<Candidate>,
    pub pool: Option<Arc<AiPool>>,
}

/// 模型池里的一个可用服务（已解析好密钥和网络客户端）。
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub base: String,
    pub key: Option<String>,
    pub model: String,
    pub client: reqwest::Client,
}

impl Endpoint {
    pub fn single(base: String, key: Option<String>, client: reqwest::Client, retry_ms: u64) -> Endpoint {
        Endpoint { base, key, client, retry_ms, attempts: 3, cands: vec![], pool: None }
    }
}

fn endpoint(st: &AppState, settings: &Settings, base: &str, key_name: &str) -> AppResult<Endpoint> {
    if base.is_empty() {
        return Err(AppError::invalid("请先在“设置 → AI”里填写接口地址。"));
    }
    let client = st.net.clients_for(&settings.network, base)?.download;
    Ok(Endpoint::single(base.to_string(), st.vault.get(key_name).filter(|k| !k.is_empty()), client, 2000))
}

/// 对话类任务用的接口（问答、卡片、笔记等）。
pub fn chat_endpoint(st: &AppState, settings: &Settings) -> AppResult<Endpoint> {
    chat_endpoint_for(st, settings, Task::Chat)
}

/// 按任务类型从模型池里挑服务。池是空的就退回到“默认”那一组设置，行为和以前一样。
pub fn chat_endpoint_for(st: &AppState, settings: &Settings, task: Task) -> AppResult<Endpoint> {
    let ai = &settings.ai;
    // 候选 = 池里的服务 + 原来的“默认”设置（排在最后）
    struct Row {
        id: String,
        name: String,
        base: String,
        model: String,
        key: Option<String>,
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut cands: Vec<ai_pool::Cand> = Vec::new();
    for e in &ai.pool {
        let key = st.vault.get(&ai_pool::key_name(&e.id)).filter(|k| !k.is_empty());
        let usable = !e.base_url.is_empty() && !e.model.is_empty() && (key.is_some() || is_local_url(&e.base_url));
        cands.push(ai_pool::from_entry(e, usable));
        rows.push(Row {
            id: e.id.clone(),
            name: if e.name.is_empty() { e.id.clone() } else { e.name.clone() },
            base: e.base_url.clone(),
            model: e.model.clone(),
            key,
        });
    }
    if !ai.base_url.is_empty() && !ai.model.is_empty() {
        let key = st.vault.get(KEY_CHAT).filter(|k| !k.is_empty());
        let usable = key.is_some() || is_local_url(&ai.base_url);
        cands.push(ai_pool::Cand { id: "default".into(), priority: 500, daily_limit: 0, tasks: vec![], usable });
        rows.push(Row { id: "default".into(), name: "默认".into(), base: ai.base_url.clone(), model: ai.model.clone(), key });
    }
    if ai.pool.is_empty() {
        // 没有用池：保持原来的行为和提示
        if ai.model.is_empty() {
            return Err(AppError::invalid("请先在“设置 → AI”里填写模型名称。"));
        }
        return endpoint(st, settings, &ai.base_url, KEY_CHAT);
    }
    let now = crate::db::now();
    let rt = st.ai_pool.snapshot(now);
    let order = ai_pool::rank(&cands, &rt, task, now);
    if order.is_empty() {
        let any_usable = cands.iter().any(|c| c.usable);
        return Err(match (any_usable, ai_pool::earliest_ready(&cands, &rt, task, now)) {
            (true, Some(t)) => {
                AppError::new(ErrorKind::RateLimited, format!("模型池里的服务暂时都不可用（额度用完或在冷却），最早 {} 秒后恢复。", (t - now).max(1)))
            }
            (true, None) => AppError::invalid("模型池里没有适用于这个任务的服务，请在“设置 → AI → 模型池”里检查“适用任务”。"),
            (false, _) => AppError::invalid("模型池里还没有可用的服务：请启用并填好 API 密钥。"),
        });
    }
    let mut out = Vec::new();
    for i in order {
        let r = &rows[i];
        let client = st.net.clients_for(&settings.network, &r.base)?.download;
        out.push(Candidate { id: r.id.clone(), name: r.name.clone(), base: r.base.clone(), key: r.key.clone(), model: r.model.clone(), client });
    }
    let attempts = if out.len() == 1 { 3 } else { 1 };
    let first = &out[0];
    Ok(Endpoint {
        base: first.base.clone(),
        key: first.key.clone(),
        client: first.client.clone(),
        retry_ms: 2000,
        attempts,
        cands: out,
        pool: Some(st.ai_pool.clone()),
    })
}

pub fn stt_endpoint(st: &AppState, settings: &Settings) -> AppResult<Endpoint> {
    let base = settings.ai.stt_url();
    // 语音转文字单独没有密钥时沿用对话接口的密钥（同一家服务商）
    let key_name = if st.vault.has(KEY_STT) { KEY_STT } else { KEY_CHAT };
    endpoint(st, settings, base, key_name)
}

/// 云端语音转文字：从模型池里挑出“适用任务”勾了 stt 且填了语音模型的服务，按优先级 / 今日用量排队，
/// 失败（额度、冷却、网络）自动换下一家。没有这样的服务返回 None，沿用原来的单一接口。
pub fn stt_pool_endpoint(st: &AppState, settings: &Settings) -> Option<Endpoint> {
    let mut cands: Vec<ai_pool::Cand> = Vec::new();
    let mut rows: Vec<Candidate> = Vec::new();
    for e in &settings.ai.pool {
        if e.stt_model.is_empty() || !e.tasks.iter().any(|t| t == "stt") || e.base_url.is_empty() {
            continue;
        }
        let key = st.vault.get(&ai_pool::key_name(&e.id)).filter(|k| !k.is_empty());
        let usable = key.is_some() || is_local_url(&e.base_url);
        let Ok(clients) = st.net.clients_for(&settings.network, &e.base_url) else { continue };
        cands.push(ai_pool::from_entry(e, usable));
        rows.push(Candidate {
            id: e.id.clone(),
            name: if e.name.is_empty() { e.id.clone() } else { e.name.clone() },
            base: e.base_url.clone(),
            key,
            model: e.stt_model.clone(),
            client: clients.download,
        });
    }
    if cands.is_empty() {
        return None;
    }
    let now = crate::db::now();
    let rt = st.ai_pool.snapshot(now);
    let order = ai_pool::rank(&cands, &rt, Task::Stt, now);
    let mut out: Vec<Candidate> = Vec::new();
    for i in order {
        let r = &rows[i];
        out.push(Candidate {
            id: r.id.clone(),
            name: r.name.clone(),
            base: r.base.clone(),
            key: r.key.clone(),
            model: r.model.clone(),
            client: r.client.clone(),
        });
    }
    let first = out.first()?;
    Some(Endpoint {
        base: first.base.clone(),
        key: first.key.clone(),
        client: first.client.clone(),
        retry_ms: 2000,
        attempts: 1,
        cands: out,
        pool: Some(st.ai_pool.clone()),
    })
}

/// 转写一段音频；有候选服务时依次尝试并记录成功 / 失败（决定冷却），否则用 `model` 直接调接口。
pub async fn transcribe_rotating(ep: &Endpoint, model: &str, language: &str, file: &Path) -> AppResult<String> {
    if ep.cands.is_empty() {
        return transcribe_chunk(ep, model, language, file).await;
    }
    let mut tried: Vec<String> = Vec::new();
    let mut last: Option<AppError> = None;
    for c in &ep.cands {
        let one = Endpoint {
            base: c.base.clone(),
            key: c.key.clone(),
            client: c.client.clone(),
            retry_ms: ep.retry_ms,
            attempts: ep.attempts,
            cands: vec![],
            pool: None,
        };
        match transcribe_chunk(&one, &c.model, language, file).await {
            Ok(t) => {
                if let Some(p) = &ep.pool {
                    p.record_ok(&c.id, crate::db::now());
                }
                return Ok(t);
            }
            Err(e) => {
                if let Some(p) = &ep.pool {
                    p.record_fail(&c.id, &e, crate::db::now());
                }
                tried.push(format!("{}：{}", c.name, e.message));
                last = Some(e);
            }
        }
    }
    let last = last.unwrap_or_else(|| AppError::msg("没有可用的语音转文字服务"));
    if tried.len() == 1 {
        return Err(last);
    }
    Err(AppError::new(last.kind, format!("{} 个语音转文字服务都失败了：{}", tried.len(), tried.join("；"))))
}

fn api_error(status: u16, body: &str) -> AppError {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message").or_else(|| v.get("message")).or_else(|| v.pointer("/error")).and_then(|m| m.as_str().map(str::to_string)))
        .unwrap_or_else(|| body.chars().take(200).collect());
    match status {
        401 | 403 => AppError::new(ErrorKind::NeedLogin, format!("API 密钥无效或没有权限（{status}）：{msg}")),
        404 => AppError::invalid(format!("接口地址或模型名称不对（404）：{msg}")),
        429 => AppError::new(ErrorKind::RateLimited, format!("请求太频繁或额度用完（429）：{msg}")),
        s if s >= 500 => AppError::new(ErrorKind::Network, format!("服务暂时不可用（{s}）：{msg}")),
        s => AppError::msg(format!("接口返回 {s}：{msg}")),
    }
}

pub(crate) async fn send(ep: &Endpoint, path: &str, content_type: &str, body: Vec<u8>, timeout: Duration) -> AppResult<String> {
    let url = format!("{}{path}", ep.base);
    let mut last: Option<AppError> = None;
    for attempt in 0..ep.attempts.max(1) {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(ep.retry_ms * (1 << (attempt - 1)))).await;
        }
        let mut req = ep.client.post(&url).header("Content-Type", content_type).timeout(timeout).body(body.clone());
        if let Some(k) = &ep.key {
            req = req.header("Authorization", format!("Bearer {k}"));
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let text = resp.text().await.map_err(|e| AppError::new(ErrorKind::Network, format!("读取响应失败：{e}")))?;
                if (200..300).contains(&status) {
                    return Ok(text);
                }
                let err = api_error(status, &text);
                if status == 429 || status >= 500 {
                    last = Some(err);
                    continue;
                }
                return Err(err);
            }
            Err(e) => last = Some(AppError::new(ErrorKind::Network, format!("无法连接接口：{e}"))),
        }
    }
    Err(last.unwrap_or_else(|| AppError::msg("请求失败")))
}

/// 只用池里指定的一个服务（测试按钮用）；`default` 表示原来的默认设置。
pub fn entry_endpoint(st: &AppState, settings: &Settings, id: &str) -> AppResult<Endpoint> {
    let ai = &settings.ai;
    let (name, base, model, key) = if id == "default" {
        ("默认".to_string(), ai.base_url.clone(), ai.model.clone(), st.vault.get(KEY_CHAT))
    } else {
        let e = ai.pool.iter().find(|e| e.id == id).ok_or_else(|| AppError::not_found("池里没有这个服务。"))?;
        (if e.name.is_empty() { e.id.clone() } else { e.name.clone() }, e.base_url.clone(), e.model.clone(), st.vault.get(&ai_pool::key_name(&e.id)))
    };
    if base.is_empty() || model.is_empty() {
        return Err(AppError::invalid("请先填写接口地址和模型名称。"));
    }
    let key = key.filter(|k| !k.is_empty());
    if key.is_none() && !is_local_url(&base) {
        return Err(AppError::invalid("请先填写 API 密钥。"));
    }
    let client = st.net.clients_for(&settings.network, &base)?.download;
    let cand = Candidate { id: id.to_string(), name, base: base.clone(), key: key.clone(), model, client: client.clone() };
    Ok(Endpoint { base, key, client, retry_ms: 1000, attempts: 1, cands: vec![cand], pool: Some(st.ai_pool.clone()) })
}

/// 一次对话，返回模型的回答文字。用了模型池时 `model` 被忽略，按候选服务各自的模型依次尝试。
pub async fn chat(ep: &Endpoint, model: &str, system: &str, user: &str) -> AppResult<String> {
    if ep.cands.is_empty() {
        return chat_one(ep, model, system, user).await;
    }
    let now = || crate::db::now();
    let mut tried: Vec<String> = Vec::new();
    let mut last: Option<AppError> = None;
    for c in &ep.cands {
        let one = Endpoint {
            base: c.base.clone(),
            key: c.key.clone(),
            client: c.client.clone(),
            retry_ms: ep.retry_ms,
            attempts: ep.attempts,
            cands: vec![],
            pool: None,
        };
        match chat_one(&one, &c.model, system, user).await {
            Ok(t) => {
                if let Some(p) = &ep.pool {
                    p.record_ok(&c.id, now());
                }
                return Ok(t);
            }
            Err(e) => {
                if let Some(p) = &ep.pool {
                    p.record_fail(&c.id, &e, now());
                }
                tried.push(format!("{}：{}", c.name, e.message));
                last = Some(e);
            }
        }
    }
    let last = last.unwrap_or_else(|| AppError::msg("没有可用的服务"));
    if tried.len() == 1 {
        return Err(last);
    }
    Err(AppError::new(last.kind, format!("模型池里的 {} 个服务都失败了：{}", tried.len(), tried.join("；"))))
}

async fn chat_one(ep: &Endpoint, model: &str, system: &str, user: &str) -> AppResult<String> {
    let body = json!({
        "model": model,
        "temperature": 0.2,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    });
    let text = send(ep, "/chat/completions", "application/json", serde_json::to_vec(&body)?, Duration::from_secs(180)).await?;
    let v: Value =
        serde_json::from_str(&text).map_err(|_| AppError::parser("接口返回的不是有效的 JSON。请确认接口地址是兼容 OpenAI 的（通常以 /v1 结尾）。"))?;
    v.pointer("/choices/0/message/content").and_then(Value::as_str).map(str::to_string).ok_or_else(|| AppError::parser("接口没有返回内容，请检查模型名称。"))
}

// ---------- 翻译 ----------

pub fn translate_system(lang: &str) -> String {
    format!(
        "You are a professional subtitle translator. Translate every subtitle line in the user's JSON array into {lang}. \
         Rules: (1) Reply with ONE JSON array only, no commentary and no code fences. (2) The array must have exactly the same number of \
         strings, in the same order. Never merge or split lines. (3) Keep names and terms consistent, use natural spoken language. \
         (4) A line break inside a subtitle is written as \\n and must stay as \\n."
    )
}

pub(crate) fn strip_fences(s: &str) -> &str {
    let t = s.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```JSON")).or_else(|| t.strip_prefix("```")).unwrap_or(t);
    t.strip_suffix("```").unwrap_or(t).trim()
}

/// 从模型回答里取出 `n` 条译文。回答里夹带说明文字、代码围栏、`{"translations": [...]}` 都能处理；数量不对返回 None。
pub fn parse_array(resp: &str, n: usize) -> Option<Vec<String>> {
    let t = strip_fences(resp);
    let arr: Vec<Value> = if let (Some(a), Some(b)) = (t.find('['), t.rfind(']')) {
        // 如果最外层是对象，里面的数组也在 [ ] 之间，取最外面的一对即可
        serde_json::from_str(&t[a..=b]).ok()?
    } else {
        return None;
    };
    let out: Vec<String> = arr
        .into_iter()
        .map(|v| match v {
            Value::String(s) => s,
            other => other.to_string(),
        })
        .collect();
    (out.len() == n).then_some(out)
}

pub fn reflect_system(lang: &str) -> String {
    format!(
        "You review subtitle translations into {lang}. The user sends a JSON array; each item has the form \"SOURCE: <original>\\nDRAFT: <current translation>\". \
         For each item return the best final translation: fix mistranslations, omissions, wrong terms, awkward or unnatural phrasing, and keep it concise \
         enough for a subtitle. If the draft is already good return it unchanged. Keep every number, name and proper noun consistent with SOURCE. \
         Reply with ONE JSON array of strings only (the final translations, without the SOURCE/DRAFT labels), the same length and order, no commentary, no code fences."
    )
}

/// 反思翻译的结果能不能采用：不能为空、数字要和草稿一致、长度不能差太多。
pub fn guard_reflect(draft: &str, improved: &str) -> bool {
    let (d, i) = (draft.trim(), improved.trim());
    if i.is_empty() {
        return false;
    }
    let digits = |s: &str| {
        let mut v: Vec<char> = s.chars().filter(char::is_ascii_digit).collect();
        v.sort_unstable();
        v
    };
    if digits(d) != digits(i) {
        return false;
    }
    let (a, b) = (d.chars().count().max(1) as f64, i.chars().count() as f64);
    (0.4..=2.5).contains(&(b / a))
}

pub struct Reflected {
    pub texts: Vec<Option<String>>,
    pub changed: usize,
    pub kept: usize,
}

fn strip_label(s: &str) -> &str {
    let t = s.trim();
    for p in ["DRAFT:", "FINAL:", "IMPROVED:"] {
        if let Some(r) = t.strip_prefix(p) {
            return r.trim();
        }
    }
    t
}

/// 反思翻译：把原文和第一遍译文一起交给模型，让它检查、润色；不合格的结果（空、数字变了、长度悬殊、请求失败）保留第一遍译文。
#[allow(clippy::too_many_arguments)]
pub async fn reflect_translations(
    ep: &Endpoint,
    model: &str,
    lang: &str,
    cues: &[Cue],
    drafts: &[Option<String>],
    batch: usize,
    terms: &[crate::glossary::Term],
    ctx: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Reflected> {
    let idx: Vec<usize> = (0..cues.len()).filter(|&i| drafts.get(i).is_some_and(|d| d.as_deref().is_some_and(|t| !t.trim().is_empty()))).collect();
    let items: Vec<Cue> = idx
        .iter()
        .map(|&i| Cue {
            start: cues[i].start,
            end: cues[i].end,
            lines: vec![format!("SOURCE: {}\nDRAFT: {}", cues[i].lines.join(" "), drafts[i].as_deref().unwrap_or(""))],
        })
        .collect();
    let mut texts: Vec<Option<String>> = drafts.to_vec();
    if items.is_empty() {
        return Ok(Reflected { texts, changed: 0, kept: 0 });
    }
    let got = map_cues_g(ep, model, &reflect_system(lang), &items, batch, terms, ctx).await?;
    let (mut changed, mut kept) = (0usize, 0usize);
    for (k, &i) in idx.iter().enumerate() {
        let draft = drafts[i].as_deref().unwrap_or("");
        match got.texts.get(k).and_then(|t| t.as_deref()).map(strip_label) {
            Some(imp) if guard_reflect(draft, imp) => {
                if imp != draft.trim() {
                    changed += 1;
                } else {
                    kept += 1;
                }
                texts[i] = Some(imp.to_string());
            }
            _ => kept += 1,
        }
    }
    Ok(Reflected { texts, changed, kept })
}

pub struct Translated {
    pub texts: Vec<Option<String>>,
    /// 翻译失败、保留原文的条数
    pub failed: usize,
}

/// 分批翻译。某一批回答的数量对不上就拆成两半重试，直到单条；单条还不行就保留原文。
/// 网络 / 密钥错误直接返回错误（继续试也没用）。
pub async fn translate_cues(
    ep: &Endpoint,
    model: &str,
    lang: &str,
    cues: &[Cue],
    batch: usize,
    ctx: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Translated> {
    translate_cues_g(ep, model, lang, cues, batch, &[], ctx).await
}

/// 同 `translate_cues`，并把术语库里出现在每一批文字中的术语写进提示词。
pub async fn translate_cues_g(
    ep: &Endpoint,
    model: &str,
    lang: &str,
    cues: &[Cue],
    batch: usize,
    terms: &[crate::glossary::Term],
    ctx: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Translated> {
    map_cues_g(ep, model, &translate_system(lang), cues, batch, terms, ctx).await
}

/// 同一套“一批进、一批出、对不上就拆”的流程，系统提示词由调用方给（翻译、顺句都用它）。
pub async fn map_cues(
    ep: &Endpoint,
    model: &str,
    system: &str,
    cues: &[Cue],
    batch: usize,
    ctx: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Translated> {
    map_cues_g(ep, model, system, cues, batch, &[], ctx).await
}

pub async fn map_cues_g(
    ep: &Endpoint,
    model: &str,
    system: &str,
    cues: &[Cue],
    batch: usize,
    terms: &[crate::glossary::Term],
    ctx: &(dyn Fn(usize, usize) -> AppResult<()> + Sync),
) -> AppResult<Translated> {
    let texts: Vec<String> = cues.iter().map(|c| c.lines.join("\n")).collect();
    let mut out: Vec<Option<String>> = vec![None; texts.len()];
    let mut queue: VecDeque<(usize, usize)> = (0..texts.len()).step_by(batch.max(1)).map(|s| (s, (s + batch.max(1)).min(texts.len()))).collect();
    let mut done = 0usize;
    let mut failed = 0usize;
    while let Some((a, b)) = queue.pop_front() {
        ctx(done, texts.len())?;
        let user = serde_json::to_string(&texts[a..b])?;
        let hit = crate::glossary::relevant(terms, &texts[a..b], crate::glossary::MAX_IN_PROMPT);
        let sys = if hit.is_empty() {
            std::borrow::Cow::Borrowed(system)
        } else {
            std::borrow::Cow::Owned(format!("{system}{}", crate::glossary::prompt_block(&hit)))
        };
        let resp = chat(ep, model, &sys, &user).await;
        let parsed = match resp {
            Ok(r) => parse_array(&r, b - a),
            Err(e) if matches!(e.kind, ErrorKind::NeedLogin | ErrorKind::Invalid | ErrorKind::RateLimited | ErrorKind::Network) => return Err(e),
            Err(_) => None,
        };
        match parsed {
            Some(v) => {
                for (i, t) in v.into_iter().enumerate() {
                    out[a + i] = Some(t);
                }
                done += b - a;
            }
            None if b - a > 1 => {
                let mid = a + (b - a) / 2;
                queue.push_front((mid, b));
                queue.push_front((a, mid));
            }
            None => {
                failed += 1;
                done += 1;
            }
        }
    }
    ctx(texts.len(), texts.len())?;
    Ok(Translated { texts: out, failed })
}

/// 把译文合回字幕。`bilingual` 时原文在上、译文在下；没翻译成功的条目保持原文。
pub fn apply_translation(cues: &[Cue], texts: &[Option<String>], bilingual: bool) -> Vec<Cue> {
    cues.iter()
        .zip(texts)
        .map(|(c, t)| {
            let lines = match t {
                Some(t) if bilingual => c.lines.iter().cloned().chain(t.split('\n').map(str::to_string)).collect(),
                Some(t) => t.split('\n').map(str::to_string).collect(),
                None => c.lines.clone(),
            };
            Cue { start: c.start, end: c.end, lines }
        })
        .collect()
}

/// 目标语言的文件名标记：`课.zh.srt`。认不出的用 `ai`。
pub fn lang_tag(target: &str) -> &'static str {
    let t = target.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["繁体", "繁體", "traditional", "zh-hant", "zh-tw"]) {
        "zh-Hant"
    } else if has(&["中文", "汉语", "漢語", "chinese", "zh"]) {
        "zh"
    } else if has(&["english", "英语", "英文", "en"]) {
        "en"
    } else if has(&["日语", "日本語", "日文", "japanese", "ja"]) {
        "ja"
    } else if has(&["韩语", "한국어", "韩文", "korean", "ko"]) {
        "ko"
    } else if has(&["法语", "french", "fr"]) {
        "fr"
    } else if has(&["德语", "german", "de"]) {
        "de"
    } else if has(&["西班牙", "spanish", "es"]) {
        "es"
    } else if has(&["俄语", "russian", "ru"]) {
        "ru"
    } else {
        "ai"
    }
}

/// 去掉字幕文件名末尾的语言标记（`课.en` → `课`）。
pub fn strip_lang_suffix(stem: &str) -> &str {
    match stem.rsplit_once('.') {
        Some((head, tail)) if !head.is_empty() && tail.len() <= 8 && tail.chars().all(|c| c.is_ascii_alphabetic() || c == '-') && !tail.is_empty() => head,
        _ => stem,
    }
}

// ---------- 摘要与章节 ----------

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub summary: String,
    pub points: Vec<String>,
    pub chapters: Vec<Chapter>,
}

pub(crate) fn clock_label(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// `[0:12] 文字` 一行一条，给模型看带时间的文稿。
pub fn transcript_lines(cues: &[Cue]) -> Vec<String> {
    cues.iter().map(|c| format!("[{}] {}", clock_label(c.start), c.lines.join(" "))).collect()
}

/// 把文稿按字数切块，不拆开单行。
pub fn chunk_lines(lines: &[String], max_chars: usize) -> Vec<String> {
    let mut chunks = vec![];
    let mut cur = String::new();
    for l in lines {
        if !cur.is_empty() && cur.chars().count() + l.chars().count() + 1 > max_chars {
            chunks.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(l);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// `1:23` / `01:02:03` / `83` → 毫秒。
pub fn parse_label(s: &str) -> Option<u64> {
    let s = s.trim().trim_matches(['[', ']']);
    let nums: Vec<u64> = s.split(':').map(|p| p.trim().split('.').next().unwrap_or("").parse().ok()).collect::<Option<_>>()?;
    let secs = match nums.as_slice() {
        [s] => *s,
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(secs * 1000)
}

pub fn parse_summary(resp: &str) -> Option<Summary> {
    let t = strip_fences(resp);
    let (a, b) = (t.find('{')?, t.rfind('}')?);
    let v: Value = serde_json::from_str(&t[a..=b]).ok()?;
    let summary = v.get("summary").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let points: Vec<String> = v
        .get("points")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|p| p.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut chapters: Vec<Chapter> = v
        .get("chapters")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let start = c.get("time").and_then(|t| t.as_str().and_then(parse_label).or_else(|| t.as_u64().map(|n| n * 1000)))?;
                    let title = c.get("title").and_then(Value::as_str)?.trim().to_string();
                    (!title.is_empty()).then_some(Chapter { title, start_ms: start, end_ms: start })
                })
                .collect()
        })
        .unwrap_or_default();
    chapters.sort_by_key(|c| c.start_ms);
    chapters.dedup_by_key(|c| c.start_ms);
    if summary.is_empty() && points.is_empty() && chapters.is_empty() {
        return None;
    }
    Some(Summary { summary, points, chapters })
}

fn summary_system(lang: &str, final_pass: bool) -> String {
    let chapters = if final_pass { "" } else { " and \"chapters\" (array of {\"time\": \"m:ss\" or \"h:mm:ss\", \"title\": short title})" };
    format!(
        "You summarize video transcripts. The transcript lines look like \"[m:ss] text\". Write in {lang}. Reply with ONE JSON object only, no code fences, \
         with keys \"summary\" (2-4 sentences), \"points\" (array of 3-8 key takeaways){chapters}. \
         Chapters must use timestamps that appear in the transcript, start near the beginning, be in order and be 4-12 in number for long videos (fewer for short ones)."
    )
}

/// 摘要、要点和章节。长文稿分块处理后再合并。
pub async fn summarize(ep: &Endpoint, model: &str, lang: &str, cues: &[Cue], progress: &(dyn Fn(usize, usize) -> AppResult<()> + Sync)) -> AppResult<Summary> {
    let lines = transcript_lines(cues);
    let chunks = chunk_lines(&lines, 12_000);
    if chunks.is_empty() {
        return Err(AppError::invalid("字幕里没有内容。"));
    }
    let system = summary_system(lang, false);
    let mut parts: Vec<Summary> = vec![];
    for (i, c) in chunks.iter().enumerate() {
        progress(i, chunks.len() + 1)?;
        let resp = chat(ep, model, &system, c).await?;
        parts.push(parse_summary(&resp).ok_or_else(|| AppError::parser("模型没有按要求返回摘要，请换一个模型或稍后重试。"))?);
    }
    if parts.len() == 1 {
        progress(1, 1)?;
        return Ok(parts.remove(0));
    }
    // 合并：各块的摘要再总结一次，章节直接按时间拼接
    progress(chunks.len(), chunks.len() + 1)?;
    let joined = parts.iter().enumerate().map(|(i, p)| format!("Part {}: {}\n{}", i + 1, p.summary, p.points.join("; "))).collect::<Vec<_>>().join("\n\n");
    let resp = chat(ep, model, &summary_system(lang, true), &joined).await?;
    let merged = parse_summary(&resp).unwrap_or_default();
    let mut chapters: Vec<Chapter> = parts.into_iter().flat_map(|p| p.chapters).collect();
    chapters.sort_by_key(|c| c.start_ms);
    chapters.dedup_by_key(|c| c.start_ms);
    progress(chunks.len() + 1, chunks.len() + 1)?;
    Ok(Summary { summary: merged.summary, points: merged.points, chapters })
}

/// 补全每章的结束时间（下一章的开始；最后一章用总时长）。
pub fn close_chapters(mut chapters: Vec<Chapter>, total_ms: u64) -> Vec<Chapter> {
    for i in 0..chapters.len() {
        let end = chapters.get(i + 1).map(|n| n.start_ms).unwrap_or(total_ms).max(chapters[i].start_ms + 1);
        chapters[i].end_ms = end;
    }
    chapters
}

/// 视频平台描述里常见的章节写法：每行 `0:00 标题`。
pub fn chapters_text(chapters: &[Chapter]) -> String {
    chapters.iter().map(|c| format!("{} {}", clock_label(c.start_ms), c.title)).collect::<Vec<_>>().join("\n")
}

pub fn summary_markdown(title: &str, s: &Summary) -> String {
    let mut out = format!("# {title}\n\n");
    if !s.summary.is_empty() {
        out.push_str(&format!("## 摘要\n\n{}\n\n", s.summary));
    }
    if !s.points.is_empty() {
        out.push_str("## 要点\n\n");
        for p in &s.points {
            out.push_str(&format!("- {p}\n"));
        }
        out.push('\n');
    }
    if !s.chapters.is_empty() {
        out.push_str("## 章节\n\n");
        for c in &s.chapters {
            out.push_str(&format!("- {} {}\n", clock_label(c.start_ms), c.title));
        }
    }
    out
}

// ---------- 语音转文字 ----------

/// 手工拼 multipart/form-data（不需要额外的依赖）。
pub fn multipart(fields: &[(&str, &str)], file_field: &str, file_name: &str, mime: &str, bytes: &[u8], boundary: &str) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    for (k, v) in fields {
        out.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    out.extend(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
    );
    out.extend(bytes);
    out.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// 把音频切成每段 `seg_secs` 秒的小文件（单声道 16 kHz AAC，体积小，接口大小限制不会超）。返回（文件，起点毫秒）。
pub async fn split_audio(ffmpeg: &Path, input: &Path, dir: &Path, seg_secs: u64) -> AppResult<Vec<(PathBuf, u64)>> {
    std::fs::create_dir_all(dir)?;
    let mut a = base_args();
    a.extend([
        "-i".to_string(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-map".into(),
        "0:a:0".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "32k".into(),
        "-f".into(),
        "segment".into(),
        "-segment_time".into(),
        seg_secs.to_string(),
        "-segment_format".into(),
        "mp4".into(),
        "-reset_timestamps".into(),
        "1".into(),
        dir.join("part_%03d.m4a").to_string_lossy().into_owned(),
    ]);
    run_ffmpeg_in(ffmpeg, &a, None).await?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "m4a")).collect();
    files.sort();
    if files.is_empty() {
        return Err(AppError::invalid("这个文件里没有可以转写的音频。"));
    }
    Ok(files.into_iter().enumerate().map(|(i, p)| (p, i as u64 * seg_secs * 1000)).collect())
}

/// 调用 `/audio/transcriptions` 转写一段音频，返回 SRT 文字。
pub async fn transcribe_chunk(ep: &Endpoint, model: &str, language: &str, file: &Path) -> AppResult<String> {
    let bytes = std::fs::read(file)?;
    let boundary = format!("----clearclip{:x}", crate::db::now() as u64 ^ (bytes.len() as u64).rotate_left(17));
    let mut fields = vec![("model", model), ("response_format", "srt"), ("temperature", "0")];
    if !language.is_empty() {
        fields.push(("language", language));
    }
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "audio.m4a".into());
    let body = multipart(&fields, "file", &name, "audio/mp4", &bytes, &boundary);
    let text = send(ep, "/audio/transcriptions", &format!("multipart/form-data; boundary={boundary}"), body, Duration::from_secs(600)).await?;
    if text.contains("-->") {
        Ok(text)
    } else {
        Err(AppError::parser("接口没有返回字幕。请确认它支持 response_format=srt（OpenAI、Groq 等支持）。"))
    }
}

/// whisper.cpp 的命令行参数。
pub fn whisper_args(model: &str, wav: &Path, out_prefix: &Path, language: &str) -> Vec<String> {
    vec![
        "-m".into(),
        model.into(),
        "-f".into(),
        wav.to_string_lossy().into_owned(),
        "-l".into(),
        if language.is_empty() { "auto".into() } else { language.into() },
        "-osrt".into(),
        "-of".into(),
        out_prefix.to_string_lossy().into_owned(),
        "-pp".into(),
    ]
}

/// 这个 whisper-cli 支持 `--vad` 吗（较新的版本才有）。
async fn whisper_has_vad(bin: &Path) -> bool {
    let mut c = tokio::process::Command::new(bin);
    c.arg("--help").stdin(std::process::Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    match tokio::time::timeout(Duration::from_secs(8), c.output()).await {
        Ok(Ok(o)) => String::from_utf8_lossy(&o.stdout).contains("--vad") || String::from_utf8_lossy(&o.stderr).contains("--vad"),
        _ => false,
    }
}

/// whisper.cpp 输出里的进度（`progress =  35%`）。
pub fn whisper_progress(line: &str) -> Option<f64> {
    let rest = line.split("progress =").nth(1)?;
    rest.trim().trim_end_matches('%').trim().parse::<f64>().ok()
}

pub(crate) fn find_whisper_bin(settings: &Settings) -> Option<PathBuf> {
    let configured = settings.ai.whisper_bin.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return p.is_file().then_some(p);
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe"] } else { &[""] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["whisper-cli", "whisper-cpp", "main"] {
            for e in exts {
                let p = dir.join(format!("{name}{e}"));
                // “main” 太常见，只接受明确是 whisper.cpp 的（同目录有模型的情况不好判断，所以要求用户在设置里指定）
                if name != "main" && p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// 是否能找到本机的 whisper.cpp。
pub fn whisper_available(settings: &Settings) -> bool {
    find_whisper_bin(settings).is_some()
}

async fn run_whisper_local(
    ctx: &JobCtx,
    settings: &Settings,
    ffmpeg: &Path,
    input: &Path,
    language: &str,
    work: &Path,
    want_words: bool,
) -> AppResult<(String, Option<String>)> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let bin = find_whisper_bin(settings)
        .ok_or_else(|| AppError::invalid("找不到 whisper.cpp（whisper-cli）。请在“设置 → AI”里指定它的位置，或把它放进系统 PATH。"))?;
    let model = settings.ai.whisper_model.trim();
    if model.is_empty() || !Path::new(model).is_file() {
        return Err(AppError::invalid("请在“设置 → AI”里选择 whisper.cpp 的模型文件（ggml-*.bin）。"));
    }
    ctx.note("正在提取音频…");
    let wav = work.join("audio.wav");
    let mut a = base_args();
    a.extend([
        "-i".to_string(),
        input.to_string_lossy().into_owned(),
        "-vn".into(),
        "-ac".into(),
        "1".into(),
        "-ar".into(),
        "16000".into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        wav.to_string_lossy().into_owned(),
    ]);
    run_ffmpeg_in(ffmpeg, &a, None).await?;
    ctx.check()?;
    ctx.note("正在用本机 whisper.cpp 转写（第一次会比较慢）…");
    let prefix = work.join("out");
    let mut cmd = tokio::process::Command::new(&bin);
    let mut args = whisper_args(model, &wav, &prefix, language);
    if want_words {
        // 同时输出带逐词（token）时间的完整 JSON，用来把断句位置对到真实的词间停顿上
        args.push("-ojf".into());
    }
    let vad = settings.ai.whisper_vad_model.trim();
    if !vad.is_empty() && Path::new(vad).is_file() {
        if whisper_has_vad(&bin).await {
            args.extend(["--vad".to_string(), "--vad-model".to_string(), vad.to_string()]);
            ctx.note("已启用 VAD：只识别有人声的部分。");
        } else {
            ctx.note("这个版本的 whisper.cpp 不支持 VAD，已忽略 VAD 模型。");
        }
    }
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| AppError::new(ErrorKind::NeedUpdate, format!("无法运行 whisper.cpp：{e}")))?;
    let stderr = child.stderr.take();
    let reader = async {
        let mut tail = String::new();
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(p) = whisper_progress(&line) {
                    ctx.percent(p);
                } else {
                    tail = line;
                }
            }
        }
        tail
    };
    let cancelled = async {
        loop {
            if ctx.canceled() {
                return;
            }
            ctx.wake.notified().await;
        }
    };
    let status = tokio::select! {
        r = async { let tail = reader.await; (child.wait().await, tail) } => r,
        _ = cancelled => return Err(AppError::msg("canceled")),
    };
    match status {
        (Ok(s), tail) if s.success() => {
            // 不同版本 whisper.cpp 对 -of 的处理不一样：先找约定的 out.srt，再找工作目录里任意 .srt
            let want = prefix.with_extension("srt");
            let found = if want.is_file() {
                Some(want)
            } else {
                std::fs::read_dir(work).ok().and_then(|d| d.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "srt")))
            };
            let words_json = if want_words { std::fs::read_to_string(prefix.with_extension("json")).ok() } else { None };
            match found.and_then(|p| std::fs::read_to_string(p).ok()) {
                Some(t) => Ok((t, words_json)),
                None => Err(AppError::msg(format!(
                    "whisper.cpp 没有生成字幕文件（可能是模型文件不对或音频里没有可识别的语音）。程序输出：{}",
                    if tail.is_empty() { "（无）" } else { tail.as_str() }
                ))),
            }
        }
        (_, tail) => Err(AppError::msg(format!("whisper.cpp 运行失败：{tail}"))),
    }
}

/// 把若干段 SRT 按各自的起点偏移合并。
pub fn merge_srt_parts(parts: Vec<(String, u64)>) -> Vec<Cue> {
    let mut out: Vec<Cue> = vec![];
    for (text, offset) in parts {
        let cues = subtitle::parse_srt(&text);
        out.extend(subtitle_tools::shift(&cues, offset as i64));
    }
    out.sort_by_key(|c| c.start);
    out
}

// ---------- 下载后自动处理 ----------

fn is_local_url(url: &str) -> bool {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h == "localhost" || h == "127.0.0.1" || h == "::1" || h == "[::1]")).unwrap_or(false)
}

/// 资源 ID（`sub-en`、`auto-en`、`sub-ai-zh-ab12cd`）里的语言部分。
fn lang_of_asset(asset_id: &str) -> &str {
    asset_id.split_once('-').map(|(_, l)| l).unwrap_or("")
}

/// 作品下是否已有目标语言的字幕。
pub fn has_lang(assets: &[String], tag: &str) -> bool {
    tag != "ai" && assets.iter().any(|a| subtitle::lang_matches(lang_of_asset(a), tag))
}

/// 下载到字幕后，如果不是目标语言、作品下也没有目标语言字幕，就自动翻译（设置里开启时）。
pub fn auto_translate(app: &tauri::AppHandle, st: &Arc<AppState>, item: &crate::db::LibraryItem) {
    let s = st.settings();
    if s.metered_mode
        || !s.ai.auto_translate
        || item.kind != "subtitle"
        || item.source == "ai"
        || item.path.ends_with(".xml")
        || !Path::new(&item.path).exists()
    {
        return;
    }
    if st.vault.get(KEY_CHAT).is_none() && !is_local_url(&s.ai.base_url) {
        return;
    }
    let tag = lang_tag(&s.ai.target_lang);
    let assets = st.db.subtitle_assets_for(&item.platform, &item.media_id).unwrap_or_default();
    if has_lang(&assets, tag) {
        return;
    }
    let path = PathBuf::from(&item.path);
    crate::media_tools::spawn_job(app, stem_of(&path), "翻译字幕", move |ctx| async move { run_translate(&ctx, &path).await });
}

/// 视频下载完成一小段时间后，作品下还是没有任何字幕，就自动转写（设置里开启时）。
pub fn auto_transcribe(app: &tauri::AppHandle, st: &Arc<AppState>, item: &crate::db::LibraryItem) {
    let s = st.settings();
    if s.metered_mode || !s.ai.auto_transcribe || item.kind != "video" || matches!(item.source.as_str(), "tool" | "ai" | "import") {
        return;
    }
    let ready = if s.ai.stt_engine == "local" {
        !s.ai.whisper_model.is_empty()
    } else {
        st.vault.get(KEY_STT).or_else(|| st.vault.get(KEY_CHAT)).is_some() || is_local_url(s.ai.stt_url())
    };
    if !ready {
        return;
    }
    let (app, st, item) = (app.clone(), st.clone(), item.clone());
    let auto_correct = s.ai.correct_auto;
    tauri::async_runtime::spawn(async move {
        // 字幕通常和视频一起下载，等一会儿再看有没有
        tokio::time::sleep(Duration::from_secs(20)).await;
        if st.db.subtitle_assets_for(&item.platform, &item.media_id).map(|a| !a.is_empty()).unwrap_or(true) || !Path::new(&item.path).exists() {
            return;
        }
        let path = PathBuf::from(&item.path);
        let correct = if auto_correct { None } else { Some("off".to_string()) };
        crate::media_tools::spawn_job(&app, stem_of(&path), "语音转文字", move |ctx| async move {
            crate::subgen::run_subgen(&ctx, &path, crate::subgen::GenOpts { correct, ..Default::default() }).await
        });
    });
}

// ---------- 流程（后台任务） ----------

pub(crate) fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into())
}

pub(crate) fn out_beside(input: &Path, base: &str, tag: &str, ext: &str) -> PathBuf {
    crate::naming::unique_path(input.with_file_name(format!("{base}.{tag}.{ext}")), &|p: &Path| p.exists())
}

/// 把新字幕登记进媒体库：挂在原视频（或原字幕）所属的作品下，并建立搜索索引。
pub(crate) fn register_subtitle(st: &AppState, source: &Path, sub: &Path, lang: &str) {
    let source_str = source.to_string_lossy().into_owned();
    let (platform, media_id, platform_name, title) = match st.db.item_by_path(&source_str).ok().flatten() {
        Some(i) => (i.platform, i.media_id, i.platform_name, stem_of(sub)),
        None => ("local".to_string(), format!("ai-{}", crate::library::path_key(source)), "AI 生成".to_string(), stem_of(sub)),
    };
    let asset = format!("sub-ai-{lang}-{}", &crate::library::path_key(sub)[..6]);
    let size = std::fs::metadata(sub).map(|m| m.len() as i64).unwrap_or(0);
    let ok = st
        .db
        .record_download(&crate::db::NewDownload {
            platform: &platform,
            media_id: &media_id,
            asset_id: &asset,
            title: &title,
            author: "",
            cover: None,
            path: &sub.to_string_lossy(),
            size,
            kind: "subtitle",
            source: "ai",
            source_url: "",
            platform_name: &platform_name,
        })
        .is_ok();
    if ok {
        if let Ok(Some(id)) = st.db.id_by_key(&platform, &media_id, &asset) {
            if let Ok(Some(item)) = st.db.library_item(id) {
                let _ = crate::library::index_subtitle_item(&st.db, &item);
            }
        }
    }
}

pub async fn run_translate(ctx: &JobCtx, input: &Path) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ep = chat_endpoint_for(&ctx.st, &settings, Task::Translate)?;
    let cues = subtitle_tools::read(input)?;
    ctx.note(format!("正在翻译成{}…", settings.ai.target_lang));
    let progress = |done: usize, total: usize| -> AppResult<()> {
        ctx.check()?;
        ctx.percent(done as f64 / total.max(1) as f64 * 100.0);
        Ok(())
    };
    let terms = if settings.ai.glossary_on { ctx.st.glossary.terms_for("", lang_tag(&settings.ai.target_lang)) } else { vec![] };
    let t = translate_cues_g(&ep, &settings.ai.model, &settings.ai.target_lang, &cues, settings.ai.batch_size, &terms, &progress).await?;
    let merged = apply_translation(&cues, &t.texts, settings.ai.bilingual);
    let tag = lang_tag(&settings.ai.target_lang);
    let base = strip_lang_suffix(&stem_of(input)).to_string();
    let out = out_beside(input, &base, tag, "srt");
    std::fs::write(&out, subtitle::format_srt(&merged))?;
    if t.failed > 0 {
        ctx.note(format!("有 {} 条没有翻译成功，保留了原文。", t.failed));
    }
    register_subtitle(&ctx.st, input, &out, tag);
    Ok(out)
}

pub async fn run_summarize(ctx: &JobCtx, input: &Path) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ep = chat_endpoint_for(&ctx.st, &settings, Task::Summary)?;
    let cues = subtitle_tools::read(input)?;
    ctx.note("正在总结…");
    let progress = |done: usize, total: usize| -> AppResult<()> {
        ctx.check()?;
        ctx.percent(done as f64 / total.max(1) as f64 * 100.0);
        Ok(())
    };
    let s = summarize(&ep, &settings.ai.model, &settings.ai.target_lang, &cues, &progress).await?;
    let base = strip_lang_suffix(&stem_of(input)).to_string();
    let md = out_beside(input, &base, "摘要", "md");
    std::fs::write(&md, summary_markdown(&base, &s))?;
    if !s.chapters.is_empty() {
        let txt = md.with_extension("章节.txt");
        std::fs::write(txt, chapters_text(&s.chapters))?;
    }
    Ok(md)
}

/// 语音转文字还缺什么设置：返回给用户看的原因；齐全时返回 `Ok`。
/// 用本机 whisper.cpp 吗？选了“本机”就用；还停在默认的“接口”、又没有单独给语音转文字配过接口和密钥，
/// 而本机已经有 whisper-cli 和模型（比如在“设置 → 组件”里下载了模型），就自动用本机，
/// 免得把转写请求发给不支持 /audio/transcriptions 的对话接口（404）。
pub fn use_local_stt(st: &AppState, settings: &Settings) -> bool {
    if settings.ai.stt_engine == "local" {
        return true;
    }
    let own_api = !settings.ai.stt_base_url.trim().is_empty() || st.vault.has(KEY_STT) || stt_pool_endpoint(st, settings).is_some();
    let model = settings.ai.whisper_model.trim();
    !own_api && !model.is_empty() && Path::new(model).is_file() && find_whisper_bin(settings).is_some()
}

fn stt_api_hint(e: AppError) -> AppError {
    if e.message.contains("404") {
        AppError::invalid(format!(
            "{}\n你的 AI 接口可能不支持语音转文字（/audio/transcriptions）。可以改用本机 Whisper：在“设置 → 组件”下载模型，再在“设置 → AI → 语音转文字”里选“本机 whisper.cpp”，并指定 whisper-cli 程序的位置。",
            e.message
        ))
    } else {
        e
    }
}

pub fn stt_ready(st: &AppState, settings: &Settings) -> AppResult<()> {
    if use_local_stt(st, settings) {
        if find_whisper_bin(settings).is_none() {
            return Err(AppError::invalid("找不到 whisper.cpp（whisper-cli）。请在“设置 → AI”里指定它的位置，或把它放进系统 PATH。"));
        }
        let model = settings.ai.whisper_model.trim();
        if model.is_empty() || !Path::new(model).is_file() {
            return Err(AppError::invalid("请在“设置 → AI”里选择 whisper.cpp 的模型文件（ggml-*.bin）。"));
        }
        return Ok(());
    }
    if stt_pool_endpoint(st, settings).is_some() {
        return Ok(());
    }
    stt_endpoint(st, settings)?;
    if settings.ai.stt_model.is_empty() {
        return Err(AppError::invalid("请先在“设置 → AI”里填写语音转文字的模型名称（OpenAI 是 whisper-1）。"));
    }
    Ok(())
}

/// 把一个音视频文件转成带时间戳的文字（按设置用本机 whisper.cpp 或云端接口）。`work` 是调用方管理的临时目录。
pub async fn transcribe_cues(ctx: &JobCtx, settings: &Settings, ffmpeg: &Path, input: &Path, lang: &str, work: &Path) -> AppResult<Vec<Cue>> {
    Ok(transcribe_cues_words(ctx, settings, ffmpeg, input, lang, work, false).await?.0)
}

/// 同 `transcribe_cues`；`want_words` 为真且用本机 whisper.cpp 时，另外返回逐词时间的 JSON（云端接口没有）。
pub async fn transcribe_cues_words(
    ctx: &JobCtx,
    settings: &Settings,
    ffmpeg: &Path,
    input: &Path,
    lang: &str,
    work: &Path,
    want_words: bool,
) -> AppResult<(Vec<Cue>, Option<String>)> {
    let mut words_json = None;
    let cues = if use_local_stt(&ctx.st, settings) {
        let (srt, wj) = run_whisper_local(ctx, settings, ffmpeg, input, lang, work, want_words).await?;
        words_json = wj;
        subtitle::parse_srt(&srt)
    } else {
        let ep = match stt_pool_endpoint(&ctx.st, settings) {
            Some(ep) => ep,
            None => {
                let ep = stt_endpoint(&ctx.st, settings)?;
                if settings.ai.stt_model.is_empty() {
                    return Err(AppError::invalid("请先在“设置 → AI”里填写语音转文字的模型名称（OpenAI 是 whisper-1）。"));
                }
                ep
            }
        };
        ctx.note("正在提取并切分音频…");
        let parts = split_audio(ffmpeg, input, work, 600).await?;
        let mut texts = vec![];
        for (i, (file, offset)) in parts.iter().enumerate() {
            ctx.check()?;
            ctx.note(format!("正在转写第 {} / {} 段…", i + 1, parts.len()));
            ctx.percent(i as f64 / parts.len() as f64 * 100.0);
            texts.push((transcribe_rotating(&ep, &settings.ai.stt_model, lang, file).await.map_err(stt_api_hint)?, *offset));
        }
        merge_srt_parts(texts)
    };
    if cues.is_empty() {
        return Err(AppError::msg("没有识别出任何语音。"));
    }
    Ok((cues, words_json))
}

pub async fn run_transcribe(ctx: &JobCtx, input: &Path, language: Option<String>) -> AppResult<PathBuf> {
    let settings = ctx.st.settings();
    let ffmpeg = find_ffmpeg(&ctx.st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let lang = language.unwrap_or_else(|| settings.ai.stt_language.clone());
    let work = std::env::temp_dir().join(format!("clearclip-stt-{}-{}", std::process::id(), ctx.id));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let result = async {
        let cues = transcribe_cues(ctx, &settings, &ffmpeg, input, &lang, &work).await?;
        let tag = if lang.is_empty() { "ai".to_string() } else { lang.clone() };
        let out = out_beside(input, &stem_of(input), &tag, "srt");
        std::fs::write(&out, subtitle::format_srt(&cues))?;
        register_subtitle(&ctx.st, input, &out, &tag);
        Ok(out)
    }
    .await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
