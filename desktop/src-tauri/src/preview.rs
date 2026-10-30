//! 解析界面的“通用预览”：能直接播放的 HTTP 文件由本机媒体服务代理；其余（m3u8 / mpd 分片流、flv / mkv、
//! HEVC 等浏览器不认的编码、音视频分离的格式、yt-dlp 解析的流、RTMP / RTSP）交给 ffmpeg 取开头一小段，
//! 转成浏览器一定能播的 MP4 临时文件再播放。FTP / SFTP / ed2k / BT 要连上对方才有数据，不预览。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;

use crate::download::{build_request, stream_route};
use crate::engine::stream::{prepare, Prepared, PROXY_ENV};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::model::{Asset, AssetKind, MediaInfo, Protocol};
use crate::AppState;

/// 预览片段的长度（秒）。完整播放请下载后看。
pub const CLIP_SECS: u32 = 90;
/// 生成一段预览最多等多久。
const CLIP_TIMEOUT: Duration = Duration::from_secs(150);
/// 预览缓存的总大小上限；超过后删最久没用的。
const CACHE_LIMIT: u64 = 400 * 1024 * 1024;
const PREFIX: &str = "dl-preview-";

/// 浏览器（系统 WebView）一定能直接播放的 HTTP 文件格式。
pub fn natively_playable(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "mp4" | "m4v" | "webm" | "mov" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "flac")
}

/// 一个 ffmpeg 输入：地址和请求头。
#[derive(Debug, Clone, PartialEq)]
struct Input {
    url: String,
    headers: Vec<(String, String)>,
}

/// ffmpeg 的 `-headers` 参数值：每行 `名: 值`，以 CRLF 结尾。`User-Agent` 另外用 `-user_agent` 传。
fn header_block(headers: &[(String, String)]) -> (String, Option<String>) {
    let mut ua = None;
    let mut block = String::new();
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("user-agent") {
            ua = Some(v.clone());
        } else if !k.contains(['\r', '\n', ':']) && !v.contains(['\r', '\n']) {
            block.push_str(&format!("{k}: {v}\r\n"));
        }
    }
    (block, ua)
}

/// 视频是否已经是 H.264（可以不重新编码直接放进 MP4）。编码未知时先按可以处理，播不出来界面会换成转码重试。
fn copyable(asset: &Asset) -> bool {
    match asset.vcodec.as_deref().map(|c| c.to_ascii_lowercase()) {
        None => true,
        Some(c) => c == "none" || c.starts_with("avc") || c.starts_with("h264"),
    }
}

/// 转码时使用的 H.264 编码器参数；ffmpeg 没有任何 H.264 编码器时返回 None。
async fn h264_args(ffmpeg: &Path) -> Option<Vec<String>> {
    static ENC: tokio::sync::OnceCell<Option<&'static str>> = tokio::sync::OnceCell::const_new();
    let enc = *ENC.get_or_init(|| async { crate::vidcaps::detect(ffmpeg).await.encoders.h264 }).await;
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    Some(match enc? {
        "libx264" => s(&["-c:v", "libx264", "-preset", "ultrafast", "-crf", "30"]),
        other => s(&["-c:v", other, "-b:v", "1800k"]),
    })
}

