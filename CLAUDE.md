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
- **GUI:** egui + eframe
- **Async Runtime:** tokio
- **HTTP:** reqwest
- **Serialization:** serde, serde_json, toml
- **Archives:** zip crate
- **Windows APIs:** windows crate

## Build & Run

```bash
cargo build
cargo run
```

## Project Structure

```
phoenix/
├── docs/
│   ├── PLAN.md          # Development roadmap
│   └── ANALYSIS.md      # Original launcher analysis
├── reference/           # Original Python source (gitignored)
├── src/
│   ├── main.rs          # Entry point, logging setup
│   ├── app.rs           # Application state and UI
│   ├── config.rs        # Configuration management (TOML)
│   ├── db.rs            # SQLite database for version caching
│   ├── github.rs        # GitHub API client
│   └── game.rs          # Game detection and launching
├── Cargo.toml
└── CLAUDE.md
```

## Development Progress

See [docs/PLAN.md](docs/PLAN.md) for detailed spiral roadmap.

**Completed:**
- Spiral 1: Game Launching - browse, detect, launch game
- Spiral 2: Version Detection - SHA256 lookup, SQLite caching, VERSION.txt fallback

**Next:** Spiral 3 - GitHub Integration (fetch releases, display changelog)

## Key External APIs

### GitHub API
- Game releases: `GET /repos/CleverRaven/Cataclysm-DDA/releases`
- Rate limit: 60 requests/hour (unauthenticated)

## Data Storage

- **Config:** `%APPDATA%\phoenix\Phoenix\config\config.toml`
- **Database:** `%APPDATA%\phoenix\Phoenix\data\phoenix.db` (SQLite, version cache)

```toml
[launcher]
dark_theme = true
keep_open = false

[game]
directory = "C:\\Games\\CDDA"
branch = "experimental"

[updates]
check_on_startup = true
```

## Code Style

- Use `thiserror` for error types
- Use `tracing` for logging
- Prefer async/await for I/O operations
- Keep UI responsive - never block the main thread
