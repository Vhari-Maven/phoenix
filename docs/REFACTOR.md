# Phoenix Refactoring Plan

This document outlines the refactoring work needed to improve maintainability after completing the MVP.

## Executive Summary

The Phoenix codebase has a solid foundation with clean service modules (`github.rs`, `backup.rs`, `soundpack.rs`, etc.). However, the UI layer was never properly separated from state management. The 2,700-line `app.rs` file violates single responsibility and deviates from the planned architecture in `PLAN.md`.

## Current State

### Module Size Analysis

| File | Lines | Status |
|------|-------|--------|
| `app.rs` | ~1,165 | **Improved** - State + coordination (was ~2,700) |
| `ui/soundpacks_tab.rs` | ~576 | Good - Extracted from app.rs |
| `ui/main_tab.rs` | ~502 | Good - Extracted from app.rs |
| `ui/backups_tab.rs` | ~417 | Good - Extracted from app.rs |
| `ui/settings_tab.rs` | ~323 | Good - Extracted from app.rs |
| `soundpack.rs` | ~900 | Good |
| `update.rs` | ~750 | Good |
| `backup.rs` | ~750 | Good |
| `migration.rs` | ~700 | Good |
| `game.rs` | ~550 | Good |
| `github.rs` | ~330 | Excellent |
| `config.rs` | ~290 | Excellent |
| `theme.rs` | ~270 | Excellent |
| `db.rs` | ~240 | Good |
| `main.rs` | ~130 | Excellent |

### Architecture Gap

**Planned (from PLAN.md):**
```
src/
├── ui/
│   ├── mod.rs
│   ├── main_tab.rs
│   ├── update_tab.rs
│   ├── backups_tab.rs
│   ├── soundpacks_tab.rs
│   └── settings_tab.rs
├── services/
│   └── ...
```

**Actual:**
```
src/
├── app.rs          <- ALL UI code crammed here
├── backup.rs
├── config.rs
├── ...
```

---

## Phase 1: UI Module Extraction (High Impact)

**Goal:** Split `app.rs` into separate UI modules per the original plan.

### 1.1 Create UI Directory Structure

```
src/ui/
├── mod.rs              # Exports, common helpers, traits
├── main_tab.rs         # Game section, launch button (~250 lines)
├── update_tab.rs       # Branch selection, releases, changelog (~300 lines)
├── backups_tab.rs      # Backup list, create/restore/delete (~250 lines)
├── soundpacks_tab.rs   # Two-column layout, install/delete (~300 lines)
├── settings_tab.rs     # Appearance, behavior, game settings (~200 lines)
└── components.rs       # Shared dialogs, progress displays (~200 lines)
```

### 1.2 Extract Tab Rendering

Move these methods from `PhoenixApp` to separate modules:

| Method | Target Module |
|--------|---------------|
| `render_main_tab()` | `ui/main_tab.rs` |
| `render_update_section()` | `ui/update_tab.rs` |
| `render_backups_tab()` | `ui/backups_tab.rs` |
| `render_soundpacks_tab()` | `ui/soundpacks_tab.rs` |
| `render_settings_tab()` | `ui/settings_tab.rs` |
| `render_*_dialog()` | `ui/components.rs` |

### 1.3 Define UI Trait

```rust
// src/ui/mod.rs
pub trait TabRenderer {
    fn render(&mut self, ui: &mut egui::Ui, app: &mut PhoenixApp);
}
```

**Expected Result:** `app.rs` shrinks from ~2,700 to ~700 lines (state + coordination only).

---

## Phase 2: Deduplication (Medium Impact)

### 2.1 Extract `format_size()` to Utils

Currently duplicated in:
- `backup.rs`
- `soundpack.rs`
- `game.rs`

Create `src/utils/format.rs`:
```rust
pub fn format_size(bytes: u64) -> String { ... }
```

### 2.2 Consolidate Progress Display

The same progress rendering pattern is repeated for:
- Update progress
- Backup progress
- Soundpack progress

Extract to `src/ui/components.rs`:
```rust
pub fn render_progress_dialog(
    ui: &mut egui::Ui,
    phase: &str,
    fraction: f32,
    speed: Option<u64>,
    current_file: &str,
);
```

### 2.3 Extract Async Task Polling

This pattern appears 4+ times in `app.rs`:
```rust
if let Some(task) = &mut self.some_task {
    if task.is_finished() {
        let task = self.some_task.take().unwrap();
        match task.now_or_never() { ... }
    }
}
```

Extract to helper:
```rust
pub fn poll_task<T, E>(task: &mut Option<JoinHandle<Result<T, E>>>) -> Option<Result<T, E>>
```

---

## Phase 3: State Organization (Medium Impact)

### 3.1 Group Related State

Current `PhoenixApp` has 50+ fields. Group into nested structs:

```rust
pub struct PhoenixApp {
    config: Config,
    db: Option<Database>,
    theme_state: ThemeState,
    ui_state: UiState,
    game_state: GameState,
    releases_state: ReleasesState,
    update_state: UpdateState,
    backup_state: BackupState,
    soundpack_state: SoundpackState,
}

pub struct UiState {
    active_tab: Tab,
    show_about_dialog: bool,
    status_message: String,
}

pub struct ReleasesState {
    experimental: Vec<Release>,
    stable: Vec<Release>,
    selected_index: Option<usize>,
    fetch_task: Option<JoinHandle<...>>,
    rate_limit: RateLimitInfo,
}
```

