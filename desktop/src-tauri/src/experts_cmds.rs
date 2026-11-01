//! AI 专家的前端命令。

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult, ErrorKind};
use crate::experts::{self, Expert};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertsState {
    pub experts: Vec<Expert>,
    pub selected: String,
    pub auto: bool,
    pub community_page: &'static str,
}

#[tauri::command]
pub fn experts_state(state: St<'_>) -> ExpertsState {
    let ai = state.settings().ai;
    ExpertsState {
        experts: state.experts.all(),
        selected: if ai.expert.trim().is_empty() { experts::GENERAL.into() } else { ai.expert },
        auto: ai.expert_auto,
        community_page: experts::COMMUNITY_PAGE,
    }
}

/// 新建或保存自建 / 已安装的专家（内置的不能改，要改请先复制成自己的）。
#[tauri::command]
pub fn experts_save(state: St<'_>, mut expert: Expert) -> AppResult<Expert> {
    if experts::builtin().iter().any(|b| b.id == expert.id) {
        return Err(AppError::invalid("内置专家不能直接修改，请先“复制”一份再改。"));
    }
    let keep_origin = state.experts.saved().into_iter().find(|e| e.id == expert.id).map(|e| (e.origin, e.remote_id));
    if let Some((o, r)) = keep_origin {
        expert.origin = o;
        expert.remote_id = r;
    }
    experts::clean_expert(&mut expert);
    if expert.guide.is_empty() {
        return Err(AppError::invalid("策略说明不能为空。"));
    }
    if expert.id.trim().is_empty() {
        expert.id = experts::new_id();
    }
    expert.updated = crate::db::now();
    let out = expert.clone();
    state.experts.edit(|v| match v.iter_mut().find(|e| e.id == expert.id) {
        Some(e) => *e = expert,
        None => v.push(expert),
    })?;
    Ok(out)
}

#[tauri::command]
pub fn experts_delete(state: St<'_>, id: String) -> AppResult<()> {
    state.experts.edit(|v| v.retain(|e| e.id != id))
}

/// 导入 JSON（本程序导出的）或沉浸式翻译的专家 YAML。
#[tauri::command]
pub fn experts_import(state: St<'_>, text: String) -> AppResult<Expert> {
    let mut e = experts::parse_import(&text)?;
    e.id = experts::new_id();
    e.updated = crate::db::now();
    let out = e.clone();
    state.experts.edit(|v| v.push(e))?;
    Ok(out)
}

#[tauri::command]
pub fn experts_import_file(state: St<'_>, path: String) -> AppResult<Expert> {
    let bytes = std::fs::read(&path).map_err(|e| AppError::msg(format!("读不了这个文件：{e}")))?;
    if bytes.len() > 1024 * 1024 {
        return Err(AppError::invalid("文件太大（上限 1 MB）。"));
    }
    experts_import(state, String::from_utf8_lossy(&bytes).to_string())
}

#[tauri::command]
pub fn experts_export(state: St<'_>, id: String) -> AppResult<String> {
    let e = state.experts.all().into_iter().find(|e| e.id == id).ok_or_else(|| AppError::invalid("专家不存在。"))?;
    let out = Expert { id: String::new(), origin: "user".into(), remote_id: String::new(), updated: 0, ..e };
    Ok(serde_json::to_string_pretty(&out)?)
}

// ---------- 社区专家 ----------

static COMMUNITY: LazyLock<Mutex<Option<(Instant, Vec<Expert>)>>> = LazyLock::new(|| Mutex::new(None));

async fn get_yaml(st: &AppState, id: &str) -> AppResult<String> {
    experts::check_remote_id(id)?;
    let url = format!("{}/{}.yml", experts::COMMUNITY_RAW, id);
    let settings = st.settings();
    let client = st.net.clients_for(&settings.network, &url)?.download;
    let resp =
        client.get(&url).timeout(Duration::from_secs(30)).send().await.map_err(|e| AppError::new(ErrorKind::Network, format!("无法连接 GitHub：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!("GitHub 返回 {}（{id}）", resp.status().as_u16())));
    }
    resp.text().await.map_err(|e| AppError::new(ErrorKind::Network, format!("读取失败：{e}")))
}

async fn fetch_one(st: &AppState, id: &str) -> Option<Expert> {
    experts::from_immersive_yaml(&get_yaml(st, id).await.ok()?)
}

/// 沉浸式翻译公开专家列表（10 分钟内用缓存）。列表取自内置的文件名清单，不依赖 GitHub 接口。
#[tauri::command]
pub async fn experts_community(state: St<'_>) -> AppResult<Vec<Expert>> {
    if let Some((at, list)) = COMMUNITY.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return Ok(list.clone());
        }
    }
    let st: &AppState = &state;
    let mut items = vec![];
    for group in experts::COMMUNITY_SNAPSHOT.chunks(8) {
        let got = futures_util::future::join_all(group.iter().map(|id| fetch_one(st, id))).await;
        items.extend(got.into_iter().flatten());
    }
    if items.is_empty() {
        return Err(AppError::msg("没有取到社区专家，请检查网络后重试。"));
    }
    *COMMUNITY.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), items.clone()));
    Ok(items)
}

/// 安装（或更新）一个社区专家；已装过的按原文件名更新，保留你改过的生效网站。
#[tauri::command]
pub async fn experts_community_install(state: St<'_>, remote_id: String) -> AppResult<Expert> {
    let st: &AppState = &state;
    let mut fresh = fetch_one(st, &remote_id).await.ok_or_else(|| AppError::msg("取不到这个专家，或它没有可用的提示词。"))?;
    fresh.updated = crate::db::now();
    st.experts.edit(|v| {
        if let Some(e) = v.iter_mut().find(|e| e.origin == "community" && e.remote_id == remote_id) {
            fresh.id = e.id.clone();
            fresh.matches = e.matches.clone();
            *e = fresh.clone();
        } else {
            fresh.id = experts::new_id();
            v.push(fresh.clone());
        }
        fresh.clone()
    })
}
