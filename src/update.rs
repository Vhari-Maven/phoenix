//! Update functionality for downloading and installing game updates.
//!
//! This module handles:
//! - Downloading release assets from GitHub with progress tracking
//! - Backing up the current installation
//! - Extracting new versions while preserving user data

use anyhow::{Context, Result};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

/// Directories to preserve during updates (user data)
const PRESERVE_DIRS: &[&str] = &[
    "save",      // Player saves - CRITICAL
    "config",    // User settings, keybindings
    "mods",      // User-installed mods
    "templates", // Character templates
    "memorial",  // Memorial files
    "graveyard", // Graveyard data
    "font",      // Custom fonts
];

/// Current phase of the update process
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatePhase {
    #[default]
    Idle,
    Downloading,
    BackingUp,
    Extracting,
    Restoring,
    Complete,
    Failed,
}

impl UpdatePhase {
    /// Get a human-readable description of the current phase
    pub fn description(&self) -> &'static str {
        match self {
            UpdatePhase::Idle => "Ready",
            UpdatePhase::Downloading => "Downloading update...",
            UpdatePhase::BackingUp => "Backing up current installation...",
            UpdatePhase::Extracting => "Extracting new version...",
            UpdatePhase::Restoring => "Restoring saves and settings...",
            UpdatePhase::Complete => "Update complete!",
            UpdatePhase::Failed => "Update failed",
        }
    }
}

/// Progress information for the update process
#[derive(Debug, Clone, Default)]
pub struct UpdateProgress {
    pub phase: UpdatePhase,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub speed: u64,           // bytes/sec
    pub files_extracted: usize,
    pub total_files: usize,
    pub current_file: String,
    pub error: Option<String>,
}

impl UpdateProgress {
    /// Calculate download progress as a fraction (0.0 - 1.0)
    pub fn download_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.bytes_downloaded as f32 / self.total_bytes as f32
        }
    }

    /// Calculate extraction progress as a fraction (0.0 - 1.0)
    pub fn extract_fraction(&self) -> f32 {
        if self.total_files == 0 {
            0.0
        } else {
            self.files_extracted as f32 / self.total_files as f32
        }
    }

    /// Get progress fraction for current phase
    pub fn fraction(&self) -> f32 {
        match self.phase {
            UpdatePhase::Downloading => self.download_fraction(),
            UpdatePhase::Extracting => self.extract_fraction(),
            _ => 0.0,
        }
    }
}

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
    tracing::info!("Starting download from: {}", url);

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
    tracing::info!("Download size: {} bytes", total_size);

    // Create parent directory if needed
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("Failed to create download directory")?;
    }

    // Download to a temporary .part file
    let temp_path = dest_path.with_extension("zip.part");
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .context("Failed to create temporary download file")?;

    // Stream the response body to disk
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_progress_time = Instant::now();
    let mut last_downloaded: u64 = 0;
    let mut current_speed: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.context("Error reading download stream")?;

        file.write_all(&chunk)
            .await
            .context("Failed to write to download file")?;

        downloaded += chunk.len() as u64;

        // Update progress every 100ms
        let now = Instant::now();
        let elapsed = now.duration_since(last_progress_time);
        if elapsed >= Duration::from_millis(100) {
            // Calculate speed
            let bytes_since_last = downloaded - last_downloaded;
            current_speed = (bytes_since_last as f64 / elapsed.as_secs_f64()) as u64;

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

    tracing::info!("Download complete: {} bytes to {:?}", downloaded, dest_path);

    Ok(DownloadResult {
        file_path: dest_path,
        bytes: downloaded,
    })
}

/// Perform the full update process: backup, extract, restore.
pub async fn install_update(
    zip_path: PathBuf,
    game_dir: PathBuf,
    progress_tx: watch::Sender<UpdateProgress>,
) -> Result<()> {
    let previous_version_dir = game_dir.join("previous_version");

    // Phase 1: Backup current installation
    tracing::info!("Backing up current installation to {:?}", previous_version_dir);
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::BackingUp,
        ..Default::default()
    });

    backup_current_installation(&game_dir, &previous_version_dir).await?;

    // Phase 2: Extract new version
    tracing::info!("Extracting update from {:?}", zip_path);
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Extracting,
        ..Default::default()
    });

    let total_files = extract_zip(&zip_path, &game_dir, progress_tx.clone()).await?;
    tracing::info!("Extracted {} files", total_files);

    // Log directory structure after extraction for debugging
    log_directory_structure(&game_dir).await;

    // Phase 3: Restore user data
    tracing::info!("Restoring user data from previous version");
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Restoring,
        ..Default::default()
    });

    restore_user_directories(&previous_version_dir, &game_dir).await?;

    // Complete
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Complete,
        files_extracted: total_files,
        total_files,
        ..Default::default()
    });

    tracing::info!("Update installation complete");
    Ok(())
}

