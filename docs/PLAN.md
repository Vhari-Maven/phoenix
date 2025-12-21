# Phoenix Development Plan

## Architecture Overview

### Layer Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      UI Layer                           │
│  (egui views, user interaction, display state)          │
├─────────────────────────────────────────────────────────┤
│                   Application Layer                     │
│  (PhoenixApp state, coordination, business logic)       │
├─────────────────────────────────────────────────────────┤
│                    Service Layer                        │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌───────────────┐  │
│  │ GitHub  │ │  Game   │ │ Backup  │ │  Soundpacks   │  │
│  │ Client  │ │ Manager │ │ Manager │ │   Manager     │  │
│  └─────────┘ └─────────┘ └─────────┘ └───────────────┘  │
├─────────────────────────────────────────────────────────┤
│                  Infrastructure Layer                   │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌───────────────┐  │
│  │ Config  │ │  HTTP   │ │  File   │ │   Archive     │  │
│  │ (TOML)  │ │(reqwest)│ │   I/O   │ │  (ZIP/7z)     │  │
│  └─────────┘ └─────────┘ └─────────┘ └───────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Module Structure

```
src/
├── main.rs              # Entry point, logging setup, eframe launch
├── app.rs               # PhoenixApp struct, top-level UI coordination
├── config.rs            # Config struct, TOML load/save
├── ui/
│   ├── mod.rs           # UI module exports
│   ├── main_tab.rs      # Game directory, version info, launch
│   ├── update_tab.rs    # Branch selection, releases, download
│   ├── backups_tab.rs   # Backup list, create/restore
│   ├── soundpacks_tab.rs# Soundpack management
│   └── settings_tab.rs  # Launcher settings
├── services/
│   ├── mod.rs           # Service module exports
│   ├── github.rs        # GitHub API client (releases, downloads)
│   ├── game.rs          # Game detection, launching, version info
│   ├── backup.rs        # Backup creation/restoration
│   └── soundpacks.rs    # Soundpack discovery/installation
└── utils/
    ├── mod.rs           # Utility exports
    ├── archive.rs       # ZIP extraction
    └── paths.rs         # Platform-specific paths
```

### Key Design Decisions

1. **State Management**: Single `PhoenixApp` struct owns all state; UI components borrow from it
2. **Async Strategy**: Use tokio for I/O; communicate with UI via channels or polling
3. **Error Handling**: `anyhow` for application errors, `thiserror` for library-style errors
4. **Database**: SQLite for caching version hashes (many experimental builds); TOML for user config

---

## Development Spirals

### Spiral 1: Game Launching (Foundation) ✅ COMPLETE
**Goal:** Browse for game directory, launch the game

**Tasks:**
- [x] Add `rfd` crate for native file dialogs
- [x] Wire Browse button to directory picker
- [x] Store selected directory in app state
- [x] Validate directory contains game executable
- [x] Wire Launch button to spawn game process
- [x] Display basic game info (path, executable found)
- [x] Save/load config persistence

**Files modified:**
- `Cargo.toml` - added rfd dependency
- `src/app.rs` - directory state, button handlers, config save
- `src/game.rs` - detect_game and launch_game functions

**Rust concepts learned:** Option types, Result handling, std::process::Command, file dialogs

---

### Spiral 2: Game Detection ✅ COMPLETE
**Goal:** Detect game version, display detailed info

**Tasks:**
- [x] Calculate SHA256 of game executable (already implemented in game.rs)
- [x] Show save directory size (already implemented)
- [x] Persist selected directory to config (done in Spiral 1)
- [x] Load directory from config on startup (done in Spiral 1)
- [x] Display version/build info from SHA256 hash lookup
- [x] Create SQLite database for caching version hashes

**Files modified:**
- `Cargo.toml` - added rusqlite dependency
- `src/db.rs` - new database module with schema and version lookup
- `src/game.rs` - integrated database lookup, added VERSION.txt fallback
- `src/app.rs` - display version info with stable/experimental indicator

