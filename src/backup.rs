//! Backup functionality for creating and restoring save backups.
//!
//! This module handles:
//! - Creating ZIP backups of the save directory
//! - Listing existing backups with metadata
//! - Restoring backups with optional pre-restore backup
//! - Automatic backups before launch, after end, and before updates
//! - Backup retention enforcement

use chrono::{DateTime, Local};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::sync::watch;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::util::format_size;

/// Files that indicate a world directory
const WORLD_FILES: &[&str] = &["master.gsav", "worldoptions.json", "worldoptions.txt"];

/// Errors that can occur during backup operations
#[derive(Error, Debug)]
pub enum BackupError {
    #[error("Save directory not found: {0}")]
    SaveDirNotFound(PathBuf),

    #[error("Backup not found: {0}")]
    BackupNotFound(String),

    #[error("Invalid backup name: {0}")]
    InvalidName(String),

    #[error("Failed to create backup: {0}")]
    CreateFailed(String),

    #[error("No saves to backup")]
    NoSaves,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Task cancelled")]
    Cancelled,
}

/// Metadata about a backup file
#[derive(Debug, Clone)]
pub struct BackupInfo {
    /// Name of the backup (without .zip extension)
    pub name: String,
    /// Full path to the backup file
    pub path: PathBuf,
    /// File size in bytes (compressed)
    pub compressed_size: u64,
    /// Total uncompressed size of all files
    pub uncompressed_size: u64,
    /// Number of world directories found
    pub worlds_count: u32,
    /// Number of character save files found
    pub characters_count: u32,
    /// Last modified time
    pub modified: DateTime<Local>,
    /// Whether this is an automatic backup
    pub is_auto: bool,
}

impl BackupInfo {
    /// Calculate compression ratio as a percentage (0-100)
    pub fn compression_ratio(&self) -> f32 {
        if self.uncompressed_size == 0 {
            0.0
        } else {
            (1.0 - (self.compressed_size as f64 / self.uncompressed_size as f64)) as f32 * 100.0
        }
    }

    /// Format compressed size for display
    pub fn compressed_size_display(&self) -> String {
        format_size(self.compressed_size)
    }

    /// Format uncompressed size for display
    pub fn uncompressed_size_display(&self) -> String {
        format_size(self.uncompressed_size)
    }
}

/// Type of automatic backup
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoBackupType {
    BeforeUpdate,
}

impl AutoBackupType {
    /// Get the prefix used for automatic backup names
    pub fn prefix(&self) -> &'static str {
        match self {
            Self::BeforeUpdate => "auto_before_update",
        }
    }
}

/// Current phase of backup/restore operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackupPhase {
    #[default]
    Idle,
    Scanning,
    Compressing,
    Extracting,
    Cleaning,
    Complete,
    Failed,
}

impl BackupPhase {
    /// Get a human-readable description of the current phase
    pub fn description(&self) -> &'static str {
        match self {
            BackupPhase::Idle => "Ready",
            BackupPhase::Scanning => "Scanning files...",
            BackupPhase::Compressing => "Compressing saves...",
            BackupPhase::Extracting => "Extracting backup...",
            BackupPhase::Cleaning => "Cleaning up...",
            BackupPhase::Complete => "Complete!",
            BackupPhase::Failed => "Failed",
        }
    }
}

/// Progress information for backup/restore operations
#[derive(Debug, Clone, Default)]
pub struct BackupProgress {
    pub phase: BackupPhase,
    pub files_processed: usize,
    pub total_files: usize,
    pub current_file: String,
}

impl BackupProgress {
    /// Calculate progress as a fraction (0.0 - 1.0)
    pub fn fraction(&self) -> f32 {
        match self.phase {
            BackupPhase::Compressing | BackupPhase::Extracting => {
                if self.total_files == 0 {
                    0.0
                } else {
                    self.files_processed as f32 / self.total_files as f32
                }
            }
            BackupPhase::Complete => 1.0,
            _ => 0.0,
        }
    }
}

/// Get the backup directory for a game installation
pub fn backup_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("save_backups")
}

