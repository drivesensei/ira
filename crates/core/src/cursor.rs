//! Cursor semantics of the terminal list without a widget dependency.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CursorState {
    selected: Option<usize>,
    offset: usize,
}
impl CursorState {
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub const fn offset(&self) -> usize {
        self.offset
    }
    pub const fn offset_mut(&mut self) -> &mut usize {
        &mut self.offset
    }
    pub const fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        if index.is_none() {
            self.offset = 0;
        }
    }
}
