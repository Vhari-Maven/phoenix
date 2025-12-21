//! Update functionality for downloading and installing game updates.
//!
//! This module handles:
//! - Downloading release assets from GitHub with progress tracking
//! - Backing up the current installation
//! - Extracting new versions while preserving user data
//! - Smart migration to only restore custom mods/tilesets/soundpacks/fonts

use anyhow::{Context, Result};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use crate::migration::{self, MigrationPlan, CONFIG_SKIP_FILES};

/// Directories to always restore completely (no smart filtering)
const SIMPLE_RESTORE_DIRS: &[&str] = &[
    "save",         // Player saves - CRITICAL
    "save_backups", // User backup archives
    "templates",    // Character templates
    "memorial",     // Memorial files
    "graveyard",    // Graveyard data
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

/// Perform the full update process: backup, extract, restore.
pub async fn install_update(
    zip_path: PathBuf,
    game_dir: PathBuf,
    progress_tx: watch::Sender<UpdateProgress>,
    prevent_save_move: bool,
    remove_previous_version: bool,
) -> Result<()> {
    let update_start = Instant::now();
    let previous_version_dir = game_dir.join("previous_version");
    let old_backup_dir = game_dir.join("previous_version_old");

    // Phase 1: Backup current installation (fast - uses rename, defers deletion)
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::BackingUp,
        ..Default::default()
    });

    let phase_start = Instant::now();
    backup_current_installation(&game_dir, &previous_version_dir, &old_backup_dir, prevent_save_move).await?;
    tracing::info!("Backup complete in {:.1}s", phase_start.elapsed().as_secs_f32());

    // Phase 2: Extract new version
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Extracting,
        ..Default::default()
    });

    let phase_start = Instant::now();
    let total_files = extract_zip(&zip_path, &game_dir, progress_tx.clone()).await?;
    tracing::info!("Extracted {} files in {:.1}s", total_files, phase_start.elapsed().as_secs_f32());

    // Verify extraction succeeded
    verify_extraction(&game_dir).await;

    // Phase 3: Smart restore user data
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Restoring,
        ..Default::default()
    });

    let phase_start = Instant::now();
    restore_user_directories_smart(&previous_version_dir, &game_dir, prevent_save_move).await?;
    tracing::info!("Restore complete in {:.1}s", phase_start.elapsed().as_secs_f32());

    // Phase 4: Cleanup
    // Always delete old_backup_dir (the stale previous_version from last update)
    // Delete in background to not block completion
    // Uses remove_dir_all crate which is faster than std::fs::remove_dir_all on Windows
    let old_backup_for_cleanup = old_backup_dir.clone();
    tokio::spawn(async move {
        if old_backup_for_cleanup.exists() {
            let start = Instant::now();
            let path = old_backup_for_cleanup.clone();
            let result = tokio::task::spawn_blocking(move || {
                remove_dir_all::remove_dir_all(&path)
            })
            .await;

            match result {
                Ok(Ok(())) => {
                    tracing::info!(
                        "Background cleanup complete in {:.1}s",
                        start.elapsed().as_secs_f32()
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to remove old backup: {}", e);
                }
                Err(e) => {
                    tracing::warn!("Cleanup task panicked: {}", e);
                }
            }
        }
    });

    // Optional cleanup of current previous_version
    if remove_previous_version {
        if let Err(e) = tokio::fs::remove_dir_all(&previous_version_dir).await {
            tracing::warn!("Failed to remove previous_version: {}", e);
        }
    }

    // Complete
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Complete,
        files_extracted: total_files,
        total_files,
        ..Default::default()
    });

    tracing::info!("Update complete in {:.1}s total", update_start.elapsed().as_secs_f32());
    Ok(())
}

