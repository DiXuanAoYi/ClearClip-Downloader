//! 流媒体抓取（RTMP、RTSP、MMS 等）：交给 ffmpeg 原样复制到 mkv，不重新编码。
//! 流结束（点播播完或直播主播下播）时任务完成；暂停 / 取消会让 ffmpeg 正常收尾，已抓到的内容可以播放，但再次开始时会重新抓取。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

use super::proxy_bridge::{Bridge, Upstream};
use super::{ensure_space, wait_ctrl, DlError};

/// 带上登录信息的地址（只用于传给 ffmpeg，不写进日志）。
fn url_with_credentials(url: &str, credentials: Option<&str>) -> String {
    let Some(c) = credentials else { return url.to_string() };
    let Ok(mut u) = url::Url::parse(url) else { return url.to_string() };
    let (user, pass) = c.split_once(':').unwrap_or((c, ""));
    let _ = u.set_username(user);
    let _ = u.set_password((!pass.is_empty()).then_some(pass));
    u.to_string()
}

/// 这个流的地址按分流规则该怎么出去。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamRoute {
    /// 系统代理：ffmpeg 自己只认环境变量里的 http_proxy 等
    System,
    /// 直连：不让 ffmpeg 读环境变量里的代理
    Direct,
    /// 自定义代理（设置里的代理地址）
    Proxy(String),
}

/// 按出口准备 ffmpeg 的启动方式：改写后的地址、输入前的参数、是否清掉环境变量里的代理，以及要保持存活的代理桥。
pub(crate) struct Prepared {
    pub(crate) url: String,
    pub(crate) pre_input: Vec<String>,
    pub(crate) clear_proxy_env: bool,
    _bridge: Option<Bridge>,
}

pub(crate) async fn prepare(url: &str, route: &StreamRoute) -> Result<Prepared, DlError> {
    let plain = |clear| Prepared { url: url.to_string(), pre_input: vec![], clear_proxy_env: clear, _bridge: None };
    let proxy = match route {
        StreamRoute::System => return Ok(plain(false)),
        StreamRoute::Direct => return Ok(plain(true)),
        StreamRoute::Proxy(p) => p,
    };
    let up =
        Upstream::parse(proxy).ok_or_else(|| DlError::Other("代理地址无法识别：支持 http://、socks5://、socks5h://（https:// 的代理暂不支持）。".into()))?;
    let u = url::Url::parse(url).map_err(|e| DlError::Other(format!("流地址无效：{e}")))?;
    let scheme = u.scheme().to_ascii_lowercase();
    let bridge_err = |e: std::io::Error| DlError::Other(format!("无法启动代理转发：{e}"));
    match scheme.as_str() {
        "http" | "https" => {
            let bridge = Bridge::http_proxy(up).await.map_err(bridge_err)?;
            let pre_input = vec!["-http_proxy".to_string(), format!("http://127.0.0.1:{}", bridge.addr.port())];
            Ok(Prepared { url: url.to_string(), pre_input, clear_proxy_env: true, _bridge: Some(bridge) })
        }
        "rtmp" | "rtsp" => {
            let host = u.host_str().ok_or_else(|| DlError::Other("流地址缺少主机".into()))?.trim_matches(['[', ']']).to_string();
            let port = u.port().unwrap_or(if scheme == "rtmp" { 1935 } else { 554 });
            let bridge = Bridge::forward(up, &host, port).await.map_err(bridge_err)?;
            let mut pre_input = vec![];
            if scheme == "rtmp" {
                // 地址里的主机换成了本机，服务器校验的 tcUrl 仍然要用原来的
                let app = u.path_segments().and_then(|mut s| s.next()).unwrap_or("");
                let authority = if u.port().is_some() { format!("{}:{}", u.host_str().unwrap_or(""), port) } else { u.host_str().unwrap_or("").to_string() };
                pre_input.extend(["-rtmp_tcurl".to_string(), format!("rtmp://{authority}/{app}")]);
            }
            let mut local = u.clone();
            let _ = local.set_host(Some("127.0.0.1"));
            let _ = local.set_port(Some(bridge.addr.port()));
            Ok(Prepared { url: local.to_string(), pre_input, clear_proxy_env: true, _bridge: Some(bridge) })
        }
        other => Err(DlError::Other(format!("“{other}://” 地址无法经代理抓取，请在“设置 → 网络”里把这个网站设为直连或系统代理。"))),
    }
}

/// ffmpeg 读取的环境变量里的代理。
pub(crate) const PROXY_ENV: [&str; 8] = ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY", "no_proxy", "NO_PROXY"];

