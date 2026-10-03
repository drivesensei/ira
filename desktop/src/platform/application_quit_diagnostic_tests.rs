use super::*;

#[test]
fn structural_metadata_escapes_control_characters_without_multiline_output() {
    assert_eq!(
        bounded_structure(b"GPUI\nDelegate\r\t\0"),
        "GPUI\\nDelegate\\r\\t\\x00"
    );
    assert!(!bounded_structure(b"\n\r").contains('\n'));
}
#[test]
fn structural_metadata_has_fixed_output_bound_even_for_invalid_utf8() {
    let bytes = vec![0xff; 1024];
    let output = bounded_structure(&bytes);
    assert!(output.len() <= STRUCTURE_BYTES * 4 + 3);
    assert!(output.ends_with("..."));
    assert_eq!(bounded_structure(&[0xff]), "\\xff");
}
#[test]
fn pinned_class_names_and_void_pointer_encoding_remain_exact_in_diagnostics() {
    assert_eq!(bounded_structure(b"GPUIApplication"), "GPUIApplication");
    assert_eq!(
        bounded_structure(b"GPUIApplicationDelegate"),
        "GPUIApplicationDelegate"
    );
    assert_eq!(bounded_structure(b"^v"), "^v");
    assert_eq!(bounded_structure(&[]), "");
}
