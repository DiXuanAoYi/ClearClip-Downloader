//! AI 术语库的前端命令。

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::glossary::{self, Candidate, CommunityItem, Data, ExtractSite, Glossary, Term};
use crate::{ai, ai_pool, AppState};

type St<'a> = State<'a, Arc<AppState>>;

#[tauri::command]
pub fn glossary_state(state: St<'_>) -> Data {
    state.glossary.snapshot()
}

/// 新建或保存（id 为空或不存在时新建）。
#[tauri::command]
pub fn glossary_save(state: St<'_>, mut glossary: Glossary) -> AppResult<Glossary> {
    glossary::clean_glossary(&mut glossary);
    glossary.updated = crate::db::now();
    if glossary.id.trim().is_empty() {
        glossary.id = glossary::new_id();
    }
    let out = glossary.clone();
    state.glossary.edit(|d| match d.glossaries.iter_mut().find(|g| g.id == glossary.id) {
        Some(g) => *g = glossary,
        None => d.glossaries.push(glossary),
    })?;
    Ok(out)
}

#[tauri::command]
pub fn glossary_delete(state: St<'_>, id: String) -> AppResult<()> {
    state.glossary.edit(|d| {
        d.glossaries.retain(|g| g.id != id);
        for s in &mut d.sites {
            if s.glossary == id {
                s.glossary.clear();
            }
        }
    })
}

/// 导入 CSV 文本。`into` 非空 = 并入已有术语库（同名原文以新的为准），否则新建一个。
#[tauri::command]
pub fn glossary_import(state: St<'_>, text: String, name: String, into: String, lang: String) -> AppResult<Glossary> {
    let terms = glossary::parse_csv(&text);
    if terms.is_empty() {
        return Err(AppError::invalid("没有读到术语。CSV 需要表头 source,target（可再加 tgt_lng、note），或每行“原文,译文”。"));
    }
    let now = crate::db::now();
    state.glossary.edit(|d| {
        if let Some(g) = d.glossaries.iter_mut().find(|g| !into.is_empty() && g.id == into) {
            let incoming: std::collections::HashSet<String> = terms.iter().map(|t| t.source.to_lowercase()).collect();
            g.terms.retain(|t| !incoming.contains(&t.source.to_lowercase()));
            g.terms.extend(terms);
            g.terms = glossary::clean_terms(std::mem::take(&mut g.terms));
            g.updated = now;
            g.clone()
        } else {
            let mut g = Glossary { id: glossary::new_id(), name, lang, terms, updated: now, ..Default::default() };
            glossary::clean_glossary(&mut g);
            d.glossaries.push(g.clone());
            g
        }
    })
}

/// 从文件导入（CSV / TSV / TXT）。
#[tauri::command]
pub fn glossary_import_file(state: St<'_>, path: String, name: String, into: String, lang: String) -> AppResult<Glossary> {
    let bytes = std::fs::read(&path).map_err(|e| AppError::msg(format!("读不了这个文件：{e}")))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(AppError::invalid("文件太大（上限 8 MB）。"));
    }
    let name = if name.trim().is_empty() {
        std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("导入的术语库").to_string()
    } else {
        name
    };
    glossary_import(state, String::from_utf8_lossy(&bytes).to_string(), name, into, lang)
}

#[tauri::command]
pub fn glossary_export(state: St<'_>, id: String) -> AppResult<String> {
    let d = state.glossary.snapshot();
    let g = d.glossaries.iter().find(|g| g.id == id).ok_or_else(|| AppError::invalid("术语库不存在。"))?;
    Ok(glossary::to_csv(&g.terms, &g.lang))
}

// ---------- 社区术语库 ----------

type CommunityCache = Option<(Instant, Vec<CommunityItem>)>;
static COMMUNITY: LazyLock<Mutex<CommunityCache>> = LazyLock::new(|| Mutex::new(None));

async fn get_text(st: &AppState, path: &str) -> AppResult<String> {
    let url = format!("{}/{}", glossary::COMMUNITY_RAW, path);
    let settings = st.settings();
    let client = st.net.clients_for(&settings.network, &url)?.download;
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("无法连接 GitHub：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!("GitHub 返回 {}（{path}）", resp.status().as_u16())));
    }
    resp.text().await.map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("读取失败：{e}")))
}

