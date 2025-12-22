//! Backups tab UI rendering

use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

use crate::app::PhoenixApp;
use crate::backup::BackupPhase;
use super::theme::Theme;
use crate::ui::components::{progress_frame, render_current_file, render_file_progress};

/// Render the backups tab
pub fn render_backups_tab(app: &mut PhoenixApp, ui: &mut egui::Ui) {
    let theme = app.ui.current_theme.clone();

    ui.label(
        RichText::new("Backups")
            .color(theme.text_primary)
            .size(20.0)
            .strong(),
    );
    ui.add_space(16.0);

    // Check if game directory is set
    let game_dir = app.config.game.directory.as_ref().map(PathBuf::from);

    if game_dir.is_none() {
        ui.label(
            RichText::new("Set a game directory in Main tab to manage backups.")
                .color(theme.text_muted),
        );
        return;
    }

    let game_dir = game_dir.unwrap();
    let is_busy = app.is_backup_busy();

    // Manual backup section
    egui::Frame::none()
        .fill(theme.bg_medium)
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Create Backup")
                    .color(theme.accent)
                    .size(13.0)
                    .strong(),
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Backup name:").color(theme.text_muted));
                ui.add_sized(
                    [200.0, 20.0],
                    egui::TextEdit::singleline(&mut app.backup.name_input).hint_text("my_backup"),
                );

                ui.add_space(16.0);

                let can_backup = !is_busy && !app.backup.name_input.trim().is_empty();
                if ui
                    .add_enabled(can_backup, egui::Button::new("Backup Current Saves"))
                    .clicked()
                {
                    app.start_manual_backup(&game_dir);
                }
            });

            // Show validation error
            if !app.backup.name_input.is_empty() {
                if let Err(e) = app.validate_backup_name(&app.backup.name_input) {
                    ui.add_space(4.0);
                    ui.label(RichText::new(e).color(theme.error).size(11.0));
                }
            }
        });

    ui.add_space(12.0);

    // Backup list section
    egui::Frame::none()
        .fill(theme.bg_medium)
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Available Backups")
                        .color(theme.accent)
                        .size(13.0)
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!is_busy, egui::Button::new("Refresh"))
                        .clicked()
                    {
                        app.refresh_backup_list(&game_dir);
                    }
                });
            });
            ui.add_space(12.0);

            if app.backup.list_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Loading backups...").color(theme.text_muted));
                });
            } else if app.backup.list.is_empty() {
                ui.label(RichText::new("No backups found.").color(theme.text_muted));
            } else {
                // Backup table
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        egui::Grid::new("backup_list_grid")
                            .num_columns(7)
                            .spacing([12.0, 8.0])
                            .striped(true)
                            .show(ui, |ui| {
                                // Header row
                                ui.label(
                                    RichText::new("Name")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new("Date")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new("Worlds")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new("Chars")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new("Size")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new("Uncomp.")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new("Ratio")
                                        .color(theme.text_muted)
                                        .strong()
                                        .size(11.0),
                                );
                                ui.end_row();

                                // Data rows
                                for (i, backup) in app.backup.list.iter().enumerate() {
                                    let is_selected = app.backup.selected_idx == Some(i);
                                    let text_color = if is_selected {
                                        theme.accent
                                    } else {
                                        theme.text_primary
                                    };

                                    // Truncate long names
                                    let display_name = if backup.name.len() > 25 {
                                        format!("{}...", &backup.name[..22])
                                    } else {
                                        backup.name.clone()
                                    };

                                    if ui
                                        .selectable_label(
                                            is_selected,
                                            RichText::new(&display_name).color(text_color).size(12.0),
                                        )
                                        .clicked()
                                    {
                                        app.backup.selected_idx = Some(i);
                                    }

                                    ui.label(
                                        RichText::new(
                                            backup.modified.format("%Y-%m-%d %H:%M").to_string(),
                                        )
                                        .color(text_color)
                                        .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new(backup.worlds_count.to_string())
                                            .color(text_color)
                                            .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new(backup.characters_count.to_string())
                                            .color(text_color)
                                            .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new(backup.compressed_size_display())
                                            .color(text_color)
                                            .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new(backup.uncompressed_size_display())
                                            .color(text_color)
                                            .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new(format!("{:.0}%", backup.compression_ratio()))
                                            .color(text_color)
                                            .size(12.0),
                                    );
                                    ui.end_row();
                                }
                            });
                    });

                ui.add_space(12.0);

                // Action buttons
                ui.horizontal(|ui| {
                    let has_selection = app.backup.selected_idx.is_some();

                    // Restore button
                    if ui
                        .add_enabled(has_selection && !is_busy, egui::Button::new("Restore"))
                        .clicked()
                    {
                        app.backup.confirm_restore = true;
                    }

                    // Delete button
                    if ui
                        .add_enabled(has_selection && !is_busy, egui::Button::new("Delete"))
                        .clicked()
                    {
                        app.backup.confirm_delete = true;
                    }
                });
            }
        });

    // Confirmation dialogs
    render_backup_confirm_dialogs(app, ui, &theme, &game_dir);

    // Progress section
    if is_busy
        || app.backup.progress.phase == BackupPhase::Complete
        || app.backup.progress.phase == BackupPhase::Failed
    {
        ui.add_space(12.0);
        render_backup_progress(app, ui, &theme);
    }

    // Error display
    if let Some(ref err) = app.backup.error {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
    }
}

