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

/// GitHub API client
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

    /// Fetch recent releases for CDDA
    pub async fn get_releases(&self, per_page: u32) -> Result<Vec<Release>> {
        let url = format!(
            "{}/repos/{}/releases?per_page={}",
            GITHUB_API_BASE, CDDA_REPO, per_page
        );

        tracing::debug!("Fetching releases from: {}", url);

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

        let releases: Vec<Release> = response.json().await?;
        tracing::info!("Fetched {} releases", releases.len());

        Ok(releases)
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
pub fn filter_releases_by_branch(releases: &[Release], branch: &str) -> Vec<&Release> {
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
