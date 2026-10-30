//! 音效：降噪、人声增强、混响、响度标准化，串成一条音频滤镜链，用在工具箱的“音效 / 混响”和剪辑的片段上。
//!
//! 混响是离线卷积：ffmpeg 的 `afir` 滤镜加一段按参数合成的脉冲响应，不需要附带任何脉冲响应文件。
//! 脉冲响应 = 左右声道各一路白噪声（所以有立体感），按 1.8 kHz 分成低频和高频两段，各自乘指数衰减包络：
//! 低频衰减到 -60 dB 的时间就是“混响时间”，高频是它乘“高频衰减比”。再加预延迟，然后用 `gtype=gn`（增益归一到噪声）
//! 让“混响混合 0 dB”大致等于干声的响度，所以混响量只由“混响混合”决定。
//! 精简版 ffmpeg 没有 `afir` 时退回 `aecho` 多抽头回声（效果简陋，会提示）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::media_tools::{audio_codec, muxer_args, out_path, s, Encoders, Input, Plan};
use crate::vidcaps::Caps;

/// 混响的参数（对应 PotPlayer 的“输入增益 / 混响混合 / 混响时间 / 高频衰减比”，另加预延迟）。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReverbSpec {
    /// 混响声相对干声的音量（dB）。0 = 和干声一样大
    pub mix_db: f64,
    /// 低频衰减到 -60 dB 所用的时间（毫秒）
    pub time_ms: u32,
    /// 高频的混响时间是低频的多少倍（0.1–1）；越小越“闷”
    pub hf_ratio: f64,
    /// 预延迟（毫秒）：直达声和第一个反射之间的间隔，越大房间越“大”
    pub pre_delay_ms: u32,
    /// 进入混响前的输入增益（dB）
    pub in_gain_db: f64,
}

impl Default for ReverbSpec {
    fn default() -> Self {
        ReverbSpec { mix_db: -9.0, time_ms: 800, hf_ratio: 0.7, pre_delay_ms: 15, in_gain_db: 0.0 }
    }
}

impl ReverbSpec {
    pub fn checked(&self) -> ReverbSpec {
        let f = |v: f64, lo: f64, hi: f64, d: f64| if v.is_finite() { v.clamp(lo, hi) } else { d };
        ReverbSpec {
            mix_db: f(self.mix_db, -40.0, 6.0, -9.0),
            time_ms: self.time_ms.clamp(100, 10_000),
            hf_ratio: f(self.hf_ratio, 0.1, 1.0, 0.7),
            pre_delay_ms: self.pre_delay_ms.min(300),
            in_gain_db: f(self.in_gain_db, -24.0, 12.0, 0.0),
        }
    }

    /// 混响尾巴有多长（毫秒）：音频文件结尾要多留这么长，免得混响被截断。
    pub fn tail_ms(&self) -> u64 {
        (self.time_ms as u64 + self.pre_delay_ms as u64).min(10_000)
    }
}

/// 一条音效链。顺序固定：降噪 → 人声增强 → 混响 → 响度标准化 → 限幅。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioFxSpec {
    /// off / light / medium / strong
    pub denoise: String,
    /// 人声增强（切低频噪声、提升人声清晰度、轻度压缩）
    pub enhance: bool,
    pub reverb: Option<ReverbSpec>,
    /// 处理完标准化到这个响度（LUFS）
    pub normalize: Option<f64>,
    /// 预设名，只用来记录
    pub preset: Option<String>,
}

impl Default for AudioFxSpec {
    fn default() -> Self {
        AudioFxSpec { denoise: "off".into(), enhance: false, reverb: None, normalize: None, preset: None }
    }
}

impl AudioFxSpec {
    pub fn checked(&self) -> AppResult<AudioFxSpec> {
        let denoise = match self.denoise.as_str() {
            "light" | "medium" | "strong" => self.denoise.clone(),
            "" | "off" => "off".into(),
            other => return Err(AppError::invalid(format!("不认识的降噪强度：{other}"))),
        };
        let spec = AudioFxSpec {
            denoise,
            enhance: self.enhance,
            reverb: self.reverb.as_ref().map(ReverbSpec::checked),
            normalize: self.normalize.filter(|v| v.is_finite()).map(|v| v.clamp(-30.0, -5.0)),
            preset: self.preset.clone(),
        };
        if !spec.is_active() {
            return Err(AppError::invalid("请至少打开一项效果：降噪、人声增强、混响或响度标准化。"));
        }
        Ok(spec)
    }

    pub fn is_active(&self) -> bool {
        self.denoise != "off" || self.enhance || self.reverb.is_some() || self.normalize.is_some()
    }

