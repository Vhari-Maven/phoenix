use anyhow::Result;
use eframe::egui::{self, Color32, RichText, Rounding, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use futures::FutureExt;
use std::path::PathBuf;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};
use crate::github::{FetchResult, GitHubClient, RateLimitInfo, Release};
use crate::theme::{Theme, ThemePreset};

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

    // GitHub integration
    /// GitHub API client
    github_client: GitHubClient,
    /// Fetched experimental releases
    experimental_releases: Vec<Release>,
    /// Fetched stable releases
    stable_releases: Vec<Release>,
    /// Index of selected release in current list
    selected_release_idx: Option<usize>,
    /// Async task for fetching releases
    releases_task: Option<JoinHandle<Result<FetchResult<Vec<Release>>>>>,
    /// Which branch is being fetched
    fetching_branch: Option<String>,
    /// Whether releases are currently being fetched
    releases_loading: bool,
    /// Error message from last fetch attempt
    releases_error: Option<String>,

    // Markdown rendering
    /// Cache for markdown rendering
    markdown_cache: CommonMarkCache,

    // Rate limiting
    /// Last known rate limit info from GitHub API
    rate_limit: RateLimitInfo,

    // UI state
    /// Current theme
    current_theme: Theme,
    /// Currently selected tab
    active_tab: Tab,
    /// Whether theme needs to be applied
    theme_dirty: bool,
}

