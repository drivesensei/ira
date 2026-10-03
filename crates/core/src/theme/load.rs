use super::{icons::IconSet, ChipStyle, Theme, ThemePreset};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeCapabilities {
    pub truecolor: bool,
    pub nerd_font: bool,
    pub wide_emoji: bool,
}
impl Default for ThemeCapabilities {
    fn default() -> Self {
        Self {
            truecolor: true,
            nerd_font: false,
            wide_emoji: false,
        }
    }
}
#[cfg(test)]
pub(super) type TermCaps = ThemeCapabilities;
/// Optional user theme file. Every field is optional; unknown keys are
/// ignored; invalid colors keep the default.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ThemeFile {
    /// Built-in palette name (`mocha`, `cyberpunk2077`). Color keys below
    /// still override individual slots.
    pub(super) preset: Option<String>,
    pub(super) icons: Option<String>,
    /// `"outline"` | `"rounded"` | `"square"`; default follows the icon set.
    pub(super) chips: Option<String>,
    pub(super) bg: Option<String>,
    pub(super) surface: Option<String>,
    pub(super) surface_alt: Option<String>,
    pub(super) border: Option<String>,
    pub(super) border_active: Option<String>,
    pub(super) text: Option<String>,
    pub(super) text_muted: Option<String>,
    pub(super) accent: Option<String>,
    pub(super) key_fg: Option<String>,
    pub(super) key_bg: Option<String>,
    pub(super) success: Option<String>,
    pub(super) warning: Option<String>,
    pub(super) error: Option<String>,
    pub(super) info: Option<String>,
    pub(super) cursor_bg: Option<String>,
    pub(super) cursor_fg: Option<String>,
    pub(super) selection: Option<String>,
    pub(super) dir: Option<String>,
    pub(super) hidden: Option<String>,
    pub(super) shadow: Option<String>,
    #[serde(default)]
    pub(super) files: ThemeFileFiles,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct ThemeFileFiles {
    pub(super) image: Option<String>,
    pub(super) video: Option<String>,
    pub(super) audio: Option<String>,
    pub(super) archive: Option<String>,
    pub(super) code: Option<String>,
    pub(super) document: Option<String>,
    pub(super) executable: Option<String>,
    pub(super) data: Option<String>,
}

/// Where the active preset came from (for `--check-terminal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetSource {
    /// `theme=` in the session state (last `\` press).
    State,
    /// `preset =` in `theme.toml`.
    Toml,
    /// Neither: the built-in default.
    Default,
}

/// Everything needed to (re)build a [`Theme`] for any preset at runtime:
/// the detected terminal caps and the user's `theme.toml` overrides.
/// Held by `App` so `\` can switch presets without re-reading disk.
#[derive(Debug, Default)]
pub struct Loader {
    caps: ThemeCapabilities,
    overrides: ThemeFile,
    /// Auto default for the chip style when `theme.toml` has no `chips`
    /// key: `Outline` once the Nerd icon set is active (the caps are Nerd
    /// Font Powerline glyphs), `Square` otherwise.
    chips_auto: ChipStyle,
    nerd_glyphs: bool,
}

impl Loader {
    /// Reads `~/.config/ira/theme.toml` once on a worker, using host-supplied
    /// font/capability results plus the preserved NERD_FONT presence override.
    pub fn from_env(mut caps: ThemeCapabilities) -> Self {
        if std::env::var("NERD_FONT").is_ok() {
            caps.nerd_font = true;
        }
        let file = theme_file_path().and_then(|p| fs::read_to_string(p).ok());
        Self::from_toml(&file.unwrap_or_default(), caps)
    }

