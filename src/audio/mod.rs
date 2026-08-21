//! 音频模块
//!
//! - [`device`]：音频设备探测（麦克风 / 系统音频 monitor）
//! - [`capture`]：从 cpal 设备捕获原始音频

mod block;
mod capture;
mod device;
mod gstreamer;
mod source;

pub use block::AudioBlockAssembler;
pub use capture::{process_chunk, AudioCapture};
pub use device::{resolve, AudioSource};
pub use gstreamer::{standard_audio_caps, GstreamerCapture};
