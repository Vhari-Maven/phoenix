//! Update management commands

use anyhow::Result;
use clap::Subcommand;

use crate::cli::output::OutputFormat;

#[derive(Subcommand, Debug)]
pub enum UpdateCommands {
    /// Check for available updates
    Check,

    /// List available releases
    Releases {
        /// Maximum number of releases to show
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Filter by branch (stable or experimental)
        #[arg(long)]
        branch: Option<String>,
    },

    /// Download an update (without installing)
    Download {
        /// Specific version to download
        #[arg(long)]
        version: Option<String>,
    },

    /// Install a downloaded update
    Install,

    /// Download and install in one step
    Apply {
        /// Keep saves in place during update
        #[arg(long)]
        keep_saves: bool,

        /// Remove previous version after update
        #[arg(long)]
        remove_old: bool,

        /// Show what would happen without doing it
        #[arg(long)]
        dry_run: bool,
    },
}

pub async fn run(command: UpdateCommands, _format: OutputFormat, _quiet: bool) -> Result<()> {
    match command {
        UpdateCommands::Check => {
            println!("update check: not yet implemented");
        }
        UpdateCommands::Releases { .. } => {
            println!("update releases: not yet implemented");
        }
        UpdateCommands::Download { .. } => {
            println!("update download: not yet implemented");
        }
        UpdateCommands::Install => {
            println!("update install: not yet implemented");
        }
        UpdateCommands::Apply { .. } => {
            println!("update apply: not yet implemented");
        }
    }
    Ok(())
}
