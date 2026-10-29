//! 统一错误类型。每个错误带一个分类，前端据此给出下一步操作（添加 Cookie、检查代理、更新组件等）。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// 需要登录 / Cookie 失效 / 年龄限制 / 会员专享
    NeedLogin,
    /// 地区限制或网站在当前网络下不可达
    GeoBlocked,
    /// 作品不存在或已删除
    NotFound,
    /// 内容加密（DRM、加密短剧等），不支持
    Encrypted,
    /// 请求过于频繁 / 触发风控
    RateLimited,
    /// 网络错误（超时、连接失败）
    Network,
    /// 网站结构变化，解析器需要更新
    ParserBroken,
    /// 不支持的平台或链接
    Unsupported,
    /// 外部组件需要安装或更新（yt-dlp / ffmpeg）
    NeedUpdate,
    /// 磁盘空间不足或文件读写失败
    Disk,
    /// 用户输入或设置有误
    Invalid,
    Other,
}

#[derive(Debug, Clone)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        AppError { kind, message: message.into() }
    }

    pub fn msg(message: impl Into<String>) -> Self {
        AppError::new(ErrorKind::Other, message)
    }

    pub fn need_login(message: impl Into<String>) -> Self {
        AppError::new(ErrorKind::NeedLogin, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        AppError::new(ErrorKind::NotFound, message)
    }

    pub fn parser(message: impl Into<String>) -> Self {
        AppError::new(ErrorKind::ParserBroken, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        AppError::new(ErrorKind::Unsupported, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        AppError::new(ErrorKind::Invalid, message)
    }

    /// 在原消息前加上下文，保留分类。
    pub fn context(self, prefix: impl std::fmt::Display) -> Self {
        AppError { kind: self.kind, message: format!("{prefix}{}", self.message) }
    }

    /// 在原消息后追加说明，保留分类。
    pub fn context_suffix(self, suffix: impl std::fmt::Display) -> Self {
        AppError { kind: self.kind, message: format!("{}{suffix}", self.message) }
    }

    /// 根据 HTTP 状态码推断分类。
    pub fn from_status(status: u16, what: &str) -> Self {
        let kind = match status {
            401 | 403 => ErrorKind::NeedLogin,
            404 | 410 => ErrorKind::NotFound,
            412 | 429 => ErrorKind::RateLimited,
            451 => ErrorKind::GeoBlocked,
            _ => ErrorKind::Network,
        };
        AppError::new(kind, format!("{what}返回 {status}"))
    }

    /// 根据报错文本推断分类，用于远程接口和 yt-dlp 等外部来源的错误。
    pub fn classify(message: impl Into<String>) -> Self {
        let message = message.into();
        let m = message.to_lowercase();
        let has = |keys: &[&str]| keys.iter().any(|k| m.contains(k));
        let kind = if has(&[
            "sign in",
            "login",
            "log in",
            "cookies",
            "登录",
            "confirm your age",
            "age-restricted",
            "age restricted",
            "members-only",
            "members only",
            "premium",
            "private video",
            "会员",
        ]) {
            ErrorKind::NeedLogin
        } else if has(&["drm", "encrypted", "加密"]) {
            ErrorKind::Encrypted
        } else if has(&["in your country", "geo-restrict", "geo restrict", "region", "地区", "blocked in"]) {
            ErrorKind::GeoBlocked
        } else if has(&["429", "too many requests", "rate limit", "rate-limit", "频繁", "风控"]) {
            ErrorKind::RateLimited
        } else if has(&["404", "not found", "deleted", "removed", "不存在", "已删除", "unavailable"]) {
            ErrorKind::NotFound
        } else if has(&["unsupported url", "暂不支持", "no suitable extractor"]) {
            ErrorKind::Unsupported
        } else if has(&["timed out", "timeout", "connection", "network", "网络", "dns"]) {
            ErrorKind::Network
        } else if has(&["unable to extract", "page structure", "页面结构", "keyerror", "jsondecodeerror"]) {
            ErrorKind::ParserBroken
        } else {
            ErrorKind::Other
        };
        AppError { kind, message }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", &self.kind)?;
        st.serialize_field("message", &self.message)?;
        st.end()
    }
}

/// reqwest 的顶层错误只有“error sending request”，沿着错误链找出真正原因并给出说明。
pub fn describe_reqwest(e: &reqwest::Error) -> String {
    let mut chain: Vec<String> = vec![];
    let mut cur: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(err) = cur {
        chain.push(err.to_string());
        cur = err.source();
    }
    let all = chain.join(" | ").to_lowercase();
    if all.contains("certificate") || all.contains("unknownissuer") {
        return "HTTPS 证书校验失败。可能是代理软件、公司网络或防火墙拦截了加密连接；请检查代理设置，或把代理的根证书安装到系统中。".into();
    }
    if e.is_timeout() || all.contains("timed out") {
        return "连接超时".into();
    }
    if all.contains("dns") || all.contains("failed to lookup") || all.contains("name or service not known") {
        return "无法解析域名，请检查网络连接或 DNS".into();
    }
    if all.contains("connection refused") {
        return "连接被拒绝（如果设置了代理，请确认代理软件正在运行）".into();
    }
    if all.contains("proxy") {
        return format!("代理连接失败：{}", chain.last().cloned().unwrap_or_default());
    }
    match chain.last() {
        Some(root) => format!("{}（{root}）", e.without_url_ref()),
        None => e.without_url_ref().to_string(),
    }
}

trait WithoutUrlRef {
    fn without_url_ref(&self) -> String;
}

impl WithoutUrlRef for reqwest::Error {
    fn without_url_ref(&self) -> String {
        let s = self.to_string();
        match self.url() {
            Some(u) => s.replace(&format!(" ({u})"), "").replace(u.as_str(), ""),
            None => s,
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(status) = e.status() {
            return AppError::from_status(status.as_u16(), "服务器");
        }
        let kind = if e.is_decode() { ErrorKind::ParserBroken } else { ErrorKind::Network };
        // 不带 URL，避免把带签名参数的地址写进界面和日志
        AppError::new(kind, format!("网络请求失败：{}", describe_reqwest(&e)))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::parser(format!("数据解析失败：{e}"))
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        let kind = if e.raw_os_error() == Some(28) || e.raw_os_error() == Some(112) { ErrorKind::Disk } else { ErrorKind::Other };
        AppError::new(kind, format!("文件读写失败：{e}"))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::msg(format!("数据库错误：{e}"))
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::msg(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_messages() {
        assert_eq!(AppError::classify("ERROR: Sign in to confirm your age").kind, ErrorKind::NeedLogin);
        assert_eq!(AppError::classify("This video is DRM protected").kind, ErrorKind::Encrypted);
        assert_eq!(AppError::classify("The uploader has not made this video available in your country").kind, ErrorKind::GeoBlocked);
        assert_eq!(AppError::classify("HTTP Error 429: Too Many Requests").kind, ErrorKind::RateLimited);
        assert_eq!(AppError::classify("Video has been removed").kind, ErrorKind::NotFound);
        assert_eq!(AppError::classify("Unsupported URL: https://x").kind, ErrorKind::Unsupported);
        assert_eq!(AppError::classify("something odd").kind, ErrorKind::Other);
    }

    #[test]
    fn status_mapping_and_serialization() {
        assert_eq!(AppError::from_status(403, "x").kind, ErrorKind::NeedLogin);
        assert_eq!(AppError::from_status(404, "x").kind, ErrorKind::NotFound);
        let json = serde_json::to_string(&AppError::need_login("请登录")).unwrap();
        assert_eq!(json, r#"{"kind":"need_login","message":"请登录"}"#);
    }

    #[test]
    fn context_keeps_kind() {
        let e = AppError::need_login("Cookie 失效").context("B站：");
        assert_eq!(e.kind, ErrorKind::NeedLogin);
        assert_eq!(e.message, "B站：Cookie 失效");
    }
}
