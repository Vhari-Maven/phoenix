//! Update management commands

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use serde::Serialize;
use tokio::sync::watch;

use crate::cli::output::{print_error, print_formatted, print_success, should_show_progress, OutputFormat};
use crate::config::Config;
use crate::db::Database;
use crate::game;
use crate::github::GitHubClient;
use crate::update::{self, UpdateProgress};
use crate::util::format_size;

#[derive(Subcommand, Debug)]
pub enum UpdateCommands {
    /// Check for available updates
    Check,

    /// List available releases
    Releases {
        /// Maximum number of releases to show
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Filter by branch (stable or experimental)
        #[arg(long)]
        branch: Option<String>,

        /// Fetch specific tags (comma-separated, e.g. "0.H-RELEASE,0.G")
        #[arg(long)]
        tags: Option<String>,
    },

    /// Show changelog for a specific release
    Changelog {
        /// Release tag (e.g., "0.H-RELEASE")
        tag: String,

        /// Skip database cache and fetch fresh from GitHub
        #[arg(long)]
        no_cache: bool,
    },

    /// Download an update (without installing)
    Download {
        /// Specific version to download
        #[arg(long)]
        version: Option<String>,
    },

    /// Install a downloaded update
    Install,

    /// Download and install in one step
    Apply {
        /// Keep saves in place during update
        #[arg(long)]
        keep_saves: bool,

        /// Remove previous version after update
        #[arg(long)]
        remove_old: bool,

        /// Show what would happen without doing it
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Serialize)]
struct CheckResult {
    current_version: Option<String>,
    latest_version: Option<String>,
    update_available: bool,
    download_url: Option<String>,
    download_size_bytes: Option<u64>,
}

#[derive(Serialize)]
struct ReleaseEntry {
    tag: String,
    name: String,
    published: String,
    has_windows_asset: bool,
    asset_size_bytes: Option<u64>,
}

#[derive(Serialize)]
struct ReleasesResult {
    branch: String,
    releases: Vec<ReleaseEntry>,
}

pub async fn run(command: UpdateCommands, format: OutputFormat, quiet: bool) -> Result<()> {
    match command {
        UpdateCommands::Check => check(format).await,
        UpdateCommands::Releases { limit, branch, tags } => releases(limit, branch, tags, format).await,
        UpdateCommands::Changelog { tag, no_cache } => changelog(tag, no_cache, format, quiet).await,
        UpdateCommands::Download { version } => download(version, format, quiet).await,
        UpdateCommands::Install => install(format, quiet).await,
        UpdateCommands::Apply { keep_saves, remove_old, dry_run } => {
            apply(keep_saves, remove_old, dry_run, format, quiet).await
        }
    }
}

async fn check(format: OutputFormat) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    // Detect current version
    let db = Database::open().ok();
    let current_info = game::detect_game_with_db(&game_dir, db.as_ref())?;
    let current_version = current_info.as_ref().map(|g| g.version_display().to_string());

    // Fetch latest release
    let client = GitHubClient::new()?;
    let branch = &config.game.branch;

    let releases = if branch == "stable" {
        client.get_stable_releases().await?.data
    } else {
        client.get_experimental_releases().await?.data
    };

    let latest = releases.first();
    let latest_asset = latest.and_then(|r| GitHubClient::find_windows_asset(r));

    let latest_version = latest.map(|r| r.tag_name.clone());
    let update_available = match (&current_version, &latest_version) {
        (Some(current), Some(latest)) => current != latest,
        _ => false,
    };

    let result = CheckResult {
        current_version,
        latest_version,
        update_available,
        download_url: latest_asset.map(|a| a.browser_download_url.clone()),
        download_size_bytes: latest_asset.map(|a| a.size),
    };

    print_formatted(&result, format, |r| {
        let current = r.current_version.as_deref().unwrap_or("Unknown");
        let latest = r.latest_version.as_deref().unwrap_or("Unknown");

        if r.update_available {
            format!(
                "Current version: {}\nLatest version:  {}\n\nUpdate available! Run 'phoenix update apply' to install.",
                current, latest
            )
        } else {
            format!(
                "Current version: {}\nYou are running the latest version.",
                current
            )
        }
    });

    Ok(())
}

