//! BT / 磁力下载，基于 librqbit（DHT、PEX、UPnP、uTP）。
//! 每个任务下载种子里的一个文件：多文件种子在解析时展开成合集，逐个文件加入队列。
//! 不内置搜索；下载完成后不做种（任务结束即从会话中移除）。

pub mod trackers;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::limits::LimitsConfig;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode, ListenerOptions, Session, SessionOptions};
use tokio::sync::watch;

use crate::engine::{ensure_space, wait_ctrl, DlError};
use crate::error::ErrorKind;
use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind, PlaylistEntry, Protocol};

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static SESSION: tokio::sync::OnceCell<Arc<Session>> = tokio::sync::OnceCell::const_new();
static INFOS: Mutex<Option<HashMap<String, Arc<Info>>>> = Mutex::new(None);
static LOCKS: Mutex<Option<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = Mutex::new(None);

/// 解析磁力链接（要在 DHT 里找到种子信息）最多等多久
const METADATA_TIMEOUT: Duration = Duration::from_secs(90);
const INFO_CACHE_MAX: usize = 64;

pub fn init(data_dir: PathBuf) {
    let _ = DATA_DIR.set(data_dir);
}

fn data_dir() -> PathBuf {
    DATA_DIR.get().cloned().unwrap_or_else(std::env::temp_dir)
}

/// 种子里的一个文件
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub len: u64,
}

/// 种子的信息（文件列表、原始 .torrent 内容），解析后缓存，下载时直接使用。
#[derive(Debug, Clone)]
pub struct Info {
    pub hash: String,
    pub name: String,
    pub files: Vec<FileEntry>,
    pub torrent: Vec<u8>,
}

fn is_padding(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("_____padding_file"))
}

/// librqbit 里的 reqwest 不带 TLS 提供者：安装 ring 作为进程默认值（只需一次）。
fn install_tls() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

async fn session() -> Result<Arc<Session>, String> {
    SESSION
        .get_or_try_init(|| async {
            install_tls();
            let dir = data_dir();
            let _ = std::fs::create_dir_all(dir.join("bt"));
            let opts = |ipv4_only: bool| SessionOptions {
                dht: Some(DhtSessionConfig {
                    persistence: Some(DhtPersistenceConfig { config_filename: Some(dir.join("bt_dht.json")), ..Default::default() }),
                    ..Default::default()
                }),
                listen: Some(ListenerOptions { mode: ListenerMode::TcpAndUtp, enable_upnp_port_forwarding: true, ipv4_only, ..Default::default() }),
                ipv4_only,
                ..Default::default()
            };
            // 系统没有 IPv6 时（或被禁用）双栈套接字创建不了：退回只用 IPv4
            match Session::new_with_opts(dir.join("bt"), opts(false)).await {
                Ok(s) => Ok(s),
                Err(first) => {
                    log::warn!("BT session (dual-stack) failed: {first:#}; retrying IPv4 only");
                    Session::new_with_opts(dir.join("bt"), opts(true)).await.map_err(|e| format!("无法启动 BT 引擎：{e:#}。如果系统禁用了 IPv6，请先启用它。"))
                }
            }
        })
        .await
        .cloned()
}

// ---------- 链接 ----------

/// 链接里指定的文件序号：磁力的 `so=N`，或 .torrent 地址的 `#bt-file=N`。
pub fn select_of(link: &str) -> Option<usize> {
    if link.starts_with("magnet:") {
        let q = link.split_once('?')?.1;
        return q.split('&').find_map(|kv| kv.strip_prefix("so=")).and_then(|v| v.split(',').next()?.parse().ok());
    }
    link.split_once('#')?.1.split('&').find_map(|kv| kv.strip_prefix("bt-file=")).and_then(|v| v.parse().ok())
}

/// 去掉文件选择部分的链接（缓存用的键）。
fn strip_select(link: &str) -> String {
    if link.starts_with("magnet:") {
        let Some((head, q)) = link.split_once('?') else { return link.to_string() };
        let kept: Vec<&str> = q.split('&').filter(|kv| !kv.starts_with("so=")).collect();
        return format!("{head}?{}", kept.join("&"));
    }
    link.split('#').next().unwrap_or(link).to_string()
}

