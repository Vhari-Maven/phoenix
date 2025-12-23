//! Configuration management commands

use anyhow::Result;
use clap::Subcommand;
use serde::Serialize;

use crate::cli::output::{print_formatted, OutputFormat};
use crate::config::Config;

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Show current configuration
    Show,

    /// Get a specific config value
    Get {
        /// Config key (e.g., "game.directory", "launcher.theme")
        key: String,
    },

    /// Set a config value
    Set {
        /// Config key (e.g., "game.directory", "launcher.theme")
        key: String,

        /// Value to set
        value: String,
    },

    /// Show config file path
    Path,
}

#[derive(Serialize)]
struct ConfigPathResult {
    path: String,
    exists: bool,
}

pub async fn run(command: ConfigCommands, format: OutputFormat, _quiet: bool) -> Result<()> {
    match command {
        ConfigCommands::Show => show(format).await,
        ConfigCommands::Get { key } => get(&key, format).await,
        ConfigCommands::Set { key, value } => set(&key, &value).await,
        ConfigCommands::Path => path(format).await,
    }
}

async fn show(format: OutputFormat) -> Result<()> {
    let config = Config::load()?;

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&config)?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            let toml = toml::to_string_pretty(&config)?;
            println!("{}", toml);
        }
    }

    Ok(())
}

async fn get(key: &str, format: OutputFormat) -> Result<()> {
    let config = Config::load()?;

    // Parse dotted key path and extract value
    let value = get_config_value(&config, key)?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(&value)?);
        }
        OutputFormat::Text => {
            println!("{}", value);
        }
    }

    Ok(())
}

fn get_config_value(config: &Config, key: &str) -> Result<String> {
    let parts: Vec<&str> = key.split('.').collect();

    match parts.as_slice() {
        ["launcher", "theme"] => Ok(format!("{:?}", config.launcher.theme)),
        ["launcher", "keep_open"] => Ok(config.launcher.keep_open.to_string()),
        ["launcher", "locale"] => Ok(config.launcher.locale.clone()),
        ["game", "directory"] => Ok(config
            .game
            .directory
            .clone()
            .unwrap_or_else(|| "<not set>".to_string())),
        ["game", "branch"] => Ok(config.game.branch.clone()),
        ["game", "command_params"] => Ok(config.game.command_params.clone()),
        ["updates", "check_on_startup"] => Ok(config.updates.check_on_startup.to_string()),
        ["updates", "prevent_save_move"] => Ok(config.updates.prevent_save_move.to_string()),
        ["updates", "remove_previous_version"] => {
            Ok(config.updates.remove_previous_version.to_string())
        }
        ["backups", "max_count"] => Ok(config.backups.max_count.to_string()),
        ["backups", "compression_level"] => Ok(config.backups.compression_level.to_string()),
        ["backups", "backup_on_launch"] => Ok(config.backups.backup_on_launch.to_string()),
        ["backups", "backup_on_end"] => Ok(config.backups.backup_on_end.to_string()),
        ["backups", "backup_before_update"] => Ok(config.backups.backup_before_update.to_string()),
        _ => anyhow::bail!("Unknown config key: {}", key),
    }
}

async fn set(key: &str, value: &str) -> Result<()> {
    let mut config = Config::load()?;

    set_config_value(&mut config, key, value)?;
    config.save()?;

    println!("Set {} = {}", key, value);
    Ok(())
}

fn set_config_value(config: &mut Config, key: &str, value: &str) -> Result<()> {
    let parts: Vec<&str> = key.split('.').collect();

    match parts.as_slice() {
        ["launcher", "keep_open"] => {
            config.launcher.keep_open = value.parse()?;
        }
        ["launcher", "locale"] => {
            config.launcher.locale = value.to_string();
        }
        ["game", "directory"] => {
            config.game.directory = Some(value.to_string());
        }
        ["game", "branch"] => {
            config.game.branch = value.to_string();
        }
        ["game", "command_params"] => {
            config.game.command_params = value.to_string();
        }
        ["updates", "check_on_startup"] => {
            config.updates.check_on_startup = value.parse()?;
        }
        ["updates", "prevent_save_move"] => {
            config.updates.prevent_save_move = value.parse()?;
        }
        ["updates", "remove_previous_version"] => {
            config.updates.remove_previous_version = value.parse()?;
        }
        ["backups", "max_count"] => {
            config.backups.max_count = value.parse()?;
        }
        ["backups", "compression_level"] => {
            config.backups.compression_level = value.parse()?;
        }
        ["backups", "backup_on_launch"] => {
            config.backups.backup_on_launch = value.parse()?;
        }
        ["backups", "backup_on_end"] => {
            config.backups.backup_on_end = value.parse()?;
        }
        ["backups", "backup_before_update"] => {
            config.backups.backup_before_update = value.parse()?;
        }
        _ => anyhow::bail!("Unknown or read-only config key: {}", key),
    }

    Ok(())
}

async fn path(format: OutputFormat) -> Result<()> {
    let path = Config::config_path()?;
    let exists = path.exists();

    let result = ConfigPathResult {
        path: path.to_string_lossy().to_string(),
        exists,
    };

    print_formatted(&result, format, |r| {
        format!("{}{}", r.path, if r.exists { "" } else { " (not found)" })
    });

    Ok(())
}
