use ira_core::{input::InputContext, observable::Snapshot};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Rename,
    Goto,
    Create,
    Search,
    Editor,
}
pub fn input_mode(snapshot: &Snapshot) -> Option<InputMode> {
    match snapshot.input_context {
        InputContext::Editor => Some(InputMode::Editor),
        InputContext::Rename => Some(InputMode::Rename),
        InputContext::Goto => Some(InputMode::Goto),
        InputContext::Create => Some(InputMode::Create),
        InputContext::Search => Some(InputMode::Search),
        _ => None,
    }
}
