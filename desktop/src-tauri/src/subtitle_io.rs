//! 字幕文件的获取与转换：下载队列里的字幕任务，以及内嵌 / 烧录进视频时用到的字幕。
//!
//! 字幕文件很小，不走分段下载引擎：直接请求（带 Referer 和对应网站的 Cookie），
//! 失败时（YouTube 的字幕地址有时需要额外凭据）回退让 yt-dlp 取；取回后按设置转换格式。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::engine::DlError;
use crate::model::{Asset, Clip, MediaInfo};
use crate::postprocess::{self, SubTrack};
use crate::settings::Settings;
use crate::subtitle::{self, DanmakuLayout};
use crate::{download, providers, AppState};

/// 内容是否像这种格式的字幕（防止拿到网页或错误提示）。
pub fn looks_valid(ext: &str, text: &str) -> bool {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.is_empty() {
        return false;
    }
    match ext {
        "vtt" => t.starts_with("WEBVTT"),
        "json" => t.starts_with('{') || t.starts_with('['),
        "xml" => t.contains("<d ") || t.starts_with("<?xml") || t.starts_with("<i>"),
        "srt" => t.contains("-->"),
        "ass" | "ssa" => t.contains("[Script Info]") || t.contains("[Events]"),
        _ => !t.starts_with("<!DOCTYPE") && !t.to_ascii_lowercase().starts_with("<html"),
    }
}

