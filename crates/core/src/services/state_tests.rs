use super::{
    parse_folder, parse_size_entry, parse_state, serialize_state, SessionState, SizeEntry,
};
use crate::domain::data::Folder;

#[test]
fn default_state_opens_in_details_mode() {
    // A fresh install (no state file) opens with the details columns.
    assert_eq!(SessionState::default().preview, [3, 3]);
    // So does a state file written before `preview=` existed.
    let parsed = parse_state("split=0\nhidden=0\n");
    assert_eq!(parsed.preview, [3, 3]);
    // An explicit choice is preserved: 0 = off, 2 = grid.
    let parsed = parse_state("preview0=0\npreview1=2\n");
    assert_eq!(parsed.preview, [0, 2]);
}

#[test]
fn parses_label_path_pairs_and_bare_paths() {
    let f = parse_folder("Home\t/home/vlad").unwrap();
    assert_eq!(f.label, "Home");
    assert_eq!(f.path, "/home/vlad");

    assert!(parse_folder("").is_none());
    assert!(parse_folder("label\t").is_none()); // empty path

    let bare = parse_folder("/home/vlad").unwrap();
    assert_eq!(bare.label, "vlad");
    assert_eq!(bare.path, "/home/vlad");
}

#[test]
fn size_entries_roundtrip_through_the_state_format() {
    let state = SessionState {
        split: true,
        active_pane: 1,
        show_hidden: true,
        left: Some(Folder::new(
            "Home".to_string(),
            "/home/vlad".to_string(),
            '#',
        )),
        right: Some(Folder::new(
            "Data".to_string(),
            "/mnt/data".to_string(),
            '#',
        )),
        preview: [1, 2],
        theme: Some("cyberpunk2077".to_string()),
        sizes: vec![
            SizeEntry {
                path: "/home/vlad/big folder".to_string(),
                bytes: 3_221_225_472,
                items: 123_456,
                on_disk: 3_300_000_000,
                complete: true,
                updated_epoch: 1_790_000_000,
            },
            SizeEntry {
                path: "/mnt/a=b/path=with=equals".to_string(),
                bytes: 1,
                items: 2,
                on_disk: 3,
                complete: false,
                updated_epoch: 42,
            },
        ],
    };
    assert_eq!(parse_state(&serialize_state(&state)), state);
    assert!(state.show_hidden, "hidden flag must roundtrip");
    assert_eq!(state.preview, [1, 2], "preview modes must roundtrip");
    assert_eq!(state.theme.as_deref(), Some("cyberpunk2077"));
}

#[test]
fn legacy_icons_line_is_ignored_and_not_rewritten() {
    let parsed = parse_state("icons=unicode\ntheme=nord\n");
    assert_eq!(parsed.theme.as_deref(), Some("nord"));
    assert!(
        !serialize_state(&parsed).contains("icons="),
        "the icon set is never persisted anymore"
    );
}

#[test]
fn theme_key_normalizes_aliases_and_drops_unknown() {
    let parsed = parse_state("theme=Tokyo_Night\n");
    assert_eq!(parsed.theme.as_deref(), Some("tokyo-night"));
    let parsed = parse_state("theme=not-a-theme\n");
    assert_eq!(parsed.theme, None);
}

#[test]
fn malformed_size_lines_are_ignored() {
    let parsed = parse_state("size=not\tnumbers\nsize=1\t2\nsize=1\t2\t3\t1\t5\t/ok\n");
    assert_eq!(parsed.sizes.len(), 1);
    assert_eq!(parsed.sizes[0].path, "/ok");
    assert!(parse_size_entry("").is_none());
    assert!(parse_size_entry("1\t2\t3\t2\t5\t/p").is_none()); // bad complete flag
    assert!(parse_size_entry("1\t2\t3\t1\t5\t").is_none()); // empty path
}
