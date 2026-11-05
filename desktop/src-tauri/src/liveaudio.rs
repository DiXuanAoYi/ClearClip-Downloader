//! 系统声音实时字幕：抓取电脑正在播放的声音（WASAPI 环回），按说话停顿切成一句一句，
//! 识别（本机 SenseVoice 或云端）、翻译，放进悬浮字幕条。只支持 Windows。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::error::{AppError, AppResult};

pub const RATE: u32 = 16_000;
const FRAME: usize = 320; // 20 毫秒

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: Mutex<Option<std::sync::Arc<AtomicBool>>> = Mutex::new(None);

pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

// ---------- 纯逻辑：重采样 / 分句 / 封装 wav ----------

/// 多声道取平均、线性插值重采样到 16 kHz 单声道。
pub fn mono16k(samples: &[f32], channels: usize, rate: u32) -> Vec<f32> {
    let ch = channels.max(1);
    let mono: Vec<f32> = samples.chunks(ch).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect();
    if rate == RATE || mono.is_empty() {
        return mono;
    }
    let ratio = f64::from(rate) / f64::from(RATE);
    let n = (mono.len() as f64 / ratio).floor() as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let k = p as usize;
            let t = (p - k as f64) as f32;
            let a = mono[k];
            let b = mono.get(k + 1).copied().unwrap_or(a);
            a + (b - a) * t
        })
        .collect()
}

pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + samples.len() * 2);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    v
}

fn rms(f: &[f32]) -> f32 {
    if f.is_empty() {
        return 0.0;
    }
    (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt()
}

/// 按能量切句：说话够久才算一句，安静够久就收尾，太长强行切开。
pub struct Segmenter {
    buf: Vec<f32>,
    pre: std::collections::VecDeque<Vec<f32>>,
    cur: Vec<f32>,
    speaking: bool,
    quiet: usize,
    speech: usize,
    noise: f32,
    pub min_speech: usize,
    pub end_quiet: usize,
    pub max_frames: usize,
}

impl Default for Segmenter {
    fn default() -> Self {
        Segmenter {
            buf: vec![],
            pre: Default::default(),
            cur: vec![],
            speaking: false,
            quiet: 0,
            speech: 0,
            noise: 0.004,
            min_speech: 20,
            end_quiet: 30,
            max_frames: 600,
        }
    }
}

impl Segmenter {
    fn threshold(&self) -> f32 {
        (self.noise * 3.0).max(0.012)
    }

    /// 喂入 16 kHz 单声道样本，返回已经完整的句子。
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.buf.extend_from_slice(samples);
        let mut out = vec![];
        while self.buf.len() >= FRAME {
            let frame: Vec<f32> = self.buf.drain(..FRAME).collect();
            let e = rms(&frame);
            let loud = e > self.threshold();
            if !loud && !self.speaking {
                // 只在安静时慢慢跟踪底噪
                self.noise = self.noise * 0.98 + e * 0.02;
            }
            if self.speaking {
                self.cur.extend_from_slice(&frame);
                if loud {
                    self.quiet = 0;
                    self.speech += 1;
                } else {
                    self.quiet += 1;
                }
                let frames = self.cur.len() / FRAME;
                if self.quiet >= self.end_quiet || frames >= self.max_frames {
                    if self.speech >= self.min_speech {
                        // 去掉尾部多余的安静，留一点余量
                        let keep = self.cur.len().saturating_sub(self.quiet.saturating_sub(6) * FRAME);
                        self.cur.truncate(keep.max(FRAME));
                        out.push(std::mem::take(&mut self.cur));
                    } else {
                        self.cur.clear();
                    }
                    self.speaking = false;
                    self.quiet = 0;
                    self.speech = 0;
                }
            } else {
                self.pre.push_back(frame.clone());
                while self.pre.len() > 10 {
                    self.pre.pop_front();
                }
                if loud {
                    self.speaking = true;
                    self.quiet = 0;
                    self.speech = 1;
                    self.cur = self.pre.drain(..).flatten().collect();
                }
            }
        }
        out
    }
}

/// 识别结果去掉口头“嗯”之类的空内容。
pub fn clean(text: &str) -> String {
    let t = text.trim();
    if t.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        return String::new();
    }
    t.to_string()
}

// ---------- 命令 ----------

#[tauri::command]
pub fn sc_audio_stop(app: tauri::AppHandle) {
    stop(&app);
}

