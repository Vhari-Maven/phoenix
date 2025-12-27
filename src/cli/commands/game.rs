//! Game detection and launching commands

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::app_data::migration_config;
use crate::cli::output::{print_error, print_formatted, print_success, should_show_progress, OutputFormat};
use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};
use crate::util::format_size;

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

    /// Export user data (saves, config, mods, etc.) for use with external builds
    Export {
        /// Output file path (defaults to Phoenix data directory)
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Compression level (0-9, default 6)
        #[arg(long, default_value = "6")]
        compression: u8,
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
        GameCommands::Export { output, compression } => export(output, compression, format, quiet).await,
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

#[derive(Serialize)]
struct ExportResult {
    output_path: String,
    compressed_size_bytes: u64,
    uncompressed_size_bytes: u64,
    file_count: usize,
    directories_exported: Vec<String>,
}

async fn export(
    output: Option<PathBuf>,
    compression: u8,
    format: OutputFormat,
    quiet: bool,
) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured. Set it in Phoenix settings first.")?;

    // Determine output path
    let output_path = match output {
        Some(p) => p,
        None => {
            let data_dir = Config::data_dir()?;
            data_dir.join("export.zip")
        }
    };

    // Collect directories that exist
    let export_dirs = &migration_config().export.directories;
    let mut dirs_to_export: Vec<(String, PathBuf)> = Vec::new();
    for dir_name in export_dirs {
        let dir_path = game_dir.join(dir_name);
        if dir_path.exists() && dir_path.is_dir() {
            dirs_to_export.push((dir_name.clone(), dir_path));
        }
    }

    if dirs_to_export.is_empty() {
        print_error("No exportable directories found in game directory");
        return Err(anyhow::anyhow!("Nothing to export"));
    }

    let show_progress = should_show_progress(quiet, format);
    let game_dir_clone = game_dir.clone();
    let output_path_clone = output_path.clone();
    let dirs_exported: Vec<String> = dirs_to_export.iter().map(|(name, _)| name.clone()).collect();

    // Run export in blocking task
    let (file_count, compressed_size, uncompressed_size) = tokio::task::spawn_blocking(move || {
        export_sync(
            &game_dir_clone,
            &output_path_clone,
            &dirs_to_export,
            compression,
            show_progress,
        )
    })
    .await??;

    let result = ExportResult {
        output_path: output_path.to_string_lossy().to_string(),
        compressed_size_bytes: compressed_size,
        uncompressed_size_bytes: uncompressed_size,
        file_count,
        directories_exported: dirs_exported,
    };

    print_formatted(&result, format, |r| {
        let mut lines = vec![
            format!("Exported to: {}", r.output_path),
            format!(
                "Size: {} ({} uncompressed)",
                format_size(r.compressed_size_bytes),
                format_size(r.uncompressed_size_bytes)
            ),
            format!("Files: {}", r.file_count),
            format!("Directories: {}", r.directories_exported.join(", ")),
        ];
        lines.push(String::new());
        lines.push("Extract this zip into your CDDA build directory to use your saves and settings.".to_string());
        lines.join("\n")
    });

    Ok(())
}

fn export_sync(
    game_dir: &Path,
    output_path: &Path,
    dirs_to_export: &[(String, PathBuf)],
    compression_level: u8,
    show_progress: bool,
) -> Result<(usize, u64, u64)> {
    // Collect all files to export
    let mut files_to_export: Vec<(PathBuf, String)> = Vec::new();

    for (_dir_name, dir_path) in dirs_to_export {
        for entry in WalkDir::new(dir_path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let path = entry.path().to_path_buf();
                // Create relative path from game_dir so extraction preserves structure
                let relative = path
                    .strip_prefix(game_dir)
                    .unwrap_or(&path);
                let relative_str = relative.to_string_lossy().replace('\\', "/");

                // Skip debug logs
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if filename == "debug.log" || filename == "debug.log.prev" {
                    continue;
                }

                files_to_export.push((path, relative_str));
            }
        }
    }

    let total_files = files_to_export.len();

    if total_files == 0 {
        return Err(anyhow::anyhow!("No files to export"));
    }

    // Create parent directory if needed
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create ZIP file
    let file = File::create(output_path)?;
    let mut zip = ZipWriter::new(file);

    let compression = if compression_level == 0 {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };

    let options = SimpleFileOptions::default()
        .compression_method(compression)
        .compression_level(Some(compression_level.min(9) as i64));

    let mut uncompressed_size: u64 = 0;

    for (i, (path, relative)) in files_to_export.iter().enumerate() {
        if show_progress {
            eprint!("\rExporting: {}/{}   ", i + 1, total_files);
        }

        // Read file content
        let mut file_content = Vec::new();
        let mut file = File::open(path)?;
        file.read_to_end(&mut file_content)?;

        uncompressed_size += file_content.len() as u64;

        // Add to ZIP
        zip.start_file(relative, options)?;
        zip.write_all(&file_content)?;
    }

    zip.finish()?;

    if show_progress {
        eprintln!(); // Clear progress line
    }

    // Get compressed size
    let compressed_size = std::fs::metadata(output_path)?.len();

    Ok((total_files, compressed_size, uncompressed_size))
}
