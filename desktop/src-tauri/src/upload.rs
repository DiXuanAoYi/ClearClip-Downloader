//! 自动上传：下载完成后把文件上传到 WebDAV（Nextcloud、坚果云、Alist 等），或复制到另一个文件夹（NAS 挂载盘等）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream;
use tokio::io::AsyncReadExt;
use url::Url;

use crate::db::LibraryItem;
use crate::error::{AppError, AppResult, ErrorKind};
use crate::media_tools::JobCtx;
use crate::settings::UploadSettings;
use crate::AppState;

pub const SECRET: &str = "upload.webdav";

/// 目标子目录（相对路径各级）。
pub fn remote_segments(template: &str, item: &LibraryItem) -> Vec<String> {
    crate::library::render_dir(template, item).iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

/// WebDAV 上某个路径的地址（逐级编码）。
pub fn dav_url(base: &str, segments: &[String]) -> AppResult<Url> {
    let mut u = Url::parse(base).map_err(|_| AppError::invalid("WebDAV 地址不正确，应以 https:// 开头。"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(AppError::invalid("WebDAV 地址应以 http:// 或 https:// 开头。"));
    }
    {
        let mut p = u.path_segments_mut().map_err(|_| AppError::invalid("WebDAV 地址不正确。"))?;
        p.pop_if_empty();
        for s in segments.iter().filter(|s| !s.is_empty()) {
            p.push(s);
        }
    }
    Ok(u)
}

async fn mkcol(client: &reqwest::Client, url: &Url, user: &str, pass: &str) -> AppResult<()> {
    let mut req = client.request(reqwest::Method::from_bytes(b"MKCOL").unwrap(), url.clone()).timeout(Duration::from_secs(30));
    if !user.is_empty() {
        req = req.basic_auth(user, Some(pass));
    }
    let status = req.send().await.map_err(|e| AppError::new(ErrorKind::Network, format!("无法连接 WebDAV：{e}")))?.status().as_u16();
    match status {
        // 201 创建成功；405 已存在；301/302 有的服务器对已存在目录的重定向
        200 | 201 | 204 | 301 | 302 | 405 => Ok(()),
        401 | 403 => Err(AppError::new(ErrorKind::NeedLogin, format!("WebDAV 拒绝了访问（{status}），请检查用户名和密码。"))),
        409 => Err(AppError::msg("WebDAV 上父目录不存在，无法创建文件夹。")),
        s => Err(AppError::msg(format!("创建 WebDAV 文件夹失败（{s}）。"))),
    }
}

/// WebDAV 服务器和凭据。
pub struct Dav<'a> {
    pub base: &'a str,
    pub user: &'a str,
    pub pass: &'a str,
}

/// 上传一个文件（先逐级创建文件夹）。`sent` 记录已发送的字节数，用来显示进度。
pub async fn webdav_put(client: &reqwest::Client, dav: &Dav<'_>, dirs: &[String], name: &str, file: &Path, sent: Arc<AtomicU64>) -> AppResult<String> {
    for i in 1..=dirs.len() {
        mkcol(client, &dav_url(dav.base, &dirs[..i])?, dav.user, dav.pass).await?;
    }
    let mut all = dirs.to_vec();
    all.push(name.to_string());
    let url = dav_url(dav.base, &all)?;
    let f = tokio::fs::File::open(file).await?;
    let len = f.metadata().await?.len();
    let body = reqwest::Body::wrap_stream(stream::unfold((f, sent.clone()), |(mut f, sent)| async move {
        let mut buf = vec![0u8; 256 * 1024];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                sent.fetch_add(n as u64, Ordering::Relaxed);
                Some((Ok::<_, std::io::Error>(buf), (f, sent)))
            }
            Err(e) => Some((Err(e), (f, sent))),
        }
    }));
    let mut req = client.put(url.clone()).header("Content-Length", len).timeout(Duration::from_secs(6 * 3600)).body(body);
    if !dav.user.is_empty() {
        req = req.basic_auth(dav.user, Some(dav.pass));
    }
    let resp = req.send().await.map_err(|e| AppError::new(ErrorKind::Network, format!("上传失败：{e}")))?;
    let status = resp.status().as_u16();
    match status {
        200 | 201 | 204 => Ok(url.to_string()),
        401 | 403 => Err(AppError::new(ErrorKind::NeedLogin, format!("WebDAV 拒绝了访问（{status}），请检查用户名和密码。"))),
        507 => Err(AppError::new(ErrorKind::Disk, "WebDAV 空间不足。")),
        s => Err(AppError::msg(format!("上传失败（{s}）。"))),
    }
}

