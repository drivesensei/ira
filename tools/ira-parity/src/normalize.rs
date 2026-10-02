use thiserror::Error;
#[derive(Debug, Error)]
#[error("scenario {scenario} observation {observation} mismatch: expected {expected:?}, actual {actual:?}")]
pub struct Difference {
    scenario: String,
    observation: String,
    expected: String,
    actual: String,
}
pub fn normalize_screen(input: &str, roots: &[&str]) -> String {
    let mut out = String::new();
    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        Csi,
        Osc,
        OscEscape,
    }
    let mut state = State::Text;
    for ch in input.chars() {
        state = match state {
            State::Text if ch == '\x1b' => State::Escape,
            State::Text => {
                out.push(ch);
                State::Text
            }
            State::Escape => match ch {
                '[' => State::Csi,
                ']' => State::Osc,
                // A two-byte ESC sequence is complete here. Other control
                // strings (DCS, SOS, PM, APC) are not produced by the TUI;
                // do not discard any printable text following this pair.
                _ => State::Text,
            },
            State::Csi if ('@'..='~').contains(&ch) => State::Text,
            State::Csi => State::Csi,
            State::Osc if ch == '\x07' => State::Text,
            State::Osc if ch == '\x1b' => State::OscEscape,
            State::Osc => State::Osc,
            State::OscEscape if ch == '\\' => State::Text,
            State::OscEscape if ch == '\x07' => State::Text,
            // ESC not followed by ST is OSC payload; preserve it if printable.
            State::OscEscape => State::Osc,
        };
    }
    for root in roots {
        if !root.is_empty() {
            out = out.replace(root, "<TEMP>")
        }
    }
    out
}
pub fn compare_screen(
    scenario: &str,
    observation: &str,
    expected: &str,
    actual: &str,
) -> Result<(), Difference> {
    if expected == actual {
        Ok(())
    } else {
        Err(Difference {
            scenario: scenario.into(),
            observation: observation.into(),
            expected: expected.into(),
            actual: actual.into(),
        })
    }
}
pub fn compare_bytes(expected: &[u8], actual: &[u8]) -> Result<(), String> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "byte mismatch: expected {} bytes, actual {} bytes",
            expected.len(),
            actual.len()
        ))
    }
}
