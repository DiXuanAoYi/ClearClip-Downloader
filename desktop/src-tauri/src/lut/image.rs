//! 图片：用 ffmpeg 把图片（或视频的一帧）读成缩小的 RGB 像素，供预览和参考图匹配用；导出时用 ffmpeg 的 `lut3d` 处理原图。
//!
//! 不额外引入图片解码库：ffmpeg 已经是必备组件，能读的格式比任何一个 Rust 库都多。

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::adjust::Rgb;
use super::Lut3;
use crate::error::{AppError, AppResult};
use crate::postprocess::run_ffmpeg_capture;

/// 8 位 RGB 的小图。
#[derive(Debug, Clone)]
pub struct Sample {
    pub width: usize,
    pub height: usize,
    pub rgb: Vec<u8>,
}

const STILL: &[&str] = &["jpg", "jpeg", "jpe", "png", "webp", "bmp", "gif", "tif", "tiff", "heic", "heif", "avif", "jxl", "ico"];

/// 按扩展名判断是不是静态图片（不是的话当作视频，可以取某个时间点的一帧）。
pub fn is_still(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| STILL.contains(&e.to_ascii_lowercase().as_str()))
}

/// 解析 ffmpeg 输出的 PPM（P6，8 位）。
pub fn parse_ppm(bytes: &[u8]) -> Result<Sample, String> {
    let mut pos = 0;
    let mut tokens: Vec<String> = Vec::with_capacity(4);
    while tokens.len() < 4 {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos < bytes.len() && bytes[pos] == b'#' {
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        let start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if start == pos {
            return Err("读到的图片数据不完整。".into());
        }
        tokens.push(String::from_utf8_lossy(&bytes[start..pos]).into_owned());
    }
    // maxval 后面正好一个空白字符，接着就是像素数据
    pos += 1;
    if tokens[0] != "P6" || tokens[3] != "255" {
        return Err("读到的图片格式不对。".into());
    }
    let (w, h): (usize, usize) = (tokens[1].parse().map_err(|_| "图片尺寸不对。")?, tokens[2].parse().map_err(|_| "图片尺寸不对。")?);
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return Err("图片尺寸不对。".into());
    }
    let data = bytes.get(pos..pos + w * h * 3).ok_or("图片数据比声明的少。")?;
    Ok(Sample { width: w, height: h, rgb: data.to_vec() })
}

/// 读图的 ffmpeg 参数：缩到 `max_side` 以内（不放大），输出 PPM 到标准输出。
pub fn decode_args(path: &Path, at_ms: u64, max_side: u32) -> Vec<String> {
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin"].iter().map(|s| s.to_string()).collect();
    if !is_still(path) && at_ms > 0 {
        a.extend(["-ss".into(), format!("{:.3}", at_ms as f64 / 1000.0)]);
    }
    a.extend(["-i".into(), path.to_string_lossy().into_owned(), "-frames:v".into(), "1".into()]);
    a.extend([
        "-vf".into(),
        format!("scale='min({max_side},iw)':'min({max_side},ih)':force_original_aspect_ratio=decrease,format=rgb24"),
        "-f".into(),
        "image2pipe".into(),
        "-c:v".into(),
        "ppm".into(),
        "pipe:1".into(),
    ]);
    a
}

pub async fn load(ffmpeg: &Path, path: &Path, at_ms: u64, max_side: u32) -> AppResult<Sample> {
    if !path.is_file() {
        return Err(AppError::not_found("找不到这个图片文件。"));
    }
    let (out, _) = run_ffmpeg_capture(ffmpeg, &decode_args(path, at_ms, max_side), Duration::from_secs(60))
        .await
        .map_err(|e| AppError::invalid(format!("无法读取这张图片：{e}")))?;
    if out.is_empty() {
        return Err(AppError::invalid(if is_still(path) {
            "无法读取这张图片。"
        } else {
            "没有取到画面，可能是时间点超出了视频长度。"
        }));
    }
    parse_ppm(&out).map_err(AppError::invalid)
}

