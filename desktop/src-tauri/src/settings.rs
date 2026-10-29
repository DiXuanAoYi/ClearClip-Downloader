use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParseMode {
    /// 只用本地解析。
    Local,
    /// 只用远程 API。
    Remote,
    /// 先本地解析，失败后改用远程接口。
    #[default]
    LocalThenRemote,
}

/// 网络出口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Route {
    Direct,
    /// 系统代理（Windows / macOS 系统设置，或环境变量）
    #[default]
    System,
    /// 自定义代理，内容为代理 ID
    Proxy(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRule {
    /// 域名后缀，如 `youtube.com`（同时匹配其子域名）
    pub pattern: String,
    pub route: Route,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyDef {
    pub id: String,
    pub name: String,
    /// `http://127.0.0.1:7890`、`socks5://127.0.0.1:1080` 等
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub default_route: Route,
    pub rules: Vec<RouteRule>,
    pub proxies: Vec<ProxyDef>,
}

/// 国内平台默认直连（走海外代理时常被拦截或返回不同内容）。
pub const DIRECT_DOMAINS: &[&str] = &[
    "douyin.com",
    "iesdouyin.com",
    "douyinvod.com",
    "douyinpic.com",
    "snssdk.com",
    "amemv.com",
    "kuaishou.com",
    "chenzhongtech.com",
    "gifshow.com",
    "kwaicdn.com",
    "yximgs.com",
    "xiaohongshu.com",
    "xhscdn.com",
    "xhslink.com",
    "bilibili.com",
    "bilivideo.com",
    "bilivideo.cn",
    "hdslb.com",
    "b23.tv",
    "weibo.com",
    "weibo.cn",
    "sinaimg.cn",
    "weibocdn.com",
    "t.cn",
    "douyu.com",
    "douyucdn.cn",
    "huya.com",
    "iqiyi.com",
    "youku.com",
    "acfun.cn",
    "ximalaya.com",
];

/// 海外网站默认走系统代理。
pub const PROXY_DOMAINS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "googlevideo.com",
    "ytimg.com",
    "google.com",
    "pornhub.com",
    "phncdn.com",
    "pixiv.net",
    "pximg.net",
    "twitter.com",
    "x.com",
    "twimg.com",
    "instagram.com",
    "cdninstagram.com",
    "facebook.com",
    "fbcdn.net",
    "twitch.tv",
    "ttvnw.net",
    "tiktok.com",
    "vimeo.com",
    "dailymotion.com",
    "nicovideo.jp",
    "soundcloud.com",
];

impl Default for NetworkSettings {
    fn default() -> Self {
        let rules = DIRECT_DOMAINS
            .iter()
            .map(|d| RouteRule { pattern: d.to_string(), route: Route::Direct })
            .chain(PROXY_DOMAINS.iter().map(|d| RouteRule { pattern: d.to_string(), route: Route::System }))
            .collect();
        NetworkSettings { default_route: Route::System, rules, proxies: vec![] }
    }
}

impl NetworkSettings {
    /// 按最长后缀匹配找出某个主机使用的出口。
    pub fn route_for_host(&self, host: &str) -> Route {
        let host = host.to_ascii_lowercase();
        self.rules
            .iter()
            .filter(|r| {
                let p = r.pattern.trim().trim_start_matches('.').to_ascii_lowercase();
                !p.is_empty() && (host == p || host.ends_with(&format!(".{p}")))
            })
            .max_by_key(|r| r.pattern.len())
            .map(|r| r.route.clone())
            .unwrap_or_else(|| self.default_route.clone())
    }

    pub fn proxy_url(&self, id: &str) -> Option<&str> {
        self.proxies.iter().find(|p| p.id == id).map(|p| p.url.as_str())
    }
}

/// 默认选择哪种清晰度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QualityPreset {
    /// 最高画质
    #[default]
    Best,
    /// 不超过 1080P 的最高画质
    Max1080,
    /// 省空间：不超过 720P、码率最低
    Small,
    /// 只要音频
    Audio,
}

