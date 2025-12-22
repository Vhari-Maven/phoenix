//! Shared UI components for Phoenix launcher

use eframe::egui::{self, Color32, RichText, Rounding, Vec2};
use std::path::PathBuf;

use crate::app::{PhoenixApp, Tab};

/// Render a tab button
pub fn render_tab(app: &mut PhoenixApp, ui: &mut egui::Ui, tab: Tab, label: &str) {
    let theme = &app.current_theme;
    let is_active = app.active_tab == tab;

    let (bg, text_color) = if is_active {
        (theme.bg_medium, theme.accent)
    } else {
        (Color32::TRANSPARENT, theme.text_secondary)
    };

    let button = egui::Button::new(RichText::new(label).color(text_color))
        .fill(bg)
        .rounding(Rounding {
            nw: 6.0,
            ne: 6.0,
            sw: 0.0,
            se: 0.0,
        })
        .min_size(Vec2::new(80.0, 32.0));

    if ui.add(button).clicked() {
        let previous_tab = app.active_tab;
        app.active_tab = tab;

        // Load backup list when switching to Backups tab
        if tab == Tab::Backups && previous_tab != Tab::Backups {
            if let Some(ref dir) = app.config.game.directory {
                if app.backup_list.is_empty() && !app.backup_list_loading {
                    app.refresh_backup_list(&PathBuf::from(dir));
                }
            }
        }

        // Load soundpack list when switching to Soundpacks tab
        if tab == Tab::Soundpacks && previous_tab != Tab::Soundpacks {
            if let Some(ref dir) = app.config.game.directory {
                if app.soundpack_list.is_empty() && !app.soundpack_list_loading {
                    app.refresh_soundpack_list(&PathBuf::from(dir));
                }
            }
        }
    }
}

/// Render the About dialog
pub fn render_about_dialog(app: &mut PhoenixApp, ctx: &egui::Context) {
    if !app.show_about_dialog {
        return;
    }

    let theme = &app.current_theme;

    egui::Window::new("About Phoenix")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([300.0, 280.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);

                // App name
                ui.label(
                    RichText::new("Phoenix")
                        .size(24.0)
                        .strong()
                        .color(theme.accent)
                );

                ui.add_space(4.0);
                ui.label(
                    RichText::new("CDDA Game Launcher")
                        .size(14.0)
                        .color(theme.text_secondary)
                );

                ui.add_space(12.0);

                // Version
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .color(theme.text_muted)
                );

                ui.add_space(12.0);

                // Description
                ui.label(
                    RichText::new("A fast, native launcher for")
                        .color(theme.text_secondary)
                );
                ui.label(
                    RichText::new("Cataclysm: Dark Days Ahead")
                        .color(theme.text_secondary)
                );

                ui.add_space(12.0);

                // Links (separate lines for proper centering)
                if ui.link("GitHub").clicked() {
                    let _ = open::that("https://github.com/Vhari-Maven/phoenix");
                }
                ui.add_space(4.0);
                if ui.link("CDDA Website").clicked() {
                    let _ = open::that("https://cataclysmdda.org/");
                }

                ui.add_space(12.0);

                // Built with
                ui.label(
                    RichText::new("Built with Rust + egui")
                        .size(11.0)
                        .color(theme.text_muted)
                );

                ui.add_space(12.0);

                // Close button
                if ui.button("Close").clicked() {
                    app.show_about_dialog = false;
                }

                ui.add_space(8.0);
            });
        });
}
