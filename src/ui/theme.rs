use eframe::egui::{self, Color32, Stroke, Visuals};
use serde::{Deserialize, Serialize};

/// Available theme presets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreset {
    #[default]
    Amber,
    Purple,
    Cyan,
    Green,
    Catppuccin,
}

impl ThemePreset {
    /// Get all available presets
    pub fn all() -> &'static [ThemePreset] {
        &[
            ThemePreset::Amber,
            ThemePreset::Purple,
            ThemePreset::Cyan,
            ThemePreset::Green,
            ThemePreset::Catppuccin,
        ]
    }

    /// Get display name for the preset
    pub fn name(&self) -> &'static str {
        match self {
            ThemePreset::Amber => "Amber",
            ThemePreset::Purple => "Purple",
            ThemePreset::Cyan => "Cyan",
            ThemePreset::Green => "Green",
            ThemePreset::Catppuccin => "Catppuccin Mocha",
        }
    }

    /// Get the theme colors for this preset
    pub fn theme(&self) -> Theme {
        match self {
            ThemePreset::Amber => Theme::amber(),
            ThemePreset::Purple => Theme::purple(),
            ThemePreset::Cyan => Theme::cyan(),
            ThemePreset::Green => Theme::green(),
            ThemePreset::Catppuccin => Theme::catppuccin(),
        }
    }
}

/// Theme color definitions
#[derive(Debug, Clone)]
pub struct Theme {
    // Base colors
    pub bg_darkest: Color32,
    pub bg_dark: Color32,
    pub bg_medium: Color32,
    pub bg_light: Color32,

    // Text colors
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    // Accent colors
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_muted: Color32,

    // Semantic colors
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,

    // UI element colors
    pub border: Color32,
    pub selection: Color32,
}

impl Theme {
    /// Amber theme - post-apocalyptic, matches CDDA aesthetic
    pub fn amber() -> Self {
        Self {
            bg_darkest: Color32::from_rgb(16, 16, 18),
            bg_dark: Color32::from_rgb(24, 24, 27),
            bg_medium: Color32::from_rgb(32, 32, 36),
            bg_light: Color32::from_rgb(48, 48, 54),

            text_primary: Color32::from_rgb(250, 250, 250),
            text_secondary: Color32::from_rgb(200, 200, 200),
            text_muted: Color32::from_rgb(140, 140, 140),

            accent: Color32::from_rgb(245, 158, 11),       // Amber-500
            accent_hover: Color32::from_rgb(251, 191, 36), // Amber-400
            accent_muted: Color32::from_rgb(180, 116, 8),  // Darker amber

            success: Color32::from_rgb(34, 197, 94),  // Green-500
            warning: Color32::from_rgb(234, 179, 8),  // Yellow-500
            error: Color32::from_rgb(239, 68, 68),    // Red-500

            border: Color32::from_rgb(63, 63, 70),
            selection: Color32::from_rgb(245, 158, 11).gamma_multiply(0.3),
        }
    }

    /// Purple theme - similar to old launcher
    pub fn purple() -> Self {
        Self {
            bg_darkest: Color32::from_rgb(22, 18, 32),
            bg_dark: Color32::from_rgb(30, 26, 46),
            bg_medium: Color32::from_rgb(42, 36, 62),
            bg_light: Color32::from_rgb(58, 50, 82),

            text_primary: Color32::from_rgb(250, 250, 255),
            text_secondary: Color32::from_rgb(200, 195, 220),
            text_muted: Color32::from_rgb(140, 135, 160),

            accent: Color32::from_rgb(168, 85, 247),        // Purple-500
            accent_hover: Color32::from_rgb(192, 132, 252), // Purple-400
            accent_muted: Color32::from_rgb(126, 58, 200),  // Darker purple

            success: Color32::from_rgb(74, 222, 128),  // Green-400
            warning: Color32::from_rgb(250, 204, 21),  // Yellow-400
            error: Color32::from_rgb(248, 113, 113),   // Red-400

            border: Color32::from_rgb(75, 65, 100),
            selection: Color32::from_rgb(168, 85, 247).gamma_multiply(0.3),
        }
    }

    /// Cyan theme - modern, techy
    pub fn cyan() -> Self {
        Self {
            bg_darkest: Color32::from_rgb(12, 20, 30),
            bg_dark: Color32::from_rgb(15, 23, 42),
            bg_medium: Color32::from_rgb(22, 33, 54),
            bg_light: Color32::from_rgb(35, 48, 70),

            text_primary: Color32::from_rgb(248, 250, 252),
            text_secondary: Color32::from_rgb(200, 210, 220),
            text_muted: Color32::from_rgb(130, 145, 160),

            accent: Color32::from_rgb(6, 182, 212),         // Cyan-500
            accent_hover: Color32::from_rgb(34, 211, 238),  // Cyan-400
            accent_muted: Color32::from_rgb(8, 140, 165),   // Darker cyan

            success: Color32::from_rgb(52, 211, 153),  // Emerald-400
            warning: Color32::from_rgb(251, 191, 36),  // Amber-400
            error: Color32::from_rgb(251, 113, 133),   // Rose-400

            border: Color32::from_rgb(51, 65, 85),
            selection: Color32::from_rgb(6, 182, 212).gamma_multiply(0.3),
        }
    }

