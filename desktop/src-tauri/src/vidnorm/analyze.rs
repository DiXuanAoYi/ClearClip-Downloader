//! 需要解码才能知道的信息：可变帧率、黑边、响度；以及“处理前 / 处理后”的单帧预览。

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::postprocess::run_ffmpeg_capture;

use super::facts::Facts;
use super::spec::{Analysis, Crop, Loudness, VfrInfo};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---------- 可变帧率 ----------

/// 从 showinfo 的输出里取出每一帧的时间戳（秒）。
pub fn parse_showinfo(stderr: &str) -> Vec<f64> {
    stderr
        .lines()
        .filter(|l| l.contains("showinfo") && l.contains(" n:"))
        .filter_map(|l| l.split("pts_time:").nth(1)?.split_whitespace().next()?.parse::<f64>().ok())
        .collect()
}

/// 由帧时间戳判断是不是可变帧率。至少要 10 帧。
pub fn vfr_from_times(times: &[f64]) -> Option<VfrInfo> {
    if times.len() < 10 {
        return None;
    }
    let mut deltas: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 1e-6).collect();
    if deltas.len() < 8 {
        return None;
    }
    deltas.sort_by(f64::total_cmp);
    let pick = |q: f64| deltas[((deltas.len() - 1) as f64 * q).round() as usize];
    let median = pick(0.5);
    // 帧间隔偏离中位数 25% 以上的算“不均匀”，超过一成就是可变帧率
    let off = deltas.iter().filter(|d| (**d - median).abs() > median * 0.25).count();
    // 间隔越小帧率越高：最快的帧率取 5% 分位的间隔，最慢的取 95% 分位的间隔
    Some(VfrInfo {
        variable: off as f64 / deltas.len() as f64 > 0.1,
        median_fps: 1.0 / median,
        min_fps: 1.0 / pick(0.95),
        max_fps: 1.0 / pick(0.05),
        frames: times.len() as u32,
    })
}

pub async fn detect_vfr(ffmpeg: &Path, file: &Path) -> Option<VfrInfo> {
    let mut a = args(&["-hide_banner", "-nostats", "-i"]);
    a.push(file.to_string_lossy().into_owned());
    a.extend(args(&["-map", "0:v:0", "-an", "-sn", "-vf", "showinfo", "-frames:v", "240", "-f", "null", "-"]));
    let (_, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(60)).await.ok()?;
    vfr_from_times(&parse_showinfo(&err))
}

// ---------- 黑边 ----------

/// 取 cropdetect 输出里最后一个 `crop=w:h:x:y`。
pub fn parse_cropdetect(stderr: &str) -> Option<(u32, u32, u32, u32)> {
    let line = stderr.lines().rev().find(|l| l.contains("cropdetect") && l.contains("crop="))?;
    let rest = line.rsplit("crop=").next()?.split_whitespace().next()?;
    let mut it = rest.split(':').map(|v| v.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next()??, it.next()??))
}

/// 多处采样结果取并集（保守：宁可少裁，也不能把内容裁掉），再判断值不值得裁。
pub fn merge_crops(samples: &[(u32, u32, u32, u32)], src_w: u32, src_h: u32) -> Option<Crop> {
    if samples.is_empty() || src_w < 64 || src_h < 64 {
        return None;
    }
    let left = samples.iter().map(|c| c.2).min()? & !1;
    let top = samples.iter().map(|c| c.3).min()? & !1;
    let right = samples.iter().map(|c| c.2 + c.0).max()?.min(src_w);
    let bottom = samples.iter().map(|c| c.3 + c.1).max()?.min(src_h);
    let (w, h) = (right.saturating_sub(left) & !1, bottom.saturating_sub(top) & !1);
    if w < src_w / 2 || h < src_h / 2 {
        // 裁掉大半个画面：多半是画面本身很暗，不是黑边
        return None;
    }
    let (rm_w, rm_h) = (src_w - w, src_h - h);
    // 每个方向至少 8 像素，且整体至少去掉 2% 的面积，否则不值得重新编码
    let area_cut = 1.0 - f64::from(w) * f64::from(h) / (f64::from(src_w) * f64::from(src_h));
    if (rm_w < 8 && rm_h < 8) || area_cut < 0.02 {
        return None;
    }
    Some(Crop { w, h, x: left, y: top, src_w, src_h })
}

