use super::*;
use std::fs;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ira-terminal-editor-safety-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn app(&self) -> (App, PathBuf) {
        let path = self.0.join("document.txt");
        fs::write(&path, b"document").unwrap();
        let mut app = App::default();
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "document.txt".into(),
            is_dir: false,
            size: 8,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane();
        assert!(app.edit.is_some());
        assert!(app.edit_input(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('X'),
            ratatui::crossterm::event::KeyModifiers::empty()
        )));
        (app, path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn terminal_save_ignores_legacy_temp_symlink_and_preserves_victim() {
    let f = Fixture::new();
    let (mut app, path) = f.app();
    let victim = f.0.join("victim.txt");
    fs::write(&victim, b"unrelated").unwrap();
    let tmp = f.0.join("document.txt.ira-tmp");
    std::os::unix::fs::symlink(&victim, &tmp).unwrap();
    app.save_edit();
    assert_eq!(
        fs::read(&victim).unwrap(),
        b"unrelated",
        "victim must survive"
    );
    assert_eq!(fs::read(&path).unwrap(), b"Xdocument");
    assert!(!fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&tmp).unwrap(), victim);
    assert!(!app.edit.as_ref().unwrap().dirty);
}

#[test]
fn terminal_save_ignores_legacy_regular_temp_and_saves_draft() {
    let f = Fixture::new();
    let (mut app, path) = f.app();
    let tmp = f.0.join("document.txt.ira-tmp");
    fs::write(&tmp, b"unowned").unwrap();
    app.save_edit();
    assert_eq!(fs::read(&path).unwrap(), b"Xdocument");
    assert_eq!(fs::read(&tmp).unwrap(), b"unowned");
    assert!(!app.edit.as_ref().unwrap().dirty);
}
