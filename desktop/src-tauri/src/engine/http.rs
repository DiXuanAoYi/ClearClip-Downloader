//! HTTP 下载：单线程流式下载与分段并行下载，都支持断点续传和一致性校验。

use std::io::SeekFrom;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;

use super::{ensure_space, wait_ctrl, DlError, ResumeMeta, Segment, CTRL_RUN};
use crate::net::NetManager;

const MB: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl HttpRequest {
    pub fn new(url: impl Into<String>) -> Self {
        HttpRequest { url: url.into(), headers: vec![] }
    }

    pub fn header(mut self, k: &str, v: impl Into<String>) -> Self {
        let v = v.into();
        if !v.is_empty() {
            self.headers.push((k.to_string(), v));
        }
        self
    }

    pub fn build_get(&self, client: &reqwest::Client) -> reqwest::RequestBuilder {
        self.build(client)
    }

    fn build(&self, client: &reqwest::Client) -> reqwest::RequestBuilder {
        let mut req = client.get(&self.url);
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req
    }
}

#[derive(Clone, Default)]
pub struct HttpOptions {
    /// 分段数；1 表示单线程
    pub segments: usize,
    /// 文件不小于该大小才分段
    pub min_segment_size: u64,
    pub speed_limit_kbps: u64,
    /// 要求保留的剩余磁盘空间
    pub disk_reserve: u64,
    pub net: Option<Arc<NetManager>>,
}

impl HttpOptions {
    async fn throttle(&self, bytes: usize) {
        if let Some(net) = &self.net {
            net.throttle(bytes, self.speed_limit_kbps).await;
        }
    }
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// 解析 `Content-Range: bytes 100-199/1000`，返回 (起点, 总大小)。
pub fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    let total = total.trim().parse().ok();
    Some((start, total))
}

fn header_str(resp: &reqwest::Response, name: &str) -> Option<String> {
    resp.headers().get(name).and_then(|v| v.to_str().ok()).map(|s| s.to_string())
}

#[derive(Debug, Clone, Default)]
pub struct ProbeInfo {
    pub total: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// 服务器对 Range 请求返回了 206
    pub ranges: bool,
}

/// 用 1 字节的 Range 请求探测文件总大小和是否支持分段。
pub async fn probe(client: &reqwest::Client, req: &HttpRequest) -> Result<ProbeInfo, DlError> {
    let resp = req.build(client).header("Range", "bytes=0-0").send().await.map_err(|e| DlError::Net(e.without_url().to_string()))?;
    let status = resp.status().as_u16();
    let etag = header_str(&resp, "etag").filter(|e| !e.starts_with("W/"));
    let last_modified = header_str(&resp, "last-modified");
    match status {
        206 => Ok(ProbeInfo {
            total: header_str(&resp, "content-range").and_then(|v| parse_content_range(&v)).and_then(|(_, t)| t),
            etag,
            last_modified,
            ranges: true,
        }),
        200..=299 => Ok(ProbeInfo { total: resp.content_length(), etag, last_modified, ranges: false }),
        _ => Err(DlError::Status(status)),
    }
}

pub async fn probe_total(client: &reqwest::Client, req: &HttpRequest) -> Option<u64> {
    probe(client, req).await.ok().and_then(|p| p.total)
}

fn plan_segments(total: u64, n: usize) -> Vec<Segment> {
    let n = (n as u64).min(total / MB).max(1);
    let size = total / n;
    (0..n)
        .map(|i| {
            let start = i * size;
            let end = if i == n - 1 { total - 1 } else { (i + 1) * size - 1 };
            Segment { start, end, done: 0 }
        })
        .collect()
}

