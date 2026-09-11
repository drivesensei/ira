//! Finds out whether a font that can render IRA's Nerd Font glyphs will be
//! reached by the terminal's glyph lookup, and where it comes from.
//!
//! A terminal app cannot pick or ship a font, so the icon set has to follow
//! what the terminal will actually draw. Three tiers, in order:
//!
//! 1. Terminals that bundle Symbols Nerd Font (kitty >= 0.36, WezTerm,
//!    Ghostty, Warp) always render the glyphs.
//! 2. Terminals whose profile font is readable from a config file
//!    (Windows Terminal, VS Code): a Nerd-flavoured face name decides.
//!    Windows Terminal renders with DirectWrite, which does not fall back
//!    to arbitrary installed fonts for Private Use Area glyphs, so a plain
//!    face there is a definitive "no".
//! 3. On Linux and macOS the OS resolves missing glyphs per character
//!    across all installed fonts (fontconfig / CoreText), so any installed
//!    font covering the sample glyphs is enough.
//!
//! Everything that parses text is a pure function with tests; the OS reads
//! are behind [`ProbeIo`] so the tiering itself is testable too.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::caps::EnvSnapshot;

/// Glyphs the probe checks coverage for: `nf-fa-folder` (the default
/// directory icon), `nf-dev-nodejs_small` (a Devicons glyph Font Awesome
/// does not carry, so a bare Font Awesome install does not pass) and the
/// Powerline pill cap used by the key chips.
pub const SAMPLE_GLYPHS: [char; 3] = ['\u{f07b}', '\u{e718}', '\u{e0b6}'];

/// Why the Nerd icon set is (or is not) usable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NerdSource {
    /// The terminal ships Symbols Nerd Font itself.
    Bundled(&'static str),
    /// The terminal profile font carries Nerd glyphs (face name).
    ProfileFont(String),
    /// An installed font covers the glyphs and the OS falls back to it.
    InstalledFont(String),
    /// The profile font is known and has no Nerd glyphs (face name).
    PlainProfileFont(String),
    /// Nothing found.
    #[default]
    NotFound,
}

impl NerdSource {
    /// `true` when Nerd glyphs will render.
    pub fn has_nerd_glyphs(&self) -> bool {
        matches!(
            self,
            NerdSource::Bundled(_) | NerdSource::ProfileFont(_) | NerdSource::InstalledFont(_)
        )
    }
}

/// OS reads used by [`probe_with`]; swapped for fakes in tests.
pub trait ProbeIo {
    /// Font face of the active Windows Terminal profile, if readable.
    fn wt_font_face(&self, profile_id: Option<&str>) -> Option<String>;
    /// `terminal.integrated.fontFamily` (or `editor.fontFamily`) of the
    /// VS Code family editor hosting the terminal, if readable.
    fn vscode_font_family(&self) -> Option<String>;
    /// An installed font family covering [`SAMPLE_GLYPHS`], if any.
    fn installed_nerd_font(&self) -> Option<String>;
}

/// Runs the probe against the real machine.
pub fn probe(env: &EnvSnapshot) -> NerdSource {
    probe_with(env, &OsIo)
}

/// Tiered probe with injectable IO.
pub fn probe_with(env: &EnvSnapshot, io: &dyn ProbeIo) -> NerdSource {
    let program = env
        .term_program
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let term = env.term.as_deref().unwrap_or("").to_ascii_lowercase();

    // Tier 1: terminals with a built-in Nerd symbols fallback font.
    if let Some(name) = bundled_terminal(&program, &term, env.kitty_window_id) {
        return NerdSource::Bundled(name);
    }

    // Tier 2: profile font from a readable config.
    let mut profile_face: Option<String> = None;
    if env.wt_session {
        // Windows Terminal: DirectWrite never reaches other installed fonts
        // for PUA glyphs, so the face is the whole story, even from WSL.
        // Unreadable settings mean the stock profile: Cascadia Mono.
        let face = io
            .wt_font_face(env.wt_profile_id.as_deref())
            .unwrap_or_else(|| WT_DEFAULT_FACE.to_string());
        return if name_has_nerd_glyphs(&face) {
            NerdSource::ProfileFont(face)
        } else {
            NerdSource::PlainProfileFont(face)
        };
    }
    if program.contains("vscode") {
        if let Some(face) = io.vscode_font_family() {
            if name_has_nerd_glyphs(&face) {
                return NerdSource::ProfileFont(face);
            }
            profile_face = Some(face);
        }
    }

    // Tier 3: per-glyph OS fallback to any installed font (not on Windows,
    // where DirectWrite hosts do not do this for PUA glyphs).
    if !env.is_windows {
        if let Some(family) = io.installed_nerd_font() {
            return NerdSource::InstalledFont(family);
        }
    }

    match profile_face {
        Some(face) => NerdSource::PlainProfileFont(face),
        None => NerdSource::NotFound,
    }
}

