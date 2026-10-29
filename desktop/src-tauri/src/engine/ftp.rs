//! FTP 下载：自带的精简客户端（明文 FTP，被动模式），支持断点续传（REST）、限速、暂停 / 取消。
//! 只做下载需要的几条命令：USER / PASS / TYPE / SIZE / REST / EPSV / PASV / RETR。

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::watch;
use url::Url;

use super::http::HttpOptions;
use super::{ensure_space, wait_ctrl, DlError, ResumeMeta};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

fn net<E: std::fmt::Display>(e: E) -> DlError {
    DlError::Net(e.to_string())
}

/// 一条（可能多行的）服务器回复。
#[derive(Debug)]
struct Reply {
    code: u16,
    text: String,
}

struct Control {
    rd: BufReader<tokio::net::tcp::OwnedReadHalf>,
    wr: tokio::net::tcp::OwnedWriteHalf,
    peer: IpAddr,
}

impl Control {
    async fn connect(host: &str, port: u16) -> Result<Control, DlError> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
            .await
            .map_err(|_| DlError::Net(format!("连接 {host}:{port} 超时")))?
            .map_err(|e| DlError::Net(format!("无法连接 {host}:{port}：{e}")))?;
        let _ = stream.set_nodelay(true);
        let peer = stream.peer_addr().map_err(net)?.ip();
        let (r, wr) = stream.into_split();
        Ok(Control { rd: BufReader::new(r), wr, peer })
    }

    async fn read_reply(&mut self) -> Result<Reply, DlError> {
        let mut first: Option<(u16, bool)> = None;
        let mut text = String::new();
        loop {
            let mut line = String::new();
            let n = tokio::time::timeout(IDLE_TIMEOUT, self.rd.read_line(&mut line))
                .await
                .map_err(|_| DlError::Net("服务器长时间没有响应".into()))?
                .map_err(net)?;
            if n == 0 {
                return Err(DlError::Net("服务器关闭了连接".into()));
            }
            let l = line.trim_end_matches(['\r', '\n']);
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(l);
            let b = l.as_bytes();
            if b.len() >= 3 && b[..3].iter().all(u8::is_ascii_digit) {
                let code: u16 = l[..3].parse().unwrap_or(0);
                match first {
                    None => {
                        let multi = b.get(3) == Some(&b'-');
                        first = Some((code, multi));
                        if !multi {
                            return Ok(Reply { code, text });
                        }
                    }
                    Some((c, true)) if c == code && b.get(3) == Some(&b' ') => return Ok(Reply { code, text }),
                    _ => {}
                }
            }
        }
    }

    async fn cmd(&mut self, line: &str) -> Result<Reply, DlError> {
        self.send(line).await?;
        self.read_reply().await
    }

    async fn send(&mut self, line: &str) -> Result<(), DlError> {
        if line.contains(['\r', '\n']) {
            return Err(DlError::Other("路径里含有换行符".into()));
        }
        self.wr.write_all(format!("{line}\r\n").as_bytes()).await.map_err(net)
    }

    /// 打开被动模式的数据连接：先试 EPSV，不行再用 PASV。
    async fn open_data(&mut self) -> Result<TcpStream, DlError> {
        let r = self.cmd("EPSV").await?;
        let addr = if r.code == 229 { parse_epsv(&r.text).map(|p| SocketAddr::new(self.peer, p)) } else { None };
        let addr = match addr {
            Some(a) => a,
            None => {
                let r = self.cmd("PASV").await?;
                if r.code != 227 {
                    return Err(DlError::Other(format!("服务器不支持被动模式：{}", r.text)));
                }
                let (ip, port) = parse_pasv(&r.text).ok_or_else(|| DlError::Other(format!("无法理解服务器的 PASV 回复：{}", r.text)))?;
                // 内网服务器常在 PASV 里报告自己的内网地址：以实际连接的地址为准
                let ip = if ip.is_private() || ip.is_loopback() || ip.is_unspecified() { self.peer } else { IpAddr::V4(ip) };
                SocketAddr::new(ip, port)
            }
        };
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| DlError::Net("数据连接超时".into()))?
            .map_err(|e| DlError::Net(format!("数据连接失败：{e}")))
    }
}

