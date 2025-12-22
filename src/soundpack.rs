//! Soundpack management for CDDA.
//!
//! This module handles:
//! - Scanning installed soundpacks from the game directory
//! - Loading the embedded soundpack repository
//! - Downloading and installing soundpacks from the repository
//! - Enabling/disabling soundpacks via file rename
//! - Extracting archives (ZIP, RAR, 7z)

use futures::StreamExt;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

/// Embedded soundpacks repository JSON
const SOUNDPACKS_JSON: &str = include_str!("../assets/soundpacks.json");

/// Information about an installed soundpack
#[derive(Debug, Clone)]
pub struct InstalledSoundpack {
    /// Internal name from soundpack.txt NAME field
    pub name: String,
    /// Display name from soundpack.txt VIEW field
    pub view_name: String,
    /// Path to the soundpack directory
    pub path: PathBuf,
    /// Whether enabled (soundpack.txt exists vs soundpack.txt.disabled)
    pub enabled: bool,
    /// Size in bytes
    pub size: u64,
}

/// Repository soundpack entry (from embedded JSON)
#[derive(Debug, Clone, Deserialize)]
pub struct RepoSoundpack {
    /// Download type: "direct_download" or "browser_download"
    #[serde(rename = "type")]
    pub download_type: String,
    /// Display name (shown in UI)
    pub viewname: String,
    /// Internal name (matches soundpack.txt NAME)
    pub name: String,
    /// Download URL
    pub url: String,
    /// Homepage URL
    pub homepage: String,
    /// Optional pre-known size in bytes
    pub size: Option<u64>,
}

/// Current phase of soundpack operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SoundpackPhase {
    #[default]
    Idle,
    Downloading,
    Extracting,
    Installing,
    Complete,
    Failed,
}

impl SoundpackPhase {
    /// Get a human-readable description of the current phase
    pub fn description(&self) -> &'static str {
        match self {
            SoundpackPhase::Idle => "Ready",
            SoundpackPhase::Downloading => "Downloading soundpack...",
            SoundpackPhase::Extracting => "Extracting archive...",
            SoundpackPhase::Installing => "Installing soundpack...",
            SoundpackPhase::Complete => "Installation complete!",
            SoundpackPhase::Failed => "Installation failed",
        }
    }
}

/// Progress information for soundpack operations
#[derive(Debug, Clone, Default)]
pub struct SoundpackProgress {
    pub phase: SoundpackPhase,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub speed: u64,
    pub files_extracted: usize,
    pub total_files: usize,
    pub current_file: String,
    pub error: Option<String>,
}

impl SoundpackProgress {
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
}

/// Archive format for extraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    Rar,
    SevenZ,
}

/// Errors that can occur during soundpack operations
#[derive(Error, Debug)]
pub enum SoundpackError {
    #[error("Soundpack not found: {0}")]
    SoundpackNotFound(String),

    #[error("Invalid archive format: {0}")]
    InvalidArchiveFormat(String),

    #[error("Archive extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("No soundpack.txt found in archive")]
    NoSoundpackTxt,

    #[error("Soundpack already exists: {0}")]
    AlreadyExists(String),

    #[error("Download failed: {0}")]
    DownloadFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Task cancelled")]
    Cancelled,
}

// ============================================================================
// Repository Loading
// ============================================================================

/// Load the soundpacks repository from embedded JSON
pub fn load_repository() -> Vec<RepoSoundpack> {
    serde_json::from_str(SOUNDPACKS_JSON).unwrap_or_else(|e| {
        tracing::error!("Failed to parse soundpacks.json: {}", e);
        Vec::new()
    })
}

// ============================================================================
// Soundpack Scanning
// ============================================================================

/// Get the soundpacks directory for a game installation
pub fn soundpacks_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("data").join("sound")
}

