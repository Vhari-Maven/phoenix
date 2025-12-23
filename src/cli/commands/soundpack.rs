//! Soundpack management commands

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;

use crate::cli::output::OutputFormat;

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

pub async fn run(command: SoundpackCommands, _format: OutputFormat, _quiet: bool) -> Result<()> {
    match command {
        SoundpackCommands::List => {
            println!("soundpack list: not yet implemented");
        }
        SoundpackCommands::Available => {
            println!("soundpack available: not yet implemented");
        }
        SoundpackCommands::Install { .. } => {
            println!("soundpack install: not yet implemented");
        }
        SoundpackCommands::Delete { .. } => {
            println!("soundpack delete: not yet implemented");
        }
        SoundpackCommands::Enable { .. } => {
            println!("soundpack enable: not yet implemented");
        }
        SoundpackCommands::Disable { .. } => {
            println!("soundpack disable: not yet implemented");
        }
    }
    Ok(())
}
