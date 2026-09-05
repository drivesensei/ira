//! Clipboard helper: copies text to the system clipboard without new
//! dependencies. Strategy: native tools first (wl-copy on Wayland, xclip on
//! X11, pbcopy on macOS), then the OSC 52 escape sequence as a fallback
//! (works in foot, alacritty, kitty, wezterm and over SSH).
use std::io::Write;
use std::process::{Command, Stdio};

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Minimal base64 encoder (std only, no padding skipping needed for OSC 52).
fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[(n >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[n as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// OSC 52 escape sequence that sets the clipboard to `text` (base64-encoded).
pub fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

fn spawn_paste_tool(program: &str, args: &[&str], text: &str) -> bool {
    let mut child = match Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    matches!(child.wait(), Ok(status) if status.success())
}

/// Copies `text` to the system clipboard. Returns `true` when at least one
/// strategy reported success.
pub fn copy_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    // Native clipboard tools, best-effort per platform.
    #[cfg(unix)]
    {
        for (program, args) in [
            ("wl-copy", vec![]),
            ("xclip", vec!["-selection", "clipboard"]),
            ("pbcopy", vec![]),
        ] {
            if spawn_paste_tool(program, &args, text) {
                return true;
            }
        }
    }
    #[cfg(windows)]
    {
        if spawn_paste_tool("clip", &[], text) {
            return true;
        }
    }
    // OSC 52 fallback: works in foot, alacritty, kitty, wezterm, and over SSH.
    let Ok(mut stderr) = std::fs::OpenOptions::new().write(true).open("/dev/stderr") else {
        return false;
    };
    stderr.write_all(osc52_sequence(text).as_bytes()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn osc52_sequence_wraps_base64() {
        let seq = osc52_sequence("/tmp");
        assert!(seq.starts_with("\x1b]52;c;"), "{seq:?}");
        assert!(seq.ends_with('\x07'), "{seq:?}");
        // base64("/tmp") == L3RtcA==
        assert!(seq.contains("L3RtcA=="), "{seq:?}");
    }
}