pub fn link_for_file(link: &str, idx: usize) -> String {
    let base = strip_select(link);
    if base.starts_with("magnet:") {
        format!("{base}&so={idx}")
    } else {
        format!("{base}#bt-file={idx}")
    }
}

/// 给磁力链接补上 tracker（已有的不重复）。
fn magnet_with_trackers(link: &str, extra: &[String]) -> String {
    let mut out = strip_select(link);
    for t in extra {
        if !out.contains(&format!("tr={}", urlencoding_encode(t))) {
            out.push_str(&format!("&tr={}", urlencoding_encode(t)));
        }
    }
    out
}

fn urlencoding_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// 是不是 .torrent 文件的网址
pub fn is_torrent_url(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.path().to_ascii_lowercase().ends_with(".torrent"))
}

// ---------- 种子信息 ----------

fn cache_get(key: &str) -> Option<Arc<Info>> {
    INFOS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(key).cloned()
}

fn cache_put(key: String, info: Arc<Info>) {
    let mut g = INFOS.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    if map.len() >= INFO_CACHE_MAX {
        map.clear();
    }
    map.insert(key, info);
}

fn info_from_list_only(r: &librqbit::ListOnlyResponse) -> Info {
    let files: Vec<FileEntry> = r.info.iter_file_details().map(|fd| FileEntry { path: fd.filename.to_pathbuf(), len: fd.len }).collect();
    Info { hash: r.info_hash.as_string(), name: r.info.name().map(|n| n.into_owned()).unwrap_or_default(), files, torrent: r.torrent_bytes.to_vec() }
}

/// 取得种子信息：磁力链接从 DHT / tracker 获取，.torrent 地址先下载再解析。
pub async fn load_info(client: &reqwest::Client, link: &str, extra_trackers: &[String]) -> Result<Arc<Info>, String> {
    let key = strip_select(link);
    if let Some(i) = cache_get(&key) {
        return Ok(i);
    }
    let session = session().await?;
    let opts = AddTorrentOptions { list_only: true, ..Default::default() };
    let add = if link.starts_with("magnet:") {
        AddTorrent::from_url(magnet_with_trackers(link, extra_trackers))
    } else {
        let url = strip_select(link);
        let r = client.get(&url).timeout(Duration::from_secs(30)).send().await.map_err(|e| format!("无法下载种子文件：{e}"))?;
        if !r.status().is_success() {
            return Err(format!("下载种子文件失败：服务器返回 {}", r.status().as_u16()));
        }
        let bytes = r.bytes().await.map_err(|e| format!("下载种子文件中断：{e}"))?;
        if bytes.len() > 20 * 1024 * 1024 {
            return Err("种子文件太大".into());
        }
        AddTorrent::from_bytes(bytes)
    };
    let res = tokio::time::timeout(METADATA_TIMEOUT, session.add_torrent(add, Some(opts)))
        .await
        .map_err(|_| "长时间没有获取到种子信息（找不到可用的 BT 节点）。可以稍后重试，或在设置里更新 tracker 列表。".to_string())?
        .map_err(|e| format!("无法解析种子：{e:#}"))?;
    let AddTorrentResponse::ListOnly(r) = res else { return Err("无法读取种子信息".into()) };
    let info = Arc::new(info_from_list_only(&r));
    cache_put(key, info.clone());
    Ok(info)
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).filter(|e| !e.is_empty() && e.len() <= 8).unwrap_or_else(|| "bin".into())
}

fn stem_of(p: &Path) -> String {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or("file").to_string()
}

