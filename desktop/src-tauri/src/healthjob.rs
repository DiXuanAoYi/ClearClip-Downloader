//! 平台健康检查作为后台任务：命令行 `clearclip health` 用 `POST /api/v1/health/run` 提交、`GET /api/v1/health/job` 轮询
//! （体检要联网跑几十秒，本机接口的处理函数是同步的，所以不能在里面等）。同一时间只跑一个；跑着的时候再提交，返回正在跑的那个。

use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::commands::HealthResult;

enum State {
    Running,
    Done(Vec<HealthResult>),
    Failed(String),
}

struct Job {
    id: u64,
    state: State,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn lock() -> std::sync::MutexGuard<'static, Option<Job>> {
    JOB.lock().unwrap_or_else(|e| e.into_inner())
}

/// 开始一次体检，返回任务编号；已经有一次在跑就返回那一次的编号。
pub fn start(app: &AppHandle) -> u64 {
    let id = {
        let mut g = lock();
        if let Some(Job { id, state: State::Running }) = g.as_ref() {
            return *id;
        }
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *g = Some(Job { id, state: State::Running });
        id
    };
    use tauri::Manager;
    let st = app.state::<std::sync::Arc<crate::AppState>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let r = crate::commands::run_health(&st).await;
        let mut g = lock();
        if let Some(job) = g.as_mut().filter(|j| j.id == id) {
            job.state = match r {
                Ok(v) => State::Done(v),
                Err(e) => State::Failed(e.message),
            };
        }
    });
    id
}

/// 任务的状态和结果（任务不存在返回 None）。
pub fn job_json(id: u64) -> Option<Value> {
    let g = lock();
    let job = g.as_ref().filter(|j| j.id == id)?;
    Some(match &job.state {
        State::Running => json!({"state": "running"}),
        State::Done(v) => json!({"state": "done", "results": v, "allOk": v.iter().all(|r| r.ok)}),
        State::Failed(e) => json!({"state": "failed", "error": e}),
    })
}
