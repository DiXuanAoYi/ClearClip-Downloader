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
    /// 扩展的显示名（配对确认时显示）
    pub device_name: String,
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

fn url_name(url: &Url) -> String {
    let last = url.path_segments().and_then(|mut s| s.rfind(|x| !x.is_empty())).unwrap_or("");
    let decoded = percent_decode(last);
    clean_filename(&decoded)
}

fn percent_decode(s: &str) -> String {
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
    let clean_url = url.to_string();
    let referer = Url::parse(req.referer.trim())
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|u| u.to_string())
        .unwrap_or_else(|| format!("{}://{host}/", url.scheme()));

    let mut asset = Asset::base("file", AssetKind::File, clean_url.clone(), format!("文件 {}", ext.to_uppercase()), ext.clone());
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
    use super::*;

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
}
