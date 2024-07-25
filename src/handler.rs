use crate::app::{App, AppResult};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handles the key events and updates the state of [`App`].
pub fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        // Exit application on `Ctrl-C` or q
        KeyCode::Char(c) if c == 'q' => app.quit(),
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
            let shortcuts = app.get_drive_shortcuts();
            if index > 0 && index <= shortcuts.len() {
                app.set_tab1_folder_from_drives(index - 1);
            }
        }

        KeyCode::Char(c) if !c.is_digit(10) => {
            let shortcuts = app.get_common_folders_shortcuts();
            if let Some(idx) = shortcuts.iter().position(|sc| *sc == c) {
                app.set_tab1_folder_from_common_folders(idx);
                // println!("l en is {}", shortcuts.len());
            }
        }

        _ => {}
    }
    Ok(())
}
