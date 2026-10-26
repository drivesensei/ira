use super::*;

#[test]
fn returns_immediate_parent() {
    let folder = get_parent_directory("/a/b/c").unwrap().unwrap();
    assert_eq!(folder.path, "/a/b");
    assert_eq!(folder.label, "b");
    assert_eq!(folder.shortcut, '#');
}

#[test]
fn parent_of_single_level_is_root() {
    let folder = get_parent_directory("/a").unwrap().unwrap();
    assert_eq!(folder.path, "/");
    assert_eq!(folder.label, "");
}

#[test]
fn root_has_no_parent() {
    assert!(get_parent_directory("/").unwrap().is_none());
}

#[test]
fn trailing_slash_is_normalized() {
    let folder = get_parent_directory("/a/b/c/").unwrap().unwrap();
    assert_eq!(folder.path, "/a/b");
    assert_eq!(folder.label, "b");
}
