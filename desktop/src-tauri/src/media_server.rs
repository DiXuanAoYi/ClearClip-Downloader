//! 内置播放器用的本机媒体服务：只监听 127.0.0.1，按“令牌 → 文件”提供视频和音频，支持 Range（拖动进度条）。
//!
//! 为什么不用 Tauri 自带的 asset 协议：Linux 上的 WebKitGTK 不能通过自定义协议流式播放视频，
//! 用本机 HTTP 地址在 Windows、macOS、Linux 上的表现一致。
//!
//! 安全：只绑定回环地址；每个文件有一次性随机令牌（128 位），没登记的路径取不到；
//! 校验 Host 头，防止别的网页通过 DNS 重绑定访问；锁定应用时清空全部登记。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::safebox::random_bytes;

/// 还在下载的文件：按声明的总大小回应，读到还没下载好的位置就等一会儿。
#[derive(Clone)]
pub struct Growing {
    /// 正在写入的文件（.part）；下载完成改名后用 `final_path`
    pub path: PathBuf,
    pub final_path: PathBuf,
    pub total: u64,
    /// 从文件开头起已经连续下载好的字节数
    pub available: Arc<dyn Fn() -> u64 + Send + Sync>,
}

/// 还没下载的远程文件（解析界面的预览）：本机服务代为请求，带上站点要求的 Referer / Cookie，
/// 并把播放器的 Range 请求原样转给源站，所以防盗链的地址和拖动进度条都能用。
#[derive(Clone)]
pub struct Remote {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub client: reqwest::Client,
    /// 源站没有给出可用的 Content-Type 时用这个（按资源扩展名猜的）
    pub mime: &'static str,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    growing: Option<Growing>,
    remote: Option<Remote>,
}

#[derive(Clone)]
pub struct MediaServer {
    port: u16,
    files: Arc<Mutex<HashMap<String, Entry>>>,
}

/// 等待还没下载到的数据，超过这个时间没有进展就放弃（播放器会提示“读取失败”）
const GROWING_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

