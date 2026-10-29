//! 下载引擎：HTTP（单线程 / 分段并行）、m3u8 等具体传输实现。任务调度在 `download.rs`。

pub mod hls;
pub mod http;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;

pub const CTRL_RUN: u8 = 0;
pub const CTRL_PAUSE: u8 = 1;
pub const CTRL_CANCEL: u8 = 2;

/// 分段下载中的一段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start: u64,
    /// 包含该字节
    pub end: u64,
    /// 已下载的字节数
    pub done: u64,
}

impl Segment {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    pub fn is_empty(&self) -> bool {
        self.end < self.start
    }

    pub fn is_complete(&self) -> bool {
        self.done >= self.len()
    }
}

/// 续传所需的服务器信息与分段进度。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResumeMeta {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub total: Option<u64>,
    /// 服务器是否支持 Range；None 表示还不知道
    pub resumable: Option<bool>,
    /// 分段下载的各段进度；为空表示单线程下载
    pub segments: Vec<Segment>,
}

impl ResumeMeta {
    pub fn segmented_received(&self) -> u64 {
        self.segments.iter().map(|s| s.done.min(s.len())).sum()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DlError {
    #[error("已暂停")]
    Paused,
    #[error("已取消")]
    Canceled,
    #[error("服务器返回 {0}")]
    Status(u16),
    #[error("网络错误：{0}")]
    Net(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Disk(String),
    #[error("{0}")]
    Encrypted(String),
    #[error("{0}")]
    Other(String),
}

impl DlError {
    pub fn kind(&self) -> crate::error::ErrorKind {
        use crate::error::ErrorKind;
        match self {
            DlError::Status(code) => crate::error::AppError::from_status(*code, "").kind,
            DlError::Net(_) => ErrorKind::Network,
            DlError::Io(_) | DlError::Disk(_) => ErrorKind::Disk,
            DlError::Encrypted(_) => ErrorKind::Encrypted,
            _ => ErrorKind::Other,
        }
    }

    pub fn from_ctrl(c: u8) -> DlError {
        if c == CTRL_PAUSE {
            DlError::Paused
        } else {
            DlError::Canceled
        }
    }
}

/// 等待控制信号变为暂停或取消，返回该信号。
pub async fn wait_ctrl(rx: &mut watch::Receiver<u8>) -> u8 {
    loop {
        let v = *rx.borrow_and_update();
        if v != CTRL_RUN {
            return v;
        }
        if rx.changed().await.is_err() {
            // 发送端已释放（任务被移除），视为取消
            return CTRL_CANCEL;
        }
    }
}

/// 检查目录所在磁盘的剩余空间是否足够。
pub fn ensure_space(dir: &std::path::Path, need: u64, reserve: u64) -> Result<(), DlError> {
    let probe = std::iter::successors(Some(dir), |d| d.parent()).find(|d| d.exists());
    let Some(probe) = probe else { return Ok(()) };
    match fs4::available_space(probe) {
        Ok(free) if free < need.saturating_add(reserve) => Err(DlError::Disk(format!(
            "磁盘空间不足：需要约 {:.1} MB，剩余 {:.1} MB（另需保留 {:.0} MB）",
            need as f64 / 1048576.0,
            free as f64 / 1048576.0,
            reserve as f64 / 1048576.0
        ))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_math() {
        let s = Segment { start: 10, end: 19, done: 4 };
        assert_eq!(s.len(), 10);
        assert!(!s.is_complete());
        let m = ResumeMeta { segments: vec![s, Segment { start: 20, end: 29, done: 10 }], ..Default::default() };
        assert_eq!(m.segmented_received(), 14);
    }

    #[test]
    fn space_check() {
        let dir = std::env::temp_dir();
        assert!(ensure_space(&dir, 1, 0).is_ok());
        assert!(matches!(ensure_space(&dir, u64::MAX / 2, 0), Err(DlError::Disk(_))));
        // 不存在的子目录按最近的已存在父目录检查
        assert!(ensure_space(&dir.join("no/such/dir"), 1, 0).is_ok());
    }
}