/// Application tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Main,
    Backups,
    Soundpacks,
    Settings,
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

        // Create GitHub client
        let github_client = GitHubClient::default();

        // Load theme
        let current_theme = config.launcher.theme.theme();

        let branch = config.game.branch.clone();

        let mut app = Self {
            config,
            db,
            game_info,
            status_message,
            github_client,
            experimental_releases: Vec::new(),
            stable_releases: Vec::new(),
            selected_release_idx: None,
            releases_task: None,
            fetching_branch: None,
            releases_loading: false,
            releases_error: None,
            markdown_cache: CommonMarkCache::default(),
            rate_limit: RateLimitInfo::default(),
            current_theme,
            active_tab: Tab::default(),
            theme_dirty: true, // Apply theme on first frame
        };

        // Auto-fetch releases for current branch on startup
        app.fetch_releases_for_branch(&branch);

        app
    }

    /// Get releases for the current branch
    fn current_releases(&self) -> &Vec<Release> {
        if self.config.game.branch == "stable" {
            &self.stable_releases
        } else {
            &self.experimental_releases
        }
    }

    /// Check if the selected release is different from the installed version
    /// Returns (is_update_available, description)
    fn check_update_status(&self) -> Option<(bool, String)> {
        let game_info = self.game_info.as_ref()?;
        let releases = self.current_releases();
        let selected_release = self.selected_release_idx.and_then(|i| releases.get(i))?;

        let installed_version = game_info.version_display();

        // For stable releases, compare version strings directly
        if game_info.is_stable() && self.config.game.branch == "stable" {
            // Compare stable version tags (e.g., "0.F-3" vs "0.G")
            let release_version = &selected_release.tag_name;
            if installed_version == release_version {
                return Some((false, "Up to date".to_string()));
            } else {
                return Some((true, format!("Update: {} -> {}", installed_version, release_version)));
            }
        }

        // For experimental builds, the installed version is a commit SHA (7 chars)
        // and releases have tag names like "cdda-experimental-2024-01-15-1234"
        // Best approach: compare dates from the release
        if let Some(version_info) = &game_info.version_info {
            // If we have a release date for installed version, compare with selected
            if let Some(ref installed_date) = version_info.released_on {
                let release_date = &selected_release.published_at[..10]; // YYYY-MM-DD
                if installed_date == release_date {
                    return Some((false, format!("Current build ({})", installed_version)));
                } else if release_date > installed_date.as_str() {
                    return Some((true, format!("Update available: {} -> {}", installed_date, release_date)));
                } else {
                    return Some((false, format!("Installed is newer ({})", installed_version)));
                }
            }
        }

        // Fallback: check if the installed commit SHA appears in the release name or tag
        // Extract just the SHA part if version contains date (format: "2024-01-15 (abc1234)")
        let sha_part = if installed_version.contains('(') {
            installed_version
                .split('(')
                .nth(1)
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or(installed_version)
        } else {
            installed_version
        };

        let tag_lower = selected_release.tag_name.to_lowercase();
        let name_lower = selected_release.name.to_lowercase();
        if tag_lower.contains(&sha_part.to_lowercase()) || name_lower.contains(&sha_part.to_lowercase()) {
            return Some((false, "Up to date".to_string()));
        }

        // Can't determine exact match, assume update is available if latest is selected
        if self.selected_release_idx == Some(0) {
            Some((true, "Newer version available".to_string()))
        } else {
            Some((false, "Older version selected".to_string()))
        }
    }

    /// Check if we have releases for the given branch
    fn has_releases_for_branch(&self, branch: &str) -> bool {
        if branch == "stable" {
            !self.stable_releases.is_empty()
        } else {
            !self.experimental_releases.is_empty()
        }
    }

    /// Start fetching releases for a specific branch
    fn fetch_releases_for_branch(&mut self, branch: &str) {
        if self.releases_loading {
            return; // Already fetching
        }

        self.releases_loading = true;
        self.releases_error = None;
        self.fetching_branch = Some(branch.to_string());
        self.status_message = format!("Fetching {} releases...", branch);

        let client = self.github_client.clone();
        let is_stable = branch == "stable";

        self.releases_task = Some(tokio::spawn(async move {
            if is_stable {
                client.get_stable_releases().await
            } else {
                client.get_experimental_releases(50).await
            }
        }));
    }

    /// Poll the async releases task for completion
    fn poll_releases_task(&mut self, ctx: &egui::Context) {
        if let Some(task) = &mut self.releases_task {
            if task.is_finished() {
                let task = self.releases_task.take().unwrap();
                let branch = self.fetching_branch.take();

                // Use now_or_never() since we know the task is finished
                match task.now_or_never() {
                    Some(Ok(Ok(result))) => {
                        let count = result.data.len();
                        // Update rate limit info
                        self.rate_limit = result.rate_limit;

                        // Store in appropriate list based on which branch we fetched
                        let is_current_branch = branch.as_deref() == Some(self.config.game.branch.as_str());
                        if branch.as_deref() == Some("stable") {
                            self.stable_releases = result.data;
                        } else {
                            self.experimental_releases = result.data;
                        }
                        // Auto-select latest release if this is for the current branch
                        if is_current_branch && count > 0 {
                            self.selected_release_idx = Some(0);
                        }
                        self.status_message = format!("Fetched {} releases", count);
                        tracing::info!("Fetched {} {} releases from GitHub", count, branch.as_deref().unwrap_or("unknown"));
                    }
                    Some(Ok(Err(e))) => {
                        let msg = e.to_string();
                        tracing::error!("Failed to fetch releases: {}", msg);
                        self.releases_error = Some(msg.clone());
                        self.status_message = format!("Error: {}", msg);
                    }
                    Some(Err(e)) => {
                        let msg = e.to_string();
                        tracing::error!("Task panicked: {}", msg);
                        self.releases_error = Some(msg);
                    }
                    None => {
                        // Shouldn't happen since we checked is_finished()
                        tracing::warn!("Task not ready despite is_finished()");
                    }
                }
                self.releases_loading = false;
            } else {
                // Task still running, request repaint to keep polling
                ctx.request_repaint();
            }
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
        // Apply theme if needed
        if self.theme_dirty {
            self.current_theme.apply(ctx);
            self.theme_dirty = false;
        }

        // Poll async tasks
        self.poll_releases_task(ctx);

        let theme = &self.current_theme;

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar")
            .frame(egui::Frame::none().fill(theme.bg_darkest).inner_margin(4.0))
            .show(ctx, |ui| {
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
        egui::TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame::none().fill(theme.bg_darkest).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status_message).color(theme.text_muted));
                });
            });

        // Main content area
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme.bg_dark).inner_margin(16.0))
            .show(ctx, |ui| {
                // Tab bar
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.render_tab(ui, Tab::Main, "Main");
                    self.render_tab(ui, Tab::Backups, "Backups");
                    self.render_tab(ui, Tab::Soundpacks, "Soundpacks");
                    self.render_tab(ui, Tab::Settings, "Settings");
                });

                ui.add_space(16.0);

                // Tab content
                match self.active_tab {
                    Tab::Main => self.render_main_tab(ui),
                    Tab::Backups => self.render_backups_tab(ui),
                    Tab::Soundpacks => self.render_soundpacks_tab(ui),
                    Tab::Settings => self.render_settings_tab(ui),
                }
            });
    }
}

