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
    /// 截图翻译的全局快捷键；留空表示不启用
    pub shot_hotkey: String,
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
    /// 批量添加时先打开预览面板（关闭后直接按默认选项加入队列）
    pub batch_preview: bool,
    /// 一批链接达到这个数量时，默认放进以批次名命名的文件夹
    pub batch_folder_min: usize,
    /// 批量下载时同一网站相邻两条开始之间的随机间隔（秒）
    pub batch_jitter_min: u32,
    pub batch_jitter_max: u32,
    /// 勾选“只在设定时段下载”的批次：时段内正常下载，时段外按 `limit_kbps` 限速（0 表示时段外暂停）
    pub run_window_on: bool,
    pub run_window: SpeedWindow,
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
    /// ed2k 服务器（`主机:端口`），链接里没有来源时向它们查询
    pub ed2k_servers: Vec<String>,
    /// ed2k 服务器列表（server.met）的地址；留空则只用上面手动添加的
    pub ed2k_server_met_url: String,
    /// BT：定期从在线列表更新 tracker
    pub bt_tracker_auto_update: bool,
    /// 在线 tracker 列表的地址，按顺序尝试；留空用内置默认（见 bt::trackers::DEFAULT_SOURCES）
    pub bt_tracker_sources: Vec<String>,
    /// 自动更新间隔（天）
    pub bt_tracker_interval_days: u32,
    /// 自己添加的 tracker（http / https / udp），始终使用
    pub bt_custom_trackers: Vec<String>,
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
    /// 可选组件：微信公众号文章视频下载（默认关闭）
    pub wechat_on: bool,
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
    /// 写作浮窗（桌面端入口）
    pub writing: WritingSettings,
    /// 实时屏幕翻译
    pub live: LiveSettings,
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

/// 写作浮窗在桌面端的设置：全局快捷键、自动浮标、不在哪些程序里出现、模型路线。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WritingSettings {
    /// 全局快捷键；留空表示不启用
    pub hotkey: String,
    /// 在其他程序里点进输入框时自动出现小浮标
    pub badge: bool,
    /// 暂停到这个时间（Unix 秒），0 表示没暂停
    pub paused_until: i64,
    /// 不在这些程序里出现（进程名，小写，如 `code.exe`）
    pub off_apps: Vec<String>,
    /// local_first / local_only / cloud_only
    pub route: String,
    /// 一次给几个版本（1–3）
    pub candidates: u8,
    /// 个人风格：一两句话，会附在每次改写的要求里；空表示不用
    pub style: String,
    /// 写作窗固定在最前
    pub pin: bool,
    /// 写回后自动关闭写作窗
    pub auto_close: bool,
    /// 写作窗上次的位置和大小（物理像素）；宽为 0 表示没记过
    pub win: [i32; 4],
    /// 上次用的动作
    pub last_action: String,
    /// 最近用过的“额外要求”
    pub extras: Vec<String>,
}

impl Default for WritingSettings {
    fn default() -> Self {
        WritingSettings {
            hotkey: "Ctrl+Alt+J".into(),
            badge: true,
            paused_until: 0,
            off_apps: vec![],
            route: "local_first".into(),
            candidates: 1,
            style: String::new(),
            pin: true,
            auto_close: false,
            win: [0; 4],
            last_action: String::new(),
            extras: vec![],
        }
    }
}

impl WritingSettings {
    pub fn sanitize(&mut self) {
        self.hotkey = self.hotkey.trim().to_string();
        if !["local_first", "local_only", "cloud_only"].contains(&self.route.as_str()) {
            self.route = "local_first".into();
        }
        self.candidates = self.candidates.clamp(1, 3);
        let mut seen = std::collections::HashSet::new();
        self.off_apps = self.off_apps.iter().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty() && seen.insert(a.clone())).take(100).collect();
        self.style = self.style.trim().chars().take(300).collect();
        self.last_action = self.last_action.trim().chars().take(24).collect();
        let mut seen = std::collections::HashSet::new();
        self.extras =
            self.extras.iter().map(|e| e.trim().chars().take(200).collect::<String>()).filter(|e| !e.is_empty() && seen.insert(e.clone())).take(8).collect();
        if self.win[2] < 300 || self.win[3] < 300 {
            self.win = [0; 4];
        }
    }
    /// 现在是否被暂停。
    pub fn paused(&self, now: i64) -> bool {
        self.paused_until > now
    }
    pub fn app_off(&self, exe: &str) -> bool {
        let e = exe.trim().to_lowercase();
        !e.is_empty() && self.off_apps.contains(&e)
    }
}

