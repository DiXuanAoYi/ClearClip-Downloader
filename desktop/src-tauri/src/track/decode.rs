//! 用 ffmpeg 把视频里的一段解成低分辨率灰度帧，给运动感知和追踪用。
//!
//! 每一帧都带自己的真实时间（用 `showinfo` 读出来），不假设帧率恒定：素材是可变帧率、或者帧率和取样率不是整数倍时，
//! 位置和时间照样对得上。取样用 `select` 按最小间隔挑帧（而不是 `fps` 滤镜——它按取整后的时间格子重复或丢帧，会差出一帧）。
//!
//! 参照帧的时间要对上界面里看到的那一帧：暂停在某个时刻时，界面显示的是“时间不晚于它的最近一帧”，
//! 而 ffmpeg 的 `-ss` 取“时间不早于它的第一帧”，所以起点往前挪一帧再取第一帧。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::gray::Gray;
use super::tracker::{Frames, Timed};
use crate::error::AppResult;
use crate::postprocess::run_ffmpeg_capture;
use crate::vidcaps::Caps;

/// 追踪用的画面长边（像素）。再大对追踪精度帮助不大，速度和内存却差很多。
pub const TRACK_LONG_SIDE: usize = 480;
/// 每次解多长（毫秒）：480×270 的画面约 130 KB 一帧，15 帧/秒 × 6 秒约 12 MB
const CHUNK_MS: f64 = 6000.0;

/// 追踪用的画面尺寸：保持比例，长边不超过 [`TRACK_LONG_SIDE`]，不放大。
pub fn track_size(src_w: u32, src_h: u32) -> (usize, usize) {
    let (w, h) = (src_w.max(1) as f64, src_h.max(1) as f64);
    let s = (TRACK_LONG_SIDE as f64 / w.max(h)).min(1.0);
    (((w * s).round() as usize).max(16), ((h * s).round() as usize).max(16))
}

#[derive(Clone)]
pub struct Decoder {
    pub ffmpeg: PathBuf,
    /// 不要补帧丢帧：`-fps_mode passthrough`（老版本 ffmpeg 是 `-vsync 0`）
    passthrough: Vec<String>,
}

/// 从 showinfo 的输出里按顺序取出每一帧的 `pts_time`（秒）。
fn pts_times(stderr: &str) -> Vec<f64> {
    stderr
        .lines()
        .filter(|l| l.contains("showinfo") && l.contains("pts_time:"))
        .filter_map(|l| {
            let rest = l.split("pts_time:").nth(1)?;
            rest.split_whitespace().next()?.parse::<f64>().ok()
        })
        .collect()
}

impl Decoder {
    pub fn new(ffmpeg: &Path, caps: &Caps) -> Decoder {
        let passthrough = if caps.has_fps_mode() { vec!["-fps_mode".into(), "passthrough".into()] } else { vec!["-vsync".into(), "0".into()] };
        Decoder { ffmpeg: ffmpeg.to_path_buf(), passthrough }
    }

    /// 取时间在 `[start_ms, start_ms + span_ms)` 里的帧。`min_gap_ms` 有值时，相邻两帧至少相隔这么久（从第一帧起挑选）；
    /// 没有值就是每一帧。返回的帧按时间排列，到视频结尾不够长时帧数会少。
    pub async fn window(&self, file: &str, start_ms: f64, span_ms: f64, min_gap_ms: Option<f64>, size: (usize, usize)) -> AppResult<Vec<Timed>> {
        let start = start_ms.max(0.0);
        let mut vf = String::new();
        if let Some(g) = min_gap_ms {
            vf.push_str(&format!("select='isnan(prev_selected_t)+gte(t-prev_selected_t,{:.4})',", g / 1000.0));
        }
        vf.push_str(&format!("scale={}:{}:flags=area,format=gray,showinfo", size.0, size.1));
        let mut a: Vec<String> = ["-hide_banner", "-loglevel", "info", "-nostats", "-y"].iter().map(|s| s.to_string()).collect();
        a.extend(["-ss".into(), format!("{:.3}", start / 1000.0), "-t".into(), format!("{:.3}", span_ms / 1000.0), "-i".into(), file.to_string()]);
        a.extend(["-an".into(), "-sn".into(), "-dn".into(), "-vf".into(), vf]);
        a.extend(self.passthrough.iter().cloned());
        a.extend(["-f".into(), "rawvideo".into(), "-pix_fmt".into(), "gray".into(), "-".into()]);
        let (raw, log) = run_ffmpeg_capture(&self.ffmpeg, &a, Duration::from_secs(180)).await?;
        let times = pts_times(&log);
        let n = size.0 * size.1;
        Ok(raw
            .chunks_exact(n)
            .zip(times)
            .filter_map(|(c, t)| Some(Timed { gray: Gray::from_raw(size.0, size.1, c.to_vec())?, t_ms: start + t * 1000.0 }))
            .collect())
    }
}

