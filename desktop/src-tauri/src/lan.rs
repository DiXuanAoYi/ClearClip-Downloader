//! 选出手机能连上的电脑地址。
//!
//! 开了代理软件的虚拟网卡（TUN / 增强模式）、VPN、虚拟机或 Docker 后，系统的默认路由会指向虚拟网卡，
//! 只看“默认路由用哪个地址”会选到手机连不上的地址（比如 Clash 的 198.18.0.1）。
//! 这里枚举全部网卡，过滤并排序：真实的 Wi-Fi / 有线网卡上的私有地址优先，虚拟网卡排在后面，
//! 界面上列出全部候选，用户可以手动切换或填写地址。

use std::net::{IpAddr, Ipv4Addr};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanAddr {
    pub ip: String,
    /// 网卡名
    pub name: String,
    /// 看起来是真实网卡上的局域网地址（手机大概率连得上）
    pub likely: bool,
}

/// 名字里带这些词的多半是虚拟网卡 / 隧道 / 虚拟机 / 容器。
const VIRTUAL_NAMES: &[&str] = &[
    "tun",
    "tap",
    "utun",
    "wintun",
    "wireguard",
    "wg",
    "tailscale",
    "zerotier",
    "zt",
    "clash",
    "meta",
    "mihomo",
    "sing-box",
    "singbox",
    "v2ray",
    "surge",
    "docker",
    "veth",
    "br-",
    "virbr",
    "vmnet",
    "vmware",
    "vbox",
    "virtualbox",
    "vethernet",
    "hyper-v",
    "loopback",
    "pseudo",
    "awdl",
    "llw",
    "npcap",
    "bridge",
    "p2p",
    "ppp",
    "vpn",
    "openvpn",
    "nordlynx",
    "hamachi",
];
/// 名字里带这些词的多半是真实的 Wi-Fi / 有线网卡。
const REAL_NAMES: &[&str] = &["wi-fi", "wifi", "wlan", "wlp", "wlo", "ethernet", "eth", "en0", "en1", "en2", "enp", "eno", "ens", "以太网", "无线"];

fn name_has(name: &str, words: &[&str]) -> bool {
    let n = name.to_lowercase();
    words.iter().any(|w| {
        // 很短的词（tun、lo、wg、zt）只在名字开头匹配，避免误伤
        if w.len() <= 3 {
            n.starts_with(w)
        } else {
            n.contains(w)
        }
    })
}

/// 198.18.0.0/15 是 Clash / Surge 等虚拟网卡 fake-ip 用的保留段（基准测试地址），不会是真实局域网。
fn is_benchmark(ip: Ipv4Addr) -> bool {
    ip.octets()[0] == 198 && (ip.octets()[1] & 0xfe) == 18
}

/// 地址本身的得分：越像家里 / 公司路由器分配的局域网地址越高。
fn addr_score(ip: Ipv4Addr) -> i32 {
    let o = ip.octets();
    if is_benchmark(ip) || ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return -100;
    }
    if ip.is_link_local() {
        return -50;
    }
    if o[0] == 100 && (o[1] & 0xc0) == 64 {
        return -20; // CGNAT / Tailscale
    }
    match o {
        [192, 168, ..] => 30,
        [10, ..] => 20,
        [172, b, ..] if (16..32).contains(&b) => 15,
        _ => -10, // 公网地址：不是局域网
    }
}

/// 对 (网卡名, 地址) 排序，得分最高的在前。`likely` 表示可以放心使用。
pub fn rank(found: &[(String, Ipv4Addr)]) -> Vec<LanAddr> {
    let mut scored: Vec<(i32, LanAddr)> = Vec::new();
    for (name, ip) in found {
        let mut score = addr_score(*ip);
        if score <= -100 {
            continue;
        }
        let virt = name_has(name, VIRTUAL_NAMES);
        if virt {
            score -= 60;
        } else if name_has(name, REAL_NAMES) {
            score += 15;
        }
        // 172.17.x.x 是 Docker 默认网段
        if ip.octets()[0] == 172 && ip.octets()[1] == 17 {
            score -= 20;
        }
        let likely = !virt && score >= 15;
        scored.push((score, LanAddr { ip: ip.to_string(), name: name.clone(), likely }));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.ip.cmp(&b.1.ip)));
    let mut out: Vec<LanAddr> = Vec::new();
    for (_, a) in scored {
        if !out.iter().any(|x| x.ip == a.ip) {
            out.push(a);
        }
    }
    out
}

