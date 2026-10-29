//! 流媒体抓取（RTMP、RTSP、MMS 等）：交给 ffmpeg 原样复制到 mkv，不重新编码。
//! 流结束（点播播完或直播主播下播）时任务完成；暂停 / 取消会让 ffmpeg 正常收尾，已抓到的内容可以播放，但再次开始时会重新抓取。

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

use super::{ensure_space, wait_ctrl, DlError};

/// 带上登录信息的地址（只用于传给 ffmpeg，不写进日志）。
fn url_with_credentials(url: &str, credentials: Option<&str>) -> String {
    let Some(c) = credentials else { return url.to_string() };
    let Ok(mut u) = url::Url::parse(url) else { return url.to_string() };
    let (user, pass) = c.split_once(':').unwrap_or((c, ""));
    let _ = u.set_username(user);
    let _ = u.set_password((!pass.is_empty()).then_some(pass));
    u.to_string()
}

/// ffmpeg 的命令行参数。
pub fn build_args(url: &str, out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-y"].iter().map(|s| s.to_string()).collect();
    let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
    if scheme.starts_with("rtsp") {
        // UDP 容易丢包或被防火墙拦截：用 TCP 传输
        a.extend(["-rtsp_transport", "tcp"].map(String::from));
    }
    if scheme != "file" && !url.starts_with('/') {
        // 15 秒收不到数据就算断线（微秒）
        a.extend(["-rw_timeout", "15000000"].map(String::from));
    }
    a.extend(["-i".to_string(), url.to_string(), "-map".into(), "0".into(), "-c".into(), "copy".into(), "-f".into(), "matroska".into()]);
    a.push(out.to_string_lossy().into_owned());
    a
}

/// 从 `-progress` 的输出行里取已写入的字节数。
pub fn parse_progress(line: &str) -> Option<u64> {
    line.strip_prefix("total_size=").and_then(|v| v.trim().parse().ok())
}

