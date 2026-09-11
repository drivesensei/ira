//! Terminal capability detection and RGB → xterm-256 quantization.

use super::font_probe::{self, NerdSource};

/// What the current terminal can render, detected once at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermCaps {
    /// 24-bit `Color::Rgb` is safe. When `false`, the theme is quantized
    /// to `Color::Indexed` so Terminal.app and other 256-color hosts do
    /// not fall back to the nearest named ANSI color.
    pub truecolor: bool,
    /// A font with Nerd Font symbols is reachable by this terminal (see
    /// [`font_probe`]) or `NERD_FONT` is set. Overrides live in
    /// `IRA_ICONS` and `theme.toml`.
    pub nerd_font: bool,
    /// Where the Nerd glyphs come from (or why they are unavailable), for
    /// `--check-terminal`.
    pub nerd_source: NerdSource,
}

impl Default for TermCaps {
    /// Optimistic defaults for tests / headless construction: truecolor
    /// on, no Nerd Font. Real runs go through [`detect`].
    fn default() -> Self {
        TermCaps {
            truecolor: true,
            nerd_font: false,
            nerd_source: NerdSource::NotFound,
        }
    }
}

/// Environment snapshot so detection is unit-testable without mutating
/// process env.
#[derive(Debug, Clone, Default)]
pub struct EnvSnapshot {
    pub colorterm: Option<String>,
    pub term_program: Option<String>,
    pub term: Option<String>,
    pub wt_session: bool,
    /// `WT_PROFILE_ID`: GUID of the Windows Terminal profile in use.
    pub wt_profile_id: Option<String>,
    pub kitty_window_id: bool,
    pub nerd_font_env: bool,
    pub ira_icons: Option<String>,
    pub is_windows: bool,
    pub is_macos: bool,
    /// Result of the font probe. `None` = not run (plain [`from_os`]);
    /// [`from_os_probed`] fills it. Tests inject any value.
    ///
    /// [`from_os`]: EnvSnapshot::from_os
    /// [`from_os_probed`]: EnvSnapshot::from_os_probed
    pub probe: Option<NerdSource>,
}

impl EnvSnapshot {
    /// Reads the real process environment. Cheap: env vars only, no
    /// font probe.
    pub fn from_os() -> Self {
        Self {
            colorterm: std::env::var("COLORTERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            term: std::env::var("TERM").ok(),
            wt_session: std::env::var("WT_SESSION").is_ok(),
            wt_profile_id: std::env::var("WT_PROFILE_ID").ok(),
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").is_ok(),
            nerd_font_env: std::env::var("NERD_FONT").is_ok(),
            ira_icons: std::env::var("IRA_ICONS").ok(),
            is_windows: cfg!(windows),
            is_macos: cfg!(target_os = "macos"),
            probe: None,
        }
    }

    /// [`from_os`](Self::from_os) plus the font probe (config-file reads,
    /// one `fc-list` call or a font-directory scan). Run once at startup.
    pub fn from_os_probed() -> Self {
        let mut env = Self::from_os();
        env.probe = Some(font_probe::probe(&env));
        env
    }
}

/// Detects capabilities from the live environment, including the font
/// probe.
pub fn detect() -> TermCaps {
    detect_from_env(&EnvSnapshot::from_os_probed())
}

/// Detects capabilities from an explicit snapshot (tests + `--check-terminal`).
pub fn detect_from_env(env: &EnvSnapshot) -> TermCaps {
    let program = env
        .term_program
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let term = env.term.as_deref().unwrap_or("").to_ascii_lowercase();
    let colorterm = env.colorterm.as_deref().unwrap_or("").to_ascii_lowercase();

    // Terminal.app is 256-color only, even when other hints look modern.
    let apple_terminal = program.contains("apple_terminal");

    let truecolor = if apple_terminal {
        false
    } else {
        matches!(colorterm.as_str(), "truecolor" | "24bit")
            || env.wt_session
            || env.is_windows
            || program.contains("iterm")
            || program.contains("wezterm")
            || program.contains("ghostty")
            || program.contains("vscode")
            || program.contains("alacritty")
            || term.contains("alacritty")
    };

    // Nerd glyphs render only when a font carrying them is reachable:
    // bundled by the terminal, set as its profile font, or installed where
    // the OS falls back per glyph. The probe decides; `NERD_FONT` in the
    // environment is the manual escape hatch.
    let nerd_source = env.probe.clone().unwrap_or_default();
    let nerd_font = env.nerd_font_env || nerd_source.has_nerd_glyphs();

    TermCaps {
        truecolor,
        nerd_font,
        nerd_source,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_terminal_is_never_truecolor() {
        let env = EnvSnapshot {
            colorterm: Some("truecolor".into()),
            term_program: Some("Apple_Terminal".into()),
            ..EnvSnapshot::default()
        };
        assert!(!detect_from_env(&env).truecolor);
    }

    #[test]
    fn colorterm_and_known_hosts_enable_truecolor() {
        let mut env = EnvSnapshot {
            colorterm: Some("24bit".into()),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&env).truecolor);

        env = EnvSnapshot {
            term_program: Some("iTerm.app".into()),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&env).truecolor);

        env = EnvSnapshot {
            wt_session: true,
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&env).truecolor);

