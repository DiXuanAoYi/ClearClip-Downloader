//! 给 ffmpeg 用的本机代理桥：ffmpeg 只认 HTTP 代理（`-http_proxy`），也不会走 SOCKS5，RTMP / RTSP 则完全没有代理支持。
//! 桥在 127.0.0.1 上监听，把连接经设置里的 HTTP / SOCKS5 代理转发出去：
//! - `http_proxy`：当作 HTTP 代理（CONNECT 和绝对地址的普通请求），给 http / https 地址用；
//! - `forward`：固定转发到一个主机和端口，给 rtmp / rtsp 用（把地址的主机改成桥的地址）。

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use base64::Engine as _;
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{lookup_host, TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Http,
    /// `remote_dns` 为真（`socks5h://`）时把域名交给代理解析，否则在本机解析
    Socks5 {
        remote_dns: bool,
    },
}

/// 设置里的一个代理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    kind: Kind,
    host: String,
    port: u16,
    auth: Option<(String, String)>,
}

impl Upstream {
    /// 解析 `http://用户:密码@主机:端口`、`socks5://…`、`socks5h://…`。`https://` 的代理（到代理本身是 TLS）不支持。
    pub fn parse(s: &str) -> Option<Upstream> {
        let u = Url::parse(s.trim()).ok()?;
        let kind = match u.scheme() {
            "http" => Kind::Http,
            "socks5" => Kind::Socks5 { remote_dns: false },
            "socks5h" => Kind::Socks5 { remote_dns: true },
            _ => return None,
        };
        let host = u.host_str()?.trim_matches(['[', ']']).to_string();
        let port = u.port().unwrap_or(if kind == Kind::Http { 80 } else { 1080 });
        let auth = (!u.username().is_empty()).then(|| {
            let dec = |s: &str| percent_decode(s);
            (dec(u.username()), dec(u.password().unwrap_or("")))
        });
        Some(Upstream { kind, host, port, auth })
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() && hex(b[i + 1]).is_some() && hex(b[i + 2]).is_some() {
            out.push((hex(b[i + 1]).unwrap() * 16 + hex(b[i + 2]).unwrap()) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

/// 经代理连到目标主机和端口。
pub async fn connect_via(up: &Upstream, host: &str, port: u16) -> io::Result<TcpStream> {
    let mut s = TcpStream::connect((up.host.as_str(), up.port)).await.map_err(|e| other(format!("无法连接代理 {}:{}：{e}", up.host, up.port)))?;
    match &up.kind {
        Kind::Http => {
            let mut req = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
            if let Some((u, p)) = &up.auth {
                let token = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{p}"));
                req.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
            }
            req.push_str("\r\n");
            s.write_all(req.as_bytes()).await?;
            // 逐字节读到响应头结束，免得多读走隧道里的数据
            let mut head = Vec::new();
            let mut b = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if head.len() > 8192 || s.read(&mut b).await? == 0 {
                    return Err(other("代理的响应不完整"));
                }
                head.push(b[0]);
            }
            let status = String::from_utf8_lossy(&head);
            let code = status.split_whitespace().nth(1).unwrap_or("");
            if code != "200" {
                return Err(other(format!("代理拒绝了连接（{}）", status.lines().next().unwrap_or(""))));
            }
        }
        Kind::Socks5 { remote_dns } => {
            let methods: &[u8] = if up.auth.is_some() { &[5, 2, 0, 2] } else { &[5, 1, 0] };
            s.write_all(methods).await?;
            let mut r = [0u8; 2];
            s.read_exact(&mut r).await?;
            match r {
                [5, 0] => {}
                [5, 2] => {
                    let (u, p) = up.auth.as_ref().ok_or_else(|| other("SOCKS5 代理要求用户名和密码"))?;
                    if u.len() > 255 || p.len() > 255 {
                        return Err(other("SOCKS5 的用户名或密码太长"));
                    }
                    let mut m = vec![1, u.len() as u8];
                    m.extend_from_slice(u.as_bytes());
                    m.push(p.len() as u8);
                    m.extend_from_slice(p.as_bytes());
                    s.write_all(&m).await?;
                    s.read_exact(&mut r).await?;
                    if r[1] != 0 {
                        return Err(other("SOCKS5 代理的用户名或密码不对"));
                    }
                }
                _ => return Err(other("SOCKS5 代理不接受这种认证方式")),
            }
            let mut m = vec![5, 1, 0];
            let ip = if *remote_dns { host.parse::<IpAddr>().ok() } else { Some(resolve_local(host, port).await?) };
            match ip {
                Some(IpAddr::V4(a)) => {
                    m.push(1);
                    m.extend_from_slice(&a.octets());
                }
                Some(IpAddr::V6(a)) => {
                    m.push(4);
                    m.extend_from_slice(&a.octets());
                }
                None => {
                    if host.len() > 255 {
                        return Err(other("域名太长"));
                    }
                    m.push(3);
                    m.push(host.len() as u8);
                    m.extend_from_slice(host.as_bytes());
                }
            }
            m.extend_from_slice(&port.to_be_bytes());
            s.write_all(&m).await?;
            let mut h = [0u8; 4];
            s.read_exact(&mut h).await?;
            if h[1] != 0 {
                return Err(other(format!("SOCKS5 代理无法连接目标（错误码 {}）", h[1])));
            }
            let skip = match h[3] {
                1 => 4,
                4 => 16,
                3 => {
                    let mut n = [0u8; 1];
                    s.read_exact(&mut n).await?;
                    n[0] as usize
                }
                _ => return Err(other("SOCKS5 代理的响应无法识别")),
            } + 2;
            let mut rest = vec![0u8; skip];
            s.read_exact(&mut rest).await?;
        }
    }
    Ok(s)
}

async fn resolve_local(host: &str, port: u16) -> io::Result<IpAddr> {
    if let Ok(ip) = host.parse() {
        return Ok(ip);
    }
    lookup_host((host, port)).await?.next().map(|a| a.ip()).ok_or_else(|| other(format!("无法解析 {host}")))
}

/// 一个正在监听的桥；丢弃时停止监听并断开所有转发中的连接。
pub struct Bridge {
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Bridge {
    /// 当作 HTTP 代理使用（`http_proxy` 参数填 `http://127.0.0.1:端口`）。
    pub async fn http_proxy(up: Upstream) -> io::Result<Bridge> {
        Self::start(up, None).await
    }

    /// 固定转发到某个主机和端口。
    pub async fn forward(up: Upstream, host: &str, port: u16) -> io::Result<Bridge> {
        Self::start(up, Some((host.to_string(), port))).await
    }

    async fn start(up: Upstream, target: Option<(String, u16)>) -> io::Result<Bridge> {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = l.local_addr()?;
        let task = tokio::spawn(async move {
            // 连接任务放在 JoinSet 里：桥被丢弃、监听任务被取消时一起结束
            let mut conns = JoinSet::new();
            loop {
                tokio::select! {
                    a = l.accept() => {
                        let Ok((c, _)) = a else { break };
                        let (up, target) = (up.clone(), target.clone());
                        conns.spawn(async move {
                            let _ = match target {
                                Some((h, p)) => forward_conn(c, &up, &h, p).await,
                                None => proxy_conn(c, &up).await,
                            };
                        });
                    }
                    Some(_) = conns.join_next(), if !conns.is_empty() => {}
                }
            }
        });
        Ok(Bridge { addr, task })
    }
}

async fn forward_conn(mut c: TcpStream, up: &Upstream, host: &str, port: u16) -> io::Result<()> {
    let mut s = connect_via(up, host, port).await?;
    copy_bidirectional(&mut c, &mut s).await.map(|_| ())
}

/// 解析 `host:port`（含方括号的 IPv6）。
fn split_authority(a: &str, default_port: u16) -> Option<(String, u16)> {
    let u = Url::parse(&format!("http://{a}/")).ok()?;
    Some((u.host_str()?.trim_matches(['[', ']']).to_string(), u.port().unwrap_or(default_port)))
}

async fn proxy_conn(mut c: TcpStream, up: &Upstream) -> io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > 32 * 1024 {
            return Err(other("请求头太长"));
        }
        let n = c.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).into_owned();
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    let (method, target, version) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or("HTTP/1.1"));
    if method.eq_ignore_ascii_case("CONNECT") {
        let (h, p) = split_authority(target, 443).ok_or_else(|| other("CONNECT 的目标地址不对"))?;
        match connect_via(up, &h, p).await {
            Ok(mut s) => {
                c.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;
                // CONNECT 之后客户端多发的字节（一般没有）也带过去
                if buf.len() > end {
                    s.write_all(&buf[end..]).await?;
                }
                copy_bidirectional(&mut c, &mut s).await.map(|_| ())
            }
            Err(e) => {
                let _ = c.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n").await;
                Err(e)
            }
        }
    } else {
        // 普通请求：`GET http://主机/路径 HTTP/1.1` 改成 `GET /路径 HTTP/1.1` 再转发
        let u = Url::parse(target).map_err(|_| other("代理请求的地址不对"))?;
        let host = u.host_str().ok_or_else(|| other("代理请求缺少主机"))?.trim_matches(['[', ']']).to_string();
        let port = u.port_or_known_default().unwrap_or(80);
        let mut path = u.path().to_string();
        if let Some(q) = u.query() {
            path.push('?');
            path.push_str(q);
        }
        let mut s = match connect_via(up, &host, port).await {
            Ok(s) => s,
            Err(e) => {
                let _ = c.write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n").await;
                return Err(e);
            }
        };
        let mut out = format!("{method} {path} {version}\r\n").into_bytes();
        for l in head.lines().skip(1) {
            if !l.to_ascii_lowercase().starts_with("proxy-") {
                out.extend_from_slice(l.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&buf[end..]);
        s.write_all(&out).await?;
        copy_bidirectional(&mut c, &mut s).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// 回显服务器：把收到的每段数据加前缀 `echo:` 发回去。
    async fn echo() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                tokio::spawn(async move {
                    let mut b = [0u8; 256];
                    while let Ok(n) = s.read(&mut b).await {
                        if n == 0 || s.write_all(&[b"echo:".as_slice(), &b[..n]].concat()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        port
    }

    /// 模拟 HTTP 代理（只支持 CONNECT），记录收到的认证头和连接次数。
    async fn http_upstream(seen: Arc<std::sync::Mutex<Vec<String>>>, hits: Arc<AtomicUsize>) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut c, _)) = l.accept().await else { return };
                let (seen, hits) = (seen.clone(), hits.clone());
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        if c.read(&mut b).await.unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(b[0]);
                    }
                    hits.fetch_add(1, Ordering::SeqCst);
                    let text = String::from_utf8_lossy(&head).into_owned();
                    seen.lock().unwrap().push(text.clone());
                    let target = text.split_whitespace().nth(1).unwrap_or("").to_string();
                    let Ok(mut s) = TcpStream::connect(target).await else { return };
                    let _ = c.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
                    let _ = copy_bidirectional(&mut c, &mut s).await;
                });
            }
        });
        port
    }

    /// 模拟 SOCKS5 代理（可要求用户名密码），记录收到的目标。
    async fn socks_upstream(auth: Option<(&'static str, &'static str)>, targets: Arc<std::sync::Mutex<Vec<String>>>) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut c, _)) = l.accept().await else { return };
                let targets = targets.clone();
                tokio::spawn(async move {
                    let mut g = [0u8; 2];
                    c.read_exact(&mut g).await.unwrap();
                    let mut methods = vec![0u8; g[1] as usize];
                    c.read_exact(&mut methods).await.unwrap();
                    if let Some((u, p)) = auth {
                        assert!(methods.contains(&2));
                        c.write_all(&[5, 2]).await.unwrap();
                        let mut v = [0u8; 2];
                        c.read_exact(&mut v).await.unwrap();
                        let mut user = vec![0u8; v[1] as usize];
                        c.read_exact(&mut user).await.unwrap();
                        let mut pl = [0u8; 1];
                        c.read_exact(&mut pl).await.unwrap();
                        let mut pass = vec![0u8; pl[0] as usize];
                        c.read_exact(&mut pass).await.unwrap();
                        let ok = user == u.as_bytes() && pass == p.as_bytes();
                        c.write_all(&[1, if ok { 0 } else { 1 }]).await.unwrap();
                        if !ok {
                            return;
                        }
                    } else {
                        c.write_all(&[5, 0]).await.unwrap();
                    }
                    let mut h = [0u8; 4];
                    c.read_exact(&mut h).await.unwrap();
                    let (host, port) = match h[3] {
                        1 => {
                            let mut a = [0u8; 4];
                            c.read_exact(&mut a).await.unwrap();
                            let mut p = [0u8; 2];
                            c.read_exact(&mut p).await.unwrap();
                            (Ipv4Addr::from(a).to_string(), u16::from_be_bytes(p))
                        }
                        4 => {
                            let mut a = [0u8; 16];
                            c.read_exact(&mut a).await.unwrap();
                            let mut p = [0u8; 2];
                            c.read_exact(&mut p).await.unwrap();
                            ("ip6".to_string(), u16::from_be_bytes(p))
                        }
                        3 => {
                            let mut n = [0u8; 1];
                            c.read_exact(&mut n).await.unwrap();
                            let mut name = vec![0u8; n[0] as usize];
                            c.read_exact(&mut name).await.unwrap();
                            let mut p = [0u8; 2];
                            c.read_exact(&mut p).await.unwrap();
                            (String::from_utf8(name).unwrap(), u16::from_be_bytes(p))
                        }
                        _ => return,
                    };
                    targets.lock().unwrap().push(format!("{host}:{port}"));
                    // 测试里的域名都当作本机
                    let real = if host == "localhost" || host == "ip6" || host.parse::<Ipv4Addr>().is_ok() { "127.0.0.1".to_string() } else { host.clone() };
                    let Ok(mut s) = TcpStream::connect((real.as_str(), port)).await else { return };
                    c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
                    let _ = copy_bidirectional(&mut c, &mut s).await;
                });
            }
        });
        port
    }

    async fn talk(addr: SocketAddr, msg: &[u8]) -> Vec<u8> {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(msg).await.unwrap();
        let mut out = vec![0u8; 5 + msg.len()];
        s.read_exact(&mut out).await.unwrap();
        out
    }

    #[test]
    fn parses_proxy_urls() {
        let u = Upstream::parse("socks5h://us%40er:p%3Aw@127.0.0.1:1081").unwrap();
        assert_eq!(u.kind, Kind::Socks5 { remote_dns: true });
        assert_eq!((u.host.as_str(), u.port), ("127.0.0.1", 1081));
        assert_eq!(u.auth, Some(("us@er".into(), "p:w".into())));
        assert_eq!(Upstream::parse("socks5://h").unwrap().port, 1080);
        assert_eq!(Upstream::parse("http://h").unwrap().port, 80);
        assert!(Upstream::parse("https://h:1").is_none());
        assert!(Upstream::parse("not a url").is_none());
    }

    #[tokio::test]
    async fn forwards_through_an_http_proxy_with_credentials() {
        let seen = Arc::new(std::sync::Mutex::new(vec![]));
        let hits = Arc::new(AtomicUsize::new(0));
        let proxy = http_upstream(seen.clone(), hits.clone()).await;
        let target = echo().await;
        let up = Upstream::parse(&format!("http://alice:secret@127.0.0.1:{proxy}")).unwrap();
        let bridge = Bridge::forward(up, "127.0.0.1", target).await.unwrap();
        assert_eq!(talk(bridge.addr, b"hello").await, b"echo:hello");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let head = seen.lock().unwrap()[0].clone();
        assert!(head.starts_with(&format!("CONNECT 127.0.0.1:{target} ")), "{head}");
        // base64("alice:secret")
        assert!(head.contains("Proxy-Authorization: Basic YWxpY2U6c2VjcmV0"), "{head}");
    }

    #[tokio::test]
    async fn forwards_through_socks5_with_local_and_remote_dns() {
        let targets = Arc::new(std::sync::Mutex::new(vec![]));
        let proxy = socks_upstream(Some(("bob", "pw")), targets.clone()).await;
        let target = echo().await;
        // socks5://：本机解析 localhost，代理收到的是 IP
        let up = Upstream::parse(&format!("socks5://bob:pw@127.0.0.1:{proxy}")).unwrap();
        let b = Bridge::forward(up, "localhost", target).await.unwrap();
        assert_eq!(talk(b.addr, b"a").await, b"echo:a");
        // socks5h://：域名原样交给代理
        let up = Upstream::parse(&format!("socks5h://bob:pw@127.0.0.1:{proxy}")).unwrap();
        let b = Bridge::forward(up, "localhost", target).await.unwrap();
        assert_eq!(talk(b.addr, b"b").await, b"echo:b");
        let t = targets.lock().unwrap().clone();
        assert_eq!(t.len(), 2);
        assert!(!t[0].starts_with("localhost"), "{t:?}");
        assert_eq!(t[1], format!("localhost:{target}"));
        // 密码不对：连接被拒绝，桥不会把数据转出去
        let bad = Upstream::parse(&format!("socks5://bob:nope@127.0.0.1:{proxy}")).unwrap();
        assert!(connect_via(&bad, "127.0.0.1", target).await.is_err());
    }

    #[tokio::test]
    async fn acts_as_an_http_proxy_for_connect_and_plain_requests() {
        let seen = Arc::new(std::sync::Mutex::new(vec![]));
        let hits = Arc::new(AtomicUsize::new(0));
        let proxy = http_upstream(seen, hits.clone()).await;
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let oport = origin.local_addr().unwrap().port();
        let got = Arc::new(std::sync::Mutex::new(String::new()));
        let got2 = got.clone();
        tokio::spawn(async move {
            let (mut s, _) = origin.accept().await.unwrap();
            let mut b = [0u8; 2048];
            let n = s.read(&mut b).await.unwrap();
            *got2.lock().unwrap() = String::from_utf8_lossy(&b[..n]).into_owned();
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await.unwrap();
        });
        let up = Upstream::parse(&format!("http://127.0.0.1:{proxy}")).unwrap();
        let bridge = Bridge::http_proxy(up).await.unwrap();
        // 普通请求：绝对地址被改成路径，Proxy-* 头被去掉
        let mut s = TcpStream::connect(bridge.addr).await.unwrap();
        s.write_all(format!("GET http://127.0.0.1:{oport}/a/b?x=1 HTTP/1.1\r\nHost: 127.0.0.1:{oport}\r\nProxy-Connection: keep-alive\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut resp = String::new();
        s.read_to_string(&mut resp).await.unwrap();
        assert!(resp.ends_with("ok"), "{resp}");
        let req = got.lock().unwrap().clone();
        assert!(req.starts_with("GET /a/b?x=1 HTTP/1.1\r\n"), "{req}");
        assert!(!req.to_ascii_lowercase().contains("proxy-connection"), "{req}");
        // CONNECT：返回 200 后是透明隧道
        let target = echo().await;
        let mut s = TcpStream::connect(bridge.addr).await.unwrap();
        s.write_all(format!("CONNECT 127.0.0.1:{target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).await.unwrap();
        let mut head = [0u8; 39];
        s.read_exact(&mut head).await.unwrap();
        assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 200"));
        s.write_all(b"hi").await.unwrap();
        let mut r = [0u8; 7];
        s.read_exact(&mut r).await.unwrap();
        assert_eq!(&r, b"echo:hi");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dropping_the_bridge_stops_listening() {
        let up = Upstream::parse("socks5://127.0.0.1:9").unwrap();
        let b = Bridge::forward(up, "127.0.0.1", 9).await.unwrap();
        let addr = b.addr;
        drop(b);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(TcpStream::connect(addr).await.is_err());
    }
}
