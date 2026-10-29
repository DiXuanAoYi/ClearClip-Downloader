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
