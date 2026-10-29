//! 直播间开播检测与直播流地址。
//!
//! 原生实现：B站、抖音、快手、虎牙；其他（斗鱼、YouTube、Twitch 等）交给 yt-dlp；
//! 也可以直接填写 .flv / .m3u8 直播流地址。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{extract_json_after, str_at, u64_at, ytdlp, Ctx, DESKTOP_UA};
use crate::model::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamFormat {
    Flv,
    Hls,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStream {
    /// 清晰度名称，如“原画”
    pub quality: String,
    /// 越大越清晰
    pub rank: u32,
    pub url: String,
    pub format: StreamFormat,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStatus {
    pub platform: String,
    pub platform_name: String,
    /// 平台内的房间号
    pub room_id: String,
    pub streamer: String,
    pub title: String,
    pub cover: Option<String>,
    pub avatar: Option<String>,
    pub live: bool,
    /// 按清晰度从高到低
    pub streams: Vec<LiveStream>,
}

impl LiveStatus {
    fn offline(platform: &str, name: &str, room: &str) -> Self {
        LiveStatus {
            platform: platform.into(),
            platform_name: name.into(),
            room_id: room.into(),
            streamer: String::new(),
            title: String::new(),
            cover: None,
            avatar: None,
            live: false,
            streams: vec![],
        }
    }

    /// 按偏好选择直播流：`quality` 为空或“原画”时选最高；否则选名称匹配的，找不到时选最高。FLV 优先（延迟低、断线后已录部分可播放）。
    pub fn pick(&self, quality: &str) -> Option<&LiveStream> {
        let mut v: Vec<&LiveStream> = self.streams.iter().collect();
        v.sort_by_key(|s| (std::cmp::Reverse(s.rank), s.format != StreamFormat::Flv));
        if !quality.is_empty() {
            if let Some(s) = v.iter().find(|s| s.quality == quality) {
                return Some(s);
            }
        }
        v.first().copied()
    }
}

static BILI_ROOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"live\.bilibili\.com/(?:h5/)?(\d+)").unwrap());
static DOUYIN_ROOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"live\.douyin\.com/(\d+)").unwrap());
static KUAISHOU_ROOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"live\.kuaishou\.com/u/([A-Za-z0-9_\-]+)").unwrap());
static HUYA_ROOM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"huya\.com/([A-Za-z0-9_]+)").unwrap());

/// 链接对应的平台（用于检测间隔下限等）。
pub fn platform_of(url: &str) -> &'static str {
    if BILI_ROOM.is_match(url) {
        "bilibili"
    } else if DOUYIN_ROOM.is_match(url) {
        "douyin"
    } else if KUAISHOU_ROOM.is_match(url) {
        "kuaishou"
    } else if HUYA_ROOM.is_match(url) {
        "huya"
    } else if is_direct(url) {
        "direct"
    } else {
        "ytdlp"
    }
}

/// 各平台开播检测间隔的下限（秒）。
pub fn min_interval(platform: &str) -> u64 {
    match platform {
        "bilibili" | "direct" => 30,
        _ => 60,
    }
}

fn is_direct(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    path.ends_with(".flv") || path.ends_with(".m3u8")
}

pub async fn status(ctx: &Ctx<'_>, url: &str) -> AppResult<LiveStatus> {
    if let Some(c) = BILI_ROOM.captures(url) {
        return bilibili(ctx, &c[1]).await;
    }
    if let Some(c) = DOUYIN_ROOM.captures(url) {
        return douyin(ctx, &c[1]).await;
    }
    if let Some(c) = KUAISHOU_ROOM.captures(url) {
        return kuaishou(ctx, &c[1]).await;
    }
    if let Some(c) = HUYA_ROOM.captures(url).filter(|_| url.contains("huya.com")) {
        return huya(ctx, &c[1]).await;
    }
    if is_direct(url) {
        return direct(ctx, url).await;
    }
    via_ytdlp(ctx, url).await
}

