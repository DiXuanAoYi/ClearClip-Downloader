//! `ed2k://` 链接的解析与生成。
//!
//! 文件链接：`ed2k://|file|名称|大小|MD4 哈希|/`，后面可以带 `h=`（AICH 根哈希）、`p=`（各分块的 MD4，用冒号分隔）、
//! `s=`（HTTP 来源）和 `sources,IP:端口`。服务器链接：`ed2k://|server|IP|端口|/`；服务器列表：`ed2k://|serverlist|地址|/`。

use std::net::SocketAddr;

use super::hash::{self, Hash};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLink {
    pub name: String,
    pub size: u64,
    pub hash: Hash,
    pub aich: Option<String>,
    /// 链接里自带的分块哈希（没有时要从来源取）
    pub part_hashes: Vec<Hash>,
    pub sources: Vec<SocketAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ed2kLink {
    File(FileLink),
    Server { host: String, port: u16 },
    ServerList(String),
}

pub fn parse_hash(s: &str) -> Option<Hash> {
    let b = hex::decode(s.trim()).ok()?;
    <Hash>::try_from(b.as_slice()).ok()
}

pub fn hash_hex(h: &Hash) -> String {
    hex::encode_upper(h)
}

pub fn parse(link: &str) -> Result<Ed2kLink, String> {
    let rest = link.trim().get(..7).filter(|p| p.eq_ignore_ascii_case("ed2k://")).map(|_| &link.trim()[7..]).ok_or("不是 ed2k 链接")?;
    let parts: Vec<&str> = rest.split('|').collect();
    // 以 | 开头：第一项为空
    if parts.first() != Some(&"") {
        return Err("ed2k 链接格式不正确".into());
    }
    match parts.get(1).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("file") => {
            // 名称里的 + 当作空格（部分网站这样写）；真正的加号会写成 %2B
            let raw_name = parts.get(2).copied().unwrap_or("");
            let name = crate::grab::percent_decode(&raw_name.replace('+', " "));
            let size: u64 = parts.get(3).and_then(|s| s.parse().ok()).ok_or("链接里的文件大小不正确")?;
            let hash = parts.get(4).and_then(|s| parse_hash(s)).ok_or("链接里的文件哈希不正确（应为 32 位十六进制）")?;
            if name.trim().is_empty() {
                return Err("链接里没有文件名".into());
            }
            let mut link = FileLink { name, size, hash, aich: None, part_hashes: vec![], sources: vec![] };
            for extra in parts.iter().skip(5) {
                if let Some(v) = extra.strip_prefix("h=") {
                    link.aich = Some(v.to_string());
                } else if let Some(v) = extra.strip_prefix("p=") {
                    let hs: Option<Vec<Hash>> = v.split(':').filter(|x| !x.is_empty()).map(parse_hash).collect();
                    if let Some(hs) = hs {
                        // 只接受能还原出文件哈希的分块哈希
                        if hs.len() as u64 == hash::part_count(size) && hash::file_hash(&hs) == link.hash {
                            link.part_hashes = hs;
                        }
                    }
                } else if let Some(v) = extra.strip_prefix("sources,") {
                    link.sources.extend(v.split(',').filter_map(|a| a.trim().parse::<SocketAddr>().ok()));
                }
            }
            Ok(Ed2kLink::File(link))
        }
        Some("server") => {
            let host = parts.get(2).copied().unwrap_or("").to_string();
            let port: u16 = parts.get(3).and_then(|s| s.parse().ok()).ok_or("服务器端口不正确")?;
            if host.is_empty() {
                return Err("服务器地址为空".into());
            }
            Ok(Ed2kLink::Server { host, port })
        }
        Some("serverlist") => Ok(Ed2kLink::ServerList(parts.get(2).copied().unwrap_or("").to_string())),
        _ => Err("暂不支持这种 ed2k 链接（只支持文件链接）".into()),
    }
}

impl FileLink {
    pub fn to_link(&self) -> String {
        let enc: String = self
            .name
            .bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~()[]".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect();
        format!("ed2k://|file|{enc}|{}|{}|/", self.size, hash_hex(&self.hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "31D6CFE0D16AE931B73C59D7E0C089C0";

    #[test]
    fn parses_a_file_link() {
        let l = parse(&format!("ed2k://|file|Some%20Movie%20(2020).mkv|734003200|{H}|h=ABCDEFGHIJKLMNOPQRSTUVWXYZ234567|/")).unwrap();
        let Ed2kLink::File(f) = l else { panic!() };
        assert_eq!(f.name, "Some Movie (2020).mkv");
        assert_eq!(f.size, 734_003_200);
        assert_eq!(hash_hex(&f.hash), H);
        assert_eq!(f.aich.as_deref(), Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"));
        assert_eq!(f.to_link(), format!("ed2k://|file|Some%20Movie%20(2020).mkv|734003200|{H}|/"));
    }

    #[test]
    fn plus_signs_and_unicode_names() {
        let Ed2kLink::File(f) = parse(&format!("ed2k://|file|a+b+c.zip|10|{H}|/")).unwrap() else { panic!() };
        assert_eq!(f.name, "a b c.zip");
        let Ed2kLink::File(f) = parse(&format!("ed2k://|file|%E6%B5%8B%E8%AF%95.zip|10|{H}|/")).unwrap() else { panic!() };
        assert_eq!(f.name, "测试.zip");
        // 往返
        let Ed2kLink::File(g) = parse(&f.to_link()).unwrap() else { panic!() };
        assert_eq!(g, f);
    }

    #[test]
    fn parses_sources_and_part_hashes() {
        // 两个分块的文件：文件哈希必须等于分块哈希拼接后的 MD4
        let size = hash::PART_SIZE + 5;
        let p1 = super::super::md4::md4(b"one");
        let p2 = super::super::md4::md4(b"two");
        let fh = hash::file_hash(&[p1, p2]);
        let p = format!("{}:{}", hash_hex(&p1), hash_hex(&p2));
        let l = format!("ed2k://|file|x.bin|{size}|{}|p={p}|sources,1.2.3.4:4662,5.6.7.8:4672|/", hash_hex(&fh));
        let Ed2kLink::File(f) = parse(&l).unwrap() else { panic!() };
        assert_eq!(f.part_hashes, vec![p1, p2]);
        assert_eq!(f.sources, vec!["1.2.3.4:4662".parse().unwrap(), "5.6.7.8:4672".parse().unwrap()]);
        // 分块哈希对不上文件哈希：忽略
        let bad = format!("ed2k://|file|x.bin|{size}|{H}|p={p}|/");
        let Ed2kLink::File(f) = parse(&bad).unwrap() else { panic!() };
        assert!(f.part_hashes.is_empty());
    }

    #[test]
    fn server_and_errors() {
        assert_eq!(parse("ed2k://|server|45.82.80.155|5687|/").unwrap(), Ed2kLink::Server { host: "45.82.80.155".into(), port: 5687 });
        assert!(matches!(parse("ed2k://|serverlist|http://x/server.met|/").unwrap(), Ed2kLink::ServerList(_)));
        assert!(parse("ed2k://|file|a|x|abc|/").is_err());
        assert!(parse(&format!("ed2k://|file|a|10|{}|/", &H[..30])).is_err());
        assert!(parse(&format!("ed2k://|file||10|{H}|/")).is_err());
        assert!(parse("http://a.com").is_err());
        assert!(parse("ed2k://file|a|1|x|/").is_err());
        assert!(parse("ed2k://|friend|x|/").is_err());
    }
}
