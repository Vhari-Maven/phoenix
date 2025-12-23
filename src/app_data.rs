//! Application data embedded from TOML/JSON files at compile time.
//!
//! This module provides access to application-level constants that are:
//! - Embedded at compile time via `include_str!`
//! - Parsed lazily on first access via `OnceLock`
//! - Immutable at runtime (not user-configurable)
//!
//! This is distinct from `config.rs` which handles user preferences.
//! App data defines *how the application works* (CDDA file formats, API endpoints),
//! while config defines *user choices* (theme, backup settings).
//!
//! Data files are located in `embedded/`:
//! - `game_config.toml` - CDDA-specific paths and detection
//! - `migration_config.toml` - Update and migration behavior
//! - `launcher_config.toml` - Application settings and URLs
//! - `release_config.toml` - GitHub release version patterns
//! - `stable_hashes.toml` - SHA256 hashes for stable version identification
//! - `soundpacks.json` - Soundpack repository

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

// Embed data files at compile time
const GAME_CONFIG_TOML: &str = include_str!("../embedded/game_config.toml");
const MIGRATION_CONFIG_TOML: &str = include_str!("../embedded/migration_config.toml");
const LAUNCHER_CONFIG_TOML: &str = include_str!("../embedded/launcher_config.toml");
const RELEASE_CONFIG_TOML: &str = include_str!("../embedded/release_config.toml");
const STABLE_HASHES_TOML: &str = include_str!("../embedded/stable_hashes.toml");
const SOUNDPACKS_JSON: &str = include_str!("../embedded/soundpacks.json");

// ============================================================================
// Game Configuration
// ============================================================================

/// Game-specific configuration (executables, directories, version parsing)
#[derive(Debug, Deserialize)]
pub struct GameConfig {
    pub executables: ExecutablesConfig,
    pub directories: DirectoriesConfig,
    pub version: VersionConfig,
    pub world: WorldConfig,
    pub metadata: MetadataConfig,
}

#[derive(Debug, Deserialize)]
pub struct ExecutablesConfig {
    pub names: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DirectoriesConfig {
    pub save: String,
    pub data: String,
    pub sound: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionConfig {
    pub filename: String,
    pub commit_sha_prefix: String,
    pub commit_date_prefix: String,
    pub build_number_prefix: String,
    pub sha_display_length: usize,
    pub min_build_number_length: usize,
    pub date_dash_positions: Vec<usize>,
}

#[derive(Debug, Deserialize)]
pub struct WorldConfig {
    pub marker_files: Vec<String>,
    pub save_extensions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct MetadataConfig {
    pub mod_info: String,
    pub mod_info_disabled: String,
    pub tileset_info: String,
    pub soundpack_info: String,
    pub soundpack_info_disabled: String,
    pub name_field: String,
}

/// Get game configuration (lazy-loaded)
pub fn game_config() -> &'static GameConfig {
    static CONFIG: OnceLock<GameConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        toml::from_str(GAME_CONFIG_TOML).unwrap_or_else(|e| {
            panic!("Failed to parse game_config.toml: {}", e);
        })
    })
}

// ============================================================================
// Migration Configuration
// ============================================================================

/// Migration and update configuration
#[derive(Debug, Deserialize)]
pub struct MigrationConfig {
    pub restore: RestoreConfig,
    pub archive: ArchiveConfig,
    pub soundpack: SoundpackMigrationConfig,
    pub download: DownloadConfig,
}

#[derive(Debug, Deserialize)]
pub struct RestoreConfig {
    pub simple_dirs: Vec<String>,
    pub skip_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveConfig {
    pub directory: String,
    pub directory_old: String,
}

#[derive(Debug, Deserialize)]
pub struct SoundpackMigrationConfig {
    pub content_extensions: Vec<String>,
    pub min_search_depth: usize,
    pub max_search_depth: usize,
}

#[derive(Debug, Deserialize)]
pub struct DownloadConfig {
    pub temp_extension: String,
    pub progress_interval_ms: u64,
    pub extraction_batch_size: usize,
    pub soundpack_extraction_batch: usize,
}

/// Get migration configuration (lazy-loaded)
pub fn migration_config() -> &'static MigrationConfig {
    static CONFIG: OnceLock<MigrationConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        toml::from_str(MIGRATION_CONFIG_TOML).unwrap_or_else(|e| {
            panic!("Failed to parse migration_config.toml: {}", e);
        })
    })
}