/// 目标文件已存在时的处理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ConflictPolicy {
    /// 在文件名后加 (1)、(2)…
    #[default]
    Rename,
    /// 跳过，不下载
    Skip,
    /// 覆盖原文件
    Overwrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: String,
    pub subfolder_by_platform: bool,
    /// 可用变量：{author} {title} {date} {id} {platform}
    pub filename_template: String,
    pub concurrency: usize,
    pub skip_existing: bool,
    pub watch_clipboard: bool,
    pub auto_download: bool,
    pub notify_on_complete: bool,
    pub parse_mode: ParseMode,
    pub remote_endpoint: String,
    /// 旧版：平台 → Cookie 字符串。启动时迁移到加密的 Cookie 存储后清空。
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub cookies: HashMap<String, String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub cookie_updated_at: HashMap<String, i64>,
    /// `system` / `dark` / `light`
    pub theme: String,
    pub close_to_tray: bool,
    pub shortcut: String,
    pub check_update: bool,
    pub disclaimer_accepted: bool,
    /// 启动时自动继续未完成的任务（否则恢复为“已暂停”）
    pub auto_resume: bool,
    /// 网络错误时的自动重试次数
    pub max_retries: u32,
    /// 第一次重试前的等待秒数，之后每次翻倍
    pub retry_delay_secs: u64,
    /// 取消任务时保留已下载的部分
    pub keep_part_on_cancel: bool,
    /// 保存解析时的原始响应，用于排查问题
    pub record_samples: bool,
    pub network: NetworkSettings,
    /// 大文件分段并行下载的段数（1 表示不分段）
    pub segments: usize,
    /// 文件大于该值（MB）时才分段
    pub segment_min_mb: u64,
    /// 全局限速（KB/s），0 表示不限
    pub speed_limit_kbps: u64,
    /// 每个网站同时下载的任务数
    pub per_site_concurrency: usize,
    /// 解析时对同一网站两次请求的最小间隔（毫秒）
    pub site_request_interval_ms: u64,
    /// 临时文件目录，留空表示与保存位置相同
    pub temp_dir: String,
    pub conflict_policy: ConflictPolicy,
    /// 下载前要求保留的剩余磁盘空间（MB）
    pub disk_reserve_mb: u64,
    pub quality_preset: QualityPreset,
    /// 同等清晰度下优先 H.264（兼容性最好，AV1 / VP9 很多设备播不了）
    pub prefer_h264: bool,
    /// 合并音视频时的封装格式：mp4 / mkv
    pub merge_container: String,
    /// 只下载音频时的格式：mp3 / m4a
    pub audio_format: String,
    /// m3u8 分片并发数
    pub hls_concurrency: usize,
    /// 跳过与正片来源明显不同的 m3u8 分片（常见于插播广告）
    pub hls_skip_ads: bool,
    /// 剧集的命名模板；变量：{series} {episode} {season} {index} {title}
    pub series_template: String,
    /// 内置平台解析失败或不支持时使用 yt-dlp
    pub use_ytdlp: bool,
    /// yt-dlp 也不支持时，尝试在网页里查找视频地址
    pub generic_sniffer: bool,
    /// 下载组件时使用的 GitHub 镜像前缀，如 `https://ghfast.top/`
    pub component_mirrors: Vec<String>,
    /// 下载 ffmpeg 时用的版本：lite 精简版（默认）/ full 完整版（带 x264、x265、vidstab、zimg）
    pub ffmpeg_edition: String,
    /// 剪贴板监听是否识别所有网址；关闭时只识别内置平台和 `clipboard_domains`
    pub clipboard_all_sites: bool,
    /// 剪贴板监听额外识别的网站（域名后缀）
    pub clipboard_domains: Vec<String>,
    /// 手机发链接到电脑
    pub phone: PhoneSettings,
    /// 把标题、作者、封面写入视频 / 音频文件
    pub embed_metadata: bool,
    /// 保存作品信息 JSON（标题、简介、原链接等）
    pub write_info_json: bool,
    /// 生成 Jellyfin / Plex 能识别的 NFO 文件
    pub write_nfo: bool,
    /// 全部下载完成后打开下载文件夹
    pub open_folder_on_done: bool,
    /// 每个任务完成后运行的命令（默认关闭）。文件路径通过环境变量 CLEARCLIP_FILE 传入
    pub post_script_enabled: bool,
    pub post_script: String,
    /// 有下载任务时阻止系统休眠
    pub prevent_sleep: bool,
    /// 订阅与追更（默认关闭，首次开启时说明用途和风险）
    pub subscriptions_enabled: bool,
    /// 开机自动启动（在托盘运行，用于订阅检查）
    pub launch_at_login: bool,
    /// 同时录制的直播间数量上限
    pub live_max_recordings: usize,
    /// 自动删除多少天前的直播录像（0 表示不删除）
    pub live_cleanup_days: u32,
    /// 收到的链接（收件箱）
    pub inbox: InboxSettings,
    /// 字幕偏好语言，靠前的优先（`zh` 同时匹配简体、繁体等变体）
    pub subtitle_langs: Vec<String>,
    /// 解析时包含自动生成和自动翻译的字幕（YouTube）
    pub subtitle_auto: bool,
    /// 自动下载时的字幕处理：off 不下载 / file 单独保存字幕文件 / embed 内嵌到视频 / burn 烧录进画面
    pub subtitle_mode: String,
    /// VTT 字幕转成 SRT（播放器和剪辑软件通用）
    pub subtitle_convert: bool,
    /// B站弹幕 XML 转成 ASS（播放器可直接显示滚动弹幕）
    pub danmaku_ass: bool,
    /// 下载完成后用 ffmpeg 检查文件能否正常读取，损坏时自动重新下载一次
    pub verify_downloads: bool,
    /// 媒体库：回收站、整理规则
    pub library: LibrarySettings,
    /// 弹幕转 ASS 时的样式
    pub danmaku: DanmakuStyle,
    /// 默认选中的视频格式预估超过这个大小（MB）时，改选不超过的最高清晰度；0 表示不限
    pub max_size_mb: u64,
    /// 下载失败（网络、文件损坏等）时自动改用更低一档的清晰度重新下载
    pub auto_downgrade: bool,
    /// AI：语音转文字、字幕翻译、摘要与章节
    pub ai: AiSettings,
    /// 自定义站点规则：用正则从网页源码里找视频地址
    pub custom_sites: Vec<SiteRule>,
    /// 把事件推送到手机 / 聊天工具
    pub notify: NotifySettings,
    /// 下载完成后自动上传 / 复制
    pub upload: UploadSettings,
    /// 下载完成后的自动规则
    pub rules: Vec<AutoRule>,
    /// 限速计划：不同时段用不同的限速
    pub speed_schedule: Vec<SpeedWindow>,
    /// 省流量模式（按流量计费的网络）：一次只下一个任务、默认选低清晰度、订阅不自动下载、不自动上传和 AI 处理
    pub metered_mode: bool,
    /// 安全与隐私
    pub security: SecuritySettings,
    /// 已完成首次使用引导（升级前就在使用的用户不再弹出）
    pub onboarded: bool,
    /// 界面语言：zh / en / ja
    pub language: String,
    /// 悬浮拖拽窗：把链接拖到桌面上的小圆标就能下载
    pub float_ball: bool,
}