type Slot = Mutex<Option<(String, Arc<Sample>)>>;
/// 示例图片的缓存（预览时每动一下滑块都要用，不能每次都重新解码）。
static SAMPLE: Slot = Mutex::new(None);
/// 参考图的缓存。
static REFERENCE: Slot = Mutex::new(None);

#[derive(Clone, Copy)]
pub enum Which {
    Sample,
    Reference,
}

/// 读图，带一格缓存：同一个文件（路径、修改时间、取帧时间、尺寸都一样）直接用上次的。
pub async fn load_cached(which: Which, ffmpeg: &Path, path: &Path, at_ms: u64, max_side: u32) -> AppResult<Arc<Sample>> {
    let slot = match which {
        Which::Sample => &SAMPLE,
        Which::Reference => &REFERENCE,
    };
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let key = format!("{}|{at_ms}|{mtime}|{max_side}", path.display());
    if let Some((k, s)) = slot.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == key {
            return Ok(s.clone());
        }
    }
    let s = Arc::new(load(ffmpeg, path, at_ms, max_side).await?);
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, s.clone()));
    Ok(s)
}

impl Sample {
    pub fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.width * self.height * 4);
        for p in self.rgb.chunks_exact(3) {
            out.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        out
    }

    /// 取出像素（0–1），最多 `max` 个。
    pub fn pixels(&self, max: usize) -> Vec<Rgb> {
        let all: Vec<Rgb> = self.rgb.chunks_exact(3).map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
        super::matching::thin(&all, max)
    }

    /// 套上一张 LUT，得到 RGBA 像素。
    pub fn render_rgba(&self, lut: &Lut3) -> Vec<u8> {
        let mut out = vec![255u8; self.width * self.height * 4];
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
        let rows_per = self.height.div_ceil(threads).max(1);
        std::thread::scope(|s| {
            for (ti, chunk) in out.chunks_mut(rows_per * self.width * 4).enumerate() {
                let src = &self.rgb[ti * rows_per * self.width * 3..];
                s.spawn(move || {
                    for (i, px) in chunk.chunks_exact_mut(4).enumerate() {
                        let c = [src[i * 3] as f32 / 255.0, src[i * 3 + 1] as f32 / 255.0, src[i * 3 + 2] as f32 / 255.0];
                        let o = lut.sample(c);
                        for k in 0..3 {
                            px[k] = (o[k] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                        }
                    }
                });
            }
        });
        out
    }
}

