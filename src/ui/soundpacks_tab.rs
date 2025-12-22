//! Soundpacks tab UI rendering

use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};

use crate::app::PhoenixApp;
use crate::soundpack::{self, SoundpackError, SoundpackPhase};
use super::theme::Theme;
use crate::ui::components::{progress_frame, render_current_file};
use crate::util::format_size;

/// Render the soundpacks tab
pub fn render_soundpacks_tab(app: &mut PhoenixApp, ui: &mut egui::Ui) {
    let theme = app.ui.current_theme.clone();

    ui.label(
        RichText::new("Soundpacks")
            .color(theme.text_primary)
            .size(20.0)
            .strong(),
    );
    ui.add_space(16.0);

    // Check if game directory is set
    let game_dir = match &app.config.game.directory {
        Some(dir) => PathBuf::from(dir),
        None => {
            ui.label(
                RichText::new("Set a game directory in the Main tab to manage soundpacks.")
                    .color(theme.text_muted),
            );
            return;
        }
    };

    let is_busy = app.is_soundpack_busy();

    // Two-column layout using columns
    ui.columns(2, |columns| {
        // Left column: Installed soundpacks
        render_installed_soundpacks_panel(app, &mut columns[0], &theme, &game_dir, is_busy);

        // Right column: Repository soundpacks
        render_repository_soundpacks_panel(app, &mut columns[1], &theme, &game_dir, is_busy);
    });

    ui.add_space(12.0);

    // Details panel
    render_soundpack_details_panel(app, ui, &theme);

    // Progress section
    if is_busy
        || app.soundpack.progress.phase == SoundpackPhase::Complete
        || app.soundpack.progress.phase == SoundpackPhase::Failed
    {
        ui.add_space(12.0);
        render_soundpack_progress(app, ui, &theme);
    }

    // Delete confirmation dialog
    if app.soundpack.confirm_delete {
        render_soundpack_delete_dialog(app, ui, &theme, &game_dir);
    }

    // Browser download dialog
    if app.soundpack.browser_download_url.is_some() {
        render_browser_download_dialog(app, ui, &theme, &game_dir);
    }

    // Error display
    if let Some(ref err) = app.soundpack.error {
        ui.add_space(8.0);
        ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
    }
}

/// Render the installed soundpacks panel
fn render_installed_soundpacks_panel(
    app: &mut PhoenixApp,
    ui: &mut egui::Ui,
    theme: &Theme,
    game_dir: &Path,
    is_busy: bool,
) {
    egui::Frame::none()
        .fill(theme.bg_medium)
        .rounding(8.0)
        .inner_margin(12.0)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Installed")
                        .color(theme.accent)
                        .size(13.0)
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !is_busy,
                            egui::Button::new(
                                RichText::new("⟳").color(theme.text_secondary).size(14.0),
                            ),
                        )
                        .on_hover_text("Refresh list")
                        .clicked()
                    {
                        app.refresh_soundpack_list(game_dir);
                    }
                });
            });

            ui.add_space(8.0);

            // Soundpack list
            egui::ScrollArea::vertical()
                .id_salt("installed_soundpacks")
                .max_height(200.0)
                .show(ui, |ui| {
                    if app.soundpack.list.is_empty() && !app.soundpack.list_loading {
                        ui.label(
                            RichText::new("No soundpacks installed")
                                .color(theme.text_muted)
                                .italics(),
                        );
                    } else {
                        for (idx, soundpack) in app.soundpack.list.iter().enumerate() {
                            let is_selected = app.soundpack.installed_idx == Some(idx);
                            let display_name = if soundpack.enabled {
                                soundpack.view_name.clone()
                            } else {
                                format!("{} (Disabled)", soundpack.view_name)
                            };

                            let text_color = if soundpack.enabled {
                                theme.text_primary
                            } else {
                                theme.text_muted
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                RichText::new(&display_name).color(text_color),
                            );

                            if response.clicked() {
                                app.soundpack.installed_idx = Some(idx);
                                app.soundpack.repo_idx = None;
                            }
                        }
                    }
                });

            ui.add_space(8.0);

            // Action buttons
            ui.horizontal(|ui| {
                let has_selection = app.soundpack.installed_idx.is_some();
                let selected_enabled = app
                    .soundpack.installed_idx
                    .and_then(|i| app.soundpack.list.get(i))
                    .map(|s| s.enabled)
                    .unwrap_or(false);

                let toggle_text = if selected_enabled { "Disable" } else { "Enable" };

                if ui
                    .add_enabled(
                        has_selection && !is_busy,
                        egui::Button::new(RichText::new(toggle_text).color(theme.text_primary)),
                    )
                    .clicked()
                {
                    if let Some(idx) = app.soundpack.installed_idx {
                        if let Some(soundpack) = app.soundpack.list.get(idx) {
                            let path = soundpack.path.clone();
                            let new_enabled = !soundpack.enabled;
                            let game_dir = game_dir.to_path_buf();

                            tokio::spawn(async move {
                                if let Err(e) =
                                    soundpack::set_soundpack_enabled(&path, new_enabled).await
                                {
                                    tracing::error!("Failed to toggle soundpack: {}", e);
                                }
                            });

                            // Refresh the list after a short delay
                            app.refresh_soundpack_list(&game_dir);
                        }
                    }
                }

                if ui
                    .add_enabled(
                        has_selection && !is_busy,
                        egui::Button::new(RichText::new("Delete").color(theme.error)),
                    )
                    .clicked()
                {
                    app.soundpack.confirm_delete = true;
                }
            });
        });
}

