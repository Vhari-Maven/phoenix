//! Interactive shell mode for Phoenix CLI
//!
//! Provides a REPL with command history and tab completion.

use anyhow::Result;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Config, Editor, Helper};

use super::commands;
use super::output::OutputFormat;

/// Command completer for the shell
#[derive(Default)]
struct ShellCompleter {
    commands: Vec<(&'static str, Vec<&'static str>)>,
}

impl ShellCompleter {
    fn new() -> Self {
        Self {
            commands: vec![
                ("game", vec!["detect", "launch", "info"]),
                ("backup", vec!["list", "create", "restore", "delete", "verify"]),
                ("update", vec!["check", "releases", "download", "install", "apply"]),
                ("soundpack", vec!["list", "available", "install", "delete", "enable", "disable"]),
                ("config", vec!["show", "get", "set", "path"]),
                ("diag", vec!["paths", "check", "clear-cache"]),
                ("help", vec![]),
                ("exit", vec![]),
                ("quit", vec![]),
            ],
        }
    }
}

impl Completer for ShellCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let line = &line[..pos];
        let words: Vec<&str> = line.split_whitespace().collect();

        match words.len() {
            0 => {
                // Empty line - suggest all commands
                let candidates: Vec<Pair> = self
                    .commands
                    .iter()
                    .map(|(cmd, _)| Pair {
                        display: cmd.to_string(),
                        replacement: cmd.to_string(),
                    })
                    .collect();
                Ok((0, candidates))
            }
            1 => {
                // Partial first word - complete command names
                let prefix = words[0];
                if line.ends_with(' ') {
                    // Command complete, suggest subcommands
                    if let Some((_, subs)) = self.commands.iter().find(|(cmd, _)| *cmd == prefix) {
                        let candidates: Vec<Pair> = subs
                            .iter()
                            .map(|sub| Pair {
                                display: sub.to_string(),
                                replacement: sub.to_string(),
                            })
                            .collect();
                        return Ok((pos, candidates));
                    }
                    Ok((pos, vec![]))
                } else {
                    // Still typing command
                    let candidates: Vec<Pair> = self
                        .commands
                        .iter()
                        .filter(|(cmd, _)| cmd.starts_with(prefix))
                        .map(|(cmd, _)| Pair {
                            display: cmd.to_string(),
                            replacement: cmd.to_string(),
                        })
                        .collect();
                    let start = line.rfind(' ').map(|i| i + 1).unwrap_or(0);
                    Ok((start, candidates))
                }
            }
            2 => {
                // Complete subcommand
                let cmd = words[0];
                let prefix = words[1];
                if let Some((_, subs)) = self.commands.iter().find(|(c, _)| *c == cmd) {
                    let candidates: Vec<Pair> = if line.ends_with(' ') {
                        // Subcommand complete, no more suggestions
                        vec![]
                    } else {
                        subs.iter()
                            .filter(|sub| sub.starts_with(prefix))
                            .map(|sub| Pair {
                                display: sub.to_string(),
                                replacement: sub.to_string(),
                            })
                            .collect()
                    };
                    let start = line.rfind(' ').map(|i| i + 1).unwrap_or(0);
                    return Ok((start, candidates));
                }
                Ok((pos, vec![]))
            }
            _ => Ok((pos, vec![])),
        }
    }
}

impl Hinter for ShellCompleter {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        None
    }
}

impl Highlighter for ShellCompleter {}
impl Validator for ShellCompleter {}
impl Helper for ShellCompleter {}

