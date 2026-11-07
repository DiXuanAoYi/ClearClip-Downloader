//! 增强识字组件：PaddleOCR（PP-OCRv4 的 ONNX 版）。检测文字行（DB）→ 逐行识别（CTC）。
//! 在本机用 tract 跑，不上传任何图片。模型按需下载（见 `ocr_install`），也可以自己导入。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::livecore::{self, OLine};
use crate::ml::onnx::Net;
use crate::AppState;

type St<'a> = State<'a, Arc<AppState>>;

pub const DET_FILE: &str = "ocr_det.onnx";
pub const REC_FILE: &str = "ocr_rec.onnx";
pub const DICT_FILE: &str = "ocr_dict.txt";
pub const EVT: &str = "ocr://download";

/// 默认下载地址：RapidOCR 发布的 PP-OCRv4 中文（含英文、数字）模型包，加上 PaddleOCR 的字典。
pub const MODEL_ZIP_URL: &str = "https://github.com/RapidAI/RapidOCR/releases/download/v1.1.0/required_for_whl_v1.3.0.zip";
pub const DICT_URL: &str = "https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/release/2.7/ppocr/utils/ppocr_keys_v1.txt";
const ZIP_DET: &str = "ch_PP-OCRv4_det_infer.onnx";
const ZIP_REC: &str = "ch_PP-OCRv4_rec_infer.onnx";

pub fn dir_of(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join("ocr")
}

#[derive(Debug, Clone)]
pub struct Files {
    pub det: PathBuf,
    pub rec: PathBuf,
    pub dict: PathBuf,
}

pub fn find_files(data_dir: &Path) -> Option<Files> {
    let d = dir_of(data_dir);
    let f = Files { det: d.join(DET_FILE), rec: d.join(REC_FILE), dict: d.join(DICT_FILE) };
    (f.det.is_file() && f.rec.is_file() && f.dict.is_file()).then_some(f)
}

// ---------- 纯算法 ----------

fn round32(v: f32) -> usize {
    (((v / 32.0).round() as usize).max(1)) * 32
}

/// 检测网络的输入大小：长边不超过 `limit`，两边都取 32 的倍数。
pub fn det_size(w: usize, h: usize, limit: usize) -> (usize, usize) {
    let k = (limit as f32 / w.max(h).max(1) as f32).min(1.0);
    (round32(w as f32 * k), round32(h as f32 * k))
}

/// BGRA → 归一化后的 CHW（RGB 顺序）。
pub fn to_chw(bgra: &[u8], w: usize, h: usize, mean: [f32; 3], std: [f32; 3]) -> Vec<f32> {
    let mut out = vec![0f32; 3 * w * h];
    for i in 0..w * h {
        let (b, g, r) = (bgra[i * 4] as f32, bgra[i * 4 + 1] as f32, bgra[i * 4 + 2] as f32);
        out[i] = (r / 255.0 - mean[0]) / std[0];
        out[w * h + i] = (g / 255.0 - mean[1]) / std[1];
        out[2 * w * h + i] = (b / 255.0 - mean[2]) / std[2];
    }
    out
}

