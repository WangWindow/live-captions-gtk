//! ASR 的内部自动策略。
//!
//! 这里保留一套运行时策略，不把推理参数暴露成用户需要理解的性能模式。

use std::time::Duration;

const MAX_INFERENCE_THREADS: usize = 8;
const AUDIO_BLOCK_MILLIS: u64 = 100;
const PUNCTUATION_INTERVAL_MILLIS: u64 = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InferencePolicy {
    pub num_threads: i32,
    pub decoding_method: &'static str,
    pub audio_block_duration: Duration,
    pub punctuation_interval: Duration,
}

impl InferencePolicy {
    pub fn automatic() -> Self {
        let parallelism = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1);
        Self::from_parallelism(parallelism)
    }

    pub fn from_parallelism(parallelism: usize) -> Self {
        let num_threads = parallelism
            .saturating_sub(1)
            .clamp(1, MAX_INFERENCE_THREADS);
        Self {
            num_threads: num_threads as i32,
            decoding_method: "greedy_search",
            audio_block_duration: Duration::from_millis(AUDIO_BLOCK_MILLIS),
            punctuation_interval: Duration::from_millis(PUNCTUATION_INTERVAL_MILLIS),
        }
    }

    pub fn audio_block_samples(self, sample_rate: u32) -> usize {
        let samples = sample_rate as u64 * self.audio_block_duration.as_millis() as u64 / 1_000;
        samples.max(1) as usize
    }
}
