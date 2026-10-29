//! Cookie 存储：按条保存完整字段（域名、路径、过期时间等），请求时按域名匹配，
//! 同一网站可保存多个账号。整个存储用 AES-GCM 加密写入磁盘。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{AppError, AppResult};
use crate::{providers, secret};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    /// 不带前导点的域名
    pub domain: String,
    /// 为 true 时只发给与 `domain` 完全相同的主机，不发给子域名
    #[serde(default)]
    pub host_only: bool,
    #[serde(default = "root_path")]
    pub path: String,
    /// Unix 秒；None 表示会话 Cookie
    #[serde(default)]
    pub expires: Option<i64>,
    #[serde(default)]
    pub secure: bool,
    #[serde(default)]
    pub http_only: bool,
}

fn root_path() -> String {
    "/".into()
}

impl StoredCookie {
    pub fn matches(&self, url: &Url, now: i64) -> bool {
        let Some(host) = url.host_str() else { return false };
        let host = host.to_ascii_lowercase();
        let domain_ok = host == self.domain || (!self.host_only && host.ends_with(&format!(".{}", self.domain)));
        let path_ok = path_matches(url.path(), &self.path);
        let secure_ok = !self.secure || url.scheme() == "https";
        let fresh = self.expires.is_fresh(now);
        domain_ok && path_ok && secure_ok && fresh
    }
}

trait ExpiryExt {
    fn is_fresh(&self, now: i64) -> bool;
}

impl ExpiryExt for Option<i64> {
    /// 未设置过期时间，或过期时间晚于当前时间（0 也视为会话 Cookie）
    fn is_fresh(&self, now: i64) -> bool {
        match self {
            None | Some(0) => true,
            Some(t) => *t > now,
        }
    }
}

