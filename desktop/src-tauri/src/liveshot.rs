//! 屏幕翻译“保存带译文的截图”：贴字层窗口是不进截图的（否则会被自己再识别一遍），
//! 所以系统截图、别的截图软件截下来都没有译文。这里的做法是：取一帧干净的画面，
//! 由贴字层窗口把译文叠上去合成一张 PNG，再交回来存成文件、放进剪贴板。
//! 这个文件只放不碰系统接口的部分（编码、解码、命名、校验），方便测试。

use crate::error::{AppError, AppResult};
use base64::Engine as _;

/// 合成图的大小上限（字节）。整屏 4K 的 PNG 一般在十几 MB 以内。
pub const MAX_PNG: usize = 60 * 1024 * 1024;

/// `data:image/png;base64,xxxx` 或纯 base64 → 字节，并确认是 PNG。
pub fn png_from_data(data: &str) -> AppResult<Vec<u8>> {
    let b64 = data.split_once(',').map_or(data, |(_, b)| b).trim();
    if b64.len() > MAX_PNG * 4 / 3 + 16 {
        return Err(AppError::invalid("截图太大。"));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|_| AppError::invalid("截图数据不对。"))?;
    if bytes.len() > MAX_PNG || !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err(AppError::invalid("截图数据不是 PNG。"));
    }
    Ok(bytes)
}

/// PNG 字节 → RGBA8 像素（放进剪贴板用）。只认 8 位的 RGB / RGBA / 灰度 / 灰度+透明，浏览器导出的都是这几种。
pub fn png_to_rgba(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let raw = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => raw.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => raw.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => raw.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((rgba, info.width, info.height))
}

/// 文件名：屏幕翻译-20260101-093000.png（本地时间，同一秒内再存会加序号）。
pub fn shot_name(dir: &std::path::Path, now: chrono::DateTime<chrono::Local>) -> std::path::PathBuf {
    let base = format!("屏幕翻译-{}", now.format("%Y%m%d-%H%M%S"));
    let first = dir.join(format!("{base}.png"));
    if !first.exists() {
        return first;
    }
    (2..1000).map(|n| dir.join(format!("{base}-{n}.png"))).find(|p| !p.exists()).unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn png(color: png::ColorType, w: u32, h: u32, data: &[u8]) -> Vec<u8> {
        let mut out = vec![];
        {
            let mut e = png::Encoder::new(&mut out, w, h);
            e.set_color(color);
            e.set_depth(png::BitDepth::Eight);
            let mut wr = e.write_header().unwrap();
            wr.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn data_url_must_be_a_png() {
        let p = png(png::ColorType::Rgb, 1, 1, &[1, 2, 3]);
        let b64 = base64::engine::general_purpose::STANDARD.encode(&p);
        assert_eq!(png_from_data(&format!("data:image/png;base64,{b64}")).unwrap(), p);
        assert_eq!(png_from_data(&b64).unwrap(), p);
        let jpg = base64::engine::general_purpose::STANDARD.encode([0xff, 0xd8, 0xff, 0xe0]);
        assert!(png_from_data(&jpg).is_err());
        assert!(png_from_data("data:image/png;base64,@@@@").is_err());
    }

    #[test]
    fn rgba_comes_out_for_every_common_png_layout() {
        let rgb = png_to_rgba(&png(png::ColorType::Rgb, 2, 1, &[10, 20, 30, 40, 50, 60])).unwrap();
        assert_eq!(rgb, (vec![10, 20, 30, 255, 40, 50, 60, 255], 2, 1));
        let rgba = png_to_rgba(&png(png::ColorType::Rgba, 1, 1, &[1, 2, 3, 4])).unwrap();
        assert_eq!(rgba, (vec![1, 2, 3, 4], 1, 1));
        let gray = png_to_rgba(&png(png::ColorType::Grayscale, 1, 2, &[7, 9])).unwrap();
        assert_eq!(gray.0, vec![7, 7, 7, 255, 9, 9, 9, 255]);
        assert!(png_to_rgba(b"not a png").is_none());
    }

    #[test]
    fn names_do_not_overwrite_each_other() {
        let dir = std::env::temp_dir().join(format!("cc-liveshot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = chrono::Local.with_ymd_and_hms(2026, 1, 2, 9, 30, 5).unwrap();
        let a = shot_name(&dir, t);
        assert_eq!(a.file_name().unwrap().to_string_lossy(), "屏幕翻译-20260102-093005.png");
        std::fs::write(&a, b"x").unwrap();
        let b = shot_name(&dir, t);
        assert_eq!(b.file_name().unwrap().to_string_lossy(), "屏幕翻译-20260102-093005-2.png");
        std::fs::remove_dir_all(&dir).ok();
    }
}
