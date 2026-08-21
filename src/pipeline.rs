//! 音频→ASR→UI 流水线控制器

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::asr::TranscriptionEngine;
use crate::audio::{AudioBlockAssembler, AudioSource, GstreamerCapture};
use crate::presets::SettingsHandle;

const POLL_INTERVAL: Duration = Duration::from_millis(40);
const CHUNK_SECS: f64 = 0.2;
const AUDIO_QUEUE_CAPACITY: usize = 4;

/// 流水线状态消息
pub enum PipelineMsg {
    Loading,
    Ready,
    Text(String),
    Error(String),
}

pub struct PipelineController {
    stop_flag: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl PipelineController {
    pub fn start(settings: SettingsHandle, microphone: bool) -> (Receiver<PipelineMsg>, Self) {
        let (sender, receiver) = mpsc::channel::<PipelineMsg>();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let worker = std::thread::Builder::new()
            .name("asr-pipeline".into())
            .spawn(move || {
                run_pipeline(settings, sender, stop_clone, microphone);
            })
            .expect("无法启动 ASR 工作线程");

        (
            receiver,
            Self {
                stop_flag,
                worker: Some(worker),
            },
        )
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for PipelineController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_pipeline(
    settings: SettingsHandle,
    sender: Sender<PipelineMsg>,
    stop_flag: Arc<AtomicBool>,
    microphone: bool,
) {
    // ---- 1. 模型目录 ----
    let model_dir = {
        let s = settings.read().unwrap_or_else(|e| e.into_inner());
        let p = std::path::Path::new(&s.model_path);
        if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().unwrap_or(p).to_path_buf()
        }
    };

    let _ = sender.send(PipelineMsg::Loading);

    // ---- 2. 加载引擎（由模型定义驱动） ----
    let model_info = crate::presets::find_model_by_dir(&model_dir)
        .filter(|info| info.category == crate::presets::ModelCategory::Asr);
    let mut engine = match model_info {
        Some(info) => match TranscriptionEngine::from_model(info, &model_dir) {
            Ok(e) => e,
            Err(e) => {
                let _ = sender.send(PipelineMsg::Error(format!("模型加载失败: {e}")));
                return;
            }
        },
        None => {
            let _ = sender.send(PipelineMsg::Error(
                "未知模型类型，请在设置中选择有效模型".into(),
            ));
            return;
        }
    };

    // ---- 3. 标点模型 ----
    let punctuator = load_punctuator(&settings);

    // ---- 4. 启动 GStreamer 音频捕获 ----
    let source = if microphone {
        AudioSource::Microphone
    } else {
        AudioSource::SystemAudio
    };
    let capture = match GstreamerCapture::start(source) {
        Ok(capture) => capture,
        Err(error) => {
            let _ = sender.send(PipelineMsg::Error(format!("音频捕获失败: {error}")));
            return;
        }
    };

    let sample_rate = capture.audio_info().rate();
    let _ = sender.send(PipelineMsg::Ready);

    // ---- 5. 分离采集和识别线程 ----
    let (audio_sender, audio_receiver) = mpsc::sync_channel(AUDIO_QUEUE_CAPACITY);
    let dropped_blocks = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let capture_stop = stop_flag.clone();
    let capture_drops = dropped_blocks.clone();
    let capture_sender = sender.clone();
    let capture_worker = match std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            run_capture(
                capture,
                sample_rate,
                audio_sender,
                capture_sender,
                capture_stop,
                capture_drops,
            );
        }) {
        Ok(worker) => worker,
        Err(error) => {
            let _ = sender.send(PipelineMsg::Error(format!("无法启动音频采集线程: {error}")));
            stop_flag.store(true, Ordering::Relaxed);
            engine.finish();
            return;
        }
    };

    run_recognition(
        &mut engine,
        audio_receiver,
        &punctuator,
        sample_rate,
        &sender,
        &stop_flag,
        &dropped_blocks,
    );
    stop_flag.store(true, Ordering::Relaxed);
    let _ = capture_worker.join();
    engine.finish();
}

