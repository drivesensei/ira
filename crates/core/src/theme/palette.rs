use super::load::{ThemeCapabilities, ThemeFile};
use super::{icons, ThemePreset};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    /// Resets the foreground or background color
    #[default]
    Reset,
    /// ANSI Color: Black. Foreground: 30, Background: 40
    Black,
    /// ANSI Color: Red. Foreground: 31, Background: 41
    Red,
    /// ANSI Color: Green. Foreground: 32, Background: 42
    Green,
    /// ANSI Color: Yellow. Foreground: 33, Background: 43
    Yellow,
    /// ANSI Color: Blue. Foreground: 34, Background: 44
    Blue,
    /// ANSI Color: Magenta. Foreground: 35, Background: 45
    Magenta,
    /// ANSI Color: Cyan. Foreground: 36, Background: 46
    Cyan,
    /// ANSI Color: White. Foreground: 37, Background: 47
    ///
    /// Note that this is sometimes called `silver` or `white` but we use `white` for bright white
    Gray,
    /// ANSI Color: Bright Black. Foreground: 90, Background: 100
    ///
    /// Note that this is sometimes called `light black` or `bright black` but we use `dark gray`
    DarkGray,
    /// ANSI Color: Bright Red. Foreground: 91, Background: 101
    LightRed,
    /// ANSI Color: Bright Green. Foreground: 92, Background: 102
    LightGreen,
    /// ANSI Color: Bright Yellow. Foreground: 93, Background: 103
    LightYellow,
    /// ANSI Color: Bright Blue. Foreground: 94, Background: 104
    LightBlue,
    /// ANSI Color: Bright Magenta. Foreground: 95, Background: 105
    LightMagenta,
    /// ANSI Color: Bright Cyan. Foreground: 96, Background: 106
    LightCyan,
    /// ANSI Color: Bright White. Foreground: 97, Background: 107
    /// Sometimes called `bright white` or `light white` in some terminals
    White,
    /// An RGB color.
    ///
    /// Note that only terminals that support 24-bit true color will display this correctly.
    /// Notably versions of Windows Terminal prior to Windows 10 and macOS Terminal.app do not
    /// support this.
    ///
    /// If the terminal does not support true color, code using the  `TermwizBackend` will
    /// fallback to the default text color. Crossterm and Termion do not have this capability and
    /// the display will be unpredictable (e.g. Terminal.app may display glitched blinking text).
    ///
    /// See also: <https://en.wikipedia.org/wiki/ANSI_escape_code#24-bit>
    Rgb(u8, u8, u8),
    /// An 8-bit 256 color.
    ///
    /// See also <https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit>
    Indexed(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseColorError;
impl FromStr for Color {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(
            // There is a mix of different color names and formats in the wild.
            // This is an attempt to support as many as possible.
            match s
                .to_lowercase()
                .replace([' ', '-', '_'], "")
                .replace("bright", "light")
                .replace("grey", "gray")
                .replace("silver", "gray")
                .replace("lightblack", "darkgray")
                .replace("lightwhite", "white")
                .replace("lightgray", "white")
                .as_ref()
            {
                "reset" => Self::Reset,
                "black" => Self::Black,
                "red" => Self::Red,
                "green" => Self::Green,
                "yellow" => Self::Yellow,
                "blue" => Self::Blue,
                "magenta" => Self::Magenta,
                "cyan" => Self::Cyan,
                "gray" => Self::Gray,
                "darkgray" => Self::DarkGray,
                "lightred" => Self::LightRed,
                "lightgreen" => Self::LightGreen,
                "lightyellow" => Self::LightYellow,
                "lightblue" => Self::LightBlue,
                "lightmagenta" => Self::LightMagenta,
                "lightcyan" => Self::LightCyan,
                "white" => Self::White,
                _ => {
                    if let Ok(index) = s.parse::<u8>() {
                        Self::Indexed(index)
                    } else if let Some((r, g, b)) = parse_hex_color(s) {
                        Self::Rgb(r, g, b)
                    } else {
                        return Err(ParseColorError);
                    }
                }
            },
        )
    }
}

