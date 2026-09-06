use crate::app::{App, AppResult};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Keys that trigger the delete flow in normal mode. Windows/Linux have a
/// dedicated Del key; on macOS the key labeled "delete" is Backspace (Del
/// is fn+Backspace), so Backspace joins the delete keys there — matching
/// Finder's convention. Either way the y/n confirmation guards it.
pub fn is_delete_key(code: KeyCode) -> bool {
    match code {
        KeyCode::Delete => true,
        #[cfg(target_os = "macos")]
        KeyCode::Backspace => true,
        _ => false,
    }
}

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

    // Go-to-path dialog: typing/pasting builds the path; Enter navigates
    // or creates.
    if app.goto_prompt.is_some() {
        match key_event.code {
            KeyCode::Esc => app.cancel_goto(),
            KeyCode::Enter => app.confirm_goto(),
            KeyCode::Backspace => app.goto_pop(),
            KeyCode::Char(c) => app.goto_push(&c.to_string()),
            _ => {}
        }
        return Ok(());
    }

    // Create-new dialog: typing builds the name; Enter creates.
    if app.new_entry.is_some() {
        match key_event.code {
            KeyCode::Esc => app.cancel_new_entry(),
            KeyCode::Enter => app.confirm_new_entry(),
            KeyCode::Backspace => app.new_entry_backspace(),
            KeyCode::Left => app.new_entry_left(),
            KeyCode::Right => app.new_entry_right(),
            KeyCode::Char(c) => app.new_entry_insert(c),
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

    // Deletion progress dialog: any key hides it; the background deletion
    // keeps running.
    if app.deletion_box_visible() {
        app.deletion_box_hidden = true;
        return Ok(());
    }

    // Multi-selection info dialog: any key closes it; the walks keep
    // running in the background (sizes stay cached).
    if app.multi_info.is_some() {
        app.multi_info = None;
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

    // Confirmation prompt (delete / copy / move). `o` cycles the overwrite
    // policy for copy/move.
    if app.confirming.is_some() {
        match key_event.code {
            KeyCode::Char('o') => app.cycle_confirm_policy(),
            KeyCode::Char('y') | KeyCode::Enter => app.confirm_pending(),
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

        // `0` spawns the user's terminal emulator in the active pane's
        // folder (drives shortcuts start at 1, so 0 is free).
        KeyCode::Char('0') => app.spawn_native_terminal(),

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

        // `n` opens the create-new dialog (folder or file by extension).
        KeyCode::Char('n') => app.start_new_entry(),

        // `[` opens the go-to-path dialog (paste or type a path).
        KeyCode::Char('[') => app.start_goto(),

        // `]` copies the active pane's current folder path to the clipboard.
        KeyCode::Char(']') => app.copy_folder_path(),

        // `-` ejects (unmounts) the removable drive of the active pane.
        // (`e` belongs to the Desktop common-folder shortcut on macOS.)
        KeyCode::Char('-') => app.eject_active_drive(),

        // Esc clears the confirmed search filter (all files visible again).
        KeyCode::Esc => app.clear_filter(),

        // Toggle a bookmark for the current folder.
        KeyCode::Char('b') => app.toggle_bookmark(),
        // `/` starts fuzzy search within the current folder.
        KeyCode::Char('/') => app.start_search(),

        // `+` toggles the vertical split of the files pane.
        KeyCode::Char('+') => app.toggle_split(),

        // Backtick toggles the Copy Board sidebar.
        KeyCode::Char('`') => app.toggle_copy_board(),

        // `c` copies the selected entry to the other pane; `m` moves it.
        KeyCode::Char('c') => app.request_copy(),
        KeyCode::Char('m') => app.request_move(),

        // Tab cycles focus among panes and the Copy Board.
        KeyCode::Tab => app.switch_pane(),

        // Space multi-selects entries; Del deletes the selection (with
        // confirmation).
        KeyCode::Char(' ') => app.toggle_select_current(),
        KeyCode::Delete | KeyCode::Backspace if is_delete_key(key_event.code) => {
            app.request_delete()
        }

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

        // `,` cycles the active pane's sort mode: Name → Size → Modified → Kind.
        KeyCode::Char(',') => app.cycle_sort(),

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
