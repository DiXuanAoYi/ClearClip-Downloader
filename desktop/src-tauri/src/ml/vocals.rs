//! 人声分离：MDX-Net 一类的 ONNX 模型（输入是左右声道的 STFT，输出是人声的 STFT）。
//! 流程：解码成 44.1 kHz 立体声 → 分块（半块重叠、渐变拼接）→ STFT → 模型 → 逆 STFT → 人声；伴奏 = 原声 − 人声。

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use super::onnx::{declared_shape, Net};
use super::store;
use crate::error::{AppError, AppResult};
use crate::postprocess::run_ffmpeg_capture;
use crate::track::cmds::{begin, end, need_ffmpeg, EVT_TRACK};
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub const SAMPLE_RATE: usize = 44_100;
const HOP: usize = 1024;
/// 模型没有声明形状时用的默认值（Kim_Vocal_2 / UVR MDX-Net 常见配置）
const DEFAULT_DIM_F: usize = 3072;
const DEFAULT_DIM_T: usize = 256;
/// 一次最长处理这么多（毫秒）：解码后的声音要整段放在内存里
const MAX_SPAN_MS: u64 = 20 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    pub n_fft: usize,
    pub hop: usize,
    /// 模型用到的频率 bin 数
    pub dim_f: usize,
    /// 模型一次吃的帧数
    pub dim_t: usize,
}

impl Params {
    /// 一块多少个采样
    pub fn chunk(&self) -> usize {
        self.hop * (self.dim_t - 1)
    }

    pub fn check(&self) -> AppResult<()> {
        if self.dim_t < 2 || self.dim_f == 0 || self.hop == 0 || self.n_fft < 2 * self.hop.min(self.n_fft / 2).max(1) {
            return Err(AppError::invalid("人声分离的参数不对。"));
        }
        if !(256..=32768).contains(&self.n_fft) || self.n_fft / 2 + 1 < self.dim_f {
            return Err(AppError::invalid(format!("FFT 点数 {} 和模型的频率数 {} 对不上。MDX-Net 模型常见的是 6144 或 7680。", self.n_fft, self.dim_f)));
        }
        if self.chunk() <= self.n_fft / 2 {
            return Err(AppError::invalid("模型一次处理的时间太短。"));
        }
        Ok(())
    }
}

struct Dsp {
    p: Params,
    fwd: Arc<dyn Fft<f32>>,
    inv: Arc<dyn Fft<f32>>,
    win: Vec<f32>,
}

impl Dsp {
    fn new(p: Params) -> Dsp {
        let mut planner = FftPlanner::<f32>::new();
        let win = (0..p.n_fft).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / p.n_fft as f32).cos()).collect();
        Dsp { p, fwd: planner.plan_fft_forward(p.n_fft), inv: planner.plan_fft_inverse(p.n_fft), win }
    }

    /// 一块（`chunk()` 个采样）→ `[f][t]` 排列的复数谱，只留前 `dim_f` 个 bin。边缘按反射补齐（和 torch.stft 的 center 一样）。
    fn stft(&self, x: &[f32]) -> Vec<Complex<f32>> {
        let Params { n_fft: n, hop, dim_f, dim_t } = self.p;
        let c = x.len() as isize;
        let at = |i: usize| -> f32 {
            let mut s = i as isize - (n / 2) as isize;
            if s < 0 {
                s = -s;
            }
            if s >= c {
                s = 2 * (c - 1) - s;
            }
            x[s.clamp(0, c - 1) as usize]
        };
        let mut out = vec![Complex::new(0.0, 0.0); dim_f * dim_t];
        let mut buf = vec![Complex::new(0.0, 0.0); n];
        for t in 0..dim_t {
            for (k, b) in buf.iter_mut().enumerate() {
                *b = Complex::new(at(t * hop + k) * self.win[k], 0.0);
            }
            self.fwd.process(&mut buf);
            for f in 0..dim_f {
                out[f * dim_t + t] = buf[f];
            }
        }
        out
    }

    /// `stft` 的逆：`[f][t]` 复数谱 → `chunk()` 个采样（加窗叠加，再除以窗的平方和）。
    fn istft(&self, spec: &[Complex<f32>]) -> Vec<f32> {
        let Params { n_fft: n, hop, dim_f, dim_t } = self.p;
        let total = (dim_t - 1) * hop + n;
        let mut acc = vec![0f32; total];
        let mut wsum = vec![0f32; total];
        let mut buf = vec![Complex::new(0.0, 0.0); n];
        for t in 0..dim_t {
            buf.fill(Complex::new(0.0, 0.0));
            for f in 0..dim_f {
                buf[f] = spec[f * dim_t + t];
            }
            for k in 1..n.div_ceil(2) {
                buf[n - k] = buf[k].conj();
            }
            self.inv.process(&mut buf);
            for k in 0..n {
                let w = self.win[k];
                acc[t * hop + k] += buf[k].re / n as f32 * w;
                wsum[t * hop + k] += w * w;
            }
        }
        let c = self.p.chunk();
        (0..c).map(|i| acc[i + n / 2] / wsum[i + n / 2].max(1e-8)).collect()
    }
}

