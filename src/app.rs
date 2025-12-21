use eframe::egui;

use crate::config::Config;

/// Main application state
pub struct PhoenixApp {
    /// Application configuration
    config: Config,
}

impl PhoenixApp {
    /// Create a new application instance
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Load configuration
        let config = Config::load().unwrap_or_default();

        Self { config }
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
                ui.label("Ready");
            });
        });

        // Main content area with tabs
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_label(true, "Main");
                ui.selectable_label(false, "Backups");
                ui.selectable_label(false, "Soundpacks");
                ui.selectable_label(false, "Settings");
            });

            ui.separator();

            // Main tab content placeholder
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
                        // TODO: Open directory picker
                    }
                });

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
                    if ui.button("Launch Game").clicked() {
                        // TODO: Launch game
                    }
                    if ui.button("Update").clicked() {
                        // TODO: Check for updates
                    }
                });
            });
        });
    }
}