/// 限速计划里的一个时段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SpeedWindow {
    /// 星期几：1 = 周一 … 7 = 周日；为空表示每天
    pub days: Vec<u8>,
    pub start: String,
    pub end: String,
    /// 这个时段的限速（KB/s），0 表示不限速
    pub limit_kbps: u64,
}

impl Default for SpeedWindow {
    fn default() -> Self {
        SpeedWindow { days: vec![], start: "09:00".into(), end: "18:00".into(), limit_kbps: 500 }
    }
}

impl SpeedWindow {
    pub fn contains(&self, weekday: u8, minute: u32) -> bool {
        crate::live::in_schedule(&[crate::live::TimeWindow { days: self.days.clone(), start: self.start.clone(), end: self.end.clone() }], weekday, minute)
    }
}

/// 计划里此刻生效的限速（KB/s，0 表示不限）；不在任何时段里返回 None（用全局限速）。
pub fn schedule_limit(windows: &[SpeedWindow], weekday: u8, minute: u32) -> Option<u64> {
    windows.iter().find(|w| w.contains(weekday, minute)).map(|w| w.limit_kbps)
}

/// 自动规则：下载完成后，满足条件就执行动作。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub when: RuleWhen,
    pub then: RuleThen,
}

/// 条件：留空的不限制，填了的必须全部满足。文字条件用 `|` 分隔多个关键词（满足任意一个），以 `re:` 开头表示正则。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RuleWhen {
    /// 平台 ID（douyin / bilibili / …）或平台名称
    pub platform: String,
    pub author: String,
    pub title: String,
    /// video / audio / image / subtitle
    pub kind: String,
    /// manual / subscription / live …
    pub source: String,
    pub min_size_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RuleThen {
    pub add_tags: Vec<String>,
    pub favorite: bool,
    /// 移动到下载目录下的这个子目录（可用 {platform} {author} {year} {month} {day} {date} {kind}）
    pub move_to: String,
    /// 另外提取音频：mp3 / m4a / flac / opus；留空不提取
    pub extract_audio: String,
    /// 上传（需要在“自动上传”里填好目标，不论是否启用自动上传）
    pub upload: bool,
    /// 命中时推送一条通知
    pub notify: bool,
    /// 按这个视频规整预设处理（“工具箱 → 视频规整”里的预设编号，例如 platform）；留空不处理。会在文件旁边生成新文件，原文件保留
    pub normalize: String,
}

/// 下载完成后自动上传到 WebDAV（Nextcloud、坚果云、Alist 等），或复制到另一个文件夹（NAS 挂载盘等）。
/// WebDAV 密码保存在加密的保险箱里（`upload.webdav`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UploadSettings {
    pub enabled: bool,
    /// webdav / folder
    pub kind: String,
    /// WebDAV 地址，或目标文件夹路径
    pub url: String,
    pub user: String,
    /// 目标下的子目录规则；变量同“整理文件夹”：{platform} {author} {year} {month} {day} {date} {kind}
    pub remote_dir: String,
    /// 哪些类型的文件要上传
    pub kinds: Vec<String>,
    /// 上传成功后删除本地文件（默认保留）
    pub delete_after: bool,
}

