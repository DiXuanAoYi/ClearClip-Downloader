//! 浏览器扩展“接管下载”：浏览器自己的下载（压缩包、安装包、文档、大文件等）交给清影，
//! 带上浏览器的 Referer、Cookie、User-Agent 和浏览器已确定的文件名。
//!
//! 和 `/api/send` 的区别：这里不经过站点解析（地址已经是文件本身），直接按普通 HTTP 文件入队。

use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GrabRequest {
    pub url: String,
    /// 发起下载的网页地址（作为 Referer）
    pub referer: String,
    /// 浏览器对这个地址的 Cookie 头（`a=1; b=2`）
    pub cookie: String,
    /// 浏览器确定的文件名（可能带目录，只取最后一段）
    pub filename: String,
    pub size: Option<u64>,
    pub mime: String,
    pub user_agent: String,
    /// 期望的文件哈希（`sha256:…`、`md5:…`，也可只给哈希值），下载完成后校验
    pub checksum: String,
    /// 备用下载地址（同一个文件）
    pub mirrors: Vec<String>,
    /// 扩展的显示名（配对确认时显示）
    pub device_name: String,
    /// 用户在右键菜单里明确点了“用清影下载”：电脑上把主窗口带到前台
    pub reveal: bool,
}

/// 文件名里去掉目录和非法字符，限制长度。
pub fn clean_filename(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let s = crate::naming::sanitize(last).trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if s.chars().count() <= 150 {
        return s;
    }
    // 过长时保留扩展名
    let (stem, ext) = split_name(&s);
    let keep = 140usize.saturating_sub(ext.chars().count());
    let stem: String = stem.chars().take(keep).collect();
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

/// 拆成（主名，扩展名）。`a.tar.gz` 这类双扩展名的主名保留 `.tar`，拼回去仍是原名；点开头的隐藏文件没有扩展名。
pub fn split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() && name.len() - i <= 12 => (name[..i].to_string(), name[i + 1..].to_ascii_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

/// 常见 MIME 类型对应的扩展名（文件名和地址里都没有扩展名时才用）。
pub fn ext_from_mime(mime: &str) -> Option<&'static str> {
    let m = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    Some(match m.as_str() {
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/x-rar-compressed" | "application/vnd.rar" => "rar",
        "application/x-7z-compressed" => "7z",
        "application/gzip" | "application/x-gzip" => "gz",
        "application/x-tar" => "tar",
        "application/x-bzip2" => "bz2",
        "application/x-xz" => "xz",
        "application/pdf" => "pdf",
        "application/vnd.android.package-archive" => "apk",
        "application/x-msdownload" | "application/vnd.microsoft.portable-executable" => "exe",
        "application/x-msi" => "msi",
        "application/x-apple-diskimage" => "dmg",
        "application/x-iso9660-image" => "iso",
        "application/x-debian-package" | "application/vnd.debian.binary-package" => "deb",
        "application/x-rpm" => "rpm",
        "application/json" => "json",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/ogg" => "ogg",
        _ => return None,
    })
}

/// 按文件开头的几个字节认出真实格式（地址里没有后缀、服务器又没说类型时用）。
pub fn sniff_ext(head: &[u8]) -> Option<&'static str> {
    let starts = |m: &[u8]| head.starts_with(m);
    if starts(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if starts(&[0x89, b'P', b'N', b'G']) {
        return Some("png");
    }
    if starts(b"GIF8") {
        return Some("gif");
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" {
        match &head[8..12] {
            b"WEBP" => return Some("webp"),
            b"WAVE" => return Some("wav"),
            _ => {}
        }
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return Some(match &head[8..12] {
            b"avif" | b"avis" => "avif",
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" => "heic",
            b"M4A " | b"M4B " => "m4a",
            b"qt  " => "mov",
            _ => "mp4",
        });
    }
    if starts(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("webm");
    }
    if starts(b"BM") && head.len() > 14 {
        return Some("bmp");
    }
    if starts(&[0, 0, 1, 0]) {
        return Some("ico");
    }
    if starts(b"ID3") || (head.len() > 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        return Some("mp3");
    }
    if starts(b"OggS") {
        return Some("ogg");
    }
    if starts(b"fLaC") {
        return Some("flac");
    }
    if starts(b"%PDF") {
        return Some("pdf");
    }
    if starts(b"PK\x03\x04") {
        return Some("zip");
    }
    let text = String::from_utf8_lossy(&head[..head.len().min(512)]).to_ascii_lowercase();
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg")) {
        return Some("svg");
    }
    None
}

