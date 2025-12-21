use anyhow::Result;
use eframe::egui::{self, Color32, RichText, Rounding, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use futures::FutureExt;
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};
use crate::github::{FetchResult, GitHubClient, RateLimitInfo, Release};
use crate::theme::{Theme, ThemePreset};
use crate::update::{self, UpdatePhase, UpdateProgress};

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

    // Update state
    /// Async task for update operation
    update_task: Option<JoinHandle<Result<()>>>,
    /// Channel receiver for update progress
    update_progress_rx: Option<watch::Receiver<UpdateProgress>>,
    /// Current update progress
    update_progress: UpdateProgress,
    /// Error message from last update attempt
    update_error: Option<String>,
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
        let startup_start = Instant::now();

        // Load configuration
        let phase_start = Instant::now();
        let config = Config::load().unwrap_or_default();
        tracing::debug!("Config loaded in {:.1}ms", phase_start.elapsed().as_secs_f32() * 1000.0);

        // Open database
        let phase_start = Instant::now();
        let db = match Database::open() {
            Ok(db) => Some(db),
            Err(e) => {
                tracing::error!("Failed to open database: {}", e);
                None
            }
        };
        tracing::debug!("Database opened in {:.1}ms", phase_start.elapsed().as_secs_f32() * 1000.0);

        // Try to detect game if directory is configured
        let phase_start = Instant::now();
        let game_info = config
            .game
            .directory
            .as_ref()
            .and_then(|dir| {
                game::detect_game_with_db(&PathBuf::from(dir), db.as_ref())
                    .ok()
                    .flatten()
            });
        if config.game.directory.is_some() {
            tracing::debug!("Game detection in {:.1}ms", phase_start.elapsed().as_secs_f32() * 1000.0);
        }

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
            update_task: None,
            update_progress_rx: None,
            update_progress: UpdateProgress::default(),
            update_error: None,
        };

        // Auto-fetch releases for current branch on startup
        app.fetch_releases_for_branch(&branch);

        tracing::info!("Startup complete in {:.1}ms", startup_start.elapsed().as_secs_f32() * 1000.0);

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

    /// Simple check: is the selected release different from the installed version?
    /// Returns true if they are different (can update/switch), false if same or can't compare
    fn is_selected_release_different(&self) -> bool {
        let Some(game_info) = &self.game_info else {
            return false; // No game installed
        };
        let releases = self.current_releases();
        let Some(selected_release) = self.selected_release_idx.and_then(|i| releases.get(i)) else {
            return false; // No release selected
        };

        // Compare build numbers - this distinguishes multiple builds on the same day
        // Installed: build_number like "2025-12-20-2147" stored in released_on
        // Release tag: like "cdda-experimental-2025-12-20-2147"
        if let Some(version_info) = &game_info.version_info {
            if let Some(ref installed_build) = version_info.released_on {
                // Check if the release tag contains our build number
                // e.g., "cdda-experimental-2025-12-20-2147" contains "2025-12-20-2147"
                if selected_release.tag_name.contains(installed_build) {
                    return false; // Same version
                }
                return true; // Different version
            }
        }

        // Fallback: assume different (allow update)
        true
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

    /// Start the update process for the selected release
    fn start_update(&mut self) {
        // Don't start if already updating
        if self.update_task.is_some() {
            return;
        }

        // Get the selected release and its Windows asset
        let releases = self.current_releases();
        let release = match self.selected_release_idx.and_then(|i| releases.get(i)) {
            Some(r) => r.clone(),
            None => {
                self.update_error = Some("No release selected".to_string());
                return;
            }
        };

        let asset = match GitHubClient::find_windows_asset(&release) {
            Some(a) => a.clone(),
            None => {
                self.update_error = Some("No Windows build available for this release".to_string());
                return;
            }
        };

        // Get the game directory
        let game_dir = match &self.config.game.directory {
            Some(dir) => PathBuf::from(dir),
            None => {
                self.update_error = Some("No game directory configured".to_string());
                return;
            }
        };

        // Get download directory
        let download_dir = match update::download_dir() {
            Ok(dir) => dir,
            Err(e) => {
                self.update_error = Some(format!("Failed to get download directory: {}", e));
                return;
            }
        };

        let zip_path = download_dir.join(&asset.name);
        let download_url = asset.browser_download_url.clone();

        // Create progress channel
        let (progress_tx, progress_rx) = watch::channel(UpdateProgress::default());
        self.update_progress_rx = Some(progress_rx);
        self.update_error = None;
        self.update_progress = UpdateProgress {
            phase: UpdatePhase::Downloading,
            total_bytes: asset.size,
            ..Default::default()
        };

        // Clone what we need for the async task
        let client = self.github_client.clone();
        let prevent_save_move = self.config.updates.prevent_save_move;
        let remove_previous_version = self.config.updates.remove_previous_version;

        self.status_message = format!("Downloading {}...", release.name);
        tracing::info!("Starting update: {} from {}", asset.name, download_url);

        // Spawn the update task
        self.update_task = Some(tokio::spawn(async move {
            // Phase 1: Download
            let result = update::download_asset(
                client.client().clone(),
                download_url,
                zip_path.clone(),
                progress_tx.clone(),
            ).await?;

            tracing::info!("Download complete: {} bytes", result.bytes);

            // Phase 2: Install (backup, extract, restore with smart migration)
            update::install_update(
                result.file_path,
                game_dir,
                progress_tx,
                prevent_save_move,
                remove_previous_version,
            ).await?;

            Ok(())
        }));
    }

    /// Poll the update task for progress and completion
    fn poll_update_task(&mut self, ctx: &egui::Context) {
        // Update progress from channel
        if let Some(rx) = &mut self.update_progress_rx {
            if rx.has_changed().unwrap_or(false) {
                self.update_progress = rx.borrow_and_update().clone();

                // Update status message based on phase
                self.status_message = self.update_progress.phase.description().to_string();
            }
        }

        // Check if task is complete
        if let Some(task) = &mut self.update_task {
            if task.is_finished() {
                let task = self.update_task.take().unwrap();
                self.update_progress_rx = None;

                match task.now_or_never() {
                    Some(Ok(Ok(()))) => {
                        self.update_progress.phase = UpdatePhase::Complete;
                        self.status_message = "Update complete! Refreshing game info...".to_string();
                        tracing::info!("Update completed successfully");

                        // Refresh game info
                        self.refresh_game_info();
                    }
                    Some(Ok(Err(e))) => {
                        self.update_progress.phase = UpdatePhase::Failed;
                        let msg = e.to_string();
                        tracing::error!("Update failed: {}", msg);
                        self.update_error = Some(msg.clone());
                        self.status_message = format!("Update failed: {}", msg);
                    }
                    Some(Err(e)) => {
                        self.update_progress.phase = UpdatePhase::Failed;
                        let msg = format!("Update task panicked: {}", e);
                        tracing::error!("{}", msg);
                        self.update_error = Some(msg.clone());
                        self.status_message = msg;
                    }
                    None => {
                        tracing::warn!("Update task not ready despite is_finished()");
                    }
                }
            } else {
                // Task still running, keep polling
                ctx.request_repaint();
            }
        }
    }

    /// Refresh game info after an update
    fn refresh_game_info(&mut self) {
        if let Some(ref dir) = self.config.game.directory {
            match game::detect_game_with_db(&PathBuf::from(dir), self.db.as_ref()) {
                Ok(Some(info)) => {
                    self.status_message = format!("Game updated to: {}", info.version_display());
                    self.game_info = Some(info);
                }
                Ok(None) => {
                    self.status_message = "Update complete, but game not detected".to_string();
                    self.game_info = None;
                }
                Err(e) => {
                    self.status_message = format!("Update complete, detection error: {}", e);
                }
            }
        }
    }

    /// Check if an update is currently in progress
    fn is_updating(&self) -> bool {
        self.update_task.is_some()
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
        self.poll_update_task(ctx);

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
                app.render_update_progress(ui, &theme);
            }

            // Show update error
            if let Some(ref err) = app.update_error {
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
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
        let is_updating = self.is_updating();

        ui.horizontal(|ui| {
            let button_width = (ui.available_width() - 16.0) / 2.0;

            // Simple logic:
            // - No game installed + directory + release selected → Install
            // - Game installed + different release selected → Update/Switch
            // - Same version or no release selected → Disabled
            let has_game = self.game_info.is_some();
            let has_directory = self.config.game.directory.is_some();
            let has_release = self.selected_release_idx.is_some();

            // Check if selected release is different from installed version
            let is_different_version = self.is_selected_release_different();

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
                self.start_update();
            }

            ui.add_space(16.0);

            // Launch button - right side, prominent (disabled during update)
            let can_launch = self.game_info.is_some() && !is_updating;
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

        // Updates section
        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.label(RichText::new("Updates").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                if ui.checkbox(
                    &mut self.config.updates.prevent_save_move,
                    "Do not copy saves during updates"
                ).changed() {
                    self.save_config();
                }
                ui.label(RichText::new("  Leave saves in place instead of copying from previous_version/")
                    .color(theme.text_muted).size(11.0));

                ui.add_space(8.0);

                if ui.checkbox(
                    &mut self.config.updates.remove_previous_version,
                    "Remove previous_version after update"
                ).changed() {
                    self.save_config();
                }
                ui.label(RichText::new("  Not recommended - removes rollback capability")
                    .color(theme.warning).size(11.0));
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

    /// Render update progress UI
    fn render_update_progress(&self, ui: &mut egui::Ui, theme: &Theme) {
        let progress = &self.update_progress;

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
}

/// Format bytes as human-readable size
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
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
