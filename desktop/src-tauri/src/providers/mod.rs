//! 各平台解析器。每个平台实现 [`Provider`]，新增平台时只需加一个文件并在 [`all`] 里注册。

pub mod bilibili;
pub mod custom;
pub mod douyin;
pub mod generic;
pub mod kuaishou;
pub mod listing;
pub mod live;
pub mod pixiv;
pub mod protocols;
pub mod remote;
pub mod rss;
pub mod weibo;
pub mod xiaohongshu;
pub mod ytdlp;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde::Serialize;
use url::Url;

use crate::cookies::CookieStore;
use crate::model::{AppError, AppResult, ErrorKind, MediaInfo};
use crate::net::NetManager;
use crate::settings::{ParseMode, Settings};

pub const DESKTOP_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36";
pub const MOBILE_UA: &str =
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";

/// 解析时共享的上下文。
pub struct Ctx<'a> {
    /// 没有网络管理器时（测试）使用的默认客户端
    pub client: &'a reqwest::Client,
    pub settings: &'a Settings,
    pub cookies: &'a CookieStore,
    /// 按网站分流的客户端；为 None 时一律用 `client`
    pub net: Option<&'a NetManager>,
    /// 开启“录制样本”时保存原始响应的目录
    pub samples: Option<PathBuf>,
    /// 指定使用某个账号的 Cookie（检查账号状态时），否则用网站的默认账号
    pub account: Option<String>,
    /// yt-dlp 位置；为 None 时不使用 yt-dlp（未安装或已在设置中关闭）
    pub ytdlp: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
}

impl<'a> Ctx<'a> {
    pub fn new(client: &'a reqwest::Client, settings: &'a Settings, cookies: &'a CookieStore) -> Self {
        Ctx { client, settings, cookies, net: None, samples: None, account: None, ytdlp: None, ffmpeg: None }
    }

    /// 按网络分流规则选择访问 `url` 的客户端。
    pub fn http(&self, url: &str) -> reqwest::Client {
        self.net.and_then(|n| n.clients_for(&self.settings.network, url).ok()).map(|c| c.api).unwrap_or_else(|| self.client.clone())
    }

    pub fn get(&self, url: impl AsRef<str>) -> reqwest::RequestBuilder {
        let url = url.as_ref();
        self.http(url).get(url)
    }

    pub fn post(&self, url: impl AsRef<str>) -> reqwest::RequestBuilder {
        let url = url.as_ref();
        self.http(url).post(url)
    }

    /// 请求 `url` 时应带的 Cookie（按域名匹配当前默认账号）。
    pub fn cookie(&self, url: &str) -> Option<String> {
        match &self.account {
            Some(id) => self.cookies.header_for_account(id, url),
            None => self.cookies.header_for(url),
        }
    }

    /// 保存一份原始响应作为调试样本（去掉 Cookie、令牌等敏感片段）。
    pub fn record(&self, platform: &str, label: &str, url: &str, body: &str) {
        if let Some(dir) = &self.samples {
            crate::diagnostics::save_sample(dir, platform, label, url, body);
        }
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// 判断链接是否属于本平台。
    fn matches(&self, url: &Url) -> bool;
    /// 下载该平台资源时使用的 Referer。
    fn referer(&self) -> &'static str;
    /// 内置登录窗口打开的地址，也用于读取登录后的 Cookie。
    fn login_url(&self) -> &'static str;
    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo>;

    /// 检查当前账号的登录状态；平台不支持时返回 None。
    async fn account_status(&self, _ctx: &Ctx<'_>) -> AppResult<Option<AccountStatus>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub logged_in: bool,
    pub user_name: Option<String>,
    /// 会员等级 / 状态描述，如“大会员”
    pub vip: Option<String>,
}

pub fn all() -> &'static [&'static dyn Provider] {
    static ALL: [&dyn Provider; 6] = [&douyin::Douyin, &kuaishou::Kuaishou, &xiaohongshu::Xiaohongshu, &bilibili::Bilibili, &weibo::Weibo, &pixiv::Pixiv];
    &ALL
}

pub fn by_id(id: &str) -> Option<&'static dyn Provider> {
    all().iter().copied().find(|p| p.id() == id)
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(MOBILE_UA)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .gzip(true)
        .build()
        .expect("failed to build http client")
}