/// Move current installation to backup directory.
async fn backup_current_installation(game_dir: &Path, backup_dir: &Path) -> Result<()> {
    // Remove old previous_version if exists
    if backup_dir.exists() {
        tokio::fs::remove_dir_all(backup_dir)
            .await
            .context("Failed to remove old previous_version directory")?;
    }

    tokio::fs::create_dir_all(backup_dir)
        .await
        .context("Failed to create previous_version directory")?;

    // Move all files/dirs except previous_version itself
    let mut entries = tokio::fs::read_dir(game_dir)
        .await
        .context("Failed to read game directory")?;

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip previous_version directory and any .part download files
        if name_str == "previous_version" || name_str.ends_with(".part") {
            continue;
        }

        let src = entry.path();
        let dst = backup_dir.join(&name);

        tracing::debug!("Moving {:?} to {:?}", src, dst);
        tokio::fs::rename(&src, &dst)
            .await
            .with_context(|| format!("Failed to move {:?} to backup", src))?;
    }

    Ok(())
}

/// Extract a ZIP archive to the destination directory.
async fn extract_zip(
    zip_path: &Path,
    destination: &Path,
    progress_tx: watch::Sender<UpdateProgress>,
) -> Result<usize> {
    let zip_path = zip_path.to_path_buf();
    let destination = destination.to_path_buf();

    // ZIP extraction is blocking, run in spawn_blocking
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&zip_path)
            .context("Failed to open ZIP file")?;
        let mut archive = zip::ZipArchive::new(file)
            .context("Failed to read ZIP archive")?;

        let total = archive.len();
        tracing::info!("ZIP contains {} entries", total);

        // Log first few entries to understand ZIP structure
        for i in 0..std::cmp::min(10, total) {
            if let Ok(f) = archive.by_index(i) {
                tracing::debug!("ZIP entry [{}]: {}", i, f.name());
            }
        }

        // Send initial extraction progress
        let _ = progress_tx.send(UpdateProgress {
            phase: UpdatePhase::Extracting,
            total_files: total,
            files_extracted: 0,
            ..Default::default()
        });

        for i in 0..total {
            let mut file = archive.by_index(i)
                .context("Failed to read ZIP entry")?;

            // Get the output path - extract directly without modifying paths
            let outpath = match file.enclosed_name() {
                Some(path) => destination.join(path),
                None => continue, // Skip entries with unsafe paths
            };

            // Log destination for first few files for debugging
            if i < 5 {
                tracing::debug!("Extracting: {} -> {:?}", file.name(), outpath);
            }

            // Handle directory or file
            if file.name().ends_with('/') {
                std::fs::create_dir_all(&outpath)
                    .with_context(|| format!("Failed to create directory {:?}", outpath))?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = outpath.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent)
                            .with_context(|| format!("Failed to create parent directory {:?}", parent))?;
                    }
                }

                // Extract file
                let mut outfile = std::fs::File::create(&outpath)
                    .with_context(|| format!("Failed to create file {:?}", outpath))?;
                std::io::copy(&mut file, &mut outfile)
                    .with_context(|| format!("Failed to extract file {:?}", outpath))?;
            }

            // Update progress periodically (every 50 files)
            if i % 50 == 0 || i == total - 1 {
                let current_file = file.name().to_string();
                let _ = progress_tx.send(UpdateProgress {
                    phase: UpdatePhase::Extracting,
                    files_extracted: i + 1,
                    total_files: total,
                    current_file,
                    ..Default::default()
                });
            }
        }

        Ok::<_, anyhow::Error>(total)
    })
    .await
    .context("ZIP extraction task panicked")?
}

/// Restore user directories from the backup.
async fn restore_user_directories(previous_dir: &Path, game_dir: &Path) -> Result<()> {
    for dir_name in PRESERVE_DIRS {
        let src = previous_dir.join(dir_name);
        let dst = game_dir.join(dir_name);

        if src.exists() {
            tracing::info!("Restoring user directory: {}", dir_name);

            // Remove any directory that might have been extracted
            if dst.exists() {
                tokio::fs::remove_dir_all(&dst)
                    .await
                    .with_context(|| format!("Failed to remove extracted {}", dir_name))?;
            }

            // Copy from backup (we copy instead of move to keep backup intact)
            copy_dir_recursive(&src, &dst).await?;
        }
    }

    Ok(())
}

