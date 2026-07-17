//! 音频捕获模块
//!
//! 从 cpal 设备捕获音频，混合为单声道，以设备原始采样率存储。
//! sherpa-onnx 内部自动处理重采样，因此不再需要 rubato 重采样器。

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::atomic::{AtomicBool, Ordering};

/// 音频环形缓冲区最大容量（采样点数），约 30 秒 @ 48kHz
pub const RING_BUFFER_CAPACITY: usize = 48_000 * 30;

static CALLBACK_FIRED: AtomicBool = AtomicBool::new(false);

/// 音频捕获器 —— 从 cpal 设备捕获原始音频，混合为单声道 f32
pub struct AudioCapture {
    stream: cpal::Stream,
    sample_rate: u32,
    _channels: u16,
    buffer: HeapCons<f32>,
}

impl AudioCapture {
    /// 创建一个新的音频捕获会话
    ///
    /// - `device`: cpal 音频设备
    /// - `chunk_frames`: 每次回调处理的帧数
    pub fn new(device: &cpal::Device, chunk_frames: usize) -> Result<Self> {
        // 优先使用 default_input_config；对于 PipeWire sink capture，
        // 退而用 default_output_config 获取设备原生参数。
        let supported_config = device
            .default_input_config()
            .or_else(|_| device.default_output_config())
            .context("无法获取音频设备配置")?;

        let sample_rate: u32 = supported_config.sample_rate().into();
        let channels = supported_config.channels() as usize;
        let sample_format = supported_config.sample_format();
        let stream_config = supported_config.config();

        let ring = HeapRb::<f32>::new(RING_BUFFER_CAPACITY);
        let (mut producer, consumer) = ring.split();

        let err_fn = |err| eprintln!("音频流错误: {err}");

        let stream = match sample_format {
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        process_chunk(data, channels, chunk_frames, &mut producer, |s| {
                            s as f32 / 32_768.0
                        });
                    },
                    err_fn,
                    None,
                )
                .context("无法构建 i16 音频流")?,
            cpal::SampleFormat::U16 => device
                .build_input_stream(
                    stream_config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        process_chunk(data, channels, chunk_frames, &mut producer, |s| {
                            (s as f32 - 32_768.0) / 32_768.0
                        });
                    },
                    err_fn,
                    None,
                )
                .context("无法构建 u16 音频流")?,
            cpal::SampleFormat::I24 => device
                .build_input_stream(
                    stream_config,
                    move |data: &[cpal::I24], _: &cpal::InputCallbackInfo| {
                        process_chunk(data, channels, chunk_frames, &mut producer, |s| {
                            s.inner() as f32 / 8_388_608.0
                        });
                    },
                    err_fn,
                    None,
                )
                .context("无法构建 i24 音频流")?,
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        process_chunk(data, channels, chunk_frames, &mut producer, |s| s);
                    },
                    err_fn,
                    None,
                )
                .context("无法构建 f32 音频流")?,
            cpal::SampleFormat::I32 => device
                .build_input_stream(
                    stream_config,
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        process_chunk(data, channels, chunk_frames, &mut producer, |s| {
                            s as f32 / 2_147_483_648.0
                        });
                    },
                    err_fn,
                    None,
                )
                .context("无法构建 i32 音频流")?,
            _ => anyhow::bail!("不支持的采样格式: {sample_format:?}"),
        };

        stream.play().context("无法启动音频流")?;

        Ok(Self {
            stream,
            sample_rate,
            _channels: supported_config.channels(),
            buffer: consumer,
        })
    }

    /// 设备原始采样率（Hz）
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 声道数
    #[allow(dead_code)]
    pub fn channels(&self) -> u16 {
        self._channels
    }

    /// 取出缓冲区中最多 `max_samples` 个采样点
    pub fn drain(&mut self, max_samples: usize) -> Vec<f32> {
        let occupied = self.buffer.occupied_len();
        let available = occupied.min(max_samples);
        if available == 0 {
            return vec![];
        }
        let mut chunk = Vec::with_capacity(available);
        for _ in 0..available {
            if let Some(s) = self.buffer.try_pop() {
                chunk.push(s);
            } else {
                break;
            }
        }
        chunk
    }

    /// 检查缓冲区中是否有足够的数据
    #[allow(dead_code)]
    pub fn available(&self) -> usize {
        self.buffer.occupied_len()
    }

    /// 清空缓冲区
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        while self.buffer.try_pop().is_some() {}
    }

    /// 立即释放音频设备（暂停捕获流，让系统释放麦克风权限）
    pub fn release(&mut self) {
        let _ = self.stream.pause();
    }
}

/// 处理 cpal 回调数据：多声道混合为单声道后送入环形缓冲区
fn process_chunk<T: Copy>(
    data: &[T],
    channels: usize,
    chunk_frames: usize,
    producer: &mut HeapProd<f32>,
    to_f32: impl Fn(T) -> f32,
) {
    if !CALLBACK_FIRED.swap(true, Ordering::Relaxed) {
        eprintln!("音频回调已触发：{} 采样，{} 声道", data.len(), channels);
    }

    // 处理完整的帧（每个 frame 包含 channels 个采样）
    let frames = data.len() / channels;
    let process_frames = if chunk_frames > 0 {
        chunk_frames.min(frames)
    } else {
        frames
    };

    for frame_start in (0..process_frames * channels).step_by(channels) {
        let left = to_f32(data[frame_start]);
        let mono = if channels >= 2 {
            (left + to_f32(data[frame_start + 1])) * 0.5
        } else {
            left
        };
        let _ = producer.try_push(mono);
    }
}
