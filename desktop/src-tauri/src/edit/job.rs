//! 剪辑的后台任务：导出成片、生成预览。先读取所有素材的信息，核对起止时间，再生成参数运行 ffmpeg。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::media_tools::{muxer_args, run_ffmpeg_progress, JobCtx, RunEnd, ToolJob};
use crate::postprocess::find_ffmpeg;
use crate::vidcaps::Caps;
use crate::vidnorm::facts;

use super::build::{build, EditPlan, Mode, Source};
use super::fonts;
use super::region::render_mask;
use super::spec::{ClipKind, Project, MIN_MS};

fn tmp_dir(ctx: &JobCtx) -> PathBuf {
    ctx.st.data_dir.join("tmp").join(format!("edit-{}", ctx.id))
}

fn nonempty(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false)
}

fn short_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(text.as_bytes())[..8])
}

fn show_ms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}.{}", s / 60, s % 60, (ms % 1000) / 100)
}

/// 核对每个片段的起止时间和素材的实际长度：结束时间超出素材的缩短到素材结尾，开始时间就超出的报错。返回要告诉用户的提示。
pub fn fit_to_sources(p: &mut Project, src: &HashMap<String, Source>) -> AppResult<Vec<String>> {
    let mut notes = vec![];
    for (n, c) in p.clips.iter_mut().enumerate() {
        if c.kind != ClipKind::Video {
            continue;
        }
        let Some(d) = src.get(&c.path).and_then(|s| s.duration_ms) else { continue };
        if c.in_ms + MIN_MS > d {
            return Err(AppError::invalid(format!("第 {} 个片段的开始时间（{}）已经超出素材的长度（{}）。", n + 1, show_ms(c.in_ms), show_ms(d))));
        }
        if c.out_ms > d {
            c.out_ms = d;
            notes.push(format!("第 {} 个片段超出素材的长度，已缩短到素材结尾", n + 1));
        }
    }
    for (n, o) in p.overlays.iter_mut().enumerate() {
        if o.kind != ClipKind::Video {
            continue;
        }
        let Some(d) = src.get(&o.path).and_then(|s| s.duration_ms) else { continue };
        if o.in_ms + MIN_MS > d {
            return Err(AppError::invalid(format!("第 {} 个叠加素材的开始时间（{}）已经超出素材的长度（{}）。", n + 1, show_ms(o.in_ms), show_ms(d))));
        }
        // 设了“循环播放”的可以比素材长（重复播放）；其他的缩短到素材结尾
        if o.out_ms > d && !o.looped {
            o.out_ms = d;
            let dur = o.duration_ms();
            o.fade_in_ms = o.fade_in_ms.min(dur);
            o.fade_out_ms = o.fade_out_ms.min(dur.saturating_sub(o.fade_in_ms));
            notes.push(format!("第 {} 个叠加素材超出素材的长度，已缩短到素材结尾", n + 1));
        }
    }
    for (n, a) in p.audio.iter_mut().enumerate() {
        let Some(d) = src.get(&a.path).and_then(|s| s.duration_ms) else { continue };
        if a.in_ms + MIN_MS > d {
            return Err(AppError::invalid(format!("第 {} 条音频轨的开始时间（{}）已经超出素材的长度（{}）。", n + 1, show_ms(a.in_ms), show_ms(d))));
        }
        if a.out_ms.is_some_and(|o| o > d) {
            a.out_ms = Some(d);
        }
    }
    Ok(notes)
}

/// 读取所有素材的信息。
async fn read_sources(ffmpeg: &Path, p: &Project, ctx: &JobCtx) -> AppResult<HashMap<String, Source>> {
    let mut src: HashMap<String, Source> = HashMap::new();
    let paths: Vec<&String> = p.clips.iter().map(|c| &c.path).chain(p.overlays.iter().map(|o| &o.path)).chain(p.audio.iter().map(|a| &a.path)).collect();
    for path in paths {
        if src.contains_key(path) {
            continue;
        }
        if !Path::new(path).is_file() {
            return Err(AppError::invalid(format!("找不到素材文件：{path}")));
        }
        let f = facts::read(ffmpeg, Path::new(path)).await.ok_or_else(|| AppError::invalid(format!("无法读取素材：{path}")))?;
        src.insert(path.clone(), Source::from_facts(&f));
        ctx.check()?;
    }
    for t in &p.texts {
        if let Some(f) = &t.font {
            if !Path::new(f).is_file() {
                return Err(AppError::invalid(format!("找不到字体文件：{f}")));
            }
        }
    }
    Ok(src)
}

struct Prepared {
    ffmpeg: PathBuf,
    caps: std::sync::Arc<Caps>,
    project: Project,
    plan: EditPlan,
    notes: Vec<String>,
    tmp: PathBuf,
}