async fn releases(limit: usize, branch: Option<String>, tags: Option<String>, format: OutputFormat) -> Result<()> {
    let config = Config::load()?;
    let branch = branch.unwrap_or_else(|| config.game.branch.clone());

    let client = GitHubClient::new()?;

    let releases = if let Some(tags_str) = tags {
        // Fetch specific tags directly
        let tag_list: Vec<&str> = tags_str.split(',').map(|s| s.trim()).collect();
        client.get_releases_by_tags(&tag_list).await?.data
    } else if branch == "stable" {
        client.get_stable_releases().await?.data
    } else {
        client.get_experimental_releases().await?.data
    };

    let entries: Vec<ReleaseEntry> = releases
        .into_iter()
        .take(limit)
        .map(|r| {
            let asset = GitHubClient::find_windows_asset(&r);
            let has_windows_asset = asset.is_some();
            let asset_size_bytes = asset.map(|a| a.size);
            ReleaseEntry {
                tag: r.tag_name,
                name: r.name,
                published: r.published_at[..10].to_string(), // Just date
                has_windows_asset,
                asset_size_bytes,
            }
        })
        .collect();

    let result = ReleasesResult {
        branch: branch.clone(),
        releases: entries,
    };

    print_formatted(&result, format, |r| {
        let mut lines = vec![format!("Available {} releases:\n", r.branch)];

        lines.push(format!("{:<40} {:>12} {:>10}", "TAG", "DATE", "SIZE"));
        lines.push("-".repeat(65));

        for release in &r.releases {
            let size = release
                .asset_size_bytes
                .map(format_size)
                .unwrap_or_else(|| "N/A".to_string());
            let marker = if release.has_windows_asset { "" } else { " *" };
            lines.push(format!(
                "{:<40} {:>12} {:>10}{}",
                release.tag, release.published, size, marker
            ));
        }

        if r.releases.iter().any(|r| !r.has_windows_asset) {
            lines.push(String::new());
            lines.push("* = No Windows x64 graphical build available".to_string());
        }

        lines.join("\n")
    });

    Ok(())
}

#[derive(Serialize)]
struct ChangelogResult {
    tag: String,
    source: String, // "cache" or "api"
    body: Option<String>,
}

async fn changelog(tag: String, no_cache: bool, format: OutputFormat, quiet: bool) -> Result<()> {
    let db = Database::open().ok();
    let mut source = "api";
    let mut body: Option<String> = None;

    // Check DB cache first (unless --no-cache)
    if !no_cache {
        if let Some(ref db) = db {
            if let Ok(Some(cached_body)) = db.get_changelog(&tag) {
                if !quiet {
                    eprintln!("Found changelog in cache");
                }
                source = "cache";
                body = Some(cached_body);
            }
        }
    }

    // Fetch from GitHub API if not in cache
    if body.is_none() {
        if !quiet {
            eprintln!("Fetching changelog from GitHub API...");
        }
        let client = GitHubClient::new()?;
        let (release, _rate_limit) = client.get_release_by_tag(&tag).await;

        if let Some(release) = release {
            body = release.body;

            // Cache in database
            if let (Some(db), Some(changelog_body)) = (&db, &body) {
                if let Err(e) = db.store_changelog(&tag, changelog_body) {
                    eprintln!("Warning: Failed to cache changelog: {}", e);
                } else if !quiet {
                    eprintln!("Cached changelog in database");
                }
            }
        }
    }

    let result = ChangelogResult {
        tag: tag.clone(),
        source: source.to_string(),
        body: body.clone(),
    };

    print_formatted(&result, format, |r| {
        match &r.body {
            Some(text) => format!(
                "Changelog for {} (from {}):\n\n{}",
                r.tag, r.source, text
            ),
            None => format!("No changelog found for {}", r.tag),
        }
    });

    Ok(())
}

async fn download(version: Option<String>, format: OutputFormat, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let client = GitHubClient::new()?;

    // Get releases
    let branch = &config.game.branch;
    let releases = if branch == "stable" {
        client.get_stable_releases().await?.data
    } else {
        client.get_experimental_releases().await?.data
    };

    // Find the release to download
    let release = match version {
        Some(ref v) => releases.iter().find(|r| r.tag_name == *v || r.name == *v),
        None => releases.first(),
    };

    let release = release.context("No release found")?;
    let asset = GitHubClient::find_windows_asset(release)
        .context("No Windows x64 graphical asset found for this release")?;

    // Create progress channel
    let (progress_tx, mut progress_rx) = watch::channel(UpdateProgress::default());

    // Spawn progress reporter (only if TTY and not quiet)
    let show_progress = should_show_progress(quiet, format);
    if show_progress {
        tokio::spawn(async move {
            while progress_rx.changed().await.is_ok() {
                let p = progress_rx.borrow().clone();
                if p.total_bytes > 0 {
                    let percent = (p.bytes_downloaded as f64 / p.total_bytes as f64 * 100.0) as u32;
                    let speed_mb = p.speed as f64 / 1024.0 / 1024.0;
                    eprint!(
                        "\rDownloading: {}% ({} / {}) {:.1} MB/s   ",
                        percent,
                        format_size(p.bytes_downloaded),
                        format_size(p.total_bytes),
                        speed_mb
                    );
                }
            }
            eprintln!();
        });
    }

    // Download to temp location
    let download_dir = std::env::temp_dir().join("phoenix");
    std::fs::create_dir_all(&download_dir)?;
    let dest_path = download_dir.join(format!("{}.zip", release.tag_name));

    let result = update::download_asset(
        client.client().clone(),
        asset.browser_download_url.clone(),
        dest_path.clone(),
        progress_tx,
    )
    .await?;

    print_success(
        &format!(
            "Downloaded: {} ({})\nSaved to: {}",
            release.tag_name,
            format_size(result.bytes),
            dest_path.display()
        ),
        quiet,
    );

    Ok(())
}

