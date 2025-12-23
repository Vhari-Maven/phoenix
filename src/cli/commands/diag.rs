//! Diagnostic and debugging commands

use anyhow::Result;
use clap::Subcommand;
use serde::Serialize;

use crate::backup;
use crate::cli::output::{format_size, print_formatted, print_success, OutputFormat};
use crate::config::Config;
use crate::db::Database;
use crate::game;

#[derive(Subcommand, Debug)]
pub enum DiagCommands {
    /// Show all data paths (config, database, backups)
    Paths,

    /// Verify installation health
    Check,

    /// Clear the version hash cache
    ClearCache,
}

#[derive(Serialize)]
struct PathsResult {
    config_file: String,
    database: String,
    backups_dir: String,
    game_dir: Option<String>,
}

#[derive(Serialize)]
struct CheckResult {
    config_exists: bool,
    database_accessible: bool,
    cached_versions: usize,
    backups_dir_exists: bool,
    backup_count: usize,
    backups_size_bytes: u64,
    game_dir_exists: bool,
    game_executable_found: bool,
}

pub async fn run(command: DiagCommands, format: OutputFormat, quiet: bool) -> Result<()> {
    match command {
        DiagCommands::Paths => paths(format).await,
        DiagCommands::Check => check(format).await,
        DiagCommands::ClearCache => clear_cache(quiet).await,
    }
}

async fn paths(format: OutputFormat) -> Result<()> {
    let config = Config::load().ok();

    let result = PathsResult {
        config_file: Config::config_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<error>".to_string()),
        database: Database::db_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<error>".to_string()),
        backups_dir: Config::backups_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<error>".to_string()),
        game_dir: config.and_then(|c| c.game.directory),
    };

    print_formatted(&result, format, |r| {
        let mut lines = vec![
            format!("Config file:  {}", r.config_file),
            format!("Database:     {}", r.database),
            format!("Backups dir:  {}", r.backups_dir),
        ];

        if let Some(game_dir) = &r.game_dir {
            lines.push(format!("Game dir:     {}", game_dir));
        } else {
            lines.push("Game dir:     <not configured>".to_string());
        }

        lines.join("\n")
    });

    Ok(())
}

async fn check(format: OutputFormat) -> Result<()> {
    // Check config file
    let config_path = Config::config_path().ok();
    let config_exists = config_path.as_ref().is_some_and(|p| p.exists());
    let config = Config::load().ok();

    // Check database
    let db = Database::open().ok();
    let database_accessible = db.is_some();
    let cached_versions = db
        .as_ref()
        .and_then(|d| d.count_cached_versions().ok())
        .unwrap_or(0);

    // Check backups
    let backups_dir = Config::backups_dir().ok();
    let backups_dir_exists = backups_dir.as_ref().is_some_and(|p| p.exists());
    let backups = backup::list_backups().await.unwrap_or_default();
    let backup_count = backups.len();
    let backups_size: u64 = backups.iter().map(|b| b.compressed_size).sum();

    // Check game directory
    let game_dir = config.as_ref().and_then(|c| c.game.directory.clone());
    let game_dir_exists = game_dir
        .as_ref()
        .map(|d| std::path::Path::new(d).exists())
        .unwrap_or(false);

    let game_executable_found = if let Some(ref dir) = game_dir {
        game::detect_game_with_db(std::path::Path::new(dir), db.as_ref())
            .ok()
            .flatten()
            .is_some()
    } else {
        false
    };

    let result = CheckResult {
        config_exists,
        database_accessible,
        cached_versions,
        backups_dir_exists,
        backup_count,
        backups_size_bytes: backups_size,
        game_dir_exists,
        game_executable_found,
    };

    print_formatted(&result, format, |r| {
        let mut lines = Vec::new();

        print_status_line(&mut lines, r.config_exists, "Config file exists");
        print_status_line(
            &mut lines,
            r.database_accessible,
            &format!("Database accessible ({} cached versions)", r.cached_versions),
        );
        print_status_line(
            &mut lines,
            r.backups_dir_exists,
            &format!(
                "Backups directory exists ({} backups, {})",
                r.backup_count,
                format_size(r.backups_size_bytes)
            ),
        );
        print_status_line(&mut lines, r.game_dir_exists, "Game directory exists");
        print_status_line(&mut lines, r.game_executable_found, "Game executable found");

        lines.join("\n")
    });

    Ok(())
}

fn print_status_line(lines: &mut Vec<String>, ok: bool, message: &str) {
    if ok {
        lines.push(format!("[OK] {}", message));
    } else {
        lines.push(format!("[  ] {}", message));
    }
}

async fn clear_cache(quiet: bool) -> Result<()> {
    let db = Database::open()?;
    let count = db.clear_hash_cache()?;

    print_success(&format!("Cleared {} cached hashes", count), quiet);

    Ok(())
}