/// Windows Terminal's stock profile font (no Nerd glyphs).
const WT_DEFAULT_FACE: &str = "Cascadia Mono";

/// Terminals that bundle Symbols Nerd Font and use it automatically.
fn bundled_terminal(program: &str, term: &str, kitty_window_id: bool) -> Option<&'static str> {
    if kitty_window_id || term.contains("kitty") {
        return Some("kitty");
    }
    for (needle, name) in [
        ("wezterm", "WezTerm"),
        ("ghostty", "Ghostty"),
        ("warp", "Warp"),
    ] {
        if program.contains(needle) || term.contains(needle) {
            return Some(name);
        }
    }
    None
}

/// Whether a font face name (or a comma-separated fallback list of them,
/// as Windows Terminal, VS Code and foot accept) names a font with Nerd
/// glyphs. Matches the Nerd Fonts naming scheme (`... Nerd Font`,
/// `...NF` / `NFM` / `NFP`), Microsoft's `Cascadia Code NF`, and the older
/// community builds (`Caskaydia Cove`, `Delugia`). `PL` variants carry
/// only Powerline glyphs and do not count.
pub fn name_has_nerd_glyphs(name: &str) -> bool {
    name.split(',').any(|entry| {
        let entry = entry.trim().trim_matches(|c| c == '"' || c == '\'');
        let lower = entry.to_ascii_lowercase();
        if lower.contains("nerd") || lower.contains("caskaydia") || lower.contains("delugia") {
            return true;
        }
        entry
            .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'))
            .filter(|t| !t.is_empty())
            .any(|token| {
                let upper_suffix = ["NF", "NFM", "NFP"]
                    .iter()
                    .any(|s| token.ends_with(s) && token.len() > s.len());
                let exact = matches!(token.to_ascii_lowercase().as_str(), "nf" | "nfm" | "nfp");
                upper_suffix || exact
            })
    })
}

/// First family from `fc-list ... family` output. fontconfig prints one
/// font per line with comma-separated family aliases; the first alias is
/// the canonical name.
pub fn first_family(fc_list_output: &str) -> Option<String> {
    fc_list_output
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.split(',').next().unwrap_or(l).trim().to_string())
}

/// Font file (by name) that carries Nerd glyphs, judged from the Nerd
/// Fonts naming scheme. Used where fontconfig is unavailable (macOS) or
/// missing (minimal Linux installs).
pub fn font_file_has_nerd_glyphs(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let is_font = [".ttf", ".otf", ".ttc"].iter().any(|e| lower.ends_with(e));
    if !is_font {
        return false;
    }
    let stem = &file_name[..file_name.rfind('.').unwrap_or(file_name.len())];
    name_has_nerd_glyphs(stem)
}

/// Font directories the OS font systems scan, per platform.
fn font_dirs(is_macos: bool) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = dirs_next::home_dir();
    if is_macos {
        if let Some(h) = &home {
            dirs.push(h.join("Library").join("Fonts"));
        }
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts"));
    } else {
        if let Some(h) = &home {
            dirs.push(h.join(".local").join("share").join("fonts"));
            dirs.push(h.join(".fonts"));
        }
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(xdg) = std::env::var("XDG_DATA_DIRS") {
            for d in xdg.split(':').filter(|d| !d.is_empty()) {
                dirs.push(Path::new(d).join("fonts"));
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Walks `dirs` (bounded depth) for a font file whose name passes
/// [`font_file_has_nerd_glyphs`]; returns its stem.
pub fn scan_font_dirs(dirs: &[PathBuf]) -> Option<String> {
    const MAX_DEPTH: usize = 4;
    fn walk(dir: &Path, depth: usize) -> Option<String> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if depth < MAX_DEPTH {
                    subdirs.push(entry.path());
                }
            } else if font_file_has_nerd_glyphs(&name) {
                let stem = &name[..name.rfind('.').unwrap_or(name.len())];
                return Some(stem.to_string());
            }
        }
        subdirs.into_iter().find_map(|d| walk(&d, depth + 1))
    }
    dirs.iter().find_map(|d| walk(d, 0))
}

/// Real OS reads.
pub struct OsIo;

impl ProbeIo for OsIo {
    fn wt_font_face(&self, profile_id: Option<&str>) -> Option<String> {
        super::wt_font::windows_terminal_font_face(profile_id)
    }

