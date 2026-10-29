//! 网络：按网站分流（直连 / 系统代理 / 自定义代理）、客户端缓存、全局限速、同一网站的请求间隔。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use url::Url;

use crate::error::{AppError, AppResult, ErrorKind};
use crate::providers::MOBILE_UA;
use crate::settings::{NetworkSettings, Route};

#[derive(Clone)]
pub struct Clients {
    /// 解析用：有总超时
    pub api: reqwest::Client,
    /// 下载用：只有读超时，适合大文件
    pub download: reqwest::Client,
}

pub struct NetManager {
    cache: Mutex<HashMap<String, Clients>>,
    site_last: Mutex<HashMap<String, Instant>>,
    limiter: SpeedLimiter,
    /// 限速计划此刻生效的限速（KB/s）；`NO_OVERRIDE` 表示不在计划时段里，用任务自己带的全局限速
    override_limit: AtomicU64,
}

pub const NO_OVERRIDE: u64 = u64::MAX;

impl Default for NetManager {
    fn default() -> Self {
        NetManager { cache: Default::default(), site_last: Default::default(), limiter: Default::default(), override_limit: AtomicU64::new(NO_OVERRIDE) }
    }
}

impl NetManager {
    /// 设置变化后清空客户端缓存。
    pub fn invalidate(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub fn route_for(&self, net: &NetworkSettings, url: &str) -> Route {
        Url::parse(url).ok().and_then(|u| u.host_str().map(|h| net.route_for_host(h))).unwrap_or_else(|| net.default_route.clone())
    }

    /// 访问某个地址应使用的客户端。
    pub fn clients_for(&self, net: &NetworkSettings, url: &str) -> AppResult<Clients> {
        let route = self.route_for(net, url);
        self.clients_for_route(net, &route)
    }

    pub fn clients_for_route(&self, net: &NetworkSettings, route: &Route) -> AppResult<Clients> {
        let proxy_url = match route {
            Route::Proxy(id) => {
                Some(net.proxy_url(id).ok_or_else(|| AppError::invalid(format!("代理规则引用了不存在的代理“{id}”，请检查网络设置。")))?.to_string())
            }
            _ => None,
        };
        let key = format!("{route:?}|{}", proxy_url.as_deref().unwrap_or(""));
        if let Some(c) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(c.clone());
        }
        let clients = build_clients(route, proxy_url.as_deref())?;
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(key, clients.clone());
        Ok(clients)
    }

