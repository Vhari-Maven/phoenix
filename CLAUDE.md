# Phoenix - CDDA Game Launcher

A fast, native game launcher for Cataclysm: Dark Days Ahead, built in Rust.

## Branch: refactor/maintainability

This branch focuses on improving code organization and maintainability after MVP completion. The primary goal is extracting UI code from the monolithic `app.rs` into separate modules.

## Documentation

- [docs/REFACTOR.md](docs/REFACTOR.md) - **Refactoring plan and progress** (start here)
- [docs/PLAN.md](docs/PLAN.md) - Original development plan with spiral roadmap
- [docs/ANALYSIS.md](docs/ANALYSIS.md) - Analysis of the original Python launcher

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

## Releases

Releases are automated via GitHub Actions. To create a new release:

```bash
git tag v0.2.0
git push origin v0.2.0
```

This triggers the workflow which:
1. Builds on Windows
2. Creates a zip with `phoenix.exe`, `README.md`, and `LICENSE`
3. Publishes to GitHub Releases

Tags containing `alpha`, `beta`, or `rc` are marked as prereleases.

## Project Structure

```
phoenix/
├── .github/
│   └── workflows/
│       └── release.yml  # Automated release builds on version tags
├── assets/
│   ├── icon.svg         # Phoenix flame icon source
│   ├── icon.png         # Embedded window icon
│   └── soundpacks.json  # Embedded soundpack repository
├── docs/
│   ├── REFACTOR.md      # Refactoring plan (this branch)
│   ├── PLAN.md          # Development roadmap
│   └── ANALYSIS.md      # Original launcher analysis
├── reference/           # Original Python source (gitignored)
├── src/
│   ├── main.rs          # Entry point, logging setup, icon loading
│   ├── app.rs           # Application state + coordination (~474 lines)
│   ├── state/           # Grouped state structs with poll methods
│   │   ├── mod.rs       #   StateEvent enum + module exports
│   │   ├── ui.rs        #   UiState, Tab enum
│   │   ├── backup.rs    #   BackupState + poll
│   │   ├── soundpack.rs #   SoundpackState + poll
│   │   ├── update.rs    #   UpdateState + poll
│   │   └── releases.rs  #   ReleasesState + poll
│   ├── ui/              # Extracted UI modules
│   │   ├── mod.rs       #   Module exports
│   │   ├── theme.rs     #   Theme system with color presets
│   │   ├── components.rs#   Shared UI components (tabs, dialogs, progress)
│   │   ├── main_tab.rs  #   Game info, updates, changelog (~490 lines)
│   │   ├── backups_tab.rs#  Backup list, create/restore/delete (~405 lines)
│   │   ├── soundpacks_tab.rs# Two-column layout, install/delete (~568 lines)
│   │   └── settings_tab.rs#  Appearance, behavior, game settings (~323 lines)
│   ├── task.rs          # Generic task polling helper
│   ├── util.rs          # Shared utilities (format_size)
│   ├── backup.rs        # Save backup creation, restoration, auto-backup
│   ├── config.rs        # Configuration management (TOML)
│   ├── db.rs            # SQLite database for version caching
│   ├── game.rs          # Game detection and launching
│   ├── github.rs        # GitHub API client, release fetching
│   ├── migration.rs     # Smart migration for updates
│   ├── soundpack.rs     # Soundpack management
│   └── update.rs        # Download, extract, backup, restore logic
├── build.rs             # Windows resource embedding (icon, version info)
├── Cargo.toml
├── CLAUDE.md
├── README.md
└── LICENSE
```

## Refactoring Progress

See [docs/REFACTOR.md](docs/REFACTOR.md) for the full refactoring plan.

**Completed:**
- Build warnings cleanup (27 warnings eliminated)
- Executable icon embedding via build.rs
- Console window hidden in release builds
- Phase 1: UI Module Extraction (app.rs reduced from ~2,700 to ~1,165 lines)
- Phase 1.5: UI Components & Task Helper (app.rs reduced to ~988 lines)
- Phase 2: format_size() deduplication into util.rs
- Phase 2.5: Progress rendering deduplication into ui/components.rs
- Phase 3: State Struct Extraction (app.rs reduced to ~474 lines)
- Module Reorganization (theme.rs moved to ui/)

**Planned:**
- Phase 4: Service Abstraction (optional)

## MVP Status

The MVP is complete on the `main` branch. All core features work:
- Game detection, launching, version identification
- Update downloads with smart migration
- Backup/restore system
- Soundpack management
- Theme system

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

## Refactoring Guidelines

- **Run tests after each change:** `cargo test`
- **Keep commits small and focused** - one logical change per commit
- **Don't change behavior** - refactoring should be purely structural
- **Preserve public APIs** - PhoenixApp's interface should remain stable
- **Extract, don't rewrite** - move code as-is first, then clean up
