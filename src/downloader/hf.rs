//! HuggingFace 下载后端 —— 基于 hf-hub 库
//!
//! hf-hub 自动处理缓存、并发锁、断点续传。
//! 下载后复制到我们自己的模型目录中。

use std::path::Path;
use std::sync::mpsc;

use crate::presets::DownloadMsg;

/// 使用 hf-hub 从 HuggingFace 下载一个文件
///
/// - `hf_repo`: "owner/name"
/// - `filename`: 仓库中的文件名
/// - `dest`: 本地目标路径
/// - `tx`: 进度消息发送端
pub fn download_file(
    hf_repo: &str,
    filename: &str,
    dest: &Path,
    estimated_size_bytes: u64,
    tx: &mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    // 拆解 repo 为 owner 和 name
    let (owner, name) = hf_repo
        .split_once('/')
        .ok_or_else(|| format!("无效的 HuggingFace 仓库 ID: {hf_repo}"))?;

    // 创建 hf-hub 同步客户端
    let client = hf_hub::HFClientSync::new().map_err(|e| format!("创建 HF 客户端失败: {e}"))?;

    let _ = tx.send(DownloadMsg::Progress {
        downloaded: 0,
        total: estimated_size_bytes,
    });

    // 下载（hf-hub 自动缓存到 ~/.cache/huggingface/hub/）
    let cached_path = client
        .model(owner, name)
        .download_file()
        .filename(filename)
        .send()
        .map_err(|e| format!("HF 下载失败 ({filename}): {e}"))?;

    // 复制到目标目录
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::copy(&cached_path, dest).map_err(|e| format!("复制文件失败 ({filename}): {e}"))?;

    // 报告完成
    if let Ok(meta) = std::fs::metadata(dest) {
        let _ = tx.send(DownloadMsg::Progress {
            downloaded: meta.len(),
            total: estimated_size_bytes,
        });
    }

    Ok(())
}