        env = EnvSnapshot {
            is_windows: true,
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&env).truecolor);
    }

    #[test]
    fn nerd_font_follows_the_probe_not_the_terminal_name() {
        // A known terminal name alone proves nothing about the font.
        let alacritty_unprobed = EnvSnapshot {
            term_program: Some("alacritty".into()),
            ..EnvSnapshot::default()
        };
        let caps = detect_from_env(&alacritty_unprobed);
        assert!(!caps.nerd_font);
        assert_eq!(caps.nerd_source, NerdSource::NotFound);

        // Windows Terminal with the stock font: definitive no.
        let wt_plain = EnvSnapshot {
            wt_session: true,
            is_windows: true,
            probe: Some(NerdSource::PlainProfileFont("Cascadia Mono".into())),
            ..EnvSnapshot::default()
        };
        assert!(!detect_from_env(&wt_plain).nerd_font);

        // Windows Terminal with an NF profile font.
        let wt_nf = EnvSnapshot {
            probe: Some(NerdSource::ProfileFont("Cascadia Mono NF".into())),
            ..wt_plain.clone()
        };
        assert!(detect_from_env(&wt_nf).nerd_font);

        // foot on Linux with a covering font installed.
        let foot = EnvSnapshot {
            term: Some("foot".into()),
            probe: Some(NerdSource::InstalledFont("JetBrainsMono Nerd Font".into())),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&foot).nerd_font);

        // Bundled symbols (kitty).
        let kitty = EnvSnapshot {
            kitty_window_id: true,
            probe: Some(NerdSource::Bundled("kitty")),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&kitty).nerd_font);
    }

    #[test]
    fn nerd_font_env_overrides_a_negative_probe() {
        let env = EnvSnapshot {
            nerd_font_env: true,
            probe: Some(NerdSource::NotFound),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&env).nerd_font);
    }

    #[test]
    fn probed_snapshot_runs_the_probe_once_and_is_consistent() {
        // Smoke test against the real machine: whatever the probe says,
        // detection must agree with it.
        let env = EnvSnapshot::from_os_probed();
        let src = env.probe.clone().expect("probe ran");
        let caps = detect_from_env(&env);
        assert_eq!(caps.nerd_source, src);
        assert_eq!(caps.nerd_font, env.nerd_font_env || src.has_nerd_glyphs());
    }

    #[test]
    fn rgb_to_indexed_maps_primaries_and_grays() {
        assert_eq!(rgb_to_indexed(255, 0, 0), 196); // cube red
        assert_eq!(rgb_to_indexed(0, 255, 0), 46); // cube green
        assert_eq!(rgb_to_indexed(0, 0, 255), 21); // cube blue
        assert_eq!(rgb_to_indexed(255, 255, 255), 231); // cube white
        assert_eq!(rgb_to_indexed(0, 0, 0), 16); // cube black
                                                 // Mid gray is closer to the ramp than to a cube cell.
        let gray = rgb_to_indexed(128, 128, 128);
        assert!(
            gray >= 232,
            "mid gray should land on the grayscale ramp, got {gray}"
        );
    }
}