/// 下载到 `part`：优先续传已有进度；新任务在文件足够大且服务器支持 Range 时分段并行，否则单线程。
pub async fn download(
    client: &reqwest::Client,
    req: &HttpRequest,
    part: &Path,
    meta: &mut ResumeMeta,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    mut on_progress: impl FnMut(u64, Option<u64>, &ResumeMeta) + Send,
) -> Result<u64, DlError> {
    if !meta.segments.is_empty() {
        match segmented(client, req, part, meta, ctrl, opts, &mut on_progress).await {
            Err(SegError::Mismatch) => {
                log::info!("segment resume rejected by server; restarting single-threaded");
                let _ = tokio::fs::remove_file(part).await;
                *meta = ResumeMeta::default();
            }
            Err(SegError::Dl(e)) => return Err(e),
            Ok(n) => return Ok(n),
        }
    }

    if file_len(part) == 0 && opts.segments > 1 {
        let info = tokio::select! {
            r = probe(client, req) => r?,
            c = wait_ctrl(ctrl) => return Err(DlError::from_ctrl(c)),
        };
        if let (true, Some(total)) = (info.ranges, info.total) {
            if total >= opts.min_segment_size.max(2 * MB) {
                if let Some(dir) = part.parent() {
                    ensure_space(dir, total, opts.disk_reserve)?;
                }
                *meta = ResumeMeta {
                    etag: info.etag,
                    last_modified: info.last_modified,
                    total: Some(total),
                    resumable: Some(true),
                    segments: plan_segments(total, opts.segments),
                };
                match segmented(client, req, part, meta, ctrl, opts, &mut on_progress).await {
                    Err(SegError::Mismatch) => {
                        let _ = tokio::fs::remove_file(part).await;
                        *meta = ResumeMeta::default();
                    }
                    Err(SegError::Dl(e)) => return Err(e),
                    Ok(n) => return Ok(n),
                }
            }
        }
    }
    single(client, req, part, meta, ctrl, opts, &mut on_progress).await
}

/// 单线程流式下载，支持断点续传：
/// - 已有部分内容时发送 `Range`，并用 `If-Range`（ETag / Last-Modified）确认服务器上的文件没变；
/// - 收到 206 时核对 `Content-Range` 起点与总大小，不一致就从头下载；
/// - 服务器返回 200（不支持续传或文件已变化）时从头下载；
/// - 暂停 / 取消立即生效，不等待下一块数据。
pub async fn single(
    client: &reqwest::Client,
    req: &HttpRequest,
    part: &Path,
    meta: &mut ResumeMeta,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    on_progress: &mut (impl FnMut(u64, Option<u64>, &ResumeMeta) + Send),
) -> Result<u64, DlError> {
    meta.segments.clear();
    for attempt in 0..2 {
        let existing = if attempt == 0 { file_len(part) } else { 0 };
        if attempt > 0 {
            let _ = tokio::fs::remove_file(part).await;
        }
        let mut rb = req.build(client);
        if existing > 0 {
            rb = rb.header("Range", format!("bytes={existing}-"));
            if let Some(v) = meta.etag.as_deref().or(meta.last_modified.as_deref()) {
                rb = rb.header("If-Range", v);
            }
        }
        let resp = tokio::select! {
            r = rb.send() => r.map_err(|e| DlError::Net(e.without_url().to_string()))?,
            c = wait_ctrl(ctrl) => return Err(DlError::from_ctrl(c)),
        };
        let status = resp.status().as_u16();

        if status == 416 && existing > 0 {
            if meta.total == Some(existing) {
                on_progress(existing, Some(existing), meta);
                return Ok(existing);
            }
            *meta = ResumeMeta::default();
            continue;
        }

        let (start, append) = match status {
            206 => match header_str(&resp, "content-range").and_then(|v| parse_content_range(&v)) {
                Some((s, total)) if s == existing && (meta.total.is_none() || total.is_none() || total == meta.total) => {
                    if total.is_some() {
                        meta.total = total;
                    }
                    (existing, true)
                }
                _ => {
                    log::info!("content-range mismatch, restarting download from zero");
                    *meta = ResumeMeta::default();
                    continue;
                }
            },
            200..=299 => {
                if existing > 0 {
                    log::info!("server returned {status} for a range request; restarting from zero");
                }
                meta.total = resp.content_length();
                (0, false)
            }
            _ => return Err(DlError::Status(status)),
        };

        let accepts_ranges = header_str(&resp, "accept-ranges").is_some_and(|v| v.contains("bytes"));
        meta.resumable = Some(status == 206 || accepts_ranges);
        if let Some(etag) = header_str(&resp, "etag").filter(|e| !e.starts_with("W/")) {
            meta.etag = Some(etag);
        }
        if let Some(lm) = header_str(&resp, "last-modified") {
            meta.last_modified = Some(lm);
        }
        if let (Some(total), Some(dir)) = (meta.total, part.parent()) {
            ensure_space(dir, total.saturating_sub(start), opts.disk_reserve)?;
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(part)
            .await
            .map_err(|e| DlError::Io(format!("无法写入文件 {}：{e}", part.display())))?;

        let mut received = start;
        let total = meta.total;
        on_progress(received, total, meta);
        let mut stream = resp.bytes_stream();
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                c = wait_ctrl(ctrl) => {
                    let _ = file.flush().await;
                    return Err(DlError::from_ctrl(c));
                }
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| DlError::Net(e.without_url().to_string()))?;
            file.write_all(&chunk).await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
            received += chunk.len() as u64;
            on_progress(received, total, meta);
            opts.throttle(chunk.len()).await;
        }
        file.flush().await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
        if let Some(t) = total {
            if received < t {
                return Err(DlError::Net(format!("连接提前结束（{received}/{t} 字节）")));
            }
        }
        return Ok(received);
    }
    Err(DlError::Other("服务器的续传响应不一致，请稍后重试".into()))
}