    /// Green theme - terminal/survival aesthetic
    pub fn green() -> Self {
        Self {
            bg_darkest: Color32::from_rgb(12, 17, 14),
            bg_dark: Color32::from_rgb(20, 28, 22),
            bg_medium: Color32::from_rgb(28, 40, 32),
            bg_light: Color32::from_rgb(42, 58, 46),

            text_primary: Color32::from_rgb(240, 253, 244),
            text_secondary: Color32::from_rgb(190, 220, 200),
            text_muted: Color32::from_rgb(120, 150, 130),

            accent: Color32::from_rgb(34, 197, 94),         // Green-500
            accent_hover: Color32::from_rgb(74, 222, 128),  // Green-400
            accent_muted: Color32::from_rgb(22, 150, 70),   // Darker green

            success: Color32::from_rgb(74, 222, 128),   // Green-400
            warning: Color32::from_rgb(253, 224, 71),   // Yellow-300
            error: Color32::from_rgb(252, 165, 165),    // Red-300

            border: Color32::from_rgb(50, 70, 55),
            selection: Color32::from_rgb(34, 197, 94).gamma_multiply(0.3),
        }
    }

    /// Catppuccin Mocha theme - popular community theme
    pub fn catppuccin() -> Self {
        Self {
            bg_darkest: Color32::from_rgb(17, 17, 27),    // Crust
            bg_dark: Color32::from_rgb(24, 24, 37),       // Mantle
            bg_medium: Color32::from_rgb(30, 30, 46),     // Base
            bg_light: Color32::from_rgb(49, 50, 68),      // Surface0

            text_primary: Color32::from_rgb(205, 214, 244),   // Text
            text_secondary: Color32::from_rgb(186, 194, 222), // Subtext1
            text_muted: Color32::from_rgb(147, 153, 178),     // Overlay1

            accent: Color32::from_rgb(137, 180, 250),        // Blue
            accent_hover: Color32::from_rgb(180, 190, 254),  // Lavender
            accent_muted: Color32::from_rgb(116, 148, 204),  // Darker blue

            success: Color32::from_rgb(166, 227, 161),  // Green
            warning: Color32::from_rgb(249, 226, 175),  // Yellow
            error: Color32::from_rgb(243, 139, 168),    // Red

            border: Color32::from_rgb(69, 71, 90),  // Surface1
            selection: Color32::from_rgb(137, 180, 250).gamma_multiply(0.3),
        }
    }

    /// Apply this theme to egui's visuals
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = Visuals::dark();

        // Window and panel backgrounds
        visuals.window_fill = self.bg_dark;
        visuals.panel_fill = self.bg_dark;
        visuals.faint_bg_color = self.bg_medium;
        visuals.extreme_bg_color = self.bg_darkest;

        // Widget backgrounds
        visuals.widgets.noninteractive.bg_fill = self.bg_medium;
        visuals.widgets.noninteractive.weak_bg_fill = self.bg_light;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_secondary);

        // Inactive widgets
        visuals.widgets.inactive.bg_fill = self.bg_medium;
        visuals.widgets.inactive.weak_bg_fill = self.bg_light;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);

        // Hovered widgets
        visuals.widgets.hovered.bg_fill = self.bg_light;
        visuals.widgets.hovered.weak_bg_fill = self.bg_light;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.text_primary);

        // Active/pressed widgets
        visuals.widgets.active.bg_fill = self.accent_muted;
        visuals.widgets.active.weak_bg_fill = self.accent_muted;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.accent_hover);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.text_primary);

        // Open widgets (dropdowns, etc)
        visuals.widgets.open.bg_fill = self.bg_light;
        visuals.widgets.open.weak_bg_fill = self.bg_light;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, self.text_primary);

        // Selection
        visuals.selection.bg_fill = self.selection;
        visuals.selection.stroke = Stroke::new(1.0, self.accent);

        // Hyperlinks
        visuals.hyperlink_color = self.accent;

        // Window styling
        visuals.window_stroke = Stroke::new(1.0, self.border);
        visuals.window_shadow = egui::epaint::Shadow::NONE;

        // Popup styling
        visuals.popup_shadow = egui::epaint::Shadow::NONE;

        ctx.set_visuals(visuals);
    }
}
