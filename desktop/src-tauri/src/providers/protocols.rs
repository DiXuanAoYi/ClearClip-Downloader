//! FTP / SFTP / 流媒体（RTMP、RTSP 等）链接：不需要联网解析，链接本身就是下载地址。

use sha2::{Digest, Sha256};
use url::Url;

use crate::grab::{clean_filename, percent_decode, split_name};
use crate::links::{kind_of, LinkKind};
use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind, Protocol};

fn media(platform: &str, platform_name: &str, id_src: &str, source_url: String, title: String, kind: MediaKind, assets: Vec<Asset>) -> MediaInfo {
    MediaInfo {
        platform: platform.into(),
        platform_name: platform_name.into(),
        id: hex::encode(&Sha256::digest(id_src.as_bytes())[..6]),
        source_url,
        title,
        author: String::new(),
        cover: None,
        duration_ms: None,
        kind,
        width: None,
        height: None,
        published_at: None,
        assets,
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some(platform.into()),
    }
}

/// 地址里的用户名密码拿出来，地址本身不再带它们（不写进日志、列表和数据库里的链接）。
/// 返回（不含凭据的地址，"user:pass"）。没有凭据时 FTP 用匿名登录，SFTP 留空由引擎报错。
pub fn split_credentials(url: &Url) -> (Url, Option<String>) {
    let mut clean = url.clone();
    let user = percent_decode(url.username());
    let pass = percent_decode(url.password().unwrap_or(""));
    let _ = clean.set_username("");
    let _ = clean.set_password(None);
    let cred = (!user.is_empty()).then(|| format!("{user}:{pass}"));
    (clean, cred)
}

fn file_name(url: &Url) -> String {
    crate::grab::url_name(url)
}

/// FTP / SFTP：一个文件（地址以 `/` 结尾的目录暂不支持）。
fn resolve_remote_file(url: &str, protocol: Protocol) -> AppResult<MediaInfo> {
    let parsed = Url::parse(url.trim()).map_err(|_| AppError::invalid("链接格式不正确。"))?;
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err(AppError::invalid("链接里没有服务器地址。"));
    }
    if parsed.path().ends_with('/') || parsed.path().is_empty() {
        return Err(AppError::invalid("这是一个目录，请提供具体文件的链接（目录整体下载暂不支持）。"));
    }
    let (clean, cred) = split_credentials(&parsed);
    let name = file_name(&clean);
    let (stem, ext) = split_name(&name);
    let ext = if ext.is_empty() { "bin".to_string() } else { ext };
    let stem = if stem.is_empty() { clean.host_str().unwrap_or("file").to_string() } else { stem };
    let (pid, pname) = if protocol == Protocol::Sftp { ("sftp", "SFTP") } else { ("ftp", "FTP") };
    let mut asset = Asset::base("file", AssetKind::File, clean.to_string(), format!("文件 {}", ext.to_uppercase()), ext);
    asset.protocol = protocol;
    if let Some(c) = cred {
        asset.headers = vec![("Authorization".into(), c)];
    }
    Ok(media(pid, pname, clean.as_str(), clean.to_string(), stem, MediaKind::File, vec![asset]))
}

/// RTMP / RTSP 等流：交给 ffmpeg 抓取，保存为 mkv（容器兼容各种编码，中途断开也不会损坏）。
fn resolve_stream(url: &str) -> AppResult<MediaInfo> {
    let parsed = Url::parse(url.trim()).map_err(|_| AppError::invalid("链接格式不正确。"))?;
    let host = parsed.host_str().unwrap_or("").to_string();
    if host.is_empty() {
        return Err(AppError::invalid("链接里没有服务器地址。"));
    }
    let (clean, cred) = split_credentials(&parsed);
    // 直播流的地址常常是 rtmp://host/app/stream_key：用最后一段作名字
    let last = clean.path_segments().and_then(|mut s| s.rfind(|x| !x.is_empty())).map(percent_decode).unwrap_or_default();
    let title = clean_filename(&format!("{host} {last}")).trim().to_string();
    let title = if title.is_empty() { host.clone() } else { title };
    let mut asset = Asset::base("video", AssetKind::Video, clean.to_string(), format!("视频流 {}", clean.scheme().to_ascii_uppercase()), "mkv");
    asset.protocol = Protocol::Stream;
    if let Some(c) = cred {
        asset.headers = vec![("Authorization".into(), c)];
    }
    Ok(media("stream", "流媒体", clean.as_str(), clean.to_string(), title, MediaKind::Video, vec![asset]))
}