/// 本机所有网卡上的 IPv4 地址。
fn interfaces() -> Vec<(String, Ipv4Addr)> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) => Some((i.name.clone(), v4)),
            _ => None,
        })
        .collect()
}

/// 候选地址（已排序）。
pub fn candidates() -> Vec<LanAddr> {
    rank(&interfaces())
}

/// 默认要用的地址：手动指定的优先；否则取排序第一的真实网卡地址；
/// 都没有时退回“默认路由使用的地址”（可能是虚拟网卡，但总比没有强）。
pub fn pick(manual: &str) -> Option<IpAddr> {
    if let Ok(ip) = manual.trim().parse::<IpAddr>() {
        return Some(ip);
    }
    let list = candidates();
    if let Some(a) = list.iter().find(|a| a.likely).or_else(|| list.first()) {
        return a.ip.parse().ok();
    }
    route_ip()
}

/// 通过 UDP “连接”选路得到默认路由使用的本机地址（不会真的发包）。
fn route_ip() -> Option<IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.168.1.1:9").or_else(|_| sock.connect("10.255.255.255:9")).ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(list: &[(&str, &str)]) -> Vec<(String, Ipv4Addr)> {
        list.iter().map(|(n, i)| (n.to_string(), i.parse().unwrap())).collect()
    }

    #[test]
    fn clash_tun_address_is_never_first() {
        let r = rank(&v(&[("Meta", "198.18.0.1"), ("WLAN", "192.168.1.23"), ("Loopback Pseudo-Interface 1", "127.0.0.1")]));
        assert_eq!(r.len(), 1, "198.18/15 和回环不进候选");
        assert_eq!(r[0].ip, "192.168.1.23");
        assert!(r[0].likely);
    }

    #[test]
    fn virtual_adapters_rank_below_real_ones_but_stay_listed() {
        let r = rank(&v(&[
            ("utun3", "10.8.0.2"),
            ("en0", "192.168.0.8"),
            ("docker0", "172.17.0.1"),
            ("vEthernet (WSL)", "172.28.80.1"),
            ("tailscale0", "100.101.1.2"),
        ]));
        assert_eq!(r[0].ip, "192.168.0.8");
        assert!(r[0].likely);
        for a in &r[1..] {
            assert!(!a.likely, "{} 不应标为可靠", a.ip);
        }
        assert!(r.iter().any(|a| a.ip == "10.8.0.2"), "虚拟网卡仍可手动选择");
    }

    #[test]
    fn real_network_wins_even_when_the_vpn_has_a_prettier_address() {
        let r = rank(&v(&[("tun0", "192.168.200.1"), ("eth0", "10.0.0.5")]));
        assert_eq!(r[0].ip, "10.0.0.5");
    }

    #[test]
    fn short_words_only_match_at_the_start() {
        // “Intel(R) Ethernet” 里有 tun？不应被当成虚拟网卡
        let r = rank(&v(&[("Ethernet", "192.168.1.9"), ("Intel Gigabit tunnel-less", "192.168.1.10")]));
        assert!(r.iter().all(|a| a.likely));
    }

    #[test]
    fn manual_address_wins() {
        assert_eq!(pick("192.168.7.7"), Some("192.168.7.7".parse().unwrap()));
        assert_eq!(pick(" 10.1.2.3 "), Some("10.1.2.3".parse().unwrap()));
    }

    #[test]
    fn duplicates_are_collapsed() {
        let r = rank(&v(&[("eth0", "192.168.1.2"), ("eth0:1", "192.168.1.2")]));
        assert_eq!(r.len(), 1);
    }
}
