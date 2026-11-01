//! 字幕工坊 S2：字幕轨列表、校对编辑（读取 / 校验 / 保存）、硬字幕样式（双语 ASS）和软 / 硬字幕合成。
//!
//! 编辑本身（改字、拆分、合并、查找替换、撤销重做）在界面里完成；这里负责落盘前的校验、
//! 保存时不覆盖原文件的保护（覆盖前留 `.bak`），以及合成视频时需要的样式。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::postprocess;
use crate::subtitle::{self, Cue};
use crate::{library, player, subgen, AppState};

type St<'a> = State<'a, Arc<AppState>>;

// ---------------------------------------------------------------- 字幕轨列表

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub path: String,
    pub label: String,
    pub ext: String,
    pub cues: usize,
    /// 多数条目是两行，认为是双语字幕
    pub bilingual: bool,
    pub size: u64,
}

/// 视频旁边所有同名字幕。
#[tauri::command]
pub fn subtitle_tracks(path: String) -> AppResult<Vec<TrackInfo>> {
    let video = PathBuf::from(&path);
    Ok(player::find_subtitles(&video)
        .into_iter()
        .map(|p| {
            let cues = library::read_cues(&p).unwrap_or_default();
            let two = cues.iter().filter(|c| c.lines.len() >= 2).count();
            TrackInfo {
                label: player::track_label(&video, &p),
                ext: p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default(),
                cues: cues.len(),
                bilingual: !cues.is_empty() && two * 2 > cues.len(),
                size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
                path: p.to_string_lossy().into_owned(),
            }
        })
        .collect())
}

// ---------------------------------------------------------------- 编辑

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub lines: Vec<String>,
}

impl From<&Cue> for EditCue {
    fn from(c: &Cue) -> Self {
        EditCue { start_ms: c.start, end_ms: c.end, lines: c.lines.clone() }
    }
}

impl From<&EditCue> for Cue {
    fn from(c: &EditCue) -> Self {
        Cue { start: c.start_ms, end: c.end_ms, lines: c.lines.clone() }
    }
}

/// 校对时提示的问题。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Issue {
    pub index: usize,
    /// reversed / overlap / empty / too-long / too-fast / too-short
    pub kind: &'static str,
    pub message: String,
}

/// 单条字幕最长显示时间（毫秒）和阅读速度上限（字 / 秒）。
const MAX_DURATION: u64 = 10_000;
const MAX_CPS: f64 = 20.0;
const MIN_DURATION: u64 = 300;

/// 检查时间和文字：结束早于开始、与上一条重叠、空文字、显示太久、太短、读不过来。
pub fn validate(cues: &[Cue]) -> Vec<Issue> {
    let mut out = vec![];
    for (i, c) in cues.iter().enumerate() {
        let text: String = c.lines.join("");
        if c.end <= c.start {
            out.push(Issue { index: i, kind: "reversed", message: "结束时间不晚于开始时间".into() });
            continue;
        }
        if text.trim().is_empty() {
            out.push(Issue { index: i, kind: "empty", message: "没有文字".into() });
        }
        let dur = c.end - c.start;
        if dur > MAX_DURATION {
            out.push(Issue { index: i, kind: "too-long", message: format!("显示 {:.1} 秒，太久", dur as f64 / 1000.0) });
        }
        if dur < MIN_DURATION {
            out.push(Issue { index: i, kind: "too-short", message: format!("只显示 {dur} 毫秒，一闪而过") });
        }
        let chars = text.chars().filter(|c| !c.is_whitespace()).count() as f64;
        if dur >= MIN_DURATION && chars / (dur as f64 / 1000.0) > MAX_CPS {
            out.push(Issue { index: i, kind: "too-fast", message: format!("每秒 {:.0} 个字，读不过来", chars / (dur as f64 / 1000.0)) });
        }
        if i > 0 && c.start < cues[i - 1].end {
            out.push(Issue { index: i, kind: "overlap", message: format!("和上一条重叠 {} 毫秒", cues[i - 1].end - c.start) });
        }
    }
    out
}

/// 读出一个字幕文件的条目（SRT / VTT / ASS）。
#[tauri::command]
pub fn subedit_load(path: String) -> AppResult<Vec<EditCue>> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(AppError::not_found("找不到字幕文件。"));
    }
    let cues = library::read_cues(&p).ok_or_else(|| AppError::invalid("不是可以编辑的字幕文件（支持 SRT、VTT、ASS）。"))?;
    Ok(cues.iter().map(EditCue::from).collect())
}