/// `229 Entering Extended Passive Mode (|||6446|)`
fn parse_epsv(text: &str) -> Option<u16> {
    let inner = &text[text.find('(')? + 1..text.rfind(')')?];
    let d = inner.chars().next()?;
    inner.split(d).nth(3)?.parse().ok()
}

/// `227 Entering Passive Mode (192,168,0,1,19,137)`
fn parse_pasv(text: &str) -> Option<(std::net::Ipv4Addr, u16)> {
    // 没有括号时跳过开头的回复码
    let inner = &text[text.find('(').map(|i| i + 1).unwrap_or_else(|| text.find(' ').map(|i| i + 1).unwrap_or(0))..];
    let nums: Vec<u16> = inner.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse().ok()).collect::<Option<_>>()?;
    if nums.len() < 6 || nums[..6].iter().any(|n| *n > 255) {
        return None;
    }
    Some((std::net::Ipv4Addr::new(nums[0] as u8, nums[1] as u8, nums[2] as u8, nums[3] as u8), nums[4] * 256 + nums[5]))
}

/// 地址里的路径还原成服务器上的路径（百分号解码）。
fn remote_path(url: &Url) -> String {
    crate::grab::percent_decode(url.path())
}

fn fail(what: &str, r: &Reply) -> DlError {
    DlError::Other(format!("{what}：{} {}", r.code, r.text.lines().last().unwrap_or("").get(4..).unwrap_or("")))
}

