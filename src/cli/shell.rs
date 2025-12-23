//! Interactive shell mode for Phoenix CLI
//!
//! Provides a REPL with command history and tab completion.

use anyhow::Result;
use clap::Parser;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Config, Editor, Helper};

use super::commands;
use super::{Cli, Commands};

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

/// Run a single command in the shell.
/// Returns Ok(true) to continue, Ok(false) to exit gracefully.
async fn run_command(args: Vec<String>) -> Result<bool> {
    if args.is_empty() {
        return Ok(true);
    }

    // Handle shell built-in commands
    let cmd = args[0].as_str();
    match cmd {
        "help" => {
            print_help();
            return Ok(true);
        }
        "exit" | "quit" => {
            return Ok(false);
        }
        _ => {}
    }

    // Build a fake argv for clap: ["phoenix", ...args]
    let mut argv: Vec<String> = vec!["phoenix".to_string()];
    argv.extend(args);

    // Parse using clap
    let cli = match Cli::try_parse_from(&argv) {
        Ok(cli) => cli,
        Err(e) => {
            // Print clap's error message (includes usage hints)
            println!("{}", e);
            return Ok(true);
        }
    };

    // Reject nested shell command
    if matches!(cli.command, Commands::Shell) {
        println!("Already in shell mode.");
        return Ok(true);
    }

    // Dispatch to the appropriate command handler
    let format = cli.output.format();
    let quiet = cli.output.quiet;

    let result = match cli.command {
        Commands::Game { command } => commands::game::run(command, format, quiet).await,
        Commands::Backup { command } => commands::backup::run(command, format, quiet).await,
        Commands::Update { command } => commands::update::run(command, format, quiet).await,
        Commands::Soundpack { command } => commands::soundpack::run(command, format, quiet).await,
        Commands::Config { command } => commands::config::run(command, format, quiet).await,
        Commands::Diag { command } => commands::diag::run(command, format, quiet).await,
        Commands::Shell => unreachable!(), // Already handled above
    };

    result?;
    Ok(true)
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
                match run_command(args).await {
                    Ok(true) => continue,  // Command succeeded, keep running
                    Ok(false) => break,    // Exit requested, break to save history
                    Err(e) => eprintln!("Error: {}", e),
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
