# Phoenix Refactoring Plan

This document tracks refactoring work to improve maintainability after MVP completion.

---

## Current State

### Module Sizes

| File | Lines | Status |
|------|-------|--------|
| `app.rs` | ~474 | Excellent (was ~2,700 → ~988 → ~474) |
| `state/backup.rs` | ~254 | New - BackupState + poll |
| `state/update.rs` | ~206 | New - UpdateState + poll |
| `state/soundpack.rs` | ~202 | New - SoundpackState + poll |
| `state/releases.rs` | ~169 | New - ReleasesState + poll |
| `state/ui.rs` | ~42 | New - UiState + Tab enum |
| `state/mod.rs` | ~33 | New - StateEvent enum |
| `ui/soundpacks_tab.rs` | ~576 | Extracted |
| `ui/main_tab.rs` | ~502 | Extracted |
| `ui/backups_tab.rs` | ~417 | Extracted |
| `ui/settings_tab.rs` | ~323 | Extracted |
| `ui/components.rs` | ~135 | Shared UI components |
| `task.rs` | ~58 | Task polling helper |
| `util.rs` | ~58 | Shared utilities |
| `soundpack.rs` | ~885 | Good |
| `update.rs` | ~750 | Good |
| `backup.rs` | ~750 | Good |
| `migration.rs` | ~700 | Good |
| `game.rs` | ~515 | Good |
| `github.rs` | ~330 | Excellent |
| `config.rs` | ~290 | Excellent |
| `theme.rs` | ~270 | Excellent |
| `db.rs` | ~240 | Good |
| `main.rs` | ~138 | Excellent |

### Architecture

```
src/
├── main.rs              # Entry point
├── app.rs               # PhoenixApp coordination (~474 lines)
├── state/               # Grouped state structs with poll methods
│   ├── mod.rs           # StateEvent enum + module exports
│   ├── ui.rs            # UiState, Tab enum
│   ├── backup.rs        # BackupState + poll
│   ├── soundpack.rs     # SoundpackState + poll
│   ├── update.rs        # UpdateState + poll
│   └── releases.rs      # ReleasesState + poll
├── task.rs              # Generic task polling helper
├── util.rs              # Shared utilities (format_size)
├── ui/                  # UI rendering modules
│   ├── mod.rs
│   ├── components.rs    # Shared UI components (tabs, dialogs)
│   ├── main_tab.rs      # Game info, updates, changelog
│   ├── backups_tab.rs   # Backup management
│   ├── soundpacks_tab.rs# Soundpack management
│   └── settings_tab.rs  # Settings
├── backup.rs            # Backup service
├── config.rs            # Configuration
├── db.rs                # SQLite cache
├── game.rs              # Game detection/launch
├── github.rs            # GitHub API client
├── migration.rs         # Update migration
├── soundpack.rs         # Soundpack service
├── theme.rs             # Theme system
└── update.rs            # Update download/install
```

---

## Completed Work

### Phase 3: State Struct Extraction (Done)

Extracted `PhoenixApp`'s 50+ fields into nested state structs in `src/state/`:

| Module | Lines | Contents |
|--------|-------|----------|
| `backup.rs` | 254 | BackupState + poll + 6 methods |
| `update.rs` | 206 | UpdateState + UpdateParams + poll |
| `soundpack.rs` | 202 | SoundpackState + poll + 4 methods |
| `releases.rs` | 169 | ReleasesState + poll + 5 methods |
| `ui.rs` | 42 | UiState + Tab enum |
| `mod.rs` | 33 | StateEvent enum + exports |

**Result:** `app.rs` reduced from ~988 to ~474 lines (52% reduction)

**Design decisions:**
- `StateEvent` enum for cross-cutting concerns (status messages, refresh triggers, logging)
- Poll methods return `Vec<StateEvent>` instead of directly mutating app state
- Config values passed as parameters rather than stored in state structs
- PhoenixApp has delegation methods for backward compatibility with UI modules

**Final PhoenixApp structure:**
```rust
pub struct PhoenixApp {
    // Core (stays at app level)
    config: Config,
    db: Option<Database>,
    game_info: Option<GameInfo>,
    status_message: String,
    github_client: GitHubClient,

    // Grouped state
    ui: UiState,
    releases: ReleasesState,
    update: UpdateState,
    backup: BackupState,
    soundpack: SoundpackState,
}
```

### Phase 1: UI Module Extraction (Done)

Extracted UI rendering from `app.rs` into `src/ui/` modules:

| Module | Lines | Contents |
|--------|-------|----------|
| `main_tab.rs` | 502 | Game info, update section, changelog, buttons |
| `soundpacks_tab.rs` | 576 | Two-column layout, install/delete, progress |
| `backups_tab.rs` | 417 | Backup list, create/restore/delete |
| `settings_tab.rs` | 323 | Appearance, behavior, game settings |
| `mod.rs` | 13 | Module exports |

**Result:** `app.rs` reduced from ~2,700 to ~1,165 lines (57% reduction)

**Design decisions:**
- Used free functions `fn render_xxx(app: &mut PhoenixApp, ui: &mut egui::Ui)` instead of traits
- Combined update section into `main_tab.rs` (matches actual UI structure)

### Build Warnings Cleanup (Done)

Removed 27 unused items (error variants, struct fields, functions, constants, types).

**Result:** Clean build with 0 warnings, 55 tests passing.

### Phase 1.5: UI Components & Task Helper (Done)

Extracted remaining UI helpers and created task polling abstraction:

| Module | Lines | Contents |
|--------|-------|----------|
| `ui/components.rs` | 135 | `render_tab()`, `render_about_dialog()` |
| `task.rs` | 58 | Generic `poll_task()` helper with `PollResult` enum |

**Result:** `app.rs` reduced from ~1,165 to ~988 lines (15% reduction)

**Design decisions:**
- Moved `render_tab()` and `render_about_dialog()` to `ui/components.rs`
- Created `PollResult` enum to encapsulate task polling states (NoTask, Pending, Complete)
- Refactored all 4 poll functions to use the new helper, reducing boilerplate

### Phase 2: format_size() Deduplication (Done)

Consolidated 4 copies of `format_size()` into `src/util.rs`:

| Location | Action |
|----------|--------|
| `game.rs` | Removed (18 lines + 4 tests) |
| `backup.rs` | Removed (16 lines + 1 test), now imports from util |
| `soundpack.rs` | Removed (15 lines + 1 test) |
| `ui/main_tab.rs` | Removed (15 lines) |
| `util.rs` | Created (58 lines with tests) |

**Result:** Net reduction of ~6 lines, eliminated code duplication, unified formatting style (1 decimal, KB/MB/GB labels)

---

## Future Work

### Phase 2: Deduplication (Partial)

- ~~**format_size()** - Duplicated in `backup.rs`, `soundpack.rs`, `game.rs`~~ Done
- **Progress rendering** - Similar patterns for update/backup/soundpack progress

### Phase 4: Service Abstraction (Optional)

Create `AppService` layer wrapping async operations for cleaner API surface.

---

## Guidelines

- **Run tests after each change:** `cargo test`
- **Keep commits small and focused**
- **Don't change behavior** - refactoring is purely structural
- **Preserve public APIs** - PhoenixApp's interface should remain stable