/// 解析磁力 / .torrent 链接成可下载的内容。
pub async fn resolve(client: &reqwest::Client, settings: &crate::settings::Settings, link: &str) -> AppResult<MediaInfo> {
    let data_dir = data_dir();
    trackers::refresh_if_due(client, &data_dir, settings).await;
    let extra = trackers::effective(&data_dir, settings);
    let info = load_info(client, link, &extra).await.map_err(|e| AppError::new(ErrorKind::Network, e))?;
    let files: Vec<(usize, &FileEntry)> = info.files.iter().enumerate().filter(|(_, f)| !is_padding(&f.path)).collect();
    if files.is_empty() {
        return Err(AppError::invalid("这个种子里没有文件。"));
    }
    let selected = select_of(link);
    let single = selected.and_then(|i| files.iter().find(|(k, _)| *k == i)).or(if files.len() == 1 { files.first() } else { None });
    let id = info.hash.chars().take(12).collect::<String>();
    let source = strip_select(link);
    let mut media = MediaInfo {
        platform: "bt".into(),
        platform_name: "BT / 磁力".into(),
        id,
        source_url: link.to_string(),
        title: String::new(),
        author: String::new(),
        cover: None,
        duration_ms: None,
        kind: MediaKind::File,
        width: None,
        height: None,
        published_at: None,
        assets: vec![],
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("bt".into()),
    };
    if let Some((idx, f)) = single {
        let ext = ext_of(&f.path);
        let mut a = Asset::base("file", AssetKind::File, source, format!("文件 {}", ext.to_uppercase()), ext);
        a.protocol = Protocol::Bt;
        a.format_id = Some(idx.to_string());
        a.filesize = Some(f.len);
        media.title = stem_of(&f.path);
        media.assets = vec![a];
        return Ok(media);
    }
    if selected.is_some() {
        return Err(AppError::invalid("种子里没有这个序号的文件。"));
    }
    media.kind = MediaKind::Playlist;
    media.title = if info.name.is_empty() { format!("种子 {}", &info.hash[..8.min(info.hash.len())]) } else { info.name.clone() };
    media.entries = files
        .iter()
        .enumerate()
        .map(|(n, (idx, f))| PlaylistEntry {
            id: idx.to_string(),
            title: f.path.to_string_lossy().replace('\\', "/"),
            url: link_for_file(link, *idx),
            duration_ms: None,
            thumbnail: None,
            index: n as u32 + 1,
        })
        .collect();
    Ok(media)
}

// ---------- 下载 ----------

#[derive(Debug, Clone, Default)]
pub struct DownloadOptions {
    pub trackers: Vec<String>,
    /// 限速（KB/s），0 为不限
    pub speed_limit_kbps: u64,
    pub disk_reserve: u64,
    /// 直接连接的节点（测试和已知来源用）
    pub initial_peers: Vec<SocketAddr>,
}

fn staging_dir(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_owned();
    s.push(".bt");
    PathBuf::from(s)
}