/// 复制到另一个文件夹（目标已有同名文件时加序号）。返回目标路径。
pub fn folder_copy(root: &Path, dirs: &[String], name: &str, src: &Path) -> AppResult<PathBuf> {
    if root.as_os_str().is_empty() {
        return Err(AppError::invalid("请先填写目标文件夹。"));
    }
    let mut dir = root.to_path_buf();
    dir.extend(dirs.iter().filter(|s| !s.is_empty()));
    std::fs::create_dir_all(&dir).map_err(|e| AppError::new(ErrorKind::Disk, format!("无法创建目标文件夹：{e}")))?;
    let dest = crate::naming::unique_path(dir.join(name), &|p: &Path| p.exists());
    std::fs::copy(src, &dest).map_err(|e| AppError::new(ErrorKind::Disk, format!("复制失败：{e}")))?;
    Ok(dest)
}

/// 这个记录是否需要上传。
pub fn wanted(s: &UploadSettings, item: &LibraryItem) -> bool {
    s.enabled && !s.url.is_empty() && s.kinds.contains(&item.kind) && item.exists && item.source != "ai"
}

/// 上传（或复制）一个媒体库里的文件。成功后按设置删除本地文件，返回目标位置。
pub async fn run_upload(ctx: &JobCtx, item_id: i64) -> AppResult<PathBuf> {
    let st = &ctx.st;
    let settings = st.settings();
    let u = settings.upload.clone();
    let item = st.db.library_item(item_id)?.ok_or_else(|| AppError::not_found("记录不存在"))?;
    let src = PathBuf::from(&item.path);
    if !src.is_file() {
        return Err(AppError::not_found("文件已不存在"));
    }
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let dirs = remote_segments(&u.remote_dir, &item);
    let total = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0).max(1);
    let target = if u.kind == "folder" {
        ctx.note("正在复制…");
        let (s2, d2, n2, root) = (src.clone(), dirs.clone(), name.clone(), PathBuf::from(&u.url));
        tokio::task::spawn_blocking(move || folder_copy(&root, &d2, &n2, &s2)).await.map_err(|e| AppError::msg(e.to_string()))??
    } else {
        ctx.note("正在上传…");
        let pass = st.vault.get(SECRET).unwrap_or_default();
        let client = st.net.clients_for(&settings.network, &u.url)?.download;
        let sent = Arc::new(AtomicU64::new(0));
        let watcher = {
            let (sent, ctx) = (sent.clone(), ctx.clone());
            tokio::spawn(async move {
                loop {
                    ctx.percent(sent.load(Ordering::Relaxed) as f64 / total as f64 * 100.0);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            })
        };
        let dav = Dav { base: &u.url, user: &u.user, pass: &pass };
        let upload = webdav_put(&client, &dav, &dirs, &name, &src, sent);
        let cancelled = async {
            loop {
                if ctx.canceled() {
                    return;
                }
                ctx.wake.notified().await;
            }
        };
        let res = tokio::select! { r = upload => r, _ = cancelled => Err(AppError::msg("canceled")) };
        watcher.abort();
        PathBuf::from(res?)
    };
    if u.delete_after {
        // 走媒体库的删除（开启回收站时先放进回收站）
        crate::library_cmds::delete_items(st, &[item_id], true);
    }
    Ok(target)
}

