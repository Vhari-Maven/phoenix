//! Main tab UI rendering

use eframe::egui::{self, RichText, Vec2};
use egui_commonmark::CommonMarkViewer;

use crate::app::PhoenixApp;
use crate::theme::Theme;
use crate::update::UpdatePhase;
use crate::util::format_size;

/// Render the main tab content
pub fn render_main_tab(app: &mut PhoenixApp, ui: &mut egui::Ui) {
    let theme = app.current_theme.clone();

    // Game section
    render_section_frame(app, ui, "Game", |app, ui| {
        // Directory row
        ui.horizontal(|ui| {
            ui.label(RichText::new("Directory:").color(theme.text_muted));
            let dir_text = app
                .config
                .game
                .directory
                .as_deref()
                .unwrap_or("Not selected");
            ui.label(RichText::new(dir_text).color(theme.text_primary));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Browse...").clicked() {
                    app.browse_for_directory();
                }
            });
        });

        // Game info
        if let Some(ref info) = app.game_info {
            ui.add_space(8.0);

            // Create a subtle inner frame for game details
            egui::Frame::none()
                .fill(theme.bg_light.gamma_multiply(0.5))
                .rounding(4.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Left column - version info
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Version").color(theme.text_muted).size(11.0));
                            let version_text = info.version_display();
                            ui.label(RichText::new(version_text).color(theme.text_primary).size(16.0).strong());

                            if info.is_stable() {
                                ui.label(RichText::new("Stable").color(theme.success).size(11.0));
                            }
                        });

                        ui.add_space(40.0);

                        // Middle column - executable
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Executable").color(theme.text_muted).size(11.0));
                            ui.label(RichText::new(
                                info.executable.file_name().unwrap_or_default().to_string_lossy().to_string()
                            ).color(theme.text_primary));
                        });

                        ui.add_space(40.0);

                        // Right column - saves
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Saves").color(theme.text_muted).size(11.0));
                            ui.label(RichText::new(format_size(info.saves_size)).color(theme.text_primary));
                        });
                    });
                });
        }
    });

    ui.add_space(12.0);

    // Update section
    render_section_frame(app, ui, "Update", |app, ui| {
        // Track branch changes
        let previous_branch = app.config.game.branch.clone();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Branch:").color(theme.text_muted));
            egui::ComboBox::from_id_salt("branch_select")
                .selected_text(&app.config.game.branch)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut app.config.game.branch,
                        "stable".to_string(),
                        "Stable",
                    );
                    ui.selectable_value(
                        &mut app.config.game.branch,
                        "experimental".to_string(),
                        "Experimental",
                    );
                });

            ui.add_space(16.0);

            ui.label(RichText::new("Release:").color(theme.text_muted));

            if app.releases_loading {
                ui.spinner();
            } else {
                let has_releases = !app.current_releases().is_empty();
                if has_releases {
                    let releases = app.current_releases();
                    let selected_text = app
                        .selected_release_idx
                        .and_then(|i| releases.get(i))
                        .map(|r| r.name.as_str())
                        .unwrap_or("Select a release")
                        .to_string();

                    let release_labels: Vec<(usize, String)> = releases
                        .iter()
                        .enumerate()
                        .map(|(i, r)| (i, format!("{} ({})", r.name, &r.published_at[..10])))
                        .collect();

                    let current_selection = app.selected_release_idx;

                    egui::ComboBox::from_id_salt("release_select")
                        .selected_text(&selected_text)
                        .width(350.0)
                        .show_ui(ui, |ui| {
                            for (i, label) in &release_labels {
                                if ui
                                    .selectable_label(current_selection == Some(*i), label)
                                    .clicked()
                                {
                                    app.selected_release_idx = Some(*i);
                                }
                            }
                        });
                } else {
                    ui.label(RichText::new("No releases").color(theme.text_muted));
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!app.releases_loading, egui::Button::new("Refresh")).clicked() {
                    let branch = app.config.game.branch.clone();
                    app.fetch_releases_for_branch(&branch);
                }
            });
        });

        // If branch changed, update selection and fetch if needed
        if app.config.game.branch != previous_branch {
            if !app.has_releases_for_branch(&app.config.game.branch) {
                app.selected_release_idx = None;
                let branch = app.config.game.branch.clone();
                app.fetch_releases_for_branch(&branch);
            } else {
                app.selected_release_idx = Some(0);
            }
        }

        // Show error if any
        if let Some(ref err) = app.releases_error {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
        }

        // Show rate limit warning if low
        if app.rate_limit.is_low() {
            ui.add_space(4.0);
            let remaining = app.rate_limit.remaining.unwrap_or(0);
            let reset_mins = app.rate_limit.reset_in_minutes().unwrap_or(0);
            let warning = format!(
                "API limit: {} requests remaining (resets in {} min)",
                remaining, reset_mins
            );
            ui.label(RichText::new(warning).color(theme.warning).size(11.0));
        }

        // Update status indicator (only show when not updating)
        if !app.is_updating() && app.selected_release_idx.is_some() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if app.game_info.is_none() && app.config.game.directory.is_some() {
                    // No game installed - show install prompt
                    egui::Frame::none()
                        .fill(theme.accent.gamma_multiply(0.2))
                        .rounding(4.0)
                        .inner_margin(egui::vec2(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Ready to install").color(theme.accent).strong());
                        });
                } else if app.is_selected_release_different() {
                    // Different version selected - can update
                    egui::Frame::none()
                        .fill(theme.success.gamma_multiply(0.2))
                        .rounding(4.0)
                        .inner_margin(egui::vec2(8.0, 4.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Update available").color(theme.success).strong());
                        });
                } else if app.game_info.is_some() {
                    // Same version
                    ui.label(RichText::new("Up to date").color(theme.text_muted));
                }
            });
        }

        // Show update progress
        if app.is_updating() || app.update_progress.phase == UpdatePhase::Complete || app.update_progress.phase == UpdatePhase::Failed {
            ui.add_space(12.0);
            render_update_progress(app, ui, &theme);
        }

        // Show update error
        if let Some(ref err) = app.update_error {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
        }
    });

    ui.add_space(12.0);

    // Changelog section - use remaining vertical space
    let has_releases = !app.current_releases().is_empty();
    if has_releases && app.selected_release_idx.is_some() {
        // Calculate available height for changelog (leave room for buttons)
        let available_height = ui.available_height() - 70.0; // Reserve space for button row

        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.label(RichText::new("Changelog").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                if let Some(idx) = app.selected_release_idx {
                    let releases = app.current_releases();
                    if let Some(release) = releases.get(idx) {
                        // Release date header
                        let date = &release.published_at[..10];
                        ui.label(RichText::new(date).color(theme.accent).size(14.0).strong());
                        ui.add_space(8.0);

                        let body = release.body.clone();
                        let scroll_height = (available_height - 80.0).max(100.0);
                        egui::ScrollArea::vertical()
                            .max_height(scroll_height)
                            .show(ui, |ui| {
                                if let Some(ref text) = body {
                                    let processed = convert_urls_to_links(text);
                                    CommonMarkViewer::new()
                                        .show(ui, &mut app.markdown_cache, &processed);
                                } else {
                                    ui.label(RichText::new("No changelog available").color(theme.text_muted));
                                }
                            });
                    }
                }
            });
    }

    // Spacer to push buttons to bottom
    ui.add_space(ui.available_height() - 50.0);

    // Bottom action buttons - full width row
    let is_updating = app.is_updating();

    ui.horizontal(|ui| {
        let button_width = (ui.available_width() - 16.0) / 2.0;

        // Simple logic:
        // - No game installed + directory + release selected → Install
        // - Game installed + different release selected → Update/Switch
        // - Same version or no release selected → Disabled
        let has_game = app.game_info.is_some();
        let has_directory = app.config.game.directory.is_some();
        let has_release = app.selected_release_idx.is_some();

        // Check if selected release is different from installed version
        let is_different_version = app.is_selected_release_different();

        let can_install = !has_game && has_directory && has_release && !is_updating;
        let can_update = has_game && has_release && is_different_version && !is_updating;
        let can_click = can_install || can_update;

        let update_label = if is_updating {
            "Updating..."
        } else if can_install {
            "Install Game"
        } else if can_update {
            "Update Game"
        } else {
            "Up to Date"
        };

        let update_btn = egui::Button::new(
            RichText::new(update_label)
                .color(if can_click { theme.bg_darkest } else { theme.text_secondary })
                .size(16.0)
                .strong()
        )
        .fill(if can_click { theme.success } else { theme.bg_medium })
        .min_size(Vec2::new(button_width, 44.0))
        .rounding(6.0);

        if ui.add_enabled(can_click, update_btn).clicked() {
            app.start_update();
        }

        ui.add_space(16.0);

        // Launch button - right side, prominent (disabled during update)
        let can_launch = app.game_info.is_some() && !is_updating;
        let launch_btn = egui::Button::new(
            RichText::new("Launch Game")
                .color(if can_launch { theme.bg_darkest } else { theme.text_muted })
                .size(16.0)
                .strong()
        )
        .fill(if can_launch { theme.accent } else { theme.bg_medium })
        .min_size(Vec2::new(button_width, 44.0))
        .rounding(6.0);

        if ui.add_enabled(can_launch, launch_btn).clicked() {
            app.launch_game();
        }
    });
}