async fn fetch_meta(st: &AppState, id: &str) -> Option<CommunityItem> {
    let text = get_text(st, &format!("meta/{id}.json")).await.ok()?;
    glossary::parse_community_meta(&serde_json::from_str::<Value>(&text).ok()?)
}

/// 沉浸式翻译公开术语库列表（10 分钟内用缓存）。
#[tauri::command]
pub async fn glossary_community(state: St<'_>) -> AppResult<Vec<CommunityItem>> {
    if let Some((at, list)) = COMMUNITY.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return Ok(list.clone());
        }
    }
    let st: &AppState = &state;
    let index: Vec<String> = serde_json::from_str(&get_text(st, "meta/index.json").await?).map_err(|_| AppError::parser("社区术语库列表格式不对。"))?;
    let ids: Vec<String> = index.into_iter().filter(|i| glossary::check_remote_id(i).is_ok()).take(300).collect();
    let mut items = vec![];
    for group in ids.chunks(8) {
        let got = futures_util::future::join_all(group.iter().map(|id| fetch_meta(st, id))).await;
        items.extend(got.into_iter().flatten());
    }
    if items.is_empty() {
        return Err(AppError::msg("没有取到社区术语库，请检查网络后重试。"));
    }
    *COMMUNITY.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), items.clone()));
    Ok(items)
}

/// 安装（或更新）一个社区术语库，语言按“设置 → AI → 翻译成”选。
#[tauri::command]
pub async fn glossary_community_install(state: St<'_>, id: String) -> AppResult<Glossary> {
    glossary::check_remote_id(&id)?;
    let st: &AppState = &state;
    let item = fetch_meta(st, &id).await.ok_or_else(|| AppError::msg("取不到这个术语库的说明。"))?;
    let tag = ai::lang_tag(&st.settings().ai.target_lang);
    let (lang, file) = glossary::community_csv(&item, tag).ok_or_else(|| AppError::invalid("这个术语库没有可用的语言。"))?;
    let terms = glossary::parse_csv(&get_text(st, &format!("glossaries/{file}")).await?);
    if terms.is_empty() {
        return Err(AppError::msg("术语文件是空的。"));
    }
    let now = crate::db::now();
    st.glossary.edit(|d| {
        let mut fresh = Glossary {
            id: glossary::new_id(),
            name: item.name.clone(),
            description: item.description.clone(),
            origin: "community".into(),
            remote_id: id.clone(),
            matches: item.matches.clone(),
            lang: lang.clone(),
            terms,
            updated: now,
            ..Default::default()
        };
        glossary::clean_glossary(&mut fresh);
        if let Some(g) = d.glossaries.iter_mut().find(|g| g.remote_id == id && g.lang == lang) {
            // 更新：保留 id、启用状态和你改过的生效网站
            fresh.id = g.id.clone();
            fresh.enabled = g.enabled;
            fresh.matches = g.matches.clone();
            *g = fresh.clone();
        } else {
            d.glossaries.push(fresh.clone());
        }
        fresh
    })
}

// ---------- 智能提取 ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractReq {
    /// text / file / url
    pub kind: String,
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractOut {
    /// 模型给出的术语条数
    pub found: usize,
    /// 新进入待确认（或直接加入术语库）的条数
    pub added: usize,
    pub site: String,
}

fn text_of_file(path: &str) -> AppResult<String> {
    let p = std::path::Path::new(path);
    let bytes = std::fs::read(p).map_err(|e| AppError::msg(format!("读不了这个文件：{e}")))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(AppError::invalid("文件太大（上限 8 MB）。"));
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let cues = match ext.as_str() {
        "srt" => crate::subtitle::parse_srt(&text),
        "vtt" => crate::subtitle::parse_vtt(&text),
        "ass" | "ssa" => crate::subtitle::parse_ass(&text),
        _ => return Ok(if ext == "html" || ext == "htm" { glossary::html_to_text(&text) } else { text }),
    };
    Ok(cues.iter().map(|c| c.lines.join(" ")).collect::<Vec<_>>().join("\n"))
}

