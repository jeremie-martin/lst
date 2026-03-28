use iced::widget::text_editor;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::PathBuf;

const MAX_UNDO: usize = 100;

#[derive(Clone, Copy, PartialEq)]
pub enum EditKind {
    Insert,
    Delete,
    Other,
}

struct Snapshot {
    text: String,
    cursor: text_editor::Position,
}

pub struct Tab {
    pub path: Option<PathBuf>,
    pub content: text_editor::Content,
    pub modified: bool,
    undo_stack: VecDeque<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit_kind: Option<EditKind>,
}

impl Tab {
    pub fn new() -> Self {
        Self {
            path: None,
            content: text_editor::Content::new(),
            modified: false,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            last_edit_kind: None,
        }
    }

    pub fn from_path(path: PathBuf, body: &str) -> Self {
        Self {
            path: Some(path),
            content: text_editor::Content::with_text(body),
            modified: false,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            last_edit_kind: None,
        }
    }

    pub fn display_name(&self) -> Cow<'_, str> {
        match &self.path {
            Some(p) => p.file_name().unwrap_or_default().to_string_lossy(),
            None => Cow::Borrowed("untitled"),
        }
    }

    fn current_snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.content.text(),
            cursor: self.content.cursor().position,
        }
    }

    fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.content = text_editor::Content::with_text(&snapshot.text);
        self.content.move_to(text_editor::Cursor {
            position: snapshot.cursor,
            selection: None,
        });
        self.last_edit_kind = None;
    }

    /// Push an undo snapshot if this edit starts a new logical group.
    /// Groups consecutive same-kind edits; breaks on kind change, whitespace, or non-character edits.
    pub fn push_undo_snapshot(&mut self, kind: EditKind, boundary: bool) {
        let kind_changed = self.last_edit_kind != Some(kind);
        let is_streaming = matches!(kind, EditKind::Insert | EditKind::Delete);
        let should_snapshot = kind_changed || !is_streaming || boundary;

        if should_snapshot {
            self.undo_stack.push_back(self.current_snapshot());
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.pop_front();
            }
            self.redo_stack.clear();
        }
        self.last_edit_kind = Some(kind);
    }

    pub fn undo(&mut self) {
        if let Some(snapshot) = self.undo_stack.pop_back() {
            self.redo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
        }
    }

    pub fn redo(&mut self) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push_back(self.current_snapshot());
            self.restore_snapshot(snapshot);
        }
    }
}
