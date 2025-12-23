# Phoenix Architecture Guide

This document explains the architecture of Phoenix, a native game launcher for Cataclysm: Dark Days Ahead (CDDA). It's written for programmers who may not have extensive Rust experience.

## Table of Contents

1. [Overview](#1-overview)
2. [Rust Concepts for Non-Rust Developers](#2-rust-concepts-for-non-rust-developers)
3. [Application Structure](#3-application-structure)
4. [State Management](#4-state-management)
5. [Async Task Pattern](#5-async-task-pattern)
6. [UI Rendering](#6-ui-rendering)
7. [Error Handling](#7-error-handling)
8. [The Update Loop](#8-the-update-loop)
9. [Data Flow Example](#9-data-flow-example)
10. [Key Files Reference](#10-key-files-reference)

---

## 1. Overview

Phoenix is a Windows-native launcher for CDDA that provides:

- **Game Management**: Detect installed game, launch it, identify versions
- **Updates**: Download and install game updates with smart migration (preserves mods, tilesets, soundpacks)
- **Backups**: Create/restore compressed save backups
- **Soundpacks**: Install soundpacks from a curated repository or local files
- **Theming**: Multiple color themes (Amber, Purple, Cyan, Green, Catppuccin)

### Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Rust (2024 edition) |
| GUI Framework | egui + eframe |
| Async Runtime | tokio |
| HTTP Client | reqwest |
| Archive Handling | zip crate |
| Database | SQLite (rusqlite) |
| Config | TOML (serde) |

### Why These Choices?

- **egui**: Immediate-mode GUI that's simple to use and renders at 60fps
- **tokio**: Industry-standard async runtime for Rust
- **Immediate mode**: UI is redrawn every frame based on current state (no event callbacks)

---

## 2. Rust Concepts for Non-Rust Developers

### Ownership and Borrowing

Rust's key feature is **ownership** - every value has exactly one owner, and when that owner goes out of scope, the value is dropped (freed).

**References** let you "borrow" a value without taking ownership:

```rust
// Immutable borrow - can read but not modify
fn read_config(config: &Config) { ... }

// Mutable borrow - can read and modify
fn update_config(config: &mut Config) { ... }
```

**Why `&mut PhoenixApp` appears everywhere:**

UI rendering functions take `&mut PhoenixApp` because they need to:
1. Read state to display it
2. Modify state when users interact (click buttons, etc.)

```rust
// UI function borrows the app mutably
pub fn render_main_tab(app: &mut PhoenixApp, ui: &mut egui::Ui) {
    // Can read: app.game_info
    // Can modify: app.status_message = "..."
}
```

The borrow checker ensures only one mutable reference exists at a time, preventing data races at compile time.

### The Result Type and `?` Operator

Rust doesn't have exceptions. Functions that can fail return `Result<T, E>`:

```rust
enum Result<T, E> {
    Ok(T),    // Success with value of type T
    Err(E),   // Failure with error of type E
}
```

**The `?` operator** propagates errors automatically:

```rust
fn load_file() -> Result<String, io::Error> {
    let content = fs::read_to_string("file.txt")?;  // Returns early if error
    Ok(content)
}

// Equivalent to:
fn load_file_verbose() -> Result<String, io::Error> {
    let content = match fs::read_to_string("file.txt") {
        Ok(c) => c,
        Err(e) => return Err(e),  // Early return on error
    };
    Ok(content)
}
```

### Async/Await and Tokio

**Async functions** don't block the thread while waiting for I/O:

```rust
async fn download_file(url: &str) -> Result<Vec<u8>, Error> {
    let response = reqwest::get(url).await?;  // Yields while waiting
    let bytes = response.bytes().await?;
    Ok(bytes.to_vec())
}
```

**tokio::spawn** runs an async function in the background:

```rust
let handle: JoinHandle<Result<(), Error>> = tokio::spawn(async move {
    download_file("https://...").await?;
    Ok(())
});
// handle can be polled later to check if done
```

**Key insight**: The UI thread never calls `.await` directly. Instead, it spawns tasks and polls their handles each frame.

### Option Type

Rust uses `Option<T>` instead of null:

```rust
enum Option<T> {
    Some(T),  // Has a value
    None,     // No value
}

// Common pattern:
if let Some(info) = &app.game_info {
    println!("Version: {}", info.version);
}
```

---

## 3. Application Structure

### The Central Hub: PhoenixApp

`PhoenixApp` (in `src/app.rs`) is the main application struct that orchestrates everything:

```rust
pub struct PhoenixApp {
    // Core state
    pub config: Config,              // User settings
    pub db: Option<Database>,        // Version hash cache
    pub game_info: Option<GameInfo>, // Detected game
    pub status_message: String,      // Status bar text
    pub github_client: GitHubClient, // HTTP client

    // Grouped state structs
    pub ui: UiState,                 // Theme, active tab
    pub releases: ReleasesState,     // Available releases
    pub update: UpdateState,         // Download/install progress
    pub backup: BackupState,         // Backup operations
    pub soundpack: SoundpackState,   // Soundpack operations
}
```

### Separation of Concerns

The codebase separates:

1. **State structs** (`src/state/`) - Own data and async task handles
2. **Service modules** (`src/backup.rs`, `src/update.rs`, etc.) - Implement business logic
3. **UI modules** (`src/ui/`) - Render the interface

```
PhoenixApp (coordinator)
    │
    ├── State Structs (own tasks, emit events)
    │   ├── BackupState
    │   ├── UpdateState
    │   ├── ReleasesState
    │   └── SoundpackState
    │
    ├── Service Modules (business logic)
    │   ├── backup.rs
    │   ├── update.rs
    │   ├── github.rs
    │   └── soundpack.rs
    │
    └── UI Modules (rendering)
        ├── main_tab.rs
        ├── backups_tab.rs
        ├── soundpacks_tab.rs
        └── settings_tab.rs
```

---

## 4. State Management

### The Poll-Based Event System

Phoenix uses a **decoupled state pattern** where state structs communicate back to `PhoenixApp` through events rather than direct mutation.

### StateEvent Enum

```rust
// src/state/mod.rs
pub enum StateEvent {
    StatusMessage(String),  // Update the status bar
    RefreshGameInfo,        // Re-detect the game
    LogError(String),       // Log an error
    LogInfo(String),        // Log info message
}
```

### Why Events?

1. **Decoupling**: State structs don't need a reference to PhoenixApp
2. **Explicit flow**: Easy to trace what triggers what
3. **No circular dependencies**: States emit events, app handles them
4. **Testability**: Can test state structs in isolation

### How Events Flow

```
[State Struct]              [PhoenixApp]              [UI]
      │                          │                      │
      │  poll() called           │                      │
      ├─────────────────────────>│                      │
      │                          │                      │
      │  Returns Vec<StateEvent> │                      │
      │<─────────────────────────┤                      │
      │                          │                      │
      │                    handle_events()              │
      │                          │                      │
      │                    Updates fields               │
      │                          ├─────────────────────>│
      │                          │        Renders       │
```

### Event Handling in PhoenixApp

```rust
fn handle_event(&mut self, event: StateEvent) {
    match event {
        StateEvent::StatusMessage(msg) => {
            self.status_message = msg;
        }
        StateEvent::RefreshGameInfo => {
            self.refresh_game_info();
        }
        StateEvent::LogError(msg) => {
            tracing::error!("{}", msg);
        }
        StateEvent::LogInfo(msg) => {
            tracing::info!("{}", msg);
        }
    }
}
```

---

## 5. Async Task Pattern

### The Challenge

The UI must stay responsive at 60fps, but operations like downloading a 500MB game take time. We can't block the UI thread.

### The Solution: Poll and Check

Instead of callbacks or promises, Phoenix **polls** async tasks each frame:

```rust
// src/task.rs
pub enum PollResult<T> {
    NoTask,     // No task exists
    Pending,    // Task still running
    Complete(Result<T, JoinError>),  // Task finished
}

pub fn poll_task<T>(task: &mut Option<JoinHandle<T>>) -> PollResult<T> {
    let Some(handle) = task else {
        return PollResult::NoTask;
    };

    if !handle.is_finished() {
        return PollResult::Pending;
    }

    // Task is done - take ownership and extract result
    let handle = task.take().unwrap();
    match handle.now_or_never() {
        Some(result) => PollResult::Complete(result),
        None => PollResult::Pending,
    }
}
```

### Progress Channels

For operations with progress (downloads, backups), we use **watch channels**:

```rust
// Sender side (in async task)
let (progress_tx, progress_rx) = watch::channel(Progress::default());

tokio::spawn(async move {
    for chunk in chunks {
        // ... process chunk ...
        progress_tx.send(Progress { bytes_done, total_bytes }).ok();
    }
});

// Receiver side (in poll method)
if progress_rx.has_changed().unwrap_or(false) {
    self.progress = progress_rx.borrow_and_update().clone();
}
```

### Complete State Poll Pattern

```rust
impl BackupState {
    pub fn poll(&mut self, ctx: &egui::Context) -> Vec<StateEvent> {
        let mut events = Vec::new();

        // 1. Check progress channel for updates
        if let Some(rx) = &mut self.progress_rx {
            if rx.has_changed().unwrap_or(false) {
                self.progress = rx.borrow_and_update().clone();
                events.push(StateEvent::StatusMessage(
                    self.progress.phase.description().to_string()
                ));
            }
        }

        // 2. Check if task completed
        match poll_task(&mut self.task) {
            PollResult::Complete(Ok(Ok(()))) => {
                // Success!
                events.push(StateEvent::StatusMessage("Complete!".into()));
                self.refresh_list();  // Start next operation
            }
            PollResult::Complete(Ok(Err(e))) => {
                // Task returned an error
                self.error = Some(e.to_string());
            }
            PollResult::Complete(Err(e)) => {
                // Task panicked
                self.error = Some(format!("Task panicked: {}", e));
            }
            PollResult::Pending => {
                // Still running - request another frame
                ctx.request_repaint();
            }
            PollResult::NoTask => {}
        }

        events
    }
}
```

---

## 6. UI Rendering

### Immediate Mode GUI

egui uses **immediate mode** rendering: the UI is rebuilt every frame based on current state. There are no persistent widgets or event callbacks.

```rust
// This runs 60 times per second
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Every frame: check state, draw UI, handle clicks
    if ui.button("Download").clicked() {
        self.start_download();
    }
}
```

### Tab System

The UI is organized into tabs:

```rust
pub enum Tab {
    Main,       // Game info, updates, launch
    Backups,    // Backup management
    Soundpacks, // Soundpack installation
    Settings,   // Configuration
}
```

Tab rendering uses a match statement:

```rust
match self.ui.active_tab {
    Tab::Main => crate::ui::render_main_tab(self, ui),
    Tab::Backups => crate::ui::render_backups_tab(self, ui),
    Tab::Soundpacks => crate::ui::render_soundpacks_tab(self, ui),
    Tab::Settings => crate::ui::render_settings_tab(self, ui),
}
```

### UI Functions as Free Functions

UI rendering is done in **free functions** (not methods) that borrow PhoenixApp:

```rust
// src/ui/main_tab.rs
pub fn render_main_tab(app: &mut PhoenixApp, ui: &mut egui::Ui) {
    // Read state
    if let Some(ref info) = app.game_info {
        ui.label(format!("Version: {}", info.version_display()));
    }

    // Handle interactions
    if ui.button("Launch Game").clicked() {
        app.launch_game();
    }
}
```

**Why free functions?**

1. Clear separation between UI and state logic
2. UI can't directly access internal state struct fields
3. All interactions go through PhoenixApp's public methods

---

## 7. Error Handling

### Custom Error Types with thiserror

Each service module defines its own error type using the `thiserror` crate:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackupError {
    #[error("Save directory not found: {0}")]
    SaveDirNotFound(PathBuf),

    #[error("Backup not found: {0}")]
    BackupNotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
}
```

The `#[from]` attribute auto-implements `From<io::Error> for BackupError`, enabling `?` to convert errors automatically.

### Error Flow Pattern

```rust
// In async task
async fn create_backup(...) -> Result<(), BackupError> {
    let file = File::create(&path)?;  // io::Error -> BackupError::Io
    // ...
}

// In state poll
match poll_task(&mut self.task) {
    PollResult::Complete(Ok(Ok(()))) => { /* success */ }
    PollResult::Complete(Ok(Err(e))) => {
        // BackupError - display to user
        self.error = Some(e.to_string());
    }
    PollResult::Complete(Err(e)) => {
        // JoinError - task panicked (shouldn't happen)
        tracing::error!("Task panicked: {}", e);
    }
    // ...
}
```

### Result Nesting

Task results are often nested: `Result<Result<T, AppError>, JoinError>`:

- **Outer Result**: Did the task run without panicking?
- **Inner Result**: Did the operation succeed?

```rust
match poll_task(&mut self.task) {
    PollResult::Complete(Ok(Ok(value))) => { /* full success */ }
    PollResult::Complete(Ok(Err(app_error))) => { /* operation failed */ }
    PollResult::Complete(Err(join_error)) => { /* task panicked */ }
    // ...
}
```

---

## 8. The Update Loop

Here's what happens **every frame** (60 times per second):

```
Frame Start (eframe calls PhoenixApp::update)
│
├─ Apply theme if changed
│
├─ Poll all state structs
│   ├─ releases.poll(ctx, branch)  → Vec<StateEvent>
│   ├─ update.poll(ctx)            → Vec<StateEvent>
│   ├─ backup.poll(ctx)            → Vec<StateEvent>
│   └─ soundpack.poll(ctx, dir)    → Vec<StateEvent>
│
├─ Handle all collected events
│   ├─ StatusMessage → update self.status_message
│   ├─ RefreshGameInfo → re-detect game
│   └─ LogError/LogInfo → write to log
│
└─ Render UI
    ├─ Top menu bar (File, Help)
    ├─ Tab buttons (Main, Backups, Soundpacks, Settings)
    ├─ Tab content (based on active_tab)
    └─ Status bar (shows self.status_message)
```

### Code Structure

```rust
impl eframe::App for PhoenixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Theme
        if self.ui.theme_dirty {
            self.ui.current_theme.apply(ctx);
            self.ui.theme_dirty = false;
        }

        // 2. Poll states
        let release_events = self.releases.poll(ctx, &self.config.game.branch);
        self.handle_events(release_events);

        let update_events = self.update.poll(ctx);
        self.handle_events(update_events);

        let backup_events = self.backup.poll(ctx);
        self.handle_events(backup_events);

        let soundpack_events = self.soundpack.poll(ctx, game_dir_ref);
        self.handle_events(soundpack_events);

        // 3. Render UI panels
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| { ... });
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| { ... });
        egui::CentralPanel::default().show(ctx, |ui| {
            // Tab bar
            // Tab content
        });
    }
}
```

### Repaint Requests

When a task is still running, `ctx.request_repaint()` schedules another frame immediately. Without pending tasks, egui only repaints on user input.

---

## 9. Data Flow Example

Let's trace what happens when a user clicks "Download Update":

### Step 1: User Clicks Button

In `src/ui/main_tab.rs`:

```rust
if ui.button("Download Update").clicked() {
    app.start_update();
}
```

### Step 2: PhoenixApp Prepares Parameters

In `src/app.rs`:

```rust
pub fn start_update(&mut self) {
    // Get selected release and asset
    let (release, asset) = /* ... */;
    let game_dir = PathBuf::from(self.config.game.directory.as_ref()?);

    let params = UpdateParams {
        release,
        asset,
        game_dir,
        client: self.github_client.clone(),
        // ... config values ...
    };

    // Delegate to UpdateState
    if let Some(event) = self.update.start(params) {
        self.handle_event(event);  // "Downloading..."
    }
}
```

### Step 3: UpdateState Spawns Async Task

In `src/state/update.rs`:

```rust
pub fn start(&mut self, params: UpdateParams) -> Option<StateEvent> {
    // Create progress channel
    let (progress_tx, progress_rx) = watch::channel(UpdateProgress::default());
    self.progress_rx = Some(progress_rx);

    // Spawn async task
    self.task = Some(tokio::spawn(async move {
        // Downloads file, extracts, installs - sends progress via progress_tx
        update::download_and_install(params, progress_tx).await
    }));

    Some(StateEvent::StatusMessage("Downloading update...".into()))
}
```

### Step 4: Each Frame While Downloading

```rust
// In PhoenixApp::update()
let update_events = self.update.poll(ctx);

// In UpdateState::poll()
pub fn poll(&mut self, ctx: &egui::Context) -> Vec<StateEvent> {
    let mut events = Vec::new();

    // Check for progress updates
    if let Some(rx) = &mut self.progress_rx {
        if rx.has_changed().unwrap_or(false) {
            self.progress = rx.borrow_and_update().clone();
            events.push(StateEvent::StatusMessage(
                format!("Downloading: {}%", self.progress.percent())
            ));
        }
    }

    // Check if done
    match poll_task(&mut self.task) {
        PollResult::Pending => ctx.request_repaint(),
        PollResult::Complete(Ok(Ok(()))) => {
            events.push(StateEvent::RefreshGameInfo);
            events.push(StateEvent::StatusMessage("Update complete!".into()));
        }
        // ... error handling ...
    }

    events
}
```

### Step 5: UI Renders Progress

```rust
// In render_main_tab()
if app.update.is_updating() {
    ui.add(ProgressBar::new(app.update.progress.percent() / 100.0));
    ui.label(&app.update.progress.phase.description());
}
```

### Step 6: Task Completes

When finished:
1. `poll_task` returns `Complete(Ok(Ok(())))`
2. State emits `RefreshGameInfo` event
3. PhoenixApp calls `refresh_game_info()` to re-detect game
4. Status bar shows "Update complete!"
5. Main tab shows new version info

---

## 10. Key Files Reference

### Core

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point, logging setup, icon loading |
| `src/app.rs` | PhoenixApp struct, main update loop, event handling |
| `src/config.rs` | Configuration loading/saving (TOML) |
| `src/task.rs` | `poll_task()` helper for async task polling |

### State

| File | Purpose |
|------|---------|
| `src/state/mod.rs` | StateEvent enum, module exports |
| `src/state/ui.rs` | UiState (theme, tabs), Tab enum |
| `src/state/backup.rs` | BackupState (backup task management) |
| `src/state/update.rs` | UpdateState (download/install tasks) |
| `src/state/releases.rs` | ReleasesState (GitHub release fetching) |
| `src/state/soundpack.rs` | SoundpackState (soundpack installation) |

### Services

| File | Purpose |
|------|---------|
| `src/backup.rs` | Create/restore/list backups |
| `src/update.rs` | Download and install updates |
| `src/github.rs` | GitHub API client, release fetching |
| `src/game.rs` | Game detection, version parsing, launching |
| `src/migration.rs` | Smart migration (preserve mods/tilesets) |
| `src/soundpack.rs` | Soundpack installation |
| `src/db.rs` | SQLite database for version hash cache |

### UI

| File | Purpose |
|------|---------|
| `src/ui/mod.rs` | Module exports |
| `src/ui/theme.rs` | Theme definitions (colors, presets) |
| `src/ui/components.rs` | Shared components (tabs, dialogs, progress) |
| `src/ui/main_tab.rs` | Main tab rendering |
| `src/ui/backups_tab.rs` | Backups tab rendering |
| `src/ui/soundpacks_tab.rs` | Soundpacks tab rendering |
| `src/ui/settings_tab.rs` | Settings tab rendering |

---

## Further Reading

- **CLAUDE.md**: Project overview and quick reference
- **Rust Book**: https://doc.rust-lang.org/book/ (especially chapters on ownership and error handling)
- **egui documentation**: https://docs.rs/egui
- **tokio documentation**: https://tokio.rs
