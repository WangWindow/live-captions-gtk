//! 压缩包解压模块（纯 Rust 实现，无系统依赖）
//!
//! 支持格式：.tar.bz2, .tar.gz, .tar.xz, .tar, .zip, .bz2, .gz

use std::fs::File;
use std::io::{BufReader, Read};
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
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive_path, dest_dir)
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        extract_tar_xz(archive_path, dest_dir)
    } else if name.ends_with(".tar") {
        extract_tar_plain(archive_path, dest_dir)
    } else if name.ends_with(".zip") {
        extract_zip(archive_path, dest_dir)
    } else if name.ends_with(".bz2") {
        extract_bz2(archive_path, dest_dir)
    } else if name.ends_with(".gz") {
        extract_gz(archive_path, dest_dir)
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

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("打开文件失败: {e}"))?;
    extract_tar(flate2::read::GzDecoder::new(file), dest)
}

fn extract_tar_xz(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("打开文件失败: {e}"))?;
    extract_tar(xz2::read::XzDecoder::new(file), dest)
}

fn extract_tar_plain(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("打开文件失败: {e}"))?;
    extract_tar(BufReader::new(file), dest)
}

// ---------------------------------------------------------------------------
//  .zip
// ---------------------------------------------------------------------------

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| format!("打开压缩包失败: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("读取 zip 失败: {e}"))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {e}"))?;

        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let target = dest.join(name);

        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&target);
        } else {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut out = File::create(&target).map_err(|e| format!("创建文件失败: {e}"))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压文件失败: {e}"))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
//  单文件 .bz2 / .gz
// ---------------------------------------------------------------------------

fn extract_bz2(archive: &Path, dest: &Path) -> Result<(), String> {
    let out_name = archive
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let out_path = dest.join(out_name);
    let file = File::open(archive).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut decoder = bzip2::read::BzDecoder::new(file);
    let mut out = File::create(&out_path).map_err(|e| format!("创建文件失败: {e}"))?;
    std::io::copy(&mut decoder, &mut out).map_err(|e| format!("解压失败: {e}"))?;
    Ok(())
}

fn extract_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let out_name = archive
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let out_path = dest.join(out_name);
    let file = File::open(archive).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut out = File::create(&out_path).map_err(|e| format!("创建文件失败: {e}"))?;
    std::io::copy(&mut decoder, &mut out).map_err(|e| format!("解压失败: {e}"))?;
    Ok(())
}
