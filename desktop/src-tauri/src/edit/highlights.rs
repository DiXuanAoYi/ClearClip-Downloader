//! 精彩片段提取（长视频 → 短视频的候选）：给出若干 15–90 秒的片段，每个有标题、分数和理由。
//!
//! 不靠视频理解模型，用**可解释的信号**打分：声音响度和起伏、语速、镜头切换的节奏、开头有没有“钩子”（问题、数字、
//! 转折词，前面有停顿）、结尾是不是一句话的结束。起止点吸附到句子（有文稿时）和静音谷 / 镜头边界（没文稿时）。
//! 可选地把文稿文字交给用户配置的大模型挑选（`llm_pick`），默认不用，界面上每次都要确认。
//! 这只是建议：一定会有选得不对的时候。

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ai;
use crate::error::{AppError, AppResult};
use crate::postprocess::{base_args, run_ffmpeg_capture};

/// 响度包络每秒几个值
pub const ENV_HZ: usize = 4;
const ENV_RATE: usize = 4000;
/// 一次最多分析这么长（毫秒）：3 小时
pub const MAX_MS: u64 = 3 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 0–100
    pub score: f64,
    pub title: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Opts {
    pub count: usize,
    pub min_ms: u64,
    pub max_ms: u64,
}

impl Opts {
    pub fn new(count: u32, min_secs: u32, max_secs: u32) -> Opts {
        let min = min_secs.clamp(5, 300);
        let max = max_secs.clamp(min + 5, 600);
        Opts { count: count.clamp(1, 12) as usize, min_ms: u64::from(min) * 1000, max_ms: u64::from(max) * 1000 }
    }
}

// ---------------------------------------------------------------- 响度包络

/// 每 1/`ENV_HZ` 秒一个值的响度（dBFS）。
pub fn envelope(pcm: &[i16]) -> Vec<f64> {
    let step = ENV_RATE / ENV_HZ;
    pcm.chunks(step)
        .filter(|c| c.len() >= step / 2)
        .map(|c| {
            let ms = c.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / c.len() as f64;
            20.0 * (ms.sqrt() / 32768.0 + 1e-6).log10()
        })
        .collect()
}

pub async fn decode_envelope(ffmpeg: &Path, path: &str, from_ms: u64, to_ms: u64) -> AppResult<Vec<f64>> {
    if !Path::new(path).is_file() {
        return Err(AppError::not_found("找不到素材文件。"));
    }
    if to_ms <= from_ms {
        return Err(AppError::invalid("范围不对：结束要比开始晚。"));
    }
    let to = to_ms.min(from_ms + MAX_MS);
    let mut a = base_args();
    a.extend([
        "-ss".into(),
        format!("{:.3}", from_ms as f64 / 1000.0),
        "-t".into(),
        format!("{:.3}", (to - from_ms) as f64 / 1000.0),
        "-i".into(),
        path.to_string(),
    ]);
    a.extend(["-vn".into(), "-ac".into(), "1".into(), "-ar".into(), ENV_RATE.to_string(), "-f".into(), "s16le".into(), "-".into()]);
    let (raw, _) = run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(900)).await.map_err(|e| AppError::msg(format!("读取声音失败：{e}")))?;
    let pcm: Vec<i16> = raw.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    Ok(envelope(&pcm))
}

// ---------------------------------------------------------------- 打分

fn env_idx(env_from_ms: u64, t: u64) -> usize {
    (t.saturating_sub(env_from_ms) as usize * ENV_HZ) / 1000
}

fn stats(env: &[f64], from: usize, to: usize) -> (f64, f64) {
    let to = to.min(env.len());
    if to <= from {
        return (-90.0, 0.0);
    }
    let s = &env[from..to];
    let mean = s.iter().sum::<f64>() / s.len() as f64;
    let sd = (s.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / s.len() as f64).sqrt();
    (mean, sd)
}

fn percentile_ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    if n <= 1 {
        return vec![0.5; n];
    }
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut out = vec![0.0; n];
    for (rank, &i) in idx.iter().enumerate() {
        out[i] = rank as f64 / (n - 1) as f64;
    }
    out
}