/// 组装 ffmpeg 参数。`pre` 是每个输入前面的参数（代理等）。
fn build_args(inputs: &[(Input, Vec<String>)], audio_only: bool, video: Option<&[String]>, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y"].iter().map(|s| s.to_string()).collect();
    for (input, pre) in inputs {
        if input.url.starts_with("http") {
            a.extend(["-rw_timeout".to_string(), "15000000".into()]);
            let (block, ua) = header_block(&input.headers);
            if let Some(ua) = ua {
                a.extend(["-user_agent".to_string(), ua]);
            }
            if !block.is_empty() {
                a.extend(["-headers".to_string(), block]);
            }
        } else if input.url.starts_with("rtsp") {
            a.extend(["-rtsp_transport".to_string(), "tcp".into()]);
        }
        a.extend(pre.iter().cloned());
        a.extend(["-i".to_string(), input.url.clone()]);
    }
    // 画面取第一个输入，声音优先取第二个输入（音视频分离），没有就取第一个输入里的
    if audio_only {
        a.extend(["-vn".to_string(), "-map".into(), "0:a:0?".into()]);
    } else {
        a.extend(["-map".to_string(), "0:v:0".into(), "-map".into(), if inputs.len() > 1 { "1:a:0?".into() } else { "0:a:0?".to_string() }]);
        match video {
            Some(v) => {
                a.extend(["-vf".to_string(), "scale=-2:'min(540,ih)',format=yuv420p".into()]);
                a.extend(v.iter().cloned());
            }
            None => a.extend(["-c:v".to_string(), "copy".into()]),
        }
    }
    a.extend(["-c:a", "aac", "-b:a", "128k", "-ac", "2", "-t"].map(String::from));
    a.push(CLIP_SECS.to_string());
    a.extend(["-movflags", "+faststart", "-f", "mp4"].map(String::from));
    a.push(out.to_string_lossy().into_owned());
    a
}

/// yt-dlp 解析的流：向 yt-dlp 要这个格式当前的直链（音视频分离时有两条）和它需要的请求头。
async fn ytdlp_inputs(state: &AppState, asset: &Asset) -> AppResult<Vec<Input>> {
    let settings = state.settings();
    let ctx = state.parse_ctx(&settings);
    let (mut cmd, cookie_file) = crate::providers::ytdlp::command(&ctx, &asset.url)?;
    if let Some(f) = &asset.format_id {
        cmd.args(["-f", f]);
    }
    cmd.args(["--no-playlist", "--dump-single-json", "--"]).arg(&asset.url).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .map_err(|_| AppError::new(ErrorKind::Network, "yt-dlp 取直链超时"))?
        .map_err(|e| AppError::new(ErrorKind::NeedUpdate, format!("无法运行 yt-dlp：{e}")))?;
    if let Some(cf) = &cookie_file {
        cf.merge_back(&state.cookies);
    }
    if !out.status.success() {
        return Err(crate::providers::ytdlp::classify_stderr(&String::from_utf8_lossy(&out.stderr)));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| AppError::parser(format!("yt-dlp 输出无法解析：{e}")))?;
    Ok(inputs_from_ytdlp_json(&v))
}

fn inputs_from_ytdlp_json(v: &Value) -> Vec<Input> {
    let one = |f: &Value| -> Option<Input> {
        let url = f.get("url")?.as_str()?.to_string();
        let headers = f
            .get("http_headers")
            .and_then(|h| h.as_object())
            .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).filter(|(k, _)| !k.eq_ignore_ascii_case("cookie")).collect())
            .unwrap_or_default();
        Some(Input { url, headers })
    };
    match v.get("requested_formats").and_then(|r| r.as_array()) {
        Some(list) if !list.is_empty() => list.iter().filter_map(one).collect(),
        _ => one(v).into_iter().collect(),
    }
}

/// 缓存文件的名字：同一个资源、同一种处理方式只生成一次。
fn cache_name(asset: &Asset, pair: Option<&Asset>, transcode: bool) -> String {
    use sha2::{Digest, Sha256};
    let key = format!("{}|{:?}|{}|{}|{}", asset.url, asset.format_id, pair.map(|p| p.url.as_str()).unwrap_or(""), transcode, CLIP_SECS);
    format!("{PREFIX}{}.mp4", hex::encode(&Sha256::digest(key.as_bytes())[..8]))
}

