//! Update functionality for downloading and installing game updates.
//!
//! This module handles:
//! - Downloading release assets from GitHub with progress tracking
//! - Backing up the current installation
//! - Extracting new versions while preserving user data
//! - Smart migration to only restore custom mods/tilesets/soundpacks/fonts

mod access;
mod download;
mod install;

pub use access::check_installation_access;
pub use download::{download_asset, download_dir};
pub use install::install_update;

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
    pub speed: u64, // bytes/sec
    pub files_extracted: usize,
    pub total_files: usize,
    pub current_file: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_phase_description() {
        assert_eq!(
            UpdatePhase::Downloading.description(),
            "Downloading update..."
        );
        assert_eq!(
            UpdatePhase::BackingUp.description(),
            "Backing up current installation..."
        );
        assert_eq!(
            UpdatePhase::Extracting.description(),
            "Extracting new version..."
        );
        assert_eq!(
            UpdatePhase::Restoring.description(),
            "Restoring saves and settings..."
        );
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

        // Extract progress
        progress.phase = UpdatePhase::Extracting;
        progress.files_extracted = 25;
        progress.total_files = 100;
        assert_eq!(progress.extract_fraction(), 0.25);

        // Zero total should return 0
        progress.total_bytes = 0;
        progress.total_files = 0;
        assert_eq!(progress.download_fraction(), 0.0);
        assert_eq!(progress.extract_fraction(), 0.0);
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
