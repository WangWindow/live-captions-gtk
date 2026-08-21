//! HTTP 下载后端
//!
//! 对 GitHub Release 大归档使用 Range 分块，小归档使用单连接流式下载。
//! 所有写入都先落到临时文件，完成并校验后才替换目标文件。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::presets::DownloadMsg;

const NUM_THREADS: usize = 6;
const MAX_ATTEMPTS: usize = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(400);
const SINGLE_THREAD_THRESHOLD: u64 = 100 * 1024 * 1024;
const PROGRESS_STEP: u64 = 1024 * 1024;
const BUF_SIZE: usize = 64 * 1024;

/// 下载一个文件，并在成功后原子替换目标文件。
pub fn download_file(url: &str, dest: &Path, tx: &mpsc::Sender<DownloadMsg>) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("live-captions-gtk/0.1")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let (total, accepts_ranges) = probe_file_size(&client, url).unwrap_or((0, false));
    let _ = tx.send(DownloadMsg::Progress {
        downloaded: 0,
        total,
    });

    let partial = partial_path(dest)?;
    cleanup_parts(&partial, NUM_THREADS);
    let acc = Arc::new(AtomicU64::new(0));
    let result = if accepts_ranges && total > SINGLE_THREAD_THRESHOLD {
        download_multithreaded(&client, url, &partial, total, Arc::clone(&acc), tx.clone())
    } else {
        download_single(&client, url, &partial, total, &acc, tx)
    };

    if let Err(error) = result {
        let _ = std::fs::remove_file(&partial);
        cleanup_parts(&partial, NUM_THREADS);
        return Err(error);
    }

    std::fs::rename(&partial, dest).map_err(|e| format!("替换下载文件失败: {e}"))?;
    Ok(())
}

fn partial_path(dest: &Path) -> Result<PathBuf, String> {
    let file_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "下载目标文件名无效".to_string())?;
    Ok(dest.with_file_name(format!(".{file_name}.part")))
}

fn download_single(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    total: u64,
    acc: &AtomicU64,
    tx: &mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        acc.store(0, Ordering::Relaxed);
        match download_single_once(client, url, dest, total, acc, tx) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error;
                if attempt < MAX_ATTEMPTS {
                    retry_sleep(attempt);
                }
            }
        }
    }
    Err(last_error)
}

fn download_single_once(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    total: u64,
    acc: &AtomicU64,
    tx: &mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    let resp = request_with_retry(|| client.get(url).send())?;
    let mut file = std::fs::File::create(dest).map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut body = resp;
    let mut buf = vec![0u8; BUF_SIZE];
    let mut last_report = 0;

    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("读取下载内容失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入临时文件失败: {e}"))?;
        acc.fetch_add(n as u64, Ordering::Relaxed);
        report_progress(acc, total, &mut last_report, tx, false);
    }
    file.flush().map_err(|e| format!("刷新临时文件失败: {e}"))?;

    let downloaded = acc.load(Ordering::Relaxed);
    if total > 0 && downloaded != total {
        return Err(format!(
            "下载大小不匹配：期望 {total} 字节，实际 {downloaded} 字节"
        ));
    }
    report_progress(acc, total, &mut last_report, tx, true);
    Ok(())
}

fn download_multithreaded(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    total: u64,
    acc: Arc<AtomicU64>,
    tx: mpsc::Sender<DownloadMsg>,
) -> Result<(), String> {
    let n = NUM_THREADS.min((total / (1024 * 1024) + 1) as usize).max(2);
    let chunk_size = total / n as u64;
    let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();
    let client = Arc::new(client.clone());
    let mut handles = Vec::with_capacity(n);
    let mut spawn_error = None;

    for i in 0..n {
        let start = i as u64 * chunk_size;
        let end = if i == n - 1 {
            total - 1
        } else {
            (i as u64 + 1) * chunk_size - 1
        };
        let part = part_path(dest, i);
        let url = url.to_string();
        let client = Arc::clone(&client);
        let acc = Arc::clone(&acc);
        let tx = tx.clone();
        let done_tx = done_tx.clone();
        match std::thread::Builder::new()
            .name(format!("dl-part-{i}"))
            .spawn(move || {
                let result = download_range(&client, &url, &part, start, end, total, &acc, &tx);
                let _ = done_tx.send(result);
            }) {
            Ok(handle) => handles.push(handle),
            Err(error) => {
                spawn_error = Some(format!("创建下载线程失败: {error}"));
                break;
            }
        }
    }
    drop(done_tx);

    let mut first_error = None;
    for result in done_rx {
        if let Err(error) = result {
            first_error.get_or_insert(error);
        }
    }
    for handle in handles {
        if handle.join().is_err() {
            first_error.get_or_insert("下载线程异常退出".to_string());
        }
    }
    if let Some(error) = spawn_error {
        cleanup_parts(dest, n);
        return Err(error);
    }
    if let Some(error) = first_error {
        cleanup_parts(dest, n);
        return Err(error);
    }

    merge_parts(dest, n)?;
    let actual = std::fs::metadata(dest)
        .map_err(|e| format!("读取合并文件大小失败: {e}"))?
        .len();
    if actual != total {
        cleanup_parts(dest, n);
        return Err(format!(
            "分块合并大小不匹配：期望 {total} 字节，实际 {actual} 字节"
        ));
    }
    let _ = tx.send(DownloadMsg::Progress {
        downloaded: total,
        total,
    });
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
    let resp = request_with_retry(|| {
        client
            .get(url)
            .header(reqwest::header::RANGE, &range)
            .send()
    })?;
    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "分块请求期望 206 Partial Content，收到 {}",
            resp.status()
        ));
    }
    let expected = end - start + 1;
    let mut file = std::fs::File::create(part).map_err(|e| format!("创建分片文件失败: {e}"))?;
    let mut body = resp;
    let mut buf = vec![0u8; BUF_SIZE];
    let mut written = 0u64;
    let mut last_report = acc.load(Ordering::Relaxed);

    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("读取分片失败: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("写入分片失败: {e}"))?;
        written += n as u64;
        acc.fetch_add(n as u64, Ordering::Relaxed);
        report_progress(acc, total, &mut last_report, tx, false);
    }
    file.flush().map_err(|e| format!("刷新分片失败: {e}"))?;
    if written != expected {
        return Err(format!(
            "分片大小不匹配：期望 {expected} 字节，实际 {written} 字节"
        ));
    }
    Ok(())
}

