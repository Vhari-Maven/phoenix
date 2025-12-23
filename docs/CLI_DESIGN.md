# Phoenix CLI Design

This document outlines the proposed CLI interface for Phoenix.

## Goals

1. **LLM-friendly** - Easy for AI assistants to invoke and parse output
2. **Scriptable** - Enable automation and integration with other tools
3. **Consistent** - Follow conventions from well-known CLIs (cargo, git, gh)
4. **Minimal** - Start small, expand based on needs

## Command Structure

```
phoenix [OPTIONS] <COMMAND>

Options:
  -q, --quiet       Suppress non-essential output
  -v, --verbose     Increase output verbosity
  --json            Output in JSON format (for machine parsing)
  -h, --help        Print help
  -V, --version     Print version

Commands:
  game        Game detection and launching
  backup      Backup management
  update      Update management
  soundpack   Soundpack management
  config      Configuration management
  diag        Diagnostics and debugging
```

## Commands Detail

### Diagnostic Commands

```
phoenix diag <COMMAND>

Commands:
  paths         Show all data paths (config, database, backups)
  check         Verify installation health
  clear-cache   Clear the version hash cache
```

**Examples:**
```bash
# Show where everything is stored
phoenix diag paths

# Verify paths exist and database is accessible
phoenix diag check

# Clear version cache (useful for testing fresh detection)
phoenix diag clear-cache
```

**Output (paths):**
```
Config file:  C:\Users\You\AppData\Roaming\phoenix\Phoenix\config\config.toml
Database:     C:\Users\You\AppData\Roaming\phoenix\Phoenix\data\phoenix.db
Backups dir:  C:\Users\You\AppData\Roaming\phoenix\Phoenix\data\backups
Game dir:     C:\Games\CDDA
```

**Output (check):**
```
[OK] Config file exists
[OK] Database accessible (5 cached versions)
[OK] Backups directory exists (3 backups, 126 MB)
[OK] Game directory exists
[OK] Game executable found
```

---

### Game Commands

```
phoenix game <COMMAND>

Commands:
  detect      Detect game installation and version
  launch      Launch the game
  info        Show detailed game information
```

**Examples:**
```bash
# Detect game in configured directory
phoenix game detect

# Detect game in specific directory
phoenix game detect --dir "C:\Games\CDDA"

# Launch game
phoenix game launch

# Launch with parameters
phoenix game launch --params "-seed=12345"

# JSON output for scripting
phoenix game detect --json
```

**Output (detect):**
```
Game detected: Cataclysm: Dark Days Ahead
Version: 0.G-2024-12-15-0832 (experimental)
Directory: C:\Games\CDDA
Executable: cataclysm-tiles.exe
Size: 1.24 GB
```

**Output (detect --json):**
```json
{
  "detected": true,
  "version": "0.G-2024-12-15-0832",
  "branch": "experimental",
  "directory": "C:\\Games\\CDDA",
  "executable": "cataclysm-tiles.exe",
  "size_bytes": 1331234567
}
```

---

### Backup Commands

```
phoenix backup <COMMAND>

Commands:
  list        List all backups
  create      Create a new backup
  restore     Restore a backup
  delete      Delete a backup
  verify      Check backup archive integrity
```

**Examples:**
```bash
# List all backups
phoenix backup list

# Create backup with auto-generated name
phoenix backup create

# Create backup with custom name
phoenix backup create --name "before-update"

# Create with specific compression (1-9)
phoenix backup create --compression 9

# Restore most recent backup
phoenix backup restore --latest

# Restore specific backup
phoenix backup restore "backup-2024-12-20-143052"

# Restore without creating safety backup first
phoenix backup restore "backup-name" --no-safety-backup

# Delete a backup
phoenix backup delete "backup-2024-12-20-143052"

# Delete all but N most recent
phoenix backup delete --keep 3

# Verify backup integrity without restoring
phoenix backup verify "backup-2024-12-20-143052"

# Dry-run restore (show what would happen)
phoenix backup restore "backup-name" --dry-run
```

**Output (list):**
```
Backups (3 total):

NAME                        SIZE      DATE                 WORLDS
backup-2024-12-20-143052    45.2 MB   2024-12-20 14:30:52  MyWorld, TestWorld
backup-2024-12-19-091523    42.1 MB   2024-12-19 09:15:23  MyWorld
backup-2024-12-18-200000    38.7 MB   2024-12-18 20:00:00  MyWorld

Total: 126.0 MB
```

---

### Update Commands

```
phoenix update <COMMAND>

Commands:
  check       Check for available updates
  download    Download an update (without installing)
  install     Install a downloaded update
  apply       Download and install in one step
  releases    List available releases
```

**Examples:**
```bash
# Check for updates
phoenix update check

# List recent releases
phoenix update releases
phoenix update releases --limit 10
phoenix update releases --branch stable

# Download latest
phoenix update download

# Download specific version
phoenix update download --version "0.G-2024-12-20-0832"

# Install already-downloaded update
phoenix update install

# Download and install in one step
phoenix update apply

# Apply with options
phoenix update apply --keep-saves --remove-old

# Dry-run (show what would happen without doing it)
phoenix update apply --dry-run
```

**Output (check):**
```
Current version: 0.G-2024-12-15-0832 (experimental)
Latest version:  0.G-2024-12-20-0832 (experimental)

Update available! Run 'phoenix update apply' to install.
```