async fn install(format: OutputFormat, quiet: bool) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    // Look for downloaded update
    let download_dir = std::env::temp_dir().join("phoenix");
    let mut zip_files: Vec<_> = std::fs::read_dir(&download_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "zip"))
        .collect();

    if zip_files.is_empty() {
        print_error("No downloaded update found. Run 'phoenix update download' first.");
        return Err(anyhow::anyhow!("No update to install"));
    }

    // Sort by modification time, newest first
    zip_files.sort_by(|a, b| {
        b.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            .cmp(
                &a.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
    });

    let zip_path = zip_files[0].path();
    println!("Installing: {}", zip_path.display());

    // Create progress channel
    let (progress_tx, mut progress_rx) = watch::channel(UpdateProgress::default());

    // Spawn progress reporter (only if TTY and not quiet)
    let show_progress = should_show_progress(quiet, format);
    if show_progress {
        tokio::spawn(async move {
            while progress_rx.changed().await.is_ok() {
                let p = progress_rx.borrow().clone();
                eprint!(
                    "\r{}: {}/{}   ",
                    p.phase.description(),
                    p.files_extracted,
                    p.total_files
                );
            }
            eprintln!();
        });
    }

    update::install_update(
        zip_path,
        game_dir,
        progress_tx,
        config.updates.prevent_save_move,
        config.updates.remove_previous_version,
    )
    .await?;

    print_success("Update installed successfully!", quiet);

    Ok(())
}

async fn apply(
    keep_saves: bool,
    remove_old: bool,
    dry_run: bool,
    format: OutputFormat,
    quiet: bool,
) -> Result<()> {
    let config = Config::load()?;
    let game_dir = config
        .game
        .directory
        .as_ref()
        .map(PathBuf::from)
        .context("No game directory configured")?;

    let client = GitHubClient::new()?;

    // Get latest release
    let branch = &config.game.branch;
    let releases = if branch == "stable" {
        client.get_stable_releases().await?.data
    } else {
        client.get_experimental_releases().await?.data
    };

    let release = releases.first().context("No releases found")?;
    let asset = GitHubClient::find_windows_asset(release)
        .context("No Windows x64 graphical asset found")?;

    if dry_run {
        println!("Dry run - would apply update:");
        println!("  Version: {}", release.tag_name);
        println!("  Size: {}", format_size(asset.size));
        println!("  Keep saves in place: {}", keep_saves || config.updates.prevent_save_move);
        println!("  Remove old version: {}", remove_old || config.updates.remove_previous_version);
        return Ok(());
    }

    // Create progress channel
    let (progress_tx, mut progress_rx) = watch::channel(UpdateProgress::default());

    // Spawn progress reporter (only if TTY and not quiet)
    let show_progress = should_show_progress(quiet, format);
    if show_progress {
        tokio::spawn(async move {
            while progress_rx.changed().await.is_ok() {
                let p = progress_rx.borrow().clone();
                match p.phase {
                    update::UpdatePhase::Downloading => {
                        if p.total_bytes > 0 {
                            let percent =
                                (p.bytes_downloaded as f64 / p.total_bytes as f64 * 100.0) as u32;
                            eprint!(
                                "\rDownloading: {}% ({})   ",
                                percent,
                                format_size(p.bytes_downloaded)
                            );
                        }
                    }
                    _ => {
                        eprint!(
                            "\r{}: {}/{}   ",
                            p.phase.description(),
                            p.files_extracted,
                            p.total_files
                        );
                    }
                }
            }
            eprintln!();
        });
    }

    // Download
    let download_dir = std::env::temp_dir().join("phoenix");
    std::fs::create_dir_all(&download_dir)?;
    let zip_path = download_dir.join(format!("{}.zip", release.tag_name));

    if !quiet {
        println!("Downloading {}...", release.tag_name);
    }

    update::download_asset(
        client.client().clone(),
        asset.browser_download_url.clone(),
        zip_path.clone(),
        progress_tx.clone(),
    )
    .await?;

    // Install
    if !quiet {
        println!("Installing...");
    }

    let prevent_save_move = keep_saves || config.updates.prevent_save_move;
    let remove_previous = remove_old || config.updates.remove_previous_version;

    update::install_update(zip_path, game_dir, progress_tx, prevent_save_move, remove_previous)
        .await?;

    print_success(
        &format!("Update complete! Now running: {}", release.tag_name),
        quiet,
    );

    Ok(())
}
