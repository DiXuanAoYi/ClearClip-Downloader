//! 按域名记住下载经验：服务器不支持 Range（只能单线程）、或者同时连接数多了会被限流（429 / 503），
//! 下次直接用合适的连接数，不用再试一遍。记录保存在数据目录的 `host_limits.json`，过期自动忘记。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 不支持 Range 的记录保留多久（服务器可能升级，隔段时间重新探测）
const NO_RANGE_TTL_SECS: i64 = 7 * 86_400;
/// 连接数上限的记录保留多久
const LIMIT_TTL_SECS: i64 = 30 * 86_400;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HostInfo {
    /// 记录“不支持分段”的时间（Unix 秒）；0 表示没有
    pub no_range_at: i64,
    /// 同时连接数上限；0 表示没有限制记录
    pub max_conns: u32,
    pub limit_at: i64,
}

impl HostInfo {
    pub fn no_range(&self) -> bool {
        self.no_range_at > 0 && now() - self.no_range_at < NO_RANGE_TTL_SECS
    }

    pub fn max_conns(&self) -> Option<usize> {
        (self.max_conns > 0 && now() - self.limit_at < LIMIT_TTL_SECS).then_some(self.max_conns as usize)
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Default)]
struct Mem {
    map: Mutex<HashMap<String, HostInfo>>,
    path: Mutex<Option<PathBuf>>,
}

fn mem() -> &'static Mem {
    static M: OnceLock<Mem> = OnceLock::new();
    M.get_or_init(Mem::default)
}

/// 启动时调用一次：读取已有记录，之后的改动写回这个目录。
pub fn init(dir: PathBuf) {
    let path = dir.join("host_limits.json");
    let loaded: HashMap<String, HostInfo> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let m = mem();
    *m.map.lock().unwrap_or_else(|e| e.into_inner()) = loaded;
    *m.path.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

fn save(map: &HashMap<String, HostInfo>) {
    let Some(path) = mem().path.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    // 丢掉已经过期的记录
    let live: HashMap<&String, &HostInfo> = map.iter().filter(|(_, v)| v.no_range() || v.max_conns().is_some()).collect();
    if let Ok(json) = serde_json::to_vec(&live) {
        let _ = std::fs::write(path, json);
    }
}

/// 链接的主机名（小写）；不是有效地址时为空。
pub fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default()
}

pub fn get(host: &str) -> HostInfo {
    if host.is_empty() {
        return HostInfo::default();
    }
    mem().map.lock().unwrap_or_else(|e| e.into_inner()).get(host).copied().unwrap_or_default()
}

fn update(host: &str, f: impl FnOnce(&mut HostInfo)) {
    if host.is_empty() {
        return;
    }
    let mut map = mem().map.lock().unwrap_or_else(|e| e.into_inner());
    f(map.entry(host.to_string()).or_default());
    save(&map);
}

/// 服务器不支持 Range：以后直接单线程。
pub fn note_no_range(host: &str) {
    update(host, |h| h.no_range_at = now());
}

/// 同时连接太多被限流：把上限记成 `n`（至少 1）。
pub fn note_limit(host: &str, n: usize) {
    update(host, |h| {
        h.max_conns = n.clamp(1, u32::MAX as usize) as u32;
        h.limit_at = now();
    });
}

/// 忘掉某个域名的记录（设置里的“重置”用）。
pub fn forget(host: &str) {
    let mut map = mem().map.lock().unwrap_or_else(|e| e.into_inner());
    if map.remove(host).is_some() {
        save(&map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_no_range_and_limits_per_host() {
        let h = "a-host-for-test.example";
        assert_eq!(get(h), HostInfo::default());
        note_no_range(h);
        note_limit(h, 0);
        let i = get(h);
        assert!(i.no_range());
        assert_eq!(i.max_conns(), Some(1));
        note_limit(h, 8);
        assert_eq!(get(h).max_conns(), Some(8));
        forget(h);
        assert!(!get(h).no_range());
    }

    #[test]
    fn old_records_expire() {
        let old = HostInfo { no_range_at: now() - NO_RANGE_TTL_SECS - 5, max_conns: 4, limit_at: now() - LIMIT_TTL_SECS - 5 };
        assert!(!old.no_range());
        assert_eq!(old.max_conns(), None);
        let fresh = HostInfo { no_range_at: now() - 10, max_conns: 4, limit_at: now() - 10 };
        assert!(fresh.no_range());
        assert_eq!(fresh.max_conns(), Some(4));
    }

    #[test]
    fn host_names_are_lowercased() {
        assert_eq!(host_of("https://Cdn.Example.com:8443/a?b=1"), "cdn.example.com");
        assert_eq!(host_of("not a url"), "");
    }
}
