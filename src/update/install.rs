//! Installation functionality for game updates.
//!
//! Handles archiving, extraction, restoration, and rollback.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;
use tokio::sync::watch;

use crate::app_data::{game_config, migration_config};
use crate::migration::{self, config_skip_files, MigrationPlan};

use super::access::check_installation_access;
use super::{UpdatePhase, UpdateProgress};

/// Perform the full update process: backup, extract, restore.
///
/// If extraction or restore fails after archiving, automatically rolls back
/// to the previous installation.
pub async fn install_update(
    archive_path: PathBuf,
    game_dir: PathBuf,
    progress_tx: watch::Sender<UpdateProgress>,
    prevent_save_move: bool,
    remove_previous_version: bool,
) -> Result<()> {
    let update_start = Instant::now();
    let archive_dir = game_dir.join(&migration_config().archive.directory);
    let old_archive_dir = game_dir.join(&migration_config().archive.directory_old);

    // Pre-flight check: verify we have write access before making any changes
    check_installation_access(&game_dir).await?;

    // Phase 1: Archive current installation (fast - uses rename, defers deletion)
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::BackingUp,
        ..Default::default()
    });

    let phase_start = Instant::now();
    archive_current_installation(&game_dir, &archive_dir, &old_archive_dir, prevent_save_move)
        .await?;
    tracing::info!(
        "Archive complete in {:.1}s",
        phase_start.elapsed().as_secs_f32()
    );

    // Phase 2: Extract new version
    // If this fails, we need to rollback
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Extracting,
        ..Default::default()
    });

    let phase_start = Instant::now();
    let extract_result = extract_archive(&archive_path, &game_dir, progress_tx.clone()).await;

    let total_files = match extract_result {
        Ok(count) => count,
        Err(e) => {
            tracing::error!("Extraction failed, rolling back: {}", e);
            if let Err(rollback_err) = rollback_from_archive(&game_dir, &archive_dir).await {
                tracing::error!("Rollback also failed: {}", rollback_err);
                anyhow::bail!(
                    "Update failed during extraction AND rollback failed.\n\n\
                     Extraction error: {}\n\
                     Rollback error: {}\n\n\
                     Your installation may be corrupted. Please reinstall the game.",
                    e,
                    rollback_err
                );
            }
            anyhow::bail!(
                "Update failed during extraction. Previous version has been restored.\n\nError: {}",
                e
            );
        }
    };
    tracing::info!(
        "Extracted {} files in {:.1}s",
        total_files,
        phase_start.elapsed().as_secs_f32()
    );

    // Verify extraction succeeded
    verify_extraction(&game_dir).await;

    // Phase 3: Smart restore user data
    // If this fails, we also need to rollback
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Restoring,
        ..Default::default()
    });

    let phase_start = Instant::now();
    let restore_result =
        restore_user_directories_smart(&archive_dir, &game_dir, prevent_save_move).await;

    if let Err(e) = restore_result {
        tracing::error!("Restore failed, rolling back: {}", e);
        if let Err(rollback_err) = rollback_from_archive(&game_dir, &archive_dir).await {
            tracing::error!("Rollback also failed: {}", rollback_err);
            anyhow::bail!(
                "Update failed during restore AND rollback failed.\n\n\
                 Restore error: {}\n\
                 Rollback error: {}\n\n\
                 Your installation may be corrupted. Please reinstall the game.",
                e,
                rollback_err
            );
        }
        anyhow::bail!(
            "Update failed during restore. Previous version has been restored.\n\nError: {}",
            e
        );
    }
    tracing::info!(
        "Restore complete in {:.1}s",
        phase_start.elapsed().as_secs_f32()
    );

    // Phase 4: Cleanup
    // Always delete old_archive_dir (the stale archive from last update)
    // Delete in background to not block completion
    // Uses remove_dir_all crate which is faster than std::fs::remove_dir_all on Windows
    let old_archive_for_cleanup = old_archive_dir.clone();
    tokio::spawn(async move {
        if old_archive_for_cleanup.exists() {
            let start = Instant::now();
            let path = old_archive_for_cleanup.clone();
            let result =
                tokio::task::spawn_blocking(move || remove_dir_all::remove_dir_all(&path)).await;

            match result {
                Ok(Ok(())) => {
                    tracing::info!(
                        "Background cleanup complete in {:.1}s",
                        start.elapsed().as_secs_f32()
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to remove old archive: {}", e);
                }
                Err(e) => {
                    tracing::warn!("Cleanup task panicked: {}", e);
                }
            }
        }
    });

    // Optional cleanup of current archive directory
    if remove_previous_version {
        if let Err(e) = tokio::fs::remove_dir_all(&archive_dir).await {
            tracing::warn!("Failed to remove installation archive: {}", e);
        }
    }

    // Complete
    let _ = progress_tx.send(UpdateProgress {
        phase: UpdatePhase::Complete,
        files_extracted: total_files,
        total_files,
        ..Default::default()
    });

    tracing::info!(
        "Update complete in {:.1}s total",
        update_start.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Move current installation to archive directory for rollback.
///
/// This preserves the current game installation in `.phoenix_archive/` so users
/// can roll back if needed. This is distinct from save backups (compressed archives
/// of save data managed via the Backups tab, stored in the platform data directory).
///
/// Uses fast rename operations to avoid blocking on deletion:
/// 1. If old_archive_dir exists, delete it (from a previous failed update)
/// 2. If archive_dir exists, rename it to old_archive_dir (instant)
/// 3. Create new archive_dir and move files into it
///
/// The old_archive_dir will be cleaned up in the background after the update completes.
async fn archive_current_installation(
    game_dir: &Path,
    archive_dir: &Path,
    old_archive_dir: &Path,
    prevent_save_move: bool,
) -> Result<()> {
    // If old_archive_dir exists from a previous failed update, remove it first
    // This should be rare, so blocking here is acceptable
    if old_archive_dir.exists() {
        tokio::fs::remove_dir_all(old_archive_dir)
            .await
            .context("Failed to remove stale old archive directory")?;
    }

    // If archive_dir exists, rename it to old_archive_dir (instant operation)
    // This is the key optimization - we defer the expensive deletion
    if archive_dir.exists() {
        tokio::fs::rename(archive_dir, old_archive_dir)
            .await
            .context("Failed to rename .phoenix_archive to old archive")?;
        tracing::debug!("Renamed existing .phoenix_archive to old archive (deferred deletion)");
    }

    // Create fresh archive directory
    tokio::fs::create_dir_all(archive_dir)
        .await
        .context("Failed to create .phoenix_archive directory")?;

    // Move all files/dirs except archive directories and download files
    let mut entries = tokio::fs::read_dir(game_dir)
        .await
        .context("Failed to read game directory")?;

    let mut items_moved = 0u32;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip archive directories and any temp download files
        // Also skip save directory if prevent_save_move is enabled (leave saves in place)
        let config = migration_config();
        let is_archive_dir =
            name_str == config.archive.directory || name_str == config.archive.directory_old;
        let is_temp_download = name_str.ends_with(&config.download.temp_extension);
        let is_save_protected = name_str == game_config().directories.save && prevent_save_move;

        if is_archive_dir || is_temp_download || is_save_protected {
            continue;
        }

        let src = entry.path();
        let dst = archive_dir.join(&name);

        tokio::fs::rename(&src, &dst)
            .await
            .with_context(|| format!("Failed to move {:?} to archive", src))?;
        items_moved += 1;
    }

    tracing::debug!("Moved {} items to archive", items_moved);
    Ok(())
}

/// Extract a downloaded release archive, dispatching on its format.
///
/// Windows releases ship as `.zip`; Linux releases ship as `.tar.gz`.
async fn extract_archive(
    archive_path: &Path,
    destination: &Path,
    progress_tx: watch::Sender<UpdateProgress>,
) -> Result<usize> {
    let name = archive_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive_path, destination, progress_tx).await
    } else {
        extract_zip(archive_path, destination, progress_tx).await
    }
}