/// List all backups in the backup directory
pub async fn list_backups(game_dir: &Path) -> Result<Vec<BackupInfo>, BackupError> {
    let backup_path = backup_dir(game_dir);

    if !backup_path.exists() {
        return Ok(Vec::new());
    }

    let backup_path_clone = backup_path.clone();

    tokio::task::spawn_blocking(move || {
        let mut backups = Vec::new();

        for entry in fs::read_dir(&backup_path_clone)? {
            let entry = entry?;
            let path = entry.path();

            // Only process .zip files
            if path.extension().map_or(false, |e| e == "zip") {
                if let Some(info) = read_backup_info(&path) {
                    backups.push(info);
                }
            }
        }

        // Sort by modified date, newest first
        backups.sort_by(|a, b| b.modified.cmp(&a.modified));

        Ok(backups)
    })
    .await
    .map_err(|_| BackupError::Cancelled)?
}

/// Read metadata from a backup file
fn read_backup_info(path: &Path) -> Option<BackupInfo> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let modified: DateTime<Local> = metadata.modified().ok()?.into();
    let compressed_size = metadata.len();

    let mut archive = ZipArchive::new(file).ok()?;
    let mut uncompressed_size = 0u64;
    let mut worlds_count = 0u32;
    let mut characters_count = 0u32;

    // Scan archive for metadata
    for i in 0..archive.len() {
        if let Ok(file) = archive.by_index(i) {
            uncompressed_size += file.size();

            let name = file.name();

            // Count worlds: directories containing world marker files
            for world_file in WORLD_FILES {
                if name.ends_with(world_file) {
                    worlds_count += 1;
                    break;
                }
            }

            // Count characters: .sav files at depth 3 (save/world/character.sav)
            if name.ends_with(".sav") {
                let depth = name.matches('/').count();
                if depth == 2 {
                    characters_count += 1;
                }
            }
        }
    }

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let is_auto = name.starts_with("auto_");

    Some(BackupInfo {
        name,
        path: path.to_path_buf(),
        compressed_size,
        uncompressed_size,
        worlds_count,
        characters_count,
        modified,
        is_auto,
    })
}

/// Validate a backup name
fn validate_backup_name(name: &str) -> Result<(), BackupError> {
    if name.is_empty() {
        return Err(BackupError::InvalidName("Name cannot be empty".to_string()));
    }

    if name.len() > 100 {
        return Err(BackupError::InvalidName("Name too long (max 100 chars)".to_string()));
    }

    // Allow alphanumeric, underscore, dash, and space
    for c in name.chars() {
        if !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ' {
            return Err(BackupError::InvalidName(format!(
                "Invalid character '{}'. Only letters, numbers, underscore, dash, and space allowed.",
                c
            )));
        }
    }

    Ok(())
}

/// Create a backup of the save directory
pub async fn create_backup(
    game_dir: &Path,
    name: &str,
    compression_level: u8,
    progress_tx: watch::Sender<BackupProgress>,
) -> Result<BackupInfo, BackupError> {
    validate_backup_name(name)?;

    let save_dir = game_dir.join("save");
    if !save_dir.exists() {
        return Err(BackupError::SaveDirNotFound(save_dir));
    }

    // Check if save directory is empty
    if fs::read_dir(&save_dir)?.next().is_none() {
        return Err(BackupError::NoSaves);
    }

    // Ensure backup directory exists
    let backup_path = backup_dir(game_dir);
    fs::create_dir_all(&backup_path)?;

    // Generate backup file path
    let backup_file = backup_path.join(format!("{}.zip", name));

    // Check if backup already exists
    if backup_file.exists() {
        return Err(BackupError::InvalidName(format!(
            "Backup '{}' already exists",
            name
        )));
    }

    let game_dir = game_dir.to_path_buf();
    let name = name.to_string();

    tokio::task::spawn_blocking(move || {
        create_backup_sync(&game_dir, &name, compression_level, progress_tx)
    })
    .await
    .map_err(|_| BackupError::Cancelled)?
}