static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[A-Za-z0-9\-._~:/?#\[\]@!$&'*+,;=%]+"#).unwrap());

/// 从整段分享文案里提取所有链接（去掉尾部标点）。
pub fn extract_urls(text: &str) -> Vec<String> {
    URL_RE.find_iter(text).map(|m| m.as_str().trim_end_matches(|c: char| ".,;:!?'\")]".contains(c)).to_string()).filter(|u| Url::parse(u).is_ok()).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedLink {
    pub url: String,
    pub platform: String,
    pub platform_name: String,
}

/// 找出文本里所有受支持平台的链接，按出现顺序去重。
pub fn detect_links(text: &str) -> Vec<DetectedLink> {
    let mut out: Vec<DetectedLink> = Vec::new();
    for raw in extract_urls(text) {
        let Ok(parsed) = Url::parse(&raw) else { continue };
        if let Some(p) = all().iter().find(|p| p.matches(&parsed)) {
            if !out.iter().any(|d| d.url == raw) {
                out.push(DetectedLink { url: raw, platform: p.id().into(), platform_name: p.name().into() });
            }
        }
    }
    for raw in crate::links::extract_links(text) {
        use crate::links::LinkKind;
        let (platform, name) = match crate::links::kind_of(&raw) {
            LinkKind::Ftp => ("ftp", "FTP"),
            LinkKind::Sftp => ("sftp", "SFTP"),
            LinkKind::Stream => ("stream", "流媒体"),
            LinkKind::Ed2k => ("ed2k", "ed2k"),
            LinkKind::Magnet => ("magnet", "磁力链接"),
            LinkKind::Wrapped => ("wrapped", "迅雷 / 快车链接"),
            _ => continue,
        };
        // 专用链接必须能还原才算
        if crate::links::kind_of(&raw) == LinkKind::Wrapped && crate::links::unwrap_link(&raw).is_none() {
            continue;
        }
        if !out.iter().any(|d| d.url == raw) {
            out.push(DetectedLink { url: raw, platform: platform.into(), platform_name: name.into() });
        }
    }
    out
}

/// 剪贴板 / 手机发送用的链接识别：内置平台，加上设置里的额外网站（或全部网址）。
pub fn detect_links_with(text: &str, settings: &Settings) -> Vec<DetectedLink> {
    let mut out = detect_links(text);
    for raw in extract_urls(text) {
        if out.iter().any(|d| d.url == raw) {
            continue;
        }
        let Some(host) = Url::parse(&raw).ok().and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase())) else { continue };
        let listed = settings.clipboard_domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")));
        if settings.clipboard_all_sites || listed {
            let site = crate::cookies::registrable_domain(&host);
            out.push(DetectedLink { url: raw, platform: site.clone(), platform_name: site });
        }
    }
    out
}

/// 解析一段文本中的第一个链接，按设置决定本地 / 远程策略。
pub async fn resolve_text(ctx: &Ctx<'_>, text: &str) -> AppResult<MediaInfo> {
    let urls = extract_urls(text);
    let Some(first) = urls.first() else {
        // 没有网页链接：看看有没有 FTP、磁力、ed2k、迅雷专用链接等
        if let Some(link) = crate::links::extract_links(text).into_iter().next() {
            return resolve_url(ctx, &link).await;
        }
        return Err(AppError::invalid("没有找到链接。请粘贴分享文案或链接。"));
    };
    // 优先选受支持平台的链接，避免文案中混有其他网址。
    let url = urls.iter().find(|u| Url::parse(u).map(|p| all().iter().any(|pr| pr.matches(&p))).unwrap_or(false)).unwrap_or(first).clone();
    resolve_url(ctx, &url).await
}

/// 远程 API 只支持这些平台。
pub const REMOTE_PLATFORMS: &[&str] = &["douyin", "kuaishou"];

/// 远程 API 不支持的平台始终走本地解析；无法识别平台的链接交给远程接口判断。
pub fn remote_allowed(provider: Option<&dyn Provider>) -> bool {
    provider.map_or(true, |p| REMOTE_PLATFORMS.contains(&p.id()))
}