    /// Builds a loader from an explicit TOML body and caps (tests).
    pub fn from_toml(toml_src: &str, caps: ThemeCapabilities) -> Self {
        let overrides = if toml_src.trim().is_empty() {
            ThemeFile::default()
        } else {
            toml::from_str::<ThemeFile>(toml_src).unwrap_or_default()
        };
        Self {
            caps,
            overrides,
            chips_auto: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Ties the chip-style default (and glyph availability) to the
    /// resolved icon set.
    pub fn set_icon_set(&mut self, icons: IconSet) {
        self.nerd_glyphs = icons == IconSet::Nerd;
        self.chips_auto = match icons {
            IconSet::Nerd => ChipStyle::Outline,
            // Emoji come from a fallback font; the Powerline caps do not.
            IconSet::Emoji | IconSet::Unicode => ChipStyle::Square,
        };
    }

    /// `chips = "..."` from `theme.toml`, else the auto default.
    pub fn chip_style(&self) -> ChipStyle {
        self.overrides
            .chips
            .as_deref()
            .and_then(ChipStyle::parse)
            .unwrap_or(self.chips_auto)
    }

    /// Detected terminal caps.
    pub fn caps(&self) -> &ThemeCapabilities {
        &self.caps
    }

    /// `preset = "..."` from `theme.toml`, if valid.
    pub fn toml_preset(&self) -> Option<ThemePreset> {
        self.overrides
            .preset
            .as_deref()
            .and_then(ThemePreset::parse)
    }

    /// `icons = "..."` from `theme.toml`.
    pub fn icons_pref(&self) -> Option<&str> {
        self.overrides.icons.as_deref()
    }

    /// Preset palette → TOML per-key overrides → quantization for caps.
    pub fn theme_for(&self, preset: ThemePreset) -> Theme {
        let mut theme = preset.theme();
        theme.apply_overrides(&self.overrides);
        theme.adapt(&self.caps);
        theme.chips = self.chip_style();
        theme.nerd_glyphs = self.nerd_glyphs;
        theme
    }

    /// Startup preset: persisted session state > `theme.toml` > default.
    pub fn resolve_preset(&self, persisted: Option<&str>) -> (ThemePreset, PresetSource) {
        if let Some(p) = persisted.and_then(ThemePreset::parse) {
            return (p, PresetSource::State);
        }
        if let Some(p) = self.toml_preset() {
            return (p, PresetSource::Toml);
        }
        (ThemePreset::default(), PresetSource::Default)
    }

    /// Icon set: `IRA_ICONS` > `theme.toml` > auto (font probe).
    pub fn resolve_icons(&self, ira_icons: Option<&str>) -> IconSet {
        resolve_icon_set(&self.caps, self.icons_pref(), ira_icons)
    }
}

/// Result of startup theme resolution.
#[derive(Debug)]
pub struct Loaded {
    pub theme: Theme,
    pub icons: IconSet,
    pub preset: ThemePreset,
    pub source: PresetSource,
    pub loader: Loader,
}

/// Loads the theme and icon set: defaults, then `~/.config/ira/theme.toml`,
/// then `adapt()` for the detected terminal, then icon-set resolution
/// (`IRA_ICONS` > toml `icons` > auto via the font probe).
pub fn load(caps: ThemeCapabilities) -> (Theme, IconSet) {
    let l = load_with_persisted(None, caps);
    (l.theme, l.icons)
}

/// Full startup resolution using the persisted theme preference
/// (`theme=` in the state file) when toml does not override. The icon set
/// is never persisted: it is re-detected from the fonts on every launch.
pub fn load_with_persisted(persisted_theme: Option<&str>, caps: ThemeCapabilities) -> Loaded {
    let ira_icons = std::env::var("IRA_ICONS").ok();
    let mut loader = Loader::from_env(caps);
    let (preset, source) = loader.resolve_preset(persisted_theme);
    let icons = loader.resolve_icons(ira_icons.as_deref());
    loader.set_icon_set(icons);
    let theme = loader.theme_for(preset);
    Loaded {
        theme,
        icons,
        preset,
        source,
        loader,
    }
}

/// Test entry: parse an optional TOML body against already-detected caps.
#[cfg(test)]
pub(super) fn load_from(
    toml_src: &str,
    caps: &ThemeCapabilities,
    ira_icons: Option<&str>,
) -> (Theme, IconSet) {
    let mut loader = Loader::from_toml(toml_src, caps.clone());
    let (preset, _) = loader.resolve_preset(None);
    let icons = loader.resolve_icons(ira_icons);
    loader.set_icon_set(icons);
    (loader.theme_for(preset), icons)
}

/// Path to `~/.config/ira/theme.toml`.
pub fn theme_file_path() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("ira").join("theme.toml"))
}

/// `IRA_ICONS` env beats `theme.toml`, which beats auto-detection (the
/// font probe behind `caps.nerd_font`).
pub fn resolve_icon_set(
    caps: &ThemeCapabilities,
    toml_icons: Option<&str>,
    ira_icons: Option<&str>,
) -> IconSet {
    if let Some(set) = parse_icon_pref(ira_icons) {
        return set;
    }
    if let Some(set) = parse_icon_pref(toml_icons) {
        return set;
    }
    // Auto: the richest set the terminal can actually draw.
    if caps.nerd_font {
        IconSet::Nerd
    } else if caps.wide_emoji {
        IconSet::Emoji
    } else {
        IconSet::Unicode
    }
}

fn parse_icon_pref(raw: Option<&str>) -> Option<IconSet> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "nerd" => Some(IconSet::Nerd),
        "emoji" => Some(IconSet::Emoji),
        "unicode" => Some(IconSet::Unicode),
        "auto" => None,
        _ => None,
    }
}

impl Loader {
    pub fn from_path(path: &Path, caps: ThemeCapabilities) -> Self {
        Self::from_toml(&fs::read_to_string(path).unwrap_or_default(), caps)
    }
}