/// 追踪用的取帧：从参照帧出发，向后（时间更晚）、向前（时间更早）各自分块解码，每次只在内存里放一块。
pub struct FfFrames {
    dec: Decoder,
    file: String,
    handle: tokio::runtime::Handle,
    size: (usize, usize),
    frame_ms: f64,
    /// 取样的最小间隔
    gap_ms: f64,
    ref_ms: f64,
    back_ms: f64,
    fwd_ms: f64,
    ref_t: f64,
    fwd_q: VecDeque<Timed>,
    fwd_cursor: f64,
    fwd_done: bool,
    /// 按时间从早到晚存放，取的时候从最晚的一端取
    bwd_q: Vec<Timed>,
    bwd_cursor: f64,
    bwd_done: bool,
}

impl FfFrames {
    /// `ref_ms` 是参照帧的时间，`frame_ms` 是素材一帧的时长，`back_ms` / `fwd_ms` 是参照帧前后要追踪多远，`fps` 是取样的帧率上限。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handle: tokio::runtime::Handle,
        dec: Decoder,
        file: &str,
        ref_ms: u64,
        frame_ms: f64,
        back_ms: u64,
        fwd_ms: u64,
        fps: f64,
        size: (usize, usize),
    ) -> FfFrames {
        FfFrames {
            dec,
            file: file.to_string(),
            handle,
            size,
            frame_ms,
            gap_ms: 0.9 * 1000.0 / fps,
            ref_ms: ref_ms as f64,
            back_ms: back_ms as f64,
            fwd_ms: fwd_ms as f64,
            ref_t: ref_ms as f64,
            fwd_q: VecDeque::new(),
            fwd_cursor: ref_ms as f64,
            fwd_done: false,
            bwd_q: vec![],
            bwd_cursor: ref_ms as f64,
            bwd_done: false,
        }
    }

    fn window(&self, start: f64, span: f64) -> Result<Vec<Timed>, String> {
        self.handle.block_on(self.dec.window(&self.file, start, span, Some(self.gap_ms), self.size)).map_err(|e| e.message)
    }
}

impl Frames for FfFrames {
    fn reference(&mut self) -> Result<Option<Timed>, String> {
        let start = (self.ref_ms - self.frame_ms + 1.0).max(0.0);
        let frames = self.handle.block_on(self.dec.window(&self.file, start, 2.5 * self.frame_ms, None, self.size)).map_err(|e| e.message)?;
        let Some(first) = frames.into_iter().next() else { return Ok(None) };
        self.ref_t = first.t_ms;
        self.fwd_cursor = first.t_ms + self.gap_ms;
        self.bwd_cursor = first.t_ms - self.gap_ms;
        Ok(Some(first))
    }

    fn next_forward(&mut self) -> Result<Option<Timed>, String> {
        loop {
            if let Some(f) = self.fwd_q.pop_front() {
                return Ok(Some(f));
            }
            if self.fwd_done {
                return Ok(None);
            }
            let span = (self.ref_t + self.fwd_ms - self.fwd_cursor).min(CHUNK_MS);
            if span < self.frame_ms * 0.5 {
                self.fwd_done = true;
                continue;
            }
            let frames = self.window(self.fwd_cursor, span)?;
            match frames.last() {
                None => self.fwd_done = true,
                Some(l) => {
                    self.fwd_cursor = l.t_ms + self.gap_ms;
                    self.fwd_q.extend(frames);
                }
            }
        }
    }

    fn next_backward(&mut self) -> Result<Option<Timed>, String> {
        loop {
            if let Some(f) = self.bwd_q.pop() {
                return Ok(Some(f));
            }
            if self.bwd_done {
                return Ok(None);
            }
            let lowest = (self.ref_t - self.back_ms).max(0.0);
            let start = (self.bwd_cursor - CHUNK_MS).max(lowest);
            let span = self.bwd_cursor - start;
            if span < self.frame_ms * 0.5 {
                self.bwd_done = true;
                continue;
            }
            let frames = self.window(start, span)?;
            match frames.first() {
                None => self.bwd_done = true,
                Some(f) => {
                    self.bwd_cursor = f.t_ms - self.gap_ms;
                    self.bwd_q = frames;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_frames_keep_the_aspect_and_never_grow() {
        assert_eq!(track_size(1920, 1080), (480, 270));
        assert_eq!(track_size(1080, 1920), (270, 480));
        assert_eq!(track_size(320, 240), (320, 240));
        assert_eq!(track_size(3840, 800), (480, 100));
        assert_eq!(track_size(0, 0), (16, 16));
    }

    #[test]
    fn frame_times_are_read_from_showinfo_lines() {
        let log = "[Parsed_showinfo_2 @ 0x55d0c8a0] n:   0 pts:      0 pts_time:0       duration:  1 duration_time:0.04\n\
                   [Parsed_showinfo_2 @ 0x55d0c8a0] n:   1 pts:   1280 pts_time:0.08333 pos:123 fmt:gray\n\
                   [info] something else pts_time:9\n\
                   [Parsed_showinfo_2 @ 0x55d0c8a0] n:   2 pts:   2 pts_time:1.5e-1 pos:1\n";
        assert_eq!(pts_times(log), vec![0.0, 0.08333, 0.15]);
        assert!(pts_times("").is_empty());
    }
}