**Rust concepts:** SQLite with rusqlite, OnceLock for static data, HashMap

---

### Spiral 3: GitHub Integration ✅ COMPLETE
**Goal:** Fetch and display available game releases

**Tasks:**
- [x] Implement async GitHub API calls
- [x] Parse releases JSON into structs
- [x] Filter by branch (stable/experimental)
- [x] Display release list in dropdown
- [x] Show changelog for selected release (with markdown rendering)
- [x] Auto-fetch releases on startup
- [x] Auto-select latest release
- [x] Show update status indicator (compare installed vs selected)
- [x] Clickable links in changelog
- [x] Handle rate limiting gracefully (warning when ≤10 requests remaining)

**Files modified:**
- `src/github.rs` - GitHubClient with stable/experimental fetch methods, RateLimitInfo
- `src/app.rs` - release state, async polling, markdown changelog, update indicator, rate limit warning
- `Cargo.toml` - added egui_commonmark, futures

**Rust concepts:** async/await, reqwest, JSON deserialization, tokio runtime, egui_commonmark

---

### Spiral 3.5: Theme System & UX Polish ✅ COMPLETE
**Goal:** Add theme customization and improve UI appearance

**Tasks:**
- [x] Create Theme struct with color definitions
- [x] Add 5 built-in theme presets (Amber, Purple, Cyan, Green, Catppuccin)
- [x] Store theme choice in config
- [x] Apply theme colors to egui Visuals
- [x] Add theme dropdown in Settings tab with color preview swatches
- [x] Implement proper tab navigation
- [x] Add visual grouping with frames/cards for sections
- [x] Richer game info display (version, executable, saves in columns)
- [x] Prominent Launch button with accent color
- [x] Contextual Update button (changes when update available)
- [x] Changelog improvements (date header, better scrolling)
- [x] Settings tab with Appearance, Behavior, and Game sections
- [x] Custom application icon (flame/phoenix SVG converted to PNG)

**Files modified:**
- `src/theme.rs` - new Theme struct, ThemePreset enum, 5 color schemes
- `src/config.rs` - replaced dark_theme bool with ThemePreset
- `src/app.rs` - themed UI, tab system, section frames, improved layout
- `src/main.rs` - added theme module, custom icon loading
- `assets/icon.svg` - phoenix flame icon source
- `assets/icon.png` - embedded icon for window
- `Cargo.toml` - added image crate for icon loading

**Rust concepts:** egui Visuals customization, Color32, RichText styling, include_bytes! macro, image crate

---

### Spiral 4: Download & Update ✅ COMPLETE
**Goal:** Download and install game updates with smart migration

**Phase 1 - Core Download/Update ✅ COMPLETE:**
- [x] Download release ZIP with progress tracking (bytes/sec, progress bar)
- [x] Stream download to disk (not memory) with .part temp file
- [x] Extract ZIP to game directory preserving structure
- [x] Backup current installation to `previous_version/` before update
- [x] Restore user directories (save, config, mods, templates, memorial, graveyard, font)
- [x] Show download/extract progress in UI with phase indicators
- [x] Install button for fresh installs (no existing game)
- [x] Update button with precise build number comparison (not just date)
- [x] Handle new asset naming convention (with-graphics vs tiles)

**Phase 2 - Smart Migration ✅ COMPLETE:**
- [x] Smart mod restoration - only restore custom mods, not official ones
  - Parse `modinfo.json` to get mod ident
  - Compare old vs new `data/mods/` directories
  - Only copy mods not present in new version
- [x] Smart tileset restoration - only restore custom tilesets
  - Parse `tileset.txt` to get tileset name
  - Compare old vs new `gfx/` directories
  - Only copy tilesets not present in new version
- [x] Smart soundpack restoration - only restore custom soundpacks
  - Parse soundpack metadata
  - Compare old vs new `data/sound/` directories
