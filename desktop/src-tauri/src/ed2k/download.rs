//! eD2k 下载调度：向服务器要来源，同时连几个来源取数据，分块校验，支持续传。

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{watch, Notify};
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::hash::{self, Hash};
use super::link::FileLink;
use super::peer::{self, PeerEnd, Shared, Tuning};
use super::sched::Sched;
use super::server;
use crate::engine::http::HttpOptions;
use crate::engine::{ensure_space, wait_ctrl, DlError};

#[derive(Debug, Clone)]
pub struct Config {
    pub max_peers: usize,
    pub peer: Tuning,
    pub server: server::Timeouts,
    /// 一直没有可用来源时多久后报错
    pub no_source: Duration,
    /// 重新向服务器要来源的间隔
    pub requery: Duration,
    pub retry_base: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_peers: 4,
            peer: Tuning::default(),
            server: server::Timeouts::default(),
            no_source: Duration::from_secs(600),
            requery: Duration::from_secs(300),
            retry_base: Duration::from_secs(30),
        }
    }
}

/// 随机的用户标识（eMule 约定第 5、14 字节为固定值）。
pub fn new_user_hash() -> Hash {
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; 16];
    for chunk in out.chunks_mut(8) {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out[5] = 14;
    out[14] = 111;
    out
}

pub fn sidecar_path(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_owned();
    s.push(".ed2k");
    PathBuf::from(s)
}

struct Saved {
    part_hashes: Vec<Hash>,
    done: Vec<usize>,
}

fn load_sidecar(path: &Path, link: &FileLink) -> Option<Saved> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if v["hash"].as_str()? != hex::encode(link.hash) || v["size"].as_u64()? != link.size {
        return None;
    }
    let part_hashes = v["partHashes"].as_array()?.iter().filter_map(|h| hex::decode(h.as_str()?).ok()?.try_into().ok()).collect();
    let done = v["done"].as_array()?.iter().filter_map(|n| n.as_u64().map(|n| n as usize)).collect();
    Some(Saved { part_hashes, done })
}

struct PeerState {
    running: bool,
    next_try: Instant,
    fails: u32,
    dead: bool,
}