fn prune(dir: &Path, keep: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(PREFIX) && n.ends_with(".mp4")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, size, path) in files {
        if total <= CACHE_LIMIT {
            break;
        }
        if path != keep && std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// 生成（或取缓存里的）预览片段，返回 MP4 文件路径。`transcode`：强制重新编码成 H.264。
pub async fn make_clip(state: &AppState, media: &MediaInfo, asset: &Asset, transcode: bool) -> AppResult<PathBuf> {
    let ffmpeg = crate::postprocess::find_ffmpeg(state)
        .ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "预览这类资源需要 ffmpeg。请在“设置 → 组件”里安装 ffmpeg 后重试，或直接下载。"))?;
    let audio_only = asset.kind == AssetKind::Audio;
    let pair = asset.pair_audio.as_ref().and_then(|id| media.assets.iter().find(|a| &a.id == id));
    let dir = state.data_dir.join("previews");
    std::fs::create_dir_all(&dir)?;
    let out = dir.join(cache_name(asset, pair, transcode));
    if std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        if let Ok(f) = std::fs::File::options().write(true).open(&out) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        return Ok(out);
    }

    let mut inputs: Vec<Input> = match asset.protocol {
        Protocol::Http | Protocol::Hls | Protocol::Stream => {
            let req = build_request(state, media, asset);
            let mut list = vec![Input { url: asset.url.clone(), headers: req.headers }];
            if let Some(p) = pair {
                list.push(Input { url: p.url.clone(), headers: build_request(state, media, p).headers });
            }
            list
        }
        Protocol::Ytdlp => ytdlp_inputs(state, asset).await?,
        Protocol::Ftp | Protocol::Sftp | Protocol::Ed2k | Protocol::Bt => {
            return Err(AppError::unsupported("FTP / SFTP / ed2k / BT 要连上对方才有数据，不能预览，下载后即可播放。"));
        }
    };
    inputs.retain(|i| !i.url.is_empty());
    if inputs.is_empty() {
        return Err(AppError::unsupported("没有取到可预览的地址。"));
    }

    // 按分流规则走代理（和下载时一致）
    let settings = state.settings();
    let mut prepared: Vec<Prepared> = vec![];
    for input in &mut inputs {
        let route = stream_route(&settings, &input.url);
        let p = prepare(&input.url, &route).await.map_err(|e| AppError::new(ErrorKind::Network, format!("无法准备预览的网络出口：{e:?}")))?;
        input.url = p.url.clone();
        prepared.push(p);
    }
    let clear_env = prepared.iter().any(|p| p.clear_proxy_env);
    let with_pre: Vec<(Input, Vec<String>)> = inputs.into_iter().zip(prepared.iter().map(|p| p.pre_input.clone())).collect();

    let video_args =
        if audio_only || (!transcode && copyable(asset)) {
            None
        } else {
            Some(h264_args(&ffmpeg).await.ok_or_else(|| {
                AppError::unsupported("这个 ffmpeg 没有 H.264 编码器，不能预览这种编码。请在“设置 → 组件”里安装完整版 ffmpeg，或下载后播放。")
            })?)
        };
    let tmp = out.with_extension("part.mp4");
    let _ = std::fs::remove_file(&tmp);
    let args = build_args(&with_pre, audio_only, video_args.as_deref(), &tmp);
    let mut cmd = tokio::process::Command::new(&ffmpeg);
    if clear_env {
        for k in PROXY_ENV {
            cmd.env_remove(k);
        }
    }
    cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let child = cmd.spawn().map_err(|e| AppError::new(ErrorKind::NeedUpdate, format!("无法运行 ffmpeg：{e}")))?;
    let result = tokio::time::timeout(CLIP_TIMEOUT, child.wait_with_output()).await;
    drop(prepared);
    let output = match result {
        Ok(r) => r.map_err(|e| AppError::msg(format!("ffmpeg 运行失败：{e}")))?,
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(AppError::new(ErrorKind::Network, "生成预览超时：网络太慢或源站没有响应。可以直接下载。"));
        }
    };
    let ok = output.status.success() && std::fs::metadata(&tmp).map(|m| m.len() > 0).unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        let err = String::from_utf8_lossy(&output.stderr);
        let tail: String = err.lines().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
        let kind = if tail.contains("403") || tail.contains("401") || tail.contains("410") { ErrorKind::NeedLogin } else { ErrorKind::Other };
        return Err(AppError::new(kind, format!("生成预览失败：{}", if tail.is_empty() { "ffmpeg 没有输出".to_string() } else { tail })));
    }
    std::fs::rename(&tmp, &out)?;
    prune(&dir, &out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(vcodec: Option<&str>) -> Asset {
        let mut a = Asset::base("v", AssetKind::Video, "https://h/a.m3u8".into(), "v", "m3u8");
        a.vcodec = vcodec.map(String::from);
        a
    }

    #[test]
    fn native_formats_are_proxied_and_the_rest_go_through_ffmpeg() {
        for e in ["mp4", "WEBM", "m4a", "mp3"] {
            assert!(natively_playable(e), "{e}");
        }
        for e in ["m3u8", "mpd", "flv", "mkv", "ts", "avi", "wmv"] {
            assert!(!natively_playable(e), "{e}");
        }
    }

    #[test]
    fn only_h264_is_copied_into_the_clip() {
        assert!(copyable(&asset(None)));
        assert!(copyable(&asset(Some("avc1.640028"))));
        assert!(copyable(&asset(Some("h264"))));
        assert!(!copyable(&asset(Some("hev1.1.6.L93"))));
        assert!(!copyable(&asset(Some("vp09.00.40.08"))));
        assert!(!copyable(&asset(Some("av01.0.08M.08"))));
    }

    #[test]
    fn headers_are_split_into_user_agent_and_a_block_and_unsafe_ones_dropped() {
        let (block, ua) = header_block(&[
            ("Referer".into(), "https://site/".into()),
            ("User-Agent".into(), "UA/1".into()),
            ("Cookie".into(), "a=b".into()),
            ("Bad\r\nHeader".into(), "x".into()),
            ("X-Evil".into(), "a\r\nInjected: 1".into()),
        ]);
        assert_eq!(block, "Referer: https://site/\r\nCookie: a=b\r\n");
        assert_eq!(ua.as_deref(), Some("UA/1"));
    }

    #[test]
    fn ffmpeg_args_cover_copy_transcode_split_audio_and_audio_only() {
        let hls = Input { url: "https://h/a.m3u8".into(), headers: vec![("Referer".into(), "https://site/".into())] };
        let audio = Input { url: "https://h/a.m4a".into(), headers: vec![] };
        let out = Path::new("/tmp/o.part.mp4");
        let join = |a: Vec<String>| a.join(" ");

        let copy = join(build_args(&[(hls.clone(), vec![])], false, None, out));
        assert!(copy.contains("-c:v copy") && copy.contains("-map 0:a:0?") && copy.contains("-t 90") && copy.contains("-i https://h/a.m3u8"), "{copy}");
        assert!(copy.contains("-headers Referer: https://site/"), "{copy}");

        let enc = vec!["-c:v".to_string(), "libx264".into()];
        let tr = join(build_args(&[(hls.clone(), vec![])], false, Some(&enc), out));
        assert!(tr.contains("-vf scale=-2:'min(540,ih)',format=yuv420p") && tr.contains("-c:v libx264") && !tr.contains("-c:v copy"), "{tr}");

        let split = join(build_args(&[(hls.clone(), vec!["-http_proxy".into(), "http://127.0.0.1:1".into()]), (audio.clone(), vec![])], false, None, out));
        assert!(split.contains("-map 0:v:0 -map 1:a:0?") && split.contains("-http_proxy http://127.0.0.1:1 -i https://h/a.m3u8"), "{split}");

        let only = join(build_args(&[(audio, vec![])], true, None, out));
        assert!(only.contains("-vn -map 0:a:0?") && !only.contains("-c:v"), "{only}");
    }

    #[test]
    fn ytdlp_json_gives_one_or_two_inputs_without_cookies() {
        let split = serde_json::json!({"requested_formats": [
            {"url": "https://r/v", "http_headers": {"User-Agent": "UA", "Cookie": "x=1", "Referer": "https://y/"}},
            {"url": "https://r/a", "http_headers": {}}
        ]});
        let list = inputs_from_ytdlp_json(&split);
        assert_eq!(list.len(), 2);
        assert!(list[0].headers.iter().all(|(k, _)| k != "Cookie") && list[0].headers.len() == 2);
        let single = serde_json::json!({"url": "https://r/x.mp4"});
        assert_eq!(inputs_from_ytdlp_json(&single), vec![Input { url: "https://r/x.mp4".into(), headers: vec![] }]);
        assert!(inputs_from_ytdlp_json(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn cache_name_depends_on_the_asset_and_the_mode() {
        let a = asset(None);
        assert_eq!(cache_name(&a, None, false), cache_name(&a, None, false));
        assert_ne!(cache_name(&a, None, false), cache_name(&a, None, true));
        let mut b = asset(None);
        b.url = "https://h/b.m3u8".into();
        assert_ne!(cache_name(&a, None, false), cache_name(&b, None, false));
    }

    // ---- 需要 ffmpeg 的端到端：本机没有 ffmpeg 时跳过 ----

    fn ffmpeg_bin() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    /// 一个本机 HTTP 服务：要求 Referer，吐出一个 ffmpeg 现做的 mkv（浏览器不认的容器）。
    #[tokio::test]
    async fn clip_from_a_hotlink_protected_mkv_plays_as_mp4() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = std::env::temp_dir().join(format!("clearclip-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.mkv");
        let st = std::process::Command::new(&ff)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=15:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=2",
                "-c:v",
                "mpeg4",
                "-c:a",
                "mp2",
            ])
            .arg(&src)
            .status()
            .unwrap();
        assert!(st.success());
        let bytes = std::fs::read(&src).unwrap();

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let body = bytes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_ascii_lowercase();
                    let ok = req.contains("referer: https://site.example/");
                    let head = if ok {
                        format!("HTTP/1.1 200 OK\r\nContent-Type: video/x-matroska\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                    } else {
                        "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                    };
                    let _ = s.write_all(head.as_bytes()).await;
                    if ok && !req.starts_with("head") {
                        let _ = s.write_all(&body).await;
                    }
                });
            }
        });

        let url = format!("http://127.0.0.1:{port}/v.mkv");
        let args = build_args(
            &[(Input { url: url.clone(), headers: vec![("Referer".into(), "https://site.example/".into())] }, vec![])],
            false,
            Some(&["-c:v".to_string(), "mpeg4".into(), "-q:v".into(), "8".into()]),
            &dir.join("out.mp4"),
        );
        let (_, err) = crate::postprocess::run_ffmpeg_capture(&ff, &args, Duration::from_secs(60)).await.map_err(|e| e.message).unwrap();
        let _ = err;
        let probe = std::process::Command::new(&ff).args(["-hide_banner", "-i"]).arg(dir.join("out.mp4")).output().unwrap();
        let info = String::from_utf8_lossy(&probe.stderr);
        assert!(info.contains("Duration: 00:00:0") && info.contains("Video:") && info.contains("aac"), "{info}");

        // 没有 Referer 时被源站拒绝，错误里带着 403
        let bad = build_args(&[(Input { url, headers: vec![] }, vec![])], false, None, &dir.join("bad.mp4"));
        let e = crate::postprocess::run_ffmpeg_capture(&ff, &bad, Duration::from_secs(30)).await.unwrap_err();
        assert!(e.message.contains("403") || e.message.to_ascii_lowercase().contains("forbidden"), "{}", e.message);
        let _ = std::fs::remove_dir_all(dir);
    }
}