    /// 输出文件名里的标记。
    fn tag(&self) -> &'static str {
        match (self.reverb.is_some(), self.denoise != "off" || self.enhance, self.normalize.is_some()) {
            (true, false, false) => "混响",
            (false, true, false) => "人声",
            (false, false, true) => "响度",
            _ => "音效",
        }
    }
}

/// 音效链用到的 ffmpeg 能力。
#[derive(Debug, Clone, Default)]
pub struct FxCaps {
    pub afir: bool,
    pub afftdn: bool,
    pub arnndn: bool,
    /// 本机有 RNNoise 模型文件时的路径（`arnndn` 要用）
    pub rnn_model: Option<PathBuf>,
    /// `amix` 的 `normalize` 选项（ffmpeg 4.4 起）
    pub amix_normalize: bool,
    pub limiter: bool,
    pub loudnorm: bool,
    /// 实测的底噪电平（dBFS）；有它时降噪按这个电平设置，没有就让 afftdn 自己跟踪
    pub noise_floor_db: Option<f64>,
}

impl FxCaps {
    pub fn from_caps(c: &Caps, rnn_model: Option<PathBuf>) -> FxCaps {
        FxCaps {
            afir: c.has_filter("afir"),
            afftdn: c.has_filter("afftdn"),
            arnndn: c.has_filter("arnndn"),
            rnn_model: rnn_model.filter(|p| p.is_file()),
            amix_normalize: c.major == 0 || (c.major, c.minor) >= (4, 4),
            limiter: c.has_filter("alimiter"),
            loudnorm: c.has_filter("loudnorm"),
            noise_floor_db: None,
        }
    }

    /// 全部能力都有（测试和没探测时用）。
    pub fn all() -> FxCaps {
        FxCaps { afir: true, afftdn: true, arnndn: false, rnn_model: None, amix_normalize: true, limiter: true, loudnorm: true, noise_floor_db: None }
    }
}

#[derive(Debug, Clone)]
pub struct Built {
    /// 完整的 filter_complex：输入 `[0:a:0]`，输出 `[out]`
    pub graph: String,
    pub notes: Vec<String>,
    /// 输出比输入多出的尾巴（毫秒）；0 表示不加长
    pub tail_ms: u64,
}

fn db(v: f64) -> String {
    format!("{v:.2}dB")
}

/// 路径放进滤镜参数时的转义（反斜杠、冒号、单引号）。
fn esc_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").replace(':', "\\:").replace('\'', "\\'")
}

/// 降噪 + 人声增强：单一路径上的滤镜，剪辑里每个片段也用它。返回滤镜链和说明。
pub fn simple_chain(spec: &AudioFxSpec, caps: &FxCaps) -> (Vec<String>, Vec<String>) {
    let mut f = vec![];
    let mut notes = vec![];
    if spec.denoise != "off" {
        let (nr, mix) = match spec.denoise.as_str() {
            "light" => (10.0, 0.7),
            "strong" => (24.0, 1.0),
            _ => (16.0, 0.9),
        };
        match (&caps.rnn_model, caps.arnndn) {
            (Some(m), true) if spec.denoise != "light" => {
                f.push(format!("arnndn=m='{}':mix={mix}", esc_path(m)));
                notes.push("降噪：RNNoise 神经网络降噪".into());
            }
            _ if caps.afftdn => {
                f.push(match caps.noise_floor_db {
                    Some(nf) => format!("afftdn=nr={nr}:nf={:.0}:tn=0", nf.clamp(-80.0, -20.0)),
                    None => format!("afftdn=nr={nr}:nf=-45:tn=1"),
                });
                notes.push(if spec.denoise == "light" {
                    "降噪：频域降噪（轻）".into()
                } else {
                    "降噪：频域降噪（没有安装 RNNoise 模型，用的是 afftdn；效果更好的 RNNoise 见“设置 → 组件”）".into()
                });
            }
            _ => notes.push("当前的 ffmpeg 没有降噪滤镜（afftdn / arnndn），已跳过降噪。".into()),
        }
        if spec.denoise == "strong" {
            f.push("highpass=f=70".into());
        }
    }
    if spec.enhance {
        // 切掉低频隆隆声 → 削一点浑浊的低中频 → 提一点人声清晰度 → 轻度压缩让音量更均匀
        f.push("highpass=f=80".into());
        f.push("equalizer=f=250:t=q:w=1:g=-2".into());
        f.push("equalizer=f=3200:t=q:w=1.2:g=2.5".into());
        f.push("acompressor=threshold=0.1:ratio=3:attack=15:release=200:makeup=2".into());
    }
    (f, notes)
}

