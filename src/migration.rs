//! Smart migration logic for preserving custom content during updates.
//!
//! This module handles identity-based detection of custom mods, tilesets,
//! soundpacks, and fonts to avoid overwriting new official content with old versions.

use crate::app_data::{game_config, migration_config};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Get files to skip during config restoration
pub fn config_skip_files() -> &'static [String] {
    &migration_config().restore.skip_files
}

/// Represents a mod with its identifier and path
#[derive(Debug, Clone)]
pub struct ModInfo {
    /// Unique mod identifier from modinfo.json
    pub id: String,
    /// Path to the mod directory
    pub path: PathBuf,
}

/// Represents a tileset with its name and path
#[derive(Debug, Clone)]
pub struct TilesetInfo {
    /// Tileset name from tileset.txt NAME field
    pub name: String,
    /// Path to the tileset directory
    pub path: PathBuf,
}

/// Represents a soundpack with its name and path
#[derive(Debug, Clone)]
pub struct SoundpackInfo {
    /// Soundpack name from soundpack.txt NAME field
    pub name: String,
    /// Path to the soundpack directory
    pub path: PathBuf,
}

/// Represents files to merge into an official soundpack during migration.
///
/// When a soundpack exists in both old and new versions (same NAME),
/// this tracks custom files the user added that should be preserved.
#[derive(Debug, Clone)]
pub struct SoundpackMergeInfo {
    /// Soundpack name (matching both old and new versions)
    #[allow(dead_code)] // Used for debug logging
    pub name: String,
    /// Path to the soundpack in the old (archived) version
    pub old_path: PathBuf,
    /// Path to the soundpack in the new version
    pub new_path: PathBuf,
    /// Custom files to restore (relative paths within the soundpack)
    pub custom_files: Vec<PathBuf>,
}

/// Result of analyzing directories for custom content
#[derive(Debug, Default)]
pub struct MigrationPlan {
    /// Custom mods to restore (from data/mods/)
    pub custom_mods: Vec<ModInfo>,
    /// Custom user mods to restore (from mods/)
    pub custom_user_mods: Vec<ModInfo>,
    /// Custom tilesets to restore
    pub custom_tilesets: Vec<TilesetInfo>,
    /// Custom soundpacks to restore (not in new version at all)
    pub custom_soundpacks: Vec<SoundpackInfo>,
    /// Soundpacks with custom files to merge (exist in both versions)
    pub soundpack_merges: Vec<SoundpackMergeInfo>,
    /// Custom fonts to restore (files not in new version)
    pub custom_fonts: Vec<PathBuf>,
    /// Custom data/fonts to restore
    pub custom_data_fonts: Vec<PathBuf>,
    /// Whether to restore user-default-mods.json
    pub restore_user_default_mods: bool,
}

/// Parse modinfo.json to extract the mod identifier.
///
/// Handles both formats:
/// - Single object: `{"type": "MOD_INFO", "id": "my_mod", ...}`
/// - Array: `[{"type": "MOD_INFO", "id": "my_mod", ...}]`
///
/// Also checks for .disabled extension on the JSON file.
pub fn parse_mod_ident(mod_dir: &Path) -> Option<ModInfo> {
    let metadata = &game_config().metadata;
    let json_file = mod_dir.join(&metadata.mod_info);
    let disabled_file = mod_dir.join(&metadata.mod_info_disabled);

    let file_path = if json_file.exists() {
        json_file
    } else if disabled_file.exists() {
        disabled_file
    } else {
        return None;
    };

    let content = std::fs::read_to_string(&file_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try as single object
    if let Some(obj) = json.as_object() {
        if obj.get("type").and_then(|v| v.as_str()) == Some("MOD_INFO") {
            if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                return Some(ModInfo {
                    id: id.to_string(),
                    path: mod_dir.to_path_buf(),
                });
            }
        }
    }

    // Try as array - find first MOD_INFO entry
    if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(obj) = item.as_object() {
                if obj.get("type").and_then(|v| v.as_str()) == Some("MOD_INFO") {
                    if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                        return Some(ModInfo {
                            id: id.to_string(),
                            path: mod_dir.to_path_buf(),
                        });
                    }
                }
            }
        }
    }

    None
}