/// Render backup confirmation dialogs
fn render_backup_confirm_dialogs(
    app: &mut PhoenixApp,
    ui: &mut egui::Ui,
    theme: &Theme,
    game_dir: &Path,
) {
    // Delete confirmation
    if app.backup.confirm_delete {
        egui::Window::new("Confirm Delete")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                if let Some(idx) = app.backup.selected_idx {
                    if let Some(backup) = app.backup.list.get(idx) {
                        ui.label(format!("Delete backup \"{}\"?", backup.name));
                        ui.add_space(8.0);
                        ui.label(RichText::new("This cannot be undone.").color(theme.warning));
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                app.backup.confirm_delete = false;
                            }
                            if ui.button("Delete").clicked() {
                                app.delete_selected_backup(game_dir);
                                app.backup.confirm_delete = false;
                            }
                        });
                    }
                } else {
                    app.backup.confirm_delete = false;
                }
            });
    }

    // Restore confirmation
    if app.backup.confirm_restore {
        egui::Window::new("Confirm Restore")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                if let Some(idx) = app.backup.selected_idx {
                    if let Some(backup) = app.backup.list.get(idx) {
                        ui.label(format!("Restore backup \"{}\"?", backup.name));
                        ui.add_space(8.0);

                        if !app.config.backups.skip_backup_before_restore {
                            ui.label(
                                RichText::new("Your current saves will be backed up first.")
                                    .color(theme.text_muted),
                            );
                        } else {
                            ui.label(
                                RichText::new("Warning: Current saves will be replaced!")
                                    .color(theme.warning),
                            );
                        }
                        ui.add_space(12.0);

                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                app.backup.confirm_restore = false;
                            }
                            if ui.button("Restore").clicked() {
                                app.restore_selected_backup(game_dir);
                                app.backup.confirm_restore = false;
                            }
                        });
                    }
                } else {
                    app.backup.confirm_restore = false;
                }
            });
    }
}

/// Render backup progress
fn render_backup_progress(app: &PhoenixApp, ui: &mut egui::Ui, theme: &Theme) {
    let progress = &app.backup.progress;

    progress_frame(theme).show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Phase label
            let (phase_text, phase_color) = match progress.phase {
                BackupPhase::Scanning => ("Scanning files...", theme.accent),
                BackupPhase::Compressing => ("Compressing saves...", theme.accent),
                BackupPhase::Extracting => ("Extracting backup...", theme.accent),
                BackupPhase::Cleaning => ("Cleaning up...", theme.warning),
                BackupPhase::Complete => ("Backup operation complete!", theme.success),
                BackupPhase::Failed => ("Operation failed", theme.error),
                BackupPhase::Idle => ("Ready", theme.text_muted),
            };

            ui.label(
                RichText::new(phase_text)
                    .color(phase_color)
                    .size(13.0)
                    .strong(),
            );
            ui.add_space(8.0);

            // Progress bar for compress/extract phases
            match progress.phase {
                BackupPhase::Compressing | BackupPhase::Extracting => {
                    let fraction = progress.fraction();
                    ui.add(egui::ProgressBar::new(fraction).show_percentage());

                    ui.add_space(4.0);
                    render_file_progress(ui, progress.files_processed, progress.total_files, theme);
                    render_current_file(ui, &progress.current_file, theme);
                }
                BackupPhase::Scanning | BackupPhase::Cleaning => {
                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                }
                _ => {}
            }
        });
}