/// Recursively copy a directory.
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dst)
        .await
        .with_context(|| format!("Failed to create directory {:?}", dst))?;

    let mut entries = tokio::fs::read_dir(src)
        .await
        .with_context(|| format!("Failed to read directory {:?}", src))?;

    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path)
                .await
                .with_context(|| format!("Failed to copy {:?}", src_path))?;
        }
    }

    Ok(())
}

/// Log the directory structure for debugging
async fn log_directory_structure(game_dir: &Path) {
    tracing::info!("Post-extraction directory structure:");

    // Check for key directories
    let dirs_to_check = [
        "data",
        "data/font",
        "data/json",
        "data/mods",
        "font",
        "json",
        "mods",
        "gfx",
        "save",
        "config",
    ];

    for dir in dirs_to_check {
        let path = game_dir.join(dir);
        if path.exists() {
            tracing::info!("  [EXISTS] {}/", dir);
        } else {
            tracing::debug!("  [MISSING] {}/", dir);
        }
    }

    // Check for executables
    let exes = ["cataclysm-tiles.exe", "cataclysm.exe"];
    for exe in exes {
        let path = game_dir.join(exe);
        if path.exists() {
            tracing::info!("  [EXISTS] {}", exe);
        }
    }

    // Check VERSION.txt
    if game_dir.join("VERSION.txt").exists() {
        tracing::info!("  [EXISTS] VERSION.txt");
    }
}

/// Get the download cache directory.
pub fn download_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("com", "phoenix", "Phoenix")
        .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;

    let download_dir = dirs.data_dir().join("downloads");
    std::fs::create_dir_all(&download_dir)?;

    Ok(download_dir)
}