pub async fn detect_crop(ffmpeg: &Path, file: &Path, facts: &Facts) -> Option<Crop> {
    let v = facts.video.as_ref()?;
    let (sw, sh) = v.display_size();
    let dur = facts.duration_ms.unwrap_or(0) as f64 / 1000.0;
    let marks: Vec<f64> = if dur > 20.0 {
        vec![0.1, 0.3, 0.5, 0.7, 0.9]
    } else if dur > 3.0 {
        vec![0.2, 0.5, 0.8]
    } else {
        vec![0.0]
    };
    let jobs = marks.iter().map(|m| async move {
        let mut a = args(&["-hide_banner", "-nostats", "-ss"]);
        a.push(format!("{:.2}", dur * m));
        a.extend(args(&["-i"]));
        a.push(file.to_string_lossy().into_owned());
        a.extend(args(&["-map", "0:v:0", "-an", "-sn", "-vf", "cropdetect=limit=24:round=2:reset=0", "-frames:v", "30", "-f", "null", "-"]));
        let (_, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(60)).await.ok()?;
        parse_cropdetect(&err)
    });
    let found: Vec<_> = futures_util::future::join_all(jobs).await.into_iter().flatten().collect();
    merge_crops(&found, sw, sh)
}

// ---------- 响度 ----------

/// 解析 loudnorm 打印的 JSON。静音（-inf）等无法使用的结果返回 None。
pub fn parse_loudnorm(stderr: &str) -> Option<Loudness> {
    let end = stderr.rfind('}')?;
    let start = stderr[..end].rfind('{')?;
    let v: serde_json::Value = serde_json::from_str(&stderr[start..=end]).ok()?;
    let num = |k: &str| v.get(k)?.as_str()?.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    Some(Loudness {
        input_i: num("input_i")?,
        input_tp: num("input_tp")?,
        input_lra: num("input_lra")?,
        input_thresh: num("input_thresh")?,
        target_offset: num("target_offset")?,
    })
}

/// 第一遍：测量整段音频的响度（两遍标准化比单遍动态标准化保真，不会忽大忽小）。
pub async fn measure_loudness(ffmpeg: &Path, file: &Path, target: f64) -> Option<Loudness> {
    let mut a = args(&["-hide_banner", "-nostats", "-i"]);
    a.push(file.to_string_lossy().into_owned());
    a.extend(args(&["-map", "0:a:0", "-vn", "-sn", "-af"]));
    a.push(format!("loudnorm=I={target}:TP=-1.5:LRA=11:print_format=json"));
    a.extend(args(&["-f", "null", "-"]));
    let (_, err) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(30 * 60)).await.ok()?;
    parse_loudnorm(&err)
}

/// 界面里“检测”按钮用的分析：帧率和黑边（响度要读完整段音频，放到真正处理时再测）。
pub async fn analyze_quick(ffmpeg: &Path, file: &Path, facts: &Facts) -> Analysis {
    let (vfr, crop) = tokio::join!(detect_vfr(ffmpeg, file), detect_crop(ffmpeg, file, facts));
    Analysis { vfr, crop, ..Default::default() }
}

// ---------- 预览 ----------

