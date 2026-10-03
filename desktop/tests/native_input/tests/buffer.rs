use ira_native_input_validation::text_buffer::TextBuffer;
#[test]
fn astral_utf16_ranges_never_split_utf8() {
    let b = TextBuffer::new("a😀b");
    assert_eq!(b.byte(2), 1);
    assert_eq!(b.from_utf16(1..3), 1..5);
    assert_eq!(b.to_utf16(1..5), 1..3);
    assert_eq!(b.byte(999), 6);
}
#[test]
fn combining_and_emoji_graphemes_delete_atomically() {
    let mut b = TextBuffer::new("a e\u{301}👩‍👩‍👧‍👦");
    b.delete(false);
    assert_eq!(b.text(), "a e\u{301}");
    b.delete(false);
    assert_eq!(b.text(), "a ");
    b.undo();
    assert_eq!(b.text(), "a e\u{301}");
}
#[test]
fn reversed_selection_crosses_anchor_safely() {
    let mut b = TextBuffer::new("abc");
    b.select(2, 1);
    assert!(b.reversed());
    b.horizontal(true, true);
    assert_eq!(b.selection(), 2..2);
    b.horizontal(true, true);
    assert_eq!(b.selection(), 2..3);
    assert!(!b.reversed());
}
#[test]
fn composition_relative_selection_uses_new_text_not_old_buffer() {
    let mut b = TextBuffer::new("prefix suffix");
    b.select(7, 7);
    b.compose(None, "😀あ", Some(2..3));
    assert_eq!(b.selection(), 11..14);
    assert_eq!(b.marked(), Some(7..14));
    b.compose(None, "😀い", Some(3..3));
    b.replace(None, "😀う", false);
    assert_eq!(b.text(), "prefix 😀うsuffix");
    assert_eq!(b.marked(), None);
    b.undo();
    assert_eq!(b.text(), "prefix suffix");
    b.redo();
    assert_eq!(b.text(), "prefix 😀うsuffix");
}
#[test]
fn unmark_composition_is_one_undo_unit() {
    let mut b = TextBuffer::new("");
    b.compose(None, "n", None);
    b.compose(None, "に", None);
    b.unmark();
    b.undo();
    assert_eq!(b.text(), "");
    b.redo();
    assert_eq!(b.text(), "に");
}
#[test]
fn multiline_vertical_retains_grapheme_column_across_short_lines() {
    let mut b = TextBuffer::new("a😀cd\nx\na😀cd");
    b.select(6, 6);
    b.vertical(true, false);
    assert_eq!(b.caret(), 9);
    b.vertical(true, false);
    assert_eq!(b.caret(), 16);
    b.vertical(false, false);
    assert_eq!(b.caret(), 9);
}
#[test]
fn typing_groups_break_on_navigation_and_paste() {
    let mut b = TextBuffer::new("");
    b.replace(None, "a", true);
    b.replace(None, "b", true);
    b.move_to(1, false);
    b.replace(None, "X", false);
    b.undo();
    assert_eq!(b.text(), "ab");
    b.undo();
    assert_eq!(b.text(), "");
    b.redo();
    assert_eq!(b.text(), "ab");
}
#[test]
fn multiline_selection_replace_and_undo_restores_selection() {
    let mut b = TextBuffer::new("first\nsecond\nthird");
    b.select(3, 14);
    let before = b.snapshot();
    b.replace(None, "😀\n", false);
    assert_eq!(b.text(), "fir😀\nhird");
    b.undo();
    assert_eq!(b.snapshot(), before);
}
#[test]
fn editor_ctrl_a_line_start_does_not_select_all() {
    let mut b = TextBuffer::new("first\nsecond");
    b.move_to(b.line_start(), false);
    assert_eq!(b.caret(), 6);
    assert!(b.selection().is_empty());
}
#[test]
fn empty_and_out_of_range_selection_remain_safe() {
    let mut b = TextBuffer::new("");
    b.select(usize::MAX, usize::MAX);
    b.delete(true);
    b.delete(false);
    b.vertical(true, true);
    assert_eq!(b.text(), "");
    assert_eq!(b.selection(), 0..0);
}
#[test]
fn inserted_base_character_keeps_caret_on_combining_grapheme_boundary() {
    let mut b = TextBuffer::new("\u{301}x");
    b.select(0, 0);
    b.replace(None, "e", false);
    assert_eq!(b.text(), "e\u{301}x");
    assert_eq!(b.caret(), 3);
    b.delete(false);
    assert_eq!(b.text(), "x");
}

#[test]
fn strict_utf16_selection_rejects_surrogates_combining_and_emoji_cluster_splits() {
    use ira_native_input_validation::text_buffer::SelectionError;
    let b = TextBuffer::new("a😀e\u{301}👩‍👩‍👧‍👦z");
    assert_eq!(b.checked_utf16_selection(1..5), Ok(1..8));
    assert_eq!(
        b.checked_utf16_selection(2..3),
        Err(SelectionError::InvalidBoundary)
    );
    assert_eq!(
        b.checked_utf16_selection(3..4),
        Err(SelectionError::InvalidBoundary)
    );
    assert_eq!(
        b.checked_utf16_selection(5..7),
        Err(SelectionError::InvalidBoundary)
    );
    assert_eq!(
        b.checked_utf16_selection(4..3),
        Err(SelectionError::InvalidRange)
    );
    assert_eq!(
        b.checked_utf16_selection(0..usize::MAX),
        Err(SelectionError::InvalidRange)
    );
    let empty = TextBuffer::new("");
    assert_eq!(empty.checked_utf16_selection(0..0), Ok(0..0));
    assert_eq!(
        empty.checked_utf16_selection(0..1),
        Err(SelectionError::InvalidRange)
    );
}
