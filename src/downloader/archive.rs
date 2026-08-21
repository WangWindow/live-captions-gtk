//! 压缩包解压模块（纯 Rust 实现，无系统依赖）
//!
//! 支持格式：.tar.bz2

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// 将压缩包解压到目标目录
///
/// 根据文件后缀自动选择解压方式，全部使用 Rust 库实现。
pub fn extract(archive_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
        extract_tar_bz2(archive_path, dest_dir)
    } else {
        Err(format!("不支持的压缩格式: {name}"))
    }
}

fn extract_tar<R: Read>(reader: R, dest_dir: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    archive
        .unpack(dest_dir)
        .map_err(|e| format!("解压 tar 失败: {e}"))
}

fn extract_tar_bz2(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("打开文件失败: {e}"))?;
    extract_tar(bzip2::read::BzDecoder::new(file), dest)
}