/// 把左右声道分成重叠的块，每块交给 `run`（输入 / 输出都是 `[1,4,dim_f,dim_t]` 展平），再渐变拼回去。返回人声的左右声道。
pub fn separate(
    p: Params,
    left: &[f32],
    right: &[f32],
    run: &mut dyn FnMut(Vec<f32>) -> AppResult<Vec<f32>>,
    cancel: &AtomicBool,
    progress: &dyn Fn(f64),
) -> AppResult<[Vec<f32>; 2]> {
    p.check()?;
    let n = left.len().min(right.len());
    if n == 0 {
        return Err(AppError::msg("这段没有声音可以分离。"));
    }
    let dsp = Dsp::new(p);
    let chunk = p.chunk();
    let step = chunk / 2;
    let fade: Vec<f32> = (0..chunk).map(|k| 0.5 - 0.5 * (2.0 * PI * (k as f32 + 0.5) / chunk as f32).cos()).collect();
    let mut out = [vec![0f32; n], vec![0f32; n]];
    let mut norm = vec![0f32; n];
    let plane = p.dim_f * p.dim_t;
    let mut s = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::msg("canceled"));
        }
        let mut data = vec![0f32; 4 * plane];
        for (c, src) in [left, right].into_iter().enumerate() {
            let mut x = vec![0f32; chunk];
            let end = (s + chunk).min(n);
            x[..end - s].copy_from_slice(&src[s..end]);
            for (i, v) in dsp.stft(&x).into_iter().enumerate() {
                data[2 * c * plane + i] = v.re;
                data[(2 * c + 1) * plane + i] = v.im;
            }
        }
        let y = run(data)?;
        if y.len() != 4 * plane {
            return Err(AppError::msg(format!("模型输出的大小不对（{} ≠ {}）。这不像是 MDX-Net 人声分离模型。", y.len(), 4 * plane)));
        }
        for (c, o) in out.iter_mut().enumerate() {
            let spec: Vec<Complex<f32>> = (0..plane).map(|i| Complex::new(y[2 * c * plane + i], y[(2 * c + 1) * plane + i])).collect();
            let wav = dsp.istft(&spec);
            for k in 0..chunk.min(n - s) {
                o[s + k] += wav[k] * fade[k];
            }
        }
        for k in 0..chunk.min(n - s) {
            norm[s + k] += fade[k];
        }
        progress(((s + chunk).min(n)) as f64 / n as f64);
        if s + chunk >= n {
            break;
        }
        s += step;
    }
    for o in out.iter_mut() {
        for (v, w) in o.iter_mut().zip(&norm) {
            *v /= w.max(1e-8);
        }
    }
    Ok(out)
}