/// Parse soundpack.txt to extract NAME and VIEW fields
///
/// Returns (name, view_name, enabled) if successful
pub fn parse_soundpack_txt(soundpack_dir: &Path) -> Option<(String, String, bool)> {
    let normal = soundpack_dir.join("soundpack.txt");
    let disabled = soundpack_dir.join("soundpack.txt.disabled");

    let (file_path, enabled) = if normal.exists() {
        (normal, true)
    } else if disabled.exists() {
        (disabled, false)
    } else {
        return None;
    };

    let content = std::fs::read(&file_path).ok()?;
    // Use lossy UTF-8 conversion (original uses latin1)
    let text = String::from_utf8_lossy(&content);

    let mut name = None;
    let mut view = None;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("NAME") {
            if let Some(rest) = line.strip_prefix("NAME") {
                let value = rest.trim().replace(',', "");
                if !value.is_empty() {
                    name = Some(value);
                }
            }
        } else if line.starts_with("VIEW") {
            if let Some(rest) = line.strip_prefix("VIEW") {
                let value = rest.trim().to_string();
                if !value.is_empty() {
                    view = Some(value);
                }
            }
        }

        // Stop early if we found both
        if name.is_some() && view.is_some() {
            break;
        }
    }

    let name = name?;
    let view = view.unwrap_or_else(|| name.clone());

    Some((name, view, enabled))
}

/// Calculate directory size recursively
fn calculate_dir_size(path: &Path) -> u64 {
    let mut size = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                size += calculate_dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                size += meta.len();
            }
        }
    }
    size
}

/// Scan soundpacks directory and return list of installed soundpacks
pub async fn list_installed_soundpacks(
    game_dir: &Path,
) -> Result<Vec<InstalledSoundpack>, SoundpackError> {
    let sound_dir = soundpacks_dir(game_dir);
    if !sound_dir.exists() {
        return Ok(Vec::new());
    }

    let sound_dir_owned = sound_dir.clone();

    // Run in spawn_blocking since it's filesystem I/O
    tokio::task::spawn_blocking(move || {
        let mut soundpacks = Vec::new();

        let entries = std::fs::read_dir(&sound_dir_owned)?;
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some((name, view_name, enabled)) = parse_soundpack_txt(&entry.path()) {
                    let size = calculate_dir_size(&entry.path());
                    soundpacks.push(InstalledSoundpack {
                        name,
                        view_name,
                        path: entry.path(),
                        enabled,
                        size,
                    });
                }
            }
        }

        // Sort by view name
        soundpacks.sort_by(|a, b| a.view_name.to_lowercase().cmp(&b.view_name.to_lowercase()));

        Ok(soundpacks)
    })
    .await
    .map_err(|_| SoundpackError::Cancelled)?
}

// ============================================================================
// Enable/Disable/Delete
// ============================================================================

/// Enable or disable a soundpack by renaming soundpack.txt
pub async fn set_soundpack_enabled(
    soundpack_path: &Path,
    enabled: bool,
) -> Result<(), SoundpackError> {
    let txt_file = soundpack_path.join("soundpack.txt");
    let disabled_file = soundpack_path.join("soundpack.txt.disabled");

    if enabled {
        if disabled_file.exists() {
            tokio::fs::rename(&disabled_file, &txt_file).await?;
            tracing::info!("Enabled soundpack: {:?}", soundpack_path);
        }
    } else if txt_file.exists() {
        tokio::fs::rename(&txt_file, &disabled_file).await?;
        tracing::info!("Disabled soundpack: {:?}", soundpack_path);
    }

    Ok(())
}

/// Delete a soundpack directory
pub async fn delete_soundpack(soundpack_path: PathBuf) -> Result<(), SoundpackError> {
    if !soundpack_path.exists() {
        return Err(SoundpackError::SoundpackNotFound(
            soundpack_path.display().to_string(),
        ));
    }

    // Use remove_dir_all crate for faster deletion on Windows
    tokio::task::spawn_blocking(move || {
        remove_dir_all::remove_dir_all(&soundpack_path)
            .map_err(|e| SoundpackError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))
    })
    .await
    .map_err(|_| SoundpackError::Cancelled)??;

    Ok(())
}

// ============================================================================
// Archive Extraction
// ============================================================================

/// Detect archive format from file extension
pub fn detect_archive_format(path: &Path) -> Option<ArchiveFormat> {
    let extension = path.extension()?.to_str()?.to_lowercase();
    match extension.as_str() {
        "zip" => Some(ArchiveFormat::Zip),
        "rar" => Some(ArchiveFormat::Rar),
        "7z" => Some(ArchiveFormat::SevenZ),
        _ => None,
    }
}