/// 从概率图里找文字块：二值化 → 连通域 → 外接框（再按 DB 的办法向外扩）。返回 [x, y, w, h]（概率图坐标）。
pub fn db_boxes(prob: &[f32], w: usize, h: usize, thresh: f32, box_thresh: f32, unclip: f32) -> Vec<[f32; 4]> {
    let mut seen = vec![false; w * h];
    let mut out = vec![];
    let mut stack: Vec<usize> = vec![];
    for start in 0..w * h {
        if seen[start] || prob[start] <= thresh {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
        let (mut sum, mut n) = (0f32, 0usize);
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            sum += prob[i];
            n += 1;
            let mut push = |j: usize| {
                if !seen[j] && prob[j] > thresh {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        if n < 6 || bw.min(bh) < 3.0 || sum / (n as f32) < box_thresh {
            continue;
        }
        // DB 的二值图比真实文字块小一圈：向外扩 面积 × unclip / 周长
        let d = bw * bh * unclip / (2.0 * (bw + bh));
        let (ex0, ey0) = ((x0 as f32 - d).max(0.0), (y0 as f32 - d).max(0.0));
        let (ex1, ey1) = ((x1 as f32 + 1.0 + d).min(w as f32), (y1 as f32 + 1.0 + d).min(h as f32));
        out.push([ex0, ey0, ex1 - ex0, ey1 - ey0]);
    }
    out
}

/// CTC 贪心解码。`data` 是 [t, c] 的概率（或 logits），第 0 类是空白，最后一类是空格。
pub fn ctc_decode(data: &[f32], t: usize, c: usize, dict: &[String]) -> (String, f32) {
    let (mut text, mut conf, mut n) = (String::new(), 0f32, 0usize);
    let mut last = usize::MAX;
    for step in 0..t {
        let row = &data[step * c..(step + 1) * c];
        let (idx, best) = row.iter().copied().enumerate().fold((0usize, f32::MIN), |a, (i, v)| if v > a.1 { (i, v) } else { a });
        if idx != 0 && idx != last {
            if idx - 1 < dict.len() {
                text.push_str(&dict[idx - 1]);
            } else {
                text.push(' ');
            }
            conf += best;
            n += 1;
        }
        last = idx;
    }
    (text.trim().to_string(), if n == 0 { 0.0 } else { conf / n as f32 })
}

const REC_BUCKETS: [usize; 8] = [128, 256, 384, 512, 768, 1024, 1536, 2048];

/// 识别网络的输入宽：高固定 48，宽按比例，再向上取到固定的几档（每档加载一次网络）。
pub fn rec_width(w: usize, h: usize) -> (usize, usize) {
    let scaled = ((48.0 * w as f32 / h.max(1) as f32).ceil() as usize).max(16);
    let bucket = REC_BUCKETS.iter().copied().find(|b| *b >= scaled).unwrap_or(*REC_BUCKETS.last().unwrap_or(&2048));
    (scaled.min(bucket), bucket)
}

/// 竖排的一列拆成一个个方格（每格一个字），识别完再从上到下接起来。
pub fn split_vertical(x: f32, y: f32, w: f32, h: f32) -> Vec<[f32; 4]> {
    let n = ((h / w.max(1.0)).round() as usize).clamp(1, 60);
    let ch = h / n as f32;
    (0..n).map(|i| [x, y + ch * i as f32, w, ch]).collect()
}

// ---------- 引擎 ----------

pub struct Engine {
    files: Files,
    dict: Vec<String>,
    det: HashMap<(usize, usize), Net>,
    rec: HashMap<usize, Net>,
}

static ENGINE: Mutex<Option<(PathBuf, Engine)>> = Mutex::new(None);

impl Engine {
    pub fn load(files: &Files) -> AppResult<Engine> {
        let dict: Vec<String> = std::fs::read_to_string(&files.dict)
            .map_err(|e| AppError::msg(format!("读不了识字字典：{e}")))?
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        if dict.len() < 100 {
            return Err(AppError::msg("识字字典不完整，请重新下载增强识字组件。"));
        }
        Ok(Engine { files: files.clone(), dict, det: HashMap::new(), rec: HashMap::new() })
    }

    fn det_net(&mut self, nh: usize, nw: usize) -> AppResult<&Net> {
        if !self.det.contains_key(&(nh, nw)) {
            if self.det.len() > 6 {
                self.det.clear();
            }
            self.det.insert((nh, nw), Net::load(&self.files.det, &[1, 3, nh, nw])?);
        }
        Ok(&self.det[&(nh, nw)])
    }

    fn rec_net(&mut self, bucket: usize) -> AppResult<&Net> {
        if !self.rec.contains_key(&bucket) {
            self.rec.insert(bucket, Net::load(&self.files.rec, &[1, 3, 48, bucket])?);
        }
        Ok(&self.rec[&bucket])
    }

    /// 识别一块 BGRA 图，返回文字行（像素坐标）。
    pub fn recognize(&mut self, bgra: &[u8], w: usize, h: usize) -> AppResult<Vec<OLine>> {
        let (nw, nh) = det_size(w, h, 960);
        let small = livecore::resize_bgra(bgra, w, h, nw, nh);
        let input = to_chw(&small, nw, nh, [0.485, 0.456, 0.406], [0.229, 0.224, 0.225]);
        let (shape, prob) = self.det_net(nh, nw)?.run(&[1, 3, nh, nw], input)?;
        let (ph, pw) = match shape.as_slice() {
            [.., a, b] => (*a, *b),
            _ => return Err(AppError::msg("检测模型的输出形状不对")),
        };
        if prob.len() < ph * pw {
            return Err(AppError::msg("检测模型的输出不完整"));
        }
        let (kx, ky) = (w as f32 / pw as f32, h as f32 / ph as f32);
        let mut boxes: Vec<[f32; 4]> =
            db_boxes(&prob[..ph * pw], pw, ph, 0.3, 0.5, 1.6).into_iter().map(|b| [b[0] * kx, b[1] * ky, b[2] * kx, b[3] * ky]).collect();
        boxes.sort_by(|a, b| a[1].total_cmp(&b[1]).then(a[0].total_cmp(&b[0])));
        boxes.truncate(120);
        let mut out = vec![];
        for b in boxes {
            let vertical = b[3] > b[2] * 1.6;
            let cells = if vertical { split_vertical(b[0], b[1], b[2], b[3]) } else { vec![b] };
            let mut text = String::new();
            for c in cells {
                let (cw, ch) = (c[2].round().max(1.0) as usize, c[3].round().max(1.0) as usize);
                let crop = livecore::crop_bgra(bgra, w, h, c[0].round().max(0.0) as usize, c[1].round().max(0.0) as usize, cw, ch);
                let (cw, ch) = (cw.min(w), ch.min(h));
                if crop.len() < cw * ch * 4 || cw == 0 || ch == 0 {
                    continue;
                }
                let (rw, bucket) = rec_width(cw, ch);
                let img = livecore::resize_bgra(&crop, cw, ch, rw, 48);
                let mut chw = to_chw(&img, rw, 48, [0.5; 3], [0.5; 3]);
                if rw < bucket {
                    // 右边补零，补到这一档的宽度（每个通道各补一遍）
                    let mut padded = vec![0f32; 3 * 48 * bucket];
                    for ch_i in 0..3 {
                        for y in 0..48 {
                            padded[(ch_i * 48 + y) * bucket..(ch_i * 48 + y) * bucket + rw]
                                .copy_from_slice(&chw[(ch_i * 48 + y) * rw..(ch_i * 48 + y + 1) * rw]);
                        }
                    }
                    chw = padded;
                }
                let (shape, data) = self.rec_net(bucket)?.run(&[1, 3, 48, bucket], chw)?;
                let (t, classes) = match shape.as_slice() {
                    [_, t, c] => (*t, *c),
                    _ => continue,
                };
                // 只解码有内容的那部分时间步（补零部分会解出空白）
                let used = ((t * rw).div_ceil(bucket)).clamp(1, t);
                let (s, conf) = ctc_decode(&data[..used * classes], used, classes, &self.dict);
                if conf >= 0.35 {
                    text.push_str(&s);
                }
            }
            let text = text.trim().to_string();
            if !text.is_empty() {
                // 识字用扩过的框（识得准）；报给排版的是收紧到字迹的框（字号、分段才对得上，和 Windows 自带识字一致）
                let t = if vertical { b } else { livecore::tighten_to_ink(bgra, w, h, b) };
                out.push(OLine { text, x: t[0], y: t[1], w: t[2], h: t[3], vertical });
            }
        }
        Ok(out)
    }
}

/// 用已安装的组件识别一张图（引擎在内存里缓存，路径变了才重新装载）。
pub fn recognize(data_dir: &Path, bgra: &[u8], w: usize, h: usize) -> AppResult<Vec<OLine>> {
    let files = find_files(data_dir)
        .ok_or_else(|| AppError::new(crate::error::ErrorKind::NeedUpdate, "还没有安装“增强识字组件”。到“屏幕翻译 → 识字”里下载或导入。"))?;
    let mut g = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().map_or(true, |(p, _)| *p != files.det) {
        *g = Some((files.det.clone(), Engine::load(&files)?));
    }
    match g.as_mut() {
        Some((_, e)) => e.recognize(bgra, w, h),
        None => Err(AppError::msg("识字引擎没有装载")),
    }
}

// ---------- 安装 / 状态 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrStatus {
    pub installed: bool,
    pub dir: String,
    pub size: u64,
    pub supported: bool,
}

pub fn status_of(data_dir: &Path) -> OcrStatus {
    let d = dir_of(data_dir);
    let size = [DET_FILE, REC_FILE, DICT_FILE].iter().filter_map(|f| std::fs::metadata(d.join(f)).ok()).map(|m| m.len()).sum();
    OcrStatus {
        installed: find_files(data_dir).is_some(),
        dir: d.to_string_lossy().into_owned(),
        size,
        supported: cfg!(not(all(windows, target_arch = "aarch64"))),
    }
}

#[tauri::command]
pub fn ocr_status(state: St<'_>) -> OcrStatus {
    status_of(&state.data_dir)
}

#[tauri::command]
pub fn ocr_remove(state: St<'_>) -> OcrStatus {
    let d = dir_of(&state.data_dir);
    for f in [DET_FILE, REC_FILE, DICT_FILE] {
        let _ = std::fs::remove_file(d.join(f));
    }
    *ENGINE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    status_of(&state.data_dir)
}

/// 从模型包（zip）里取出检测和识别两个 ONNX。
pub fn extract_models(zip_bytes: &[u8], dest: &Path) -> AppResult<()> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| AppError::msg(format!("模型包打不开：{e}")))?;
    let mut got = 0;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| AppError::msg(e.to_string()))?;
        let name = f.name().rsplit('/').next().unwrap_or("").to_string();
        let target = match name.as_str() {
            ZIP_DET => DET_FILE,
            ZIP_REC => REC_FILE,
            _ => continue,
        };
        let part = dest.join(format!("{target}.part"));
        let mut out = std::fs::File::create(&part)?;
        std::io::copy(&mut f, &mut out)?;
        drop(out);
        std::fs::rename(&part, dest.join(target))?;
        got += 1;
    }
    if got == 2 {
        Ok(())
    } else {
        Err(AppError::msg("模型包里没有找到检测和识别模型。"))
    }
}

