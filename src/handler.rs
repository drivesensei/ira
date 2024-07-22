use crate::app::{App, AppResult};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handles the key events and updates the state of [`App`].
pub fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        // Exit application on `ESC` or `q`
        KeyCode::Esc | KeyCode::Char('q') => {
            app.quit();
        }
        // Exit application on `Ctrl-C`
        KeyCode::Char('c') | KeyCode::Char('C') => {
            if key_event.modifiers == KeyModifiers::CONTROL {
                app.quit();
            }
        }
        // Files navigation handlers
        KeyCode::Right => {
            // app.increment_counter();
        }
        KeyCode::Left => {
            // app.decrement_counter();
        }
        // Any digit represents a shortcut to a Drive path
        KeyCode::Char(c) if c.is_digit(10) => {
            let index = c.to_digit(10).unwrap() as usize;
            if app.get_drive_shortcuts().contains(&index) {
                app.set_initial_drive_and_folder(index - 1);
            }
        }
        _ => {}
    }
    Ok(())
}
