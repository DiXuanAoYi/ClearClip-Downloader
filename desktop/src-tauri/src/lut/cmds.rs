//! LUT 工作室的前端命令。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::ipc::Response;
use tauri::State;

use super::adjust::{looks, Look};
use super::image::{self, Sample, Which};
use super::{Lut3, MatchInput, Prepared, Recipe};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::{postprocess, AppState};

type St<'a> = State<'a, Arc<AppState>>;

/// 预览和参考图匹配用的示例图片，最长边不超过这个值。
const PREVIEW_SIDE: u32 = 720;
/// 参考图缩到这个尺寸再取像素。
const REFERENCE_SIDE: u32 = 320;
/// 参与匹配统计的像素数上限。
const MATCH_PIXELS: usize = 40_000;
/// 预览烘焙用的格点数上限（和 65 的结果差别在 1/255 以内，但快得多）。
const PREVIEW_LUT: usize = 33;
/// 读进来的 .cube 文件大小上限。
const MAX_CUBE_BYTES: u64 = 160 * 1024 * 1024;

fn need_ffmpeg() -> AppError {
    AppError::new(ErrorKind::NeedUpdate, "需要 ffmpeg 来读取图片。请在“设置 → 组件”里安装 ffmpeg 后重试。")
}

fn file_name(p: &str) -> String {
    Path::new(p).file_name().map_or_else(|| p.to_string(), |n| n.to_string_lossy().into_owned())
}

// ---------- 读 .cube（带缓存） ----------

static LUTS: Mutex<Vec<(String, Arc<Lut3>)>> = Mutex::new(Vec::new());

/// 读一个 `.cube`。同一个文件（路径、大小、修改时间都没变）用缓存，预览时滑块一动就会反复用到。
fn load_lut(path: &str) -> AppResult<Arc<Lut3>> {
    let meta = std::fs::metadata(path).map_err(|_| AppError::not_found(format!("找不到 LUT 文件：{}", file_name(path))))?;
    if !meta.is_file() {
        return Err(AppError::invalid(format!("{} 不是文件。", file_name(path))));
    }
    if meta.len() > MAX_CUBE_BYTES {
        return Err(AppError::invalid(format!("{} 太大了（超过 {} MB），不像是正常的 LUT。", file_name(path), MAX_CUBE_BYTES / 1024 / 1024)));
    }
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let key = format!("{path}|{}|{mtime}", meta.len());
    if let Some((_, l)) = LUTS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| *k == key) {
        return Ok(l.clone());
    }
    let bytes = std::fs::read(path)?;
    let lut = Arc::new(Lut3::parse(&String::from_utf8_lossy(&bytes)).map_err(|e| AppError::invalid(format!("{}：{e}", file_name(path))))?);
    let mut cache = LUTS.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|(k, _)| !k.starts_with(&format!("{path}|")));
    cache.push((key, lut.clone()));
    if cache.len() > 8 {
        cache.remove(0);
    }
    Ok(lut)
}

// ---------- 命令 ----------

/// 一键风格。
#[tauri::command]
pub fn lut_looks() -> Vec<Look> {
    looks()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LutInfo {
    pub title: String,
    pub size: usize,
}

/// 检查一个 `.cube` 文件能不能用：返回名字和格点数。
#[tauri::command]
pub async fn lut_inspect(path: String) -> AppResult<LutInfo> {
    let lut = tokio::task::spawn_blocking(move || load_lut(&path)).await.map_err(|e| AppError::msg(e.to_string()))??;
    Ok(LutInfo { title: lut.title.clone(), size: lut.size })
}

/// 8 字节头（宽、高，小端 u32）加 RGBA 像素。前端直接画到 canvas 上。
fn frame(width: usize, height: usize, rgba: Vec<u8>) -> Response {
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&(width as u32).to_le_bytes());
    out.extend_from_slice(&(height as u32).to_le_bytes());
    out.extend_from_slice(&rgba);
    Response::new(out)
}

/// 读入示例图片（或视频 `at_ms` 处的一帧），返回缩小后的原图像素。之后的预览都用这一份缓存。
#[tauri::command]
pub async fn lut_sample(state: St<'_>, path: String, at_ms: u64) -> AppResult<Response> {
    let ff = postprocess::find_ffmpeg(&state).ok_or_else(need_ffmpeg)?;
    let s = image::load_cached(Which::Sample, &ff, Path::new(&path), at_ms, PREVIEW_SIDE).await?;
    Ok(frame(s.width, s.height, s.rgba()))
}

