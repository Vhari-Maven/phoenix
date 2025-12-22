# Phoenix Refactoring Plan

This document tracks refactoring work to improve maintainability after MVP completion.

---

## Next Steps

**Goal:** Reduce `app.rs` from ~988 to ~700 lines.

| Priority | Task | Savings | Complexity |
|----------|------|---------|------------|
| 1 | Group state into nested structs (Phase 3) | ~200 lines | Medium |

### Task: State Structs (Phase 3)

Group `PhoenixApp`'s 50+ fields into nested structs:
```rust
pub struct PhoenixApp {
    config: Config,
    db: Option<Database>,
    ui: UiState,
    releases: ReleasesState,
    update: UpdateState,
    backup: BackupState,
    soundpack: SoundpackState,
}
```

Each state struct would own its `poll()` method, moving ~200 lines out of `app.rs`.

---

## Current State

### Module Sizes

| File | Lines | Status |
|------|-------|--------|
| `app.rs` | ~988 | Improved (was ~2,700) |
| `ui/soundpacks_tab.rs` | ~576 | Extracted |
| `ui/main_tab.rs` | ~502 | Extracted |
| `ui/backups_tab.rs` | ~417 | Extracted |
| `ui/settings_tab.rs` | ~323 | Extracted |
| `ui/components.rs` | ~135 | New - shared UI components |
| `task.rs` | ~58 | New - task polling helper |
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

### Architecture

```
src/
├── main.rs              # Entry point
├── app.rs               # PhoenixApp state + eframe::App impl (~988 lines)
├── task.rs              # Generic task polling helper
├── ui/                  # Extracted UI modules
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

---

## Future Work

### Phase 2: Deduplication

- **format_size()** - Duplicated in `backup.rs`, `soundpack.rs`, `game.rs`
- **Progress rendering** - Similar patterns for update/backup/soundpack progress

### Phase 3: State Organization

Group `PhoenixApp`'s 50+ fields into nested structs with their own methods.

### Phase 4: Service Abstraction (Optional)

Create `AppService` layer wrapping async operations for cleaner API surface.

---

## Guidelines

- **Run tests after each change:** `cargo test`
- **Keep commits small and focused**
- **Don't change behavior** - refactoring is purely structural
- **Preserve public APIs** - PhoenixApp's interface should remain stable