/// Extract archive to destination directory
pub async fn extract_archive(
    archive_path: PathBuf,
    dest_dir: PathBuf,
    progress_tx: watch::Sender<SoundpackProgress>,
) -> Result<(), SoundpackError> {
    let format = detect_archive_format(&archive_path).ok_or_else(|| {
        SoundpackError::InvalidArchiveFormat(
            archive_path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| "none".to_string()),
        )
    })?;

    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Extracting,
        ..Default::default()
    });

    tokio::task::spawn_blocking(move || match format {
        ArchiveFormat::Zip => extract_zip_archive(&archive_path, &dest_dir, &progress_tx),
        ArchiveFormat::Rar => extract_rar_archive(&archive_path, &dest_dir, &progress_tx),
        ArchiveFormat::SevenZ => extract_7z_archive(&archive_path, &dest_dir, &progress_tx),
    })
    .await
    .map_err(|_| SoundpackError::Cancelled)?
}

/// Extract ZIP archive
fn extract_zip_archive(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: &watch::Sender<SoundpackProgress>,
) -> Result<(), SoundpackError> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| SoundpackError::ExtractionFailed(e.to_string()))?;

    let total = archive.len();

    for i in 0..total {
        let mut file = archive
            .by_index(i)
            .map_err(|e| SoundpackError::ExtractionFailed(e.to_string()))?;

        let outpath = match file.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // Update progress every 50 files
        if i % 50 == 0 || i == total - 1 {
            let _ = progress_tx.send(SoundpackProgress {
                phase: SoundpackPhase::Extracting,
                files_extracted: i + 1,
                total_files: total,
                current_file: file.name().to_string(),
                ..Default::default()
            });
        }
    }

    Ok(())
}

/// Extract RAR archive using unrar crate
fn extract_rar_archive(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: &watch::Sender<SoundpackProgress>,
) -> Result<(), SoundpackError> {
    use unrar::Archive;

    let archive = Archive::new(archive_path)
        .open_for_processing()
        .map_err(|e| SoundpackError::ExtractionFailed(format!("Failed to open RAR: {}", e)))?;

    let mut extracted = 0;

    let mut archive = archive;
    while let Some(header) = archive
        .read_header()
        .map_err(|e| SoundpackError::ExtractionFailed(format!("RAR header error: {}", e)))?
    {
        let entry_name = header.entry().filename.to_string_lossy().to_string();

        archive = header
            .extract_to(dest_dir)
            .map_err(|e| SoundpackError::ExtractionFailed(format!("RAR extract error: {}", e)))?;

        extracted += 1;

        if extracted % 50 == 0 {
            let _ = progress_tx.send(SoundpackProgress {
                phase: SoundpackPhase::Extracting,
                files_extracted: extracted,
                current_file: entry_name,
                ..Default::default()
            });
        }
    }

    tracing::info!("Extracted {} files from RAR archive", extracted);
    Ok(())
}

/// Extract 7z archive using sevenz-rust crate
fn extract_7z_archive(
    archive_path: &Path,
    dest_dir: &Path,
    progress_tx: &watch::Sender<SoundpackProgress>,
) -> Result<(), SoundpackError> {
    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Extracting,
        current_file: "Decompressing 7z archive...".to_string(),
        ..Default::default()
    });

    sevenz_rust::decompress_file(archive_path, dest_dir)
        .map_err(|e| SoundpackError::ExtractionFailed(format!("7z extraction failed: {}", e)))?;

    tracing::info!("Extracted 7z archive to {:?}", dest_dir);
    Ok(())
}

// ============================================================================
// Download and Install
// ============================================================================

/// Find soundpack.txt in extracted directory (may be nested)
pub fn find_soundpack_dir(extract_dir: &Path) -> Option<PathBuf> {
    for entry in walkdir::WalkDir::new(extract_dir)
        .min_depth(1)
        .max_depth(5)
    {
        if let Ok(entry) = entry {
            if entry.file_name() == "soundpack.txt" || entry.file_name() == "soundpack.txt.disabled"
            {
                return entry.path().parent().map(|p| p.to_path_buf());
            }
        }
    }
    None
}

/// Extract filename from URL
fn extract_filename_from_url(url: &str) -> String {
    url.split('/')
        .last()
        .and_then(|s| s.split('?').next())
        .unwrap_or("soundpack.zip")
        .to_string()
}