fn parse_hex_color(input: &str) -> Option<(u8, u8, u8)> {
    if !input.starts_with('#') || input.len() != 7 {
        return None;
    }
    let r = u8::from_str_radix(input.get(1..3)?, 16).ok()?;
    let g = u8::from_str_radix(input.get(3..5)?, 16).ok()?;
    let b = u8::from_str_radix(input.get(5..7)?, 16).ok()?;
    Some((r, g, b))
}

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

impl ThemePreset {
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

impl Theme {
    /// Quantizes every RGB color to the nearest xterm-256 index when the
    /// terminal does not support truecolor. Render code never branches.
    pub fn adapt(&mut self, caps: &ThemeCapabilities) {
        if caps.truecolor {
            return;
        }
        for color in self.all_colors_mut() {
            *color = quantize(*color);
        }
    }

    /// Derive a light presentation without introducing a new persisted preset.
    /// Reflect HSL lightness while preserving hue/chroma, then darken foreground
    /// roles for readable defaults. User overrides are applied by Loader afterward.
    pub(super) fn adapt_desktop_light(&mut self) {
        for color in self.all_colors_mut() {
            if let Color::Rgb(r, g, b) = *color {
                let shift = 255i16 - i16::from(r.max(g).max(b)) - i16::from(r.min(g).min(b));
                *color = rgb(
                    (i16::from(r) + shift) as u8,
                    (i16::from(g) + shift) as u8,
                    (i16::from(b) + shift) as u8,
                );
            }
        }
        // Keep non-cursor selected rows on a light background. This ensures
        // black is an available contrast fallback for every ordinary text role.
        for _ in 0..32 {
            if luminance(self.selection) >= 0.3 {
                break;
            }
            let Color::Rgb(r, g, b) = self.selection else {
                break;
            };
            self.selection = rgb(
                r.saturating_add(8),
                g.saturating_add(8),
                b.saturating_add(8),
            );
        }
        let backgrounds = [self.bg, self.surface, self.surface_alt, self.selection];
        for foreground in [
            &mut self.text,
            &mut self.text_muted,
            &mut self.success,
            &mut self.warning,
            &mut self.error,
            &mut self.info,
            &mut self.dir,
            &mut self.hidden,
            &mut self.image,
            &mut self.video,
            &mut self.audio,
            &mut self.archive,
            &mut self.code,
            &mut self.document,
            &mut self.executable,
            &mut self.data,
        ] {
            for _ in 0..32 {
                if backgrounds
                    .iter()
                    .all(|background| contrast(*foreground, *background) >= 4.5)
                {
                    break;
                }
                let Color::Rgb(r, g, b) = *foreground else {
                    break;
                };
                *foreground = rgb(
                    (u16::from(r) * 4 / 5) as u8,
                    (u16::from(g) * 4 / 5) as u8,
                    (u16::from(b) * 4 / 5) as u8,
                );
            }
            if backgrounds
                .iter()
                .any(|background| contrast(*foreground, *background) < 4.5)
            {
                *foreground = rgb(0, 0, 0);
            }
        }
        for (foreground, background) in [
            (&mut self.key_fg, self.key_bg),
            (&mut self.cursor_fg, self.cursor_bg),
        ] {
            if contrast(*foreground, background) < 4.5 {
                *foreground = if luminance(background) > 0.179 {
                    rgb(0, 0, 0)
                } else {
                    rgb(255, 255, 255)
                };
            }
        }
    }

