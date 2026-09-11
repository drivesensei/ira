//! Semantic theme, terminal-capability detection, and file-type icons.

pub mod caps;
pub mod icons;

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use ratatui::style::Color;
use serde::Deserialize;

use caps::{detect, detect_from_env, rgb_to_indexed, EnvSnapshot, TermCaps};
use icons::IconSet;

/// Resolved palette used by every panel and dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub border: Color,
    pub border_active: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub key_fg: Color,
    pub key_bg: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub cursor_bg: Color,
    pub cursor_fg: Color,
    pub selection: Color,
    pub dir: Color,
    pub hidden: Color,
    pub shadow: Color,
    pub image: Color,
    pub video: Color,
    pub audio: Color,
    pub archive: Color,
    pub code: Color,
    pub document: Color,
    pub executable: Color,
    pub data: Color,
    /// How shortcut keys are framed (see [`ChipStyle`]).
    pub chips: ChipStyle,
    /// Whether Nerd Font PUA glyphs (Powerline caps) may be emitted by
    /// chrome; mirrors the active icon set.
    pub nerd_glyphs: bool,
}

/// Visual style of the shortcut-key chips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipStyle {
    /// ` key ` in `key_fg` on a filled `key_bg` rectangle.
    #[default]
    Square,
    /// Filled body plus thick half-circle caps (Nerd Font Powerline).
    Rounded,
    /// No fill: bold `accent` key between thin rounded caps drawn in
    /// `border_active`, echoing the panels' rounded borders. Falls back to
    /// `(key)` without a Nerd Font.
    Outline,
}

impl ChipStyle {
    /// Cells a chip adds around its key text.
    pub fn extra_width(self) -> u16 {
        match self {
            ChipStyle::Square | ChipStyle::Outline => 2,
            ChipStyle::Rounded => 4,
        }
    }

    /// Accepts the `chips = "..."` values from `theme.toml`.
    pub fn parse(raw: &str) -> Option<ChipStyle> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "square" | "plain" | "filled" => Some(ChipStyle::Square),
            "rounded" | "pill" => Some(ChipStyle::Rounded),
            "outline" | "outlined" | "border" => Some(ChipStyle::Outline),
            _ => None,
        }
    }
}

impl Default for Theme {
    /// Catppuccin Mocha-inspired dark palette (truecolor hex values).
    fn default() -> Self {
        Self::mocha()
    }
}

