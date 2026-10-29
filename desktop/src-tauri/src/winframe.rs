//! Windows：让无边框小窗（迷你窗、悬浮拖拽窗）永远不带原生标题栏。
//!
//! 窗口库在 Windows 上做“无边框”的办法，是保留窗口的标题栏样式（`WS_CAPTION` 等），再在 `WM_NCCALCSIZE` 里把非客户区缩成 0；
//! 每次显示、隐藏窗口、改尺寸可调整性、置顶时，窗口库都会重新写一遍窗口样式，把标题栏样式写回去。
//! 一旦非客户区不为 0，系统就会在窗口上方画出完整的原生标题栏（带图标和最小化 / 最大化 / 关闭按钮），而且点不动。
//!
//! 在窗口显示之后再去掉样式不可靠：窗口操作是排队到界面线程执行的，调用方这边的“去掉样式”可能先于窗口库的“写回样式”。
//! 这里改成给窗口挂一个子类过程，截获 `WM_STYLECHANGING`：任何人写窗口样式时，都把标题栏相关的样式位从新样式里去掉，
//! 这样不管谁在什么时候写样式，结果里都不会有标题栏。

/// 窗口库写样式时被去掉的样式位：标题栏、系统菜单、最大化 / 最小化按钮、可拖动的粗边框。
#[cfg(windows)]
const STRIPPED_STYLES: u32 = {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME};
    WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_THICKFRAME
};

/// 给小窗装上样式守卫，并立即去掉已有的标题栏样式。其他平台什么也不做。
///
/// 子类过程必须在窗口所属的线程（界面线程）上安装，所以放到界面线程的任务队列里做。
#[cfg(windows)]
pub fn guard(w: &tauri::WebviewWindow) {
    let Ok(hwnd) = w.hwnd() else {
        log::warn!("取不到小窗的窗口句柄，无法去掉标题栏样式");
        return;
    };
    let raw = hwnd.0 as isize;
    let label = w.label().to_string();
    let _ = w.run_on_main_thread(move || install(raw as windows_sys::Win32::Foundation::HWND, &label));
}

#[cfg(windows)]
fn install(hwnd: windows_sys::Win32::Foundation::HWND, label: &str) {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    };
    // SAFETY: `hwnd` 是本进程里刚创建的窗口的句柄，这里在它所属的界面线程上操作；
    // 子类过程是本模块里的 `extern "system"` 函数，不带引用数据。
    unsafe {
        let ok = SetWindowSubclass(hwnd, Some(guard_proc), GUARD_ID, 0);
        let before = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let after = before & !STRIPPED_STYLES;
        if after != before {
            SetWindowLongW(hwnd, GWL_STYLE, after as i32);
            SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
        // 留一条记录：万一标题栏还是出现了，从日志里能看到窗口当时的样式
        log::info!(
            "小窗 {label}：样式守卫{}，样式 {before:#010x} → {:#010x}",
            if ok != 0 { "已安装" } else { "安装失败" },
            GetWindowLongW(hwnd, GWL_STYLE) as u32
        );
    }
}

#[cfg(windows)]
const GUARD_ID: usize = 0x436C_6561; // "Clea"

/// 子类过程：改窗口样式时去掉标题栏相关的位。
#[cfg(windows)]
unsafe extern "system" fn guard_proc(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wparam: usize, lparam: isize, _id: usize, _data: usize) -> isize {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_STYLE, STYLESTRUCT, WM_STYLECHANGING};
    if msg == WM_STYLECHANGING && wparam as i32 == GWL_STYLE && lparam != 0 {
        // SAFETY: 系统在 `WM_STYLECHANGING` 的 `lparam` 里传的是指向 `STYLESTRUCT` 的有效指针，允许修改其中的 `styleNew`。
        let styles = &mut *(lparam as *mut STYLESTRUCT);
        styles.styleNew &= !STRIPPED_STYLES;
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(not(windows))]
pub fn guard(_: &tauri::WebviewWindow) {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn stripped_styles_cover_the_title_bar() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WS_BORDER, WS_CAPTION, WS_DLGFRAME, WS_VISIBLE};
        assert_eq!(STRIPPED_STYLES & WS_CAPTION, WS_CAPTION);
        assert_eq!(WS_CAPTION, WS_BORDER | WS_DLGFRAME);
        assert_eq!(STRIPPED_STYLES & WS_VISIBLE, 0, "不能把“可见”样式去掉");
    }
}