/// 云端文字识别（OCR）服务的账号。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OcrApi {
    /// ocrspace / baidu / google
    pub provider: String,
    pub key: String,
    /// 百度需要 Secret Key
    pub secret: String,
    /// 自建或代理的地址；留空用官方地址
    pub endpoint: String,
}

impl Default for OcrApi {
    fn default() -> Self {
        OcrApi { provider: "ocrspace".into(), key: String::new(), secret: String::new(), endpoint: String::new() }
    }
}

/// 每个程序单独记住的实时翻译设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveProfile {
    /// 进程名（小写，如 `game.exe`）
    pub exe: String,
    /// 空表示沿用全局
    pub mode: String,
    pub ocr: String,
    pub ocr_lang: String,
    pub target_lang: String,
    /// 上次框的区域（屏幕物理像素 x, y, w, h）；宽为 0 表示没记过
    pub region: [i32; 4],
    /// 整个窗口还是区域：window / region
    pub target: String,
}

/// 实时屏幕翻译（标准 / 字幕 / 漫画 三种模式共用）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LiveSettings {
    /// 开始 / 停止
    pub hotkey: String,
    /// 重新框选区域
    pub hotkey_pick: String,
    /// 显示 / 隐藏译文
    pub hotkey_hide: String,
    /// 切换模式（标准 → 字幕 → 漫画）
    pub hotkey_mode: String,
    /// 屏幕边上的小悬浮钮
    pub float_button: bool,
    /// standard / subtitle / manga
    pub mode: String,
    /// 识字：system（Windows 自带）/ enhanced（增强识字组件）/ api（云端识字服务）/ vision（视觉模型）
    pub ocr: String,
    /// 系统识字用的语言（BCP-47，如 ja、en-US）；空表示用已装语言里最合适的
    pub ocr_lang: String,
    pub ocr_api: OcrApi,
    /// 译成什么语言；空表示用“设置 → AI”里的目标语言
    pub target_lang: String,
    /// 每秒看几次屏幕（1–10）
    pub fps: u8,
    /// 画面稳定多久再翻（毫秒）
    pub stable_ms: u32,
    /// 画面变化超过百分之几才重新识别（1–50）
    pub change_pct: u8,
    /// 画面里的文字消失后译文再保留多久（毫秒）
    pub keep_ms: u32,
    /// 译文最小字号（像素）
    pub font_min: u8,
    /// 译文字号放大倍数（百分比 60–200）
    pub font_scale: u16,
    /// 同时显示原文（小字）
    pub bilingual: bool,
    /// 数字、符号、已经是目标语言的文字不翻
    pub skip_same: bool,
    /// 译文层不透明度（百分比 30–100）
    pub opacity: u8,
    // ---- 字幕模式 ----
    /// cover（盖住原字幕）/ bar（悬浮条）
    pub sub_place: String,
    pub sub_bilingual: bool,
    pub sub_fps: u8,
    pub sub_stable_ms: u32,
    pub sub_keep_ms: u32,
    pub sub_dedupe: bool,
    pub sub_merge: bool,
    /// 悬浮条位置和大小（屏幕物理像素）；宽为 0 表示没记过
    pub sub_bar: [i32; 4],
    pub sub_font: u8,
    // ---- 漫画模式 ----
    /// vision（视觉模型，会上传图片）/ local（本机识字 + 文字翻译）
    pub manga_engine: String,
    /// 已经确认过“漫画模式会上传图片”
    pub manga_ack: bool,
    /// 擦掉原文的方式：fill（取色填充）/ none（只盖译文）
    pub manga_erase: String,
    // ---- 系统声音实时字幕 ----
    /// 听的语言（空表示自动）
    pub audio_lang: String,
    /// local（本机 SenseVoice）/ cloud（云端语音识别）
    pub audio_stt: String,
    pub audio_bilingual: bool,
    // ---- 按程序记住 ----
    pub profiles: Vec<LiveProfile>,
    /// 保存翻译历史
    pub history: bool,
}

