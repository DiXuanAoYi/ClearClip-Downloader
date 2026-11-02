//! 本机 AI 模型：人像抠图、人声分离。用纯 Rust 的 tract 跑 ONNX，不带任何动态库；
//! 模型文件不随程序发布，按需下载（地址可改）或从本机导入，放在数据目录的 `models/` 下。

pub mod matte;
pub mod onnx;
pub mod store;
pub mod vocals;
pub mod whisper_cli;

/// 数据目录（便携版会随文件夹搬家）；设置里存的是绝对路径，搬家后失效，所以要在这里重新找。
pub static DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
