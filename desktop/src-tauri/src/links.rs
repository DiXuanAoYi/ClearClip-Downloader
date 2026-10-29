//! 非 http(s) 的下载链接：识别、迅雷 / 快车 / QQ 旋风的专用链接解码。
//! 解码后得到普通链接的，交回原有的解析流程；其余协议（FTP、SFTP、流媒体、ed2k、磁力）由 `providers::protocols` 处理。

use std::sync::LazyLock;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use regex::Regex;

/// 链接所属的下载方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Http,
    Ftp,
    Sftp,
    /// RTMP / RTSP / MMS 等流，交给 ffmpeg
    Stream,
    Ed2k,
    Magnet,
    /// thunder:// flashget:// qqdl://，解码后是普通链接
    Wrapped,
    Other,
}

pub fn kind_of(url: &str) -> LinkKind {
    let lower = url.trim().to_ascii_lowercase();
    let scheme = lower.split([':']).next().unwrap_or("");
    match scheme {
        "http" | "https" => LinkKind::Http,
        "ftp" | "ftps" => LinkKind::Ftp,
        "sftp" => LinkKind::Sftp,
        "rtmp" | "rtmps" | "rtmpe" | "rtmpt" | "rtsp" | "rtsps" | "mms" | "mmsh" | "mmst" | "rtp" | "srt" => LinkKind::Stream,
        "ed2k" => LinkKind::Ed2k,
        "magnet" => LinkKind::Magnet,
        "thunder" | "flashget" | "qqdl" => LinkKind::Wrapped,
        _ => LinkKind::Other,
    }
}

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:(?:thunder|flashget|qqdl|ed2k|ftps?|sftp|rtmps?|rtmpe|rtmpt|rtsps?|mmsh?|mmst|srt)://[^\s<>"'\u{3000}\u{ff0c}\u{3002}]+|magnet:\?[^\s<>"'\u{3000}]+)"#,
    )
    .unwrap()
});

/// 从文本里找出非 http 的下载链接（按出现顺序，去重）。
pub fn extract_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for m in LINK_RE.find_iter(text) {
        let mut s = m.as_str().trim_end_matches(['.', ',', ';', '!', '?', ')', ']', '）', '，', '。']).to_string();
        // ed2k 链接以 |/ 结尾，上面不会误删；磁力链接末尾的 & 属于多余分隔
        s = s.trim_end_matches('&').to_string();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

fn b64(s: &str) -> Option<Vec<u8>> {
    let t: String = s.trim().trim_end_matches('/').chars().filter(|c| !c.is_whitespace()).collect();
    let t = t.trim_end_matches('=');
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(t).ok().or_else(|| STANDARD.decode(t).ok())
}

fn text_of(bytes: Vec<u8>) -> Option<String> {
    // 旧版迅雷链接里的中文常常是 GBK
    match String::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(e) => {
            let (cow, _, bad) = encoding_rs::GBK.decode(e.as_bytes());
            (!bad).then(|| cow.into_owned())
        }
    }
}

/// 迅雷（thunder://）、快车（flashget://）、QQ 旋风（qqdl://）的专用链接还原成原始地址。
/// 地址本身不是有效链接时返回 None。
pub fn unwrap_link(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let raw = text_of(b64(rest.split(['&', '#']).next()?)?)?;
    let inner = match scheme.to_ascii_lowercase().as_str() {
        // AA<地址>ZZ
        "thunder" => raw.strip_prefix("AA")?.strip_suffix("ZZ")?.to_string(),
        // [FLASHGET]<地址>[FLASHGET]
        "flashget" => raw.strip_prefix("[FLASHGET]")?.strip_suffix("[FLASHGET]")?.to_string(),
        "qqdl" => raw,
        _ => return None,
    };
    let inner = inner.trim().to_string();
    (kind_of(&inner) != LinkKind::Other && kind_of(&inner) != LinkKind::Wrapped).then_some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thunder(url: &str) -> String {
        format!("thunder://{}", STANDARD.encode(format!("AA{url}ZZ")))
    }

    #[test]
    fn classifies_schemes() {
        assert_eq!(kind_of("https://a.com/x"), LinkKind::Http);
        assert_eq!(kind_of("FTP://a.com/x"), LinkKind::Ftp);
        assert_eq!(kind_of("sftp://a.com/x"), LinkKind::Sftp);
        assert_eq!(kind_of("rtmp://a.com/live/x"), LinkKind::Stream);
        assert_eq!(kind_of("rtsp://10.0.0.2/cam"), LinkKind::Stream);
        assert_eq!(kind_of("ed2k://|file|a|1|0123|/"), LinkKind::Ed2k);
        assert_eq!(kind_of("magnet:?xt=urn:btih:abc"), LinkKind::Magnet);
        assert_eq!(kind_of("thunder://QUE="), LinkKind::Wrapped);
        assert_eq!(kind_of("mailto:a@b.c"), LinkKind::Other);
    }

    #[test]
    fn decodes_thunder_flashget_qqdl() {
        let u = "http://example.com/a%20b.zip";
        assert_eq!(unwrap_link(&thunder(u)).as_deref(), Some(u));
        let fg = format!("flashget://{}&refererUrl", STANDARD.encode(format!("[FLASHGET]{u}[FLASHGET]")));
        assert_eq!(unwrap_link(&fg).as_deref(), Some(u));
        let qq = format!("qqdl://{}", STANDARD.encode(u));
        assert_eq!(unwrap_link(&qq).as_deref(), Some(u));
        // 没有补齐的 base64 也接受
        let t = thunder(u).trim_end_matches('=').to_string();
        assert_eq!(unwrap_link(&t).as_deref(), Some(u));
        // 解出来的是 ed2k / 磁力 / ftp 也算数
        assert_eq!(unwrap_link(&thunder("ftp://h/f.iso")).as_deref(), Some("ftp://h/f.iso"));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(unwrap_link("thunder://!!!"), None);
        assert_eq!(unwrap_link(&format!("thunder://{}", STANDARD.encode("no wrapper"))), None);
        assert_eq!(unwrap_link(&thunder("not a link")), None);
        assert_eq!(unwrap_link("https://a.com"), None);
    }

    #[test]
    fn thunder_with_gbk_name() {
        let (gbk, _, _) = encoding_rs::GBK.encode("ftp://h/测试.zip");
        let link = format!("thunder://{}", STANDARD.encode([b"AA".as_slice(), &gbk, b"ZZ"].concat()));
        assert_eq!(unwrap_link(&link).as_deref(), Some("ftp://h/测试.zip"));
    }

    #[test]
    fn extracts_links_from_text() {
        let t = "看这个 ftp://example.com/pub/a.iso。还有 magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x 以及\ned2k://|file|a.zip|100|0123456789ABCDEF0123456789ABCDEF|/ 和 rtmp://live.example.com/app/stream1, ok";
        let l = extract_links(t);
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], "ftp://example.com/pub/a.iso");
        assert!(l[1].starts_with("magnet:?xt=urn:btih:0123"));
        assert!(l[2].ends_with("|/"));
        assert_eq!(l[3], "rtmp://live.example.com/app/stream1");
        assert!(extract_links("没有链接 https://a.com").is_empty());
    }
}
