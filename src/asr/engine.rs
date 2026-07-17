//! 语音识别引擎 —— 流式 ASR

use std::path::Path;

use anyhow::{Context, Result, bail};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};

// ============================================================================
//  对外统一接口
// ============================================================================

pub struct TranscriptionEngine {
    recognizer: OnlineRecognizer,
    stream: OnlineStream,
}

impl TranscriptionEngine {
    pub fn from_model(model_info: &crate::presets::ModelInfo, model_dir: &Path) -> Result<Self> {
        use crate::presets::ModelKind;
        match &model_info.kind {
            Some(ModelKind::StreamingZipformer {
                encoder,
                decoder,
                joiner,
                tokens,
                bpe_vocab,
            }) => Self::new_streaming(model_dir, encoder, decoder, joiner, tokens, *bpe_vocab),
            Some(ModelKind::Paraformer {
                encoder,
                decoder,
                tokens,
            }) => Self::new_paraformer(model_dir, encoder, decoder, tokens),
            _ => bail!("不支持的模型类型"),
        }
    }

    pub fn transcribe(&mut self, samples: &[f32], sample_rate: u32) -> Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        self.stream.accept_waveform(sample_rate as i32, samples);
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        let r = self
            .recognizer
            .get_result(&self.stream)
            .context("转录失败")?;
        Ok(r.text.trim().to_string())
    }

    pub fn is_endpoint(&self) -> bool {
        self.recognizer.is_endpoint(&self.stream)
    }

    pub fn reset_stream(&mut self) {
        self.recognizer.reset(&self.stream);
    }

    pub fn finish(&self) {
        self.stream.input_finished();
    }

    fn new_streaming(
        model_dir: &Path,
        encoder: &str,
        decoder: &str,
        joiner: &str,
        tokens: &str,
        bpe_vocab: Option<&str>,
    ) -> Result<Self> {
        let enc = model_dir.join(encoder);
        let dec = model_dir.join(decoder);
        let joi = model_dir.join(joiner);
        let tok = model_dir.join(tokens);

        if !enc.exists() {
            bail!("缺少编码器: {}", enc.display());
        }
        if !dec.exists() {
            bail!("缺少解码器: {}", dec.display());
        }
        if !joi.exists() {
            bail!("缺少连接器: {}", joi.display());
        }
        if !tok.exists() {
            bail!("缺少词表: {}", tok.display());
        }

        let mut cfg = OnlineRecognizerConfig::default();
        cfg.model_config.transducer.encoder = Some(enc.to_string_lossy().into_owned());
        cfg.model_config.transducer.decoder = Some(dec.to_string_lossy().into_owned());
        cfg.model_config.transducer.joiner = Some(joi.to_string_lossy().into_owned());
        cfg.model_config.tokens = Some(tok.to_string_lossy().into_owned());
        cfg.model_config.model_type = Some("zipformer2".into());
        cfg.model_config.num_threads = 4;
        cfg.model_config.provider = Some("cpu".into());
        cfg.enable_endpoint = true;
        cfg.decoding_method = Some("greedy_search".into());

        if let Some(bpe) = bpe_vocab {
            let bpe_path = model_dir.join(bpe);
            if bpe_path.exists() {
                cfg.model_config.bpe_vocab = Some(bpe_path.to_string_lossy().into_owned());
            }
        }

        let recognizer = OnlineRecognizer::create(&cfg).context("无法创建流式识别器")?;
        let stream = recognizer.create_stream();
        Ok(Self { recognizer, stream })
    }

    fn new_paraformer(
        model_dir: &Path,
        encoder: &str,
        decoder: &str,
        tokens: &str,
    ) -> Result<Self> {
        let enc = model_dir.join(encoder);
        let dec = model_dir.join(decoder);
        let tok = model_dir.join(tokens);

        if !enc.exists() {
            bail!("缺少编码器: {}", enc.display());
        }
        if !dec.exists() {
            bail!("缺少解码器: {}", dec.display());
        }
        if !tok.exists() {
            bail!("缺少词表: {}", tok.display());
        }

        let mut cfg = OnlineRecognizerConfig::default();
        cfg.model_config.paraformer.encoder = Some(enc.to_string_lossy().into_owned());
        cfg.model_config.paraformer.decoder = Some(dec.to_string_lossy().into_owned());
        cfg.model_config.tokens = Some(tok.to_string_lossy().into_owned());
        cfg.model_config.model_type = Some("paraformer".into());
        cfg.model_config.num_threads = 4;
        cfg.model_config.provider = Some("cpu".into());
        cfg.enable_endpoint = true;
        cfg.decoding_method = Some("greedy_search".into());

        let recognizer = OnlineRecognizer::create(&cfg).context("无法创建流式识别器")?;
        let stream = recognizer.create_stream();
        Ok(Self { recognizer, stream })
    }
}
