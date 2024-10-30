use crate::domain::data::Folder;

pub fn list_bookmarks() -> Vec<Folder> {
    vec![
        Folder::new(String::from("Projects"), String::from("~/projects"), 'y'),
        Folder::new(String::from(".ssh"), String::from("~/.ssh"), 'u'),
    ]
}