/// Synchronous backup creation (runs in spawn_blocking)
fn create_backup_sync(
    game_dir: &Path,
    name: &str,
    compression_level: u8,
    progress_tx: watch::Sender<BackupProgress>,
) -> Result<BackupInfo, BackupError> {
    let save_dir = game_dir.join("save");
    let backup_path = backup_dir(game_dir);
    let backup_file = backup_path.join(format!("{}.zip", name));

    // Phase 1: Scan files
    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Scanning,
        ..Default::default()
    });

    let mut files_to_backup: Vec<(PathBuf, String)> = Vec::new();

    for entry in WalkDir::new(&save_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let path = entry.path().to_path_buf();
            let relative = path
                .strip_prefix(game_dir)
                .map_err(|e| BackupError::CreateFailed(e.to_string()))?;
            let relative_str = relative.to_string_lossy().replace('\\', "/");

            files_to_backup.push((path, relative_str));
        }
    }

    let total_files = files_to_backup.len();

    if total_files == 0 {
        return Err(BackupError::NoSaves);
    }

    // Phase 2: Create ZIP
    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Compressing,
        total_files,
        ..Default::default()
    });

    let file = File::create(&backup_file)?;
    let mut zip = ZipWriter::new(file);

    // Configure compression
    let compression = if compression_level == 0 {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };

    let options = SimpleFileOptions::default()
        .compression_method(compression)
        .compression_level(Some(compression_level.min(9) as i64));

    for (i, (path, relative)) in files_to_backup.iter().enumerate() {
        // Update progress
        let _ = progress_tx.send(BackupProgress {
            phase: BackupPhase::Compressing,
            files_processed: i,
            total_files,
            current_file: relative.clone(),
        });

        // Read file content
        let mut file_content = Vec::new();
        let mut file = File::open(path)?;
        file.read_to_end(&mut file_content)?;

        // Add to ZIP
        zip.start_file(relative, options)?;
        zip.write_all(&file_content)?;
    }

    zip.finish()?;

    // Complete
    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Complete,
        files_processed: total_files,
        total_files,
        current_file: String::new(),
    });

    // Read back the info
    read_backup_info(&backup_file).ok_or_else(|| {
        BackupError::CreateFailed("Failed to read created backup info".to_string())
    })
}

/// Delete a backup
pub async fn delete_backup(game_dir: &Path, backup_name: &str) -> Result<(), BackupError> {
    let backup_path = backup_dir(game_dir);
    let backup_file = backup_path.join(format!("{}.zip", backup_name));

    if !backup_file.exists() {
        return Err(BackupError::BackupNotFound(backup_name.to_string()));
    }

    tokio::fs::remove_file(&backup_file).await?;

    tracing::info!("Deleted backup: {}", backup_name);
    Ok(())
}

/// Restore a backup
pub async fn restore_backup(
    game_dir: &Path,
    backup_name: &str,
    backup_current_first: bool,
    compression_level: u8,
    progress_tx: watch::Sender<BackupProgress>,
) -> Result<(), BackupError> {
    let backup_path = backup_dir(game_dir);
    let backup_file = backup_path.join(format!("{}.zip", backup_name));

    if !backup_file.exists() {
        return Err(BackupError::BackupNotFound(backup_name.to_string()));
    }

    let save_dir = game_dir.join("save");

    // Optionally backup current saves first
    if backup_current_first && save_dir.exists() {
        // Check if there are saves to backup
        if fs::read_dir(&save_dir)?.next().is_some() {
            let pre_restore_name = generate_unique_name(&backup_path, "before_last_restore");
            tracing::info!("Backing up current saves as: {}", pre_restore_name);

            create_backup(game_dir, &pre_restore_name, compression_level, progress_tx.clone())
                .await?;
        }
    }

    let game_dir = game_dir.to_path_buf();

    tokio::task::spawn_blocking(move || {
        restore_backup_sync(&game_dir, &backup_file, progress_tx)
    })
    .await
    .map_err(|_| BackupError::Cancelled)?
}

/// Synchronous backup restoration (runs in spawn_blocking)
fn restore_backup_sync(
    game_dir: &Path,
    backup_file: &Path,
    progress_tx: watch::Sender<BackupProgress>,
) -> Result<(), BackupError> {
    let save_dir = game_dir.join("save");

    // Phase 1: Move current saves to temp
    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Cleaning,
        ..Default::default()
    });

    let temp_save = game_dir.join(format!("save-{:x}", rand_u64()));
    if save_dir.exists() {
        fs::rename(&save_dir, &temp_save)?;
    }

    // Phase 2: Extract backup
    let file = File::open(backup_file)?;
    let mut archive = ZipArchive::new(file)?;
    let total_files = archive.len();

    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Extracting,
        total_files,
        ..Default::default()
    });

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        // Update progress
        let _ = progress_tx.send(BackupProgress {
            phase: BackupPhase::Extracting,
            files_processed: i,
            total_files,
            current_file: name.clone(),
            ..Default::default()
        });

        let out_path = game_dir.join(&name);

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut outfile = File::create(&out_path)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    // Phase 3: Delete old saves
    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Cleaning,
        files_processed: total_files,
        total_files,
        ..Default::default()
    });

    if temp_save.exists() {
        // Fire-and-forget deletion in background
        std::thread::spawn(move || {
            if let Err(e) = remove_dir_all::remove_dir_all(&temp_save) {
                tracing::warn!("Failed to clean up old saves: {}", e);
            }
        });
    }

    // Complete
    let _ = progress_tx.send(BackupProgress {
        phase: BackupPhase::Complete,
        files_processed: total_files,
        total_files,
        ..Default::default()
    });

    tracing::info!("Restored backup: {:?}", backup_file);
    Ok(())
}

