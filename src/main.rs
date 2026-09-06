use ira::app::{App, AppResult};
use ira::event::{Event, EventHandler};
use ira::handler::handle_key_events;
use ira::tui::Tui;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use std::io;

fn main() -> AppResult<()> {
    // Package-manager entry point: `--version` / `-V` prints and exits before
    // the TUI initializes. Homebrew's `brew test` and other packagers rely on it.
    if std::env::args()
        .skip(1)
        .any(|a| a == "--version" || a == "-V")
    {
        println!("ira {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Probe the terminal for image-protocol support and font size BEFORE raw
    // mode and the alternate screen: the probe runs blocking stdin queries
    // that would otherwise race crossterm's event reader (same class of stall
    // as the `terminal.clear()` note in `Tui::init`). Halfblocks renders in
    // every terminal and is the universal fallback.
    let picker = match Picker::from_query_stdio() {
        Ok(picker) => picker,
        Err(_) => Picker::halfblocks(),
    };

    // Create the application and install the probed picker.
    let mut app = App::new();
    app.set_picker(picker);

    // Initialize the terminal user interface.
    let backend = CrosstermBackend::new(io::stderr());
    let terminal = Terminal::new(backend)?;
    // Short tick so async results (drive list, initial file listings) reach
    // the UI promptly instead of straggling behind a 2 s cadence.
    let events = EventHandler::new(500);
    let mut tui = Tui::new(terminal, events);
    tui.init()?;

    // Draw immediately so the UI appears instantly instead of waiting up to
    // the first tick (2000 ms).
    tui.draw(&mut app)?;

    // Start the main loop.
    while app.running {
        // Render the user interface.
        tui.draw(&mut app)?;
        // Handle events.
        match tui.events.next()? {
            Event::Tick => app.tick(),
            Event::Key(key_event) => handle_key_events(key_event, &mut app)?,
            Event::Mouse(_) => {}
            Event::Paste(text) => app.handle_paste(&text),
            Event::Resize(_, _) => {}
        }
    }
    // Persist session state (split layout and pane folders) on exit.
    app.persist_state();

    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
