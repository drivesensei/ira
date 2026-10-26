#[path = "../../src/editor.rs"]
pub mod editor;
#[path = "../../src/preview.rs"]
pub mod preview;
#[test]
fn published_preview_and_editor_limits_match_source() {
    assert_eq!(editor::EDIT_MAX_BYTES, 5 * 1024 * 1024);
    assert_eq!(preview::TEXT_PREVIEW_CAP, 256 * 1024);
    assert_eq!(preview::MAX_DECODE_THREADS, 6);
    assert_eq!(preview::JOB_QUEUE_HI_CAP, 32);
    assert_eq!(preview::JOB_QUEUE_LO_CAP, 256);
    assert_eq!(
        preview::preview_kind(".env.local"),
        Some(preview::PreviewKind::Text)
    );
}
mod preview_tests;
