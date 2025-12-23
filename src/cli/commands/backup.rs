//! Backup management commands

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use tokio::sync::watch;

use crate::backup::{self, BackupInfo, BackupProgress};
use crate::cli::output::{format_size, print_error, print_formatted, print_success, OutputFormat};
use crate::config::Config;

#[derive(Subcommand, Debug)]
pub enum BackupCommands {
    /// List all backups
    List,

    /// Create a new backup
    Create {
        /// Backup name (auto-generated if not specified)
        #[arg(long)]
        name: Option<String>,

        /// Compression level (0-9)
        #[arg(long, default_value = "6")]
        compression: u8,
    },

    /// Restore a backup
    Restore {
        /// Backup name to restore
        name: String,

        /// Skip creating safety backup before restore
        #[arg(long)]
        no_safety_backup: bool,

        /// Show what would happen without doing it
        #[arg(long)]
        dry_run: bool,
    },

    /// Delete a backup
    Delete {
        /// Backup name to delete
        name: Option<String>,

        /// Keep only the N most recent backups
        #[arg(long)]
        keep: Option<usize>,
    },

    /// Check backup archive integrity
    Verify {
        /// Backup name to verify
        name: String,
    },
}

#[derive(Serialize)]
struct BackupListResult {
    backups: Vec<BackupEntry>,
    total_count: usize,
    total_size_bytes: u64,
}

#[derive(Serialize)]
struct BackupEntry {
    name: String,
    compressed_size_bytes: u64,
    uncompressed_size_bytes: u64,
    worlds_count: u32,
    characters_count: u32,
    modified: String,
    is_auto: bool,
}

#[derive(Serialize)]
struct BackupCreateResult {
    name: String,
    compressed_size_bytes: u64,
    uncompressed_size_bytes: u64,
    compression_ratio: f32,
}

#[derive(Serialize)]
struct BackupVerifyResult {
    name: String,
    valid: bool,
    compressed_size_bytes: u64,
    uncompressed_size_bytes: u64,
    file_count: usize,
    error: Option<String>,
}

pub async fn run(command: BackupCommands, format: OutputFormat, quiet: bool) -> Result<()> {
    match command {
        BackupCommands::List => list(format).await,
        BackupCommands::Create { name, compression } => create(name, compression, format, quiet).await,
        BackupCommands::Restore { name, no_safety_backup, dry_run } => {
            restore(&name, !no_safety_backup, dry_run, format, quiet).await
        }
        BackupCommands::Delete { name, keep } => delete(name, keep, quiet).await,
        BackupCommands::Verify { name } => verify(&name, format).await,
    }
}

async fn list(format: OutputFormat) -> Result<()> {
    let backups = backup::list_backups().await?;

    let total_size: u64 = backups.iter().map(|b| b.compressed_size).sum();

    let entries: Vec<BackupEntry> = backups
        .iter()
        .map(|b| BackupEntry {
            name: b.name.clone(),
            compressed_size_bytes: b.compressed_size,
            uncompressed_size_bytes: b.uncompressed_size,
            worlds_count: b.worlds_count,
            characters_count: b.characters_count,
            modified: b.modified.format("%Y-%m-%d %H:%M:%S").to_string(),
            is_auto: b.is_auto,
        })
        .collect();

    let result = BackupListResult {
        total_count: entries.len(),
        total_size_bytes: total_size,
        backups: entries,
    };

    print_formatted(&result, format, |r| format_backup_list(r));

    Ok(())
}

fn format_backup_list(result: &BackupListResult) -> String {
    if result.backups.is_empty() {
        return "No backups found.".to_string();
    }

    let mut lines = vec![format!("Backups ({} total):\n", result.total_count)];

    // Header
    lines.push(format!(
        "{:<30} {:>10} {:>12} {:>8}",
        "NAME", "SIZE", "DATE", "WORLDS"
    ));
    lines.push("-".repeat(65));

    for backup in &result.backups {
        let auto_marker = if backup.is_auto { "*" } else { "" };
        let date = &backup.modified[..10]; // Just the date part
        lines.push(format!(
            "{:<30} {:>10} {:>12} {:>8}",
            format!("{}{}", backup.name, auto_marker),
            format_size(backup.compressed_size_bytes),
            date,
            backup.worlds_count
        ));
    }

    lines.push(String::new());
    lines.push(format!("Total: {}", format_size(result.total_size_bytes)));
    lines.push("* = automatic backup".to_string());

    lines.join("\n")
}

