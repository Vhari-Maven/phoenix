# Phoenix - CDDA Game Launcher

A fast, native game launcher for Cataclysm: Dark Days Ahead, built in Rust.

## Tech Stack

- **Language:** Rust (2024 edition)
- **Target:** Windows x64 only
- **GUI:** egui + eframe + egui_commonmark (markdown)
- **Async Runtime:** tokio
- **HTTP:** reqwest
- **Serialization:** serde, serde_json, toml
- **Database:** rusqlite (SQLite)
- **Archives:** zip crate (ZIP only)
- **Images:** image crate (icon loading)
- **File ops:** remove_dir_all crate (fast directory deletion)
- **Browser:** open crate (open URLs in default browser)
- **Windows APIs:** windows crate
- **CLI:** clap (derive macros), rustyline (interactive shell)

## Build & Run

```bash
cargo build
cargo run
```

## CLI Usage

Running with no arguments opens the GUI. Any subcommand runs in CLI mode:

```bash
cargo run -- diag check              # Verify installation health
cargo run -- game detect             # Detect game version
cargo run -- game export             # Export user data for external builds
cargo run -- config show             # Show current configuration
cargo run -- backup list --json      # List backups as JSON
cargo run -- update check            # Check for updates
cargo run -- shell                   # Interactive shell with history/completion
```

Use `--json` for machine-readable output, `--quiet` to suppress progress.

The interactive shell (`cargo run -- shell`) provides tab completion, command history, and a REPL for running multiple commands without restarting.

## Releases

Releases are automated via GitHub Actions. To create a new release:

```bash
git tag -a v0.2.0 -m "Brief description of the release"
git push origin v0.2.0
```

Use annotated tags (`-a`) with a message (`-m`) for releases. This records who created the tag and when, and the message appears on the GitHub Releases page.

This triggers the workflow which:
1. Builds on Windows
2. Creates a zip with `phoenix.exe`, `README.md`, and `LICENSE`
3. Publishes to GitHub Releases

Tags containing `alpha`, `beta`, or `rc` are marked as prereleases.

## Features

- Game detection, launching, version identification
- Update downloads with smart migration (preserves custom mods, tilesets, soundpacks)
- Backup/restore system with compression
- Soundpack management (install from repository or local files)
- Theme system (Amber, Purple, Cyan, Green, Catppuccin)
- CLI mode for scripting and automation (all features available via command line)

## Architecture

```
src/
├── main.rs              # Entry point, logging, icon loading
├── app.rs               # PhoenixApp coordination
├── state/               # Grouped state structs with poll methods
│   ├── mod.rs           # StateEvent enum + module exports
│   ├── ui.rs            # UiState, Tab enum
│   ├── backup.rs        # BackupState + poll
│   ├── soundpack.rs     # SoundpackState + poll
│   ├── update.rs        # UpdateState + poll
│   └── releases.rs      # ReleasesState + poll
├── ui/                  # UI rendering modules
│   ├── mod.rs           # Module exports
│   ├── theme.rs         # Theme system (colors, presets)
│   ├── components.rs    # Shared UI components (tabs, dialogs, progress)
│   ├── main_tab.rs      # Game info, updates, changelog
│   ├── backups_tab.rs   # Backup management
│   ├── soundpacks_tab.rs# Soundpack management
│   └── settings_tab.rs  # Settings
├── cli/                 # CLI interface (clap-based)
│   ├── mod.rs           # CLI argument definitions
│   ├── output.rs        # Output formatting (text/JSON)
│   └── commands/        # Command implementations
│       ├── game.rs      # game detect|launch|info
│       ├── backup.rs    # backup list|create|restore|delete|verify
│       ├── update.rs    # update check|releases|download|install|apply
│       ├── soundpack.rs # soundpack list|available|install|delete|enable|disable
│       ├── config.rs    # config show|get|set|path
│       └── diag.rs      # diag paths|check|clear-cache
├── task.rs              # Generic task polling helper
├── util.rs              # Shared utilities (format_size)
├── app_data.rs          # Compile-time embedded data (TOML/JSON configs)
├── backup.rs            # Backup service (create, restore, delete)
├── config.rs            # User configuration (TOML) and data directories
├── db.rs                # SQLite cache for version hashes
├── game.rs              # Game detection and launching
├── github.rs            # GitHub API client
├── legacy.rs            # One-time migration of old data locations
├── migration.rs         # Smart migration for updates (mods, tilesets, etc.)
├── soundpack.rs         # Soundpack service
└── update/              # Update download and installation
    ├── mod.rs           # Types (UpdatePhase, UpdateProgress), re-exports
    ├── access.rs        # Pre-flight checks (locked files, game running)
    ├── download.rs      # Download with progress tracking
    └── install.rs       # Archive, extract, restore, rollback
```

**Key patterns:**
- State structs (`BackupState`, `UpdateState`, etc.) own async task handles and progress channels
- Poll methods return `Vec<StateEvent>` for cross-cutting concerns (status messages, logging)
- UI modules are free functions that borrow `&mut PhoenixApp`
- Config values passed as parameters rather than stored in state structs

## Project Structure

```
phoenix/
├── .github/
│   └── workflows/
│       └── release.yml  # Automated release builds on version tags
├── assets/
│   ├── icon.svg         # Phoenix flame icon source
│   └── icon.png         # Embedded window icon
├── embedded/            # Compile-time embedded data (loaded via app_data.rs)
│   ├── game_config.toml      # CDDA-specific paths and detection
│   ├── launcher_config.toml  # Application settings and URLs
│   ├── migration_config.toml # Update and migration behavior
│   ├── stable_releases.toml  # Known stable releases with SHA256 hashes
│   └── soundpacks.json       # Soundpack repository
├── docs/
│   ├── ARCHITECTURE.md  # In-depth architecture guide for developers
│   └── TODO.md          # Bug tracking and feature ideas
├── reference/           # Original Python source (gitignored)
├── src/                 # Source code (see Architecture above)
├── build.rs             # Windows resource embedding (icon, version info)
├── Cargo.toml
├── CLAUDE.md
├── README.md
└── LICENSE
```

## Key External APIs

### GitHub API
- Game releases: `GET /repos/CleverRaven/Cataclysm-DDA/releases`
- Rate limit: 60 requests/hour (unauthenticated)

## Data Storage

**AppData locations:**
- **Config:** `%APPDATA%\phoenix\Phoenix\config\config.toml`
- **Database:** `%APPDATA%\phoenix\Phoenix\data\phoenix.db` (SQLite, version cache)
- **Backups:** `%APPDATA%\phoenix\Phoenix\data\backups\` (compressed save archives)

**Game folder:**
- **Installation archive:** `.phoenix_archive/` (previous version for rollback after updates)

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
- Run `cargo test` after changes