const HOOK_WORDS: &[&str] = &[
    "为什么",
    "怎么",
    "怎样",
    "千万",
    "注意",
    "秘密",
    "记住",
    "没想到",
    "原来",
    "其实",
    "但是",
    "然而",
    "第一",
    "最重要",
    "关键",
    "why",
    "how",
    "never",
    "secret",
    "actually",
    "but",
    "first",
    "most important",
];

fn is_hook(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    let head: String = t.chars().take(30).collect();
    head.contains('？') || head.contains('?') || head.chars().any(|c| c.is_ascii_digit()) || HOOK_WORDS.iter().any(|w| head.contains(w))
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end().chars().last().is_some_and(|c| "。！？.!?…」』\"”".contains(c))
}

struct Window {
    a: u64,
    b: u64,
    loud: f64,
    dyn_: f64,
    rate: f64,
    cuts: f64,
    quiet: f64,
    hook: bool,
    complete: bool,
    first: Option<usize>,
}

fn clip_title(text: &str, n: usize) -> String {
    let t: String = text.trim().chars().filter(|c| !c.is_control()).collect();
    if t.chars().count() <= n {
        t
    } else {
        format!("{}…", t.chars().take(n).collect::<String>())
    }
}

fn mmss(ms: u64) -> String {
    format!("{}:{:02}", ms / 60000, (ms / 1000) % 60)
}

/// 候选窗口的起点 / 终点。有文稿：句子的开头 / 结尾；没有：开头结尾、镜头切点、响度的低谷。
fn boundaries(env: &[f64], env_from: u64, from_ms: u64, to_ms: u64, scenes: &[u64], sentences: &[Sentence]) -> (Vec<u64>, Vec<u64>) {
    if !sentences.is_empty() {
        let s: Vec<&Sentence> = sentences.iter().filter(|s| s.start_ms >= from_ms && s.end_ms <= to_ms && s.end_ms > s.start_ms).collect();
        return (s.iter().map(|s| s.start_ms).collect(), s.iter().map(|s| s.end_ms).collect());
    }
    let mut v = vec![from_ms, to_ms];
    v.extend(scenes.iter().copied().filter(|&t| t > from_ms && t < to_ms));
    // 响度低谷：比前后 1.5 秒内都低，并且低于整体中位数
    let mut sorted: Vec<f64> = env.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(-60.0);
    let w = ENV_HZ * 3 / 2;
    for i in w..env.len().saturating_sub(w) {
        if env[i] < median - 3.0 && env[i - w..=i + w].iter().all(|&x| x >= env[i]) {
            let t = env_from + (i * 1000 / ENV_HZ) as u64;
            if t > from_ms && t < to_ms {
                v.push(t);
            }
        }
    }
    // 声音一直很平的素材找不到低谷：每 5 秒补一个点，落在附近最安静的地方
    let mut t = from_ms + 5000;
    while t + 2000 < to_ms {
        let (ia, ib) = (env_idx(env_from, t.saturating_sub(2000)), env_idx(env_from, t + 2000).min(env.len()));
        if ia < ib {
            let best = (ia..ib).min_by(|&x, &y| env[x].total_cmp(&env[y])).unwrap_or(ia);
            v.push(env_from + (best * 1000 / ENV_HZ) as u64);
        }
        t += 5000;
    }
    v.sort_unstable();
    v.dedup_by(|a, b| a.abs_diff(*b) < 1500);
    (v.clone(), v)
}

