//! 与 eD2k 服务器交互：登录、按文件哈希查询来源；以及解析 server.met 服务器列表。

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::TcpStream;

use super::hash::Hash;
use super::proto::{self, In};

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub connect: Duration,
    /// 登录后等待 ID 分配、以及查询后等待来源回复
    pub reply: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts { connect: Duration::from_secs(8), reply: Duration::from_secs(12) }
    }
}

/// 向一个服务器查询文件的来源。服务器地址形如 `host:port`。
pub async fn find_sources(server: &str, user_hash: &Hash, file: &Hash, size: u64, t: Timeouts) -> Result<Vec<SocketAddr>, String> {
    let mut s = tokio::time::timeout(t.connect, TcpStream::connect(server))
        .await
        .map_err(|_| format!("连接服务器 {server} 超时"))?
        .map_err(|e| format!("无法连接服务器 {server}：{e}"))?;
    let _ = s.set_nodelay(true);
    proto::write_packet(&mut s, &proto::login_request(user_hash, 4662, "ClearClip")).await.map_err(|e| e.to_string())?;
    // 等待 ID 分配（其间会收到服务器消息、状态等，忽略）
    let deadline = tokio::time::Instant::now() + t.reply;
    let mut id = None;
    while id.is_none() {
        let p = tokio::time::timeout_at(deadline, proto::read_packet(&mut s))
            .await
            .map_err(|_| format!("服务器 {server} 没有回应登录"))?
            .map_err(|e| format!("服务器 {server} 断开：{e}"))?;
        if p.opcode == proto::OP_IDCHANGE {
            id = Some(proto::parse_id_change(&p.data).map_err(|e| e.to_string())?);
        }
    }
    if id == Some(0) {
        return Err(format!("服务器 {server} 拒绝了登录"));
    }
    proto::write_packet(&mut s, &proto::get_sources(file, size)).await.map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + t.reply;
    loop {
        let p = tokio::time::timeout_at(deadline, proto::read_packet(&mut s)).await;
        let Ok(Ok(p)) = p else { return Ok(vec![]) };
        if p.opcode == proto::OP_FOUNDSOURCES {
            let (h, addrs) = proto::parse_found_sources(&p.data).map_err(|e| e.to_string())?;
            if &h == file {
                return Ok(addrs);
            }
        }
    }
}

/// 解析 eMule 的 server.met：返回 `ip:端口` 列表。
pub fn parse_server_met(data: &[u8]) -> Result<Vec<String>, String> {
    let mut r = In::new(data);
    let magic = r.u8().map_err(|e| e.to_string())?;
    if magic != 0xE0 && magic != 0x0E {
        return Err("不是 server.met 文件".into());
    }
    let n = r.u32().map_err(|e| e.to_string())?;
    let mut out = vec![];
    for _ in 0..n.min(10_000) {
        let Ok(ip) = r.take(4) else { break };
        let Ok(port) = r.u16() else { break };
        if proto::read_tags(&mut r).is_err() {
            break;
        }
        if port != 0 && ip != [0, 0, 0, 0] {
            out.push(format!("{}:{port}", Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])));
        }
    }
    Ok(out)
}

/// 服务器的 `host:port` 写法检查，补默认端口 4661。
pub fn normalize_server(s: &str) -> Option<String> {
    let s = s.trim().trim_start_matches("ed2k://|server|").trim_end_matches("|/");
    let s = s.replace('|', ":");
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.parse::<u16>().is_ok_and(|p| p > 0) => Some(s),
        Some(_) => None,
        None => Some(format!("{s}:4661")),
    }
}

/// 最多使用 server.met 里的这么多台服务器（每台都要连一次，太多没有意义）
const MET_LIMIT: usize = 6;

/// 汇总要查询的服务器：设置里手动填的，加上 server.met 列表（下载失败时用上次缓存的）。
pub async fn collect_servers(client: &reqwest::Client, data_dir: &std::path::Path, custom: &[String], met_url: &str) -> Vec<String> {
    let mut out: Vec<String> = custom.iter().filter_map(|s| normalize_server(s)).collect();
    let cache = data_dir.join("ed2k_servers.met");
    let met_url = met_url.trim();
    if !met_url.is_empty() {
        let fetched = async {
            let r = client.get(met_url).timeout(Duration::from_secs(15)).send().await.ok()?.error_for_status().ok()?;
            let b = r.bytes().await.ok()?;
            (b.len() <= 4 * 1024 * 1024).then(|| b.to_vec())
        }
        .await;
        let list = match fetched.as_deref().map(parse_server_met) {
            Some(Ok(l)) if !l.is_empty() => {
                let _ = std::fs::write(&cache, fetched.as_deref().unwrap_or_default());
                l
            }
            _ => std::fs::read(&cache).ok().and_then(|b| parse_server_met(&b).ok()).unwrap_or_default(),
        };
        out.extend(list.into_iter().take(MET_LIMIT));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| seen.insert(s.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_met() {
        let mut o = proto::Out::default();
        o.u8(0xE0).u32(3);
        for (ip, port) in [([45u8, 82, 80, 155], 5687u16), ([0, 0, 0, 0], 4661), ([1, 2, 3, 4], 4242)] {
            o.bytes(&ip).u16(port).u32(1).tag_str(1, "srv");
        }
        assert_eq!(parse_server_met(&o.0).unwrap(), vec!["45.82.80.155:5687", "1.2.3.4:4242"]);
        assert!(parse_server_met(&[0x11, 0, 0, 0, 0]).is_err());
        // 文件被截断：返回已解析出的部分
        assert_eq!(parse_server_met(&o.0[..o.0.len() - 3]).unwrap(), vec!["45.82.80.155:5687"]);
    }

    #[test]
    fn server_names() {
        assert_eq!(normalize_server("1.2.3.4:4661").as_deref(), Some("1.2.3.4:4661"));
        assert_eq!(normalize_server("srv.example.com").as_deref(), Some("srv.example.com:4661"));
        assert_eq!(normalize_server("ed2k://|server|1.2.3.4|4242|/").as_deref(), Some("1.2.3.4:4242"));
        assert_eq!(normalize_server("a b:1"), None);
        assert_eq!(normalize_server("host:0"), None);
        assert_eq!(normalize_server(""), None);
    }
}
