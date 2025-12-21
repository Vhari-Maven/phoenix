use anyhow::Result;
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use futures::FutureExt;
use std::path::PathBuf;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::db::Database;
use crate::game::{self, GameInfo};
use crate::github::{FetchResult, GitHubClient, RateLimitInfo, Release};

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
        // Poll async tasks
        self.poll_releases_task(ctx);

        // Top menu bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
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
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status_message);
            });
        });

        // Main content area with tabs
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                let _ = ui.selectable_label(true, "Main");
                let _ = ui.selectable_label(false, "Backups");
                let _ = ui.selectable_label(false, "Soundpacks");
                let _ = ui.selectable_label(false, "Settings");
            });

            ui.separator();

            // Main tab content
            ui.vertical(|ui| {
                ui.heading("Game Directory");

                ui.horizontal(|ui| {
                    ui.label("Directory:");
                    let dir_text = self
                        .config
                        .game
                        .directory
                        .as_deref()
                        .unwrap_or("Not selected");
                    ui.label(dir_text);
                    if ui.button("Browse...").clicked() {
                        self.browse_for_directory();
                    }
                });

                // Show game info if detected
                if let Some(ref info) = self.game_info {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label("Executable:");
                        ui.label(info.executable.file_name().unwrap_or_default().to_string_lossy().to_string());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Version:");
                        let version_text = if info.is_stable() {
                            format!("{} (Stable)", info.version_display())
                        } else {
                            info.version_display().to_string()
                        };
                        ui.label(version_text);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Saves size:");
                        ui.label(game::format_size(info.saves_size));
                    });
                }

                ui.add_space(20.0);

                ui.heading("Update");

                // Track branch changes
                let previous_branch = self.config.game.branch.clone();

                ui.horizontal(|ui| {
                    ui.label("Branch:");
                    egui::ComboBox::from_id_salt("branch_select")
                        .selected_text(&self.config.game.branch)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.config.game.branch,
                                "stable".to_string(),
                                "Stable",
                            );
                            ui.selectable_value(
                                &mut self.config.game.branch,
                                "experimental".to_string(),
                                "Experimental",
                            );
                        });
                });

                // If branch changed, update selection and fetch if needed
                if self.config.game.branch != previous_branch {
                    if !self.has_releases_for_branch(&self.config.game.branch) {
                        // Need to fetch - clear selection for now
                        self.selected_release_idx = None;
                        let branch = self.config.game.branch.clone();
                        self.fetch_releases_for_branch(&branch);
                    } else {
                        // Already have releases for this branch - auto-select latest
                        self.selected_release_idx = Some(0);
                    }
                }

                // Releases section
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Releases:");
                    if self.releases_loading {
                        ui.spinner();
                        ui.label("Loading...");
                    } else {
                        // Show refresh button
                        if ui.button("↻ Refresh").clicked() {
                            let branch = self.config.game.branch.clone();
                            self.fetch_releases_for_branch(&branch);
                        }
                    }
                });

                // Show error if any
                if let Some(ref err) = self.releases_error {
                    ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                }

                // Show rate limit warning if low
                if self.rate_limit.is_low() {
                    let remaining = self.rate_limit.remaining.unwrap_or(0);
                    let reset_mins = self.rate_limit.reset_in_minutes().unwrap_or(0);
                    let warning = format!(
                        "⚠ {} GitHub API requests remaining. Resets in {} min.",
                        remaining, reset_mins
                    );
                    ui.colored_label(egui::Color32::from_rgb(255, 165, 0), warning)
                        .on_hover_text(
                            "GitHub limits unauthenticated API requests to 60/hour.\n\
                             Avoid refreshing frequently to conserve requests."
                        );
                }

                // Release dropdown (if releases loaded for current branch)
                let has_releases = !self.current_releases().is_empty();
                if has_releases {
                    let releases = self.current_releases();
                    let selected_text = self
                        .selected_release_idx
                        .and_then(|i| releases.get(i))
                        .map(|r| r.name.as_str())
                        .unwrap_or("Select a release")
                        .to_string();

                    // Build list of release labels
                    let release_labels: Vec<(usize, String)> = releases
                        .iter()
                        .enumerate()
                        .map(|(i, r)| (i, format!("{} ({})", r.name, &r.published_at[..10])))
                        .collect();

                    let current_selection = self.selected_release_idx;

                    ui.horizontal(|ui| {
                        ui.label("Version:");
                        egui::ComboBox::from_id_salt("release_select")
                            .selected_text(&selected_text)
                            .width(400.0)
                            .show_ui(ui, |ui| {
                                for (i, label) in &release_labels {
                                    if ui
                                        .selectable_label(current_selection == Some(*i), label)
                                        .clicked()
                                    {
                                        self.selected_release_idx = Some(*i);
                                    }
                                }
                            });
                    });

                    // Update status indicator
                    if let Some((is_update, status_text)) = self.check_update_status() {
                        ui.horizontal(|ui| {
                            ui.label("Status:");
                            if is_update {
                                ui.colored_label(egui::Color32::from_rgb(50, 205, 50), format!("[NEW] {}", status_text));
                            } else {
                                ui.label(format!("[OK] {}", status_text));
                            }
                        });
                    }

                    // Changelog area
                    if let Some(idx) = self.selected_release_idx {
                        let releases = self.current_releases();
                        if let Some(release) = releases.get(idx) {
                            ui.add_space(10.0);
                            ui.heading("Changelog");

                            let body = release.body.clone();
                            egui::ScrollArea::vertical()
                                .max_height(150.0)
                                .show(ui, |ui| {
                                    if let Some(ref text) = body {
                                        // Convert raw URLs to markdown links for clickability
                                        let processed = convert_urls_to_links(text);
                                        CommonMarkViewer::new()
                                            .show(ui, &mut self.markdown_cache, &processed);
                                    } else {
                                        ui.label("No changelog available");
                                    }
                                });
                        }
                    }
                } else if !self.releases_loading {
                    ui.label("No releases loaded - click Refresh");
                }

                ui.add_space(20.0);

                ui.horizontal(|ui| {
                    let can_launch = self.game_info.is_some();
                    if ui.add_enabled(can_launch, egui::Button::new("Launch Game")).clicked() {
                        self.launch_game();
                    }
                    if ui.button("Update").clicked() {
                        // TODO: Check for updates (Spiral 4)
                    }
                });
            });
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
