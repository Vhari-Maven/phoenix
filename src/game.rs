use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::db::{Database, VersionInfo};

/// Game executable names to look for
const GAME_EXECUTABLES: &[&str] = &["cataclysm-tiles.exe", "cataclysm.exe"];

/// Information about a detected game installation
#[derive(Debug, Clone)]
pub struct GameInfo {
    /// Path to the game executable
    pub executable: PathBuf,
    /// Detected version info
    pub version_info: Option<VersionInfo>,
    /// Size of save directory in bytes
    pub saves_size: u64,
}

impl GameInfo {
    /// Get a display-friendly version string
    pub fn version_display(&self) -> &str {
        self.version_info
            .as_ref()
            .map(|v| v.version.as_str())
            .unwrap_or("Unknown")
    }

    /// Check if this is a stable release
    pub fn is_stable(&self) -> bool {
        self.version_info
            .as_ref()
            .is_some_and(|v| v.stable)
    }
}

/// Detect game installation with optional database for version lookup
///
/// Optimized flow:
/// 1. Try VERSION.txt first (fast - experimental builds always have this)
/// 2. Only calculate SHA256 if VERSION.txt missing (needed for stable lookup)
/// 3. Use cached hash when possible to avoid expensive recalculation
pub fn detect_game_with_db(directory: &Path, db: Option<&Database>) -> Result<Option<GameInfo>> {
    // Look for game executable
    let executable = GAME_EXECUTABLES
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.exists());

    let Some(executable) = executable else {
        return Ok(None);
    };

    // Fast path: Try VERSION.txt first (experimental builds always have this)
    if let Some(version_info) = read_version_txt(directory) {
        // Calculate saves size
        let saves_dir = directory.join("save");
        let saves_size = if saves_dir.exists() {
            calculate_dir_size(&saves_dir).unwrap_or(0)
        } else {
            0
        };

        return Ok(Some(GameInfo {
            executable,
            version_info: Some(version_info),
            saves_size,
        }));
    }

    // Slow path: No VERSION.txt, need SHA256 for stable version lookup
    // Use cached hash if available to avoid expensive recalculation
    let sha256 = get_or_calculate_sha256(&executable, db)?;

    // Look up version from database (for stable releases)
    let version_info = if let Some(db) = db {
        match db.get_version(sha256.as_str()) {
            Ok(info) => info,
            Err(e) => {
                tracing::warn!("Failed to look up version: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Calculate saves size
    let saves_dir = directory.join("save");
    let saves_size = if saves_dir.exists() {
        calculate_dir_size(&saves_dir).unwrap_or(0)
    } else {
        0
    };

    Ok(Some(GameInfo {
        executable,
        version_info,
        saves_size,
    }))
}

/// Get file metadata (size and mtime) for cache key
fn get_file_metadata(path: &Path) -> Result<(u64, i64)> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    let mtime = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok((size, mtime))
}

/// Get SHA256 from cache or calculate it (and update cache)
fn get_or_calculate_sha256(executable: &Path, db: Option<&Database>) -> Result<String> {
    let path_str = executable.to_string_lossy().to_string();
    let (size, mtime) = get_file_metadata(executable)?;

    // Try to get from cache
    if let Some(db) = db {
        if let Ok(Some(cached_hash)) = db.get_cached_hash(&path_str, size, mtime) {
            tracing::debug!("Using cached SHA256 for {}", executable.display());
            return Ok(cached_hash);
        }
    }

    // Calculate hash (slow)
    tracing::debug!("Calculating SHA256 for {} (not cached)", executable.display());
    let sha256 = calculate_sha256(executable)?;

    // Store in cache for next time
    if let Some(db) = db {
        if let Err(e) = db.store_cached_hash(&path_str, size, mtime, &sha256) {
            tracing::warn!("Failed to cache SHA256: {}", e);
        }
    }

    Ok(sha256)
}

/// Read version info from VERSION.txt file (fallback for experimental builds)
fn read_version_txt(directory: &Path) -> Option<VersionInfo> {
    let version_file = directory.join("VERSION.txt");
    if !version_file.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&version_file).ok()?;

    // Parse VERSION.txt which contains lines like:
    // build number: 2025-12-13-1446
    // commit sha: abc1234567890...
    // commit date: 2024-01-15 (optional, older format)
    let mut commit_sha: Option<String> = None;
    let mut commit_date: Option<String> = None;
    let mut build_number: Option<String> = None;

    for line in content.lines() {
        if let Some(sha) = line.strip_prefix("commit sha:") {
            let sha = sha.trim();
            if sha.len() >= 7 {
                commit_sha = Some(sha[..7].to_string());
            }
        } else if let Some(date) = line.strip_prefix("commit date:") {
            commit_date = Some(date.trim().to_string());
        } else if let Some(bn) = line.strip_prefix("build number:") {
            build_number = Some(bn.trim().to_string());
        }
    }

    // Need at least the SHA to identify the version
    let sha = commit_sha?;

    // Build number format: "2025-12-13-1446" (YYYY-MM-DD-HHMM)
    // Extract date for display, but keep full build number for comparison
    let display_date = if let Some(ref bn) = build_number {
        // Extract just the date part (first 10 characters: YYYY-MM-DD)
        if bn.len() >= 10 && bn.chars().nth(4) == Some('-') && bn.chars().nth(7) == Some('-') {
            Some(bn[..10].to_string())
        } else {
            commit_date.clone()
        }
    } else {
        commit_date.clone()
    };

    // Use date as the display version if available (more user-friendly)
    // Format: "2024-01-15 (abc1234)" or just "abc1234" if no date
    let version = if let Some(ref date) = display_date {
        format!("{} ({})", date, sha)
    } else {
        sha.clone()
    };

    // Store the full build number for precise version comparison
    // This allows distinguishing between multiple builds on the same day
    Some(VersionInfo {
        version,
        stable: false,
        released_on: build_number.or(commit_date), // Prefer full build number
    })
}

/// Calculate SHA256 hash of a file
pub fn calculate_sha256(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Calculate total size of a directory recursively
pub fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;

    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total += calculate_dir_size(&path)?;
            } else {
                total += entry.metadata()?.len();
            }
        }
    }

    Ok(total)
}

