//! Platform decoding is separate from the authoritative core precedence chain.
use gpui::Keystroke;
use ira_core::input::{KeyCode, KeyEvent, KeyModifiers};
pub fn decode(key: &Keystroke) -> KeyEvent {
    let mut modifiers = KeyModifiers::NONE;
    if key.modifiers.control {
        modifiers = modifiers | KeyModifiers::CONTROL;
    }
    if key.modifiers.alt {
        modifiers = modifiers | KeyModifiers::ALT;
    }
    if key.modifiers.shift {
        modifiers = modifiers | KeyModifiers::SHIFT;
    }
    if key.modifiers.platform {
        modifiers = modifiers | KeyModifiers::SUPER;
    }
    let code = match key.key.as_str() {
        "escape" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "tab" => KeyCode::Tab,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        _ => {
            let text = if key.modifiers.control || key.modifiers.platform || key.modifiers.alt {
                &key.key
            } else {
                key.key_char.as_ref().unwrap_or(&key.key)
            };
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => KeyCode::Other,
            }
        }
    };
    KeyEvent::new(code, modifiers)
}
