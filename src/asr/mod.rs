//! 语音识别（ASR）模块
//!
//! 封装 Whisper.cpp 的 GGML 模型推理，提供分块音频转录能力。

mod engine;
mod metrics;
mod policy;
mod punctuation;

pub use engine::TranscriptionEngine;
pub use metrics::{InferenceMetrics, InferenceSnapshot};
pub use policy::InferencePolicy;
pub use punctuation::PunctuationScheduler;
