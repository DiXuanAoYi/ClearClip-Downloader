//! AI 模型池：把多个兼容 OpenAI 接口的服务（很多家都有免费额度）放在一起，
//! 按任务类型自动挑选、轮流使用；某家额度用完、密钥失效或暂时不可用时自动换下一家。
//!
//! 选择规则（[`rank`]）：
//! 1. 只考虑已启用、有密钥（或本机服务）、适用于当前任务、没有在冷却、没超过每日上限的服务；
//! 2. 优先级数字小的先用；
//! 3. 同一优先级里，今天用得最少的先用——多家免费额度因此会“轮流”消耗。
//!
//! 失败处理（[`classify`]）：429 里带“额度 / 余额 / quota”字样 → 当天不再用；普通 429 → 冷却 1 分钟；
//! 401 / 403 → 密钥有问题，冷却 1 小时并在界面标红；网络 / 5xx → 冷却 30 秒；其他错误（如模型名不对）冷却 10 分钟。
//! 计数和冷却只保存在内存里，重启后清零。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{AppError, ErrorKind};
use crate::settings::PoolEntry;

/// 任务类型：不同任务可以指定不同的服务（例如翻译用便宜的小模型，摘要用更强的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// 字幕 / 网页翻译
    Translate,
    /// 摘要、章节、要点、导图
    Summary,
    /// 问答、卡片、笔记、解释等对话
    Chat,
    /// 剪辑相关（精彩片段、文稿剪辑）
    Edit,
}

impl Task {
    pub fn key(self) -> &'static str {
        match self {
            Task::Translate => "translate",
            Task::Summary => "summary",
            Task::Chat => "chat",
            Task::Edit => "edit",
        }
    }
}

