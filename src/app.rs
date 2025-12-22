use anyhow::Result;
use eframe::egui::{self, Color32, RichText, Rounding, Vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use futures::FutureExt;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::backup::{self, AutoBackupType, BackupInfo, BackupPhase, BackupProgress};
use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};
use crate::github::{FetchResult, GitHubClient, RateLimitInfo, Release};
use crate::soundpack::{
    self, InstalledSoundpack, RepoSoundpack, SoundpackError, SoundpackPhase, SoundpackProgress,
};
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

    // Backup state
    /// List of available backups
    backup_list: Vec<BackupInfo>,
    /// Whether backup list is being loaded
    backup_list_loading: bool,
    /// Index of selected backup in list
    backup_selected_idx: Option<usize>,
    /// Input field for manual backup name
    backup_name_input: String,
    /// Async task for backup operation
    backup_task: Option<JoinHandle<Result<(), backup::BackupError>>>,
    /// Async task for loading backup list
    backup_list_task: Option<JoinHandle<Result<Vec<BackupInfo>, backup::BackupError>>>,
    /// Channel receiver for backup progress
    backup_progress_rx: Option<watch::Receiver<BackupProgress>>,
    /// Current backup progress
    backup_progress: BackupProgress,
    /// Error message from last backup attempt
    backup_error: Option<String>,
    /// Whether to show delete confirmation
    backup_confirm_delete: bool,
    /// Whether to show restore confirmation
    backup_confirm_restore: bool,

    // Soundpack state
    /// List of installed soundpacks
    soundpack_list: Vec<InstalledSoundpack>,
    /// Whether soundpack list is being loaded
    soundpack_list_loading: bool,
    /// Index of selected installed soundpack
    soundpack_installed_idx: Option<usize>,
    /// Index of selected repository soundpack
    soundpack_repo_idx: Option<usize>,
    /// Repository soundpacks
    soundpack_repository: Vec<RepoSoundpack>,
    /// Async task for soundpack install/delete operations
    soundpack_task: Option<JoinHandle<Result<InstalledSoundpack, SoundpackError>>>,
    /// Async task for loading soundpack list
    soundpack_list_task: Option<JoinHandle<Result<Vec<InstalledSoundpack>, SoundpackError>>>,
    /// Channel receiver for soundpack progress
    soundpack_progress_rx: Option<watch::Receiver<SoundpackProgress>>,
    /// Current soundpack progress
    soundpack_progress: SoundpackProgress,
    /// Error message from last soundpack operation
    soundpack_error: Option<String>,
    /// Whether to show delete confirmation
    soundpack_confirm_delete: bool,
    /// Browser download state
    browser_download_url: Option<String>,
    /// Browser download repo soundpack
    browser_download_soundpack: Option<RepoSoundpack>,

    // Dialogs
    /// Whether to show the About dialog
    show_about_dialog: bool,
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
            backup_list: Vec::new(),
            backup_list_loading: false,
            backup_selected_idx: None,
            backup_name_input: String::new(),
            backup_task: None,
            backup_list_task: None,
            backup_progress_rx: None,
            backup_progress: BackupProgress::default(),
            backup_error: None,
            backup_confirm_delete: false,
            backup_confirm_restore: false,
            soundpack_list: Vec::new(),
            soundpack_list_loading: false,
            soundpack_installed_idx: None,
            soundpack_repo_idx: None,
            soundpack_repository: soundpack::load_repository(),
            soundpack_task: None,
            soundpack_list_task: None,
            soundpack_progress_rx: None,
            soundpack_progress: SoundpackProgress::default(),
            soundpack_error: None,
            soundpack_confirm_delete: false,
            browser_download_url: None,
            browser_download_soundpack: None,

            show_about_dialog: false,
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
        let backup_before_update = self.config.backups.backup_before_update;
        let compression_level = self.config.backups.compression_level;
        let max_backups = self.config.backups.max_count;
        let version_tag = release.tag_name.clone();

        self.status_message = format!("Downloading {}...", release.name);
        tracing::info!("Starting update: {} from {}", asset.name, download_url);

        // Spawn the update task
        self.update_task = Some(tokio::spawn(async move {
            // Phase 0: Auto-backup before update (if enabled)
            if backup_before_update {
                tracing::info!("Creating pre-update backup...");
                let backup_progress_tx = watch::channel(BackupProgress::default()).0;
                match backup::create_auto_backup(
                    &game_dir,
                    AutoBackupType::BeforeUpdate,
                    Some(&version_tag),
                    compression_level,
                    max_backups,
                    backup_progress_tx,
                ).await {
                    Ok(Some(info)) => {
                        tracing::info!("Pre-update backup created: {}", info.name);
                    }
                    Ok(None) => {
                        tracing::info!("No saves to backup before update");
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create pre-update backup: {} (continuing with update)", e);
                        // Continue with update even if backup fails
                    }
                }
            }

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
        self.poll_backup_task(ctx);
        self.poll_soundpack_tasks(ctx);

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
                            self.show_about_dialog = true;
                            ui.close_menu();
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

        // About dialog
        self.render_about_dialog(ctx);
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
            let previous_tab = self.active_tab;
            self.active_tab = tab;

            // Load backup list when switching to Backups tab
            if tab == Tab::Backups && previous_tab != Tab::Backups {
                if let Some(ref dir) = self.config.game.directory {
                    if self.backup_list.is_empty() && !self.backup_list_loading {
                        self.refresh_backup_list(&PathBuf::from(dir));
                    }
                }
            }

            // Load soundpack list when switching to Soundpacks tab
            if tab == Tab::Soundpacks && previous_tab != Tab::Soundpacks {
                if let Some(ref dir) = self.config.game.directory {
                    if self.soundpack_list.is_empty() && !self.soundpack_list_loading {
                        self.refresh_soundpack_list(&PathBuf::from(dir));
                    }
                }
            }
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
        let theme = self.current_theme.clone();

        ui.label(RichText::new("Backups").color(theme.text_primary).size(20.0).strong());
        ui.add_space(16.0);

        // Check if game directory is set
        let game_dir = self.config.game.directory.as_ref().map(PathBuf::from);

        if game_dir.is_none() {
            ui.label(RichText::new("Set a game directory in Main tab to manage backups.").color(theme.text_muted));
            return;
        }

        let game_dir = game_dir.unwrap();
        let is_busy = self.is_backup_busy();

        // Manual backup section
        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Create Backup").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Backup name:").color(theme.text_muted));
                    ui.add_sized(
                        [200.0, 20.0],
                        egui::TextEdit::singleline(&mut self.backup_name_input)
                            .hint_text("my_backup")
                    );

                    ui.add_space(16.0);

                    let can_backup = !is_busy && !self.backup_name_input.trim().is_empty();
                    if ui.add_enabled(can_backup, egui::Button::new("Backup Current Saves")).clicked() {
                        self.start_manual_backup(&game_dir);
                    }
                });

                // Show validation error
                if !self.backup_name_input.is_empty() {
                    if let Err(e) = self.validate_backup_name(&self.backup_name_input) {
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
                    ui.label(RichText::new("Available Backups").color(theme.accent).size(13.0).strong());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!is_busy, egui::Button::new("Refresh")).clicked() {
                            self.refresh_backup_list(&game_dir);
                        }
                    });
                });
                ui.add_space(12.0);

                if self.backup_list_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Loading backups...").color(theme.text_muted));
                    });
                } else if self.backup_list.is_empty() {
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
                                    ui.label(RichText::new("Name").color(theme.text_muted).strong().size(11.0));
                                    ui.label(RichText::new("Date").color(theme.text_muted).strong().size(11.0));
                                    ui.label(RichText::new("Worlds").color(theme.text_muted).strong().size(11.0));
                                    ui.label(RichText::new("Chars").color(theme.text_muted).strong().size(11.0));
                                    ui.label(RichText::new("Size").color(theme.text_muted).strong().size(11.0));
                                    ui.label(RichText::new("Uncomp.").color(theme.text_muted).strong().size(11.0));
                                    ui.label(RichText::new("Ratio").color(theme.text_muted).strong().size(11.0));
                                    ui.end_row();

                                    // Data rows
                                    for (i, backup) in self.backup_list.iter().enumerate() {
                                        let is_selected = self.backup_selected_idx == Some(i);
                                        let text_color = if is_selected { theme.accent } else { theme.text_primary };

                                        // Truncate long names
                                        let display_name = if backup.name.len() > 25 {
                                            format!("{}...", &backup.name[..22])
                                        } else {
                                            backup.name.clone()
                                        };

                                        if ui.selectable_label(is_selected, RichText::new(&display_name).color(text_color).size(12.0)).clicked() {
                                            self.backup_selected_idx = Some(i);
                                        }

                                        ui.label(RichText::new(backup.modified.format("%Y-%m-%d %H:%M").to_string()).color(text_color).size(12.0));
                                        ui.label(RichText::new(backup.worlds_count.to_string()).color(text_color).size(12.0));
                                        ui.label(RichText::new(backup.characters_count.to_string()).color(text_color).size(12.0));
                                        ui.label(RichText::new(backup.compressed_size_display()).color(text_color).size(12.0));
                                        ui.label(RichText::new(backup.uncompressed_size_display()).color(text_color).size(12.0));
                                        ui.label(RichText::new(format!("{:.0}%", backup.compression_ratio())).color(text_color).size(12.0));
                                        ui.end_row();
                                    }
                                });
                        });

                    ui.add_space(12.0);

                    // Action buttons
                    ui.horizontal(|ui| {
                        let has_selection = self.backup_selected_idx.is_some();

                        // Restore button
                        if ui.add_enabled(has_selection && !is_busy, egui::Button::new("Restore")).clicked() {
                            self.backup_confirm_restore = true;
                        }

                        // Delete button
                        if ui.add_enabled(has_selection && !is_busy, egui::Button::new("Delete")).clicked() {
                            self.backup_confirm_delete = true;
                        }
                    });
                }
            });

        // Confirmation dialogs
        self.render_backup_confirm_dialogs(ui, &theme, &game_dir);

        // Progress section
        if is_busy || self.backup_progress.phase == BackupPhase::Complete || self.backup_progress.phase == BackupPhase::Failed {
            ui.add_space(12.0);
            self.render_backup_progress(ui, &theme);
        }

        // Error display
        if let Some(ref err) = self.backup_error {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
        }
    }

    /// Render backup confirmation dialogs
    fn render_backup_confirm_dialogs(&mut self, ui: &mut egui::Ui, theme: &Theme, game_dir: &Path) {
        // Delete confirmation
        if self.backup_confirm_delete {
            egui::Window::new("Confirm Delete")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    if let Some(idx) = self.backup_selected_idx {
                        if let Some(backup) = self.backup_list.get(idx) {
                            ui.label(format!("Delete backup \"{}\"?", backup.name));
                            ui.add_space(8.0);
                            ui.label(RichText::new("This cannot be undone.").color(theme.warning));
                            ui.add_space(12.0);

                            ui.horizontal(|ui| {
                                if ui.button("Cancel").clicked() {
                                    self.backup_confirm_delete = false;
                                }
                                if ui.button("Delete").clicked() {
                                    self.delete_selected_backup(game_dir);
                                    self.backup_confirm_delete = false;
                                }
                            });
                        }
                    } else {
                        self.backup_confirm_delete = false;
                    }
                });
        }

        // Restore confirmation
        if self.backup_confirm_restore {
            egui::Window::new("Confirm Restore")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    if let Some(idx) = self.backup_selected_idx {
                        if let Some(backup) = self.backup_list.get(idx) {
                            ui.label(format!("Restore backup \"{}\"?", backup.name));
                            ui.add_space(8.0);

                            if !self.config.backups.skip_backup_before_restore {
                                ui.label(RichText::new("Your current saves will be backed up first.").color(theme.text_muted));
                            } else {
                                ui.label(RichText::new("Warning: Current saves will be replaced!").color(theme.warning));
                            }
                            ui.add_space(12.0);

                            ui.horizontal(|ui| {
                                if ui.button("Cancel").clicked() {
                                    self.backup_confirm_restore = false;
                                }
                                if ui.button("Restore").clicked() {
                                    self.restore_selected_backup(game_dir);
                                    self.backup_confirm_restore = false;
                                }
                            });
                        }
                    } else {
                        self.backup_confirm_restore = false;
                    }
                });
        }
    }

    /// Render backup progress
    fn render_backup_progress(&self, ui: &mut egui::Ui, theme: &Theme) {
        let progress = &self.backup_progress;

        egui::Frame::none()
            .fill(theme.bg_light.gamma_multiply(0.5))
            .rounding(6.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
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

                ui.label(RichText::new(phase_text).color(phase_color).size(13.0).strong());
                ui.add_space(8.0);

                // Progress bar for compress/extract phases
                match progress.phase {
                    BackupPhase::Compressing | BackupPhase::Extracting => {
                        let fraction = progress.fraction();
                        ui.add(egui::ProgressBar::new(fraction).show_percentage());

                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "{} / {} files",
                                progress.files_processed,
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
                    BackupPhase::Scanning | BackupPhase::Cleaning => {
                        ui.add(egui::ProgressBar::new(0.0).animate(true));
                    }
                    _ => {}
                }
            });
    }

    /// Check if a backup operation is in progress
    fn is_backup_busy(&self) -> bool {
        self.backup_task.is_some() || self.backup_list_loading
    }

    /// Validate a backup name
    fn validate_backup_name(&self, name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        if name.len() > 100 {
            return Err("Name too long (max 100 chars)".to_string());
        }
        for c in name.chars() {
            if !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ' {
                return Err(format!("Invalid character: '{}'", c));
            }
        }
        // Check if already exists
        if self.backup_list.iter().any(|b| b.name == name) {
            return Err("A backup with this name already exists".to_string());
        }
        Ok(())
    }

    /// Start a manual backup
    fn start_manual_backup(&mut self, game_dir: &Path) {
        let name = self.backup_name_input.trim().to_string();
        if name.is_empty() {
            return;
        }

        // Clear previous state
        self.backup_error = None;
        self.backup_progress = BackupProgress::default();

        let (progress_tx, progress_rx) = watch::channel(BackupProgress::default());
        self.backup_progress_rx = Some(progress_rx);

        let game_dir = game_dir.to_path_buf();
        let compression_level = self.config.backups.compression_level;

        self.status_message = format!("Creating backup: {}", name);
        tracing::info!("Starting manual backup: {}", name);

        self.backup_task = Some(tokio::spawn(async move {
            backup::create_backup(&game_dir, &name, compression_level, progress_tx).await?;
            Ok(())
        }));

        // Clear input on success start
        self.backup_name_input.clear();
    }

    /// Refresh the backup list
    fn refresh_backup_list(&mut self, game_dir: &Path) {
        if self.backup_list_loading || self.backup_list_task.is_some() {
            return;
        }

        self.backup_list_loading = true;
        self.backup_selected_idx = None;
        self.backup_error = None;

        let game_dir = game_dir.to_path_buf();

        self.backup_list_task = Some(tokio::spawn(async move {
            backup::list_backups(&game_dir).await
        }));
    }

    /// Delete the selected backup
    fn delete_selected_backup(&mut self, game_dir: &Path) {
        let Some(idx) = self.backup_selected_idx else { return };
        let Some(backup) = self.backup_list.get(idx) else { return };

        let backup_name = backup.name.clone();
        let game_dir = game_dir.to_path_buf();

        self.backup_error = None;
        self.status_message = format!("Deleting backup: {}", backup_name);
        tracing::info!("Deleting backup: {}", backup_name);

        self.backup_task = Some(tokio::spawn(async move {
            backup::delete_backup(&game_dir, &backup_name).await
        }));

        self.backup_selected_idx = None;
    }

    /// Restore the selected backup
    fn restore_selected_backup(&mut self, game_dir: &Path) {
        let Some(idx) = self.backup_selected_idx else { return };
        let Some(backup) = self.backup_list.get(idx) else { return };

        let backup_name = backup.name.clone();
        let game_dir = game_dir.to_path_buf();
        let backup_first = !self.config.backups.skip_backup_before_restore;
        let compression_level = self.config.backups.compression_level;

        self.backup_error = None;
        self.backup_progress = BackupProgress::default();

        let (progress_tx, progress_rx) = watch::channel(BackupProgress::default());
        self.backup_progress_rx = Some(progress_rx);

        self.status_message = format!("Restoring backup: {}", backup_name);
        tracing::info!("Restoring backup: {}", backup_name);

        self.backup_task = Some(tokio::spawn(async move {
            backup::restore_backup(&game_dir, &backup_name, backup_first, compression_level, progress_tx).await
        }));
    }

    /// Poll the backup task for progress and completion
    fn poll_backup_task(&mut self, ctx: &egui::Context) {
        // Update progress from channel
        if let Some(rx) = &mut self.backup_progress_rx {
            if rx.has_changed().unwrap_or(false) {
                self.backup_progress = rx.borrow_and_update().clone();
                self.status_message = self.backup_progress.phase.description().to_string();
            }
        }

        // Check if backup operation task is complete
        if let Some(task) = &mut self.backup_task {
            if task.is_finished() {
                let task = self.backup_task.take().unwrap();
                self.backup_progress_rx = None;

                match task.now_or_never() {
                    Some(Ok(Ok(()))) => {
                        self.backup_progress.phase = BackupPhase::Complete;
                        self.status_message = "Backup operation complete!".to_string();
                        tracing::info!("Backup operation completed successfully");

                        // Trigger backup list refresh
                        if let Some(ref dir) = self.config.game.directory {
                            self.refresh_backup_list(&PathBuf::from(dir));
                        }
                    }
                    Some(Ok(Err(e))) => {
                        self.backup_progress.phase = BackupPhase::Failed;
                        let msg = e.to_string();
                        tracing::error!("Backup operation failed: {}", msg);
                        self.backup_error = Some(msg.clone());
                        self.status_message = format!("Backup failed: {}", msg);
                    }
                    Some(Err(e)) => {
                        self.backup_progress.phase = BackupPhase::Failed;
                        let msg = format!("Backup task panicked: {}", e);
                        tracing::error!("{}", msg);
                        self.backup_error = Some(msg.clone());
                        self.status_message = msg;
                    }
                    None => {
                        tracing::warn!("Backup task not ready despite is_finished()");
                    }
                }
            } else {
                ctx.request_repaint();
            }
        }

        // Check if backup list loading task is complete
        if let Some(task) = &mut self.backup_list_task {
            if task.is_finished() {
                let task = self.backup_list_task.take().unwrap();
                self.backup_list_loading = false;

                match task.now_or_never() {
                    Some(Ok(Ok(list))) => {
                        self.backup_list = list;
                        tracing::info!("Loaded {} backups", self.backup_list.len());
                    }
                    Some(Ok(Err(e))) => {
                        tracing::error!("Failed to load backup list: {}", e);
                        self.backup_error = Some(format!("Failed to load backups: {}", e));
                    }
                    Some(Err(e)) => {
                        tracing::error!("Backup list task panicked: {}", e);
                    }
                    None => {
                        tracing::warn!("Backup list task not ready despite is_finished()");
                    }
                }
            } else {
                ctx.request_repaint();
            }
        }
    }

    /// Render the soundpacks tab
    fn render_soundpacks_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme.clone();

        ui.label(
            RichText::new("Soundpacks")
                .color(theme.text_primary)
                .size(20.0)
                .strong(),
        );
        ui.add_space(16.0);

        // Check if game directory is set
        let game_dir = match &self.config.game.directory {
            Some(dir) => PathBuf::from(dir),
            None => {
                ui.label(
                    RichText::new("Set a game directory in the Main tab to manage soundpacks.")
                        .color(theme.text_muted),
                );
                return;
            }
        };

        let is_busy = self.soundpack_task.is_some() || self.soundpack_list_loading;

        // Two-column layout using columns
        ui.columns(2, |columns| {
            // Left column: Installed soundpacks
            self.render_installed_soundpacks_panel(&mut columns[0], &theme, &game_dir, is_busy);

            // Right column: Repository soundpacks
            self.render_repository_soundpacks_panel(&mut columns[1], &theme, &game_dir, is_busy);
        });

        ui.add_space(12.0);

        // Details panel
        self.render_soundpack_details_panel(ui, &theme);

        // Progress section
        if is_busy
            || self.soundpack_progress.phase == SoundpackPhase::Complete
            || self.soundpack_progress.phase == SoundpackPhase::Failed
        {
            ui.add_space(12.0);
            self.render_soundpack_progress(ui, &theme);
        }

        // Delete confirmation dialog
        if self.soundpack_confirm_delete {
            self.render_soundpack_delete_dialog(ui, &theme, &game_dir);
        }

        // Browser download dialog
        if self.browser_download_url.is_some() {
            self.render_browser_download_dialog(ui, &theme, &game_dir);
        }

        // Error display
        if let Some(ref err) = self.soundpack_error {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Error: {}", err)).color(theme.error));
        }
    }

    /// Render the installed soundpacks panel
    fn render_installed_soundpacks_panel(
        &mut self,
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
                            self.refresh_soundpack_list(game_dir);
                        }
                    });
                });

                ui.add_space(8.0);

                // Soundpack list
                egui::ScrollArea::vertical()
                    .id_salt("installed_soundpacks")
                    .max_height(200.0)
                    .show(ui, |ui| {
                        if self.soundpack_list.is_empty() && !self.soundpack_list_loading {
                            ui.label(
                                RichText::new("No soundpacks installed")
                                    .color(theme.text_muted)
                                    .italics(),
                            );
                        } else {
                            for (idx, soundpack) in self.soundpack_list.iter().enumerate() {
                                let is_selected = self.soundpack_installed_idx == Some(idx);
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
                                    self.soundpack_installed_idx = Some(idx);
                                    self.soundpack_repo_idx = None;
                                }
                            }
                        }
                    });

                ui.add_space(8.0);

                // Action buttons
                ui.horizontal(|ui| {
                    let has_selection = self.soundpack_installed_idx.is_some();
                    let selected_enabled = self
                        .soundpack_installed_idx
                        .and_then(|i| self.soundpack_list.get(i))
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
                        if let Some(idx) = self.soundpack_installed_idx {
                            if let Some(soundpack) = self.soundpack_list.get(idx) {
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
                                self.refresh_soundpack_list(&game_dir);
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
                        self.soundpack_confirm_delete = true;
                    }
                });
            });
    }

    /// Render the repository soundpacks panel
    fn render_repository_soundpacks_panel(
        &mut self,
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
                        for (idx, repo_soundpack) in self.soundpack_repository.iter().enumerate() {
                            let is_selected = self.soundpack_repo_idx == Some(idx);
                            let is_installed =
                                soundpack::is_soundpack_installed(&self.soundpack_list, &repo_soundpack.name);

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
                                self.soundpack_repo_idx = Some(idx);
                                self.soundpack_installed_idx = None;
                            }
                        }
                    });

                ui.add_space(8.0);

                // Install button
                ui.horizontal(|ui| {
                    let has_selection = self.soundpack_repo_idx.is_some();
                    let selected_installed = self
                        .soundpack_repo_idx
                        .and_then(|i| self.soundpack_repository.get(i))
                        .map(|r| soundpack::is_soundpack_installed(&self.soundpack_list, &r.name))
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
                        if let Some(idx) = self.soundpack_repo_idx {
                            if let Some(repo_soundpack) = self.soundpack_repository.get(idx) {
                                self.install_soundpack(repo_soundpack.clone(), game_dir);
                            }
                        }
                    }
                });
            });
    }

    /// Render the soundpack details panel
    fn render_soundpack_details_panel(&self, ui: &mut egui::Ui, theme: &Theme) {
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
                if let Some(idx) = self.soundpack_installed_idx {
                    if let Some(soundpack) = self.soundpack_list.get(idx) {
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
                                RichText::new(soundpack::format_size(soundpack.size))
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
                } else if let Some(idx) = self.soundpack_repo_idx {
                    if let Some(repo) = self.soundpack_repository.get(idx) {
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
                            ui.label(
                                RichText::new(&repo.url).color(theme.text_secondary),
                            );
                        });
                        if let Some(size) = repo.size {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Size:").color(theme.text_muted));
                                ui.label(
                                    RichText::new(soundpack::format_size(size))
                                        .color(theme.text_primary),
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
    fn render_soundpack_progress(&self, ui: &mut egui::Ui, theme: &Theme) {
        let progress = &self.soundpack_progress;

        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(12.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
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
                                    soundpack::format_size(progress.bytes_downloaded),
                                    soundpack::format_size(progress.total_bytes),
                                    soundpack::format_size(progress.speed)
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
                            ui.add(egui::ProgressBar::new(0.5).text("Extracting...").fill(theme.accent));
                        }
                        if !progress.current_file.is_empty() {
                            ui.label(
                                RichText::new(&progress.current_file)
                                    .color(theme.text_muted)
                                    .small(),
                            );
                        }
                    }
                    SoundpackPhase::Complete => {
                        ui.label(
                            RichText::new("Soundpack installed successfully!")
                                .color(theme.success),
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
    fn render_soundpack_delete_dialog(&mut self, ui: &mut egui::Ui, theme: &Theme, game_dir: &Path) {
        let selected_name = self
            .soundpack_installed_idx
            .and_then(|i| self.soundpack_list.get(i))
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
                        if let Some(idx) = self.soundpack_installed_idx {
                            if let Some(soundpack) = self.soundpack_list.get(idx) {
                                let path = soundpack.path.clone();
                                let game_dir = game_dir.to_path_buf();

                                let task = tokio::spawn(async move {
                                    soundpack::delete_soundpack(path).await?;
                                    // Return a dummy InstalledSoundpack to satisfy the type
                                    Err(SoundpackError::Cancelled) // Will be handled specially
                                });

                                self.soundpack_task = Some(task);
                                self.soundpack_installed_idx = None;

                                // Schedule list refresh
                                self.refresh_soundpack_list(&game_dir);
                            }
                        }
                        self.soundpack_confirm_delete = false;
                    }

                    if ui.button("Cancel").clicked() {
                        self.soundpack_confirm_delete = false;
                    }
                });
            });
    }

    /// Render browser download dialog
    fn render_browser_download_dialog(&mut self, ui: &mut egui::Ui, theme: &Theme, game_dir: &Path) {
        let url = self.browser_download_url.clone().unwrap_or_default();

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
                            self.install_soundpack_from_file(path, game_dir);
                            self.browser_download_url = None;
                            self.browser_download_soundpack = None;
                        }
                    }

                    if ui.button("Cancel").clicked() {
                        self.browser_download_url = None;
                        self.browser_download_soundpack = None;
                    }
                });
            });
    }

    /// Refresh the installed soundpack list
    fn refresh_soundpack_list(&mut self, game_dir: &Path) {
        if self.soundpack_list_loading {
            return;
        }

        self.soundpack_list_loading = true;
        self.soundpack_error = None;

        let game_dir = game_dir.to_path_buf();
        let task = tokio::spawn(async move { soundpack::list_installed_soundpacks(&game_dir).await });

        self.soundpack_list_task = Some(task);
    }

    /// Install a soundpack from the repository
    fn install_soundpack(&mut self, repo_soundpack: RepoSoundpack, game_dir: &Path) {
        // Check if it's a browser download
        if repo_soundpack.download_type == "browser_download" {
            self.browser_download_url = Some(repo_soundpack.url.clone());
            self.browser_download_soundpack = Some(repo_soundpack);
            return;
        }

        self.soundpack_error = None;
        self.soundpack_progress = SoundpackProgress::default();

        let (progress_tx, progress_rx) = watch::channel(SoundpackProgress::default());
        self.soundpack_progress_rx = Some(progress_rx);

        let client = reqwest::Client::new();
        let game_dir = game_dir.to_path_buf();

        let task = tokio::spawn(async move {
            soundpack::install_soundpack(client, repo_soundpack, game_dir, progress_tx).await
        });

        self.soundpack_task = Some(task);
    }

    /// Install a soundpack from a local file
    fn install_soundpack_from_file(&mut self, archive_path: PathBuf, game_dir: &Path) {
        self.soundpack_error = None;
        self.soundpack_progress = SoundpackProgress::default();

        let (progress_tx, progress_rx) = watch::channel(SoundpackProgress::default());
        self.soundpack_progress_rx = Some(progress_rx);

        let game_dir = game_dir.to_path_buf();

        let task = tokio::spawn(async move {
            soundpack::install_from_file(archive_path, game_dir, progress_tx).await
        });

        self.soundpack_task = Some(task);
    }

    /// Poll soundpack tasks for completion
    fn poll_soundpack_tasks(&mut self, ctx: &egui::Context) {
        // Update progress from receiver
        if let Some(ref mut rx) = self.soundpack_progress_rx {
            if rx.has_changed().unwrap_or(false) {
                self.soundpack_progress = rx.borrow_and_update().clone();
                ctx.request_repaint();
            }
        }

        // Check main soundpack task
        if let Some(ref task) = self.soundpack_task {
            if task.is_finished() {
                let task = self.soundpack_task.take().unwrap();
                self.soundpack_progress_rx = None;

                match task.now_or_never() {
                    Some(Ok(Ok(installed))) => {
                        tracing::info!("Soundpack installed: {}", installed.name);
                        self.soundpack_progress.phase = SoundpackPhase::Complete;

                        // Refresh the list
                        if let Some(dir) = &self.config.game.directory {
                            self.refresh_soundpack_list(&PathBuf::from(dir));
                        }
                    }
                    Some(Ok(Err(SoundpackError::Cancelled))) => {
                        // This is the delete case - just refresh
                        self.soundpack_progress = SoundpackProgress::default();
                    }
                    Some(Ok(Err(e))) => {
                        tracing::error!("Soundpack operation failed: {}", e);
                        self.soundpack_error = Some(e.to_string());
                        self.soundpack_progress.phase = SoundpackPhase::Failed;
                        self.soundpack_progress.error = Some(e.to_string());
                    }
                    Some(Err(e)) => {
                        tracing::error!("Soundpack task panicked: {}", e);
                        self.soundpack_error = Some("Task panicked".to_string());
                        self.soundpack_progress.phase = SoundpackPhase::Failed;
                    }
                    None => {
                        tracing::warn!("Soundpack task not ready despite is_finished()");
                    }
                }
            } else {
                ctx.request_repaint();
            }
        }

        // Check list loading task
        if let Some(ref task) = self.soundpack_list_task {
            if task.is_finished() {
                let task = self.soundpack_list_task.take().unwrap();
                self.soundpack_list_loading = false;

                match task.now_or_never() {
                    Some(Ok(Ok(list))) => {
                        self.soundpack_list = list;
                        // Preserve selection if still valid
                        if let Some(idx) = self.soundpack_installed_idx {
                            if idx >= self.soundpack_list.len() {
                                self.soundpack_installed_idx = None;
                            }
                        }
                    }
                    Some(Ok(Err(e))) => {
                        tracing::error!("Failed to load soundpack list: {}", e);
                        self.soundpack_error = Some(e.to_string());
                    }
                    Some(Err(e)) => {
                        tracing::error!("Soundpack list task panicked: {}", e);
                        self.soundpack_error = Some("Task panicked".to_string());
                    }
                    None => {
                        tracing::warn!("Soundpack list task not ready despite is_finished()");
                    }
                }
            } else {
                ctx.request_repaint();
            }
        }
    }

    /// Render the settings tab
    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        let theme = self.current_theme.clone();

        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .show(ui, |ui| {
        // Use full available width
        let available_width = ui.available_width();

        ui.label(RichText::new("Settings").color(theme.text_primary).size(20.0).strong());
        ui.add_space(16.0);

        // Appearance section
        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_width(available_width - 32.0); // Account for frame margins
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
                ui.set_width(available_width - 32.0);
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
                ui.set_width(available_width - 32.0);
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

        // Backups section
        egui::Frame::none()
            .fill(theme.bg_medium)
            .rounding(8.0)
            .inner_margin(16.0)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(ui, |ui| {
                ui.set_width(available_width - 32.0);
                ui.label(RichText::new("Backups").color(theme.accent).size(13.0).strong());
                ui.add_space(12.0);

                // Auto-backup toggles
                if ui.checkbox(
                    &mut self.config.backups.backup_before_update,
                    "Backup saves before updating game"
                ).changed() {
                    self.save_config();
                }
                ui.label(RichText::new("  Creates an automatic backup before each update")
                    .color(theme.text_muted).size(11.0));

                ui.add_space(8.0);

                if ui.checkbox(
                    &mut self.config.backups.backup_on_launch,
                    "Backup saves before launching game"
                ).changed() {
                    self.save_config();
                }

                ui.add_space(8.0);

                if ui.checkbox(
                    &mut self.config.backups.skip_backup_before_restore,
                    "Skip backup when restoring"
                ).changed() {
                    self.save_config();
                }
                ui.label(RichText::new("  Not recommended - restoring will overwrite current saves")
                    .color(theme.warning).size(11.0));

                ui.add_space(12.0);

                // Max backups
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Max auto-backups to keep:").color(theme.text_muted));
                    if ui.add(egui::DragValue::new(&mut self.config.backups.max_count)
                        .range(1..=100)
                        .speed(1.0)).changed() {
                        self.save_config();
                    }
                });

                ui.add_space(8.0);

                // Compression level
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Compression level:").color(theme.text_muted));
                    if ui.add(egui::Slider::new(&mut self.config.backups.compression_level, 0..=9)
                        .text("")).changed() {
                        self.save_config();
                    }
                });
                ui.label(RichText::new("  0 = no compression (fast), 9 = best compression (slow)")
                    .color(theme.text_muted).size(11.0));
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
        }); // ScrollArea
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

    /// Render the About dialog
    fn render_about_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_about_dialog {
            return;
        }

        let theme = &self.current_theme;

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
                        self.show_about_dialog = false;
                    }

                    ui.add_space(8.0);
                });
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
