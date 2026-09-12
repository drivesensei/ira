//! Startup image-protocol probe with Windows and override fallbacks.
//!
//! `ratatui-image`'s stdin query is authoritative when it returns a real
//! graphics protocol *except Sixel on native Windows*. WT's Sixel support
//! exists, but the crate's stateless `Image` widget leaves the cell grid
//! empty there — the preview column goes blank. Automatic Sixel on Windows
//! is therefore skipped; the braille renderer always paints cells.
//! `IRA_IMAGES=sixel` still forces it for experiments.
//!
//! `IRA_IMAGES=auto|sixel|kitty|iterm2|blocks` overrides the decision.

use ratatui_image::picker::{cap_parser::QueryStdioOptions, Picker, ProtocolType};

/// Inputs that affect the protocol decision (no I/O).
#[derive(Debug, Clone, Default)]
pub struct ProbeEnv {
    pub wt_session: bool,
    pub is_windows: bool,
    pub override_images: Option<String>,
    pub in_iterm2: bool,
}

impl ProbeEnv {
    pub fn from_os() -> Self {
        Self {
            wt_session: std::env::var("WT_SESSION").is_ok(),
            is_windows: cfg!(target_os = "windows"),
            override_images: std::env::var("IRA_IMAGES").ok(),
            in_iterm2: std::env::var("TERM_PROGRAM").is_ok_and(|t| t.contains("iTerm"))
                || std::env::var("LC_TERMINAL").is_ok_and(|t| t.contains("iTerm")),
        }
    }
}

/// How the picker was chosen, for `--check-terminal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerSource {
    Query,
    EnvOverride(String),
    Fallback,
}

/// Probed picker plus the reason it was chosen.
pub struct Probed {
    pub picker: Picker,
    pub source: PickerSource,
}

impl Probed {
    /// Short protocol name for `--check-terminal` (`Blocks` is our
    /// braille renderer, which replaces the crate's Halfblocks path).
    pub fn protocol_name(&self) -> &'static str {
        match self.picker.protocol_type() {
            ProtocolType::Kitty => "Kitty",
            ProtocolType::Sixel => "Sixel",
            ProtocolType::Iterm2 => "Iterm2",
            ProtocolType::Halfblocks => "Blocks",
        }
    }

    pub fn source_reason(&self) -> String {
        match &self.source {
            PickerSource::Query => "from terminal query".into(),
            PickerSource::EnvOverride(_) => "IRA_IMAGES override".into(),
            PickerSource::Fallback => "no graphics protocol".into(),
        }
    }
}

/// Query the terminal and apply Windows / override fallbacks.
pub fn probe() -> Probed {
    let env = ProbeEnv::from_os();
    let query = with_vt_console_modes(|| run_query(&env));
    decide(query, &env)
}

/// Pure decision table: unit-tested without a terminal.
pub fn decide(query: Result<Picker, ()>, env: &ProbeEnv) -> Probed {
    if let Some(raw) = env.override_images.as_deref() {
        if let Some(proto) = parse_override(raw) {
            let mut picker = picker_or_halfblocks(query);
            picker.set_protocol_type(proto);
            return Probed {
                picker,
                source: PickerSource::EnvOverride(raw.trim().to_string()),
            };
        }
    }
    match query {
        Ok(picker) if is_graphics(&picker) && !reject_windows_sixel(&picker, env) => Probed {
            picker,
            source: PickerSource::Query,
        },
        query => {
            let mut picker = picker_or_halfblocks(query);
            // A Windows Sixel query result must not leak into the fallback
            // picker — `build_rendered` would still emit a blank graphic.
            if reject_windows_sixel(&picker, env) {
                picker.set_protocol_type(ProtocolType::Halfblocks);
            }
            Probed {
                picker,
                source: PickerSource::Fallback,
            }
        }
    }
}

fn run_query(env: &ProbeEnv) -> Result<Picker, ()> {
    let mut options = QueryStdioOptions::default();
    // iTerm2 3.5+ answers the kitty query but does not draw unicode
    // placeholders (U+10EEEE); every cell becomes "?". Skip kitty so the
    // crate falls through to the iTerm2 protocol it actually implements.
    if env.in_iterm2 {
        options.blacklist_protocols.push(ProtocolType::Kitty);
    }
    Picker::from_query_stdio_with_options(options).map_err(|_| ())
}

fn parse_override(raw: &str) -> Option<ProtocolType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "sixel" => Some(ProtocolType::Sixel),
        "kitty" => Some(ProtocolType::Kitty),
        "iterm2" | "iterm" => Some(ProtocolType::Iterm2),
        "blocks" | "halfblocks" => Some(ProtocolType::Halfblocks),
        _ => None, // "auto" and unknown values: follow the normal path
    }
}

fn is_graphics(picker: &Picker) -> bool {
    !matches!(picker.protocol_type(), ProtocolType::Halfblocks)
}

/// WT advertises Sixel and may even answer the query, but the encoded
/// graphic does not appear in our stderr/alternate-screen TUI — the
/// cells stay empty. Prefer the braille fallback unless the user asked
/// for Sixel.
fn reject_windows_sixel(picker: &Picker, env: &ProbeEnv) -> bool {
    env.is_windows && matches!(picker.protocol_type(), ProtocolType::Sixel)
}

