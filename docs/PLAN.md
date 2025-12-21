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
4. **No Database**: Simple TOML config file instead of SQLite

---

## Development Spirals

### Spiral 1: Game Launching (Foundation)
**Goal:** Browse for game directory, launch the game

**Tasks:**
- [ ] Add `rfd` crate for native file dialogs
- [ ] Wire Browse button to directory picker
- [ ] Store selected directory in app state
- [ ] Validate directory contains game executable
- [ ] Wire Launch button to spawn game process
- [ ] Display basic game info (path, executable found)

**Files to modify:**
- `Cargo.toml` - add rfd dependency
- `src/app.rs` - add directory state, button handlers
- `src/game.rs` - use detect_game and launch_game functions

**Rust concepts:** Option types, Result handling, std::process::Command

---

### Spiral 2: Game Detection
**Goal:** Detect game version, display detailed info

**Tasks:**
- [ ] Calculate SHA256 of game executable
- [ ] Display version/build info (or "Unknown" if not in database)
- [ ] Show save directory size
- [ ] Persist selected directory to config
- [ ] Load directory from config on startup

**Files to modify:**
- `src/game.rs` - flesh out detection logic
- `src/config.rs` - implement save/load
- `src/app.rs` - integrate config persistence

**Rust concepts:** File I/O, hashing, serde serialization

---

### Spiral 3: GitHub Integration
**Goal:** Fetch and display available game releases

**Tasks:**
- [ ] Implement async GitHub API calls
- [ ] Parse releases JSON into structs
- [ ] Filter by branch (stable/experimental)
- [ ] Display release list in dropdown
- [ ] Show changelog for selected release
- [ ] Handle rate limiting gracefully

**Files to modify:**
- `src/github.rs` - complete the client implementation
- `src/app.rs` - add release state, async handling

**Rust concepts:** async/await, reqwest, JSON deserialization, tokio runtime

---

### Spiral 4: Download & Update
**Goal:** Download and install game updates

**Tasks:**
- [ ] Download release ZIP with progress tracking
- [ ] Extract ZIP to game directory
- [ ] Handle existing installation (backup saves first?)
- [ ] Show download/extract progress in UI
- [ ] Verify download integrity

**Files to modify:**
- `src/github.rs` - add download functionality
- `src/utils/archive.rs` - ZIP extraction
- `src/app.rs` - progress UI state

**Rust concepts:** Streams, progress callbacks, ZIP handling

---

### Spiral 5: Settings & Persistence
**Goal:** Full settings UI, window state persistence

**Tasks:**
- [ ] Implement Settings tab UI
- [ ] Dark/light theme toggle
- [ ] Keep launcher open setting
- [ ] Custom launch parameters
- [ ] Save/restore window size and position

**Files to modify:**
- `src/ui/settings_tab.rs` - new file
- `src/config.rs` - add all settings fields
- `src/app.rs` - apply settings

**Rust concepts:** More serde, egui styling

---

### Spiral 6: Backup System
**Goal:** Create and restore save backups

**Tasks:**
- [ ] List existing backups
- [ ] Create backup (compress saves to ZIP)
- [ ] Restore backup (extract, handle conflicts)
- [ ] Auto-backup before updates
- [ ] Configurable backup retention

**Files to modify:**
- `src/ui/backups_tab.rs` - new file
- `src/services/backup.rs` - new file
- `src/utils/archive.rs` - add compression

**Rust concepts:** Directory traversal, ZIP creation, timestamps

---

### Spiral 7: Soundpacks
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

### Spiral 8: Polish
**Goal:** Final refinements

**Tasks:**
- [ ] Error dialogs and user feedback
- [ ] Single instance enforcement
- [ ] About dialog
- [ ] Performance optimization
- [ ] Testing

---

## Current Status

**Active:** Spiral 1 - Game Launching