async fn get_json(ctx: &Ctx<'_>, url: &str, referer: &str) -> AppResult<Value> {
    let mut req = ctx.get(url).header("User-Agent", DESKTOP_UA).header("Referer", referer);
    if let Some(c) = ctx.cookie(url) {
        req = req.header("Cookie", c);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), "直播接口"));
    }
    let text = resp.text().await?;
    ctx.record("live", "api", url, &text);
    Ok(serde_json::from_str(&text)?)
}

// ---------- B站 ----------

async fn bilibili(ctx: &Ctx<'_>, id: &str) -> AppResult<LiveStatus> {
    const REF: &str = "https://live.bilibili.com/";
    let init = get_json(ctx, &format!("https://api.live.bilibili.com/room/v1/Room/room_init?id={id}"), REF).await?;
    if init.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(AppError::not_found(format!("B站直播间不存在：{}", str_at(&init, "/message").unwrap_or(""))));
    }
    let rid = u64_at(&init, "/data/room_id").unwrap_or(0).to_string();
    let uid = u64_at(&init, "/data/uid").unwrap_or(0);
    let live = u64_at(&init, "/data/live_status") == Some(1);
    let mut st = LiveStatus::offline("bilibili", "B站", &rid);
    if let Ok(info) = get_json(ctx, &format!("https://api.live.bilibili.com/room/v1/Room/get_info?room_id={rid}"), REF).await {
        st.title = str_at(&info, "/data/title").unwrap_or("").to_string();
        st.cover = str_at(&info, "/data/user_cover").map(String::from);
    }
    if let Ok(m) = get_json(ctx, &format!("https://api.live.bilibili.com/live_user/v1/Master/info?uid={uid}"), REF).await {
        st.streamer = str_at(&m, "/data/info/uname").unwrap_or("").to_string();
        st.avatar = str_at(&m, "/data/info/face").map(String::from);
    }
    st.live = live;
    if live {
        let play = get_json(
            ctx,
            &format!("https://api.live.bilibili.com/xlive/web-room/v2/index/getRoomPlayInfo?room_id={rid}&protocol=0,1&format=0,1,2&codec=0&qn=10000&platform=web&ptype=8&dolby=5&panorama=1"),
            REF,
        )
        .await?;
        st.streams = parse_bili_play(&play);
    }
    Ok(st)
}

pub fn parse_bili_play(v: &Value) -> Vec<LiveStream> {
    let desc: std::collections::HashMap<u64, String> = v
        .pointer("/data/playurl_info/playurl/g_qn_desc")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|d| Some((u64_at(d, "/qn")?, str_at(d, "/desc")?.to_string()))).collect())
        .unwrap_or_default();
    let headers = vec![("Referer".to_string(), "https://live.bilibili.com/".to_string()), ("User-Agent".to_string(), DESKTOP_UA.to_string())];
    let mut out = vec![];
    for stream in v.pointer("/data/playurl_info/playurl/stream").and_then(Value::as_array).into_iter().flatten() {
        for fmt in stream.get("format").and_then(Value::as_array).into_iter().flatten() {
            let format = match str_at(fmt, "/format_name") {
                Some("flv") => StreamFormat::Flv,
                Some("ts") => StreamFormat::Hls,
                _ => continue, // fmp4 HLS 需要初始化分片，暂不录制
            };
            for codec in fmt.get("codec").and_then(Value::as_array).into_iter().flatten() {
                if str_at(codec, "/codec_name") != Some("avc") {
                    continue;
                }
                let qn = u64_at(codec, "/current_qn").unwrap_or(0);
                let base = str_at(codec, "/base_url").unwrap_or("");
                let Some(info) = codec.pointer("/url_info/0") else { continue };
                let url = format!("{}{}{}", str_at(info, "/host").unwrap_or(""), base, str_at(info, "/extra").unwrap_or(""));
                out.push(LiveStream {
                    quality: desc.get(&qn).cloned().unwrap_or_else(|| format!("{qn}")),
                    rank: qn as u32,
                    url,
                    format,
                    headers: headers.clone(),
                });
            }
        }
    }
    out
}

// ---------- 抖音 ----------

