//! HTTP 下载后端 —— 基于 reqwest
//!
//! 为通常的 http 下载预留。
//! 大文件自动启用多线程 Range 分块加速。

#![allow(dead_code)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

use crate::presets::DownloadMsg;

/// 并发下载线程数
const NUM_THREADS: usize = 6;

/// 大文件分块阈值（GitHub CDN 对 Range 支持不稳定，设高点）
const SINGLE_THREAD_THRESHOLD: u64 = 100 * 1024 * 1024;

const BUF_SIZE: usize = 64 * 1024;  

/// 通过 HTTP 下载一个文件
///
/// - 大文件（>50MB）自动启用多线程 Range 分块下载
/// - 小文件使用单线程流式下载
pub fn download_file(url: &str, dest: &Path, tx: &mpsc::Sender<DownloadMsg>) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("live-captions-gtk/0.1")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    // 探测文件大小
    let (total, accepts_ranges) = probe_file_size(&client, url)?;
    let _ = tx.send(DownloadMsg::Progress {
        downloaded: 0,
        total,
    });

    let acc = Arc::new(AtomicU64::new(0));

    if accepts_ranges && total > SINGLE_THREAD_THRESHOLD {
        download_multithreaded(&client, url, dest, total, Arc::clone(&acc), tx.clone())?;
    } else {
        download_single(&client, url, dest, total, &acc, tx)?;
    }

    Ok(())
}

// ============================================================================
//  探测
// ============================================================================

fn probe_file_size(client: &reqwest::blocking::Client, url: &str) -> Result<(u64, bool), String> {
    let resp = client
        .head(url)
        .send()
        .map_err(|e| format!("HEAD 失败: {e}"))?;
    if resp.status().is_success() {
        let total = get_content_length(resp.headers())?;
        let accepts = resp
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("bytes"))
            .unwrap_or(false);
        return Ok((total, accepts));
    }
    // HEAD 失败（如 GitHub CDN 重定向后不支持 HEAD），尝试 Range 探测
    if let Ok((total, true)) = probe_via_range(client, url) {
        return Ok((total, true));
    }
    // 都失败则返回 0，走单线程下载
    Ok((0, false))
}

fn probe_via_range(client: &reqwest::blocking::Client, url: &str) -> Result<(u64, bool), String> {
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .map_err(|e| format!("探测失败: {e}"))?;
    let total = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split('/').nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| "未返回 Content-Range".to_string())?;
    Ok((total, true))
}

fn get_content_length(headers: &reqwest::header::HeaderMap) -> Result<u64, String> {
    headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or_else(|| "未返回 Content-Length".to_string())
}

// ============================================================================
//  单线程下载
// ============================================================================

fn download_single(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    total: u64,
    acc: &AtomicU64,
    tx: &mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("GET 失败: {e}"))?;
    check_status(resp.status())?;
    let mut file = std::fs::File::create(dest).map_err(|e| format!("创建文件: {e}"))?;
    let mut buf = vec![0u8; BUF_SIZE];
    let mut body = resp;
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("读失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写失败: {e}"))?;
        acc.fetch_add(n as u64, Ordering::Relaxed);
        let _ = tx.send(DownloadMsg::Progress {
            downloaded: acc.load(Ordering::Relaxed),
            total,
        });
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

// ============================================================================
//  多线程分块下载
// ============================================================================

fn download_multithreaded(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    total: u64,
    acc: Arc<AtomicU64>,
    tx: mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    let tmp_dir = dest.parent().unwrap();
    let n = NUM_THREADS.min((total / (1024 * 1024) + 1) as usize).max(2);
    let chunk_size = total / n as u64;
    let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();
    let client = Arc::new(client.clone());

    for i in 0..n {
        let start = i as u64 * chunk_size;
        let end = if i == n - 1 {
            total - 1
        } else {
            (i as u64 + 1) * chunk_size - 1
        };
        let part = tmp_dir.join(format!(
            "{}.part_{i}",
            dest.file_name().unwrap().to_string_lossy()
        ));
        let u = url.to_string();
        let c = Arc::clone(&client);
        let a = Arc::clone(&acc);
        let t = tx.clone();
        let dt = done_tx.clone();
        std::thread::Builder::new()
            .name(format!("dl-part-{i}"))
            .spawn(move || {
                let _ = dt.send(download_range(&c, &u, &part, start, end, total, &a, &t));
            })
            .map_err(|e| format!("创建线程失败: {e}"))?;
    }
    drop(done_tx);
    for r in done_rx {
        r?;
    }
    merge_parts(dest, tmp_dir, n)?;
    Ok(())
}

fn download_range(
    client: &reqwest::blocking::Client,
    url: &str,
    part: &Path,
    start: u64,
    end: u64,
    total: u64,
    acc: &AtomicU64,
    tx: &mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    let range = format!("bytes={start}-{end}");
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, &range)
        .send()
        .map_err(|e| format!("Range 失败: {e}"))?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("期望 206，收到 {}", resp.status()));
    }
    let mut file = std::fs::File::create(part).map_err(|e| format!("创段: {e}"))?;
    let mut buf = vec![0u8; BUF_SIZE];
    let mut body = resp;
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("读段: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写段: {e}"))?;
        acc.fetch_add(n as u64, Ordering::Relaxed);
        let _ = tx.send(DownloadMsg::Progress {
            downloaded: acc.load(Ordering::Relaxed),
            total,
        });
    }
    file.flush().map_err(|e| format!("段 flush: {e}"))?;
    Ok(())
}

fn merge_parts(dest: &Path, tmp_dir: &Path, n: usize) -> Result<(), String> {
    let stem = dest.file_name().unwrap().to_string_lossy();
    let mut out = std::fs::File::create(dest).map_err(|e| format!("合并: {e}"))?;
    let mut buf = vec![0u8; BUF_SIZE];
    for i in 0..n {
        let p = tmp_dir.join(format!("{stem}.part_{i}"));
        let mut f = std::fs::File::open(&p).map_err(|e| format!("打开 part_{i}: {e}"))?;
        loop {
            let n = f.read(&mut buf).map_err(|e| format!("读 part_{i}: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| format!("写: {e}"))?;
        }
        drop(f);
        let _ = std::fs::remove_file(&p);
    }
    out.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

fn check_status(status: reqwest::StatusCode) -> Result<(), String> {
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("服务器返回 {status}"))
    }
}