/// 测试连接：WebDAV 用 PROPFIND 访问根地址；文件夹检查能否创建。
pub async fn test(st: &AppState) -> AppResult<String> {
    let settings = st.settings();
    let u = settings.upload;
    if u.url.is_empty() {
        return Err(AppError::invalid("请先填写地址。"));
    }
    if u.kind == "folder" {
        let root = PathBuf::from(&u.url);
        std::fs::create_dir_all(&root).map_err(|e| AppError::new(ErrorKind::Disk, format!("无法创建这个文件夹：{e}")))?;
        let probe = root.join(".clearclip-write-test");
        std::fs::write(&probe, b"ok").map_err(|e| AppError::new(ErrorKind::Disk, format!("这个文件夹不能写入：{e}")))?;
        let _ = std::fs::remove_file(probe);
        return Ok("文件夹可以写入".into());
    }
    let url = dav_url(&u.url, &[])?;
    let pass = st.vault.get(SECRET).unwrap_or_default();
    let client = st.net.clients_for(&settings.network, &u.url)?.api;
    let mut req = client.request(reqwest::Method::from_bytes(b"PROPFIND").unwrap(), url).header("Depth", "0").timeout(Duration::from_secs(20));
    if !u.user.is_empty() {
        req = req.basic_auth(&u.user, Some(&pass));
    }
    let status = req.send().await.map_err(|e| AppError::new(ErrorKind::Network, format!("无法连接 WebDAV：{e}")))?.status().as_u16();
    match status {
        200 | 207 => Ok("WebDAV 连接正常".into()),
        401 | 403 => Err(AppError::new(ErrorKind::NeedLogin, format!("WebDAV 拒绝了访问（{status}），请检查用户名和密码。"))),
        404 => Err(AppError::not_found("WebDAV 地址不存在（404），请检查路径。")),
        s => Err(AppError::msg(format!("WebDAV 返回 {s}。"))),
    }
}