impl MediaServer {
    /// 绑定一个随机端口（同步，便于在启动流程里使用）；返回服务和待运行的监听器。
    pub fn bind() -> std::io::Result<(MediaServer, std::net::TcpListener)> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        Ok((MediaServer { port, files: Arc::new(Mutex::new(HashMap::new())) }, listener))
    }

    /// 没有真正监听的占位实例（测试用）。
    pub fn unbound() -> MediaServer {
        MediaServer { port: 0, files: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// 登记一个文件，返回可以直接给 `<video>` 用的地址。
    pub fn register(&self, path: &Path) -> String {
        let mut g = self.files.lock().unwrap_or_else(|e| e.into_inner());
        // 同一个文件重复登记时沿用原来的令牌
        if let Some((tok, _)) = g.iter().find(|(_, e)| e.growing.is_none() && e.remote.is_none() && e.path.as_path() == path) {
            return format!("http://127.0.0.1:{}/m/{tok}", self.port);
        }
        let tok = hex::encode(random_bytes::<16>());
        g.insert(tok.clone(), Entry { path: path.to_path_buf(), growing: None, remote: None });
        format!("http://127.0.0.1:{}/m/{tok}", self.port)
    }

    /// 登记一个正在下载的文件（边下边播）。每次登记都换新令牌，旧的播放地址不受影响。
    pub fn register_growing(&self, g: Growing) -> String {
        let tok = hex::encode(random_bytes::<16>());
        self.files.lock().unwrap_or_else(|e| e.into_inner()).insert(tok.clone(), Entry { path: g.final_path.clone(), growing: Some(g), remote: None });
        format!("http://127.0.0.1:{}/m/{tok}", self.port)
    }

    /// 登记一个远程文件，返回可以直接给 `<video>` 用的本机地址。
    pub fn register_remote(&self, r: Remote) -> String {
        let tok = hex::encode(random_bytes::<16>());
        self.files.lock().unwrap_or_else(|e| e.into_inner()).insert(tok.clone(), Entry { path: PathBuf::new(), growing: None, remote: Some(r) });
        format!("http://127.0.0.1:{}/m/{tok}", self.port)
    }

    pub fn clear(&self) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn lookup(&self, token: &str) -> Option<Entry> {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).get(token).cloned()
    }

    /// 在异步运行时里开始接受连接。
    pub async fn serve(self, listener: std::net::TcpListener) {
        let Ok(listener) = TcpListener::from_std(listener) else { return };
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let me = self.clone();
            tokio::spawn(async move {
                let _ = me.handle(stream).await;
            });
        }
    }

    async fn handle(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let mut buf = Vec::with_capacity(1024);
        let mut chunk = [0u8; 1024];
        let head_end = loop {
            let n = tokio::time::timeout(std::time::Duration::from_secs(10), stream.read(&mut chunk)).await.map_err(|_| std::io::ErrorKind::TimedOut)??;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(p) = find_head_end(&buf) {
                break p;
            }
            if buf.len() > 16 * 1024 {
                return respond(&mut stream, 431, &[], b"").await;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let req = parse_request(&head);
        let Some(req) = req else { return respond(&mut stream, 400, &[], b"").await };
        if req.method != "GET" && req.method != "HEAD" {
            return respond(&mut stream, 405, &[("Allow", "GET, HEAD".into())], b"").await;
        }
        // 只接受发往 127.0.0.1:端口 的请求
        if req.host.as_deref() != Some(&format!("127.0.0.1:{}", self.port)) {
            return respond(&mut stream, 403, &[], b"").await;
        }
        let Some(token) = req.path.strip_prefix("/m/") else { return respond(&mut stream, 404, &[], b"").await };
        let Some(entry) = self.lookup(token) else { return respond(&mut stream, 404, &[], b"").await };
        if let Some(remote) = entry.remote {
            return proxy_remote(&mut stream, &req, &remote).await;
        }
        let path = entry.path.clone();
        let growing = entry.growing;
        // 还在下载的文件先试 .part，改名完成后再试最终文件
        let opened = match &growing {
            Some(g) => match tokio::fs::File::open(&g.path).await {
                Ok(f) => Ok(f),
                Err(_) => tokio::fs::File::open(&g.final_path).await,
            },
            None => tokio::fs::File::open(&path).await,
        };
        let Ok(mut file) = opened else { return respond(&mut stream, 404, &[], b"").await };
        let size = match &growing {
            Some(g) => g.total,
            None => file.metadata().await?.len(),
        };
        let mime = mime_for(&path);
        let (status, start, end) = match req.range.as_deref().map(|r| parse_range(r, size)) {
            Some(Ok(Some((s, e)))) => (206, s, e),
            Some(Err(())) => {
                return respond(&mut stream, 416, &[("Content-Range", format!("bytes */{size}"))], b"").await;
            }
            _ => (200, 0, size.saturating_sub(1)),
        };
        let len = if size == 0 { 0 } else { end - start + 1 };
        let mut headers = vec![
            ("Content-Type", mime.to_string()),
            ("Accept-Ranges", "bytes".into()),
            ("Content-Length", len.to_string()),
            ("Cache-Control", "no-store".into()),
        ];
        if status == 206 {
            headers.push(("Content-Range", format!("bytes {start}-{end}/{size}")));
        }
        write_head(&mut stream, status, &headers).await?;
        if req.method == "HEAD" || len == 0 {
            return Ok(());
        }
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let mut left = len;
        let mut pos = start;
        let mut data = vec![0u8; 64 * 1024];
        let mut waited = std::time::Instant::now();
        while left > 0 {
            let mut want = left.min(data.len() as u64);
            if let Some(g) = &growing {
                // 数据还没下载到这里：等一会儿；很久没有进展就结束这次连接
                let avail = (g.available)();
                if pos >= avail {
                    if waited.elapsed() > GROWING_WAIT {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    continue;
                }
                want = want.min(avail - pos);
                waited = std::time::Instant::now();
            }
            let n = file.read(&mut data[..want as usize]).await?;
            if n == 0 {
                if growing.is_some() {
                    // .part 刚好改名：重新打开最终文件接着读
                    if let Some(g) = &growing {
                        if let Ok(mut f2) = tokio::fs::File::open(&g.final_path).await {
                            f2.seek(std::io::SeekFrom::Start(pos)).await?;
                            file = f2;
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            continue;
                        }
                    }
                }
                break;
            }
            // 播放器拖动进度条会断开旧连接，写失败直接结束
            if stream.write_all(&data[..n]).await.is_err() {
                break;
            }
            left -= n as u64;
            pos += n as u64;
        }
        Ok(())
    }
}

/// 把播放器的请求转给源站，再把应答原样（状态、长度、范围）回给播放器。
async fn proxy_remote(stream: &mut TcpStream, req: &Req, remote: &Remote) -> std::io::Result<()> {
    let mut rb = if req.method == "HEAD" { remote.client.head(&remote.url) } else { remote.client.get(&remote.url) };
    for (k, v) in &remote.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    // 不让源站压缩：压缩后字节范围和长度对不上
    rb = rb.header("Accept-Encoding", "identity");
    if let Some(r) = &req.range {
        rb = rb.header("Range", r.as_str());
    }
    let mut resp = match rb.send().await {
        Ok(r) => r,
        Err(e) => return respond(stream, 502, &[("X-Upstream-Error", e.to_string().replace(['\r', '\n'], " "))], b"").await,
    };
    let status = resp.status().as_u16();
    if !(status == 200 || status == 206 || status == 416) {
        return respond(stream, 502, &[("X-Upstream-Status", status.to_string())], b"").await;
    }
    let h = resp.headers();
    let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    let ctype =
        get("content-type").filter(|c| !c.starts_with("application/octet-stream") && !c.starts_with("text/")).unwrap_or_else(|| remote.mime.to_string());
    let mut headers = vec![("Content-Type", ctype), ("Accept-Ranges", "bytes".into()), ("Cache-Control", "no-store".into())];
    if let Some(l) = get("content-length") {
        headers.push(("Content-Length", l));
    }
    if let Some(cr) = get("content-range") {
        headers.push(("Content-Range", cr));
    }
    write_head(stream, status, &headers).await?;
    if req.method == "HEAD" || status == 416 {
        return Ok(());
    }
    // 播放器拖动进度条会断开旧连接，写失败直接结束
    while let Ok(Some(chunk)) = resp.chunk().await {
        if stream.write_all(&chunk).await.is_err() {
            break;
        }
    }
    Ok(())
}

struct Req {
    method: String,
    path: String,
    host: Option<String>,
    range: Option<String>,
}

fn find_head_end(b: &[u8]) -> Option<usize> {
    b.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_request(head: &str) -> Option<Req> {
    let mut lines = head.split("\r\n");
    let first = lines.next()?;
    let mut parts = first.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if !version.starts_with("HTTP/1.") {
        return None;
    }
    // 令牌里没有查询串；丢掉 ? 之后的部分
    let path = target.split('?').next().unwrap_or(target).to_string();
    let (mut host, mut range) = (None, None);
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => host = Some(v.trim().to_string()),
                "range" => range = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    Some(Req { method: method.to_string(), path, host, range })
}

/// 解析 `bytes=a-b` / `bytes=a-` / `bytes=-n`。`Ok(None)` 表示忽略（不是单段范围），`Err` 表示范围无效（416）。
fn parse_range(h: &str, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = h.strip_prefix("bytes=") else { return Ok(None) };
    if spec.contains(',') {
        return Ok(None);
    }
    let Some((a, b)) = spec.split_once('-') else { return Ok(None) };
    if size == 0 {
        return Err(());
    }
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 {
            return Err(());
        }
        (size.saturating_sub(n), size - 1)
    } else {
        let s: u64 = a.parse().map_err(|_| ())?;
        let e: u64 = if b.is_empty() { size - 1 } else { b.parse::<u64>().map_err(|_| ())?.min(size - 1) };
        (s, e)
    };
    if start >= size || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

pub(crate) fn mime_for(p: &Path) -> &'static str {
    match p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("flv") => "video/x-flv",
        Some("ts" | "m2ts" | "mts") => "video/mp2t",
        Some("ogv") => "video/ogg",
        Some("3gp") => "video/3gpp",
        Some("mpg" | "mpeg") => "video/mpeg",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        Some("ogg" | "opus") => "audio/ogg",
        Some("png") => "image/png",
        Some("jpg" | "jpeg" | "jfif") => "image/jpeg",
        Some("avif") => "image/avif",
        Some("tif" | "tiff") => "image/tiff",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        _ => "Error",
    }
}

async fn write_head(stream: &mut TcpStream, status: u16, headers: &[(&str, String)]) -> std::io::Result<()> {
    let mut out = format!("HTTP/1.1 {status} {}\r\nConnection: close\r\n", reason(status));
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    stream.write_all(out.as_bytes()).await
}

async fn respond(stream: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &[u8]) -> std::io::Result<()> {
    let mut h: Vec<(&str, String)> = headers.to_vec();
    h.push(("Content-Length", body.len().to_string()));
    write_head(stream, status, &h).await?;
    stream.write_all(body).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range("bytes=0-99", 1000), Ok(Some((0, 99))));
        assert_eq!(parse_range("bytes=500-", 1000), Ok(Some((500, 999))));
        assert_eq!(parse_range("bytes=-100", 1000), Ok(Some((900, 999))));
        assert_eq!(parse_range("bytes=900-5000", 1000), Ok(Some((900, 999))), "end is clamped");
        assert_eq!(parse_range("bytes=1000-1001", 1000), Err(()));
        assert_eq!(parse_range("bytes=5-2", 1000), Err(()));
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), Ok(None), "multi-range falls back to the whole file");
        assert_eq!(parse_range("items=0-1", 1000), Ok(None));
        assert_eq!(parse_range("bytes=0-0", 0), Err(()));
    }

    #[test]
    fn content_types_cover_the_formats_the_editor_accepts() {
        for (name, mime) in [
            ("a.GIF", "image/gif"),
            ("a.avif", "image/avif"),
            ("a.tif", "image/tiff"),
            ("a.jfif", "image/jpeg"),
            ("a.heic", "image/heic"),
            ("a.ogv", "video/ogg"),
            ("a.mts", "video/mp2t"),
            ("a.mp4", "video/mp4"),
            ("a.xyz", "application/octet-stream"),
        ] {
            assert_eq!(mime_for(Path::new(name)), mime, "{name}");
        }
    }

    #[test]
    fn request_parsing() {
        let r = parse_request("GET /m/abc?x=1 HTTP/1.1\r\nHost: 127.0.0.1:9\r\nRange: bytes=0-1\r\nX: y").unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str(), r.host.as_deref(), r.range.as_deref()), ("GET", "/m/abc", Some("127.0.0.1:9"), Some("bytes=0-1")));
        assert!(parse_request("GET /x\r\n").is_none());
        assert!(parse_request("GET /x SPDY/3\r\n").is_none());
    }

    async fn setup(content: &[u8], name: &str) -> (MediaServer, String, PathBuf) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("clearclip-media-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, content).unwrap();
        let (srv, listener) = MediaServer::bind().unwrap();
        tokio::spawn(srv.clone().serve(listener));
        let url = srv.register(&file);
        (srv, url, dir)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    /// 一个只认 Referer 的“源站”：缺 Referer 回 403，否则按 Range 回数据。
    async fn hotlink_origin(data: Vec<u8>) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let data = data.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                    if !head.contains("referer: https://site.example/") {
                        let _ = s.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                    let (status, body, extra) = match head.split("range: bytes=").nth(1).and_then(|r| r.split("\r\n").next()).and_then(|r| r.split_once('-')) {
                        Some((a, b)) => {
                            let a: usize = a.parse().unwrap();
                            let b: usize = b.parse().unwrap_or(data.len() - 1);
                            (206, data[a..=b].to_vec(), format!("Content-Range: bytes {a}-{b}/{}\r\n", data.len()))
                        }
                        None => (200, data.clone(), String::new()),
                    };
                    let head = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/octet-stream\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(&body).await;
                });
            }
        });
        format!("http://127.0.0.1:{port}/v.mp4")
    }

    #[tokio::test]
    async fn remote_preview_sends_site_headers_and_forwards_ranges() {
        let data: Vec<u8> = (0..3000u32).map(|i| (i % 199) as u8).collect();
        let origin = hotlink_origin(data.clone()).await;
        let (srv, listener) = MediaServer::bind().unwrap();
        tokio::spawn(srv.clone().serve(listener));
        let c = client();
        // 不带 Referer：源站拒绝，本机服务回 502
        let bad = srv.register_remote(Remote { url: origin.clone(), headers: vec![], client: c.clone(), mime: "video/mp4" });
        assert_eq!(c.get(&bad).send().await.unwrap().status(), 502);

        let good = srv.register_remote(Remote {
            url: origin,
            headers: vec![("Referer".into(), "https://site.example/".into())],
            client: c.clone(),
            mime: "video/mp4",
        });
        let r = c.get(&good).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "video/mp4", "octet-stream from the origin is replaced by the guessed type");
        assert_eq!(r.bytes().await.unwrap().as_ref(), data.as_slice());

        let r = c.get(&good).header("Range", "bytes=100-199").send().await.unwrap();
        assert_eq!(r.status(), 206);
        assert_eq!(r.headers()["content-range"], "bytes 100-199/3000");
        assert_eq!(r.bytes().await.unwrap().as_ref(), &data[100..200]);
    }

    #[tokio::test]
    async fn serves_whole_file_ranges_and_head() {
        let data: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let (_srv, url, dir) = setup(&data, "a.webm").await;
        let c = client();
        let r = c.get(&url).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "video/webm");
        assert_eq!(r.headers()["accept-ranges"], "bytes");
        assert_eq!(r.bytes().await.unwrap().as_ref(), data.as_slice());

        let r = c.get(&url).header("Range", "bytes=100-199").send().await.unwrap();
        assert_eq!(r.status(), 206);
        assert_eq!(r.headers()["content-range"], "bytes 100-199/5000");
        assert_eq!(r.bytes().await.unwrap().as_ref(), &data[100..200]);

        let r = c.get(&url).header("Range", "bytes=-50").send().await.unwrap();
        assert_eq!(r.bytes().await.unwrap().as_ref(), &data[4950..]);

        let r = c.get(&url).header("Range", "bytes=9000-").send().await.unwrap();
        assert_eq!(r.status(), 416);
        assert_eq!(r.headers()["content-range"], "bytes */5000");

        let r = c.head(&url).send().await.unwrap();
        assert_eq!((r.status().as_u16(), r.headers()["content-length"].to_str().unwrap()), (200, "5000"));
        assert!(r.bytes().await.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_file_that_is_still_downloading_is_served_as_it_grows() {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 241) as u8).collect();
        let dir = std::env::temp_dir().join(format!("clearclip-media-grow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (part, fin) = (dir.join("v.mp4.part"), dir.join("v.mp4"));
        std::fs::write(&part, &data[..100]).unwrap();
        let have = Arc::new(AtomicU64::new(100));
        let (srv, listener) = MediaServer::bind().unwrap();
        tokio::spawn(srv.clone().serve(listener));
        let have2 = have.clone();
        let url = srv.register_growing(Growing {
            path: part.clone(),
            final_path: fin.clone(),
            total: 1000,
            available: Arc::new(move || have2.load(Ordering::SeqCst)),
        });

        // 头信息按总大小回应，类型按最终文件名判断
        let c = client();
        let r = c.head(&url).send().await.unwrap();
        assert_eq!((r.headers()["content-length"].to_str().unwrap(), r.headers()["content-type"].to_str().unwrap()), ("1000", "video/mp4"));

        // 后台继续下载：先补到 600，再补完并把 .part 改名
        let (data2, part2, fin2) = (data.clone(), part.clone(), fin.clone());
        let writer = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let mut f = std::fs::OpenOptions::new().append(true).open(&part2).unwrap();
            f.write_all(&data2[100..600]).unwrap();
            have.store(600, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            f.write_all(&data2[600..]).unwrap();
            drop(f);
            std::fs::rename(&part2, &fin2).unwrap();
            have.store(1000, Ordering::SeqCst);
        });
        // 完整读取：会在还没下载到的位置等待，最后拿到全部内容
        let r = c.get(&url).send().await.unwrap();
        assert_eq!(r.bytes().await.unwrap().as_ref(), data.as_slice());
        // 拖动到后面：范围请求同样按总大小计算
        let r = c.get(&url).header("Range", "bytes=700-799").send().await.unwrap();
        assert_eq!(
            (r.status(), r.headers()["content-range"].to_str().unwrap().to_string()),
            (reqwest::StatusCode::PARTIAL_CONTENT, "bytes 700-799/1000".to_string())
        );
        assert_eq!(r.bytes().await.unwrap().as_ref(), &data[700..800]);
        writer.await.unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn rejects_unknown_tokens_bad_hosts_and_other_methods() {
        let (srv, url, dir) = setup(b"hello", "a.mp4").await;
        let c = client();
        let port = srv.port();
        assert_eq!(c.get(format!("http://127.0.0.1:{port}/m/deadbeef")).send().await.unwrap().status(), 404);
        assert_eq!(c.get(format!("http://127.0.0.1:{port}/etc/passwd")).send().await.unwrap().status(), 404);
        assert_eq!(c.post(&url).send().await.unwrap().status(), 405);
        // 伪造 Host（DNS 重绑定）
        assert_eq!(c.get(&url).header("Host", "evil.example.com").send().await.unwrap().status(), 403);
        // 登记被清空后再取不到
        srv.clear();
        assert_eq!(c.get(&url).send().await.unwrap().status(), 404);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn registering_twice_reuses_the_token_and_empty_files_work() {
        let (srv, url, dir) = setup(b"", "e.mp3").await;
        assert_eq!(srv.register(&dir.join("e.mp3")), url);
        let r = client().get(&url).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.bytes().await.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
