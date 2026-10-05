//! Downloads a catalog model with the app's downloader:
//! `cargo run --example download_model -- <model-id> [dest-dir]`.

use kikitori_lib::models::{catalog, download};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let id = std::env::args().nth(1).unwrap_or_else(|| "base-q5".into());
    let dir = std::env::args().nth(2).map(std::path::PathBuf::from).unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join("dev.yusai.kikitori").join("models")
    });
    let entry = catalog::find(&id).ok_or_else(|| anyhow::anyhow!("unknown model {id}"))?;
    let req = download::DownloadRequest {
        url: entry.url(),
        dest: entry.path_in(&dir),
        part: entry.part_path_in(&dir),
        expected_size: entry.size_bytes,
        sha256: entry.sha256.clone(),
    };
    if req.dest.exists() {
        println!("{} already present at {}", id, req.dest.display());
        return Ok(());
    }
    let (_tx, rx) = tokio::sync::watch::channel(false);
    let started = std::time::Instant::now();
    let mut last_print = std::time::Instant::now();
    download::download(&download::client(), &req, rx, |p| {
        if last_print.elapsed().as_secs_f32() > 2.0 || p.received == p.total {
            last_print = std::time::Instant::now();
            println!("{:>6.1}% {:>8.1} MB/s", p.received as f64 * 100.0 / p.total as f64, p.bytes_per_sec / 1e6);
        }
    })
    .await?;
    println!("{} verified and saved to {} in {:?}", id, req.dest.display(), started.elapsed());
    Ok(())
}