/// 给一段素材范围里的候选打分、挑出最好的几个（互相不重叠）。`env` 从 `from_ms` 开始，每秒 `ENV_HZ` 个值（dBFS）；
/// `scenes` 是镜头切点（素材时间）；`sentences` 可以为空（只用声音和镜头）。
pub fn rank(env: &[f64], from_ms: u64, to_ms: u64, scenes: &[u64], sentences: &[Sentence], o: Opts) -> Vec<Candidate> {
    let (starts, ends) = boundaries(env, from_ms, from_ms, to_ms, scenes, sentences);
    let mut wins: Vec<Window> = vec![];
    for (si, &a) in starts.iter().enumerate() {
        let ok: Vec<u64> = ends.iter().copied().filter(|&b| b > a + o.min_ms && b <= a + o.max_ms).collect();
        let step = (ok.len() / 4).max(1);
        for (taken, &b) in ok.iter().enumerate() {
            // 每个起点只取几个长度：最短的、最长的和中间的，免得窗口太多
            let keep = ok.len() <= 4 || taken % step == 0;
            if !keep {
                continue;
            }
            let (ia, ib) = (env_idx(from_ms, a), env_idx(from_ms, b));
            let (mean, sd) = stats(env, ia, ib);
            let secs = (b - a) as f64 / 1000.0;
            let quiet =
                if ib > ia { env[ia..ib.min(env.len())].iter().filter(|&&x| x < mean - 14.0 || x < -55.0).count() as f64 / (ib - ia) as f64 } else { 1.0 };
            let in_win: Vec<usize> = sentences.iter().enumerate().filter(|(_, s)| s.start_ms >= a && s.end_ms <= b).map(|(i, _)| i).collect();
            let chars: usize = in_win.iter().map(|&i| sentences[i].text.chars().filter(|c| !c.is_whitespace()).count()).sum();
            let cuts = scenes.iter().filter(|&&t| t > a && t < b).count() as f64 / secs * 10.0;
            let first = in_win.first().copied();
            let prev_gap =
                first.and_then(|f| f.checked_sub(1)).map_or(f64::MAX, |p| (sentences[first.unwrap()].start_ms.saturating_sub(sentences[p].end_ms)) as f64);
            let hook = first.is_some_and(|f| is_hook(&sentences[f].text) || prev_gap >= 600.0) && !sentences.is_empty();
            let complete = in_win.last().is_some_and(|&l| ends_sentence(&sentences[l].text));
            wins.push(Window { a, b, loud: mean, dyn_: sd, rate: if secs > 0.0 { chars as f64 / secs } else { 0.0 }, cuts, quiet, hook, complete, first });
        }
        let _ = si;
    }
    if wins.is_empty() {
        return vec![];
    }
    let loud = percentile_ranks(&wins.iter().map(|w| w.loud).collect::<Vec<_>>());
    let dynr = percentile_ranks(&wins.iter().map(|w| w.dyn_).collect::<Vec<_>>());
    let rate = percentile_ranks(&wins.iter().map(|w| w.rate).collect::<Vec<_>>());
    let has_text = !sentences.is_empty();
    let mut scored: Vec<(f64, usize, Vec<String>)> = wins
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let cuts = (1.0 - (w.cuts - 2.0).abs() / 4.0).clamp(0.0, 1.0);
            let mut reasons = vec![];
            if loud[i] >= 0.75 {
                reasons.push("声音响度高于大部分片段".to_string());
            }
            if dynr[i] >= 0.75 {
                reasons.push("声音起伏大，有情绪变化".to_string());
            }
            if has_text && rate[i] >= 0.75 {
                reasons.push("语速偏快，信息密度高".to_string());
            }
            if w.hook {
                reasons.push("开头是问题、数字或转折，或前面有停顿".to_string());
            }
            if has_text && w.complete {
                reasons.push("结尾是一句话的完整结束".to_string());
            }
            if cuts >= 0.7 && w.cuts > 0.0 {
                reasons.push("镜头切换节奏适中".to_string());
            }
            let base = if has_text {
                0.28 * loud[i] + 0.2 * dynr[i] + 0.2 * rate[i] + 0.1 * cuts + 0.12 * f64::from(u8::from(w.hook)) + 0.1 * f64::from(u8::from(w.complete))
            } else {
                0.6 * loud[i] + 0.2 * dynr[i] + 0.2 * cuts
            };
            // 大段安静（停顿、背景音）扣分
            let score = (base * (1.0 - 0.8 * w.quiet.clamp(0.0, 1.0))).clamp(0.0, 1.0);
            (score, i, reasons)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut picked: Vec<Candidate> = vec![];
    for (score, i, mut reasons) in scored {
        let w = &wins[i];
        let overlaps = picked.iter().any(|p| {
            let inter = w.b.min(p.end_ms).saturating_sub(w.a.max(p.start_ms));
            inter as f64 > (0.1 * (w.b - w.a).min(p.end_ms - p.start_ms) as f64).min(2000.0)
        });
        if overlaps {
            continue;
        }
        if reasons.is_empty() {
            reasons.push("综合声音、语速和镜头节奏得分较高".to_string());
        }
        let title = w.first.map_or_else(|| format!("候选片段 {}", mmss(w.a)), |f| clip_title(&sentences[f].text, 24));
        picked.push(Candidate { start_ms: w.a, end_ms: w.b, score: (score * 1000.0).round() / 10.0, title, reasons });
        if picked.len() >= o.count {
            break;
        }
    }
    picked
}

