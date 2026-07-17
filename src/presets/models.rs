//! 预设模型定义
//!
//! 每个模型不仅定义文件列表，还通过 [`ModelKind`] 告诉引擎如何加载。

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
#[allow(dead_code)]
pub struct ModelFile {
    pub filename: &'static str,
    pub description: &'static str,
    /// 独立下载地址。`None` 时由 ModelInfo.hf_repo 决定来源。
    pub download_url: Option<&'static str>,
}

/// 模型元信息
pub struct ModelInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub dir_name: &'static str,
    /// HF 仓库 ID。`None` 表示所有文件都有独立 download_url。
    pub hf_repo: Option<&'static str>,
    /// 引擎加载方式（仅 ASR 模型有效）
    pub kind: Option<ModelKind>,
    /// 模型用途类型
    pub category: ModelCategory,
    /// 若模型以压缩包形式发布（如 .tar.bz2），设置此 URL 下载后自动解压
    pub archive_url: Option<&'static str>,
    pub files: &'static [ModelFile],
}

/// ASR 识别模型列表
pub const ASR_MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Streaming Zipformer (中文)",
        description: "流式 Zipformer 中文 (int8, ~160MB, 推荐)",
        dir_name: "sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30",
        hf_repo: Some("csukuangfj/sherpa-onnx-streaming-zipformer-zh-int8-2025-06-30"),
        category: ModelCategory::Asr,
        kind: Some(ModelKind::StreamingZipformer {
            encoder: "encoder.int8.onnx",
            decoder: "decoder.onnx",
            joiner: "joiner.int8.onnx",
            tokens: "tokens.txt",
            bpe_vocab: None,
        }),
        archive_url: None,
        files: &[
            ModelFile {
                filename: "encoder.int8.onnx",
                description: "编码器 (154 MB)",
                download_url: None,
            },
            ModelFile {
                filename: "decoder.onnx",
                description: "解码器 (4.9 MB)",
                download_url: None,
            },
            ModelFile {
                filename: "joiner.int8.onnx",
                description: "连接器 (1.0 MB)",
                download_url: None,
            },
            ModelFile {
                filename: "tokens.txt",
                description: "词表",
                download_url: None,
            },
        ],
    },
    ModelInfo {
        name: "Streaming Paraformer (中英双语)",
        description: "流式 Paraformer 中英双语 (int8, ~226MB, 支持方言/hotwords/language_hints)",
        dir_name: "sherpa-onnx-streaming-paraformer-bilingual-zh-en",
        hf_repo: Some("csukuangfj/sherpa-onnx-streaming-paraformer-bilingual-zh-en"),
        category: ModelCategory::Asr,
        kind: Some(ModelKind::Paraformer {
            encoder: "encoder.int8.onnx",
            decoder: "decoder.int8.onnx",
            tokens: "tokens.txt",
        }),
        archive_url: None,
        files: &[
            ModelFile {
                filename: "encoder.int8.onnx",
                description: "编码器 (158 MB)",
                download_url: None,
            },
            ModelFile {
                filename: "decoder.int8.onnx",
                description: "解码器 (68 MB)",
                download_url: None,
            },
            ModelFile {
                filename: "tokens.txt",
                description: "词表",
                download_url: None,
            },
        ],
    },
];

/// 标点恢复模型列表
pub const PUNCT_MODELS: &[ModelInfo] = &[ModelInfo {
    name: "Punctuation CT-Transformer (中英)",
    description: "标点恢复 (int8, 72MB)，自动添加逗号句号问号",
    dir_name: "sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8",
    hf_repo: None,
    category: ModelCategory::Punctuation,
    kind: None,
    archive_url: Some(
        "https://github.com/k2-fsa/sherpa-onnx/releases/download/punctuation-models/sherpa-onnx-punct-ct-transformer-zh-en-vocab272727-2024-04-12-int8.tar.bz2",
    ),
    files: &[ModelFile {
        filename: "model.int8.onnx",
        description: "标点模型 (72 MB)",
        download_url: None, // 通过 archive_url 下载解压
    }],
}];

pub enum DownloadMsg {
    Progress { downloaded: u64, total: u64 },
    Done(String),
    Error(String),
}