/// Render the repository soundpacks panel
fn render_repository_soundpacks_panel(
    app: &mut PhoenixApp,
    ui: &mut egui::Ui,
    theme: &Theme,
    game_dir: &Path,
    is_busy: bool,
) {
    egui::Frame::none()
        .fill(theme.bg_medium)
        .rounding(8.0)
        .inner_margin(12.0)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Repository")
                    .color(theme.accent)
                    .size(13.0)
                    .strong(),
            );
            ui.add_space(8.0);

            // Repository list
            egui::ScrollArea::vertical()
                .id_salt("repository_soundpacks")
                .max_height(200.0)
                .show(ui, |ui| {
                    for (idx, repo_soundpack) in app.soundpack.repository.iter().enumerate() {
                        let is_selected = app.soundpack.repo_idx == Some(idx);
                        let is_installed =
                            soundpack::is_soundpack_installed(&app.soundpack.list, &repo_soundpack.name);

                        let display_name = if is_installed {
                            format!("{} ✓", repo_soundpack.viewname)
                        } else {
                            repo_soundpack.viewname.clone()
                        };

                        let text_color = if is_installed {
                            theme.success
                        } else {
                            theme.text_primary
                        };

                        let response = ui.selectable_label(
                            is_selected,
                            RichText::new(&display_name).color(text_color),
                        );

                        if response.clicked() {
                            app.soundpack.repo_idx = Some(idx);
                            app.soundpack.installed_idx = None;
                        }
                    }
                });

            ui.add_space(8.0);

            // Install button
            ui.horizontal(|ui| {
                let has_selection = app.soundpack.repo_idx.is_some();
                let selected_installed = app
                    .soundpack.repo_idx
                    .and_then(|i| app.soundpack.repository.get(i))
                    .map(|r| soundpack::is_soundpack_installed(&app.soundpack.list, &r.name))
                    .unwrap_or(false);

                if ui
                    .add_enabled(
                        has_selection && !is_busy && !selected_installed,
                        egui::Button::new(
                            RichText::new("Install Selected").color(theme.text_primary),
                        ),
                    )
                    .clicked()
                {
                    if let Some(idx) = app.soundpack.repo_idx {
                        if let Some(repo_soundpack) = app.soundpack.repository.get(idx) {
                            app.install_soundpack(repo_soundpack.clone(), game_dir);
                        }
                    }
                }
            });
        });
}

/// Render the soundpack details panel
fn render_soundpack_details_panel(app: &PhoenixApp, ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::none()
        .fill(theme.bg_medium)
        .rounding(8.0)
        .inner_margin(12.0)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Details")
                    .color(theme.accent)
                    .size(13.0)
                    .strong(),
            );
            ui.add_space(8.0);

            // Show details for selected soundpack
            if let Some(idx) = app.soundpack.installed_idx {
                if let Some(soundpack) = app.soundpack.list.get(idx) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("View name:").color(theme.text_muted));
                        ui.label(RichText::new(&soundpack.view_name).color(theme.text_primary));
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Name:").color(theme.text_muted));
                        ui.label(RichText::new(&soundpack.name).color(theme.text_primary));
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Path:").color(theme.text_muted));
                        ui.label(
                            RichText::new(soundpack.path.display().to_string())
                                .color(theme.text_secondary),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Size:").color(theme.text_muted));
                        ui.label(
                            RichText::new(format_size(soundpack.size))
                                .color(theme.text_primary),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Status:").color(theme.text_muted));
                        let status = if soundpack.enabled { "Enabled" } else { "Disabled" };
                        let color = if soundpack.enabled {
                            theme.success
                        } else {
                            theme.text_muted
                        };
                        ui.label(RichText::new(status).color(color));
                    });
                }
            } else if let Some(idx) = app.soundpack.repo_idx {
                if let Some(repo) = app.soundpack.repository.get(idx) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("View name:").color(theme.text_muted));
                        ui.label(RichText::new(&repo.viewname).color(theme.text_primary));
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Name:").color(theme.text_muted));
                        ui.label(RichText::new(&repo.name).color(theme.text_primary));
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("URL:").color(theme.text_muted));
                        ui.label(RichText::new(&repo.url).color(theme.text_secondary));
                    });
                    if let Some(size) = repo.size {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Size:").color(theme.text_muted));
                            ui.label(
                                RichText::new(format_size(size)).color(theme.text_primary),
                            );
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Homepage:").color(theme.text_muted));
                        if ui.link(&repo.homepage).clicked() {
                            let _ = open::that(&repo.homepage);
                        }
                    });
                }
            } else {
                ui.label(
                    RichText::new("Select a soundpack to view details")
                        .color(theme.text_muted)
                        .italics(),
                );
            }
        });
}

