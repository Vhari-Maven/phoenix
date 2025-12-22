//! Backup-related application state

use std::path::Path;

use eframe::egui;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::backup::{self, BackupError, BackupInfo, BackupPhase, BackupProgress};
use crate::state::StateEvent;
use crate::task::{poll_task, PollResult};

/// Backup-related state
pub struct BackupState {
    /// List of available backups
    pub list: Vec<BackupInfo>,
    /// Whether backup list is being loaded
    pub list_loading: bool,
    /// Index of selected backup in list
    pub selected_idx: Option<usize>,
    /// Input field for manual backup name
    pub name_input: String,
    /// Async task for backup operation
    task: Option<JoinHandle<Result<(), BackupError>>>,
    /// Async task for loading backup list
    list_task: Option<JoinHandle<Result<Vec<BackupInfo>, BackupError>>>,
    /// Channel receiver for backup progress
    progress_rx: Option<watch::Receiver<BackupProgress>>,
    /// Current backup progress
    pub progress: BackupProgress,
    /// Error message from last backup attempt
    pub error: Option<String>,
    /// Whether to show delete confirmation
    pub confirm_delete: bool,
    /// Whether to show restore confirmation
    pub confirm_restore: bool,
}

impl Default for BackupState {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            list_loading: false,
            selected_idx: None,
            name_input: String::new(),
            task: None,
            list_task: None,
            progress_rx: None,
            progress: BackupProgress::default(),
            error: None,
            confirm_delete: false,
            confirm_restore: false,
        }
    }
}

impl BackupState {
    /// Check if a backup operation is in progress
    pub fn is_busy(&self) -> bool {
        self.task.is_some() || self.list_loading
    }

    /// Validate a backup name
    pub fn validate_name(&self, name: &str) -> Result<(), String> {
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
        if self.list.iter().any(|b| b.name == name) {
            return Err("A backup with this name already exists".to_string());
        }
        Ok(())
    }

    /// Start a manual backup
    pub fn start_manual_backup(
        &mut self,
        game_dir: &Path,
        compression_level: u8,
    ) -> Option<StateEvent> {
        let name = self.name_input.trim().to_string();
        if name.is_empty() {
            return None;
        }

        // Clear previous state
        self.error = None;
        self.progress = BackupProgress::default();

        let (progress_tx, progress_rx) = watch::channel(BackupProgress::default());
        self.progress_rx = Some(progress_rx);

        let game_dir = game_dir.to_path_buf();
        let name_for_status = name.clone();

        tracing::info!("Starting manual backup: {}", name);

        self.task = Some(tokio::spawn(async move {
            backup::create_backup(&game_dir, &name, compression_level, progress_tx).await?;
            Ok(())
        }));

        // Clear input on success start
        self.name_input.clear();

        Some(StateEvent::StatusMessage(format!("Creating backup: {}", name_for_status)))
    }

    /// Refresh the backup list
    pub fn refresh_list(&mut self) {
        if self.list_loading || self.list_task.is_some() {
            return;
        }

        self.list_loading = true;
        self.selected_idx = None;
        self.error = None;

        self.list_task = Some(tokio::spawn(async move {
            backup::list_backups().await
        }));
    }

    /// Delete the selected backup
    pub fn delete_selected(&mut self) -> Option<StateEvent> {
        let idx = self.selected_idx?;
        let backup = self.list.get(idx)?;

        let backup_name = backup.name.clone();
        let backup_name_for_status = backup_name.clone();

        self.error = None;
        tracing::info!("Deleting backup: {}", backup_name);

        self.task = Some(tokio::spawn(async move {
            backup::delete_backup(&backup_name).await
        }));

        self.selected_idx = None;

        Some(StateEvent::StatusMessage(format!("Deleting backup: {}", backup_name_for_status)))
    }

    /// Restore the selected backup
    pub fn restore_selected(
        &mut self,
        game_dir: &Path,
        skip_backup_before_restore: bool,
        compression_level: u8,
    ) -> Option<StateEvent> {
        let idx = self.selected_idx?;
        let backup = self.list.get(idx)?;

        let backup_name = backup.name.clone();
        let backup_name_for_status = backup_name.clone();
        let game_dir = game_dir.to_path_buf();
        let backup_first = !skip_backup_before_restore;

        self.error = None;
        self.progress = BackupProgress::default();

        let (progress_tx, progress_rx) = watch::channel(BackupProgress::default());
        self.progress_rx = Some(progress_rx);

        tracing::info!("Restoring backup: {}", backup_name);

        self.task = Some(tokio::spawn(async move {
            backup::restore_backup(&game_dir, &backup_name, backup_first, compression_level, progress_tx).await
        }));

        Some(StateEvent::StatusMessage(format!("Restoring backup: {}", backup_name_for_status)))
    }

    /// Poll backup tasks for progress and completion
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

        // Check if backup operation task is complete
        match poll_task(&mut self.task) {
            PollResult::Complete(Ok(Ok(()))) => {
                self.progress_rx = None;
                self.progress.phase = BackupPhase::Complete;
                events.push(StateEvent::StatusMessage("Backup operation complete!".to_string()));
                events.push(StateEvent::LogInfo("Backup operation completed successfully".to_string()));

                // Trigger backup list refresh
                self.refresh_list();
            }
            PollResult::Complete(Ok(Err(e))) => {
                self.progress_rx = None;
                self.progress.phase = BackupPhase::Failed;
                let msg = e.to_string();
                events.push(StateEvent::LogError(format!("Backup operation failed: {}", msg)));
                self.error = Some(msg.clone());
                events.push(StateEvent::StatusMessage(format!("Backup failed: {}", msg)));
            }
            PollResult::Complete(Err(e)) => {
                self.progress_rx = None;
                self.progress.phase = BackupPhase::Failed;
                let msg = format!("Backup task panicked: {}", e);
                events.push(StateEvent::LogError(msg.clone()));
                self.error = Some(msg.clone());
                events.push(StateEvent::StatusMessage(msg));
            }
            PollResult::Pending => ctx.request_repaint(),
            PollResult::NoTask => {}
        }

        // Check if backup list loading task is complete
        match poll_task(&mut self.list_task) {
            PollResult::Complete(Ok(Ok(list))) => {
                self.list_loading = false;
                self.list = list;
                events.push(StateEvent::LogInfo(format!("Loaded {} backups", self.list.len())));
            }
            PollResult::Complete(Ok(Err(e))) => {
                self.list_loading = false;
                events.push(StateEvent::LogError(format!("Failed to load backup list: {}", e)));
                self.error = Some(format!("Failed to load backups: {}", e));
            }
            PollResult::Complete(Err(e)) => {
                self.list_loading = false;
                events.push(StateEvent::LogError(format!("Backup list task panicked: {}", e)));
            }
            PollResult::Pending => ctx.request_repaint(),
            PollResult::NoTask => {}
        }

        events
    }
}