/// Extract a gzip-compressed tarball to the destination directory.
///
/// CDDA's Linux releases wrap everything in a single top-level directory
/// (e.g. `cataclysmdda-0.J/`). To match the flat layout produced by the
/// Windows ZIP, that common wrapper directory is stripped during extraction.
/// File permissions (notably the executable bit on the game binary) are
/// preserved.
async fn extract_tar_gz(
    archive_path: &Path,
    destination: &Path,
    progress_tx: watch::Sender<UpdateProgress>,
) -> Result<usize> {
    use flate2::read::GzDecoder;
    use std::ffi::OsString;

    let archive_path = archive_path.to_path_buf();
    let destination = destination.to_path_buf();

    // Decompression + extraction is blocking, run in spawn_blocking
    tokio::task::spawn_blocking(move || {
        // Pass 1: count entries and detect a wrapper directory shared by all
        // entries (so we only strip it when it genuinely wraps everything).
        let (total, strip_prefix) = {
            let file =
                std::fs::File::open(&archive_path).context("Failed to open tar.gz file")?;
            let mut archive = tar::Archive::new(GzDecoder::new(file));

            let mut total = 0usize;
            let mut prefix: Option<OsString> = None;
            let mut shared = true;

            for entry in archive.entries().context("Failed to read tar.gz archive")? {
                let entry = entry.context("Failed to read tar entry")?;
                let path = entry.path().context("Invalid path in tar entry")?;

                let first = match path.components().next() {
                    Some(Component::Normal(c)) => Some(c.to_owned()),
                    _ => None,
                };

                match (&prefix, first) {
                    (None, Some(c)) => prefix = Some(c),
                    (Some(p), Some(c)) if *p == c => {}
                    _ => shared = false,
                }

                total += 1;
            }

            let strip_prefix = if shared { prefix } else { None };
            (total, strip_prefix)
        };

        if let Some(prefix) = &strip_prefix {
            tracing::debug!("Stripping tar.gz wrapper directory: {:?}", prefix);
        }

        // Send initial extraction progress
        let _ = progress_tx.send(UpdateProgress {
            phase: UpdatePhase::Extracting,
            total_files: total,
            files_extracted: 0,
            ..Default::default()
        });

        // Pass 2: extract, stripping the wrapper directory.
        let file = std::fs::File::open(&archive_path).context("Failed to open tar.gz file")?;
        let mut archive = tar::Archive::new(GzDecoder::new(file));
        archive.set_preserve_permissions(true);
        archive.set_overwrite(true);

        let batch_size = migration_config().download.extraction_batch_size;
        let mut extracted = 0usize;

        for (i, entry) in archive
            .entries()
            .context("Failed to read tar.gz archive")?
            .enumerate()
        {
            let mut entry = entry.context("Failed to read tar entry")?;
            let entry_path = entry.path().context("Invalid path in tar entry")?.into_owned();

            // Strip the common wrapper directory, if any.
            let relative = match &strip_prefix {
                Some(prefix) => entry_path
                    .strip_prefix(prefix)
                    .unwrap_or(&entry_path)
                    .to_path_buf(),
                None => entry_path.clone(),
            };

            // Skip the wrapper directory entry itself.
            if relative.as_os_str().is_empty() {
                continue;
            }

            // Guard against path traversal (entries containing "..").
            if relative
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            {
                tracing::warn!("Skipping unsafe tar entry: {:?}", entry_path);
                continue;
            }

            let outpath = destination.join(&relative);

            // Ensure the parent directory exists before unpacking.
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create parent directory {:?}", parent)
                    })?;
                }
            }

            entry
                .unpack(&outpath)
                .with_context(|| format!("Failed to extract {:?}", outpath))?;
            extracted += 1;

            // Update progress periodically
            if i % batch_size == 0 || i == total.saturating_sub(1) {
                let current_file = relative.to_string_lossy().to_string();
                let _ = progress_tx.send(UpdateProgress {
                    phase: UpdatePhase::Extracting,
                    files_extracted: i + 1,
                    total_files: total,
                    current_file,
                    ..Default::default()
                });
            }
        }

        Ok::<_, anyhow::Error>(extracted)
    })
    .await
    .context("tar.gz extraction task panicked")?
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
        let file = std::fs::File::open(&zip_path).context("Failed to open ZIP file")?;
        let mut archive = zip::ZipArchive::new(file).context("Failed to read ZIP archive")?;

        let total = archive.len();

        // Send initial extraction progress
        let _ = progress_tx.send(UpdateProgress {
            phase: UpdatePhase::Extracting,
            total_files: total,
            files_extracted: 0,
            ..Default::default()
        });

        for i in 0..total {
            let mut file = archive.by_index(i).context("Failed to read ZIP entry")?;

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
                        std::fs::create_dir_all(parent).with_context(|| {
                            format!("Failed to create parent directory {:?}", parent)
                        })?;
                    }
                }

                // Extract file
                let mut outfile = std::fs::File::create(&outpath)
                    .with_context(|| format!("Failed to create file {:?}", outpath))?;
                std::io::copy(&mut file, &mut outfile)
                    .with_context(|| format!("Failed to extract file {:?}", outpath))?;
            }

            // Update progress periodically
            let batch_size = migration_config().download.extraction_batch_size;
            if i % batch_size == 0 || i == total - 1 {
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
    let save_dir = &game_config().directories.save;
    for dir_name in &migration_config().restore.simple_dirs {
        // Skip save if prevent_save_move is enabled
        if dir_name == save_dir && prevent_save_move {
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
            restored_dirs.push(dir_name.clone());
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
        if config_skip_files().iter().any(|skip| name_str == *skip) {
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

    // Restore custom files within matched soundpacks (smart merge)
    if !plan.soundpack_merges.is_empty() {
        let mut file_count = 0;
        for merge_info in &plan.soundpack_merges {
            for relative_path in &merge_info.custom_files {
                let src = merge_info.old_path.join(relative_path);
                let dst = merge_info.new_path.join(relative_path);

                // Only copy if source exists and destination doesn't
                if src.exists() && !dst.exists() {
                    // Ensure parent directory exists (for custom subdirectories)
                    if let Some(parent) = dst.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::copy(&src, &dst).await?;
                    file_count += 1;
                }
            }
        }
        if file_count > 0 {
            restored_counts.push(format!("{} soundpack files", file_count));
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
pub(crate) async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
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
    let exe_exists = game_config()
        .executables
        .names()
        .iter()
        .any(|exe| game_dir.join(exe).exists());

    if !exe_exists {
        tracing::warn!("Game executable not found after extraction");
    }

    exe_exists
}

/// Rollback to the previous installation from archive.
///
/// Called when an update fails after archiving but before completion.
/// Moves all files from .phoenix_archive back to the game directory.
async fn rollback_from_archive(game_dir: &Path, archive_dir: &Path) -> Result<()> {
    tracing::warn!("Rolling back to previous installation from archive...");

    // First, clear any partially extracted files from game_dir
    // (except the archive directories themselves)
    let config = migration_config();
    let mut entries = tokio::fs::read_dir(game_dir)
        .await
        .context("Failed to read game directory during rollback")?;

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Keep archive directories
        if name_str == config.archive.directory || name_str == config.archive.directory_old {
            continue;
        }

        // Remove everything else (partial extraction)
        let path = entry.path();
        if path.is_dir() {
            if let Err(e) = tokio::fs::remove_dir_all(&path).await {
                tracing::warn!("Failed to remove {} during rollback: {}", path.display(), e);
            }
        } else if let Err(e) = tokio::fs::remove_file(&path).await {
            tracing::warn!("Failed to remove {} during rollback: {}", path.display(), e);
        }
    }

    // Now move everything from archive back to game directory
    let mut archive_entries = tokio::fs::read_dir(archive_dir)
        .await
        .context("Failed to read archive directory during rollback")?;

    let mut items_restored = 0u32;
    while let Some(entry) = archive_entries.next_entry().await? {
        let name = entry.file_name();
        let src = entry.path();
        let dst = game_dir.join(&name);

        tokio::fs::rename(&src, &dst)
            .await
            .with_context(|| format!("Failed to restore {:?} from archive", src))?;
        items_restored += 1;
    }

    // Remove the now-empty archive directory
    if let Err(e) = tokio::fs::remove_dir(archive_dir).await {
        tracing::warn!("Failed to remove empty archive directory: {}", e);
    }

    tracing::info!("Rollback complete: restored {} items", items_restored);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    use crate::app_data::{game_config, migration_config};

    #[test]
    fn test_simple_restore_dirs_contains_critical_directories() {
        // Ensure save directory is always in simple restore config
        let simple_dirs = &migration_config().restore.simple_dirs;
        let save_dir = &game_config().directories.save;
        assert!(
            simple_dirs.contains(save_dir),
            "save directory must be preserved"
        );
        // Config is handled separately with filtering, not in simple_dirs
        // Mods are handled via smart migration, not simple restore
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

        // Archive the installation (prevent_save_move = false, so saves are archived)
        let archive_dir = game_dir.join(".phoenix_archive");
        let old_archive_dir = game_dir.join(".phoenix_archive_old");
        archive_current_installation(&game_dir, &archive_dir, &old_archive_dir, false)
            .await
            .unwrap();

        // Verify archive contains the files
        assert!(archive_dir.join("cataclysm-tiles.exe").exists());
        assert!(archive_dir.join("save").join("test_world.sav").exists());
        assert!(archive_dir.join("config").join("options.json").exists());
        assert!(archive_dir.join("config").join("debug.log").exists());
        assert!(archive_dir
            .join("data")
            .join("mods")
            .join("my_custom_mod")
            .join("modinfo.json")
            .exists());

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
        restore_user_directories_smart(&archive_dir, &game_dir, false)
            .await
            .unwrap();

        // Verify saves are restored
        assert!(game_dir.join("save").join("test_world.sav").exists());
        let save_content =
            fs::read_to_string(game_dir.join("save").join("test_world.sav")).unwrap();
        assert_eq!(save_content, "save data");

        // Verify config is restored (options.json should exist)
        assert!(game_dir.join("config").join("options.json").exists());
        // Verify debug.log is NOT restored (filtered out)
        assert!(!game_dir.join("config").join("debug.log").exists());

        // Verify custom mod is restored
        assert!(game_dir
            .join("data")
            .join("mods")
            .join("my_custom_mod")
            .join("modinfo.json")
            .exists());

        // Verify archive still exists (for rollback)
        assert!(archive_dir.join("save").join("test_world.sav").exists());
    }

    #[tokio::test]
    async fn test_restore_with_prevent_save_move() {
        let temp_dir = TempDir::new().unwrap();
        let previous_dir = temp_dir.path().join(".phoenix_archive");
        let game_dir = temp_dir.path().join("game");

        // Create save in previous version
        let save_dir = previous_dir.join("save");
        fs::create_dir_all(&save_dir).unwrap();
        fs::write(save_dir.join("world.sav"), b"save data").unwrap();

        // Create game dir
        fs::create_dir_all(&game_dir).unwrap();

        // Restore with prevent_save_move = true
        restore_user_directories_smart(&previous_dir, &game_dir, true)
            .await
            .unwrap();

        // Save should NOT be restored when prevent_save_move is true
        assert!(!game_dir.join("save").exists());
    }

    #[tokio::test]
    async fn test_archive_renames_old_archive() {
        let temp_dir = TempDir::new().unwrap();
        let game_dir = temp_dir.path().to_path_buf();

        // Create an old .phoenix_archive directory
        let archive_dir = game_dir.join(".phoenix_archive");
        let old_archive_dir = game_dir.join(".phoenix_archive_old");
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(archive_dir.join("old_file.txt"), b"old data").unwrap();

        // Create current game files
        fs::write(game_dir.join("game.exe"), b"game").unwrap();

        // Archive should rename old .phoenix_archive to old_archive_dir
        archive_current_installation(&game_dir, &archive_dir, &old_archive_dir, false)
            .await
            .unwrap();

        // Old file should be in old_archive_dir (renamed, not deleted)
        assert!(old_archive_dir.join("old_file.txt").exists());

        // New .phoenix_archive should have current game file
        assert!(archive_dir.join("game.exe").exists());
        assert!(!archive_dir.join("old_file.txt").exists());
    }

    #[tokio::test]
    async fn test_extract_tar_gz_strips_wrapper_and_preserves_mode() {
        use flate2::write::GzEncoder;
        use flate2::Compression;

        let temp_dir = TempDir::new().unwrap();
        let archive_path = temp_dir.path().join("release.tar.gz");
        let dest = temp_dir.path().join("game");
        fs::create_dir_all(&dest).unwrap();

        // Build a tarball that wraps everything in a top-level directory,
        // mirroring CDDA's Linux release layout.
        {
            let tar_gz = fs::File::create(&archive_path).unwrap();
            let enc = GzEncoder::new(tar_gz, Compression::default());
            let mut builder = tar::Builder::new(enc);

            // Executable binary entry (mode 0755).
            let exe_contents = b"#!/bin/true\n";
            let mut header = tar::Header::new_gnu();
            header.set_size(exe_contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    "cataclysmdda-0.J/cataclysm-tiles",
                    &exe_contents[..],
                )
                .unwrap();

            // A nested data file (mode 0644).
            let data_contents = b"{}";
            let mut header = tar::Header::new_gnu();
            header.set_size(data_contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(
                    &mut header,
                    "cataclysmdda-0.J/data/json/test.json",
                    &data_contents[..],
                )
                .unwrap();

            builder.into_inner().unwrap().finish().unwrap();
        }

        let (progress_tx, _progress_rx) = watch::channel(UpdateProgress::default());
        let extracted = extract_tar_gz(&archive_path, &dest, progress_tx)
            .await
            .unwrap();
        assert_eq!(extracted, 2);

        // Wrapper directory should be stripped: files land directly in dest.
        let exe_path = dest.join("cataclysm-tiles");
        let data_path = dest.join("data").join("json").join("test.json");
        assert!(exe_path.exists(), "binary should be extracted without wrapper dir");
        assert!(data_path.exists(), "nested data file should be extracted");
        assert!(
            !dest.join("cataclysmdda-0.J").exists(),
            "wrapper directory should not be present"
        );

        // Executable bit should be preserved on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&exe_path).unwrap().permissions().mode();
            assert_ne!(mode & 0o111, 0, "executable bit should be preserved");
        }
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
        assert_eq!(
            fs::read_to_string(dst.join("file1.txt")).unwrap(),
            "content1"
        );
        assert_eq!(
            fs::read_to_string(dst.join("subdir").join("file2.txt")).unwrap(),
            "content2"
        );

        // Verify source still exists
        assert!(src.join("file1.txt").exists());
    }
}
