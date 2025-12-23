// Hide console window in release builds (Windows GUI app)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod app_data;
mod backup;
mod cli;
mod config;
mod db;
mod game;
mod github;
mod legacy;
mod migration;
mod soundpack;
mod state;
mod task;
mod ui;
mod update;
mod util;

use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::app_data::launcher_config;

#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::core::PCWSTR;

/// Load the application icon from embedded PNG data
fn load_icon() -> Option<egui::IconData> {
    let icon_data = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(icon_data).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Single instance enforcement using a Windows named mutex.
/// Returns a handle that must be kept alive for the duration of the app.
#[cfg(windows)]
fn acquire_single_instance() -> Option<HANDLE> {
    use windows::Win32::Foundation::GetLastError;
    use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;

    // Create a unique mutex name for this application
    let mutex_name: Vec<u16> = "Global\\PhoenixCDDALauncher\0"
        .encode_utf16()
        .collect();

    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr())).ok()?;

        // Check if another instance already owns this mutex
        if GetLastError() == ERROR_ALREADY_EXISTS {
            tracing::warn!("Another instance of Phoenix is already running");
            return None;
        }

        Some(handle)
    }
}

#[cfg(not(windows))]
fn acquire_single_instance() -> Option<()> {
    Some(()) // No-op on non-Windows platforms
}

/// Check if we should run in CLI mode based on command-line arguments
fn should_run_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    // CLI mode if we have arguments beyond just the executable name
    // and the first argument looks like a subcommand (not a flag like --help)
    args.len() > 1
}

/// Attach to parent console or allocate a new one for CLI output
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, AllocConsole, ATTACH_PARENT_PROCESS};

    unsafe {
        // Try to attach to parent console (e.g., cmd.exe)
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            // If no parent console, allocate a new one
            let _ = AllocConsole();
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {
    // No-op on non-Windows platforms
}

#[tokio::main]
async fn main() -> Result<()> {
    // Determine mode before doing anything else
    let cli_mode = should_run_cli();

    // In CLI mode, we need a console for output
    if cli_mode {
        attach_console();
    }

    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| {
                if cli_mode {
                    "phoenix=warn".into()
                } else {
                    "phoenix=debug,info".into()
                }
            }),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Run CLI or GUI
    if cli_mode {
        run_cli().await
    } else {
        run_gui().await
    }
}

/// Run the CLI interface
async fn run_cli() -> Result<()> {
    let cli = cli::Cli::parse();
    cli::run(cli).await
}

/// Run the GUI interface
async fn run_gui() -> Result<()> {
    tracing::info!("Starting Phoenix launcher");

    // Enforce single instance
    let _instance_lock = match acquire_single_instance() {
        Some(lock) => lock,
        None => {
            tracing::error!("Phoenix is already running. Exiting.");
            // Show a message box on Windows
            #[cfg(windows)]
            {
                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_ICONINFORMATION};
                let title: Vec<u16> = "Phoenix\0".encode_utf16().collect();
                let msg: Vec<u16> = "Phoenix is already running.\0".encode_utf16().collect();
                unsafe {
                    MessageBoxW(
                        None,
                        PCWSTR(msg.as_ptr()),
                        PCWSTR(title.as_ptr()),
                        MB_OK | MB_ICONINFORMATION,
                    );
                }
            }
            return Ok(());
        }
    };

    // Load application icon
    let icon = load_icon().map(Arc::new);

    // Configure native options
    let config = launcher_config();
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size(config.window.initial_size)
        .with_min_inner_size(config.window.min_size)
        .with_title(&config.window.title);

    let viewport = if let Some(icon) = icon {
        viewport.with_icon(icon)
    } else {
        tracing::warn!("Failed to load application icon");
        viewport
    };

    let native_options = eframe::NativeOptions {
        viewport,
        persist_window: true, // Save/restore window size and position
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "Phoenix",
        native_options,
        Box::new(|cc| Ok(Box::new(app::PhoenixApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run application: {}", e))?;

    Ok(())
}
