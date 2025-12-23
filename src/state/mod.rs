//! Application state modules
//!
//! This module contains grouped state structs extracted from PhoenixApp.
//! Each state struct owns its related fields and poll methods.

mod backup;
mod releases;
mod soundpack;
mod ui;
mod update;

pub use backup::BackupState;
pub use releases::ReleasesState;
pub use soundpack::SoundpackState;
pub use ui::{Tab, UiState};
pub use update::{UpdateParams, UpdateState};

/// Events that state poll methods can return.
/// These communicate results back to PhoenixApp without direct mutation.
#[derive(Debug)]
pub enum StateEvent {
    /// Update the status message
    StatusMessage(String),

    /// Trigger game info refresh
    RefreshGameInfo,

    /// Log an error message
    LogError(String),

    /// Log an info message
    LogInfo(String),

    /// Changelog was fetched for a release (tag, body)
    ChangelogFetched { tag: String, body: String },
}