pub async fn resolve_url(ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
    // 迅雷 / 快车 / QQ 旋风的专用链接先还原成原始地址
    let unwrapped;
    let url = if crate::links::kind_of(url) == crate::links::LinkKind::Wrapped {
        unwrapped = crate::links::unwrap_link(url).ok_or_else(|| AppError::invalid("这个专用下载链接无法还原，请确认链接完整。"))?;
        unwrapped.as_str()
    } else {
        url
    };
    if let Some(r) = protocols::resolve(url) {
        return r;
    }
    let settings = ctx.settings;
    let has_remote = !settings.remote_endpoint.is_empty();
    let parsed = Url::parse(url).map_err(|_| AppError::invalid("链接格式不正确。"))?;
    let provider = all().iter().find(|p| p.matches(&parsed));
    let remote_ok = remote_allowed(provider.copied());

    if settings.parse_mode == ParseMode::Remote && remote_ok {
        if !has_remote {
            return Err(AppError::invalid("当前为远程解析模式，但没有填写远程 API 地址。请在设置中填写，或改为本地解析。"));
        }
        return remote::resolve(ctx, url).await;
    }

    if let (Some(net), Some(p)) = (ctx.net, provider) {
        net.wait_turn(p.id(), std::time::Duration::from_millis(settings.site_request_interval_ms)).await;
    }
    let local = match provider {
        Some(p) => match p.resolve(ctx, url).await {
            // 内置解析器失效时让 yt-dlp 试一次（它也支持 B站、微博等大部分平台）
            Err(e) if e.kind == ErrorKind::ParserBroken && ctx.ytdlp.is_some() => {
                ytdlp::resolve(ctx, url).await.map_err(|ye| AppError::new(e.kind, format!("{e}；yt-dlp 也失败：{ye}")))
            }
            other => other,
        },
        None => resolve_generic(ctx, url).await,
    };

    match local {
        Ok(mut info) => {
            crate::quality::sort_videos(&mut info, settings);
            Ok(info)
        }
        Err(e) if settings.parse_mode != ParseMode::Local && has_remote && remote_ok && provider.is_some() => {
            remote::resolve(ctx, url).await.map_err(|re| AppError::new(e.kind, format!("本地解析失败：{e}；远程解析也失败：{re}")))
        }
        Err(e) => Err(e),
    }
}

/// 没有内置解析器的网站：先交给 yt-dlp，再尝试网页嗅探。
async fn resolve_generic(ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
    // 用户自己写的站点规则优先
    if let Some(rule) = custom::find(&ctx.settings.custom_sites, url) {
        return custom::resolve(ctx, rule, url).await;
    }
    // 直接的音视频文件：不用问 yt-dlp
    if generic::is_direct_media(url) {
        return generic::resolve(ctx, url).await;
    }
    // 看起来像播客 / RSS 订阅源：先按订阅源解析，不是的话再走后面的流程
    if rss::looks_like_feed_url(url) {
        match rss::resolve(ctx, url).await {
            Ok(info) => return Ok(info),
            Err(e) => log::info!("not a usable feed ({e}), trying other resolvers"),
        }
    }
    let mut first_err: Option<AppError> = None;
    if ctx.ytdlp.is_some() {
        if let Some(net) = ctx.net {
            let host = Url::parse(url).ok().and_then(|u| u.host_str().map(crate::cookies::registrable_domain)).unwrap_or_default();
            net.wait_turn(&host, Duration::from_millis(ctx.settings.site_request_interval_ms)).await;
        }
        match ytdlp::resolve(ctx, url).await {
            Ok(info) => return Ok(info),
            // 需要登录、地区限制等明确的错误直接返回，嗅探也不会成功
            Err(e) if !matches!(e.kind, ErrorKind::Unsupported | ErrorKind::NotFound | ErrorKind::Other) => return Err(e),
            Err(e) => first_err = Some(e),
        }
    }
    if ctx.settings.generic_sniffer {
        match generic::resolve(ctx, url).await {
            Ok(info) => return Ok(info),
            Err(e) if first_err.is_none() => first_err = Some(e),
            Err(_) => {}
        }
    }
    Err(match first_err {
        Some(e) if ctx.ytdlp.is_some() => e,
        _ if ctx.ytdlp.is_none() => AppError::new(
            ErrorKind::NeedUpdate,
            "内置解析器不支持这个网站。在“设置 → 组件”中安装 yt-dlp 后可支持上千个视频网站（YouTube、Pornhub、Twitter/X、TikTok 等）。",
        ),
        Some(e) => e,
        None => AppError::unsupported("暂不支持这个网站。"),
    })
}