impl Default for LiveSettings {
    fn default() -> Self {
        LiveSettings {
            hotkey: "Ctrl+Alt+L".into(),
            hotkey_pick: "Ctrl+Alt+R".into(),
            hotkey_hide: "Ctrl+Alt+H".into(),
            hotkey_mode: "Ctrl+Alt+M".into(),
            float_button: false,
            mode: "standard".into(),
            ocr: "system".into(),
            ocr_lang: String::new(),
            ocr_api: OcrApi::default(),
            target_lang: String::new(),
            fps: 2,
            stable_ms: 500,
            change_pct: 3,
            keep_ms: 1500,
            font_min: 12,
            font_scale: 100,
            bilingual: false,
            skip_same: true,
            opacity: 100,
            sub_place: "cover".into(),
            sub_bilingual: true,
            sub_fps: 6,
            sub_stable_ms: 300,
            sub_keep_ms: 600,
            sub_dedupe: true,
            sub_merge: true,
            sub_bar: [0; 4],
            sub_font: 24,
            manga_engine: "vision".into(),
            manga_ack: false,
            manga_erase: "fill".into(),
            audio_lang: String::new(),
            audio_stt: "local".into(),
            audio_bilingual: true,
            profiles: vec![],
            history: true,
        }
    }
}

impl LiveSettings {
    pub fn sanitize(&mut self) {
        for h in [&mut self.hotkey, &mut self.hotkey_pick, &mut self.hotkey_hide, &mut self.hotkey_mode] {
            *h = h.trim().to_string();
        }
        let pick = |v: &mut String, ok: &[&str], d: &str| {
            if !ok.contains(&v.as_str()) {
                *v = d.into();
            }
        };
        pick(&mut self.mode, &["standard", "subtitle", "manga"], "standard");
        pick(&mut self.ocr, &["system", "enhanced", "api", "vision"], "system");
        pick(&mut self.ocr_api.provider, &["ocrspace", "baidu", "google"], "ocrspace");
        pick(&mut self.sub_place, &["cover", "bar"], "cover");
        pick(&mut self.manga_engine, &["vision", "local"], "vision");
        pick(&mut self.manga_erase, &["fill", "none"], "fill");
        pick(&mut self.audio_stt, &["local", "cloud"], "local");
        self.ocr_lang = self.ocr_lang.trim().chars().take(16).collect();
        self.target_lang = self.target_lang.trim().chars().take(40).collect();
        self.audio_lang = self.audio_lang.trim().chars().take(16).collect();
        self.ocr_api.key = self.ocr_api.key.trim().to_string();
        self.ocr_api.secret = self.ocr_api.secret.trim().to_string();
        self.ocr_api.endpoint = self.ocr_api.endpoint.trim().to_string();
        self.fps = self.fps.clamp(1, 10);
        self.sub_fps = self.sub_fps.clamp(1, 12);
        self.stable_ms = self.stable_ms.clamp(100, 5000);
        self.sub_stable_ms = self.sub_stable_ms.clamp(100, 3000);
        self.keep_ms = self.keep_ms.min(10_000);
        self.sub_keep_ms = self.sub_keep_ms.min(5_000);
        self.change_pct = self.change_pct.clamp(1, 50);
        self.font_min = self.font_min.clamp(8, 40);
        self.font_scale = self.font_scale.clamp(60, 200);
        self.opacity = self.opacity.clamp(30, 100);
        self.sub_font = self.sub_font.clamp(12, 72);
        if self.sub_bar[2] < 200 || self.sub_bar[3] < 30 {
            self.sub_bar = [0; 4];
        }
        let mut seen = std::collections::HashSet::new();
        for p in self.profiles.iter_mut() {
            p.exe = p.exe.trim().to_lowercase();
            if !["", "standard", "subtitle", "manga"].contains(&p.mode.as_str()) {
                p.mode.clear();
            }
            if !["", "system", "enhanced", "api", "vision"].contains(&p.ocr.as_str()) {
                p.ocr.clear();
            }
            if !["", "window", "region"].contains(&p.target.as_str()) {
                p.target.clear();
            }
            if p.region[2] < 20 || p.region[3] < 20 {
                p.region = [0; 4];
            }
        }
        self.profiles.retain(|p| !p.exe.is_empty() && seen.insert(p.exe.clone()));
        self.profiles.truncate(200);
    }