**Output (check, up to date):**
```
Current version: 0.G-2024-12-20-0832 (experimental)
You are running the latest version.
```

---

### Soundpack Commands

```
phoenix soundpack <COMMAND>

Commands:
  list        List installed soundpacks
  available   List soundpacks available for download
  install     Install a soundpack
  delete      Delete a soundpack
  enable      Enable a soundpack
  disable     Disable a soundpack
```

**Examples:**
```bash
# List installed
phoenix soundpack list

# List available from repository
phoenix soundpack available

# Install from repository
phoenix soundpack install "CO.AG-ModsUpdates"

# Install from local file
phoenix soundpack install --file "C:\Downloads\soundpack.zip"

# Enable/disable
phoenix soundpack enable "CO.AG-ModsUpdates"
phoenix soundpack disable "CO.AG-ModsUpdates"

# Delete
phoenix soundpack delete "CO.AG-ModsUpdates"
```

---

### Config Commands

```
phoenix config <COMMAND>

Commands:
  show        Show current configuration
  get         Get a specific config value
  set         Set a config value
  path        Show config file path
```

**Examples:**
```bash
# Show all config
phoenix config show

# Get specific value
phoenix config get game.directory
phoenix config get launcher.theme

# Set value
phoenix config set game.directory "C:\Games\CDDA"
phoenix config set launcher.theme "Cyan"
phoenix config set updates.check_on_startup false

# Show config file location
phoenix config path
```

---

## Progress Display

For long-running operations (backup, download, install), show progress:

```
Downloading update...
[████████████░░░░░░░░] 58% (234.5 / 402.1 MB) 12.3 MB/s

Extracting...
[████████████████░░░░] 82% (1,234 / 1,502 files)
```

With `--quiet`, suppress progress bars and only show completion:
```
Download complete: 402.1 MB
Installation complete.
```

With `--json`, emit newline-delimited JSON progress events:
```json
{"event":"progress","phase":"download","percent":58,"bytes":245890234,"total":421654789}
{"event":"progress","phase":"extract","percent":82,"files":1234,"total":1502}
{"event":"complete","success":true}
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0    | Success |
| 1    | General error |
| 2    | Invalid arguments |
| 3    | Game not found |
| 4    | Network error |
| 5    | File system error |
| 6    | Backup not found |

---

## Implementation Plan

### Phase 1: Foundation
- [ ] Add `clap` dependency
- [ ] Create `src/cli/mod.rs` module structure
- [ ] Implement argument parsing in `main.rs`
- [ ] Add `--json` output formatter utility

### Phase 2: Core Commands
- [ ] `game detect` - wraps `game::detect_game_with_db()`
- [ ] `game launch` - wraps `game::launch_game()`
- [ ] `game info` - combines detect with size calculation
- [ ] `config show/get/set/path`
- [ ] `diag paths/check/clear-cache`

### Phase 3: Backup Commands
- [ ] `backup list` - wraps `backup::list_backups()`
- [ ] `backup create` - wraps `backup::create_backup()`
- [ ] `backup restore` - wraps `backup::restore_backup()` (+ `--dry-run`)
- [ ] `backup delete` - wraps `backup::delete_backup()`
- [ ] `backup verify` - check archive integrity
- [ ] Progress bar using `indicatif`

### Phase 4: Update Commands
- [ ] `update check` - compare versions
- [ ] `update releases` - wraps `github::fetch_releases()`
- [ ] `update download` - wraps `update::download_asset()`
- [ ] `update install` - wraps `update::install_update()`
- [ ] `update apply` - combined download + install (+ `--dry-run`)

### Phase 5: Soundpack Commands
- [ ] `soundpack list` - wraps `soundpack::list_installed_soundpacks()`
- [ ] `soundpack available` - wraps `soundpack::load_repository()`
- [ ] `soundpack install` - wraps `soundpack::install_soundpack()`
- [ ] `soundpack delete/enable/disable`

---

## Dependencies to Add

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }

[dev-dependencies]
assert_cmd = "2"      # CLI testing
predicates = "3"      # Test assertions
```

Optional (can add later):
```toml
indicatif = "0.17"    # Progress bars
colored = "2"         # Terminal colors
```

---

## File Structure

```
src/
├── main.rs           # Entry point - detect GUI vs CLI mode
├── cli/
│   ├── mod.rs        # CLI module, argument parsing
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── game.rs
│   │   ├── backup.rs
│   │   ├── update.rs
│   │   ├── soundpack.rs
│   │   ├── config.rs
│   │   └── diag.rs
│   ├── output.rs     # Formatting utilities (table, json)
│   └── progress.rs   # Progress bar wrapper
├── app.rs            # GUI app (existing)
└── ...               # Other existing modules
```

---

## Mode Detection

The binary supports both GUI and CLI modes. **GUI is the default** when run with no arguments.

```bash
phoenix              # Opens GUI (default behavior)
phoenix game detect  # CLI mode
phoenix backup list  # CLI mode
phoenix --help       # Show CLI help
phoenix --version    # Show version
```

**Implementation:**

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();

    // No arguments = launch GUI
    // Any subcommand = CLI mode
    if args.len() == 1 {
        gui::run();
    } else {
        cli::run();
    }
}
```

This means double-clicking `phoenix.exe` opens the GUI, while command-line usage gets CLI mode automatically.
