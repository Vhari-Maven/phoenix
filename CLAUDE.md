# Phoenix - CDDA Game Launcher

A fast, native game launcher for Cataclysm: Dark Days Ahead, built in Rust.

## Project Overview

This is a ground-up rebuild of the CDDA Game Launcher, aiming for sub-second startup times compared to the original Python/Qt launcher's 10-15 second load time.

## Documentation

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
│   └── ANALYSIS.md      # Original launcher analysis
├── reference/           # Original Python source (gitignored)
├── src/
│   ├── main.rs          # Entry point
│   ├── app.rs           # Application state and UI
│   ├── config.rs        # Configuration management
│   ├── github.rs        # GitHub API client
│   ├── game.rs          # Game detection and launching
│   └── ...
├── Cargo.toml
└── CLAUDE.md
```

## Development Phases

### Phase 1 - Core (MVP)
- [ ] Game directory selection
- [ ] Version detection (SHA256 of executable)
- [ ] Game launching
- [ ] Branch selection (stable/experimental)
- [ ] Fetch available builds from GitHub
- [ ] Download and extract game updates

### Phase 2 - Essential
- [ ] Settings persistence (TOML config)
- [ ] Backup creation and restoration
- [ ] Dark/light theme

### Phase 3 - Complete
- [ ] Soundpack management
- [ ] Multiple game directories
- [ ] Internationalization
- [ ] Self-update

## Key External APIs

### GitHub API
- Game releases: `GET /repos/CleverRaven/Cataclysm-DDA/releases`
- Rate limit: 60 requests/hour (unauthenticated)

## Configuration

Config stored at: `%APPDATA%\phoenix\config.toml`

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