/// 合成立体声脉冲响应的滤镜图，输出标签 `[ir]`。
fn impulse_graph(r: &ReverbSpec, caps: &FxCaps) -> String {
    let rt = r.time_ms as f64 / 1000.0;
    let rt_hi = (rt * r.hf_ratio).max(0.05);
    // 高频先衰减完，所以脉冲响应只需要覆盖低频衰减到 -60 dB 再多一点
    let len = rt * 1.25 + 0.05;
    let mix2 = |a: &str, b: &str, out: &str| {
        if caps.amix_normalize {
            format!("[{a}][{b}]amix=inputs=2:normalize=0:duration=longest[{out}]")
        } else {
            format!("[{a}][{b}]amix=inputs=2:duration=longest,volume=2[{out}]")
        }
    };
    let mut g = vec![];
    for (i, seed) in [(0, 7), (1, 11)] {
        g.push(format!("anoisesrc=d={len:.3}:c=white:r=48000:a=0.5:s={seed}:n=256,asplit=2[n{i}a][n{i}b]"));
        g.push(format!("[n{i}a]lowpass=f=1800:p=2,volume='pow(10,-3*t/{rt:.4})':eval=frame[n{i}lo]"));
        g.push(format!("[n{i}b]highpass=f=1800:p=2,volume='pow(10,-3*t/{rt_hi:.4})':eval=frame[n{i}hi]"));
        g.push(mix2(&format!("n{i}lo"), &format!("n{i}hi"), &format!("n{i}")));
    }
    let mut tail = String::from("[n0][n1]amerge=inputs=2");
    if r.pre_delay_ms > 0 {
        tail.push_str(&format!(",adelay=delays={0}|{0}", r.pre_delay_ms));
    }
    tail.push_str(",afade=t=in:d=0.004[ir]");
    g.push(tail);
    g.join(";")
}

/// `afir` 不可用时的备选：多抽头回声。
fn echo_chain(r: &ReverbSpec) -> String {
    let rt = r.time_ms as f64;
    let amp = 10f64.powf(r.mix_db / 20.0);
    let mut delays = vec![];
    let mut decays = vec![];
    for k in [0.06, 0.12, 0.19, 0.27, 0.37, 0.5, 0.66] {
        let d = (r.pre_delay_ms as f64 + rt * k * 0.6).max(20.0).round();
        delays.push(format!("{d:.0}"));
        decays.push(format!("{:.3}", (amp * 10f64.powf(-3.0 * d / rt.max(1.0))).clamp(0.01, 0.9)));
    }
    format!("aecho=0.85:0.9:{}:{}", delays.join("|"), decays.join("|"))
}

/// 构建完整的滤镜图。`pad_tail`：音频文件在结尾留出混响尾巴；视频不加长（视频流是直接复制的）。
pub fn build_graph(spec: &AudioFxSpec, caps: &FxCaps, pad_tail: bool) -> AppResult<Built> {
    let spec = spec.checked()?;
    let (pre, mut notes) = simple_chain(&spec, caps);
    let mut parts: Vec<String> = vec![];
    let mut head = vec!["aresample=48000".to_string()];
    if spec.reverb.is_some() {
        head.push("aformat=sample_fmts=fltp:channel_layouts=stereo".into());
    }
    head.extend(pre);
    let mut tail_ms = 0;
    let mut cur = "s0".to_string();
    parts.push(format!("[0:a:0]{}[{cur}]", head.join(",")));

    if let Some(r) = &spec.reverb {
        let mut src = cur.clone();
        if pad_tail {
            tail_ms = r.tail_ms();
            parts.push(format!("[{src}]apad=pad_dur={:.3}[s1]", tail_ms as f64 / 1000.0));
            src = "s1".into();
        }
        if caps.afir {
            parts.push(format!("[{src}]volume={},asplit=2[d][w]", db(r.in_gain_db)));
            parts.push(impulse_graph(r, caps));
            parts.push("[w][ir]afir=dry=1:wet=1:gtype=gn:irfmt=input[w2]".into());
            parts.push(format!("[w2]volume={}[w3]", db(r.mix_db)));
            parts.push(if caps.amix_normalize {
                "[d][w3]amix=inputs=2:normalize=0:duration=longest[r]".into()
            } else {
                "[d][w3]amix=inputs=2:duration=longest,volume=2[r]".into()
            });
            notes.push(format!("混响：{} 毫秒，高频 ×{:.2}，混合 {:.1} dB，预延迟 {} 毫秒", r.time_ms, r.hf_ratio, r.mix_db, r.pre_delay_ms));
        } else {
            parts.push(format!("[{src}]volume={},{}[r]", db(r.in_gain_db), echo_chain(r)));
            notes
                .push("当前的 ffmpeg 没有卷积滤镜（afir），混响退回成多抽头回声，效果比较简陋。在“设置 → 组件”里安装完整版 ffmpeg 可以得到真正的混响。".into());
        }
        cur = "r".into();
    }

    let mut post = vec![];
    if let Some(t) = spec.normalize {
        if caps.loudnorm {
            post.push(format!("loudnorm=I={t}:TP=-1.5:LRA=11"));
            // loudnorm 内部按 192 kHz 输出，必须明确指定采样率
            post.push("aresample=48000".into());
            notes.push(format!("响度标准化到 {t} LUFS"));
        } else {
            notes.push("当前的 ffmpeg 没有 loudnorm，已跳过响度标准化。".into());
        }
    }
    if (spec.reverb.is_some() || spec.enhance) && caps.limiter {
        post.push("alimiter=limit=0.95".into());
    }
    if post.is_empty() {
        post.push("anull".into());
    }
    parts.push(format!("[{cur}]{}[out]", post.join(",")));
    Ok(Built { graph: parts.join(";"), notes, tail_ms })
}

