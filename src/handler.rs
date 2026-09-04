use crate::app::{App, AppResult};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handles the key events and updates the state of [`App`].
pub fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    // Ctrl combos: Ctrl+C quits in every mode; Ctrl+A selects all / clears all
    // (except while editing a rename). Ctrl is the only modifier every
    // terminal reports reliably.
    if key_event.modifiers == KeyModifiers::CONTROL {
        match key_event.code {
            KeyCode::Char('c') | KeyCode::Char('C') => {
                app.quit();
                return Ok(());
            }
            _ => {}
        }
        if app.renaming.is_none() {
            match key_event.code {
                KeyCode::Char('a') | KeyCode::Char('A') => app.toggle_select_all(),
                _ => {}
            }
        }
        return Ok(());
    }

    // Rename text editor.
    if app.renaming.is_some() {
        match key_event.code {
            KeyCode::Esc => app.cancel_rename(),
            KeyCode::Enter => app.commit_rename(),
            KeyCode::Backspace => app.rename_backspace(),
            KeyCode::Left => app.rename_cursor_left(),
            KeyCode::Right => app.rename_cursor_right(),
            KeyCode::Char(c) => app.rename_insert(c),
            _ => {}
        }
        return Ok(());
    }

    // Error dialog: any key dismisses it. Checked before the info dialog so
    // an eject failure's error box never swallows the next action.
    if app.status.is_some() {
        app.clear_status();
        return Ok(());
    }

    // Info dialog: `x` cancels the folder's background size walk (keeping
    // the dialog open with the partial size), `r` restarts the measurement
    // from scratch; any other key dismisses it and leaves the walk running.
    if app.info.is_some() {
        if let KeyCode::Char('x') = key_event.code {
            if key_event.modifiers.is_empty() {
                app.cancel_dialog_size_walk();
                return Ok(());
            }
        }
        if let KeyCode::Char('r') = key_event.code {
            if key_event.modifiers.is_empty() {
                app.recalculate_dialog_size();
                return Ok(());
            }
        }
        app.close_info();
        return Ok(());
    }

    // Confirmation prompt (e.g. delete).
    if app.confirming.is_some() {
        match key_event.code {
            KeyCode::Char('y') | KeyCode::Enter => app.confirm_delete(),
            KeyCode::Char('n') | KeyCode::Esc => app.cancel_confirm(),
            _ => {}
        }
        return Ok(());
    }

    // Fuzzy search input.
    if app.is_searching() {
        match key_event.code {
            KeyCode::Esc => app.cancel_search(),
            KeyCode::Enter => app.confirm_search(),
            KeyCode::Backspace => app.pop_search_char(),
            KeyCode::Char(c) => app.push_search_char(c),
            KeyCode::Right => app.enter_folder(),
            KeyCode::Up => {
                if key_event.modifiers == KeyModifiers::ALT {
                    app.goto_top();
                } else {
                    app.prev_item();
                }
            }
            KeyCode::Down => {
                if key_event.modifiers == KeyModifiers::ALT {
                    app.goto_bottom();
                } else {
                    app.next_item();
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Copy Board controls (contextual — only while the board has focus).
    if app.board_has_focus() {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('`') => app.toggle_copy_board(),
            KeyCode::Tab => app.switch_pane(),
            KeyCode::Up => app.copy_board_prev(),
            KeyCode::Down => app.copy_board_next(),
            KeyCode::Char('p') | KeyCode::Char(' ') => app.toggle_selected_job_pause(),
            KeyCode::Char('x') => app.cancel_selected_job(),
            KeyCode::Char('q') => app.quit(),
            _ => {}
        }
        return Ok(());
    }

    match key_event.code {
        // Exit application on `q`
        KeyCode::Char(c) if c == 'q' => app.quit(),

        // Any digit represents a shortcut to a Drive path
        KeyCode::Char(c) if c.is_digit(10) => {
            let index = c.to_digit(10).unwrap() as usize;
            let shortcuts = app.get_drive_shortcuts();
            if index > 0 && index <= shortcuts.len() {
                app.set_folder_from_drives(index - 1);
            }
        }

        KeyCode::Char('z') => app.goto_top(),
        KeyCode::Char('x') => app.goto_bottom(),

        // `e` ejects (unmounts) the removable drive of the active pane.
        KeyCode::Char('e') => app.eject_active_drive(),

        // Toggle a bookmark for the current folder.
        KeyCode::Char('b') => app.toggle_bookmark(),
        // `/` starts fuzzy search within the current folder.
        KeyCode::Char('/') => app.start_search(),

        // `+` toggles the vertical split of the files pane.
        KeyCode::Char('+') => app.toggle_split(),

        // Backtick toggles the Copy Board sidebar.
        KeyCode::Char('`') => app.toggle_copy_board(),

        // `c` copies the selected entry to the other pane; `m` moves it.
        KeyCode::Char('c') => app.copy_to_other_pane(),
        KeyCode::Char('m') => app.move_to_other_pane(),

        // Tab cycles focus among panes and the Copy Board.
        KeyCode::Tab => app.switch_pane(),

        // Space multi-selects entries; Del deletes the selection (with
        // confirmation).
        KeyCode::Char(' ') => app.toggle_select_current(),
        KeyCode::Delete => app.request_delete(),

        // Select all / clear all and invert. Ctrl+A (above) and Alt+? are the
        // reliable paths — terminals generally do not forward Super, so the
        // Super variants only fire where the terminal happens to report it.
        KeyCode::Char(c)
            if key_event
                .modifiers
                .intersects(KeyModifiers::SUPER | KeyModifiers::ALT)
                && matches!(c, 'a' | 'A') =>
        {
            app.toggle_select_all()
        }
        KeyCode::Char(c)
            if key_event
                .modifiers
                .intersects(KeyModifiers::SUPER | KeyModifiers::ALT)
                && matches!(c, 'i' | 'I') =>
        {
            app.invert_selection()
        }

        // `.` toggles hidden files.
        KeyCode::Char('.') => app.toggle_hidden(),

        // `?` shows metadata for the selected entry.
        KeyCode::Char('?') => app.show_info(),

        KeyCode::Char(c) if !c.is_digit(10) => {
            let common = app.get_common_folders_shortcuts();
            if let Some(idx) = common.iter().position(|sc| *sc == c) {
                app.set_folder_from_common_folders(idx);
            } else {
                let bookmarks = app.get_bookmark_shortcuts();
                if let Some(idx) = bookmarks.iter().position(|sc| *sc == c) {
                    app.set_folder_from_bookmark(idx);
                }
            }
        }

        // Files navigation handlers
        KeyCode::Right => app.enter_folder(),
        KeyCode::Left => app.out_of_folder(),

        // Enter renames the selected entry (macOS-style).
        KeyCode::Enter => app.start_rename(),

        KeyCode::Up => {
            if key_event.modifiers == KeyModifiers::ALT {
                app.goto_top();
            }
            app.prev_item();
        }
        KeyCode::Down => {
            if key_event.modifiers == KeyModifiers::ALT {
                app.goto_bottom();
            }
            app.next_item();
        }

        _ => {}
    }
    Ok(())
}