pub fn stop(app: &tauri::AppHandle) {
    if let Some(s) = STOP.lock().unwrap_or_else(|e| e.into_inner()).take() {
        s.store(true, Ordering::Relaxed);
    }
    RUNNING.store(false, Ordering::Relaxed);
    crate::livetr::emit_state(app, "");
}

#[tauri::command]
pub fn sc_audio_start(app: tauri::AppHandle, state: tauri::State<'_, std::sync::Arc<crate::AppState>>) -> AppResult<crate::livetr::Status> {
    if !cfg!(windows) {
        return Err(AppError::invalid("系统声音字幕目前只支持 Windows。"));
    }
    if running() {
        return Ok(crate::livetr::status());
    }
    let st = state.inner().clone();
    let live = st.settings_raw().live;
    if live.audio_stt != "cloud" && !crate::ml::DATA_DIR.get().is_some_and(|d| crate::ml::sensevoice::find_files(d).is_some()) {
        return Err(AppError::invalid("本机语音识别还没装好。请在“设置 → AI”或“设置 → 组件”里安装 SenseVoice，或把语音识别改成云端。"));
    }
    #[cfg(windows)]
    {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        *STOP.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop.clone());
        RUNNING.store(true, Ordering::Relaxed);
        crate::livetr::ensure_subbar(&app, &live);
        crate::livetr::emit_state(&app, "系统声音字幕已开始");
        let app2 = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = win::run(&app2, &st, &stop) {
                crate::livetr::emit_state(&app2, &format!("系统声音字幕出错：{}", e.message));
            }
            RUNNING.store(false, Ordering::Relaxed);
            crate::livetr::close_subbar_if_idle(&app2);
            crate::livetr::emit_state(&app2, "");
        });
    }
    let _ = &app;
    Ok(crate::livetr::status())
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::livetr::{SubPayload, EVT_SUB, SUBBAR};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::Emitter;

    fn text_of(cues: &[crate::subtitle::Cue]) -> String {
        cues.iter().flat_map(|c| c.lines.iter()).map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
    }

    async fn recognize(st: &Arc<crate::AppState>, wav: &std::path::Path, lang: &str, cloud: bool, stop: &Arc<AtomicBool>) -> AppResult<String> {
        if !cloud {
            let files =
                crate::ml::DATA_DIR.get().and_then(|d| crate::ml::sensevoice::find_files(d)).ok_or_else(|| AppError::invalid("本机语音识别还没装好。"))?;
            let stop2 = stop.clone();
            let cues = crate::ml::sensevoice::recognize(&files, wav, lang, move || stop2.load(Ordering::Relaxed)).await?;
            return Ok(text_of(&cues));
        }
        let settings = st.settings_raw();
        let (ep, model) = match crate::ai::stt_pool_endpoint(st, &settings) {
            Some(ep) => (ep, settings.ai.stt_model.clone()),
            None => {
                if settings.ai.stt_model.is_empty() {
                    return Err(AppError::invalid("请先在“设置 → AI”里填写语音转文字的模型名称。"));
                }
                (crate::ai::stt_endpoint(st, &settings)?, settings.ai.stt_model.clone())
            }
        };
        let srt = crate::ai::transcribe_rotating(&ep, &model, lang, wav).await?;
        Ok(text_of(&crate::subtitle::parse_srt(&srt)))
    }

    pub fn run(app: &tauri::AppHandle, st: &Arc<crate::AppState>, stop: &Arc<AtomicBool>) -> AppResult<()> {
        let host = cpal::default_host();
        let dev = host.default_output_device().ok_or_else(|| AppError::msg("找不到正在播放声音的设备。"))?;
        let cfg = dev.default_output_config().map_err(|e| AppError::msg(format!("读取声音设备失败：{e}")))?;
        let (ch, rate) = (cfg.channels() as usize, cfg.sample_rate().0);
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let err = |e| log::warn!("系统声音采集出错：{e}");
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => dev.build_input_stream(&cfg.clone().into(), move |d: &[f32], _| drop(tx.send(mono16k(d, ch, rate))), err, None),
            cpal::SampleFormat::I16 => dev.build_input_stream(
                &cfg.clone().into(),
                move |d: &[i16], _| {
                    let f: Vec<f32> = d.iter().map(|x| f32::from(*x) / 32768.0).collect();
                    drop(tx.send(mono16k(&f, ch, rate)))
                },
                err,
                None,
            ),
            other => return Err(AppError::msg(format!("不支持的声音格式：{other:?}"))),
        }
        .map_err(|e| AppError::msg(format!("无法抓取系统声音：{e}")))?;
        stream.play().map_err(|e| AppError::msg(format!("无法开始抓取系统声音：{e}")))?;

        let mut seg = Segmenter::default();
        let mut shown_at: Option<Instant> = None;
        let mut n = 0u32;
        while !stop.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(chunk) => {
                    let mut ready = seg.push(&chunk);
                    // 积压太多时丢掉最旧的，保证字幕跟得上
                    while rx.try_recv().map(|c| ready.extend(seg.push(&c))).is_ok() {}
                    while ready.len() > 3 {
                        ready.remove(0);
                    }
                    for s in ready {
                        n += 1;
                        let live = st.settings_raw().live;
                        let ai_target = st.settings_raw().ai.target_lang.clone();
                        let lang = crate::livetx::target_lang(&live, &ai_target);
                        let wav = st.data_dir.join(format!("live_audio_{}.wav", n % 4));
                        if std::fs::write(&wav, wav_bytes(&s)).is_err() {
                            continue;
                        }
                        let cloud = live.audio_stt == "cloud";
                        let heard = tauri::async_runtime::block_on(recognize(st, &wav, &live.audio_lang, cloud, stop));
                        let _ = std::fs::remove_file(&wav);
                        let text = match heard {
                            Ok(t) => clean(&t),
                            Err(e) => {
                                crate::livetr::emit_state(app, &format!("识别失败：{}", e.message));
                                continue;
                            }
                        };
                        if text.is_empty() {
                            continue;
                        }
                        let tr = crate::livetx::translate(app, std::slice::from_ref(&text), &lang, "", "", stop)
                            .ok()
                            .and_then(|v| v.into_iter().next())
                            .unwrap_or_default();
                        if tr.trim().is_empty() {
                            continue;
                        }
                        let _ = app.emit_to(
                            SUBBAR,
                            EVT_SUB,
                            SubPayload {
                                seq: u64::from(n),
                                text: tr.clone(),
                                orig: text.clone(),
                                bilingual: live.audio_bilingual,
                                font: live.sub_font,
                                source: "audio".into(),
                            },
                        );
                        if live.history {
                            crate::livetx::history_push(
                                &st.data_dir,
                                &crate::livetx::Entry { at: crate::db::now(), mode: "audio".into(), exe: String::new(), src: text, text: tr },
                            );
                        }
                        shown_at = Some(Instant::now());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            // 一阵没有新句子就把字幕撤掉
            if shown_at.is_some_and(|t| t.elapsed() > Duration::from_secs(7)) {
                shown_at = None;
                let _ = app.emit_to(
                    SUBBAR,
                    EVT_SUB,
                    SubPayload { seq: u64::from(n) + 1, text: String::new(), orig: String::new(), bilingual: false, font: 24, source: "audio".into() },
                );
            }
        }
        drop(stream);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize).map(|i| (i as f32 * 0.3).sin() * amp).collect()
    }

    #[test]
    fn resample_halves_and_downmixes() {
        let st: Vec<f32> = (0..96_000).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect(); // 48k 双声道，1 秒
        let m = mono16k(&st, 2, 48_000);
        assert!((m.len() as i32 - 16_000).abs() <= 1);
        assert!(m.iter().all(|x| (*x - 0.5).abs() < 1e-5));
    }

    #[test]
    fn segments_a_sentence_between_silences() {
        let mut s = Segmenter::default();
        let mut audio = vec![0.0; RATE as usize];
        audio.extend(tone(1.5, 0.3));
        audio.extend(vec![0.0; RATE as usize]);
        let out = s.push(&audio);
        assert_eq!(out.len(), 1);
        let secs = out[0].len() as f32 / RATE as f32;
        assert!((1.4..2.4).contains(&secs), "len {secs}");
    }

    #[test]
    fn ignores_short_blips_and_cuts_long_speech() {
        let mut s = Segmenter::default();
        let mut a = tone(0.12, 0.3);
        a.extend(vec![0.0; RATE as usize]);
        assert!(s.push(&a).is_empty());
        let mut s = Segmenter::default();
        let long = tone(30.0, 0.3);
        let out = s.push(&long);
        assert!(out.len() >= 2, "{}", out.len());
        assert!(out.iter().all(|x| x.len() <= 12 * RATE as usize + FRAME));
    }

    #[test]
    fn wav_has_a_valid_header() {
        let w = wav_bytes(&[0.0, 1.0, -1.0]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
    }

    #[test]
    fn clean_drops_noise_words() {
        assert_eq!(clean(" ， "), "");
        assert_eq!(clean("嗯"), "");
        assert_eq!(clean(" Hello there "), "Hello there");
    }
}