enum SegError {
    /// 服务器的分段响应与记录不一致（文件变化或不支持 Range），需要从头单线程下载
    Mismatch,
    Dl(DlError),
}

impl From<DlError> for SegError {
    fn from(e: DlError) -> Self {
        SegError::Dl(e)
    }
}

/// 分段并行下载：文件预分配到总大小，每段用 Range 下载并写到对应位置；每段的进度记录在 `meta.segments`。
async fn segmented(
    client: &reqwest::Client,
    req: &HttpRequest,
    part: &Path,
    meta: &mut ResumeMeta,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    on_progress: &mut (impl FnMut(u64, Option<u64>, &ResumeMeta) + Send),
) -> Result<u64, SegError> {
    let Some(total) = meta.total else { return Err(SegError::Mismatch) };
    if file_len(part) != total {
        // 预分配；已有进度但文件大小不对时，进度作废
        if part.exists() {
            for s in &mut meta.segments {
                s.done = 0;
            }
        }
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(part).map_err(|e| DlError::Io(format!("无法创建文件：{e}")))?;
        f.set_len(total).map_err(|e| DlError::Io(format!("无法预分配文件：{e}")))?;
    }
    let validator = meta.etag.clone().or(meta.last_modified.clone());
    let state = Arc::new(Mutex::new(meta.segments.clone()));
    let (stop_tx, stop_rx) = watch::channel(CTRL_RUN);
    let mut set = tokio::task::JoinSet::new();
    for i in 0..meta.segments.len() {
        if meta.segments[i].is_complete() {
            continue;
        }
        let (client, req, part, state, validator, opts) = (client.clone(), req.clone(), part.to_path_buf(), state.clone(), validator.clone(), opts.clone());
        let mut stop = stop_rx.clone();
        set.spawn(async move { run_segment(&client, &req, &part, i, &state, validator.as_deref(), &mut stop, &opts).await });
    }

    let snapshot = |state: &Arc<Mutex<Vec<Segment>>>, meta: &mut ResumeMeta| {
        meta.segments = state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        meta.segmented_received()
    };
    let mut result: Result<(), SegError> = Ok(());
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            joined = set.join_next() => match joined {
                None => break,
                Some(Ok(Ok(()))) => {}
                Some(Ok(Err(e))) => {
                    if result.is_ok() {
                        result = Err(e);
                        let _ = stop_tx.send(super::CTRL_CANCEL);
                    }
                }
                Some(Err(join)) => {
                    if result.is_ok() {
                        result = Err(SegError::Dl(DlError::Other(format!("分段任务异常：{join}"))));
                        let _ = stop_tx.send(super::CTRL_CANCEL);
                    }
                }
            },
            _ = tick.tick() => {
                let got = snapshot(&state, meta);
                on_progress(got, Some(total), meta);
            }
            c = wait_ctrl(ctrl), if result.is_ok() => {
                result = Err(SegError::Dl(DlError::from_ctrl(c)));
                let _ = stop_tx.send(c);
            }
        }
    }
    let got = snapshot(&state, meta);
    on_progress(got, Some(total), meta);
    result?;
    if meta.segments.iter().all(Segment::is_complete) {
        meta.segments.clear();
        Ok(total)
    } else {
        Err(SegError::Dl(DlError::Net("部分分段未完成".into())))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_segment(
    client: &reqwest::Client,
    req: &HttpRequest,
    part: &Path,
    i: usize,
    state: &Arc<Mutex<Vec<Segment>>>,
    validator: Option<&str>,
    stop: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
) -> Result<(), SegError> {
    let seg = state.lock().unwrap_or_else(|e| e.into_inner())[i];
    let from = seg.start + seg.done;
    let mut rb = req.build(client).header("Range", format!("bytes={from}-{}", seg.end));
    if let Some(v) = validator {
        rb = rb.header("If-Range", v);
    }
    let resp = tokio::select! {
        r = rb.send() => r.map_err(|e| DlError::Net(e.without_url().to_string()))?,
        c = wait_ctrl(stop) => return Err(SegError::Dl(DlError::from_ctrl(c))),
    };
    match resp.status().as_u16() {
        206 => {}
        200 => return Err(SegError::Mismatch),
        s => return Err(SegError::Dl(DlError::Status(s))),
    }
    match header_str(&resp, "content-range").and_then(|v| parse_content_range(&v)) {
        Some((s, _)) if s == from => {}
        _ => return Err(SegError::Mismatch),
    }
    let mut file = tokio::fs::OpenOptions::new().write(true).open(part).await.map_err(|e| DlError::Io(format!("无法写入文件：{e}")))?;
    file.seek(SeekFrom::Start(from)).await.map_err(|e| DlError::Io(e.to_string()))?;
    let mut remaining = seg.end + 1 - from;
    let mut stream = resp.bytes_stream();
    while remaining > 0 {
        let next = tokio::select! {
            n = stream.next() => n,
            c = wait_ctrl(stop) => {
                let _ = file.flush().await;
                return Err(SegError::Dl(DlError::from_ctrl(c)));
            }
        };
        let Some(chunk) = next else { break };
        let chunk = chunk.map_err(|e| DlError::Net(e.without_url().to_string()))?;
        let take = (chunk.len() as u64).min(remaining) as usize;
        file.write_all(&chunk[..take]).await.map_err(|e| DlError::Io(format!("写入失败：{e}")))?;
        remaining -= take as u64;
        state.lock().unwrap_or_else(|e| e.into_inner())[i].done += take as u64;
        opts.throttle(take).await;
    }
    file.flush().await.map_err(|e| DlError::Io(e.to_string()))?;
    if remaining > 0 {
        return Err(SegError::Dl(DlError::Net("分段连接提前结束".into())));
    }
    Ok(())
}

pub fn build_download_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(crate::providers::MOBILE_UA)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("failed to build download client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CTRL_PAUSE;
    use std::path::PathBuf;
    use std::time::Instant;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// 3 MB 的测试数据，足够触发分段（最小 2 MB）。
    fn body() -> &'static [u8] {
        static B: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        B.get_or_init(|| (0..3 * MB as usize).map(|i| (i * 31 % 251) as u8).collect())
    }
    const SMALL: &[u8] = b"0123456789abcdefghij";

    fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
        req.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    fn parse_range(r: &str, len: usize) -> Option<(usize, usize)> {
        let r = r.strip_prefix("bytes=")?;
        let (a, b) = r.split_once('-')?;
        let a: usize = a.parse().ok()?;
        let b: usize = if b.is_empty() { len - 1 } else { b.parse().ok()? };
        Some((a, b.min(len - 1)))
    }

    /// 模拟服务器。/small（20 字节，ETag v1）、/big（3 MB，ETag v1，支持任意 Range）、/changed（ETag v2）、
    /// /badrange、/norange、/stall、/forbidden；`?slow` 使 /big 每 64 KB 停顿 20 毫秒。
    async fn serve() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let full_path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let slow = full_path.contains("slow");
                    let path = full_path.split('?').next().unwrap_or("/").to_string();
                    let etag = if path == "/changed" { "\"v2\"" } else { "\"v1\"" };
                    let data: &[u8] = if path == "/big" { body() } else { SMALL };
                    let range = header(&req, "range").and_then(|r| parse_range(r, data.len()));
                    let if_range_ok = header(&req, "if-range").is_none_or(|v| v == etag);
                    let send = |status: &str, extra: String, payload: &[u8]| {
                        let mut v = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nETag: {etag}\r\nAccept-Ranges: bytes\r\n{extra}Connection: close\r\n\r\n",
                            payload.len()
                        )
                        .into_bytes();
                        v.extend_from_slice(payload);
                        v
                    };
                    let resp: Vec<u8> = match path.as_str() {
                        "/forbidden" => b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                        "/norange" => b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n0123456789abcdefghij".to_vec(),
                        "/badrange" if range.is_some() => send("206 Partial Content", "Content-Range: bytes 0-19/20\r\n".to_string(), SMALL),
                        "/stall" => {
                            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n01234").await;
                            tokio::time::sleep(Duration::from_secs(20)).await;
                            return;
                        }
                        _ => match range {
                            Some((a, _)) if a >= data.len() => b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                            Some((a, b)) if if_range_ok => {
                                send("206 Partial Content", format!("Content-Range: bytes {a}-{b}/{}\r\n", data.len()), &data[a..=b])
                            }
                            _ => send("200 OK", String::new(), data),
                        },
                    };
                    if slow {
                        for chunk in resp.chunks(64 * 1024) {
                            if sock.write_all(chunk).await.is_err() {
                                return;
                            }
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    } else {
                        let _ = sock.write_all(&resp).await;
                    }
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clearclip-http-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("f.bin.part")
    }

    fn opts(segments: usize) -> HttpOptions {
        HttpOptions { segments, min_segment_size: 2 * MB, ..Default::default() }
    }

    async fn dl(url: &str, part: &Path, meta: &mut ResumeMeta, segments: usize) -> Result<u64, DlError> {
        let (_tx, mut rx) = watch::channel(CTRL_RUN);
        download(&build_download_client(), &HttpRequest::new(url), part, meta, &mut rx, &opts(segments), |_, _, _| {}).await
    }

    #[tokio::test]
    async fn downloads_full_file_and_records_meta() {
        let base = serve().await;
        let part = tmp("full");
        let mut meta = ResumeMeta::default();
        assert_eq!(dl(&format!("{base}/small"), &part, &mut meta, 1).await.unwrap(), 20);
        assert_eq!(std::fs::read(&part).unwrap(), SMALL);
        assert_eq!(meta.etag.as_deref(), Some("\"v1\""));
        assert_eq!((meta.total, meta.resumable), (Some(20), Some(true)));
    }

    #[tokio::test]
    async fn resumes_with_matching_etag() {
        let base = serve().await;
        let part = tmp("resume");
        std::fs::write(&part, &SMALL[..8]).unwrap();
        let mut meta = ResumeMeta { etag: Some("\"v1\"".into()), total: Some(20), ..Default::default() };
        assert_eq!(dl(&format!("{base}/small"), &part, &mut meta, 1).await.unwrap(), 20);
        assert_eq!(std::fs::read(&part).unwrap(), SMALL);
    }

    #[tokio::test]
    async fn restarts_when_file_changed_on_server() {
        let base = serve().await;
        let part = tmp("changed");
        std::fs::write(&part, b"XXXXXXXX").unwrap();
        let mut meta = ResumeMeta { etag: Some("\"v1\"".into()), total: Some(20), ..Default::default() };
        assert_eq!(dl(&format!("{base}/changed"), &part, &mut meta, 1).await.unwrap(), 20);
        assert_eq!(std::fs::read(&part).unwrap(), SMALL, "stale bytes must be discarded");
        assert_eq!(meta.etag.as_deref(), Some("\"v2\""));
    }

    #[tokio::test]
    async fn restarts_on_content_range_mismatch() {
        let base = serve().await;
        let part = tmp("badrange");
        std::fs::write(&part, b"XXXXXXXX").unwrap();
        assert_eq!(dl(&format!("{base}/badrange"), &part, &mut ResumeMeta::default(), 1).await.unwrap(), 20);
        assert_eq!(std::fs::read(&part).unwrap(), SMALL);
    }

    #[tokio::test]
    async fn restarts_when_server_ignores_range() {
        let base = serve().await;
        let part = tmp("norange");
        std::fs::write(&part, b"XXXXXXXX").unwrap();
        let mut meta = ResumeMeta::default();
        assert_eq!(dl(&format!("{base}/norange"), &part, &mut meta, 1).await.unwrap(), 20);
        assert_eq!(std::fs::read(&part).unwrap(), SMALL);
        assert_eq!(meta.resumable, Some(false));
    }

    #[tokio::test]
    async fn complete_file_with_416_is_done() {
        let base = serve().await;
        let part = tmp("416");
        std::fs::write(&part, SMALL).unwrap();
        let mut meta = ResumeMeta { total: Some(20), ..Default::default() };
        assert_eq!(dl(&format!("{base}/small"), &part, &mut meta, 1).await.unwrap(), 20);
    }

    #[tokio::test]
    async fn reports_http_status() {
        let base = serve().await;
        let err = dl(&format!("{base}/forbidden"), &tmp("403"), &mut ResumeMeta::default(), 1).await.unwrap_err();
        assert!(matches!(err, DlError::Status(403)));
        assert_eq!(err.kind(), crate::error::ErrorKind::NeedLogin);
    }

    #[tokio::test]
    async fn pause_takes_effect_while_stream_is_stalled() {
        let base = serve().await;
        let part = tmp("stall");
        let (tx, mut rx) = watch::channel(CTRL_RUN);
        let url = format!("{base}/stall");
        let task = tokio::spawn(async move {
            let mut meta = ResumeMeta::default();
            download(&build_download_client(), &HttpRequest::new(url), &part, &mut meta, &mut rx, &opts(1), |_, _, _| {}).await
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        let t0 = Instant::now();
        tx.send(CTRL_PAUSE).unwrap();
        let res = tokio::time::timeout(Duration::from_secs(2), task).await.expect("pause must not wait for the read timeout").unwrap();
        assert!(matches!(res, Err(DlError::Paused)));
        assert!(t0.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn segmented_download_produces_identical_file() {
        let base = serve().await;
        let part = tmp("seg");
        let mut meta = ResumeMeta::default();
        assert_eq!(dl(&format!("{base}/big"), &part, &mut meta, 4).await.unwrap(), 3 * MB);
        assert_eq!(std::fs::read(&part).unwrap(), body());
        assert!(meta.segments.is_empty(), "segments are cleared once complete");
    }

    #[tokio::test]
    async fn segmented_download_resumes_after_pause() {
        let base = serve().await;
        let part = tmp("segpause");
        let (tx, mut rx) = watch::channel(CTRL_RUN);
        let url = format!("{base}/big?slow");
        let mut meta = ResumeMeta::default();
        let p2 = part.clone();
        let handle = tokio::spawn(async move {
            let r = download(&build_download_client(), &HttpRequest::new(url), &p2, &mut meta, &mut rx, &opts(3), |_, _, _| {}).await;
            (r, meta)
        });
        tokio::time::sleep(Duration::from_millis(250)).await;
        tx.send(CTRL_PAUSE).unwrap();
        let (r, mut meta) = handle.await.unwrap();
        assert!(matches!(r, Err(DlError::Paused)));
        assert_eq!(meta.segments.len(), 3);
        let partial = meta.segmented_received();
        assert!(partial > 0 && partial < 3 * MB, "partial progress recorded: {partial}");
        // 继续：只补未完成的部分，结果与原文件一致
        assert_eq!(dl(&format!("{base}/big"), &part, &mut meta, 3).await.unwrap(), 3 * MB);
        assert_eq!(std::fs::read(&part).unwrap(), body());
    }

    #[tokio::test]
    async fn segmented_resume_falls_back_when_file_changed() {
        let base = serve().await;
        let part = tmp("segchanged");
        // 伪造一份针对旧版本（ETag v0）的分段进度
        let total = body().len() as u64;
        std::fs::write(&part, vec![0u8; total as usize]).unwrap();
        let mut meta =
            ResumeMeta { etag: Some("\"v0\"".into()), last_modified: None, total: Some(total), resumable: Some(true), segments: plan_segments(total, 3) };
        meta.segments[0].done = 1000;
        assert_eq!(dl(&format!("{base}/big"), &part, &mut meta, 3).await.unwrap(), total);
        assert_eq!(std::fs::read(&part).unwrap(), body());
    }

    #[tokio::test]
    async fn probe_reads_content_range() {
        let base = serve().await;
        let info = probe(&build_download_client(), &HttpRequest::new(format!("{base}/big"))).await.unwrap();
        assert_eq!(info.total, Some(3 * MB));
        assert!(info.ranges);
    }

    #[test]
    fn segment_planning() {
        let s = plan_segments(10 * MB + 7, 4);
        assert_eq!(s.len(), 4);
        assert_eq!(s[0].start, 0);
        assert_eq!(s[3].end, 10 * MB + 6);
        for w in s.windows(2) {
            assert_eq!(w[0].end + 1, w[1].start);
        }
        assert_eq!(plan_segments(MB / 2, 8).len(), 1);
    }

    #[test]
    fn content_range_parsing() {
        assert_eq!(parse_content_range("bytes 100-199/1000"), Some((100, Some(1000))));
        assert_eq!(parse_content_range("bytes 0-0/*"), Some((0, None)));
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }
}