/// 下载某个平台资源时使用的 Referer。
pub fn referer_for(platform: &str) -> &'static str {
    by_id(platform).map(|p| p.referer()).unwrap_or("")
}

// ---------- 供各平台复用的小工具 ----------

/// 从 HTML 中截取 `marker = {...}` 形式的 JSON 对象（按括号配对，忽略字符串内的括号）。
pub(crate) fn extract_json_after(html: &str, marker: &str) -> Option<serde_json::Value> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let brace = rest.find('{')?;
    let body = &rest[brace..];
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in body.char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let raw = &body[..=i];
                    // 有的页面会把 undefined 写进对象里
                    return serde_json::from_str(raw).ok().or_else(|| serde_json::from_str(&raw.replace(":undefined", ":null")).ok());
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn str_at<'a>(v: &'a serde_json::Value, ptr: &str) -> Option<&'a str> {
    v.pointer(ptr).and_then(|x| x.as_str()).filter(|s| !s.is_empty())
}

pub(crate) fn u64_at(v: &serde_json::Value, ptr: &str) -> Option<u64> {
    v.pointer(ptr).and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)).or_else(|| x.as_str().and_then(|s| s.parse().ok())))
}

pub(crate) fn u32_at(v: &serde_json::Value, ptr: &str) -> Option<u32> {
    u64_at(v, ptr).and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_url_from_douyin_share_text() {
        let text = "7.43 复制打开抖音，看看【山野厨房的作品】秋天第一锅板栗焖鸡 https://v.douyin.com/ehHpu7V/ a@B.Ok 02/18 :0pm";
        assert_eq!(extract_urls(text), vec!["https://v.douyin.com/ehHpu7V/"]);
    }

    #[test]
    fn strips_trailing_punctuation() {
        assert_eq!(extract_urls("看这个：https://v.kuaishou.com/abc12, 不错"), vec!["https://v.kuaishou.com/abc12"]);
    }

    #[test]
    fn detects_supported_platforms_only() {
        let text = "https://example.com/x https://v.douyin.com/ehHpu7V/ https://www.kuaishou.com/short-video/3xabc https://v.douyin.com/ehHpu7V/";
        let links = detect_links(text);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].platform, "douyin");
        assert_eq!(links[1].platform, "kuaishou");
    }

    #[test]
    fn clipboard_whitelist() {
        let text = "https://www.youtube.com/watch?v=1 https://example.com/a https://v.douyin.com/x/";
        let s = Settings::default();
        let links: Vec<String> = detect_links_with(text, &s).into_iter().map(|d| d.platform).collect();
        assert_eq!(links, vec!["douyin", "youtube.com"]);
        let s = Settings { clipboard_all_sites: true, ..Settings::default() };
        assert_eq!(detect_links_with(text, &s).len(), 3);
    }

    #[test]
    fn extract_json_handles_braces_in_strings() {
        let html = r#"<script>window.X = {"a":"}{","b":{"c":1}};</script>"#;
        let v = extract_json_after(html, "window.X").unwrap();
        assert_eq!(v["a"], "}{");
        assert_eq!(v["b"]["c"], 1);
    }

    #[tokio::test]
    async fn remote_mode_without_endpoint_is_an_error() {
        let s = Settings { parse_mode: ParseMode::Remote, ..Settings::default() };
        let (c, store) = (build_client(), CookieStore::in_memory());
        let err = resolve_text(&Ctx::new(&c, &s, &store), "https://v.douyin.com/x/").await.unwrap_err();
        assert!(err.to_string().contains("远程 API"));
    }

    #[test]
    fn remote_api_only_for_legacy_platforms() {
        assert!(remote_allowed(by_id("douyin")));
        assert!(remote_allowed(by_id("kuaishou")));
        assert!(!remote_allowed(by_id("bilibili")));
        assert!(!remote_allowed(by_id("xiaohongshu")));
        assert!(!remote_allowed(by_id("weibo")));
        assert!(remote_allowed(None));
    }

    #[tokio::test]
    async fn text_without_url_is_an_error() {
        let (c, s, store) = (build_client(), Settings::default(), CookieStore::in_memory());
        let err = resolve_text(&Ctx::new(&c, &s, &store), "没有链接").await.unwrap_err();
        assert!(err.to_string().contains("没有找到链接"));
    }
}
