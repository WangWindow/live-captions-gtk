//! 音频模块
//!
//! - [`source`]：解析麦克风和系统音频的 GStreamer source
//! - [`gstreamer`]：通过 appsink 获取标准化音频样本

mod block;
mod gstreamer;
mod source;

pub use block::AudioBlockAssembler;
pub use gstreamer::{standard_audio_caps, GstreamerCapture};
pub use source::AudioSource;