/// 已经下载的字节数（续传时显示进度用）。
pub fn staged_bytes(part: &Path) -> u64 {
    fn walk(d: &Path) -> u64 {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.flatten()
                    .map(|e| match e.metadata() {
                        Ok(m) if m.is_dir() => walk(&e.path()),
                        Ok(m) => m.len(),
                        Err(_) => 0,
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    walk(&staging_dir(part))
}

pub fn remove_staging(part: &Path) {
    let _ = std::fs::remove_dir_all(staging_dir(part));
}

fn lock_for(hash: &str) -> Arc<tokio::sync::Mutex<()>> {
    LOCKS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).entry(hash.to_string()).or_default().clone()
}

fn bps(kbps: u64) -> Option<std::num::NonZeroU32> {
    std::num::NonZeroU32::new(kbps.saturating_mul(1024).min(u32::MAX as u64) as u32)
}

pub async fn download(
    client: &reqwest::Client,
    link: &str,
    file_idx: Option<usize>,
    part: &Path,
    ctrl: &mut watch::Receiver<u8>,
    opts: &DownloadOptions,
    mut on_progress: impl FnMut(u64, u64, u64) + Send,
) -> Result<u64, DlError> {
    let mut outer = ctrl.clone();
    let info = tokio::select! {
        r = load_info(client, link, &opts.trackers) => r.map_err(DlError::Net)?,
        c = wait_ctrl(&mut outer) => return Err(DlError::from_ctrl(c)),
    };
    let idx = file_idx.or_else(|| select_of(link)).or(if info.files.iter().filter(|f| !is_padding(&f.path)).count() == 1 {
        info.files.iter().position(|f| !is_padding(&f.path))
    } else {
        None
    });
    let idx = idx.ok_or_else(|| DlError::Other("这个种子有多个文件，请先在列表里选择要下载的文件。".into()))?;
    let file = info.files.get(idx).ok_or_else(|| DlError::Other("种子里没有这个序号的文件。".into()))?;
    let size = file.len;
    let staging = staging_dir(part);
    if let Some(dir) = part.parent() {
        ensure_space(dir, size.saturating_sub(staged_bytes(part)), opts.disk_reserve)?;
    }
    let session = session().await.map_err(DlError::Other)?;

    // 同一个种子的文件一个接一个下载：它们共用 BT 会话里的同一个任务
    let lock = lock_for(&info.hash);
    let _guard = tokio::select! {
        g = lock.lock() => g,
        c = wait_ctrl(&mut outer) => return Err(DlError::from_ctrl(c)),
    };

    let make_opts = || AddTorrentOptions {
        only_files: Some(vec![idx]),
        overwrite: true,
        output_folder: Some(staging.to_string_lossy().into_owned()),
        trackers: Some(opts.trackers.clone()),
        ratelimits: LimitsConfig { download_bps: bps(opts.speed_limit_kbps), upload_bps: None },
        initial_peers: (!opts.initial_peers.is_empty()).then(|| opts.initial_peers.clone()),
        ..Default::default()
    };
    let add = |s: &Arc<Session>| {
        let s = s.clone();
        let o = make_opts();
        let bytes = info.torrent.clone();
        async move { s.add_torrent(AddTorrent::from_bytes(bytes), Some(o)).await }
    };
    let mut resp = add(&session).await.map_err(|e| DlError::Other(format!("无法开始 BT 下载：{e:#}")))?;
    if let AddTorrentResponse::AlreadyManaged(id, _) = &resp {
        // 上一次留在会话里的同一个种子：移除后按这次的选择重新加入
        let _ = session.delete(TorrentIdOrHash::Id(*id), false).await;
        resp = add(&session).await.map_err(|e| DlError::Other(format!("无法开始 BT 下载：{e:#}")))?;
    }
    let (id, handle) = match resp {
        AddTorrentResponse::Added(id, h) | AddTorrentResponse::AlreadyManaged(id, h) => (id, h),
        AddTorrentResponse::ListOnly(_) => return Err(DlError::Other("无法开始 BT 下载".into())),
    };

    let started = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    let outcome: Result<(), DlError> = loop {
        tokio::select! {
            c = wait_ctrl(ctrl) => break Err(DlError::from_ctrl(c)),
            _ = tick.tick() => {}
        }
        let st = handle.stats();
        if let Some(e) = &st.error {
            break Err(DlError::Net(format!("BT 下载出错：{e}")));
        }
        let got = st.file_progress.get(idx).copied().unwrap_or(0).min(size);
        let speed = st.live.as_ref().map(|l| (l.download_speed.mbps * 1_048_576.0) as u64).unwrap_or(0);
        on_progress(got, size, speed);
        if got >= size || st.finished {
            break Ok(());
        }
        // 长时间一个字节也没有：多半是找不到来源
        if got == 0 && started.elapsed() > Duration::from_secs(1200) {
            break Err(DlError::Net("20 分钟内没有收到任何数据：这个种子可能没有人在做种。".into()));
        }
    };
    // 不论结果如何都要把任务从会话里移除（文件保留，下次续传）
    let _ = session.delete(TorrentIdOrHash::Id(id), false).await;
    outcome?;

    let src = staging.join(&file.path);
    let meta = std::fs::metadata(&src).map_err(|e| DlError::Io(format!("找不到下载好的文件 {}：{e}", src.display())))?;
    if meta.len() != size {
        return Err(DlError::Io(format!("文件大小不对（{} / {} 字节）", meta.len(), size)));
    }
    let _ = std::fs::remove_file(part);
    std::fs::rename(&src, part).or_else(|_| std::fs::copy(&src, part).map(|_| ())).map_err(|e| DlError::Io(format!("无法保存文件：{e}")))?;
    remove_staging(part);
    on_progress(size, size, 0);
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use librqbit::Magnet;

    #[test]
    fn link_selection_roundtrip() {
        let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x&tr=udp%3A%2F%2Ft.example%3A1";
        assert_eq!(select_of(m), None);
        let one = link_for_file(m, 3);
        assert_eq!(select_of(&one), Some(3));
        assert_eq!(strip_select(&one), m);
        assert_eq!(select_of(&link_for_file(&one, 5)), Some(5), "重新选择会替换旧序号");
        let t = "https://example.com/a.torrent?x=1";
        let tf = link_for_file(t, 2);
        assert_eq!(tf, "https://example.com/a.torrent?x=1#bt-file=2");
        assert_eq!(select_of(&tf), Some(2));
        assert_eq!(strip_select(&tf), t);
        assert!(is_torrent_url("https://example.com/a/B.TORRENT?x=1#bt-file=2"));
        assert!(!is_torrent_url("https://example.com/a.mp4"));
        assert!(!is_torrent_url("magnet:?xt=urn:btih:abc"));
    }

    #[test]
    fn trackers_are_appended_once() {
        let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=udp%3A%2F%2Ft.example%3A1";
        let out = magnet_with_trackers(m, &["udp://t.example:1".into(), "https://b.example/announce".into()]);
        assert_eq!(out.matches("tr=").count(), 2);
        assert!(out.contains("tr=https%3A%2F%2Fb.example%2Fannounce"));
        assert!(Magnet::parse(&out).is_ok());
    }

    #[test]
    fn padding_files_are_recognised() {
        assert!(is_padding(Path::new("dir/_____padding_file_0_x")));
        assert!(!is_padding(Path::new("dir/movie.mkv")));
    }

    fn cached(link: &str, name: &str, files: &[(&str, u64)]) {
        let hash = "0123456789abcdef0123456789abcdef01234567".to_string();
        let info = Info { hash, name: name.into(), files: files.iter().map(|(p, l)| FileEntry { path: PathBuf::from(p), len: *l }).collect(), torrent: vec![] };
        cache_put(strip_select(link), Arc::new(info));
    }

    fn offline_settings() -> crate::settings::Settings {
        crate::settings::Settings { bt_tracker_auto_update: false, ..Default::default() }
    }

    #[tokio::test]
    async fn single_file_torrent_resolves_to_one_asset() {
        let link = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&x=single";
        cached(link, "movie.mkv", &[("movie.mkv", 5_000)]);
        let client = reqwest::Client::new();
        let m = resolve(&client, &offline_settings(), link).await.unwrap();
        assert_eq!(m.kind, MediaKind::File);
        assert_eq!(m.title, "movie");
        let a = &m.assets[0];
        assert_eq!((a.protocol, a.ext.as_str(), a.filesize, a.format_id.as_deref()), (Protocol::Bt, "mkv", Some(5_000), Some("0")));
    }

    #[tokio::test]
    async fn multi_file_torrent_becomes_a_playlist_and_entries_resolve_to_files() {
        let link = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&x=multi";
        cached(link, "Season 1", &[("Season 1/e01.mkv", 10), ("Season 1/_____padding_file_0_", 3), ("Season 1/e02.mkv", 20), ("Season 1/readme.txt", 1)]);
        let client = reqwest::Client::new();
        let m = resolve(&client, &offline_settings(), link).await.unwrap();
        assert_eq!(m.kind, MediaKind::Playlist);
        assert_eq!(m.title, "Season 1");
        assert_eq!(m.entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), vec!["Season 1/e01.mkv", "Season 1/e02.mkv", "Season 1/readme.txt"]);
        assert_eq!(m.entries.iter().map(|e| e.index).collect::<Vec<_>>(), vec![1, 2, 3]);
        let e = &m.entries[1];
        assert_eq!(select_of(&e.url), Some(2), "序号是种子里的真实序号，不受填充文件影响");
        let one = resolve(&client, &offline_settings(), &e.url).await.unwrap();
        assert_eq!((one.kind, one.title.as_str(), one.assets[0].format_id.as_deref(), one.assets[0].filesize), (MediaKind::File, "e02", Some("2"), Some(20)));
        assert!(resolve(&client, &offline_settings(), &link_for_file(link, 1)).await.is_err(), "填充文件不能被选中");
        assert!(resolve(&client, &offline_settings(), &link_for_file(link, 99)).await.is_err());
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-bt-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 本机上做种，再用我们的下载流程取回来：覆盖加入种子、选文件、进度、完成后移动文件、清理。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn downloads_one_file_from_a_local_seeder() {
        use librqbit::spawn_utils::BlockingSpawner;
        use librqbit::{CreateTorrentOptions, ListenerOptions, SessionOptions};

        // librqbit 的 UDP tracker 客户端固定绑定 IPv6：内核不支持 IPv6 的环境（部分容器）没法运行
        if std::net::UdpSocket::bind("[::]:0").is_err() {
            eprintln!("skipped: this machine has no IPv6 support");
            return;
        }
        install_tls();
        let seed_dir = tmp("seed");
        let content: Vec<u8> = (0..3_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        std::fs::write(seed_dir.join("data.bin"), &content).unwrap();
        let created = librqbit::create_torrent(
            &seed_dir.join("data.bin"),
            CreateTorrentOptions { name: Some("data.bin"), trackers: vec![], piece_length: Some(65536) },
            &BlockingSpawner::new(2),
        )
        .await
        .unwrap();
        let torrent = created.as_bytes().unwrap().to_vec();
        let hash = created.info_hash().as_string();

        let seeder = Session::new_with_opts(
            seed_dir.clone(),
            SessionOptions {
                dht: None,
                disable_trackers: true,
                disable_local_service_discovery: true,
                ipv4_only: true,
                listen: Some(ListenerOptions {
                    listen_addr: (std::net::Ipv4Addr::UNSPECIFIED, 0).into(),
                    mode: ListenerMode::TcpOnly,
                    ipv4_only: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let r = seeder
            .add_torrent(
                AddTorrent::from_bytes(torrent.clone()),
                Some(AddTorrentOptions { overwrite: true, output_folder: Some(seed_dir.to_string_lossy().into_owned()), ..Default::default() }),
            )
            .await
            .unwrap();
        let handle = r.into_handle().unwrap();
        tokio::time::timeout(Duration::from_secs(30), handle.wait_until_initialized()).await.unwrap().unwrap();
        let port = seeder.announce_port().expect("seeder listen port");

        // 直接放进缓存，避免联网解析
        let link = format!("magnet:?xt=urn:btih:{hash}");
        let info = Arc::new(Info {
            hash: hash.clone(),
            name: "data.bin".into(),
            files: vec![FileEntry { path: PathBuf::from("data.bin"), len: content.len() as u64 }],
            torrent,
        });
        cache_put(strip_select(&link), info);

        let out_dir = tmp("out");
        init(out_dir.clone());
        let part = out_dir.join("data.bin.part");
        let (_tx, mut rx) = watch::channel(crate::engine::CTRL_RUN);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let opts = DownloadOptions { initial_peers: vec![SocketAddr::from(([127, 0, 0, 1], port))], ..Default::default() };
        let mut last = 0u64;
        let n = tokio::time::timeout(
            Duration::from_secs(90),
            download(&client, &link, None, &part, &mut rx, &opts, |got, total, _| {
                assert_eq!(total, 3_000_000);
                assert!(got >= last, "进度不应倒退");
                last = got;
            }),
        )
        .await
        .expect("下载超时")
        .unwrap();
        assert_eq!(n, 3_000_000);
        assert_eq!(std::fs::read(&part).unwrap(), content);
        assert!(!staging_dir(&part).exists(), "完成后应清理临时目录");
    }
}