/// Clean up partial download files.
pub async fn cleanup_partial_downloads(download_dir: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(download_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("part") {
            tracing::info!("Cleaning up partial download: {:?}", path);
            tokio::fs::remove_file(&path).await.ok();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_update_phase_description() {
        assert_eq!(UpdatePhase::Downloading.description(), "Downloading update...");
        assert_eq!(UpdatePhase::BackingUp.description(), "Backing up current installation...");
        assert_eq!(UpdatePhase::Extracting.description(), "Extracting new version...");
        assert_eq!(UpdatePhase::Restoring.description(), "Restoring saves and settings...");
        assert_eq!(UpdatePhase::Complete.description(), "Update complete!");
        assert_eq!(UpdatePhase::Failed.description(), "Update failed");
        assert_eq!(UpdatePhase::Idle.description(), "Ready");
    }

    #[test]
    fn test_progress_fraction() {
        let mut progress = UpdateProgress::default();

        // Download progress
        progress.phase = UpdatePhase::Downloading;
        progress.bytes_downloaded = 50;
        progress.total_bytes = 100;
        assert_eq!(progress.download_fraction(), 0.5);
        assert_eq!(progress.fraction(), 0.5);

        // Extract progress
        progress.phase = UpdatePhase::Extracting;
        progress.files_extracted = 25;
        progress.total_files = 100;
        assert_eq!(progress.extract_fraction(), 0.25);
        assert_eq!(progress.fraction(), 0.25);

        // Zero total should return 0
        progress.total_bytes = 0;
        progress.total_files = 0;
        assert_eq!(progress.download_fraction(), 0.0);
        assert_eq!(progress.extract_fraction(), 0.0);
    }

    #[test]
    fn test_preserve_dirs_contains_critical_directories() {
        // Ensure save directory is always preserved
        assert!(PRESERVE_DIRS.contains(&"save"), "save directory must be preserved");
        assert!(PRESERVE_DIRS.contains(&"config"), "config directory must be preserved");
        assert!(PRESERVE_DIRS.contains(&"mods"), "mods directory must be preserved");
    }

    #[test]
    fn test_download_dir_creation() {
        // This test verifies download_dir() returns a valid path
        let result = download_dir();
        assert!(result.is_ok(), "download_dir should succeed");

        let dir = result.unwrap();
        assert!(dir.exists(), "download directory should be created");
        assert!(dir.is_dir(), "download path should be a directory");
    }

    #[tokio::test]
    async fn test_backup_and_restore_preserves_saves() {
        // Create a temporary game directory structure
        let temp_dir = TempDir::new().unwrap();
        let game_dir = temp_dir.path().to_path_buf();

        // Create game files
        fs::write(game_dir.join("cataclysm-tiles.exe"), b"fake exe").unwrap();
        fs::write(game_dir.join("VERSION.txt"), b"0.G").unwrap();

        // Create save directory with test save
        let save_dir = game_dir.join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("test_world.sav"), b"save data").unwrap();

        // Create config directory
        let config_dir = game_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("options.json"), b"{}").unwrap();

        // Create mods directory
        let mods_dir = game_dir.join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        fs::write(mods_dir.join("my_mod.json"), b"mod data").unwrap();

        // Backup the installation
        let backup_dir = game_dir.join("previous_version");
        backup_current_installation(&game_dir, &backup_dir).await.unwrap();

        // Verify backup contains the files
        assert!(backup_dir.join("cataclysm-tiles.exe").exists());
        assert!(backup_dir.join("save").join("test_world.sav").exists());
        assert!(backup_dir.join("config").join("options.json").exists());
        assert!(backup_dir.join("mods").join("my_mod.json").exists());

        // Verify original files are moved (not copied)
        assert!(!game_dir.join("cataclysm-tiles.exe").exists());
        assert!(!game_dir.join("save").exists());

        // Simulate extracting a new version (just create the exe)
        fs::write(game_dir.join("cataclysm-tiles.exe"), b"new exe").unwrap();

        // Restore user directories
        restore_user_directories(&backup_dir, &game_dir).await.unwrap();

        // Verify saves are restored
        assert!(game_dir.join("save").join("test_world.sav").exists());
        let save_content = fs::read_to_string(game_dir.join("save").join("test_world.sav")).unwrap();
        assert_eq!(save_content, "save data");

        // Verify config is restored
        assert!(game_dir.join("config").join("options.json").exists());

        // Verify mods are restored
        assert!(game_dir.join("mods").join("my_mod.json").exists());

        // Verify backup still exists (for rollback)
        assert!(backup_dir.join("save").join("test_world.sav").exists());
    }

    #[tokio::test]
    async fn test_backup_removes_old_previous_version() {
        let temp_dir = TempDir::new().unwrap();
        let game_dir = temp_dir.path().to_path_buf();

        // Create an old previous_version directory
        let backup_dir = game_dir.join("previous_version");
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(backup_dir.join("old_file.txt"), b"old data").unwrap();

        // Create current game files
        fs::write(game_dir.join("game.exe"), b"game").unwrap();

        // Backup should remove old previous_version
        backup_current_installation(&game_dir, &backup_dir).await.unwrap();

        // Old file should be gone
        assert!(!backup_dir.join("old_file.txt").exists());

        // New file should be there
        assert!(backup_dir.join("game.exe").exists());
    }

    #[tokio::test]
    async fn test_copy_dir_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let src = temp_dir.path().join("src");
        let dst = temp_dir.path().join("dst");

        // Create nested directory structure
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("file1.txt"), b"content1").unwrap();
        fs::write(src.join("subdir").join("file2.txt"), b"content2").unwrap();

        // Copy recursively
        copy_dir_recursive(&src, &dst).await.unwrap();

        // Verify structure is copied
        assert!(dst.join("file1.txt").exists());
        assert!(dst.join("subdir").join("file2.txt").exists());

        // Verify content
        assert_eq!(fs::read_to_string(dst.join("file1.txt")).unwrap(), "content1");
        assert_eq!(fs::read_to_string(dst.join("subdir").join("file2.txt")).unwrap(), "content2");

        // Verify source still exists
        assert!(src.join("file1.txt").exists());
    }

    #[tokio::test]
    async fn test_cleanup_partial_downloads() {
        let temp_dir = TempDir::new().unwrap();
        let download_dir = temp_dir.path();

        // Create some files including .part files
        fs::write(download_dir.join("complete.zip"), b"zip data").unwrap();
        fs::write(download_dir.join("incomplete.zip.part"), b"partial").unwrap();
        fs::write(download_dir.join("another.part"), b"partial2").unwrap();

        // Cleanup
        cleanup_partial_downloads(download_dir).await.unwrap();

        // .part files should be removed
        assert!(!download_dir.join("incomplete.zip.part").exists());
        assert!(!download_dir.join("another.part").exists());

        // Complete file should remain
        assert!(download_dir.join("complete.zip").exists());
    }

    #[test]
    fn test_update_progress_default() {
        let progress = UpdateProgress::default();
        assert_eq!(progress.phase, UpdatePhase::Idle);
        assert_eq!(progress.bytes_downloaded, 0);
        assert_eq!(progress.total_bytes, 0);
        assert_eq!(progress.speed, 0);
        assert_eq!(progress.files_extracted, 0);
        assert_eq!(progress.total_files, 0);
        assert!(progress.current_file.is_empty());
    }
}
