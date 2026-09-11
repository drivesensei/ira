//! Terminal capability detection and RGB → xterm-256 quantization.

/// What the current terminal can render, detected once at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermCaps {
    /// 24-bit `Color::Rgb` is safe. When `false`, the theme is quantized
    /// to `Color::Indexed` so Terminal.app and other 256-color hosts do
    /// not fall back to the nearest named ANSI color.
    pub truecolor: bool,
    /// Auto-heuristic: this terminal is likely to have Nerd Font symbols
    /// (bundled fallback or a patched font). Overrides live in `IRA_ICONS`
    /// and `theme.toml`.
    pub nerd_font: bool,
}

/// Environment snapshot so detection is unit-testable without mutating
/// process env.
#[derive(Debug, Clone, Default)]
pub struct EnvSnapshot {
    pub colorterm: Option<String>,
    pub term_program: Option<String>,
    pub term: Option<String>,
    pub wt_session: bool,
    pub kitty_window_id: bool,
    pub nerd_font_env: bool,
    pub ira_icons: Option<String>,
    pub is_windows: bool,
}

impl EnvSnapshot {
    /// Reads the real process environment.
    pub fn from_os() -> Self {
        Self {
            colorterm: std::env::var("COLORTERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
            term: std::env::var("TERM").ok(),
            wt_session: std::env::var("WT_SESSION").is_ok(),
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").is_ok(),
            nerd_font_env: std::env::var("NERD_FONT").is_ok(),
            ira_icons: std::env::var("IRA_ICONS").ok(),
            is_windows: cfg!(windows),
        }
    }
}

/// Detects capabilities from the live environment.
pub fn detect() -> TermCaps {
    detect_from_env(&EnvSnapshot::from_os())
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

    // Terminals that ship Nerd Font symbol fallbacks, or whose default
    // distro configs (Omarchy, WezTerm, Ghostty, kitty, modern Alacritty
    // / foot) almost always use a patched Mono font.
    let nerd_font = env.nerd_font_env
        || program.contains("wezterm")
        || program.contains("ghostty")
        || program.contains("alacritty")
        || program.contains("vscode")
        || program.contains("warp")
        || env.kitty_window_id
        || env.wt_session
        || term.contains("xterm-kitty")
        || term.contains("kitty")
        || term.contains("alacritty")
        || term.contains("wezterm")
        || term.contains("ghostty")
        || term.contains("foot")
        || term.contains("warp");

    TermCaps {
        truecolor,
        nerd_font,
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
    fn kitty_and_ghostty_enable_nerd_font_auto() {
        let kitty = EnvSnapshot {
            kitty_window_id: true,
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&kitty).nerd_font);

        let ghostty = EnvSnapshot {
            term_program: Some("ghostty".into()),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&ghostty).nerd_font);

        let unknown = EnvSnapshot::default();
        assert!(!detect_from_env(&unknown).nerd_font);

        let foot = EnvSnapshot {
            term: Some("foot".into()),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&foot).nerd_font);

        let alacritty = EnvSnapshot {
            term_program: Some("alacritty".into()),
            ..EnvSnapshot::default()
        };
        assert!(detect_from_env(&alacritty).nerd_font);
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
