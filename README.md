# Phoenix - CDDA Game Launcher

A fast, native game launcher for [Cataclysm: Dark Days Ahead](https://cataclysmdda.org/), built in Rust.

## Why Phoenix?

Phoenix is a ground-up rewrite inspired by the excellent [CDDA Game Launcher](https://github.com/remyroy/CDDA-Game-Launcher) by Rémy Roy and its continued development by [Fris0uman](https://github.com/Fris0uman/CDDA-Game-Launcher) (Kitten CDDA Launcher).

The original Python/Qt launcher is feature-rich but can take 10-15 seconds to start. Phoenix aims for **sub-second startup** while retaining the core functionality players rely on.

## Features

- **Game Launching** - Browse for game directory, launch with one click
- **Version Detection** - SHA256-based build identification with SQLite caching
- **Automatic Updates** - Download and install updates with progress tracking
- **Smart Migration** - Preserves your mods, saves, tilesets, soundpacks, and fonts during updates
- **Save Backups** - Manual and automatic backup management with compression
- **Soundpack Manager** - Install, enable/disable, and delete soundpacks (ZIP, RAR, 7z)
- **Theme System** - 5 built-in color themes (Amber, Purple, Cyan, Green, Catppuccin)
- **Fast Updates** - Optimized update process (~18 seconds vs ~54 seconds naive approach)

## Download

See the [Releases](https://github.com/Vhari-Maven/phoenix/releases) page for the latest build.

## FAQ

### Where is my previous version?

It is stored in the `previous_version` directory inside your game directory.

### How does the launcher update my game?

1. Downloads the archive for the new version
2. Moves your current installation to `previous_version` (old backups are cleaned up in the background)
3. Extracts the new version to your game directory
4. Intelligently restores your content:
   - **Saves** - Copied from previous version (or left in place with `prevent_save_move` option)
   - **Mods** - Only custom mods are restored; official mods use the new version
   - **Tilesets** - Only custom tilesets are restored
   - **Soundpacks** - Only custom soundpacks are restored
   - **Fonts** - Only fonts not included in the new version are restored
   - **Config** - Your settings are preserved (excluding debug logs)

### I think the launcher deleted my files. What can I do?

Phoenix goes to great lengths not to delete important files. With default settings, files are always moved rather than deleted. Check these locations:

1. `previous_version` subdirectory in your game folder
2. Your system recycle bin

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

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