async fn douyin(ctx: &Ctx<'_>, rid: &str) -> AppResult<LiveStatus> {
    // 先访问直播页拿 ttwid（接口需要），已登录时带上账号 Cookie
    let page_url = format!("https://live.douyin.com/{rid}");
    let mut cookie = ctx.cookie(&page_url).unwrap_or_default();
    if !cookie.contains("ttwid=") {
        let resp = ctx.get(&page_url).header("User-Agent", DESKTOP_UA).send().await?;
        let ttwid = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|c| c.split(';').next().filter(|kv| kv.starts_with("ttwid=")).map(String::from));
        if let Some(t) = ttwid {
            cookie = if cookie.is_empty() { t } else { format!("{cookie}; {t}") };
        }
    }
    let api = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=129.0.0.0&web_rid={rid}"
    );
    let resp = ctx.get(&api).header("User-Agent", DESKTOP_UA).header("Referer", &page_url).header("Cookie", cookie).send().await?;
    let text = resp.text().await?;
    ctx.record("live", "douyin", &api, &text);
    let v: Value = serde_json::from_str(&text).map_err(|_| AppError::need_login("抖音直播接口没有返回数据，可能需要登录抖音。"))?;
    parse_douyin(&v, rid)
}

pub fn parse_douyin(v: &Value, rid: &str) -> AppResult<LiveStatus> {
    let mut st = LiveStatus::offline("douyin", "抖音", rid);
    let room = v.pointer("/data/data/0").ok_or_else(|| AppError::not_found("抖音直播间不存在或接口已变化"))?;
    st.streamer = str_at(v, "/data/user/nickname").unwrap_or("").to_string();
    st.avatar = str_at(v, "/data/user/avatar_thumb/url_list/0").map(String::from);
    st.title = str_at(room, "/title").unwrap_or("").to_string();
    st.cover = str_at(room, "/cover/url_list/0").map(String::from);
    st.live = u64_at(room, "/status") == Some(2);
    if st.live {
        let names = [("FULL_HD1", "原画", 4), ("HD1", "超清", 3), ("SD1", "高清", 2), ("SD2", "标清", 1)];
        let headers = vec![("User-Agent".to_string(), DESKTOP_UA.to_string())];
        for (key, name, rank) in names {
            if let Some(u) = str_at(room, &format!("/stream_url/flv_pull_url/{key}")) {
                st.streams.push(LiveStream { quality: name.into(), rank, url: u.to_string(), format: StreamFormat::Flv, headers: headers.clone() });
            }
            if let Some(u) = str_at(room, &format!("/stream_url/hls_pull_url_map/{key}")) {
                st.streams.push(LiveStream { quality: name.into(), rank, url: u.to_string(), format: StreamFormat::Hls, headers: headers.clone() });
            }
        }
    }
    Ok(st)
}

// ---------- 快手 ----------

async fn kuaishou(ctx: &Ctx<'_>, id: &str) -> AppResult<LiveStatus> {
    let url = format!("https://live.kuaishou.com/u/{id}");
    let mut req = ctx.get(&url).header("User-Agent", DESKTOP_UA);
    if let Some(c) = ctx.cookie(&url) {
        req = req.header("Cookie", c);
    }
    let html = req.send().await?.text().await?;
    ctx.record("live", "kuaishou", &url, &html);
    let state =
        extract_json_after(&html, "window.__INITIAL_STATE__").ok_or_else(|| AppError::need_login("快手直播页没有数据，可能需要登录快手或触发了验证。"))?;
    Ok(parse_kuaishou(&state, id))
}

pub fn parse_kuaishou(state: &Value, id: &str) -> LiveStatus {
    let mut st = LiveStatus::offline("kuaishou", "快手", id);
    let Some(item) = state.pointer("/liveroom/playList/0") else { return st };
    st.streamer = str_at(item, "/author/name").unwrap_or("").to_string();
    st.avatar = str_at(item, "/author/avatar").map(String::from);
    st.title = str_at(item, "/liveStream/caption").unwrap_or("").to_string();
    st.cover = str_at(item, "/liveStream/poster").map(String::from);
    st.live = item.get("isLiving").and_then(Value::as_bool).unwrap_or(false);
    if st.live {
        let reps = item.pointer("/liveStream/playUrls/0/adaptationSet/representation").and_then(Value::as_array).cloned().unwrap_or_default();
        for (i, r) in reps.iter().enumerate() {
            if let Some(u) = str_at(r, "/url") {
                st.streams.push(LiveStream {
                    quality: str_at(r, "/name").unwrap_or("默认").to_string(),
                    rank: u64_at(r, "/bitrate").unwrap_or(i as u64) as u32,
                    url: u.to_string(),
                    format: if u.contains(".m3u8") { StreamFormat::Hls } else { StreamFormat::Flv },
                    headers: vec![("User-Agent".to_string(), DESKTOP_UA.to_string())],
                });
            }
        }
    }
    st
}