impl Default for UploadSettings {
    fn default() -> Self {
        UploadSettings {
            enabled: false,
            kind: "webdav".into(),
            url: String::new(),
            user: String::new(),
            remote_dir: "ClearClip/{platform}".into(),
            kinds: vec!["video".into(), "audio".into()],
            delete_after: false,
        }
    }
}

/// 外部通知渠道。令牌 / Webhook 地址保存在加密的保险箱里（`notify.<id>`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifyChannel {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// webhook / telegram / bark / serverchan / wecom / dingtalk / feishu / ntfy
    pub kind: String,
    /// Telegram 的 chat id、Bark 的服务器地址等不敏感的部分
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifySettings {
    pub channels: Vec<NotifyChannel>,
    /// 下载完成
    pub on_done: bool,
    /// 下载失败
    pub on_failed: bool,
    /// 直播开播 / 录制开始 / 结束 / 异常
    pub on_live: bool,
    /// 订阅有新内容
    pub on_sub: bool,
    /// 登录失效 / 即将失效
    pub on_account: bool,
}

impl Default for NotifySettings {
    fn default() -> Self {
        NotifySettings { channels: vec![], on_done: true, on_failed: true, on_live: true, on_sub: true, on_account: true }
    }
}

/// 安全与隐私选项。应用锁的密码不在这里（只保存校验值，放在加密的密钥保险箱里）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecuritySettings {
    /// 空闲多少分钟后自动锁定（需要先设置应用锁密码）；0 表示不自动锁定
    pub auto_lock_minutes: u32,
    /// 窗口收进托盘时自动锁定
    pub lock_on_hide: bool,
    /// 隐私模式：不记录解析历史、不进媒体库、任务结束即清除记录、通知不显示标题
    pub privacy_mode: bool,
    /// 禁止截屏和录屏（Windows、macOS 有效）
    pub content_protection: bool,
    /// 老板键：立刻隐藏窗口并锁定；留空表示不启用
    pub panic_shortcut: String,
    /// 保险箱空闲多少分钟后自动上锁；0 表示不自动上锁
    pub safebox_auto_lock_minutes: u32,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        SecuritySettings {
            auto_lock_minutes: 10,
            lock_on_hide: true,
            privacy_mode: false,
            content_protection: false,
            panic_shortcut: String::new(),
            safebox_auto_lock_minutes: 15,
        }
    }
}

/// 自定义站点规则。内置解析器、yt-dlp 都不支持的网站，可以自己写一条正则来取视频地址。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SiteRule {
    pub name: String,
    pub enabled: bool,
    /// 网址里包含这段文字时使用本规则；以 `re:` 开头表示正则
    pub pattern: String,
    /// 在网页源码里找视频地址的正则：取第一个捕获组，没有捕获组就取整个匹配
    pub video_regex: String,
    /// 标题的正则（取第一个捕获组）；留空用网页标题
    pub title_regex: String,
    /// 封面的正则（取第一个捕获组）；留空用 og:image
    pub cover_regex: String,
    /// 下载时的 Referer；留空用网页地址
    pub referer: String,
    /// 请求网页和下载时的 User-Agent；留空用电脑浏览器的
    pub user_agent: String,
}

impl Default for SiteRule {
    fn default() -> Self {
        SiteRule {
            name: String::new(),
            enabled: true,
            pattern: String::new(),
            video_regex: String::new(),
            title_regex: String::new(),
            cover_regex: String::new(),
            referer: String::new(),
            user_agent: String::new(),
        }
    }
}

/// AI 功能的设置。API 密钥不在这里，保存在加密的保险箱里（`ai.api_key`、`stt.api_key`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// 兼容 OpenAI 接口的地址（OpenAI、DeepSeek、通义、月之暗面、本机 Ollama 等），如 `https://api.openai.com/v1`
    pub base_url: String,
    pub model: String,
    /// 翻译成什么语言（直接写进提示词，如“简体中文”“English”）
    pub target_lang: String,
    /// 翻译结果同时保留原文（原文在上，译文在下）
    pub bilingual: bool,
    /// 每次请求翻译多少条字幕
    pub batch_size: usize,
    /// 语音转文字：`api`（兼容 OpenAI 的 `/audio/transcriptions`）或 `local`（本机的 whisper.cpp）
    pub stt_engine: String,
    /// 语音转文字接口地址；留空与上面相同
    pub stt_base_url: String,
    pub stt_model: String,
    /// 音频语言代码（zh / en / ja…），留空自动识别
    pub stt_language: String,
    /// whisper.cpp 可执行文件（whisper-cli），留空时在 PATH 里找
    pub whisper_bin: String,
    /// whisper.cpp 的 ggml 模型文件
    pub whisper_model: String,
    /// 下载到的字幕不是目标语言时自动翻译
    pub auto_translate: bool,
    /// 下载的视频没有字幕时自动转写
    pub auto_transcribe: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            target_lang: "简体中文".into(),
            bilingual: true,
            batch_size: 40,
            stt_engine: "api".into(),
            stt_base_url: String::new(),
            stt_model: "whisper-1".into(),
            stt_language: String::new(),
            whisper_bin: String::new(),
            whisper_model: String::new(),
            auto_translate: false,
            auto_transcribe: false,
        }
    }
}

