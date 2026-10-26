use super::*;

fn folder(shortcut: char) -> Folder {
    Folder::new("x".to_string(), "/x".to_string(), shortcut)
}

#[test]
fn assigns_letters_in_keyboard_order_skipping_used() {
    let mut bookmarks: Vec<Folder> = Vec::new();
    let mut assigned: Vec<char> = Vec::new();
    while let Some(sc) = next_free_shortcut(&bookmarks) {
        assigned.push(sc);
        bookmarks.push(folder(sc));
    }

    assert_eq!(
        assigned,
        vec!['o', 'p', 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'n']
    );
}
