//! 清影写作的实时听写：浏览器窗口把麦克风声音（16 kHz 单声道 16 位）一小段一小段送过来，
//! 这里按说话停顿切句，边说边识别——说到一半给出“临时结果”（会被改写），停顿后给出这一句的“最终结果”。
//! 识别引擎跟着“设置 → AI → 语音转文字”走：本机开源模型（SenseVoice / whisper.cpp）或在线接口，也可以在听写时临时指定。
//! 没有流式接口的引擎（都没有）用“识别这一句到目前为止的声音”来模拟边说边出字，慢的引擎自动少做临时结果。

use std::collections::VecDeque;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::liveaudio::{clean, wav_bytes, Segmenter};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub const EVT: &str = "write://dictate";
/// 一次最多收这么长的声音（秒），防止忘了停。
const MAX_SECS: usize = 600;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub session: u64,
    /// interim 临时结果 / final 一句话的最终结果 / error 出错 / done 结束
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub session: u64,
    /// 用的是哪个引擎，给界面显示
    pub engine: String,
    /// 本机还是在线
    pub local: bool,
}

enum Msg {
    Pcm(Vec<f32>),
    Stop,
}

struct Active {
    id: u64,
    tx: Sender<Msg>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    SenseVoice,
    Whisper,
    Api,
}

impl Engine {
    fn label(self) -> &'static str {
        match self {
            Engine::SenseVoice => "本机 SenseVoice",
            Engine::Whisper => "本机 whisper.cpp",
            Engine::Api => "在线接口",
        }
    }
}

// ---------- 纯逻辑 ----------

/// 16 位小端 PCM 的 base64 → 样本（-1..1）。
pub fn pcm_from_b64(b64: &str) -> AppResult<Vec<f32>> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| AppError::invalid("声音数据不对。"))?;
    if bytes.len() % 2 != 0 {
        return Err(AppError::invalid("声音数据长度不对。"));
    }
    Ok(bytes.chunks_exact(2).map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0).collect())
}

/// 要不要现在做一次“临时识别”。
/// - 这一句已经比上次临时识别时多出 `min_grow` 个样本以上，才值得再识别；
/// - 距离上次至少隔 `interval`，而且隔得比上次识别花的时间长（识别慢的引擎自动少做，不让声音积压）。
pub fn want_interim(since_last: Duration, last_cost: Duration, grown: usize, min_grow: usize) -> bool {
    let wait = Duration::from_millis(1200).max(last_cost.mul_f32(1.5));
    grown >= min_grow && since_last >= wait
}

/// 一次听写里已经定稿的句子接成一段：中日韩直接连，英文加空格。
pub fn join_finals(parts: &[String]) -> String {
    parts.iter().filter(|p| !p.trim().is_empty()).fold(String::new(), |acc, p| crate::livecore::join_text(&acc, p))
}

// ---------- 识别 ----------

fn settings_for(st: &AppState, mode: &str) -> crate::settings::Settings {
    let mut s = st.settings();
    match mode {
        "local" => s.ai.stt_engine = "local".into(),
        "api" => s.ai.stt_engine = "api".into(),
        _ => {}
    }
    s
}

fn pick(st: &AppState, mode: &str) -> AppResult<(Engine, crate::settings::Settings)> {
    let settings = settings_for(st, mode);
    crate::ai::stt_ready(st, &settings)?;
    let engine = if crate::ai::use_local_stt(st, &settings) {
        match crate::ai::local_stt(&settings) {
            crate::ai::LocalStt::SenseVoice => Engine::SenseVoice,
            crate::ai::LocalStt::Whisper => Engine::Whisper,
        }
    } else {
        Engine::Api
    };
    Ok((engine, settings))
}

