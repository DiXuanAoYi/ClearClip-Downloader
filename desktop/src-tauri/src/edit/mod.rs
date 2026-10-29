//! 剪辑：多段素材（视频、图片）在时间线上排布，叠加轨放画中画 / 贴纸 / GIF，加转场、文字、配乐，导出成一个视频。
//!
//! - [`spec`]：工程（片段、叠加轨、文字、音频轨、输出规格）和时间线排布计算
//! - [`fonts`]：给文字找默认字体
//! - [`build`]：工程 → ffmpeg 参数（纯函数）
//! - [`region`]：区域效果（跟着物体走的马赛克 / 模糊 / 局部调色 / 聚焦）的遮罩和路径
//! - [`job`]：后台任务（导出、生成预览）和界面用的命令

pub mod build;
pub mod cmds;
pub mod fonts;
pub mod job;
pub mod region;
pub mod spec;

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod overlay_e2e_tests;
#[cfg(test)]
mod region_e2e_tests;