/// Download a file with progress tracking
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest_path: &Path,
    progress_tx: &watch::Sender<SoundpackProgress>,
    known_size: Option<u64>,
) -> Result<u64, SoundpackError> {
    let download_start = Instant::now();

    // Start the download request
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| SoundpackError::DownloadFailed(e.to_string()))?;

    if !response.status().is_success() {
        return Err(SoundpackError::DownloadFailed(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let total_size = response.content_length().or(known_size).unwrap_or(0);

    // Create parent directory if needed
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Download to a temporary .part file
    let temp_path = dest_path.with_extension("part");
    let mut file = tokio::fs::File::create(&temp_path).await?;

    // Stream the response body to disk
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_progress_time = Instant::now();
    let mut last_downloaded: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| SoundpackError::DownloadFailed(e.to_string()))?;

        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        // Update progress every 100ms
        let now = Instant::now();
        let elapsed = now.duration_since(last_progress_time);
        if elapsed >= Duration::from_millis(100) {
            let bytes_since_last = downloaded - last_downloaded;
            let current_speed = (bytes_since_last as f64 / elapsed.as_secs_f64()) as u64;

            let _ = progress_tx.send(SoundpackProgress {
                phase: SoundpackPhase::Downloading,
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
    file.sync_all().await?;
    drop(file);

    // Rename temp file to final destination
    tokio::fs::rename(&temp_path, dest_path).await?;

    let elapsed = download_start.elapsed().as_secs_f32();
    let speed_mbps = (downloaded as f32 / 1_000_000.0) / elapsed;
    tracing::info!(
        "Download complete: {:.1} MB in {:.1}s ({:.1} MB/s)",
        downloaded as f32 / 1_000_000.0,
        elapsed,
        speed_mbps
    );

    Ok(downloaded)
}

/// Install soundpack from extracted directory to game's sound folder
pub async fn install_extracted_soundpack(
    extract_dir: &Path,
    game_dir: &Path,
) -> Result<InstalledSoundpack, SoundpackError> {
    // Find the actual soundpack directory (may be nested)
    let soundpack_source =
        find_soundpack_dir(extract_dir).ok_or(SoundpackError::NoSoundpackTxt)?;

    // Parse metadata
    let (name, view_name, enabled) =
        parse_soundpack_txt(&soundpack_source).ok_or(SoundpackError::NoSoundpackTxt)?;

    // Determine destination - use the source directory name
    let soundpack_name = soundpack_source
        .file_name()
        .ok_or_else(|| SoundpackError::ExtractionFailed("Invalid path".to_string()))?;

    let dest = soundpacks_dir(game_dir).join(soundpack_name);

    // Check if already exists
    if dest.exists() {
        return Err(SoundpackError::AlreadyExists(
            soundpack_name.to_string_lossy().to_string(),
        ));
    }

    // Ensure destination parent exists
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Copy to final location (can't rename across filesystems)
    let src = soundpack_source.clone();
    let dst = dest.clone();
    tokio::task::spawn_blocking(move || copy_dir_sync(&src, &dst))
        .await
        .map_err(|_| SoundpackError::Cancelled)??;

    // Calculate size
    let dest_for_size = dest.clone();
    let size = tokio::task::spawn_blocking(move || calculate_dir_size(&dest_for_size))
        .await
        .unwrap_or(0);

    tracing::info!("Installed soundpack '{}' to {:?}", name, dest);

    Ok(InstalledSoundpack {
        name,
        view_name,
        path: dest,
        enabled,
        size,
    })
}

/// Synchronous directory copy
fn copy_dir_sync(src: &Path, dst: &Path) -> Result<(), SoundpackError> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_sync(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Download and install a soundpack from the repository
pub async fn install_soundpack(
    client: reqwest::Client,
    repo_soundpack: RepoSoundpack,
    game_dir: PathBuf,
    progress_tx: watch::Sender<SoundpackProgress>,
) -> Result<InstalledSoundpack, SoundpackError> {
    // Create temp directory for download and extraction
    let temp_dir = tempfile::tempdir()?;

    let filename = extract_filename_from_url(&repo_soundpack.url);
    let download_path = temp_dir.path().join(&filename);

    // Phase 1: Download
    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Downloading,
        total_bytes: repo_soundpack.size.unwrap_or(0),
        ..Default::default()
    });

    download_file(
        &client,
        &repo_soundpack.url,
        &download_path,
        &progress_tx,
        repo_soundpack.size,
    )
    .await?;

    // Phase 2: Extract
    let extract_dir = temp_dir.path().join("extract");
    std::fs::create_dir_all(&extract_dir)?;

    extract_archive(download_path, extract_dir.clone(), progress_tx.clone()).await?;

    // Phase 3: Install
    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Installing,
        ..Default::default()
    });

    let installed = install_extracted_soundpack(&extract_dir, &game_dir).await?;

    // Complete
    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Complete,
        ..Default::default()
    });

    Ok(installed)
}

