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
use super::spec::{ClipKind, OutputSpec, Project, MAX_COMPOUND_DEPTH, MIN_MS};
use crate::ml::matte::render_matte;

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
        if c.out_ms > d && !c.freeze {
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
        // 调整图层没有素材文件
        if path.is_empty() || src.contains_key(path) {
            continue;
        }
        if !Path::new(path).is_file() {
            return Err(AppError::invalid(format!("找不到素材文件：{path}")));
        }
        let f = facts::read(ffmpeg, Path::new(path)).await.ok_or_else(|| AppError::invalid(format!("无法读取素材：{path}")))?;
        src.insert(path.clone(), Source::from_facts(&f));
        ctx.check()?;
    }
    for (n, c) in p.clips.iter().enumerate() {
        if !c.lut.is_empty() && !Path::new(&c.lut).is_file() {
            return Err(AppError::invalid(format!("找不到第 {} 个片段的 LUT 文件：{}", n + 1, c.lut)));
        }
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
    prepare_at(ctx, project, mode, 0).await
}

/// 一个要先渲染的复合片段。
pub(super) struct CompoundJob {
    pub index: usize,
    pub id: u32,
    pub name: String,
    /// 子时间线，已经带上和外面一样的输出尺寸、帧率和画面适配方式，质量固定用“高”（免得多压一遍掉画质）
    pub inner: Project,
}

/// 工程里有哪些复合片段要先渲染（按片段顺序）。
pub(super) fn compound_jobs(project: &Project) -> AppResult<Vec<CompoundJob>> {
    let mut out = vec![];
    for (i, c) in project.clips.iter().enumerate() {
        let Some(id) = c.compound else { continue };
        let comp = project.compounds.iter().find(|x| x.id == id).ok_or_else(|| AppError::invalid(format!("找不到复合片段（编号 {id}）的内容。")))?;
        let mut inner = comp.project.clone();
        inner.title.clear();
        inner.compounds = project.compounds.iter().filter(|x| x.id != id).cloned().collect();
        inner.out = OutputSpec {
            width: project.out.width,
            height: project.out.height,
            fps: project.out.fps,
            fit: project.out.fit.clone(),
            codec: project.out.codec.clone(),
            quality: "high".into(),
            ..OutputSpec::default()
        };
        out.push(CompoundJob { index: i, id, name: comp.name.clone(), inner });
    }
    Ok(out)
}

/// 复合片段渲染好以后，把它换成指向这个文件的普通视频片段（片段自己的裁剪、转场、音量等设置保留）。
pub(super) fn apply_rendered(project: &mut Project, index: usize, path: &Path, out_ms: u64) {
    let c = &mut project.clips[index];
    c.path = path.to_string_lossy().into_owned();
    c.kind = ClipKind::Video;
    c.compound = None;
    c.in_ms = c.in_ms.min(out_ms.saturating_sub(MIN_MS));
    c.out_ms = c.out_ms.min(out_ms).max(c.in_ms + MIN_MS);
}

/// 把工程里的复合片段先各自渲染成一个临时视频，换成普通的视频片段（子时间线里再有复合片段，就一层一层往里渲染）。
/// 临时文件放在任务的临时文件夹里，任务结束时一起删。
fn flatten_compounds<'a>(
    ctx: &'a JobCtx,
    project: &'a mut Project,
    mode: Mode,
    depth: u32,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send + 'a>> {
    Box::pin(async move {
        let jobs = compound_jobs(project)?;
        if jobs.is_empty() {
            return Ok(());
        }
        if depth >= MAX_COMPOUND_DEPTH {
            return Err(AppError::invalid(format!("复合片段嵌套太深了（最多 {MAX_COMPOUND_DEPTH} 层）。")));
        }
        let tmp = tmp_dir(ctx);
        std::fs::create_dir_all(&tmp)?;
        let n = jobs.len();
        for (k, j) in jobs.into_iter().enumerate() {
            ctx.check()?;
            ctx.note(format!("正在渲染复合片段“{}”（{}/{}）…", j.name, k + 1, n));
            let pre = prepare_at(ctx, &j.inner, mode, depth + 1).await?;
            let out = tmp.join(format!("compound-{}-{depth}-{}.{}", j.id, j.index, pre.plan.ext));
            run(ctx, &pre, &out).await?;
            apply_rendered(project, j.index, &out, pre.plan.out_ms);
        }
        Ok(())
    })
}

