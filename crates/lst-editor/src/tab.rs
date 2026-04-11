use lst_core::{
    document::{EditKind, Tab, UndoBoundary},
    position::Position,
};
use std::{ops::Range, path::PathBuf, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    pub fn from_raw(id: u64) -> Self {
        Self(id)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone)]
pub struct EditorTab {
    id: TabId,
    pub(crate) doc: Tab,
    pub marked_range: Option<Range<usize>>,
}

impl EditorTab {
    pub fn empty(id: TabId, name_hint: String) -> Self {
        Self::from_doc(id, Tab::empty(name_hint))
    }

    pub fn from_path(id: TabId, path: PathBuf, text: &str) -> Self {
        Self::from_doc(id, Tab::from_path(path, text))
    }

    pub fn from_text(id: TabId, name_hint: String, path: Option<PathBuf>, text: &str) -> Self {
        Self::from_doc(id, Tab::from_text(name_hint, path, text, false))
    }

    pub fn from_doc(id: TabId, doc: Tab) -> Self {
        Self {
            id,
            doc,
            marked_range: None,
        }
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.doc.path.as_ref()
    }

    pub fn buffer(&self) -> &ropey::Rope {
        &self.doc.buffer
    }

    pub fn buffer_clone(&self) -> ropey::Rope {
        self.doc.buffer.clone()
    }

    pub fn selection(&self) -> Range<usize> {
        self.doc.selection.clone()
    }

    pub fn selection_reversed(&self) -> bool {
        self.doc.selection_reversed
    }

    pub fn marked_range(&self) -> Option<&Range<usize>> {
        self.marked_range.as_ref()
    }

    pub fn modified(&self) -> bool {
        self.doc.modified
    }

    pub fn display_name(&self) -> String {
        self.doc.display_name()
    }

    pub fn revision(&self) -> u64 {
        self.doc.revision()
    }

    pub fn len_chars(&self) -> usize {
        self.doc.len_chars()
    }

    pub fn line_count(&self) -> usize {
        self.doc.line_count()
    }

    pub fn buffer_text(&self) -> String {
        self.doc.buffer_text()
    }

    pub fn cursor_char(&self) -> usize {
        self.doc.cursor_char()
    }

    pub fn cursor_position(&self) -> Position {
        self.doc.cursor_position()
    }

    pub fn selected_range(&self) -> Range<usize> {
        self.doc.selected_range()
    }

    pub fn has_selection(&self) -> bool {
        self.doc.has_selection()
    }

    pub fn selected_text(&self) -> Option<String> {
        self.doc.selected_text()
    }

    pub fn lines(&mut self) -> Arc<[String]> {
        self.doc.lines()
    }

    pub fn select_all(&mut self) {
        self.doc.select_all();
        self.marked_range = None;
    }

    pub(crate) fn set_cursor_position(
        &mut self,
        position: Position,
        select_from: Option<Position>,
    ) {
        self.doc.set_cursor_position(position, select_from);
        self.marked_range = None;
    }

    pub(crate) fn push_undo_snapshot(&mut self, kind: EditKind, boundary: UndoBoundary) {
        self.doc.push_undo_snapshot(kind, boundary);
    }

    pub fn move_to(&mut self, offset: usize) {
        self.doc.move_to(offset);
        self.marked_range = None;
    }

    pub fn select_to(&mut self, offset: usize) {
        self.doc.select_to(offset);
        self.marked_range = None;
    }

    pub fn replace_char_range(&mut self, range: Range<usize>, new_text: &str) -> usize {
        let new_cursor = self.doc.replace_char_range(range, new_text);
        self.marked_range = None;
        new_cursor
    }

    pub fn edit(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        new_text: &str,
    ) -> usize {
        let new_cursor = self.doc.edit(kind, boundary, range, new_text);
        self.marked_range = None;
        new_cursor
    }

    pub fn set_text(&mut self, text: &str) {
        self.doc.set_text(text);
        self.marked_range = None;
    }

    pub fn undo(&mut self) -> bool {
        let changed = self.doc.undo();
        if changed {
            self.marked_range = None;
        }
        changed
    }

    pub fn redo(&mut self) -> bool {
        let changed = self.doc.redo();
        if changed {
            self.marked_range = None;
        }
        changed
    }
}
