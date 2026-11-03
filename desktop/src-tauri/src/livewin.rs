//! 实时屏幕翻译用到的 Windows 系统接口：抓一块屏幕、Windows 自带的文字识别（Windows.Media.Ocr）、枚举窗口。
//! 这个文件不依赖 tauri，其他系统上只留下返回“不支持”的空壳。

use crate::livecore::OLine;

/// 一帧画面：BGRA，自上而下，alpha 已置为 255。
pub struct Frame {
    pub bgra: Vec<u8>,
    pub w: usize,
    pub h: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinInfo {
    pub hwnd: isize,
    pub title: String,
    pub exe: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLang {
    pub tag: String,
    pub name: String,
}

#[cfg(not(windows))]
pub use other::*;
#[cfg(windows)]
pub use win::*;

#[cfg(not(windows))]
mod other {
    use super::*;
    pub const SUPPORTED: bool = false;
    pub fn grab_rect(_: i32, _: i32, _: u32, _: u32) -> Result<Frame, String> {
        Err("实时屏幕翻译目前只支持 Windows".into())
    }
    pub fn list_windows() -> Vec<WinInfo> {
        vec![]
    }
    pub fn window_bounds(_: isize) -> Option<(i32, i32, i32, i32)> {
        None
    }
    pub fn ocr_languages() -> Vec<OcrLang> {
        vec![]
    }
    pub fn ocr(_: &[u8], _: usize, _: usize, _: &str) -> Result<(Vec<OLine>, String), String> {
        Err("Windows 自带的文字识别只在 Windows 上可用".into())
    }
    pub fn init_thread() {}
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub const SUPPORTED: bool = true;

    // ---------- 抓屏 ----------

    /// 抓虚拟屏幕上的一块（物理像素坐标）。
    pub fn grab_rect(x: i32, y: i32, w: u32, h: u32) -> Result<Frame, String> {
        use std::ptr::null_mut;
        use windows_sys::Win32::Graphics::Gdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
        };
        if w == 0 || h == 0 || w > 16384 || h > 16384 {
            return Err("取景区域大小不对".into());
        }
        // SAFETY: 全是对本进程创建的 GDI 对象的调用，出口处统一释放。
        unsafe {
            let screen = GetDC(null_mut());
            if screen.is_null() {
                return Err("无法访问屏幕".into());
            }
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w as i32, h as i32);
            let mut result = Err("截屏失败".to_string());
            if !mem.is_null() && !bmp.is_null() {
                let old = SelectObject(mem, bmp);
                if BitBlt(mem, 0, 0, w as i32, h as i32, screen, x, y, SRCCOPY | CAPTUREBLT) != 0 {
                    let mut info: BITMAPINFO = std::mem::zeroed();
                    info.bmiHeader = BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w as i32,
                        biHeight: -(h as i32),
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
                    if GetDIBits(mem, bmp, 0, h, buf.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS) != 0 {
                        for px in buf.chunks_exact_mut(4) {
                            px[3] = 255;
                        }
                        result = Ok(Frame { bgra: buf, w: w as usize, h: h as usize });
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

    // ---------- 窗口 ----------

    fn bounds_of(h: windows_sys::Win32::Foundation::HWND) -> Option<(i32, i32, i32, i32)> {
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: r 是有效的输出缓冲。优先取不含阴影的可见边框，失败再用 GetWindowRect。
        let ok = unsafe {
            DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS as u32, (&mut r as *mut RECT).cast(), std::mem::size_of::<RECT>() as u32) == 0
                || GetWindowRect(h, &mut r) != 0
        };
        (ok && r.right > r.left && r.bottom > r.top).then_some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }

    /// 某个窗口现在的位置；窗口没了或最小化了就是 None。
    pub fn window_bounds(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow, IsWindowVisible};
        let h = hwnd as windows_sys::Win32::Foundation::HWND;
        // SAFETY: 只读查询；句柄失效时这些函数返回 0。
        unsafe {
            if IsWindow(h) == 0 || IsWindowVisible(h) == 0 || IsIconic(h) != 0 {
                return None;
            }
        }
        bounds_of(h)
    }

    pub fn list_windows() -> Vec<WinInfo> {
        use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible};

        unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
            let out = &mut *(lp as *mut Vec<WinInfo>);
            if IsWindowVisible(h) == 0 || IsIconic(h) != 0 {
                return 1;
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(h, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4) == 0 && cloaked != 0 {
                return 1;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == GetCurrentProcessId() {
                return 1;
            }
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
            if n <= 0 {
                return 1;
            }
            let Some((x, y, w, hh)) = bounds_of(h) else { return 1 };
            if w < 120 || hh < 80 {
                return 1;
            }
            let exe = crate::writeassist::exe_of_hwnd(h as isize);
            out.push(WinInfo { hwnd: h as isize, title: String::from_utf16_lossy(&buf[..n as usize]), exe, x, y, w, h: hh });
            1
        }

        let mut out: Vec<WinInfo> = vec![];
        // SAFETY: 回调只在本次调用期间运行，lp 指向上面的 out。
        unsafe {
            EnumWindows(Some(each), (&mut out as *mut Vec<WinInfo>) as LPARAM);
        }
        out
    }

    // ---------- Windows 自带文字识别 ----------

    thread_local! {
        static ENGINES: RefCell<HashMap<String, windows::Media::Ocr::OcrEngine>> = RefCell::new(HashMap::new());
    }

    /// 识别线程开始时调用一次（WinRT 需要先初始化）。
    pub fn init_thread() {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
        // SAFETY: 每个线程初始化一次；已经初始化过（返回 S_FALSE 或模式冲突）都无妨。
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
    }

    /// 系统里装了识别包的语言。
    pub fn ocr_languages() -> Vec<OcrLang> {
        use windows::Media::Ocr::OcrEngine;
        let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else { return vec![] };
        list.into_iter().filter_map(|l| Some(OcrLang { tag: l.LanguageTag().ok()?.to_string(), name: l.DisplayName().ok()?.to_string() })).collect()
    }

    fn engine_for(tag: &str) -> Result<windows::Media::Ocr::OcrEngine, String> {
        use windows::core::HSTRING;
        use windows::Globalization::Language;
        use windows::Media::Ocr::OcrEngine;
        ENGINES.with(|m| {
            if let Some(e) = m.borrow().get(tag) {
                return Ok(e.clone());
            }
            let e = if tag.is_empty() {
                OcrEngine::TryCreateFromUserProfileLanguages()
            } else {
                Language::CreateLanguage(&HSTRING::from(tag)).and_then(|l| OcrEngine::TryCreateFromLanguage(&l))
            }
            .map_err(|_| {
                if tag.is_empty() {
                    "Windows 没有可用的文字识别语言包。到“设置 → 时间和语言 → 语言和区域”给语言装上“光学字符识别”。".to_string()
                } else {
                    format!("没有安装 {tag} 的文字识别语言包。到“设置 → 时间和语言 → 语言和区域”，给这个语言装上“光学字符识别”。")
                }
            })?;
            m.borrow_mut().insert(tag.to_string(), e.clone());
            Ok(e)
        })
    }

    fn werr(what: &str, er: windows::core::Error) -> String {
        format!("文字识别失败（{what}）：{er}")
    }

    fn run(engine: &windows::Media::Ocr::OcrEngine, bgra: &[u8], w: usize, h: usize) -> Result<Vec<OLine>, String> {
        use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::DataWriter;
        let max = OcrEngine::MaxImageDimension().map_err(|x| werr("读上限", x))? as usize;
        if w > max || h > max {
            return Err(format!("取景区域太大（上限 {max} 像素），请缩小区域。"));
        }
        let writer = DataWriter::new().map_err(|x| werr("写缓冲", x))?;
        writer.WriteBytes(bgra).map_err(|x| werr("写缓冲", x))?;
        let buf = writer.DetachBuffer().map_err(|x| werr("缓冲", x))?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, w as i32, h as i32).map_err(|x| werr("位图", x))?;
        let res = engine.RecognizeAsync(&bmp).map_err(|x| werr("识别", x))?.join().map_err(|x| werr("识别", x))?;
        let mut out = vec![];
        for line in res.Lines().map_err(|x| werr("取行", x))? {
            let text = line.Text().map(|t| t.to_string()).unwrap_or_default();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            if let Ok(words) = line.Words() {
                for wd in words {
                    if let Ok(r) = wd.BoundingRect() {
                        x0 = x0.min(r.X);
                        y0 = y0.min(r.Y);
                        x1 = x1.max(r.X + r.Width);
                        y1 = y1.max(r.Y + r.Height);
                    }
                }
            }
            if x1 > x0 && y1 > y0 && !text.trim().is_empty() {
                out.push(OLine::new(&text, x0, y0, x1 - x0, y1 - y0));
            }
        }
        Ok(out)
    }

    /// 识别一张图。`lang`：语言前缀（`ja`、`en`、`zh`、`ko`）或具体标签；`auto` 时每种已装语言都试一遍，取识别出文字最多的。
    /// 返回识别结果和用到的语言标签（下次可以直接指定，省得再试一遍）。
    pub fn ocr(bgra: &[u8], w: usize, h: usize, lang: &str) -> Result<(Vec<OLine>, String), String> {
        let lang = lang.trim();
        let installed = ocr_languages();
        if lang.is_empty() || lang == "auto" {
            if installed.is_empty() {
                return engine_for("").and_then(|e| run(&e, bgra, w, h)).map(|l| (l, String::new()));
            }
            let mut best: Option<(usize, Vec<OLine>, String)> = None;
            for l in installed.iter().take(6) {
                if let Ok(lines) = engine_for(&l.tag).and_then(|e| run(&e, bgra, w, h)) {
                    let score: usize = lines.iter().map(|x| x.text.chars().filter(|c| c.is_alphanumeric()).count()).sum();
                    if best.as_ref().map_or(true, |b| score > b.0) {
                        best = Some((score, lines, l.tag.clone()));
                    }
                }
            }
            return best.map(|(_, l, t)| (l, t)).ok_or_else(|| "文字识别没有可用的语言。".to_string());
        }
        let prefix = lang.to_lowercase();
        let tag = installed
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case(lang))
            .or_else(|| installed.iter().find(|l| l.tag.to_lowercase().starts_with(&prefix)))
            .map(|l| l.tag.clone())
            .ok_or_else(|| format!("没有安装“{lang}”的文字识别语言包。到“设置 → 时间和语言 → 语言和区域”，给这个语言装上“光学字符识别”；或者在“屏幕翻译”里换成增强识字 / 识字接口。"))?;
        engine_for(&tag).and_then(|e| run(&e, bgra, w, h)).map(|l| (l, tag))
    }
}
