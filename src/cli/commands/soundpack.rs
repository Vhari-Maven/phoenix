//! Soundpack management commands

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use tokio::sync::watch;

use crate::cli::output::{print_error, print_formatted, print_success, should_show_progress, OutputFormat};
use crate::config::Config;
use crate::github::GitHubClient;
use crate::soundpack::{self, SoundpackProgress};
use crate::util::format_size;

#[derive(Subcommand, Debug)]
pub enum SoundpackCommands {
    /// List installed soundpacks
    List,

    /// List soundpacks available for download
    Available,

    /// Install a soundpack
    Install {
        /// Soundpack name from repository
        name: Option<String>,

        /// Install from local file
        #[arg(long)]
        file: Option<PathBuf>,
    },

    /// Delete a soundpack
    Delete {
        /// Soundpack name to delete
        name: String,
    },

    /// Enable a soundpack
    Enable {
        /// Soundpack name to enable
        name: String,
    },

    /// Disable a soundpack
    Disable {
        /// Soundpack name to disable
        name: String,
    },
}

#[derive(Serialize)]
struct InstalledEntry {
    name: String,
    view_name: String,
    enabled: bool,
    size_bytes: u64,
}

#[derive(Serialize)]
struct InstalledListResult {
    soundpacks: Vec<InstalledEntry>,
    total_count: usize,
    total_size_bytes: u64,
}

#[derive(Serialize)]
struct AvailableEntry {
    name: String,
    description: String,
    size_bytes: Option<u64>,
}

#[derive(Serialize)]
struct AvailableListResult {
    soundpacks: Vec<AvailableEntry>,
}

pub async fn run(command: SoundpackCommands, format: OutputFormat, quiet: bool) -> Result<()> {
    match command {
        SoundpackCommands::List => list(format).await,
        SoundpackCommands::Available => available(format).await,
        SoundpackCommands::Install { name, file } => install(name, file, format, quiet).await,
        SoundpackCommands::Delete { name } => delete(&name, quiet).await,
        SoundpackCommands::Enable { name } => set_enabled(&name, true, quiet).await,
        SoundpackCommands::Disable { name } => set_enabled(&name, false, quiet).await,
    }
}

async fn list(format: OutputFormat) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    let soundpacks = soundpack::list_installed_soundpacks(&game_dir).await?;

    let total_size: u64 = soundpacks.iter().map(|s| s.size).sum();

    let entries: Vec<InstalledEntry> = soundpacks
        .iter()
        .map(|s| InstalledEntry {
            name: s.name.clone(),
            view_name: s.view_name.clone(),
            enabled: s.enabled,
            size_bytes: s.size,
        })
        .collect();

    let result = InstalledListResult {
        total_count: entries.len(),
        total_size_bytes: total_size,
        soundpacks: entries,
    };

    print_formatted(&result, format, |r| {
        if r.soundpacks.is_empty() {
            return "No soundpacks installed.".to_string();
        }

        let mut lines = vec![format!("Installed soundpacks ({}):\n", r.total_count)];

        lines.push(format!(
            "{:<30} {:>10} {:>8}",
            "NAME", "SIZE", "STATUS"
        ));
        lines.push("-".repeat(52));

        for sp in &r.soundpacks {
            let status = if sp.enabled { "enabled" } else { "disabled" };
            lines.push(format!(
                "{:<30} {:>10} {:>8}",
                sp.view_name,
                format_size(sp.size_bytes),
                status
            ));
        }

        lines.push(String::new());
        lines.push(format!("Total: {}", format_size(r.total_size_bytes)));

        lines.join("\n")
    });

    Ok(())
}

async fn available(format: OutputFormat) -> Result<()> {
    let repo = soundpack::load_repository();

    let entries: Vec<AvailableEntry> = repo
        .iter()
        .map(|s| AvailableEntry {
            name: s.name.clone(),
            description: s.viewname.clone(),
            size_bytes: s.size,
        })
        .collect();

    let result = AvailableListResult { soundpacks: entries };

    print_formatted(&result, format, |r| {
        if r.soundpacks.is_empty() {
            return "No soundpacks available in repository.".to_string();
        }

        let mut lines = vec!["Available soundpacks:\n".to_string()];

        lines.push(format!("{:<30} {:>10} {}", "NAME", "SIZE", "DESCRIPTION"));
        lines.push("-".repeat(70));

        for sp in &r.soundpacks {
            let size = sp
                .size_bytes
                .map(format_size)
                .unwrap_or_else(|| "?".to_string());
            // Truncate description if too long
            let desc = if sp.description.len() > 30 {
                format!("{}...", &sp.description[..27])
            } else {
                sp.description.clone()
            };
            lines.push(format!("{:<30} {:>10} {}", sp.name, size, desc));
        }

        lines.join("\n")
    });

    Ok(())
}

