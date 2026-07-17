//! 下载后端调度模块
//!
//! 提供两种下载路线：
//! - **HuggingFace** → `hf` 后端（基于 hf-hub，自动缓存 + 并发锁）
//! - **通用 HTTP** → `http` 后端（基于 reqwest，大文件多线程 Range 分块）
//!
//! 对外仅暴露 [`download_model`]，根据 `ModelInfo` 自动选择后端。

mod archive;
mod hf;
mod http;

use std::path::Path;
use std::sync::mpsc;

use crate::presets::{DownloadMsg, ModelInfo};

/// 下载一个模型的所有文件
///
/// 依次下载 `model_info.files` 中定义的每个文件到 `dest_dir/model_info.dir_name/` 下。
///
/// 下载路线：
/// - 有 archive_url → 下载压缩包并解压
/// - HuggingFace → `hf` 后端（基于 hf-hub，自动缓存）
/// - 其他来源 → `http` 后端（基于 reqwest）
pub fn download_model(model_info: &ModelInfo, dest_dir: &Path, tx: &mpsc::Sender<DownloadMsg>) {
    let model_dir = dest_dir.join(model_info.dir_name);

    // 压缩包发布模式：下载并解压
    if let Some(archive_url) = model_info.archive_url {
        // 检查所有定义的文件是否已存在
        let all_exist = model_info
            .files
            .iter()
            .all(|f| model_dir.join(f.filename).exists());
        if all_exist {
            let _ = tx.send(DownloadMsg::Done(model_dir.to_string_lossy().into_owned()));
            return;
        }
        let archive_name = archive_url
            .rsplit_once('/')
            .map(|(_, n)| n)
            .unwrap_or("model.tar.bz2");
        let archive_path = dest_dir.join(archive_name);
        let _ = tx.send(DownloadMsg::Progress {
            downloaded: 0,
            total: 0,
        });
        if let Err(e) = http::download_file(archive_url, &archive_path, tx) {
            let _ = tx.send(DownloadMsg::Error(format!("下载压缩包失败: {e}")));
            return;
        }
        // 根据后缀自动解压
        if let Err(e) = archive::extract(&archive_path, dest_dir) {
            let _ = tx.send(DownloadMsg::Error(format!("解压失败: {e}")));
            return;
        }
        let _ = std::fs::remove_file(&archive_path);
        let _ = tx.send(DownloadMsg::Done(model_dir.to_string_lossy().into_owned()));
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&model_dir) {
        let _ = tx.send(DownloadMsg::Error(format!("创建目录失败: {e}")));
        return;
    }

    // 先探测所有文件的总大小（仅用于进度显示）
    let total_bytes: u64 = estimate_total_size(model_info);
    let _ = tx.send(DownloadMsg::Progress {
        downloaded: 0,
        total: total_bytes,
    });

    // 逐个下载
    let mut downloaded: u64 = 0;
    for file_entry in model_info.files {
        let dest_path = model_dir.join(file_entry.filename);

        // 跳过已存在的文件
        if dest_path.exists() {
            if let Ok(meta) = std::fs::metadata(&dest_path) {
                downloaded += meta.len();
                let _ = tx.send(DownloadMsg::Progress {
                    downloaded,
                    total: total_bytes,
                });
            }
            continue;
        }

        // 选择后端：优先文件独立 URL → HF 仓库 → 错误
        let result = if let Some(url) = file_entry.download_url {
            http::download_file(url, &dest_path, tx)
        } else if let Some(repo) = &model_info.hf_repo {
            hf::download_file(repo, file_entry.filename, &dest_path, tx)
        } else {
            let _ = tx.send(DownloadMsg::Error(format!(
                "{} 无下载来源",
                file_entry.filename
            )));
            return;
        };

        match result {
            Ok(()) => {
                if let Ok(meta) = std::fs::metadata(&dest_path) {
                    downloaded += meta.len();
                }
            }
            Err(e) => {
                let _ = tx.send(DownloadMsg::Error(format!(
                    "下载 {} 失败: {e}",
                    file_entry.filename
                )));
                return;
            }
        }
    }

    let _ = tx.send(DownloadMsg::Done(model_dir.to_string_lossy().into_owned()));
}

/// 估算模型所有文件的总大小（已存在文件的实际大小 + 缺失文件按 50MB 估算）
fn estimate_total_size(model_info: &ModelInfo) -> u64 {
    let model_dir = crate::presets::models_dir().join(model_info.dir_name);
    model_info
        .files
        .iter()
        .map(|f| {
            let path = model_dir.join(f.filename);
            std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(50 * 1024 * 1024)
        })
        .sum()
}
