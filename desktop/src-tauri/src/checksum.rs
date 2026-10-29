//! 普通文件的哈希校验：链接里可以带期望的哈希（`#sha256=…`、`?md5=…`、`#checksum=sha256:…`），
//! 浏览器扩展和本机接口也可以直接给。下载完成后流式计算，不整文件读进内存。

use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::io::Read;
use std::path::Path;
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
            Algo::Sha512 => "sha512",
        }
    }

    fn from_name(s: &str) -> Option<Algo> {
        match s.trim().to_ascii_lowercase().replace('-', "").as_str() {
            "md5" => Some(Algo::Md5),
            "sha1" => Some(Algo::Sha1),
            "sha256" => Some(Algo::Sha256),
            "sha512" => Some(Algo::Sha512),
            _ => None,
        }
    }

    fn from_len(hex_len: usize) -> Option<Algo> {
        match hex_len {
            32 => Some(Algo::Md5),
            40 => Some(Algo::Sha1),
            64 => Some(Algo::Sha256),
            128 => Some(Algo::Sha512),
            _ => None,
        }
    }

    fn hex_len(self) -> usize {
        match self {
            Algo::Md5 => 32,
            Algo::Sha1 => 40,
            Algo::Sha256 => 64,
            Algo::Sha512 => 128,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    pub algo: Algo,
    /// 小写十六进制
    pub hex: String,
}

impl Checksum {
    /// `algo:hex` 形式，存进资源里。
    pub fn spec(&self) -> String {
        format!("{}:{}", self.algo.name(), self.hex)
    }
}

fn clean_hex(s: &str) -> Option<String> {
    let h: String = s.chars().filter(|c| !c.is_whitespace() && *c != ':').collect::<String>().to_ascii_lowercase();
    (!h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit())).then_some(h)
}

/// 解析 `sha256:abcd…`、`sha256=abcd…`，或只有哈希值（按长度判断算法）。
pub fn parse(s: &str) -> Option<Checksum> {
    let s = s.trim();
    if let Some((a, h)) = s.split_once([':', '=']) {
        if let (Some(algo), Some(hex)) = (Algo::from_name(a), clean_hex(h)) {
            return (hex.len() == algo.hex_len()).then_some(Checksum { algo, hex });
        }
    }
    let hex = clean_hex(s)?;
    let algo = Algo::from_len(hex.len())?;
    Some(Checksum { algo, hex })
}

/// 从链接的片段或查询参数里找期望的哈希；返回时不改动链接本身。
pub fn from_url(url: &Url) -> Option<Checksum> {
    let mut pairs: Vec<(String, String)> = vec![];
    if let Some(f) = url.fragment() {
        pairs.extend(url::form_urlencoded::parse(f.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())));
    }
    pairs.extend(url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())));
    for (k, v) in pairs {
        let k = k.to_ascii_lowercase();
        if matches!(k.as_str(), "checksum" | "hash" | "digest") {
            if let Some(c) = parse(&v) {
                return Some(c);
            }
        } else if let Some(algo) = Algo::from_name(&k) {
            if let Some(hex) = clean_hex(&v).filter(|h| h.len() == algo.hex_len()) {
                return Some(Checksum { algo, hex });
            }
        }
    }
    None
}

fn digest_file<D: Digest>(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut d = D::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        d.update(&buf[..n]);
    }
    Ok(hex::encode(d.finalize()))
}

/// 计算文件的哈希（小写十六进制）。
pub fn compute(path: &Path, algo: Algo) -> std::io::Result<String> {
    match algo {
        Algo::Md5 => digest_file::<Md5>(path),
        Algo::Sha1 => digest_file::<Sha1>(path),
        Algo::Sha256 => digest_file::<Sha256>(path),
        Algo::Sha512 => digest_file::<Sha512>(path),
    }
}

/// 校验文件：一致返回 `Ok`，不一致返回实际的哈希，读取失败返回错误说明。
pub async fn verify(path: &Path, expect: &Checksum) -> Result<Result<(), String>, String> {
    let p = path.to_path_buf();
    let algo = expect.algo;
    let actual = tokio::task::spawn_blocking(move || compute(&p, algo)).await.map_err(|e| e.to_string())?.map_err(|e| format!("读取文件失败：{e}"))?;
    Ok(if actual == expect.hex { Ok(()) } else { Err(actual) })
}

#[cfg(test)]
mod tests {
    use super::*;

    // "abc" 的标准测试向量
    const MD5: &str = "900150983cd24fb0d6963f7d28e17f72";
    const SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
    const SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const SHA512: &str = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cc-checksum-{}-{name}", std::process::id()));
        std::fs::write(&d, b"abc").unwrap();
        d
    }

    #[test]
    fn parses_prefixed_and_bare_values() {
        assert_eq!(parse(&format!("sha256:{SHA256}")).unwrap().algo, Algo::Sha256);
        assert_eq!(parse(&format!("SHA-1={}", SHA1.to_uppercase())).unwrap(), Checksum { algo: Algo::Sha1, hex: SHA1.into() });
        assert_eq!(parse(MD5).unwrap().algo, Algo::Md5);
        assert_eq!(parse(SHA512).unwrap().algo, Algo::Sha512);
        // 长度和算法对不上、不是十六进制，都不接受
        assert!(parse(&format!("md5:{SHA256}")).is_none());
        assert!(parse("sha256:xyz").is_none());
        assert!(parse("hello").is_none());
    }

    #[test]
    fn finds_the_checksum_in_fragment_or_query() {
        let u = Url::parse(&format!("https://x.test/a.zip#sha256={SHA256}")).unwrap();
        assert_eq!(from_url(&u).unwrap().hex, SHA256);
        let u = Url::parse(&format!("https://x.test/a.zip?v=1&md5={MD5}")).unwrap();
        assert_eq!(from_url(&u).unwrap().algo, Algo::Md5);
        let u = Url::parse(&format!("https://x.test/a.zip#checksum=sha1:{SHA1}")).unwrap();
        assert_eq!(from_url(&u).unwrap().algo, Algo::Sha1);
        assert!(from_url(&Url::parse("https://x.test/a.zip#top").unwrap()).is_none());
    }

    #[test]
    fn computes_every_algorithm() {
        let p = tmp("all");
        assert_eq!(compute(&p, Algo::Md5).unwrap(), MD5);
        assert_eq!(compute(&p, Algo::Sha1).unwrap(), SHA1);
        assert_eq!(compute(&p, Algo::Sha256).unwrap(), SHA256);
        assert_eq!(compute(&p, Algo::Sha512).unwrap(), SHA512);
        let _ = std::fs::remove_file(p);
    }

    #[tokio::test]
    async fn verify_reports_the_actual_hash_on_mismatch() {
        let p = tmp("verify");
        let ok = Checksum { algo: Algo::Sha256, hex: SHA256.into() };
        assert_eq!(verify(&p, &ok).await.unwrap(), Ok(()));
        let bad = Checksum { algo: Algo::Sha256, hex: "0".repeat(64) };
        assert_eq!(verify(&p, &bad).await.unwrap(), Err(SHA256.to_string()));
        assert!(verify(&p.with_extension("missing"), &ok).await.is_err());
        let _ = std::fs::remove_file(p);
    }
}
