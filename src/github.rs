//! GitHub API client for fetching CDDA releases.
//!
//! This module provides:
//!
//! - `GitHubClient`: HTTP client wrapper with rate limit tracking
//! - `Release` and `ReleaseAsset`: Deserialized GitHub API responses
//! - Functions to fetch experimental and stable releases
//!
//! The client supports two release branches:
//!
//! - **Experimental**: Fetched from the GitHub releases API, filtered to Windows x64 builds
//! - **Stable**: Uses tag-based candidate generation (0.G, 0.H, etc.) to find stable releases
//!   efficiently without pagination
//!
//! Rate limiting is tracked and exposed via `RateLimitInfo` for UI display.
//!
//! Configuration loaded via `app_data::launcher_config()` and `app_data::release_config()`.

use anyhow::Result;
use serde::Deserialize;

use crate::app_data::{launcher_config, stable_releases_config};

/// User agent for API requests
const USER_AGENT: &str = concat!("Phoenix-Launcher/", env!("CARGO_PKG_VERSION"));

/// A GitHub release
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: String,
    pub body: Option<String>,
    pub published_at: String,
    pub assets: Vec<ReleaseAsset>,
}

/// An asset attached to a release
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
}

/// GitHub API rate limit information
#[derive(Debug, Clone, Default)]
pub struct RateLimitInfo {
    /// Requests remaining in current window
    pub remaining: Option<u32>,
    /// Unix timestamp when limit resets
    pub reset_at: Option<i64>,
}

impl RateLimitInfo {
    /// Parse rate limit info from response headers
    fn from_response(response: &reqwest::Response) -> Self {
        let remaining = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        let reset_at = response
            .headers()
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        Self { remaining, reset_at }
    }

    /// Check if rate limit is low (at or below warning threshold)
    pub fn is_low(&self) -> bool {
        let threshold = launcher_config().github.rate_limit_warning_threshold;
        self.remaining.map(|r| r <= threshold).unwrap_or(false)
    }

    /// Get human-readable time until reset
    pub fn reset_in_minutes(&self) -> Option<i64> {
        self.reset_at.map(|reset| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            ((reset - now) / 60).max(0)
        })
    }
}

/// Result of a GitHub API fetch including rate limit info
#[derive(Debug)]
pub struct FetchResult<T> {
    pub data: T,
    pub rate_limit: RateLimitInfo,
}

/// GitHub API client
#[derive(Clone)]
pub struct GitHubClient {
    client: reqwest::Client,
}

impl GitHubClient {
    /// Create a new GitHub API client
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()?;

