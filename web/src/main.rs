use std::{cell::RefCell, collections::VecDeque, io, rc::Rc};

use ira::{
    app::{App, PreviewMode},
    domain::data::Folder,
    services::{
        list_files::FEntry,
        thumbnails::{preview_kind, PreviewKind},
    },
};
use ratzilla::ratatui::Terminal;
use ratzilla::{
    event::{KeyCode, KeyEvent},
    DomBackend, WebRenderer,
};
use serde::Deserialize;
use wasm_bindgen::prelude::*;

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
    static BROWSER_EVENTS: RefCell<VecDeque<BrowserEvent>> = const { RefCell::new(VecDeque::new()) };
}

enum BrowserEvent {
    Listing {
        entries: Vec<BrowserEntry>,
        label: String,
        path: String,
        pane: usize,
    },
    Text {
        path: String,
        content: String,
        binary: bool,
        truncated: bool,
    },
    Image {
        path: String,
        bytes: Vec<u8>,
    },
    Status {
        message: String,
        is_error: bool,
    },
}

#[derive(Deserialize)]
struct BrowserEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: Option<i64>,
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = window, js_name = iraRequestDirectory)]
    fn request_directory(path: &str, pane: usize);
    #[wasm_bindgen(js_namespace = window, js_name = iraRequestPreview)]
    fn request_preview(path: &str, kind: &str);
    #[wasm_bindgen(js_namespace = window, js_name = iraCreateEntry)]
    fn create_entry(path: &str);
    #[wasm_bindgen(js_namespace = window, js_name = iraRenameEntry)]
    fn rename_entry(path: &str);
    #[wasm_bindgen(js_namespace = window, js_name = iraDeleteEntry)]
    fn delete_entry(path: &str);
    #[wasm_bindgen(js_namespace = window, js_name = iraTransferEntry)]
    fn transfer_entry(source: &str, destination: &str, move_entry: bool);
    #[wasm_bindgen(js_namespace = window, js_name = iraOpenEditor)]
    fn open_editor(path: &str);
}

fn push_browser_event(event: BrowserEvent) {
    BROWSER_EVENTS.with(|events| events.borrow_mut().push_back(event));
}

fn apply_browser_events(app: &mut App) {
    BROWSER_EVENTS.with(|events| {
        for event in events.borrow_mut().drain(..) {
            match event {
                BrowserEvent::Listing {
                    entries,
                    label,
                    path,
                    pane,
                } => {
                    let pane_index = pane.min(1);
                    let target = &mut app.panes[pane_index];
                    target.folder = Some(Folder::new(label.clone(), path, 'W'));
                    target.files = entries
                        .into_iter()
                        .map(|entry| FEntry {
                            path: entry.path,
                            label: entry.name,
                            is_dir: entry.is_dir,
                            size: entry.size,
                            modified: entry.modified,
                        })
                        .collect();
                    target.selected = vec![false; target.files.len()];
                    target.listing_settled = true;
                    target.filter_query = None;
                    target.filter_indices.clear();
                    target.state.select((!target.files.is_empty()).then_some(0));
                    target.render_scroll = 0;
                    app.set_status(
                        format!("Connected to {label} · browser permission active"),
                        false,
                    );
                    request_selected_preview(app);
                }
                BrowserEvent::Text {
                    path,
                    content,
                    binary,
                    truncated,
                } => app.install_browser_text_preview(&path, content, binary, truncated),
                BrowserEvent::Image { path, bytes } => {
                    app.install_browser_image_preview(&path, &bytes)
                }
                BrowserEvent::Status { message, is_error } => app.set_status(message, is_error),
            }
        }
    });
}

fn selected(app: &App) -> Option<(String, bool)> {
    app.selected_visible_entry()
        .map(|entry| (entry.path.clone(), entry.is_dir))
}

fn request_selected_preview(app: &App) {
    let Some(entry) = app.selected_visible_entry() else {
        return;
    };
    if entry.is_dir {
        return;
    }
    let kind = match preview_kind(&entry.path) {
        Some(PreviewKind::Text) => "text",
        Some(PreviewKind::Image) => "image",
        Some(PreviewKind::Video) => "video",
        Some(PreviewKind::Pdf) => "pdf",
        Some(PreviewKind::Heic) => "image",
        None => "unknown",
    };
    request_preview(&entry.path, kind);
}

fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit_once('/')
        .map(|(p, _)| if p.is_empty() { "/" } else { p })
        .unwrap_or("/")
        .to_string()
}

