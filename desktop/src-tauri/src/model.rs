use serde::{Deserialize, Serialize};

/// 解析结果里的内容类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Video,
    Images,
    Audio,
    /// 合集 / 播放列表 / 剧集：`entries` 列出条目，按需逐条解析
    Playlist,
}

/// 资源的下载方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// 普通 HTTP 文件
    #[default]
    Http,
    /// m3u8 播放列表
    Hls,
    /// 交给 yt-dlp 下载（url 为原始页面地址，format 为 yt-dlp 的格式 ID）
    Ytdlp,
}

/// 单个可下载资源的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Video,
    Image,
    Audio,
    Cover,
    /// 字幕或弹幕（quality 字段为语言代码，弹幕为 danmaku）
    Subtitle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// 在同一条作品内唯一，例如 `video`、`image-3`、`music`、`cover`。
    pub id: String,
    pub kind: AssetKind,
    pub url: String,
    pub label: String,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 图集中的序号（从 0 开始），其他资源为 `None`。
    pub index: Option<usize>,
    #[serde(default)]
    pub protocol: Protocol,
    /// 下载时额外携带的请求头（如 Referer、User-Agent）
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// 清晰度标签，如“1080P”
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub vcodec: Option<String>,
    #[serde(default)]
    pub acodec: Option<String>,
    /// 码率（kbps）
    #[serde(default)]
    pub bitrate: Option<u64>,
    /// 预估大小（字节）
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default)]
    pub fps: Option<f32>,
    /// 视频轨是否带声音；None 表示未知（按带声音处理）
    #[serde(default)]
    pub has_audio: Option<bool>,
    /// 无声视频轨需要合并的音频轨资源 ID（音视频分离的格式）
    #[serde(default)]
    pub pair_audio: Option<String>,
    /// yt-dlp 的格式 ID（protocol = ytdlp 时使用）
    #[serde(default)]
    pub format_id: Option<String>,
    /// 平台特有的附加数据（如 Pixiv 动图的帧时长）
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}

/// 合集 / 播放列表里的一个条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub id: String,
    pub title: String,
    pub url: String,
    pub duration_ms: Option<u64>,
    pub thumbnail: Option<String>,
    /// 在列表中的序号（从 1 开始）
    pub index: u32,
}

/// 剧集信息，用于 `{series}` `{episode}` 等命名变量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SeriesInfo {
    pub name: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl Asset {
    pub fn base(id: impl Into<String>, kind: AssetKind, url: String, label: impl Into<String>, ext: impl Into<String>) -> Self {
        Asset {
            id: id.into(),
            kind,
            url,
            label: label.into(),
            ext: ext.into(),
            width: None,
            height: None,
            index: None,
            protocol: Protocol::Http,
            headers: vec![],
            quality: None,
            vcodec: None,
            acodec: None,
            bitrate: None,
            filesize: None,
            fps: None,
            has_audio: None,
            pair_audio: None,
            format_id: None,
            extra: None,
        }
    }

    pub fn video(url: String, width: Option<u32>, height: Option<u32>) -> Self {
        let mut a = Asset::base("video", AssetKind::Video, url, "视频 MP4", "mp4");
        a.width = width;
        a.height = height;
        a.quality = quality_label(width, height);
        a
    }

    pub fn image(index: usize, url: String, width: Option<u32>, height: Option<u32>) -> Self {
        let ext = guess_image_ext(&url);
        let mut a = Asset::base(format!("image-{index}"), AssetKind::Image, url, format!("图片 {}", index + 1), ext);
        a.width = width;
        a.height = height;
        a.index = Some(index);
        a
    }

    pub fn audio(url: String) -> Self {
        let ext = if url.contains(".m4a") { "m4a" } else { "mp3" };
        Asset::base("music", AssetKind::Audio, url, "背景音乐", ext)
    }

    /// 字幕：`lang` 用于文件名（如 `视频.zh-Hans.srt`，播放器会自动加载）。
    pub fn subtitle(lang: &str, name: &str, url: String, ext: &str) -> Self {
        let mut a = Asset::base(format!("sub-{lang}"), AssetKind::Subtitle, url, format!("字幕 · {name}"), ext);
        a.quality = Some(lang.to_string());
        a
    }

    pub fn cover(url: String) -> Self {
        let ext = guess_image_ext(&url);
        Asset::base("cover", AssetKind::Cover, url, "封面图", ext)
    }

    /// 视频的短边像素（竖屏视频的“1080P”指宽度）。
    pub fn short_side(&self) -> Option<u32> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => Some(w.min(h)),
            (w, h) => w.or(h),
        }
    }
}

