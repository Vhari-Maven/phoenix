//! UI-related application state

use egui_commonmark::CommonMarkCache;

use crate::ui::theme::Theme;

/// Application tabs representing the main navigation sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// Main tab: game info, version display, update controls, launch button
    #[default]
    Main,
    /// Backups tab: create, restore, and manage save backups
    Backups,
    /// Soundpacks tab: install soundpacks from repository or local files
    Soundpacks,
    /// Settings tab: theme selection, update preferences, backup options
    Settings,
}

/// UI-related state
pub struct UiState {
    /// Cache for markdown rendering
    pub markdown_cache: CommonMarkCache,
    /// Current theme
    pub current_theme: Theme,
    /// Currently selected tab
    pub active_tab: Tab,
    /// Whether theme needs to be applied
    pub theme_dirty: bool,
    /// Whether to show the About dialog
    pub show_about_dialog: bool,
}

impl UiState {
    /// Create a new UiState with the given theme
    pub fn new(theme: Theme) -> Self {
        Self {
            markdown_cache: CommonMarkCache::default(),
            current_theme: theme,
            active_tab: Tab::default(),
            theme_dirty: true, // Apply theme on first frame
            show_about_dialog: false,
        }
    }
}
