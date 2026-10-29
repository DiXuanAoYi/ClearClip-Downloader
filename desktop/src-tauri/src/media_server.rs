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

#[derive(Clone)]
pub struct MediaServer {
    port: u16,
    files: Arc<Mutex<HashMap<String, PathBuf>>>,
}

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
        if let Some((tok, _)) = g.iter().find(|(_, p)| p.as_path() == path) {
            return format!("http://127.0.0.1:{}/m/{tok}", self.port);
        }
        let tok = hex::encode(random_bytes::<16>());
        g.insert(tok.clone(), path.to_path_buf());
        format!("http://127.0.0.1:{}/m/{tok}", self.port)
    }

    pub fn clear(&self) {
        self.files.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn lookup(&self, token: &str) -> Option<PathBuf> {
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
        let Some(path) = self.lookup(token) else { return respond(&mut stream, 404, &[], b"").await };
        let Ok(mut file) = tokio::fs::File::open(&path).await else { return respond(&mut stream, 404, &[], b"").await };
        let size = file.metadata().await?.len();
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
        let mut data = vec![0u8; 64 * 1024];
        while left > 0 {
            let want = left.min(data.len() as u64) as usize;
            let n = file.read(&mut data[..want]).await?;
            if n == 0 {
                break;
            }
            // 播放器拖动进度条会断开旧连接，写失败直接结束
            if stream.write_all(&data[..n]).await.is_err() {
                break;
            }
            left -= n as u64;
        }
        Ok(())
    }
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

fn mime_for(p: &Path) -> &'static str {
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
