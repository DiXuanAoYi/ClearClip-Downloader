//! m3u8（HLS）下载：分片并发下载、AES-128 解密、按分片续传（已完成的分片保存在临时目录）、
//! 可选跳过广告分片，全部完成后按顺序拼接。拼接结果由调用方用 ffmpeg 转为 MP4。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Semaphore};
use url::Url;

use super::http::HttpRequest;
use super::{ensure_space, wait_ctrl, DlError};
use crate::net::NetManager;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub url: String,
    pub bandwidth: u64,
    pub resolution: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsKey {
    pub method: String,
    pub uri: Option<String>,
    pub iv: Option<[u8; 16]>,
    pub keyformat: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HlsSegment {
    pub url: String,
    pub duration: f64,
    pub seq: u64,
    pub key: Option<HlsKey>,
    /// 第几个不连续段（#EXT-X-DISCONTINUITY 分隔）
    pub group: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaPlaylist {
    pub segments: Vec<HlsSegment>,
    pub init: Option<String>,
    pub end_list: bool,
    /// #EXT-X-TARGETDURATION（秒），直播轮询间隔参考
    pub target_duration: f64,
}

impl MediaPlaylist {
    pub fn duration(&self) -> f64 {
        self.segments.iter().map(|s| s.duration).sum()
    }

    pub fn is_fmp4(&self) -> bool {
        self.init.is_some() || self.segments.first().is_some_and(|s| s.url.split('?').next().is_some_and(|p| p.ends_with(".m4s") || p.ends_with(".mp4")))
    }
}

pub fn is_master(text: &str) -> bool {
    text.contains("#EXT-X-STREAM-INF")
}

fn join(base: &Url, rel: &str) -> String {
    base.join(rel.trim()).map(|u| u.to_string()).unwrap_or_else(|_| rel.trim().to_string())
}

/// 解析标签里的属性：`KEY=VALUE,KEY2="VALUE2"`。
fn attrs(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = s;
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().to_ascii_uppercase();
        rest = &rest[eq + 1..];
        let value;
        if let Some(r) = rest.strip_prefix('"') {
            let end = r.find('"').unwrap_or(r.len());
            value = r[..end].to_string();
            rest = r.get(end + 1..).unwrap_or("");
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].trim().to_string();
            rest = &rest[end..];
        }
        rest = rest.trim_start_matches(',');
        out.insert(key, value);
    }
    out
}

fn parse_iv(v: &str) -> Option<[u8; 16]> {
    let hex = v.trim().trim_start_matches("0x").trim_start_matches("0X");
    let bytes = hex::decode(format!("{hex:0>32}")).ok()?;
    bytes.try_into().ok()
}

