//! Game detection and launching commands

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;

use crate::cli::output::{format_size, print_error, print_formatted, print_success, OutputFormat};
use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};

#[derive(Subcommand, Debug)]
pub enum GameCommands {
    /// Detect game installation and version
    Detect {
        /// Game directory (uses configured directory if not specified)
        #[arg(long)]
        dir: Option<PathBuf>,
    },

    /// Launch the game
    Launch {
        /// Additional command-line parameters
        #[arg(long)]
        params: Option<String>,
    },

    /// Show detailed game information
    Info {
        /// Game directory (uses configured directory if not specified)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

/// JSON-serializable game detection result
#[derive(Serialize)]
struct DetectResult {
    detected: bool,
    version: Option<String>,
    branch: Option<String>,
    directory: String,
    executable: Option<String>,
    saves_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    released_on: Option<String>,
}

pub async fn run(command: GameCommands, format: OutputFormat, quiet: bool) -> Result<()> {
    match command {
        GameCommands::Detect { dir } => detect(dir, format, quiet).await,
        GameCommands::Launch { params } => launch(params, quiet).await,
        GameCommands::Info { dir } => info(dir, format, quiet).await,
    }
}

async fn detect(dir: Option<PathBuf>, format: OutputFormat, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = get_game_dir(dir, &config)?;

    // Open database for version lookup
    let db = Database::open().ok();

    let result = game::detect_game_with_db(&game_dir, db.as_ref())?;

    match result {
        Some(game_info) => {
            let detect_result = build_detect_result(&game_info, &game_dir, &config);
            print_formatted(&detect_result, format, |r| format_detect_text(r));
        }
        None => {
            let detect_result = DetectResult {
                detected: false,
                version: None,
                branch: None,
                directory: game_dir.to_string_lossy().to_string(),
                executable: None,
                saves_size_bytes: 0,
                released_on: None,
            };

            print_formatted(&detect_result, format, |_| {
                format!("No game detected in: {}", game_dir.display())
            });

            if !quiet {
                print_error("Game executable not found");
            }
        }
    }

    Ok(())
}

async fn launch(params: Option<String>, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = get_game_dir(None, &config)?;

    // Open database for version lookup
    let db = Database::open().ok();

    let game_info = game::detect_game_with_db(&game_dir, db.as_ref())?
        .context("No game detected. Configure game directory first.")?;

    // Combine configured params with CLI params
    let combined_params = match params {
        Some(p) => {
            if config.game.command_params.is_empty() {
                p
            } else {
                format!("{} {}", config.game.command_params, p)
            }
        }
        None => config.game.command_params.clone(),
    };

    game::launch_game(&game_info.executable, &combined_params)?;

    print_success(
        &format!("Launched: {}", game_info.executable.display()),
        quiet,
    );

    Ok(())
}

async fn info(dir: Option<PathBuf>, format: OutputFormat, _quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = get_game_dir(dir, &config)?;

    // Open database for version lookup
    let db = Database::open().ok();

    let result = game::detect_game_with_db(&game_dir, db.as_ref())?;

    match result {
        Some(game_info) => {
            // Calculate directory size
            let dir_size = game::calculate_dir_size(&game_dir).unwrap_or(0);

            let info_result = GameInfoResult {
                detected: true,
                version: Some(game_info.version_display().to_string()),
                branch: Some(determine_branch(&game_info)),
                directory: game_dir.to_string_lossy().to_string(),
                executable: Some(game_info.executable.to_string_lossy().to_string()),
                saves_size_bytes: game_info.saves_size,
                total_size_bytes: dir_size,
                released_on: game_info
                    .version_info
                    .as_ref()
                    .and_then(|v| v.released_on.clone()),
            };

            print_formatted(&info_result, format, |r| format_info_text(r));
        }
        None => {
            print_error(&format!("No game detected in: {}", game_dir.display()));
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct GameInfoResult {
    detected: bool,
    version: Option<String>,
    branch: Option<String>,
    directory: String,
    executable: Option<String>,
    saves_size_bytes: u64,
    total_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    released_on: Option<String>,
}

fn get_game_dir(dir: Option<PathBuf>, config: &Config) -> Result<PathBuf> {
    dir.or_else(|| config.game.directory.as_ref().map(PathBuf::from))
        .context("No game directory specified. Use --dir or configure in settings.")
}

fn build_detect_result(game_info: &GameInfo, game_dir: &PathBuf, _config: &Config) -> DetectResult {
    DetectResult {
        detected: true,
        version: Some(game_info.version_display().to_string()),
        branch: Some(determine_branch(game_info)),
        directory: game_dir.to_string_lossy().to_string(),
        executable: Some(game_info.executable.to_string_lossy().to_string()),
        saves_size_bytes: game_info.saves_size,
        released_on: game_info
            .version_info
            .as_ref()
            .and_then(|v| v.released_on.clone()),
    }
}

fn determine_branch(game_info: &GameInfo) -> String {
    if game_info.is_stable() {
        "stable".to_string()
    } else {
        "experimental".to_string()
    }
}

fn format_detect_text(result: &DetectResult) -> String {
    if !result.detected {
        return format!("No game detected in: {}", result.directory);
    }

    let mut lines = vec![
        format!("Game detected: Cataclysm: Dark Days Ahead"),
        format!(
            "Version: {} ({})",
            result.version.as_deref().unwrap_or("Unknown"),
            result.branch.as_deref().unwrap_or("unknown")
        ),
        format!("Directory: {}", result.directory),
    ];

    if let Some(exe) = &result.executable {
        lines.push(format!("Executable: {}", exe));
    }

    if result.saves_size_bytes > 0 {
        lines.push(format!("Saves size: {}", format_size(result.saves_size_bytes)));
    }

    lines.join("\n")
}

fn format_info_text(result: &GameInfoResult) -> String {
    if !result.detected {
        return format!("No game detected in: {}", result.directory);
    }

    let mut lines = vec![
        format!("Game: Cataclysm: Dark Days Ahead"),
        format!(
            "Version: {} ({})",
            result.version.as_deref().unwrap_or("Unknown"),
            result.branch.as_deref().unwrap_or("unknown")
        ),
        format!("Directory: {}", result.directory),
    ];

    if let Some(exe) = &result.executable {
        // Just show the filename, not full path
        let exe_name = std::path::Path::new(exe)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| exe.clone());
        lines.push(format!("Executable: {}", exe_name));
    }

    lines.push(format!("Total size: {}", format_size(result.total_size_bytes)));
    lines.push(format!("Saves size: {}", format_size(result.saves_size_bytes)));

    if let Some(released) = &result.released_on {
        lines.push(format!("Build: {}", released));
    }

    lines.join("\n")
}
