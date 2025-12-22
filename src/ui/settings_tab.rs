//! Settings tab UI rendering

use eframe::egui::{self, RichText, Vec2};

use crate::app::PhoenixApp;
use crate::theme::ThemePreset;

/// Render the settings tab
pub fn render_settings_tab(app: &mut PhoenixApp, ui: &mut egui::Ui) {
    let theme = app.ui.current_theme.clone();

    egui::ScrollArea::vertical()
        .id_salt("settings_scroll")
        .show(ui, |ui| {
            // Use full available width
            let available_width = ui.available_width();

            ui.label(
                RichText::new("Settings")
                    .color(theme.text_primary)
                    .size(20.0)
                    .strong(),
            );
            ui.add_space(16.0);

            // Appearance section
            egui::Frame::none()
                .fill(theme.bg_medium)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    ui.set_width(available_width - 32.0); // Account for frame margins
                    ui.label(
                        RichText::new("Appearance")
                            .color(theme.accent)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Theme:").color(theme.text_muted));

                        let current_name = app.config.launcher.theme.name();
                        egui::ComboBox::from_id_salt("theme_select")
                            .selected_text(current_name)
                            .show_ui(ui, |ui| {
                                for preset in ThemePreset::all() {
                                    if ui
                                        .selectable_label(
                                            app.config.launcher.theme == *preset,
                                            preset.name(),
                                        )
                                        .clicked()
                                    {
                                        app.config.launcher.theme = *preset;
                                        app.ui.current_theme = preset.theme();
                                        app.ui.theme_dirty = true;
                                        app.save_config();
                                    }
                                }
                            });
                    });

                    // Theme preview swatches
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Preview:").color(theme.text_muted));
                        ui.add_space(8.0);

                        let swatch_size = Vec2::new(24.0, 24.0);
                        let colors = [
                            ("Bg", theme.bg_dark),
                            ("Accent", theme.accent),
                            ("Success", theme.success),
                            ("Warning", theme.warning),
                            ("Error", theme.error),
                        ];

                        for (label, color) in colors {
                            let (rect, _) = ui.allocate_exact_size(swatch_size, egui::Sense::hover());
                            ui.painter().rect_filled(rect, 4.0, color);
                            if ui.rect_contains_pointer(rect) {
                                egui::show_tooltip(
                                    ui.ctx(),
                                    ui.layer_id(),
                                    egui::Id::new(label),
                                    |ui| {
                                        ui.label(label);
                                    },
                                );
                            }
                            ui.add_space(4.0);
                        }
                    });
                });

            ui.add_space(12.0);

            // Behavior section
            egui::Frame::none()
                .fill(theme.bg_medium)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    ui.set_width(available_width - 32.0);
                    ui.label(
                        RichText::new("Behavior")
                            .color(theme.accent)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    if ui
                        .checkbox(
                            &mut app.config.launcher.keep_open,
                            "Keep launcher open after game exits",
                        )
                        .changed()
                    {
                        app.save_config();
                    }

                    if ui
                        .checkbox(
                            &mut app.config.updates.check_on_startup,
                            "Check for updates on startup",
                        )
                        .changed()
                    {
                        app.save_config();
                    }
                });

            ui.add_space(12.0);

            // Updates section
            egui::Frame::none()
                .fill(theme.bg_medium)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    ui.set_width(available_width - 32.0);
                    ui.label(
                        RichText::new("Updates")
                            .color(theme.accent)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    if ui
                        .checkbox(
                            &mut app.config.updates.prevent_save_move,
                            "Do not copy saves during updates",
                        )
                        .changed()
                    {
                        app.save_config();
                    }
                    ui.label(
                        RichText::new(
                            "  Leave saves in place instead of copying from previous_version/",
                        )
                        .color(theme.text_muted)
                        .size(11.0),
                    );

                    ui.add_space(8.0);

                    if ui
                        .checkbox(
                            &mut app.config.updates.remove_previous_version,
                            "Remove previous_version after update",
                        )
                        .changed()
                    {
                        app.save_config();
                    }
                    ui.label(
                        RichText::new("  Not recommended - removes rollback capability")
                            .color(theme.warning)
                            .size(11.0),
                    );
                });

            ui.add_space(12.0);

            // Backups section
            egui::Frame::none()
                .fill(theme.bg_medium)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    ui.set_width(available_width - 32.0);
                    ui.label(
                        RichText::new("Backups")
                            .color(theme.accent)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    // Auto-backup toggles
                    if ui
                        .checkbox(
                            &mut app.config.backups.backup_before_update,
                            "Backup saves before updating game",
                        )
                        .changed()
                    {
                        app.save_config();
                    }
                    ui.label(
                        RichText::new("  Creates an automatic backup before each update")
                            .color(theme.text_muted)
                            .size(11.0),
                    );

                    ui.add_space(8.0);

                    if ui
                        .checkbox(
                            &mut app.config.backups.backup_on_launch,
                            "Backup saves before launching game",
                        )
                        .changed()
                    {
                        app.save_config();
                    }

                    ui.add_space(8.0);

                    if ui
                        .checkbox(
                            &mut app.config.backups.skip_backup_before_restore,
                            "Skip backup when restoring",
                        )
                        .changed()
                    {
                        app.save_config();
                    }
                    ui.label(
                        RichText::new("  Not recommended - restoring will overwrite current saves")
                            .color(theme.warning)
                            .size(11.0),
                    );

                    ui.add_space(12.0);

                    // Max backups
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Max auto-backups to keep:").color(theme.text_muted));
                        if ui
                            .add(
                                egui::DragValue::new(&mut app.config.backups.max_count)
                                    .range(1..=100)
                                    .speed(1.0),
                            )
                            .changed()
                        {
                            app.save_config();
                        }
                    });

                    ui.add_space(8.0);

                    // Compression level
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Compression level:").color(theme.text_muted));
                        if ui
                            .add(
                                egui::Slider::new(&mut app.config.backups.compression_level, 0..=9)
                                    .text(""),
                            )
                            .changed()
                        {
                            app.save_config();
                        }
                    });
                    ui.label(
                        RichText::new("  0 = no compression (fast), 9 = best compression (slow)")
                            .color(theme.text_muted)
                            .size(11.0),
                    );
                });

            ui.add_space(12.0);

            // Game settings section
            egui::Frame::none()
                .fill(theme.bg_medium)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    ui.set_width(available_width - 32.0);
                    ui.label(
                        RichText::new("Game")
                            .color(theme.accent)
                            .size(13.0)
                            .strong(),
                    );
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Command line parameters:").color(theme.text_muted));
                    });
                    ui.add_space(4.0);
                    if ui
                        .text_edit_singleline(&mut app.config.game.command_params)
                        .changed()
                    {
                        app.save_config();
                    }
                });
        }); // ScrollArea
}