/// Parse a tileset.txt or soundpack.txt file to extract the NAME field.
///
/// Format: `NAME <name>` where name may contain spaces and commas are stripped.
fn parse_asset_name(asset_dir: &Path, filename: &str, disabled_filename: &str) -> Option<String> {
    let normal_file = asset_dir.join(filename);
    let disabled_file = asset_dir.join(disabled_filename);

    let file_path = if normal_file.exists() {
        normal_file
    } else if disabled_file.exists() {
        disabled_file
    } else {
        return None;
    };

    // Read file - use lossy conversion for latin1 compatibility
    let content = std::fs::read(&file_path).ok()?;
    let text = String::from_utf8_lossy(&content);

    let name_field = &game_config().metadata.name_field;
    for line in text.lines() {
        if line.starts_with(name_field) {
            // Find first space after the name field
            if let Some(space_idx) = line.find(' ') {
                let name = line[space_idx..].trim().replace(',', "");
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }

    None
}

/// Parse tileset.txt to get tileset info
pub fn parse_tileset_info(tileset_dir: &Path) -> Option<TilesetInfo> {
    let metadata = &game_config().metadata;
    // tileset.txt doesn't have a disabled variant in config, construct it
    let disabled_filename = format!("{}.disabled", metadata.tileset_info);
    let name = parse_asset_name(tileset_dir, &metadata.tileset_info, &disabled_filename)?;
    Some(TilesetInfo {
        name,
        path: tileset_dir.to_path_buf(),
    })
}

/// Parse soundpack.txt to get soundpack info
pub fn parse_soundpack_info(soundpack_dir: &Path) -> Option<SoundpackInfo> {
    let metadata = &game_config().metadata;
    let name = parse_asset_name(
        soundpack_dir,
        &metadata.soundpack_info,
        &metadata.soundpack_info_disabled,
    )?;
    Some(SoundpackInfo {
        name,
        path: soundpack_dir.to_path_buf(),
    })
}

/// Scan a mods directory and build a map of mod_id -> ModInfo
pub fn scan_mods_directory(mods_dir: &Path) -> HashMap<String, ModInfo> {
    let mut mods = HashMap::new();

    if !mods_dir.exists() || !mods_dir.is_dir() {
        return mods;
    }

    if let Ok(entries) = std::fs::read_dir(mods_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if let Some(mod_info) = parse_mod_ident(&path) {
                    // Only insert first occurrence (like Python does)
                    mods.entry(mod_info.id.clone()).or_insert(mod_info);
                }
            }
        }
    }

    mods
}

/// Scan a tilesets directory (gfx/) and build a map of name -> TilesetInfo
pub fn scan_tilesets_directory(gfx_dir: &Path) -> HashMap<String, TilesetInfo> {
    let mut tilesets = HashMap::new();

    if !gfx_dir.exists() || !gfx_dir.is_dir() {
        return tilesets;
    }

    if let Ok(entries) = std::fs::read_dir(gfx_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if let Some(tileset_info) = parse_tileset_info(&path) {
                    tilesets
                        .entry(tileset_info.name.clone())
                        .or_insert(tileset_info);
                }
            }
        }
    }

    tilesets
}

/// Scan a soundpacks directory (data/sound/) and build a map of name -> SoundpackInfo
pub fn scan_soundpacks_directory(sound_dir: &Path) -> HashMap<String, SoundpackInfo> {
    let mut soundpacks = HashMap::new();

    if !sound_dir.exists() || !sound_dir.is_dir() {
        return soundpacks;
    }

    if let Ok(entries) = std::fs::read_dir(sound_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if let Some(soundpack_info) = parse_soundpack_info(&path) {
                    soundpacks
                        .entry(soundpack_info.name.clone())
                        .or_insert(soundpack_info);
                }
            }
        }
    }

    soundpacks
}

/// Scan font directory and return set of filenames
pub fn scan_fonts_directory(font_dir: &Path) -> HashSet<String> {
    let mut fonts = HashSet::new();

    if !font_dir.exists() || !font_dir.is_dir() {
        return fonts;
    }

    if let Ok(entries) = std::fs::read_dir(font_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Some(name) = entry.file_name().to_str() {
                fonts.insert(name.to_string());
            }
        }
    }

    fonts
}

/// Find custom mods that exist in old version but not in new version
pub fn find_custom_mods(
    old_mods: &HashMap<String, ModInfo>,
    new_mods: &HashMap<String, ModInfo>,
) -> Vec<ModInfo> {
    old_mods
        .iter()
        .filter(|(id, _)| !new_mods.contains_key(*id))
        .map(|(_, info)| info.clone())
        .collect()
}

