use super::*;

fn ticket(request_id: u64, kind: ChooserKind) -> ChooserTicket {
    ChooserTicket { request_id, kind }
}

fn absolute(spelling: &str) -> PathBuf {
    #[cfg(windows)]
    let root = PathBuf::from(r"C:\");
    #[cfg(not(windows))]
    let root = PathBuf::from("/");
    root.join(spelling)
}

fn selected(paths: Vec<PathBuf>) -> ChooserOutcome {
    normalize_received(Ok(Ok(Some(paths))))
}

#[test]
fn exact_file_and_folder_gpui_options() {
    for kind in [ChooserKind::File, ChooserKind::Folder] {
        let actual: gpui::PathPromptOptions = options(kind);
        assert_eq!(actual.files, kind == ChooserKind::File);
        assert_eq!(actual.directories, kind == ChooserKind::Folder);
        assert!(!actual.multiple);
        assert_eq!(actual.prompt.as_deref(), Some("Select"));
    }
}

#[test]
fn outer_receiver_loss_retains_classification_and_message() {
    assert_eq!(
        normalize_received(Err("lost channel".into())),
        ChooserOutcome::Error("Chooser receiver failed: lost channel".into())
    );
}

#[test]
fn inner_prompt_error_retains_classification_and_message() {
    assert_eq!(
        normalize_received(Ok(Err("native failure".into()))),
        ChooserOutcome::Error("Chooser prompt failed: native failure".into())
    );
}

#[test]
fn none_classifies_api_no_selection() {
    assert_eq!(normalize_received(Ok(Ok(None))), ChooserOutcome::Canceled);
}

#[test]
fn empty_selection_is_error() {
    assert!(matches!(selected(vec![]), ChooserOutcome::Error(_)));
}

#[test]
fn multiple_selection_including_duplicates_is_error() {
    let path = absolute("synthetic");
    assert!(matches!(
        selected(vec![path.clone(), path]),
        ChooserOutcome::Error(_)
    ));
    assert!(matches!(
        selected(vec![absolute("one"), absolute("two")]),
        ChooserOutcome::Error(_)
    ));
}

#[test]
fn empty_and_relative_paths_are_errors() {
    for path in [
        PathBuf::new(),
        PathBuf::from("relative"),
        PathBuf::from("~/synthetic"),
    ] {
        assert!(matches!(selected(vec![path]), ChooserOutcome::Error(_)));
    }
}

#[test]
fn unicode_spaces_and_dot_components_are_transported_unchanged() {
    let path = absolute(" synthetic /../日本語 🦀 /file ");
    match selected(vec![path.clone()]) {
        ChooserOutcome::Selected(actual) => assert_eq!(actual.as_os_str(), path.as_os_str()),
        other => panic!("expected selection, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn unix_invalid_utf8_path_transport_is_unchanged() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let raw = b"/synthetic/invalid-\xff ".to_vec();
    let path = PathBuf::from(std::ffi::OsString::from_vec(raw.clone()));
    assert!(path.to_str().is_none());
    match selected(vec![path]) {
        ChooserOutcome::Selected(actual) => assert_eq!(actual.as_os_str().as_bytes(), raw),
        other => panic!("expected raw selection, got {other:?}"),
    }
}

#[cfg(windows)]
#[test]
fn windows_unpaired_utf16_path_transport_is_unchanged() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let raw = vec![67, 58, 92, 0xd800, 32];
    let path = PathBuf::from(std::ffi::OsString::from_wide(&raw));
    assert!(path.to_str().is_none());
    match selected(vec![path]) {
        ChooserOutcome::Selected(actual) => {
            assert_eq!(actual.as_os_str().encode_wide().collect::<Vec<_>>(), raw)
        }
        other => panic!("expected raw selection, got {other:?}"),
    }
}

#[test]
fn new_and_default_have_no_active_ticket() {
    assert_eq!(SingleFlight::new().active(), None);
    assert_eq!(SingleFlight::default().active(), None);
}

#[test]
fn busy_acquire_leaves_original_ticket_unchanged() {
    let first = ticket(1, ChooserKind::File);
    let mut flight = SingleFlight::new();
    assert_eq!(flight.try_acquire(first), Ok(()));
    for attempted in [first, ticket(2, ChooserKind::Folder)] {
        assert_eq!(flight.try_acquire(attempted), Err(Busy));
        assert_eq!(flight.active(), Some(first));
    }
}

#[test]
fn wrong_kind_and_stale_release_do_not_clear_current_ticket() {
    let first = ticket(1, ChooserKind::File);
    let newer = ticket(2, ChooserKind::Folder);
    let mut flight = SingleFlight::new();
    assert!(!flight.release(first));
    assert_eq!(flight.try_acquire(first), Ok(()));
    assert!(!flight.release(ticket(1, ChooserKind::Folder)));
    assert!(!flight.release(newer));
    assert_eq!(flight.active(), Some(first));
    assert!(flight.release(first));
    assert!(!flight.release(first));
    assert_eq!(flight.try_acquire(newer), Ok(()));
    assert!(!flight.release(first));
    assert_eq!(flight.active(), Some(newer));
    assert!(flight.release(newer));
    assert_eq!(flight.active(), None);
}