    pub fn profile(&self, exe: &str) -> Option<&LiveProfile> {
        let e = exe.trim().to_lowercase();
        (!e.is_empty()).then(|| self.profiles.iter().find(|p| p.exe == e)).flatten()
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
/// 模型池里的一个服务（密钥单独保存在保险箱里，名字见 `ai_pool::key_name`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolEntry {
    /// 只含小写字母、数字、`-`、`_`
    pub id: String,
    /// 来自哪个预设（`ai_pool::PRESETS`），自定义为 `custom`
    pub preset: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// 语音转文字模型；填了并且“适用任务”里勾了 stt，才会参与云端字幕识别的轮换
    pub stt_model: String,
    pub enabled: bool,
    /// 只用于这些任务（translate / summary / chat / edit / stt）；空 = 全部（stt 除外，要明确勾选）
    pub tasks: Vec<String>,
    /// 数字越小越先用；相同时今天用得少的先用
    pub priority: u32,
    /// 每天最多请求多少次（0 = 不限），用来避开免费额度
    pub daily_limit: u32,
}

impl Default for PoolEntry {
    fn default() -> Self {
        PoolEntry {
            id: String::new(),
            preset: "custom".into(),
            name: String::new(),
            base_url: String::new(),
            model: String::new(),
            stt_model: String::new(),
            enabled: true,
            tasks: vec![],
            priority: 10,
            daily_limit: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    /// 模型池：多个服务按任务自动选择、轮流使用。空时只用下面这一组“默认”设置。
    pub pool: Vec<PoolEntry>,
    /// 用 Supadata 取在线字幕（备用通道，密钥在保险箱 `ai.supadata`）
    pub supadata_on: bool,
    /// 翻译时使用术语库
    pub glossary_on: bool,
    /// 翻译用哪位 AI 专家（专家 id，空 / general = 通用）
    pub expert: String,
    /// 选“通用”时，按网址自动换成匹配的专家
    pub expert_auto: bool,
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
    /// 本机语音转文字用哪个模型：`whisper_tiny` / `whisper_base` / `whisper_small` / `sensevoice`；留空自动选。
    /// “设置 → AI”和“设置 → 组件”里是同一个选择。
    pub stt_local_model: String,
    /// 下载到的字幕不是目标语言时自动翻译
    pub auto_translate: bool,
    /// 下载的视频没有字幕时自动转写
    pub auto_transcribe: bool,
    /// AI 纠错（补标点、改同音错字）的力度：`conservative`（只补标点和明显错字）/ `aggressive`（再修语法）/ `rules`（只用规则）
    pub correct_mode: String,
    /// 自动转写 / 自动下载的字幕几乎没有标点时，自动做 AI 纠错
    pub correct_auto: bool,
    /// whisper.cpp 的 VAD 模型（ggml-silero-*.bin）；填了且 whisper-cli 支持时，只识别有人声的部分
    pub whisper_vad_model: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            pool: vec![],
            supadata_on: false,
            glossary_on: true,
            expert: "general".into(),
            expert_auto: true,
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            target_lang: "简体中文".into(),
            bilingual: true,
            batch_size: 40,
            stt_engine: "local".into(),
            stt_base_url: String::new(),
            stt_model: "whisper-1".into(),
            stt_language: String::new(),
            whisper_bin: String::new(),
            whisper_model: String::new(),
            stt_local_model: String::new(),
            auto_translate: false,
            auto_transcribe: false,
            correct_mode: "conservative".into(),
            correct_auto: false,
            whisper_vad_model: String::new(),
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
            self.stt_engine = "local".into();
        }
        self.stt_language = self.stt_language.trim().to_ascii_lowercase();
        self.whisper_bin = self.whisper_bin.trim().to_string();
        self.whisper_model = self.whisper_model.trim().to_string();
        if !matches!(self.stt_local_model.as_str(), "" | "whisper_tiny" | "whisper_base" | "whisper_small" | "sensevoice") {
            self.stt_local_model = String::new();
        }
        self.whisper_vad_model = self.whisper_vad_model.trim().to_string();
        if !matches!(self.correct_mode.as_str(), "conservative" | "aggressive" | "rules") {
            self.correct_mode = "conservative".into();
        }
        self.normalize_pool();
    }

