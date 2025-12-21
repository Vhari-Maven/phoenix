use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
    /// Detected version (if known)
    pub version: Option<String>,
    /// Size of save directory in bytes
    pub saves_size: u64,
}

/// Detect game installation in the given directory
pub fn detect_game(directory: &Path) -> Result<Option<GameInfo>> {
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
        version: None, // TODO: Look up version from hash
        saves_size,
    }))
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
}