pub fn parse_master(text: &str, base: &str) -> Vec<Variant> {
    let Ok(base) = Url::parse(base) else { return vec![] };
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(a) = l.strip_prefix("#EXT-X-STREAM-INF:") {
            let a = attrs(a);
            let Some(uri) = lines[i + 1..].iter().find(|x| !x.is_empty() && !x.starts_with('#')) else { continue };
            let resolution = a.get("RESOLUTION").and_then(|r| r.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
            out.push(Variant { url: join(&base, uri), bandwidth: a.get("BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0), resolution });
        }
    }
    out
}

pub fn parse_media(text: &str, base: &str) -> Result<MediaPlaylist, DlError> {
    let base = Url::parse(base).map_err(|_| DlError::Other("m3u8 地址无效".into()))?;
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(DlError::Other("不是有效的 m3u8 播放列表".into()));
    }
    let mut pl = MediaPlaylist::default();
    let mut seq: u64 = 0;
    let mut duration = 0.0;
    let mut key: Option<HlsKey> = None;
    let mut group = 0u32;
    for l in text.lines().map(str::trim) {
        if let Some(v) = l.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            seq = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("#EXT-X-TARGETDURATION:") {
            pl.target_duration = v.trim().parse().unwrap_or(0.0);
        } else if let Some(v) = l.strip_prefix("#EXTINF:") {
            duration = v.split(',').next().and_then(|d| d.trim().parse().ok()).unwrap_or(0.0);
        } else if let Some(v) = l.strip_prefix("#EXT-X-KEY:") {
            let a = attrs(v);
            let method = a.get("METHOD").cloned().unwrap_or_else(|| "NONE".into());
            key = (method != "NONE").then(|| HlsKey {
                method,
                uri: a.get("URI").map(|u| join(&base, u)),
                iv: a.get("IV").and_then(|iv| parse_iv(iv)),
                keyformat: a.get("KEYFORMAT").cloned(),
            });
        } else if let Some(v) = l.strip_prefix("#EXT-X-MAP:") {
            pl.init = attrs(v).get("URI").map(|u| join(&base, u));
        } else if l == "#EXT-X-DISCONTINUITY" {
            group += 1;
        } else if l == "#EXT-X-ENDLIST" {
            pl.end_list = true;
        } else if l.starts_with("#EXT-X-BYTERANGE") {
            return Err(DlError::Other("暂不支持使用 BYTERANGE 的 m3u8".into()));
        } else if !l.is_empty() && !l.starts_with('#') {
            pl.segments.push(HlsSegment { url: join(&base, l), duration, seq, key: key.clone(), group });
            seq += 1;
            duration = 0.0;
        }
    }
    if pl.segments.is_empty() {
        return Err(DlError::Other("m3u8 里没有分片".into()));
    }
    Ok(pl)
}

/// 跳过广告：去掉与大多数分片来源（主机名）不同的不连续段。
pub fn filter_ads(pl: &mut MediaPlaylist) -> usize {
    let host = |u: &str| Url::parse(u).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for s in &pl.segments {
        *counts.entry(host(&s.url)).or_default() += 1;
    }
    let Some((main, _)) = counts.into_iter().max_by_key(|(_, c)| *c) else { return 0 };
    let mut ad_groups: Vec<u32> = Vec::new();
    for g in pl.segments.iter().map(|s| s.group).collect::<std::collections::BTreeSet<_>>() {
        if pl.segments.iter().filter(|s| s.group == g).all(|s| host(&s.url) != main) {
            ad_groups.push(g);
        }
    }
    let before = pl.segments.len();
    pl.segments.retain(|s| !ad_groups.contains(&s.group));
    before - pl.segments.len()
}

#[derive(Clone, Default)]
pub struct HlsOptions {
    pub concurrency: usize,
    pub skip_ads: bool,
    pub speed_limit_kbps: u64,
    pub disk_reserve: u64,
    pub net: Option<Arc<NetManager>>,
}

pub struct HlsResult {
    pub size: u64,
    pub fmp4: bool,
    pub skipped_ads: usize,
}

async fn fetch_bytes(client: &reqwest::Client, req: &HttpRequest, url: &str) -> Result<Vec<u8>, DlError> {
    let mut r = HttpRequest { url: url.to_string(), headers: req.headers.clone() }.build_get(client);
    r = r.timeout(Duration::from_secs(60));
    let resp = r.send().await.map_err(|e| DlError::Net(e.without_url().to_string()))?;
    if !resp.status().is_success() {
        return Err(DlError::Status(resp.status().as_u16()));
    }
    Ok(resp.bytes().await.map_err(|e| DlError::Net(e.without_url().to_string()))?.to_vec())
}

pub fn segment_dir(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_owned();
    s.push(".hls");
    PathBuf::from(s)
}

fn decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>, DlError> {
    Aes128CbcDec::new(key.into(), iv.into()).decrypt_padded_vec_mut::<Pkcs7>(data).map_err(|_| DlError::Other("分片解密失败，密钥可能不正确".into()))
}

/// 获取媒体播放列表：如果是主播放列表，选码率最高的子流。
pub async fn load_playlist(client: &reqwest::Client, req: &HttpRequest) -> Result<(MediaPlaylist, String), DlError> {
    let text = String::from_utf8_lossy(&fetch_bytes(client, req, &req.url).await?).to_string();
    if is_master(&text) {
        let best = parse_master(&text, &req.url).into_iter().max_by_key(|v| v.bandwidth).ok_or_else(|| DlError::Other("主播放列表里没有子流".into()))?;
        let media = String::from_utf8_lossy(&fetch_bytes(client, req, &best.url).await?).to_string();
        Ok((parse_media(&media, &best.url)?, best.url))
    } else {
        Ok((parse_media(&text, &req.url)?, req.url.clone()))
    }
}

pub async fn download(
    client: &reqwest::Client,
    req: &HttpRequest,
    part: &Path,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HlsOptions,
    mut on_progress: impl FnMut(u64, Option<u64>) + Send,
) -> Result<HlsResult, DlError> {
    let (mut pl, _) = tokio::select! {
        r = load_playlist(client, req) => r?,
        c = wait_ctrl(ctrl) => return Err(DlError::from_ctrl(c)),
    };
    if !pl.end_list {
        return Err(DlError::Other("这是直播流，请使用“直播录制”功能".into()));
    }
    if pl.segments.iter().any(|s| s.key.as_ref().is_some_and(|k| k.method != "AES-128" || k.keyformat.as_deref().is_some_and(|f| f != "identity"))) {
        return Err(DlError::Encrypted("内容使用 DRM 或 SAMPLE-AES 加密，不支持下载".into()));
    }
    let skipped_ads = if opts.skip_ads { filter_ads(&mut pl) } else { 0 };

    let dir = segment_dir(part);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| DlError::Io(format!("无法创建临时目录：{e}")))?;

    // 密钥：同一个 URI 只取一次，并保存到临时目录，续传时链接过期也能继续解密
    let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
    for (i, uri) in
        pl.segments.iter().filter_map(|s| s.key.as_ref().and_then(|k| k.uri.clone())).collect::<std::collections::BTreeSet<_>>().into_iter().enumerate()
    {
        let cache = dir.join(format!("key-{i}.bin"));
        let bytes = match std::fs::read(&cache) {
            Ok(b) if b.len() == 16 => b,
            _ => {
                let b = fetch_bytes(client, req, &uri).await?;
                let _ = std::fs::write(&cache, &b);
                b
            }
        };
        let k: [u8; 16] = bytes.try_into().map_err(|_| DlError::Other("m3u8 密钥长度不对".into()))?;
        keys.insert(uri, k);
    }
    let keys = Arc::new(keys);

    let total_n = pl.segments.len();
    let seg_path = |i: usize| dir.join(format!("{i:05}.seg"));
    let done_bytes = Arc::new(AtomicU64::new(0));
    let done_count = Arc::new(AtomicU64::new(0));
    for i in 0..total_n {
        if let Ok(m) = std::fs::metadata(seg_path(i)) {
            done_bytes.fetch_add(m.len(), Ordering::Relaxed);
            done_count.fetch_add(1, Ordering::Relaxed);
        }
    }
    if let Some(dir_parent) = part.parent() {
        // 按平均码率粗估所需空间（拼接时需要再占一份）
        let est = estimate(done_bytes.load(Ordering::Relaxed), done_count.load(Ordering::Relaxed), total_n as u64).unwrap_or(0);
        ensure_space(dir_parent, est.saturating_mul(2), opts.disk_reserve)?;
    }

    let sem = Arc::new(Semaphore::new(opts.concurrency.max(1)));
    let (stop_tx, stop_rx) = watch::channel(super::CTRL_RUN);
    let mut set = tokio::task::JoinSet::new();
    for (i, seg) in pl.segments.iter().enumerate() {
        if seg_path(i).exists() {
            continue;
        }
        let (client, req, seg, keys, sem, out, db, dc, opts) =
            (client.clone(), req.clone(), seg.clone(), keys.clone(), sem.clone(), seg_path(i), done_bytes.clone(), done_count.clone(), opts.clone());
        let mut stop = stop_rx.clone();
        set.spawn(async move {
            let _permit = tokio::select! {
                p = sem.acquire_owned() => p.map_err(|_| DlError::Canceled)?,
                c = wait_ctrl(&mut stop) => return Err(DlError::from_ctrl(c)),
            };
            let mut attempt = 0;
            let data = loop {
                let r = tokio::select! {
                    r = fetch_bytes(&client, &req, &seg.url) => r,
                    c = wait_ctrl(&mut stop) => return Err(DlError::from_ctrl(c)),
                };
                match r {
                    Ok(d) => break d,
                    Err(DlError::Net(_)) if attempt < 2 => {
                        attempt += 1;
                        tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
                    }
                    Err(e) => return Err(e),
                }
            };
            let data = match &seg.key {
                Some(k) => {
                    let key = k.uri.as_ref().and_then(|u| keys.get(u)).ok_or_else(|| DlError::Other("缺少分片密钥".into()))?;
                    let iv = k.iv.unwrap_or_else(|| {
                        let mut iv = [0u8; 16];
                        iv[8..].copy_from_slice(&seg.seq.to_be_bytes());
                        iv
                    });
                    decrypt(&data, key, &iv)?
                }
                None => data,
            };
            if let Some(net) = &opts.net {
                net.throttle(data.len(), opts.speed_limit_kbps).await;
            }
            let tmp = out.with_extension("tmp");
            tokio::fs::write(&tmp, &data).await.map_err(|e| DlError::Io(format!("写入分片失败：{e}")))?;
            tokio::fs::rename(&tmp, &out).await.map_err(|e| DlError::Io(e.to_string()))?;
            db.fetch_add(data.len() as u64, Ordering::Relaxed);
            dc.fetch_add(1, Ordering::Relaxed);
            Ok(())
        });
    }

    let report = |on_progress: &mut dyn FnMut(u64, Option<u64>)| {
        let b = done_bytes.load(Ordering::Relaxed);
        let c = done_count.load(Ordering::Relaxed);
        on_progress(b, estimate(b, c, total_n as u64));
    };
    let mut result: Result<(), DlError> = Ok(());
    let mut tick = tokio::time::interval(Duration::from_millis(300));
    loop {
        tokio::select! {
            j = set.join_next() => match j {
                None => break,
                Some(Ok(Ok(()))) => {}
                Some(Ok(Err(e))) => if result.is_ok() { result = Err(e); let _ = stop_tx.send(super::CTRL_CANCEL); },
                Some(Err(e)) => if result.is_ok() { result = Err(DlError::Other(format!("分片任务异常：{e}"))); let _ = stop_tx.send(super::CTRL_CANCEL); },
            },
            _ = tick.tick() => report(&mut on_progress),
            c = wait_ctrl(ctrl), if result.is_ok() => {
                result = Err(DlError::from_ctrl(c));
                let _ = stop_tx.send(c);
            }
        }
    }
    report(&mut on_progress);
    result?;

    // 按顺序拼接：初始化分片（fMP4）+ 全部分片
    let mut out = tokio::fs::File::create(part).await.map_err(|e| DlError::Io(format!("无法写入文件：{e}")))?;
    let mut size = 0u64;
    if let Some(init) = &pl.init {
        let data = fetch_bytes(client, req, init).await?;
        out.write_all(&data).await.map_err(|e| DlError::Io(e.to_string()))?;
        size += data.len() as u64;
    }
    for i in 0..total_n {
        let data = tokio::fs::read(seg_path(i)).await.map_err(|e| DlError::Io(format!("读取分片失败：{e}")))?;
        out.write_all(&data).await.map_err(|e| DlError::Io(e.to_string()))?;
        size += data.len() as u64;
    }
    out.flush().await.map_err(|e| DlError::Io(e.to_string()))?;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    Ok(HlsResult { size, fmp4: pl.is_fmp4(), skipped_ads })
}

fn estimate(bytes: u64, done: u64, total: u64) -> Option<u64> {
    (done > 0).then(|| bytes / done * total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{CTRL_PAUSE, CTRL_RUN};
    use aes::cipher::BlockEncryptMut;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
    const KEY: [u8; 16] = *b"0123456789abcdef";

    fn plain(i: usize) -> Vec<u8> {
        format!("SEGMENT-{i}-").repeat(200).into_bytes()
    }

    fn encrypted(i: usize, seq: u64) -> Vec<u8> {
        let mut iv = [0u8; 16];
        iv[8..].copy_from_slice(&seq.to_be_bytes());
        Aes128CbcEnc::new(&KEY.into(), &iv.into()).encrypt_padded_vec_mut::<Pkcs7>(&plain(i))
    }

    /// /master.m3u8 → /v/media.m3u8（相对地址、AES-128、媒体序号从 10 开始）；/ads.m3u8 含另一主机的广告段；
    /// /live.m3u8 无 ENDLIST；/drm.m3u8 使用 SAMPLE-AES；`/slow/` 前缀的分片每个延迟 400 毫秒。
    async fn serve() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let port = addr.port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body: Vec<u8> = match path.as_str() {
                        "/master.m3u8" => b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=640x360\nlow/media.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\nv/media.m3u8\n".to_vec(),
                        "/v/media.m3u8" | "/slow/media.m3u8" => {
                            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:10\n#EXT-X-KEY:METHOD=AES-128,URI=\"../key.bin\"\n#EXTINF:4.0,\ns0.ts\n#EXTINF:4.0,\ns1.ts\n#EXTINF:3.5,\ns2.ts\n#EXT-X-ENDLIST\n".to_vec()
                        }
                        "/ads.m3u8" => format!("#EXTM3U\n#EXTINF:4,\nhttp://127.0.0.1:{port}/p/s0.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:4,\nhttp://localhost:{port}/p/s1.ts\n#EXT-X-DISCONTINUITY\n#EXTINF:4,\nhttp://127.0.0.1:{port}/p/s2.ts\n#EXT-X-ENDLIST\n").into_bytes(),
                        "/live.m3u8" => b"#EXTM3U\n#EXTINF:4,\np/s0.ts\n".to_vec(),
                        "/drm.m3u8" => b"#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\",KEYFORMAT=\"com.apple.streamingkeys\"\n#EXTINF:4,\np/s0.ts\n#EXT-X-ENDLIST\n".to_vec(),
                        "/key.bin" => KEY.to_vec(),
                        p if p.starts_with("/p/s") => plain(p[4..5].parse().unwrap()),
                        p if p.starts_with("/v/s") || p.starts_with("/slow/s") => {
                            if p.starts_with("/slow/") {
                                tokio::time::sleep(Duration::from_millis(400)).await;
                            }
                            let i: usize = p.trim_start_matches("/v/s").trim_start_matches("/slow/s")[..1].parse().unwrap();
                            encrypted(i, 10 + i as u64)
                        }
                        _ => {
                            let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                            return;
                        }
                    };
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://127.0.0.1:{port}")
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-hls-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("out.ts.part")
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    async fn run(url: &str, part: &Path, skip_ads: bool) -> Result<HlsResult, DlError> {
        let (_tx, mut rx) = watch::channel(CTRL_RUN);
        let opts = HlsOptions { concurrency: 3, skip_ads, ..Default::default() };
        download(&client(), &HttpRequest::new(url), part, &mut rx, &opts, |_, _| {}).await
    }

    #[test]
    fn parses_attributes_and_iv() {
        let a = attrs(r#"METHOD=AES-128,URI="https://k/key?a=1,b=2",IV=0x000102030405060708090a0b0c0d0e0f"#);
        assert_eq!(a["URI"], "https://k/key?a=1,b=2");
        assert_eq!(parse_iv(&a["IV"]).unwrap()[15], 15);
    }

    #[test]
    fn parses_master_and_media() {
        let v = parse_master("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=1280x720\nhd/index.m3u8\n", "https://cdn.x/path/master.m3u8");
        assert_eq!(v[0].url, "https://cdn.x/path/hd/index.m3u8");
        assert_eq!(v[0].resolution, Some((1280, 720)));
        let m = parse_media("#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2.0,\na.m4s\n#EXTINF:2.5,\nb.m4s\n#EXT-X-ENDLIST\n", "https://cdn.x/p/index.m3u8")
            .unwrap();
        assert!(m.end_list && m.is_fmp4());
        assert_eq!(m.init.as_deref(), Some("https://cdn.x/p/init.mp4"));
        assert!((m.duration() - 4.5).abs() < 1e-9);
        assert!(parse_media("<html>", "https://x/").is_err());
    }

    #[tokio::test]
    async fn downloads_and_decrypts_best_variant() {
        let base = serve().await;
        let part = tmp("aes");
        let r = run(&format!("{base}/master.m3u8"), &part, false).await.unwrap();
        let expected: Vec<u8> = (0..3).flat_map(plain).collect();
        assert_eq!(std::fs::read(&part).unwrap(), expected);
        assert_eq!(r.size, expected.len() as u64);
        assert!(!r.fmp4);
        assert!(!segment_dir(&part).exists(), "segment directory is removed after success");
    }

    #[tokio::test]
    async fn skips_ad_segments_from_other_hosts() {
        let base = serve().await;
        let part = tmp("ads");
        let r = run(&format!("{base}/ads.m3u8"), &part, true).await.unwrap();
        assert_eq!(r.skipped_ads, 1);
        let expected: Vec<u8> = [0, 2].into_iter().flat_map(plain).collect();
        assert_eq!(std::fs::read(&part).unwrap(), expected);
    }

    #[tokio::test]
    async fn rejects_live_and_drm() {
        let base = serve().await;
        assert!(matches!(run(&format!("{base}/live.m3u8"), &tmp("live"), false).await, Err(DlError::Other(m)) if m.contains("直播")));
        let e = run(&format!("{base}/drm.m3u8"), &tmp("drm"), false).await.err().unwrap();
        assert_eq!(e.kind(), crate::error::ErrorKind::Encrypted);
    }

    #[tokio::test]
    async fn resumes_with_completed_segments_after_pause() {
        let base = serve().await;
        let part = tmp("resume");
        let (tx, mut rx) = watch::channel(CTRL_RUN);
        let url = format!("{base}/slow/media.m3u8");
        let p2 = part.clone();
        let h = tokio::spawn(async move {
            let opts = HlsOptions { concurrency: 1, ..Default::default() };
            download(&client(), &HttpRequest::new(url), &p2, &mut rx, &opts, |_, _| {}).await
        });
        // 等第一个分片落盘后再暂停（固定等待时间在繁忙的 CI 机器上可能一个分片都没下完）
        let seg_dir = segment_dir(&part);
        let have_seg = || std::fs::read_dir(&seg_dir).map(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "seg"))).unwrap_or(false);
        let started = std::time::Instant::now();
        while !have_seg() && started.elapsed() < Duration::from_secs(20) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tx.send(CTRL_PAUSE).unwrap();
        assert!(matches!(h.await.unwrap(), Err(DlError::Paused)));
        let kept = std::fs::read_dir(segment_dir(&part)).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "seg")).count();
        assert!((1..3).contains(&kept), "some segments kept: {kept}");
        // 继续：只下载缺少的分片
        let r = run(&format!("{base}/slow/media.m3u8"), &part, false).await.unwrap();
        let expected: Vec<u8> = (0..3).flat_map(plain).collect();
        assert_eq!(r.size, expected.len() as u64);
        assert_eq!(std::fs::read(&part).unwrap(), expected);
    }
}