/// 截取某一时刻的“处理前”和“处理后”两张图（宽度不超过 960）。`graph` 是 `video_graph` 生成的滤镜图（输入 `[0:v:0]`，输出 `[v]`）。
/// 从 `start_ms` 处开始读，丢掉前 `warm_ms` 毫秒再取一帧：降噪这类要看前后几帧的滤镜，需要先“预热”才和真正处理的结果一样。
pub async fn preview(ffmpeg: &Path, file: &Path, start_ms: u64, warm_ms: u64, graph: Option<&str>, dir: &Path, key: &str) -> AppResult<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let (before, after) = (dir.join(format!("{key}-before.jpg")), dir.join(format!("{key}-after.jpg")));
    let at = format!("{:.3}", start_ms as f64 / 1000.0);
    // 前面预热的画面丢掉，只留时间点那一帧
    let shrink = if warm_ms > 0 {
        format!("select='gte(t,{:.3})',scale='min(960,iw)':-2", warm_ms as f64 / 1000.0 - 0.001)
    } else {
        "scale='min(960,iw)':-2".to_string()
    };
    let mk = |out: &Path, complex: String| {
        let mut a = args(&["-hide_banner", "-loglevel", "error", "-y", "-ss"]);
        a.push(at.clone());
        a.push("-i".into());
        a.push(file.to_string_lossy().into_owned());
        a.extend(["-filter_complex".into(), complex, "-map".into(), "[p]".into()]);
        a.extend(args(&["-frames:v", "1", "-q:v", "3"]));
        a.push(out.to_string_lossy().into_owned());
        a
    };
    let before_args = mk(&before, format!("[0:v:0]{shrink}[p]"));
    let after_args = mk(
        &after,
        match graph {
            Some(g) => format!("{g};[v]{shrink}[p]"),
            None => format!("[0:v:0]{shrink}[p]"),
        },
    );
    let (b, a) =
        tokio::join!(run_ffmpeg_capture(ffmpeg, &before_args, Duration::from_secs(60)), run_ffmpeg_capture(ffmpeg, &after_args, Duration::from_secs(120)));
    b?;
    a?;
    for p in [&before, &after] {
        if !std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false) {
            return Err(AppError::invalid("没有截到画面，可能是预览的时间点超出了视频长度。"));
        }
    }
    Ok((before, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showinfo_times() {
        let log = "[Parsed_showinfo_0 @ 0x55] config in time_base: 1/90000, frame_rate: 30/1\n[Parsed_showinfo_0 @ 0x55] n:   0 pts:      0 pts_time:0       pos:   48 fmt:yuv420p\n[Parsed_showinfo_0 @ 0x55] n:   1 pts:   3000 pts_time:0.0333333 pos: 100\n[Parsed_showinfo_0 @ 0x55] n:   2 pts:   6000 pts_time:0.0666667 pos: 200\nsome other line pts_time:9\n";
        assert_eq!(parse_showinfo(log), vec![0.0, 0.0333333, 0.0666667]);
    }

    #[test]
    fn vfr_classification() {
        let cfr: Vec<f64> = (0..60).map(|i| f64::from(i) / 30.0).collect();
        let r = vfr_from_times(&cfr).unwrap();
        assert!(!r.variable && (r.median_fps - 30.0).abs() < 0.01, "{r:?}");
        // 带一点抖动的固定帧率（时间戳取整到毫秒）
        let jitter: Vec<f64> = (0..60).map(|i| ((f64::from(i) / 30.0) * 1000.0).round() / 1000.0).collect();
        assert!(!vfr_from_times(&jitter).unwrap().variable);
        // 录屏：画面静止时帧很少，运动时帧很密
        let mut t = 0.0;
        let vfr: Vec<f64> = (0..80)
            .map(|i| {
                t += if i % 3 == 0 { 0.5 } else { 0.016 };
                t
            })
            .collect();
        let r = vfr_from_times(&vfr).unwrap();
        assert!(r.variable && r.min_fps < 5.0 && r.max_fps > 50.0, "{r:?}");
        assert!(vfr_from_times(&[0.0, 0.1, 0.2]).is_none(), "帧数太少不下结论");
    }

    #[test]
    fn cropdetect_parsing_and_merging() {
        let log = "[Parsed_cropdetect_0 @ 0x1] x1:0 x2:1079 y1:96 y2:2303 w:1080 h:2208 x:0 y:96 pts:1 t:0.03 limit:0.094118 crop=1080:2208:0:96\n[Parsed_cropdetect_0 @ 0x1] x1:0 x2:1079 y1:96 y2:2303 w:1080 h:2208 x:0 y:96 pts:2 t:0.06 limit:0.094118 crop=1080:2208:0:96\n";
        assert_eq!(parse_cropdetect(log), Some((1080, 2208, 0, 96)));
        assert_eq!(parse_cropdetect("nothing"), None);
        // 并集：暗场景检测出的小矩形不会拖累结果
        let c = merge_crops(&[(1080, 2208, 0, 96), (1080, 1900, 0, 250), (1080, 2208, 0, 96)], 1080, 2400).unwrap();
        assert_eq!((c.w, c.h, c.x, c.y), (1080, 2208, 0, 96));
        assert_eq!(c.margins(), (96, 96, 0, 0));
        // 没有黑边 / 太小的差别 / 几乎全黑
        assert!(merge_crops(&[(1920, 1080, 0, 0)], 1920, 1080).is_none());
        assert!(merge_crops(&[(1920, 1074, 0, 2)], 1920, 1080).is_none(), "6 像素不值得");
        assert!(merge_crops(&[(400, 200, 100, 100)], 1920, 1080).is_none(), "裁掉大半个画面多半是误判");
        assert!(merge_crops(&[], 1920, 1080).is_none());
        // 奇数坐标取偶
        let c = merge_crops(&[(1000, 560, 51, 41)], 1280, 720).unwrap();
        assert_eq!((c.x % 2, c.y % 2, c.w % 2, c.h % 2), (0, 0, 0, 0));
    }

    #[test]
    fn loudnorm_json() {
        let log = "[Parsed_loudnorm_0 @ 0x5]\n{\n\t\"input_i\" : \"-23.81\",\n\t\"input_tp\" : \"-6.01\",\n\t\"input_lra\" : \"7.20\",\n\t\"input_thresh\" : \"-34.15\",\n\t\"output_i\" : \"-16.40\",\n\t\"output_tp\" : \"-1.50\",\n\t\"output_lra\" : \"5.60\",\n\t\"output_thresh\" : \"-26.60\",\n\t\"normalization_type\" : \"dynamic\",\n\t\"target_offset\" : \"0.40\"\n}\n";
        let l = parse_loudnorm(log).unwrap();
        assert_eq!((l.input_i, l.input_tp, l.input_lra, l.input_thresh, l.target_offset), (-23.81, -6.01, 7.20, -34.15, 0.40));
        assert!(parse_loudnorm(&log.replace("-23.81", "-inf")).is_none(), "静音测不出响度");
        assert!(parse_loudnorm("no json here").is_none());
    }
}
