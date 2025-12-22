//! UI-related application state

use egui_commonmark::CommonMarkCache;

use crate::ui::theme::Theme;

/// Application tabs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Main,
    Backups,
    Soundpacks,
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
