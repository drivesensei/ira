//! Profile font lookups from editor / terminal config files.
//!
//! Windows Terminal and VS Code both keep a `settings.json` written in
//! JSONC (comments and trailing commas allowed), so a tolerant pre-pass
//! turns it into strict JSON before `serde_json` parses it. All parsing is
//! pure and tested on every platform; only path discovery is OS-specific.

use std::path::PathBuf;

use serde_json::Value;

/// Font face of the Windows Terminal profile this process runs in.
///
/// Looks at the packaged (Store), Preview and unpackaged install locations
/// on Windows and, for WSL, the same locations under every `/mnt/c/Users/*`
/// profile. `None` when no settings file is readable.
pub fn windows_terminal_font_face(profile_id: Option<&str>) -> Option<String> {
    windows_terminal_settings_paths()
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|src| wt_profile_font_face(&src, profile_id))
}

/// Point size of the Windows Terminal profile font (`font.size` / `fontSize`).
pub fn windows_terminal_font_size(profile_id: Option<&str>) -> Option<f64> {
    windows_terminal_settings_paths()
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|src| wt_profile_font_size(&src, profile_id))
}

/// Line-box multiplier from `font.lineHeight` / `font.cellHeight` when the
/// value is a simple number or percent. `None` for missing or exotic units.
pub fn windows_terminal_line_height(profile_id: Option<&str>) -> Option<f64> {
    windows_terminal_settings_paths()
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|src| wt_profile_line_height(&src, profile_id))
}

/// Candidate `settings.json` locations for Windows Terminal.
fn windows_terminal_settings_paths() -> Vec<PathBuf> {
    const REL: [&str; 3] = [
        "Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
        "Packages/Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe/LocalState/settings.json",
        "Microsoft/Windows Terminal/settings.json",
    ];
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(local));
    }
    // WSL: the Windows user profiles are mounted under /mnt/c/Users.
    if let Ok(users) = std::fs::read_dir("/mnt/c/Users") {
        for u in users.flatten() {
            roots.push(u.path().join("AppData").join("Local"));
        }
    }
    roots
        .iter()
        .flat_map(|r| REL.iter().map(move |rel| r.join(rel)))
        .collect()
}

/// Resolves the font face from a Windows Terminal `settings.json` body:
/// the profile whose `guid` matches `profile_id` (case-insensitive), else
/// `profiles.defaults`. Both the `font.face` object form (1.10+) and the
/// legacy flat `fontFace` key are understood.
pub fn wt_profile_font_face(jsonc: &str, profile_id: Option<&str>) -> Option<String> {
    let v: Value = serde_json::from_str(&strip_jsonc(jsonc)).ok()?;
    let profiles = v.get("profiles")?;
    // Very old settings files had `profiles` as a bare array.
    let (list, defaults) = match profiles {
        Value::Array(list) => (Some(list), None),
        Value::Object(_) => (
            profiles.get("list").and_then(Value::as_array),
            profiles.get("defaults"),
        ),
        _ => (None, None),
    };
    let face_of = |p: &Value| -> Option<String> {
        p.get("font")
            .and_then(|f| f.get("face"))
            .and_then(Value::as_str)
            .or_else(|| p.get("fontFace").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if let (Some(list), Some(id)) = (list, profile_id) {
        let hit = list.iter().find(|p| {
            p.get("guid")
                .and_then(Value::as_str)
                .is_some_and(|g| g.eq_ignore_ascii_case(id))
        });
        if let Some(face) = hit.and_then(face_of) {
            return Some(face);
        }
    }
    defaults.and_then(face_of)
}

/// `font.size` (1.10+) or legacy `fontSize`, profile then `profiles.defaults`.
pub fn wt_profile_font_size(jsonc: &str, profile_id: Option<&str>) -> Option<f64> {
    let v: Value = serde_json::from_str(&strip_jsonc(jsonc)).ok()?;
    let profiles = v.get("profiles")?;
    let (list, defaults) = match profiles {
        Value::Array(list) => (Some(list), None),
        Value::Object(_) => (
            profiles.get("list").and_then(Value::as_array),
            profiles.get("defaults"),
        ),
        _ => (None, None),
    };
    let size_of = |p: &Value| -> Option<f64> {
        p.get("font")
            .and_then(|f| f.get("size"))
            .and_then(Value::as_f64)
            .or_else(|| p.get("fontSize").and_then(Value::as_f64))
            .filter(|s| *s > 0.0)
    };
    if let (Some(list), Some(id)) = (list, profile_id) {
        let hit = list.iter().find(|p| {
            p.get("guid")
                .and_then(Value::as_str)
                .is_some_and(|g| g.eq_ignore_ascii_case(id))
        });
        if let Some(size) = hit.and_then(size_of) {
            return Some(size);
        }
    }
    defaults.and_then(size_of)
}

/// Simple line-box multiplier from `font.lineHeight` or `font.cellHeight`.
/// JSON number, numeric string ("1.2"), or percent ("120%") only — px/pt/ch
/// and keywords ("leading") are ignored rather than guessed.
pub fn parse_wt_line_height(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| *x > 0.0 && *x < 10.0),
        Value::String(s) => parse_wt_line_height_str(s),
        _ => None,
    }
}

