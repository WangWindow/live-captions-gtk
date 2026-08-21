//! 音频→ASR→UI 流水线控制器

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::asr::TranscriptionEngine;
use crate::audio::{AudioBlockAssembler, AudioSource, GstreamerCapture};
use crate::presets::SettingsHandle;

const POLL_INTERVAL: Duration = Duration::from_millis(40);
const CHUNK_SECS: f64 = 0.2;

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
    let mut capture = match GstreamerCapture::start(source) {
        Ok(capture) => capture,
        Err(error) => {
            let _ = sender.send(PipelineMsg::Error(format!("音频捕获失败: {error}")));
            return;
        }
    };

    let sample_rate = capture.audio_info().rate();
    let _ = sender.send(PipelineMsg::Ready);

    // ---- 5. 运行主循环 ----
    run_loop(
        &mut engine,
        &mut capture,
        &punctuator,
        sample_rate,
        &sender,
        &stop_flag,
    );
    let _ = capture.stop();
    engine.finish();
}

fn run_loop(
    engine: &mut TranscriptionEngine,
    capture: &mut GstreamerCapture,
    punctuator: &Option<sherpa_onnx::OfflinePunctuation>,
    sample_rate: u32,
    sender: &Sender<PipelineMsg>,
    stop_flag: &Arc<AtomicBool>,
) {
    let chunk = (sample_rate as f64 * CHUNK_SECS) as usize;
    let mut blocks = AudioBlockAssembler::new(chunk.max(1));
    let mut last_text = String::new();
    while !stop_flag.load(Ordering::Relaxed) {
        let dropped_buffers = capture.take_dropped_buffers();
        if dropped_buffers > 0 {
            eprintln!("GStreamer appsink 丢弃了 {dropped_buffers} 个音频 buffer");
            blocks.clear();
            engine.reset_stream();
            last_text.clear();
            continue;
        }

        let sample = match capture.try_pull_sample(POLL_INTERVAL) {
            Ok(Some(sample)) => sample,
            Ok(None) => continue,
            Err(error) => {
                let _ = sender.send(PipelineMsg::Error(format!(
                    "GStreamer 音频读取失败: {error}"
                )));
                break;
            }
        };
        let samples = match GstreamerCapture::samples_from_sample(&sample, capture.audio_info()) {
            Ok(samples) => samples,
            Err(error) => {
                let _ = sender.send(PipelineMsg::Error(format!(
                    "GStreamer 音频格式错误: {error}"
                )));
                break;
            }
        };
        blocks.push(&samples);

        while let Some(samples) = blocks.take_block() {
            match engine.transcribe(&samples, sample_rate) {
                Ok(text) => {
                    let mut trimmed = text.trim().to_string();
                    if let Some(punct) = punctuator {
                        if !trimmed.is_empty() {
                            trimmed = punct.add_punctuation(&trimmed).unwrap_or(trimmed);
                            // 移除标点模型自动附加的结尾标点
                            while trimmed
                                .ends_with(['。', '，', '、', '！', '？', '.', ',', '!', '?'])
                            {
                                trimmed.pop();
                            }
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
