//! 测试共用辅助：写“假可执行脚本”。
//!
//! 直接写完就执行会偶发 ETXTBSY（Text file busy）：别的线程恰好在 fork，子进程在 exec 之前
//! 继承了我们的写句柄。所以这里：唯一文件名 → 写完 sync_all 并关闭句柄 → 改名到最终路径 →
//! 用探测执行确认不再被占用（遇到 ETXTBSY 就重试），之后调用方可放心 spawn。
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const PROBE_ENV: &str = "CC_FAKE_BIN_PROBE";

pub fn write_fake_exe(dir: &Path, name: &str, script: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let bin = dir.join(format!("{name}-{n}"));
    let tmp = dir.join(format!(".{name}-{n}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp).unwrap();
        // 探测模式下立即退出，不跑脚本本体
        f.write_all(format!("#!/bin/sh\n[ -n \"${PROBE_ENV}\" ] && exit 0\n{script}\n").as_bytes()).unwrap();
        f.sync_all().unwrap();
    }
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&tmp, &bin).unwrap();
    for _ in 0..200 {
        match std::process::Command::new(&bin).env(PROBE_ENV, "1").status() {
            Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(std::time::Duration::from_millis(10)),
            _ => break,
        }
    }
    bin
}