/// 由宽高得到“1080P”这类清晰度标签。
pub fn quality_label(width: Option<u32>, height: Option<u32>) -> Option<String> {
    let short = match (width, height) {
        (Some(w), Some(h)) => w.min(h),
        _ => return None,
    };
    let label = match short {
        s if s >= 2100 => "4K",
        s if s >= 1400 => "2K",
        s if s >= 1000 => "1080P",
        s if s >= 700 => "720P",
        s if s >= 460 => "480P",
        s if s >= 340 => "360P",
        _ => return Some(format!("{short}P")),
    };
    Some(label.to_string())
}

fn guess_image_ext(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    let path = lower.split('?').next().unwrap_or(&lower);
    if lower.contains("format/png") {
        "png".into()
    } else if lower.contains("format/webp") || path.contains(".webp") {
        "webp".into()
    } else if path.contains(".png") {
        "png".into()
    } else if path.contains(".gif") {
        "gif".into()
    } else if path.contains(".heic") {
        "heic".into()
    } else {
        "jpg".into()
    }
}

/// 一条作品的统一解析结果，所有平台都转换成这个结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    /// 平台标识：`douyin` / `kuaishou` / `remote`。
    pub platform: String,
    pub platform_name: String,
    /// 平台内作品 ID。
    pub id: String,
    /// 用户粘贴的原始链接，用于直链过期后重新解析。
    pub source_url: String,
    pub title: String,
    pub author: String,
    pub cover: Option<String>,
    pub duration_ms: Option<u64>,
    pub kind: MediaKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 发布时间，Unix 秒。
    pub published_at: Option<i64>,
    pub assets: Vec<Asset>,
    /// 合集 / 播放列表的条目（kind = playlist 时）
    #[serde(default)]
    pub entries: Vec<PlaylistEntry>,
    #[serde(default)]
    pub series: Option<SeriesInfo>,
    /// 解析来源：native / yt-dlp / remote
    #[serde(default)]
    pub extractor: Option<String>,
    /// 视频章节（YouTube 等），用于按章节拆分或选择片段
    #[serde(default)]
    pub chapters: Vec<Chapter>,
}

/// 只下载 / 保留的时间段（毫秒）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Clip {
    pub start_ms: u64,
    /// 为空表示到结尾
    pub end_ms: Option<u64>,
    /// 重新编码，起点精确到帧；默认不重新编码，起点落在前一个关键帧
    pub precise: bool,
}

impl Clip {
    pub fn range(&self) -> (u64, Option<u64>) {
        (self.start_ms, self.end_ms)
    }
}

/// 视频章节（时间单位毫秒）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl MediaInfo {
    /// 默认勾选的资源：视频作品选第一个（最佳）视频格式，图集选全部图片，音频选第一个音频。
    pub fn default_asset_ids(&self) -> Vec<String> {
        match self.kind {
            MediaKind::Video => self.assets.iter().find(|a| a.kind == AssetKind::Video).map(|a| vec![a.id.clone()]).unwrap_or_default(),
            MediaKind::Images => self.assets.iter().filter(|a| a.kind == AssetKind::Image).map(|a| a.id.clone()).collect(),
            MediaKind::Audio => self.assets.iter().find(|a| a.kind == AssetKind::Audio).map(|a| vec![a.id.clone()]).unwrap_or_default(),
            MediaKind::Playlist => vec![],
        }
    }

    pub fn asset(&self, id: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }
}

pub use crate::error::{AppError, AppResult, ErrorKind};