pub async fn download(
    link: &FileLink,
    servers: &[String],
    part: &Path,
    ctrl: &mut watch::Receiver<u8>,
    opts: &HttpOptions,
    cfg: Config,
    mut on_progress: impl FnMut(u64) + Send,
) -> Result<u64, DlError> {
    if link.size == 0 {
        return Err(DlError::Other("ed2k 链接里的文件大小为 0".into()));
    }
    if let Some(dir) = part.parent() {
        ensure_space(dir, link.size.saturating_sub(std::fs::metadata(part).map(|m| m.len()).unwrap_or(0)), opts.disk_reserve)?;
    }
    let sidecar = sidecar_path(part);
    let saved = load_sidecar(&sidecar, link);
    let file_ok = std::fs::metadata(part).map(|m| m.len() == link.size).unwrap_or(false);
    let io = |e: std::io::Error| DlError::Io(format!("无法写入文件 {}：{e}", part.display()));
    let mut sched = Sched::new(link.size);
    let mut part_hashes = link.part_hashes.clone();
    if saved.is_some() && file_ok {
        let s = saved.unwrap();
        if part_hashes.is_empty() {
            part_hashes = s.part_hashes;
        }
        if !part_hashes.is_empty() {
            for p in s.done {
                sched.mark_verified(p);
            }
        }
    }
    // 单分块的文件：分块哈希就是文件哈希
    if part_hashes.is_empty() && hash::part_count(link.size) == 1 {
        part_hashes = vec![link.hash];
    }
    let file = tokio::fs::OpenOptions::new().create(true).write(true).read(true).truncate(false).open(part).await.map_err(io)?;
    if !file_ok {
        file.set_len(link.size).await.map_err(io)?;
    }

    let shared = Arc::new(Shared {
        file_hash: link.hash,
        size: link.size,
        user_hash: new_user_hash(),
        part_hashes: Mutex::new(part_hashes),
        sched: Mutex::new(sched),
        path: part.to_path_buf(),
        file: tokio::sync::Mutex::new(file),
        banned: Mutex::new(HashSet::new()),
        strikes: Mutex::new(HashMap::new()),
        opts: opts.clone(),
        tuning: cfg.peer,
        next_peer: Default::default(),
        wake: Notify::new(),
        sidecar,
        last_error: Mutex::new(String::new()),
    });

    let mut states: HashMap<SocketAddr, PeerState> = HashMap::new();
    let add = |states: &mut HashMap<SocketAddr, PeerState>, addrs: &[SocketAddr]| {
        for a in addrs {
            states.entry(*a).or_insert(PeerState { running: false, next_try: Instant::now(), fails: 0, dead: false });
        }
    };
    add(&mut states, &link.sources);

    let mut peers: JoinSet<(SocketAddr, u64, Result<PeerEnd, String>)> = JoinSet::new();
    let mut queries: JoinSet<Vec<SocketAddr>> = JoinSet::new();
    let mut next_query = Instant::now();
    let mut last_progress = Instant::now();
    let mut last_received = 0u64;
    let mut tick = tokio::time::interval(Duration::from_millis(500));

    let result = loop {
        // 服务器查询
        if !servers.is_empty() && Instant::now() >= next_query && queries.is_empty() {
            next_query = Instant::now() + cfg.requery;
            for s in servers {
                let (s, h, size, uh, t) = (s.clone(), link.hash, link.size, shared.user_hash, cfg.server);
                queries.spawn(async move {
                    match server::find_sources(&s, &uh, &h, size, t).await {
                        Ok(v) => v,
                        Err(e) => {
                            log::info!("ed2k: {e}");
                            vec![]
                        }
                    }
                });
            }
        }
        // 启动新的来源连接
        let running = states.values().filter(|s| s.running).count();
        if running < cfg.max_peers && !shared.sched.lock().unwrap_or_else(|e| e.into_inner()).is_complete() {
            let now = Instant::now();
            let banned = shared.banned.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let ready: Vec<SocketAddr> = states
                .iter()
                .filter(|(a, s)| !s.running && !s.dead && s.next_try <= now && !banned.contains(*a))
                .map(|(a, _)| *a)
                .take(cfg.max_peers - running)
                .collect();
            for a in ready {
                states.get_mut(&a).unwrap().running = true;
                let sh = shared.clone();
                let id = sh.new_peer_id();
                peers.spawn(async move { (a, id, peer::run_peer(&sh, a, id).await) });
            }
        }

        let received = shared.sched.lock().unwrap_or_else(|e| e.into_inner()).received();
        if received != last_received {
            last_received = received;
            last_progress = Instant::now();
            on_progress(received);
        }
        if shared.sched.lock().unwrap_or_else(|e| e.into_inner()).is_complete() {
            break Ok(link.size);
        }
        if peers.is_empty() && queries.is_empty() && states.values().all(|s| s.dead) && servers.is_empty() {
            break Err(DlError::Net(no_source_msg(&shared, "没有可用的来源。链接里没有来源地址，请在设置里添加 ed2k 服务器")));
        }
        if peers.is_empty() && last_progress.elapsed() > cfg.no_source {
            break Err(DlError::Net(no_source_msg(&shared, "长时间找不到可用的来源")));
        }

        tokio::select! {
            c = wait_ctrl(ctrl) => break Err(DlError::from_ctrl(c)),
            Some(r) = peers.join_next(), if !peers.is_empty() => {
                if let Ok((addr, id, res)) = r {
                    let banned_now = shared.struck_out(id);
                    if banned_now {
                        shared.banned.lock().unwrap_or_else(|e| e.into_inner()).insert(addr);
                    }
                    if let Some(st) = states.get_mut(&addr) {
                        st.running = false;
                        match res {
                            Ok(PeerEnd::Finished) => st.next_try = Instant::now() + cfg.retry_base * 2,
                            Ok(PeerEnd::Dropped) => st.next_try = Instant::now() + cfg.retry_base,
                            Err(_) => {
                                st.fails += 1;
                                st.next_try = Instant::now() + cfg.retry_base * st.fails * st.fails;
                                if st.fails >= 3 || banned_now {
                                    st.dead = true;
                                }
                            }
                        }
                    }
                }
            }
            Some(r) = queries.join_next(), if !queries.is_empty() => {
                if let Ok(addrs) = r {
                    add(&mut states, &addrs);
                }
            }
            _ = shared.wake.notified() => {}
            _ = tick.tick() => {}
        }
    };

    // 停止所有来源连接，保存已校验的进度
    peers.abort_all();
    queries.abort_all();
    while peers.join_next().await.is_some() {}
    {
        use tokio::io::AsyncWriteExt;
        let _ = shared.file.lock().await.flush().await;
    }
    match &result {
        Ok(_) => {
            let _ = std::fs::remove_file(&shared.sidecar);
            on_progress(link.size);
        }
        Err(_) => shared.save_sidecar(),
    }
    result
}

