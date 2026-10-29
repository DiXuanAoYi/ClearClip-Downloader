//! B站：从链接取 BV / av 号，调用 `x/web-interface/view` 取稿件信息，
//! 再用 wbi 签名的 `x/player/wbi/playurl` 取 DASH 音视频分轨（全部清晰度、H.264 / H.265 / AV1、杜比 / Hi-Res 音轨），
//! 下载后用 ffmpeg 合并。DASH 失败时退回 html5 平台音视频合一的 MP4。
//!
//! 未登录时清晰度通常最高 480P / 720P；登录后 1080P，大会员可选 4K / HDR / 杜比视界。
//! 多 P 稿件按链接里的 `?p=` 选择分 P，默认第 1 P。

use std::sync::{LazyLock, Mutex};

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use url::Url;

use super::{str_at, u32_at, u64_at, Ctx, Provider, DESKTOP_UA};
use crate::model::{AppError, AppResult, Asset, AssetKind, MediaInfo, MediaKind};

pub struct Bilibili;

static BV_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(BV[0-9A-Za-z]{10})").unwrap());
static AV_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/av(\d+)").unwrap());
static PAGE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&]p=(\d+)").unwrap());

const REFERER: &str = "https://www.bilibili.com/";

#[derive(Debug, PartialEq)]
pub enum VideoId {
    Bv(String),
    Av(u64),
}

impl VideoId {
    fn query(&self) -> String {
        match self {
            VideoId::Bv(b) => format!("bvid={b}"),
            VideoId::Av(a) => format!("aid={a}"),
        }
    }
}

#[async_trait]
impl Provider for Bilibili {
    fn id(&self) -> &'static str {
        "bilibili"
    }

    fn name(&self) -> &'static str {
        "B站"
    }

    fn matches(&self, url: &Url) -> bool {
        url.host_str().is_some_and(|h| h == "b23.tv" || h.ends_with(".b23.tv") || h == "bilibili.com" || h.ends_with(".bilibili.com") || h == "bili2233.cn")
    }

    fn referer(&self) -> &'static str {
        REFERER
    }

    fn login_url(&self) -> &'static str {
        "https://www.bilibili.com/"
    }

    async fn account_status(&self, ctx: &Ctx<'_>) -> AppResult<Option<super::AccountStatus>> {
        let api = "https://api.bilibili.com/x/web-interface/nav";
        let v = get_json(ctx, api, ctx.cookie(api).as_deref()).await?;
        Ok(Some(parse_nav(&v)))
    }

    async fn resolve(&self, ctx: &Ctx<'_>, url: &str) -> AppResult<MediaInfo> {
        let cookie = ctx.cookie("https://api.bilibili.com/");
        let (vid, page) = match video_id_from_url(url) {
            Some(v) => (v, page_from_url(url)),
            None => {
                // b23.tv 短链
                let resp = ctx.get(url).header("User-Agent", DESKTOP_UA).send().await?;
                let final_url = resp.url().to_string();
                // 番剧、课程、空间列表等交给 yt-dlp（解析器错误会触发 yt-dlp 兜底）
                let v = video_id_from_url(&final_url).ok_or_else(|| AppError::parser("没能从链接里识别出 B站视频号（BV/av），内置解析器只支持视频稿件。"))?;
                (v, page_from_url(&final_url))
            }
        };

        let view = get_json(ctx, &format!("https://api.bilibili.com/x/web-interface/view?{}", vid.query()), cookie.as_deref()).await?;
        let data = api_data(&view)?;
        let pages = data.get("pages").and_then(Value::as_array).cloned().unwrap_or_default();
        let index = page.saturating_sub(1).min(pages.len().saturating_sub(1));
        let cid = pages
            .get(index)
            .and_then(|p| u64_at(p, "/cid"))
            .or_else(|| u64_at(data, "/cid"))
            .ok_or_else(|| AppError::parser("B站稿件缺少 cid，无法获取播放地址。"))?;

        // 优先 DASH 分轨（所有清晰度）；失败时退回音视频合一的 MP4
        let dash = match wbi_keys(ctx, cookie.as_deref()).await {
            Ok(keys) => {
                let params = vec![
                    (vid.query_key().to_string(), vid.query_value()),
                    ("cid".into(), cid.to_string()),
                    ("qn".into(), "127".into()),
                    ("fnval".into(), "4048".into()),
                    ("fnver".into(), "0".into()),
                    ("fourk".into(), "1".into()),
                ];
                let q = wbi_sign(params, &keys, chrono::Utc::now().timestamp());
                match get_json(ctx, &format!("https://api.bilibili.com/x/player/wbi/playurl?{q}"), cookie.as_deref()).await {
                    Ok(v) => api_data(&v).ok().map(parse_dash).filter(|a| a.iter().any(|x| x.kind == AssetKind::Video)),
                    Err(e) => {
                        log::info!("bilibili dash failed: {e}");
                        None
                    }
                }
            }
            Err(e) => {
                log::info!("bilibili wbi keys failed: {e}");
                None
            }
        };
        if let Some(assets) = dash {
            let mut info = parse_view_meta(data, index, url)?;
            let cover = info.assets.pop().filter(|a| a.kind == AssetKind::Cover);
            info.assets = assets;
            info.assets.extend(cover);
            info.assets.extend(cc_subtitles(ctx, &vid, cid, cookie.as_deref()).await);
            info.assets.push(danmaku(cid));
            return Ok(info);
        }

        let qn = if cookie.is_some() { 80 } else { 64 };
        let play_api =
            format!("https://api.bilibili.com/x/player/playurl?{}&cid={cid}&qn={qn}&fnval=1&fnver=0&fourk=0&platform=html5&high_quality=1", vid.query());
        let play = get_json(ctx, &play_api, cookie.as_deref()).await?;
        let play_data = api_data(&play)?;
        let mut info = parse_view(data, play_data, index, url)?;
        info.assets.extend(cc_subtitles(ctx, &vid, cid, cookie.as_deref()).await);
        info.assets.push(danmaku(cid));
        Ok(info)
    }
}

