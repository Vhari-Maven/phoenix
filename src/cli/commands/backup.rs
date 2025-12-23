//! Backup management commands

use anyhow::Result;
use clap::Subcommand;

use crate::cli::output::OutputFormat;

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

pub async fn run(command: BackupCommands, _format: OutputFormat, _quiet: bool) -> Result<()> {
    match command {
        BackupCommands::List => {
            println!("backup list: not yet implemented");
        }
        BackupCommands::Create { .. } => {
            println!("backup create: not yet implemented");
        }
        BackupCommands::Restore { .. } => {
            println!("backup restore: not yet implemented");
        }
        BackupCommands::Delete { .. } => {
            println!("backup delete: not yet implemented");
        }
        BackupCommands::Verify { .. } => {
            println!("backup verify: not yet implemented");
        }
    }
    Ok(())
}
