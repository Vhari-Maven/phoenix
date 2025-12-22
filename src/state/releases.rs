//! Releases-related application state

use anyhow::Result;
use eframe::egui;
use tokio::task::JoinHandle;

use crate::game::GameInfo;
use crate::github::{FetchResult, GitHubClient, RateLimitInfo, Release};
use crate::state::StateEvent;
use crate::task::{poll_task, PollResult};

/// Releases-related state
pub struct ReleasesState {
    /// Fetched experimental releases
    pub experimental: Vec<Release>,
    /// Fetched stable releases
    pub stable: Vec<Release>,
    /// Index of selected release in current list
    pub selected_idx: Option<usize>,
    /// Async task for fetching releases
    task: Option<JoinHandle<Result<FetchResult<Vec<Release>>>>>,
    /// Which branch is being fetched
    fetching_branch: Option<String>,
    /// Whether releases are currently being fetched
    pub loading: bool,
    /// Error message from last fetch attempt
    pub error: Option<String>,
    /// Last known rate limit info from GitHub API
    pub rate_limit: RateLimitInfo,
}

impl Default for ReleasesState {
    fn default() -> Self {
        Self {
            experimental: Vec::new(),
            stable: Vec::new(),
            selected_idx: None,
            task: None,
            fetching_branch: None,
            loading: false,
            error: None,
            rate_limit: RateLimitInfo::default(),
        }
    }
}

impl ReleasesState {
    /// Get releases for the specified branch
    pub fn for_branch(&self, branch: &str) -> &Vec<Release> {
        if branch == "stable" {
            &self.stable
        } else {
            &self.experimental
        }
    }

    /// Check if we have releases for the given branch
    pub fn has_for_branch(&self, branch: &str) -> bool {
        if branch == "stable" {
            !self.stable.is_empty()
        } else {
            !self.experimental.is_empty()
        }
    }

    /// Simple check: is the selected release different from the installed version?
    /// Returns true if they are different (can update/switch), false if same or can't compare
    pub fn is_selected_different(&self, branch: &str, game_info: Option<&GameInfo>) -> bool {
        let Some(game_info) = game_info else {
            return false; // No game installed
        };
        let releases = self.for_branch(branch);
        let Some(selected_release) = self.selected_idx.and_then(|i| releases.get(i)) else {
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

    /// Start fetching releases for a specific branch
    pub fn fetch_for_branch(&mut self, branch: &str, client: &GitHubClient) -> Option<StateEvent> {
        if self.loading {
            return None; // Already fetching
        }

        self.loading = true;
        self.error = None;
        self.fetching_branch = Some(branch.to_string());

        let client = client.clone();
        let is_stable = branch == "stable";

        self.task = Some(tokio::spawn(async move {
            if is_stable {
                client.get_stable_releases().await
            } else {
                client.get_experimental_releases(50).await
            }
        }));

        Some(StateEvent::StatusMessage(format!("Fetching {} releases...", branch)))
    }

    /// Poll the async releases task for completion
    pub fn poll(&mut self, ctx: &egui::Context, current_branch: &str) -> Vec<StateEvent> {
        let mut events = Vec::new();

        match poll_task(&mut self.task) {
            PollResult::Complete(Ok(Ok(result))) => {
                let branch = self.fetching_branch.take();
                let count = result.data.len();
                self.rate_limit = result.rate_limit;

                // Store in appropriate list based on which branch we fetched
                let is_current_branch = branch.as_deref() == Some(current_branch);
                if branch.as_deref() == Some("stable") {
                    self.stable = result.data;
                } else {
                    self.experimental = result.data;
                }
                // Auto-select latest release if this is for the current branch
                if is_current_branch && count > 0 {
                    self.selected_idx = Some(0);
                }
                events.push(StateEvent::StatusMessage(format!("Fetched {} releases", count)));
                events.push(StateEvent::LogInfo(format!(
                    "Fetched {} {} releases from GitHub",
                    count,
                    branch.as_deref().unwrap_or("unknown")
                )));
                self.loading = false;
            }
            PollResult::Complete(Ok(Err(e))) => {
                self.fetching_branch.take();
                let msg = e.to_string();
                events.push(StateEvent::LogError(format!("Failed to fetch releases: {}", msg)));
                self.error = Some(msg.clone());
                events.push(StateEvent::StatusMessage(format!("Error: {}", msg)));
                self.loading = false;
            }
            PollResult::Complete(Err(e)) => {
                self.fetching_branch.take();
                let msg = e.to_string();
                events.push(StateEvent::LogError(format!("Task panicked: {}", msg)));
                self.error = Some(msg);
                self.loading = false;
            }
            PollResult::Pending => ctx.request_repaint(),
            PollResult::NoTask => {}
        }

        events
    }
}
