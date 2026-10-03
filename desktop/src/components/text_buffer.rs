//! Host-owned UTF-8 draft; platform input ranges are UTF-16.
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferSnapshot {
    pub text: String,
    pub selection: Range<usize>,
    pub reversed: bool,
    pub marked: Option<Range<usize>>,
}
#[derive(Clone, Debug)]
pub struct TextBuffer {
    state: BufferSnapshot,
    undo: Vec<BufferSnapshot>,
    redo: Vec<BufferSnapshot>,
    group: bool,
    composition: Option<BufferSnapshot>,
    preferred_column: Option<usize>,
}
impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.len();
        Self {
            state: BufferSnapshot {
                text,
                selection: end..end,
                reversed: false,
                marked: None,
            },
            undo: vec![],
            redo: vec![],
            group: false,
            composition: None,
            preferred_column: None,
        }
    }
    pub fn snapshot(&self) -> BufferSnapshot {
        self.state.clone()
    }
    pub fn text(&self) -> &str {
        &self.state.text
    }
    pub fn selection(&self) -> Range<usize> {
        self.state.selection.clone()
    }
    pub fn marked(&self) -> Option<Range<usize>> {
        self.state.marked.clone()
    }
    pub fn reversed(&self) -> bool {
        self.state.reversed
    }
    pub fn caret(&self) -> usize {
        if self.state.reversed {
            self.state.selection.start
        } else {
            self.state.selection.end
        }
    }
    pub fn utf16(&self, byte: usize) -> usize {
        self.text()[..self.char_floor(byte)].encode_utf16().count()
    }
    fn char_floor(&self, byte: usize) -> usize {
        let mut b = byte.min(self.text().len());
        while !self.text().is_char_boundary(b) {
            b -= 1;
        }
        b
    }
    pub fn byte(&self, units: usize) -> usize {
        let mut n = 0;
        for (b, c) in self.text().char_indices() {
            if n + c.len_utf16() > units {
                return b;
            }
            n += c.len_utf16();
        }
        self.text().len()
    }
    pub fn from_utf16(&self, r: Range<usize>) -> Range<usize> {
        let a = self.byte(r.start);
        let b = self.byte(r.end);
        a.min(b)..a.max(b)
    }
    pub fn to_utf16(&self, r: Range<usize>) -> Range<usize> {
        self.utf16(r.start)..self.utf16(r.end)
    }
    pub fn previous(&self, b: usize) -> usize {
        self.text()
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .filter(|i| *i < b)
            .next_back()
            .unwrap_or(0)
    }
    pub fn next(&self, b: usize) -> usize {
        self.text()
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i > b)
            .unwrap_or(self.text().len())
    }
    fn boundary(&self, b: usize) -> usize {
        if b >= self.text().len() {
            return self.text().len();
        }
        self.text()
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i <= b)
            .last()
            .unwrap_or(0)
    }
    pub fn break_group(&mut self) {
        self.group = false;
        self.preferred_column = None;
    }
    pub fn select(&mut self, anchor: usize, caret: usize) {
        self.unmark();
        self.break_group();
        let a = self.boundary(anchor);
        let c = self.boundary(caret);
        self.state.selection = a.min(c)..a.max(c);
        self.state.reversed = c < a;
    }
    pub fn move_to(&mut self, b: usize, extend: bool) {
        let anchor = if extend {
            if self.reversed() {
                self.selection().end
            } else {
                self.selection().start
            }
        } else {
            b
        };
        self.select(anchor, b);
    }
    pub fn horizontal(&mut self, right: bool, extend: bool) {
        let b = if !extend && !self.selection().is_empty() {
            if right {
                self.selection().end
            } else {
                self.selection().start
            }
        } else if right {
            self.next(self.caret())
        } else {
            self.previous(self.caret())
        };
        self.move_to(b, extend);
    }
    pub fn line_start(&self) -> usize {
        self.text()[..self.caret()].rfind('\n').map_or(0, |i| i + 1)
    }
    pub fn line_end(&self) -> usize {
        self.text()[self.caret()..]
            .find('\n')
            .map_or(self.text().len(), |i| self.caret() + i)
    }
    pub fn vertical(&mut self, down: bool, extend: bool) {
        let start = self.line_start();
        let col = self
            .preferred_column
            .unwrap_or_else(|| self.text()[start..self.caret()].graphemes(true).count());
        let target = if down {
            let end = self.line_end();
            if end == self.text().len() {
                self.text().len()
            } else {
                end + 1
            }
        } else if start == 0 {
            0
        } else {
            self.text()[..start - 1].rfind('\n').map_or(0, |i| i + 1)
        };
        let end = self.text()[target..]
            .find('\n')
            .map_or(self.text().len(), |i| target + i);
        let b = self.text()[target..end]
            .grapheme_indices(true)
            .nth(col)
            .map_or(end, |(i, _)| target + i);
        self.move_to(b, extend);
        self.preferred_column = Some(col);
    }
    fn history(&mut self, group: bool) {
        if self.composition.is_none() && !(group && self.group) {
            let mut s = self.snapshot();
            s.marked = None;
            self.undo.push(s);
        }
        self.redo.clear();
        self.group = group;
        self.preferred_column = None;
    }
    fn replace_raw(&mut self, r: Range<usize>, text: &str) {
        self.state.text.replace_range(r.clone(), text);
        let b = r.start + text.len();
        self.state.selection = b..b;
        self.state.reversed = false;
    }
    pub fn replace(&mut self, range: Option<Range<usize>>, text: &str, group: bool) {
        let r = range
            .map(|r| self.from_utf16(r))
            .or(self.marked())
            .unwrap_or_else(|| self.selection());
        self.history(group);
        self.replace_raw(r, text);
        self.state.marked = None;
        if let Some(mut before) = self.composition.take() {
            before.marked = None;
            self.undo.push(before);
            self.group = false;
        }
    }
    pub fn compose(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
    ) {
        if self.composition.is_none() {
            self.break_group();
            self.composition = Some(self.snapshot());
            self.redo.clear();
        }
        let r = range
            .map(|r| self.from_utf16(r))
            .or(self.marked())
            .unwrap_or_else(|| self.selection());
        let start = r.start;
        self.replace_raw(r, text);
        self.state.marked = Some(start..start + text.len());
        if let Some(s) = selection {
            let temp = Self::new(text);
            let rel = temp.from_utf16(s);
            self.state.selection = start + rel.start..start + rel.end;
        }
    }
    pub fn unmark(&mut self) {
        self.state.marked = None;
        if let Some(mut before) = self.composition.take() {
            before.marked = None;
            if before.text != self.state.text {
                self.undo.push(before);
            }
            self.break_group();
        }
    }
    pub fn delete(&mut self, forward: bool) {
        self.unmark();
        if self.selection().is_empty() {
            let c = self.caret();
            self.state.selection = if forward {
                c..self.next(c)
            } else {
                self.previous(c)..c
            };
        }
        if !self.selection().is_empty() {
            self.replace(None, "", false);
        }
    }
    pub fn undo(&mut self) {
        self.unmark();
        self.break_group();
        if let Some(s) = self.undo.pop() {
            let current = self.snapshot();
            self.redo.push(current);
            self.state = s;
        }
    }
    pub fn redo(&mut self) {
        self.unmark();
        self.break_group();
        if let Some(s) = self.redo.pop() {
            let current = self.snapshot();
            self.undo.push(current);
            self.state = s;
        }
    }
}
