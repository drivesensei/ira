//! Startup image-protocol probe with Windows Terminal and override fallbacks.
//!
//! `ratatui-image`'s stdin query is authoritative when it returns a real
//! graphics protocol. On native Windows it often does not: ConPTY may drop
//! the DA1 / `CSI 16 t` replies, and the crate's `font_size_fallback` is
//! hard-coded to `None`, which forces Halfblocks even when Sixel was seen.
//! Windows Terminal ≥ 1.22 does render Sixel at a fixed virtual cell of
//! 10×20 px — the same size `Picker::halfblocks()` already carries — so a
//! silent probe plus `WT_SESSION` is enough to assume Sixel.
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
    WindowsTerminalAssumed,
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
    /// quadrant renderer, which replaces the crate's Halfblocks path).
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
            PickerSource::WindowsTerminalAssumed => {
                "Windows Terminal assumed: probe got no answer".into()
            }
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
        Ok(picker) if is_graphics(&picker) => Probed {
            picker,
            source: PickerSource::Query,
        },
        query => {
            if env.is_windows && env.wt_session {
                let mut picker = picker_or_halfblocks(query);
                picker.set_protocol_type(ProtocolType::Sixel);
                return Probed {
                    picker,
                    source: PickerSource::WindowsTerminalAssumed,
                };
            }
            Probed {
                picker: picker_or_halfblocks(query),
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
        let probed = decide(Ok(picker), &env(true, true, None));
        assert_eq!(probed.source, PickerSource::Query);
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Sixel));
    }

    #[test]
    fn silent_probe_on_windows_terminal_assumes_sixel() {
        let probed = decide(Ok(Picker::halfblocks()), &env(true, true, None));
        assert_eq!(probed.source, PickerSource::WindowsTerminalAssumed);
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Sixel));
        let fs = probed.picker.font_size();
        assert_eq!((fs.width, fs.height), (10, 20));

        let probed = decide(Err(()), &env(true, true, None));
        assert_eq!(probed.source, PickerSource::WindowsTerminalAssumed);
        assert!(matches!(probed.picker.protocol_type(), ProtocolType::Sixel));
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
    }

    #[test]
    fn auto_and_unknown_overrides_are_ignored() {
        let probed = decide(Ok(Picker::halfblocks()), &env(true, true, Some("auto")));
        assert_eq!(probed.source, PickerSource::WindowsTerminalAssumed);
        let probed = decide(Ok(Picker::halfblocks()), &env(false, false, Some("nope")));
        assert_eq!(probed.source, PickerSource::Fallback);
    }
}