/// ed2k 文件链接：链接里有文件名、大小、哈希，可能还带来源；下载地址就是链接本身。
fn resolve_ed2k(url: &str) -> AppResult<MediaInfo> {
    match crate::ed2k::link::parse(url.trim()) {
        Ok(crate::ed2k::link::Ed2kLink::File(f)) => {
            let (stem, ext) = split_name(&f.name);
            let ext = if ext.is_empty() { "bin".to_string() } else { ext };
            let stem = if stem.is_empty() { hex::encode(&f.hash[..4]) } else { stem };
            let mut asset = Asset::base("file", AssetKind::File, url.trim().to_string(), format!("文件 {}", ext.to_uppercase()), ext);
            asset.protocol = Protocol::Ed2k;
            asset.filesize = Some(f.size);
            Ok(media("ed2k", "ed2k", url.trim(), url.trim().to_string(), stem, MediaKind::File, vec![asset]))
        }
        Ok(_) => Err(AppError::invalid("这是 ed2k 服务器链接，不是文件。请在 设置 → 下载 → ed2k 里添加服务器。")),
        Err(e) => Err(AppError::invalid(format!("ed2k 链接不正确：{e}"))),
    }
}

/// 解析 FTP / SFTP / 流媒体 / ed2k 链接；其他协议返回 None。
pub fn resolve(url: &str) -> Option<AppResult<MediaInfo>> {
    match kind_of(url) {
        LinkKind::Ftp => Some(resolve_remote_file(url, Protocol::Ftp)),
        LinkKind::Sftp => Some(resolve_remote_file(url, Protocol::Sftp)),
        LinkKind::Stream => Some(resolve_stream(url)),
        LinkKind::Ed2k => Some(resolve_ed2k(url)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ftp_file_keeps_credentials_out_of_the_url() {
        let info = resolve("ftp://bob:p%40ss@files.example.com:2121/pub/%E6%B5%8B%E8%AF%95.tar.gz").unwrap().unwrap();
        let a = &info.assets[0];
        assert_eq!(a.protocol, Protocol::Ftp);
        assert_eq!(a.url, "ftp://files.example.com:2121/pub/%E6%B5%8B%E8%AF%95.tar.gz");
        assert_eq!(a.headers, vec![("Authorization".to_string(), "bob:p@ss".to_string())]);
        assert_eq!(info.title, "测试.tar");
        assert_eq!(a.ext, "gz");
        assert!(!info.source_url.contains("bob") && !info.source_url.contains("p%40ss"));
        assert_eq!(info.kind, MediaKind::File);
    }

    #[test]
    fn anonymous_ftp_and_sftp() {
        let f = resolve("ftp://ftp.example.com/a/b.iso").unwrap().unwrap();
        assert!(f.assets[0].headers.is_empty());
        let s = resolve("sftp://me@host/home/me/data.csv").unwrap().unwrap();
        assert_eq!(s.assets[0].protocol, Protocol::Sftp);
        assert_eq!(s.assets[0].headers[0].1, "me:");
        assert_eq!(s.platform, "sftp");
    }

    #[test]
    fn directories_and_empty_hosts_are_rejected() {
        assert!(resolve("ftp://h/pub/").unwrap().is_err());
        assert!(resolve("ftp://h").unwrap().is_err());
        assert!(resolve("https://a.com/x").is_none());
    }

    #[test]
    fn ed2k_file_link() {
        let h = "31D6CFE0D16AE931B73C59D7E0C089C0";
        let info = resolve(&format!("ed2k://|file|%E6%B5%8B%E8%AF%95.iso|1234|{h}|/")).unwrap().unwrap();
        let a = &info.assets[0];
        assert_eq!(a.protocol, Protocol::Ed2k);
        assert_eq!(a.filesize, Some(1234));
        assert_eq!(info.title, "测试");
        assert_eq!(a.ext, "iso");
        assert!(resolve("ed2k://|server|1.2.3.4|4661|/").unwrap().is_err());
        assert!(resolve("ed2k://|file|a|1|zz|/").unwrap().is_err());
    }

    #[test]
    fn stream_links() {
        let info = resolve("rtmp://live.example.com/app/room42").unwrap().unwrap();
        let a = &info.assets[0];
        assert_eq!(a.protocol, Protocol::Stream);
        assert_eq!(a.ext, "mkv");
        assert_eq!(a.kind, AssetKind::Video);
        assert!(info.title.contains("live.example.com") && info.title.contains("room42"));
        let r = resolve("rtsp://admin:123456@192.168.1.9/stream1").unwrap().unwrap();
        assert_eq!(r.assets[0].url, "rtsp://192.168.1.9/stream1");
        assert_eq!(r.assets[0].headers[0].1, "admin:123456");
    }
}