impl VideoId {
    fn query_key(&self) -> &'static str {
        match self {
            VideoId::Bv(_) => "bvid",
            VideoId::Av(_) => "aid",
        }
    }

    fn query_value(&self) -> String {
        match self {
            VideoId::Bv(b) => b.clone(),
            VideoId::Av(a) => a.to_string(),
        }
    }
}

/// CC 字幕（UP 主上传的和 AI 生成的）。获取字幕地址需要登录，未登录时通常是空列表。
/// 偏好语言排在前面；获取失败不影响解析。
async fn cc_subtitles(ctx: &Ctx<'_>, vid: &VideoId, cid: u64, cookie: Option<&str>) -> Vec<Asset> {
    let Ok(keys) = wbi_keys(ctx, cookie).await else { return vec![] };
    let params = vec![(vid.query_key().to_string(), vid.query_value()), ("cid".into(), cid.to_string())];
    let q = wbi_sign(params, &keys, chrono::Utc::now().timestamp());
    match get_json(ctx, &format!("https://api.bilibili.com/x/player/wbi/v2?{q}"), cookie).await {
        Ok(v) => {
            let mut list = parse_cc(&v);
            list.sort_by_key(|a| crate::subtitle::lang_rank(a.format_id.as_deref().unwrap_or(""), &ctx.settings.subtitle_langs).unwrap_or(usize::MAX));
            list
        }
        Err(e) => {
            log::info!("bilibili cc subtitles failed: {e}");
            vec![]
        }
    }
}

/// 解析 `x/player/wbi/v2` 返回里的字幕列表。
pub fn parse_cc(v: &Value) -> Vec<Asset> {
    let Some(list) = v.pointer("/data/subtitle/subtitles").and_then(Value::as_array) else { return vec![] };
    list.iter()
        .filter_map(|t| {
            let lan = str_at(t, "/lan")?;
            let url = str_at(t, "/subtitle_url").filter(|u| !u.is_empty())?;
            let url = if let Some(rest) = url.strip_prefix("//") { format!("https://{rest}") } else { url.replacen("http://", "https://", 1) };
            let ai = lan.starts_with("ai-") || t.get("ai_type").and_then(Value::as_u64).unwrap_or(0) != 0;
            let name = str_at(t, "/lan_doc").map(String::from).unwrap_or_else(|| crate::subtitle::lang_name(lan));
            let mut a = Asset::base(format!("cc-{lan}"), AssetKind::Subtitle, url, format!("字幕 · {name}{}", if ai { "（AI 生成）" } else { "" }), "json");
            let base = crate::subtitle::normalize_lang(lan);
            a.quality = Some(if ai { format!("{base}.ai") } else { base });
            a.format_id = Some(lan.to_string());
            Some(a)
        })
        .collect()
}

