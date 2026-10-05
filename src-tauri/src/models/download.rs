//! Model download with resume, progress and checksum (section 12).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub dest: PathBuf,
    pub part: PathBuf,
    pub expected_size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub received: u64,
    pub total: u64,
    pub bytes_per_sec: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("cancelled")]
    Cancelled,
    #[error("checksum mismatch")]
    Checksum,
    #[error("not enough disk space: need {needed} bytes, have {available}")]
    DiskFull { needed: u64, available: u64 },
    #[error("network error: {0}")]
    Network(String),
    #[error("server answered HTTP {0}")]
    Http(u16),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        DownloadError::Network(e.to_string())
    }
}

const PROGRESS_EVERY: Duration = Duration::from_millis(250);

pub fn client() -> reqwest::Client {
    crate::net::ensure_crypto_provider();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!("Kikitori/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client builds")
}

/// Downloads to `part`, resuming when possible, then verifies and renames to `dest`.
pub async fn download(
    client: &reqwest::Client,
    req: &DownloadRequest,
    mut cancel: watch::Receiver<bool>,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), DownloadError> {
    if let Some(dir) = req.dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let mut existing = tokio::fs::metadata(&req.part).await.map(|m| m.len()).unwrap_or(0);
    if existing > req.expected_size {
        tokio::fs::remove_file(&req.part).await?;
        existing = 0;
    }

    if let Some(dir) = req.dest.parent() {
        let needed = req.expected_size.saturating_sub(existing) + req.expected_size / 10;
        if let Some(available) = free_space(dir)
            && available < needed
        {
            return Err(DownloadError::DiskFull { needed, available });
        }
    }

    let mut hasher = Sha256::new();
    let mut request = client.get(&req.url);
    if existing > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let response = request.send().await?;
    let status = response.status();

    let mut file = if existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
        hash_existing(&req.part, &mut hasher).await?;
        tokio::fs::OpenOptions::new().append(true).open(&req.part).await?
    } else if status.is_success() {
        // 200 to a Range request means the server will not resume: start over.
        existing = 0;
        tokio::fs::File::create(&req.part).await?
    } else {
        return Err(DownloadError::Http(status.as_u16()));
    };

    let total = req.expected_size;
    let mut received = existing;
    let started = Instant::now();
    let mut last_emit = Instant::now() - PROGRESS_EVERY;
    let mut stream = response.bytes_stream();
    on_progress(Progress { received, total, bytes_per_sec: 0.0 });

    loop {
        let next = tokio::select! {
            chunk = stream.next() => chunk,
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    file.flush().await?;
                    return Err(DownloadError::Cancelled);
                }
                continue;
            }
        };
        let Some(chunk) = next else { break };
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                // Keep what arrived so the next attempt resumes from it.
                file.flush().await?;
                return Err(e.into());
            }
        };
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        received += chunk.len() as u64;
        if last_emit.elapsed() >= PROGRESS_EVERY {
            last_emit = Instant::now();
            let secs = started.elapsed().as_secs_f64().max(0.001);
            on_progress(Progress { received, total, bytes_per_sec: (received - existing) as f64 / secs });
        }
        if *cancel.borrow() {
            file.flush().await?;
            return Err(DownloadError::Cancelled);
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    let secs = started.elapsed().as_secs_f64().max(0.001);
    on_progress(Progress { received, total, bytes_per_sec: (received - existing) as f64 / secs });

    if received != total {
        return Err(DownloadError::Network(format!("connection closed at {received} of {total} bytes")));
    }
    let digest = hex(&hasher.finalize());
    if !digest.eq_ignore_ascii_case(&req.sha256) {
        let _ = tokio::fs::remove_file(&req.part).await;
        return Err(DownloadError::Checksum);
    }
    tokio::fs::rename(&req.part, &req.dest).await?;
    Ok(())
}

async fn hash_existing(path: &Path, hasher: &mut Sha256) -> std::io::Result<()> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file on disk, on the calling (blocking) thread.
pub fn sha256_file(path: &Path, mut on_progress: impl FnMut(u64)) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        on_progress(done);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is a NUL-terminated path that outlives the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }.ok()?;
    Some(available)
}

