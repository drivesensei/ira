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

        // Any digit represents a shortcut to a Drive path
        KeyCode::Char(c) if c.is_digit(10) => {
            let index = c.to_digit(10).unwrap() as usize;
            let shortcuts = app.get_drive_shortcuts();
            if index > 0 && index <= shortcuts.len() {
                app.set_tab1_folder_from_drives(index - 1);
            }
        }

        KeyCode::Char('z') => {
            app.tab1_goto_top();
        }

        KeyCode::Char('x') => {
            app.tab1_goto_bottom();
        }

        KeyCode::Char(c) if !c.is_digit(10) => {
            let shortcuts = app.get_common_folders_shortcuts();
            if let Some(idx) = shortcuts.iter().position(|sc| *sc == c) {
                app.set_tab1_folder_from_common_folders(idx);
            }
        }

        KeyCode::Char('b') => {
            println!("placeholder add bookmark");
        }

        // Files navigation handlers
        KeyCode::Right => {
            app.enter_folder();
        }

        KeyCode::Left => {
            app.out_of_folder();
        }

        // Files navigation handlers
        KeyCode::Up => {
            if key_event.modifiers == KeyModifiers::ALT {
                app.tab1_goto_top();
            }
            app.tab1_prev_item();
        }
        KeyCode::Down => {
            if key_event.modifiers == KeyModifiers::ALT {
                app.tab1_goto_bottom();
            }
            app.tab1_next_item();
        }

        _ => {}
    }
    Ok(())
}