/// Launch the game
pub fn launch_game(executable: &Path, params: &str) -> Result<()> {
    use std::process::Command;

    let mut cmd = Command::new(executable);

    // Set working directory to game directory
    let working_dir = executable.parent().context("Executable has no parent directory")?;
    cmd.current_dir(working_dir);

    // Add user params
    if !params.is_empty() {
        // Split params by whitespace (simple approach)
        for param in params.split_whitespace() {
            cmd.arg(param);
        }
    }

    tracing::info!(
        "Launching game: {:?} with working dir: {:?}",
        executable,
        working_dir
    );

    cmd.spawn()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_calculate_sha256() {
        // Create a temp file with known content
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("phoenix_test_sha256.txt");

        {
            let mut file = std::fs::File::create(&temp_file).unwrap();
            file.write_all(b"hello world").unwrap();
        }

        let hash = calculate_sha256(&temp_file).unwrap();

        // SHA256 of "hello world" is known
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        // Clean up
        std::fs::remove_file(&temp_file).ok();
    }

    #[test]
    fn test_calculate_dir_size() {
        // Create a temp directory with some files
        let temp_dir = std::env::temp_dir().join("phoenix_test_dir_size");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create two files with known sizes
        let file1 = temp_dir.join("file1.txt");
        let file2 = temp_dir.join("file2.txt");

        std::fs::write(&file1, "12345").unwrap(); // 5 bytes
        std::fs::write(&file2, "1234567890").unwrap(); // 10 bytes

        let size = calculate_dir_size(&temp_dir).unwrap();
        assert_eq!(size, 15);

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_detect_game_no_executable() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_no_game");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = detect_game_with_db(&temp_dir, None).unwrap();
        assert!(result.is_none());

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_read_version_txt_valid() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_version_txt");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a VERSION.txt with commit sha and date
        let version_file = temp_dir.join("VERSION.txt");
        std::fs::write(
            &version_file,
            "Main branch: master\ncommit sha: abc1234567890def\ncommit date: 2024-01-15\n",
        )
        .unwrap();

        let result = read_version_txt(&temp_dir);
        assert!(result.is_some());

        let info = result.unwrap();
        // Version now includes date: "2024-01-15 (abc1234)"
        assert_eq!(info.version, "2024-01-15 (abc1234)");
        assert!(!info.stable);
        assert_eq!(info.released_on, Some("2024-01-15".to_string()));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_read_version_txt_sha_only() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_version_sha_only");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a VERSION.txt with only commit sha (no date)
        let version_file = temp_dir.join("VERSION.txt");
        std::fs::write(
            &version_file,
            "Main branch: master\ncommit sha: def7890123456abc\n",
        )
        .unwrap();

        let result = read_version_txt(&temp_dir);
        assert!(result.is_some());

        let info = result.unwrap();
        // Without date, version is just the SHA
        assert_eq!(info.version, "def7890");
        assert!(!info.stable);
        assert_eq!(info.released_on, None);

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_read_version_txt_with_build_number() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_version_build_number");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a VERSION.txt with build number (actual format from CDDA)
        let version_file = temp_dir.join("VERSION.txt");
        std::fs::write(
            &version_file,
            "build type: windows-with-graphics-x64\nbuild number: 2025-12-13-1446\ncommit sha: 302bb35a02fa115e34c30f04041ee81972ee7933\ncommit url: https://github.com/CleverRaven/Cataclysm-DDA/commit/302bb35\n",
        )
        .unwrap();

        let result = read_version_txt(&temp_dir);
        assert!(result.is_some());

        let info = result.unwrap();
        // Should extract date from build number for display, but store full build number
        assert_eq!(info.version, "2025-12-13 (302bb35)");
        assert!(!info.stable);
        // released_on stores full build number for precise version comparison
        assert_eq!(info.released_on, Some("2025-12-13-1446".to_string()));

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_read_version_txt_missing() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_no_version");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let result = read_version_txt(&temp_dir);
        assert!(result.is_none());

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_read_version_txt_no_commit_sha() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_version_no_sha");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a VERSION.txt without commit sha line
        let version_file = temp_dir.join("VERSION.txt");
        std::fs::write(&version_file, "Some other content\nNo sha here\n").unwrap();

        let result = read_version_txt(&temp_dir);
        assert!(result.is_none());

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_game_info_version_display() {
        let info_with_version = GameInfo {
            executable: PathBuf::from("C:\\test\\game.exe"),
            version_info: Some(VersionInfo {
                version: "0.F-3".to_string(),
                stable: true,
                released_on: None,
            }),
            saves_size: 0,
        };

        assert_eq!(info_with_version.version_display(), "0.F-3");
        assert!(info_with_version.is_stable());

        let info_without_version = GameInfo {
            executable: PathBuf::from("C:\\test\\game.exe"),
            version_info: None,
            saves_size: 0,
        };

        assert_eq!(info_without_version.version_display(), "Unknown");
        assert!(!info_without_version.is_stable());
    }

    #[test]
    fn test_game_info_experimental() {
        let info = GameInfo {
            executable: PathBuf::from("C:\\test\\game.exe"),
            version_info: Some(VersionInfo {
                version: "abc1234".to_string(),
                stable: false,
                released_on: Some("2024-01-15".to_string()),
            }),
            saves_size: 1024,
        };

        assert_eq!(info.version_display(), "abc1234");
        assert!(!info.is_stable());
    }
}