pub async fn download(
    url: &Url,
    credentials: Option<&str>,
    part: &Path,
    meta: &mut ResumeMeta,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    mut on_progress: impl FnMut(u64, Option<u64>, &ResumeMeta) + Send,
) -> Result<u64, DlError> {
    if url.scheme() != "ftp" {
        return Err(DlError::Other("暂不支持加密的 FTPS，请使用 ftp:// 或 SFTP。".into()));
    }
    let host = url.host_str().ok_or_else(|| DlError::Other("链接里没有服务器地址".into()))?.to_string();
    let port = url.port().unwrap_or(21);
    let path = remote_path(url);
    meta.segments.clear();

    let mut outer = ctrl.clone();
    let work = async {
        let mut c = Control::connect(&host, port).await?;
        let hello = c.read_reply().await?;
        if hello.code != 220 {
            return Err(fail("服务器拒绝连接", &hello));
        }
        let (user, pass) = match credentials {
            Some(c) => c.split_once(':').map(|(u, p)| (u.to_string(), p.to_string())).unwrap_or((c.to_string(), String::new())),
            None => ("anonymous".into(), "clearclip@example.com".into()),
        };
        let mut r = c.cmd(&format!("USER {user}")).await?;
        if r.code == 331 || r.code == 332 {
            r = c.cmd(&format!("PASS {pass}")).await?;
        }
        if r.code != 230 {
            return Err(DlError::Other(format!("登录失败（{} {}）。请检查用户名和密码。", r.code, r.text.lines().last().unwrap_or(""))));
        }
        let r = c.cmd("TYPE I").await?;
        if r.code != 200 {
            return Err(fail("无法切换到二进制模式", &r));
        }
        let size = {
            let r = c.cmd(&format!("SIZE {path}")).await?;
            if r.code == 213 {
                r.text.rsplit(' ').next().and_then(|s| s.trim().parse::<u64>().ok())
            } else if r.code == 550 {
                return Err(DlError::Other(format!("文件不存在或没有读取权限：{path}")));
            } else {
                None
            }
        };
        meta.total = size;
        // 续传：已有的内容必须比文件小
        let mut existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if let Some(t) = size {
            if existing == t && t > 0 {
                on_progress(t, Some(t), meta);
                let _ = c.cmd("QUIT").await;
                return Ok(t);
            }
            if existing > t {
                existing = 0;
            }
            if let Some(dir) = part.parent() {
                ensure_space(dir, t - existing, opts.disk_reserve)?;
            }
        }
        let mut data = c.open_data().await?;
        let mut start = 0;
        if existing > 0 {
            let r = c.cmd(&format!("REST {existing}")).await?;
            if r.code == 350 {
                start = existing;
                meta.resumable = Some(true);
            } else {
                meta.resumable = Some(false);
            }
        }
        let r = c.cmd(&format!("RETR {path}")).await?;
        if !(r.code == 125 || r.code == 150) {
            return if r.code == 550 { Err(DlError::Other(format!("文件不存在或没有读取权限：{path}"))) } else { Err(fail("无法下载", &r)) };
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(start > 0)
            .truncate(start == 0)
            .open(part)
            .await
            .map_err(|e| DlError::Io(format!("无法写入文件 {}：{e}", part.display())))?;
        let mut received = start;
        on_progress(received, size, meta);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = tokio::select! {
                n = tokio::time::timeout(IDLE_TIMEOUT, data.read(&mut buf)) => {
                    n.map_err(|_| DlError::Net("数据连接长时间没有数据".into()))?.map_err(net)?
                }
                cc = wait_ctrl(ctrl) => {
                    let _ = file.flush().await;
                    return Err(DlError::from_ctrl(cc));
                }
            };
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
            received += n as u64;
            on_progress(received, size, meta);
            opts.throttle(n).await;
        }
        file.flush().await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
        drop(data);
        // 传输结束的 226 回复；没读到也不算失败，以收到的字节数为准
        let _ = tokio::time::timeout(Duration::from_secs(10), c.read_reply()).await;
        let _ = c.send("QUIT").await;
        if let Some(t) = size {
            if received < t {
                return Err(DlError::Net(format!("连接提前结束（{received}/{t} 字节）")));
            }
        }
        meta.total = Some(received);
        Ok(received)
    };
    // 连接、登录阶段也要能暂停 / 取消：外层用一份独立的接收端监听
    tokio::select! {
        r = work => r,
        c = wait_ctrl(&mut outer) => Err(DlError::from_ctrl(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[test]
    fn parses_passive_replies() {
        assert_eq!(parse_epsv("229 Entering Extended Passive Mode (|||6446|)"), Some(6446));
        assert_eq!(parse_epsv("229 nope"), None);
        let (ip, port) = parse_pasv("227 Entering Passive Mode (192,168,0,1,19,137).").unwrap();
        assert_eq!((ip.to_string(), port), ("192.168.0.1".to_string(), 19 * 256 + 137));
        assert_eq!(parse_pasv("227 Entering Passive Mode 10,0,0,5,4,1").unwrap().1, 1025);
        assert!(parse_pasv("227 (1,2,3)").is_none());
        assert!(parse_pasv("227 (1,2,3,4,5,300)").is_none());
    }

    /// 本地模拟的 FTP 服务器：支持 USER/PASS、SIZE、REST、PASV（可关闭 EPSV 或 REST）、RETR。
    struct Mock {
        port: u16,
        _stop: Arc<AtomicBool>,
    }

    struct MockOpts {
        user: Option<(&'static str, &'static str)>,
        content: Vec<u8>,
        rest_ok: bool,
        /// 只发送这么多字节就断开（模拟中途断线）
        cut_at: Option<usize>,
        epsv: bool,
    }

    async fn reply(w: &mut tokio::net::tcp::OwnedWriteHalf, s: &str) {
        w.write_all(format!("{s}\r\n").as_bytes()).await.unwrap();
    }

    async fn mock(o: MockOpts) -> Mock {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let o = Arc::new(o);
        tokio::spawn(async move {
            loop {
                let Ok((s, _)) = l.accept().await else { return };
                let o = o.clone();
                tokio::spawn(async move {
                    let (r, mut w) = s.into_split();
                    let mut r = BufReader::new(r);
                    reply(&mut w, "220-欢迎\r\n220 ready").await;
                    let (mut user_ok, mut rest, mut pasv): (bool, u64, Option<TcpListener>) = (o.user.is_none(), 0, None);
                    let mut given_user = String::new();
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        let line = line.trim().to_string();
                        let (cmd, arg) = line.split_once(' ').unwrap_or((line.as_str(), ""));
                        match cmd {
                            "USER" => {
                                given_user = arg.to_string();
                                reply(&mut w, if o.user.is_some() { "331 password please" } else { "230 anonymous ok" }).await
                            }
                            "PASS" => match o.user {
                                Some((u, p)) if given_user == u && arg == p => {
                                    user_ok = true;
                                    reply(&mut w, "230 ok").await
                                }
                                _ => reply(&mut w, "530 Login incorrect").await,
                            },
                            "TYPE" => reply(&mut w, "200 binary").await,
                            "SIZE" if !user_ok => reply(&mut w, "530 login first").await,
                            "SIZE" if arg == "/pub/missing.bin" => reply(&mut w, "550 no such file").await,
                            "SIZE" => reply(&mut w, &format!("213 {}", o.content.len())).await,
                            "EPSV" if !o.epsv => reply(&mut w, "500 unknown command").await,
                            "EPSV" | "PASV" => {
                                let dl = TcpListener::bind("127.0.0.1:0").await.unwrap();
                                let p = dl.local_addr().unwrap().port();
                                pasv = Some(dl);
                                if cmd == "EPSV" {
                                    reply(&mut w, &format!("229 Entering Extended Passive Mode (|||{p}|)")).await
                                } else {
                                    reply(&mut w, &format!("227 Entering Passive Mode (127,0,0,1,{},{})", p / 256, p % 256)).await
                                }
                            }
                            "REST" if o.rest_ok => {
                                rest = arg.parse().unwrap();
                                reply(&mut w, "350 restarting").await
                            }
                            "REST" => reply(&mut w, "502 not implemented").await,
                            "RETR" => {
                                let dl = pasv.take().unwrap();
                                reply(&mut w, "150 opening").await;
                                let (mut d, _) = dl.accept().await.unwrap();
                                let from = if o.rest_ok { rest as usize } else { 0 };
                                let end = o.cut_at.unwrap_or(o.content.len());
                                let _ = d.write_all(&o.content[from.min(end)..end]).await;
                                drop(d);
                                if o.cut_at.is_some() {
                                    return;
                                }
                                reply(&mut w, "226 done").await;
                            }
                            "QUIT" => {
                                reply(&mut w, "221 bye").await;
                                return;
                            }
                            _ => reply(&mut w, "502 no").await,
                        }
                    }
                });
            }
        });
        Mock { port, _stop: stop }
    }

    fn content(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8).collect()
    }

    async fn run(url: &str, cred: Option<&str>, part: &Path, meta: &mut ResumeMeta) -> Result<u64, DlError> {
        let (_tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        download(&Url::parse(url).unwrap(), cred, part, meta, &mut rx, &HttpOptions::default(), |_, _, _| {}).await
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cc-ftp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d
    }

    #[tokio::test]
    async fn downloads_anonymously_via_epsv() {
        let data = content(300_000);
        let m = mock(MockOpts { user: None, content: data.clone(), rest_ok: true, cut_at: None, epsv: true }).await;
        let part = tmp("full");
        let mut meta = ResumeMeta::default();
        let n = run(&format!("ftp://127.0.0.1:{}/pub/a.bin", m.port), None, &part, &mut meta).await.unwrap();
        assert_eq!(n, 300_000);
        assert_eq!(std::fs::read(&part).unwrap(), data);
        assert_eq!(meta.total, Some(300_000));
    }

    #[tokio::test]
    async fn falls_back_to_pasv_and_logs_in() {
        let data = content(5000);
        let m = mock(MockOpts { user: Some(("bob", "p@ss")), content: data.clone(), rest_ok: true, cut_at: None, epsv: false }).await;
        let part = tmp("login");
        let url = format!("ftp://127.0.0.1:{}/pub/a.bin", m.port);
        let err = run(&url, Some("bob:wrong"), &part, &mut ResumeMeta::default()).await.unwrap_err();
        assert!(err.to_string().contains("登录失败"), "{err}");
        run(&url, Some("bob:p@ss"), &part, &mut ResumeMeta::default()).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), data);
    }

    #[tokio::test]
    async fn resumes_after_a_cut_connection() {
        let data = content(200_000);
        let cut = mock(MockOpts { user: None, content: data.clone(), rest_ok: true, cut_at: Some(80_000), epsv: true }).await;
        let part = tmp("resume");
        let mut meta = ResumeMeta::default();
        let err = run(&format!("ftp://127.0.0.1:{}/f.bin", cut.port), None, &part, &mut meta).await.unwrap_err();
        assert!(matches!(err, DlError::Net(_)), "{err}");
        assert_eq!(std::fs::metadata(&part).unwrap().len(), 80_000);
        // 服务器恢复：从断点继续
        let ok = mock(MockOpts { user: None, content: data.clone(), rest_ok: true, cut_at: None, epsv: true }).await;
        let n = run(&format!("ftp://127.0.0.1:{}/f.bin", ok.port), None, &part, &mut meta).await.unwrap();
        assert_eq!(n, 200_000);
        assert_eq!(std::fs::read(&part).unwrap(), data);
        assert_eq!(meta.resumable, Some(true));
    }

    #[tokio::test]
    async fn restarts_when_server_cannot_resume() {
        let data = content(50_000);
        let m = mock(MockOpts { user: None, content: data.clone(), rest_ok: false, cut_at: None, epsv: true }).await;
        let part = tmp("norest");
        std::fs::write(&part, &data[..10_000]).unwrap();
        let mut meta = ResumeMeta::default();
        run(&format!("ftp://127.0.0.1:{}/f.bin", m.port), None, &part, &mut meta).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), data);
        assert_eq!(meta.resumable, Some(false));
    }

    #[tokio::test]
    async fn complete_part_needs_no_transfer_and_missing_file_is_reported() {
        let data = content(1000);
        let m = mock(MockOpts { user: None, content: data.clone(), rest_ok: true, cut_at: None, epsv: true }).await;
        let part = tmp("done");
        std::fs::write(&part, &data).unwrap();
        assert_eq!(run(&format!("ftp://127.0.0.1:{}/f.bin", m.port), None, &part, &mut ResumeMeta::default()).await.unwrap(), 1000);
        let err = run(&format!("ftp://127.0.0.1:{}/pub/missing.bin", m.port), None, &tmp("missing"), &mut ResumeMeta::default()).await.unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
        let err = run("ftps://127.0.0.1/x", None, &tmp("ftps"), &mut ResumeMeta::default()).await.unwrap_err();
        assert!(err.to_string().contains("FTPS"));
    }

    #[tokio::test]
    async fn pause_stops_the_transfer() {
        let m = mock(MockOpts { user: None, content: content(10), rest_ok: true, cut_at: None, epsv: true }).await;
        let (tx, mut rx) = watch::channel(super::super::CTRL_PAUSE);
        let _keep = tx;
        let r = download(
            &Url::parse(&format!("ftp://127.0.0.1:{}/f", m.port)).unwrap(),
            None,
            &tmp("pause"),
            &mut ResumeMeta::default(),
            &mut rx,
            &HttpOptions::default(),
            |_, _, _| {},
        )
        .await;
        assert!(matches!(r, Err(DlError::Paused)));
    }
}