#[tauri::command]
pub fn subedit_validate(cues: Vec<EditCue>) -> Vec<Issue> {
    validate(&cues.iter().map(Cue::from).collect::<Vec<_>>())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub path: String,
    pub backup: Option<String>,
    pub count: usize,
}

/// 保存编辑结果。`overwrite=false`：另存为 `名字.已编辑.格式`；`true`：覆盖原文件，覆盖前先留一份 `.bak`（只留第一次的）。
/// `format` 不给时沿用原文件格式（只支持 srt / vtt / ass）。
pub fn save_cues(path: &Path, cues: &[Cue], overwrite: bool, format: Option<&str>) -> AppResult<SaveResult> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let fmt = format.map(str::to_ascii_lowercase).unwrap_or_else(|| if ext == "ssa" { "ass".into() } else { ext.clone() });
    if !matches!(fmt.as_str(), "srt" | "vtt" | "ass") {
        return Err(AppError::invalid("保存格式只能是 SRT、VTT 或 ASS。"));
    }
    if cues.is_empty() {
        return Err(AppError::invalid("没有字幕条目，不保存。"));
    }
    let mut sorted: Vec<Cue> = cues.to_vec();
    sorted.sort_by_key(|c| c.start);
    let text = subgen::export(&sorted, &fmt)?;
    let (target, backup) = if overwrite && fmt == ext {
        let bak = path.with_extension(format!("{ext}.bak"));
        let backup = if bak.exists() {
            None
        } else {
            std::fs::copy(path, &bak).map_err(|e| AppError::msg(format!("备份原字幕失败：{e}")))?;
            Some(bak.to_string_lossy().into_owned())
        };
        (path.to_path_buf(), backup)
    } else {
        (subtitle_out(path, "已编辑", &fmt), None)
    };
    std::fs::write(&target, text).map_err(|e| AppError::msg(format!("保存失败：{e}")))?;
    Ok(SaveResult { path: target.to_string_lossy().into_owned(), backup, count: sorted.len() })
}

fn subtitle_out(input: &Path, tag: &str, ext: &str) -> PathBuf {
    let stem = input.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "subtitle".into());
    let stem = stem.strip_suffix(&format!(".{tag}")).unwrap_or(&stem).to_string();
    crate::naming::unique_path(input.with_file_name(format!("{stem}.{tag}.{ext}")), &|p: &Path| p.exists())
}

#[tauri::command]
pub fn subedit_save(state: St<'_>, path: String, cues: Vec<EditCue>, overwrite: bool, format: Option<String>) -> AppResult<SaveResult> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(AppError::not_found("找不到字幕文件。"));
    }
    let list: Vec<Cue> = cues.iter().map(Cue::from).collect();
    let r = save_cues(&p, &list, overwrite, format.as_deref())?;
    // 新文件挂到原作品下，覆盖则重建搜索索引
    if !overwrite {
        crate::ai::register_subtitle(&state, &p, Path::new(&r.path), "edited");
    } else if let Ok(Some(item)) = state.db.item_by_path(&path) {
        let _ = library::index_subtitle_item(&state.db, &item);
    }
    Ok(r)
}

// ---------------------------------------------------------------- 硬字幕样式

/// 硬字幕样式。字号以 1080 高的画面为基准，其他分辨率按比例缩放。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct BurnStyle {
    pub font: String,
    /// 第一行（通常是原文）字号
    pub size: u32,
    /// 第二行（译文）字号
    pub size2: u32,
    pub color: String,
    pub color2: String,
    pub outline: u32,
    pub shadow: u32,
    /// 不透明底框
    pub boxed: bool,
    pub bold: bool,
    /// 距离画面底边（或顶边）的距离
    pub margin: u32,
    /// bottom / top
    pub position: String,
}

impl Default for BurnStyle {
    fn default() -> Self {
        preset("standard").unwrap()
    }
}

pub const PRESETS: [(&str, &str); 4] = [("standard", "标准白字"), ("bilingual", "双语（译文黄色）"), ("boxed", "黑底白字"), ("big", "大字（短视频）")];

pub fn preset(id: &str) -> Option<BurnStyle> {
    let base = BurnStyle {
        font: String::new(),
        size: 56,
        size2: 56,
        color: "#FFFFFF".into(),
        color2: "#FFFFFF".into(),
        outline: 2,
        shadow: 0,
        boxed: false,
        bold: false,
        margin: 50,
        position: "bottom".into(),
    };
    Some(match id {
        "standard" => base,
        "bilingual" => BurnStyle { size: 54, size2: 44, color2: "#FFD966".into(), ..base },
        "boxed" => BurnStyle { boxed: true, outline: 6, ..base },
        "big" => BurnStyle { size: 78, size2: 62, bold: true, outline: 4, margin: 260, color2: "#FFE066".into(), ..base },
        _ => return None,
    })
}

