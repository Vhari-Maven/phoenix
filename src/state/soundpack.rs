//! Soundpack-related application state

use std::path::{Path, PathBuf};

use eframe::egui;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::app_data::RepoSoundpack;
use crate::soundpack::{
    self, InstalledSoundpack, SoundpackError, SoundpackPhase, SoundpackProgress,
};
use crate::state::StateEvent;
use crate::task::{poll_task, PollResult};

/// Soundpack-related state
pub struct SoundpackState {
    /// List of installed soundpacks
    pub list: Vec<InstalledSoundpack>,
    /// Whether soundpack list is being loaded
    pub list_loading: bool,
    /// Index of selected installed soundpack
    pub installed_idx: Option<usize>,
    /// Index of selected repository soundpack
    pub repo_idx: Option<usize>,
    /// Repository soundpacks
    pub repository: Vec<RepoSoundpack>,
    /// Async task for soundpack install/delete operations
    pub task: Option<JoinHandle<Result<InstalledSoundpack, SoundpackError>>>,
    /// Async task for loading soundpack list
    list_task: Option<JoinHandle<Result<Vec<InstalledSoundpack>, SoundpackError>>>,
    /// Channel receiver for soundpack progress
    progress_rx: Option<watch::Receiver<SoundpackProgress>>,
    /// Current soundpack progress
    pub progress: SoundpackProgress,
    /// Error message from last soundpack operation
    pub error: Option<String>,
    /// Whether to show delete confirmation
    pub confirm_delete: bool,
    /// Browser download state
    pub browser_download_url: Option<String>,
    /// Browser download repo soundpack
    pub browser_download_soundpack: Option<RepoSoundpack>,
}

impl Default for SoundpackState {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            list_loading: false,
            installed_idx: None,
            repo_idx: None,
            repository: soundpack::load_repository(),
            task: None,
            list_task: None,
            progress_rx: None,
            progress: SoundpackProgress::default(),
            error: None,
            confirm_delete: false,
            browser_download_url: None,
            browser_download_soundpack: None,
        }
    }
}

impl SoundpackState {
    /// Check if a soundpack operation is in progress
    pub fn is_busy(&self) -> bool {
        self.task.is_some() || self.list_loading
    }

    /// Refresh the installed soundpack list
    pub fn refresh_list(&mut self, game_dir: &Path) {
        if self.list_loading {
            return;
        }

        self.list_loading = true;
        self.error = None;

        let game_dir = game_dir.to_path_buf();
        let task = tokio::spawn(async move { soundpack::list_installed_soundpacks(&game_dir).await });

        self.list_task = Some(task);
    }

    /// Install a soundpack from the repository
    pub fn install(&mut self, repo_soundpack: RepoSoundpack, game_dir: &Path) {
        // Check if it's a browser download
        if repo_soundpack.download_type == "browser_download" {
            self.browser_download_url = Some(repo_soundpack.url.clone());
            self.browser_download_soundpack = Some(repo_soundpack);
            return;
        }

        self.error = None;
        self.progress = SoundpackProgress::default();

        let (progress_tx, progress_rx) = watch::channel(SoundpackProgress::default());
        self.progress_rx = Some(progress_rx);

        let client = reqwest::Client::new();
        let game_dir = game_dir.to_path_buf();

        let task = tokio::spawn(async move {
            soundpack::install_soundpack(client, repo_soundpack, game_dir, progress_tx).await
        });

        self.task = Some(task);
    }

    /// Install a soundpack from a local file
    pub fn install_from_file(&mut self, archive_path: PathBuf, game_dir: &Path) {
        self.error = None;
        self.progress = SoundpackProgress::default();

        let (progress_tx, progress_rx) = watch::channel(SoundpackProgress::default());
        self.progress_rx = Some(progress_rx);

        let game_dir = game_dir.to_path_buf();

        let task = tokio::spawn(async move {
            soundpack::install_from_file(archive_path, game_dir, progress_tx).await
        });

        self.task = Some(task);
    }

    /// Poll soundpack tasks for completion
    pub fn poll(&mut self, ctx: &egui::Context, game_dir: Option<&Path>) -> Vec<StateEvent> {
        let mut events = Vec::new();

        // Update progress from receiver
        if let Some(ref mut rx) = self.progress_rx {
            if rx.has_changed().unwrap_or(false) {
                self.progress = rx.borrow_and_update().clone();
                ctx.request_repaint();
            }
        }

        // Check main soundpack task
        match poll_task(&mut self.task) {
            PollResult::Complete(Ok(Ok(installed))) => {
                self.progress_rx = None;
                events.push(StateEvent::LogInfo(format!("Soundpack installed: {}", installed.name)));
                self.progress.phase = SoundpackPhase::Complete;

                // Refresh the list
                if let Some(dir) = game_dir {
                    self.refresh_list(dir);
                }
            }
            PollResult::Complete(Ok(Err(SoundpackError::Cancelled))) => {
                self.progress_rx = None;
                // This is the delete case - show complete and refresh the list
                self.progress.phase = SoundpackPhase::Complete;
                if let Some(dir) = game_dir {
                    self.refresh_list(dir);
                }
            }
            PollResult::Complete(Ok(Err(e))) => {
                self.progress_rx = None;
                events.push(StateEvent::LogError(format!("Soundpack operation failed: {}", e)));
                self.error = Some(e.to_string());
                self.progress.phase = SoundpackPhase::Failed;
                self.progress.error = Some(e.to_string());
            }
            PollResult::Complete(Err(e)) => {
                self.progress_rx = None;
                events.push(StateEvent::LogError(format!("Soundpack task panicked: {}", e)));
                self.error = Some("Task panicked".to_string());
                self.progress.phase = SoundpackPhase::Failed;
            }
            PollResult::Pending => ctx.request_repaint(),
            PollResult::NoTask => {}
        }

        // Check list loading task
        match poll_task(&mut self.list_task) {
            PollResult::Complete(Ok(Ok(list))) => {
                self.list_loading = false;
                self.list = list;
                // Preserve selection if still valid
                if let Some(idx) = self.installed_idx {
                    if idx >= self.list.len() {
                        self.installed_idx = None;
                    }
                }
            }
            PollResult::Complete(Ok(Err(e))) => {
                self.list_loading = false;
                events.push(StateEvent::LogError(format!("Failed to load soundpack list: {}", e)));
                self.error = Some(e.to_string());
            }
            PollResult::Complete(Err(e)) => {
                self.list_loading = false;
                events.push(StateEvent::LogError(format!("Soundpack list task panicked: {}", e)));
                self.error = Some("Task panicked".to_string());
            }
            PollResult::Pending => ctx.request_repaint(),
            PollResult::NoTask => {}
        }

        events
    }
}