        Ok(Self { client })
    }

    /// Get a reference to the underlying HTTP client
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Fetch a release by tag name (returns None if tag doesn't exist)
    /// Also returns rate limit info from the response
    async fn get_release_by_tag(&self, tag: &str) -> (Option<Release>, RateLimitInfo) {
        let github = &launcher_config().github;
        let url = format!(
            "{}/repos/{}/releases/tags/{}",
            github.api_base, github.repository, tag
        );

        let response = match self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Request failed for tag {}: {}", tag, e);
                return (None, RateLimitInfo::default());
            }
        };

        let rate_limit = RateLimitInfo::from_response(&response);
        let status = response.status();

        if !status.is_success() {
            if status.as_u16() != 404 {
                tracing::debug!("Tag {} returned status {}", tag, status);
            }
            return (None, rate_limit);
        }

        match response.json().await {
            Ok(release) => {
                tracing::debug!("Found release for tag {}", tag);
                (Some(release), rate_limit)
            }
            Err(e) => {
                tracing::debug!("Failed to parse release for tag {}: {}", tag, e);
                (None, rate_limit)
            }
        }
    }

    /// Fetch stable releases using embedded data plus API check for new releases.
    ///
    /// This approach:
    /// 1. Loads known stable releases from embedded stable_releases.toml (instant, no API)
    /// 2. Checks for any new releases via API (only 1-2 requests for future letters)
    /// 3. Returns combined results sorted by version letter descending
    ///
    /// This avoids rate limiting issues and provides instant results for known releases.
    pub async fn get_stable_releases(&self) -> Result<FetchResult<Vec<Release>>> {
        let start = std::time::Instant::now();
        let config = stable_releases_config();

        let mut releases: Vec<Release> = Vec::new();
        let mut last_rate_limit = RateLimitInfo::default();

        // Convert embedded releases to Release structs
        for embedded in &config.releases {
            // Only include releases that have Windows assets
            if let (Some(asset_name), Some(asset_url), Some(asset_size)) =
                (&embedded.asset_name, &embedded.asset_url, embedded.asset_size)
            {
                releases.push(Release {
                    tag_name: embedded.tag.clone(),
                    name: embedded.name.clone(),
                    body: None,
                    published_at: format!("{}T00:00:00Z", embedded.published),
                    assets: vec![ReleaseAsset {
                        name: asset_name.clone(),
                        size: asset_size,
                        browser_download_url: asset_url.clone(),
                    }],
                });
            }
        }

        tracing::debug!(
            "Loaded {} embedded stable releases",
            releases.len()
        );

        // Check for new releases (future letters) via API
        for letter in &config.check_letters {
            // Try -RELEASE format first (used for 0.H+)
            let tag = format!("0.{}-RELEASE", letter);
            let (release, rate_limit) = self.get_release_by_tag(&tag).await;
            last_rate_limit = rate_limit;

            if let Some(r) = release {
                tracing::info!("Found new stable release: {}", r.tag_name);
                releases.push(r);
            }
        }

        // Sort by version letter descending (newest first)
        releases.sort_by(|a, b| {
            let get_letter = |tag: &str| -> char {
                let tag = tag.strip_prefix("cdda-").unwrap_or(tag);
                tag.chars().nth(2).unwrap_or('A')
            };
            get_letter(&b.tag_name).cmp(&get_letter(&a.tag_name))
        });

        tracing::info!(
            "Loaded {} stable releases in {:.1}s",
            releases.len(),
            start.elapsed().as_secs_f32()
        );
        Ok(FetchResult { data: releases, rate_limit: last_rate_limit })
    }

    /// Fetch releases by specific tag names (for debugging/testing)
    pub async fn get_releases_by_tags(&self, tags: &[&str]) -> Result<FetchResult<Vec<Release>>> {
        let mut releases = Vec::new();
        let mut last_rate_limit = RateLimitInfo::default();

        for tag in tags {
            let (release, rate_limit) = self.get_release_by_tag(tag).await;
            last_rate_limit = rate_limit;
            if let Some(r) = release {
                releases.push(r);
            }
        }

        Ok(FetchResult { data: releases, rate_limit: last_rate_limit })
    }

    /// Fetch experimental releases (recent builds from releases list)
    pub async fn get_experimental_releases(&self) -> Result<FetchResult<Vec<Release>>> {
        let start = std::time::Instant::now();
        let github = &launcher_config().github;
        let url = format!(
            "{}/repos/{}/releases?per_page={}",
            github.api_base, github.repository, github.releases_per_page
        );

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await?;

        // Extract rate limit info before consuming response
        let rate_limit = RateLimitInfo::from_response(&response);

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error: {} - {}", status, text);
        }

        let releases: Vec<Release> = response.json().await?;
        tracing::info!(
            "Fetched {} experimental releases in {:.1}s",
            releases.len(),
            start.elapsed().as_secs_f32()
        );

        Ok(FetchResult { data: releases, rate_limit })
    }

    /// Find the Windows x64 graphical asset in a release.
    ///
    /// Searches release assets for a Windows build matching these criteria:
    /// - Contains "windows" in the filename
    /// - Contains "tiles" or "graphics" (graphical build, not console)
    /// - Contains "x64" (64-bit build)
    /// - Ends with ".zip"
    ///
    /// If multiple matches exist, prefers the version with sounds included.
    /// Returns `None` if no suitable asset is found.
    pub fn find_windows_asset(release: &Release) -> Option<&ReleaseAsset> {
        let mut best_match: Option<&ReleaseAsset> = None;

        for asset in &release.assets {
            let name = asset.name.to_lowercase();
            let is_windows = name.contains("windows");
            // Match both old "tiles" naming and new "with-graphics" naming
            let is_graphical = name.contains("tiles") || name.contains("graphics");
            let is_x64 = name.contains("x64");
            let is_zip = name.ends_with(".zip");
            let has_sounds = name.contains("sounds");

            if is_windows && is_graphical && is_x64 && is_zip {
                // Prefer version with sounds
                if has_sounds || best_match.is_none() {
                    best_match = Some(asset);
                    if has_sounds {
                        // Found best possible match, no need to continue
                        break;
                    }
                }
            }
        }

        if best_match.is_none() {
            tracing::warn!(
                "No Windows x64 graphical asset in {} ({} assets)",
                release.name,
                release.assets.len()
            );
        }

        best_match
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP client")
    }
}