    /// 同一网站两次解析请求之间至少间隔 `interval`，降低被风控的概率。
    pub async fn wait_turn(&self, site: &str, interval: Duration) {
        if interval.is_zero() {
            return;
        }
        let wait = {
            let mut map = self.site_last.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let next = map.get(site).map(|t| *t + interval).unwrap_or(now);
            let slot = next.max(now);
            map.insert(site.to_string(), slot);
            slot.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    pub async fn throttle(&self, bytes: usize, limit_kbps: u64) {
        self.limiter.consume(bytes as u64, self.effective_limit(limit_kbps)).await;
    }

    /// 任务带的全局限速和限速计划里此刻的限速，以计划为准。
    pub fn effective_limit(&self, global_kbps: u64) -> u64 {
        match self.override_limit.load(Ordering::Relaxed) {
            NO_OVERRIDE => global_kbps,
            planned => planned,
        }
    }

    /// 按限速计划和当前时间刷新生效的限速（设置保存后、以及每隔一会儿调用）。返回此刻的限速（None 表示用全局限速）。
    pub fn refresh_limit(&self, plan: &[crate::settings::SpeedWindow]) -> Option<u64> {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let planned = crate::settings::schedule_limit(plan, now.weekday().number_from_monday() as u8, now.hour() * 60 + now.minute());
        self.override_limit.store(planned.unwrap_or(NO_OVERRIDE), Ordering::Relaxed);
        planned
    }

    /// 对所有出口（直连、系统代理、每个自定义代理）测速，找出访问这个地址最快的线路。
    pub async fn speed_test(&self, net: &NetworkSettings, url: &str) -> Vec<RouteSpeed> {
        let mut routes: Vec<(Route, String)> = vec![(Route::Direct, "直连".into()), (Route::System, "系统代理".into())];
        routes.extend(net.proxies.iter().map(|p| (Route::Proxy(p.id.clone()), format!("代理：{}", p.name))));
        let mut out = vec![];
        for (route, name) in routes {
            let r = match self.clients_for_route(net, &route) {
                Ok(c) => measure(&c.download, url, 1_500_000, 6).await,
                Err(e) => Err(e.message),
            };
            out.push(match r {
                Ok((ttfb_ms, bytes, kbps)) => RouteSpeed { route, name, ok: true, ttfb_ms, kbps, bytes, error: None },
                Err(error) => RouteSpeed { route, name, ok: false, ttfb_ms: 0, kbps: 0, bytes: 0, error: Some(error) },
            });
        }
        out
    }

    /// 测试某个地址在当前规则下能否访问，返回耗时（毫秒）。
    pub async fn test(&self, net: &NetworkSettings, url: &str) -> AppResult<RouteTest> {
        let route = self.route_for(net, url);
        let clients = self.clients_for_route(net, &route)?;
        let t0 = Instant::now();
        let resp = clients.api.get(url).timeout(Duration::from_secs(10)).send().await.map_err(|e| {
            let msg = if e.is_timeout() {
                "连接超时".to_string()
            } else if e.is_connect() {
                "无法连接（代理不可用或网站不可达）".to_string()
            } else {
                e.without_url().to_string()
            };
            AppError::new(ErrorKind::Network, msg)
        })?;
        Ok(RouteTest { route, status: resp.status().as_u16(), millis: t0.elapsed().as_millis() as u64 })
    }
}

/// 一条线路的测速结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSpeed {
    pub route: Route,
    pub name: String,
    pub ok: bool,
    /// 收到第一个字节的耗时（毫秒）
    pub ttfb_ms: u64,
    /// 下载速度（KB/s）
    pub kbps: u64,
    pub bytes: u64,
    pub error: Option<String>,
}

/// 用一个客户端下载 `url` 的开头最多 `max_bytes` 字节（最多 `max_secs` 秒），测首字节耗时和速度。
pub async fn measure(client: &reqwest::Client, url: &str, max_bytes: u64, max_secs: u64) -> Result<(u64, u64, u64), String> {
    let t0 = Instant::now();
    let mut resp =
        client.get(url).header("Range", format!("bytes=0-{}", max_bytes.saturating_sub(1))).timeout(Duration::from_secs(max_secs + 10)).send().await.map_err(
            |e| {
                if e.is_timeout() {
                    "连接超时".to_string()
                } else if e.is_connect() {
                    "无法连接".to_string()
                } else {
                    e.without_url().to_string()
                }
            },
        )?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("服务器返回 {status}"));
    }
    let ttfb = t0.elapsed();
    let start = Instant::now();
    let mut bytes = 0u64;
    loop {
        match tokio::time::timeout(Duration::from_secs(max_secs).saturating_sub(start.elapsed()).max(Duration::from_millis(100)), resp.chunk()).await {
            Ok(Ok(Some(c))) => {
                bytes += c.len() as u64;
                if bytes >= max_bytes || start.elapsed() >= Duration::from_secs(max_secs) {
                    break;
                }
            }
            Ok(Ok(None)) | Err(_) => break,
            Ok(Err(e)) => return Err(e.without_url().to_string()),
        }
    }
    if bytes == 0 {
        return Err("没有收到数据".into());
    }
    let secs = start.elapsed().as_secs_f64().max(0.001);
    Ok((ttfb.as_millis() as u64, bytes, (bytes as f64 / 1024.0 / secs) as u64))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTest {
    pub route: Route,
    pub status: u16,
    pub millis: u64,
}

