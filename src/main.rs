use ira::app::{App, AppResult};
use ira::event::{Event, EventHandler};
use ira::handler::handle_key_events;
use ira::tui::Tui;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui_image::picker::{cap_parser::QueryStdioOptions, Picker, ProtocolType};
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
    //
    // iTerm2 3.5+ answers the kitty graphics-protocol query, but its kitty
    // implementation does not render kitty's unicode placeholders (U+10EEEE):
    // every preview cell shows up as "?" instead of pixels. Blacklisting
    // kitty here makes the query fall through to the iTerm2 protocol, which
    // iTerm2 renders correctly (same measure the crate itself applies to
    // WezTerm and Konsole).
    let in_iterm2 = std::env::var("TERM_PROGRAM").is_ok_and(|t| t.contains("iTerm"))
        || std::env::var("LC_TERMINAL").is_ok_and(|t| t.contains("iTerm"));
    let mut options = QueryStdioOptions::default();
    if in_iterm2 {
        options.blacklist_protocols.push(ProtocolType::Kitty);
    }
    let picker = match Picker::from_query_stdio_with_options(options) {
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
    // Graphics-protocol cleanup: transmitted kitty images persist in the
    // terminal beyond the program's lifetime unless explicitly deleted.
    if matches!(
        app.picker.as_ref().map(|p| p.protocol_type()),
        Some(ProtocolType::Kitty)
    ) {
        use std::io::Write as _;
        let _ = write!(io::stderr(), "\x1b_Ga=d,d=e\x1b\\");
        let _ = io::stderr().flush();
    }
    // Exit the user interface.
    tui.exit()?;
    Ok(())
}