async fn install(
    name: Option<String>,
    file: Option<PathBuf>,
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

    // Create progress channel
    let (progress_tx, mut progress_rx) = watch::channel(SoundpackProgress::default());

    // Spawn progress reporter (only if TTY and not quiet)
    let show_progress = should_show_progress(quiet, format);
    if show_progress {
        tokio::spawn(async move {
            while progress_rx.changed().await.is_ok() {
                let p = progress_rx.borrow().clone();
                match p.phase {
                    soundpack::SoundpackPhase::Downloading => {
                        if p.total_bytes > 0 {
                            let percent =
                                (p.bytes_downloaded as f64 / p.total_bytes as f64 * 100.0) as u32;
                            eprint!(
                                "\rDownloading: {}% ({})   ",
                                percent,
                                format_size(p.bytes_downloaded)
                            );
                        }
                    }
                    soundpack::SoundpackPhase::Extracting => {
                        eprint!(
                            "\rExtracting: {}/{}   ",
                            p.files_extracted, p.total_files
                        );
                    }
                    _ => {
                        eprint!("\r{}   ", p.phase.description());
                    }
                }
            }
            eprintln!();
        });
    }

    match (name, file) {
        (_, Some(archive_path)) => {
            // Install from local file
            let result = soundpack::install_from_file(archive_path.clone(), game_dir, progress_tx)
                .await?;

            print_success(
                &format!(
                    "Installed soundpack: {} ({})",
                    result.view_name,
                    format_size(result.size)
                ),
                quiet,
            );
        }
        (Some(name), None) => {
            // Install from repository
            let repo = soundpack::load_repository();
            let repo_pack = repo
                .iter()
                .find(|s| s.name.to_lowercase() == name.to_lowercase())
                .context(format!("Soundpack '{}' not found in repository", name))?;

            let client = GitHubClient::new()?;
            let result = soundpack::install_soundpack(
                client.client().clone(),
                repo_pack.clone(),
                game_dir,
                progress_tx,
            )
            .await?;

            print_success(
                &format!(
                    "Installed soundpack: {} ({})",
                    result.view_name,
                    format_size(result.size)
                ),
                quiet,
            );
        }
        (None, None) => {
            print_error("Specify a soundpack name or use --file");
            return Err(anyhow::anyhow!("No soundpack specified"));
        }
    }

    Ok(())
}

async fn delete(name: &str, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    let soundpacks = soundpack::list_installed_soundpacks(&game_dir).await?;

    let found = soundpacks.iter().find(|s| {
        s.name.to_lowercase() == name.to_lowercase()
            || s.view_name.to_lowercase() == name.to_lowercase()
    });

    match found {
        Some(sp) => {
            soundpack::delete_soundpack(sp.path.clone()).await?;
            print_success(&format!("Deleted soundpack: {}", sp.view_name), quiet);
        }
        None => {
            print_error(&format!("Soundpack '{}' not found", name));
            return Err(anyhow::anyhow!("Soundpack not found"));
        }
    }

    Ok(())
}

async fn set_enabled(name: &str, enabled: bool, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    let soundpacks = soundpack::list_installed_soundpacks(&game_dir).await?;

    let found = soundpacks.iter().find(|s| {
        s.name.to_lowercase() == name.to_lowercase()
            || s.view_name.to_lowercase() == name.to_lowercase()
    });

    match found {
        Some(sp) => {
            soundpack::set_soundpack_enabled(&sp.path, enabled).await?;
            let action = if enabled { "Enabled" } else { "Disabled" };
            print_success(&format!("{} soundpack: {}", action, sp.view_name), quiet);
        }
        None => {
            print_error(&format!("Soundpack '{}' not found", name));
            return Err(anyhow::anyhow!("Soundpack not found"));
        }
    }

    Ok(())
}
