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

use anyhow::Result;
use serde::Deserialize;

/// GitHub API base URL
const GITHUB_API_BASE: &str = "https://api.github.com";

/// Repository for CDDA game releases
const CDDA_REPO: &str = "CleverRaven/Cataclysm-DDA";

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

    /// Check if rate limit is low (≤10 remaining)
    pub fn is_low(&self) -> bool {
        self.remaining.map(|r| r <= 10).unwrap_or(false)
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
        let url = format!(
            "{}/repos/{}/releases/tags/{}",
            GITHUB_API_BASE, CDDA_REPO, tag
        );

        let response = match self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return (None, RateLimitInfo::default()),
        };

        let rate_limit = RateLimitInfo::from_response(&response);

        if !response.status().is_success() {
            return (None, rate_limit);
        }

        match response.json().await {
            Ok(release) => (Some(release), rate_limit),
            Err(_) => (None, rate_limit),
        }
    }

    /// Known stable version letters (A through J)
    /// Includes future letters (H, I, J) that will return 404 until released.
    /// CDDA releases ~1 major version per year, so this covers several years.
    const STABLE_VERSIONS: &'static [char] = &['J', 'I', 'H', 'G', 'F', 'E', 'D', 'C', 'B', 'A'];

    /// Maximum point release number to check (e.g., 0.F-3)
    /// Set high enough to handle any reasonable point release count.
    const MAX_POINT_RELEASE: u8 = 10;

    /// Generate candidate stable tags to try
    /// Returns tags in priority order (latest point release first for each version)
    fn generate_stable_tag_candidates() -> Vec<String> {
        let mut candidates = Vec::new();

        for &letter in Self::STABLE_VERSIONS {
            // Try point releases first (highest to lowest), then base version
            for point in (0..=Self::MAX_POINT_RELEASE).rev() {
                if point == 0 {
                    candidates.push(format!("0.{}", letter));
                } else {
                    candidates.push(format!("0.{}-{}", letter, point));
                }
            }
        }

        candidates
    }

    /// Fetch stable releases by trying known tag patterns directly.
    ///
    /// CDDA stable releases follow a predictable naming pattern: `0.C`, `0.D`, ..., `0.G`,
    /// with optional point releases like `0.F-1`, `0.F-2`, etc.
    ///
    /// Instead of fetching all 1000+ tags and filtering, this function:
    /// 1. Generates candidate tag names (`0.C` through `0.Z`, plus `-1` through `-10` variants)
    /// 2. Fetches all candidates in parallel (most will 404)
    /// 3. Keeps only the latest release per version letter (e.g., `0.F-3` over `0.F-2`)
    /// 4. Returns releases sorted by version letter descending (newest first)
    ///
    /// This approach is ~10x faster than pagination and uses fewer API requests.
    pub async fn get_stable_releases(&self) -> Result<FetchResult<Vec<Release>>> {
        let start = std::time::Instant::now();

        // Generate all candidate tags and fetch them in parallel
        let candidates = Self::generate_stable_tag_candidates();

        let futures: Vec<_> = candidates
            .iter()
            .map(|tag| self.get_release_by_tag(tag))
            .collect();

        let results = futures::future::join_all(futures).await;

        // Collect successful releases, keeping only the latest per version letter
        let mut releases_by_letter: std::collections::HashMap<char, Release> =
            std::collections::HashMap::new();
        let mut last_rate_limit = RateLimitInfo::default();

        for (release, rate_limit) in results {
            last_rate_limit = rate_limit;
            if let Some(r) = release {
                // Extract version letter from tag
                let tag = r.tag_name.strip_prefix("cdda-").unwrap_or(&r.tag_name);
                if let Some(letter) = tag.chars().nth(2) {
                    // Only keep the first (latest) release for each letter
                    releases_by_letter.entry(letter).or_insert(r);
                }
            }
        }

        // Convert to sorted vec (by letter descending)
        let mut stable: Vec<Release> = releases_by_letter.into_values().collect();
        stable.sort_by(|a, b| {
            let get_letter = |tag: &str| -> char {
                let tag = tag.strip_prefix("cdda-").unwrap_or(tag);
                tag.chars().nth(2).unwrap_or('A')
            };
            get_letter(&b.tag_name).cmp(&get_letter(&a.tag_name))
        });

        tracing::info!(
            "Fetched {} stable releases in {:.1}s",
            stable.len(),
            start.elapsed().as_secs_f32()
        );
        Ok(FetchResult { data: stable, rate_limit: last_rate_limit })
    }

    /// Fetch experimental releases (recent builds from releases list)
    pub async fn get_experimental_releases(&self, per_page: u32) -> Result<FetchResult<Vec<Release>>> {
        let start = std::time::Instant::now();
        let url = format!(
            "{}/repos/{}/releases?per_page={}",
            GITHUB_API_BASE, CDDA_REPO, per_page
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