pub async fn download(
    ffmpeg: &Path,
    url: &str,
    credentials: Option<&str>,
    part: &Path,
    ctrl: &mut watch::Receiver<u8>,
    disk_reserve: u64,
    mut on_progress: impl FnMut(u64) + Send,
) -> Result<u64, DlError> {
    if let Some(dir) = part.parent() {
        ensure_space(dir, 0, disk_reserve)?;
    }
    let _ = tokio::fs::remove_file(part).await;
    let args = build_args(&url_with_credentials(url, credentials), part);
    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| DlError::Other(format!("无法运行 ffmpeg：{e}")))?;
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().ok_or_else(|| DlError::Other("无法读取 ffmpeg 输出".into()))?;
    let stderr = child.stderr.take();
    // 错误信息只留最后几行
    let err_task = tokio::spawn(async move {
        let mut tail: Vec<String> = vec![];
        if let Some(e) = stderr {
            let mut lines = BufReader::new(e).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                tail.push(l);
                if tail.len() > 6 {
                    tail.remove(0);
                }
            }
        }
        tail
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut stop: Option<u8> = None;
    loop {
        tokio::select! {
            l = lines.next_line() => match l {
                Ok(Some(l)) => {
                    if let Some(n) = parse_progress(&l) {
                        on_progress(n);
                    }
                }
                _ => break,
            },
            c = wait_ctrl(ctrl), if stop.is_none() => {
                stop = Some(c);
                // 让 ffmpeg 写完文件尾再退出
                if let Some(mut s) = stdin.take() {
                    let _ = s.write_all(b"q").await;
                    let _ = s.flush().await;
                }
                break;
            }
        }
    }
    let status = if stop.is_some() {
        match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            Ok(r) => r.ok(),
            Err(_) => {
                let _ = child.kill().await;
                None
            }
        }
    } else {
        child.wait().await.ok()
    };
    let tail = err_task.await.unwrap_or_default();
    let size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if let Some(c) = stop {
        return Err(DlError::from_ctrl(c));
    }
    match status {
        Some(s) if s.success() && size > 0 => {
            on_progress(size);
            Ok(size)
        }
        Some(s) if s.success() => Err(DlError::Other("流里没有数据".into())),
        _ => {
            let msg = tail.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
            // 已经抓到一部分时，中途断线按“流结束”处理，保留内容
            if size > 0 && !msg.to_ascii_lowercase().contains("connection refused") && !msg.contains("401") && !msg.contains("403") {
                on_progress(size);
                return Ok(size);
            }
            Err(DlError::Net(format!("ffmpeg 无法抓取这个流：{}", if msg.is_empty() { "未知错误" } else { msg.as_str() })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_bin() -> Option<std::path::PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| "ffmpeg".into())
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("cc-stream-{}-{name}", std::process::id()));
        let _ = std::fs::remove_file(&d);
        d
    }

    #[test]
    fn builds_command_lines() {
        let a = build_args("rtsp://10.0.0.2/cam", Path::new("/tmp/o.part"));
        assert!(a.windows(2).any(|w| w == ["-rtsp_transport", "tcp"]));
        assert!(a.windows(2).any(|w| w[0] == "-rw_timeout"));
        assert!(a.windows(2).any(|w| w == ["-c", "copy"]));
        assert_eq!(a.last().map(String::as_str), Some("/tmp/o.part"));
        let r = build_args("rtmp://h/app/s", Path::new("o"));
        assert!(!r.iter().any(|x| x == "-rtsp_transport"));
        let f = build_args("/tmp/x.mp4", Path::new("o"));
        assert!(!f.iter().any(|x| x == "-rw_timeout"));
    }

    #[test]
    fn credentials_are_added_for_ffmpeg_only() {
        assert_eq!(url_with_credentials("rtsp://10.0.0.2/cam", Some("admin:p@ss")), "rtsp://admin:p%40ss@10.0.0.2/cam");
        assert_eq!(url_with_credentials("rtsp://10.0.0.2/cam", None), "rtsp://10.0.0.2/cam");
        assert_eq!(url_with_credentials("rtmp://h/a", Some("u:")), "rtmp://u@h/a");
    }

    #[test]
    fn parses_progress_lines() {
        assert_eq!(parse_progress("total_size=12345"), Some(12345));
        assert_eq!(parse_progress("total_size=N/A"), None);
        assert_eq!(parse_progress("out_time_us=1"), None);
    }

    #[tokio::test]
    async fn captures_a_source_to_mkv_and_reports_failures() {
        let Some(ff) = ffmpeg_bin() else { return };
        let src = tmp("src.mp4");
        let ok = std::process::Command::new(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=10:duration=2", "-pix_fmt", "yuv420p"])
            .arg(&src)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let (_tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let out = tmp("out.mkv.part");
        let mut last = 0;
        let n = download(&ff, src.to_str().unwrap(), None, &out, &mut rx, 0, |b| last = b).await.unwrap();
        assert!(n > 1000 && last == n);
        // 抓到的文件可以被 ffmpeg 读取
        let probe = std::process::Command::new(&ff).args(["-v", "error", "-i"]).arg(&out).args(["-f", "null", "-"]).status().unwrap();
        assert!(probe.success());
        // 连不上的流报错
        let err = download(&ff, "rtmp://127.0.0.1:9/live/none", None, &tmp("bad.part"), &mut rx, 0, |_| {}).await.unwrap_err();
        assert!(matches!(err, DlError::Net(_)), "{err}");
    }

    /// 用本地 TCP 服务器慢慢推送一段 MPEG-TS，模拟直播流。
    async fn live_source(ff: &Path) -> u16 {
        let ts = tmp("live.ts");
        let ok = std::process::Command::new(ff)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=10:duration=30",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "mpegts",
            ])
            .arg(&ts)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let data = std::fs::read(&ts).unwrap();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let Ok((mut s, _)) = l.accept().await else { return };
            for chunk in data.chunks(1880) {
                if s.write_all(chunk).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn pause_finishes_the_file_and_returns_paused() {
        let Some(ff) = ffmpeg_bin() else { return };
        let port = live_source(&ff).await;
        let (tx, mut rx) = watch::channel(super::super::CTRL_RUN);
        let out = tmp("paused.part");
        let t = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            let _ = tx.send(super::super::CTRL_PAUSE);
            tx
        });
        let r = download(&ff, &format!("tcp://127.0.0.1:{port}"), None, &out, &mut rx, 0, |_| {}).await;
        let _tx = t.await.unwrap();
        assert!(matches!(r, Err(DlError::Paused)), "{r:?}");
        // 已抓到的部分是完整可播放的 mkv
        assert!(std::fs::metadata(&out).unwrap().len() > 1000);
        let probe = std::process::Command::new(&ff).args(["-v", "error", "-i"]).arg(&out).args(["-f", "null", "-"]).status().unwrap();
        assert!(probe.success());
    }
}