/// 弹幕 XML（下载时按设置转成 ASS，播放器可直接显示滚动弹幕）。
fn danmaku(cid: u64) -> Asset {
    let mut a = Asset::base("danmaku", AssetKind::Subtitle, format!("https://comment.bilibili.com/{cid}.xml"), "弹幕 XML", "xml");
    a.quality = Some("danmaku".into());
    a
}

// ---------- wbi 签名 ----------

const MIXIN_TABLE: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61,
    26, 17, 0, 1, 60, 51, 30, 4, 22, 25, 54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// wbi 混合密钥（每天更新，缓存 6 小时）
static WBI_CACHE: Mutex<Option<(String, i64)>> = Mutex::new(None);

pub fn mixin_key(img_key: &str, sub_key: &str) -> String {
    let raw: Vec<char> = format!("{img_key}{sub_key}").chars().collect();
    MIXIN_TABLE.iter().filter_map(|&i| raw.get(i)).take(32).collect()
}

fn key_from_url(u: &str) -> &str {
    let name = u.rsplit('/').next().unwrap_or(u);
    name.split('.').next().unwrap_or(name)
}

pub(crate) async fn wbi_keys(ctx: &Ctx<'_>, cookie: Option<&str>) -> AppResult<String> {
    let now = chrono::Utc::now().timestamp();
    if let Some((k, t)) = WBI_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if now - t < 6 * 3600 {
            return Ok(k);
        }
    }
    // nav 接口未登录时 code 为 -101，但仍然返回 wbi_img
    let v = get_json(ctx, "https://api.bilibili.com/x/web-interface/nav", cookie).await?;
    let img = str_at(&v, "/data/wbi_img/img_url").ok_or_else(|| AppError::parser("B站没有返回 wbi 密钥"))?;
    let sub = str_at(&v, "/data/wbi_img/sub_url").ok_or_else(|| AppError::parser("B站没有返回 wbi 密钥"))?;
    let key = mixin_key(key_from_url(img), key_from_url(sub));
    *WBI_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((key.clone(), now));
    Ok(key)
}

/// 与 JavaScript encodeURIComponent 一致的编码（签名要求）。
fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 给参数加上 wts 和 w_rid 签名，返回查询字符串。
pub fn wbi_sign(mut params: Vec<(String, String)>, mixin: &str, wts: i64) -> String {
    use md5::{Digest, Md5};
    params.push(("wts".into(), wts.to_string()));
    params.sort_by(|a, b| a.0.cmp(&b.0));
    let query = params
        .iter()
        .map(|(k, v)| {
            let v: String = v.chars().filter(|c| !"!'()*".contains(*c)).collect();
            format!("{}={}", encode_component(k), encode_component(&v))
        })
        .collect::<Vec<_>>()
        .join("&");
    let rid = hex::encode(Md5::digest(format!("{query}{mixin}").as_bytes()));
    format!("{query}&w_rid={rid}")
}

// ---------- DASH ----------

fn quality_name(qn: u64) -> String {
    match qn {
        127 => "8K",
        126 => "杜比视界",
        125 => "HDR",
        120 => "4K",
        116 => "1080P60",
        112 => "1080P 高码率",
        100 => "智能修复",
        80 => "1080P",
        74 => "720P60",
        64 => "720P",
        32 => "480P",
        16 => "360P",
        6 => "240P",
        _ => return format!("清晰度 {qn}"),
    }
    .into()
}

fn codec_name(codecs: &str) -> (&'static str, &'static str) {
    let c = codecs.to_ascii_lowercase();
    if c.starts_with("avc") {
        ("avc", "H.264")
    } else if c.starts_with("hev") || c.starts_with("hvc") {
        ("hevc", "H.265")
    } else if c.starts_with("av01") {
        ("av1", "AV1")
    } else {
        ("other", "其他编码")
    }
}

fn dash_url(t: &Value) -> Option<String> {
    str_at(t, "/baseUrl").or_else(|| str_at(t, "/base_url")).or_else(|| str_at(t, "/backupUrl/0")).map(String::from)
}

