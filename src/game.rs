use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::db::{Database, VersionInfo};

/// Game executable names to look for
const GAME_EXECUTABLES: &[&str] = &["cataclysm-tiles.exe", "cataclysm.exe"];

/// Files that indicate a world/save exists
const WORLD_FILES: &[&str] = &["worldoptions.json", "worldoptions.txt", "master.gsav"];

/// Information about a detected game installation
#[derive(Debug, Clone)]
pub struct GameInfo {
    /// Path to the game directory
    pub directory: PathBuf,
    /// Path to the game executable
    pub executable: PathBuf,
    /// SHA256 hash of the executable (for version detection)
    pub sha256: String,
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

/// Detect game installation in the given directory
pub fn detect_game(directory: &Path) -> Result<Option<GameInfo>> {
    detect_game_with_db(directory, None)
}

/// Detect game installation with optional database for version lookup
pub fn detect_game_with_db(directory: &Path, db: Option<&Database>) -> Result<Option<GameInfo>> {
    // Look for game executable
    let executable = GAME_EXECUTABLES
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.exists());

    let Some(executable) = executable else {
        return Ok(None);
    };

    // Calculate SHA256 of executable
    let sha256 = calculate_sha256(&executable)?;

    // Look up version from database
    let version_info = if let Some(db) = db {
        match db.get_version(&sha256) {
            Ok(Some(info)) => Some(info),
            Ok(None) => {
                // Try to read VERSION.txt as fallback for experimental builds
                read_version_txt(directory)
            }
            Err(e) => {
                tracing::warn!("Failed to look up version: {}", e);
                read_version_txt(directory)
            }
        }
    } else {
        read_version_txt(directory)
    };

    // Calculate saves size
    let saves_dir = directory.join("save");
    let saves_size = if saves_dir.exists() {
        calculate_dir_size(&saves_dir).unwrap_or(0)
    } else {
        0
    };

    Ok(Some(GameInfo {
        directory: directory.to_path_buf(),
        executable,
        sha256,
        version_info,
        saves_size,
    }))
}

/// Read version info from VERSION.txt file (fallback for experimental builds)
fn read_version_txt(directory: &Path) -> Option<VersionInfo> {
    let version_file = directory.join("VERSION.txt");
    if !version_file.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&version_file).ok()?;

    // Look for commit SHA in the file
    // Format: "commit sha: abc1234..."
    for line in content.lines() {
        if let Some(sha) = line.strip_prefix("commit sha:") {
            let sha = sha.trim();
            if sha.len() >= 7 {
                return Some(VersionInfo {
                    version: sha[..7].to_string(),
                    stable: false,
                    build_number: None,
                    released_on: None,
                });
            }
        }
    }

    None
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

/// Check if a directory contains world/save data
pub fn has_saves(directory: &Path) -> bool {
    let save_dir = directory.join("save");
    if !save_dir.exists() {
        return false;
    }

    // Look for world directories containing world files
    if let Ok(entries) = std::fs::read_dir(&save_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                for world_file in WORLD_FILES {
                    if path.join(world_file).exists() {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Launch the game
pub fn launch_game(executable: &Path, params: &str) -> Result<()> {
    use std::process::Command;

    let mut cmd = Command::new(executable);

    if !params.is_empty() {
        // Split params by whitespace (simple approach)
        for param in params.split_whitespace() {
            cmd.arg(param);
        }
    }

    // Set working directory to game directory
    if let Some(parent) = executable.parent() {
        cmd.current_dir(parent);
    }

    tracing::info!("Launching game: {:?} {}", executable, params);
    cmd.spawn()?;

    Ok(())
}

/// Format byte size to human-readable string
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1), "1 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kibibytes() {
        assert_eq!(format_size(1024), "1.00 KiB");
        assert_eq!(format_size(1536), "1.50 KiB");
        assert_eq!(format_size(10240), "10.00 KiB");
    }

    #[test]
    fn test_format_size_mebibytes() {
        assert_eq!(format_size(1024 * 1024), "1.00 MiB");
        assert_eq!(format_size(150 * 1024 * 1024), "150.00 MiB"); // 150 MB warning threshold
    }

    #[test]
    fn test_format_size_gibibytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GiB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.00 GiB");
    }

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

        let result = detect_game(&temp_dir).unwrap();
        assert!(result.is_none());

        // Clean up
        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_read_version_txt_valid() {
        let temp_dir = std::env::temp_dir().join("phoenix_test_version_txt");
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create a VERSION.txt with commit sha
        let version_file = temp_dir.join("VERSION.txt");
        std::fs::write(
            &version_file,
            "Main branch: master\ncommit sha: abc1234567890def\ncommit date: 2024-01-15\n",
        )
        .unwrap();

        let result = read_version_txt(&temp_dir);
        assert!(result.is_some());

        let info = result.unwrap();
        assert_eq!(info.version, "abc1234"); // First 7 characters
        assert!(!info.stable);

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
            directory: PathBuf::from("C:\\test"),
            executable: PathBuf::from("C:\\test\\game.exe"),
            sha256: "abc123".to_string(),
            version_info: Some(VersionInfo {
                version: "0.F-3".to_string(),
                stable: true,
                build_number: None,
                released_on: None,
            }),
            saves_size: 0,
        };

        assert_eq!(info_with_version.version_display(), "0.F-3");
        assert!(info_with_version.is_stable());

        let info_without_version = GameInfo {
            directory: PathBuf::from("C:\\test"),
            executable: PathBuf::from("C:\\test\\game.exe"),
            sha256: "abc123".to_string(),
            version_info: None,
            saves_size: 0,
        };

        assert_eq!(info_without_version.version_display(), "Unknown");
        assert!(!info_without_version.is_stable());
    }

    #[test]
    fn test_game_info_experimental() {
        let info = GameInfo {
            directory: PathBuf::from("C:\\test"),
            executable: PathBuf::from("C:\\test\\game.exe"),
            sha256: "abc123".to_string(),
            version_info: Some(VersionInfo {
                version: "abc1234".to_string(),
                stable: false,
                build_number: Some(12345),
                released_on: Some("2024-01-15".to_string()),
            }),
            saves_size: 1024,
        };

        assert_eq!(info.version_display(), "abc1234");
        assert!(!info.is_stable());
    }
}
