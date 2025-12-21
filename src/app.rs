use eframe::egui;
use std::path::PathBuf;

use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};

/// Main application state
pub struct PhoenixApp {
    /// Application configuration
    config: Config,
    /// Version database
    db: Option<Database>,
    /// Detected game information
    game_info: Option<GameInfo>,
    /// Status message for the status bar
    status_message: String,
}

impl PhoenixApp {
    /// Create a new application instance
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Load configuration
        let config = Config::load().unwrap_or_default();

        // Open database
        let db = match Database::open() {
            Ok(db) => {
                tracing::info!("Database opened successfully");
                Some(db)
            }
            Err(e) => {
                tracing::error!("Failed to open database: {}", e);
                None
            }
        };

        // Try to detect game if directory is configured
        let game_info = config
            .game
            .directory
            .as_ref()
            .and_then(|dir| {
                game::detect_game_with_db(&PathBuf::from(dir), db.as_ref())
                    .ok()
                    .flatten()
            });

        let status_message = if let Some(ref info) = game_info {
            format!("Game detected: {}", info.version_display())
        } else {
            "Ready".to_string()
        };

        Self {
            config,
            db,
            game_info,
            status_message,
        }
    }

    /// Open directory picker and update game directory
    fn browse_for_directory(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Select CDDA Game Directory")
            .pick_folder()
        {
            let path_str = path.to_string_lossy().to_string();
            self.config.game.directory = Some(path_str);

            // Try to detect game in selected directory
            match game::detect_game_with_db(&path, self.db.as_ref()) {
                Ok(Some(info)) => {
                    self.status_message = format!(
                        "Game found: {} ({})",
                        info.executable.file_name().unwrap_or_default().to_string_lossy(),
                        info.version_display()
                    );
                    self.game_info = Some(info);
                }
                Ok(None) => {
                    self.status_message = "No game executable found in directory".to_string();
                    self.game_info = None;
                }
                Err(e) => {
                    self.status_message = format!("Error detecting game: {}", e);
                    self.game_info = None;
                }
            }

            // Save config after directory change
            self.save_config();
        }
    }

    /// Save configuration to disk
    fn save_config(&self) {
        if let Err(e) = self.config.save() {
            tracing::error!("Failed to save config: {}", e);
        }
    }

    /// Launch the game
    fn launch_game(&mut self) {
        if let Some(ref info) = self.game_info {
            match game::launch_game(&info.executable, &self.config.game.command_params) {
                Ok(()) => {
                    self.status_message = "Game launched!".to_string();
                }
                Err(e) => {
                    self.status_message = format!("Failed to launch: {}", e);
                }
            }
        } else {
            self.status_message = "No game detected - select a valid game directory".to_string();
        }
    }
}

impl eframe::App for PhoenixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Exit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("About").clicked() {
                        // TODO: Show about dialog
                    }
                });
            });
        });

        // Status bar at bottom
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_message);
            });
        });

        // Main content area with tabs
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let _ = ui.selectable_label(true, "Main");
                let _ = ui.selectable_label(false, "Backups");
                let _ = ui.selectable_label(false, "Soundpacks");
                let _ = ui.selectable_label(false, "Settings");
            });

            ui.separator();

            // Main tab content
            ui.vertical(|ui| {
                ui.heading("Game Directory");

                ui.horizontal(|ui| {
                    ui.label("Directory:");
                    let dir_text = self
                        .config
                        .game
                        .directory
                        .as_deref()
                        .unwrap_or("Not selected");
                    ui.label(dir_text);
                    if ui.button("Browse...").clicked() {
                        self.browse_for_directory();
                    }
                });

                // Show game info if detected
                if let Some(ref info) = self.game_info {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label("Executable:");
                        ui.label(info.executable.file_name().unwrap_or_default().to_string_lossy().to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Version:");
                        let version_text = if info.is_stable() {
                            format!("{} (Stable)", info.version_display())
                        } else {
                            info.version_display().to_string()
                        };
                        ui.label(version_text);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Saves size:");
                        ui.label(game::format_size(info.saves_size));
                    });
                }

                ui.add_space(20.0);

                ui.heading("Update");
                ui.horizontal(|ui| {
                    ui.label("Branch:");
                    egui::ComboBox::from_id_salt("branch_select")
                        .selected_text(&self.config.game.branch)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.game.branch,
                                "stable".to_string(),
                                "Stable",
                            );
                            ui.selectable_value(
                                &mut self.config.game.branch,
                                "experimental".to_string(),
                                "Experimental",
                            );
                        });
                });

                ui.add_space(20.0);

                ui.horizontal(|ui| {
                    let can_launch = self.game_info.is_some();
                    if ui.add_enabled(can_launch, egui::Button::new("Launch Game")).clicked() {
                        self.launch_game();
                    }
                    if ui.button("Update").clicked() {
                        // TODO: Check for updates
                    }
                });
            });
        });
    }
}