fn handle_key(event: KeyEvent, app: &mut App) {
    if app.keybindings_visible {
        app.close_keybindings();
        return;
    }
    if app.is_searching() {
        match event.code {
            KeyCode::Esc => app.cancel_search(),
            KeyCode::Enter => app.confirm_search(),
            KeyCode::Backspace => app.pop_search_char(),
            KeyCode::Up => app.prev_item(),
            KeyCode::Down => app.next_item(),
            KeyCode::Char(c) if !event.ctrl && !event.alt => app.push_search_char(c),
            _ => {}
        }
        request_selected_preview(app);
        return;
    }

    match event.code {
        KeyCode::Up => app.prev_item(),
        KeyCode::Down => app.next_item(),
        KeyCode::Home => app.goto_top(),
        KeyCode::End => app.goto_bottom(),
        KeyCode::Tab => app.switch_pane(),
        KeyCode::Right => {
            if let Some((path, true)) = selected(app) {
                request_directory(&path, app.active_pane);
            }
        }
        KeyCode::Left => {
            if let Some(folder) = &app.panes[app.active_pane].folder {
                request_directory(&parent(&folder.path), app.active_pane);
            }
        }
        KeyCode::Enter => {
            if let Some((path, is_dir)) = selected(app) {
                if is_dir {
                    request_directory(&path, app.active_pane);
                } else {
                    rename_entry(&path);
                }
            }
        }
        KeyCode::Delete | KeyCode::Backspace => {
            if let Some((path, _)) = selected(app) {
                delete_entry(&path);
            }
        }
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('v' | 'V') => app.cycle_preview(),
        KeyCode::Char(' ') => app.toggle_select_current(),
        KeyCode::Char('+') => app.toggle_split(),
        KeyCode::Char('\\') => app.cycle_theme(),
        KeyCode::Char('*') => app.show_keybindings(),
        KeyCode::Char('n' | 'N') => {
            if let Some(folder) = &app.panes[app.active_pane].folder {
                create_entry(&folder.path);
            }
        }
        KeyCode::Char('e' | 'E') => {
            if let Some((path, false)) = selected(app) {
                open_editor(&path);
            }
        }
        KeyCode::Char('c' | 'C') => {
            if app.split {
                if let (Some((source, _)), Some(dest)) = (
                    selected(app),
                    app.panes[1 - app.active_pane].folder.as_ref(),
                ) {
                    transfer_entry(&source, &dest.path, false);
                }
            }
        }
        KeyCode::Char('m' | 'M') => {
            if app.split {
                if let (Some((source, _)), Some(dest)) = (
                    selected(app),
                    app.panes[1 - app.active_pane].folder.as_ref(),
                ) {
                    transfer_entry(&source, &dest.path, true);
                }
            }
        }
        KeyCode::Char('a' | 'A') if event.ctrl => app.toggle_select_all(),
        _ => {}
    }
    request_selected_preview(app);
}

#[wasm_bindgen]
pub fn ira_load_listing(json: &str, label: &str, path: &str, pane: usize) -> Result<(), JsValue> {
    let entries: Vec<BrowserEntry> =
        serde_json::from_str(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    push_browser_event(BrowserEvent::Listing {
        entries,
        label: label.to_string(),
        path: path.to_string(),
        pane,
    });
    Ok(())
}

#[wasm_bindgen]
pub fn ira_load_text(path: &str, content: String, binary: bool, truncated: bool) {
    push_browser_event(BrowserEvent::Text {
        path: path.to_string(),
        content,
        binary,
        truncated,
    });
}

#[wasm_bindgen]
pub fn ira_load_image(path: &str, bytes: &[u8]) {
    push_browser_event(BrowserEvent::Image {
        path: path.to_string(),
        bytes: bytes.to_vec(),
    });
}

#[wasm_bindgen]
pub fn ira_status(message: &str, is_error: bool) {
    push_browser_event(BrowserEvent::Status {
        message: message.to_string(),
        is_error,
    });
}

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();
    let mut app = App::default();
    app.drives = Some(Vec::new());
    app.folders = Some(Vec::new());
    app.bookmarks = Some(Vec::new());
    app.panes[0].preview_mode = PreviewMode::Details;
    app.panes[1].preview_mode = PreviewMode::Details;
    app.set_status(
        "Choose a folder to begin · files stay on this device",
        false,
    );
    let app = Rc::new(RefCell::new(app));
    APP.with(|slot| *slot.borrow_mut() = Some(app.clone()));

    let backend = DomBackend::new_by_id("ira-terminal")?;
    let mut terminal = Terminal::new(backend)?;
    terminal.on_key_event({
        let app = app.clone();
        move |event| handle_key(event, &mut app.borrow_mut())
    })?;
    terminal.draw_web(move |frame| {
        let mut app = app.borrow_mut();
        apply_browser_events(&mut app);
        ira::ui::render(&mut app, frame);
    });
    Ok(())
}