/// Find custom tilesets that exist in old version but not in new version
pub fn find_custom_tilesets(
    old_tilesets: &HashMap<String, TilesetInfo>,
    new_tilesets: &HashMap<String, TilesetInfo>,
) -> Vec<TilesetInfo> {
    old_tilesets
        .iter()
        .filter(|(name, _)| !new_tilesets.contains_key(*name))
        .map(|(_, info)| info.clone())
        .collect()
}

/// Find custom soundpacks that exist in old version but not in new version
pub fn find_custom_soundpacks(
    old_soundpacks: &HashMap<String, SoundpackInfo>,
    new_soundpacks: &HashMap<String, SoundpackInfo>,
) -> Vec<SoundpackInfo> {
    old_soundpacks
        .iter()
        .filter(|(name, _)| !new_soundpacks.contains_key(*name))
        .map(|(_, info)| info.clone())
        .collect()
}

/// Recursively scan a soundpack directory for audio and JSON files.
///
/// Returns a set of relative paths (from the soundpack root) for files
/// with extensions defined in migration_config().soundpack.content_extensions
fn scan_soundpack_files_recursive(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut HashSet<PathBuf>,
    content_extensions: &[String],
) {
    let Ok(entries) = std::fs::read_dir(current_dir) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            scan_soundpack_files_recursive(base_dir, &path, files, content_extensions);
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                // Only track files with configured extensions (audio and JSON)
                if content_extensions.iter().any(|e| e == &ext_lower) {
                    if let Ok(relative) = path.strip_prefix(base_dir) {
                        files.insert(relative.to_path_buf());
                    }
                }
            }
        }
    }
}

/// Scan a soundpack directory and return set of relative file paths.
///
/// Only includes files with extensions defined in migration_config().soundpack.content_extensions
pub fn scan_soundpack_files(soundpack_dir: &Path) -> HashSet<PathBuf> {
    let mut files = HashSet::new();

    if !soundpack_dir.exists() || !soundpack_dir.is_dir() {
        return files;
    }

    let content_extensions = &migration_config().soundpack.content_extensions;
    scan_soundpack_files_recursive(soundpack_dir, soundpack_dir, &mut files, content_extensions);
    files
}

/// Find custom files within a soundpack (files in old but not in new).
pub fn find_custom_soundpack_files(old_soundpack: &Path, new_soundpack: &Path) -> Vec<PathBuf> {
    let old_files = scan_soundpack_files(old_soundpack);
    let new_files = scan_soundpack_files(new_soundpack);

    old_files.difference(&new_files).cloned().collect()
}

/// Find soundpacks that exist in both versions and have custom files to merge.
pub fn find_soundpack_merges(
    old_soundpacks: &HashMap<String, SoundpackInfo>,
    new_soundpacks: &HashMap<String, SoundpackInfo>,
) -> Vec<SoundpackMergeInfo> {
    let mut merges = Vec::new();

    for (name, old_info) in old_soundpacks {
        if let Some(new_info) = new_soundpacks.get(name) {
            let custom_files = find_custom_soundpack_files(&old_info.path, &new_info.path);

            if !custom_files.is_empty() {
                tracing::debug!(
                    "Soundpack '{}' has {} custom files to merge",
                    name,
                    custom_files.len()
                );

                merges.push(SoundpackMergeInfo {
                    name: name.clone(),
                    old_path: old_info.path.clone(),
                    new_path: new_info.path.clone(),
                    custom_files,
                });
            }
        }
    }

    merges
}

/// Find custom fonts (filenames in old but not in new)
pub fn find_custom_fonts(
    old_fonts: &HashSet<String>,
    new_fonts: &HashSet<String>,
    old_font_dir: &Path,
) -> Vec<PathBuf> {
    old_fonts
        .difference(new_fonts)
        .map(|name| old_font_dir.join(name))
        .collect()
}