fn no_source_msg(sh: &Shared, base: &str) -> String {
    let last = sh.last_error.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if last.is_empty() {
        base.to_string()
    } else {
        format!("{base}（最近的错误：{last}）")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ed2k::md4::Md4;
    use crate::ed2k::proto::{self, Packet};
    use tokio::net::TcpListener;

    type Ranges = Arc<Mutex<Vec<(u64, u64)>>>;

    fn data(size: usize) -> Vec<u8> {
        (0..size).map(|i| ((i * 31 + i / 7) % 251) as u8).collect()
    }

    fn part_hashes(d: &[u8]) -> Vec<Hash> {
        hash::hash_parts(d, d.len() as u64).unwrap()
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clearclip-ed2k-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("f.part")
    }

    fn fast() -> Config {
        Config {
            max_peers: 2,
            peer: Tuning {
                connect: Duration::from_secs(2),
                handshake: Duration::from_secs(2),
                queue_wait: Duration::from_secs(2),
                idle: Duration::from_secs(2),
            },
            server: server::Timeouts { connect: Duration::from_secs(2), reply: Duration::from_secs(2) },
            no_source: Duration::from_secs(3),
            requery: Duration::from_secs(60),
            retry_base: Duration::from_millis(100),
        }
    }

    /// 模拟对等端：`corrupt` 为真时把每个数据块的第一个字节改坏。
    async fn mock_peer(content: Vec<u8>, corrupt: bool, requested: Ranges) -> SocketAddr {
        let hashes = part_hashes(&content);
        let fh = hash::file_hash(&hashes);
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let (content, hashes, requested) = (content.clone(), hashes.clone(), requested.clone());
                tokio::spawn(async move {
                    while let Ok(p) = proto::read_packet(&mut s).await {
                        let reply: Vec<Packet> = match (p.proto, p.opcode) {
                            (proto::PROTO_EDONKEY, proto::OP_HELLO) => vec![proto::hello_answer(&[7; 16], 4662, "mock")],
                            (_, proto::OP_FILEREQUEST) => vec![proto::file_req_answer(&fh, "f.bin")],
                            (_, proto::OP_SETREQFILEID) => vec![proto::file_status(&fh, None)],
                            (_, proto::OP_HASHSETREQUEST) => vec![proto::hashset_answer(&fh, &hashes)],
                            (_, proto::OP_STARTUPLOADREQ) => vec![Packet::new(proto::OP_ACCEPTUPLOADREQ, vec![])],
                            (_, proto::OP_REQUESTPARTS) => {
                                let (_, ranges) = proto::parse_request_parts(&p.data, false).unwrap();
                                let mut out = vec![];
                                for (a, b) in ranges.into_iter().filter(|r| r.1 > r.0) {
                                    requested.lock().unwrap().push((a, b));
                                    for off in (a..b).step_by(60_000) {
                                        let end = (off + 60_000).min(b);
                                        let mut chunk = content[off as usize..end as usize].to_vec();
                                        if corrupt {
                                            chunk[0] ^= 0xFF;
                                        }
                                        out.push(proto::sending_part(&fh, off, &chunk, false));
                                    }
                                }
                                out
                            }
                            _ => vec![],
                        };
                        for r in reply {
                            if proto::write_packet(&mut s, &r).await.is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });
        addr
    }

    /// 模拟服务器：登录后分配高 ID，被问到来源时返回给定地址。
    async fn mock_server(sources: Vec<SocketAddr>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let sources = sources.clone();
                tokio::spawn(async move {
                    while let Ok(p) = proto::read_packet(&mut s).await {
                        if p.opcode == proto::OP_LOGINREQUEST {
                            let _ = proto::write_packet(&mut s, &Packet::new(proto::OP_IDCHANGE, 0x0A0B_0C0Du32.to_le_bytes().to_vec())).await;
                        } else if p.opcode == proto::OP_GETSOURCES {
                            let h: Hash = p.data[..16].try_into().unwrap();
                            let v4: Vec<_> = sources.iter().filter_map(|a| if let SocketAddr::V4(a) = a { Some(*a) } else { None }).collect();
                            let _ = proto::write_packet(&mut s, &proto::found_sources(&h, &v4)).await;
                        }
                    }
                });
            }
        });
        addr.to_string()
    }

    fn link_for(content: &[u8], with_hashes: bool, sources: Vec<SocketAddr>) -> FileLink {
        let hashes = part_hashes(content);
        FileLink {
            name: "f.bin".into(),
            size: content.len() as u64,
            hash: hash::file_hash(&hashes),
            aich: None,
            part_hashes: if with_hashes { hashes } else { vec![] },
            sources,
        }
    }

    async fn run(link: &FileLink, servers: &[String], part: &Path, cfg: Config) -> Result<u64, DlError> {
        let (_tx, mut rx) = watch::channel(crate::engine::CTRL_RUN);
        download(link, servers, part, &mut rx, &HttpOptions::default(), cfg, |_| {}).await
    }

    #[tokio::test]
    async fn downloads_via_server_and_fetches_hashset() {
        let content = data(21_000_000);
        let peer = mock_peer(content.clone(), false, Default::default()).await;
        let server = mock_server(vec![peer]).await;
        let part = tmp("server");
        let link = link_for(&content, false, vec![]);
        let n = run(&link, &[server], &part, fast()).await.unwrap();
        assert_eq!(n, content.len() as u64);
        assert_eq!(std::fs::read(&part).unwrap(), content);
        assert!(!sidecar_path(&part).exists());
    }

    #[tokio::test]
    async fn small_file_from_link_source() {
        let content = data(500_000);
        let peer = mock_peer(content.clone(), false, Default::default()).await;
        let part = tmp("small");
        let link = link_for(&content, false, vec![peer]);
        run(&link, &[], &part, fast()).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), content);
    }

    #[tokio::test]
    async fn corrupt_source_is_rejected() {
        let content = data(500_000);
        let peer = mock_peer(content.clone(), true, Default::default()).await;
        let part = tmp("corrupt");
        let link = link_for(&content, true, vec![peer]);
        let err = run(&link, &[], &part, fast()).await.unwrap_err();
        assert!(matches!(err, DlError::Net(_)), "{err:?}");
    }

    #[tokio::test]
    async fn resumes_verified_parts() {
        let content = data(21_000_000);
        let requested: Ranges = Default::default();
        let peer = mock_peer(content.clone(), false, requested.clone()).await;
        let part = tmp("resume");
        let link = link_for(&content, true, vec![peer]);
        // 先把第 0 个分块放进文件，并写下记录
        let mut partial = vec![0u8; content.len()];
        partial[..hash::PART_SIZE as usize].copy_from_slice(&content[..hash::PART_SIZE as usize]);
        std::fs::write(&part, &partial).unwrap();
        let hashes: Vec<String> = link.part_hashes.iter().map(hex::encode).collect();
        std::fs::write(
            sidecar_path(&part),
            serde_json::json!({"hash": hex::encode(link.hash), "size": link.size, "partHashes": hashes, "done": [0]}).to_string(),
        )
        .unwrap();
        run(&link, &[], &part, fast()).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), content);
        assert!(requested.lock().unwrap().iter().all(|r| r.0 >= hash::PART_SIZE), "不应重新请求已校验的分块");
    }

    #[tokio::test]
    async fn no_source_without_servers_fails_fast() {
        let content = data(1000);
        let link = link_for(&content, true, vec![]);
        let err = run(&link, &[], &tmp("nosrc"), fast()).await.unwrap_err();
        assert!(err.to_string().contains("没有可用的来源"), "{err}");
    }

    #[tokio::test]
    async fn pause_stops_and_keeps_progress() {
        let content = data(500_000);
        let link = link_for(&content, true, vec!["127.0.0.1:1".parse().unwrap()]);
        let part = tmp("pause");
        let (tx, mut rx) = watch::channel(crate::engine::CTRL_RUN);
        let cfg = Config { no_source: Duration::from_secs(60), ..fast() };
        let h = tokio::spawn({
            let (link, part) = (link.clone(), part.clone());
            async move { download(&link, &[], &part, &mut rx, &HttpOptions::default(), cfg, |_| {}).await }
        });
        tokio::time::sleep(Duration::from_millis(300)).await;
        tx.send(crate::engine::CTRL_PAUSE).unwrap();
        assert!(matches!(h.await.unwrap(), Err(DlError::Paused)));
        assert!(sidecar_path(&part).exists());
    }

    #[test]
    fn user_hash_markers() {
        let h = new_user_hash();
        assert_eq!((h[5], h[14]), (14, 111));
        assert_ne!(h, new_user_hash());
        let _ = Md4::new();
    }
}