impl PhoenixApp {
    /// Render a tab button
    fn render_tab(&mut self, ui: &mut egui::Ui, tab: Tab, label: &str) {
        let theme = &self.current_theme;
        let is_active = self.active_tab == tab;

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
            self.active_tab = tab;
        }
    }

    /// Render the main tab content
    fn render_main_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme.clone();

        // Game section
        self.render_section_frame(ui, "Game", |app, ui| {
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
                                ui.label(RichText::new(game::format_size(info.saves_size)).color(theme.text_primary));
                            });
                        });
                    });
            }
        });

        ui.add_space(12.0);

        // Update section
        self.render_section_frame(ui, "Update", |app, ui| {
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

            // Update status indicator
            if let Some((is_update, status_text)) = app.check_update_status() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if is_update {
                        // Update available badge
                        egui::Frame::none()
                            .fill(theme.success.gamma_multiply(0.2))
                            .rounding(4.0)
                            .inner_margin(egui::vec2(8.0, 4.0))
                            .show(ui, |ui| {
                                ui.label(RichText::new(format!("UPDATE {}", status_text)).color(theme.success).strong());
                            });
                    } else {
                        ui.label(RichText::new(status_text).color(theme.text_muted));
                    }
                });
            }
        });

        ui.add_space(12.0);

        // Changelog section - use remaining vertical space
        let has_releases = !self.current_releases().is_empty();
        if has_releases && self.selected_release_idx.is_some() {
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

                    if let Some(idx) = self.selected_release_idx {
                        let releases = self.current_releases();
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
                                            .show(ui, &mut self.markdown_cache, &processed);
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
        ui.horizontal(|ui| {
            let button_width = (ui.available_width() - 16.0) / 2.0;

            // Update button - left side
            let update_available = self.check_update_status().map(|(u, _)| u).unwrap_or(false);
            let update_btn = egui::Button::new(
                RichText::new(if update_available { "Update Game" } else { "Update" })
                    .color(if update_available { theme.bg_darkest } else { theme.text_secondary })
                    .size(16.0)
                    .strong()
            )
            .fill(if update_available { theme.success } else { theme.bg_medium })
            .min_size(Vec2::new(button_width, 44.0))
            .rounding(6.0);

            if ui.add_enabled(update_available && self.selected_release_idx.is_some(), update_btn).clicked() {
                // TODO: Implement update (Spiral 4)
            }

            ui.add_space(16.0);

            // Launch button - right side, prominent
            let can_launch = self.game_info.is_some();
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
                self.launch_game();
            }
        });
    }

    /// Render section frame with title
    fn render_section_frame<F>(&mut self, ui: &mut egui::Ui, title: &str, content: F)
    where
        F: FnOnce(&mut PhoenixApp, &mut egui::Ui),
    {
        let theme = self.current_theme.clone();

        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);
                content(self, ui);
            });
    }

    /// Render the backups tab
    fn render_backups_tab(&mut self, ui: &mut egui::Ui) {
        let theme = &self.current_theme;
        ui.label(RichText::new("Backups").color(theme.text_primary).size(20.0).strong());
        ui.add_space(8.0);
        ui.label(RichText::new("Backup management coming in a future update.").color(theme.text_muted));
    }

    /// Render the soundpacks tab
    fn render_soundpacks_tab(&mut self, ui: &mut egui::Ui) {
        let theme = &self.current_theme;
        ui.label(RichText::new("Soundpacks").color(theme.text_primary).size(20.0).strong());
        ui.add_space(8.0);
        ui.label(RichText::new("Soundpack management coming in a future update.").color(theme.text_muted));
    }

    /// Render the settings tab
    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme.clone();

        ui.label(RichText::new("Settings").color(theme.text_primary).size(20.0).strong());
        ui.add_space(16.0);

        // Appearance section
        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.label(RichText::new("Appearance").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Theme:").color(theme.text_muted));

                    let current_name = self.config.launcher.theme.name();
                    egui::ComboBox::from_id_salt("theme_select")
                        .selected_text(current_name)
                        .show_ui(ui, |ui| {
                            for preset in ThemePreset::all() {
                                if ui.selectable_label(
                                    self.config.launcher.theme == *preset,
                                    preset.name(),
                                ).clicked() {
                                    self.config.launcher.theme = *preset;
                                    self.current_theme = preset.theme();
                                    self.theme_dirty = true;
                                    self.save_config();
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
                            egui::show_tooltip(ui.ctx(), ui.layer_id(), egui::Id::new(label), |ui| {
                                ui.label(label);
                            });
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
                ui.label(RichText::new("Behavior").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                if ui.checkbox(&mut self.config.launcher.keep_open, "Keep launcher open after game exits").changed() {
                    self.save_config();
                }

                if ui.checkbox(&mut self.config.updates.check_on_startup, "Check for updates on startup").changed() {
                    self.save_config();
                }
            });

        ui.add_space(12.0);

        // Game settings section
        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.label(RichText::new("Game").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Command line parameters:").color(theme.text_muted));
                });
                ui.add_space(4.0);
                if ui.text_edit_singleline(&mut self.config.game.command_params).changed() {
                    self.save_config();
                }
            });
    }
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
