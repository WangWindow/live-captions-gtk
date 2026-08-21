//! 下载后端调度模块
//!
//! 使用通用 HTTP 后端下载 sherpa-onnx 官方 GitHub Release 归档。
//!
//! 对外仅暴露 [`download_model`]，根据 `ModelInfo` 自动选择后端。

mod archive;
mod http;

use std::path::Path;
use std::sync::mpsc;

use crate::presets::{DownloadMsg, ModelInfo};

/// 下载一个模型的所有文件
///
/// 依次下载 `model_info.files` 中定义的每个文件到 `dest_dir/model_info.dir_name/` 下。
///
/// 下载归档，校验必需文件后发送完成消息。
pub fn download_model(model_info: &ModelInfo, dest_dir: &Path, tx: &mpsc::Sender<DownloadMsg>) {
    let model_dir = dest_dir.join(model_info.dir_name);

    if model_info.is_complete(&model_dir) {
        let _ = tx.send(DownloadMsg::Done(model_dir.to_string_lossy().into_owned()));
        return;
    }

    if let Err(e) = std::fs::create_dir_all(dest_dir) {
        let _ = tx.send(DownloadMsg::Error(format!("创建目录失败: {e}")));
        return;
    }

    let _ = tx.send(DownloadMsg::Progress {
        downloaded: 0,
        total: 0,
    });

    let archive_name = model_info
        .archive_url
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or("model.tar.bz2");
    let archive_path = dest_dir.join(archive_name);
    if let Err(error) = http::download_file(model_info.archive_url, &archive_path, tx) {
        let _ = tx.send(DownloadMsg::Error(format!("下载模型归档失败: {error}")));
        return;
    }
    if let Err(error) = archive::extract(&archive_path, dest_dir) {
        let _ = tx.send(DownloadMsg::Error(format!("解压模型归档失败: {error}")));
        return;
    }
    let _ = std::fs::remove_file(&archive_path);

    if !model_info.is_complete(&model_dir) {
        let _ = tx.send(DownloadMsg::Error("解压完成但模型文件不完整".into()));
        return;
    }

    let _ = tx.send(DownloadMsg::Done(model_dir.to_string_lossy().into_owned()));
}
