//! 与一个对等端（其他 eD2k / eMule 客户端）直连，按调度器分配的数据块下载。

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Notify;

use super::hash::{self, Hash};
use super::md4::Md4;
use super::proto::{self, Packet};
use super::sched::{Claim, Sched};
use crate::engine::http::HttpOptions;

#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    pub connect: Duration,
    pub handshake: Duration,
    /// 排队等待对方开放上传位置的最长时间
    pub queue_wait: Duration,
    /// 开始传输后，多久没有数据就放弃这个来源
    pub idle: Duration,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { connect: Duration::from_secs(8), handshake: Duration::from_secs(15), queue_wait: Duration::from_secs(300), idle: Duration::from_secs(60) }
    }
}

/// 所有来源连接共享的下载状态。
pub struct Shared {
    pub file_hash: Hash,
    pub size: u64,
    pub user_hash: Hash,
    pub part_hashes: Mutex<Vec<Hash>>,
    pub sched: Mutex<Sched>,
    pub path: std::path::PathBuf,
    pub file: tokio::sync::Mutex<tokio::fs::File>,
    pub banned: Mutex<HashSet<SocketAddr>>,
    pub strikes: Mutex<std::collections::HashMap<u64, u32>>,
    pub opts: HttpOptions,
    pub tuning: Tuning,
    pub next_peer: AtomicU64,
    /// 有进展（数据块落盘、分块校验）时唤醒主循环
    pub wake: Notify,
    pub sidecar: std::path::PathBuf,
    /// 记录各来源的最近错误，用于最终的报错信息
    pub last_error: Mutex<String>,
}

impl Shared {
    pub fn new_peer_id(&self) -> u64 {
        self.next_peer.fetch_add(1, Ordering::Relaxed)
    }

    pub fn is_large(&self) -> bool {
        self.size > u32::MAX as u64
    }

    pub fn total_parts(&self) -> usize {
        hash::data_parts(self.size) as usize
    }

    fn hashset_known(&self) -> bool {
        !self.part_hashes.lock().unwrap_or_else(|e| e.into_inner()).is_empty()
    }

    fn note_error(&self, addr: SocketAddr, msg: &str) {
        *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = format!("{addr}：{msg}");
    }

    /// 把已校验的分块序号写到旁边的记录里，续传时据此恢复。
    pub fn save_sidecar(&self) {
        let done = self.sched.lock().unwrap_or_else(|e| e.into_inner()).verified_parts();
        let hashes: Vec<String> = self.part_hashes.lock().unwrap_or_else(|e| e.into_inner()).iter().map(hex::encode).collect();
        let v = serde_json::json!({ "hash": hex::encode(self.file_hash), "size": self.size, "partHashes": hashes, "done": done });
        let _ = std::fs::write(&self.sidecar, v.to_string());
    }

