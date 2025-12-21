use anyhow::Result;
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

/// GitHub API base URL
const GITHUB_API_BASE: &str = "https://api.github.com";

/// Repository for CDDA game releases
const CDDA_REPO: &str = "CleverRaven/Cataclysm-DDA";

/// Regex pattern to match stable release tags
/// Matches: 0.A, 0.A-1, 0.A-2, cdda-0.A, cdda-0.A-1, etc.
const STABLE_TAG_PATTERN: &str = r"^(cdda-)?(0\.[A-Z])(-[0-9]+)?$";

/// User agent for API requests
const USER_AGENT: &str = concat!("Phoenix-Launcher/", env!("CARGO_PKG_VERSION"));

/// A GitHub release
#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub name: String,
    pub body: Option<String>,
    pub published_at: String,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

/// An asset attached to a release
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
    pub content_type: String,
}

/// A git reference (tag) from GitHub API
#[derive(Debug, Clone, Deserialize)]
struct GitRef {
    #[serde(rename = "ref")]
    ref_name: String,
}

/// Get the compiled regex for matching stable tags
fn stable_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(STABLE_TAG_PATTERN).expect("Invalid stable tag regex"))
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

    /// Fetch a release by tag name (returns None if tag doesn't exist)
    /// Also returns rate limit info from the response
    async fn get_release_by_tag(&self, tag: &str) -> (Option<Release>, RateLimitInfo) {
        let url = format!(
            "{}/repos/{}/releases/tags/{}",
            GITHUB_API_BASE, CDDA_REPO, tag
        );

        tracing::debug!("Fetching release by tag: {}", url);

        let response = match self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!("Failed to fetch tag {}: {}", tag, e);
                return (None, RateLimitInfo::default());
            }
        };

        let rate_limit = RateLimitInfo::from_response(&response);

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            tracing::debug!("Tag {} not found (not released yet)", tag);
            return (None, rate_limit);
        }

        if !response.status().is_success() {
            tracing::debug!("Failed to fetch tag {}: {}", tag, response.status());
            return (None, rate_limit);
        }

        match response.json().await {
            Ok(release) => (Some(release), rate_limit),
            Err(e) => {
                tracing::debug!("Failed to parse release {}: {}", tag, e);
                (None, rate_limit)
            }
        }
    }

    /// Fetch all git tags and filter for stable release tags
    async fn get_stable_tags(&self) -> Result<(Vec<String>, RateLimitInfo)> {
        let url = format!(
            "{}/repos/{}/git/refs/tags",
            GITHUB_API_BASE, CDDA_REPO
        );

        tracing::debug!("Fetching git tags from: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await?;

        let rate_limit = RateLimitInfo::from_response(&response);

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error fetching tags: {} - {}", status, text);
        }

        let refs: Vec<GitRef> = response.json().await?;
        let regex = stable_tag_regex();

        // Extract tag names and filter for stable releases
        let mut tags: Vec<String> = refs
            .into_iter()
            .filter_map(|r| {
                // Strip "refs/tags/" prefix
                let tag = r.ref_name.strip_prefix("refs/tags/")?;
                // Check if it matches stable pattern
                if regex.is_match(tag) {
                    Some(tag.to_string())
                } else {
                    None
                }
            })
            .collect();

        // Sort by version letter (descending) then by point release number (descending)
        // e.g., 0.G, 0.F-3, 0.F-2, 0.F-1, 0.F, 0.E-3, ...
        tags.sort_by(|a, b| {
            // Extract version letter and point release
            let parse = |s: &str| -> (char, i32) {
                let s = s.strip_prefix("cdda-").unwrap_or(s);
                let letter = s.chars().nth(2).unwrap_or('A');
                let point = s
                    .split('-')
                    .nth(1)
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                (letter, point)
            };
            let (la, pa) = parse(a);
            let (lb, pb) = parse(b);
            // Sort by letter desc, then point release desc
            lb.cmp(&la).then(pb.cmp(&pa))
        });

        // Remove cdda- prefixed duplicates (keep only non-prefixed if both exist)
        let mut seen_versions = std::collections::HashSet::new();
        tags.retain(|tag| {
            let normalized = tag.strip_prefix("cdda-").unwrap_or(tag);
            if seen_versions.contains(normalized) {
                false
            } else {
                seen_versions.insert(normalized.to_string());
                true
            }
        });

        tracing::info!("Found {} stable tags", tags.len());
        Ok((tags, rate_limit))
    }

    /// Fetch stable releases by discovering tags from GitHub
    pub async fn get_stable_releases(&self) -> Result<FetchResult<Vec<Release>>> {
        // First, get all stable tags
        let (tags, mut last_rate_limit) = self.get_stable_tags().await?;

        let mut stable = Vec::new();

        // Fetch release info for each tag
        for tag in &tags {
            let (release, rate_limit) = self.get_release_by_tag(tag).await;
            last_rate_limit = rate_limit;

            if let Some(r) = release {
                tracing::info!("Found stable release: {} ({})", r.name, tag);
                stable.push(r);
            }
        }

        if let Some(remaining) = last_rate_limit.remaining {
            tracing::debug!("GitHub API rate limit remaining: {}", remaining);
        }

        tracing::info!("Fetched {} stable releases", stable.len());
        Ok(FetchResult { data: stable, rate_limit: last_rate_limit })
    }

    /// Fetch experimental releases (recent builds from releases list)
    pub async fn get_experimental_releases(&self, per_page: u32) -> Result<FetchResult<Vec<Release>>> {
        let url = format!(
            "{}/repos/{}/releases?per_page={}",
            GITHUB_API_BASE, CDDA_REPO, per_page
        );

        tracing::debug!("Fetching experimental releases from: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await?;

        // Extract rate limit info before consuming response
        let rate_limit = RateLimitInfo::from_response(&response);
        if let Some(remaining) = rate_limit.remaining {
            tracing::debug!("GitHub API rate limit remaining: {}", remaining);
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error: {} - {}", status, text);
        }

        let releases: Vec<Release> = response.json().await?;
        tracing::info!("Fetched {} experimental releases", releases.len());

        Ok(FetchResult { data: releases, rate_limit })
    }

    /// Fetch the latest release
    pub async fn get_latest_release(&self) -> Result<Release> {
        let url = format!(
            "{}/repos/{}/releases/latest",
            GITHUB_API_BASE, CDDA_REPO
        );

        tracing::debug!("Fetching latest release from: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub API error: {} - {}", status, text);
        }

        let release: Release = response.json().await?;
        Ok(release)
    }

    /// Find the Windows x64 tiles asset in a release
    pub fn find_windows_asset(release: &Release) -> Option<&ReleaseAsset> {
        release.assets.iter().find(|asset| {
            let name = asset.name.to_lowercase();
            name.contains("windows") && name.contains("tiles") && name.contains("x64") && name.ends_with(".zip")
        })
    }
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP client")
    }
}

/// Filter releases by branch type
pub fn filter_releases_by_branch<'a>(releases: &'a [Release], branch: &str) -> Vec<&'a Release> {
    releases
        .iter()
        .filter(|r| {
            if branch == "stable" {
                // Stable releases are not prereleases and have version-like tags
                !r.prerelease && !r.tag_name.contains("experimental")
            } else {
                // Experimental releases
                r.tag_name.contains("experimental") || r.prerelease
            }
        })
        .collect()
}