/// 把 playurl 的 DASH 结果转换成资源列表：视频轨（按清晰度、编码）+ 音频轨，视频轨标记需要合并的音频。
pub fn parse_dash(play: &Value) -> Vec<Asset> {
    let Some(dash) = play.get("dash") else { return vec![] };
    let names: std::collections::HashMap<u64, String> = play
        .get("support_formats")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter().filter_map(|f| Some((u64_at(f, "/quality")?, str_at(f, "/new_description").or_else(|| str_at(f, "/display_desc"))?.to_string()))).collect()
        })
        .unwrap_or_default();
    let headers = vec![("User-Agent".to_string(), DESKTOP_UA.to_string())];

    let mut audios: Vec<Asset> = vec![];
    let mut audio_tracks: Vec<(Value, &str)> =
        dash.get("audio").and_then(Value::as_array).map(|a| a.iter().map(|t| (t.clone(), "")).collect()).unwrap_or_default();
    if let Some(a) = dash.pointer("/dolby/audio").and_then(Value::as_array) {
        audio_tracks.extend(a.iter().map(|t| (t.clone(), "杜比全景声")));
    }
    if let Some(t) = dash.pointer("/flac/audio").filter(|t| t.is_object()) {
        audio_tracks.push((t.clone(), "Hi-Res 无损"));
    }
    for (t, special) in &audio_tracks {
        let Some(url) = dash_url(t) else { continue };
        let id = u64_at(t, "/id").unwrap_or(0);
        let kbps = u64_at(t, "/bandwidth").map(|b| b / 1000);
        let codecs = str_at(t, "/codecs").unwrap_or("mp4a");
        let flac = codecs.starts_with("fLaC") || codecs.eq_ignore_ascii_case("flac");
        let mut a = Asset::base(format!("dash-a-{id}"), AssetKind::Audio, url, "", if flac { "flac" } else { "m4a" });
        a.acodec = Some(codecs.to_string());
        a.bitrate = kbps;
        a.headers = headers.clone();
        a.label = match (special.is_empty(), kbps) {
            (false, _) => format!("音频 · {special}"),
            (true, Some(k)) => format!("音频 · {k}kbps · M4A"),
            _ => "音频 · M4A".into(),
        };
        audios.push(a);
    }
    // 合并用的默认音轨：普通 AAC 里码率最高的（杜比 / 无损音轨封装进 MP4 兼容性差）
    let best_audio = audios.iter().filter(|a| a.ext == "m4a" && !a.label.contains("杜比")).max_by_key(|a| a.bitrate.unwrap_or(0)).map(|a| a.id.clone());

    let mut videos: Vec<Asset> = vec![];
    for t in dash.get("video").and_then(Value::as_array).into_iter().flatten() {
        let Some(url) = dash_url(t) else { continue };
        let qn = u64_at(t, "/id").unwrap_or(0);
        let codecs = str_at(t, "/codecs").unwrap_or("");
        let (ckey, cname) = codec_name(codecs);
        let id = format!("dash-v-{qn}-{ckey}");
        if videos.iter().any(|v| v.id == id) {
            continue;
        }
        let mut a = Asset::base(id, AssetKind::Video, url, "", "mp4");
        a.width = u32_at(t, "/width");
        a.height = u32_at(t, "/height");
        a.vcodec = Some(codecs.to_string());
        a.bitrate = u64_at(t, "/bandwidth").map(|b| b / 1000);
        a.fps = str_at(t, "/frameRate").or_else(|| str_at(t, "/frame_rate")).and_then(|f| f.parse::<f32>().ok());
        a.has_audio = Some(false);
        a.pair_audio = best_audio.clone();
        a.headers = headers.clone();
        let name = names.get(&qn).cloned().unwrap_or_else(|| quality_name(qn));
        a.quality = Some(name.clone());
        a.label = format!("{name} · {cname}");
        videos.push(a);
    }
    // 清晰度从高到低；同清晰度按 H.264、H.265、AV1
    let order = |a: &Asset| match a.id.rsplit('-').next() {
        Some("avc") => 0,
        Some("hevc") => 1,
        Some("av1") => 2,
        _ => 3,
    };
    videos.sort_by(|a, b| {
        let qa: u64 = a.id.split('-').nth(2).and_then(|x| x.parse().ok()).unwrap_or(0);
        let qb: u64 = b.id.split('-').nth(2).and_then(|x| x.parse().ok()).unwrap_or(0);
        qb.cmp(&qa).then(order(a).cmp(&order(b)))
    });
    audios.sort_by_key(|a| std::cmp::Reverse(a.bitrate.unwrap_or(0)));
    videos.into_iter().chain(audios).collect()
}