// ---------------------------------------------------------------- 大模型挑选（可选）

pub fn llm_system(count: usize, min_secs: u64, max_secs: u64) -> String {
    format!(
        "You pick the best short-video clips from a transcript. The user gives numbered sentences like \"[12] (03:41) text\". \
         Choose up to {count} non-overlapping clips, each a run of consecutive sentences lasting {min_secs}-{max_secs} seconds. \
         Prefer clips that are self-contained, informative or emotional, start with a hook, and end on a finished thought. \
         Reply with ONE JSON array only, no commentary and no code fences. Each element: \
         {{\"from\": <first sentence number>, \"to\": <last sentence number>, \"title\": <short title in the transcript's language>, \
         \"reason\": <one sentence why>, \"score\": <0-100>}}."
    )
}

pub fn llm_user(sentences: &[Sentence]) -> String {
    sentences.iter().enumerate().map(|(i, s)| format!("[{i}] ({}) {}", mmss(s.start_ms), s.text.trim())).collect::<Vec<_>>().join("\n")
}

/// 解析大模型的回答：容忍代码围栏和多余文字，丢掉越界 / 太短 / 太长的，去掉互相重叠的。
pub fn parse_picks(reply: &str, sentences: &[Sentence], o: Opts) -> Vec<Candidate> {
    let (Some(l), Some(r)) = (reply.find('['), reply.rfind(']')) else { return vec![] };
    if r <= l {
        return vec![];
    }
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&reply[l..=r]) else { return vec![] };
    let mut out: Vec<Candidate> = vec![];
    for it in items {
        let (Some(f), Some(t)) = (it.get("from").and_then(Value::as_u64), it.get("to").and_then(Value::as_u64)) else { continue };
        let (f, t) = (f as usize, t as usize);
        if f > t || t >= sentences.len() {
            continue;
        }
        let (a, b) = (sentences[f].start_ms, sentences[t].end_ms);
        // 允许比要求的略短 / 略长一点（句子边界），但差太多就不要
        if b <= a || b - a < o.min_ms / 2 || b - a > o.max_ms * 3 / 2 {
            continue;
        }
        if out.iter().any(|p| a < p.end_ms && b > p.start_ms) {
            continue;
        }
        let title = it.get("title").and_then(Value::as_str).map_or_else(|| clip_title(&sentences[f].text, 24), |s| clip_title(s, 30));
        let reason = it.get("reason").and_then(Value::as_str).map_or_else(|| "大模型认为这段完整、有信息量".to_string(), |s| s.trim().to_string());
        let score = it.get("score").and_then(Value::as_f64).unwrap_or(70.0).clamp(0.0, 100.0);
        out.push(Candidate { start_ms: a, end_ms: b, score, title, reasons: vec![reason] });
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(o.count);
    out
}

pub async fn llm_pick(ep: &ai::Endpoint, model: &str, sentences: &[Sentence], o: Opts) -> AppResult<Vec<Candidate>> {
    let reply = ai::chat(ep, model, &llm_system(o.count, o.min_ms / 1000, o.max_ms / 1000), &llm_user(sentences)).await?;
    let v = parse_picks(&reply, sentences, o);
    if v.is_empty() {
        return Err(AppError::parser("大模型的回答里没有可用的片段。可以换个模型或调整长度范围再试。"));
    }
    Ok(v)
}

// ---------------------------------------------------------------- 命令

type St<'a> = tauri::State<'a, std::sync::Arc<crate::AppState>>;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightReq {
    pub path: String,
    pub from_ms: u64,
    pub to_ms: u64,
    /// 文稿的句子（素材时间）；可以为空，只用声音和镜头
    #[serde(default)]
    pub sentences: Vec<Sentence>,
    pub count: u32,
    pub min_secs: u32,
    pub max_secs: u32,
}