impl AiSettings {
    pub fn normalize(&mut self) {
        let clean = |u: &str| -> String {
            let u = u.trim().trim_end_matches('/').to_string();
            if u.starts_with("http://") || u.starts_with("https://") {
                u
            } else {
                String::new()
            }
        };
        self.base_url = clean(&self.base_url);
        self.stt_base_url = clean(&self.stt_base_url);
        self.model = self.model.trim().to_string();
        self.stt_model = self.stt_model.trim().to_string();
        self.target_lang = self.target_lang.trim().chars().filter(|c| !c.is_control()).take(40).collect();
        if self.target_lang.is_empty() {
            self.target_lang = AiSettings::default().target_lang;
        }
        self.batch_size = self.batch_size.clamp(5, 100);
        if !matches!(self.stt_engine.as_str(), "api" | "local") {
            self.stt_engine = "api".into();
        }
        self.stt_language = self.stt_language.trim().to_ascii_lowercase();
        self.whisper_bin = self.whisper_bin.trim().to_string();
        self.whisper_model = self.whisper_model.trim().to_string();
    }

    /// 语音转文字用的接口地址。
    pub fn stt_url(&self) -> &str {
        if self.stt_base_url.is_empty() {
            &self.base_url
        } else {
            &self.stt_base_url
        }
    }
}

/// 弹幕样式（B站弹幕 XML 转成 ASS 时使用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DanmakuStyle {
    /// 字号（以 1080 高度的画面为准）
    pub font_size: u32,
    /// 不透明度 20–100（%）
    pub opacity: u32,
    /// 滚动弹幕从右到左穿过屏幕的大致秒数，越小越快
    pub scroll_secs: u32,
    /// 弹幕占画面高度的比例（%），避免挡住底部字幕
    pub area: u32,
    /// 字体名；留空使用微软雅黑（没有时播放器会自动换用其他中文字体）
    pub font: String,
}

impl Default for DanmakuStyle {
    fn default() -> Self {
        DanmakuStyle { font_size: 40, opacity: 100, scroll_secs: 7, area: 75, font: String::new() }
    }
}

impl DanmakuStyle {
    pub fn normalize(&mut self) {
        self.font_size = self.font_size.clamp(18, 80);
        self.opacity = self.opacity.clamp(20, 100);
        self.scroll_secs = self.scroll_secs.clamp(3, 20);
        self.area = self.area.clamp(10, 100);
        self.font = self.font.trim().chars().filter(|c| !matches!(c, ',' | '\n' | '\r')).take(60).collect();
    }
}

/// 媒体库设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LibrarySettings {
    /// 删除文件时先放进回收站（下载目录里的隐藏文件夹），可以还原
    pub use_trash: bool,
    /// 回收站里的文件保留多少天后自动清空（0 表示不自动清空）
    pub trash_keep_days: u32,
    /// “整理目录”使用的子目录规则；变量：{platform} {author} {year} {month} {day} {date} {kind}
    pub reorganize_template: String,
    /// 下载完成后给没有封面的视频截一张图，并记录时长
    pub probe_new_files: bool,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        LibrarySettings { use_trash: true, trash_keep_days: 30, reorganize_template: "{platform}/{author}".into(), probe_new_files: true }
    }
}

/// 收到的链接的记录与保留。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InboxSettings {
    /// 记录剪贴板识别到的链接（关闭后只记录手机和浏览器扩展发送的链接）
    pub record_clipboard: bool,
    /// 保留多少天（0 表示不按时间清理）
    pub keep_days: u32,
    /// 最多保留多少条（0 表示不限）
    pub max_items: usize,
}

impl Default for InboxSettings {
    fn default() -> Self {
        InboxSettings { record_clipboard: true, keep_days: 30, max_items: 1000 }
    }
}

/// 已配对的手机。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub added_at: i64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneSettings {
    pub enabled: bool,
    /// 固定端口，让快捷指令里的地址保持不变；0 表示首次开启时随机选择
    pub port: u16,
    /// 访问令牌（写在二维码和快捷指令里）
    pub token: String,
    pub devices: Vec<PairedDevice>,
    /// 手动指定二维码里的电脑地址（开了虚拟网卡 / 增强模式、自动选的地址手机连不上时用）；空表示自动选择
    pub host: String,
    /// 手机发来的文件保存到哪里；空表示“下载目录/接收”
    pub receive_dir: String,
    /// 手机上传的单个文件上限（MB）；0 表示不限制
    pub max_upload_mb: u64,
    /// 收到 .zip 后自动解压到同名文件夹（默认关闭）
    pub auto_extract: bool,
    /// 电脑发给手机的文件多久后自动撤回（小时）；0 表示不自动撤回
    pub outbox_hours: u32,
}