/// Render section frame with title
fn render_section_frame<F>(app: &mut PhoenixApp, ui: &mut egui::Ui, title: &str, content: F)
where
    F: FnOnce(&mut PhoenixApp, &mut egui::Ui),
{
    let theme = app.current_theme.clone();

    egui::Frame::none()
        .fill(theme.bg_medium)
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).color(theme.accent).size(13.0).strong());
            ui.add_space(12.0);
            content(app, ui);
        });
}

/// Render update progress UI
fn render_update_progress(app: &PhoenixApp, ui: &mut egui::Ui, theme: &Theme) {
    let progress = &app.update_progress;

    egui::Frame::none()
        .fill(theme.bg_light.gamma_multiply(0.5))
        .rounding(6.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Phase label with icon
            let (phase_text, phase_color) = match progress.phase {
                UpdatePhase::Downloading => ("Downloading...", theme.accent),
                UpdatePhase::BackingUp => ("Backing up current installation...", theme.warning),
                UpdatePhase::Extracting => ("Extracting files...", theme.accent),
                UpdatePhase::Restoring => ("Restoring saves and settings...", theme.accent),
                UpdatePhase::Complete => ("Update complete!", theme.success),
                UpdatePhase::Failed => ("Update failed", theme.error),
                UpdatePhase::Idle => ("Ready", theme.text_muted),
            };

            ui.label(RichText::new(phase_text).color(phase_color).size(13.0).strong());
            ui.add_space(8.0);

            // Progress bar for download/extract phases
            match progress.phase {
                UpdatePhase::Downloading => {
                    let fraction = progress.download_fraction();
                    ui.add(egui::ProgressBar::new(fraction).show_percentage());

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        // Downloaded / Total
                        let downloaded = format_size(progress.bytes_downloaded);
                        let total = format_size(progress.total_bytes);
                        ui.label(RichText::new(format!("{} / {}", downloaded, total)).color(theme.text_muted).size(11.0));

                        ui.add_space(16.0);

                        // Speed
                        if progress.speed > 0 {
                            let speed = format_size(progress.speed);
                            ui.label(RichText::new(format!("{}/s", speed)).color(theme.text_muted).size(11.0));
                        }
                    });
                }
                UpdatePhase::Extracting => {
                    let fraction = progress.extract_fraction();
                    ui.add(egui::ProgressBar::new(fraction).show_percentage());

                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "{} / {} files",
                            progress.files_extracted,
                            progress.total_files
                        ))
                        .color(theme.text_muted)
                        .size(11.0)
                    );

                    if !progress.current_file.is_empty() {
                        ui.label(
                            RichText::new(&progress.current_file)
                                .color(theme.text_muted)
                                .size(10.0)
                        );
                    }
                }
                UpdatePhase::BackingUp | UpdatePhase::Restoring => {
                    // Indeterminate progress (spinner-like)
                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                }
                _ => {}
            }
        });
}

/// Convert raw URLs in text to markdown links
/// This makes URLs clickable in the markdown renderer
fn convert_urls_to_links(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let mut chars = text.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        // Check for http:// or https://
        if c == 'h' && text[i..].starts_with("http") {
            // Check if we're already inside a markdown link [text](url) or <url>
            let before = &text[..i];
            let in_markdown_link = before.ends_with("](") || before.ends_with('<');

            if !in_markdown_link {
                // Find the end of the URL (space, newline, or end of string)
                let url_start = i;
                let mut url_end = i;

                for (j, ch) in text[i..].char_indices() {
                    if ch.is_whitespace() || ch == ')' && !text[i..i + j].contains('(') {
                        url_end = i + j;
                        break;
                    }
                    url_end = i + j + ch.len_utf8();
                }

                let url = &text[url_start..url_end];

                // Only convert if it looks like a valid URL
                if url.starts_with("https://") || url.starts_with("http://") {
                    result.push('<');
                    result.push_str(url);
                    result.push('>');

                    // Skip the characters we just processed
                    while let Some(&(j, _)) = chars.peek() {
                        if j >= url_end {
                            break;
                        }
                        chars.next();
                    }
                    continue;
                }
            }
        }
        result.push(c);
    }

    result
}