/// Generate a simple random u64 for temp directory names
fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_nanos() as u64
}

/// Create an automatic backup with deduplication
pub async fn create_auto_backup(
    game_dir: &Path,
    backup_type: AutoBackupType,
    version_tag: Option<&str>,
    compression_level: u8,
    max_count: u32,
    progress_tx: watch::Sender<BackupProgress>,
) -> Result<Option<BackupInfo>, BackupError> {
    let save_dir = game_dir.join("save");

    // Check if there are saves to backup
    if !save_dir.exists() {
        tracing::info!("No save directory, skipping auto-backup");
        return Ok(None);
    }

    if fs::read_dir(&save_dir)?.next().is_none() {
        tracing::info!("Save directory empty, skipping auto-backup");
        return Ok(None);
    }

    // Generate unique name
    let backup_path = backup_dir(game_dir);
    fs::create_dir_all(&backup_path)?;

    let base_name = if let Some(tag) = version_tag {
        format!("{}_{}", backup_type.prefix(), tag.replace(['/', '\\', ':'], "_"))
    } else {
        backup_type.prefix().to_string()
    };

    let name = generate_unique_name(&backup_path, &base_name);

    tracing::info!("Creating auto-backup: {}", name);

    // Create the backup
    let info = create_backup(game_dir, &name, compression_level, progress_tx).await?;

    // Enforce retention
    enforce_retention(game_dir, max_count).await?;

    Ok(Some(info))
}

/// Generate a unique backup name by appending numbers if needed
fn generate_unique_name(backup_path: &Path, base_name: &str) -> String {
    let mut name = base_name.to_string();
    let mut counter = 2;

    while backup_path.join(format!("{}.zip", name)).exists() {
        name = format!("{}{}", base_name, counter);
        counter += 1;
    }

    name
}

/// Enforce backup retention policy (delete oldest auto-backups)
pub async fn enforce_retention(game_dir: &Path, max_count: u32) -> Result<usize, BackupError> {
    if max_count == 0 {
        return Ok(0);
    }

    let mut backups = list_backups(game_dir).await?;

    // Only consider auto-backups for retention
    backups.retain(|b| b.is_auto);

    if backups.len() <= max_count as usize {
        return Ok(0);
    }

    // Sort by date, oldest first (we'll delete from the front)
    backups.sort_by(|a, b| a.modified.cmp(&b.modified));

    let to_delete = backups.len() - max_count as usize;
    let mut deleted = 0;

    for backup in backups.into_iter().take(to_delete) {
        if let Err(e) = tokio::fs::remove_file(&backup.path).await {
            tracing::warn!("Failed to delete old backup {}: {}", backup.name, e);
        } else {
            tracing::info!("Deleted old auto-backup: {}", backup.name);
            deleted += 1;
        }
    }

    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_backup_name() {
        assert!(validate_backup_name("my_backup").is_ok());
        assert!(validate_backup_name("my-backup").is_ok());
        assert!(validate_backup_name("my backup").is_ok());
        assert!(validate_backup_name("Backup123").is_ok());
        assert!(validate_backup_name("").is_err());
        assert!(validate_backup_name("bad/name").is_err());
        assert!(validate_backup_name("bad:name").is_err());
        assert!(validate_backup_name("a".repeat(101).as_str()).is_err());
    }

    #[test]
    fn test_auto_backup_type_prefix() {
        assert_eq!(AutoBackupType::BeforeUpdate.prefix(), "auto_before_update");
    }

    #[test]
    fn test_compression_ratio() {
        let info = BackupInfo {
            name: "test".to_string(),
            path: PathBuf::new(),
            compressed_size: 40,
            uncompressed_size: 100,
            worlds_count: 0,
            characters_count: 0,
            modified: Local::now(),
            is_auto: false,
        };
        assert!((info.compression_ratio() - 60.0).abs() < 0.1);
    }
}
