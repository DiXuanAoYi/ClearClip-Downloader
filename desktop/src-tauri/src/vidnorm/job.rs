//! 视频规整和防抖的后台任务：先检查文件、做需要解码的分析（可变帧率、黑边、响度），再生成参数并运行 ffmpeg。

use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::media_tools::{muxer_args, out_path, run_ffmpeg_progress, s, video_codec_for, JobCtx, RunEnd, ToolJob};
use crate::postprocess::find_ffmpeg;

use super::build::{build, filter_path};
use super::spec::{Analysis, NormSpec};
use super::{analyze, facts};

fn tmp_dir(ctx: &JobCtx) -> PathBuf {
    ctx.st.data_dir.join("tmp").join(format!("vn-{}", ctx.id))
}

/// 运行一遍 ffmpeg，进度折算到 `[from, from + span]`。成功返回 Ok；取消或失败时删除不完整的输出。
async fn exec(ctx: &JobCtx, ffmpeg: &Path, args: &[String], out_ms: Option<u64>, from: f64, span: f64, partial: Option<&Path>) -> AppResult<()> {
    let on_percent = |p: f64| ctx.percent(from + p * span / 100.0);
    let end = run_ffmpeg_progress(ffmpeg, args, out_ms, &ctx.cancel, &ctx.wake, &on_percent).await;
    match end {
        RunEnd::Ok => Ok(()),
        other => {
            if let Some(p) = partial {
                let _ = std::fs::remove_file(p);
            }
            match other {
                RunEnd::Canceled => Err(AppError::msg("canceled")),
                RunEnd::Failed(m) => Err(AppError::msg(m)),
                RunEnd::Ok => unreachable!(),
            }
        }
    }
}

fn nonempty(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.len() > 0).unwrap_or(false)
}

fn join_notes(notes: &[String], warnings: &[String]) -> String {
    let mut t = notes.join("；");
    if !warnings.is_empty() {
        if !t.is_empty() {
            t.push('　');
        }
        t.push_str(&format!("注意：{}", warnings.join("；")));
    }
    t
}

