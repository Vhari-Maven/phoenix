mod app;
mod config;
mod db;
mod game;
mod github;
mod theme;
mod update;

use anyhow::Result;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "phoenix=debug,info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Phoenix launcher");

    // Load application icon
    let icon = load_icon().map(Arc::new);

    // Configure native options
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([800.0, 750.0])
        .with_min_inner_size([600.0, 500.0])
        .with_title("Phoenix - CDDA Launcher");

    let viewport = if let Some(icon) = icon {
        viewport.with_icon(icon)
    } else {
        tracing::warn!("Failed to load application icon");
        viewport
    };

    let native_options = eframe::NativeOptions {
        viewport,
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
