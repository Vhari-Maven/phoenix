//! CLI module for Phoenix launcher
//!
//! Provides command-line interface for all launcher operations.

mod commands;
mod output;

use clap::{Parser, Subcommand};

pub use output::OutputFormat;

/// Phoenix - CDDA Game Launcher
#[derive(Parser, Debug)]
#[command(name = "phoenix")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Output format
    #[command(flatten)]
    pub output: OutputOptions,

    #[command(subcommand)]
    pub command: Commands,
}

/// Output formatting options
#[derive(Parser, Debug, Clone)]
pub struct OutputOptions {
    /// Output in JSON format (for machine parsing)
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-essential output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Increase output verbosity
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

impl OutputOptions {
    pub fn format(&self) -> OutputFormat {
        if self.json {
            OutputFormat::Json
        } else {
            OutputFormat::Text
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Game detection and launching
    Game {
        #[command(subcommand)]
        command: commands::game::GameCommands,
    },

    /// Backup management
    Backup {
        #[command(subcommand)]
        command: commands::backup::BackupCommands,
    },

    /// Update management
    Update {
        #[command(subcommand)]
        command: commands::update::UpdateCommands,
    },

    /// Soundpack management
    Soundpack {
        #[command(subcommand)]
        command: commands::soundpack::SoundpackCommands,
    },

    /// Configuration management
    Config {
        #[command(subcommand)]
        command: commands::config::ConfigCommands,
    },

    /// Diagnostics and debugging
    Diag {
        #[command(subcommand)]
        command: commands::diag::DiagCommands,
    },
}

/// Run the CLI with parsed arguments
pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let format = cli.output.format();
    let quiet = cli.output.quiet;

    match cli.command {
        Commands::Game { command } => commands::game::run(command, format, quiet).await,
        Commands::Backup { command } => commands::backup::run(command, format, quiet).await,
        Commands::Update { command } => commands::update::run(command, format, quiet).await,
        Commands::Soundpack { command } => commands::soundpack::run(command, format, quiet).await,
        Commands::Config { command } => commands::config::run(command, format, quiet).await,
        Commands::Diag { command } => commands::diag::run(command, format, quiet).await,
    }
}