#[tauri::command]
pub async fn glossary_extract(state: St<'_>, source: ExtractReq) -> AppResult<ExtractOut> {
    let st: &AppState = &state;
    let settings = st.settings();
    let (text, site) = match source.kind.as_str() {
        "text" => (source.value.clone(), String::new()),
        "file" => (text_of_file(&source.value)?, String::new()),
        "url" => {
            let url = source.value.trim().to_string();
            let parsed =
                url::Url::parse(&url).ok().filter(|u| matches!(u.scheme(), "http" | "https")).ok_or_else(|| AppError::invalid("请输入 http(s) 网址。"))?;
            let client = st.net.clients_for(&settings.network, &url)?.api;
            let resp = client
                .get(&url)
                .header("User-Agent", "Mozilla/5.0 ClearClip")
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("打不开这个网页：{e}")))?;
            if !resp.status().is_success() {
                return Err(AppError::msg(format!("网页返回 {}。需要登录的页面请直接粘贴文字。", resp.status().as_u16())));
            }
            let html = resp.text().await.map_err(|e| AppError::new(crate::error::ErrorKind::Network, format!("读取网页失败：{e}")))?;
            (glossary::html_to_text(&html), parsed.host_str().unwrap_or("").to_string())
        }
        _ => return Err(AppError::invalid("不支持的提取来源。")),
    };
    if text.trim().chars().count() < 20 {
        return Err(AppError::invalid("文字太少，提取不出术语。"));
    }
    let ep = ai::chat_endpoint_for(st, &settings, ai_pool::Task::Translate)?;
    let found = glossary::extract(&ep, &settings.ai.model, &settings.ai.target_lang, &text).await?;
    let n = found.len();
    let added = glossary::file_found(&st.glossary, found, &site, crate::db::now())?;
    Ok(ExtractOut { found: n, added, site })
}

/// 处理待确认的术语：`glossary_id` 非空（或给了 `new_name`）= 把 `terms` 加入该术语库，并移除这些条目；两者都空 = 仅忽略（删除）。
#[tauri::command]
pub fn glossary_resolve(state: St<'_>, ids: Vec<String>, glossary_id: String, new_name: String, terms: Vec<Term>) -> AppResult<Option<Glossary>> {
    let now = crate::db::now();
    state.glossary.edit(|d| {
        let mut out = None;
        if !terms.is_empty() && (!glossary_id.is_empty() || !new_name.trim().is_empty()) {
            let idx = d.glossaries.iter().position(|g| !glossary_id.is_empty() && g.id == glossary_id);
            let g = match idx {
                Some(i) => &mut d.glossaries[i],
                None => {
                    d.glossaries.push(Glossary {
                        id: glossary::new_id(),
                        name: new_name.clone(),
                        origin: "extracted".into(),
                        lang: String::new(),
                        ..Default::default()
                    });
                    d.glossaries.last_mut().expect("just pushed")
                }
            };
            let incoming: std::collections::HashSet<String> = terms.iter().map(|t| t.source.to_lowercase()).collect();
            g.terms.retain(|t| !incoming.contains(&t.source.to_lowercase()));
            g.terms.extend(terms.iter().cloned());
            g.terms = glossary::clean_terms(std::mem::take(&mut g.terms));
            g.name = if g.name.trim().is_empty() { "智能提取".into() } else { g.name.clone() };
            g.updated = now;
            out = Some(g.clone());
        }
        let drop: std::collections::HashSet<&String> = ids.iter().collect();
        d.candidates.retain(|c: &Candidate| !drop.contains(&c.id));
        out
    })
}

#[tauri::command]
pub fn glossary_sites_save(state: St<'_>, sites: Vec<ExtractSite>) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    let sites: Vec<ExtractSite> = sites
        .into_iter()
        .map(|mut s| {
            s.pattern = s.pattern.trim().to_lowercase();
            s
        })
        .filter(|s| !s.pattern.is_empty() && seen.insert(s.pattern.clone()))
        .take(200)
        .collect();
    state.glossary.edit(|d| d.sites = sites)
}