/// 预设服务：名称、接口地址、推荐模型、申请 API 密钥的网页和免费额度说明。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
    /// 注册 / 登录页
    pub signup_url: &'static str,
    /// 创建 API 密钥的页面
    pub key_url: &'static str,
    /// 免费额度说明（以服务商官网为准）
    pub note: &'static str,
    /// 有免费额度，适合放进池里轮流用
    pub free: bool,
    /// 本机服务，不需要密钥
    pub local: bool,
    /// 推荐的任务（空 = 全部）
    pub tasks: &'static [&'static str],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "hunyuan",
        name: "腾讯混元",
        base_url: "https://api.hunyuan.cloud.tencent.com/v1",
        model: "hunyuan-lite",
        signup_url: "https://cloud.tencent.com/product/hunyuan",
        key_url: "https://console.cloud.tencent.com/hunyuan/api-key",
        note: "hunyuan-lite 免费；其他模型新用户有试用额度",
        free: true,
        local: false,
        tasks: &["translate", "chat"],
    },
    Preset {
        id: "siliconflow",
        name: "硅基流动",
        base_url: "https://api.siliconflow.cn/v1",
        model: "Qwen/Qwen2.5-7B-Instruct",
        signup_url: "https://cloud.siliconflow.cn/i/",
        key_url: "https://cloud.siliconflow.cn/account/ak",
        note: "多个 7B~9B 模型免费；注册送代金券",
        free: true,
        local: false,
        tasks: &["translate", "chat"],
    },
    Preset {
        id: "zhipu",
        name: "智谱 GLM",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        model: "glm-4-flash",
        signup_url: "https://bigmodel.cn/",
        key_url: "https://bigmodel.cn/usercenter/proj-mgmt/apikeys",
        note: "glm-4-flash 免费不限量（有并发限制）",
        free: true,
        local: false,
        tasks: &[],
    },
    Preset {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
        signup_url: "https://platform.deepseek.com/",
        key_url: "https://platform.deepseek.com/api_keys",
        note: "按量计费，价格低；无免费额度",
        free: false,
        local: false,
        tasks: &["summary", "chat", "edit"],
    },
    Preset {
        id: "dashscope",
        name: "阿里云百炼（通义）",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        model: "qwen-plus",
        signup_url: "https://bailian.console.aliyun.com/",
        key_url: "https://bailian.console.aliyun.com/?apiKey=1",
        note: "新用户各模型有免费额度",
        free: true,
        local: false,
        tasks: &[],
    },
    Preset {
        id: "gemini",
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        model: "gemini-2.0-flash",
        signup_url: "https://aistudio.google.com/",
        key_url: "https://aistudio.google.com/apikey",
        note: "免费层有每分钟 / 每天次数限制；部分地区需要代理",
        free: true,
        local: false,
        tasks: &[],
    },
    Preset {
        id: "groq",
        name: "Groq",
        base_url: "https://api.groq.com/openai/v1",
        model: "llama-3.3-70b-versatile",
        signup_url: "https://console.groq.com/",
        key_url: "https://console.groq.com/keys",
        note: "免费层速度很快，有每分钟 / 每天限额",
        free: true,
        local: false,
        tasks: &["translate", "chat"],
    },
    Preset {
        id: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        model: "deepseek/deepseek-chat-v3-0324:free",
        signup_url: "https://openrouter.ai/",
        key_url: "https://openrouter.ai/keys",
        note: "带 :free 的模型免费，有每日次数限制",
        free: true,
        local: false,
        tasks: &[],
    },
    Preset {
        id: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4o-mini",
        signup_url: "https://platform.openai.com/",
        key_url: "https://platform.openai.com/api-keys",
        note: "按量计费",
        free: false,
        local: false,
        tasks: &[],
    },
    Preset {
        id: "ollama",
        name: "本机 Ollama",
        base_url: "http://127.0.0.1:11434/v1",
        model: "qwen2.5:7b",
        signup_url: "https://ollama.com/download",
        key_url: "https://ollama.com/library",
        note: "在自己电脑上运行，不花钱、不联网",
        free: true,
        local: true,
        tasks: &[],
    },
    Preset {
        id: "custom",
        name: "自定义（兼容 OpenAI）",
        base_url: "",
        model: "",
        signup_url: "",
        key_url: "",
        note: "任何兼容 OpenAI 对话接口的服务",
        free: false,
        local: false,
        tasks: &[],
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// 池里某个服务的密钥在保险箱里的名字。
pub fn key_name(id: &str) -> String {
    format!("ai.pool.{id}")
}

pub const KEY_SUPADATA: &str = "ai.supadata";

// ---------- 运行状态 ----------

#[derive(Debug, Clone, Default)]
pub struct Runtime {
    /// 统计所属的日期（东八区）
    pub day: i64,
    /// 今天已用次数
    pub used: u32,
    pub ok: u32,
    pub fail: u32,
    /// 冷却到什么时候（Unix 秒），0 = 不在冷却
    pub cool_until: i64,
    pub cool_reason: String,
    pub bad_key: bool,
    pub last_error: String,
    pub last_used: i64,
}

#[derive(Default)]
pub struct AiPool {
    inner: Mutex<HashMap<String, Runtime>>,
}

/// 东八区的“第几天”：免费额度大多按北京时间零点重置。
pub fn day_of(now: i64) -> i64 {
    (now + 8 * 3600).div_euclid(86400)
}

/// 下一个东八区零点（Unix 秒）。
pub fn next_reset(now: i64) -> i64 {
    (day_of(now) + 1) * 86400 - 8 * 3600
}

/// 冷却多久、是不是密钥问题。
#[derive(Debug, PartialEq, Eq)]
pub struct Cool {
    pub until: i64,
    pub bad_key: bool,
    pub reason: &'static str,
}

fn mentions_quota(msg: &str) -> bool {
    let m = msg.to_lowercase();
    ["额度", "余额", "欠费", "quota", "insufficient", "exceeded your", "billing", "balance", "free tier", "resource_exhausted", "daily"].iter().any(|k| m.contains(k))
}

pub fn classify(err: &AppError, now: i64) -> Cool {
    match err.kind {
        ErrorKind::RateLimited if mentions_quota(&err.message) => Cool { until: next_reset(now), bad_key: false, reason: "额度用完，明天再用" },
        ErrorKind::RateLimited => Cool { until: now + 60, bad_key: false, reason: "请求太频繁，稍后再试" },
        ErrorKind::NeedLogin => Cool { until: now + 3600, bad_key: true, reason: "密钥无效或没有权限" },
        ErrorKind::Network => Cool { until: now + 30, bad_key: false, reason: "服务暂时不可用" },
        _ => Cool { until: now + 600, bad_key: false, reason: "接口或模型设置有误" },
    }
}

impl AiPool {
    pub fn snapshot(&self, now: i64) -> HashMap<String, Runtime> {
        let mut m = self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let today = day_of(now);
        for r in m.values_mut() {
            if r.day != today {
                r.day = today;
                r.used = 0;
            }
        }
        m
    }

    fn with<R>(&self, id: &str, now: i64, f: impl FnOnce(&mut Runtime) -> R) -> R {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let r = g.entry(id.to_string()).or_default();
        let today = day_of(now);
        if r.day != today {
            r.day = today;
            r.used = 0;
        }
        f(r)
    }

    pub fn record_ok(&self, id: &str, now: i64) {
        self.with(id, now, |r| {
            r.used += 1;
            r.ok += 1;
            r.last_used = now;
            r.cool_until = 0;
            r.bad_key = false;
            r.last_error.clear();
        });
    }

    pub fn record_fail(&self, id: &str, err: &AppError, now: i64) {
        let c = classify(err, now);
        self.with(id, now, |r| {
            r.used += 1;
            r.fail += 1;
            r.last_used = now;
            r.cool_until = c.until;
            r.cool_reason = c.reason.to_string();
            r.bad_key = c.bad_key;
            r.last_error = err.message.chars().take(160).collect();
        });
    }

    /// 清除某个服务的冷却和计数（用户改了密钥，或手动点“重置”）。
    pub fn reset(&self, id: &str) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }
}

