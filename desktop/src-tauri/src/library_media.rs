//! 媒体库里用到的 ffmpeg 小工具：画面指纹、预览图、镜头检测、单帧截图。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::library;
use crate::model::Chapter;
use crate::postprocess::{base_args, probe, run_ffmpeg, run_ffmpeg_capture};

/// 取某个时间点的画面，缩成 9×8 的灰度图（72 字节）。
async fn grab_gray(ffmpeg: &Path, file: &Path, at_ms: u64) -> Option<Vec<u8>> {
    let mut a = base_args();
    a.extend([
        "-ss".into(),
        format!("{:.3}", at_ms as f64 / 1000.0),
        "-i".into(),
        file.to_string_lossy().into_owned(),
        "-frames:v".into(),
        "1".into(),
        "-an".into(),
        "-vf".into(),
        "scale=9:8:flags=area,format=gray".into(),
        "-f".into(),
        "rawvideo".into(),
        "pipe:1".into(),
    ]);
    let (bytes, _) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(60)).await.ok()?;
    (bytes.len() >= 72).then_some(bytes)
}

/// 视频的画面指纹：在 10%、50%、90% 处各取一帧。读不出画面时返回 `None`。
pub async fn compute_phash(ffmpeg: &Path, file: &Path, duration_ms: u64) -> Option<String> {
    let mut parts = vec![];
    for pct in [10u64, 50, 90] {
        let gray = grab_gray(ffmpeg, file, duration_ms * pct / 100).await?;
        parts.push(format!("{:016x}", library::dhash(&gray)?));
    }
    Some(parts.join("-"))
}

/// 视频时长（毫秒）。
pub async fn duration_ms(ffmpeg: &Path, file: &Path) -> Option<i64> {
    probe(ffmpeg, file).await.duration_ms.map(|d| d as i64)
}

/// 单帧截图（JPEG）。
pub async fn extract_frame(ffmpeg: &Path, file: &Path, at_ms: u64, out: &Path, width: u32) -> AppResult<()> {
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut a = base_args();
    a.extend([
        "-ss".into(),
        format!("{:.3}", at_ms as f64 / 1000.0),
        "-i".into(),
        file.to_string_lossy().into_owned(),
        "-frames:v".into(),
        "1".into(),
        "-an".into(),
        "-vf".into(),
        format!("scale={width}:-2"),
        "-q:v".into(),
        "3".into(),
        out.to_string_lossy().into_owned(),
    ]);
    run_ffmpeg(ffmpeg, &a).await
}

/// 预览图：均匀取 `cols × rows` 帧，拼成一张图。
pub async fn contact_sheet(ffmpeg: &Path, file: &Path, duration_ms: u64, out: &Path, cols: u32, rows: u32) -> AppResult<()> {
    let n = (cols * rows).max(1) as u64;
    let tmp = std::env::temp_dir().join(format!("clearclip-sheet-{}-{}", std::process::id(), crate::db::now()));
    std::fs::create_dir_all(&tmp)?;
    let result = async {
        let mut got = 0u32;
        for i in 0..n {
            // 避开第 0 秒（常是黑屏）和最后一帧
            let at = duration_ms * (2 * i + 1) / (2 * n);
            if extract_frame(ffmpeg, file, at, &tmp.join(format!("f{:03}.jpg", got + 1)), 320).await.is_ok() {
                got += 1;
            }
        }
        if got == 0 {
            return Err(AppError::msg("读不出视频画面，无法生成预览图。"));
        }
        let used_rows = got.div_ceil(cols).max(1);
        let mut a = base_args();
        a.extend([
            "-framerate".into(),
            "1".into(),
            "-i".into(),
            tmp.join("f%03d.jpg").to_string_lossy().into_owned(),
            "-vf".into(),
            format!("tile={cols}x{used_rows}:padding=4:color=black"),
            "-frames:v".into(),
            "1".into(),
            "-q:v".into(),
            "3".into(),
            out.to_string_lossy().into_owned(),
        ]);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        run_ffmpeg(ffmpeg, &a).await
    }
    .await;
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// 从 `showinfo` 的输出里取出镜头切换的时间点（秒）。
pub fn parse_scene_times(stderr: &str) -> Vec<f64> {
    let mut out = vec![];
    for line in stderr.lines() {
        if !line.contains("Parsed_showinfo") {
            continue;
        }
        if let Some(rest) = line.split("pts_time:").nth(1) {
            if let Some(t) = rest.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()) {
                out.push(t);
            }
        }
    }
    out
}

/// 检测镜头切换点（毫秒）。`threshold` 越小越敏感，常用 0.3–0.5。
pub async fn detect_scenes(ffmpeg: &Path, file: &Path, threshold: f64) -> AppResult<Vec<u64>> {
    // showinfo 在 info 级别输出，所以这里不用 -loglevel error
    let a: Vec<String> = [
        "-hide_banner".to_string(),
        "-nostats".into(),
        "-i".into(),
        file.to_string_lossy().into_owned(),
        "-an".into(),
        "-vf".into(),
        format!("select='gt(scene,{threshold:.2})',showinfo"),
        "-f".into(),
        "null".into(),
        "-".into(),
    ]
    .into();
    let (_, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(1800)).await?;
    Ok(parse_scene_times(&err).into_iter().map(|t| (t * 1000.0).round() as u64).collect())
}