fn run_capture(
    mut capture: GstreamerCapture,
    sample_rate: u32,
    sender: SyncSender<Vec<f32>>,
    pipeline_sender: Sender<PipelineMsg>,
    stop_flag: Arc<AtomicBool>,
    dropped_blocks: Arc<std::sync::atomic::AtomicU64>,
) {
    let chunk = (sample_rate as f64 * CHUNK_SECS) as usize;
    let mut blocks = AudioBlockAssembler::new(chunk.max(1));

    while !stop_flag.load(Ordering::Relaxed) {
        let dropped_buffers = capture.take_dropped_buffers();
        if dropped_buffers > 0 {
            dropped_blocks.fetch_add(dropped_buffers, Ordering::Release);
        }

        let sample = match capture.try_pull_sample(POLL_INTERVAL) {
            Ok(Some(sample)) => sample,
            Ok(None) => continue,
            Err(error) => {
                let _ = pipeline_sender.send(PipelineMsg::Error(format!(
                    "GStreamer 音频读取失败: {error}"
                )));
                stop_flag.store(true, Ordering::Relaxed);
                break;
            }
        };
        let samples = match GstreamerCapture::samples_from_sample(&sample, capture.audio_info()) {
            Ok(samples) => samples,
            Err(error) => {
                let _ = pipeline_sender.send(PipelineMsg::Error(format!(
                    "GStreamer 音频格式错误: {error}"
                )));
                stop_flag.store(true, Ordering::Relaxed);
                break;
            }
        };
        blocks.push(&samples);

        while let Some(block) = blocks.take_block() {
            match sender.try_send(block) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) => {
                    dropped_blocks.fetch_add(1, Ordering::Release);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    stop_flag.store(true, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    let _ = capture.stop();
}

fn run_recognition(
    engine: &mut TranscriptionEngine,
    audio_receiver: Receiver<Vec<f32>>,
    punctuator: &Option<sherpa_onnx::OfflinePunctuation>,
    sample_rate: u32,
    sender: &Sender<PipelineMsg>,
    stop_flag: &Arc<AtomicBool>,
    dropped_blocks: &std::sync::atomic::AtomicU64,
) {
    let mut last_text = String::new();
    let mut observed_drops = 0;

    while !stop_flag.load(Ordering::Relaxed) {
        let drops = dropped_blocks.load(Ordering::Acquire);
        if drops != observed_drops {
            observed_drops = drops;
            eprintln!("音频队列丢弃了 {drops} 个 buffer，重置识别流");
            engine.reset_stream();
            last_text.clear();
            while audio_receiver.try_recv().is_ok() {}
            continue;
        }

        let samples = match audio_receiver.recv_timeout(POLL_INTERVAL) {
            Ok(samples) => samples,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !stop_flag.load(Ordering::Relaxed) {
                    let _ = sender.send(PipelineMsg::Error("音频采集线程已退出".into()));
                }
                break;
            }
        };

        let drops = dropped_blocks.load(Ordering::Acquire);
        if drops != observed_drops {
            observed_drops = drops;
            engine.reset_stream();
            last_text.clear();
            while audio_receiver.try_recv().is_ok() {}
            continue;
        }

        match engine.transcribe(&samples, sample_rate) {
            Ok(text) => {
                let mut trimmed = text.trim().to_string();
                if let Some(punct) = punctuator
                    && !trimmed.is_empty()
                {
                    trimmed = punct.add_punctuation(&trimmed).unwrap_or(trimmed);
                    // 移除标点模型自动附加的结尾标点
                    while trimmed.ends_with(['。', '，', '、', '！', '？', '.', ',', '!', '?'])
                    {
                        trimmed.pop();
                    }
                }
                if !trimmed.is_empty() && trimmed != last_text {
                    last_text = trimmed.clone();
                    let _ = sender.send(PipelineMsg::Text(trimmed));
                }
                if engine.is_endpoint() {
                    engine.reset_stream();
                    last_text.clear();
                }
            }
            Err(e) => {
                let _ = sender.send(PipelineMsg::Error(format!("转录错误: {e}")));
            }
        }
    }
}

/// 尝试加载标点恢复模型（可选，不存在则返回 None）
fn load_punctuator(settings: &SettingsHandle) -> Option<sherpa_onnx::OfflinePunctuation> {
    let s = settings.read().unwrap_or_else(|e| e.into_inner());
    if !s.auto_punctuation {
        return None;
    }
    let path = s.punct_model_path.trim();
    if path.is_empty() {
        return None;
    }
    let p = std::path::Path::new(path);
    let model_file = if p.is_dir() {
        p.join("model.int8.onnx")
    } else {
        p.to_path_buf()
    };
    if !model_file.exists() {
        return None;
    }
    let config = sherpa_onnx::OfflinePunctuationConfig {
        model: sherpa_onnx::OfflinePunctuationModelConfig {
            ct_transformer: Some(model_file.to_string_lossy().into_owned()),
            ..Default::default()
        },
    };
    sherpa_onnx::OfflinePunctuation::create(&config)
}