fn parse_wt_line_height_str(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty()
        || s.eq_ignore_ascii_case("leading")
        || s.eq_ignore_ascii_case("default")
        || s.eq_ignore_ascii_case("none")
    {
        return None;
    }
    if let Some(pct) = s.strip_suffix('%') {
        let n: f64 = pct.trim().parse().ok()?;
        let m = n / 100.0;
        return (m > 0.0 && m < 10.0).then_some(m);
    }
    if s.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let n: f64 = s.parse().ok()?;
    (n > 0.0 && n < 10.0).then_some(n)
}

/// `font.lineHeight` then `font.cellHeight`, profile then `profiles.defaults`.
pub fn wt_profile_line_height(jsonc: &str, profile_id: Option<&str>) -> Option<f64> {
    let v: Value = serde_json::from_str(&strip_jsonc(jsonc)).ok()?;
    let profiles = v.get("profiles")?;
    let (list, defaults) = match profiles {
        Value::Array(list) => (Some(list), None),
        Value::Object(_) => (
            profiles.get("list").and_then(Value::as_array),
            profiles.get("defaults"),
        ),
        _ => (None, None),
    };
    let height_of = |p: &Value| -> Option<f64> {
        let font = p.get("font")?;
        font.get("lineHeight")
            .and_then(parse_wt_line_height)
            .or_else(|| font.get("cellHeight").and_then(parse_wt_line_height))
    };
    if let (Some(list), Some(id)) = (list, profile_id) {
        let hit = list.iter().find(|p| {
            p.get("guid")
                .and_then(Value::as_str)
                .is_some_and(|g| g.eq_ignore_ascii_case(id))
        });
        if let Some(h) = hit.and_then(height_of) {
            return Some(h);
        }
    }
    defaults.and_then(height_of)
}

/// Font family of the VS Code-family integrated terminal hosting this
/// process (`TERM_PROGRAM=vscode` is set by VS Code, Cursor, VSCodium...).
pub fn vscode_terminal_font_family() -> Option<String> {
    vscode_settings_paths()
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .find_map(|src| vscode_font_family(&src))
}

/// User `settings.json` of the VS Code-family editors, in preference
/// order. `dirs_next::config_dir()` is `%APPDATA%` on Windows,
/// `~/Library/Application Support` on macOS and `~/.config` on Linux,
/// which is where all of them keep `User/settings.json`.
fn vscode_settings_paths() -> Vec<PathBuf> {
    let Some(cfg) = dirs_next::config_dir() else {
        return Vec::new();
    };
    ["Code", "Code - Insiders", "Cursor", "VSCodium"]
        .iter()
        .map(|app| cfg.join(app).join("User").join("settings.json"))
        .collect()
}

