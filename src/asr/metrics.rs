//! 运行时推理指标，仅用于诊断，不参与用户配置。

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InferenceSnapshot {
    pub audio_duration: Duration,
    pub inference_duration: Duration,
    pub blocks: u64,
    pub dropped_blocks: u64,
    pub endpoints: u64,
}

impl InferenceSnapshot {
    pub fn real_time_factor(self) -> f64 {
        let audio_seconds = self.audio_duration.as_secs_f64();
        if audio_seconds == 0.0 {
            0.0
        } else {
            self.inference_duration.as_secs_f64() / audio_seconds
        }
    }
}

#[derive(Debug)]
pub struct InferenceMetrics {
    window: Duration,
    window_started: Instant,
    audio_duration: Duration,
    inference_duration: Duration,
    blocks: u64,
    dropped_blocks: u64,
    endpoints: u64,
}

impl InferenceMetrics {
    pub fn new(window: Duration, now: Instant) -> Self {
        Self {
            window,
            window_started: now,
            audio_duration: Duration::ZERO,
            inference_duration: Duration::ZERO,
            blocks: 0,
            dropped_blocks: 0,
            endpoints: 0,
        }
    }

    pub fn record(
        &mut self,
        now: Instant,
        audio_duration: Duration,
        inference_duration: Duration,
        endpoint: bool,
    ) -> Option<InferenceSnapshot> {
        self.audio_duration += audio_duration;
        self.inference_duration += inference_duration;
        self.blocks += 1;
        self.endpoints += u64::from(endpoint);

        if now.duration_since(self.window_started) < self.window {
            return None;
        }

        let snapshot = InferenceSnapshot {
            audio_duration: self.audio_duration,
            inference_duration: self.inference_duration,
            blocks: self.blocks,
            dropped_blocks: self.dropped_blocks,
            endpoints: self.endpoints,
        };
        self.window_started = now;
        self.audio_duration = Duration::ZERO;
        self.inference_duration = Duration::ZERO;
        self.blocks = 0;
        self.dropped_blocks = 0;
        self.endpoints = 0;
        Some(snapshot)
    }

    pub fn record_drops(&mut self, dropped_blocks: u64) {
        self.dropped_blocks += dropped_blocks;
    }
}
