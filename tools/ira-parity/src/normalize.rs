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
    let mut esc = false;
    for ch in input.chars() {
        if esc {
            if ch.is_ascii_alphabetic() {
                esc = false
            }
            continue;
        }
        if ch == '\x1b' {
            esc = true;
            continue;
        }
        out.push(ch)
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
