use crate::app::{App, AppResult};
use crate::event::EventHandler;
use crate::ui;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use std::io::{self, Write};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};

/// When true, alternate-screen / draw / reset go to stdout so Sixel DCS
/// and kitty APC reach ConPTY (stderr is not parsed as a graphics stream).
static TUI_ON_STDOUT: AtomicBool = AtomicBool::new(false);

/// Select the TTY stream used by [`Tui::init`] / [`Tui::exit`]. Must be
/// set before `init` and match the `CrosstermBackend` writer.
pub fn set_tui_on_stdout(on: bool) {
    TUI_ON_STDOUT.store(on, Ordering::SeqCst);
}

pub fn tui_on_stdout() -> bool {
    TUI_ON_STDOUT.load(Ordering::SeqCst)
}

fn tui_out() -> impl Write {
    if tui_on_stdout() {
        Box::new(io::stdout()) as Box<dyn Write>
    } else {
        Box::new(io::stderr()) as Box<dyn Write>
    }
}

/// Representation of a terminal user interface.
///
/// It is responsible for setting up the terminal,
/// initializing the interface and handling the draw events.
#[derive(Debug)]
pub struct Tui<B: Backend> {
    /// Interface to the Terminal.
    terminal: Terminal<B>,
    /// Terminal event handler.
    pub events: EventHandler,
}

impl<B: Backend> Tui<B> {
    /// Constructs a new instance of [`Tui`].
    pub fn new(terminal: Terminal<B>, events: EventHandler) -> Self {
        Self { terminal, events }
    }

    /// Initializes the terminal interface.
    ///
    /// It enables the raw mode and sets terminal properties.
    pub fn init(&mut self) -> AppResult<()>
    where
        <B as Backend>::Error: 'static,
    {
        terminal::enable_raw_mode()?;
        ratatui::crossterm::execute!(
            tui_out(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;

        // Define a custom panic hook to reset the terminal properties.
        // This way, you won't have your terminal messed up if an unexpected error happens.
        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic| {
            Self::reset().expect("failed to reset the terminal");
            panic_hook(panic);
        }));

        // NOTE: do NOT call `terminal.clear()` here. Since ratatui 0.30 it
        // saves/restores the cursor position, which sends a blocking \x1b[6n
        // query whose response races with the EventHandler thread - the
        // resulting ~2 s stall was the entire "slow startup" regression. The
        // alternate screen is already blank and the first draw paints it.
        self.terminal.hide_cursor()?;
        Ok(())
    }

    /// [`Draw`] the terminal interface by [`rendering`] the widgets.
    ///
    /// [`Draw`]: ratatui::Terminal::draw
    /// [`rendering`]: crate::ui::render
    pub fn draw(&mut self, app: &mut App) -> AppResult<()>
    where
        <B as Backend>::Error: 'static,
    {
        self.terminal.draw(|frame| ui::render(app, frame))?;
        app.present_overlay();
        Ok(())
    }

    /// Resets the terminal interface.
    ///
    /// This function is also used for the panic hook to revert
    /// the terminal properties if unexpected errors occur.
    fn reset() -> AppResult<()> {
        terminal::disable_raw_mode()?;
        ratatui::crossterm::execute!(
            tui_out(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        )?;
        Ok(())
    }

    /// Exits the terminal interface.
    ///
    /// It disables the raw mode and reverts back the terminal properties.
    pub fn exit(&mut self) -> AppResult<()>
    where
        <B as Backend>::Error: 'static,
    {
        Self::reset()?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}