/// Install soundpack from a local archive file (for browser downloads)
pub async fn install_from_file(
    archive_path: PathBuf,
    game_dir: PathBuf,
    progress_tx: watch::Sender<SoundpackProgress>,
) -> Result<InstalledSoundpack, SoundpackError> {
    // Create temp directory for extraction
    let temp_dir = tempfile::tempdir()?;
    let extract_dir = temp_dir.path().join("extract");
    std::fs::create_dir_all(&extract_dir)?;

    // Extract
    extract_archive(archive_path, extract_dir.clone(), progress_tx.clone()).await?;

    // Install
    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Installing,
        ..Default::default()
    });

    let installed = install_extracted_soundpack(&extract_dir, &game_dir).await?;

    // Complete
    let _ = progress_tx.send(SoundpackProgress {
        phase: SoundpackPhase::Complete,
        ..Default::default()
    });

    Ok(installed)
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Format bytes as human-readable size
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Check if a soundpack with the given name is installed
pub fn is_soundpack_installed(installed: &[InstalledSoundpack], name: &str) -> bool {
    installed.iter().any(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_repository() {
        let repo = load_repository();
        assert!(!repo.is_empty(), "Repository should have soundpacks");

        // Check first soundpack has expected fields
        let first = &repo[0];
        assert!(!first.name.is_empty());
        assert!(!first.viewname.is_empty());
        assert!(!first.url.is_empty());
        assert!(!first.homepage.is_empty());
    }

    #[test]
    fn test_detect_archive_format() {
        assert_eq!(
            detect_archive_format(Path::new("test.zip")),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(
            detect_archive_format(Path::new("test.rar")),
            Some(ArchiveFormat::Rar)
        );
        assert_eq!(
            detect_archive_format(Path::new("test.7z")),
            Some(ArchiveFormat::SevenZ)
        );
        assert_eq!(detect_archive_format(Path::new("test.tar.gz")), None);
        assert_eq!(detect_archive_format(Path::new("no_extension")), None);
    }

    #[test]
    fn test_extract_filename_from_url() {
        assert_eq!(
            extract_filename_from_url("https://example.com/path/to/file.zip"),
            "file.zip"
        );
        assert_eq!(
            extract_filename_from_url("https://example.com/file.zip?dl=1"),
            "file.zip"
        );
        assert_eq!(
            extract_filename_from_url("https://dropbox.com/s/abc123/archive.zip?dl=1"),
            "archive.zip"
        );
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1572864), "1.5 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    #[test]
    fn test_soundpack_phase_description() {
        assert_eq!(SoundpackPhase::Idle.description(), "Ready");
        assert_eq!(
            SoundpackPhase::Downloading.description(),
            "Downloading soundpack..."
        );
        assert_eq!(
            SoundpackPhase::Extracting.description(),
            "Extracting archive..."
        );
        assert_eq!(
            SoundpackPhase::Installing.description(),
            "Installing soundpack..."
        );
        assert_eq!(
            SoundpackPhase::Complete.description(),
            "Installation complete!"
        );
        assert_eq!(SoundpackPhase::Failed.description(), "Installation failed");
    }

    #[test]
    fn test_progress_fraction() {
        let mut progress = SoundpackProgress::default();

        // Download progress
        progress.phase = SoundpackPhase::Downloading;
        progress.bytes_downloaded = 50;
        progress.total_bytes = 100;
        assert_eq!(progress.download_fraction(), 0.5);

        // Extract progress
        progress.phase = SoundpackPhase::Extracting;
        progress.files_extracted = 25;
        progress.total_files = 100;
        assert_eq!(progress.extract_fraction(), 0.25);

        // Zero total should return 0
        progress.total_bytes = 0;
        progress.total_files = 0;
        assert_eq!(progress.download_fraction(), 0.0);
        assert_eq!(progress.extract_fraction(), 0.0);
    }
}