impl Theme {
    /// Default dark palette (Catppuccin Mocha).
    pub fn mocha() -> Self {
        Self {
            bg: rgb(0x1e, 0x1e, 0x2e),
            surface: rgb(0x31, 0x32, 0x44),
            surface_alt: rgb(0x45, 0x47, 0x5a),
            border: rgb(0x45, 0x47, 0x5a),
            border_active: rgb(0x89, 0xb4, 0xfa),
            text: rgb(0xcd, 0xd6, 0xf4),
            text_muted: rgb(0x6c, 0x70, 0x86),
            accent: rgb(0x89, 0xb4, 0xfa),
            key_fg: rgb(0x1e, 0x1e, 0x2e),
            key_bg: rgb(0x89, 0xb4, 0xfa),
            success: rgb(0xa6, 0xe3, 0xa1),
            warning: rgb(0xf9, 0xe2, 0xaf),
            error: rgb(0xf3, 0x8b, 0xa8),
            info: rgb(0x89, 0xdc, 0xeb),
            cursor_bg: rgb(0x45, 0x47, 0x5a),
            cursor_fg: rgb(0xcd, 0xd6, 0xf4),
            selection: rgb(0xcb, 0xa6, 0xf7),
            dir: rgb(0x89, 0xb4, 0xfa),
            hidden: rgb(0x6c, 0x70, 0x86),
            shadow: rgb(0x11, 0x11, 0x1b),
            image: rgb(0xf9, 0xe2, 0xaf),
            video: rgb(0xcb, 0xa6, 0xf7),
            audio: rgb(0xf5, 0xc2, 0xe7),
            archive: rgb(0xfa, 0xb3, 0x87),
            code: rgb(0xa6, 0xe3, 0xa1),
            document: rgb(0x89, 0xdc, 0xeb),
            executable: rgb(0xf3, 0x8b, 0xa8),
            data: rgb(0x94, 0xe2, 0xd5),
            chips: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Night City / Cyberpunk 2077 UI: yellow on black, cyan secondary,
    /// trauma-team red for danger.
    pub fn cyberpunk2077() -> Self {
        Self {
            bg: rgb(0x0a, 0x0a, 0x0c),
            surface: rgb(0x1a, 0x12, 0x0c),
            surface_alt: rgb(0x2a, 0x1c, 0x10),
            border: rgb(0x4a, 0x3a, 0x10),
            border_active: rgb(0xfc, 0xee, 0x0a),
            text: rgb(0xf5, 0xf0, 0xc8),
            text_muted: rgb(0x7a, 0x70, 0x48),
            accent: rgb(0xfc, 0xee, 0x0a),
            key_fg: rgb(0x0a, 0x0a, 0x0c),
            key_bg: rgb(0xfc, 0xee, 0x0a),
            success: rgb(0x00, 0xf0, 0xff),
            warning: rgb(0xff, 0x9f, 0x1c),
            error: rgb(0xff, 0x00, 0x3c),
            info: rgb(0x00, 0xf0, 0xff),
            cursor_bg: rgb(0xfc, 0xee, 0x0a),
            cursor_fg: rgb(0x0a, 0x0a, 0x0c),
            selection: rgb(0xff, 0x00, 0x3c),
            dir: rgb(0x00, 0xf0, 0xff),
            hidden: rgb(0x5a, 0x54, 0x38),
            shadow: rgb(0x00, 0x00, 0x00),
            image: rgb(0xff, 0x9f, 0x1c),
            video: rgb(0xff, 0x2a, 0x6d),
            audio: rgb(0x00, 0xf0, 0xff),
            archive: rgb(0xff, 0x6b, 0x35),
            code: rgb(0x39, 0xff, 0x14),
            document: rgb(0xf5, 0xf0, 0xc8),
            executable: rgb(0xff, 0x00, 0x3c),
            data: rgb(0x00, 0xf0, 0xff),
            chips: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Gruvbox Dark (medium contrast): warm retro palette.
    pub fn gruvbox_dark() -> Self {
        Self {
            bg: rgb(0x28, 0x28, 0x28),
            surface: rgb(0x3c, 0x38, 0x36),
            surface_alt: rgb(0x50, 0x49, 0x45),
            border: rgb(0x50, 0x49, 0x45),
            border_active: rgb(0xfa, 0xbd, 0x2f),
            text: rgb(0xeb, 0xdb, 0xb2),
            text_muted: rgb(0x92, 0x83, 0x74),
            accent: rgb(0xfa, 0xbd, 0x2f),
            key_fg: rgb(0x28, 0x28, 0x28),
            key_bg: rgb(0xfa, 0xbd, 0x2f),
            success: rgb(0xb8, 0xbb, 0x26),
            warning: rgb(0xfe, 0x80, 0x19),
            error: rgb(0xfb, 0x49, 0x34),
            info: rgb(0x83, 0xa5, 0x98),
            cursor_bg: rgb(0x50, 0x49, 0x45),
            cursor_fg: rgb(0xeb, 0xdb, 0xb2),
            selection: rgb(0xd3, 0x86, 0x9b),
            dir: rgb(0x83, 0xa5, 0x98),
            hidden: rgb(0x92, 0x83, 0x74),
            shadow: rgb(0x1d, 0x20, 0x21),
            image: rgb(0xfa, 0xbd, 0x2f),
            video: rgb(0xd3, 0x86, 0x9b),
            audio: rgb(0x8e, 0xc0, 0x7c),
            archive: rgb(0xfe, 0x80, 0x19),
            code: rgb(0xb8, 0xbb, 0x26),
            document: rgb(0x83, 0xa5, 0x98),
            executable: rgb(0xfb, 0x49, 0x34),
            data: rgb(0x8e, 0xc0, 0x7c),
            chips: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Nord: arctic, bluish palette.
    pub fn nord() -> Self {
        Self {
            bg: rgb(0x2e, 0x34, 0x40),
            surface: rgb(0x3b, 0x42, 0x52),
            surface_alt: rgb(0x43, 0x4c, 0x5e),
            border: rgb(0x4c, 0x56, 0x6a),
            border_active: rgb(0x88, 0xc0, 0xd0),
            text: rgb(0xec, 0xef, 0xf4),
            text_muted: rgb(0x7b, 0x88, 0xa1),
            accent: rgb(0x88, 0xc0, 0xd0),
            key_fg: rgb(0x2e, 0x34, 0x40),
            key_bg: rgb(0x88, 0xc0, 0xd0),
            success: rgb(0xa3, 0xbe, 0x8c),
            warning: rgb(0xeb, 0xcb, 0x8b),
            error: rgb(0xbf, 0x61, 0x6a),
            info: rgb(0x81, 0xa1, 0xc1),
            cursor_bg: rgb(0x43, 0x4c, 0x5e),
            cursor_fg: rgb(0xec, 0xef, 0xf4),
            selection: rgb(0xb4, 0x8e, 0xad),
            dir: rgb(0x81, 0xa1, 0xc1),
            hidden: rgb(0x7b, 0x88, 0xa1),
            shadow: rgb(0x24, 0x29, 0x33),
            image: rgb(0xeb, 0xcb, 0x8b),
            video: rgb(0xb4, 0x8e, 0xad),
            audio: rgb(0x8f, 0xbc, 0xbb),
            archive: rgb(0xd0, 0x87, 0x70),
            code: rgb(0xa3, 0xbe, 0x8c),
            document: rgb(0x88, 0xc0, 0xd0),
            executable: rgb(0xbf, 0x61, 0x6a),
            data: rgb(0x8f, 0xbc, 0xbb),
            chips: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Dracula: purple/pink on a deep indigo background.
    pub fn dracula() -> Self {
        Self {
            bg: rgb(0x28, 0x2a, 0x36),
            surface: rgb(0x34, 0x37, 0x46),
            surface_alt: rgb(0x44, 0x47, 0x5a),
            border: rgb(0x44, 0x47, 0x5a),
            border_active: rgb(0xbd, 0x93, 0xf9),
            text: rgb(0xf8, 0xf8, 0xf2),
            text_muted: rgb(0x62, 0x72, 0xa4),
            accent: rgb(0xbd, 0x93, 0xf9),
            key_fg: rgb(0x28, 0x2a, 0x36),
            key_bg: rgb(0xbd, 0x93, 0xf9),
            success: rgb(0x50, 0xfa, 0x7b),
            warning: rgb(0xf1, 0xfa, 0x8c),
            error: rgb(0xff, 0x55, 0x55),
            info: rgb(0x8b, 0xe9, 0xfd),
            cursor_bg: rgb(0x44, 0x47, 0x5a),
            cursor_fg: rgb(0xf8, 0xf8, 0xf2),
            selection: rgb(0xff, 0x79, 0xc6),
            dir: rgb(0x8b, 0xe9, 0xfd),
            hidden: rgb(0x62, 0x72, 0xa4),
            shadow: rgb(0x1e, 0x1f, 0x29),
            image: rgb(0xf1, 0xfa, 0x8c),
            video: rgb(0xff, 0x79, 0xc6),
            audio: rgb(0xbd, 0x93, 0xf9),
            archive: rgb(0xff, 0xb8, 0x6c),
            code: rgb(0x50, 0xfa, 0x7b),
            document: rgb(0x8b, 0xe9, 0xfd),
            executable: rgb(0xff, 0x55, 0x55),
            data: rgb(0x8b, 0xe9, 0xfd),
            chips: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Tokyo Night: cool night-city blues and violets.
    pub fn tokyo_night() -> Self {
        Self {
            bg: rgb(0x1a, 0x1b, 0x26),
            surface: rgb(0x24, 0x28, 0x3b),
            surface_alt: rgb(0x2f, 0x33, 0x4d),
            border: rgb(0x3b, 0x40, 0x61),
            border_active: rgb(0x7a, 0xa2, 0xf7),
            text: rgb(0xc0, 0xca, 0xf5),
            text_muted: rgb(0x56, 0x5f, 0x89),
            accent: rgb(0x7a, 0xa2, 0xf7),
            key_fg: rgb(0x1a, 0x1b, 0x26),
            key_bg: rgb(0x7a, 0xa2, 0xf7),
            success: rgb(0x9e, 0xce, 0x6a),
            warning: rgb(0xe0, 0xaf, 0x68),
            error: rgb(0xf7, 0x76, 0x8e),
            info: rgb(0x7d, 0xcf, 0xff),
            cursor_bg: rgb(0x2f, 0x33, 0x4d),
            cursor_fg: rgb(0xc0, 0xca, 0xf5),
            selection: rgb(0xbb, 0x9a, 0xf7),
            dir: rgb(0x7a, 0xa2, 0xf7),
            hidden: rgb(0x56, 0x5f, 0x89),
            shadow: rgb(0x11, 0x12, 0x1b),
            image: rgb(0xe0, 0xaf, 0x68),
            video: rgb(0xbb, 0x9a, 0xf7),
            audio: rgb(0xff, 0x9e, 0x64),
            archive: rgb(0xff, 0x9e, 0x64),
            code: rgb(0x9e, 0xce, 0x6a),
            document: rgb(0x7d, 0xcf, 0xff),
            executable: rgb(0xf7, 0x76, 0x8e),
            data: rgb(0x2a, 0xc3, 0xde),
            chips: ChipStyle::Square,
            nerd_glyphs: false,
        }
    }

    /// Built-in preset from a `theme.toml` `preset = "..."` value.
    pub fn from_preset(name: &str) -> Option<Self> {
        ThemePreset::parse(name).map(ThemePreset::theme)
    }
}

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

    /// The preset's base palette (truecolor, before overrides/quantization).
    pub fn theme(self) -> Theme {
        match self {
            ThemePreset::Mocha => Theme::mocha(),
            ThemePreset::Cyberpunk2077 => Theme::cyberpunk2077(),
            ThemePreset::GruvboxDark => Theme::gruvbox_dark(),
            ThemePreset::Nord => Theme::nord(),
            ThemePreset::Dracula => Theme::dracula(),
            ThemePreset::TokyoNight => Theme::tokyo_night(),
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// Optional user theme file. Every field is optional; unknown keys are
/// ignored; invalid colors keep the default.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThemeFile {
    /// Built-in palette name (`mocha`, `cyberpunk2077`). Color keys below
    /// still override individual slots.
    preset: Option<String>,
    icons: Option<String>,
    /// `"outline"` | `"rounded"` | `"square"`; default follows the icon set.
    chips: Option<String>,
    bg: Option<String>,
    surface: Option<String>,
    surface_alt: Option<String>,
    border: Option<String>,
    border_active: Option<String>,
    text: Option<String>,
    text_muted: Option<String>,
    accent: Option<String>,
    key_fg: Option<String>,
    key_bg: Option<String>,
    success: Option<String>,
    warning: Option<String>,
    error: Option<String>,
    info: Option<String>,
    cursor_bg: Option<String>,
    cursor_fg: Option<String>,
    selection: Option<String>,
    dir: Option<String>,
    hidden: Option<String>,
    shadow: Option<String>,
    #[serde(default)]
    files: ThemeFileFiles,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ThemeFileFiles {
    image: Option<String>,
    video: Option<String>,
    audio: Option<String>,
    archive: Option<String>,
    code: Option<String>,
    document: Option<String>,
    executable: Option<String>,
    data: Option<String>,
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
    caps: TermCaps,
    overrides: ThemeFile,
    /// Auto default for the chip style when `theme.toml` has no `chips`
    /// key: `Outline` once the Nerd icon set is active (the caps are Nerd
    /// Font Powerline glyphs), `Square` otherwise.
    chips_auto: ChipStyle,
    nerd_glyphs: bool,
}

impl Loader {
    /// Reads `~/.config/ira/theme.toml` (once) and detects caps.
    pub fn from_env() -> Self {
        let env = EnvSnapshot::from_os();
        let caps = detect_from_env(&env);
        let file = theme_file_path().and_then(|p| fs::read_to_string(p).ok());
        Self::from_toml(&file.unwrap_or_default(), caps)
    }

    /// Builds a loader from an explicit TOML body and caps (tests).
    pub fn from_toml(toml_src: &str, caps: TermCaps) -> Self {
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
            IconSet::Unicode => ChipStyle::Square,
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
    pub fn caps(&self) -> &TermCaps {
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

    /// Icon set: `IRA_ICONS` > `theme.toml` > persisted > auto.
    pub fn resolve_icons(&self, ira_icons: Option<&str>, persisted: Option<&str>) -> IconSet {
        resolve_icon_set(&self.caps, self.icons_pref(), ira_icons, persisted)
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
/// (`IRA_ICONS` > toml `icons` > persisted session > auto).
pub fn load() -> (Theme, IconSet) {
    let l = load_with_persisted(None, None);
    (l.theme, l.icons)
}

/// Full startup resolution using persisted session preferences
/// (`icons=` / `theme=` in the state file) when env/toml do not override.
pub fn load_with_persisted(persisted_icons: Option<&str>, persisted_theme: Option<&str>) -> Loaded {
    let env = EnvSnapshot::from_os();
    let mut loader = Loader::from_env();
    let (preset, source) = loader.resolve_preset(persisted_theme);
    let icons = loader.resolve_icons(env.ira_icons.as_deref(), persisted_icons);
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
fn load_from(
    toml_src: &str,
    caps: &TermCaps,
    ira_icons: Option<&str>,
    persisted_icons: Option<&str>,
) -> (Theme, IconSet) {
    let mut loader = Loader::from_toml(toml_src, *caps);
    let (preset, _) = loader.resolve_preset(None);
    let icons = loader.resolve_icons(ira_icons, persisted_icons);
    loader.set_icon_set(icons);
    (loader.theme_for(preset), icons)
}

/// Path to `~/.config/ira/theme.toml`.
pub fn theme_file_path() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("ira").join("theme.toml"))
}

/// Live-environment caps (wrapper so callers do not import `caps` for the
/// common case).
pub fn caps() -> TermCaps {
    detect()
}

impl Theme {
    /// Quantizes every RGB color to the nearest xterm-256 index when the
    /// terminal does not support truecolor. Render code never branches.
    pub fn adapt(&mut self, caps: &TermCaps) {
        if caps.truecolor {
            return;
        }
        for color in self.all_colors_mut() {
            *color = quantize(*color);
        }
    }

    /// Applies a (possibly partial) TOML override. Invalid colors are skipped.
    fn apply_overrides(&mut self, file: &ThemeFile) {
        apply(&mut self.bg, file.bg.as_deref());
        apply(&mut self.surface, file.surface.as_deref());
        apply(&mut self.surface_alt, file.surface_alt.as_deref());
        apply(&mut self.border, file.border.as_deref());
        apply(&mut self.border_active, file.border_active.as_deref());
        apply(&mut self.text, file.text.as_deref());
        apply(&mut self.text_muted, file.text_muted.as_deref());
        apply(&mut self.accent, file.accent.as_deref());
        apply(&mut self.key_fg, file.key_fg.as_deref());
        apply(&mut self.key_bg, file.key_bg.as_deref());
        apply(&mut self.success, file.success.as_deref());
        apply(&mut self.warning, file.warning.as_deref());
        apply(&mut self.error, file.error.as_deref());
        apply(&mut self.info, file.info.as_deref());
        apply(&mut self.cursor_bg, file.cursor_bg.as_deref());
        apply(&mut self.cursor_fg, file.cursor_fg.as_deref());
        apply(&mut self.selection, file.selection.as_deref());
        apply(&mut self.dir, file.dir.as_deref());
        apply(&mut self.hidden, file.hidden.as_deref());
        apply(&mut self.shadow, file.shadow.as_deref());
        apply(&mut self.image, file.files.image.as_deref());
        apply(&mut self.video, file.files.video.as_deref());
        apply(&mut self.audio, file.files.audio.as_deref());
        apply(&mut self.archive, file.files.archive.as_deref());
        apply(&mut self.code, file.files.code.as_deref());
        apply(&mut self.document, file.files.document.as_deref());
        apply(&mut self.executable, file.files.executable.as_deref());
        apply(&mut self.data, file.files.data.as_deref());
    }

    fn all_colors_mut(&mut self) -> [&mut Color; 28] {
        [
            &mut self.bg,
            &mut self.surface,
            &mut self.surface_alt,
            &mut self.border,
            &mut self.border_active,
            &mut self.text,
            &mut self.text_muted,
            &mut self.accent,
            &mut self.key_fg,
            &mut self.key_bg,
            &mut self.success,
            &mut self.warning,
            &mut self.error,
            &mut self.info,
            &mut self.cursor_bg,
            &mut self.cursor_fg,
            &mut self.selection,
            &mut self.dir,
            &mut self.hidden,
            &mut self.shadow,
            &mut self.image,
            &mut self.video,
            &mut self.audio,
            &mut self.archive,
            &mut self.code,
            &mut self.document,
            &mut self.executable,
            &mut self.data,
        ]
    }

    #[cfg(test)]
    fn all_colors(&self) -> [Color; 28] {
        [
            self.bg,
            self.surface,
            self.surface_alt,
            self.border,
            self.border_active,
            self.text,
            self.text_muted,
            self.accent,
            self.key_fg,
            self.key_bg,
            self.success,
            self.warning,
            self.error,
            self.info,
            self.cursor_bg,
            self.cursor_fg,
            self.selection,
            self.dir,
            self.hidden,
            self.shadow,
            self.image,
            self.video,
            self.audio,
            self.archive,
            self.code,
            self.document,
            self.executable,
            self.data,
        ]
    }

    /// Color for a file-type category.
    pub fn color_for(&self, cat: icons::FileCategory) -> Color {
        use icons::FileCategory::*;
        match cat {
            Folder => self.dir,
            Text => self.document,
            Code => self.code,
            Image => self.image,
            Video => self.video,
            Audio => self.audio,
            Archive => self.archive,
            Document | Pdf | Spreadsheet => self.document,
            Executable => self.executable,
            Data | DiskImage => self.data,
            Other => self.text,
        }
    }
}

fn apply(slot: &mut Color, raw: Option<&str>) {
    if let Some(c) = raw.and_then(parse_color) {
        *slot = c;
    }
}

fn parse_color(s: &str) -> Option<Color> {
    Color::from_str(s.trim()).ok()
}

fn quantize(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Indexed(rgb_to_indexed(r, g, b)),
        other => other,
    }
}

/// `IRA_ICONS` env beats `theme.toml`, which beats a persisted session
/// preference, which beats auto-detection.
pub fn resolve_icon_set(
    caps: &TermCaps,
    toml_icons: Option<&str>,
    ira_icons: Option<&str>,
    persisted_icons: Option<&str>,
) -> IconSet {
    if let Some(set) = parse_icon_pref(ira_icons) {
        return set;
    }
    if let Some(set) = parse_icon_pref(toml_icons) {
        return set;
    }
    if let Some(set) = parse_icon_pref(persisted_icons) {
        return set;
    }
    if caps.nerd_font {
        IconSet::Nerd
    } else {
        IconSet::Unicode
    }
}

fn parse_icon_pref(raw: Option<&str>) -> Option<IconSet> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "nerd" => Some(IconSet::Nerd),
        "unicode" => Some(IconSet::Unicode),
        "auto" => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_toml_overrides_only_given_keys() {
        let caps = TermCaps {
            truecolor: true,
            nerd_font: false,
        };
        let src = r##"
            accent = "#ff0000"
            error = "red"
            [files]
            image = "#00ff00"
        "##;
        let (theme, icons) = load_from(src, &caps, None, None);
        assert_eq!(theme.accent, Color::Rgb(255, 0, 0));
        assert_eq!(theme.error, Color::Red);
        assert_eq!(theme.image, Color::Rgb(0, 255, 0));
        // Untouched key keeps the default.
        assert_eq!(theme.bg, Theme::default().bg);
        assert_eq!(icons, IconSet::Unicode);
    }

    #[test]
    fn invalid_colors_and_bad_toml_are_ignored() {
        let caps = TermCaps {
            truecolor: true,
            nerd_font: false,
        };
        let (theme, _) = load_from("accent = \"not-a-color\"\n", &caps, None, None);
        assert_eq!(theme.accent, Theme::default().accent);

        let (theme, _) = load_from("this is not toml {{{", &caps, None, None);
        assert_eq!(theme, Theme::default());
    }

    #[test]
    fn adapt_removes_every_rgb_when_truecolor_is_off() {
        let mut theme = Theme::default();
        assert!(
            theme
                .all_colors()
                .iter()
                .any(|c| matches!(c, Color::Rgb(_, _, _))),
            "default palette is truecolor"
        );
        theme.adapt(&TermCaps {
            truecolor: false,
            nerd_font: false,
        });
        for c in theme.all_colors() {
            assert!(
                !matches!(c, Color::Rgb(_, _, _)),
                "RGB survived adapt(): {c:?}"
            );
        }
    }

    #[test]
    fn adapt_is_a_noop_when_truecolor_is_on() {
        let mut theme = Theme::default();
        let original = theme;
        theme.adapt(&TermCaps {
            truecolor: true,
            nerd_font: false,
        });
        assert_eq!(theme, original);
    }

    #[test]
    fn ira_icons_env_beats_toml_and_auto() {
        let caps = TermCaps {
            truecolor: true,
            nerd_font: true,
        };
        let (_, icons) = load_from("icons = \"nerd\"\n", &caps, Some("unicode"), None);
        assert_eq!(icons, IconSet::Unicode);

        let (_, icons) = load_from("icons = \"unicode\"\n", &caps, None, None);
        assert_eq!(icons, IconSet::Unicode);

        let (_, icons) = load_from("", &caps, None, Some("nerd"));
        assert_eq!(icons, IconSet::Nerd);

        let (_, icons) = load_from("", &caps, None, None);
        assert_eq!(icons, IconSet::Nerd);
    }

    #[test]
    fn cyberpunk_preset_applies_and_allows_overrides() {
        let caps = TermCaps {
            truecolor: true,
            nerd_font: false,
        };
        let (theme, _) = load_from("preset = \"cyberpunk2077\"\n", &caps, None, None);
        assert_eq!(theme.accent, Theme::cyberpunk2077().accent);
        assert_eq!(theme.error, Theme::cyberpunk2077().error);
        assert_ne!(theme.bg, Theme::mocha().bg);

        let (theme, _) = load_from(
            "preset = \"cyberpunk2077\"\naccent = \"#ffffff\"\n",
            &caps,
            None,
            None,
        );
        assert_eq!(theme.accent, Color::Rgb(255, 255, 255));
        assert_eq!(theme.bg, Theme::cyberpunk2077().bg);
    }

    #[test]
    fn preset_cycle_wraps_and_ids_roundtrip() {
        let mut p = ThemePreset::ALL[0];
        for _ in 0..ThemePreset::ALL.len() {
            p = p.next();
        }
        assert_eq!(p, ThemePreset::ALL[0], "full cycle returns to the start");

        for preset in ThemePreset::ALL {
            assert_eq!(ThemePreset::parse(preset.id()), Some(*preset), "{preset:?}");
            assert_eq!(
                ThemePreset::parse(&preset.id().to_uppercase()),
                Some(*preset)
            );
            assert!(!preset.label().is_empty());
        }
        assert_eq!(ThemePreset::parse("bogus"), None);
    }

    #[test]
    fn chip_style_follows_icon_set_unless_toml_pins_it() {
        let caps = TermCaps {
            truecolor: true,
            nerd_font: true,
        };
        // Auto: Nerd icons -> outline, Unicode -> square.
        let (theme, icons) = load_from("", &caps, None, None);
        assert_eq!(icons, IconSet::Nerd);
        assert_eq!(theme.chips, ChipStyle::Outline);
        let (theme, _) = load_from("", &caps, Some("unicode"), None);
        assert_eq!(theme.chips, ChipStyle::Square);

        // toml pins it regardless of icons; unknown values fall back to auto.
        let (theme, _) = load_from("chips = \"square\"\n", &caps, None, None);
        assert_eq!(theme.chips, ChipStyle::Square);
        let (theme, _) = load_from("chips = \"rounded\"\n", &caps, Some("unicode"), None);
        assert_eq!(theme.chips, ChipStyle::Rounded);
        let (theme, _) = load_from("chips = \"Outline\"\n", &caps, Some("unicode"), None);
        assert_eq!(theme.chips, ChipStyle::Outline);
        let (theme, _) = load_from("chips = \"blob\"\n", &caps, None, None);
        assert_eq!(theme.chips, ChipStyle::Outline);

        // Every preset keeps the style when cycling.
        let mut loader = Loader::from_toml("", caps);
        loader.set_icon_set(IconSet::Nerd);
        for p in ThemePreset::ALL {
            assert_eq!(loader.theme_for(*p).chips, ChipStyle::Outline, "{p:?}");
        }
    }

    #[test]
    fn every_preset_is_readable() {
        for preset in ThemePreset::ALL {
            let t = preset.theme();
            assert_ne!(t.bg, t.text, "{preset:?}: text must contrast bg");
            assert_ne!(t.surface, t.text, "{preset:?}: text must contrast surface");
            assert_ne!(t.key_bg, t.key_fg, "{preset:?}: chip must be legible");
        }
    }

    #[test]
    fn loader_applies_overrides_on_any_preset_and_resolves_precedence() {
        let caps = TermCaps {
            truecolor: true,
            nerd_font: false,
        };
        let loader = Loader::from_toml("preset = \"nord\"\naccent = \"#123456\"\n", caps);
        let t = loader.theme_for(ThemePreset::Dracula);
        assert_eq!(t.accent, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(t.bg, Theme::dracula().bg);

        // Persisted state beats toml, toml beats default.
        assert_eq!(
            loader.resolve_preset(Some("tokyo-night")),
            (ThemePreset::TokyoNight, PresetSource::State)
        );
        assert_eq!(
            loader.resolve_preset(Some("junk")),
            (ThemePreset::Nord, PresetSource::Toml)
        );
        let plain = Loader::from_toml("", caps);
        assert_eq!(
            plain.resolve_preset(None),
            (ThemePreset::Mocha, PresetSource::Default)
        );
    }
}
