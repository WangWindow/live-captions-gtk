/// 将捕获到的采样组装为固定大小的 ASR 音频块。
#[derive(Debug)]
pub struct AudioBlockAssembler {
    block_size: usize,
    pending: Vec<f32>,
}

impl AudioBlockAssembler {
    /// 创建一个固定大小的音频块组装器。
    pub fn new(block_size: usize) -> Self {
        assert!(block_size > 0, "音频块大小必须大于零");
        Self {
            block_size,
            pending: Vec::with_capacity(block_size),
        }
    }

    /// 当前音频块还需要多少采样点。
    pub fn remaining(&self) -> usize {
        self.block_size.saturating_sub(self.pending.len())
    }

    /// 追加采样点，直到可以取出完整音频块。
    pub fn push(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
    }

    /// 取出一个完整音频块；数据不足时返回 `None`。
    pub fn take_block(&mut self) -> Option<Vec<f32>> {
        if self.pending.len() < self.block_size {
            return None;
        }
        Some(self.pending.drain(..self.block_size).collect())
    }

    /// 丢弃尚未形成完整音频块的采样点。
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}