/// `terminal.integrated.fontFamily`, falling back to `editor.fontFamily`
/// (VS Code uses the editor font for the terminal when unset).
pub fn vscode_font_family(jsonc: &str) -> Option<String> {
    let v: Value = serde_json::from_str(&strip_jsonc(jsonc)).ok()?;
    ["terminal.integrated.fontFamily", "editor.fontFamily"]
        .iter()
        .find_map(|k| {
            v.get(*k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

/// Turns JSONC into strict JSON: drops `//` and `/* */` comments outside
/// strings and commas that directly precede `}` / `]`.
pub fn strip_jsonc(src: &str) -> String {
    // Pass 1: comments.
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    // Pass 2: trailing commas.
    let bytes: Vec<char> = out.chars().collect();
    let mut result = String::with_capacity(out.len());
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            result.push(c);
            if c == '\\' && i + 1 < bytes.len() {
                result.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                result.push(c);
            }
            ',' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && matches!(bytes[j], '}' | ']') {
                    // Drop the comma; the closer is emitted on its turn.
                } else {
                    result.push(c);
                }
            }
            _ => result.push(c),
        }
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const WT_SETTINGS: &str = r#"
    // This file was initially generated by Windows Terminal
    {
        "$help": "https://aka.ms/terminal-documentation",
        "defaultProfile": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
        /* block
           comment */
        "profiles":
        {
            "defaults": {
                "font": { "face": "Cascadia Mono", "size": 11, },
            },
            "list":
            [
                {
                    "guid": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
                    "name": "Windows PowerShell",
                    "font": { "face": "JetBrainsMono Nerd Font" },
                },
                {
                    "guid": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
                    "name": "Command Prompt // not a comment",
                    "fontFace": "Consolas",
                },
                {
                    "guid": "{2c4de342-38b7-51cf-b940-2309a097f518}",
                    "name": "Ubuntu",
                },
            ],
        },
    }
    "#;

    #[test]
    fn strip_jsonc_removes_comments_and_trailing_commas_only_outside_strings() {
        let v: Value = serde_json::from_str(&strip_jsonc(WT_SETTINGS)).expect("valid JSON");
        assert_eq!(
            v["profiles"]["list"][1]["name"],
            "Command Prompt // not a comment"
        );
        assert_eq!(v["profiles"]["defaults"]["font"]["size"], 11);
        let escaped = strip_jsonc(r#"{"a": "quote \" then // no comment", }"#);
        let v: Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(v["a"], "quote \" then // no comment");
    }

    #[test]
    fn wt_face_prefers_the_active_profile_then_defaults() {
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, Some("{61C54BBD-C2C6-5271-96E7-009A87FF44BF}")),
            Some("JetBrainsMono Nerd Font".into())
        );
        // Legacy flat key.
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, Some("{0caa0dad-35be-5f56-a8ff-afceeeaa6101}")),
            Some("Consolas".into())
        );
        // Profile without a font: defaults apply.
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, Some("{2c4de342-38b7-51cf-b940-2309a097f518}")),
            Some("Cascadia Mono".into())
        );
        // Unknown / missing profile id: defaults.
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, None),
            Some("Cascadia Mono".into())
        );
        // No font anywhere: None, caller assumes the stock font.
        assert_eq!(
            wt_profile_font_face(r#"{"profiles": {"list": [{"guid": "{x}"}]}}"#, Some("{x}")),
            None
        );
        assert_eq!(wt_profile_font_face("not json", None), None);
    }

    #[test]
    fn wt_size_prefers_the_active_profile_then_defaults() {
        // Profile font object has no size → defaults 11.
        assert_eq!(
            wt_profile_font_size(WT_SETTINGS, Some("{61C54BBD-C2C6-5271-96E7-009A87FF44BF}")),
            Some(11.0)
        );
        assert_eq!(wt_profile_font_size(WT_SETTINGS, None), Some(11.0));
        let sized = r#"{"profiles":{"defaults":{"font":{"size":12}},"list":[{"guid":"{a}","font":{"size":14}}]}}"#;
        assert_eq!(wt_profile_font_size(sized, Some("{a}")), Some(14.0));
        let legacy = r#"{"profiles":[{"guid":"{a}","fontSize":16}]}"#;
        assert_eq!(wt_profile_font_size(legacy, Some("{a}")), Some(16.0));
        assert_eq!(wt_profile_font_size("not json", None), None);
    }

    #[test]
    fn wt_line_height_parses_simple_values_only() {
        assert_eq!(parse_wt_line_height(&serde_json::json!(1.2)), Some(1.2));
        assert_eq!(parse_wt_line_height(&serde_json::json!("1.2")), Some(1.2));
        assert_eq!(parse_wt_line_height(&serde_json::json!("120%")), Some(1.2));
        assert_eq!(parse_wt_line_height(&serde_json::json!("leading")), None);
        assert_eq!(parse_wt_line_height(&serde_json::json!("12px")), None);
        assert_eq!(parse_wt_line_height(&serde_json::json!("12pt")), None);
        assert_eq!(parse_wt_line_height(&serde_json::json!("1ch")), None);
        let profile = r#"{"profiles":{"defaults":{"font":{"size":12,"cellHeight":"1.0"}},"list":[{"guid":"{a}","font":{"lineHeight":1.4}}]}}"#;
        assert_eq!(wt_profile_line_height(profile, Some("{a}")), Some(1.4));
        assert_eq!(wt_profile_line_height(profile, None), Some(1.0));
        assert_eq!(wt_profile_line_height(WT_SETTINGS, None), None);
    }

    #[test]
    fn wt_face_handles_the_legacy_bare_array_layout() {
        let legacy = r#"{"profiles": [{"guid": "{a}", "fontFace": "Cascadia Code NF"}]}"#;
        assert_eq!(
            wt_profile_font_face(legacy, Some("{a}")),
            Some("Cascadia Code NF".into())
        );
    }

    #[test]
    fn vscode_terminal_font_falls_back_to_editor_font() {
        let both = r#"{
            // comment
            "editor.fontFamily": "Consolas",
            "terminal.integrated.fontFamily": "'Hack Nerd Font Mono'",
        }"#;
        assert_eq!(
            vscode_font_family(both),
            Some("'Hack Nerd Font Mono'".into())
        );
        let editor_only = r#"{"editor.fontFamily": "Fira Code"}"#;
        assert_eq!(vscode_font_family(editor_only), Some("Fira Code".into()));
        assert_eq!(
            vscode_font_family(r#"{"terminal.integrated.fontFamily": ""}"#),
            None
        );
        assert_eq!(vscode_font_family("{}"), None);
    }
}
