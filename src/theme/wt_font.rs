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
