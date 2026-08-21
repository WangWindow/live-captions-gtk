//! 预设模型定义
//!
//! 每个模型不仅定义文件列表，还通过 [`ModelKind`] 告诉引擎如何加载。

use std::path::Path;

/// 引擎类型 & 配置 —— 模型告诉引擎怎么用自己
#[derive(Debug, Clone, Copy)]
pub enum ModelKind {
    /// 流式 Zipformer 编码器-解码器-连接器三件套
    StreamingZipformer {
        encoder: &'static str,
        decoder: &'static str,
        joiner: &'static str,
        tokens: &'static str,
        bpe_vocab: Option<&'static str>,
    },
    /// 流式 Paraformer（达摩院），仅编码器+解码器，支持 language_hints
    Paraformer {
        encoder: &'static str,
        decoder: &'static str,
        tokens: &'static str,
    },
}

/// 模型类型标记，用于区分不同用途的模型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCategory {
    /// 语音识别模型
    Asr,
    /// 标点恢复模型
    Punctuation,
}

/// 单个模型文件描述
pub struct ModelFile {
    pub filename: &'static str,
}

/// 模型元信息
pub struct ModelInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub dir_name: &'static str,
    /// sherpa-onnx 官方 GitHub Release 归档地址。
    pub archive_url: &'static str,
    /// 引擎加载方式（仅 ASR 模型有效）
    pub kind: Option<ModelKind>,
    /// 模型用途类型
    pub category: ModelCategory,
    pub files: &'static [ModelFile],
}

impl ModelInfo {
    /// 判断模型目录是否包含所有非空的必需文件。
    pub fn is_complete(&self, model_dir: &Path) -> bool {
        self.files.iter().all(|file| {
            let path = model_dir.join(file.filename);
            std::fs::metadata(path)
                .map(|metadata| metadata.is_file() && metadata.len() > 0)
                .unwrap_or(false)
        })
    }
}

/// 返回所有内置模型定义，供下载、导入和运行时识别共用。
pub fn all_models() -> impl Iterator<Item = &'static ModelInfo> {
    ASR_MODELS.iter().chain(PUNCT_MODELS.iter())
}

/// 根据目录中实际存在的完整文件识别内置模型。
pub fn find_model_by_dir(model_dir: &Path) -> Option<&'static ModelInfo> {
    all_models().find(|model| model.is_complete(model_dir))
}

/// ASR 识别模型列表
pub const ASR_MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Streaming Zipformer (中文)",
        description: "流式 Zipformer 中文 (int8，推荐)",
        dir_name: "sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30",
        archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30.tar.bz2",
        category: ModelCategory::Asr,
        kind: Some(ModelKind::StreamingZipformer {
            encoder: "encoder.int8.onnx",
            decoder: "decoder.onnx",
            joiner: "joiner.int8.onnx",
            tokens: "tokens.txt",
            bpe_vocab: None,
        }),
        files: &[
            ModelFile {
                filename: "encoder.int8.onnx",
            },
            ModelFile {
                filename: "decoder.onnx",
            },
            ModelFile {
                filename: "joiner.int8.onnx",
            },
            ModelFile {
                filename: "tokens.txt",
            },
        ],
    },
    ModelInfo {
        name: "Streaming Paraformer (中英双语)",
        description: "流式 Paraformer 中英双语 (int8，支持方言/hotwords/language_hints)",
        dir_name: "sherpa-onnx-streaming-paraformer-bilingual-zh-en",
        archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-paraformer-bilingual-zh-en.tar.bz2",
        category: ModelCategory::Asr,
        kind: Some(ModelKind::Paraformer {
            encoder: "encoder.int8.onnx",
            decoder: "decoder.int8.onnx",
            tokens: "tokens.txt",
        }),
        files: &[
            ModelFile {
                filename: "encoder.int8.onnx",
            },
            ModelFile {
                filename: "decoder.int8.onnx",
            },
            ModelFile {
                filename: "tokens.txt",
            },
        ],
    },
];

/// 标点恢复模型列表
pub const PUNCT_MODELS: &[ModelInfo] = &[ModelInfo {
    name: "Punctuation CT-Transformer (中英)",
    description: "标点恢复 (int8)，自动添加逗号句号问号",
    dir_name: "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8",
    category: ModelCategory::Punctuation,
    kind: None,
    archive_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2",
    files: &[ModelFile {
        filename: "model.int8.onnx",
    }],
}];

pub enum DownloadMsg {
    Progress { downloaded: u64, total: u64 },
    Done(String),
    Error(String),
}
