use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{self, RichText};

use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};
use crate::github::GitHubClient;
use crate::state::{
    BackupState, ReleasesState, SoundpackState, StateEvent, Tab, UiState, UpdateParams, UpdateState,
};

/// Main application state
pub struct PhoenixApp {
    // Core state (stays at app level)
    /// Application configuration
    pub(crate) config: Config,
    /// Version database
    pub(crate) db: Option<Database>,
    /// Detected game information
    pub(crate) game_info: Option<GameInfo>,
    /// Status message for the status bar
    pub(crate) status_message: String,
    /// GitHub API client
    pub(crate) github_client: GitHubClient,

    // Grouped state
    /// UI state (theme, tabs, dialogs)
    pub(crate) ui: UiState,
    /// Releases state
    pub(crate) releases: ReleasesState,
    /// Update state
    pub(crate) update: UpdateState,
    /// Backup state
    pub(crate) backup: BackupState,
    /// Soundpack state
    pub(crate) soundpack: SoundpackState,
}

impl PhoenixApp {
    /// Create a new application instance
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let startup_start = Instant::now();

        // Load configuration
        let phase_start = Instant::now();
        let config = Config::load().unwrap_or_default();
        tracing::info!(
            "Config loaded in {:.1}ms",
            phase_start.elapsed().as_secs_f32() * 1000.0
        );

        // Open database
        let phase_start = Instant::now();
        let db = match Database::open() {
            Ok(db) => Some(db),
            Err(e) => {
                tracing::error!("Failed to open database: {}", e);
                None
            }
        };
        tracing::info!(
            "Database opened in {:.1}ms",
            phase_start.elapsed().as_secs_f32() * 1000.0
        );

        // Try to detect game if directory is configured
        let phase_start = Instant::now();
        let game_info = config.game.directory.as_ref().and_then(|dir| {
            game::detect_game_with_db(&PathBuf::from(dir), db.as_ref())
                .ok()
                .flatten()
        });
        if config.game.directory.is_some() {
            tracing::info!(
                "Game detection in {:.1}ms",
                phase_start.elapsed().as_secs_f32() * 1000.0
            );
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
            ui: UiState::new(current_theme),
            releases: ReleasesState::default(),
            update: UpdateState::default(),
            backup: BackupState::default(),
            soundpack: SoundpackState::default(),
        };

        // Auto-fetch releases for current branch on startup
        if let Some(event) = app.releases.fetch_for_branch(&branch, &app.github_client) {
            app.handle_event(event);
        }

        tracing::info!(
            "Startup complete in {:.1}ms",
            startup_start.elapsed().as_secs_f32() * 1000.0
        );

