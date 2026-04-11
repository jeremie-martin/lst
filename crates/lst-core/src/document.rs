use crate::position::Position;
use ropey::Rope;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_UNDO: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoBoundary {
    Merge,
    Break,
}

#[derive(Clone)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
    selection_reversed: bool,
}

#[derive(Clone)]
struct CachedLines {
    revision: u64,
    lines: Arc<[String]>,
}

pub struct Tab {
    pub name_hint: String,
    pub path: Option<PathBuf>,
    pub buffer: Rope,
    pub modified: bool,
    pub is_scratchpad: bool,
    pub selection: Range<usize>,
    pub selection_reversed: bool,
    pub preferred_column: Option<usize>,
    revision: u64,
    line_cache: Option<CachedLines>,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit_kind: Option<EditKind>,
}

impl Tab {
    pub fn new_scratchpad(path: PathBuf) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        Self::from_text(name_hint, Some(path), "", true)
    }

    pub fn empty(name_hint: String) -> Self {
        Self::from_text(name_hint, None, "", false)
    }

    pub fn from_path(path: PathBuf, text: &str) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        Self::from_text(name_hint, Some(path), text, false)
    }

    pub fn from_text(
        name_hint: String,
        path: Option<PathBuf>,
        text: &str,
        is_scratchpad: bool,
    ) -> Self {
        Self {
            name_hint,
            path,
            buffer: Rope::from_str(text),
            modified: false,
            is_scratchpad,
            selection: 0..0,
            selection_reversed: false,
            preferred_column: None,
            revision: 0,
            line_cache: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit_kind: None,
        }
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.name_hint.clone())
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn touch_content(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.line_cache = None;
    }

    pub fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }

    pub fn line_count(&self) -> usize {
        self.buffer.len_lines().max(1)
    }

    pub fn buffer_text(&self) -> String {
        self.buffer.to_string()
    }

    pub fn cursor_char(&self) -> usize {
        if self.selection_reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    pub fn cursor_position(&self) -> Position {
        char_to_position(&self.buffer, self.cursor_char())
    }

    pub fn selected_range(&self) -> Range<usize> {
        self.selection.clone()
    }

    pub fn has_selection(&self) -> bool {
        self.selection.start != self.selection.end
    }

    pub fn selected_text(&self) -> Option<String> {
        if self.has_selection() {
            Some(self.buffer.slice(self.selection.clone()).to_string())
        } else {
            None
        }
    }

    pub fn move_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        self.selection = offset..offset;
        self.selection_reversed = false;
    }

    pub fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        if self.selection_reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.selection_reversed = !self.selection_reversed;
            self.selection = self.selection.end..self.selection.start;
        }
    }

    pub fn select_all(&mut self) {
        let end = self.len_chars();
        self.selection = 0..end;
        self.selection_reversed = false;
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer = Rope::from_str(text);
        self.move_to(0);
        self.modified = false;
        self.preferred_column = None;
        self.touch_content();
        self.last_edit_kind = None;
    }

    pub fn replace_char_range(&mut self, mut range: Range<usize>, new_text: &str) -> usize {
        range.start = range.start.min(self.len_chars());
        range.end = range.end.min(self.len_chars());
        if range.start > range.end {
            range = range.end..range.start;
        }

        if range.start != range.end {
            self.buffer.remove(range.clone());
        }
        if !new_text.is_empty() {
            self.buffer.insert(range.start, new_text);
        }

        let new_cursor = range.start + new_text.chars().count();
        self.selection = new_cursor..new_cursor;
        self.selection_reversed = false;
        self.modified = true;
        self.preferred_column = None;
        self.touch_content();
        new_cursor
    }

    pub fn edit(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        new_text: &str,
    ) -> usize {
        self.push_undo_snapshot(kind, boundary);
        self.replace_char_range(range, new_text)
    }

    pub fn set_cursor_position(&mut self, position: Position, select_from: Option<Position>) {
        let head = position_to_char(&self.buffer, position);
        match select_from {
            Some(anchor) => {
                let anchor = position_to_char(&self.buffer, anchor);
                self.selection = anchor.min(head)..anchor.max(head);
                self.selection_reversed = head < anchor;
            }
            None => self.move_to(head),
        }
    }

    pub fn lines(&mut self) -> Arc<[String]> {
        if let Some(cache) = &self.line_cache {
            if cache.revision == self.revision {
                return Arc::clone(&cache.lines);
            }
        }

        let lines: Arc<[String]> = self
            .buffer
            .to_string()
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect::<Vec<_>>()
            .into();

        self.line_cache = Some(CachedLines {
            revision: self.revision,
            lines: Arc::clone(&lines),
        });

        lines
    }

    pub fn push_undo_snapshot(&mut self, kind: EditKind, boundary: UndoBoundary) {
        let kind_changed = self.last_edit_kind != Some(kind);
        let is_streaming = matches!(kind, EditKind::Insert | EditKind::Delete);
        let should_snapshot =
            kind_changed || !is_streaming || matches!(boundary, UndoBoundary::Break);

        if should_snapshot {
            self.undo_stack.push(self.current_snapshot());
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
        self.last_edit_kind = Some(kind);
    }

    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.modified = true;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            self.modified = true;
            true
        } else {
            false
        }
    }

    fn current_snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.buffer_text(),
            selection: self.selection.clone(),
            selection_reversed: self.selection_reversed,
        }
    }

    fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.buffer = Rope::from_str(&snapshot.text);
        self.selection = snapshot.selection;
        self.selection_reversed = snapshot.selection_reversed;
        self.preferred_column = None;
        self.touch_content();
        self.last_edit_kind = None;
    }
}