#[cfg(not(windows))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Resumable,
        IgnoreRange,
        /// Sends the first `n` bytes and drops the connection.
        CutAt(usize),
    }

    /// A one-file HTTP server on localhost.
    fn serve(body: Vec<u8>, mode: Mode) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                hits2.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range_start = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("range: bytes=") {
                        range_start = v.trim().trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                let (status, slice) = match (mode, range_start) {
                    (Mode::Resumable, Some(start)) => ("206 Partial Content", &body[start..]),
                    (Mode::CutAt(n), None) => ("200 OK", &body[..n]),
                    (Mode::CutAt(_), Some(start)) => ("206 Partial Content", &body[start..]),
                    _ => ("200 OK", &body[..]),
                };
                let declared =
                    if let Mode::CutAt(_) = mode { body.len() - range_start.unwrap_or(0) } else { slice.len() };
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(slice);
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}/model.bin"), hits)
    }

    fn body() -> Vec<u8> {
        (0..300_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn request(dir: &Path, url: &str, data: &[u8]) -> DownloadRequest {
        DownloadRequest {
            url: url.to_string(),
            dest: dir.join("model.bin"),
            part: dir.join("model.bin.part"),
            expected_size: data.len() as u64,
            sha256: hex(&Sha256::digest(data)),
        }
    }

    fn no_cancel() -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        std::mem::forget(tx);
        rx
    }

    #[tokio::test]
    async fn resumes_with_206() {
        let data = body();
        let dir = tempfile::tempdir().unwrap();
        let (url, _) = serve(data.clone(), Mode::Resumable);
        let req = request(dir.path(), &url, &data);
        std::fs::write(&req.part, &data[..100_000]).unwrap();
        let mut last = None;
        download(&client(), &req, no_cancel(), |p| last = Some(p)).await.unwrap();
        assert_eq!(std::fs::read(&req.dest).unwrap(), data);
        assert!(!req.part.exists());
        assert_eq!(last.unwrap().received, data.len() as u64);
    }

    #[tokio::test]
    async fn restarts_when_server_answers_200() {
        let data = body();
        let dir = tempfile::tempdir().unwrap();
        let (url, _) = serve(data.clone(), Mode::IgnoreRange);
        let req = request(dir.path(), &url, &data);
        std::fs::write(&req.part, vec![0xAAu8; 50_000]).unwrap(); // junk that must be discarded
        download(&client(), &req, no_cancel(), |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&req.dest).unwrap(), data);
    }

    #[tokio::test]
    async fn checksum_mismatch_deletes_the_file() {
        let data = body();
        let dir = tempfile::tempdir().unwrap();
        let (url, _) = serve(data.clone(), Mode::Resumable);
        let mut req = request(dir.path(), &url, &data);
        req.sha256 = "0".repeat(64);
        let err = download(&client(), &req, no_cancel(), |_| {}).await.unwrap_err();
        assert!(matches!(err, DownloadError::Checksum));
        assert!(!req.part.exists());
        assert!(!req.dest.exists());
    }

    #[tokio::test]
    async fn interrupted_download_keeps_part_and_resumes() {
        let data = body();
        let dir = tempfile::tempdir().unwrap();
        let (url, hits) = serve(data.clone(), Mode::CutAt(120_000));
        let req = request(dir.path(), &url, &data);
        let err = download(&client(), &req, no_cancel(), |_| {}).await.unwrap_err();
        assert!(matches!(err, DownloadError::Network(_)), "{err:?}");
        let kept = std::fs::metadata(&req.part).unwrap().len();
        assert_eq!(kept, 120_000);
        download(&client(), &req, no_cancel(), |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&req.dest).unwrap(), data);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancel_keeps_part_file() {
        let data = body();
        let dir = tempfile::tempdir().unwrap();
        let (url, _) = serve(data.clone(), Mode::Resumable);
        let req = request(dir.path(), &url, &data);
        let (tx, rx) = watch::channel(false);
        tx.send(true).unwrap();
        let err = download(&client(), &req, rx, |_| {}).await.unwrap_err();
        assert!(matches!(err, DownloadError::Cancelled));
        assert!(req.part.exists());
        assert!(!req.dest.exists());
    }

    #[test]
    fn free_space_reports_something() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free_space(dir.path()).unwrap_or(1) > 0);
    }
}