fn path_matches(req: &str, cookie: &str) -> bool {
    if cookie.is_empty() || cookie == "/" || req == cookie {
        return true;
    }
    req.starts_with(cookie) && (cookie.ends_with('/') || req[cookie.len()..].starts_with('/'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    /// 平台 ID（如 `bilibili`）或可注册域名（如 `youtube.com`）
    pub site: String,
    pub label: String,
    pub cookies: Vec<StoredCookie>,
    pub updated_at: i64,
    #[serde(default)]
    pub user_name: Option<String>,
    /// 最近一次检查登录状态的时间和结果
    #[serde(default)]
    pub checked_at: Option<i64>,
    #[serde(default)]
    pub valid: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub site: String,
    pub site_name: String,
    pub label: String,
    pub cookie_count: usize,
    pub updated_at: i64,
    pub user_name: Option<String>,
    /// 最早过期的非会话 Cookie 的过期时间
    pub expires_at: Option<i64>,
    pub is_default: bool,
    pub checked_at: Option<i64>,
    /// 最近一次检查的结果：true 有效 / false 已失效 / None 未检查
    pub valid: Option<bool>,
    /// 能否检查登录状态（联网验证，或检查登录 Cookie 是否存在）
    pub checkable: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreData {
    accounts: Vec<Account>,
    /// site → 默认账号 ID
    defaults: HashMap<String, String>,
}

pub struct CookieStore {
    data: RwLock<StoreData>,
    path: Option<PathBuf>,
    key: [u8; 32],
    pub key_in_keyring: bool,
}

impl CookieStore {
    pub fn open(path: PathBuf, key: [u8; 32], key_in_keyring: bool) -> CookieStore {
        let data = std::fs::read(&path)
            .ok()
            .and_then(|enc| {
                let plain = secret::decrypt(&key, &enc);
                if plain.is_none() {
                    log::warn!("cookie store could not be decrypted; starting empty");
                }
                plain
            })
            .and_then(|plain| serde_json::from_slice(&plain).ok())
            .unwrap_or_default();
        CookieStore { data: RwLock::new(data), path: Some(path), key, key_in_keyring }
    }

    pub fn in_memory() -> CookieStore {
        CookieStore { data: RwLock::new(StoreData::default()), path: None, key: [7; 32], key_in_keyring: false }
    }

    fn save(&self) -> AppResult<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let plain = serde_json::to_vec(&*self.data.read().unwrap_or_else(|e| e.into_inner()))?;
        let enc = secret::encrypt(&self.key, &plain);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, enc)?;
        secret::restrict_permissions(&tmp);
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// 请求某个地址时应带的 Cookie 头：取该地址所属网站的默认账号，再按域名、路径过滤。
    pub fn header_for(&self, url: &str) -> Option<String> {
        let url = Url::parse(url).ok()?;
        let site = site_for_url(&url)?;
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        let account = default_account(&data, &site)?;
        let now = crate::db::now();
        let pairs: Vec<String> = account.cookies.iter().filter(|c| c.matches(&url, now)).map(|c| format!("{}={}", c.name, c.value)).collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    /// 指定账号的 Cookie 头（按域名、路径过滤）。
    pub fn header_for_account(&self, id: &str, url: &str) -> Option<String> {
        let url = Url::parse(url).ok()?;
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        let account = data.accounts.iter().find(|a| a.id == id)?;
        let now = crate::db::now();
        let pairs: Vec<String> = account.cookies.iter().filter(|c| c.matches(&url, now)).map(|c| format!("{}={}", c.name, c.value)).collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    pub fn account_site(&self, id: &str) -> Option<String> {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        data.accounts.iter().find(|a| a.id == id).map(|a| a.site.clone())
    }

    pub fn has_account(&self, site: &str) -> bool {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        data.accounts.iter().any(|a| a.site == site)
    }

    /// 某网站默认账号的全部 Cookie（交给 yt-dlp 等外部程序时使用）。
    pub fn cookies_for_site(&self, site: &str) -> Vec<StoredCookie> {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        default_account(&data, site).map(|a| a.cookies.clone()).unwrap_or_default()
    }

    /// 新增账号，或替换同一网站同名账号的 Cookie。返回账号 ID。
    pub fn upsert(&self, site: &str, label: &str, cookies: Vec<StoredCookie>) -> AppResult<String> {
        if cookies.is_empty() {
            return Err(AppError::invalid("没有可保存的 Cookie。"));
        }
        let id = {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            let now = crate::db::now();
            let id = match data.accounts.iter_mut().find(|a| a.site == site && a.label == label) {
                Some(a) => {
                    a.cookies = cookies;
                    a.updated_at = now;
                    a.checked_at = None;
                    a.valid = None;
                    a.id.clone()
                }
                None => {
                    let id = format!("{site}-{now}-{}", data.accounts.len());
                    data.accounts.push(Account {
                        id: id.clone(),
                        site: site.to_string(),
                        label: label.to_string(),
                        cookies,
                        updated_at: now,
                        user_name: None,
                        checked_at: None,
                        valid: None,
                    });
                    id
                }
            };
            data.defaults.entry(site.to_string()).or_insert_with(|| id.clone());
            id
        };
        self.save()?;
        Ok(id)
    }

    /// 合并 Cookie（用于 yt-dlp 运行后回写轮换过的 Cookie）：同名、同域、同路径的覆盖，其余追加。
    pub fn merge_into_default(&self, site: &str, fresh: Vec<StoredCookie>) -> AppResult<()> {
        {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            let Some(id) = data.defaults.get(site).cloned() else { return Ok(()) };
            let Some(acc) = data.accounts.iter_mut().find(|a| a.id == id) else { return Ok(()) };
            for c in fresh {
                match acc.cookies.iter_mut().find(|o| o.name == c.name && o.domain == c.domain && o.path == c.path) {
                    Some(o) => *o = c,
                    None => acc.cookies.push(c),
                }
            }
            acc.updated_at = crate::db::now();
        }
        self.save()
    }

    pub fn set_user_name(&self, id: &str, name: Option<String>) -> AppResult<()> {
        {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            if let Some(a) = data.accounts.iter_mut().find(|a| a.id == id) {
                a.user_name = name;
            }
        }
        self.save()
    }

    /// 记录检查结果；返回之前的结果。
    pub fn set_check_result(&self, id: &str, valid: bool, name: Option<String>) -> AppResult<Option<bool>> {
        let prev = {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            let Some(a) = data.accounts.iter_mut().find(|a| a.id == id) else { return Ok(None) };
            let prev = a.valid;
            a.valid = Some(valid);
            a.checked_at = Some(crate::db::now());
            if name.is_some() || !valid {
                a.user_name = name;
            }
            prev
        };
        self.save()?;
        Ok(prev)
    }

    /// 本地检查：账号里有没有未过期的登录 Cookie。网站不在已知列表时返回 None。
    pub fn has_login_cookie(&self, id: &str) -> Option<bool> {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        let a = data.accounts.iter().find(|a| a.id == id)?;
        let names = login_cookie_names(&a.site)?;
        let now = crate::db::now();
        Some(a.cookies.iter().any(|c| names.contains(&c.name.as_str()) && !c.value.is_empty() && c.expires.map_or(true, |t| t <= 0 || t > now)))
    }

    pub fn rename(&self, id: &str, label: &str) -> AppResult<()> {
        {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            let a = data.accounts.iter_mut().find(|a| a.id == id).ok_or_else(|| AppError::not_found("账号不存在。"))?;
            a.label = label.trim().to_string();
        }
        self.save()
    }

    pub fn set_default(&self, id: &str) -> AppResult<()> {
        {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            let site = data.accounts.iter().find(|a| a.id == id).map(|a| a.site.clone()).ok_or_else(|| AppError::not_found("账号不存在。"))?;
            data.defaults.insert(site, id.to_string());
        }
        self.save()
    }

    /// 删除所有账号和 Cookie（“一键清除”用）。
    pub fn clear_all(&self) -> AppResult<()> {
        *self.data.write().unwrap_or_else(|e| e.into_inner()) = StoreData::default();
        self.save()
    }

    pub fn delete(&self, id: &str) -> AppResult<()> {
        {
            let mut data = self.data.write().unwrap_or_else(|e| e.into_inner());
            data.accounts.retain(|a| a.id != id);
            let remaining: Vec<(String, String)> = data.accounts.iter().map(|a| (a.site.clone(), a.id.clone())).collect();
            data.defaults.retain(|_, v| v != id);
            for (site, aid) in remaining {
                data.defaults.entry(site).or_insert(aid);
            }
        }
        self.save()
    }

    pub fn summaries(&self) -> Vec<AccountSummary> {
        let data = self.data.read().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<AccountSummary> = data
            .accounts
            .iter()
            .map(|a| AccountSummary {
                id: a.id.clone(),
                site: a.site.clone(),
                site_name: site_display_name(&a.site),
                label: a.label.clone(),
                cookie_count: a.cookies.len(),
                updated_at: a.updated_at,
                user_name: a.user_name.clone(),
                expires_at: a.cookies.iter().filter_map(|c| c.expires).filter(|t| *t > 0).min(),
                is_default: data.defaults.get(&a.site) == Some(&a.id),
                checked_at: a.checked_at,
                valid: a.valid,
                checkable: providers::by_id(&a.site).is_some_and(|p| ONLINE_CHECK.contains(&p.id())) || login_cookie_names(&a.site).is_some(),
            })
            .collect();
        out.sort_by(|a, b| a.site.cmp(&b.site).then(b.is_default.cmp(&a.is_default)));
        out
    }

    /// 把旧版设置里的整串 Cookie 迁移进来。
    pub fn migrate_legacy(&self, legacy: &HashMap<String, String>) -> AppResult<usize> {
        let mut n = 0;
        for (platform, header) in legacy {
            let Some(p) = providers::by_id(platform) else { continue };
            let Some(domain) = Url::parse(p.login_url()).ok().and_then(|u| u.host_str().map(registrable_domain)) else { continue };
            let cookies = parse_header(header, &domain);
            if !cookies.is_empty() && !self.has_account(platform) {
                self.upsert(platform, "默认", cookies)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 导入 Netscape 格式的 cookies.txt，按网站分组保存。返回 (网站数, Cookie 数)。
    pub fn import_netscape(&self, text: &str, label: &str) -> AppResult<(usize, usize)> {
        let cookies = parse_netscape(text);
        if cookies.is_empty() {
            return Err(AppError::invalid("文件里没有找到 Cookie。请确认是 Netscape 格式的 cookies.txt。"));
        }
        let mut groups: HashMap<String, Vec<StoredCookie>> = HashMap::new();
        for c in cookies {
            groups.entry(site_for_domain(&c.domain)).or_default().push(c);
        }
        let total: usize = groups.values().map(Vec::len).sum();
        let sites = groups.len();
        for (site, list) in groups {
            self.upsert(&site, label, list)?;
        }
        Ok((sites, total))
    }
}

/// 支持联网检查登录状态的内置平台。
pub const ONLINE_CHECK: &[&str] = &["bilibili", "weibo"];

/// 登录后才会出现的 Cookie（任意一个存在即视为已登录）。用于自动识别登录完成和本地检查登录状态。
/// 访客也会拿到的 Cookie（如微博的 SUB、小红书的 web_session）不能用于判断，这些网站不在列表里。
pub fn login_cookie_names(site: &str) -> Option<&'static [&'static str]> {
    let site = site.trim_start_matches("www.");
    Some(match site {
        "bilibili" | "bilibili.com" => &["SESSDATA"],
        "douyin" | "douyin.com" => &["sessionid", "sessionid_ss"],
        "kuaishou" | "kuaishou.com" => &["kuaishou.server.web_st", "kuaishou.server.webday7_st"],
        "instagram.com" => &["sessionid"],
        "x.com" | "twitter.com" => &["auth_token"],
        "facebook.com" => &["c_user"],
        "tiktok.com" => &["sessionid", "sessionid_ss"],
        "youtube.com" => &["SAPISID", "__Secure-3PSID"],
        _ => return None,
    })
}

fn default_account<'a>(data: &'a StoreData, site: &str) -> Option<&'a Account> {
    data.defaults.get(site).and_then(|id| data.accounts.iter().find(|a| &a.id == id)).or_else(|| data.accounts.iter().find(|a| a.site == site))
}

/// 地址所属的“网站”：能匹配内置平台时用平台 ID，否则用可注册域名。
pub fn site_for_url(url: &Url) -> Option<String> {
    if let Some(p) = providers::all().iter().find(|p| p.matches(url)) {
        return Some(p.id().to_string());
    }
    url.host_str().map(registrable_domain)
}

pub fn site_for_domain(domain: &str) -> String {
    let d = domain.trim_start_matches('.');
    Url::parse(&format!("https://{d}/")).ok().and_then(|u| site_for_url(&u)).unwrap_or_else(|| registrable_domain(d))
}

fn site_display_name(site: &str) -> String {
    providers::by_id(site).map(|p| p.name().to_string()).unwrap_or_else(|| site.to_string())
}

/// 简化版可注册域名：`www.youtube.com` → `youtube.com`，`a.bbc.co.uk` → `bbc.co.uk`。
pub fn registrable_domain(host: &str) -> String {
    let host = host.trim_start_matches('.').to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    if labels.len() <= 2 {
        return labels.join(".");
    }
    let n = labels.len();
    let second = labels[n - 2];
    let take = if labels[n - 1].len() == 2 && ["com", "net", "org", "gov", "edu", "co", "ac", "or", "ne"].contains(&second) { 3 } else { 2 };
    labels[n - take..].join(".")
}

/// 解析 `a=1; b=2` 形式的 Cookie 头。
pub fn parse_header(header: &str, domain: &str) -> Vec<StoredCookie> {
    header
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            (!name.is_empty()).then(|| StoredCookie {
                name: name.to_string(),
                value: value.trim().to_string(),
                domain: domain.trim_start_matches('.').to_string(),
                host_only: false,
                path: "/".into(),
                expires: None,
                secure: false,
                http_only: false,
            })
        })
        .collect()
}

/// 解析 Netscape cookies.txt：domain \t includeSubdomains \t path \t secure \t expires \t name \t value
pub fn parse_netscape(text: &str) -> Vec<StoredCookie> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let (http_only, line) = match line.strip_prefix("#HttpOnly_") {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            if line.trim().is_empty() || line.starts_with('#') {
                return None;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 7 {
                return None;
            }
            let include_sub = f[1].eq_ignore_ascii_case("TRUE");
            Some(StoredCookie {
                name: f[5].to_string(),
                value: f[6..].join("\t"),
                domain: f[0].trim_start_matches('.').to_ascii_lowercase(),
                host_only: !include_sub && !f[0].starts_with('.'),
                path: if f[2].is_empty() { "/".into() } else { f[2].to_string() },
                expires: f[4].parse::<i64>().ok().filter(|t| *t > 0),
                secure: f[3].eq_ignore_ascii_case("TRUE"),
                http_only,
            })
        })
        .collect()
}

/// 生成 Netscape cookies.txt（交给 yt-dlp 使用）。
pub fn to_netscape(cookies: &[StoredCookie]) -> String {
    let mut out = String::from("# Netscape HTTP Cookie File\n# Generated by ClearClip. Do not share.\n\n");
    for c in cookies {
        let domain = if c.host_only { c.domain.clone() } else { format!(".{}", c.domain) };
        let prefix = if c.http_only { "#HttpOnly_" } else { "" };
        out.push_str(&format!(
            "{prefix}{domain}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            if c.host_only { "FALSE" } else { "TRUE" },
            c.path,
            if c.secure { "TRUE" } else { "FALSE" },
            c.expires.unwrap_or(0),
            c.name,
            c.value
        ));
    }
    out
}

/// 从 Tauri 登录窗口读到的 Cookie 转换过来。
pub fn from_webview(c: &tauri::webview::Cookie<'static>) -> Option<StoredCookie> {
    let domain = c.domain()?.trim_start_matches('.').to_ascii_lowercase();
    Some(StoredCookie {
        name: c.name().to_string(),
        value: c.value().to_string(),
        host_only: !c.domain()?.starts_with('.'),
        domain,
        path: c.path().unwrap_or("/").to_string(),
        expires: c.expires_datetime().map(|d| d.unix_timestamp()),
        secure: c.secure().unwrap_or(false),
        http_only: c.http_only().unwrap_or(false),
    })
}

pub fn store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("cookies.enc")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ck(name: &str, domain: &str, host_only: bool) -> StoredCookie {
        StoredCookie {
            name: name.into(),
            value: "v".into(),
            domain: domain.into(),
            host_only,
            path: "/".into(),
            expires: None,
            secure: false,
            http_only: false,
        }
    }

    #[test]
    fn login_cookie_check() {
        let store = CookieStore::in_memory();
        // 只有访客 Cookie：不算登录
        let id = store.upsert("bilibili", "默认", vec![ck("buvid3", "bilibili.com", false)]).unwrap();
        assert_eq!(store.has_login_cookie(&id), Some(false));
        // 有登录 Cookie
        let id = store.upsert("bilibili", "大会员", vec![ck("SESSDATA", "bilibili.com", false)]).unwrap();
        assert_eq!(store.has_login_cookie(&id), Some(true));
        // 登录 Cookie 已过期
        let mut expired = ck("SESSDATA", "bilibili.com", false);
        expired.expires = Some(100);
        let id = store.upsert("bilibili", "过期", vec![expired]).unwrap();
        assert_eq!(store.has_login_cookie(&id), Some(false));
        // 不在已知列表的网站无法本地检查
        let id = store.upsert("example.com", "x", vec![ck("sid", "example.com", false)]).unwrap();
        assert_eq!(store.has_login_cookie(&id), None);
        assert!(!store.summaries().iter().find(|a| a.id == id).unwrap().checkable);
    }

    #[test]
    fn check_result_is_recorded_and_reset_on_new_login() {
        let store = CookieStore::in_memory();
        let id = store.upsert("youtube.com", "默认", vec![ck("SAPISID", "youtube.com", false)]).unwrap();
        assert_eq!(store.set_check_result(&id, false, None).unwrap(), None);
        let s = store.summaries();
        assert_eq!(s[0].valid, Some(false));
        assert!(s[0].checked_at.is_some());
        // 重新登录保存后，之前的检查结果作废
        store.upsert("youtube.com", "默认", vec![ck("SAPISID", "youtube.com", false)]).unwrap();
        assert_eq!(store.summaries()[0].valid, None);
    }

    #[test]
    fn domain_matching() {
        let now = 1_000;
        let u = Url::parse("https://api.bilibili.com/x").unwrap();
        assert!(ck("SESSDATA", "bilibili.com", false).matches(&u, now));
        assert!(!ck("SESSDATA", "bilibili.com", true).matches(&u, now), "host-only must not match subdomain");
        assert!(!ck("a", "bilivideo.com", false).matches(&u, now));
        let expired = StoredCookie { expires: Some(10), ..ck("a", "bilibili.com", false) };
        assert!(!expired.matches(&u, now));
        let secure = StoredCookie { secure: true, ..ck("a", "bilibili.com", false) };
        assert!(!secure.matches(&Url::parse("http://www.bilibili.com/").unwrap(), now));
    }

    #[test]
    fn path_matching() {
        assert!(path_matches("/a/b", "/a"));
        assert!(path_matches("/a/", "/a/"));
        assert!(!path_matches("/ab", "/a"));
    }

    #[test]
    fn header_is_not_sent_to_cdn() {
        let store = CookieStore::in_memory();
        store.upsert("bilibili", "默认", vec![ck("SESSDATA", "bilibili.com", false)]).unwrap();
        assert_eq!(store.header_for("https://api.bilibili.com/x/web-interface/view").as_deref(), Some("SESSDATA=v"));
        assert_eq!(store.header_for("https://upos-sz-mirrorcos.bilivideo.com/x.mp4"), None);
    }

    #[test]
    fn registrable_domains() {
        assert_eq!(registrable_domain("www.youtube.com"), "youtube.com");
        assert_eq!(registrable_domain("a.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(registrable_domain("m.weibo.cn"), "weibo.cn");
        assert_eq!(registrable_domain("127.0.0.1"), "127.0.0.1");
    }

    #[test]
    fn netscape_roundtrip() {
        let txt = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t1999999999\tSID\tabc\n#HttpOnly_www.pornhub.com\tFALSE\t/\tFALSE\t0\tsess\tx\ty\n\nbad line\n";
        let parsed = parse_netscape(txt);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].domain, "youtube.com");
        assert!(!parsed[0].host_only && parsed[0].secure);
        assert_eq!(parsed[1].value, "x\ty");
        assert!(parsed[1].http_only && parsed[1].host_only);
        assert_eq!(parse_netscape(&to_netscape(&parsed)), parsed);
    }

    #[test]
    fn import_groups_by_site_and_defaults() {
        let store = CookieStore::in_memory();
        let txt = ".youtube.com\tTRUE\t/\tFALSE\t0\tSID\ta\n.bilibili.com\tTRUE\t/\tFALSE\t0\tSESSDATA\tb\n";
        assert_eq!(store.import_netscape(txt, "导入").unwrap(), (2, 2));
        let s = store.summaries();
        assert!(s.iter().any(|a| a.site == "bilibili" && a.site_name == "B站" && a.is_default));
        assert!(s.iter().any(|a| a.site == "youtube.com"));
        assert_eq!(store.header_for("https://www.youtube.com/watch?v=1").as_deref(), Some("SID=a"));
    }

    #[test]
    fn multiple_accounts_and_default_switch() {
        let store = CookieStore::in_memory();
        let a = store.upsert("bilibili", "普通号", vec![StoredCookie { value: "1".into(), ..ck("SESSDATA", "bilibili.com", false) }]).unwrap();
        let b = store.upsert("bilibili", "大会员", vec![StoredCookie { value: "2".into(), ..ck("SESSDATA", "bilibili.com", false) }]).unwrap();
        assert_eq!(store.header_for("https://www.bilibili.com/").as_deref(), Some("SESSDATA=1"));
        store.set_default(&b).unwrap();
        assert_eq!(store.header_for("https://www.bilibili.com/").as_deref(), Some("SESSDATA=2"));
        store.delete(&b).unwrap();
        assert_eq!(store.header_for("https://www.bilibili.com/").as_deref(), Some("SESSDATA=1"));
        assert!(store.summaries().iter().any(|s| s.id == a && s.is_default));
    }

    #[test]
    fn merge_overwrites_rotated_cookie() {
        let store = CookieStore::in_memory();
        store.upsert("youtube.com", "默认", vec![ck("SID", "youtube.com", false), ck("HSID", "youtube.com", false)]).unwrap();
        store.merge_into_default("youtube.com", vec![StoredCookie { value: "new".into(), ..ck("SID", "youtube.com", false) }]).unwrap();
        let c = store.cookies_for_site("youtube.com");
        assert_eq!(c.len(), 2);
        assert_eq!(c.iter().find(|c| c.name == "SID").unwrap().value, "new");
    }

    #[test]
    fn encrypted_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("clearclip-cookies-{}", std::process::id()));
        let path = store_path(&dir);
        let key = [9u8; 32];
        let s = CookieStore::open(path.clone(), key, false);
        s.upsert("bilibili", "默认", vec![ck("SESSDATA", "bilibili.com", false)]).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("SESSDATA"), "must be encrypted at rest");
        let reopened = CookieStore::open(path.clone(), key, false);
        assert_eq!(reopened.summaries().len(), 1);
        let wrong = CookieStore::open(path, [1u8; 32], false);
        assert!(wrong.summaries().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_migration() {
        let store = CookieStore::in_memory();
        let mut m = HashMap::new();
        m.insert("bilibili".to_string(), "SESSDATA=x; bili_jct=y".to_string());
        assert_eq!(store.migrate_legacy(&m).unwrap(), 1);
        assert_eq!(store.header_for("https://api.bilibili.com/").as_deref(), Some("SESSDATA=x; bili_jct=y"));
    }

    #[test]
    fn clear_all_removes_every_account() {
        let store = CookieStore::in_memory();
        store.upsert("bilibili", "主号", vec![ck("SESSDATA", "bilibili.com", false)]).unwrap();
        store.upsert("youtube.com", "号二", vec![ck("SID", "youtube.com", false)]).unwrap();
        assert_eq!(store.summaries().len(), 2);
        store.clear_all().unwrap();
        assert!(store.summaries().is_empty());
        assert!(store.header_for("https://www.bilibili.com/video/BV1").is_none());
    }
}