pub fn char_to_position(buffer: &Rope, char_offset: usize) -> Position {
    let char_offset = char_offset.min(buffer.len_chars());
    let line = buffer.char_to_line(char_offset);
    let line_start = buffer.line_to_char(line);
    Position {
        line,
        column: char_offset - line_start,
    }
}

pub fn position_to_char(buffer: &Rope, position: Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let line_len = buffer
        .line(line)
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count();
    line_start + position.column.min(line_len)
}

pub fn line_indent_prefix(buffer: &Rope, line_ix: usize) -> String {
    buffer
        .line(line_ix.min(buffer.len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .take_while(|ch| ch.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_restore_text_and_selection() {
        let mut tab = Tab::from_text("untitled".into(), None, "hello", false);
        tab.move_to(5);
        tab.edit(EditKind::Insert, UndoBoundary::Merge, 5..5, " world");

        assert_eq!(tab.buffer_text(), "hello world");
        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "hello");
        assert_eq!(tab.cursor_char(), 5);
        assert!(tab.redo());
        assert_eq!(tab.buffer_text(), "hello world");
    }

    #[test]
    fn position_conversion_clamps_to_line_width() {
        let tab = Tab::from_text("untitled".into(), None, "abc\ndef", false);
        assert_eq!(
            position_to_char(
                &tab.buffer,
                Position {
                    line: 0,
                    column: 99
                }
            ),
            3
        );
        assert_eq!(
            char_to_position(&tab.buffer, 5),
            Position { line: 1, column: 1 }
        );
    }

    #[test]
    fn lines_strip_cr_from_crlf_content() {
        let mut tab = Tab::from_text("untitled".into(), None, "alpha\r\nbeta\r\n", false);
        let lines = tab.lines();

        assert_eq!(lines.as_ref(), ["alpha", "beta", ""]);
    }

    #[test]
    fn undo_boundaries_split_streaming_edits() {
        let mut tab = Tab::from_text("untitled".into(), None, "", false);

        tab.edit(EditKind::Insert, UndoBoundary::Merge, 0..0, "a");
        tab.edit(EditKind::Insert, UndoBoundary::Break, 1..1, " paste");

        assert_eq!(tab.buffer_text(), "a paste");
        assert!(tab.undo());
        assert_eq!(tab.buffer_text(), "a");
    }
}