/// 用 ffmpeg 的 `lut3d` 把一张 `.cube` 套在原图（或视频的一帧）上，按 `dest` 的扩展名写出 png / jpg / webp 等。
pub async fn export(ffmpeg: &Path, path: &Path, at_ms: u64, cube: &Path, dest: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(AppError::not_found("找不到这个图片文件。"));
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y"].iter().map(|s| s.to_string()).collect();
    if !is_still(path) && at_ms > 0 {
        a.extend(["-ss".into(), format!("{:.3}", at_ms as f64 / 1000.0)]);
    }
    a.extend(["-i".into(), path.to_string_lossy().into_owned(), "-frames:v".into(), "1".into()]);
    a.extend(["-vf".into(), format!("lut3d=file={}:interp=tetrahedral", crate::vidnorm::build::filter_path(cube))]);
    let ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        a.extend(["-q:v".into(), "2".into()]);
    }
    a.extend(["-update".into(), "1".into(), dest.to_string_lossy().into_owned()]);
    run_ffmpeg_capture(ffmpeg, &a, Duration::from_secs(120)).await.map_err(|e| AppError::invalid(format!("导出图片失败：{e}")))?;
    if !std::fs::metadata(dest).map(|m| m.len() > 0).unwrap_or(false) {
        return Err(AppError::invalid("没有生成图片，可能是时间点超出了视频长度。"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lut::adjust::Adjust;
    use crate::lut::{MatchInput, Prepared};
    use std::path::PathBuf;

    fn ffmpeg_bin() -> Option<PathBuf> {
        std::process::Command::new("ffmpeg").arg("-version").output().ok().filter(|o| o.status.success()).map(|_| PathBuf::from("ffmpeg"))
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "clearclip-lut-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn run(ff: &Path, args: &[&str]) {
        let out = std::process::Command::new(ff).args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn ppm_parsing() {
        let mut data = b"P6\n2 1\n255\n".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        let s = parse_ppm(&data).unwrap();
        assert_eq!((s.width, s.height, s.rgb.clone()), (2, 1, vec![1, 2, 3, 4, 5, 6]));
        // 带注释、像素数据以空白字节开头
        let mut c = b"P6\n# hello\n1 1\n255\n".to_vec();
        c.extend_from_slice(&[10, 20, 30]);
        assert_eq!(parse_ppm(&c).unwrap().rgb, vec![10, 20, 30]);
        let mut ws = b"P6\n1 1\n255\n".to_vec();
        ws.extend_from_slice(&[32, 10, 13]);
        assert_eq!(parse_ppm(&ws).unwrap().rgb, vec![32, 10, 13]);
        for bad in [&b""[..], b"P5\n1 1\n255\nabc", b"P6\n1 1\n65535\nabcdef", b"P6\n2 2\n255\nabc", b"P6\n0 1\n255\n", b"hello"] {
            assert!(parse_ppm(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn still_images_are_recognised_by_extension() {
        assert!(is_still(Path::new("/a/b.JPG")) && is_still(Path::new("x.png")) && is_still(Path::new("x.heic")));
        assert!(!is_still(Path::new("x.mp4")) && !is_still(Path::new("noext")));
        // 只有视频才带 -ss
        assert!(decode_args(Path::new("a.mp4"), 1500, 720).contains(&"1.500".to_string()));
        assert!(!decode_args(Path::new("a.png"), 1500, 720).contains(&"-ss".to_string()));
        assert!(!decode_args(Path::new("a.mp4"), 0, 720).contains(&"-ss".to_string()));
    }

    #[test]
    fn render_applies_the_lut_per_pixel_and_keeps_layout() {
        let s = Sample { width: 3, height: 2, rgb: (0..18).map(|i| (i * 14) as u8).collect() };
        let id = s.render_rgba(&Lut3::identity(17));
        assert_eq!(id.len(), 24);
        for (a, b) in s.rgba().iter().zip(&id) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
        // 反相 LUT
        let mut inv = Lut3::identity(9);
        for v in &mut inv.data {
            *v = v.map(|c| 1.0 - c);
        }
        let o = s.render_rgba(&inv);
        assert!((o[0] as i32 - (255 - s.rgb[0] as i32)).abs() <= 1 && o[3] == 255);
        assert!((o[20] as i32 - (255 - s.rgb[15] as i32)).abs() <= 1, "最后一行的第一个像素");
        // 行数比线程数少也不出错
        let tiny = Sample { width: 1, height: 1, rgb: vec![9, 8, 7] };
        assert_eq!(tiny.render_rgba(&Lut3::identity(2)).len(), 4);
        assert_eq!(s.pixels(4).len(), 4);
    }

    #[tokio::test]
    async fn decodes_images_and_video_frames_with_real_ffmpeg() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("decode");
        let png = dir.join("in.png");
        run(&ff, &["-f", "lavfi", "-i", "testsrc2=size=320x180:rate=1", "-frames:v", "1", png.to_str().unwrap()]);
        let s = load(&ff, &png, 0, 720).await.unwrap();
        assert_eq!((s.width, s.height), (320, 180), "不放大");
        assert!(s.rgb.iter().any(|v| *v > 200) && s.rgb.iter().any(|v| *v < 50));
        // 缩小：最长边不超过上限，比例不变
        let small = load(&ff, &png, 0, 100).await.unwrap();
        assert_eq!(small.width, 100);
        assert!((small.height as i32 - 56).abs() <= 1, "{}", small.height);
        // 竖图
        let tall = dir.join("tall.png");
        run(&ff, &["-f", "lavfi", "-i", "testsrc2=size=90x160:rate=1", "-frames:v", "1", tall.to_str().unwrap()]);
        let t = load(&ff, &tall, 0, 80).await.unwrap();
        assert_eq!(t.height, 80);
        // 视频的某一帧：用 mpeg4 编码（精简版 ffmpeg 也有）
        let mp4 = dir.join("v.mp4");
        run(&ff, &["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=10:duration=3", "-c:v", "mpeg4", "-q:v", "3", mp4.to_str().unwrap()]);
        let f0 = load(&ff, &mp4, 0, 720).await.unwrap();
        let f2 = load(&ff, &mp4, 2000, 720).await.unwrap();
        assert_eq!((f0.width, f0.height), (160, 90));
        assert_ne!(f0.rgb, f2.rgb, "不同时间点取到不同的画面");
        // 超出时长、文件不存在、不是图片
        assert!(load(&ff, &mp4, 60_000, 720).await.is_err());
        assert!(load(&ff, &dir.join("nope.png"), 0, 720).await.is_err());
        let txt = dir.join("a.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(load(&ff, &txt, 0, 720).await.is_err());
        // 缓存：同一个文件拿到同一个对象；换了取帧时间就换
        let a = load_cached(Which::Sample, &ff, &png, 0, 720).await.unwrap();
        let b = load_cached(Which::Sample, &ff, &png, 0, 720).await.unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = load_cached(Which::Sample, &ff, &png, 0, 100).await.unwrap();
        assert!(!Arc::ptr_eq(&a, &c) && c.width == 100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 参考图匹配整条链路：真实图片读入 → 拟合 → 烘焙 → 套回示例图片，结果的平均颜色要比原图更接近参考图，
    /// 同时没有出现反相、色带（相邻灰阶不单调）这类问题。
    #[tokio::test]
    async fn reference_matching_moves_a_real_photo_towards_the_reference() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("match");
        let grad = |name: &str, c0: &str, c1: &str| {
            let p = dir.join(name);
            let src = format!("gradients=size=320x180:c0={c0}:c1={c1}:x0=0:y0=0:x1=320:y1=180:speed=0.00001:nb_colors=2:rate=1");
            run(&ff, &["-f", "lavfi", "-i", &src, "-frames:v", "1", p.to_str().unwrap()]);
            p
        };
        // 示例图片偏冷偏暗，参考图偏暖偏亮
        let sample = load(&ff, &grad("sample.png", "0x203848", "0x70a0a8"), 0, 720).await.unwrap();
        let reference = load(&ff, &grad("ref.png", "0x402818", "0xf8c080"), 0, 720).await.unwrap();
        let (sp, rp) = (sample.pixels(20_000), reference.pixels(20_000));
        let mean = |v: &[Rgb]| -> Rgb {
            let mut m = [0.0f32; 3];
            for p in v {
                for k in 0..3 {
                    m[k] += p[k] / v.len() as f32;
                }
            }
            m
        };
        let dist = |a: Rgb, b: Rgb| -> f32 { (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt() };

        let matched = Prepared::new("match", vec![], Adjust::default(), Some(MatchInput { sample: &sp, reference: &rp, tone: 1.0, color: 1.0 }));
        let lut = matched.bake(33);
        let out: Vec<Rgb> = sample.render_rgba(&lut).chunks_exact(4).map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
        let (m_in, m_out, m_ref) = (mean(&sp), mean(&out), mean(&rp));
        assert!(dist(m_out, m_ref) < dist(m_in, m_ref) * 0.5, "平均颜色应该明显靠近参考图：原 {m_in:?} → {m_out:?}，参考 {m_ref:?}");
        assert!(m_out[0] > m_out[2], "参考图偏暖，结果也应偏暖：{m_out:?}");

        // 灰阶单调：没有反相，也没有一段平的“断层”
        let mut prev = [-1.0f32; 3];
        for i in 0..=64 {
            let v = i as f32 / 64.0;
            let o = lut.sample([v, v, v]);
            let luma = 0.2126 * o[0] + 0.7152 * o[1] + 0.0722 * o[2];
            assert!(luma + 1e-3 >= prev[1], "灰阶在 {v} 处反相：{luma} < {}", prev[1]);
            prev = [o[0], luma, o[2]];
        }

        // 强度为 0 的两项就是原样；.cube 写出再读回来仍然是同一张表
        let off = Prepared::new("off", vec![], Adjust::default(), Some(MatchInput { sample: &sp, reference: &rp, tone: 0.0, color: 0.0 }));
        let o = off.map([0.3, 0.5, 0.7]);
        assert!((0..3).all(|k| (o[k] - [0.3, 0.5, 0.7][k]).abs() < 0.02), "强度 0 应近似不变：{o:?}");
        let back = Lut3::parse(&lut.to_cube()).unwrap();
        assert_eq!(back.size, lut.size);
        assert!(dist(back.sample([0.4, 0.5, 0.6]), lut.sample([0.4, 0.5, 0.6])) < 1e-3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 最重要的一条：自己烘焙、自己插值的结果，要和 ffmpeg 的 lut3d 滤镜处理同一张图的结果一致，
    /// 这样才说明 `.cube` 的写法（红变化最快）和插值方式都和 ffmpeg 对得上。
    #[tokio::test]
    async fn preview_matches_ffmpeg_lut3d_on_a_real_image() {
        let Some(ff) = ffmpeg_bin() else { return };
        let dir = temp_dir("lut3d");
        let src = dir.join("in.png");
        run(&ff, &["-f", "lavfi", "-i", "testsrc2=size=320x180:rate=1", "-frames:v", "1", src.to_str().unwrap()]);
        let sample = load(&ff, &src, 0, 720).await.unwrap();
        // 一个相当“用力”的调整：各项都动一点，包括分色调
        let adj = Adjust {
            exposure: -0.2,
            contrast: 0.3,
            temperature: 0.3,
            tint: -0.1,
            saturation: 0.2,
            highlights: -0.2,
            shadows: 0.2,
            shadow_tone: crate::lut::adjust::SplitTone { hue: 210.0, amount: 0.4 },
            ..Default::default()
        };
        let lut = Prepared::new("compare", vec![], adj, None).bake(33);
        let cube = dir.join("t.cube");
        std::fs::write(&cube, lut.to_cube()).unwrap();
        let out = dir.join("out.png");
        export(&ff, &src, 0, &cube, &out).await.unwrap();
        let theirs = load(&ff, &out, 0, 720).await.unwrap();
        assert_eq!((theirs.width, theirs.height), (sample.width, sample.height));
        let ours = sample.render_rgba(&lut);
        let (mut sum, mut worst, mut n) = (0u64, 0i32, 0u64);
        for (i, p) in theirs.rgb.chunks_exact(3).enumerate() {
            for k in 0..3 {
                let d = (p[k] as i32 - ours[i * 4 + k] as i32).abs();
                sum += d as u64;
                worst = worst.max(d);
                n += 1;
            }
        }
        let mean = sum as f64 / n as f64;
        assert!(mean < 0.8 && worst <= 4, "和 ffmpeg lut3d 的差别：平均 {mean:.3}，最大 {worst}");
        // 原图确实被改变了（不是两边都没动）
        let moved = sample.rgb.iter().zip(&theirs.rgb).filter(|(a, b)| (**a as i32 - **b as i32).abs() > 10).count();
        assert!(moved > sample.rgb.len() / 10, "调整应该明显改变画面");
        // 导出成 jpg
        let jpg = dir.join("out.jpg");
        export(&ff, &src, 0, &cube, &jpg).await.unwrap();
        assert!(std::fs::metadata(&jpg).unwrap().len() > 500);
        // 视频的一帧也能导出
        let mp4 = dir.join("v.mp4");
        run(&ff, &["-f", "lavfi", "-i", "testsrc2=size=160x90:rate=10:duration=3", "-c:v", "mpeg4", "-q:v", "3", mp4.to_str().unwrap()]);
        let frame = dir.join("frame.png");
        export(&ff, &mp4, 1500, &cube, &frame).await.unwrap();
        assert!(frame.is_file());
        // 带空格、引号、冒号的路径
        let odd = dir.join("it's a: dir");
        std::fs::create_dir_all(&odd).unwrap();
        let cube2 = odd.join("my look, v1.cube");
        std::fs::write(&cube2, lut.to_cube()).unwrap();
        export(&ff, &src, 0, &cube2, &dir.join("odd.png")).await.unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
