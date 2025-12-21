use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// Enable dark theme
    #[serde(default)]
    pub dark_theme: bool,
    /// Keep launcher open after game closes
    #[serde(default)]
    pub keep_open: bool,
    /// UI language code
    #[serde(default = "default_locale")]
    pub locale: String,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            dark_theme: true,
            keep_open: false,
            locale: default_locale(),
        }
    }
}

fn default_locale() -> String {
    "en".to_string()
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
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            max_concurrent_downloads: 4,
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
    /// Maximum number of backups to keep
    #[serde(default = "default_max_backups")]
    pub max_count: u32,
    /// Compression level (0-9)
    #[serde(default = "default_compression")]
    pub compression_level: u8,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            max_count: 10,
            compression_level: 6,
        }
    }
}

fn default_max_backups() -> u32 {
    10
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
