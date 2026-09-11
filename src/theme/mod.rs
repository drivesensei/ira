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
        }
    }

    /// Built-in preset from a `theme.toml` `preset = "..."` value.
    pub fn from_preset(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "mocha" | "default" | "catppuccin" => Some(Self::mocha()),
            "cyberpunk" | "cyberpunk2077" | "2077" | "nightcity" => Some(Self::cyberpunk2077()),
            _ => None,
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

/// Loads the theme and icon set: defaults, then `~/.config/ira/theme.toml`,
/// then `adapt()` for the detected terminal, then icon-set resolution
/// (`IRA_ICONS` > toml `icons` > persisted session > auto).
pub fn load() -> (Theme, IconSet) {
    load_with_persisted(None)
}

/// Same as [`load`], using a previously persisted icon preference
/// (`icons=nerd` in the session state) when env/toml do not override.
pub fn load_with_persisted(persisted_icons: Option<&str>) -> (Theme, IconSet) {
    let env = EnvSnapshot::from_os();
    let caps = detect_from_env(&env);
    let file = theme_file_path().and_then(|p| fs::read_to_string(p).ok());
    load_from(
        &file.unwrap_or_default(),
        &caps,
        env.ira_icons.as_deref(),
        persisted_icons,
    )
}

/// Test / `--check-terminal` entry: parse an optional TOML body against
/// already-detected caps.
pub fn load_from(
    toml_src: &str,
    caps: &TermCaps,
    ira_icons: Option<&str>,
    persisted_icons: Option<&str>,
) -> (Theme, IconSet) {
    let parsed = if toml_src.trim().is_empty() {
        ThemeFile::default()
    } else {
        toml::from_str::<ThemeFile>(toml_src).unwrap_or_default()
    };
    let mut theme = parsed
        .preset
        .as_deref()
        .and_then(Theme::from_preset)
        .unwrap_or_default();
    theme.apply_overrides(&parsed);
    theme.adapt(caps);
    let icons = resolve_icon_set(caps, parsed.icons.as_deref(), ira_icons, persisted_icons);
    (theme, icons)
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
}