- [x] Smart font restoration - only restore fonts not in new version
  - Compare `font/` and `data/font/` directories
  - Only copy fonts that don't exist in new version
- [x] Add `prevent_save_move` config option (leave saves in place)
- [x] Skip debug.log files during config restore
- [x] Option to auto-delete `previous_version/` after successful update

**Phase 3 - Performance Optimization ✅ COMPLETE:**
- [x] Deferred backup deletion - rename old backup instead of deleting (37s → 0s)
  - Rename `previous_version` to `previous_version_old` (instant)
  - Delete old backup in background after update completes
  - Self-healing: stale directories cleaned on next update
- [x] Background cleanup with `remove_dir_all` crate
- [x] Fix `prevent_save_move` to skip saves during backup phase (was only skipping restore)
- [x] Parallel stable release fetching
- [x] Cache game directory size calculations
- [x] Total update time reduced from ~54s to ~18s

**Files modified:**
- `src/migration.rs` - new module with identity-based content detection and smart migration
- `src/update.rs` - download, extract, backup, smart restore logic, deferred deletion
- `src/github.rs` - added `find_windows_asset()`, exposed HTTP client, parallel stable fetching
- `src/app.rs` - update state, progress polling, Install/Update button logic, Settings UI
- `src/game.rs` - store full build number for precise version comparison, cached dir size
- `src/db.rs` - parallel version lookups
- `src/config.rs` - added `prevent_save_move`, `remove_previous_version` options
- `src/main.rs` - added update and migration modules
- `Cargo.toml` - added remove_dir_all crate for faster directory deletion

**Rust concepts:** tokio::sync::watch channels, streaming downloads, spawn_blocking for sync ZIP ops, async file I/O, identity-based set difference for custom content detection, deferred deletion pattern, background task cleanup

---

### Spiral 5: Backup System ✅ COMPLETE
**Goal:** Create and restore save backups

**Tasks:**
- [x] List existing backups with metadata (7 columns: name, date, worlds, chars, size, uncompressed, ratio)
- [x] Create manual backup (user-named, compress saves to ZIP)
- [x] Restore backup (extract, optional pre-restore backup)
- [x] Delete backup with confirmation
- [x] Auto-backup before updates (creates `auto_before_update_{version}`)
- [x] Configurable backup retention (max auto-backups to keep)
- [x] Backup settings in Settings tab:
  - Auto-backup before update toggle (default on)
  - Auto-backup before launch toggle
  - Skip backup when restoring toggle
  - Max auto-backups count
  - Compression level slider (0-9)

**Files modified:**
- `src/backup.rs` - new module with backup operations (create, list, restore, delete, auto-backup, retention)
- `src/config.rs` - expanded BackupConfig with auto-backup settings
- `src/app.rs` - backup state, Backups tab UI, Settings backup section, update integration
- `src/main.rs` - added backup module
- `Cargo.toml` - added walkdir crate for directory traversal

**Rust concepts:** walkdir for recursive traversal, ZipWriter for compression, watch channels for async progress, spawn_blocking for I/O

---

### Spiral 6: Soundpacks
**Goal:** Manage soundpack installation

**Tasks:**
- [ ] List installed soundpacks
- [ ] Fetch available soundpacks from repository
- [ ] Download and install soundpacks
- [ ] Remove soundpacks

**Files to modify:**
- `src/ui/soundpacks_tab.rs` - new file
- `src/services/soundpacks.rs` - new file

---

### Spiral 7: Polish
**Goal:** Final refinements

**Tasks:**
- [ ] Save/restore window size and position
- [ ] Error dialogs and user feedback
- [ ] Single instance enforcement
- [ ] About dialog
- [ ] Performance optimization
- [ ] Testing

---

## Current Status

**Completed:** Spiral 1 ✅, Spiral 2 ✅, Spiral 3 ✅, Spiral 3.5 ✅, Spiral 4 ✅, Spiral 5 ✅
**Next:** Spiral 6 - Soundpacks
