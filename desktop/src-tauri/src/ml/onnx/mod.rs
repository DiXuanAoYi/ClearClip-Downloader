//! 用 tract 跑 ONNX 模型：读文件、定输入形状、跑一次、取输出。
//!
//! Windows ARM64 上 tract 的汇编内核编不过，那个平台用下面的占位实现（加载时给出明确提示）。

#[cfg(not(all(windows, target_arch = "aarch64")))]
mod real;
#[cfg(not(all(windows, target_arch = "aarch64")))]
pub use real::*;

#[cfg(all(windows, target_arch = "aarch64"))]
mod unsupported {
    use std::path::Path;

    use crate::error::{AppError, AppResult};

    pub struct Net;

    fn no() -> AppError {
        AppError::msg("Windows ARM64 版暂不支持 AI 抠像和人声分离（推理库在这个平台上无法编译）。请在 x64 版或 macOS / Linux 上使用。")
    }

    pub fn declared_shape(_: &Path) -> AppResult<Vec<Option<usize>>> {
        Err(no())
    }

    impl Net {
        pub fn load(_: &Path, _: &[usize]) -> AppResult<Net> {
            Err(no())
        }

        pub fn run(&self, _: &[usize], _: Vec<f32>) -> AppResult<(Vec<usize>, Vec<f32>)> {
            Err(no())
        }
    }
}
#[cfg(all(windows, target_arch = "aarch64"))]
pub use unsupported::*;