/// Parse a command line into arguments, handling quotes
fn parse_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';

    for c in line.chars() {
        match c {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = c;
            }
            c if c == quote_char && in_quotes => {
                in_quotes = false;
            }
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Run a single command in the shell
async fn run_command(args: Vec<String>) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }

    let cmd = args[0].as_str();
    let sub = args.get(1).map(|s| s.as_str());
    let rest: Vec<&str> = args.iter().skip(2).map(|s| s.as_str()).collect();

    // Check for flags
    let json = rest.contains(&"--json");
    let quiet = rest.contains(&"--quiet") || rest.contains(&"-q");
    let format = if json {
        OutputFormat::Json
    } else {
        OutputFormat::Text
    };

    match (cmd, sub) {
        // Built-in commands
        ("help", _) => {
            print_help();
            Ok(())
        }
        ("exit" | "quit", _) => {
            std::process::exit(0);
        }

        // Game commands
        ("game", Some("detect")) => {
            commands::game::run(
                commands::game::GameCommands::Detect { dir: None },
                format,
                quiet,
            )
            .await
        }
        ("game", Some("launch")) => {
            let params = rest.iter().find(|s| !s.starts_with('-')).map(|s| s.to_string());
            commands::game::run(
                commands::game::GameCommands::Launch { params },
                format,
                quiet,
            )
            .await
        }
        ("game", Some("info")) => {
            commands::game::run(
                commands::game::GameCommands::Info { dir: None },
                format,
                quiet,
            )
            .await
        }
        ("game", _) => {
            println!("Usage: game <detect|launch|info>");
            Ok(())
        }

        // Backup commands
        ("backup", Some("list")) => {
            commands::backup::run(commands::backup::BackupCommands::List, format, quiet).await
        }
        ("backup", Some("create")) => {
            let name = rest.iter().find(|s| !s.starts_with('-')).map(|s| s.to_string());
            let compression = rest
                .iter()
                .position(|s| *s == "--compression")
                .and_then(|i| rest.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(6);
            commands::backup::run(
                commands::backup::BackupCommands::Create { name, compression },
                format,
                quiet,
            )
            .await
        }
        ("backup", Some("restore")) => {
            let name = rest
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
                .unwrap_or_default();
            let dry_run = rest.contains(&"--dry-run");
            let no_safety_backup = rest.contains(&"--no-safety-backup");
            commands::backup::run(
                commands::backup::BackupCommands::Restore {
                    name,
                    no_safety_backup,
                    dry_run,
                },
                format,
                quiet,
            )
            .await
        }
        ("backup", Some("delete")) => {
            let name = rest.iter().find(|s| !s.starts_with('-')).map(|s| s.to_string());
            let keep = rest
                .iter()
                .position(|s| *s == "--keep")
                .and_then(|i| rest.get(i + 1))
                .and_then(|s| s.parse().ok());
            commands::backup::run(
                commands::backup::BackupCommands::Delete { name, keep },
                format,
                quiet,
            )
            .await
        }
        ("backup", Some("verify")) => {
            let name = rest
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
                .unwrap_or_default();
            commands::backup::run(
                commands::backup::BackupCommands::Verify { name },
                format,
                quiet,
            )
            .await
        }
        ("backup", _) => {
            println!("Usage: backup <list|create|restore|delete|verify>");
            Ok(())
        }

        // Update commands
        ("update", Some("check")) => {
            commands::update::run(commands::update::UpdateCommands::Check, format, quiet).await
        }
        ("update", Some("releases")) => {
            let limit = rest
                .iter()
                .position(|s| *s == "--limit")
                .and_then(|i| rest.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(10);
            let branch = rest
                .iter()
                .position(|s| *s == "--branch")
                .and_then(|i| rest.get(i + 1))
                .map(|s| s.to_string());
            commands::update::run(
                commands::update::UpdateCommands::Releases { limit, branch },
                format,
                quiet,
            )
            .await
        }
        ("update", Some("download")) => {
            let version = rest
                .iter()
                .position(|s| *s == "--version")
                .and_then(|i| rest.get(i + 1))
                .map(|s| s.to_string());
            commands::update::run(
                commands::update::UpdateCommands::Download { version },
                format,
                quiet,
            )
            .await
        }
        ("update", Some("install")) => {
            commands::update::run(commands::update::UpdateCommands::Install, format, quiet).await
        }
        ("update", Some("apply")) => {
            let keep_saves = rest.contains(&"--keep-saves");
            let remove_old = rest.contains(&"--remove-old");
            let dry_run = rest.contains(&"--dry-run");
            commands::update::run(
                commands::update::UpdateCommands::Apply {
                    keep_saves,
                    remove_old,
                    dry_run,
                },
                format,
                quiet,
            )
            .await
        }
        ("update", _) => {
            println!("Usage: update <check|releases|download|install|apply>");
            Ok(())
        }

        // Soundpack commands
        ("soundpack", Some("list")) => {
            commands::soundpack::run(commands::soundpack::SoundpackCommands::List, format, quiet)
                .await
        }
        ("soundpack", Some("available")) => {
            commands::soundpack::run(
                commands::soundpack::SoundpackCommands::Available,
                format,
                quiet,
            )
            .await
        }
        ("soundpack", Some("install")) => {
            let name = rest.iter().find(|s| !s.starts_with('-')).map(|s| s.to_string());
            commands::soundpack::run(
                commands::soundpack::SoundpackCommands::Install { name, file: None },
                format,
                quiet,
            )
            .await
        }
        ("soundpack", Some("delete")) => {
            let name = rest
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
                .unwrap_or_default();
            commands::soundpack::run(
                commands::soundpack::SoundpackCommands::Delete { name },
                format,
                quiet,
            )
            .await
        }
        ("soundpack", Some("enable")) => {
            let name = rest
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
                .unwrap_or_default();
            commands::soundpack::run(
                commands::soundpack::SoundpackCommands::Enable { name },
                format,
                quiet,
            )
            .await
        }
        ("soundpack", Some("disable")) => {
            let name = rest
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
                .unwrap_or_default();
            commands::soundpack::run(
                commands::soundpack::SoundpackCommands::Disable { name },
                format,
                quiet,
            )
            .await
        }
        ("soundpack", _) => {
            println!("Usage: soundpack <list|available|install|delete|enable|disable>");
            Ok(())
        }

        // Config commands
        ("config", Some("show")) => {
            commands::config::run(commands::config::ConfigCommands::Show, format, quiet).await
        }
        ("config", Some("get")) => {
            let key = rest
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(|s| s.to_string())
                .unwrap_or_default();
            commands::config::run(commands::config::ConfigCommands::Get { key }, format, quiet)
                .await
        }
        ("config", Some("set")) => {
            let positional: Vec<&str> = rest.iter().filter(|s| !s.starts_with('-')).copied().collect();
            let key = positional.first().map(|s| s.to_string()).unwrap_or_default();
            let value = positional.get(1).map(|s| s.to_string()).unwrap_or_default();
            commands::config::run(
                commands::config::ConfigCommands::Set { key, value },
                format,
                quiet,
            )
            .await
        }
        ("config", Some("path")) => {
            commands::config::run(commands::config::ConfigCommands::Path, format, quiet).await
        }
        ("config", _) => {
            println!("Usage: config <show|get|set|path>");
            Ok(())
        }

        // Diag commands
        ("diag", Some("paths")) => {
            commands::diag::run(commands::diag::DiagCommands::Paths, format, quiet).await
        }
        ("diag", Some("check")) => {
            commands::diag::run(commands::diag::DiagCommands::Check, format, quiet).await
        }
        ("diag", Some("clear-cache")) => {
            commands::diag::run(commands::diag::DiagCommands::ClearCache, format, quiet).await
        }
        ("diag", _) => {
            println!("Usage: diag <paths|check|clear-cache>");
            Ok(())
        }

        _ => {
            println!("Unknown command: {}. Type 'help' for available commands.", cmd);
            Ok(())
        }
    }
}

fn print_help() {
    println!(
        r#"Phoenix Interactive Shell

Commands:
  game detect              Detect installed game version
  game launch [params]     Launch the game
  game info                Show detailed game information

  backup list              List all backups
  backup create [name]     Create a new backup
  backup restore <name>    Restore a backup
  backup delete <name>     Delete a backup
  backup verify [name]     Verify backup integrity

  update check             Check for available updates
  update releases          List available releases
  update download          Download an update
  update install           Install a downloaded update
  update apply             Download and install in one step

  soundpack list           List installed soundpacks
  soundpack available      List soundpacks in repository
  soundpack install <name> Install a soundpack
  soundpack delete <name>  Delete a soundpack
  soundpack enable <name>  Enable a soundpack
  soundpack disable <name> Disable a soundpack

  config show              Show current configuration
  config get <key>         Get a specific setting
  config set <key> <value> Set a configuration value
  config path              Show config file path

  diag paths               Show all data paths
  diag check               Verify installation health
  diag clear-cache         Clear the version hash cache

  help                     Show this help
  exit, quit               Exit the shell

Flags (can be added to any command):
  --json                   Output in JSON format
  --quiet, -q              Suppress non-essential output
"#
    );
}

/// Get the history file path
fn history_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("com", "phoenix", "Phoenix")
        .map(|dirs| dirs.data_dir().join("shell_history"))
}

/// Run the interactive shell
pub async fn run() -> Result<()> {
    println!("Phoenix Interactive Shell v{}", env!("CARGO_PKG_VERSION"));
    println!("Type 'help' for available commands, 'exit' to quit.\n");

    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(rustyline::CompletionType::List)
        .build();

    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(ShellCompleter::new()));

    // Load history
    if let Some(path) = history_path() {
        let _ = rl.load_history(&path);
    }

    loop {
        match rl.readline("phoenix> ") {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line)?;

                let args = parse_args(line);
                if let Err(e) = run_command(args).await {
                    eprintln!("Error: {}", e);
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("exit");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    // Save history
    if let Some(path) = history_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = rl.save_history(&path);
    }

    Ok(())
}
