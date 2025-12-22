//! UI modules for Phoenix launcher
//!
//! This module contains the extracted UI rendering code, organized by tab.

mod backups_tab;
mod components;
mod main_tab;
mod settings_tab;
mod soundpacks_tab;
pub mod theme;

pub use backups_tab::render_backups_tab;
pub use components::{render_about_dialog, render_tab};
pub use main_tab::render_main_tab;
pub use settings_tab::render_settings_tab;
pub use soundpacks_tab::render_soundpacks_tab;