// ---------- 虎牙 ----------

async fn huya(ctx: &Ctx<'_>, rid: &str) -> AppResult<LiveStatus> {
    let url = format!("https://www.huya.com/{rid}");
    let html = ctx.get(&url).header("User-Agent", DESKTOP_UA).send().await?.text().await?;
    ctx.record("live", "huya", &url, &html);
    let stream = extract_json_after(&html, "stream: ").or_else(|| extract_json_after(&html, "\"stream\":"));
    Ok(parse_huya(stream.as_ref(), &html, rid))
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
}

pub fn parse_huya(stream: Option<&Value>, html: &str, rid: &str) -> LiveStatus {
    let mut st = LiveStatus::offline("huya", "虎牙", rid);
    let Some(data) = stream.and_then(|s| s.pointer("/data/0")) else {
        st.title = TITLE_RE.captures(html).map(|c| c[1].trim().to_string()).unwrap_or_default();
        return st;
    };
    st.streamer = str_at(data, "/gameLiveInfo/nick").unwrap_or("").to_string();
    st.title = str_at(data, "/gameLiveInfo/introduction").unwrap_or("").to_string();
    st.cover = str_at(data, "/gameLiveInfo/screenshot").map(String::from);
    st.avatar = str_at(data, "/gameLiveInfo/avatar180").map(String::from);
    let list = data.get("gameStreamInfoList").and_then(Value::as_array).cloned().unwrap_or_default();
    st.live = !list.is_empty();
    let headers = vec![("User-Agent".to_string(), DESKTOP_UA.to_string()), ("Referer".to_string(), "https://www.huya.com/".to_string())];
    for (i, s) in list.iter().enumerate() {
        let (Some(name), Some(base)) = (str_at(s, "/sStreamName"), str_at(s, "/sFlvUrl")) else { continue };
        let cdn = str_at(s, "/sCdnType").unwrap_or("CDN");
        let suffix = str_at(s, "/sFlvUrlSuffix").unwrap_or("flv");
        let anti = html_unescape(str_at(s, "/sFlvAntiCode").unwrap_or(""));
        st.streams.push(LiveStream {
            quality: format!("原画（{cdn}）"),
            rank: 100 - i as u32,
            url: format!("{base}/{name}.{suffix}?{anti}"),
            format: StreamFormat::Flv,
            headers: headers.clone(),
        });
    }
    st
}

static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<title>(.*?)</title>").unwrap());

// ---------- 直接填写的直播流 / yt-dlp ----------

async fn direct(ctx: &Ctx<'_>, url: &str) -> AppResult<LiveStatus> {
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
    let mut st = LiveStatus::offline("direct", "直播流", url);
    st.streamer = host.clone();
    st.title = "直播流".into();
    // 能连上即视为开播
    let resp = ctx.get(url).header("User-Agent", DESKTOP_UA).send().await;
    st.live = resp.is_ok_and(|r| r.status().is_success());
    if st.live {
        st.streams.push(LiveStream {
            quality: "原画".into(),
            rank: 1,
            url: url.to_string(),
            format: if url.to_ascii_lowercase().contains(".m3u8") { StreamFormat::Hls } else { StreamFormat::Flv },
            headers: vec![("User-Agent".to_string(), DESKTOP_UA.to_string())],
        });
    }
    Ok(st)
}

