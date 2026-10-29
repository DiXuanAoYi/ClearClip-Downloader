//! 电源相关：下载期间阻止系统休眠；全部完成后睡眠或关机。

use std::process::Stdio;
use std::sync::Mutex;

/// 当前的阻止休眠状态。
static INHIBITOR: Mutex<Option<Inhibitor>> = Mutex::new(None);

enum Inhibitor {
    /// macOS `caffeinate`、Linux `systemd-inhibit` 子进程，结束进程即解除
    #[allow(dead_code)]
    Child(std::process::Child),
    /// Windows：持有 SetThreadExecutionState 的线程，发送消息后退出
    #[allow(dead_code)]
    Thread(std::sync::mpsc::Sender<()>),
}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        match self {
            Inhibitor::Child(c) => {
                let _ = c.kill();
                let _ = c.wait();
            }
            Inhibitor::Thread(tx) => {
                let _ = tx.send(());
            }
        }
    }
}

/// 设置是否阻止休眠（重复调用无副作用）。
pub fn keep_awake(on: bool) {
    let mut guard = INHIBITOR.lock().unwrap_or_else(|e| e.into_inner());
    if on == guard.is_some() {
        return;
    }
    if !on {
        *guard = None;
        log::info!("sleep inhibitor released");
        return;
    }
    *guard = start();
    if guard.is_some() {
        log::info!("sleep inhibitor acquired");
    }
}

#[cfg(target_os = "windows")]
fn start() -> Option<Inhibitor> {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        // 状态与线程绑定：线程存活期间有效，退出前恢复
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        let _ = rx.recv();
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    });
    Some(Inhibitor::Thread(tx))
}

#[cfg(target_os = "macos")]
fn start() -> Option<Inhibitor> {
    let pid = std::process::id().to_string();
    std::process::Command::new("caffeinate")
        .args(["-i", "-w", &pid])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
        .map(Inhibitor::Child)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn start() -> Option<Inhibitor> {
    std::process::Command::new("systemd-inhibit")
        .args(["--what=sleep:idle", "--who=ClearClip", "--why=正在下载", "--mode=block", "sleep", "infinity"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
        .map(Inhibitor::Child)
}

/// 睡眠或关机的系统命令。
pub fn power_command(action: &str) -> Option<(&'static str, Vec<&'static str>)> {
    let cmd = match (std::env::consts::OS, action) {
        ("windows", "sleep") => ("rundll32.exe", vec!["powrprof.dll,SetSuspendState", "0,1,0"]),
        ("windows", "shutdown") => ("shutdown", vec!["/s", "/t", "30"]),
        ("macos", "sleep") => ("pmset", vec!["sleepnow"]),
        ("macos", "shutdown") => ("osascript", vec!["-e", "tell application \"System Events\" to shut down"]),
        (_, "sleep") => ("systemctl", vec!["suspend"]),
        (_, "shutdown") => ("systemctl", vec!["poweroff"]),
        _ => return None,
    };
    Some(cmd)
}

pub fn run_power_action(action: &str) {
    let Some((bin, args)) = power_command(action) else { return };
    log::info!("running power action: {action}");
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if let Err(e) = cmd.spawn() {
        log::warn!("power action {action} failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_exist() {
        assert!(power_command("sleep").is_some());
        assert!(power_command("shutdown").is_some());
        assert!(power_command("none").is_none());
    }

    #[test]
    fn toggling_is_idempotent() {
        keep_awake(false);
        keep_awake(false);
        assert!(INHIBITOR.lock().unwrap().is_none());
    }
}