    fn vscode_font_family(&self) -> Option<String> {
        super::wt_font::vscode_terminal_font_family()
    }

    fn installed_nerd_font(&self) -> Option<String> {
        let is_macos = cfg!(target_os = "macos");
        if !is_macos {
            // fontconfig answers coverage exactly: only fonts containing
            // every listed codepoint are printed.
            let charset = SAMPLE_GLYPHS
                .iter()
                .map(|c| format!("{:x}", *c as u32))
                .collect::<Vec<_>>()
                .join(" ");
            let out = Command::new("fc-list")
                .arg(format!(":charset={charset}"))
                .arg("family")
                .output();
            if let Ok(out) = out {
                if out.status.success() {
                    // fontconfig ran: its answer is authoritative, an
                    // empty list means no covering font.
                    return first_family(&String::from_utf8_lossy(&out.stdout));
                }
            }
        }
        scan_font_dirs(&font_dirs(is_macos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeIo {
        wt: Option<&'static str>,
        vscode: Option<&'static str>,
        installed: Option<&'static str>,
    }

    impl ProbeIo for FakeIo {
        fn wt_font_face(&self, _profile_id: Option<&str>) -> Option<String> {
            self.wt.map(str::to_string)
        }
        fn vscode_font_family(&self) -> Option<String> {
            self.vscode.map(str::to_string)
        }
        fn installed_nerd_font(&self) -> Option<String> {
            self.installed.map(str::to_string)
        }
    }

    const NOTHING: FakeIo = FakeIo {
        wt: None,
        vscode: None,
        installed: None,
    };

    #[test]
    fn nerd_names_are_recognized_and_plain_ones_are_not() {
        for yes in [
            "JetBrainsMono Nerd Font",
            "Symbols Nerd Font Mono",
            "Cascadia Mono NF",
            "CascadiaCode NFM",
            "FiraCode Nerd Font Propo",
            "CaskaydiaCove NF",
            "Delugia Mono",
            "JetBrainsMonoNF-Regular",
            "Cascadia Mono, Symbols Nerd Font Mono",
            "'Hack NF', monospace",
        ] {
            assert!(name_has_nerd_glyphs(yes), "{yes} should count as Nerd");
        }
        for no in [
            "Cascadia Mono",
            "Cascadia Code PL",
            "Consolas",
            "Menlo",
            "JetBrains Mono",
            "Noto Sans Symbols",
            "Infinity",
            "",
        ] {
            assert!(!name_has_nerd_glyphs(no), "{no} must not count as Nerd");
        }
    }

    #[test]
    fn font_files_are_judged_by_stem_and_extension() {
        assert!(font_file_has_nerd_glyphs("SymbolsNerdFontMono-Regular.ttf"));
        assert!(font_file_has_nerd_glyphs("CascadiaMonoNF-Regular.ttf"));
        assert!(font_file_has_nerd_glyphs("JetBrainsMonoNerdFont-Bold.ttf"));
        assert!(!font_file_has_nerd_glyphs("Menlo.ttc"));
        assert!(!font_file_has_nerd_glyphs("StandardSymbolsPS.otf"));
        assert!(!font_file_has_nerd_glyphs("NotoSansSymbols-Regular.ttf"));
        assert!(!font_file_has_nerd_glyphs("SymbolsNerdFont.txt"));
    }

    #[test]
    fn fc_list_first_family_takes_the_canonical_alias() {
        let out = "\nJetBrainsMono Nerd Font,JetBrainsMono NF\nFont Awesome 7 Free\n";
        assert_eq!(
            first_family(out).as_deref(),
            Some("JetBrainsMono Nerd Font")
        );
        assert_eq!(first_family(""), None);
        assert_eq!(first_family("  \n"), None);
    }

    #[test]
    fn bundled_terminals_short_circuit() {
        let kitty = EnvSnapshot {
            kitty_window_id: true,
            ..EnvSnapshot::default()
        };
        assert_eq!(probe_with(&kitty, &NOTHING), NerdSource::Bundled("kitty"));
        let ghostty = EnvSnapshot {
            term_program: Some("ghostty".into()),
            ..EnvSnapshot::default()
        };
        assert_eq!(
            probe_with(&ghostty, &NOTHING),
            NerdSource::Bundled("Ghostty")
        );
        let wez = EnvSnapshot {
            term: Some("wezterm".into()),
            ..EnvSnapshot::default()
        };
        assert_eq!(probe_with(&wez, &NOTHING), NerdSource::Bundled("WezTerm"));
    }

    #[test]
    fn windows_terminal_follows_the_profile_font_only() {
        let wt = EnvSnapshot {
            wt_session: true,
            is_windows: true,
            ..EnvSnapshot::default()
        };
        let plain = FakeIo {
            wt: Some("Cascadia Mono"),
            installed: Some("JetBrainsMono Nerd Font"),
            ..NOTHING
        };
        // Installed fonts are irrelevant under DirectWrite.
        assert_eq!(
            probe_with(&wt, &plain),
            NerdSource::PlainProfileFont("Cascadia Mono".into())
        );
        let nf = FakeIo {
            wt: Some("Cascadia Mono NF"),
            ..NOTHING
        };
        assert_eq!(
            probe_with(&wt, &nf),
            NerdSource::ProfileFont("Cascadia Mono NF".into())
        );
        let list = FakeIo {
            wt: Some("Cascadia Mono, Symbols Nerd Font Mono"),
            ..NOTHING
        };
        assert!(probe_with(&wt, &list).has_nerd_glyphs());
        // Unreadable settings: stock profile, Cascadia Mono.
        assert_eq!(
            probe_with(&wt, &NOTHING),
            NerdSource::PlainProfileFont("Cascadia Mono".into())
        );
        // WSL: WT_SESSION on a Linux binary still means DirectWrite.
        let wsl = EnvSnapshot {
            wt_session: true,
            is_windows: false,
            ..EnvSnapshot::default()
        };
        assert!(!probe_with(&wsl, &plain).has_nerd_glyphs());
    }

    #[test]
    fn unix_hosts_fall_back_to_installed_fonts() {
        let foot = EnvSnapshot {
            term: Some("foot".into()),
            ..EnvSnapshot::default()
        };
        let with_font = FakeIo {
            installed: Some("JetBrainsMono Nerd Font"),
            ..NOTHING
        };
        assert_eq!(
            probe_with(&foot, &with_font),
            NerdSource::InstalledFont("JetBrainsMono Nerd Font".into())
        );
        assert_eq!(probe_with(&foot, &NOTHING), NerdSource::NotFound);

        // macOS Terminal.app: same rule, CoreText falls back per glyph.
        let apple = EnvSnapshot {
            term_program: Some("Apple_Terminal".into()),
            is_macos: true,
            ..EnvSnapshot::default()
        };
        assert!(probe_with(&apple, &with_font).has_nerd_glyphs());
        assert!(!probe_with(&apple, &NOTHING).has_nerd_glyphs());
    }

    #[test]
    fn windows_without_terminal_profile_never_uses_installed_fonts() {
        let conhost = EnvSnapshot {
            is_windows: true,
            ..EnvSnapshot::default()
        };
        let with_font = FakeIo {
            installed: Some("JetBrainsMono Nerd Font"),
            ..NOTHING
        };
        assert_eq!(probe_with(&conhost, &with_font), NerdSource::NotFound);
    }

    #[test]
    fn vscode_uses_its_font_setting_then_installed_fonts() {
        let code = EnvSnapshot {
            term_program: Some("vscode".into()),
            ..EnvSnapshot::default()
        };
        let nf = FakeIo {
            vscode: Some("'FiraCode Nerd Font', monospace"),
            ..NOTHING
        };
        assert!(matches!(probe_with(&code, &nf), NerdSource::ProfileFont(_)));
        let plain_with_installed = FakeIo {
            vscode: Some("Consolas"),
            installed: Some("Symbols Nerd Font Mono"),
            ..NOTHING
        };
        assert_eq!(
            probe_with(&code, &plain_with_installed),
            NerdSource::InstalledFont("Symbols Nerd Font Mono".into())
        );
        let plain = FakeIo {
            vscode: Some("Consolas"),
            ..NOTHING
        };
        assert_eq!(
            probe_with(&code, &plain),
            NerdSource::PlainProfileFont("Consolas".into())
        );
        let code_windows = EnvSnapshot {
            is_windows: true,
            ..code
        };
        assert_eq!(
            probe_with(&code_windows, &plain_with_installed),
            NerdSource::PlainProfileFont("Consolas".into())
        );
    }

    #[test]
    fn scan_finds_nerd_font_files_in_nested_dirs() {
        let base = std::env::temp_dir().join(format!("ira_fontscan_{}", std::process::id()));
        let nested = base.join("TTF").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(base.join("Menlo.ttc"), b"").unwrap();
        assert_eq!(scan_font_dirs(std::slice::from_ref(&base)), None);
        std::fs::write(nested.join("SymbolsNerdFontMono-Regular.ttf"), b"").unwrap();
        assert_eq!(
            scan_font_dirs(std::slice::from_ref(&base)).as_deref(),
            Some("SymbolsNerdFontMono-Regular")
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