/// 切换点转成连续的片段；太短的片段并入前一段。
pub fn scenes_to_chapters(cuts_ms: &[u64], duration_ms: u64, min_len_ms: u64) -> Vec<Chapter> {
    let mut points: Vec<u64> = vec![0];
    for &c in cuts_ms {
        if c > *points.last().unwrap_or(&0) + min_len_ms && c + min_len_ms <= duration_ms {
            points.push(c);
        }
    }
    points.push(duration_ms);
    points.windows(2).enumerate().map(|(i, w)| Chapter { title: format!("镜头 {}", i + 1), start_ms: w[0], end_ms: w[1] }).collect()
}

/// 预览图的缓存位置：`<数据目录>/previews/<记录 ID>.jpg`。
pub fn sheet_path(data_dir: &Path, id: i64) -> PathBuf {
    data_dir.join("previews").join(format!("{id}.jpg"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_bin() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-lm-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    async fn make(ff: &Path, out: &Path, src: &str, secs: u32) {
        let mut a = base_args();
        a.extend(["-f", "lavfi", "-i"].map(String::from));
        a.push(format!("{src}=size=320x240:rate=25"));
        a.extend(["-t".to_string(), secs.to_string()]);
        a.extend(["-c:v", "mpeg4", "-g", "25", "-pix_fmt", "yuv420p"].map(String::from));
        a.push(out.to_string_lossy().into_owned());
        run_ffmpeg(ff, &a).await.unwrap();
    }

    #[test]
    fn scene_times_parse() {
        let err = "[Parsed_showinfo_1 @ 0x1] n:   0 pts:  75 pts_time:3.0     duration: 1\n[Parsed_showinfo_1 @ 0x1] n:   1 pts: 300 pts_time:12.52 duration: 1\nother pts_time:99\n";
        assert_eq!(parse_scene_times(err), vec![3.0, 12.52]);
    }

    #[test]
    fn scenes_become_chapters() {
        let ch = scenes_to_chapters(&[500, 10_000, 10_800, 28_000, 29_900], 30_000, 2_000);
        let spans: Vec<(u64, u64)> = ch.iter().map(|c| (c.start_ms, c.end_ms)).collect();
        // 500 太近（并入开头）、10_800 与 10_000 太近、29_900 离结尾太近
        assert_eq!(spans, vec![(0, 10_000), (10_000, 28_000), (28_000, 30_000)]);
        assert_eq!(ch[1].title, "镜头 2");
        assert_eq!(scenes_to_chapters(&[], 5_000, 2_000).len(), 1);
    }

    #[tokio::test]
    async fn phash_distinguishes_videos_and_matches_reencodes() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("phash");
        let a = dir.join("a.mp4");
        let b = dir.join("b.mp4");
        make(&ff, &a, "testsrc", 6).await;
        make(&ff, &b, "mandelbrot", 6).await;
        // 重新编码成更小的版本，画面内容相同
        let small = dir.join("small.mp4");
        let mut args = base_args();
        args.extend([
            "-i".into(),
            a.to_string_lossy().into_owned(),
            "-vf".into(),
            "scale=160:120".into(),
            "-c:v".into(),
            "mpeg4".into(),
            "-q:v".into(),
            "8".into(),
            small.to_string_lossy().into_owned(),
        ]);
        run_ffmpeg(&ff, &args).await.unwrap();
        let ha = library::parse_phash(&compute_phash(&ff, &a, 6000).await.unwrap());
        let hb = library::parse_phash(&compute_phash(&ff, &b, 6000).await.unwrap());
        let hs = library::parse_phash(&compute_phash(&ff, &small, 6000).await.unwrap());
        assert_eq!(ha.len(), 3);
        assert!(library::similar(&ha, &hs, Some(6000), Some(6000)), "{ha:x?} vs {hs:x?}");
        assert!(!library::similar(&ha, &hb, Some(6000), Some(6000)), "{ha:x?} vs {hb:x?}");
        assert_eq!(duration_ms(&ff, &a).await, Some(6000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn contact_sheet_and_frame() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("sheet");
        let v = dir.join("v.mp4");
        make(&ff, &v, "testsrc", 8).await;
        let out = dir.join("p/sheet.jpg");
        contact_sheet(&ff, &v, 8000, &out, 4, 2).await.unwrap();
        assert!(std::fs::metadata(&out).unwrap().len() > 2000);
        let frame = dir.join("f.jpg");
        extract_frame(&ff, &v, 3000, &frame, 200).await.unwrap();
        assert!(frame.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn scene_detection_finds_the_cut() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("scene");
        let (a, b, list, joined) = (dir.join("a.mp4"), dir.join("b.mp4"), dir.join("list.txt"), dir.join("j.mp4"));
        make(&ff, &a, "testsrc", 4).await;
        make(&ff, &b, "mandelbrot", 4).await;
        std::fs::write(&list, format!("file '{}'\nfile '{}'\n", a.display(), b.display())).unwrap();
        let mut args = base_args();
        args.extend(["-f", "concat", "-safe", "0", "-i"].map(String::from));
        args.push(list.to_string_lossy().into_owned());
        args.extend(["-c", "copy"].map(String::from));
        args.push(joined.to_string_lossy().into_owned());
        run_ffmpeg(&ff, &args).await.unwrap();
        let cuts = detect_scenes(&ff, &joined, 0.4).await.unwrap();
        assert!(cuts.iter().any(|c| (3800..=4200).contains(c)), "{cuts:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