/// 视频规整。返回输出文件；素材已经符合规格时返回输入文件本身（调用方据此不登记媒体库）。
pub async fn run_normalize(ctx: &JobCtx, job: &ToolJob, spec: &NormSpec) -> AppResult<PathBuf> {
    let st = &ctx.st;
    let ffmpeg = find_ffmpeg(st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let caps = st.vcaps.get(&ffmpeg).await;
    let spec = spec.clone().checked().map_err(AppError::invalid)?;
    let input = PathBuf::from(job.inputs.first().ok_or_else(|| AppError::invalid("请先选择文件。"))?);
    let dir = job.output_dir.as_deref().filter(|d| !d.trim().is_empty()).map(Path::new);

    ctx.note("正在检查文件…");
    let facts = facts::read(&ffmpeg, &input).await.ok_or_else(|| AppError::msg("无法读取这个文件。"))?;
    if facts.video.is_none() {
        return Err(AppError::invalid("这个文件里没有视频画面，视频规整只适用于视频文件。"));
    }
    ctx.check()?;

    let mut an = Analysis::default();
    if spec.fps == "auto" || spec.fix_sync || spec.autocrop {
        ctx.note("正在检测帧率和黑边…");
        let (vfr, crop) = tokio::join!(
            async {
                if spec.fps == "auto" || spec.fix_sync {
                    analyze::detect_vfr(&ffmpeg, &input).await
                } else {
                    None
                }
            },
            async {
                if spec.autocrop && caps.has_filter("cropdetect") {
                    analyze::detect_crop(&ffmpeg, &input, &facts).await
                } else {
                    None
                }
            }
        );
        an.vfr = vfr;
        an.crop = crop;
        ctx.check()?;
    }
    if let (Some(t), Some(_)) = (spec.loudness, &facts.audio) {
        ctx.note("正在测量响度…");
        an.loudness = analyze::measure_loudness(&ffmpeg, &input, t).await;
        ctx.check()?;
    }

    // 分段色彩匹配：分析镜头之间的色彩，找出和整体不一致的镜头
    let mut color_notes: Vec<String> = vec![];
    let mut color_warnings: Vec<String> = vec![];
    if spec.match_color > 0.0 {
        ctx.note("正在分析镜头之间的色彩…");
        let analysis = super::colormatch::analyze_cached(&ffmpeg, &input, &facts, &caps, &spec.match_skip).await;
        let color = match &analysis {
            Ok(a) => a.plan(spec.match_color, &spec.match_tone, &spec.match_shots),
            // 分析不了（HDR、太短……）时，统一的手动调节不需要镜头信息，照样做
            Err(_) => super::colormatch::tone_plan(&spec.match_tone, &spec.match_skip),
        };
        match &analysis {
            Ok(a) if color.fixes.is_empty() => color_notes.push(a.note.clone().unwrap_or_else(|| "没有需要校正的镜头".into())),
            Err(e) => color_warnings.push(format!("分段色彩匹配没有分析镜头之间的色彩：{e}")),
            Ok(_) => {}
        }
        if !color.fixes.is_empty() {
            an.color = Some(color);
        }
        ctx.check()?;
    }

    let tmp = tmp_dir(ctx);
    let mut plan = build(&input, &spec, &facts, &an, &caps, &tmp)?;
    plan.notes.extend(color_notes);
    plan.warnings.extend(color_warnings);
    if spec.autocrop {
        plan.warnings.extend(caps.autocrop_warning());
    }
    let note = join_notes(&plan.notes, &plan.warnings);
    if plan.unchanged {
        ctx.note(format!("已经符合所选规格，没有生成新文件。{note}"));
        let _ = std::fs::remove_dir_all(&tmp);
        return Ok(input);
    }
    ctx.note(note);

    let out = out_path(&input, dir, "规整", plan.ext);
    let mut args = plan.args.clone();
    args.extend(muxer_args(plan.ext, &out));
    let result = exec(ctx, &ffmpeg, &args, plan.out_ms, 0.0, 100.0, Some(&out)).await;
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    if !nonempty(&out) {
        let _ = std::fs::remove_file(&out);
        return Err(AppError::msg("没有生成任何输出。"));
    }
    Ok(out)
}

/// 防抖的执行计划。
pub struct StabPlan {
    /// vidstab 的第一遍（分析抖动，写出 trf 文件）；deshake 单遍时为 None
    pub pass1: Option<Vec<String>>,
    /// 生成输出的那一遍（不含输出文件）
    pub pass2: Vec<String>,
    pub ext: &'static str,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

/// 防抖的参数。有 vidstab 时两遍处理，没有时用 deshake 单遍（效果较弱），都没有就报错。
pub fn stabilize_plan(input: &Path, strength: &str, trf: &Path, caps: &crate::vidcaps::Caps, facts: &facts::Facts) -> AppResult<StabPlan> {
    let v = facts.video.as_ref().ok_or_else(|| AppError::invalid("这个文件里没有视频画面。"))?;
    let (w, h) = v.display_size();
    let pps = f64::from(w) * f64::from(h) * v.fps.unwrap_or(30.0);
    let mut vc = video_codec_for(&caps.encoders, "high", "mp4", Some(pps));
    if let Some(i) = vc.args.iter().position(|a| a == "-crf") {
        vc.args[i + 1] = "18".into();
    }
    let inp = input.to_string_lossy().into_owned();
    let (mut notes, mut warnings): (Vec<String>, Vec<String>) = (vec![], vc.note.clone().into_iter().collect());
    let (pass1, vf) = if caps.vidstab() {
        let (shaky, smooth) = match strength {
            "light" => (4, 10),
            "strong" => (8, 40),
            _ => (6, 20),
        };
        let mut a1 = s(&["-i", &inp, "-map", "0:v:0", "-an", "-sn", "-vf"]);
        a1.push(format!("vidstabdetect=stepsize=6:shakiness={shaky}:accuracy=15:result={}", filter_path(trf)));
        a1.extend(s(&["-f", "null", "-"]));
        notes.push(format!("vidstab 两遍防抖（平滑 {smooth} 帧）"));
        (
            Some(a1),
            format!("vidstabtransform=input={}:smoothing={smooth}:optzoom=1:interpol=bicubic,unsharp=5:5:0.8:3:3:0.4,format={}", filter_path(trf), vc.pix),
        )
    } else if caps.has_filter("deshake") {
        warnings.push("当前的 ffmpeg 没有 vidstab，改用 deshake，效果较弱。在“设置 → 组件”里安装完整版 ffmpeg 可以使用 vidstab。".into());
        notes.push("deshake 防抖".into());
        (None, format!("deshake=rx=32:ry=32:edge=mirror,format={}", vc.pix))
    } else {
        return Err(AppError::invalid("当前的 ffmpeg 没有防抖滤镜（vidstab / deshake）。请在“设置 → 组件”里安装完整版 ffmpeg。"));
    };
    let mut pass2 = s(&["-i", &inp, "-map", "0:v:0", "-map", "0:a:0?", "-vf", &vf]);
    pass2.extend(vc.args);
    pass2.extend(s(&["-c:a", "copy"]));
    Ok(StabPlan { pass1, pass2, ext: vc.ext, notes, warnings })
}

pub async fn run_stabilize(ctx: &JobCtx, job: &ToolJob, strength: &str) -> AppResult<PathBuf> {
    let st = &ctx.st;
    let ffmpeg = find_ffmpeg(st).ok_or_else(crate::postprocess::ffmpeg_missing)?;
    let caps = st.vcaps.get(&ffmpeg).await;
    let input = PathBuf::from(job.inputs.first().ok_or_else(|| AppError::invalid("请先选择文件。"))?);
    let dir = job.output_dir.as_deref().filter(|d| !d.trim().is_empty()).map(Path::new);
    ctx.note("正在检查文件…");
    let facts = facts::read(&ffmpeg, &input).await.ok_or_else(|| AppError::msg("无法读取这个文件。"))?;
    if facts.video.is_none() {
        return Err(AppError::invalid("这个文件里没有视频画面。"));
    }
    ctx.check()?;

    let tmp = tmp_dir(ctx);
    std::fs::create_dir_all(&tmp)?;
    let trf = tmp.join("stab.trf");
    let plan = stabilize_plan(&input, strength, &trf, &caps, &facts)?;
    let out = out_path(&input, dir, "防抖", plan.ext);
    let result: AppResult<()> = async {
        let mut from = 0.0;
        if let Some(a1) = &plan.pass1 {
            ctx.note("防抖：第 1 遍，分析抖动…");
            exec(ctx, &ffmpeg, a1, facts.duration_ms, 0.0, 40.0, None).await?;
            if !nonempty(&trf) {
                return Err(AppError::msg("没有分析出抖动数据，画面可能几乎静止或太短。"));
            }
            from = 40.0;
        }
        ctx.note(join_notes(&plan.notes, &plan.warnings));
        let mut a2 = plan.pass2.clone();
        a2.extend(muxer_args(plan.ext, &out));
        exec(ctx, &ffmpeg, &a2, facts.duration_ms, from, 100.0 - from, Some(&out)).await
    }
    .await;
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    if !nonempty(&out) {
        let _ = std::fs::remove_file(&out);
        return Err(AppError::msg("没有生成任何输出。"));
    }
    Ok(out)
}
