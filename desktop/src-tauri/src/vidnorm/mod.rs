//! 视频规整：把各种来源的视频（手机录屏、直播录制、平台下载的成片）统一成指定规格。
//!
//! - [`facts`]：从 `ffmpeg -i` 的输出读出编码、尺寸、帧率、色彩信息、旋转、HDR 类型
//! - [`spec`]：规格、预设、素材的“问题清单”和推荐预设
//! - [`build`]：规格 + 素材 + 分析结果 + ffmpeg 能力 → ffmpeg 参数（纯函数）
//! - [`hdr`]：没有 zscale 时的内置 HDR → SDR（3D LUT）
//! - [`colormatch`]：分段色彩匹配（把和整体色调不一致的镜头单独校正）
//! - [`job`]：后台任务（规整、防抖）
//! - [`analyze`]：需要解码才能知道的信息：可变帧率、黑边、响度；以及前后对比预览

pub mod analyze;
pub mod build;
pub mod colormatch;
pub mod facts;
pub mod hdr;
pub mod job;
pub mod spec;

#[cfg(test)]
mod e2e_tests;

pub use build::{build, NormPlan};
pub use facts::Facts;
pub use spec::{Analysis, NormSpec};