/// Move current installation to backup directory.
///
/// Uses fast rename operations to avoid blocking on deletion:
/// 1. If old_backup_dir exists, delete it (from a previous failed update)
/// 2. If backup_dir exists, rename it to old_backup_dir (instant)
/// 3. Create new backup_dir and move files into it
///
/// The old_backup_dir will be cleaned up in the background after the update completes.
async fn backup_current_installation(
    game_dir: &Path,
    backup_dir: &Path,
    old_backup_dir: &Path,
    prevent_save_move: bool,
) -> Result<()> {
    // If old_backup_dir exists from a previous failed update, remove it first
    // This should be rare, so blocking here is acceptable
    if old_backup_dir.exists() {
        tokio::fs::remove_dir_all(old_backup_dir)
            .await
            .context("Failed to remove stale old backup directory")?;
    }

    // If backup_dir exists, rename it to old_backup_dir (instant operation)
    // This is the key optimization - we defer the expensive deletion
    if backup_dir.exists() {
        tokio::fs::rename(backup_dir, old_backup_dir)
            .await
            .context("Failed to rename previous_version to old backup")?;
        tracing::debug!("Renamed existing previous_version to old backup (deferred deletion)");
    }

    // Create fresh backup directory
    tokio::fs::create_dir_all(backup_dir)
        .await
        .context("Failed to create previous_version directory")?;

    // Move all files/dirs except backup directories and download files
    let mut entries = tokio::fs::read_dir(game_dir)
        .await
        .context("Failed to read game directory")?;

    let mut items_moved = 0u32;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip backup directories and any .part download files
        // Also skip save directory if prevent_save_move is enabled (leave saves in place)
        if name_str == "previous_version"
            || name_str == "previous_version_old"
            || name_str.ends_with(".part")
            || (name_str == "save" && prevent_save_move)
        {
            continue;
        }

        let src = entry.path();
        let dst = backup_dir.join(&name);

        tokio::fs::rename(&src, &dst)
            .await
            .with_context(|| format!("Failed to move {:?} to backup", src))?;
        items_moved += 1;
    }

    tracing::debug!("Moved {} items to backup", items_moved);
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

            // Update progress periodically (every 100 files)
            if i % 100 == 0 || i == total - 1 {
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

/// Restore user directories with smart migration.
///
/// This performs intelligent restoration:
/// - Simple dirs (save, templates, memorial, graveyard) are copied completely
/// - Config is copied with debug.log files filtered out
/// - Mods, tilesets, soundpacks, fonts use identity-based detection to only restore custom content
async fn restore_user_directories_smart(
    previous_dir: &Path,
    game_dir: &Path,
    prevent_save_move: bool,
) -> Result<()> {
    // Phase 1: Simple directory restoration
    let mut restored_dirs = Vec::new();
    for dir_name in SIMPLE_RESTORE_DIRS {
        // Skip save if prevent_save_move is enabled
        if *dir_name == "save" && prevent_save_move {
            continue;
        }

        let src = previous_dir.join(dir_name);
        let dst = game_dir.join(dir_name);

        if src.exists() {
            // Remove any directory that might have been extracted
            if dst.exists() {
                tokio::fs::remove_dir_all(&dst)
                    .await
                    .with_context(|| format!("Failed to remove extracted {}", dir_name))?;
            }

            // Copy from backup
            copy_dir_recursive(&src, &dst).await?;
            restored_dirs.push(*dir_name);
        }
    }

    if !restored_dirs.is_empty() {
        tracing::debug!("Restored directories: {}", restored_dirs.join(", "));
    }

    if prevent_save_move {
        tracing::info!("Skipped save directory (prevent_save_move enabled)");
    }

    // Phase 2: Config directory with file filtering
    restore_config_directory(previous_dir, game_dir).await?;

    // Phase 3: Smart migration for mods, tilesets, soundpacks, fonts
    let previous_dir_owned = previous_dir.to_path_buf();
    let game_dir_owned = game_dir.to_path_buf();

    let plan = tokio::task::spawn_blocking(move || {
        migration::create_migration_plan(&previous_dir_owned, &game_dir_owned)
    })
    .await
    .context("Migration plan task panicked")?;

    // Execute the migration plan
    execute_migration_plan(&plan, game_dir, previous_dir).await?;

    Ok(())
}

/// Restore config directory, skipping debug.log files
async fn restore_config_directory(previous_dir: &Path, game_dir: &Path) -> Result<()> {
    let src = previous_dir.join("config");
    let dst = game_dir.join("config");

    if !src.exists() {
        return Ok(());
    }

    // Remove any config that was extracted
    if dst.exists() {
        tokio::fs::remove_dir_all(&dst).await?;
    }

    // Create destination
    tokio::fs::create_dir_all(&dst).await?;

    let mut entries = tokio::fs::read_dir(&src).await?;
    let mut skipped_count = 0u32;

    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Skip debug files
        if CONFIG_SKIP_FILES.iter().any(|skip| name_str == *skip) {
            skipped_count += 1;
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&file_name);

        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }

    if skipped_count > 0 {
        tracing::debug!("Skipped {} debug files from config", skipped_count);
    }

    Ok(())
}

