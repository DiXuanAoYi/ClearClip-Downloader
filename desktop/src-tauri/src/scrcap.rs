//! 整屏截图（所有显示器拼成的虚拟屏幕）。Windows 用 GDI；其他系统暂不支持。

/// 一张截图：PNG 字节，加上它在虚拟屏幕里的物理像素位置。
pub struct Grab {
    pub png: Vec<u8>,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// BGRA（自上而下）转 RGB，丢掉 alpha。
pub fn bgra_to_rgb(bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len() / 4 * 3);
    for px in bgra.as_chunks::<4>().0 {
        out.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    out
}

pub fn encode_png(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(rgb.len() / 3);
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(windows)]
pub fn grab_screen() -> Result<Grab, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    unsafe {
        let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        if w <= 0 || h <= 0 {
            return Err("读不到屏幕大小".into());
        }
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return Err("无法访问屏幕".into());
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let mut result = Err("截屏失败".to_string());
        if !mem.is_null() && !bmp.is_null() {
            let old = SelectObject(mem, bmp);
            // CAPTUREBLT 把分层（半透明）窗口也截进来
            if BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT) != 0 {
                let mut info: BITMAPINFO = std::mem::zeroed();
                info.bmiHeader = BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // 负数：自上而下
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                };
                let mut buf = vec![0u8; w as usize * h as usize * 4];
                let got = GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);
                if got != 0 {
                    result = encode_png(&bgra_to_rgb(&buf), w as u32, h as u32).map(|png| Grab { png, x, y, w: w as u32, h: h as u32 });
                }
            }
            SelectObject(mem, old);
        }
        if !bmp.is_null() {
            DeleteObject(bmp);
        }
        if !mem.is_null() {
            DeleteDC(mem);
        }
        ReleaseDC(null_mut(), screen);
        result
    }
}

#[cfg(not(windows))]
pub fn grab_screen() -> Result<Grab, String> {
    Err("全局截图翻译目前只支持 Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bgra_and_encodes_png() {
        let rgb = bgra_to_rgb(&[1, 2, 3, 255, 10, 20, 30, 0]);
        assert_eq!(rgb, vec![3, 2, 1, 30, 20, 10]);
        let png = encode_png(&rgb, 2, 1).unwrap();
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }
}