    /// 读出分块的数据并计算 MD4。
    fn hash_part(&self, part: usize) -> std::io::Result<Hash> {
        use std::io::{Read, Seek, SeekFrom};
        let (s, e) = hash::part_range(self.size, part as u64);
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(s))?;
        let mut m = Md4::new();
        let mut left = e - s;
        let mut buf = vec![0u8; 256 * 1024];
        while left > 0 {
            let n = left.min(buf.len() as u64) as usize;
            f.read_exact(&mut buf[..n])?;
            m.update(&buf[..n]);
            left -= n as u64;
        }
        Ok(m.finish())
    }

    /// 分块到齐后校验；失败时整块重来，并给贡献过数据的来源记一次过失（只有它一个贡献者时直接拉黑）。
    pub async fn finish_part(self: &Arc<Self>, part: usize) {
        let me = self.clone();
        let res = tokio::task::spawn_blocking(move || me.hash_part(part)).await;
        let expected = self.part_hashes.lock().unwrap_or_else(|e| e.into_inner()).get(part).copied();
        let ok = matches!((&res, expected), (Ok(Ok(h)), Some(x)) if *h == x);
        let contributors = self.sched.lock().unwrap_or_else(|e| e.into_inner()).verified(part, ok);
        if ok {
            self.save_sidecar();
        } else {
            log::warn!("ed2k part {part} failed verification (contributors: {contributors:?})");
            let mut strikes = self.strikes.lock().unwrap_or_else(|e| e.into_inner());
            for c in &contributors {
                *strikes.entry(*c).or_default() += if contributors.len() == 1 { 2 } else { 1 };
            }
        }
        self.wake.notify_one();
    }

    pub fn struck_out(&self, peer: u64) -> bool {
        self.strikes.lock().unwrap_or_else(|e| e.into_inner()).get(&peer).is_some_and(|n| *n >= 2)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PeerEnd {
    /// 能给的都下完了，或没有我们要的分块
    Finished,
    /// 对方收回了上传位置，稍后可以再来
    Dropped,
}

async fn wait_for(s: &mut TcpStream, t: Duration, mut pick: impl FnMut(&Packet) -> bool) -> Result<Packet, String> {
    let deadline = tokio::time::Instant::now() + t;
    loop {
        let p = tokio::time::timeout_at(deadline, proto::read_packet(s))
            .await
            .map_err(|_| "等待对方回应超时".to_string())?
            .map_err(|e| format!("连接中断：{e}"))?;
        if pick(&p) {
            return Ok(p);
        }
    }
}

async fn send(s: &mut TcpStream, p: Packet) -> Result<(), String> {
    proto::write_packet(s, &p).await.map_err(|e| format!("发送失败：{e}"))
}

/// 连接一个来源并尽可能多地下载。出错返回错误说明。
pub async fn run_peer(sh: &Arc<Shared>, addr: SocketAddr, peer: u64) -> Result<PeerEnd, String> {
    let r = run_inner(sh, addr, peer).await;
    sh.sched.lock().unwrap_or_else(|e| e.into_inner()).release_peer(peer);
    if let Err(e) = &r {
        sh.note_error(addr, e);
    }
    r
}

async fn run_inner(sh: &Arc<Shared>, addr: SocketAddr, peer: u64) -> Result<PeerEnd, String> {
    let t = sh.tuning;
    let mut s = tokio::time::timeout(t.connect, TcpStream::connect(addr)).await.map_err(|_| "连接超时".to_string())?.map_err(|e| format!("无法连接：{e}"))?;
    let _ = s.set_nodelay(true);

    // 握手
    send(&mut s, proto::hello(&sh.user_hash, 4662, "ClearClip")).await?;
    wait_for(&mut s, t.handshake, |p| p.proto == proto::PROTO_EDONKEY && p.opcode == proto::OP_HELLOANSWER).await.map_err(|e| format!("握手失败：{e}"))?;

    // 确认对方有这个文件
    send(&mut s, proto::file_request(proto::OP_FILEREQUEST, &sh.file_hash)).await?;
    let p =
        wait_for(&mut s, t.handshake, |p| matches!(p.opcode, proto::OP_FILEREQANSWER | proto::OP_FILEREQANSNOFIL) && p.proto == proto::PROTO_EDONKEY).await?;
    if p.opcode == proto::OP_FILEREQANSNOFIL {
        return Err("对方没有这个文件".into());
    }
    send(&mut s, proto::file_request(proto::OP_SETREQFILEID, &sh.file_hash)).await?;
    let p = wait_for(&mut s, t.handshake, |p| matches!(p.opcode, proto::OP_FILESTATUS | proto::OP_FILEREQANSNOFIL) && p.proto == proto::PROTO_EDONKEY).await?;
    if p.opcode == proto::OP_FILEREQANSNOFIL {
        return Err("对方没有这个文件".into());
    }
    let (_, has) = proto::parse_file_status(&p.data, sh.total_parts()).map_err(|e| format!("无法理解对方的分块信息：{e}"))?;
    if !sh.sched.lock().unwrap_or_else(|e| e.into_inner()).wants(&has) {
        return Ok(PeerEnd::Finished);
    }

    // 分块哈希：链接里没有时向对方要，并用文件哈希核对
    if !sh.hashset_known() {
        send(&mut s, proto::file_request(proto::OP_HASHSETREQUEST, &sh.file_hash)).await?;
        let p = wait_for(&mut s, t.handshake, |p| p.opcode == proto::OP_HASHSETANSWER).await?;
        let (_, set) = proto::parse_hashset_answer(&p.data).map_err(|e| format!("无法理解对方的哈希表：{e}"))?;
        if set.len() as u64 != hash::part_count(sh.size) || hash::file_hash(&set) != sh.file_hash {
            return Err("对方给出的分块哈希与文件哈希不符".into());
        }
        *sh.part_hashes.lock().unwrap_or_else(|e| e.into_inner()) = set;
        sh.save_sidecar();
    }

    // 排队：等对方开放上传位置
    send(&mut s, proto::file_request(proto::OP_STARTUPLOADREQ, &sh.file_hash)).await?;
    let deadline = tokio::time::Instant::now() + t.queue_wait;
    loop {
        let p = tokio::time::timeout_at(deadline, proto::read_packet(&mut s))
            .await
            .map_err(|_| "排队时间太长，换一个来源".to_string())?
            .map_err(|e| format!("连接中断：{e}"))?;
        match p.opcode {
            proto::OP_ACCEPTUPLOADREQ if p.proto == proto::PROTO_EDONKEY => break,
            proto::OP_OUTOFPARTREQS | proto::OP_CANCELTRANSFER if p.proto == proto::PROTO_EDONKEY => return Ok(PeerEnd::Dropped),
            _ => {}
        }
    }

    // 传输：每次最多请求三个数据块
    let large = sh.is_large();
    loop {
        if sh.struck_out(peer) {
            return Err("这个来源给出的数据没有通过校验".into());
        }
        let claims = sh.sched.lock().unwrap_or_else(|e| e.into_inner()).claim(peer, &has, 3);
        if claims.is_empty() {
            let _ = send(&mut s, proto::file_request(proto::OP_CANCELTRANSFER, &sh.file_hash)).await;
            return Ok(PeerEnd::Finished);
        }
        let ranges: Vec<(u64, u64)> = claims.iter().map(|c| (c.start, c.end)).collect();
        send(&mut s, proto::request_parts(&sh.file_hash, &ranges, large)).await?;
        receive(sh, &mut s, peer, &claims).await?;
    }
}

/// 收完这一批数据块；收到的数据直接写到文件对应位置。
async fn receive(sh: &Arc<Shared>, s: &mut TcpStream, peer: u64, claims: &[Claim]) -> Result<(), String> {
    let mut got: Vec<u64> = vec![0; claims.len()];
    let mut finished = vec![false; claims.len()];
    while finished.iter().any(|f| !f) {
        let p = tokio::time::timeout(sh.tuning.idle, proto::read_packet(s))
            .await
            .map_err(|_| "对方长时间没有发送数据".to_string())?
            .map_err(|e| format!("连接中断：{e}"))?;
        match (p.proto, p.opcode) {
            (proto::PROTO_EDONKEY, proto::OP_SENDINGPART) | (proto::PROTO_EMULE, proto::OP_SENDINGPART_I64) => {
                let (h, start, data) = proto::parse_sending_part(&p.data, p.opcode == proto::OP_SENDINGPART_I64).map_err(|e| e.to_string())?;
                if h != sh.file_hash {
                    continue;
                }
                let Some(i) = claims.iter().position(|c| start >= c.start && start < c.end) else {
                    return Err("对方发来了我们没有请求的数据".into());
                };
                let c = claims[i];
                if start != c.start + got[i] || start + data.len() as u64 > c.end {
                    return Err("对方发来的数据位置不对".into());
                }
                {
                    let mut f = sh.file.lock().await;
                    f.seek(std::io::SeekFrom::Start(start)).await.map_err(|e| format!("写入失败：{e}"))?;
                    f.write_all(&data).await.map_err(|e| format!("写入失败：{e}"))?;
                }
                got[i] += data.len() as u64;
                sh.opts.throttle(data.len()).await;
                if c.start + got[i] == c.end {
                    finished[i] = true;
                    let ready = sh.sched.lock().unwrap_or_else(|e| e.into_inner()).complete(peer, c);
                    sh.wake.notify_one();
                    if let Some(part) = ready {
                        sh.finish_part(part).await;
                    }
                }
            }
            (proto::PROTO_EMULE, proto::OP_COMPRESSEDPART) => return Err("对方发来压缩数据（暂不支持）".into()),
            (proto::PROTO_EDONKEY, proto::OP_OUTOFPARTREQS) | (proto::PROTO_EDONKEY, proto::OP_CANCELTRANSFER) => {
                return Err("对方收回了上传位置".into());
            }
            _ => {}
        }
    }
    Ok(())
}