async fn recognize(st: &Arc<AppState>, settings: &crate::settings::Settings, engine: Engine, samples: &[f32], lang: &str, tag: &str) -> AppResult<String> {
    let dir = st.data_dir.join("dictation");
    std::fs::create_dir_all(&dir)?;
    let wav = dir.join(format!("{tag}.wav"));
    std::fs::write(&wav, wav_bytes(samples))?;
    let out = async {
        match engine {
            Engine::SenseVoice => {
                let files =
                    crate::ml::DATA_DIR.get().and_then(|d| crate::ml::sensevoice::find_files(d)).ok_or_else(|| AppError::invalid("本机语音识别还没装好。"))?;
                let cues = crate::ml::sensevoice::recognize(&files, &wav, lang, || false).await?;
                Ok(cues.iter().flat_map(|c| c.lines.iter()).map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
            }
            Engine::Whisper => crate::ai::whisper_segment(settings, &wav, lang, &dir).await,
            Engine::Api => {
                let (ep, model) = match crate::ai::stt_pool_endpoint(st, settings) {
                    Some(ep) => (ep, settings.ai.stt_model.clone()),
                    None => (crate::ai::stt_endpoint(st, settings)?, settings.ai.stt_model.clone()),
                };
                if model.is_empty() {
                    return Err(AppError::invalid("请先在“设置 → AI”里填写语音转文字的模型名称。"));
                }
                let srt = crate::ai::transcribe_rotating(&ep, &model, lang, &wav).await?;
                Ok(crate::subtitle::parse_srt(&srt)
                    .iter()
                    .flat_map(|c| c.lines.iter())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "))
            }
        }
    }
    .await;
    let _ = std::fs::remove_file(&wav);
    out
}

fn emit(app: &AppHandle, session: u64, kind: &str, text: &str) {
    let _ = app.emit(EVT, Payload { session, kind: kind.into(), text: text.into() });
}

fn worker(app: AppHandle, st: Arc<AppState>, id: u64, engine: Engine, settings: crate::settings::Settings, lang: String, rx: mpsc::Receiver<Msg>) {
    let mut seg = Segmenter::default();
    let (mut n, mut total) = (0u32, 0usize);
    let (mut last_at, mut last_cost, mut last_len) = (Instant::now(), Duration::ZERO, 0usize);
    let mut backlog: VecDeque<Vec<f32>> = VecDeque::new();
    let mut stopping = false;
    let run = |samples: &[f32], tag: String| tauri::async_runtime::block_on(recognize(&st, &settings, engine, samples, &lang, &tag));
    'outer: loop {
        match rx.recv_timeout(Duration::from_millis(120)) {
            Ok(Msg::Pcm(c)) => {
                total += c.len();
                if total > MAX_SECS * 16_000 {
                    emit(&app, id, "error", "听写时间太长，已自动结束。");
                    stopping = true;
                } else {
                    backlog.push_back(c);
                }
            }
            Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => stopping = true,
            Err(RecvTimeoutError::Timeout) => {}
        }
        // 先把积压的声音都送进切句器（识别慢的时候声音会攒着，不能丢）
        while let Some(c) = backlog.pop_front() {
            for utter in seg.push(&c) {
                n += 1;
                let t0 = Instant::now();
                match run(&utter, format!("rt{id}-{n}")) {
                    Ok(t) => {
                        let t = clean(&t);
                        emit(&app, id, "final", &t);
                    }
                    Err(e) => emit(&app, id, "error", &e.message),
                }
                last_cost = t0.elapsed();
                last_at = Instant::now();
                last_len = 0;
            }
        }
        if stopping {
            if let Some(utter) = seg.flush() {
                n += 1;
                match run(&utter, format!("rt{id}-{n}")) {
                    Ok(t) => emit(&app, id, "final", &clean(&t)),
                    Err(e) => emit(&app, id, "error", &e.message),
                }
            }
            break 'outer;
        }
        // 这一句还在说：隔一会儿就识别一次“到目前为止”的声音，给出临时结果
        if let Some(cur) = seg.current() {
            if want_interim(last_at.elapsed(), last_cost, cur.len().saturating_sub(last_len), 16_000 * 6 / 10) {
                let cur = cur.to_vec();
                n += 1;
                let t0 = Instant::now();
                match run(&cur, format!("rt{id}-i{n}")) {
                    Ok(t) => emit(&app, id, "interim", &clean(&t)),
                    Err(e) => emit(&app, id, "error", &e.message),
                }
                last_cost = t0.elapsed();
                last_at = Instant::now();
                last_len = cur.len();
            }
        } else {
            last_len = 0;
        }
    }
    emit(&app, id, "done", "");
    let mut a = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
    if a.as_ref().is_some_and(|x| x.id == id) {
        *a = None;
    }
}

// ---------- 命令 ----------

/// 开始实时听写。`engine`：`auto` 跟着设置 / `local` 本机开源模型 / `api` 在线接口。
#[tauri::command]
pub fn write_dictate_start(app: AppHandle, state: St<'_>, engine: Option<String>, lang: Option<String>) -> AppResult<Info> {
    let st = state.inner().clone();
    let mode = engine.unwrap_or_else(|| "auto".into());
    let (eng, settings) = pick(&st, &mode)?;
    let lang = lang.filter(|l| !l.trim().is_empty()).unwrap_or_else(|| settings.ai.stt_language.clone());
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    {
        let mut a = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        // 上一次没结束的先停掉
        if let Some(old) = a.take() {
            let _ = old.tx.send(Msg::Stop);
        }
        *a = Some(Active { id, tx });
    }
    let app2 = app.clone();
    std::thread::spawn(move || worker(app2, st, id, eng, settings, lang, rx));
    Ok(Info { session: id, engine: eng.label().into(), local: eng != Engine::Api })
}

/// 送来一小段声音（16 位小端 PCM 的 base64，16 kHz 单声道）。会话已经结束时返回 false。
#[tauri::command]
pub fn write_dictate_push(session: u64, pcm: String) -> AppResult<bool> {
    let samples = pcm_from_b64(&pcm)?;
    let a = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
    Ok(a.as_ref().filter(|x| x.id == session).is_some_and(|x| x.tx.send(Msg::Pcm(samples)).is_ok()))
}

/// 说完了：把没收尾的那一句也识别出来，然后发 done。
#[tauri::command]
pub fn write_dictate_stop(session: u64) {
    let a = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(x) = a.as_ref().filter(|x| x.id == session) {
        let _ = x.tx.send(Msg::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(samples: &[i16]) -> String {
        base64::engine::general_purpose::STANDARD.encode(samples.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>())
    }

    #[test]
    fn pcm_round_trips_and_rejects_garbage() {
        let v = pcm_from_b64(&b64(&[0, 16384, -16384, -32768, 32767])).unwrap();
        assert_eq!(v.len(), 5);
        assert!((v[1] - 0.5).abs() < 1e-4 && (v[2] + 0.5).abs() < 1e-4 && (v[3] + 1.0).abs() < 1e-4);
        assert!(pcm_from_b64("@@@@").is_err());
        assert!(pcm_from_b64(&base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3])).is_err(), "奇数个字节不是 16 位样本");
        assert!(pcm_from_b64("").unwrap().is_empty());
    }

    #[test]
    fn interim_waits_for_new_speech_and_backs_off_for_slow_engines() {
        let ms = Duration::from_millis;
        let grow = 9_600; // 0.6 秒
        assert!(want_interim(ms(1300), ms(200), grow, grow), "够久、够长、引擎快：做");
        assert!(!want_interim(ms(800), ms(200), grow, grow), "刚做过：不做");
        assert!(!want_interim(ms(1300), ms(200), grow - 1, grow), "这一句没多出多少新声音：不做");
        // 引擎慢（上次花了 3 秒）：要隔 4.5 秒才做，免得声音积压
        assert!(!want_interim(ms(3000), ms(3000), grow, grow));
        assert!(want_interim(ms(4600), ms(3000), grow, grow));
    }

    #[test]
    fn finals_join_by_language() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_finals(&p(&["你好", "", "世界"])), "你好世界");
        assert_eq!(join_finals(&p(&["Hello", "world"])), "Hello world");
        assert_eq!(join_finals(&[]), "");
    }

    #[test]
    fn segmenter_exposes_the_sentence_in_progress() {
        // 20 毫秒一帧：先 1 秒安静，再 1 秒有声（幅度大于阈值）
        let quiet = vec![0.0f32; 16_000];
        let loud: Vec<f32> = (0..16_000).map(|i| if i % 2 == 0 { 0.3 } else { -0.3 }).collect();
        let mut seg = Segmenter::default();
        assert!(seg.push(&quiet).is_empty());
        assert!(seg.current().is_none(), "还没开始说话");
        assert!(seg.push(&loud).is_empty(), "还没说完，不该有完整的句子");
        let cur = seg.current().expect("在说话").len();
        assert!(cur >= 16_000, "到目前为止的声音至少有这 1 秒：{cur}");
        // 说完（安静够久）→ 整句出来，current 清空
        let done = seg.push(&vec![0.0f32; 16_000]);
        assert_eq!(done.len(), 1);
        assert!(seg.current().is_none());
        // 结束时没收尾的那一句用 flush 取出来
        let mut seg = Segmenter::default();
        seg.push(&loud);
        assert!(seg.flush().is_some_and(|u| u.len() >= 16_000));
        assert!(seg.flush().is_none(), "取过就没有了");
        // 太短的杂音不算
        let mut seg = Segmenter::default();
        seg.push(&loud[..800]);
        assert!(seg.flush().is_none());
    }
}