/// 下载完成后触发（设置里开启时）。
pub fn auto_upload(app: &tauri::AppHandle, st: &Arc<AppState>, item: &LibraryItem) {
    let settings = st.settings();
    if settings.metered_mode || !wanted(&settings.upload, item) {
        return;
    }
    let (id, title) = (item.id, item.title.clone());
    crate::media_tools::spawn_job(app, title, "上传", move |ctx| async move { run_upload(&ctx, id).await });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::AsyncWriteExt;

    fn item() -> LibraryItem {
        LibraryItem {
            id: 1,
            platform: "douyin".into(),
            media_id: "1".into(),
            asset_id: "video".into(),
            title: "标题".into(),
            author: "作者/甲".into(),
            cover: None,
            path: "/x/a.mp4".into(),
            size: 1,
            finished_at: 1_700_000_000,
            exists: true,
            kind: "video".into(),
            source: "manual".into(),
            source_url: String::new(),
            platform_name: "抖音".into(),
            cover_path: None,
            favorite: false,
            rating: 0,
            note: String::new(),
            tags: vec![],
            duration_ms: None,
        }
    }

    #[test]
    fn urls_are_built_segment_by_segment() {
        let segs = remote_segments("ClearClip/{platform}/{author}", &item());
        assert_eq!(segs, vec!["ClearClip", "抖音", "作者_甲"], "slashes inside names never create extra levels");
        let u = dav_url("https://dav.example.com/remote.php/dav/files/me/", &segs).unwrap();
        assert_eq!(u.as_str(), "https://dav.example.com/remote.php/dav/files/me/ClearClip/%E6%8A%96%E9%9F%B3/%E4%BD%9C%E8%80%85_%E7%94%B2");
        assert!(dav_url("ftp://x", &[]).is_err() && dav_url("nonsense", &[]).is_err());
        assert_eq!(dav_url("https://h.com/dav", &["a b".into(), "".into()]).unwrap().as_str(), "https://h.com/dav/a%20b");
    }

    #[test]
    fn wanted_respects_settings() {
        let mut s = UploadSettings { enabled: true, url: "https://x".into(), ..Default::default() };
        assert!(wanted(&s, &item()));
        assert!(!wanted(&UploadSettings { enabled: false, ..s.clone() }, &item()));
        assert!(!wanted(&UploadSettings { url: String::new(), ..s.clone() }, &item()));
        let mut sub = item();
        sub.kind = "subtitle".into();
        assert!(!wanted(&s, &sub), "subtitles are not in the default kinds");
        s.kinds.push("subtitle".into());
        assert!(wanted(&s, &sub));
        let mut ai = item();
        ai.source = "ai".into();
        assert!(!wanted(&s, &ai));
        let mut gone = item();
        gone.exists = false;
        assert!(!wanted(&s, &gone));
    }

    #[test]
    fn folder_copy_makes_directories_and_avoids_overwrites() {
        let dir = std::env::temp_dir().join(format!("clearclip-up-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.mp4");
        std::fs::write(&src, b"hello").unwrap();
        let root = dir.join("nas");
        let d1 = folder_copy(&root, &["ClearClip".into(), "抖音".into()], "a.mp4", &src).unwrap();
        assert_eq!(std::fs::read(&d1).unwrap(), b"hello");
        assert!(d1.ends_with("nas/ClearClip/抖音/a.mp4"));
        let d2 = folder_copy(&root, &["ClearClip".into(), "抖音".into()], "a.mp4", &src).unwrap();
        assert_ne!(d1, d2, "an existing file is never overwritten");
        assert!(folder_copy(Path::new(""), &[], "a", &src).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    type Log = Arc<Mutex<Vec<(String, String, Option<String>, Vec<u8>)>>>;

    /// 极简 WebDAV：记录请求（方法、路径、认证头、请求体），MKCOL 返回 201（已存在的目录 405），PUT 返回 201。
    async fn dav_server(existing: &'static [&'static str]) -> (String, Log) {
        let log: Log = Arc::new(Mutex::new(vec![]));
        let l2 = log.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let log = l2.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = vec![];
                    let mut tmp = [0u8; 65536];
                    let (head_end, cl) = loop {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..p]).to_ascii_lowercase();
                            let cl = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok());
                            break (p + 4, cl);
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
                    let cl = if head.starts_with("PUT") {
                        cl.unwrap_or_else(|| panic!("the upload must send Content-Length, not chunked: {head}"))
                    } else {
                        cl.unwrap_or(0)
                    };
                    while buf.len() < head_end + cl {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let mut parts = head.split_whitespace();
                    let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
                    let auth = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("authorization:").map(|v| v.trim().to_string()));
                    let status = match method.as_str() {
                        "MKCOL" if existing.contains(&path.as_str()) => 405,
                        "MKCOL" | "PUT" => 201,
                        _ => 400,
                    };
                    log.lock().unwrap().push((method, path, auth, buf[head_end..head_end + cl].to_vec()));
                    let _ = sock.write_all(format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/dav/"), log)
    }

    #[tokio::test]
    async fn webdav_upload_creates_folders_then_puts_the_file() {
        let dir = std::env::temp_dir().join(format!("clearclip-dav-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("视频.mp4");
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 253) as u8).collect();
        std::fs::write(&file, &data).unwrap();
        let (base, log) = dav_server(&["/dav/ClearClip"]).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let sent = Arc::new(AtomicU64::new(0));
        let url = webdav_put(&client, &Dav { base: &base, user: "me", pass: "p@ss" }, &["ClearClip".into(), "抖音".into()], "视频.mp4", &file, sent.clone())
            .await
            .unwrap();
        assert!(url.ends_with("/dav/ClearClip/%E6%8A%96%E9%9F%B3/%E8%A7%86%E9%A2%91.mp4"), "{url}");
        let log = log.lock().unwrap();
        let calls: Vec<(&str, &str)> = log.iter().map(|(m, p, _, _)| (m.as_str(), p.as_str())).collect();
        assert_eq!(calls[0], ("MKCOL", "/dav/ClearClip"));
        assert_eq!(calls[1].0, "MKCOL");
        assert_eq!(calls[2].0, "PUT");
        assert_eq!(log[2].3, data, "the whole file arrives intact");
        assert!(log[2].2.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("basic bWU6cEBzcw==")), "basic auth me:p@ss: {:?}", log[2].2);
        assert_eq!(sent.load(Ordering::Relaxed), data.len() as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn webdav_errors_are_explained() {
        let dir = std::env::temp_dir().join(format!("clearclip-dav2-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.bin");
        std::fs::write(&file, b"x").unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let e = webdav_put(&client, &Dav { base: "http://127.0.0.1:1/dav/", user: "", pass: "" }, &[], "a.bin", &file, Arc::new(AtomicU64::new(0)))
            .await
            .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Network);
        // 401
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut b = [0u8; 4096];
                    let _ = tokio::io::AsyncReadExt::read(&mut s, &mut b).await;
                    let _ = s.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                });
            }
        });
        let e = webdav_put(
            &client,
            &Dav { base: &format!("http://{addr}/dav/"), user: "u", pass: "bad" },
            &["d".into()],
            "a.bin",
            &file,
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .unwrap_err();
        assert_eq!(e.kind, ErrorKind::NeedLogin);
        assert!(e.message.contains("用户名和密码"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