impl Default for PhoneSettings {
    fn default() -> Self {
        PhoneSettings {
            enabled: false,
            port: 0,
            token: String::new(),
            devices: Vec::new(),
            host: String::new(),
            receive_dir: String::new(),
            max_upload_mb: 4096,
            auto_extract: false,
            outbox_hours: 24,
        }
    }
}

/// 剪贴板默认额外识别的常用网站。
pub const DEFAULT_CLIPBOARD_DOMAINS: &[&str] =
    &["youtube.com", "youtu.be", "pornhub.com", "x.com", "twitter.com", "tiktok.com", "instagram.com", "vimeo.com", "twitch.tv", "acfun.cn", "ixigua.com"];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: String::new(),
            subfolder_by_platform: true,
            filename_template: "{author}_{title}_{date}".into(),
            concurrency: 3,
            skip_existing: true,
            watch_clipboard: true,
            auto_download: false,
            notify_on_complete: true,
            parse_mode: ParseMode::LocalThenRemote,
            remote_endpoint: String::new(),
            cookies: HashMap::new(),
            cookie_updated_at: HashMap::new(),
            theme: "system".into(),
            close_to_tray: true,
            shortcut: "CommandOrControl+Shift+D".into(),
            check_update: true,
            disclaimer_accepted: false,
            auto_resume: false,
            max_retries: 3,
            retry_delay_secs: 2,
            keep_part_on_cancel: false,
            record_samples: false,
            network: NetworkSettings::default(),
            segments: 4,
            segment_min_mb: 8,
            speed_limit_kbps: 0,
            per_site_concurrency: 2,
            site_request_interval_ms: 500,
            temp_dir: String::new(),
            conflict_policy: ConflictPolicy::Rename,
            disk_reserve_mb: 200,
            quality_preset: QualityPreset::Best,
            prefer_h264: true,
            merge_container: "mp4".into(),
            audio_format: "mp3".into(),
            hls_concurrency: 6,
            hls_skip_ads: false,
            series_template: "{series}/第{episode}集".into(),
            use_ytdlp: true,
            generic_sniffer: true,
            component_mirrors: vec![],
            ffmpeg_edition: "lite".into(),
            clipboard_all_sites: false,
            clipboard_domains: DEFAULT_CLIPBOARD_DOMAINS.iter().map(|d| d.to_string()).collect(),
            phone: PhoneSettings::default(),
            embed_metadata: true,
            write_info_json: false,
            write_nfo: false,
            open_folder_on_done: false,
            post_script_enabled: false,
            post_script: String::new(),
            prevent_sleep: true,
            subscriptions_enabled: false,
            launch_at_login: false,
            live_max_recordings: 3,
            live_cleanup_days: 0,
            inbox: InboxSettings::default(),
            subtitle_langs: vec!["zh".into(), "en".into()],
            subtitle_auto: true,
            subtitle_mode: "off".into(),
            subtitle_convert: true,
            danmaku_ass: true,
            verify_downloads: true,
            library: LibrarySettings::default(),
            danmaku: DanmakuStyle::default(),
            max_size_mb: 0,
            auto_downgrade: true,
            ai: AiSettings::default(),
            custom_sites: vec![],
            notify: NotifySettings::default(),
            upload: UploadSettings::default(),
            rules: vec![],
            speed_schedule: vec![],
            metered_mode: false,
            security: SecuritySettings::default(),
            onboarded: false,
            language: "zh".into(),
            float_ball: false,
        }
    }
}

