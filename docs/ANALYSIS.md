# CDDA Game Launcher Analysis

This document analyzes the Python-based CDDA Game Launcher (v1.7.12) to guide a Rust rebuild focused on performance and maintainability.

## Table of Contents

1. [Overview](#overview)
2. [Application Flow](#application-flow)
3. [Features](#features)
4. [External APIs](#external-apis)
5. [Data Models](#data-models)
6. [UI Structure](#ui-structure)
7. [Windows Integration](#windows-integration)
8. [Migration Notes](#migration-notes)

---

## Overview

The CDDA Game Launcher is a desktop application for managing Cataclysm: Dark Days Ahead installations on Windows. It handles game updates, save backups, soundpack management, and configuration.

### Original Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Python 3 |
| GUI | PySide6 (Qt 6) |
| Database | SQLite via SQLAlchemy |
| Migrations | Alembic |
| HTTP | QNetworkAccessManager, requests |
| Archives | zipfile, py7zlib, rarfile |
| i18n | gettext, Babel |

### Key Source Files

```
cddagl/
├── __main__.py      # Entry point, pywin32 fix
├── launcher.py      # App initialization, logging, single-instance
├── constants.py     # URLs, stable versions, file patterns
├── functions.py     # Utility functions
├── i18n.py          # Internationalization
├── win32.py         # Windows API integration
├── sql/
│   ├── model.py     # SQLAlchemy models
│   └── functions.py # DB session management
└── ui/views/
    ├── main.py      # Main tab (game dir, updates)
    ├── backups.py   # Backup management tab
    ├── soundpacks.py # Soundpack installation tab
    ├── settings.py  # Settings tab
    ├── tabbed.py    # Main window container
    └── dialogs.py   # About, FAQ, license dialogs
```

---

## Application Flow

### Startup Sequence

1. **Entry** (`__main__.py`)
   - Fix pywin32 DLL loading path issues
   - Call `launcher.run_cddagl()`

2. **Initialization** (`launcher.py`)
   - Initialize logging (rotating file handler, 1MB max, 5 backups)
   - Set up global exception handler
   - Initialize SQLite database with Alembic migrations
   - Detect locale (user preference → system → English fallback)
   - Enforce single-instance via Windows named pipes

3. **UI Launch**
   - Create `QApplication`
   - Instantiate `TabbedWindow` (main window)
   - Apply dark theme stylesheet if enabled
   - Enter Qt event loop

### Shutdown Sequence

1. Save window geometry to database
2. Close database connections
3. Exit application

---

## Features

### 1. Game Directory Management

**Location:** `ui/views/main.py` → `GameDirGroupBox`

- Select and manage up to 6 game directories
- Select session directory (save location) - up to 20
- Display installed version via SHA256 hash lookup
- Show save game size with warning at 150MB
- Launch game with custom command-line parameters
- Restore previous game versions

**Game Detection:**
- Looks for `cataclysm-tiles.exe` or `cataclysm.exe`
- Computes SHA256 of executable to identify version
- Checks for world files: `worldoptions.json`, `worldoptions.txt`, `master.gsav`

### 2. Update Management

**Location:** `ui/views/main.py` → `UpdateGroupBox`

- Branch selection: Stable or Experimental
- Fetch available builds from GitHub API
- Display markdown changelogs
- Download builds with progress tracking
- Extract ZIP archives with progress
- Verify downloads via SHA256

**Predefined Stable Versions** (from `constants.py`):
- 0.G Gaiman
- 0.F-3, 0.F-2, 0.F-1, 0.F Frank
- 0.E-3, 0.E-2, 0.E-1, 0.E Ellison
- 0.D Danny
- 0.C Cooper

Each includes direct download URLs for x64 and x86 Windows builds.

### 3. Backup Management

**Location:** `ui/views/backups.py` → `BackupsTab`

- List existing backups with metadata (size, compression ratio, save count)
- Create manual backups with custom names
- Auto-backup before game updates
- Restore backups with file lock detection
- Delete old backups (configurable retention)
- Compression levels configurable

**Backup Format:** ZIP archives containing save directories

**Threading:**
- `CompressThread` - Multi-threaded backup creation
- `ExtractingThread` - Async extraction
- `WaitingThread` - Monitors file handles to detect game access

### 4. Soundpack Management

**Location:** `ui/views/soundpacks.py` → `SoundpacksTab`

- Fetch soundpack list from online repository
- Display installed soundpacks
- Download and extract soundpack archives (ZIP, 7Z, RAR)
- Show download/extraction progress

**Soundpack Repository:** Defined in `data/soundpacks.json`

### 5. Settings

**Location:** `ui/views/settings.py` → `SettingsTab`

**Launcher Settings:**
- Custom command-line parameters for game
- Keep launcher open after game closes
- Dark theme toggle
- Prevent version check on startup
- Allow multiple launcher instances
- Permanently delete files (vs. recycle bin)
- Language/locale selection

**Update Settings:**
- Max concurrent downloads (1-8, default 4)
- Notification on update availability

---

## External APIs

### GitHub API

**Base URL:** `https://api.github.com`

#### Endpoints Used

| Purpose | Endpoint |
|---------|----------|
| Launcher releases | `/repos/Fris0uman/CDDA-Game-Launcher/releases/latest` |
| Game releases | `/repos/CleverRaven/Cataclysm-DDA/releases` |
| Game tags | `/repos/CleverRaven/Cataclysm-DDA/tags` |

#### Request Headers

```
User-Agent: CDDA-Game-Launcher/{version}
Accept: application/vnd.github.v3+json
```

#### Rate Limiting

- Respects `X-RateLimit-Remaining` and `X-RateLimit-Reset` headers
- Unauthenticated limit: 60 requests/hour

#### Response Parsing

Releases response includes:
- `tag_name` - Version tag
- `name` - Release name
- `body` - Changelog (markdown)
- `assets[]` - Download files with `browser_download_url`
- `published_at` - Release date

### Direct Downloads

- Game builds: GitHub release assets (ZIP files)
- Soundpacks: Various URLs defined in soundpacks.json

---

## Data Models

### SQLite Database

**Location:** `%LOCALAPPDATA%\CDDA Game Launcher\configs.db`

#### ConfigValue Table

Key-value storage for all settings.

| Key | Type | Description |
|-----|------|-------------|
| `game_directories` | JSON array | List of game install paths |
| `session_directories` | JSON array | List of save directories |
| `window_geometry` | Base64 | Window position/size state |
| `keep_launcher_open` | string bool | Keep open after game launch |
| `dark_theme` | string bool | Enable dark theme |
| `branch` | string | "stable" or "experimental" |
| `command.params` | string | Game command-line args |
| `prevent_version_check_launch` | string bool | Skip update check |
| `allow_multiple_instances` | string bool | Multi-instance mode |
| `permanently_delete_files` | string bool | Skip recycle bin |
| `max_concurrent_downloads` | string int | Download thread limit |
| `locale` | string | UI language code |

#### GameVersion Table

| Column | Type | Description |
|--------|------|-------------|
| sha256 | String(64) PK | Executable hash |
| version | String | Version name (e.g., "0.F-3") |
| stable | Boolean | Is stable release |
| discovered_on | DateTime | First seen timestamp |

#### GameBuild Table

| Column | Type | Description |
|--------|------|-------------|
| id | Integer PK | Auto-increment ID |
| version | FK → GameVersion | Parent version |
| build | String | Build number |
| released_on | DateTime | Release date |
| discovered_on | DateTime | First discovered |

### Configuration File Alternative

For Rust rebuild, consider simpler JSON/TOML config:

```toml
[launcher]
dark_theme = true
keep_open = false
locale = "en"

[game]
directories = ["C:\\Games\\CDDA"]
session_directory = "C:\\Games\\CDDA\\save"
branch = "experimental"
command_params = ""

[updates]
max_concurrent_downloads = 4
check_on_startup = true

[backups]
max_count = 10
compression_level = 6
```

---

## UI Structure

### Main Window Hierarchy

```
TabbedWindow (QMainWindow)
├── Menu Bar
│   ├── File → Exit
│   └── Help → FAQ, Report Issue, Check Update, About, Licenses
├── Status Bar (busy indicator)
└── CentralWidget (QTabWidget)
    ├── Main Tab
    │   ├── Game Directory Group
    │   │   ├── Directory selector (dropdown + browse)
    │   │   ├── Version display (read-only)
    │   │   ├── Build display (read-only)
    │   │   ├── Save size display
    │   │   ├── Launch button
    │   │   └── Restore button
    │   └── Update Group
    │       ├── Branch selector (Stable/Experimental)
    │       ├── Build dropdown
    │       ├── Build search input
    │       ├── Changelog viewer (HTML/Markdown)
    │       ├── Progress bar
    │       └── Update button
    ├── Backups Tab
    │   ├── Backup list table
    │   ├── Backup/Restore/Delete buttons
    │   └── Settings (retention, compression)
    ├── Soundpacks Tab
    │   ├── Installed list
    │   ├── Available list
    │   └── Install/Remove buttons
    └── Settings Tab
        ├── Launcher settings group
        └── Update settings group
```

### Dialogs

- **AboutDialog** - Version, credits, links
- **FaqDialog** - Embedded FAQ content
- **LicenseDialog** - Third-party licenses
- **LauncherUpdateDialog** - Self-update progress
- **BrowserDownloadDialog** - Manual download fallback

---

## Windows Integration

**Location:** `win32.py`

### Functions

| Function | Purpose |
|----------|---------|
| `get_ui_locale()` | Get Windows UI language |
| `SingleInstance` | Prevent multiple instances via mutex |
| `SimpleNamedPipe` | IPC for instance communication |
| `find_process_with_file_handle()` | Find process locking a file |
| `activate_window()` | Bring window to foreground |
| `process_id_from_path()` | Get PID from executable path |
| `wait_for_pid()` | Block until process exits |
| `get_documents_directory()` | Get user Documents path |

### Shell Operations

Uses Windows Shell API via `pythoncom` for:
- File deletion (with recycle bin support)
- File/folder moves
- Progress dialogs for long operations

---

## Migration Notes

### Recommended Rust Stack

| Component | Python Original | Rust Replacement |
|-----------|-----------------|------------------|
| GUI | PySide6 (Qt) | `egui` + `eframe` or `iced` |
| HTTP Client | QNetworkAccessManager | `reqwest` (async) |
| JSON | json module | `serde_json` |
| Database | SQLAlchemy + SQLite | `rusqlite` or JSON config |
| Archives - ZIP | zipfile | `zip` crate |
| Archives - 7Z | py7zlib | `sevenz-rust` |
| Logging | logging module | `tracing` or `log` + `env_logger` |
| Windows APIs | pywin32 | `windows` crate |
| Async Runtime | QThread | `tokio` |
| i18n | gettext | `fluent` or `rust-i18n` |

### Architecture Simplifications

1. **Config Storage**: Replace SQLAlchemy + Alembic with simple JSON or TOML file
   - Eliminates database migration complexity
   - Faster startup (no SQLite initialization)
   - Human-readable config

2. **Single Instance**: Use `single-instance` crate or Windows mutex directly

3. **Archive Support**: Consider dropping RAR support initially (requires external binary)

4. **Async Model**: Use `tokio` for all I/O operations, keep UI responsive

### Feature Priority for MVP

**Phase 1 - Core:**
- [ ] Game directory selection
- [ ] Version detection (SHA256)
- [ ] Game launching
- [ ] Branch selection (stable/experimental)
- [ ] Fetch and display available builds
- [ ] Download and extract game updates

**Phase 2 - Essential:**
- [ ] Settings persistence
- [ ] Backup creation
- [ ] Backup restoration
- [ ] Dark theme

**Phase 3 - Complete:**
- [ ] Soundpack management
- [ ] Multiple game directories
- [ ] Internationalization
- [ ] Self-update

### Performance Targets

| Metric | Python Original | Rust Target |
|--------|-----------------|-------------|
| Cold startup | 10-15 seconds | < 1 second |
| Memory usage | ~100-150 MB | < 50 MB |
| Binary size | ~50 MB (bundled) | < 20 MB |

### Key Rust Crates

```toml
[dependencies]
# GUI
eframe = "0.28"
egui = "0.28"

# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP
reqwest = { version = "0.12", features = ["json"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Archives
zip = "2"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Windows
[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = ["Win32_Foundation", "Win32_UI_WindowsAndMessaging"] }

# Utilities
sha2 = "0.10"           # SHA256 hashing
directories = "5"        # Platform directories
chrono = "0.4"          # Date/time
```

---

## Appendix

### GitHub API Response Examples

#### Release Object (simplified)

```json
{
  "tag_name": "cdda-experimental-2024-01-15-0102",
  "name": "Experimental Build 2024-01-15-0102",
  "body": "## Changes\n- Fixed crash...",
  "published_at": "2024-01-15T01:02:00Z",
  "assets": [
    {
      "name": "cdda-windows-tiles-x64-2024-01-15-0102.zip",
      "browser_download_url": "https://github.com/..."
    }
  ]
}
```

### File Patterns

**Game Executables:**
- `cataclysm-tiles.exe` (graphical version)
- `cataclysm.exe` (console version)

**World Detection Files:**
- `worldoptions.json`
- `worldoptions.txt`
- `master.gsav`

**Save Directory Structure:**
```
save/
├── <world_name>/
│   ├── worldoptions.json
│   ├── master.gsav
│   └── <character_name>.sav
```