/// 读入配方里用到的东西，拟合参考图匹配，得到可以烘焙的 `Prepared`。示例图片只在需要时才读。
async fn prepare(state: &AppState, recipe: &Recipe, need_sample: bool) -> AppResult<(Prepared, Option<Arc<Sample>>)> {
    let ff = postprocess::find_ffmpeg(state);
    let layers_paths: Vec<(String, f32)> = recipe.base.iter().filter(|l| !l.path.trim().is_empty()).map(|l| (l.path.clone(), l.strength)).collect();
    let layers =
        tokio::task::spawn_blocking(move || -> AppResult<Vec<(Arc<Lut3>, f32)>> { layers_paths.into_iter().map(|(p, s)| Ok((load_lut(&p)?, s))).collect() })
            .await
            .map_err(|e| AppError::msg(e.to_string()))??;

    let reference = recipe.reference.as_ref().filter(|r| !r.path.trim().is_empty() && (r.tone > 0.0 || r.color > 0.0));
    let sample_ref = recipe.sample.as_ref().filter(|s| !s.path.trim().is_empty());
    let sample = match sample_ref {
        Some(s) if need_sample || reference.is_some() => {
            let ff = ff.as_deref().ok_or_else(need_ffmpeg)?;
            Some(image::load_cached(Which::Sample, ff, Path::new(&s.path), s.at_ms, PREVIEW_SIDE).await?)
        }
        _ => None,
    };
    if need_sample && sample.is_none() {
        return Err(AppError::invalid("先选一张示例图片（或视频），再预览或导出图片。"));
    }
    let reference_img = match reference {
        Some(r) => {
            if sample.is_none() {
                return Err(AppError::invalid("参考图匹配需要先选一张示例图片：要拿它和参考图比较，才能算出怎样调整。"));
            }
            let ff = ff.as_deref().ok_or_else(need_ffmpeg)?;
            Some((r.clone(), image::load_cached(Which::Reference, ff, Path::new(&r.path), 0, REFERENCE_SIDE).await?))
        }
        None => None,
    };

    let name = if recipe.name.trim().is_empty() { "ClearClip LUT".to_string() } else { recipe.name.trim().to_string() };
    let adjust = recipe.adjust;
    let sample_for_fit = sample.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        let (s_px, r_px);
        let matching = match (&sample_for_fit, &reference_img) {
            (Some(s), Some((r, img))) => {
                s_px = s.pixels(MATCH_PIXELS);
                r_px = img.pixels(MATCH_PIXELS);
                Some(MatchInput { sample: &s_px, reference: &r_px, tone: r.tone, color: r.color })
            }
            _ => None,
        };
        Prepared::new(&name, layers, adjust, matching)
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?;
    Ok((prepared, sample))
}

/// 预览：把当前配方烘焙成 LUT，套在示例图片上，返回处理后的像素（头部格式同 `lut_sample`）。
#[tauri::command]
pub async fn lut_preview(state: St<'_>, recipe: Recipe) -> AppResult<Response> {
    let (prepared, sample) = prepare(&state, &recipe, true).await?;
    let sample = sample.ok_or_else(|| AppError::invalid("先选一张示例图片。"))?;
    let size = recipe.lut_size().min(PREVIEW_LUT);
    let (w, h) = (sample.width, sample.height);
    let rgba = tokio::task::spawn_blocking(move || sample.render_rgba(&prepared.bake(size))).await.map_err(|e| AppError::msg(e.to_string()))?;
    Ok(frame(w, h, rgba))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LutSaved {
    pub path: String,
    pub size: usize,
    pub bytes: u64,
}

fn with_ext(dest: &str, default_ext: &str, allowed: &[&str]) -> AppResult<PathBuf> {
    let d = dest.trim();
    if d.is_empty() {
        return Err(AppError::invalid("没有选择保存位置。"));
    }
    let mut p = PathBuf::from(d);
    let ok = p.extension().and_then(|e| e.to_str()).is_some_and(|e| allowed.contains(&e.to_ascii_lowercase().as_str()));
    if !ok {
        let name = p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        p.set_file_name(format!("{name}.{default_ext}"));
    }
    Ok(p)
}

/// 生成 `.cube` 并保存。
#[tauri::command]
pub async fn lut_save(state: St<'_>, recipe: Recipe, dest: String) -> AppResult<LutSaved> {
    let dest = with_ext(&dest, "cube", &["cube"])?;
    let (prepared, _) = prepare(&state, &recipe, false).await?;
    let size = recipe.lut_size();
    let text = tokio::task::spawn_blocking(move || prepared.bake(size).to_cube()).await.map_err(|e| AppError::msg(e.to_string()))?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&dest, &text)?;
    Ok(LutSaved { path: dest.to_string_lossy().into_owned(), size, bytes: text.len() as u64 })
}