fn merge_parts(dest: &Path, count: usize) -> Result<(), String> {
    let mut output = std::fs::File::create(dest).map_err(|e| format!("创建合并文件失败: {e}"))?;
    let mut buf = vec![0u8; BUF_SIZE];
    for i in 0..count {
        let part = part_path(dest, i);
        let mut input =
            std::fs::File::open(&part).map_err(|e| format!("打开分片 {i} 失败: {e}"))?;
        loop {
            let n = input
                .read(&mut buf)
                .map_err(|e| format!("读取分片 {i} 失败: {e}"))?;
            if n == 0 {
                break;
            }
            output
                .write_all(&buf[..n])
                .map_err(|e| format!("合并分片 {i} 失败: {e}"))?;
        }
        let _ = std::fs::remove_file(&part);
    }
    output
        .flush()
        .map_err(|e| format!("刷新合并文件失败: {e}"))?;
    output
        .sync_all()
        .map_err(|e| format!("同步合并文件失败: {e}"))?;
    Ok(())
}

fn part_path(dest: &Path, index: usize) -> PathBuf {
    PathBuf::from(format!("{}.part_{index}", dest.to_string_lossy()))
}

fn cleanup_parts(dest: &Path, count: usize) {
    for i in 0..count {
        let _ = std::fs::remove_file(part_path(dest, i));
    }
}

fn report_progress(
    acc: &AtomicU64,
    total: u64,
    last_report: &mut u64,
    tx: &mpsc::Sender<DownloadMsg>,
    finished: bool,
) {
    let downloaded = acc.load(Ordering::Relaxed);
    if finished || downloaded.saturating_sub(*last_report) >= PROGRESS_STEP {
        *last_report = downloaded;
        let _ = tx.send(DownloadMsg::Progress { downloaded, total });
    }
}

fn request_with_retry<F>(mut request: F) -> Result<reqwest::blocking::Response, String>
where
    F: FnMut() -> Result<reqwest::blocking::Response, reqwest::Error>,
{
    let mut last_error = String::new();
    for attempt in 1..=MAX_ATTEMPTS {
        match request() {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) if is_retryable_status(response.status()) => {
                last_error = format!("服务器返回 {}", response.status());
            }
            Ok(response) => return Err(format!("服务器返回 {}", response.status())),
            Err(error) => last_error = format!("网络请求失败: {error}"),
        }
        if attempt < MAX_ATTEMPTS {
            retry_sleep(attempt);
        }
    }
    Err(last_error)
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn retry_sleep(attempt: usize) {
    let factor = 1u32 << attempt.saturating_sub(1);
    std::thread::sleep(RETRY_BASE_DELAY.saturating_mul(factor));
}

fn probe_file_size(client: &reqwest::blocking::Client, url: &str) -> Result<(u64, bool), String> {
    if let Ok(response) = request_with_retry(|| client.head(url).send()) {
        if let Ok(total) = get_content_length(response.headers()) {
            let accepts_ranges = response
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.contains("bytes"))
                .unwrap_or(false);
            return Ok((total, accepts_ranges));
        }
    }

    probe_via_range(client, url)
}

fn probe_via_range(client: &reqwest::blocking::Client, url: &str) -> Result<(u64, bool), String> {
    let response = request_with_retry(|| {
        client
            .get(url)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
    })?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!(
            "Range 探测期望 206 Partial Content，收到 {}",
            response.status()
        ));
    }
    let total = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit_once('/'))
        .and_then(|(_, total)| total.parse::<u64>().ok())
        .ok_or_else(|| "服务器未返回有效 Content-Range".to_string())?;
    Ok((total, true))
}

fn get_content_length(headers: &reqwest::header::HeaderMap) -> Result<u64, String> {
    headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| "服务器未返回 Content-Length".to_string())
}