impl Settings {
    pub fn load(path: &Path, default_download_dir: &Path) -> Settings {
        let raw = std::fs::read_to_string(path).ok();
        let mut s: Settings = raw.as_deref().and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default();
        // 升级前就已经在用的（同意过使用说明、设置里还没有这个字段）：不再弹出首次引导
        let has_key = raw.as_deref().and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).is_some_and(|v| v.get("onboarded").is_some());
        if !has_key && s.disclaimer_accepted {
            s.onboarded = true;
        }
        if s.download_dir.trim().is_empty() {
            s.download_dir = default_download_dir.to_string_lossy().into_owned();
        }
        s.normalize();
        s
    }

    pub fn save(&self, path: &Path) -> AppResult<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn normalize(&mut self) {
        self.concurrency = self.concurrency.clamp(1, 8);
        self.max_retries = self.max_retries.min(10);
        self.retry_delay_secs = self.retry_delay_secs.clamp(1, 60);
        self.segments = self.segments.clamp(1, 16);
        self.per_site_concurrency = self.per_site_concurrency.clamp(1, 8);
        self.site_request_interval_ms = self.site_request_interval_ms.min(10_000);
        self.temp_dir = self.temp_dir.trim().to_string();
        for r in &mut self.network.rules {
            r.pattern = r.pattern.trim().trim_start_matches('.').to_ascii_lowercase();
        }
        self.network.rules.retain(|r| !r.pattern.is_empty());
        self.hls_concurrency = self.hls_concurrency.clamp(1, 16);
        self.inbox.keep_days = self.inbox.keep_days.min(3650);
        if self.inbox.max_items > 0 {
            self.inbox.max_items = self.inbox.max_items.clamp(50, 100_000);
        }
        self.subtitle_langs = self.subtitle_langs.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(12).collect();
        if !matches!(self.subtitle_mode.as_str(), "off" | "file" | "embed" | "burn") {
            self.subtitle_mode = "off".into();
        }
        if !matches!(self.merge_container.as_str(), "mp4" | "mkv") {
            self.merge_container = "mp4".into();
        }
        if !matches!(self.audio_format.as_str(), "mp3" | "m4a" | "opus" | "flac") {
            self.audio_format = "mp3".into();
        }
        self.series_template = self.series_template.trim().to_string();
        self.clipboard_domains =
            self.clipboard_domains.iter().map(|d| d.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|d| d.contains('.')).collect();
        self.phone.devices.truncate(50);
        self.component_mirrors = self
            .component_mirrors
            .iter()
            .map(|m| m.trim().to_string())
            .filter(|m| m.starts_with("https://"))
            .map(|m| if m.ends_with('/') { m } else { format!("{m}/") })
            .collect();
        if self.filename_template.trim().is_empty() {
            self.filename_template = Settings::default().filename_template;
        }
        self.remote_endpoint = self.remote_endpoint.trim().to_string();
        if self.ffmpeg_edition != "full" {
            self.ffmpeg_edition = "lite".into();
        }
        self.danmaku.normalize();
        self.ai.normalize();
        self.speed_schedule
            .retain(|w| crate::live::parse_hhmm(&w.start).is_some() && crate::live::parse_hhmm(&w.end).is_some() && w.start.trim() != w.end.trim());
        self.speed_schedule.truncate(14);
        for w in &mut self.speed_schedule {
            w.days.retain(|d| (1..=7).contains(d));
            w.days.sort_unstable();
            w.days.dedup();
            w.start = w.start.trim().to_string();
            w.end = w.end.trim().to_string();
            w.limit_kbps = w.limit_kbps.min(10_000_000);
        }
        self.rules.truncate(50);
        for r in &mut self.rules {
            r.id = r.id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(32).collect();
            r.name = r.name.trim().to_string();
            r.then.add_tags = r.then.add_tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            r.then.move_to = r.then.move_to.trim().to_string();
            if crate::vidnorm::spec::preset(&r.then.normalize).is_none() {
                r.then.normalize.clear();
            }
            if !matches!(r.then.extract_audio.as_str(), "" | "mp3" | "m4a" | "flac" | "opus") {
                r.then.extract_audio.clear();
            }
        }
        self.upload.url = self.upload.url.trim().to_string();
        self.upload.user = self.upload.user.trim().to_string();
        self.upload.remote_dir = self.upload.remote_dir.trim().to_string();
        if !matches!(self.upload.kind.as_str(), "webdav" | "folder") {
            self.upload.kind = "webdav".into();
        }
        self.upload.kinds.retain(|k| matches!(k.as_str(), "video" | "audio" | "image" | "cover" | "subtitle"));
        self.notify.channels.truncate(10);
        for c in &mut self.notify.channels {
            c.name = c.name.trim().to_string();
            c.target = c.target.trim().to_string();
            c.id = c.id.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_').take(32).collect();
        }
        self.notify.channels.retain(|c| !c.id.is_empty());
        self.custom_sites.truncate(50);
        for r in &mut self.custom_sites {
            for f in [&mut r.name, &mut r.pattern, &mut r.video_regex, &mut r.title_regex, &mut r.cover_regex, &mut r.referer, &mut r.user_agent] {
                *f = f.trim().to_string();
            }
        }
        if !matches!(self.language.as_str(), "zh" | "en" | "ja") {
            self.language = "zh".into();
        }
        self.security.auto_lock_minutes = self.security.auto_lock_minutes.min(24 * 60);
        self.security.safebox_auto_lock_minutes = self.security.safebox_auto_lock_minutes.min(24 * 60);
        self.security.panic_shortcut = self.security.panic_shortcut.trim().to_string();
        self.max_size_mb = self.max_size_mb.min(1_000_000);
        self.library.trash_keep_days = self.library.trash_keep_days.min(3650);
        self.library.reorganize_template = self.library.reorganize_template.trim().to_string();
        if self.library.reorganize_template.is_empty() {
            self.library.reorganize_template = LibrarySettings::default().reorganize_template;
        }
    }

    /// 省流量模式下实际生效的设置（不改保存的设置本身）。
    pub fn metered_view(mut self) -> Settings {
        if self.metered_mode {
            self.concurrency = 1;
            self.per_site_concurrency = 1;
            self.segments = self.segments.min(2);
            self.hls_concurrency = self.hls_concurrency.min(2);
            if matches!(self.quality_preset, QualityPreset::Best | QualityPreset::Max1080) {
                self.quality_preset = QualityPreset::Small;
            }
        }
        self
    }

    pub fn download_root(&self) -> PathBuf {
        PathBuf::from(&self.download_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"concurrency": 5}"#).unwrap();
        assert_eq!(s.concurrency, 5);
        assert!(s.watch_clipboard);
        assert_eq!(s.parse_mode, ParseMode::LocalThenRemote);
    }

    #[test]
    fn route_matching_prefers_longest_suffix() {
        let mut n = NetworkSettings::default();
        assert_eq!(n.route_for_host("www.youtube.com"), Route::System);
        assert_eq!(n.route_for_host("upos-sz-mirrorcos.bilivideo.com"), Route::Direct);
        assert_eq!(n.route_for_host("example.org"), Route::System);
        n.rules.push(RouteRule { pattern: "music.youtube.com".into(), route: Route::Proxy("p1".into()) });
        assert_eq!(n.route_for_host("music.youtube.com"), Route::Proxy("p1".into()));
        assert_eq!(n.route_for_host("notyoutube.com"), Route::System, "suffix must match on label boundary");
    }

    #[test]
    fn route_serde_shape() {
        assert_eq!(serde_json::to_string(&Route::Direct).unwrap(), r#"{"kind":"direct"}"#);
        assert_eq!(serde_json::to_string(&Route::Proxy("a".into())).unwrap(), r#"{"kind":"proxy","id":"a"}"#);
    }

    #[test]
    fn speed_schedule_picks_the_window() {
        let w = |days: &[u8], s: &str, e: &str, l: u64| SpeedWindow { days: days.to_vec(), start: s.into(), end: e.into(), limit_kbps: l };
        let plan = vec![w(&[1, 2, 3, 4, 5], "09:00", "18:00", 300), w(&[], "00:00", "06:00", 0)];
        assert_eq!(schedule_limit(&plan, 3, 10 * 60), Some(300), "Wednesday 10:00");
        assert_eq!(schedule_limit(&plan, 6, 10 * 60), None, "Saturday daytime: global limit");
        assert_eq!(schedule_limit(&plan, 6, 3 * 60), Some(0), "night: unlimited");
        assert_eq!(schedule_limit(&plan, 3, 18 * 60), None, "end is exclusive");
        let mut s = Settings { speed_schedule: vec![w(&[9], "x", "y", 1), w(&[2, 2, 0], "10:00", "11:00", 99_999_999_999)], ..Default::default() };
        s.normalize();
        assert_eq!(s.speed_schedule, vec![w(&[2], "10:00", "11:00", 10_000_000)]);
    }

    #[test]
    fn metered_mode_overlays_without_touching_saved_values() {
        let s = Settings { metered_mode: true, concurrency: 5, quality_preset: QualityPreset::Best, ..Default::default() };
        let v = s.clone().metered_view();
        assert_eq!((v.concurrency, v.per_site_concurrency, v.quality_preset), (1, 1, QualityPreset::Small));
        assert_eq!(s.concurrency, 5, "the original is untouched");
        let audio = Settings { metered_mode: true, quality_preset: QualityPreset::Audio, ..Default::default() }.metered_view();
        assert_eq!(audio.quality_preset, QualityPreset::Audio, "audio-only stays audio-only");
        let off = Settings { metered_mode: false, concurrency: 4, ..Default::default() }.metered_view();
        assert_eq!(off.concurrency, 4);
    }

    #[test]
    fn normalize_clamps_concurrency() {
        let mut s = Settings { concurrency: 99, ..Settings::default() };
        s.normalize();
        assert_eq!(s.concurrency, 8);
    }

    #[test]
    fn upgraders_skip_onboarding_but_new_installs_get_it() {
        let dir = std::env::temp_dir().join(format!("clearclip-onb-{}-{}", std::process::id(), crate::db::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        let dl = dir.join("dl");
        // 没有设置文件：新安装
        assert!(!Settings::load(&p, &dl).onboarded);
        // 旧版本的设置：同意过说明但没有 onboarded 字段
        std::fs::write(&p, r#"{"disclaimerAccepted": true, "concurrency": 2}"#).unwrap();
        assert!(Settings::load(&p, &dl).onboarded);
        // 同意了说明但还没做完引导（字段明确为 false）：仍然显示
        std::fs::write(&p, r#"{"disclaimerAccepted": true, "onboarded": false}"#).unwrap();
        assert!(!Settings::load(&p, &dl).onboarded);
        // 语言只接受已知值
        std::fs::write(&p, r#"{"language": "xx"}"#).unwrap();
        assert_eq!(Settings::load(&p, &dl).language, "zh");
        let _ = std::fs::remove_dir_all(dir);
    }
}
