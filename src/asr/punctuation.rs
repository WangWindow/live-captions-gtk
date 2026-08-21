//! 标点恢复调度器。

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PunctuationScheduler {
    interval: Duration,
    last_input: String,
    last_run: Option<Instant>,
    last_run_was_endpoint: bool,
}

impl PunctuationScheduler {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_input: String::new(),
            last_run: None,
            last_run_was_endpoint: false,
        }
    }

    pub fn should_run(&self, text: &str, endpoint: bool, now: Instant) -> bool {
        if text.is_empty() {
            return false;
        }

        if text == self.last_input {
            return endpoint && !self.last_run_was_endpoint;
        }

        endpoint
            || self
                .last_run
                .is_none_or(|last_run| now.duration_since(last_run) >= self.interval)
    }

    pub fn record(&mut self, text: &str, endpoint: bool, now: Instant) {
        self.last_input.clear();
        self.last_input.push_str(text);
        self.last_run = Some(now);
        self.last_run_was_endpoint = endpoint;
    }

    pub fn reset(&mut self) {
        self.last_input.clear();
        self.last_run = None;
        self.last_run_was_endpoint = false;
    }
}