/// 只有稿件信息（不含播放地址）时的基础结构。
fn parse_view_meta(data: &Value, index: usize, source_url: &str) -> AppResult<MediaInfo> {
    let fake = serde_json::json!({"durl": [{"url": "about:blank"}]});
    let mut info = parse_view(data, &fake, index, source_url)?;
    info.assets.retain(|a| a.kind == AssetKind::Cover);
    Ok(info)
}

pub(crate) async fn get_json(ctx: &Ctx<'_>, api: &str, cookie: Option<&str>) -> AppResult<Value> {
    let mut req = ctx.get(api).header("User-Agent", DESKTOP_UA).header("Referer", REFERER);
    if let Some(c) = cookie {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), "B站接口").context_suffix("，可能触发了风控，请稍后再试或在设置中登录 B站。"));
    }
    let text = resp.text().await?;
    ctx.record("bilibili", "api", api, &text);
    Ok(serde_json::from_str(&text)?)
}

/// B站接口统一格式 `{code, message, data}`，code 非 0 表示失败。
pub fn api_data(v: &Value) -> AppResult<&Value> {
    let code = v.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        let msg = str_at(v, "/message").unwrap_or("未知错误");
        let text = format!("B站：{msg}（{code}）");
        return Err(match code {
            -101 | -403 | -10403 => AppError::need_login(text),
            -404 | 62002 | 62004 => AppError::not_found(text),
            -352 | -412 | -509 => AppError::new(crate::error::ErrorKind::RateLimited, text),
            _ => AppError::classify(text),
        });
    }
    v.get("data").filter(|d| d.is_object()).ok_or_else(|| AppError::msg("B站接口没有返回数据。"))
}

/// 解析 `x/web-interface/nav`：未登录时 code 为 -101。
pub fn parse_nav(v: &Value) -> super::AccountStatus {
    let data = v.get("data");
    let logged_in = data.and_then(|d| d.get("isLogin")).and_then(Value::as_bool).unwrap_or(false);
    let vip = data.filter(|d| u64_at(d, "/vipStatus") == Some(1)).map(|d| str_at(d, "/vip_label/text").unwrap_or("大会员").to_string());
    super::AccountStatus { logged_in, user_name: data.and_then(|d| str_at(d, "/uname")).map(String::from), vip }
}

pub fn video_id_from_url(url: &str) -> Option<VideoId> {
    if let Some(c) = BV_RE.captures(url) {
        return Some(VideoId::Bv(c[1].to_string()));
    }
    AV_RE.captures(url).and_then(|c| c[1].parse().ok()).map(VideoId::Av)
}

fn page_from_url(url: &str) -> usize {
    PAGE_RE.captures(url).and_then(|c| c[1].parse().ok()).filter(|p| *p > 0).unwrap_or(1)
}

