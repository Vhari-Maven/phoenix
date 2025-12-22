//! Smart migration logic for preserving custom content during updates.
//!
//! This module handles identity-based detection of custom mods, tilesets,
//! soundpacks, and fonts to avoid overwriting new official content with old versions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Files to skip during config restoration
pub const CONFIG_SKIP_FILES: &[&str] = &["debug.log", "debug.log.prev"];

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

/// Result of analyzing directories for custom content
#[derive(Debug, Default)]
pub struct MigrationPlan {
    /// Custom mods to restore (from data/mods/)
    pub custom_mods: Vec<ModInfo>,
    /// Custom user mods to restore (from mods/)
    pub custom_user_mods: Vec<ModInfo>,
    /// Custom tilesets to restore
    pub custom_tilesets: Vec<TilesetInfo>,
    /// Custom soundpacks to restore
    pub custom_soundpacks: Vec<SoundpackInfo>,
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
    let json_file = mod_dir.join("modinfo.json");
    let disabled_file = mod_dir.join("modinfo.json.disabled");

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
fn parse_asset_name(asset_dir: &Path, filename: &str) -> Option<String> {
    let normal_file = asset_dir.join(filename);
    let disabled_file = asset_dir.join(format!("{}.disabled", filename));

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

    for line in text.lines() {
        if line.starts_with("NAME") {
            // Find first space after NAME
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
    let name = parse_asset_name(tileset_dir, "tileset.txt")?;
    Some(TilesetInfo {
        name,
        path: tileset_dir.to_path_buf(),
    })
}

/// Parse soundpack.txt to get soundpack info
pub fn parse_soundpack_info(soundpack_dir: &Path) -> Option<SoundpackInfo> {
    let name = parse_asset_name(soundpack_dir, "soundpack.txt")?;
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
    plan.custom_soundpacks = find_custom_soundpacks(&old_soundpacks, &new_soundpacks);

    tracing::info!(
        "Found {} custom soundpacks out of {} total soundpacks",
        plan.custom_soundpacks.len(),
        old_soundpacks.len()
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
        assert!(CONFIG_SKIP_FILES.contains(&"debug.log"));
        assert!(CONFIG_SKIP_FILES.contains(&"debug.log.prev"));
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
}
