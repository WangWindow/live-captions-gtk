//! 音频模块
//!
//! - [`device`]：音频设备探测（麦克风 / 系统音频 monitor）
//! - [`capture`]：从 cpal 设备捕获原始音频

mod capture;
mod device;

pub use capture::AudioCapture;
pub use device::{AudioSource, resolve};