#[tauri::command]
pub fn subgen_burn_presets() -> Vec<(String, String, BurnStyle)> {
    PRESETS.iter().map(|(id, name)| (id.to_string(), name.to_string(), preset(id).unwrap())).collect()
}

/// `#RRGGBB` → ASS 的 `&H00BBGGRR`。写法不对时用白色。
fn ass_color(hex: &str) -> String {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("&H00{}{}{}", &h[4..6], &h[2..4], &h[0..2]).to_uppercase()
    } else {
        "&H00FFFFFF".into()
    }
}

/// 把条目按样式写成 ASS。第一行用主样式，其余行（译文）用第二种字号和颜色。`width` / `height` 是视频画面尺寸。
pub fn styled_ass(cues: &[Cue], style: &BurnStyle, width: u32, height: u32) -> String {
    let (w, h) = (width.max(2), height.max(2));
    // 以短边 1080 为基准缩放（横屏按高度，竖屏按宽度）
    let scale = f64::from(w.min(h)) / 1080.0;
    let sc = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    let font = if style.font.trim().is_empty() { "Microsoft YaHei" } else { style.font.trim() };
    let (border, outline_col, back) =
        if style.boxed { (3, "&H00000000".to_string(), "&H99000000".to_string()) } else { (1, "&H00000000".to_string(), "&H64000000".to_string()) };
    let align = if style.position == "top" { 8 } else { 2 };
    let side = (f64::from(w) * 0.05).round() as u32;
    let mut out = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,{font},{},{},{},{outline_col},{back},{},0,0,0,100,100,0,0,{border},{},{},{align},{side},{side},{},1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        sc(style.size),
        ass_color(&style.color),
        ass_color(&style.color),
        if style.bold { -1 } else { 0 },
        sc(style.outline),
        sc(style.shadow),
        sc(style.margin),
    );
    for c in cues {
        let mut parts = vec![];
        for (i, l) in c.lines.iter().enumerate() {
            let l = l.replace('{', "｛").replace('}', "｝");
            if i == 0 {
                parts.push(l);
            } else {
                parts.push(format!("{{\\fs{}\\c{}}}{l}", sc(style.size2), ass_color(&style.color2)));
            }
        }
        out.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
            subtitle::fmt_ass_time(c.start / 10),
            subtitle::fmt_ass_time(c.end / 10),
            parts.join("\\N")
        ));
    }
    out
}

// ---------------------------------------------------------------- 合成