/// 合并稿件信息和播放地址。`index` 是分 P 下标（从 0 开始）。
pub fn parse_view(data: &Value, play: &Value, index: usize, source_url: &str) -> AppResult<MediaInfo> {
    let bvid = str_at(data, "/bvid").map(String::from).or_else(|| u64_at(data, "/aid").map(|a| format!("av{a}"))).unwrap_or_default();
    let pages = data.get("pages").and_then(Value::as_array);
    let page = pages.and_then(|p| p.get(index));
    let multi = pages.is_some_and(|p| p.len() > 1);

    let mut title = str_at(data, "/title").unwrap_or("B站视频").trim().to_string();
    if multi {
        if let Some(part) = page.and_then(|p| str_at(p, "/part")) {
            title = format!("{title} P{} {part}", index + 1);
        }
    }
    let author = str_at(data, "/owner/name").unwrap_or("未知UP主").to_string();
    let cover = str_at(data, "/pic").map(|u| u.replace("http://", "https://"));
    let duration_ms = page.and_then(|p| u64_at(p, "/duration")).or_else(|| u64_at(data, "/duration")).map(|s| s * 1000);
    let dim = page.and_then(|p| p.get("dimension")).or_else(|| data.get("dimension"));
    let (mut width, mut height) = (dim.and_then(|d| u32_at(d, "/width")), dim.and_then(|d| u32_at(d, "/height")));
    // rotate=1 表示竖屏，宽高需要交换
    if dim.and_then(|d| u64_at(d, "/rotate")) == Some(1) {
        std::mem::swap(&mut width, &mut height);
    }

    let video = str_at(play, "/durl/0/url")
        .or_else(|| str_at(play, "/durl/0/backup_url/0"))
        .ok_or_else(|| AppError::need_login("B站没有返回可直接下载的 MP4 地址。该视频可能需要登录、大会员或仅支持分段播放。"))?;

    let mut assets = vec![Asset::video(video.to_string(), width, height)];
    if let Some(c) = &cover {
        assets.push(Asset::cover(c.clone()));
    }
    let id = if multi { format!("{bvid}-p{}", index + 1) } else { bvid };

    Ok(MediaInfo {
        platform: "bilibili".into(),
        platform_name: "B站".into(),
        id,
        source_url: source_url.to_string(),
        title,
        author,
        cover,
        duration_ms,
        kind: MediaKind::Video,
        width,
        height,
        published_at: u64_at(data, "/pubdate").map(|t| t as i64),
        assets,
        entries: vec![],
        series: None,
        chapters: vec![],
        extractor: Some("native".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: &str = include_str!("../../tests/fixtures/bili_view.json");
    const PLAY: &str = include_str!("../../tests/fixtures/bili_playurl.json");

    #[test]
    fn cc_subtitle_list() {
        let v = serde_json::json!({"code": 0, "data": {"subtitle": {"subtitles": [
            {"lan": "ai-zh", "lan_doc": "中文（自动生成）", "subtitle_url": "//aisubtitle.hdslb.com/bfs/ai_subtitle/prod/x?auth_key=k", "ai_type": 1},
            {"lan": "en-US", "lan_doc": "English", "subtitle_url": "http://i0.hdslb.com/bfs/subtitle/y.json", "ai_type": 0},
            {"lan": "ja", "lan_doc": "日本語", "subtitle_url": ""}
        ]}}});
        let list = parse_cc(&v);
        assert_eq!(list.len(), 2, "tracks without a URL (not logged in) are skipped");
        assert_eq!(list[0].id, "cc-ai-zh");
        assert_eq!(list[0].url, "https://aisubtitle.hdslb.com/bfs/ai_subtitle/prod/x?auth_key=k");
        assert_eq!(list[0].quality.as_deref(), Some("zh.ai"));
        assert!(list[0].label.contains("AI 生成"));
        assert_eq!(list[1].url, "https://i0.hdslb.com/bfs/subtitle/y.json");
        assert_eq!(list[1].quality.as_deref(), Some("en-us"));
        assert_eq!(list[1].ext, "json");
        assert!(parse_cc(&serde_json::json!({"data": {"subtitle": {"subtitles": []}}})).is_empty());
        assert!(parse_cc(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn video_ids() {
        assert_eq!(video_id_from_url("https://www.bilibili.com/video/BV1xx411c7mD/?spm=1"), Some(VideoId::Bv("BV1xx411c7mD".into())));
        assert_eq!(video_id_from_url("https://m.bilibili.com/video/av170001"), Some(VideoId::Av(170001)));
        assert_eq!(video_id_from_url("https://b23.tv/AbC123"), None);
        assert_eq!(page_from_url("https://www.bilibili.com/video/BV1xx411c7mD?p=3"), 3);
        assert_eq!(page_from_url("https://www.bilibili.com/video/BV1xx411c7mD"), 1);
    }

    #[test]
    fn parses_second_page_of_multi_part_video() {
        let view: Value = serde_json::from_str(VIEW).unwrap();
        let play: Value = serde_json::from_str(PLAY).unwrap();
        let info = parse_view(api_data(&view).unwrap(), api_data(&play).unwrap(), 1, "x").unwrap();
        assert_eq!(info.id, "BV1xx411c7mD-p2");
        assert_eq!(info.title, "Rust 入门教程 P2 第二集：所有权");
        assert_eq!(info.author, "编程老王");
        assert_eq!(info.duration_ms, Some(600_000));
        assert_eq!((info.width, info.height), (Some(1920), Some(1080)));
        assert!(info.assets[0].url.starts_with("https://upos-sz-mirrorcos.bilivideo.com/"));
        assert_eq!(info.cover.as_deref(), Some("https://i0.hdslb.com/bfs/archive/cover.jpg"));
    }

    #[test]
    fn nav_status() {
        let v: Value =
            serde_json::from_str(r#"{"code":0,"data":{"isLogin":true,"uname":"编程老王","vipStatus":1,"vip_label":{"text":"年度大会员"}}}"#).unwrap();
        let s = parse_nav(&v);
        assert!(s.logged_in);
        assert_eq!(s.user_name.as_deref(), Some("编程老王"));
        assert_eq!(s.vip.as_deref(), Some("年度大会员"));
        let v: Value = serde_json::from_str(r#"{"code":-101,"message":"账号未登录","data":{"isLogin":false}}"#).unwrap();
        assert!(!parse_nav(&v).logged_in);
    }

    #[test]
    fn api_error_surfaces_message() {
        let v: Value = serde_json::from_str(r#"{"code":-404,"message":"啥都木有","data":null}"#).unwrap();
        let err = api_data(&v).unwrap_err().to_string();
        assert!(err.contains("啥都木有") && err.contains("-404"), "{err}");
    }

    #[test]
    fn missing_durl_is_clear_error() {
        let view: Value = serde_json::from_str(VIEW).unwrap();
        let play: Value = serde_json::from_str(r#"{"quality":64}"#).unwrap();
        assert!(parse_view(api_data(&view).unwrap(), &play, 0, "x").unwrap_err().to_string().contains("MP4"));
    }

    #[test]
    fn wbi_signature_matches_reference() {
        // 参考 bilibili-API-collect 文档中的示例
        let mixin = mixin_key("7cd084941338484aae1ad9425b84077c", "4932caff0ff746eab6f01bf08b70ac45");
        assert_eq!(mixin, "ea1db124af3c7062474693fa704f4ff8");
        let q = wbi_sign(vec![("foo".into(), "114".into()), ("bar".into(), "514".into()), ("zab".into(), "1919810".into())], &mixin, 1702204169);
        assert_eq!(q, "bar=514&foo=114&wts=1702204169&zab=1919810&w_rid=8f6f2b5b3d485fe1886cec6a0be8c5d4");
        assert_eq!(key_from_url("https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png"), "7cd084941338484aae1ad9425b84077c");
        assert_eq!(encode_component("a b/中"), "a%20b%2F%E4%B8%AD");
    }

    #[test]
    fn dash_tracks() {
        let play: Value = serde_json::from_str(
            r#"{"support_formats":[{"quality":80,"new_description":"1080P 高清"},{"quality":64,"new_description":"720P 准高清"}],
            "dash":{"video":[
                {"id":64,"baseUrl":"https://cdn/v64-avc.m4s","codecs":"avc1.64001F","width":1280,"height":720,"bandwidth":1200000,"frameRate":"30"},
                {"id":80,"baseUrl":"https://cdn/v80-hevc.m4s","codecs":"hev1.1.6.L120.90","width":1920,"height":1080,"bandwidth":1500000},
                {"id":80,"base_url":"https://cdn/v80-avc.m4s","codecs":"avc1.640032","width":1920,"height":1080,"bandwidth":3000000}],
              "audio":[{"id":30216,"baseUrl":"https://cdn/a64.m4s","codecs":"mp4a.40.2","bandwidth":64000},
                       {"id":30280,"baseUrl":"https://cdn/a192.m4s","codecs":"mp4a.40.2","bandwidth":192000}],
              "dolby":{"audio":[{"id":30250,"baseUrl":"https://cdn/dolby.m4s","codecs":"ec-3","bandwidth":448000}]},
              "flac":{"audio":null}}}"#,
        )
        .unwrap();
        let assets = parse_dash(&play);
        let ids: Vec<&str> = assets.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["dash-v-80-avc", "dash-v-80-hevc", "dash-v-64-avc", "dash-a-30250", "dash-a-30280", "dash-a-30216"]);
        assert_eq!(assets[0].label, "1080P 高清 · H.264");
        assert_eq!(assets[0].pair_audio.as_deref(), Some("dash-a-30280"), "dolby not used for default merge");
        assert_eq!(assets[0].has_audio, Some(false));
        assert_eq!(assets[2].fps, Some(30.0));
        assert!(assets[3].label.contains("杜比"));
    }
}