/// ffmpeg 的命令行参数。`pre_input` 是放在 `-i` 前面的输入选项。
pub fn build_args(url: &str, out: &Path, pre_input: &[String]) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-y"].iter().map(|s| s.to_string()).collect();
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    if scheme.starts_with("rtsp") {
        // UDP 容易丢包或被防火墙拦截：用 TCP 传输
        a.extend(["-rtsp_transport", "tcp"].map(String::from));
    }
    if scheme != "file" && !url.starts_with('/') {
        // 15 秒收不到数据就算断线（微秒）
        a.extend(["-rw_timeout", "15000000"].map(String::from));
    }
    a.extend(pre_input.iter().cloned());
    a.extend(["-i".to_string(), url.to_string(), "-map".into(), "0".into(), "-c".into(), "copy".into(), "-f".into(), "matroska".into()]);
    a.push(out.to_string_lossy().into_owned());
    a
}

/// 从 `-progress` 的输出行里取已写入的字节数。
pub fn parse_progress(line: &str) -> Option<u64> {
    line.strip_prefix("total_size=").and_then(|v| v.trim().parse().ok())
}

#[allow(clippy::too_many_arguments)]
pub async fn download(
    ffmpeg: &Path,
    url: &str,
    credentials: Option<&str>,
    route: &StreamRoute,
    part: &Path,
    ctrl: &mut watch::Receiver<u8>,
    disk_reserve: u64,
    mut on_progress: impl FnMut(u64) + Send,
) -> Result<u64, DlError> {
    if let Some(dir) = part.parent() {
        ensure_space(dir, 0, disk_reserve)?;
    }
    let _ = tokio::fs::remove_file(part).await;
    let prepared = prepare(&url_with_credentials(url, credentials), route).await?;
    let args = build_args(&prepared.url, part, &prepared.pre_input);
    let mut cmd = tokio::process::Command::new(ffmpeg);
    if prepared.clear_proxy_env {
        for k in PROXY_ENV {
            cmd.env_remove(k);
        }
    }
    cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| DlError::Other(format!("无法运行 ffmpeg：{e}")))?;
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or_else(|| DlError::Other("无法读取 ffmpeg 输出".into()))?;
    let stderr = child.stderr.take();
    // 错误信息只留最后几行
    let err_task = tokio::spawn(async move {
        let mut tail: Vec<String> = vec![];
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                tail.push(l);
                if tail.len() > 6 {
                    tail.remove(0);
                }
            }
        }
        tail
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut stop: Option<u8> = None;
    loop {
        tokio::select! {
            l = lines.next_line() => match l {
                Ok(Some(l)) => {
                    if let Some(n) = parse_progress(&l) {
                        on_progress(n);
                    }
                }
                _ => break,
            },
            c = wait_ctrl(ctrl), if stop.is_none() => {
                stop = Some(c);
                // 让 ffmpeg 写完文件尾再退出
                if let Some(mut s) = stdin.take() {
                    let _ = s.write_all(b"q").await;
                    let _ = s.flush().await;
                }
                break;
            }
        }
    }
    let status = if stop.is_some() {
        match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            Ok(r) => r.ok(),
            Err(_) => {
                let _ = child.kill().await;
                None
            }
        }
    } else {
        child.wait().await.ok()
    };
    let tail = err_task.await.unwrap_or_default();
    let size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if let Some(c) = stop {
        return Err(DlError::from_ctrl(c));
    }
    match status {
        Some(s) if s.success() && size > 0 => {
            on_progress(size);
            Ok(size)
        }
        Some(s) if s.success() => Err(DlError::Other("流里没有数据".into())),
        _ => {
            let msg = tail.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
            // 已经抓到一部分时，中途断线按“流结束”处理，保留内容
            if size > 0 && !msg.to_ascii_lowercase().contains("connection refused") && !msg.contains("401") && !msg.contains("403") {
                on_progress(size);
                return Ok(size);
            }
            Err(DlError::Net(format!("ffmpeg 无法抓取这个流：{}", if msg.is_empty() { "未知错误" } else { msg.as_str() })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_bin() -> Option<std::path::PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| "ffmpeg".into())
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cc-stream-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d
    }

    #[test]
    fn builds_command_lines() {
        let a = build_args("rtsp://10.0.0.2/cam", Path::new("/tmp/o.part"), &[]);
        assert!(a.windows(2).any(|w| w == ["-rtsp_transport", "tcp"]));
        assert!(a.windows(2).any(|w| w[0] == "-rw_timeout"));
        assert!(a.windows(2).any(|w| w == ["-c", "copy"]));
        assert_eq!(a.last().map(String::as_str), Some("/tmp/o.part"));
        let r = build_args("rtmp://h/app/s", Path::new("o"), &[]);
        assert!(!r.iter().any(|x| x == "-rtsp_transport"));
        let f = build_args("/tmp/x.mp4", Path::new("o"), &[]);
        assert!(!f.iter().any(|x| x == "-rw_timeout"));
    }

    #[test]
    fn credentials_are_added_for_ffmpeg_only() {
        assert_eq!(url_with_credentials("rtsp://10.0.0.2/cam", Some("admin:p@ss")), "rtsp://admin:p%40ss@10.0.0.2/cam");
        assert_eq!(url_with_credentials("rtsp://10.0.0.2/cam", None), "rtsp://10.0.0.2/cam");
        assert_eq!(url_with_credentials("rtmp://h/a", Some("u:")), "rtmp://u@h/a");
    }

    #[test]
    fn parses_progress_lines() {
        assert_eq!(parse_progress("total_size=12345"), Some(12345));
        assert_eq!(parse_progress("total_size=N/A"), None);
        assert_eq!(parse_progress("out_time_us=1"), None);
    }

    #[tokio::test]
    async fn captures_a_source_to_mkv_and_reports_failures() {
        let Some(ff) = ffmpeg_bin() else { return };
        let src = tmp("src.mp4");
        let ok = std::process::Command::new(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=10:duration=2", "-pix_fmt", "yuv420p"])
            .arg(&src)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let (_tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let out = tmp("out.mkv.part");
        let mut last = 0;
        let n = download(&ff, src.to_str().unwrap(), None, &StreamRoute::System, &out, &mut rx, 0, |b| last = b).await.unwrap();
        assert!(n > 1000 && last == n);
        // 抓到的文件可以被 ffmpeg 读取
        let probe = std::process::Command::new(&ff).args(["-v", "error", "-i"]).arg(&out).args(["-f", "null", "-"]).status().unwrap();
        assert!(probe.success());
        // 连不上的流报错
        let err = download(&ff, "rtmp://127.0.0.1:9/live/none", None, &StreamRoute::System, &tmp("bad.part"), &mut rx, 0, |_| {}).await.unwrap_err();
        assert!(matches!(err, DlError::Net(_)), "{err}");
    }

    /// 用本地 TCP 服务器慢慢推送一段 MPEG-TS，模拟直播流。
    async fn live_source(ff: &Path) -> u16 {
        let ts = tmp("live.ts");
        let ok = std::process::Command::new(ff)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=10:duration=30",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "mpegts",
            ])
            .arg(&ts)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let data = std::fs::read(&ts).unwrap();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut s, _)) = l.accept().await else { return };
            for chunk in data.chunks(1880) {
                if s.write_all(chunk).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        });
        port
    }

    /// 最简的无认证 SOCKS5 代理：只转发，记录连接次数。
    async fn counting_socks5() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut c, _)) = l.accept().await else { return };
                let h = h2.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut g = [0u8; 2];
                    c.read_exact(&mut g).await.unwrap();
                    let mut m = vec![0u8; g[1] as usize];
                    c.read_exact(&mut m).await.unwrap();
                    c.write_all(&[5, 0]).await.unwrap();
                    let mut r = [0u8; 4];
                    c.read_exact(&mut r).await.unwrap();
                    let host = match r[3] {
                        1 => {
                            let mut a = [0u8; 4];
                            c.read_exact(&mut a).await.unwrap();
                            std::net::Ipv4Addr::from(a).to_string()
                        }
                        3 => {
                            let mut n = [0u8; 1];
                            c.read_exact(&mut n).await.unwrap();
                            let mut v = vec![0u8; n[0] as usize];
                            c.read_exact(&mut v).await.unwrap();
                            String::from_utf8(v).unwrap()
                        }
                        _ => return,
                    };
                    let mut p = [0u8; 2];
                    c.read_exact(&mut p).await.unwrap();
                    h.fetch_add(1, Ordering::SeqCst);
                    let host = if host == "localhost" { "127.0.0.1".into() } else { host };
                    let Ok(mut s) = tokio::net::TcpStream::connect((host.as_str(), u16::from_be_bytes(p))).await else { return };
                    c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut c, &mut s).await;
                });
            }
        });
        (port, hits)
    }

    /// 本地 HTTP 服务器，原样提供一个文件。
    async fn http_file(data: Vec<u8>) -> u16 {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let data = data.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let mut b = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut b).await {
                            Ok(n) if n > 0 => buf.extend_from_slice(&b[..n]),
                            _ => return,
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).into_owned();
                    let range_from = head.lines().find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("range: bytes=")
                            .map(|r| r.trim_end_matches('-').split('-').next().unwrap_or("0").parse::<usize>().unwrap_or(0))
                    });
                    let (status, body, extra) = match range_from {
                        Some(f) if f < data.len() => {
                            ("206 Partial Content", &data[f..], format!("Content-Range: bytes {}-{}/{}\r\n", f, data.len() - 1, data.len()))
                        }
                        _ => ("200 OK", &data[..], String::new()),
                    };
                    let h = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: video/mp2t\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(h.as_bytes()).await;
                    let _ = s.write_all(body).await;
                });
            }
        });
        port
    }

    #[tokio::test]
    async fn an_http_stream_goes_through_the_configured_socks5_proxy() {
        let Some(ff) = ffmpeg_bin() else { return };
        let ts = tmp("proxied.ts");
        let ok = std::process::Command::new(&ff)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=10:duration=2",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "mpegts",
            ])
            .arg(&ts)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let origin = http_file(std::fs::read(&ts).unwrap()).await;
        let (proxy, hits) = counting_socks5().await;
        let (_tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let url = format!("http://localhost:{origin}/live.ts");
        // 直连：代理没被用到
        let out = tmp("direct.part");
        let n = download(&ff, &url, None, &StreamRoute::Direct, &out, &mut rx, 0, |_| {}).await.unwrap();
        assert!(n > 1000);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        // 走代理：经过 SOCKS5，抓到的文件一样能播放
        let out = tmp("proxied.part");
        let route = StreamRoute::Proxy(format!("socks5h://127.0.0.1:{proxy}"));
        let n = download(&ff, &url, None, &route, &out, &mut rx, 0, |_| {}).await.unwrap();
        assert!(n > 1000);
        assert!(hits.load(std::sync::atomic::Ordering::SeqCst) >= 1, "ffmpeg 没有走代理");
        let probe = std::process::Command::new(&ff).args(["-v", "error", "-i"]).arg(&out).args(["-f", "null", "-"]).status().unwrap();
        assert!(probe.success());
    }

    #[tokio::test]
    async fn rtmp_is_forwarded_through_the_proxy_and_unsupported_schemes_are_refused() {
        let (proxy, hits) = counting_socks5().await;
        let route = StreamRoute::Proxy(format!("socks5://127.0.0.1:{proxy}"));
        let p = prepare("rtmp://127.0.0.1:19350/live/room1?k=v", &route).await.unwrap();
        // 地址的主机和端口换成了桥，服务器看到的 tcUrl 保持原样
        assert!(p.url.starts_with("rtmp://127.0.0.1:") && !p.url.contains(":19350/") && p.url.ends_with("/live/room1?k=v"), "{}", p.url);
        assert_eq!(p.pre_input, vec!["-rtmp_tcurl".to_string(), "rtmp://127.0.0.1:19350/live".to_string()]);
        assert!(p.clear_proxy_env);
        // 连接桥会触发代理去连原来的目标（这里没有服务器，连不上也没关系，只看代理被用到）
        let port: u16 = p.url.split("127.0.0.1:").nth(1).unwrap().split('/').next().unwrap().parse().unwrap();
        let _ = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        for _ in 0..50 {
            if hits.load(std::sync::atomic::Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        let err = |r: Result<Prepared, DlError>| match r {
            Err(DlError::Other(m)) => m,
            Err(e) => panic!("{e}"),
            Ok(_) => panic!("应该被拒绝"),
        };
        assert!(err(prepare("rtmps://h/app/s", &route).await).contains("rtmps"));
        assert!(err(prepare("http://h/x", &StreamRoute::Proxy("https://p:1".into())).await).contains("代理地址"));
        // 直连清掉环境变量里的代理，系统代理保持 ffmpeg 自己的行为
        assert!(prepare("http://h/x", &StreamRoute::Direct).await.unwrap().clear_proxy_env);
        let sys = prepare("http://h/x", &StreamRoute::System).await.unwrap();
        assert!(!sys.clear_proxy_env && sys.pre_input.is_empty());
    }

    #[tokio::test]
    async fn pause_finishes_the_file_and_returns_paused() {
        let Some(ff) = ffmpeg_bin() else { return };
        let port = live_source(&ff).await;
        let (tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let out = tmp("paused.part");
        let t = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            let _ = tx.send(super::super::CTRL_PAUSE);
            tx
        });
        let r = download(&ff, &format!("tcp://127.0.0.1:{port}"), None, &StreamRoute::System, &out, &mut rx, 0, |_| {}).await;
        let _tx = t.await.unwrap();
        assert!(matches!(r, Err(DlError::Paused)), "{r:?}");
        // 已抓到的部分是完整可播放的 mkv
        assert!(std::fs::metadata(&out).unwrap().len() > 1000);
        let probe = std::process::Command::new(&ff).args(["-v", "error", "-i"]).arg(&out).args(["-f", "null", "-"]).status().unwrap();
        assert!(probe.success());
    }
}