async fn http_text(st: &AppState, settings: &Settings, media: &MediaInfo, asset: &Asset) -> Result<String, DlError> {
    let req = download::build_request(st, media, asset);
    let clients = st.net.clients_for(&settings.network, &asset.url).map_err(|e| DlError::Other(e.message))?;
    let resp = req.build_get(&clients.api).send().await.map_err(|e| DlError::Net(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(DlError::Status(status.as_u16()));
    }
    if resp.content_length().is_some_and(|l| l > 32 * 1024 * 1024) {
        return Err(DlError::Other("字幕文件异常大，已放弃".into()));
    }
    let bytes = resp.bytes().await.map_err(|e| DlError::Net(e.to_string()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 让 yt-dlp 取字幕：只对 yt-dlp 解析的作品有效，返回内容和扩展名。
async fn ytdlp_text(st: &AppState, settings: &Settings, media: &MediaInfo, asset: &Asset) -> Option<(String, String)> {
    let lang = asset.format_id.as_deref().filter(|l| !l.is_empty())?;
    if !media.extractor.as_deref().is_some_and(|e| e.starts_with("yt-dlp")) || media.source_url.is_empty() {
        return None;
    }
    let ctx = st.parse_ctx(settings);
    let (mut cmd, cookie_file) = providers::ytdlp::command(&ctx, &media.source_url).ok()?;
    let dir = std::env::temp_dir().join(format!(
        "clearclip-sub-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).ok()?;
    let auto = asset.id.starts_with("auto-");
    cmd.args([
        "--skip-download",
        "--no-playlist",
        if auto { "--write-auto-subs" } else { "--write-subs" },
        "--sub-langs",
        lang,
        "--sub-format",
        "vtt/srt/best",
        "-o",
    ])
    .arg(dir.join("s"))
    .arg("--")
    .arg(&media.source_url)
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let ran = tokio::time::timeout(Duration::from_secs(90), cmd.status()).await;
    if let Some(cf) = &cookie_file {
        cf.merge_back(&st.cookies);
    }
    let result = (|| {
        if !matches!(ran, Ok(Ok(s)) if s.success()) {
            return None;
        }
        let file = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("vtt" | "srt" | "ass")))?;
        let ext = file.extension()?.to_str()?.to_string();
        Some((std::fs::read_to_string(&file).ok()?, ext))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// 取回字幕原文（内容和扩展名）。
pub async fn fetch_raw(st: &AppState, settings: &Settings, media: &MediaInfo, asset: &Asset) -> Result<(String, String), DlError> {
    let mut last = DlError::Other("没有取到字幕".into());
    for attempt in 0..2 {
        match http_text(st, settings, media, asset).await {
            Ok(text) if looks_valid(&asset.ext, &text) => return Ok((text, asset.ext.clone())),
            Ok(_) => {
                last = DlError::Other("字幕地址返回的内容不是字幕".into());
                break;
            }
            Err(e @ DlError::Net(_)) if attempt == 0 => {
                last = e;
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
            Err(e) => {
                last = e;
                break;
            }
        }
    }
    match ytdlp_text(st, settings, media, asset).await {
        Some(r) => Ok(r),
        None => Err(last),
    }
}

/// 取回字幕并转换为 `to`（`srt` / `ass`）；有裁剪区间时一并裁剪。返回内容和最终扩展名。
pub async fn fetch_converted(
    st: &AppState,
    settings: &Settings,
    media: &MediaInfo,
    asset: &Asset,
    to: Option<&str>,
    clip: Option<&Clip>,
) -> Result<(String, String), DlError> {
    let (raw, ext) = fetch_raw(st, settings, media, asset).await?;
    convert_text_styled(media, &raw, &ext, to, clip, &settings.danmaku).map_err(|e| DlError::Other(e.message))
}

/// 纯转换部分（便于测试）。
pub fn convert_text(media: &MediaInfo, raw: &str, ext: &str, to: Option<&str>, clip: Option<&Clip>) -> crate::error::AppResult<(String, String)> {
    convert_text_styled(media, raw, ext, to, clip, &crate::settings::DanmakuStyle::default())
}

pub fn convert_text_styled(
    media: &MediaInfo,
    raw: &str,
    ext: &str,
    to: Option<&str>,
    clip: Option<&Clip>,
    style: &crate::settings::DanmakuStyle,
) -> crate::error::AppResult<(String, String)> {
    let layout = DanmakuLayout::for_video(media.width, media.height).styled(style);
    let range = clip.map(|c| c.range());
    let (mut text, mut cur) = (raw.to_string(), ext.to_string());
    if let Some(to) = to {
        if let Some(converted) = subtitle::convert_with_font(ext, to, raw, layout, range, &style.font)? {
            text = converted;
            cur = to.to_string();
        }
        // 弹幕裁剪在转成 ASS 时已经完成；SRT（转换得到的或原本就是）在这里裁剪
        if let (Some(r), "srt") = (range, cur.as_str()) {
            text = subtitle::clip_srt(&text, r);
        }
    }
    Ok((text, cur))
}

fn track_lang(a: &Asset) -> String {
    a.format_id.clone().or_else(|| a.quality.clone()).unwrap_or_default()
}

fn track_title(a: &Asset) -> String {
    a.label.trim_start_matches("字幕 · ").to_string()
}

/// 把字幕内嵌或烧录进已下载的视频（`input` 是 .part 文件）所需的信息。
pub struct VideoSubs<'a> {
    pub st: &'a AppState,
    pub settings: &'a Settings,
    pub media: &'a MediaInfo,
    pub embed: &'a [Asset],
    /// `soft` 内嵌 / `burn` 烧录
    pub mode: &'a str,
    pub clip: Option<&'a Clip>,
    pub ffmpeg: &'a Path,
    pub input: &'a Path,
    pub final_path: &'a Path,
    /// 临时目录（处理完删除）
    pub work: &'a Path,
}

impl VideoSubs<'_> {
    /// 成功时用新文件替换 `input`。返回给用户看的提示：全部成功为 None；部分失败或没有可用字幕时给出原因
    /// （视频本身下载成功，不算任务失败）。
    pub async fn apply(&self) -> Option<String> {
        if let Err(e) = std::fs::create_dir_all(self.work) {
            return Some(format!("字幕未处理：无法创建临时目录（{e}）"));
        }
        let result = self.run().await;
        let _ = std::fs::remove_dir_all(self.work);
        result
    }

    async fn run(&self) -> Option<String> {
        let (st, settings, media, mode, clip) = (self.st, self.settings, self.media, self.mode, self.clip);
        let mut tracks: Vec<SubTrack> = vec![];
        let mut failed: Vec<String> = vec![];
        // 烧录：多种语言的文字字幕会叠在一起，只用第一条；弹幕（ASS）可以同时烧录
        let mut seen_text = false;
        let mut skipped_text = 0;
        let embed: Vec<&Asset> = self
            .embed
            .iter()
            .filter(|a| {
                if mode != "burn" || a.ext == "xml" {
                    return true;
                }
                let keep = !seen_text;
                seen_text = true;
                if !keep {
                    skipped_text += 1;
                }
                keep
            })
            .collect();
        for (i, a) in embed.iter().enumerate() {
            let to = if a.ext == "xml" { "ass" } else { "srt" };
            match fetch_converted(st, settings, media, a, Some(to), clip).await {
                Ok((text, ext)) => {
                    let path = self.work.join(format!("s{i}.{ext}"));
                    if std::fs::write(&path, text).is_ok() {
                        tracks.push(SubTrack { path, lang: track_lang(a), title: track_title(a) });
                    } else {
                        failed.push(track_title(a));
                    }
                }
                Err(e) => failed.push(format!("{}（{e}）", track_title(a))),
            }
        }
        if skipped_text > 0 {
            failed.push(format!("烧录只使用第一条字幕，其余 {skipped_text} 条没有烧录"));
        }
        let failed_note = (!failed.is_empty()).then(|| {
            if skipped_text > 0 && failed.len() == 1 {
                failed[0].clone()
            } else {
                format!("这些字幕没有处理：{}", failed.join("、"))
            }
        });
        if tracks.is_empty() {
            return Some(failed_note.unwrap_or_else(|| "没有可用的字幕".into()));
        }
        let out = suffix(self.input, ".embed");
        let outcome: Result<Option<String>, String> = if mode == "burn" {
            let paths: Vec<PathBuf> = tracks.iter().map(|t| t.path.clone()).collect();
            postprocess::burn_subtitles(self.ffmpeg, self.input, &out, self.final_path, &paths).await.map(|_| None).map_err(|e| e.message)
        } else {
            match postprocess::embed_subtitles(self.ffmpeg, self.input, &out, self.final_path, &tracks).await {
                Ok(0) => Err("这种文件格式不能内嵌字幕（MP4 不能内嵌 ASS 弹幕，可改用 MKV 或烧录）".to_string()),
                Ok(n) if n < tracks.len() => Ok(Some(format!("MP4 不能内嵌 ASS 弹幕，已内嵌 {n} 条字幕，其余被跳过"))),
                Ok(_) => Ok(None),
                Err(e) => Err(e.message),
            }
        };
        match outcome {
            Ok(note) => {
                let _ = std::fs::remove_file(self.input);
                if std::fs::rename(&out, self.input).is_err() {
                    let _ = std::fs::remove_file(&out);
                    return Some("字幕处理完成但无法替换文件".into());
                }
                match (note, failed_note) {
                    (Some(a), Some(b)) => Some(format!("{a}；{b}")),
                    (a, b) => a.or(b),
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&out);
                Some(format!("字幕处理失败，视频已保存但没有字幕：{e}"))
            }
        }
    }
}

fn suffix(p: &Path, s: &str) -> PathBuf {
    let mut o = p.as_os_str().to_owned();
    o.push(s);
    PathBuf::from(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media() -> MediaInfo {
        serde_json::from_value(serde_json::json!({
            "platform": "bilibili", "platformName": "B站", "id": "1", "title": "t", "author": "a",
            "kind": "video", "sourceUrl": "https://x", "assets": [], "width": 1920, "height": 1080
        }))
        .unwrap()
    }

    #[test]
    fn validity() {
        assert!(looks_valid("vtt", "\u{feff}WEBVTT\n\n"));
        assert!(!looks_valid("vtt", "<!DOCTYPE html><html>"));
        assert!(looks_valid("json", "{\"body\":[]}"));
        assert!(!looks_valid("json", "Forbidden"));
        assert!(looks_valid("xml", "<?xml version=\"1.0\"?><i></i>"));
        assert!(looks_valid("srt", "1\n00:00:01,000 --> 00:00:02,000\nx"));
        assert!(!looks_valid("srt", ""));
        assert!(!looks_valid("lrc", "<html>"));
    }

    #[test]
    fn conversion_and_clip() {
        let m = media();
        let vtt = "WEBVTT\n\n00:05.000 --> 00:08.000\nA\n\n00:12.000 --> 00:14.000\nB\n";
        let (text, ext) = convert_text(&m, vtt, "vtt", Some("srt"), None).unwrap();
        assert_eq!(ext, "srt");
        assert!(text.contains("00:00:05,000 --> 00:00:08,000"));
        // 转换后再裁剪（10 秒起）
        let clip = Clip { start_ms: 10_000, end_ms: None, precise: false };
        let (text, ext) = convert_text(&m, vtt, "vtt", Some("srt"), Some(&clip)).unwrap();
        assert_eq!(ext, "srt");
        assert_eq!(text, "1\n00:00:02,000 --> 00:00:04,000\nB\n\n");
        // 已经是 SRT 的字幕：裁剪
        let srt = "1\n00:00:05,000 --> 00:00:08,000\nA\n\n2\n00:00:12,000 --> 00:00:14,000\nB\n\n";
        let (text, ext) = convert_text(&m, srt, "srt", Some("srt"), Some(&clip)).unwrap();
        assert_eq!((ext.as_str(), text.as_str()), ("srt", "1\n00:00:02,000 --> 00:00:04,000\nB\n\n"));
        // 不转换：原样
        let (text, ext) = convert_text(&m, vtt, "vtt", None, None).unwrap();
        assert_eq!((ext.as_str(), text.as_str()), ("vtt", vtt));
        // 弹幕转 ASS，并按区间裁剪
        let xml = r#"<i><d p="1,1,25,16777215,1,0,a,1">早</d><d p="12,1,25,16777215,1,0,a,2">晚</d></i>"#;
        let (text, ext) = convert_text(&m, xml, "xml", Some("ass"), Some(&clip)).unwrap();
        assert_eq!(ext, "ass");
        assert!(text.contains("晚") && !text.contains("早"));
        // B站 CC JSON → SRT
        let (text, ext) = convert_text(&m, r#"{"body":[{"from":1,"to":2,"content":"你好"}]}"#, "json", Some("srt"), None).unwrap();
        assert_eq!(ext, "srt");
        assert!(text.contains("你好"));
        // 损坏的内容给出错误
        assert!(convert_text(&m, "not json", "json", Some("srt"), None).is_err());
    }
}