/// 估计底噪电平：把前 `secs` 秒切成 100 ms 一块，取最安静的 10% 的 RMS（dBFS）。
/// 估不出来（几乎全是静音、ffmpeg 报错）就返回 None。
pub async fn estimate_noise_floor(ffmpeg: &Path, input: &Path, start_s: f64, secs: u32) -> Option<f64> {
    let mut args = crate::postprocess::base_args();
    args.extend(["-ss".into(), format!("{start_s:.2}"), "-t".into(), secs.to_string(), "-i".into(), input.to_string_lossy().into_owned()]);
    args.extend([
        "-vn".into(),
        "-af".into(),
        "aformat=channel_layouts=mono,asetnsamples=n=4800:p=0,astats=metadata=1:reset=1:measure_perchannel=none:measure_overall=RMS_level,ametadata=print:key=lavfi.astats.Overall.RMS_level:file=-".into(),
        "-f".into(),
        "null".into(),
        "-".into(),
    ]);
    let (out, _) = crate::postprocess::run_ffmpeg_capture(ffmpeg, &args, std::time::Duration::from_secs(60)).await.ok()?;
    noise_floor_from(&String::from_utf8_lossy(&out))
}

fn noise_floor_from(text: &str) -> Option<f64> {
    let mut v: Vec<f64> = text
        .lines()
        .filter_map(|l| l.split_once("RMS_level=").map(|x| x.1.trim()))
        .filter_map(|x| x.parse::<f64>().ok())
        .filter(|x| x.is_finite() && *x > -90.0)
        .collect();
    if v.len() < 5 {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 10])
}

/// RNNoise 模型（`arnndn` 用）在数据目录里的位置。
pub fn rnn_model_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("rnnoise.rnnn")
}