async fn via_ytdlp(ctx: &Ctx<'_>, url: &str) -> AppResult<LiveStatus> {
    let (mut cmd, _cookie) = ytdlp::command(ctx, url)?;
    cmd.args(["-J", "--no-playlist", "--"]).arg(url).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let out = tokio::time::timeout(std::time::Duration::from_secs(90), cmd.output())
        .await
        .map_err(|_| AppError::new(crate::error::ErrorKind::Network, "yt-dlp 检测超时"))?
        .map_err(|e| AppError::new(crate::error::ErrorKind::NeedUpdate, format!("无法运行 yt-dlp：{e}")))?;
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(crate::cookies::registrable_domain)).unwrap_or_default();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        // 未开播时 yt-dlp 报错，不算异常
        if err.contains("offline") || err.contains("not currently live") || err.contains("is not live") || err.contains("will begin") || err.contains("未开播")
        {
            let mut st = LiveStatus::offline(&host, &host, url);
            st.streamer = host.clone();
            return Ok(st);
        }
        return Err(ytdlp::classify_stderr(&String::from_utf8_lossy(&out.stderr)));
    }
    let v: Value = serde_json::from_slice(&out.stdout)?;
    Ok(parse_ytdlp_live(&v, url, &host))
}

pub fn parse_ytdlp_live(v: &Value, url: &str, host: &str) -> LiveStatus {
    let key = str_at(v, "/extractor_key").unwrap_or(host);
    let mut st = LiveStatus::offline(&key.to_ascii_lowercase(), key, url);
    st.streamer = str_at(v, "/uploader").or_else(|| str_at(v, "/channel")).unwrap_or(host).to_string();
    st.title = str_at(v, "/title").unwrap_or("").to_string();
    st.cover = str_at(v, "/thumbnail").map(String::from);
    st.live = v.get("is_live").and_then(Value::as_bool).unwrap_or(false) || str_at(v, "/live_status") == Some("is_live");
    if st.live {
        for f in v.get("formats").and_then(Value::as_array).into_iter().flatten() {
            let Some(u) = str_at(f, "/url") else { continue };
            let proto = str_at(f, "/protocol").unwrap_or("");
            let format = if proto.starts_with("m3u8") {
                StreamFormat::Hls
            } else if proto.starts_with("http") && (u.contains(".flv") || str_at(f, "/ext") == Some("flv")) {
                StreamFormat::Flv
            } else {
                continue;
            };
            // 只录音视频合一的流
            if str_at(f, "/vcodec") == Some("none") || str_at(f, "/acodec") == Some("none") {
                continue;
            }
            let h = u64_at(f, "/height").unwrap_or(0);
            let headers = f
                .get("http_headers")
                .and_then(Value::as_object)
                .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect())
                .unwrap_or_default();
            st.streams.push(LiveStream {
                quality: if h > 0 { format!("{h}P") } else { str_at(f, "/format_id").unwrap_or("默认").to_string() },
                rank: h as u32,
                url: u.to_string(),
                format,
                headers,
            });
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms() {
        assert_eq!(platform_of("https://live.bilibili.com/21452505?x"), "bilibili");
        assert_eq!(platform_of("https://live.douyin.com/123456789"), "douyin");
        assert_eq!(platform_of("https://live.kuaishou.com/u/3xabc"), "kuaishou");
        assert_eq!(platform_of("https://www.huya.com/kpl"), "huya");
        assert_eq!(platform_of("http://1.2.3.4/live/a.flv?k=1"), "direct");
        assert_eq!(platform_of("https://www.douyu.com/9999"), "ytdlp");
        assert_eq!(min_interval("bilibili"), 30);
    }

    #[test]
    fn bili_play() {
        let v = serde_json::json!({"data": {"playurl_info": {"playurl": {
            "g_qn_desc": [{"qn": 10000, "desc": "原画"}, {"qn": 400, "desc": "蓝光"}],
            "stream": [
                {"protocol_name": "http_stream", "format": [{"format_name": "flv", "codec": [
                    {"codec_name": "avc", "current_qn": 10000, "base_url": "/live-bvc/x.flv?", "url_info": [{"host": "https://cn-gd.bilivideo.com", "extra": "expires=1"}]},
                    {"codec_name": "hevc", "current_qn": 10000, "base_url": "/h.flv?", "url_info": [{"host": "https://h", "extra": ""}]}]}]},
                {"protocol_name": "http_hls", "format": [
                    {"format_name": "ts", "codec": [{"codec_name": "avc", "current_qn": 400, "base_url": "/x.m3u8?", "url_info": [{"host": "https://h2", "extra": "a=1"}]}]},
                    {"format_name": "fmp4", "codec": [{"codec_name": "avc", "current_qn": 10000, "base_url": "/f.m3u8?", "url_info": [{"host": "https://h3", "extra": ""}]}]}]}]}}}});
        let s = parse_bili_play(&v);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].url, "https://cn-gd.bilivideo.com/live-bvc/x.flv?expires=1");
        assert_eq!(s[0].quality, "原画");
        assert_eq!(s[1].format, StreamFormat::Hls);
        let st = LiveStatus { streams: s, ..LiveStatus::offline("bilibili", "B站", "1") };
        assert_eq!(st.pick("").unwrap().quality, "原画");
        assert_eq!(st.pick("蓝光").unwrap().format, StreamFormat::Hls);
        assert_eq!(st.pick("不存在").unwrap().quality, "原画");
    }

    #[test]
    fn douyin_room() {
        let v = serde_json::json!({"data": {"user": {"nickname": "主播", "avatar_thumb": {"url_list": ["https://a/1.jpg"]}},
            "data": [{"status": 2, "title": "今晚开播", "stream_url": {"flv_pull_url": {"FULL_HD1": "https://pull/x_or4.flv", "SD1": "https://pull/x_sd.flv"}, "hls_pull_url_map": {"FULL_HD1": "https://pull/x.m3u8"}}}]}});
        let st = parse_douyin(&v, "1").unwrap();
        assert!(st.live);
        assert_eq!(st.streamer, "主播");
        assert_eq!(st.pick("").unwrap().url, "https://pull/x_or4.flv");
        assert_eq!(st.streams.len(), 3);
        let off = serde_json::json!({"data": {"user": {"nickname": "主播"}, "data": [{"status": 4, "title": ""}]}});
        assert!(!parse_douyin(&off, "1").unwrap().live);
    }

    #[test]
    fn kuaishou_and_huya() {
        let state = serde_json::json!({"liveroom": {"playList": [{"isLiving": true, "author": {"name": "快手主播"},
            "liveStream": {"caption": "标题", "playUrls": [{"adaptationSet": {"representation": [{"url": "https://k/a.flv", "name": "高清", "bitrate": 2000}, {"url": "https://k/b.flv", "name": "蓝光", "bitrate": 4000}]}}]}}]}});
        let k = parse_kuaishou(&state, "x");
        assert!(k.live);
        assert_eq!(k.pick("").unwrap().quality, "蓝光");
        let stream = serde_json::json!({"data": [{"gameLiveInfo": {"nick": "虎牙主播", "introduction": "比赛"},
            "gameStreamInfoList": [{"sStreamName": "123-abc", "sFlvUrl": "https://al.flv.huya.com/src", "sFlvUrlSuffix": "flv", "sFlvAntiCode": "wsSecret=1&amp;wsTime=2", "sCdnType": "AL"}]}]});
        let h = parse_huya(Some(&stream), "", "kpl");
        assert!(h.live);
        assert_eq!(h.streams[0].url, "https://al.flv.huya.com/src/123-abc.flv?wsSecret=1&wsTime=2");
        assert!(!parse_huya(None, "<title>虎牙直播</title>", "kpl").live);
    }

    #[test]
    fn ytdlp_live() {
        let v = serde_json::json!({"extractor_key": "Twitch", "uploader": "streamer", "title": "live!", "is_live": true, "formats": [
            {"format_id": "audio_only", "url": "https://t/a.m3u8", "protocol": "m3u8_native", "vcodec": "none"},
            {"format_id": "720p", "url": "https://t/720.m3u8", "protocol": "m3u8_native", "height": 720},
            {"format_id": "1080p", "url": "https://t/1080.m3u8", "protocol": "m3u8_native", "height": 1080}]});
        let st = parse_ytdlp_live(&v, "https://twitch.tv/x", "twitch.tv");
        assert!(st.live);
        assert_eq!(st.streams.len(), 2);
        assert_eq!(st.pick("").unwrap().quality, "1080P");
    }
}