        app
    }

    /// Handle a single state event
    fn handle_event(&mut self, event: StateEvent) {
        match event {
            StateEvent::StatusMessage(msg) => {
                self.status_message = msg;
            }
            StateEvent::RefreshGameInfo => {
                self.refresh_game_info();
            }
            StateEvent::LogError(msg) => {
                tracing::error!("{}", msg);
            }
            StateEvent::LogInfo(msg) => {
                tracing::info!("{}", msg);
            }
        }
    }

    /// Handle multiple state events
    fn handle_events(&mut self, events: Vec<StateEvent>) {
        for event in events {
            self.handle_event(event);
        }
    }

    /// Get releases for the current branch
    pub(crate) fn current_releases(&self) -> &Vec<crate::github::Release> {
        self.releases.for_branch(&self.config.game.branch)
    }

    /// Simple check: is the selected release different from the installed version?
    pub(crate) fn is_selected_release_different(&self) -> bool {
        self.releases
            .is_selected_different(&self.config.game.branch, self.game_info.as_ref())
    }

    /// Check if we have releases for the given branch
    pub(crate) fn has_releases_for_branch(&self, branch: &str) -> bool {
        self.releases.has_for_branch(branch)
    }

    /// Start fetching releases for a specific branch
    pub(crate) fn fetch_releases_for_branch(&mut self, branch: &str) {
        if let Some(event) = self.releases.fetch_for_branch(branch, &self.github_client) {
            self.handle_event(event);
        }
    }

    /// Start the update process for the selected release
    pub(crate) fn start_update(&mut self) {
        // Don't start if already updating
        if self.update.is_updating() {
            return;
        }

        // Get the selected release and its Windows asset
        let releases = self.current_releases();
        let release = match self.releases.selected_idx.and_then(|i| releases.get(i)) {
            Some(r) => r.clone(),
            None => {
                self.update.error = Some("No release selected".to_string());
                return;
            }
        };

        let asset = match GitHubClient::find_windows_asset(&release) {
            Some(a) => a.clone(),
            None => {
                self.update.error =
                    Some("No Windows build available for this release".to_string());
                return;
            }
        };

        // Get the game directory
        let game_dir = match &self.config.game.directory {
            Some(dir) => PathBuf::from(dir),
            None => {
                self.update.error = Some("No game directory configured".to_string());
                return;
            }
        };

        let params = UpdateParams {
            release,
            asset,
            game_dir,
            client: self.github_client.clone(),
            prevent_save_move: self.config.updates.prevent_save_move,
            remove_previous_version: self.config.updates.remove_previous_version,
            backup_before_update: self.config.backups.backup_before_update,
            compression_level: self.config.backups.compression_level,
            max_backups: self.config.backups.max_count,
        };

        if let Some(event) = self.update.start(params) {
            self.handle_event(event);
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
        self.update.is_updating()
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
                        info.executable
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
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
            self.status_message =
                "No game detected - select a valid game directory".to_string();
        }
    }

    // Backup delegation methods

    /// Check if a backup operation is in progress
    pub(crate) fn is_backup_busy(&self) -> bool {
        self.backup.is_busy()
    }

    /// Validate a backup name
    pub(crate) fn validate_backup_name(&self, name: &str) -> Result<(), String> {
        self.backup.validate_name(name)
    }

    /// Start a manual backup
    pub(crate) fn start_manual_backup(&mut self, game_dir: &std::path::Path) {
        if let Some(event) = self.backup.start_manual_backup(
            game_dir,
            self.config.backups.compression_level,
        ) {
            self.handle_event(event);
        }
    }

    /// Refresh the backup list
    pub(crate) fn refresh_backup_list(&mut self, game_dir: &std::path::Path) {
        self.backup.refresh_list(game_dir);
    }

    /// Delete the selected backup
    pub(crate) fn delete_selected_backup(&mut self, game_dir: &std::path::Path) {
        if let Some(event) = self.backup.delete_selected(game_dir) {
            self.handle_event(event);
        }
    }

    /// Restore the selected backup
    pub(crate) fn restore_selected_backup(&mut self, game_dir: &std::path::Path) {
        if let Some(event) = self.backup.restore_selected(
            game_dir,
            self.config.backups.skip_backup_before_restore,
            self.config.backups.compression_level,
        ) {
            self.handle_event(event);
        }
    }

    // Soundpack delegation methods

    /// Check if a soundpack operation is in progress
    pub(crate) fn is_soundpack_busy(&self) -> bool {
        self.soundpack.is_busy()
    }

    /// Refresh the installed soundpack list
    pub(crate) fn refresh_soundpack_list(&mut self, game_dir: &std::path::Path) {
        self.soundpack.refresh_list(game_dir);
    }

    /// Install a soundpack from the repository
    pub(crate) fn install_soundpack(
        &mut self,
        repo_soundpack: crate::soundpack::RepoSoundpack,
        game_dir: &std::path::Path,
    ) {
        self.soundpack.install(repo_soundpack, game_dir);
    }

    /// Install a soundpack from a local file
    pub(crate) fn install_soundpack_from_file(
        &mut self,
        archive_path: PathBuf,
        game_dir: &std::path::Path,
    ) {
        self.soundpack.install_from_file(archive_path, game_dir);
    }
}

impl eframe::App for PhoenixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Apply theme if needed
        if self.ui.theme_dirty {
            self.ui.current_theme.apply(ctx);
            self.ui.theme_dirty = false;
        }

        // Poll async tasks and handle events
        let game_dir = self.config.game.directory.as_ref().map(PathBuf::from);
        let game_dir_ref = game_dir.as_deref();

        let release_events = self.releases.poll(ctx, &self.config.game.branch);
        self.handle_events(release_events);

        let update_events = self.update.poll(ctx);
        self.handle_events(update_events);

        let backup_events = self.backup.poll(ctx, game_dir_ref);
        self.handle_events(backup_events);

        let soundpack_events = self.soundpack.poll(ctx, game_dir_ref);
        self.handle_events(soundpack_events);

        let theme = &self.ui.current_theme;

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar")
            .frame(egui::Frame::new().fill(theme.bg_darkest).inner_margin(4.0))
            .show(ctx, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Exit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("About").clicked() {
                            self.ui.show_about_dialog = true;
                            ui.close();
                        }
                    });
                });
            });

        // Status bar at bottom
        egui::TopBottomPanel::bottom("status_bar")
            .frame(egui::Frame::new().fill(theme.bg_darkest).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status_message).color(theme.text_muted));
                });
            });

        // Main content area
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme.bg_dark).inner_margin(16.0))
            .show(ctx, |ui| {
                // Tab bar
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    crate::ui::render_tab(self, ui, Tab::Main, "Main");
                    crate::ui::render_tab(self, ui, Tab::Backups, "Backups");
                    crate::ui::render_tab(self, ui, Tab::Soundpacks, "Soundpacks");
                    crate::ui::render_tab(self, ui, Tab::Settings, "Settings");
                });

                ui.add_space(16.0);

                // Tab content
                match self.ui.active_tab {
                    Tab::Main => crate::ui::render_main_tab(self, ui),
                    Tab::Backups => crate::ui::render_backups_tab(self, ui),
                    Tab::Soundpacks => crate::ui::render_soundpacks_tab(self, ui),
                    Tab::Settings => crate::ui::render_settings_tab(self, ui),
                }
            });

        // About dialog
        crate::ui::render_about_dialog(self, ctx);
    }
}