/// 工具箱“音效 / 混响”的计划：文件里必须有音频；视频的画面直接复制，只重新编码声音。
pub fn plan(
    first: &Input,
    spec: &AudioFxSpec,
    caps: &Caps,
    enc: &Encoders,
    dir: Option<&Path>,
    rnn_model: Option<PathBuf>,
    noise_floor_db: Option<f64>,
) -> AppResult<Plan> {
    if !first.probe.has_audio {
        return Err(AppError::invalid("文件里没有音频。"));
    }
    let spec = spec.checked()?;
    let mut fx = FxCaps::from_caps(caps, rnn_model);
    fx.noise_floor_db = noise_floor_db;
    let in_ext = first.path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let inp = first.path.to_string_lossy().into_owned();
    let video = first.probe.has_video && matches!(in_ext.as_str(), "mp4" | "mkv" | "mov" | "m4v" | "webm" | "ts" | "flv" | "avi");
    let built = build_graph(&spec, &fx, !video)?;
    let mut p = Plan {
        args: vec![],
        output: PathBuf::new(),
        is_dir: false,
        kind: "video",
        out_ms: first.probe.duration_ms.map(|d| d + built.tail_ms),
        note: None,
        fallback: None,
        temp_files: vec![],
    };
    p.note = (!built.notes.is_empty()).then(|| built.notes.join("；"));
    if video {
        let ext = if matches!(in_ext.as_str(), "mp4" | "mkv" | "mov") { in_ext.as_str() } else { "mp4" };
        p.output = out_path(&first.path, dir, spec.tag(), ext);
        p.args = s(&["-i", &inp, "-filter_complex", &built.graph, "-map", "0:v:0", "-map", "[out]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]);
        p.args.extend(muxer_args(ext, &p.output));
    } else {
        let fmt = match in_ext.as_str() {
            "mp3" => "mp3",
            "flac" => "flac",
            "wav" => "wav",
            "opus" | "ogg" => "opus",
            _ => "m4a",
        };
        p.kind = "audio";
        p.output = out_path(&first.path, dir, spec.tag(), fmt);
        p.args = s(&["-i", &inp, "-filter_complex", &built.graph, "-map", "[out]", "-vn"]);
        p.args.extend(audio_codec(enc, fmt)?);
        p.args.push(p.output.to_string_lossy().into_owned());
    }
    Ok(p)
}

/// 试听用的参数：取 `start_ms` 起的 `secs` 秒，输出 16 位 WAV。`raw = true` 时只截取、不处理（作为“处理前”）。
pub fn preview_args(input: &Path, start_ms: u64, secs: u32, spec: Option<(&AudioFxSpec, &FxCaps)>, out: &Path) -> AppResult<(Vec<String>, Vec<String>)> {
    let mut a = s(&["-ss", &format!("{:.3}", start_ms as f64 / 1000.0), "-t", &secs.to_string(), "-i", &input.to_string_lossy()]);
    let mut notes = vec![];
    match spec {
        Some((sp, caps)) => {
            let built = build_graph(sp, caps, true)?;
            notes = built.notes;
            a.extend(s(&["-filter_complex", &built.graph, "-map", "[out]"]));
        }
        None => a.extend(s(&["-map", "0:a:0", "-af", "aresample=48000,aformat=channel_layouts=stereo"])),
    }
    a.extend(s(&["-vn", "-c:a", "pcm_s16le"]));
    a.push(out.to_string_lossy().into_owned());
    Ok((a, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reverb(t: u32) -> AudioFxSpec {
        AudioFxSpec { reverb: Some(ReverbSpec { time_ms: t, ..Default::default() }), ..Default::default() }
    }

    #[test]
    fn rejects_empty_and_unknown() {
        assert!(AudioFxSpec::default().checked().is_err());
        assert!(AudioFxSpec { denoise: "wild".into(), ..Default::default() }.checked().is_err());
        assert!(AudioFxSpec { normalize: Some(-16.0), ..Default::default() }.checked().is_ok());
    }

    #[test]
    fn parameters_are_clamped() {
        let r = ReverbSpec { mix_db: 99.0, time_ms: 5, hf_ratio: 9.0, pre_delay_ms: 9999, in_gain_db: f64::NAN }.checked();
        assert_eq!((r.mix_db, r.time_ms, r.hf_ratio, r.pre_delay_ms, r.in_gain_db), (6.0, 100, 1.0, 300, 0.0));
        let s = AudioFxSpec { normalize: Some(0.0), ..Default::default() }.checked().unwrap();
        assert_eq!(s.normalize, Some(-5.0));
    }

    #[test]
    fn reverb_graph_has_convolution_and_tail() {
        let b = build_graph(&reverb(1200), &FxCaps::all(), true).unwrap();
        assert!(b.graph.contains("afir=dry=1:wet=1:gtype=gn"), "{}", b.graph);
        assert!(b.graph.contains("pow(10,-3*t/1.2000)"), "{}", b.graph);
        // 高频衰减比 0.7：高频段 0.84 秒
        assert!(b.graph.contains("pow(10,-3*t/0.8400)"), "{}", b.graph);
        assert!(b.graph.contains("apad=pad_dur=1.215"), "{}", b.graph);
        assert!(b.graph.contains("adelay=delays=15|15"), "{}", b.graph);
        assert!(b.graph.ends_with("[out]"));
        assert_eq!(b.tail_ms, 1215);
        // 视频不加长
        let v = build_graph(&reverb(1200), &FxCaps::all(), false).unwrap();
        assert!(!v.graph.contains("apad") && v.tail_ms == 0);
    }

    #[test]
    fn falls_back_to_echo_without_afir() {
        let caps = FxCaps { afir: false, ..FxCaps::all() };
        let b = build_graph(&reverb(800), &caps, true).unwrap();
        assert!(b.graph.contains("aecho=") && !b.graph.contains("afir"), "{}", b.graph);
        assert!(b.notes.iter().any(|n| n.contains("afir")));
    }

    #[test]
    fn old_amix_gets_compensation() {
        let caps = FxCaps { amix_normalize: false, ..FxCaps::all() };
        let b = build_graph(&reverb(800), &caps, true).unwrap();
        assert!(!b.graph.contains("normalize=0") && b.graph.contains("volume=2"), "{}", b.graph);
    }

    #[test]
    fn chain_order_is_denoise_enhance_reverb_loudness() {
        let spec = AudioFxSpec { denoise: "medium".into(), enhance: true, reverb: Some(ReverbSpec::default()), normalize: Some(-16.0), preset: None };
        let g = build_graph(&spec, &FxCaps::all(), true).unwrap().graph;
        let pos = |k: &str| g.find(k).unwrap_or_else(|| panic!("缺少 {k}：{g}"));
        assert!(
            pos("afftdn") < pos("highpass=f=80") && pos("highpass=f=80") < pos("afir") && pos("afir") < pos("loudnorm") && pos("loudnorm") < pos("alimiter"),
            "{g}"
        );
    }

    #[test]
    fn denoise_uses_rnnoise_only_when_the_model_exists() {
        let dir = std::env::temp_dir().join(format!("clearclip-fx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("m.rnnn");
        std::fs::write(&model, b"x").unwrap();
        let spec = AudioFxSpec { denoise: "strong".into(), ..Default::default() };
        let with = FxCaps { arnndn: true, rnn_model: Some(model.clone()), ..FxCaps::all() };
        assert!(simple_chain(&spec, &with).0.iter().any(|f| f.starts_with("arnndn=m=")));
        assert!(simple_chain(&spec, &FxCaps::all()).0.iter().any(|f| f.starts_with("afftdn")));
        // 轻度降噪不用神经网络（避免伤人声）
        let light = AudioFxSpec { denoise: "light".into(), ..Default::default() };
        assert!(simple_chain(&light, &with).0.iter().any(|f| f.starts_with("afftdn")));
        let none = FxCaps { afftdn: false, ..FxCaps::all() };
        assert!(simple_chain(&spec, &none).1.iter().any(|n| n.contains("没有降噪滤镜")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serde_shape_matches_the_frontend() {
        let j: AudioFxSpec =
            serde_json::from_str(r#"{"denoise":"light","reverb":{"mixDb":-6,"timeMs":2000,"hfRatio":0.5,"preDelayMs":20,"inGainDb":1},"normalize":-14}"#)
                .unwrap();
        assert_eq!(j.reverb.as_ref().unwrap().time_ms, 2000);
        assert_eq!(j.normalize, Some(-14.0));
        assert!(!j.enhance);
    }

    #[test]
    fn file_tag_follows_the_content() {
        assert_eq!(reverb(500).tag(), "混响");
        assert_eq!(AudioFxSpec { enhance: true, ..Default::default() }.tag(), "人声");
        assert_eq!(AudioFxSpec { normalize: Some(-16.0), ..Default::default() }.tag(), "响度");
        assert_eq!(AudioFxSpec { enhance: true, normalize: Some(-16.0), ..Default::default() }.tag(), "音效");
    }

    // ---------- 用真实的 ffmpeg 跑一遍（没有 ffmpeg 时跳过） ----------

    fn ffmpeg() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("clearclip-fx-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn run(ff: &Path, args: &[String]) {
        let mut full = crate::postprocess::base_args();
        full.extend(args.iter().cloned());
        let o = std::process::Command::new(ff).args(&full).output().unwrap();
        assert!(o.status.success(), "ffmpeg 失败：{}\n{:?}", String::from_utf8_lossy(&o.stderr), full);
    }

    /// 整个文件（或 -ss/-t 截取的一段）的 RMS 电平（dB）
    fn rms_db(ff: &Path, f: &Path, from: f64, secs: f64) -> f64 {
        let o = std::process::Command::new(ff)
            .args(["-hide_banner", "-nostats", "-ss", &from.to_string(), "-t", &secs.to_string(), "-i"])
            .arg(f)
            .args(["-af", "astats=measure_overall=RMS_level:measure_perchannel=none", "-f", "null", "-"])
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        let line = err.lines().rev().find(|l| l.contains("RMS level dB")).unwrap_or_else(|| panic!("{err}"));
        let v = line.rsplit(':').next().unwrap().trim();
        if v == "-inf" {
            -200.0
        } else {
            v.parse().unwrap()
        }
    }

    async fn input(ff: &Path, f: &Path) -> Input {
        Input { path: f.to_path_buf(), probe: crate::postprocess::probe(ff, f).await }
    }

    async fn apply(ff: &Path, f: &Path, spec: &AudioFxSpec, dir: &Path) -> (PathBuf, Option<String>) {
        let caps = crate::vidcaps::detect(ff).await;
        let enc = crate::media_tools::detect_encoders(ff).await;
        let nf = if spec.denoise != "off" { estimate_noise_floor(ff, f, 0.0, 30).await } else { None };
        let p = plan(&input(ff, f).await, spec, &caps, &enc, Some(dir), None, nf).unwrap();
        run(ff, &p.args);
        (p.output, p.note)
    }

    #[tokio::test]
    async fn reverb_keeps_the_level_and_rings_out_after_the_source_ends() {
        let Some(ff) = ffmpeg() else { return };
        let d = tmp("reverb");
        // 0.6 秒的粉红噪声，后面是静音，总共 3 秒
        let src = d.join("burst.wav");
        run(
            &ff,
            &s(&[
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=d=0.6:c=pink:r=48000:a=0.3:s=1",
                "-af",
                "apad=whole_dur=3",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                &src.to_string_lossy(),
            ]),
        );
        let spec =
            AudioFxSpec { reverb: Some(ReverbSpec { mix_db: 0.0, time_ms: 1500, hf_ratio: 0.8, pre_delay_ms: 10, in_gain_db: 0.0 }), ..Default::default() };
        let (out, note) = apply(&ff, &src, &spec, &d).await;
        assert!(note.unwrap().contains("混响"));
        assert!(out.file_name().unwrap().to_string_lossy().contains(".混响."), "{out:?}");
        // 原文件 0.7 秒之后是完全静音的；有混响尾巴才会有声音
        assert!(rms_db(&ff, &src, 0.8, 1.0) < -150.0);
        let tail = rms_db(&ff, &out, 0.8, 1.0);
        assert!(tail > -45.0, "混响尾巴太弱：{tail} dB");
        // 尾巴随时间衰减
        let late = rms_db(&ff, &out, 2.4, 0.5);
        assert!(late < tail - 6.0, "没有衰减：{tail} → {late}");
        // 文件比原来长（尾巴被保留）
        let dur = crate::postprocess::probe(&ff, &out).await.duration_ms.unwrap();
        assert!(dur > 3000 + 1200, "时长 {dur}");
        // 混合 0 dB：干声 + 同样响的混响（不相关）≈ 比干声大 3 dB 左右
        let dry = rms_db(&ff, &src, 0.1, 0.4);
        let wet = rms_db(&ff, &out, 0.1, 0.4);
        assert!((wet - dry) > 1.0 && (wet - dry) < 6.0, "干声 {dry} dB，加混响后 {wet} dB");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn reverb_mix_controls_the_wet_level() {
        let Some(ff) = ffmpeg() else { return };
        let d = tmp("mix");
        let src = d.join("n.wav");
        run(
            &ff,
            &s(&[
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=d=0.5:c=pink:r=48000:a=0.3:s=3",
                "-af",
                "apad=whole_dur=2.5",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                &src.to_string_lossy(),
            ]),
        );
        let tail_of = |mix: f64| {
            let (ff, src, d) = (ff.clone(), src.clone(), d.clone());
            async move {
                let spec = AudioFxSpec { reverb: Some(ReverbSpec { mix_db: mix, time_ms: 1200, ..Default::default() }), ..Default::default() };
                let (out, _) = apply(&ff, &src, &spec, &d).await;
                let v = rms_db(&ff, &out, 0.7, 0.8);
                let _ = std::fs::remove_file(&out);
                v
            }
        };
        let (hi, lo) = (tail_of(-3.0).await, tail_of(-15.0).await);
        assert!((hi - lo - 12.0).abs() < 2.5, "混合差 12 dB，尾巴差 {} dB", hi - lo);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn video_keeps_its_picture_and_length() {
        let Some(ff) = ffmpeg() else { return };
        let d = tmp("video");
        let src = d.join("v.mp4");
        run(
            &ff,
            &s(&[
                "-f",
                "lavfi",
                "-i",
                "testsrc=s=160x90:r=10:d=2",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=2",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-shortest",
                &src.to_string_lossy(),
            ]),
        );
        let spec = AudioFxSpec { denoise: "medium".into(), enhance: true, reverb: Some(ReverbSpec::default()), normalize: Some(-16.0), ..Default::default() };
        let (out, _) = apply(&ff, &src, &spec, &d).await;
        let pr = crate::postprocess::probe(&ff, &out).await;
        assert!(pr.has_video && pr.has_audio);
        let dur = pr.duration_ms.unwrap();
        assert!((1800..2400).contains(&dur), "视频不应被拉长：{dur}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn denoise_lowers_steady_noise_and_normalize_hits_target() {
        let Some(ff) = ffmpeg() else { return };
        let d = tmp("dn");
        let src = d.join("hiss.wav");
        // 稳定的底噪 + 间歇的 440 Hz 音
        run(
            &ff,
            &s(&[
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=d=6:c=white:r=48000:a=0.05:s=5",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=6:r=48000",
                "-filter_complex",
                "[1:a]volume='if(lt(mod(t,2),1),0.3,0)':eval=frame[t];[0:a][t]amix=inputs=2:normalize=0[o]",
                "-map",
                "[o]",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                &src.to_string_lossy(),
            ]),
        );
        let before = rms_db(&ff, &src, 1.2, 0.6);
        let spec = AudioFxSpec { denoise: "strong".into(), ..Default::default() };
        let (out, _) = apply(&ff, &src, &spec, &d).await;
        let after = rms_db(&ff, &out, 1.2, 0.6);
        assert!(after < before - 6.0, "底噪应明显下降：{before} → {after} dB");
        let spec = AudioFxSpec { normalize: Some(-16.0), ..Default::default() };
        let (out, _) = apply(&ff, &src, &spec, &d).await;
        let o = std::process::Command::new(&ff)
            .args(["-hide_banner", "-nostats", "-i"])
            .arg(&out)
            .args(["-af", "ebur128=peak=true", "-f", "null", "-"])
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&o.stderr).into_owned();
        let sum = err.rsplit("Integrated loudness:").next().unwrap();
        let lufs: f64 = sum.lines().find(|l| l.contains(" I:")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!((lufs + 16.0).abs() < 2.0, "{lufs} LUFS");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn echo_fallback_also_produces_a_tail() {
        let Some(ff) = ffmpeg() else { return };
        let d = tmp("echo");
        let src = d.join("b.wav");
        run(
            &ff,
            &s(&[
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=d=0.4:c=pink:r=48000:a=0.3:s=1",
                "-af",
                "apad=whole_dur=2",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                &src.to_string_lossy(),
            ]),
        );
        let spec = AudioFxSpec { reverb: Some(ReverbSpec { time_ms: 1000, mix_db: -3.0, ..Default::default() }), ..Default::default() };
        let built = build_graph(&spec, &FxCaps { afir: false, ..FxCaps::all() }, true).unwrap();
        let out = d.join("o.wav");
        let mut a = s(&["-i", &src.to_string_lossy(), "-filter_complex", &built.graph, "-map", "[out]", "-c:a", "pcm_s16le"]);
        a.push(out.to_string_lossy().into_owned());
        run(&ff, &a);
        assert!(rms_db(&ff, &out, 0.45, 0.3) > -60.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn noise_floor_takes_the_quiet_tenth() {
        let text: String = (0..20).map(|i| format!("frame:{i} pts:{i}\nlavfi.astats.Overall.RMS_level={}\n", -60.0 + i as f64)).collect();
        assert_eq!(noise_floor_from(&text), Some(-58.0));
        assert_eq!(noise_floor_from("lavfi.astats.Overall.RMS_level=-inf\n"), None);
    }

    #[test]
    fn measured_noise_floor_sets_afftdn() {
        let caps = FxCaps { noise_floor_db: Some(-34.2), ..FxCaps::all() };
        let spec = AudioFxSpec { denoise: "medium".into(), ..Default::default() };
        assert!(simple_chain(&spec, &caps).0.iter().any(|f| f == "afftdn=nr=16:nf=-34:tn=0"));
    }

    #[tokio::test]
    async fn preview_args_produce_playable_wavs() {
        let Some(ff) = ffmpeg() else { return };
        let d = tmp("pv");
        let src = d.join("a.wav");
        run(&ff, &s(&["-f", "lavfi", "-i", "sine=f=300:d=4:r=44100", "-c:a", "pcm_s16le", &src.to_string_lossy()]));
        let spec = reverb(900).checked().unwrap();
        let (a, _) = preview_args(&src, 1000, 2, Some((&spec, &FxCaps::all())), &d.join("after.wav")).unwrap();
        run(&ff, &a);
        let (a, _) = preview_args(&src, 1000, 2, None, &d.join("before.wav")).unwrap();
        run(&ff, &a);
        let pb = crate::postprocess::probe(&ff, &d.join("before.wav")).await;
        let pa = crate::postprocess::probe(&ff, &d.join("after.wav")).await;
        assert!((1900..2100).contains(&pb.duration_ms.unwrap()), "{:?}", pb.duration_ms);
        assert!(pa.duration_ms.unwrap() >= 2000);
        let _ = std::fs::remove_dir_all(&d);
    }
}