// ---------- 选择 ----------

/// 参与排序的一个候选。
#[derive(Debug, Clone)]
pub struct Cand {
    pub id: String,
    pub priority: u32,
    pub daily_limit: u32,
    /// 适用的任务；空 = 全部
    pub tasks: Vec<String>,
    /// 有密钥或是本机服务
    pub usable: bool,
}

pub fn from_entry(e: &PoolEntry, usable: bool) -> Cand {
    Cand { id: e.id.clone(), priority: e.priority, daily_limit: e.daily_limit, tasks: e.tasks.clone(), usable: usable && e.enabled }
}

/// 按规则排出可以依次尝试的服务（返回 `cands` 的下标）。
pub fn rank(cands: &[Cand], rt: &HashMap<String, Runtime>, task: Task, now: i64) -> Vec<usize> {
    let mut v: Vec<(usize, u32, u32)> = cands
        .iter()
        .enumerate()
        .filter(|(_, c)| c.usable)
        .filter(|(_, c)| c.tasks.is_empty() || c.tasks.iter().any(|t| t == task.key()))
        .filter_map(|(i, c)| {
            let r = rt.get(&c.id);
            if r.is_some_and(|r| r.cool_until > now) {
                return None;
            }
            let used = r.filter(|r| r.day == day_of(now)).map_or(0, |r| r.used);
            if c.daily_limit > 0 && used >= c.daily_limit {
                return None;
            }
            Some((i, c.priority, used))
        })
        .collect();
    // 先看优先级，再看今天谁用得少；最后按原顺序，保证结果稳定
    v.sort_by_key(|&(i, p, used)| (p, used, i));
    v.into_iter().map(|(i, _, _)| i).collect()
}