/// Execute a migration plan by copying only custom content
async fn execute_migration_plan(
    plan: &MigrationPlan,
    game_dir: &Path,
    previous_dir: &Path,
) -> Result<()> {
    let mut restored_counts: Vec<String> = Vec::new();

    // Restore custom mods to data/mods/
    if !plan.custom_mods.is_empty() {
        let mods_dir = game_dir.join("data").join("mods");
        let mut count = 0;
        for mod_info in &plan.custom_mods {
            if let Some(dir_name) = mod_info.path.file_name() {
                let target = mods_dir.join(dir_name);
                if !target.exists() {
                    copy_dir_recursive(&mod_info.path, &target).await?;
                    count += 1;
                }
            }
        }
        if count > 0 {
            restored_counts.push(format!("{} custom mods", count));
        }
    }

    // Restore custom user mods to mods/
    if !plan.custom_user_mods.is_empty() {
        let user_mods_dir = game_dir.join("mods");
        tokio::fs::create_dir_all(&user_mods_dir).await?;

        let mut count = 0;
        for mod_info in &plan.custom_user_mods {
            if let Some(dir_name) = mod_info.path.file_name() {
                let target = user_mods_dir.join(dir_name);
                if !target.exists() {
                    copy_dir_recursive(&mod_info.path, &target).await?;
                    count += 1;
                }
            }
        }
        if count > 0 {
            restored_counts.push(format!("{} user mods", count));
        }
    }

    // Restore custom tilesets to gfx/
    if !plan.custom_tilesets.is_empty() {
        let gfx_dir = game_dir.join("gfx");
        let mut count = 0;
        for tileset_info in &plan.custom_tilesets {
            if let Some(dir_name) = tileset_info.path.file_name() {
                let target = gfx_dir.join(dir_name);
                if !target.exists() {
                    copy_dir_recursive(&tileset_info.path, &target).await?;
                    count += 1;
                }
            }
        }
        if count > 0 {
            restored_counts.push(format!("{} tilesets", count));
        }
    }

    // Restore custom soundpacks to data/sound/
    if !plan.custom_soundpacks.is_empty() {
        let sound_dir = game_dir.join("data").join("sound");
        let mut count = 0;
        for soundpack_info in &plan.custom_soundpacks {
            if let Some(dir_name) = soundpack_info.path.file_name() {
                let target = sound_dir.join(dir_name);
                if !target.exists() {
                    copy_dir_recursive(&soundpack_info.path, &target).await?;
                    count += 1;
                }
            }
        }
        if count > 0 {
            restored_counts.push(format!("{} soundpacks", count));
        }
    }

    // Restore custom fonts
    if !plan.custom_fonts.is_empty() {
        let font_dir = game_dir.join("font");
        tokio::fs::create_dir_all(&font_dir).await?;

        let mut count = 0;
        for font_path in &plan.custom_fonts {
            if let Some(file_name) = font_path.file_name() {
                let target = font_dir.join(file_name);
                if font_path.is_file() {
                    tokio::fs::copy(font_path, &target).await?;
                    count += 1;
                } else if font_path.is_dir() {
                    copy_dir_recursive(font_path, &target).await?;
                    count += 1;
                }
            }
        }
        if count > 0 {
            restored_counts.push(format!("{} fonts", count));
        }
    }

    // Restore custom data fonts
    if !plan.custom_data_fonts.is_empty() {
        let data_font_dir = game_dir.join("data").join("font");
        tokio::fs::create_dir_all(&data_font_dir).await?;

        for font_path in &plan.custom_data_fonts {
            if let Some(file_name) = font_path.file_name() {
                let target = data_font_dir.join(file_name);
                if font_path.is_file() {
                    tokio::fs::copy(font_path, &target).await?;
                } else if font_path.is_dir() {
                    copy_dir_recursive(font_path, &target).await?;
                }
            }
        }
    }

    // Restore user-default-mods.json if needed
    if plan.restore_user_default_mods {
        let src = previous_dir
            .join("data")
            .join("mods")
            .join("user-default-mods.json");
        let dst = game_dir
            .join("data")
            .join("mods")
            .join("user-default-mods.json");
        if src.exists() && !dst.exists() {
            tokio::fs::copy(&src, &dst).await?;
            restored_counts.push("user-default-mods.json".to_string());
        }
    }

    // Log summary of restored custom content
    if !restored_counts.is_empty() {
        tracing::info!("Restored custom content: {}", restored_counts.join(", "));
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

/// Verify critical files exist after extraction
async fn verify_extraction(game_dir: &Path) -> bool {
    // Just check for the executable - the most critical file
    let exe_exists = game_dir.join("cataclysm-tiles.exe").exists()
        || game_dir.join("cataclysm.exe").exists();

    if !exe_exists {
        tracing::warn!("Game executable not found after extraction");
    }

    exe_exists
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
    fn test_simple_restore_dirs_contains_critical_directories() {
        // Ensure save directory is always in simple restore
        assert!(SIMPLE_RESTORE_DIRS.contains(&"save"), "save directory must be preserved");
        // Config is handled separately with filtering, not in SIMPLE_RESTORE_DIRS
        // Mods are handled via smart migration, not simple restore
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

        // Create config directory with options and debug.log
        let config_dir = game_dir.join("config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("options.json"), b"{}").unwrap();
        fs::write(config_dir.join("debug.log"), b"debug output").unwrap();

        // Create a custom mod with proper modinfo.json
        let custom_mod_dir = game_dir.join("data").join("mods").join("my_custom_mod");
        fs::create_dir_all(&custom_mod_dir).unwrap();
        fs::write(
            custom_mod_dir.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "my_custom_mod", "name": "My Custom Mod"}"#,
        )
        .unwrap();

        // Backup the installation (prevent_save_move = false, so saves are backed up)
        let backup_dir = game_dir.join("previous_version");
        let old_backup_dir = game_dir.join("previous_version_old");
        backup_current_installation(&game_dir, &backup_dir, &old_backup_dir, false).await.unwrap();

        // Verify backup contains the files
        assert!(backup_dir.join("cataclysm-tiles.exe").exists());
        assert!(backup_dir.join("save").join("test_world.sav").exists());
        assert!(backup_dir.join("config").join("options.json").exists());
        assert!(backup_dir.join("config").join("debug.log").exists());
        assert!(backup_dir.join("data").join("mods").join("my_custom_mod").join("modinfo.json").exists());

        // Verify original files are moved (not copied)
        assert!(!game_dir.join("cataclysm-tiles.exe").exists());
        assert!(!game_dir.join("save").exists());

        // Simulate extracting a new version with official mod
        fs::write(game_dir.join("cataclysm-tiles.exe"), b"new exe").unwrap();
        let official_mod_dir = game_dir.join("data").join("mods").join("official_mod");
        fs::create_dir_all(&official_mod_dir).unwrap();
        fs::write(
            official_mod_dir.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "official_mod", "name": "Official"}"#,
        )
        .unwrap();

        // Restore user directories with smart migration
        restore_user_directories_smart(&backup_dir, &game_dir, false).await.unwrap();

        // Verify saves are restored
        assert!(game_dir.join("save").join("test_world.sav").exists());
        let save_content = fs::read_to_string(game_dir.join("save").join("test_world.sav")).unwrap();
        assert_eq!(save_content, "save data");

        // Verify config is restored (options.json should exist)
        assert!(game_dir.join("config").join("options.json").exists());
        // Verify debug.log is NOT restored (filtered out)
        assert!(!game_dir.join("config").join("debug.log").exists());

        // Verify custom mod is restored
        assert!(game_dir.join("data").join("mods").join("my_custom_mod").join("modinfo.json").exists());

        // Verify backup still exists (for rollback)
        assert!(backup_dir.join("save").join("test_world.sav").exists());
    }

    #[tokio::test]
    async fn test_restore_with_prevent_save_move() {
        let temp_dir = TempDir::new().unwrap();
        let previous_dir = temp_dir.path().join("previous_version");
        let game_dir = temp_dir.path().join("game");

        // Create save in previous version
        let save_dir = previous_dir.join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("world.sav"), b"save data").unwrap();

        // Create game dir
        fs::create_dir_all(&game_dir).unwrap();

        // Restore with prevent_save_move = true
        restore_user_directories_smart(&previous_dir, &game_dir, true).await.unwrap();

        // Save should NOT be restored when prevent_save_move is true
        assert!(!game_dir.join("save").exists());
    }

    #[tokio::test]
    async fn test_backup_renames_old_previous_version() {
        let temp_dir = TempDir::new().unwrap();
        let game_dir = temp_dir.path().to_path_buf();

        // Create an old previous_version directory
        let backup_dir = game_dir.join("previous_version");
        let old_backup_dir = game_dir.join("previous_version_old");
        fs::create_dir_all(&backup_dir).unwrap();
        fs::write(backup_dir.join("old_file.txt"), b"old data").unwrap();

        // Create current game files
        fs::write(game_dir.join("game.exe"), b"game").unwrap();

        // Backup should rename old previous_version to old_backup_dir
        backup_current_installation(&game_dir, &backup_dir, &old_backup_dir, false).await.unwrap();

        // Old file should be in old_backup_dir (renamed, not deleted)
        assert!(old_backup_dir.join("old_file.txt").exists());

        // New previous_version should have current game file
        assert!(backup_dir.join("game.exe").exists());
        assert!(!backup_dir.join("old_file.txt").exists());
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
