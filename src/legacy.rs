//! Legacy data migration for Phoenix.
//!
//! Handles one-time migration of data from old locations to new locations:
//! - save_backups folder → AppData/backups
//! - previous_version folder → .phoenix_archive

use std::path::Path;
use std::time::Instant;

use crate::app_data::launcher_config;
use crate::backup;
use crate::config::Config;

/// Migrate legacy data from old locations to new locations.
pub fn migrate(game_dir: &Path) {
    let phase_start = Instant::now();
    let mut migrations = Vec::new();

    if let Some(count) = migrate_backups_to_appdata(game_dir) {
        migrations.push(format!("{} backups to AppData", count));
    }

    if migrate_archive_folder(game_dir) {
        migrations.push("previous_version → .phoenix_archive".to_string());
    }

    if !migrations.is_empty() {
        tracing::info!(
            "Legacy migration in {:.1}ms: {}",
            phase_start.elapsed().as_secs_f32() * 1000.0,
            migrations.join(", ")
        );
    }
}

/// Move save_backups from game folder to AppData.
/// Returns the number of backups moved, or None if nothing to migrate.
fn migrate_backups_to_appdata(game_dir: &Path) -> Option<usize> {
    let legacy_dir = backup::legacy_backup_dir(game_dir);
    if !legacy_dir.exists() || !legacy_dir.is_dir() {
        return None;
    }

    let new_dir = Config::backups_dir().ok()?;
    let entries = std::fs::read_dir(&legacy_dir).ok()?;

    let mut moved = 0;
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = new_dir.join(entry.file_name());

        if dst.exists() {
            continue;
        }

        // Try rename first (fast), fall back to copy+delete
        if std::fs::rename(&src, &dst).is_err() {
            if std::fs::copy(&src, &dst).is_ok() {
                let _ = std::fs::remove_file(&src);
                moved += 1;
            }
        } else {
            moved += 1;
        }
    }

    // Remove legacy directory if empty
    if std::fs::read_dir(&legacy_dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(false)
    {
        let _ = std::fs::remove_dir(&legacy_dir);
    }

    if moved > 0 { Some(moved) } else { None }
}

/// Rename previous_version to .phoenix_archive.
/// Returns true if migration was performed.
fn migrate_archive_folder(game_dir: &Path) -> bool {
    let old = game_dir.join(&launcher_config().legacy.old_archive_dir);
    let new = game_dir.join(".phoenix_archive");

    if old.exists() && !new.exists() {
        std::fs::rename(&old, &new).is_ok()
    } else {
        false
    }
}
