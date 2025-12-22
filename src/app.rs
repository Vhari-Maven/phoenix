use anyhow::Result;
use eframe::egui::{self, Color32, RichText, Rounding, Vec2};
use egui_commonmark::CommonMarkCache;
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
use crate::theme::Theme;
use crate::update::{self, UpdatePhase, UpdateProgress};

/// Main application state
pub struct PhoenixApp {
    /// Application configuration
    pub(crate) config: Config,
    /// Version database
    pub(crate) db: Option<Database>,
    /// Detected game information
    pub(crate) game_info: Option<GameInfo>,
    /// Status message for the status bar
    pub(crate) status_message: String,

    // GitHub integration
    /// GitHub API client
    pub(crate) github_client: GitHubClient,
    /// Fetched experimental releases
    pub(crate) experimental_releases: Vec<Release>,
    /// Fetched stable releases
    pub(crate) stable_releases: Vec<Release>,
    /// Index of selected release in current list
    pub(crate) selected_release_idx: Option<usize>,
    /// Async task for fetching releases
    releases_task: Option<JoinHandle<Result<FetchResult<Vec<Release>>>>>,
    /// Which branch is being fetched
    fetching_branch: Option<String>,
    /// Whether releases are currently being fetched
    pub(crate) releases_loading: bool,
    /// Error message from last fetch attempt
    pub(crate) releases_error: Option<String>,

    // Markdown rendering
    /// Cache for markdown rendering
    pub(crate) markdown_cache: CommonMarkCache,

    // Rate limiting
    /// Last known rate limit info from GitHub API
    pub(crate) rate_limit: RateLimitInfo,

    // UI state
    /// Current theme
    pub(crate) current_theme: Theme,
    /// Currently selected tab
    pub(crate) active_tab: Tab,
    /// Whether theme needs to be applied
    pub(crate) theme_dirty: bool,

    // Update state
    /// Async task for update operation
    update_task: Option<JoinHandle<Result<()>>>,
    /// Channel receiver for update progress
    update_progress_rx: Option<watch::Receiver<UpdateProgress>>,
    /// Current update progress
    pub(crate) update_progress: UpdateProgress,
    /// Error message from last update attempt
    pub(crate) update_error: Option<String>,

    // Backup state
    /// List of available backups
    pub(crate) backup_list: Vec<BackupInfo>,
    /// Whether backup list is being loaded
    pub(crate) backup_list_loading: bool,
    /// Index of selected backup in list
    pub(crate) backup_selected_idx: Option<usize>,
    /// Input field for manual backup name
    pub(crate) backup_name_input: String,
    /// Async task for backup operation
    backup_task: Option<JoinHandle<Result<(), backup::BackupError>>>,
    /// Async task for loading backup list
    backup_list_task: Option<JoinHandle<Result<Vec<BackupInfo>, backup::BackupError>>>,
    /// Channel receiver for backup progress
    backup_progress_rx: Option<watch::Receiver<BackupProgress>>,
    /// Current backup progress
    pub(crate) backup_progress: BackupProgress,
    /// Error message from last backup attempt
    pub(crate) backup_error: Option<String>,
    /// Whether to show delete confirmation
    pub(crate) backup_confirm_delete: bool,
    /// Whether to show restore confirmation
    pub(crate) backup_confirm_restore: bool,

    // Soundpack state
    /// List of installed soundpacks
    pub(crate) soundpack_list: Vec<InstalledSoundpack>,
    /// Whether soundpack list is being loaded
    pub(crate) soundpack_list_loading: bool,
    /// Index of selected installed soundpack
    pub(crate) soundpack_installed_idx: Option<usize>,
    /// Index of selected repository soundpack
    pub(crate) soundpack_repo_idx: Option<usize>,
    /// Repository soundpacks
    pub(crate) soundpack_repository: Vec<RepoSoundpack>,
    /// Async task for soundpack install/delete operations
    pub(crate) soundpack_task: Option<JoinHandle<Result<InstalledSoundpack, SoundpackError>>>,
    /// Async task for loading soundpack list
    soundpack_list_task: Option<JoinHandle<Result<Vec<InstalledSoundpack>, SoundpackError>>>,
    /// Channel receiver for soundpack progress
    soundpack_progress_rx: Option<watch::Receiver<SoundpackProgress>>,
    /// Current soundpack progress
    pub(crate) soundpack_progress: SoundpackProgress,
    /// Error message from last soundpack operation
    pub(crate) soundpack_error: Option<String>,
    /// Whether to show delete confirmation
    pub(crate) soundpack_confirm_delete: bool,
    /// Browser download state
    pub(crate) browser_download_url: Option<String>,
    /// Browser download repo soundpack
    pub(crate) browser_download_soundpack: Option<RepoSoundpack>,

    // Dialogs
    /// Whether to show the About dialog
    pub(crate) show_about_dialog: bool,
}

/// Application tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
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
    pub(crate) fn current_releases(&self) -> &Vec<Release> {
        if self.config.game.branch == "stable" {
            &self.stable_releases
        } else {
            &self.experimental_releases
        }
    }

    /// Simple check: is the selected release different from the installed version?
    /// Returns true if they are different (can update/switch), false if same or can't compare
    pub(crate) fn is_selected_release_different(&self) -> bool {
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
    pub(crate) fn has_releases_for_branch(&self, branch: &str) -> bool {
        if branch == "stable" {
            !self.stable_releases.is_empty()
        } else {
            !self.experimental_releases.is_empty()
        }
    }

    /// Start fetching releases for a specific branch
    pub(crate) fn fetch_releases_for_branch(&mut self, branch: &str) {
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
    pub(crate) fn start_update(&mut self) {
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
    pub(crate) fn is_updating(&self) -> bool {
        self.update_task.is_some()
    }

    /// Open directory picker and update game directory
    pub(crate) fn browse_for_directory(&mut self) {
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
    pub(crate) fn save_config(&self) {
        if let Err(e) = self.config.save() {
            tracing::error!("Failed to save config: {}", e);
        }
    }

    /// Launch the game
    pub(crate) fn launch_game(&mut self) {
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
                    Tab::Main => crate::ui::render_main_tab(self, ui),
                    Tab::Backups => crate::ui::render_backups_tab(self, ui),
                    Tab::Soundpacks => crate::ui::render_soundpacks_tab(self, ui),
                    Tab::Settings => crate::ui::render_settings_tab(self, ui),
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

    /// Check if a backup operation is in progress
    pub(crate) fn is_backup_busy(&self) -> bool {
        self.backup_task.is_some() || self.backup_list_loading
    }

    /// Validate a backup name
    pub(crate) fn validate_backup_name(&self, name: &str) -> Result<(), String> {
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
    pub(crate) fn start_manual_backup(&mut self, game_dir: &Path) {
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
    pub(crate) fn refresh_backup_list(&mut self, game_dir: &Path) {
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
    pub(crate) fn delete_selected_backup(&mut self, game_dir: &Path) {
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
    pub(crate) fn restore_selected_backup(&mut self, game_dir: &Path) {
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

    /// Check if a soundpack operation is in progress
    pub(crate) fn is_soundpack_busy(&self) -> bool {
        self.soundpack_task.is_some() || self.soundpack_list_loading
    }

    /// Refresh the installed soundpack list
    pub(crate) fn refresh_soundpack_list(&mut self, game_dir: &Path) {
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
    pub(crate) fn install_soundpack(&mut self, repo_soundpack: RepoSoundpack, game_dir: &Path) {
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
    pub(crate) fn install_soundpack_from_file(&mut self, archive_path: PathBuf, game_dir: &Path) {
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
