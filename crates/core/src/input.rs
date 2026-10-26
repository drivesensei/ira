//! Neutral translation of the frozen handler. Phase deliberately does not alter routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Tab,
    Home,
    End,
    PageUp,
    PageDown,
    Other,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers(u8);
impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const ALT: Self = Self(4);
    pub const SUPER: Self = Self(8);
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}
impl std::ops::BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyPhase {
    #[default]
    Press,
    Repeat,
    Release,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
    pub phase: KeyPhase,
}
impl KeyEvent {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            phase: KeyPhase::Press,
        }
    }
}
#[derive(Debug, Clone)]
pub enum Input {
    Key(KeyEvent),
    Paste(String),
    Tick,
    Action(Command),
}
#[derive(Debug, Clone)]
pub struct CommandEnvelope {
    pub sequence: u64,
    pub window_generation: u64,
    pub document_generation: u64,
    pub focus_generation: u64,
    pub command: Input,
}
use crate::application::{App, AppResult};

/// vlad lopez
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
    // Preview text editor (Tab-focused): the buffer captures every key —
    // chars insert (including `q` and `s`), Ctrl+S saves, `Esc`
    // exits back to pane focus, `Tab` moves focus on (discarding edits).
    // Ctrl+C still hard-quits; other Ctrl combos reach the textarea
    // (Ctrl+A = line start, Ctrl+E = line end) instead of pane actions.
    if app.edit_focus {
        if key_event.modifiers == KeyModifiers::CONTROL {
            match key_event.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    app.quit();
                    return Ok(());
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    app.save_edit();
                    return Ok(());
                }
                _ => {}
            }
        }
        // Plain `s` (and Shift+S) insert text — only Ctrl+S saves.
        match key_event.code {
            KeyCode::Esc => {
                app.close_edit();
                app.edit_focus = false;
                return Ok(());
            }
            KeyCode::Tab => {
                app.switch_pane();
                return Ok(());
            }
            _ => {
                app.edit_input(key_event);
                return Ok(());
            }
        }
    }
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
                // Eject sits on Ctrl+- so the plain `-` key can reveal in the
                // OS file browser below (its old Ctrl+O combo never reached
                // the app on Linux terminals).
                KeyCode::Char('-') => app.eject_active_drive(),
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

    // Keybindings help dialog (`*`): any key closes it — checked first so a
    // background notice (transfer done, delete error) can't steal the key.
    if app.keybindings_visible {
        app.close_keybindings();
        return Ok(());
    }

    // Error dialogs are modal: any key dismisses them (checked before the
    // info dialog so an eject failure's error box never swallows the next
    // action). Transient notices — the bottom banner `v` raises, saves,
    // clipboard copies, sort notices — must NOT swallow the key: a second
    // `v` has to cycle the mode again, replacing the banner in the same
    // press. They also expire on their own TTL.
    if app.status.as_ref().is_some_and(|s| s.is_error) {
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
            KeyCode::Char('*') => app.show_keybindings(),
            KeyCode::Char('v') => app.cycle_preview(),
            KeyCode::Char('\\') => app.cycle_theme(),
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
        KeyCode::Char('q') => app.quit(),

        // `0` spawns the user's terminal emulator in the active pane's
        // folder (drives shortcuts start at 1, so 0 is free).
        KeyCode::Char('0') => app.spawn_native_terminal(),

        // Any digit represents a shortcut to a Drive path
        KeyCode::Char(c) if c.is_ascii_digit() => {
            let index = c.to_digit(10).unwrap() as usize;
            let shortcuts = app.get_drive_shortcuts();
            if index > 0 && index <= shortcuts.len() {
                app.set_folder_from_drives(index - 1);
            }
        }

        KeyCode::Char('z') => app.goto_top(),

        // `*` opens the keybindings help dialog (closed by any key).
        KeyCode::Char('*') => app.show_keybindings(),

        KeyCode::Char('x') => app.goto_bottom(),

        // `n` opens the create-new dialog (folder or file by extension).
        KeyCode::Char('n') => app.start_new_entry(),

        // `[` opens the go-to-path dialog (paste or type a path).
        KeyCode::Char('[') => app.start_goto(),

        // `]` copies the active pane's current folder path to the clipboard.
        KeyCode::Char(']') => app.copy_folder_path(),

        // `-` reveals the selection in the OS file browser. A punctuation key,
        // so every plain letter stays in the common-folder / bookmark
        // shortcut pool; ejects the drive on Ctrl+-.
        KeyCode::Char('-') => app.open_in_file_manager(),

        // Esc clears the confirmed search filter (all files visible again).
        KeyCode::Esc => app.clear_filter(),

        // Toggle a bookmark for the current folder.
        KeyCode::Char('b') => app.toggle_bookmark(),
        // `/` starts fuzzy search within the current folder.
        KeyCode::Char('/') => app.start_search(),

        // `v` cycles the image preview: off → column → grid.
        KeyCode::Char('v') => app.cycle_preview(),

        // `+` toggles the vertical split of the files pane.
        KeyCode::Char('+') => app.toggle_split(),

        // Backtick toggles the Copy Board sidebar.
        KeyCode::Char('`') => app.toggle_copy_board(),

        // `\` cycles the built-in theme preset (banner shows the new name).
        KeyCode::Char('\\') => app.cycle_theme(),

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

        KeyCode::Char(c) if !c.is_ascii_digit() => {
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

/// Stable commands shared by keyboard, menu and mouse adapters.
#[derive(Debug, Clone)]
pub enum Command {
    Quit,
    MoveNext,
    MovePrevious,
    MoveTop,
    MoveBottom,
    EnterFolder,
    ParentFolder,
    ToggleSplit,
    SwitchPane,
    ToggleSelectCurrent,
    ToggleSelectAll,
    InvertSelection,
    ToggleHidden,
    StartSearch,
    CancelSearch,
    ConfirmSearch,
    ClearFilter,
    CycleSort,
    CyclePreview,
    CycleTheme,
    ToggleBookmark,
    StartRename,
    CancelRename,
    CommitRename,
    StartNewEntry,
    CancelNewEntry,
    ConfirmNewEntry,
    StartGoto,
    CancelGoto,
    ConfirmGoto,
    RequestCopy,
    RequestMove,
    RequestDelete,
    ConfirmPending,
    CancelConfirm,
    CycleConfirmPolicy,
    ToggleCopyBoard,
    BoardNext,
    BoardPrevious,
    PauseSelectedJob,
    CancelSelectedJob,
    ShowInfo,
    CloseInfo,
    CancelSizeWalk,
    RecalculateSize,
    ShowHelp,
    CloseHelp,
    ClearStatus,
    CopyFolderPath,
    Reveal,
    Terminal,
    Eject,
    Drive(usize),
    CommonFolder(usize),
    Bookmark(usize),
    SelectEntry(crate::model::EntryTarget),
}
impl Command {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Quit => "app.quit",
            Self::MoveNext => "pane.next",
            Self::MovePrevious => "pane.previous",
            Self::MoveTop => "pane.top",
            Self::MoveBottom => "pane.bottom",
            Self::EnterFolder => "pane.enter",
            Self::ParentFolder => "pane.parent",
            Self::ToggleSplit => "pane.split",
            Self::SwitchPane => "focus.next",
            Self::ToggleSelectCurrent => "selection.toggle",
            Self::ToggleSelectAll => "selection.all",
            Self::InvertSelection => "selection.invert",
            Self::ToggleHidden => "files.hidden",
            Self::StartSearch => "search.start",
            Self::CancelSearch => "search.cancel",
            Self::ConfirmSearch => "search.confirm",
            Self::ClearFilter => "search.clear_filter",
            Self::CycleSort => "files.sort",
            Self::CyclePreview => "preview.cycle",
            Self::CycleTheme => "theme.cycle",
            Self::ToggleBookmark => "bookmark.toggle",
            Self::StartRename => "rename.start",
            Self::CancelRename => "rename.cancel",
            Self::CommitRename => "rename.commit",
            Self::StartNewEntry => "create.start",
            Self::CancelNewEntry => "create.cancel",
            Self::ConfirmNewEntry => "create.confirm",
            Self::StartGoto => "goto.start",
            Self::CancelGoto => "goto.cancel",
            Self::ConfirmGoto => "goto.confirm",
            Self::RequestCopy => "transfer.copy",
            Self::RequestMove => "transfer.move",
            Self::RequestDelete => "delete.request",
            Self::ConfirmPending => "confirm.accept",
            Self::CancelConfirm => "confirm.cancel",
            Self::CycleConfirmPolicy => "confirm.policy",
            Self::ToggleCopyBoard => "board.toggle",
            Self::BoardNext => "board.next",
            Self::BoardPrevious => "board.previous",
            Self::PauseSelectedJob => "job.pause",
            Self::CancelSelectedJob => "job.cancel",
            Self::ShowInfo => "info.show",
            Self::CloseInfo => "info.close",
            Self::CancelSizeWalk => "info.cancel_walk",
            Self::RecalculateSize => "info.recalculate",
            Self::ShowHelp => "help.show",
            Self::CloseHelp => "help.close",
            Self::ClearStatus => "status.clear",
            Self::CopyFolderPath => "platform.copy_path",
            Self::Reveal => "platform.reveal",
            Self::Terminal => "platform.terminal",
            Self::Eject => "drive.eject",
            Self::Drive(_) => "drive.navigate",
            Self::CommonFolder(_) => "place.navigate",
            Self::Bookmark(_) => "bookmark.navigate",
            Self::SelectEntry(_) => "pane.select_entry",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Editor,
    Rename,
    Goto,
    Create,
    Help,
    Error,
    Deletion,
    MultiInfo,
    Info,
    Confirmation,
    Search,
    Board,
    Pane(usize),
}
impl App {
    pub fn input_context(&self) -> InputContext {
        if self.edit_focus {
            InputContext::Editor
        } else if self.renaming.is_some() {
            InputContext::Rename
        } else if self.goto_prompt.is_some() {
            InputContext::Goto
        } else if self.new_entry.is_some() {
            InputContext::Create
        } else if self.keybindings_visible {
            InputContext::Help
        } else if self.status.as_ref().is_some_and(|s| s.is_error) {
            InputContext::Error
        } else if self.deletion_box_visible() {
            InputContext::Deletion
        } else if self.multi_info.is_some() {
            InputContext::MultiInfo
        } else if self.info.is_some() {
            InputContext::Info
        } else if self.confirming.is_some() {
            InputContext::Confirmation
        } else if self.is_searching() {
            InputContext::Search
        } else if self.board_has_focus() {
            InputContext::Board
        } else {
            InputContext::Pane(self.active_pane)
        }
    }
}