/// 由模型声明的形状（`[1, 4, F, T]`，可能缺）和用户给的 FFT 点数得到参数。
pub fn params_for(declared: &[Option<usize>], default_n_fft: usize, n_fft: Option<usize>) -> AppResult<Params> {
    if declared.len() == 4 && declared[1].is_some_and(|c| c != 4) {
        return Err(AppError::invalid("这个模型的输入不是 4 通道（左右声道的实部 / 虚部），不是 MDX-Net 人声分离模型。"));
    }
    let dim_f = declared.get(2).copied().flatten().unwrap_or(DEFAULT_DIM_F);
    let dim_t = declared.get(3).copied().flatten().unwrap_or(DEFAULT_DIM_T);
    let p = Params { n_fft: n_fft.unwrap_or(default_n_fft), hop: HOP, dim_f, dim_t };
    p.check()?;
    Ok(p)
}

fn interleave(l: &[f32], r: &[f32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(l.len() * 8);
    for (a, b) in l.iter().zip(r) {
        v.extend_from_slice(&a.to_le_bytes());
        v.extend_from_slice(&b.to_le_bytes());
    }
    v
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocalsReq {
    pub id: u64,
    pub path: String,
    pub from_ms: u64,
    pub to_ms: u64,
    pub model: String,
    #[serde(default)]
    pub n_fft: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VocalsResult {
    pub vocals: String,
    pub instrumental: String,
    pub duration_ms: u64,
}

fn file_stem(path: &str) -> String {
    let s = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let s: String = s.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect();
    if s.is_empty() {
        "audio".into()
    } else {
        s
    }
}

/// 把 `[from_ms, to_ms]` 的声音分成人声和伴奏两个 WAV。进度走 `edit://track`，取消用 `track_cancel`。
#[tauri::command]
pub async fn edit_vocals(app: AppHandle, state: St<'_>, req: VocalsReq) -> AppResult<VocalsResult> {
    if !std::path::Path::new(&req.path).is_file() {
        return Err(AppError::invalid("找不到素材文件。"));
    }
    if req.to_ms <= req.from_ms {
        return Err(AppError::invalid("范围不对。"));
    }
    if req.to_ms - req.from_ms > MAX_SPAN_MS {
        return Err(AppError::invalid("一次最多分离 20 分钟的声音，请先把片段裁短。"));
    }
    let ff = need_ffmpeg(&state)?;
    let def = store::def(&req.model)?;
    if def.kind != "vocals" {
        return Err(AppError::invalid("这不是人声分离模型。"));
    }
    let model = store::require(&state, &req.model)?;
    let out_dir = state.data_dir.join("vocals");
    std::fs::create_dir_all(&out_dir)?;
    let id = req.id;
    let cancel = begin(id);
    let emit = {
        let app = app.clone();
        move |p: f64| {
            let _ = app.emit(EVT_TRACK, serde_json::json!({ "id": id, "percent": p.round() }));
        }
    };
    let res = run_vocals(&ff, &model, def.n_fft, &req, &out_dir, &cancel, emit.clone()).await;
    end(id);
    emit(100.0);
    res
}

async fn run_vocals(
    ff: &std::path::Path,
    model: &std::path::Path,
    default_n_fft: usize,
    req: &VocalsReq,
    out_dir: &std::path::Path,
    cancel: &Arc<AtomicBool>,
    emit: impl Fn(f64) + Clone + Send + Sync + 'static,
) -> AppResult<VocalsResult> {
    let declared = declared_shape(model)?;
    let p = params_for(&declared, default_n_fft, req.n_fft)?;
    let secs = (req.to_ms - req.from_ms) as f64 / 1000.0;
    let dec: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-ss",
        &format!("{:.3}", req.from_ms as f64 / 1000.0),
        "-t",
        &format!("{secs:.3}"),
        "-i",
        &req.path,
        "-vn",
        "-ac",
        "2",
        "-ar",
        &SAMPLE_RATE.to_string(),
        "-f",
        "f32le",
        "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let (raw, _) = run_ffmpeg_capture(ff, &dec, Duration::from_secs(900)).await?;
    emit(5.0);
    let (mut l, mut r) = (Vec::with_capacity(raw.len() / 8), Vec::with_capacity(raw.len() / 8));
    for fr in raw.chunks_exact(8) {
        l.push(f32::from_le_bytes([fr[0], fr[1], fr[2], fr[3]]));
        r.push(f32::from_le_bytes([fr[4], fr[5], fr[6], fr[7]]));
    }
    drop(raw);
    if l.is_empty() {
        return Err(AppError::msg("这段素材里没有声音。"));
    }

    let model = model.to_path_buf();
    let flag = Arc::clone(cancel);
    let (mix_l, mix_r) = (l.clone(), r.clone());
    let voc = tokio::task::spawn_blocking(move || -> AppResult<[Vec<f32>; 2]> {
        let shape = [1usize, 4, p.dim_f, p.dim_t];
        let net = Net::load(&model, &shape)?;
        let mut run = |data: Vec<f32>| net.run(&shape, data).map(|(_, v)| v);
        separate(p, &mix_l, &mix_r, &mut run, &flag, &|f| emit(5.0 + f * 90.0))
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;

    let stem = format!("{}_{}-{}", file_stem(&req.path), req.from_ms, req.to_ms);
    let vocals = out_dir.join(format!("{stem}_vocals.wav"));
    let inst = out_dir.join(format!("{stem}_instrumental.wav"));
    let (il, ir): (Vec<f32>, Vec<f32>) = (l.iter().zip(&voc[0]).map(|(m, v)| m - v).collect(), r.iter().zip(&voc[1]).map(|(m, v)| m - v).collect());
    for (dst, bytes) in [(&vocals, interleave(&voc[0], &voc[1])), (&inst, interleave(&il, &ir))] {
        let raw = out_dir.join(format!("{stem}.tmp.raw"));
        std::fs::write(&raw, bytes)?;
        let args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-f", "f32le", "-ar", &SAMPLE_RATE.to_string(), "-ac", "2", "-i"]
            .iter()
            .map(|s| s.to_string())
            .chain([raw.to_string_lossy().into_owned(), "-c:a".into(), "pcm_s16le".into(), dst.to_string_lossy().into_owned()])
            .collect();
        let res = run_ffmpeg_capture(ff, &args, Duration::from_secs(300)).await;
        let _ = std::fs::remove_file(&raw);
        res?;
    }
    Ok(VocalsResult { vocals: vocals.to_string_lossy().into_owned(), instrumental: inst.to_string_lossy().into_owned(), duration_ms: req.to_ms - req.from_ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> Params {
        Params { n_fft: 256, hop: 64, dim_f: 129, dim_t: 17 }
    }

    fn tone(n: usize, hz: f32) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * hz * i as f32 / SAMPLE_RATE as f32).sin() * 0.5).collect()
    }

    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn stft_then_istft_restores_the_chunk() {
        let p = small();
        let dsp = Dsp::new(p);
        let x = tone(p.chunk(), 440.0);
        let y = dsp.istft(&dsp.stft(&x));
        let err = x.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-3, "最大误差 {err}");
    }

    #[test]
    fn identity_model_returns_the_mix_across_chunk_seams() {
        let p = small();
        let (l, r) = (tone(5000, 300.0), tone(5000, 900.0));
        let mut calls = 0;
        let out = separate(
            p,
            &l,
            &r,
            &mut |d| {
                calls += 1;
                Ok(d)
            },
            &never(),
            &|_| {},
        )
        .unwrap();
        assert!(calls > 3, "应该分了好几块：{calls}");
        for (got, want) in [(&out[0], &l), (&out[1], &r)] {
            assert_eq!(got.len(), want.len());
            let err = got.iter().zip(want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(err < 2e-3, "最大误差 {err}");
        }
    }

    #[test]
    fn a_gain_model_scales_the_vocals() {
        let p = small();
        let l = tone(3000, 500.0);
        let out = separate(p, &l, &l, &mut |d| Ok(d.into_iter().map(|v| v * 0.5).collect()), &never(), &|_| {}).unwrap();
        let err = out[0].iter().zip(&l).map(|(a, b)| (a - b * 0.5).abs()).fold(0.0, f32::max);
        assert!(err < 2e-3, "{err}");
    }

    #[test]
    fn cancel_and_bad_outputs_are_reported() {
        let p = small();
        let l = tone(2000, 500.0);
        let stop = AtomicBool::new(true);
        let e = separate(p, &l, &l, &mut |d| Ok(d), &stop, &|_| {}).unwrap_err();
        assert!(e.to_string().contains("canceled"));
        let e = separate(p, &l, &l, &mut |_| Ok(vec![0.0; 3]), &never(), &|_| {}).unwrap_err();
        assert!(e.to_string().contains("MDX-Net"), "{e}");
        assert!(separate(p, &[], &[], &mut |d| Ok(d), &never(), &|_| {}).is_err());
    }

    #[test]
    fn params_follow_the_declared_shape() {
        let p = params_for(&[Some(1), Some(4), Some(2048), Some(128)], 6144, None).unwrap();
        assert_eq!((p.dim_f, p.dim_t, p.n_fft, p.chunk()), (2048, 128, 6144, 1024 * 127));
        let d = params_for(&[None, None, None, None], 7680, None).unwrap();
        assert_eq!((d.dim_f, d.dim_t), (3072, 256));
        assert!(params_for(&[Some(1), Some(2), Some(2048), Some(128)], 6144, None).is_err(), "不是 4 通道");
        assert!(params_for(&[None, Some(4), Some(3072), Some(256)], 7680, Some(4096)).is_err(), "FFT 点数太小装不下 dim_f");
        assert!(params_for(&[None, Some(4), Some(3072), Some(256)], 7680, Some(6144)).is_ok());
    }

    #[test]
    fn stems_are_safe_file_names() {
        assert_eq!(file_stem("/m/我的 歌:01.mp3"), "我的_歌_01");
        assert_eq!(file_stem("/m/"), "m");
    }

    fn ffmpeg() -> Option<std::path::PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| "ffmpeg".into())
    }

    async fn rms(ff: &std::path::Path, wav: &str) -> f64 {
        let args: Vec<String> =
            ["-hide_banner", "-loglevel", "error", "-i", wav, "-ac", "1", "-ar", "44100", "-f", "f32le", "-"].iter().map(|s| s.to_string()).collect();
        let (raw, _) = run_ffmpeg_capture(ff, &args, Duration::from_secs(60)).await.unwrap();
        let v: Vec<f64> = raw.chunks_exact(4).map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))).collect();
        (v.iter().map(|x| x * x).sum::<f64>() / v.len().max(1) as f64).sqrt()
    }

    #[tokio::test]
    async fn the_whole_pipeline_splits_a_real_file_into_two_wavs() {
        use prost::Message;
        let Some(ff) = ffmpeg() else { return };
        let d = std::env::temp_dir().join(format!("cc-vocals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("tone.wav");
        let gen: Vec<String> = [
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=2",
            "-ac",
            "2",
            src.to_str().unwrap(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if run_ffmpeg_capture(&ff, &gen, Duration::from_secs(30)).await.is_err() {
            return;
        }
        let model = d.join("id.onnx");
        std::fs::write(&model, crate::ml::onnx::testing::identity_model(1025, 8).encode_to_vec()).unwrap();
        let req = VocalsReq { id: 1, path: src.to_string_lossy().into_owned(), from_ms: 0, to_ms: 2000, model: "vocal_mdx".into(), n_fft: Some(2048) };
        let seen = Arc::new(std::sync::Mutex::new(Vec::<f64>::new()));
        let s2 = Arc::clone(&seen);
        let res = run_vocals(&ff, &model, 7680, &req, &d, &Arc::new(AtomicBool::new(false)), move |p| s2.lock().unwrap().push(p)).await.unwrap();
        let (o, v, i) = (rms(&ff, src.to_str().unwrap()).await, rms(&ff, &res.vocals).await, rms(&ff, &res.instrumental).await);
        assert!(o > 0.05 && (v - o).abs() < o * 0.05, "人声 = 原声：{v} vs {o}");
        assert!(i < o * 0.05, "伴奏 = 原声 − 人声 ≈ 静音：{i}");
        let p = seen.lock().unwrap();
        assert!(p.windows(2).all(|w| w[1] >= w[0]) && *p.last().unwrap() > 90.0, "进度单调上升：{p:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