/// Render soundpack progress
fn render_soundpack_progress(app: &PhoenixApp, ui: &mut egui::Ui, theme: &Theme) {
    let progress = &app.soundpack.progress;

    progress_frame(theme).show(ui, |ui| {
            let status_color = match progress.phase {
                SoundpackPhase::Complete => theme.success,
                SoundpackPhase::Failed => theme.error,
                _ => theme.text_primary,
            };

            ui.label(RichText::new(progress.phase.description()).color(status_color));

            match progress.phase {
                SoundpackPhase::Downloading => {
                    ui.add_space(4.0);
                    let fraction = progress.download_fraction();
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .text(format!(
                                "{} / {} ({}/s)",
                                format_size(progress.bytes_downloaded),
                                format_size(progress.total_bytes),
                                format_size(progress.speed)
                            ))
                            .fill(theme.accent),
                    );
                }
                SoundpackPhase::Extracting => {
                    ui.add_space(4.0);
                    if progress.total_files > 0 {
                        let fraction = progress.extract_fraction();
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .text(format!(
                                    "{} / {} files",
                                    progress.files_extracted, progress.total_files
                                ))
                                .fill(theme.accent),
                        );
                    } else {
                        ui.add(
                            egui::ProgressBar::new(0.5)
                                .text("Extracting...")
                                .fill(theme.accent),
                        );
                    }
                    render_current_file(ui, &progress.current_file, theme);
                }
                SoundpackPhase::Deleting => {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                    });
                }
                SoundpackPhase::Complete => {
                    // Description already shown above, just show success bar
                    ui.add_space(4.0);
                    ui.add(
                        egui::ProgressBar::new(1.0)
                            .fill(theme.success),
                    );
                }
                SoundpackPhase::Failed => {
                    if let Some(ref err) = progress.error {
                        ui.label(RichText::new(err).color(theme.error));
                    }
                }
                _ => {}
            }
        });
}

/// Render delete confirmation dialog
fn render_soundpack_delete_dialog(
    app: &mut PhoenixApp,
    ui: &mut egui::Ui,
    theme: &Theme,
    _game_dir: &Path,
) {
    let selected_name = app
        .soundpack.installed_idx
        .and_then(|i| app.soundpack.list.get(i))
        .map(|s| s.view_name.clone())
        .unwrap_or_default();

    egui::Window::new("Confirm Delete")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label(format!(
                "Are you sure you want to delete '{}'?",
                selected_name
            ));
            ui.add_space(8.0);
            ui.label(
                RichText::new("This action cannot be undone.")
                    .color(theme.warning)
                    .small(),
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Delete").color(theme.error))
                    .clicked()
                {
                    if let Some(idx) = app.soundpack.installed_idx {
                        if let Some(soundpack) = app.soundpack.list.get(idx) {
                            let path = soundpack.path.clone();

                            let task = tokio::spawn(async move {
                                soundpack::delete_soundpack(path).await?;
                                // Return a dummy InstalledSoundpack to satisfy the type
                                Err(SoundpackError::Cancelled) // Will be handled specially
                            });

                            app.soundpack.task = Some(task);
                            app.soundpack.installed_idx = None;
                            // Show deleting progress
                            app.soundpack.progress = soundpack::SoundpackProgress {
                                phase: soundpack::SoundpackPhase::Deleting,
                                ..Default::default()
                            };
                            // Note: refresh_list is called in poll() after delete completes
                        }
                    }
                    app.soundpack.confirm_delete = false;
                }

                if ui.button("Cancel").clicked() {
                    app.soundpack.confirm_delete = false;
                }
            });
        });
}

/// Render browser download dialog
fn render_browser_download_dialog(
    app: &mut PhoenixApp,
    ui: &mut egui::Ui,
    theme: &Theme,
    game_dir: &Path,
) {
    let url = app.soundpack.browser_download_url.clone().unwrap_or_default();

    egui::Window::new("Browser Download Required")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label("This soundpack cannot be directly downloaded by the launcher.");
            ui.label("You need to download it manually with your browser.");
            ui.add_space(8.0);

            ui.label("1. Open the URL in your browser:");
            ui.horizontal(|ui| {
                ui.label(RichText::new(&url).color(theme.text_secondary).small());
            });
            ui.add_space(4.0);
            if ui.button("Open in Browser").clicked() {
                let _ = open::that(&url);
            }

            ui.add_space(8.0);
            ui.label("2. Download the soundpack and save it to your computer.");

            ui.add_space(8.0);
            ui.label("3. Select the downloaded file:");

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Select File...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Archives", &["zip", "rar", "7z"])
                        .set_title("Select Downloaded Soundpack")
                        .pick_file()
                    {
                        // Start installation from file
                        app.install_soundpack_from_file(path, game_dir);
                        app.soundpack.browser_download_url = None;
                        app.soundpack.browser_download_soundpack = None;
                    }
                }

                if ui.button("Cancel").clicked() {
                    app.soundpack.browser_download_url = None;
                    app.soundpack.browser_download_soundpack = None;
                }
            });
        });
}