    /// Applies a (possibly partial) TOML override. Invalid colors are skipped.
    pub(super) fn apply_overrides(&mut self, file: &ThemeFile) {
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
    pub(super) fn all_colors(&self) -> [Color; 28] {
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

pub(super) fn luminance(color: Color) -> f64 {
    let rgba = color.rgba(Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    });
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b)
}

pub(super) fn contrast(a: Color, b: Color) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
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

/// Nearest xterm-256 index for an RGB triple (6×6×6 cube + grayscale ramp).
pub fn rgb_to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let (cube_idx, cube) = nearest_cube(r, g, b);
    let (gray_idx, gray) = nearest_gray(r, g, b);
    if color_dist(r, g, b, cube.0, cube.1, cube.2) <= color_dist(r, g, b, gray, gray, gray) {
        cube_idx
    } else {
        gray_idx
    }
}

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

fn nearest_cube(r: u8, g: u8, b: u8) -> (u8, (u8, u8, u8)) {
    let ri = nearest_level(r);
    let gi = nearest_level(g);
    let bi = nearest_level(b);
    let idx = 16 + 36 * ri + 6 * gi + bi;
    (
        idx as u8,
        (CUBE_LEVELS[ri], CUBE_LEVELS[gi], CUBE_LEVELS[bi]),
    )
}

fn nearest_level(v: u8) -> usize {
    let mut best = 0;
    let mut best_d = u16::MAX;
    for (i, &level) in CUBE_LEVELS.iter().enumerate() {
        let d = (v as i16 - level as i16).unsigned_abs();
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    best
}

fn nearest_gray(r: u8, g: u8, b: u8) -> (u8, u8) {
    let avg = ((r as u16 + g as u16 + b as u16) / 3) as u8;
    // Gray ramp: index 232+n → 8 + n*10, n in 0..=23.
    let n = if avg <= 8 {
        0
    } else {
        ((avg.saturating_sub(8) as u16 + 5) / 10).min(23) as u8
    };
    let val = 8 + n * 10;
    (232 + n, val)
}

fn color_dist(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> u32 {
    let dr = r1 as i32 - r2 as i32;
    let dg = g1 as i32 - g2 as i32;
    let db = b1 as i32 - b2 as i32;
    (dr * dr + dg * dg + db * db) as u32
}

/// Opaque native RGBA color; reset colors resolve through an explicit host fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}
impl Color {
    pub fn rgba(self, reset: Rgba) -> Rgba {
        let index = match self {
            Self::Reset => return reset,
            Self::Rgb(r, g, b) => return Rgba { r, g, b, a: 255 },
            Self::Indexed(i) => i,
            Self::Black => 0,
            Self::Red => 1,
            Self::Green => 2,
            Self::Yellow => 3,
            Self::Blue => 4,
            Self::Magenta => 5,
            Self::Cyan => 6,
            Self::Gray => 7,
            Self::DarkGray => 8,
            Self::LightRed => 9,
            Self::LightGreen => 10,
            Self::LightYellow => 11,
            Self::LightBlue => 12,
            Self::LightMagenta => 13,
            Self::LightCyan => 14,
            Self::White => 15,
        };
        const ANSI: [(u8, u8, u8); 16] = [
            (0, 0, 0),
            (128, 0, 0),
            (0, 128, 0),
            (128, 128, 0),
            (0, 0, 128),
            (128, 0, 128),
            (0, 128, 128),
            (192, 192, 192),
            (128, 128, 128),
            (255, 0, 0),
            (0, 255, 0),
            (255, 255, 0),
            (0, 0, 255),
            (255, 0, 255),
            (0, 255, 255),
            (255, 255, 255),
        ];
        let (r, g, b) = match index {
            0..=15 => ANSI[index as usize],
            16..=231 => {
                let c = (index - 16) as usize;
                (
                    CUBE_LEVELS[c / 36],
                    CUBE_LEVELS[(c / 6) % 6],
                    CUBE_LEVELS[c % 6],
                )
            }
            _ => {
                let gray = 8 + (index - 232) * 10;
                (gray, gray, gray)
            }
        };
        Rgba { r, g, b, a: 255 }
    }
}
