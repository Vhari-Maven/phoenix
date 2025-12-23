//! Output formatting utilities for CLI

use serde::Serialize;

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

/// Print a value in the specified format
pub fn print_output<T: Serialize + std::fmt::Display>(value: &T, format: OutputFormat) {
    match format {
        OutputFormat::Text => println!("{}", value),
        OutputFormat::Json => {
            if let Ok(json) = serde_json::to_string_pretty(value) {
                println!("{}", json);
            }
        }
    }
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

/// Print a status line with a check mark or X
pub fn print_status(ok: bool, message: &str) {
    if ok {
        println!("[OK] {}", message);
    } else {
        println!("[  ] {}", message);
    }
}

/// Format bytes as human-readable size
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
