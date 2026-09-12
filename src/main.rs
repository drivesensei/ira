use ira::app::{App, AppResult};
use ira::event::{Event, EventHandler};
use ira::handler::handle_key_events;
use ira::services::picker_probe;
use ira::tui::Tui;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui_image::picker::ProtocolType;
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
    if std::env::args().skip(1).any(|a| a == "--check-terminal") {
        print_terminal_check();
        return Ok(());
    }

    // Probe the terminal for image-protocol support and font size BEFORE raw
    // mode and the alternate screen: the probe runs blocking stdin queries
    // that would otherwise race crossterm's event reader (same class of stall
    // as the `terminal.clear()` note in `Tui::init`). Quadrant blocks render
    // in every terminal and are the universal fallback. Sixel is skipped
    // automatically on native Windows (blank cells); see picker_probe.
    let probed = picker_probe::probe();

    // Create the application and install the probed picker.
    let mut app = App::new();
    let truecolor = ira::theme::caps().truecolor;
    app.set_picker(probed.picker, truecolor);

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

/// Prints detected terminal capabilities and exits. Useful when icons
/// render as boxes or colors look wrong.
fn print_terminal_check() {
    use ira::theme::font_probe::NerdSource;

    let env = ira::theme::caps::EnvSnapshot::from_os();
    let persisted = ira::services::state::load_state();
    // Runs the font probe once; `caps` below reuses its result.
    let loaded = ira::theme::load_with_persisted(persisted.theme.as_deref());
    let caps = loaded.loader.caps();
    let icons = loaded.icons;
    let probed = picker_probe::probe();
    let fs = probed.picker.font_size();
    println!("ira {}", env!("CARGO_PKG_VERSION"));
    println!(
        "truecolor: {}",
        if caps.truecolor {
            "yes"
        } else {
            "no (256-color fallback)"
        }
    );
    println!(
        "icons: {} ({})",
        match icons {
            ira::theme::icons::IconSet::Nerd => "nerd",
            ira::theme::icons::IconSet::Emoji => "emoji",
            ira::theme::icons::IconSet::Unicode => "unicode",
        },
        if env.ira_icons.is_some() {
            "IRA_ICONS override"
        } else if loaded.loader.icons_pref().is_some() {
            "from theme.toml icons"
        } else if caps.nerd_font {
            "auto: a font with Nerd glyphs is available"
        } else if caps.wide_emoji {
            "auto: no Nerd font, terminal renders wide color emoji"
        } else {
            "auto: unicode fallback"
        }
    );
    // Where the Nerd glyphs would come from, so a wrong auto result is
    // explainable without guessing.
    let nerd_line = match &caps.nerd_source {
        NerdSource::Bundled(term) => format!("yes ({term} bundles them)"),
        NerdSource::ProfileFont(face) => format!("yes (profile font \"{face}\")"),
        NerdSource::InstalledFont(family) => format!("yes (installed font \"{family}\")"),
        NerdSource::PlainProfileFont(face) => format!("no (profile font \"{face}\")"),
        NerdSource::NotFound => "no (no covering font found)".to_string(),
    };
    println!(
        "nerd glyphs: {nerd_line}{}",
        if env.nerd_font_env {
            "; forced on by NERD_FONT"
        } else {
            ""
        }
    );
    println!(
        "theme: {} ({})",
        loaded.preset.id(),
        match loaded.source {
            ira::theme::PresetSource::State => "from session state, last `\\` press",
            ira::theme::PresetSource::Toml => "from theme.toml preset",
            ira::theme::PresetSource::Default => "default",
        }
    );
    println!(
        "image protocol: {} ({})",
        probed.protocol_name(),
        probed.source_reason()
    );
    println!("cell size: {}x{} px", fs.width, fs.height);
    if let Some(path) = ira::theme::theme_file_path() {
        println!(
            "theme file: {} ({})",
            path.display(),
            if path.exists() {
                "present"
            } else {
                "not found"
            }
        );
    }
}