fn build_clients(route: &Route, proxy_url: Option<&str>) -> AppResult<Clients> {
    let apply = |b: reqwest::ClientBuilder| -> AppResult<reqwest::ClientBuilder> {
        Ok(match route {
            Route::Direct => b.no_proxy(),
            // 默认构造会读取系统代理设置和 HTTP(S)_PROXY 环境变量
            Route::System => b,
            Route::Proxy(_) => {
                let url = proxy_url.unwrap_or_default();
                b.proxy(reqwest::Proxy::all(url).map_err(|e| AppError::invalid(format!("代理地址格式不正确：{e}")))?)
            }
        })
    };
    let api = apply(
        reqwest::Client::builder()
            .user_agent(MOBILE_UA)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .gzip(true),
    )?
    .build()
    .map_err(|e| AppError::msg(format!("创建网络客户端失败：{e}")))?;
    let download = apply(
        reqwest::Client::builder()
            .user_agent(MOBILE_UA)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10)),
    )?
    .build()
    .map_err(|e| AppError::msg(format!("创建网络客户端失败：{e}")))?;
    Ok(Clients { api, download })
}

/// 全局限速：以 100 毫秒为窗口分配字节额度，所有下载任务共享。
#[derive(Default)]
pub struct SpeedLimiter {
    state: Mutex<(Option<Instant>, u64)>,
}