// ============================================================================
// Launcher Configuration
// ============================================================================

/// Launcher application configuration
#[derive(Debug, Deserialize)]
pub struct LauncherConfig {
    pub window: WindowConfig,
    pub github: GithubConfig,
    pub backup: BackupConfig,
    pub urls: UrlsConfig,
    pub legacy: LegacyConfig,
}

#[derive(Debug, Deserialize)]
pub struct WindowConfig {
    pub initial_size: [f32; 2],
    pub min_size: [f32; 2],
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct GithubConfig {
    pub api_base: String,
    pub repository: String,
    pub releases_per_page: u32,
    pub rate_limit_warning_threshold: u32,
}

#[derive(Debug, Deserialize)]
pub struct BackupConfig {
    pub max_name_length: usize,
    pub allowed_name_chars: String,
    pub auto_backup_prefix: String,
}

#[derive(Debug, Deserialize)]
pub struct UrlsConfig {
    pub project_repository: String,
    pub cdda_website: String,
}

#[derive(Debug, Deserialize)]
pub struct LegacyConfig {
    pub old_backup_dir: String,
    pub old_archive_dir: String,
}

/// Get launcher configuration (lazy-loaded)
pub fn launcher_config() -> &'static LauncherConfig {
    static CONFIG: OnceLock<LauncherConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        toml::from_str(LAUNCHER_CONFIG_TOML).unwrap_or_else(|e| {
            panic!("Failed to parse launcher_config.toml: {}", e);
        })
    })
}

// ============================================================================
// Release Configuration
// ============================================================================

/// Release version pattern configuration
#[derive(Debug, Deserialize)]
pub struct ReleaseConfig {
    pub stable: StableReleaseConfig,
}

#[derive(Debug, Deserialize)]
pub struct StableReleaseConfig {
    pub version_letters: Vec<String>,
    pub max_point_release: u8,
}

/// Get release configuration (lazy-loaded)
pub fn release_config() -> &'static ReleaseConfig {
    static CONFIG: OnceLock<ReleaseConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        toml::from_str(RELEASE_CONFIG_TOML).unwrap_or_else(|e| {
            tracing::error!("Failed to parse release_config.toml: {}", e);
            // Fallback to minimal defaults
            ReleaseConfig {
                stable: StableReleaseConfig {
                    version_letters: vec!["G".to_string(), "F".to_string()],
                    max_point_release: 5,
                },
            }
        })
    })
}

// ============================================================================
// Stable Version Hashes
// ============================================================================

/// Parsed structure for stable_hashes.toml
#[derive(Deserialize)]
struct StableHashesConfig {
    hashes: HashMap<String, String>,
}

/// Get stable version SHA256 hashes (lazy-loaded)
///
/// Maps executable SHA256 hashes to version strings (e.g., "0.F-3").
/// Used for instant identification of known stable releases.
pub fn stable_versions() -> &'static HashMap<String, String> {
    static HASHES: OnceLock<HashMap<String, String>> = OnceLock::new();
    HASHES.get_or_init(|| {
        match toml::from_str::<StableHashesConfig>(STABLE_HASHES_TOML) {
            Ok(config) => config.hashes,
            Err(e) => {
                tracing::error!("Failed to parse stable_hashes.toml: {}", e);
                HashMap::new()
            }
        }
    })
}

// ============================================================================
// Soundpack Repository
// ============================================================================

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

/// Get the soundpacks repository (lazy-loaded)
///
/// Returns a list of available soundpacks from the embedded repository.
pub fn soundpacks_repository() -> &'static Vec<RepoSoundpack> {
    static REPO: OnceLock<Vec<RepoSoundpack>> = OnceLock::new();
    REPO.get_or_init(|| {
        serde_json::from_str(SOUNDPACKS_JSON).unwrap_or_else(|e| {
            tracing::error!("Failed to parse soundpacks.json: {}", e);
            Vec::new()
        })
    })
}
