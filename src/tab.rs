use crate::viewport;
use iced::widget::text_editor;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::Write as _;
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

pub struct LayoutCache {
    pub wrap_cols: usize,
    pub line_start_visual_row: Vec<usize>,
    pub total_visual_rows: usize,
    pub gutter_text: String,
}

pub struct Tab {
    pub path: Option<PathBuf>,
    pub content: text_editor::Content,
    pub modified: bool,
    pub is_scratchpad: bool,
    revision: u64,
    layout_cache: Option<LayoutCache>,
    undo_stack: VecDeque<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit_kind: Option<EditKind>,
}

impl Tab {
    pub fn new_scratchpad(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            content: text_editor::Content::new(),
            modified: false,
            is_scratchpad: true,
            revision: 0,
            layout_cache: None,
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
            is_scratchpad: false,
            revision: 0,
            layout_cache: None,
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
        self.touch_content();
        self.last_edit_kind = None;
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn touch_content(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.layout_cache = None;
    }

    pub fn layout_cache_for(&self, wrap_cols: usize) -> Option<&LayoutCache> {
        self.layout_cache
            .as_ref()
            .filter(|cache| cache.wrap_cols == wrap_cols)
    }

    pub fn ensure_layout_cache(&mut self, wrap_cols: usize) -> &LayoutCache {
        let needs_rebuild = self.layout_cache_for(wrap_cols).is_none();

        if needs_rebuild {
            self.layout_cache = Some(LayoutCache::build(&self.content, wrap_cols));
        }

        self.layout_cache
            .as_ref()
            .expect("layout cache should exist after rebuild")
    }

    pub fn build_layout_cache(&self, wrap_cols: usize) -> LayoutCache {
        LayoutCache::build(&self.content, wrap_cols)
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

    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop_back() {
            self.redo_stack.push(self.current_snapshot());
            self.restore_snapshot(snapshot);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push_back(self.current_snapshot());
            self.restore_snapshot(snapshot);
            true
        } else {
            false
        }
    }
}

impl LayoutCache {
    fn build(content: &text_editor::Content, wrap_cols: usize) -> Self {
        let line_count = content.line_count().max(1);
        let line_number_digits = viewport::line_number_digits_width(line_count);
        let continuation_prefix = viewport::continuation_prefix(line_count);
        let mut line_start_visual_row = Vec::with_capacity(line_count + 1);
        let mut total_visual_rows = 0usize;
        let mut gutter_text = String::with_capacity(line_count * (line_number_digits + 2));

        for line_idx in 0..line_count {
            line_start_visual_row.push(total_visual_rows);

            let visual_rows = content.line(line_idx).map_or(1, |line| {
                viewport::visual_line_count(line.text.as_ref(), wrap_cols)
            });

            total_visual_rows += visual_rows;

            let _ = write!(
                gutter_text,
                "{:>width$} ",
                line_idx + 1,
                width = line_number_digits
            );

            for _ in 1..visual_rows {
                gutter_text.push('\n');
                gutter_text.push_str(&continuation_prefix);
            }

            if line_idx + 1 < line_count {
                gutter_text.push('\n');
            }
        }

        line_start_visual_row.push(total_visual_rows);

        Self {
            wrap_cols,
            line_start_visual_row,
            total_visual_rows: total_visual_rows.max(1),
            gutter_text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_cache_tracks_prefix_rows_and_gutter_for_wrapped_lines() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "abcd\nef");

        let cache = tab.ensure_layout_cache(2);

        assert_eq!(cache.line_start_visual_row, vec![0, 2, 3]);
        assert_eq!(cache.total_visual_rows, 3);
        assert_eq!(cache.gutter_text, "   1 \n     \n   2 ");
    }

    #[test]
    fn touching_content_invalidates_cached_layout_revision() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "one");
        let initial_revision = tab.revision();

        tab.ensure_layout_cache(8);
        tab.touch_content();

        assert!(tab.layout_cache_for(8).is_none());
        assert_ne!(tab.revision(), initial_revision);
    }
}