fn is_media_ext(e: &str) -> bool {
    matches!(
        e,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif" | "heic" | "bmp" | "ico" | "svg" | "mp4" | "webm" | "mov" | "m4a" | "mp3" | "ogg" | "flac" | "wav"
    )
}

/// 下载完成后按文件内容纠正扩展名：后缀是 bin / 没有后缀，或者图片、音视频的后缀和真实格式对不上（如 .jpg 实际是 AVIF）时换成真实格式。
/// 返回新的目标路径；不需要改就返回 None。
pub fn corrected_path(file: &std::path::Path, wanted: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::io::Read;
    let mut head = [0u8; 600];
    let n = std::fs::File::open(file).ok()?.read(&mut head).ok()?;
    let real = sniff_ext(&head[..n])?;
    let cur = wanted.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let cur_norm = if cur == "jpeg" || cur == "jfif" { "jpg" } else { cur.as_str() };
    let wrong = cur.is_empty() || cur == "bin" || cur == "tmp" || (is_media_ext(cur_norm) && cur_norm != real && is_media_ext(real) && !(real == "zip"));
    // 容器类（mp4/mov/m4a）之间不强改，避免把正常的 .m4a 改成 .mp4
    let container = |e: &str| matches!(e, "mp4" | "mov" | "m4a");
    if !wrong || (container(cur_norm) && container(real)) {
        return None;
    }
    Some(wanted.with_extension(real))
}

pub(crate) fn url_name(url: &Url) -> String {
    let last = url.path_segments().and_then(|mut s| s.rfind(|x| !x.is_empty())).unwrap_or("");
    let decoded = percent_decode(last);
    clean_filename(&decoded)
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 备用地址：请求里给的，加上链接片段里的 `#mirror=<地址>`（可重复）；只要 http / https，去重，最多 8 个。
pub fn mirrors_for(given: &[String], url: &Url) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let from_fragment = url
        .fragment()
        .into_iter()
        .flat_map(|f| url::form_urlencoded::parse(f.as_bytes()))
        .filter(|(k, _)| k.eq_ignore_ascii_case("mirror"))
        .map(|(_, v)| v.into_owned());
    for raw in given.iter().cloned().chain(from_fragment) {
        let Ok(mut u) = Url::parse(raw.trim()) else { continue };
        if !matches!(u.scheme(), "http" | "https") {
            continue;
        }
        u.set_fragment(None);
        let m = u.to_string();
        let main = {
            let mut c = url.clone();
            c.set_fragment(None);
            c.to_string()
        };
        if m != main && !out.contains(&m) && out.len() < 8 {
            out.push(m);
        }
    }
    out
}

/// 常见的“文件”扩展名：粘贴这类 http(s) 链接就直接当文件下载，不再交给 yt-dlp 去猜。
const FILE_EXTS: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "iso", "img", "dmg", "exe", "msi", "pkg", "deb", "rpm", "apk", "ipa", "appimage", "jar", "pdf",
    "doc", "docx", "xls", "xlsx", "ppt", "pptx", "epub", "mobi", "csv", "bin", "ttf", "otf", "woff2",
];

/// 链接的路径是否以常见的文件扩展名结尾。
pub fn is_file_url(url: &str) -> bool {
    let Ok(u) = Url::parse(url.trim()) else { return false };
    if !matches!(u.scheme(), "http" | "https") {
        return false;
    }
    let name = url_name(&u).to_ascii_lowercase();
    name.rsplit_once('.').is_some_and(|(_, e)| FILE_EXTS.contains(&e))
}

/// 粘贴进来的文件链接：和接管下载一样按普通 HTTP 文件处理，链接里可以带 `#sha256=…` 之类的期望哈希。
pub fn media_from_url(url: &str) -> AppResult<MediaInfo> {
    build_media(&GrabRequest { url: url.trim().to_string(), ..Default::default() })
}