async fn prepare(ctx: &JobCtx, project: &Project, mode: Mode) -> AppResult<Prepared> {
    let st = &ctx.st;
    let ffmpeg = find_ffmpeg(st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let caps = st.vcaps.get(&ffmpeg).await;
    let mut project = project.clone().checked().map_err(AppError::invalid)?;
    ctx.note("正在检查素材…");
    let sources = read_sources(&ffmpeg, &project, ctx).await?;
    let notes = fit_to_sources(&mut project, &sources)?;
    let tmp = tmp_dir(ctx);
    let font = fonts::default_font();
    let plan = build(&project, &sources, &caps, &tmp, mode, font.as_deref())?;
    Ok(Prepared { ffmpeg, caps, project, plan, notes, tmp })
}

fn note_text(notes: &[String], plan: &EditPlan) -> String {
    let mut t = notes.iter().chain(plan.notes.iter()).cloned().collect::<Vec<_>>().join("；");
    if !plan.warnings.is_empty() {
        if !t.is_empty() {
            t.push('　');
        }
        t.push_str(&format!("注意：{}", plan.warnings.join("；")));
    }
    t
}

async fn run(ctx: &JobCtx, pre: &Prepared, out: &Path) -> AppResult<()> {
    // 区域效果的遮罩：先逐个生成好，ffmpeg 把它们当输入读
    let total = pre.plan.masks.len();
    for (k, m) in pre.plan.masks.iter().enumerate() {
        ctx.note(format!("正在生成区域遮罩（{}/{}）…", k + 1, total));
        render_mask(&pre.ffmpeg, m, || ctx.check()).await?;
    }
    let mut args = pre.plan.args.clone();
    args.extend(muxer_args(pre.plan.ext, out));
    let on_percent = |p: f64| ctx.percent(p);
    let end = run_ffmpeg_progress(&pre.ffmpeg, &args, Some(pre.plan.out_ms), &ctx.cancel, &ctx.wake, &on_percent).await;
    match end {
        RunEnd::Ok if nonempty(out) => Ok(()),
        RunEnd::Ok => {
            let _ = std::fs::remove_file(out);
            Err(AppError::msg("没有生成任何输出。"))
        }
        RunEnd::Canceled => {
            let _ = std::fs::remove_file(out);
            Err(AppError::msg("canceled"))
        }
        RunEnd::Failed(m) => {
            let _ = std::fs::remove_file(out);
            Err(AppError::msg(m))
        }
    }
}

/// 导出文件的路径：有标题用标题，没有就用第一个片段的名字加“.剪辑”。
pub fn output_path(p: &Project, dir: Option<&Path>, ext: &str) -> PathBuf {
    let first = Path::new(&p.clips[0].path);
    let dir = dir.map(Path::to_path_buf).or_else(|| first.parent().map(Path::to_path_buf)).unwrap_or_default();
    let title = crate::naming::sanitize(p.title.trim());
    let name = if title.is_empty() {
        format!("{}.剪辑", first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "output".into()))
    } else {
        title
    };
    crate::naming::unique_path(dir.join(format!("{name}.{ext}")), &|p: &Path| p.exists())
}

/// 导出。
pub async fn run_edit(ctx: &JobCtx, job: &ToolJob, project: &Project) -> AppResult<PathBuf> {
    let pre = prepare(ctx, project, Mode::Export).await?;
    ctx.note(note_text(&pre.notes, &pre.plan));
    let dir = job.output_dir.as_deref().filter(|d| !d.trim().is_empty()).map(Path::new);
    if let Some(d) = dir {
        std::fs::create_dir_all(d)?;
    }
    let out = output_path(&pre.project, dir, pre.plan.ext);
    let result = run(ctx, &pre, &out).await;
    let _ = std::fs::remove_dir_all(&pre.tmp);
    result?;
    Ok(out)
}