async fn create(name: Option<String>, compression: u8, format: OutputFormat, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    // Generate name if not provided
    let backup_name = name.unwrap_or_else(|| {
        let now = chrono::Local::now();
        format!("backup-{}", now.format("%Y-%m-%d-%H%M%S"))
    });

    // Create progress channel
    let (progress_tx, mut progress_rx) = watch::channel(BackupProgress::default());

    // Spawn progress reporter for non-quiet mode
    if !quiet {
        let format_clone = format;
        tokio::spawn(async move {
            while progress_rx.changed().await.is_ok() {
                let progress = progress_rx.borrow().clone();
                if format_clone == OutputFormat::Text {
                    eprint!(
                        "\r{}: {}/{}   ",
                        progress.phase.description(),
                        progress.files_processed,
                        progress.total_files
                    );
                }
            }
            if format_clone == OutputFormat::Text {
                eprintln!(); // Clear the line
            }
        });
    }

    let info = backup::create_backup(&game_dir, &backup_name, compression, progress_tx).await?;

    let result = BackupCreateResult {
        name: info.name.clone(),
        compressed_size_bytes: info.compressed_size,
        uncompressed_size_bytes: info.uncompressed_size,
        compression_ratio: info.compression_ratio(),
    };

    print_formatted(&result, format, |r| {
        format!(
            "Created backup: {}\nSize: {} (compressed from {}, {:.1}% reduction)",
            r.name,
            format_size(r.compressed_size_bytes),
            format_size(r.uncompressed_size_bytes),
            r.compression_ratio
        )
    });

    Ok(())
}

async fn restore(
    name: &str,
    backup_current: bool,
    dry_run: bool,
    format: OutputFormat,
    quiet: bool,
) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    // Find the backup
    let backups = backup::list_backups().await?;
    let backup_info = backups
        .iter()
        .find(|b| b.name == name)
        .context(format!("Backup '{}' not found", name))?;

    if dry_run {
        println!("Dry run - would restore backup: {}", name);
        println!("  Size: {}", format_size(backup_info.compressed_size));
        println!("  Worlds: {}", backup_info.worlds_count);
        println!("  Characters: {}", backup_info.characters_count);
        if backup_current {
            println!("  Would create safety backup of current saves first");
        }
        return Ok(());
    }

    // Create progress channel
    let (progress_tx, mut progress_rx) = watch::channel(BackupProgress::default());

    // Spawn progress reporter
    if !quiet {
        let format_clone = format;
        tokio::spawn(async move {
            while progress_rx.changed().await.is_ok() {
                let progress = progress_rx.borrow().clone();
                if format_clone == OutputFormat::Text {
                    eprint!(
                        "\r{}: {}/{}   ",
                        progress.phase.description(),
                        progress.files_processed,
                        progress.total_files
                    );
                }
            }
            if format_clone == OutputFormat::Text {
                eprintln!();
            }
        });
    }

    backup::restore_backup(
        &game_dir,
        name,
        backup_current,
        config.backups.compression_level,
        progress_tx,
    )
    .await?;

    print_success(&format!("Restored backup: {}", name), quiet);

    Ok(())
}

async fn delete(name: Option<String>, keep: Option<usize>, quiet: bool) -> Result<()> {
    match (name, keep) {
        (Some(backup_name), _) => {
            // Delete specific backup
            backup::delete_backup(&backup_name).await?;
            print_success(&format!("Deleted backup: {}", backup_name), quiet);
        }
        (None, Some(keep_count)) => {
            // Delete all but N most recent
            let mut backups = backup::list_backups().await?;

            if backups.len() <= keep_count {
                print_success(
                    &format!(
                        "Nothing to delete. Have {} backups, keeping {}.",
                        backups.len(),
                        keep_count
                    ),
                    quiet,
                );
                return Ok(());
            }

            // Sort by date, newest first
            backups.sort_by(|a, b| b.modified.cmp(&a.modified));

            let to_delete: Vec<_> = backups.into_iter().skip(keep_count).collect();
            let count = to_delete.len();

            for backup in to_delete {
                backup::delete_backup(&backup.name).await?;
            }

            print_success(&format!("Deleted {} old backups", count), quiet);
        }
        (None, None) => {
            print_error("Specify a backup name or use --keep N");
            return Err(anyhow::anyhow!("No backup specified"));
        }
    }

    Ok(())
}

async fn verify(name: &str, format: OutputFormat) -> Result<()> {
    let backups = backup::list_backups().await?;

    let backup_info = backups.iter().find(|b| b.name == name);

    match backup_info {
        Some(info) => {
            // Try to read the archive to verify it
            let verify_result = verify_archive(info).await;

            let result = BackupVerifyResult {
                name: info.name.clone(),
                valid: verify_result.is_ok(),
                compressed_size_bytes: info.compressed_size,
                uncompressed_size_bytes: info.uncompressed_size,
                file_count: verify_result.as_ref().copied().unwrap_or(0),
                error: verify_result.err().map(|e| e.to_string()),
            };

            print_formatted(&result, format, |r| {
                if r.valid {
                    format!(
                        "Backup '{}' is valid.\n  Files: {}\n  Size: {} (uncompressed: {})",
                        r.name,
                        r.file_count,
                        format_size(r.compressed_size_bytes),
                        format_size(r.uncompressed_size_bytes)
                    )
                } else {
                    format!(
                        "Backup '{}' is INVALID: {}",
                        r.name,
                        r.error.as_deref().unwrap_or("Unknown error")
                    )
                }
            });
        }
        None => {
            print_error(&format!("Backup '{}' not found", name));
            return Err(anyhow::anyhow!("Backup not found"));
        }
    }

    Ok(())
}

async fn verify_archive(info: &BackupInfo) -> Result<usize, anyhow::Error> {
    let path = info.path.clone();

    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        let mut count = 0;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            // Read the file to verify it's not corrupted
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buf)?;
            count += 1;
        }

        Ok(count)
    })
    .await?
}