/// 把接管请求变成一个只含单个文件资源的 `MediaInfo`。
pub fn build_media(req: &GrabRequest) -> AppResult<MediaInfo> {
    let url = Url::parse(req.url.trim()).map_err(|_| AppError::invalid("下载地址格式不正确。"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AppError::invalid("只能接管 http / https 下载。"));
    }
    let host = url.host_str().unwrap_or("").to_string();
    let mut name = clean_filename(&req.filename);
    if name.is_empty() {
        name = url_name(&url);
    }
    let (mut stem, mut ext) = split_name(&name);
    if ext.is_empty() {
        ext = ext_from_mime(&req.mime).unwrap_or("bin").to_string();
    }
    if stem.is_empty() {
        stem = host.clone();
    }
    let mut clean = url.clone();
    clean.set_fragment(None);
    let clean_url = clean.to_string();
    let referer = Url::parse(req.referer.trim())
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|u| u.to_string())
        .unwrap_or_else(|| format!("{}://{host}/", url.scheme()));

    let mut asset = Asset::base("file", AssetKind::File, clean_url.clone(), format!("文件 {}", ext.to_uppercase()), ext.clone());
    asset.mirrors = mirrors_for(&req.mirrors, &url);
    asset.checksum = crate::checksum::parse(&req.checksum).or_else(|| crate::checksum::from_url(&url)).map(|c| c.spec());
    asset.filesize = req.size.filter(|s| *s > 0);
    let ua = if req.user_agent.trim().is_empty() { crate::providers::DESKTOP_UA.to_string() } else { req.user_agent.trim().chars().take(300).collect() };
    asset.headers = vec![("Referer".into(), referer), ("User-Agent".into(), ua)];
    // 浏览器的 Cookie 只用于这一次下载：放在资源请求头里，不进入清影的账号库
    let cookie: String = req.cookie.trim().chars().filter(|c| !c.is_control()).take(8000).collect();
    if !cookie.is_empty() {
        asset.headers.push(("Cookie".into(), cookie));
    }
    let id = hex::encode(&Sha256::digest(clean_url.as_bytes())[..6]);
    Ok(MediaInfo {
        platform: "browser".into(),
        platform_name: "浏览器下载".into(),
        id,
        source_url: clean_url,
        title: stem,
        author: String::new(),
        cover: None,
        duration_ms: None,
        kind: MediaKind::File,
        width: None,
        height: None,
        published_at: None,
        assets: vec![asset],
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("browser".into()),
    })
}

/// 浏览器地址里带的敏感查询参数不写进日志和收件箱：只保留“域名 + 路径”。
pub fn display_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) => format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), u.path()),
        Err(_) => url.chars().take(200).collect(),
    }
}

