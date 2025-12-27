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
const STABLE_RELEASES_TOML: &str = include_str!("../embedded/stable_releases.toml");
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
    pub export: ExportConfig,
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

#[derive(Debug, Deserialize)]
pub struct ExportConfig {
    pub directories: Vec<String>,
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
// Stable Releases
// ============================================================================

/// Known stable release configuration
#[derive(Debug, Deserialize)]
pub struct StableReleasesConfig {
    /// Future letters to check via API
    pub check_letters: Vec<String>,
    /// Known stable releases
    pub releases: Vec<EmbeddedRelease>,
}

/// A known stable release embedded in the launcher
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddedRelease {
    /// GitHub tag (e.g., "0.H-RELEASE")
    pub tag: String,
    /// Release name (e.g., "Herbert")
    pub name: String,
    /// Publication date (YYYY-MM-DD)
    pub published: String,
    /// Windows asset filename (if available)
    pub asset_name: Option<String>,
    /// Windows asset download URL (if available)
    pub asset_url: Option<String>,
    /// Windows asset size in bytes (if available)
    pub asset_size: Option<u64>,
    /// SHA256 hashes of executables for version identification
    #[serde(default)]
    pub hashes: Vec<String>,
}

/// Get stable releases configuration (lazy-loaded)
pub fn stable_releases_config() -> &'static StableReleasesConfig {
    static CONFIG: OnceLock<StableReleasesConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        toml::from_str(STABLE_RELEASES_TOML).unwrap_or_else(|e| {
            panic!("Failed to parse stable_releases.toml: {}", e);
        })
    })
}

/// Get stable version SHA256 hashes (lazy-loaded)
///
/// Maps executable SHA256 hashes to version strings (e.g., "0.F-3").
/// Built from stable_releases_config() for instant identification of known stable releases.
pub fn stable_versions() -> &'static HashMap<String, String> {
    static HASHES: OnceLock<HashMap<String, String>> = OnceLock::new();
    HASHES.get_or_init(|| {
        let config = stable_releases_config();
        let mut map = HashMap::new();
        for release in &config.releases {
            for hash in &release.hashes {
                map.insert(hash.clone(), release.tag.clone());
            }
        }
        map
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
            panic!("Failed to parse soundpacks.json: {}", e);
        })
    })
}