/// 用声音、语速、镜头节奏给长视频找候选精彩片段（全部在本机算，不联网）。
#[tauri::command]
pub async fn edit_highlights(state: St<'_>, req: HighlightReq) -> AppResult<Vec<Candidate>> {
    let ff = crate::track::cmds::need_ffmpeg(&state)?;
    let o = Opts::new(req.count, req.min_secs, req.max_secs);
    if req.to_ms.saturating_sub(req.from_ms) < o.min_ms {
        return Err(AppError::invalid("这段素材比要求的最短长度还短。"));
    }
    let to = req.to_ms.min(req.from_ms + MAX_MS);
    let env = decode_envelope(&ff, &req.path, req.from_ms, to).await?;
    // 镜头切点是辅助信号，检测失败（比如纯音频）就不用它
    let scenes = super::analyze::scenes(&ff, &req.path, req.from_ms, to, 0.3).await.unwrap_or_default();
    Ok(rank(&env, req.from_ms, to, &scenes, &req.sentences, o))
}

/// 把文稿文字发给“设置 → AI”里配置的对话模型，让它挑出精彩片段。只发文字，不发视频；界面上每次都会先确认。
#[tauri::command]
pub async fn edit_highlights_llm(state: St<'_>, req: HighlightReq) -> AppResult<Vec<Candidate>> {
    if req.sentences.len() < 3 {
        return Err(AppError::invalid("文稿太短，先在“文稿”页生成文稿。"));
    }
    if req.sentences.len() > 4000 {
        return Err(AppError::invalid("文稿太长（超过 4000 句），请先裁短或分段再试。"));
    }
    let settings = state.settings();
    let ep = ai::chat_endpoint(&state, &settings)?;
    llm_pick(&ep, &settings.ai.model, &req.sentences, Opts::new(req.count, req.min_secs, req.max_secs)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 秒一句，共 120 秒（24 句）；40–70 秒是响亮、起伏大的一段
    fn fixture() -> (Vec<f64>, Vec<Sentence>) {
        let mut env = vec![];
        for i in 0..120 * ENV_HZ {
            let t = i as f64 / ENV_HZ as f64;
            let v = if (40.0..70.0).contains(&t) { -14.0 + 6.0 * (t * 3.0).sin() } else { -38.0 + 1.0 * (t * 2.0).sin() };
            env.push(v);
        }
        let sentences = (0..24)
            .map(|i| Sentence {
                start_ms: i * 5000 + 200,
                end_ms: i * 5000 + 4800,
                text: if i == 8 {
                    "为什么这样做才对？".into()
                } else if i % 3 == 0 {
                    "这是一句普通的话。".into()
                } else {
                    "继续往下说明".into()
                },
            })
            .collect();
        (env, sentences)
    }

    #[test]
    fn the_loud_stretch_wins_and_snaps_to_sentences() {
        let (env, sentences) = fixture();
        let o = Opts::new(3, 15, 45);
        let v = rank(&env, 0, 120_000, &[], &sentences, o);
        assert!(!v.is_empty() && v.len() <= 3);
        let top = &v[0];
        let inter = top.end_ms.min(70_000).saturating_sub(top.start_ms.max(40_000));
        assert!(inter as f64 >= 0.7 * (top.end_ms - top.start_ms) as f64, "第一名应该主要落在 40–70 秒：{top:?}");
        assert!(sentences.iter().any(|s| s.start_ms == top.start_ms) && sentences.iter().any(|s| s.end_ms == top.end_ms), "起止点吸附到句子：{top:?}");
        assert!(top.end_ms - top.start_ms >= 15_000 && top.end_ms - top.start_ms <= 45_000);
        assert!(!top.reasons.is_empty() && top.score > 0.0 && top.score <= 100.0);
        assert!(v.windows(2).all(|w| w[0].score >= w[1].score), "按分数从高到低");
        for (i, a) in v.iter().enumerate() {
            for b in &v[i + 1..] {
                assert!(
                    a.end_ms <= b.start_ms || b.end_ms <= a.start_ms || a.end_ms.min(b.end_ms) - a.start_ms.max(b.start_ms) < 3000,
                    "候选不重叠：{a:?} {b:?}"
                );
            }
        }
    }

    #[test]
    fn works_without_a_transcript_too() {
        let (env, _) = fixture();
        let v = rank(&env, 0, 120_000, &[], &[], Opts::new(2, 15, 45));
        assert!(!v.is_empty());
        let top = &v[0];
        let inter = top.end_ms.min(70_000).saturating_sub(top.start_ms.max(40_000));
        assert!(inter as f64 >= 0.6 * (top.end_ms - top.start_ms) as f64, "{top:?}");
        assert!(top.title.starts_with("候选片段"));
        // 镜头切点也能当边界
        let with_cuts = rank(&env, 0, 120_000, &[40_000, 70_000], &[], Opts::new(1, 15, 45));
        let c = &with_cuts[0];
        let inter = c.end_ms.min(70_000).saturating_sub(c.start_ms.max(40_000));
        assert!(inter >= 26_000 && c.end_ms - c.start_ms <= 36_000, "{c:?}");
    }

    #[test]
    fn nothing_to_pick_returns_empty() {
        assert!(rank(&[], 0, 10_000, &[], &[], Opts::new(3, 15, 45)).is_empty());
        let (env, s) = fixture();
        assert!(rank(&env[..40], 0, 10_000, &[], &s, Opts::new(3, 15, 45)).is_empty(), "范围比最短长度还短");
    }

    #[test]
    fn envelope_is_in_db_with_the_right_rate() {
        let loud = vec![16000i16; ENV_RATE * 2];
        let quiet = vec![10i16; ENV_RATE * 2];
        let e = envelope(&[loud, quiet].concat());
        assert_eq!(e.len(), ENV_HZ * 4);
        assert!(e[0] > -8.0 && e[0] < -5.0, "{}", e[0]);
        assert!(e[ENV_HZ * 3] < -60.0);
    }

    #[test]
    fn llm_replies_are_parsed_defensively() {
        let (_, s) = fixture();
        let o = Opts::new(3, 15, 45);
        let reply = "好的，结果如下：\n```json\n[{\"from\":8,\"to\":12,\"title\":\"为什么要这样\",\"reason\":\"有钩子\",\"score\":88},\
            {\"from\":9,\"to\":13,\"title\":\"重叠的\",\"score\":99},{\"from\":30,\"to\":40,\"title\":\"越界\"},\
            {\"from\":2,\"to\":2,\"title\":\"太短\"},{\"from\":14,\"to\":19,\"title\":\"第二段\",\"score\":150}]\n```";
        let v = parse_picks(reply, &s, o);
        assert_eq!(v.len(), 2, "{v:?}");
        assert_eq!((v[0].start_ms, v[0].end_ms, v[0].score), (14 * 5000 + 200, 19 * 5000 + 4800, 100.0));
        assert_eq!(v[1].title, "为什么要这样");
        assert_eq!(v[1].reasons, vec!["有钩子".to_string()]);
        assert!(parse_picks("没有 JSON", &s, o).is_empty());
        assert!(parse_picks("[1,2", &s, o).is_empty());
        assert!(llm_user(&s).starts_with("[0] (0:00) 这是一句普通的话。"));
    }

    #[tokio::test]
    async fn envelope_from_a_real_file_follows_the_loudness() {
        let Some(ff) =
            Some(std::path::PathBuf::from("ffmpeg")).filter(|f| std::process::Command::new(f).arg("-version").output().is_ok_and(|o| o.status.success()))
        else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("clearclip-hl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("loud.wav");
        // 前 4 秒很轻（-40 dB 的正弦），后 4 秒响（-10 dB）
        let st = std::process::Command::new(&ff)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=f=300:d=4",
                "-f",
                "lavfi",
                "-i",
                "sine=f=300:d=4",
                "-filter_complex",
                "[0]volume=-40dB[a];[1]volume=-10dB[b];[a][b]concat=n=2:v=0:a=1",
            ])
            .arg(&file)
            .status()
            .unwrap();
        assert!(st.success());
        let env = decode_envelope(&ff, &file.to_string_lossy(), 0, 8000).await.unwrap();
        assert!(env.len() >= ENV_HZ * 8 - 1 && env.len() <= ENV_HZ * 8 + 1, "{}", env.len());
        let (quiet, loud) = (env[ENV_HZ].max(env[ENV_HZ * 2]), env[ENV_HZ * 6].min(env[ENV_HZ * 5 + 2]));
        assert!(loud - quiet > 20.0, "{quiet} dB → {loud} dB");
        let _ = std::fs::remove_dir_all(dir);
    }
}
