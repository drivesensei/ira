use gpui::{App, KeyBinding, Menu, MenuItem, actions};
use ira_core::input::{Input, KeyCode, KeyEvent, KeyModifiers};
actions!(
    ira,
    [
        Quit,
        Rename,
        Open,
        Parent,
        Search,
        NewEntry,
        Copy,
        Move,
        Delete,
        ToggleSplit,
        ToggleBoard,
        Sort,
        Hidden,
        Bookmark,
        Info,
        Theme,
        FocusNext
    ]
);
pub fn input(code: KeyCode) -> Input {
    Input::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
pub fn register(cx: &mut App) {
    // Normal keys are decoded centrally; menu/native aliases enter the same gateway.
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.set_menus(vec![
        Menu {
            name: "IRA".into(),
            items: vec![MenuItem::action("Quit IRA", Quit)],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open", Open),
                MenuItem::action("Rename", Rename),
                MenuItem::action("New file or folder", NewEntry),
                MenuItem::action("Copy to other pane", Copy),
                MenuItem::action("Move to other pane", Move),
                MenuItem::action("Delete", Delete),
                MenuItem::action("Information", Info),
            ],
        },
        Menu {
            name: "Navigate".into(),
            items: vec![
                MenuItem::action("Parent folder", Parent),
                MenuItem::action("Search", Search),
                MenuItem::action("Bookmark folder", Bookmark),
                MenuItem::action("Next focus", FocusNext),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Split panes", ToggleSplit),
                MenuItem::action("Hidden files", Hidden),
                MenuItem::action("Sort", Sort),
                MenuItem::action("Copy Board", ToggleBoard),
                MenuItem::action("Theme", Theme),
            ],
        },
    ]);
}