### 3.2 Add Helper Methods

Replace repeated patterns like:
```rust
PathBuf::from(&self.config.game.directory)
```

With:
```rust
impl PhoenixApp {
    fn game_dir(&self) -> Option<&Path> { ... }
}
```

---

## Phase 4: Service Abstraction (Lower Priority)

### 4.1 Create AppService Layer

Wrap async operations in a clean service interface:

```rust
pub struct AppService {
    github: GitHubClient,
    db: Database,
}

impl AppService {
    pub async fn check_for_updates(&self) -> Result<Vec<Release>>;
    pub async fn install_update(&self, release: &Release) -> watch::Receiver<UpdateProgress>;
    pub async fn backup_saves(&self) -> watch::Receiver<BackupProgress>;
}
```

**Benefits:**
- Clear API surface for UI layer
- Testable business logic
- Consistent error handling

---

## Implementation Order

| Priority | Task | Effort | Impact |
|----------|------|--------|--------|
| 1 | Create `src/ui/` directory structure | 1h | Foundation |
| 2 | Extract `main_tab.rs` | 2h | -250 lines from app.rs |
| 3 | Extract `settings_tab.rs` | 1h | -200 lines from app.rs |
| 4 | Extract `backups_tab.rs` | 2h | -250 lines from app.rs |
| 5 | Extract `soundpacks_tab.rs` | 2h | -300 lines from app.rs |
| 6 | Extract `update_tab.rs` | 2h | -300 lines from app.rs |
| 7 | Extract shared components | 2h | -200 lines from app.rs |
| 8 | Deduplicate `format_size()` | 30m | Cleaner code |
| 9 | Consolidate progress rendering | 1h | Less duplication |
| 10 | Group state into nested structs | 2h | Better organization |

**Total Estimated Effort:** ~15-20 hours

---

## Completed Work

### Phase 1: UI Module Extraction (Done)

Extracted UI rendering from `app.rs` into separate modules:

| Module | Lines | Contents |
|--------|-------|----------|
| `ui/main_tab.rs` | 502 | Game info, update section, changelog, action buttons |
| `ui/soundpacks_tab.rs` | 576 | Two-column layout, install/delete, progress |
| `ui/backups_tab.rs` | 417 | Backup list, create/restore/delete, confirmations |
| `ui/settings_tab.rs` | 323 | Appearance, behavior, updates, game settings |
| `ui/mod.rs` | 13 | Module exports |

**Result:** `app.rs` reduced from ~2,700 to ~1,165 lines (57% reduction)

**Design decisions:**
- Used free functions `fn render_xxx(app: &mut PhoenixApp, ui: &mut egui::Ui)` instead of traits (simpler)
- Combined update section into `main_tab.rs` (matches actual UI structure)
- Left `render_tab()` and `render_about_dialog()` in app.rs (~120 lines, tightly coupled to eframe::App)

**Ideas for further reduction (~1,165 → ~700 lines):**

1. **Extract task polling to a helper** (~100 lines saved)
   The `poll_*_task()` pattern repeats 4 times. Could create:
   ```rust
   // src/task.rs
   pub fn poll_task<T, E>(task: &mut Option<JoinHandle<Result<T, E>>>) -> Option<Result<T, E>>
   ```

2. **Move poll methods to state structs** (~200 lines saved)
   Group related state + polling into nested structs (Phase 3):
   ```rust
   pub struct ReleasesState { ... }
   impl ReleasesState {
       fn poll(&mut self, ctx: &egui::Context) { ... }
   }
   ```
   This moves `poll_releases_task`, `poll_update_task`, `poll_backup_task`, `poll_soundpack_tasks` out of PhoenixApp.

3. **Extract remaining UI helpers** (~120 lines saved)
   Move `render_tab()` and `render_about_dialog()` to `ui/components.rs`

4. **Simplify PhoenixApp::new()** (~50 lines saved)
   Extract initialization into builder pattern or separate `init` module

### Build Warnings Cleanup (Done)

Removed 27 warnings by deleting unused code:

| Category | Items Removed |
|----------|---------------|
| Unused error variants | `BackupDirNotFound`, `RestoreFailed`, `NoGameDirectory`, `SoundpacksDirNotFound` |
| Unused struct fields | `build_number`, `directory`, `sha256`, `prerelease`, `content_type`, `expected_filename`, `disabled`, `error`, `bytes_processed`, `total_bytes` |
| Unused functions | `detect_game()`, `has_saves()`, `get_latest_release()`, `filter_releases_by_branch()`, `stable_tag_regex()`, `store_version()`, `version_count()`, `cleanup_partial_downloads()`, `fraction()` |
| Unused constants | `WORLD_FILES`, `STABLE_TAG_PATTERN` |
| Unused types | `GitRef`, `BeforeLaunch`, `AfterEnd` |

**Result:** Clean build with 0 warnings, 55 tests passing.

---

## Testing Strategy

1. **During refactoring:** Run `cargo test` after each module extraction
2. **Manual testing:** Verify each tab still works after extraction
3. **Add new tests:** Unit tests for extracted UI components where practical

---

## Risk Assessment

**Low Risk:**
- File/module splitting (moving code, not changing logic)
- Extracting utility functions
- Creating new modules for common patterns

**Medium Risk:**
- Restructuring state (requires careful testing)
- Creating service abstractions (needs backward compatibility)

**Mitigation:**
- Keep commits small and focused
- Maintain existing public API of `PhoenixApp`
- Run tests after each change