/// 清理旧的预览文件和缩略图。
fn clean_old(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let limit = if name.starts_with("edit-prev-") {
            3600 * 3
        } else if name.starts_with("edit-thumb-") {
            3600 * 24 * 7
        } else {
            continue;
        };
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > std::time::Duration::from_secs(limit));
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// 生成低分辨率的预览，返回预览文件。工程和素材都没变时直接用上次生成的。
pub async fn run_preview(ctx: &JobCtx, project: &Project) -> AppResult<PathBuf> {
    let pre = prepare(ctx, project, Mode::Preview).await?;
    let dir = ctx.st.data_dir.join("previews");
    std::fs::create_dir_all(&dir)?;
    clean_old(&dir);
    // 素材改过（大小或修改时间不同）预览就要重做
    let stamps: Vec<String> = pre
        .project
        .media_paths()
        .iter()
        .map(|p| {
            let m = std::fs::metadata(p).ok();
            format!(
                "{p}|{}|{}",
                m.as_ref().map_or(0, |m| m.len()),
                m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
            )
        })
        .collect();
    let key = short_hash(&format!("{}|{}|{}", serde_json::to_string(&pre.project).unwrap_or_default(), stamps.join(";"), pre.caps.version));
    let out = dir.join(format!("edit-prev-{key}.{}", pre.plan.ext));
    if nonempty(&out) {
        let _ = std::fs::remove_dir_all(&pre.tmp);
        ctx.note("工程没有变化，使用上次生成的预览");
        return Ok(out);
    }
    ctx.note(note_text(&pre.notes, &pre.plan));
    let partial = dir.join(format!("edit-prev-{key}.part.{}", pre.plan.ext));
    let result = run(ctx, &pre, &partial).await;
    let _ = std::fs::remove_dir_all(&pre.tmp);
    result?;
    std::fs::rename(&partial, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::spec::{AudioTrack, Clip, Overlay};

    fn src(paths: &[(&str, Option<u64>)]) -> HashMap<String, Source> {
        paths
            .iter()
            .map(|(p, d)| (p.to_string(), Source { has_video: true, has_audio: true, width: 1280, height: 720, fps: Some(30.0), duration_ms: *d, hdr: false }))
            .collect()
    }

    #[test]
    fn clips_past_the_end_of_the_file_are_shortened_or_refused() {
        let mut p = Project {
            clips: vec![
                Clip { path: "/m/a.mp4".into(), in_ms: 1000, out_ms: 9000, ..Default::default() },
                Clip { path: "/m/b.mp4".into(), in_ms: 0, out_ms: 9000, ..Default::default() },
            ],
            audio: vec![AudioTrack { path: "/m/a.mp3".into(), out_ms: Some(99_000), ..Default::default() }],
            ..Default::default()
        };
        let s = src(&[("/m/a.mp4", Some(5000)), ("/m/b.mp4", None), ("/m/a.mp3", Some(30_000))]);
        let notes = fit_to_sources(&mut p, &s).unwrap();
        assert_eq!(p.clips[0].out_ms, 5000);
        assert_eq!(p.clips[1].out_ms, 9000, "不知道素材长度时不改");
        assert_eq!(p.audio[0].out_ms, Some(30_000));
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("第 1 个片段"));
        p.clips[0].in_ms = 5000;
        let e = fit_to_sources(&mut p, &s).unwrap_err();
        assert!(e.message.contains("第 1 个片段的开始时间") && e.message.contains("0:05.0"), "{}", e.message);
    }

    #[test]
    fn overlays_are_shortened_unless_they_loop() {
        let mk = |looped: bool| Overlay { path: "/o/a.gif".into(), out_ms: 6000, looped, fade_out_ms: 500, ..Default::default() };
        let mut p = Project {
            clips: vec![Clip { path: "/m/a.mp4".into(), out_ms: 9000, ..Default::default() }],
            overlays: vec![mk(false), mk(true)],
            ..Default::default()
        };
        let s = src(&[("/m/a.mp4", Some(9000)), ("/o/a.gif", Some(2000))]);
        let notes = fit_to_sources(&mut p, &s).unwrap();
        assert_eq!(p.overlays[0].out_ms, 2000, "不循环：缩短到素材结尾");
        assert_eq!(p.overlays[1].out_ms, 6000, "循环：可以比素材长");
        assert!(notes.iter().any(|n| n.contains("第 1 个叠加素材")) && !notes.iter().any(|n| n.contains("第 2 个叠加素材")), "{notes:?}");
        p.overlays[1].in_ms = 2000;
        let e = fit_to_sources(&mut p, &s).unwrap_err();
        assert!(e.message.contains("第 2 个叠加素材的开始时间"), "{}", e.message);
    }

    #[test]
    fn output_is_named_after_the_title_or_the_first_clip() {
        let d = std::env::temp_dir().join(format!("clearclip-editout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let first = d.join("旅行.mp4");
        let mut p = Project { clips: vec![Clip { path: first.to_string_lossy().into_owned(), out_ms: 1000, ..Default::default() }], ..Default::default() };
        assert_eq!(output_path(&p, None, "mp4"), d.join("旅行.剪辑.mp4"));
        p.title = " 我的/成片:1 ".into();
        assert_eq!(output_path(&p, None, "mkv"), d.join("我的_成片_1.mkv"));
        std::fs::write(d.join("我的_成片_1.mkv"), "x").unwrap();
        assert_eq!(output_path(&p, None, "mkv"), d.join("我的_成片_1 (1).mkv"));
        let other = d.join("out");
        assert_eq!(output_path(&p, Some(&other), "mp4"), other.join("我的_成片_1.mp4"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn times_are_shown_as_minutes_and_seconds() {
        assert_eq!(show_ms(0), "0:00.0");
        assert_eq!(show_ms(65_400), "1:05.4");
    }
}