/// Analyze old and new game directories to create a migration plan
pub fn create_migration_plan(previous_version_dir: &Path, game_dir: &Path) -> MigrationPlan {
    let mut plan = MigrationPlan::default();

    // === MODS (data/mods/) ===
    let old_mods_dir = previous_version_dir.join("data").join("mods");
    let new_mods_dir = game_dir.join("data").join("mods");

    let old_mods = scan_mods_directory(&old_mods_dir);
    let new_mods = scan_mods_directory(&new_mods_dir);
    plan.custom_mods = find_custom_mods(&old_mods, &new_mods);

    tracing::info!(
        "Found {} custom mods out of {} total mods",
        plan.custom_mods.len(),
        old_mods.len()
    );

    // === USER MODS (mods/) ===
    let old_user_mods_dir = previous_version_dir.join("mods");
    let new_user_mods_dir = game_dir.join("mods");

    let old_user_mods = scan_mods_directory(&old_user_mods_dir);
    let new_user_mods = scan_mods_directory(&new_user_mods_dir);
    plan.custom_user_mods = find_custom_mods(&old_user_mods, &new_user_mods);

    if !plan.custom_user_mods.is_empty() {
        tracing::info!("Found {} custom user mods", plan.custom_user_mods.len());
    }

    // === TILESETS (gfx/) ===
    let old_gfx_dir = previous_version_dir.join("gfx");
    let new_gfx_dir = game_dir.join("gfx");

    let old_tilesets = scan_tilesets_directory(&old_gfx_dir);
    let new_tilesets = scan_tilesets_directory(&new_gfx_dir);
    plan.custom_tilesets = find_custom_tilesets(&old_tilesets, &new_tilesets);

    tracing::info!(
        "Found {} custom tilesets out of {} total tilesets",
        plan.custom_tilesets.len(),
        old_tilesets.len()
    );

    // === SOUNDPACKS (data/sound/) ===
    let old_sound_dir = previous_version_dir.join("data").join("sound");
    let new_sound_dir = game_dir.join("data").join("sound");

    let old_soundpacks = scan_soundpacks_directory(&old_sound_dir);
    let new_soundpacks = scan_soundpacks_directory(&new_sound_dir);

    // Custom soundpacks (not in new version at all)
    plan.custom_soundpacks = find_custom_soundpacks(&old_soundpacks, &new_soundpacks);

    // Soundpacks in both versions that have custom files to merge
    plan.soundpack_merges = find_soundpack_merges(&old_soundpacks, &new_soundpacks);

    tracing::info!(
        "Found {} custom soundpacks and {} soundpacks with custom files to merge",
        plan.custom_soundpacks.len(),
        plan.soundpack_merges.len()
    );

    // === FONTS (font/) ===
    let old_font_dir = previous_version_dir.join("font");
    let new_font_dir = game_dir.join("font");

    let old_fonts = scan_fonts_directory(&old_font_dir);
    let new_fonts = scan_fonts_directory(&new_font_dir);
    plan.custom_fonts = find_custom_fonts(&old_fonts, &new_fonts, &old_font_dir);

    if !plan.custom_fonts.is_empty() {
        tracing::info!("Found {} custom fonts", plan.custom_fonts.len());
    }

    // === DATA FONTS (data/font/) ===
    let old_data_font_dir = previous_version_dir.join("data").join("font");
    let new_data_font_dir = game_dir.join("data").join("font");

    let old_data_fonts = scan_fonts_directory(&old_data_font_dir);
    let new_data_fonts = scan_fonts_directory(&new_data_font_dir);
    plan.custom_data_fonts = find_custom_fonts(&old_data_fonts, &new_data_fonts, &old_data_font_dir);

    if !plan.custom_data_fonts.is_empty() {
        tracing::info!("Found {} custom data fonts", plan.custom_data_fonts.len());
    }

    // === user-default-mods.json ===
    let old_user_default_mods = old_mods_dir.join("user-default-mods.json");
    let new_user_default_mods = new_mods_dir.join("user-default-mods.json");
    plan.restore_user_default_mods =
        old_user_default_mods.exists() && !new_user_default_mods.exists();

    if plan.restore_user_default_mods {
        tracing::info!("Will restore user-default-mods.json");
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_parse_mod_ident_single_object() {
        let temp_dir = TempDir::new().unwrap();
        let mod_dir = temp_dir.path().join("test_mod");
        fs::create_dir(&mod_dir).unwrap();

        let modinfo = r#"{"type": "MOD_INFO", "id": "test_mod_id", "name": "Test Mod"}"#;
        fs::write(mod_dir.join("modinfo.json"), modinfo).unwrap();

        let result = parse_mod_ident(&mod_dir);
        assert!(result.is_some());
        let mod_info = result.unwrap();
        assert_eq!(mod_info.id, "test_mod_id");
    }

    #[test]
    fn test_parse_mod_ident_array_format() {
        let temp_dir = TempDir::new().unwrap();
        let mod_dir = temp_dir.path().join("test_mod");
        fs::create_dir(&mod_dir).unwrap();

        let modinfo = r#"[{"type": "MOD_INFO", "id": "array_mod_id"}, {"type": "ITEM", "id": "item1"}]"#;
        fs::write(mod_dir.join("modinfo.json"), modinfo).unwrap();

        let result = parse_mod_ident(&mod_dir);
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "array_mod_id");
    }

    #[test]
    fn test_parse_mod_ident_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let mod_dir = temp_dir.path().join("disabled_mod");
        fs::create_dir(&mod_dir).unwrap();

        let modinfo = r#"{"type": "MOD_INFO", "id": "disabled_mod_id"}"#;
        fs::write(mod_dir.join("modinfo.json.disabled"), modinfo).unwrap();

        let result = parse_mod_ident(&mod_dir);
        assert!(result.is_some());
        let mod_info = result.unwrap();
        assert_eq!(mod_info.id, "disabled_mod_id");
    }

    #[test]
    fn test_parse_mod_ident_missing() {
        let temp_dir = TempDir::new().unwrap();
        let mod_dir = temp_dir.path().join("no_modinfo");
        fs::create_dir(&mod_dir).unwrap();

        let result = parse_mod_ident(&mod_dir);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_mod_ident_invalid_json() {
        let temp_dir = TempDir::new().unwrap();
        let mod_dir = temp_dir.path().join("invalid_mod");
        fs::create_dir(&mod_dir).unwrap();

        fs::write(mod_dir.join("modinfo.json"), "not valid json {{{").unwrap();

        let result = parse_mod_ident(&mod_dir);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_tileset_name() {
        let temp_dir = TempDir::new().unwrap();
        let tileset_dir = temp_dir.path().join("test_tileset");
        fs::create_dir(&tileset_dir).unwrap();

        let tileset_txt = "NAME My Custom Tileset\nVIEW normals\nJSON tileset.json";
        fs::write(tileset_dir.join("tileset.txt"), tileset_txt).unwrap();

        let result = parse_tileset_info(&tileset_dir);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "My Custom Tileset");
    }

    #[test]
    fn test_parse_tileset_name_with_comma() {
        let temp_dir = TempDir::new().unwrap();
        let tileset_dir = temp_dir.path().join("tileset_comma");
        fs::create_dir(&tileset_dir).unwrap();

        // Commas should be stripped from the name
        let tileset_txt = "NAME Tileset, With, Commas";
        fs::write(tileset_dir.join("tileset.txt"), tileset_txt).unwrap();

        let result = parse_tileset_info(&tileset_dir);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "Tileset With Commas");
    }

    #[test]
    fn test_parse_tileset_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let tileset_dir = temp_dir.path().join("disabled_tileset");
        fs::create_dir(&tileset_dir).unwrap();

        let tileset_txt = "NAME Disabled Tileset";
        fs::write(tileset_dir.join("tileset.txt.disabled"), tileset_txt).unwrap();

        let result = parse_tileset_info(&tileset_dir);
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.name, "Disabled Tileset");
    }

    #[test]
    fn test_parse_soundpack_name() {
        let temp_dir = TempDir::new().unwrap();
        let soundpack_dir = temp_dir.path().join("test_soundpack");
        fs::create_dir(&soundpack_dir).unwrap();

        let soundpack_txt = "NAME Custom Soundpack\n";
        fs::write(soundpack_dir.join("soundpack.txt"), soundpack_txt).unwrap();

        let result = parse_soundpack_info(&soundpack_dir);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "Custom Soundpack");
    }

    #[test]
    fn test_find_custom_mods_set_difference() {
        let mut old_mods = HashMap::new();
        old_mods.insert(
            "official_mod".to_string(),
            ModInfo {
                id: "official_mod".to_string(),
                path: PathBuf::from("/old/official_mod"),
            },
        );
        old_mods.insert(
            "custom_mod".to_string(),
            ModInfo {
                id: "custom_mod".to_string(),
                path: PathBuf::from("/old/custom_mod"),
            },
        );

        let mut new_mods = HashMap::new();
        new_mods.insert(
            "official_mod".to_string(),
            ModInfo {
                id: "official_mod".to_string(),
                path: PathBuf::from("/new/official_mod"),
            },
        );

        let custom = find_custom_mods(&old_mods, &new_mods);

        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].id, "custom_mod");
    }

    #[test]
    fn test_find_custom_tilesets_set_difference() {
        let mut old_tilesets = HashMap::new();
        old_tilesets.insert(
            "UltimateCataclysm".to_string(),
            TilesetInfo {
                name: "UltimateCataclysm".to_string(),
                path: PathBuf::from("/old/gfx/UltiCa"),
            },
        );
        old_tilesets.insert(
            "MyCustomTileset".to_string(),
            TilesetInfo {
                name: "MyCustomTileset".to_string(),
                path: PathBuf::from("/old/gfx/MyCustom"),
            },
        );

        let mut new_tilesets = HashMap::new();
        new_tilesets.insert(
            "UltimateCataclysm".to_string(),
            TilesetInfo {
                name: "UltimateCataclysm".to_string(),
                path: PathBuf::from("/new/gfx/UltiCa"),
            },
        );

        let custom = find_custom_tilesets(&old_tilesets, &new_tilesets);

        assert_eq!(custom.len(), 1);
        assert_eq!(custom[0].name, "MyCustomTileset");
    }

    #[test]
    fn test_find_custom_fonts() {
        let temp_dir = TempDir::new().unwrap();
        let old_font_dir = temp_dir.path().join("old_font");
        fs::create_dir(&old_font_dir).unwrap();

        let mut old_fonts = HashSet::new();
        old_fonts.insert("official.ttf".to_string());
        old_fonts.insert("custom.ttf".to_string());

        let mut new_fonts = HashSet::new();
        new_fonts.insert("official.ttf".to_string());

        let custom = find_custom_fonts(&old_fonts, &new_fonts, &old_font_dir);

        assert_eq!(custom.len(), 1);
        assert!(custom[0].ends_with("custom.ttf"));
    }

    #[test]
    fn test_config_skip_files_includes_debug_logs() {
        let skip_files = config_skip_files();
        assert!(skip_files.iter().any(|f| f == "debug.log"));
        assert!(skip_files.iter().any(|f| f == "debug.log.prev"));
    }

    #[test]
    fn test_scan_mods_directory() {
        let temp_dir = TempDir::new().unwrap();
        let mods_dir = temp_dir.path().join("mods");
        fs::create_dir(&mods_dir).unwrap();

        // Create two mods
        let mod1_dir = mods_dir.join("mod1");
        fs::create_dir(&mod1_dir).unwrap();
        fs::write(
            mod1_dir.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "mod_one"}"#,
        )
        .unwrap();

        let mod2_dir = mods_dir.join("mod2");
        fs::create_dir(&mod2_dir).unwrap();
        fs::write(
            mod2_dir.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "mod_two"}"#,
        )
        .unwrap();

        let mods = scan_mods_directory(&mods_dir);

        assert_eq!(mods.len(), 2);
        assert!(mods.contains_key("mod_one"));
        assert!(mods.contains_key("mod_two"));
    }

    #[test]
    fn test_scan_mods_directory_nonexistent() {
        let mods = scan_mods_directory(Path::new("/nonexistent/path"));
        assert!(mods.is_empty());
    }

    #[test]
    fn test_create_migration_plan() {
        let temp_dir = TempDir::new().unwrap();
        let previous_dir = temp_dir.path().join(".phoenix_archive");
        let game_dir = temp_dir.path().join("game");

        // Set up previous version with custom mod
        let prev_mods = previous_dir.join("data/mods/custom_mod");
        fs::create_dir_all(&prev_mods).unwrap();
        fs::write(
            prev_mods.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "my_custom_mod"}"#,
        )
        .unwrap();

        // Set up previous version with official mod
        let prev_official = previous_dir.join("data/mods/official_mod");
        fs::create_dir_all(&prev_official).unwrap();
        fs::write(
            prev_official.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "official_mod"}"#,
        )
        .unwrap();

        // Set up new version with only official mod
        let new_mods = game_dir.join("data/mods/official_mod");
        fs::create_dir_all(&new_mods).unwrap();
        fs::write(
            new_mods.join("modinfo.json"),
            r#"{"type": "MOD_INFO", "id": "official_mod"}"#,
        )
        .unwrap();

        let plan = create_migration_plan(&previous_dir, &game_dir);

        // Should find only custom_mod as needing restoration
        assert_eq!(plan.custom_mods.len(), 1);
        assert_eq!(plan.custom_mods[0].id, "my_custom_mod");
    }

    #[test]
    fn test_scan_soundpack_files() {
        let temp_dir = TempDir::new().unwrap();
        let soundpack_dir = temp_dir.path().join("test_soundpack");
        fs::create_dir_all(soundpack_dir.join("music")).unwrap();
        fs::create_dir_all(soundpack_dir.join("guns")).unwrap();

        // Create various files
        fs::write(soundpack_dir.join("soundpack.txt"), "NAME Test\n").unwrap();
        fs::write(soundpack_dir.join("soundset.json"), "{}").unwrap();
        fs::write(soundpack_dir.join("music").join("theme.ogg"), b"audio").unwrap();
        fs::write(soundpack_dir.join("guns").join("shot.wav"), b"audio").unwrap();
        fs::write(soundpack_dir.join("readme.txt"), "text").unwrap(); // Should be ignored

        let files = scan_soundpack_files(&soundpack_dir);

        // Should have 3 files: soundset.json, theme.ogg, shot.wav
        // readme.txt and soundpack.txt should not be included
        assert_eq!(files.len(), 3);
        assert!(files.contains(&PathBuf::from("soundset.json")));
        assert!(files.contains(&PathBuf::from("music").join("theme.ogg")));
        assert!(files.contains(&PathBuf::from("guns").join("shot.wav")));
    }

    #[test]
    fn test_find_custom_soundpack_files() {
        let temp_dir = TempDir::new().unwrap();

        // Old soundpack with custom music
        let old_dir = temp_dir.path().join("old_soundpack");
        fs::create_dir_all(old_dir.join("music")).unwrap();
        fs::write(old_dir.join("soundset.json"), "{}").unwrap();
        fs::write(old_dir.join("music").join("official.ogg"), b"audio").unwrap();
        fs::write(old_dir.join("music").join("custom_music.ogg"), b"custom").unwrap();

        // New soundpack (official only)
        let new_dir = temp_dir.path().join("new_soundpack");
        fs::create_dir_all(new_dir.join("music")).unwrap();
        fs::write(new_dir.join("soundset.json"), "{}").unwrap();
        fs::write(new_dir.join("music").join("official.ogg"), b"audio").unwrap();

        let custom = find_custom_soundpack_files(&old_dir, &new_dir);

        assert_eq!(custom.len(), 1);
        assert!(custom.contains(&PathBuf::from("music").join("custom_music.ogg")));
    }

    #[test]
    fn test_create_migration_plan_with_soundpack_merge() {
        let temp_dir = TempDir::new().unwrap();
        let previous_dir = temp_dir.path().join(".phoenix_archive");
        let game_dir = temp_dir.path().join("game");

        // Set up old soundpack with custom file
        let old_soundpack = previous_dir.join("data/sound/CC-Sounds");
        fs::create_dir_all(old_soundpack.join("music")).unwrap();
        fs::write(old_soundpack.join("soundpack.txt"), "NAME CC-Sounds\n").unwrap();
        fs::write(old_soundpack.join("soundset.json"), "{}").unwrap();
        fs::write(old_soundpack.join("music").join("custom.ogg"), b"custom").unwrap();

        // Set up new soundpack (same name, no custom file)
        let new_soundpack = game_dir.join("data/sound/CC-Sounds");
        fs::create_dir_all(new_soundpack.join("music")).unwrap();
        fs::write(new_soundpack.join("soundpack.txt"), "NAME CC-Sounds\n").unwrap();
        fs::write(new_soundpack.join("soundset.json"), "{}").unwrap();

        let plan = create_migration_plan(&previous_dir, &game_dir);

        // Should have no custom soundpacks (same NAME exists in both)
        assert!(plan.custom_soundpacks.is_empty());

        // Should have one merge with one custom file
        assert_eq!(plan.soundpack_merges.len(), 1);
        assert_eq!(plan.soundpack_merges[0].name, "CC-Sounds");
        assert_eq!(plan.soundpack_merges[0].custom_files.len(), 1);
    }
}