fn picker_or_halfblocks(query: Result<Picker, ()>) -> Picker {
    match query {
        Ok(picker) => picker,
        Err(()) => Picker::halfblocks(),
    }
}

/// Enable VT input (so DA1 / `CSI 16 t` replies reach us) and VT output
/// on Windows, then restore the input mode. No-op elsewhere.
fn with_vt_console_modes<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(windows)]
    {
        return windows_vt::with_vt(f);
    }
    #[cfg(not(windows))]
    {
        f()
    }
}

#[cfg(windows)]
mod windows_vt {
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::{STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    use winapi::um::wincon::{ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING};

    pub fn with_vt<T>(f: impl FnOnce() -> T) -> T {
        unsafe {
            let hin = GetStdHandle(STD_INPUT_HANDLE);
            let hout = GetStdHandle(STD_OUTPUT_HANDLE);
            let mut in_mode = 0u32;
            let mut out_mode = 0u32;
            let valid_in = !hin.is_null() && hin != INVALID_HANDLE_VALUE;
            let valid_out = !hout.is_null() && hout != INVALID_HANDLE_VALUE;
            let had_in = valid_in && GetConsoleMode(hin, &mut in_mode) != 0;
            let had_out = valid_out && GetConsoleMode(hout, &mut out_mode) != 0;
            if had_in {
                let _ = SetConsoleMode(hin, in_mode | ENABLE_VIRTUAL_TERMINAL_INPUT);
            }
            if had_out {
                let _ = SetConsoleMode(hout, out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
            let result = f();
            if had_in {
                let _ = SetConsoleMode(hin, in_mode);
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(wt: bool, windows: bool, override_images: Option<&str>) -> ProbeEnv {
        ProbeEnv {
            wt_session: wt,
            is_windows: windows,
            override_images: override_images.map(str::to_string),
            in_iterm2: false,
        }
    }

    #[test]
    fn query_graphics_wins() {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Sixel);
        let probed = decide(Ok(picker), &env(false, false, None));
        assert_eq!(probed.source, PickerSource::Query);
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Sixel));

        // Kitty / iTerm2 on Windows (WezTerm) is still accepted.
        let mut kitty = Picker::halfblocks();
        kitty.set_protocol_type(ProtocolType::Kitty);
        let probed = decide(Ok(kitty), &env(true, true, None));
        assert_eq!(probed.source, PickerSource::Query);
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Kitty));
    }

    #[test]
    fn windows_sixel_is_rejected_in_favor_of_blocks() {
        // Silent probe: stay on blocks (never assume Sixel).
        let probed = decide(Ok(Picker::halfblocks()), &env(true, true, None));
        assert_eq!(probed.source, PickerSource::Fallback);
        assert!(matches!(
            probed.picker.protocol_type(),
            ProtocolType::Halfblocks
        ));

        // Query that "succeeded" with Sixel still becomes blocks — WT
        // leaves those cells empty in our TUI.
        let mut sixel = Picker::halfblocks();
        sixel.set_protocol_type(ProtocolType::Sixel);
        let probed = decide(Ok(sixel), &env(true, true, None));
        assert_eq!(probed.source, PickerSource::Fallback);
        assert!(matches!(
            probed.picker.protocol_type(),
            ProtocolType::Halfblocks
        ));
    }

    #[test]
    fn silent_probe_elsewhere_falls_back_to_blocks() {
        let probed = decide(Ok(Picker::halfblocks()), &env(false, false, None));
        assert_eq!(probed.source, PickerSource::Fallback);
        assert!(matches!(
            probed.picker.protocol_type(),
            ProtocolType::Halfblocks
        ));
        // WT_SESSION leaked into WSL must not assume Sixel on a Linux binary.
        let probed = decide(Ok(Picker::halfblocks()), &env(true, false, None));
        assert_eq!(probed.source, PickerSource::Fallback);
    }

    #[test]
    fn ira_images_override_wins_in_either_direction() {
        let probed = decide(Ok(Picker::halfblocks()), &env(true, true, Some("blocks")));
        assert_eq!(probed.source, PickerSource::EnvOverride("blocks".into()));
        assert!(matches!(
            probed.picker.protocol_type(),
            ProtocolType::Halfblocks
        ));

        let mut kitty = Picker::halfblocks();
        kitty.set_protocol_type(ProtocolType::Kitty);
        let probed = decide(Ok(kitty), &env(false, false, Some("SIXEL")));
        assert_eq!(probed.source, PickerSource::EnvOverride("SIXEL".into()));
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Sixel));

        // Explicit override is the only way to get Sixel on Windows.
        let probed = decide(Ok(Picker::halfblocks()), &env(true, true, Some("sixel")));
        assert_eq!(probed.source, PickerSource::EnvOverride("sixel".into()));
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Sixel));
    }

    #[test]
    fn auto_and_unknown_overrides_are_ignored() {
        let probed = decide(Ok(Picker::halfblocks()), &env(true, true, Some("auto")));
        assert_eq!(probed.source, PickerSource::Fallback);
        let probed = decide(Ok(Picker::halfblocks()), &env(false, false, Some("nope")));
        assert_eq!(probed.source, PickerSource::Fallback);
    }
}