    fn normalize_pool(&mut self) {
        let mut seen = std::collections::HashSet::new();
        self.pool.truncate(24);
        for e in &mut self.pool {
            e.id =
                e.id.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_')).take(32).collect();
            if e.id.is_empty() || !seen.insert(e.id.clone()) {
                // 重复或空的 id 换一个新的
                let mut n = seen.len() + 1;
                loop {
                    let id = format!("svc-{n}");
                    if seen.insert(id.clone()) {
                        e.id = id;
                        break;
                    }
                    n += 1;
                }
            }
            let u = e.base_url.trim().trim_end_matches('/');
            e.base_url = if u.starts_with("http://") || u.starts_with("https://") { u.to_string() } else { String::new() };
            e.model = e.model.trim().to_string();
            e.name = e.name.trim().chars().filter(|c| !c.is_control()).take(40).collect();
            e.preset = e.preset.trim().to_string();
            e.tasks.retain(|t| matches!(t.as_str(), "translate" | "summary" | "chat" | "edit" | "stt"));
            e.stt_model = e.stt_model.trim().to_string();
            e.tasks.dedup();
            e.priority = e.priority.min(999);
            e.daily_limit = e.daily_limit.min(1_000_000);
        }
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
            shot_hotkey: "Ctrl+Alt+T".into(),
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
            batch_preview: true,
            batch_folder_min: 3,
            batch_jitter_min: 2,
            batch_jitter_max: 6,
            run_window_on: false,
            run_window: SpeedWindow { days: vec![], start: "23:00".into(), end: "07:00".into(), limit_kbps: 0 },
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
            ed2k_servers: vec![],
            ed2k_server_met_url: "http://upd.emule-security.org/server.met".into(),
            bt_tracker_auto_update: true,
            bt_tracker_sources: crate::bt::trackers::DEFAULT_SOURCES.iter().map(|s| s.to_string()).collect(),
            bt_tracker_interval_days: 7,
            bt_custom_trackers: vec![],
            series_template: "{series}/第{episode}集".into(),
            use_ytdlp: true,
            generic_sniffer: true,
            component_mirrors: vec![],
            ffmpeg_edition: "lite".into(),
            clipboard_all_sites: false,
            wechat_on: false,
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
            writing: WritingSettings::default(),
            live: LiveSettings::default(),
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
        crate::bt::trackers::upgrade_default_sources(&mut s.bt_tracker_sources);
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
        self.segments = self.segments.clamp(1, 64);
        self.per_site_concurrency = self.per_site_concurrency.clamp(1, 8);
        self.batch_folder_min = self.batch_folder_min.clamp(1, 500);
        self.batch_jitter_max = self.batch_jitter_max.min(120);
        self.batch_jitter_min = self.batch_jitter_min.min(self.batch_jitter_max);
        self.site_request_interval_ms = self.site_request_interval_ms.min(10_000);
        self.temp_dir = self.temp_dir.trim().to_string();
        for r in &mut self.network.rules {
            r.pattern = r.pattern.trim().trim_start_matches('.').to_ascii_lowercase();
        }
        self.network.rules.retain(|r| !r.pattern.is_empty());
        self.hls_concurrency = self.hls_concurrency.clamp(1, 16);
        self.ed2k_servers = self.ed2k_servers.iter().filter_map(|s| crate::ed2k::server::normalize_server(s)).take(50).collect();
        self.ed2k_server_met_url = self.ed2k_server_met_url.trim().to_string();
        if !(self.ed2k_server_met_url.starts_with("http://") || self.ed2k_server_met_url.starts_with("https://")) {
            self.ed2k_server_met_url.clear();
        }
        self.bt_tracker_interval_days = self.bt_tracker_interval_days.clamp(1, 90);
        self.bt_tracker_sources =
            self.bt_tracker_sources.iter().map(|u| u.trim().to_string()).filter(|u| u.starts_with("http://") || u.starts_with("https://")).take(10).collect();
        self.bt_custom_trackers = self.bt_custom_trackers.iter().filter_map(|t| crate::bt::trackers::valid_tracker(t)).take(200).collect();
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
        self.shot_hotkey = self.shot_hotkey.trim().to_string();
        self.writing.sanitize();
        self.live.sanitize();
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

    #[test]
    fn live_settings_sanitize() {
        let mut l = LiveSettings { mode: "x".into(), fps: 99, stable_ms: 1, font_scale: 5, ..Default::default() };
        l.profiles = vec![
            LiveProfile { exe: " Game.EXE ".into(), mode: "bad".into(), region: [1, 2, 3, 4], ..Default::default() },
            LiveProfile { exe: "game.exe".into(), ..Default::default() },
            LiveProfile { exe: "".into(), ..Default::default() },
        ];
        l.sanitize();
        assert_eq!((l.mode.as_str(), l.fps, l.stable_ms, l.font_scale), ("standard", 10, 100, 60));
        assert_eq!(l.profiles.len(), 1);
        assert_eq!(l.profiles[0].exe, "game.exe");
        assert_eq!(l.profiles[0].mode, "");
        assert_eq!(l.profiles[0].region, [0; 4]);
        assert!(l.profile("GAME.exe").is_some() && l.profile("").is_none());
        // 旧配置里没有 live 字段也能读
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.live.mode, "standard");
    }
}
