# Phoenix - CDDA Game Launcher

A fast, native game launcher for Cataclysm: Dark Days Ahead, built in Rust.

## Project Overview

This is a ground-up rebuild of the CDDA Game Launcher, aiming for sub-second startup times compared to the original Python/Qt launcher's 10-15 second load time.

## Documentation

- [docs/PLAN.md](docs/PLAN.md) - Development plan with spiral roadmap and current progress
- [docs/ANALYSIS.md](docs/ANALYSIS.md) - Comprehensive analysis of the original Python launcher, including features, APIs, data models, and migration notes

## Reference

The original Python launcher source (v1.7.12) is in `reference/` (gitignored). Use this for understanding original behavior when implementing features.

## Tech Stack

- **Language:** Rust (2024 edition)
- **Target:** Windows x64 only
- **GUI:** egui + eframe + egui_commonmark (markdown)
- **Async Runtime:** tokio
- **HTTP:** reqwest
- **Serialization:** serde, serde_json, toml
- **Database:** rusqlite (SQLite)
- **Archives:** zip, sevenz-rust, unrar crates (ZIP, 7z, RAR)
- **Images:** image crate (icon loading)
- **File ops:** remove_dir_all crate (fast directory deletion)
- **Browser:** open crate (open URLs in default browser)
- **Windows APIs:** windows crate

## Build & Run

```bash
cargo build
cargo run
```

## Project Structure

```
phoenix/
├── assets/
│   ├── icon.svg         # Phoenix flame icon source
│   ├── icon.png         # Embedded window icon
│   └── soundpacks.json  # Embedded soundpack repository
├── docs/
│   ├── PLAN.md          # Development roadmap
│   └── ANALYSIS.md      # Original launcher analysis
├── reference/           # Original Python source (gitignored)
├── src/
│   ├── main.rs          # Entry point, logging setup, icon loading
│   ├── app.rs           # Application state, UI, tab system
│   ├── backup.rs        # Save backup creation, restoration, auto-backup
│   ├── config.rs        # Configuration management (TOML)
│   ├── db.rs            # SQLite database for version caching
│   ├── game.rs          # Game detection and launching
│   ├── github.rs        # GitHub API client, release fetching
│   ├── migration.rs     # Smart migration for updates (identity-based content detection)
│   ├── soundpack.rs     # Soundpack management (download, install, enable/disable)
│   ├── theme.rs         # Theme system with color presets
│   └── update.rs        # Download, extract, backup, restore logic
├── Cargo.toml
└── CLAUDE.md
```

## Development Progress

See [docs/PLAN.md](docs/PLAN.md) for detailed spiral roadmap.

**Completed:**
- Spiral 1: Game Launching - browse, detect, launch game
- Spiral 2: Version Detection - SHA256 lookup, SQLite caching, VERSION.txt fallback
- Spiral 3: GitHub Integration - fetch releases, markdown changelog, rate limiting
- Spiral 3.5: Theme System - 5 color presets, improved UI layout, custom icon
- Spiral 4: Download & Update - progress tracking, smart migration, performance optimization
  - Update time reduced from ~54s to ~18s via deferred backup deletion
  - Background cleanup doesn't block user
- Spiral 5: Backup System - full backup management
  - Manual and automatic backups (before updates)
  - Backup list with 7-column metadata display
  - Restore with optional pre-restore backup
  - Configurable retention and compression
- Spiral 6: Soundpacks - full soundpack management
  - Two-column UI (installed vs repository)
  - Download and install from embedded repository
  - Enable/disable soundpacks
  - Delete with confirmation
  - Support for ZIP, RAR, 7z archives
  - Browser download fallback

**Next:** Spiral 7 - Polish

## Key External APIs

### GitHub API
- Game releases: `GET /repos/CleverRaven/Cataclysm-DDA/releases`
- Rate limit: 60 requests/hour (unauthenticated)

## Data Storage

- **Config:** `%APPDATA%\phoenix\Phoenix\config\config.toml`
- **Database:** `%APPDATA%\phoenix\Phoenix\data\phoenix.db` (SQLite, version cache)

```toml
[launcher]
theme = "Amber"  # Amber, Purple, Cyan, Green, Catppuccin
keep_open = false

[game]
directory = "C:\\Games\\CDDA"
branch = "experimental"

[updates]
check_on_startup = true
prevent_save_move = false      # Leave saves in place during updates
remove_previous_version = false # Auto-delete backup after update

[backups]
max_count = 6
compression_level = 6
backup_on_launch = false           # Auto-backup before game launch
backup_on_end = false              # Auto-backup after game closes
backup_before_update = true        # Auto-backup before updates
skip_backup_before_restore = false # Skip pre-restore backup
```

## Code Style

- Use `thiserror` for error types
- Use `tracing` for logging
- Prefer async/await for I/O operations
- Keep UI responsive - never block the main thread