async fn prepare_at(ctx: &JobCtx, project: &Project, mode: Mode, depth: u32) -> AppResult<Prepared> {
    let st = &ctx.st;
    let ffmpeg = find_ffmpeg(st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let caps = st.vcaps.get(&ffmpeg).await;
    let mut project = project.clone();
    flatten_compounds(ctx, &mut project, mode, depth).await?;
    let mut project = project.checked().map_err(AppError::invalid)?.effective();
    ctx.note("正在检查素材…");
    let sources = read_sources(&ffmpeg, &project, ctx).await?;
    let notes = fit_to_sources(&mut project, &sources)?;
    let tmp = tmp_dir(ctx);
    bake_lut_strength(&mut project, &tmp)?;
    let font = fonts::default_font();
    let plan = build(&project, &sources, &caps, &tmp, mode, font.as_deref())?;
    Ok(Prepared { ffmpeg, caps, project, plan, notes, tmp })
}

/// LUT 强度不到 100% 的片段：把“原样”和这张 LUT 按强度混成一张新的 `.cube`（放在临时文件夹），导出时套这张。
pub(super) fn bake_lut_strength(p: &mut Project, tmp: &Path) -> AppResult<()> {
    let mut done: HashMap<(String, u32), String> = HashMap::new();
    for c in p.clips.iter_mut().filter(|c| !c.lut.is_empty() && c.lut_mix < 0.995) {
        let key = (c.lut.clone(), (c.lut_mix * 1000.0).round() as u32);
        if let Some(path) = done.get(&key) {
            c.lut = path.clone();
            continue;
        }
        let text = std::fs::read_to_string(&c.lut).map_err(|e| AppError::invalid(format!("读不了 LUT 文件 {}：{e}", c.lut)))?;
        let lut = crate::lut::Lut3::parse(&text).map_err(|e| AppError::invalid(format!("LUT 文件 {} 不能用：{e}", c.lut)))?;
        std::fs::create_dir_all(tmp)?;
        let out = tmp.join(format!("lut-{}.cube", done.len()));
        std::fs::write(&out, lut.with_strength(c.lut_mix as f32).to_cube())?;
        let path = out.to_string_lossy().into_owned();
        done.insert(key, path.clone());
        c.lut = path;
    }
    Ok(())
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
    // 人像抠图：逐个片段跑模型，生成抠图视频，ffmpeg 把它们当输入读
    let mt = pre.plan.mattes.len();
    for (k, m) in pre.plan.mattes.iter().enumerate() {
        let mut job = m.clone();
        job.model = crate::ml::store::require(&ctx.st, &job.model_id)?;
        ctx.note(format!("正在用 AI 抠出人像（{}/{}）…第一次会比较慢", k + 1, mt));
        let base = k as f64 / mt as f64;
        render_matte(&pre.ffmpeg, &job, &ctx.cancel, |p| ctx.percent((base + p / mt as f64) * 40.0)).await?;
    }
    // 区域效果的遮罩：先逐个生成好，ffmpeg 把它们当输入读
    let total = pre.plan.masks.len();
    for (k, m) in pre.plan.masks.iter().enumerate() {
        ctx.note(format!("正在生成区域遮罩（{}/{}）…", k + 1, total));
        render_mask(&pre.ffmpeg, m, || ctx.check()).await?;
    }
    // 防抖：先逐个片段分析抖动（vidstab 的第一遍），结果写到临时文件夹
    let ns = pre.plan.stabs.len();
    if ns > 0 {
        std::fs::create_dir_all(&pre.tmp)?;
    }
    for (k, a) in pre.plan.stabs.iter().enumerate() {
        ctx.note(format!("正在分析画面抖动（{}/{}）…", k + 1, ns));
        match run_ffmpeg_progress(&pre.ffmpeg, a, None, &ctx.cancel, &ctx.wake, &|_| {}).await {
            RunEnd::Ok => {}
            RunEnd::Canceled => return Err(AppError::msg("canceled")),
            RunEnd::Failed(m) => return Err(AppError::msg(format!("防抖分析失败：{m}"))),
        }
    }
    let mut args = pre.plan.args.clone();
    args.extend(muxer_args(pre.plan.ext, out));
    let off = if mt > 0 { 40.0 } else { 0.0 };
    let on_percent = |p: f64| ctx.percent(off + p * (100.0 - off) / 100.0);
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
    let first_path = p.first_media_path().unwrap_or_default();
    let first = Path::new(&first_path);
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
    // 用最初的工程取名和定位：渲染过的工程里复合片段已经换成了临时文件，不能拿它的路径当导出位置
    let out = output_path(project, dir, pre.plan.ext);
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