/// 全都不可用时，最早什么时候有服务恢复（用于给用户提示）。
pub fn earliest_ready(cands: &[Cand], rt: &HashMap<String, Runtime>, task: Task, now: i64) -> Option<i64> {
    cands
        .iter()
        .filter(|c| c.usable && (c.tasks.is_empty() || c.tasks.iter().any(|t| t == task.key())))
        .filter_map(|c| {
            let r = rt.get(&c.id)?;
            let cool = (r.cool_until > now).then_some(r.cool_until);
            let used = if r.day == day_of(now) { r.used } else { 0 };
            let limited = (c.daily_limit > 0 && used >= c.daily_limit).then(|| next_reset(now));
            [cool, limited].into_iter().flatten().max()
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, priority: u32) -> Cand {
        Cand { id: id.into(), priority, daily_limit: 0, tasks: vec![], usable: true }
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn same_priority_rotates_by_least_used() {
        let cands = vec![cand("a", 10), cand("b", 10), cand("c", 10)];
        let pool = AiPool::default();
        // 空计数：按原顺序
        assert_eq!(rank(&cands, &pool.snapshot(NOW), Task::Translate, NOW), vec![0, 1, 2]);
        pool.record_ok("a", NOW);
        assert_eq!(rank(&cands, &pool.snapshot(NOW), Task::Translate, NOW), vec![1, 2, 0]);
        pool.record_ok("b", NOW);
        pool.record_ok("b", NOW);
        assert_eq!(rank(&cands, &pool.snapshot(NOW), Task::Translate, NOW), vec![2, 0, 1]);
    }

    #[test]
    fn priority_beats_usage() {
        let cands = vec![cand("slow", 20), cand("fast", 5)];
        let pool = AiPool::default();
        for _ in 0..50 {
            pool.record_ok("fast", NOW);
        }
        assert_eq!(rank(&cands, &pool.snapshot(NOW), Task::Chat, NOW), vec![1, 0]);
    }

    #[test]
    fn task_filter_and_disabled_and_missing_key() {
        let mut only_tr = cand("tr", 1);
        only_tr.tasks = vec!["translate".into()];
        let mut nokey = cand("nokey", 1);
        nokey.usable = false;
        let cands = vec![only_tr, nokey, cand("all", 9)];
        let rt = HashMap::new();
        assert_eq!(rank(&cands, &rt, Task::Translate, NOW), vec![0, 2]);
        assert_eq!(rank(&cands, &rt, Task::Summary, NOW), vec![2]);
    }

    #[test]
    fn daily_limit_and_next_day_reset() {
        let mut c = cand("a", 1);
        c.daily_limit = 2;
        let cands = vec![c, cand("b", 5)];
        let pool = AiPool::default();
        pool.record_ok("a", NOW);
        assert_eq!(rank(&cands, &pool.snapshot(NOW), Task::Chat, NOW), vec![0, 1]);
        pool.record_ok("a", NOW);
        assert_eq!(rank(&cands, &pool.snapshot(NOW), Task::Chat, NOW), vec![1], "到上限后只剩 b");
        assert_eq!(rank(&cands, &pool.snapshot(NOW + 86400), Task::Chat, NOW + 86400), vec![0, 1], "第二天计数清零");
    }

    #[test]
    fn failures_cool_down_with_the_right_duration() {
        let quota = AppError::new(ErrorKind::RateLimited, "请求太频繁或额度用完（429）：You exceeded your current quota");
        let c = classify(&quota, NOW);
        assert_eq!(c.until, next_reset(NOW));
        assert!(c.until > NOW && c.until - NOW <= 86400);
        let busy = AppError::new(ErrorKind::RateLimited, "请求太频繁（429）：Too many requests, slow down");
        assert_eq!(classify(&busy, NOW).until, NOW + 60);
        let auth = AppError::new(ErrorKind::NeedLogin, "API 密钥无效（401）");
        let c = classify(&auth, NOW);
        assert!(c.bad_key && c.until == NOW + 3600);
        assert_eq!(classify(&AppError::new(ErrorKind::Network, "超时"), NOW).until, NOW + 30);
        assert_eq!(classify(&AppError::invalid("模型不对"), NOW).until, NOW + 600);
    }

    #[test]
    fn failed_provider_is_skipped_until_cooldown_ends() {
        let cands = vec![cand("a", 1), cand("b", 2)];
        let pool = AiPool::default();
        pool.record_fail("a", &AppError::new(ErrorKind::Network, "x"), NOW);
        assert_eq!(rank(&cands, &pool.snapshot(NOW + 5), Task::Chat, NOW + 5), vec![1]);
        assert_eq!(rank(&cands, &pool.snapshot(NOW + 31), Task::Chat, NOW + 31), vec![0, 1]);
        pool.record_ok("a", NOW + 40);
        assert!(!pool.snapshot(NOW + 40)["a"].bad_key);
    }

    #[test]
    fn earliest_ready_reports_when_the_first_service_comes_back() {
        let cands = vec![cand("a", 1), cand("b", 2)];
        let pool = AiPool::default();
        pool.record_fail("a", &AppError::new(ErrorKind::Network, "x"), NOW);
        pool.record_fail("b", &AppError::new(ErrorKind::RateLimited, "busy"), NOW);
        assert_eq!(earliest_ready(&cands, &pool.snapshot(NOW + 1), Task::Chat, NOW + 1), Some(NOW + 30));
    }

    #[test]
    fn presets_have_unique_ids_and_key_pages() {
        let mut ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PRESETS.len());
        for id in ["hunyuan", "siliconflow", "zhipu"] {
            let p = preset(id).unwrap();
            assert!(p.free && p.key_url.starts_with("https://") && p.base_url.starts_with("https://"), "{id}");
        }
        assert!(crate::vault::valid_name(&key_name("zhipu-1")));
    }
}
