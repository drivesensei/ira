use gpui::WindowAppearance;
use ira_core::theme::{Theme, ThemePreset};

/// Both palettes arrive from the worker; appearance changes only select cached colors.
pub(super) struct Palette {
    pub configured: Theme,
    pub light: Option<Theme>,
    pub appearance: WindowAppearance,
}
impl Default for Palette {
    fn default() -> Self {
        Self {
            configured: ThemePreset::default().theme(),
            light: None,
            appearance: WindowAppearance::Dark,
        }
    }
}
impl Palette {
    pub fn selected(&self) -> Theme {
        match self.appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => {
                self.light.unwrap_or(self.configured)
            }
            WindowAppearance::Dark | WindowAppearance::VibrantDark => self.configured,
        }
    }
}

#[cfg(test)]
#[path = "appearance_tests.rs"]
mod tests;
