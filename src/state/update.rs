//! Update-related application state

use std::path::PathBuf;

use anyhow::Result;
use eframe::egui;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::backup::{self, AutoBackupType, BackupProgress};
use crate::github::{GitHubClient, Release, ReleaseAsset};
use crate::state::StateEvent;
use crate::task::{poll_task, PollResult};
use crate::update::{self, UpdatePhase, UpdateProgress};

/// Configuration for starting an update
pub struct UpdateParams {
    pub release: Release,
    pub asset: ReleaseAsset,
    pub game_dir: PathBuf,
    pub client: GitHubClient,
    pub prevent_save_move: bool,
    pub remove_previous_version: bool,
    pub backup_before_update: bool,
    pub compression_level: u8,
    pub max_backups: u32,
}

/// Update-related state
pub struct UpdateState {
    /// Async task for update operation
    task: Option<JoinHandle<Result<()>>>,
    /// Channel receiver for update progress
    progress_rx: Option<watch::Receiver<UpdateProgress>>,
    /// Current update progress
    pub progress: UpdateProgress,
    /// Error message from last update attempt
    pub error: Option<String>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            task: None,
            progress_rx: None,
            progress: UpdateProgress::default(),
            error: None,
        }
    }
}

impl UpdateState {
    /// Check if an update is currently in progress
    pub fn is_updating(&self) -> bool {
        self.task.is_some()
    }

    /// Start the update process
    /// Returns a status message event if started successfully
    pub fn start(&mut self, params: UpdateParams) -> Option<StateEvent> {
        // Don't start if already updating
        if self.task.is_some() {
            return None;
        }

        // Get download directory
        let download_dir = match update::download_dir() {
            Ok(dir) => dir,
            Err(e) => {
                self.error = Some(format!("Failed to get download directory: {}", e));
                return None;
            }
        };

        let zip_path = download_dir.join(&params.asset.name);
        let download_url = params.asset.browser_download_url.clone();
        let release_name = params.release.name.clone();

        // Create progress channel
        let (progress_tx, progress_rx) = watch::channel(UpdateProgress::default());
        self.progress_rx = Some(progress_rx);
        self.error = None;
        self.progress = UpdateProgress {
            phase: UpdatePhase::Downloading,
            total_bytes: params.asset.size,
            ..Default::default()
        };

        let client = params.client;
        let prevent_save_move = params.prevent_save_move;
        let remove_previous_version = params.remove_previous_version;
        let backup_before_update = params.backup_before_update;
        let compression_level = params.compression_level;
        let max_backups = params.max_backups;
        let version_tag = params.release.tag_name.clone();
        let game_dir = params.game_dir;

        tracing::info!("Starting update: {} from {}", params.asset.name, download_url);

        // Spawn the update task
        self.task = Some(tokio::spawn(async move {
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
                )
                .await
                {
                    Ok(Some(info)) => {
                        tracing::info!("Pre-update backup created: {}", info.name);
                    }
                    Ok(None) => {
                        tracing::info!("No saves to backup before update");
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to create pre-update backup: {} (continuing with update)",
                            e
                        );
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
            )
            .await?;

            tracing::info!("Download complete: {} bytes", result.bytes);

            // Phase 2: Install (backup, extract, restore with smart migration)
            update::install_update(
                result.file_path,
                game_dir,
                progress_tx,
                prevent_save_move,
                remove_previous_version,
            )
            .await?;

            Ok(())
        }));

        Some(StateEvent::StatusMessage(format!("Downloading {}...", release_name)))
    }

    /// Poll the update task for progress and completion
    pub fn poll(&mut self, ctx: &egui::Context) -> Vec<StateEvent> {
        let mut events = Vec::new();

        // Update progress from channel
        if let Some(rx) = &mut self.progress_rx {
            if rx.has_changed().unwrap_or(false) {
                self.progress = rx.borrow_and_update().clone();
                events.push(StateEvent::StatusMessage(
                    self.progress.phase.description().to_string(),
                ));
            }
        }

        // Check if task is complete
        match poll_task(&mut self.task) {
            PollResult::Complete(Ok(Ok(()))) => {
                self.progress_rx = None;
                self.progress.phase = UpdatePhase::Complete;
                events.push(StateEvent::StatusMessage(
                    "Update complete! Refreshing game info...".to_string(),
                ));
                events.push(StateEvent::LogInfo("Update completed successfully".to_string()));
                events.push(StateEvent::RefreshGameInfo);
            }
            PollResult::Complete(Ok(Err(e))) => {
                self.progress_rx = None;
                self.progress.phase = UpdatePhase::Failed;
                let msg = e.to_string();
                events.push(StateEvent::LogError(format!("Update failed: {}", msg)));
                self.error = Some(msg.clone());
                events.push(StateEvent::StatusMessage(format!("Update failed: {}", msg)));
            }
            PollResult::Complete(Err(e)) => {
                self.progress_rx = None;
                self.progress.phase = UpdatePhase::Failed;
                let msg = format!("Update task panicked: {}", e);
                events.push(StateEvent::LogError(msg.clone()));
                self.error = Some(msg.clone());
                events.push(StateEvent::StatusMessage(msg));
            }
            PollResult::Pending => ctx.request_repaint(),
            PollResult::NoTask => {}
        }

        events
    }
}
