//! Download functionality for game updates.

use anyhow::{Context, Result};
use futures::StreamExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use crate::app_data::migration_config;

use super::{UpdatePhase, UpdateProgress};

/// Result of a successful download
pub struct DownloadResult {
    pub file_path: PathBuf,
    pub bytes: u64,
}

/// Download a release asset with progress tracking.
///
/// Downloads to a `.part` temporary file, then renames on success.
pub async fn download_asset(
    client: reqwest::Client,
    url: String,
    dest_path: PathBuf,
    progress_tx: watch::Sender<UpdateProgress>,
) -> Result<DownloadResult> {
    let download_start = Instant::now();

    // Send initial progress
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Downloading,
        ..Default::default()
    });

    // Start the download request
    let response = client
        .get(&url)
        .send()
        .await
        .context("Failed to connect to download server")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Download failed with status: {} - {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("Unknown error")
        );
    }

    let total_size = response.content_length().unwrap_or(0);

    // Create parent directory if needed
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("Failed to create download directory")?;
    }

    // Download to a temporary file using configured extension
    let temp_ext = format!("zip{}", migration_config().download.temp_extension);
    let temp_path = dest_path.with_extension(temp_ext);
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .context("Failed to create temporary download file")?;

    // Stream the response body to disk
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_progress_time = Instant::now();
    let mut last_downloaded: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Error reading download stream")?;

        file.write_all(&chunk)
            .await
            .context("Failed to write to download file")?;

        downloaded += chunk.len() as u64;

        // Update progress at configured interval
        let now = Instant::now();
        let elapsed = now.duration_since(last_progress_time);
        if elapsed >= Duration::from_millis(migration_config().download.progress_interval_ms) {
            // Calculate speed
            let bytes_since_last = downloaded - last_downloaded;
            let current_speed = (bytes_since_last as f64 / elapsed.as_secs_f64()) as u64;

            let _ = progress_tx.send(UpdateProgress {
                phase: UpdatePhase::Downloading,
                bytes_downloaded: downloaded,
                total_bytes: total_size,
                speed: current_speed,
                ..Default::default()
            });

            last_downloaded = downloaded;
            last_progress_time = now;
        }
    }

    // Ensure all data is written
    file.sync_all()
        .await
        .context("Failed to sync download file")?;
    drop(file);

    // Rename temp file to final destination
    tokio::fs::rename(&temp_path, &dest_path)
        .await
        .context("Failed to finalize download")?;

    let elapsed = download_start.elapsed().as_secs_f32();
    let speed_mbps = (downloaded as f32 / 1_000_000.0) / elapsed;
    tracing::info!(
        "Download complete: {:.1} MB in {:.1}s ({:.1} MB/s)",
        downloaded as f32 / 1_000_000.0,
        elapsed,
        speed_mbps
    );

    Ok(DownloadResult {
        file_path: dest_path,
        bytes: downloaded,
    })
}

/// Get the download cache directory.
pub fn download_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "phoenix", "Phoenix")
        .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;

    let download_dir = dirs.data_dir().join("downloads");
    std::fs::create_dir_all(&download_dir)?;

    Ok(download_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_dir_creation() {
        // This test verifies download_dir() returns a valid path
        let result = download_dir();
        assert!(result.is_ok(), "download_dir should succeed");

        let dir = result.unwrap();
        assert!(dir.exists(), "download directory should be created");
        assert!(dir.is_dir(), "download path should be a directory");
    }
}
