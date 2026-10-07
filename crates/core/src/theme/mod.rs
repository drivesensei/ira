//! UI-neutral preset identity and file classification.
pub mod icons;
mod load;
mod palette;
#[cfg(test)]
use icons::IconSet;
#[cfg(test)]
use load::load_from;
#[cfg(test)]
use load::TermCaps;
pub use load::{
    load, load_with_persisted, resolve_icon_set, theme_file_path, Loaded, Loader, PresetSource,
    ThemeCapabilities,
};
pub use palette::{ChipStyle, Color, Rgba, Theme};

/// Built-in palettes, in `\` cycle order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreset {
    #[default]
    Mocha,
    Cyberpunk2077,
    GruvboxDark,
    Nord,
    Dracula,
    TokyoNight,
}

impl ThemePreset {
    /// Cycle order for the `\` action.
    pub const ALL: &'static [ThemePreset] = &[
        ThemePreset::Mocha,
        ThemePreset::Cyberpunk2077,
        ThemePreset::GruvboxDark,
        ThemePreset::Nord,
        ThemePreset::Dracula,
        ThemePreset::TokyoNight,
    ];

    /// Stable key used in `theme.toml` (`preset = "..."`) and the session
    /// state (`theme=...`).
    pub fn id(self) -> &'static str {
        match self {
            ThemePreset::Mocha => "mocha",
            ThemePreset::Cyberpunk2077 => "cyberpunk2077",
            ThemePreset::GruvboxDark => "gruvbox-dark",
            ThemePreset::Nord => "nord",
            ThemePreset::Dracula => "dracula",
            ThemePreset::TokyoNight => "tokyo-night",
        }
    }

    /// Human label for banners and panel titles.
    pub fn label(self) -> &'static str {
        match self {
            ThemePreset::Mocha => "Catppuccin Mocha",
            ThemePreset::Cyberpunk2077 => "Cyberpunk 2077",
            ThemePreset::GruvboxDark => "Gruvbox Dark",
            ThemePreset::Nord => "Nord",
            ThemePreset::Dracula => "Dracula",
            ThemePreset::TokyoNight => "Tokyo Night",
        }
    }

    /// The preset after `self` in [`ThemePreset::ALL`], wrapping around.
    pub fn next(self) -> ThemePreset {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Accepts ids plus a few friendly aliases; case-insensitive.
    pub fn parse(name: &str) -> Option<ThemePreset> {
        let key = name.trim().to_ascii_lowercase().replace('_', "-");
        match key.as_str() {
            "mocha" | "default" | "catppuccin" | "catppuccin-mocha" => Some(ThemePreset::Mocha),
            "cyberpunk" | "cyberpunk2077" | "cyberpunk-2077" | "2077" | "nightcity"
            | "night-city" => Some(ThemePreset::Cyberpunk2077),
            "gruvbox" | "gruvbox-dark" | "gruvboxdark" => Some(ThemePreset::GruvboxDark),
            "nord" => Some(ThemePreset::Nord),
            "dracula" => Some(ThemePreset::Dracula),
            "tokyo-night" | "tokyonight" | "tokyo" => Some(ThemePreset::TokyoNight),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