async fn fetch(state: &AppState, app: &AppHandle, url: &str, what: &str) -> AppResult<Vec<u8>> {
    use futures_util::StreamExt;
    let resp = state.dl_client.get(url).send().await.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
    if !resp.status().is_success() {
        return Err(AppError::from_status(resp.status().as_u16(), what));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut stream = resp.bytes_stream();
    let (mut buf, mut last) = (Vec::new(), 0usize);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::classify(crate::error::describe_reqwest(&e)))?;
        buf.extend_from_slice(&chunk);
        if buf.len() - last > 512 * 1024 {
            last = buf.len();
            let _ = app.emit(EVT, serde_json::json!({ "what": what, "got": buf.len(), "total": total }));
        }
    }
    let _ = app.emit(EVT, serde_json::json!({ "what": what, "got": buf.len(), "total": buf.len() }));
    Ok(buf)
}

/// 下载增强识字组件。`zip_url` / `dict_url` 留空用默认地址。
#[tauri::command]
pub async fn ocr_install(app: AppHandle, state: St<'_>, zip_url: Option<String>, dict_url: Option<String>) -> AppResult<OcrStatus> {
    let pick = |u: Option<String>, d: &str| u.map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).unwrap_or_else(|| d.to_string());
    let dir = dir_of(&state.data_dir);
    std::fs::create_dir_all(&dir)?;
    let zip = fetch(&state, &app, &pick(zip_url, MODEL_ZIP_URL), "识字模型").await?;
    extract_models(&zip, &dir)?;
    let dict = fetch(&state, &app, &pick(dict_url, DICT_URL), "识字字典").await?;
    let text = String::from_utf8(dict).map_err(|_| AppError::msg("字典不是 UTF-8 文本"))?;
    if text.lines().count() < 1000 {
        return Err(AppError::msg("下载到的字典不对（行数太少）。"));
    }
    std::fs::write(dir.join(DICT_FILE), text)?;
    *ENGINE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(status_of(&state.data_dir))
}

