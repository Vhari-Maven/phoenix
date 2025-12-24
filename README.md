<p align="center">
  <img src="assets/icon.svg" width="128" height="128" alt="Phoenix">
</p>

# Phoenix - CDDA Game Launcher

A fast, native game launcher for [Cataclysm: Dark Days Ahead](https://cataclysmdda.org/), built in Rust.

## Why Phoenix?

Phoenix is a ground-up rewrite inspired by the excellent [CDDA Game Launcher](https://github.com/remyroy/CDDA-Game-Launcher) by Rémy Roy and its continued development by [Fris0uman](https://github.com/Fris0uman/CDDA-Game-Launcher) (Kitten CDDA Launcher).

The original Python/Qt launcher is feature-rich but can take 10-15 seconds to start. Phoenix aims for **sub-second startup** while retaining the core functionality players rely on.

## Features

- **Game Launching** - Browse for game directory, launch with one click
- **Version Detection** - Automatically identifies your installed game version
- **Automatic Updates** - Download and install updates with progress tracking
- **Smart Migration** - Preserves your mods, saves, tilesets, soundpacks, and fonts during updates
- **Save Backups** - Manual and automatic backup management with compression
- **Soundpack Manager** - Install, enable/disable, and delete soundpacks (ZIP)
- **Theme System** - 5 built-in color themes (Amber, Purple, Cyan, Green, Catppuccin)
- **Fast Updates** - Optimized update process (~18 seconds vs ~54 seconds naive approach)
- **CLI Mode** - Full command-line interface for scripting and automation

## Download

See the [Releases](https://github.com/Vhari-Maven/phoenix/releases) page for the latest build.

## Getting Started

1. **Download and run** `phoenix.exe` - no installation required
2. **Set your game directory:**
   - Click "Browse" and select your existing CDDA folder, or
   - Choose an empty folder where you'd like to install the game
3. **Install or update** (if needed) - choose a branch (Stable/Experimental), select a release, and click "Install Game" or "Update Game"
4. **Launch** - click the Launch button to play

Phoenix will remember your settings between sessions.

## Command Line Interface

Phoenix includes a full CLI for scripting and automation. Running with no arguments opens the GUI; any subcommand runs in CLI mode.

```bash
# Game management
phoenix game detect              # Detect installed game version
phoenix game launch              # Launch the game
phoenix game info                # Show detailed game information

# Backups
phoenix backup list              # List all backups
phoenix backup create            # Create a new backup
phoenix backup restore <name>    # Restore a backup
phoenix backup delete <name>     # Delete a backup

# Updates
phoenix update check             # Check for available updates
phoenix update releases          # List available releases
phoenix update changelog <tag>   # Show changelog for a release
phoenix update apply             # Download and install latest update

# Soundpacks
phoenix soundpack list           # List installed soundpacks
phoenix soundpack available      # List soundpacks in repository
phoenix soundpack install <name> # Install a soundpack

# Configuration
phoenix config show              # Show current configuration
phoenix config get <key>         # Get a specific setting
phoenix config set <key> <value> # Set a configuration value

# Diagnostics
phoenix diag paths               # Show all data paths
phoenix diag check               # Verify installation health

# Interactive shell
phoenix shell                    # Start REPL with history and tab completion
```

**Global options:**
- `--json` - Output in JSON format for machine parsing
- `--quiet` - Suppress non-essential output
- `--verbose` - Enable debug logging
- `--no-color` - Disable colored output (automatic when piping)

**Note:** CLI commands can run while the GUI is open. This is intentional for scripting use cases (e.g., scheduled backups via cron). Read operations are safe to run concurrently; write operations (backup create, update install) should be coordinated to avoid conflicts.

## FAQ

### How does the launcher update my game?

1. Downloads the archive for the new version
2. Moves your current installation to `.phoenix_archive` (old archives are cleaned up in the background)
3. Extracts the new version to your game directory
4. Intelligently restores your content:
   - **Saves** - Copied from previous version (or left in place with `prevent_save_move` option)
   - **Mods** - Only custom mods are restored; official mods use the new version
   - **Tilesets** - Only custom tilesets are restored
   - **Soundpacks** - Only custom soundpacks are restored
   - **Fonts** - Only fonts not included in the new version are restored
   - **Config** - Your settings are preserved (excluding debug logs)

### My antivirus flagged the launcher. What can I do?

Some antivirus products may flag the launcher as a threat. You can:

1. Add the launcher to your antivirus whitelist
2. Build from source yourself (see Building section below)

This is a common issue with many legitimate applications and is not indicative of actual malware.

### I found a bug or have a suggestion for the game itself.

Please [contact the CDDA developers](https://cataclysmdda.org/#ive-found-a-bug--i-would-like-to-make-a-suggestion-what-should-i-do). Phoenix is just a launcher tool and cannot provide support for the game itself.

### Where are my settings stored?

- **Config file:** `%APPDATA%\phoenix\Phoenix\config\config.toml`
- **Version cache:** `%APPDATA%\phoenix\Phoenix\data\phoenix.db`
- **Backups:** `%APPDATA%\phoenix\Phoenix\data\backups\`

## Building

### Prerequisites

- [Rust](https://rustup.rs/) (2024 edition)
- Windows x64

### Build from Source

```bash
git clone https://github.com/Vhari-Maven/phoenix.git
cd phoenix
cargo build --release
```

The binary will be at `target/release/phoenix.exe`.

## Configuration

Phoenix stores its configuration in TOML format. Key options include:

| Option | Description | Default |
|--------|-------------|---------|
| `theme` | Color theme (Amber, Purple, Cyan, Green, Catppuccin) | Amber |
| `keep_open` | Keep launcher open after starting game | false |
| `check_on_startup` | Check for game updates on launch | true |
| `prevent_save_move` | Leave saves in place during updates | false |
| `backup_before_update` | Auto-backup saves before updating | true |
| `max_count` | Maximum auto-backups to retain | 6 |

## Acknowledgments

This project owes its design and inspiration to:

- **[Rémy Roy](https://github.com/remyroy)** - Creator of the original [CDDA Game Launcher](https://github.com/remyroy/CDDA-Game-Launcher)
- **[Fris0uman](https://github.com/Fris0uman)** - Maintainer of the [Kitten CDDA Launcher](https://github.com/Fris0uman/CDDA-Game-Launcher) fork

The original launcher's thoughtful update process, smart migration logic, and user-focused design informed Phoenix's architecture.

### Built with AI

The majority of this codebase was written by [Claude](https://claude.ai) (Opus 4.5) via [Claude Code](https://claude.ai/claude-code), Anthropic's AI coding assistant. This project serves as an example of human-AI collaboration in software development.

## For Developers

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for an in-depth guide to the codebase, including:
- Rust concepts for non-Rust developers
- State management and async patterns
- UI rendering architecture
- Complete data flow examples

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