/// 把当前配方的效果套在示例图片（或视频的那一帧）的原图上，导出成 png / jpg / webp。
#[tauri::command]
pub async fn lut_export_image(state: St<'_>, recipe: Recipe, dest: String) -> AppResult<String> {
    let dest = with_ext(&dest, "png", &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"])?;
    let (ff, caps) = crate::vidcaps::current(&state).await.ok_or_else(need_ffmpeg)?;
    if !caps.has_filter("lut3d") {
        return Err(AppError::invalid("当前的 ffmpeg 没有 lut3d 滤镜，无法导出图片。"));
    }
    let (prepared, _) = prepare(&state, &recipe, true).await?;
    let s = recipe.sample.as_ref().ok_or_else(|| AppError::invalid("先选一张示例图片。"))?;
    let size = recipe.lut_size();
    let text = tokio::task::spawn_blocking(move || prepared.bake(size).to_cube()).await.map_err(|e| AppError::msg(e.to_string()))?;
    let tmp = state.data_dir.join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let cube = tmp.join(format!("lut-{}.cube", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
    std::fs::write(&cube, text)?;
    let r = image::export(&ff, Path::new(&s.path), s.at_ms, &cube, &dest).await;
    let _ = std::fs::remove_file(&cube);
    r?;
    Ok(dest.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_gets_the_right_extension() {
        assert_eq!(with_ext("/a/b/look", "cube", &["cube"]).unwrap(), PathBuf::from("/a/b/look.cube"));
        assert_eq!(with_ext("/a/b/look.CUBE", "cube", &["cube"]).unwrap(), PathBuf::from("/a/b/look.CUBE"));
        assert_eq!(with_ext("/a/my.look", "cube", &["cube"]).unwrap(), PathBuf::from("/a/my.look.cube"), "名字里的点不当作扩展名");
        assert_eq!(with_ext("/a/p.jpg", "png", &["png", "jpg"]).unwrap(), PathBuf::from("/a/p.jpg"));
        assert!(with_ext("  ", "cube", &["cube"]).is_err());
    }

    #[test]
    fn lut_files_are_cached_and_errors_name_the_file() {
        let dir = std::env::temp_dir().join(format!("clearclip-lutcmd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.cube");
        std::fs::write(&good, Lut3::identity(5).to_cube()).unwrap();
        let a = load_lut(good.to_str().unwrap()).unwrap();
        let b = load_lut(good.to_str().unwrap()).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "没改过的文件用缓存");
        // 改了内容（大小变了）就重新读
        std::fs::write(&good, Lut3::identity(9).to_cube()).unwrap();
        assert_eq!(load_lut(good.to_str().unwrap()).unwrap().size, 9);
        let bad = dir.join("bad.cube");
        std::fs::write(&bad, "LUT_1D_SIZE 4\n0 0 0\n").unwrap();
        let e = load_lut(bad.to_str().unwrap()).unwrap_err().to_string();
        assert!(e.contains("bad.cube") && e.contains("1D"), "{e}");
        assert!(load_lut(dir.join("missing.cube").to_str().unwrap()).unwrap_err().to_string().contains("missing.cube"));
        assert!(load_lut(dir.to_str().unwrap()).is_err(), "目录不是文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frame_has_a_header() {
        // Response 内部是不透明的，这里只确认构造不 panic；头部格式由前端测试覆盖
        let _ = frame(2, 1, vec![0; 8]);
    }
}
