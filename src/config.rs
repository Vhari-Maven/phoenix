//! Application configuration management.
//!
//! This module handles loading and saving Phoenix settings from a TOML file
//! located at `%APPDATA%\phoenix\Phoenix\config\config.toml`.
//!
//! Configuration is organized into sections:
//!
//! - `LauncherConfig`: Theme, window behavior
//! - `GameConfig`: Game directory, branch (experimental/stable), command line params
//! - `UpdateConfig`: Auto-check, save handling, archive cleanup
//! - `BackupConfig`: Compression level, max count, auto-backup triggers

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::ui::theme::ThemePreset;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub game: GameConfig,
    #[serde(default)]
    pub updates: UpdateConfig,
    #[serde(default)]
    pub backups: BackupConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            launcher: LauncherConfig::default(),
            game: GameConfig::default(),
            updates: UpdateConfig::default(),
            backups: BackupConfig::default(),
        }
    }
}

/// Launcher appearance and behavior settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    /// Theme preset name
    #[serde(default)]
    pub theme: ThemePreset,
    /// Keep launcher open after game closes
    #[serde(default)]
    pub keep_open: bool,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            theme: ThemePreset::default(),
            keep_open: false,
        }
    }
}

/// Game installation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// Path to game directory
    #[serde(default)]
    pub directory: Option<String>,
    /// Game branch (stable or experimental)
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Custom command-line parameters
    #[serde(default)]
    pub command_params: String,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            directory: None,
            branch: default_branch(),
            command_params: String::new(),
        }
    }
}

fn default_branch() -> String {
    "experimental".to_string()
}

/// Update behavior settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Check for updates on startup
    #[serde(default = "default_true")]
    pub check_on_startup: bool,
    /// Maximum concurrent downloads
    #[serde(default = "default_max_downloads")]
    pub max_concurrent_downloads: u8,
    /// Do not move save directory during updates (leave in place)
    #[serde(default)]
    pub prevent_save_move: bool,
    /// Automatically delete previous_version after successful update
    #[serde(default)]
    pub remove_previous_version: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            max_concurrent_downloads: 4,
            prevent_save_move: false,
            remove_previous_version: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_max_downloads() -> u8 {
    4
}

/// Backup settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Maximum number of auto-backups to keep (1-1000)
    #[serde(default = "default_max_backups")]
    pub max_count: u32,
    /// Compression level (0-9, where 0=store, 9=best)
    #[serde(default = "default_compression")]
    pub compression_level: u8,
    /// Auto-backup before game launch
    #[serde(default)]
    pub backup_on_launch: bool,
    /// Auto-backup after game closes
    #[serde(default)]
    pub backup_on_end: bool,
    /// Auto-backup before updates
    #[serde(default = "default_true")]
    pub backup_before_update: bool,
    /// Skip backing up current saves before restore
    #[serde(default)]
    pub skip_backup_before_restore: bool,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            max_count: 6,
            compression_level: 6,
            backup_on_launch: false,
            backup_on_end: false,
            backup_before_update: true,
            skip_backup_before_restore: false,
        }
    }
}

fn default_max_backups() -> u32 {
    6
}

fn default_compression() -> u8 {
    6
}

impl Config {
    /// Get the configuration file path
    pub fn config_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("com", "phoenix", "Phoenix")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        let config_dir = dirs.config_dir();
        std::fs::create_dir_all(config_dir)?;

        Ok(config_dir.join("config.toml"))
    }

    /// Get the Phoenix data directory (for database, etc.)
    pub fn data_dir() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("com", "phoenix", "Phoenix")
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;

        let data_dir = dirs.data_dir();
        std::fs::create_dir_all(data_dir)?;

        Ok(data_dir.to_path_buf())
    }

    /// Get the backups directory (in AppData)
    pub fn backups_dir() -> Result<PathBuf> {
        let data_dir = Self::data_dir()?;
        let backups_dir = data_dir.join("backups");
        std::fs::create_dir_all(&backups_dir)?;

        Ok(backups_dir)
    }

    /// Load configuration from file
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let config: Config = toml::from_str(&content)?;
            tracing::info!("Loaded configuration from {:?}", path);
            Ok(config)
        } else {
            tracing::info!("No configuration file found, using defaults");
            Ok(Self::default())
        }
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        tracing::info!("Saved configuration to {:?}", path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default_values() {
        let config = Config::default();

        // Launcher defaults
        assert_eq!(config.launcher.theme, ThemePreset::Amber);
        assert!(!config.launcher.keep_open);

        // Game defaults
        assert!(config.game.directory.is_none());
        assert_eq!(config.game.branch, "experimental");
        assert!(config.game.command_params.is_empty());

        // Update defaults
        assert!(config.updates.check_on_startup);
        assert_eq!(config.updates.max_concurrent_downloads, 4);
        assert!(!config.updates.prevent_save_move);
        assert!(!config.updates.remove_previous_version);

        // Backup defaults
        assert_eq!(config.backups.max_count, 6);
        assert_eq!(config.backups.compression_level, 6);
        assert!(!config.backups.backup_on_launch);
        assert!(!config.backups.backup_on_end);
        assert!(config.backups.backup_before_update);
        assert!(!config.backups.skip_backup_before_restore);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        // Create a config with custom values
        let mut config = Config::default();
        config.game.directory = Some("C:\\Games\\CDDA".to_string());
        config.game.branch = "stable".to_string();
        config.launcher.theme = ThemePreset::Purple;

        // Serialize to TOML
        let toml_str = toml::to_string_pretty(&config).unwrap();

        // Deserialize back
        let loaded: Config = toml::from_str(&toml_str).unwrap();

        // Verify values match
        assert_eq!(loaded.game.directory, Some("C:\\Games\\CDDA".to_string()));
        assert_eq!(loaded.game.branch, "stable");
        assert_eq!(loaded.launcher.theme, ThemePreset::Purple);
    }

    #[test]
    fn test_config_partial_toml() {
        // Test that missing fields use defaults
        let toml_str = r#"
[game]
directory = "C:\\Test"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();

        // Specified value
        assert_eq!(config.game.directory, Some("C:\\Test".to_string()));

        // Defaults for unspecified values
        assert_eq!(config.game.branch, "experimental");
        assert_eq!(config.launcher.theme, ThemePreset::Amber);
        assert_eq!(config.updates.max_concurrent_downloads, 4);
    }

    #[test]
    fn test_config_empty_toml() {
        // Empty TOML should give all defaults
        let config: Config = toml::from_str("").unwrap();

        assert!(config.game.directory.is_none());
        assert_eq!(config.game.branch, "experimental");
        assert_eq!(config.launcher.theme, ThemePreset::Amber);
    }
}