/// 把浏览器的下载加入队列，并在“收到的链接”里留一条记录。返回任务编号和文件名。
pub fn enqueue(app: &tauri::AppHandle, device: &str, device_name: &str, req: &GrabRequest) -> AppResult<serde_json::Value> {
    use crate::inbox::{self, Patch};
    let media = build_media(req)?;
    let title = media.title.clone();
    let inbox_id = inbox::record(app, "extension", device, device_name, &display_url(&req.url), "resolving");
    let result = crate::download::enqueue_with(app, media, &["file".to_string()], crate::download::PostOptions::default());
    match result {
        Ok(r) => {
            let ids: Vec<i64> = r.tasks.iter().map(|t| t.id).collect();
            let names: Vec<String> = r.tasks.iter().map(|t| crate::naming::display_name(&t.file_path)).collect();
            let (status, message) = if !ids.is_empty() {
                ("queued", None)
            } else if r.already_queued > 0 {
                ("done", Some("已在下载队列中".to_string()))
            } else {
                ("done", Some("之前已下载过".to_string()))
            };
            if let Some(id) = inbox_id {
                inbox::patch(app, id, Patch { status: Some(status), title: Some(title), message: Some(message), task_ids: Some(&ids), ..Default::default() });
            }
            Ok(
                serde_json::json!({"state": status, "taskIds": ids, "fileNames": names, "alreadyQueued": r.already_queued, "alreadyDownloaded": r.already_downloaded}),
            )
        }
        Err(e) => {
            if let Some(id) = inbox_id {
                inbox::patch(app, id, Patch { status: Some("failed"), message: Some(Some(e.message.clone())), ..Default::default() });
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_file_links_are_recognised() {
        assert!(is_file_url("https://x.test/dl/setup.EXE"));
        assert!(is_file_url("http://x.test/a/b.tar.gz?token=1#sha256=ab"));
        assert!(!is_file_url("https://x.test/watch?v=1"));
        assert!(!is_file_url("https://x.test/video.mp4"));
        assert!(!is_file_url("ftp://x.test/a.zip"));
    }

    #[test]
    fn expected_hash_comes_from_the_link_or_the_request_and_the_fragment_is_dropped() {
        let sum = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let a = media_from_url(&format!("https://x.test/a.zip#sha256={sum}")).unwrap();
        assert_eq!(a.assets[0].checksum.as_deref(), Some(format!("sha256:{sum}").as_str()));
        assert_eq!(a.assets[0].url, "https://x.test/a.zip");
        let b = build_media(&GrabRequest { url: "https://x.test/a.zip".into(), checksum: "MD5:900150983cd24fb0d6963f7d28e17f72".into(), ..Default::default() })
            .unwrap();
        assert_eq!(b.assets[0].checksum.as_deref(), Some("md5:900150983cd24fb0d6963f7d28e17f72"));
        assert!(media_from_url("https://x.test/a.zip").unwrap().assets[0].checksum.is_none());
    }

    #[test]
    fn mirrors_come_from_the_fragment_and_the_request() {
        let m =
            media_from_url("https://a.test/f.iso#mirror=https%3A%2F%2Fb.test%2Ff.iso&mirror=ftp%3A%2F%2Fc.test%2Ff.iso&mirror=https%3A%2F%2Fa.test%2Ff.iso")
                .unwrap();
        // ftp 不是 http(s)，和主地址相同的也不算备用
        assert_eq!(m.assets[0].mirrors, vec!["https://b.test/f.iso".to_string()]);
        let r = build_media(&GrabRequest {
            url: "https://a.test/f.iso#mirror=https%3A%2F%2Fb.test%2Ff.iso".into(),
            mirrors: vec!["https://c.test/f.iso".into(), "https://b.test/f.iso".into(), "nope".into()],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(r.assets[0].mirrors, vec!["https://c.test/f.iso".to_string(), "https://b.test/f.iso".to_string()]);
    }

    use super::*;
    use std::path::Path;

    fn req(url: &str, filename: &str) -> GrabRequest {
        GrabRequest { url: url.into(), filename: filename.into(), ..Default::default() }
    }

    #[test]
    fn filenames_are_cleaned_and_split() {
        assert_eq!(clean_filename("C:\\Users\\me\\Downloads\\a:b?.zip"), "a_b_.zip");
        assert_eq!(clean_filename("/home/me/dl/setup.exe"), "setup.exe");
        assert_eq!(clean_filename("..hidden."), "hidden");
        assert_eq!(split_name("data.tar.gz"), ("data.tar".into(), "gz".into()));
        assert_eq!(split_name("README"), ("README".into(), String::new()));
        assert_eq!(split_name(".bashrc"), (".bashrc".into(), String::new()));
        assert_eq!(split_name("a."), ("a.".into(), String::new()));
        let long = format!("{}.zip", "文".repeat(300));
        let c = clean_filename(&long);
        assert!(c.chars().count() <= 150 && c.ends_with(".zip"), "{c}");
    }

    #[test]
    fn media_from_browser_download() {
        let mut r = req("https://cdn.example.com/dl/abc?token=1", "D:\\下载\\工具包 v2.zip");
        r.referer = "https://example.com/get".into();
        r.cookie = "sid=1; theme=dark".into();
        r.size = Some(5_000_000);
        r.user_agent = "Mozilla/5.0 Test".into();
        let m = build_media(&r).unwrap();
        assert_eq!((m.platform.as_str(), m.kind), ("browser", MediaKind::File));
        assert_eq!(m.title, "工具包 v2");
        let a = &m.assets[0];
        assert_eq!((a.ext.as_str(), a.kind, a.filesize), ("zip", AssetKind::File, Some(5_000_000)));
        let h = |k: &str| a.headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(h("Referer"), Some("https://example.com/get"));
        assert_eq!(h("Cookie"), Some("sid=1; theme=dark"));
        assert_eq!(h("User-Agent"), Some("Mozilla/5.0 Test"));
        assert_eq!(m.default_asset_ids(), vec!["file".to_string()]);
    }

    #[test]
    fn name_falls_back_to_url_then_mime() {
        let m = build_media(&req("https://example.com/files/%E6%8A%A5%E5%91%8A.pdf?x=1", "")).unwrap();
        assert_eq!((m.title.as_str(), m.assets[0].ext.as_str()), ("报告", "pdf"));
        let mut r = req("https://example.com/download", "");
        r.mime = "application/zip; charset=binary".into();
        let m = build_media(&r).unwrap();
        assert_eq!((m.title.as_str(), m.assets[0].ext.as_str()), ("download", "zip"));
        let m = build_media(&req("https://example.com/", "")).unwrap();
        assert_eq!((m.title.as_str(), m.assets[0].ext.as_str()), ("example.com", "bin"));
        // 没有 Referer 时用站点首页；没有 Cookie 时不加这个头
        assert!(m.assets[0].headers.iter().any(|(k, v)| k == "Referer" && v == "https://example.com/"));
        assert!(!m.assets[0].headers.iter().any(|(k, _)| k == "Cookie"));
    }

    #[test]
    fn only_http_and_https() {
        assert!(build_media(&req("file:///etc/passwd", "a.txt")).is_err());
        assert!(build_media(&req("blob:https://example.com/1", "a.txt")).is_err());
        assert!(build_media(&req("ftp://example.com/a.zip", "a.zip")).is_err());
        assert!(build_media(&req("not a url", "")).is_err());
        let mut r = req("https://example.com/a.zip", "a.zip");
        r.cookie = "a=1\r\nX-Evil: 1".into();
        let m = build_media(&r).unwrap();
        let c = m.assets[0].headers.iter().find(|(k, _)| k == "Cookie").unwrap();
        assert!(!c.1.contains('\r') && !c.1.contains('\n'), "{:?}", c.1);
    }

    #[test]
    fn display_url_hides_query() {
        assert_eq!(display_url("https://a.com/x/y.zip?token=secret#f"), "https://a.com/x/y.zip");
    }

    #[test]
    fn sniffing_finds_the_real_format() {
        assert_eq!(sniff_ext(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("jpg"));
        assert_eq!(sniff_ext(b"\x89PNG\r\n\x1a\n"), Some("png"));
        assert_eq!(sniff_ext(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_ext(b"\x00\x00\x00\x20ftypavif\x00\x00"), Some("avif"));
        assert_eq!(sniff_ext(b"\x00\x00\x00\x20ftypisom\x00\x00"), Some("mp4"));
        assert_eq!(sniff_ext(b"<?xml version=\"1.0\"?><svg xmlns=\"x\"/>"), Some("svg"));
        assert_eq!(sniff_ext(b"GIF89a.."), Some("gif"));
        assert_eq!(sniff_ext(b"hello world"), None);
    }

    #[test]
    fn extension_is_corrected_only_when_it_is_wrong() {
        let dir = std::env::temp_dir().join(format!("cc-sniff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("a.part");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n0000").unwrap();
        assert_eq!(corrected_path(&png, Path::new("/x/pic.bin")), Some(Path::new("/x/pic.png").to_path_buf()));
        assert_eq!(corrected_path(&png, Path::new("/x/pic")), Some(Path::new("/x/pic.png").to_path_buf()));
        assert_eq!(corrected_path(&png, Path::new("/x/pic.png")), None);
        assert_eq!(corrected_path(&png, Path::new("/x/pic.jpg")), Some(Path::new("/x/pic.png").to_path_buf()), "图片后缀对不上真实格式");
        assert_eq!(corrected_path(&png, Path::new("/x/readme.txt")), None, "不是媒体后缀不乱改");
        let mp4 = dir.join("b.part");
        std::fs::write(&mp4, b"\x00\x00\x00\x20ftypM4A \x00\x00").unwrap();
        assert_eq!(corrected_path(&mp4, Path::new("/x/song.mp4")), None, "容器之间不强改");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