/// 导入自己下载好的文件：`kind` 是 det / rec / dict。
#[tauri::command]
pub fn ocr_import(state: St<'_>, kind: String, path: String) -> AppResult<OcrStatus> {
    let target = match kind.as_str() {
        "det" => DET_FILE,
        "rec" => REC_FILE,
        "dict" => DICT_FILE,
        _ => return Err(AppError::invalid("kind 只能是 det / rec / dict")),
    };
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err(AppError::not_found("找不到这个文件"));
    }
    let dir = dir_of(&state.data_dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(&src, dir.join(target))?;
    *ENGINE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(status_of(&state.data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_multiples_of_32() {
        assert_eq!(det_size(1920, 1080, 960), (960, 544));
        assert_eq!(det_size(100, 20, 960), (96, 32));
        assert_eq!(det_size(10, 10, 960), (32, 32));
    }

    #[test]
    fn finds_boxes_in_a_probability_map() {
        let (w, h) = (60, 30);
        let mut p = vec![0f32; w * h];
        for y in 8..16 {
            for x in 10..40 {
                p[y * w + x] = 0.9;
            }
        }
        // 一个很小的噪点
        p[2 * w + 50] = 0.9;
        // 概率低的块
        for y in 20..26 {
            for x in 5..25 {
                p[y * w + x] = 0.4;
            }
        }
        let b = db_boxes(&p, w, h, 0.3, 0.6, 1.6);
        assert_eq!(b.len(), 1, "{b:?}");
        let r = b[0];
        assert!(r[0] < 10.0 && r[1] < 8.0 && r[0] + r[2] > 40.0 && r[1] + r[3] > 16.0, "{r:?}");
    }

    #[test]
    fn ctc_collapses_repeats_and_blanks() {
        let dict: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        // 类别：0 空白，1=a，2=b，3=c，4=空格
        let one_hot = |i: usize| (0..5).map(|k| if k == i { 0.9 } else { 0.025 }).collect::<Vec<f32>>();
        let seq = [1, 1, 0, 1, 2, 2, 4, 3, 0];
        let data: Vec<f32> = seq.iter().flat_map(|i| one_hot(*i)).collect();
        let (t, c) = ctc_decode(&data, seq.len(), 5, &dict);
        assert_eq!(t, "aab c");
        assert!((c - 0.9).abs() < 1e-5);
        assert_eq!(ctc_decode(&[], 0, 5, &dict).0, "");
    }

    #[test]
    fn recognition_width_buckets() {
        assert_eq!(rec_width(480, 48), (480, 512));
        assert_eq!(rec_width(10, 100), (16, 128));
        assert_eq!(rec_width(50000, 48).1, 2048);
    }

    #[test]
    fn vertical_columns_split_into_cells() {
        let c = split_vertical(10.0, 20.0, 30.0, 95.0);
        assert_eq!(c.len(), 3);
        assert!((c[2][1] + c[2][3] - 115.0).abs() < 1e-3);
        assert_eq!(split_vertical(0.0, 0.0, 30.0, 20.0).len(), 1);
    }

    #[test]
    fn chw_layout_is_rgb_planes() {
        // 一个像素 B=0 G=128 R=255
        let v = to_chw(&[0, 128, 255, 255], 1, 1, [0.0; 3], [1.0; 3]);
        assert_eq!(v.len(), 3);
        assert!((v[0] - 1.0).abs() < 1e-6 && (v[1] - 128.0 / 255.0).abs() < 1e-6 && v[2] == 0.0);
    }

    /// 端到端：需要本机模型和测试图，用环境变量指定。`OCR_E2E_DIR` 下有三个文件，`OCR_E2E_PNG` 是图。
    #[test]
    #[ignore]
    fn e2e_recognizes_real_image() {
        let dir = PathBuf::from(std::env::var("OCR_E2E_DIR").expect("OCR_E2E_DIR"));
        let png_path = std::env::var("OCR_E2E_PNG").expect("OCR_E2E_PNG");
        let files = Files { det: dir.join("ch_PP-OCRv4_det_infer.onnx"), rec: dir.join("ch_PP-OCRv4_rec_infer.onnx"), dict: dir.join("dict.txt") };
        let dec = png::Decoder::new(std::fs::File::open(png_path).unwrap());
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size()];
        let info = r.next_frame(&mut buf).unwrap();
        let (w, h) = (info.width as usize, info.height as usize);
        let ch = info.color_type.samples();
        let mut bgra = vec![255u8; w * h * 4];
        for i in 0..w * h {
            bgra[i * 4] = buf[i * ch + 2];
            bgra[i * 4 + 1] = buf[i * ch + 1];
            bgra[i * 4 + 2] = buf[i * ch];
        }
        let mut e = Engine::load(&files).unwrap();
        let t = std::time::Instant::now();
        let lines = e.recognize(&bgra, w, h).unwrap();
        eprintln!("{} lines in {:?}", lines.len(), t.elapsed());
        for l in &lines {
            eprintln!("{:?} @ {:.0},{:.0} {:.0}x{:.0}", l.text, l.x, l.y, l.w, l.h);
        }
        let all: String = lines.iter().map(|l| l.text.clone()).collect();
        assert!(all.contains("开始游戏") && all.contains("Settings") && all.contains("退出"), "{all}");
    }
}
