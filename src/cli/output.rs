//! Output formatting utilities for CLI

use serde::Serialize;
use std::io::IsTerminal;

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

/// Print a serializable value as JSON or use custom text formatter
pub fn print_formatted<T, F>(value: &T, format: OutputFormat, text_formatter: F)
where
    T: Serialize,
    F: FnOnce(&T) -> String,
{
    match format {
        OutputFormat::Text => println!("{}", text_formatter(value)),
        OutputFormat::Json => {
            if let Ok(json) = serde_json::to_string_pretty(value) {
                println!("{}", json);
            }
        }
    }
}

/// Print a success message (suppressed in quiet mode)
pub fn print_success(message: &str, quiet: bool) {
    if !quiet {
        println!("{}", message);
    }
}

/// Print an error message (never suppressed)
pub fn print_error(message: &str) {
    eprintln!("Error: {}", message);
}

/// Check if stderr is a terminal (for progress output)
pub fn stderr_is_tty() -> bool {
    std::io::stderr().is_terminal()
}

/// Check if progress output should be shown.
/// Returns false if quiet mode is enabled or stderr is not a TTY.
pub fn should_show_progress(quiet: bool, format: OutputFormat) -> bool {
    !quiet && format == OutputFormat::Text && stderr_is_tty()
}