impl SpeedLimiter {
    pub async fn consume(&self, bytes: u64, limit_kbps: u64) {
        if limit_kbps == 0 {
            return;
        }
        let per_window = (limit_kbps * 1024 / 10).max(1);
        loop {
            let wait = {
                let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
                let now = Instant::now();
                let start = *st.0.get_or_insert(now);
                if now.duration_since(start) >= Duration::from_millis(100) {
                    *st = (Some(now), 0);
                }
                if st.1 < per_window {
                    st.1 += bytes;
                    None
                } else {
                    Some(Duration::from_millis(100).saturating_sub(now.duration_since(st.0.unwrap_or(now))))
                }
            };
            match wait {
                None => return,
                Some(d) => tokio::time::sleep(d.max(Duration::from_millis(5))).await,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ProxyDef, RouteRule};
    use std::sync::Arc;

    #[test]
    fn missing_proxy_is_a_clear_error() {
        let net = NetworkSettings { default_route: Route::Proxy("nope".into()), rules: vec![], proxies: vec![] };
        let err = NetManager::default().clients_for(&net, "https://x.com/").err().unwrap();
        assert!(err.to_string().contains("不存在的代理"));
    }

    #[test]
    fn clients_are_cached_per_route() {
        let net = NetworkSettings {
            default_route: Route::System,
            rules: vec![RouteRule { pattern: "a.com".into(), route: Route::Proxy("p".into()) }],
            proxies: vec![ProxyDef { id: "p".into(), name: "本地".into(), url: "http://127.0.0.1:7890".into() }],
        };
        let m = NetManager::default();
        m.clients_for(&net, "https://a.com/").unwrap();
        m.clients_for(&net, "https://b.a.com/").unwrap();
        m.clients_for(&net, "https://c.com/").unwrap();
        assert_eq!(m.cache.lock().unwrap().len(), 2);
        m.invalidate();
        assert!(m.cache.lock().unwrap().is_empty());
    }

    #[test]
    fn invalid_proxy_url_is_reported() {
        let net = NetworkSettings {
            default_route: Route::Proxy("p".into()),
            rules: vec![],
            proxies: vec![ProxyDef { id: "p".into(), name: "x".into(), url: "::bad::".into() }],
        };
        assert!(NetManager::default().clients_for(&net, "https://x.com/").is_err());
    }

    #[tokio::test]
    async fn wait_turn_spaces_requests() {
        let m = NetManager::default();
        let t0 = Instant::now();
        m.wait_turn("bilibili", Duration::from_millis(120)).await;
        m.wait_turn("bilibili", Duration::from_millis(120)).await;
        m.wait_turn("other", Duration::from_millis(120)).await;
        let e = t0.elapsed();
        assert!(e >= Duration::from_millis(110) && e < Duration::from_millis(400), "{e:?}");
    }

    #[tokio::test]
    async fn speed_limiter_slows_down() {
        let l = SpeedLimiter::default();
        let t0 = Instant::now();
        // 限速 100 KB/s，消耗 30 KB 约需 0.2–0.3 秒
        for _ in 0..30 {
            l.consume(1024, 100).await;
        }
        assert!(t0.elapsed() >= Duration::from_millis(180), "{:?}", t0.elapsed());
        let t1 = Instant::now();
        l.consume(10_000_000, 0).await;
        assert!(t1.elapsed() < Duration::from_millis(10));
    }

    /// 本地“源站”：任何请求都回 `hello from origin`。
    async fn origin() -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                tokio::spawn(async move {
                    let mut b = [0u8; 2048];
                    let _ = s.read(&mut b).await;
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\nhello from origin").await;
                });
            }
        });
        addr
    }

    /// 最小的 HTTP 代理：收到绝对地址的 GET 后，把请求转给本地源站，并记下被请求的主机名。
    async fn http_proxy(origin: std::net::SocketAddr, seen: Arc<Mutex<Vec<String>>>) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut b = [0u8; 4096];
                    let n = s.read(&mut b).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&b[..n]).into_owned();
                    let target = req.split_whitespace().nth(1).unwrap_or("").to_string();
                    seen.lock().unwrap().push(format!("{target} auth={}", req.to_ascii_lowercase().contains("proxy-authorization: basic")));
                    let Ok(mut up) = tokio::net::TcpStream::connect(origin).await else { return };
                    let _ = up.write_all(b"GET / HTTP/1.1\r\nHost: origin\r\nConnection: close\r\n\r\n").await;
                    let mut resp = vec![];
                    let _ = up.read_to_end(&mut resp).await;
                    let _ = s.write_all(&resp).await;
                });
            }
        });
        addr
    }

    /// 最小的 SOCKS5 代理（支持无认证和用户名密码）：把 CONNECT 的目标一律接到本地源站，并记下目标主机名。
    async fn socks5_proxy(origin: std::net::SocketAddr, creds: Option<(&'static str, &'static str)>, seen: Arc<Mutex<Vec<String>>>) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut head = [0u8; 2];
                    if s.read_exact(&mut head).await.is_err() {
                        return;
                    }
                    let mut methods = vec![0u8; head[1] as usize];
                    let _ = s.read_exact(&mut methods).await;
                    if let Some((u, p)) = creds {
                        if !methods.contains(&2) {
                            let _ = s.write_all(&[5, 0xff]).await;
                            return;
                        }
                        let _ = s.write_all(&[5, 2]).await;
                        let mut v = [0u8; 2];
                        let _ = s.read_exact(&mut v).await;
                        let mut user = vec![0u8; v[1] as usize];
                        let _ = s.read_exact(&mut user).await;
                        let mut pl = [0u8; 1];
                        let _ = s.read_exact(&mut pl).await;
                        let mut pass = vec![0u8; pl[0] as usize];
                        let _ = s.read_exact(&mut pass).await;
                        let ok = user == u.as_bytes() && pass == p.as_bytes();
                        let _ = s.write_all(&[1, if ok { 0 } else { 1 }]).await;
                        if !ok {
                            return;
                        }
                    } else {
                        let _ = s.write_all(&[5, 0]).await;
                    }
                    let mut req = [0u8; 4];
                    if s.read_exact(&mut req).await.is_err() {
                        return;
                    }
                    let host = match req[3] {
                        1 => {
                            let mut a = [0u8; 4];
                            let _ = s.read_exact(&mut a).await;
                            std::net::Ipv4Addr::from(a).to_string()
                        }
                        3 => {
                            let mut l = [0u8; 1];
                            let _ = s.read_exact(&mut l).await;
                            let mut d = vec![0u8; l[0] as usize];
                            let _ = s.read_exact(&mut d).await;
                            String::from_utf8_lossy(&d).into_owned()
                        }
                        _ => return,
                    };
                    let mut port = [0u8; 2];
                    let _ = s.read_exact(&mut port).await;
                    seen.lock().unwrap().push(host);
                    let Ok(mut up) = tokio::net::TcpStream::connect(origin).await else { return };
                    let _ = s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await;
                    let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
                });
            }
        });
        addr
    }

    fn net_with(rules: Vec<RouteRule>, url: String) -> NetworkSettings {
        NetworkSettings { default_route: Route::Direct, rules, proxies: vec![ProxyDef { id: "p".into(), name: "测试代理".into(), url }] }
    }

    async fn fetch(net: &NetworkSettings, url: &str) -> Result<String, String> {
        let c = NetManager::default().clients_for(net, url).map_err(|e| e.to_string())?.download;
        let resp = c.get(url).send().await.map_err(|e| e.to_string())?;
        resp.text().await.map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn http_proxy_is_used_only_for_the_sites_routed_to_it() {
        let seen = Arc::new(Mutex::new(vec![]));
        let proxy = http_proxy(origin().await, seen.clone()).await;
        let net = net_with(vec![RouteRule { pattern: "proxied.test".into(), route: Route::Proxy("p".into()) }], format!("http://u:p@{proxy}"));
        assert_eq!(fetch(&net, "http://www.proxied.test/file").await.unwrap(), "hello from origin");
        let log = seen.lock().unwrap().clone();
        assert_eq!(log, vec!["http://www.proxied.test/file auth=true".to_string()], "走代理并带上了代理账号");
        // 没有命中规则的网站直连：.test 域名解析不了，说明确实没走代理
        assert!(fetch(&net, "http://other.test/file").await.is_err());
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn socks5_proxy_works_with_remote_dns_and_with_credentials() {
        let seen = Arc::new(Mutex::new(vec![]));
        let rules = vec![RouteRule { pattern: "proxied.test".into(), route: Route::Proxy("p".into()) }];
        // socks5h：域名交给代理解析（本机解析不了 .test，也能连上）
        let open = socks5_proxy(origin().await, None, seen.clone()).await;
        let net = net_with(rules.clone(), format!("socks5h://{open}"));
        assert_eq!(fetch(&net, "http://proxied.test/x").await.unwrap(), "hello from origin");
        assert_eq!(seen.lock().unwrap().clone(), vec!["proxied.test".to_string()]);
        // 需要用户名密码：对了能通，错了被拒绝
        let locked = socks5_proxy(origin().await, Some(("me", "secret")), seen.clone()).await;
        let good = net_with(rules.clone(), format!("socks5h://me:secret@{locked}"));
        assert_eq!(fetch(&good, "http://proxied.test/y").await.unwrap(), "hello from origin");
        let bad = net_with(rules, format!("socks5h://me:wrong@{locked}"));
        assert!(fetch(&bad, "http://proxied.test/y").await.is_err());
    }

    #[tokio::test]
    async fn measure_reports_first_byte_and_speed() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut b = [0u8; 2048];
                    let n = s.read(&mut b).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&b[..n]).into_owned();
                    if req.contains("/missing") {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                    let body = vec![7u8; 3_000_000];
                    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await;
                    let _ = s.write_all(&body).await;
                });
            }
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let (ttfb, bytes, kbps) = measure(&client, &format!("http://{addr}/big"), 1_000_000, 5).await.unwrap();
        assert!((1_000_000..=3_000_000).contains(&bytes), "stops after the cap: {bytes}");
        assert!(ttfb < 2000 && kbps > 100, "{ttfb} ms, {kbps} KB/s");
        assert_eq!(measure(&client, &format!("http://{addr}/missing"), 1000, 2).await.unwrap_err(), "服务器返回 404");
        assert_eq!(measure(&client, "http://127.0.0.1:1/x", 1000, 2).await.unwrap_err(), "无法连接");
        // 完整的测速：直连一定能通
        let m = NetManager::default();
        let results = m.speed_test(&NetworkSettings::default(), &format!("http://{addr}/big")).await;
        assert_eq!(results.len(), 2, "direct and system, no custom proxies");
        assert!(results[0].ok && results[0].name == "直连" && results[0].kbps > 0);
        // 限速计划覆盖全局限速
        assert_eq!(m.effective_limit(100), 100);
        m.override_limit.store(500, Ordering::Relaxed);
        assert_eq!(m.effective_limit(100), 500, "the plan wins over the global limit");
        m.override_limit.store(0, Ordering::Relaxed);
        assert_eq!(m.effective_limit(100), 0, "0 means unlimited in that window");
    }
}