fn need_ffmpeg(st: &AppState) -> AppResult<PathBuf> {
    postprocess::find_ffmpeg(st).ok_or_else(|| AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg。请在“设置 → 组件”中安装 ffmpeg 后重试。"))
}

fn lang_of_track(video: &Path, sub: &Path) -> String {
    let stem = video.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let sub_stem = sub.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    sub_stem.strip_prefix(&stem).unwrap_or("").trim_matches(|c: char| c == '.' || c == '_' || c == '-' || c == ' ').split('.').next().unwrap_or("").to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MuxOpts {
    /// soft = 软字幕（多轨内嵌，不重新编码）；hard = 硬字幕（烧进画面）
    pub mode: String,
    /// 要用的字幕文件（软字幕可多个；硬字幕只用第一个）
    pub tracks: Vec<String>,
    pub style: Option<BurnStyle>,
}

impl Default for MuxOpts {
    fn default() -> Self {
        MuxOpts { mode: "soft".into(), tracks: vec![], style: None }
    }
}

/// 给视频合成字幕：软字幕多轨内嵌，或按样式烧录。输出放在原视频旁边，原视频不动。
#[tauri::command]
pub fn subgen_mux(app: AppHandle, state: St<'_>, video: String, opts: MuxOpts) -> AppResult<u64> {
    let v = PathBuf::from(&video);
    if !v.is_file() {
        return Err(AppError::not_found("找不到视频文件。"));
    }
    if opts.tracks.is_empty() {
        return Err(AppError::invalid("请先选择要合成的字幕。"));
    }
    if !matches!(opts.mode.as_str(), "soft" | "hard") {
        return Err(AppError::invalid("合成方式只能是软字幕或硬字幕。"));
    }
    let subs: Vec<PathBuf> = opts.tracks.iter().map(PathBuf::from).collect();
    if let Some(bad) = subs.iter().find(|s| !s.is_file()) {
        return Err(AppError::not_found(format!("找不到字幕文件：{}", bad.display())));
    }
    need_ffmpeg(&state)?;
    let title = v.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let label = if opts.mode == "soft" { "内嵌字幕" } else { "烧录字幕" };
    Ok(crate::media_tools::spawn_job(&app, title, label, move |ctx| async move {
        let ff = need_ffmpeg(&ctx.st)?;
        let ext = v.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| "mp4".into());
        let base = v.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if opts.mode == "soft" {
            let out = crate::ai::out_beside(&v, &base, "软字幕", &ext);
            let tracks: Vec<postprocess::SubTrack> = subs
                .iter()
                .map(|p| {
                    let lang = lang_of_track(&v, p);
                    postprocess::SubTrack { path: p.clone(), title: player::track_label(&v, p), lang: if lang.is_empty() { "und".into() } else { lang } }
                })
                .collect();
            ctx.note("正在内嵌字幕…");
            let n = postprocess::embed_subtitles(&ff, &v, &out, &out, &tracks).await?;
            if n == 0 {
                let _ = std::fs::remove_file(&out);
                return Err(AppError::unsupported("这种视频格式不能内嵌字幕（TS 等）。可以换 MP4 / MKV，或者用硬字幕。"));
            }
            return Ok(out);
        }
        // 硬字幕：按视频尺寸生成样式 ASS，放在视频旁边的临时文件，烧完删除
        let probe = postprocess::probe(&ff, &v).await;
        let (w, h) = (probe.width.unwrap_or(1920), probe.height.unwrap_or(1080));
        let cues = library::read_cues(&subs[0]).ok_or_else(|| AppError::invalid("读不了这个字幕文件。"))?;
        let ass = v.with_file_name(format!("clearclip-burn-{}.ass", ctx.id));
        std::fs::write(&ass, styled_ass(&cues, &opts.style.clone().unwrap_or_default(), w, h)).map_err(|e| AppError::msg(format!("写临时字幕失败：{e}")))?;
        let out = crate::ai::out_beside(&v, &base, "硬字幕", &ext);
        ctx.note("正在把字幕烧进画面（需要重新编码，较慢）…");
        let r = postprocess::burn_subtitles(&ff, &v, &out, &out, std::slice::from_ref(&ass)).await;
        let _ = std::fs::remove_file(&ass);
        r?;
        Ok(out)
    }))
}

/// 预览：取视频某一时刻的一帧，按样式烧上字幕，返回 PNG 的 data URL。
#[tauri::command]
pub async fn subgen_burn_preview(state: St<'_>, video: String, track: String, style: BurnStyle, at_ms: Option<u64>) -> AppResult<String> {
    use base64::Engine;
    let ff = need_ffmpeg(&state)?;
    let (v, t) = (PathBuf::from(&video), PathBuf::from(&track));
    if !v.is_file() || !t.is_file() {
        return Err(AppError::not_found("找不到视频或字幕文件。"));
    }
    let cues = library::read_cues(&t).ok_or_else(|| AppError::invalid("读不了这个字幕文件。"))?;
    // 默认取第一条字幕出现的时刻，保证画面上有字
    let at = at_ms.unwrap_or_else(|| cues.first().map(|c| (c.start + c.end) / 2).unwrap_or(0));
    let probe = postprocess::probe(&ff, &v).await;
    let (w, h) = (probe.width.unwrap_or(1920), probe.height.unwrap_or(1080));
    let dir = std::env::temp_dir().join(format!("clearclip-burn-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| AppError::msg(e.to_string()))?;
    let ass = dir.join("p.ass");
    std::fs::write(&ass, styled_ass(&cues, &style, w, h)).map_err(|e| AppError::msg(e.to_string()))?;
    let png = dir.join("p.png");
    let args: Vec<String> = [
        "-hide_banner".to_string(),
        "-y".into(),
        "-ss".into(),
        format!("{:.3}", at as f64 / 1000.0),
        "-i".into(),
        v.to_string_lossy().into_owned(),
        "-vf".into(),
        // 字幕时间轴从 0 开始，-ss 在 -i 前会让时间戳归零，所以用 setpts 把时间补回去
        format!("setpts=PTS+{:.3}/TB,ass=p.ass,scale='min(960,iw)':-2", at as f64 / 1000.0),
        "-frames:v".into(),
        "1".into(),
        "p.png".into(),
    ]
    .to_vec();
    let run = postprocess::run_ffmpeg_in(&ff, &args, Some(&dir)).await;
    let bytes = run.and_then(|_| std::fs::read(&png).map_err(|e| AppError::msg(e.to_string())));
    let _ = std::fs::remove_dir_all(&dir);
    let bytes = bytes.map_err(|e| AppError::msg(format!("生成预览失败：{}", e.message)))?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(test)]
#[path = "subedit_tests.rs"]
mod tests;
