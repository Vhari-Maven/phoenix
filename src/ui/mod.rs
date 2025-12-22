//! UI modules for Phoenix launcher
//!
//! This module contains the extracted UI rendering code, organized by tab.

mod backups_tab;
mod main_tab;
mod settings_tab;
mod soundpacks_tab;

pub use backups_tab::render_backups_tab;
pub use main_tab::render_main_tab;
pub use settings_tab::render_settings_tab;
pub use soundpacks_tab::render_soundpacks_tab;
