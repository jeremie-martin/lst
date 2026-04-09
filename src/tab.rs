use crate::viewport;
use iced::widget::text_editor;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

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

struct CachedLines {
    revision: u64,
    lines: Arc<[String]>,
}

pub struct LayoutCache {
    pub wrap_cols: usize,
    pub line_start_visual_row: Vec<usize>,
    pub total_visual_rows: usize,
}

pub struct Tab {
    pub path: Option<PathBuf>,
    pub content: text_editor::Content,
    pub modified: bool,
    pub is_scratchpad: bool,
    revision: u64,
    line_cache: Option<CachedLines>,
    layout_cache: Option<LayoutCache>,
    undo_stack: VecDeque<Snapshot>,
    redo_stack: Vec<Snapshot>,
    last_edit_kind: Option<EditKind>,
}

impl Tab {
    pub fn new_scratchpad(path: PathBuf) -> Self {
        let content = text_editor::Content::new();

        Self {
            path: Some(path),
            content,
            modified: false,
            is_scratchpad: true,
            revision: 0,
            line_cache: None,
            layout_cache: None,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            last_edit_kind: None,
        }
    }

    pub fn from_path(path: PathBuf, body: &str) -> Self {
        let content = text_editor::Content::with_text(body);

        Self {
            path: Some(path),
            content,
            modified: false,
            is_scratchpad: false,
            revision: 0,
            line_cache: None,
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
        self.line_cache = None;
        self.layout_cache = None;
    }

    pub fn lines(&mut self) -> Arc<[String]> {
        if let Some(cache) = &self.line_cache {
            if cache.revision == self.revision {
                return Arc::clone(&cache.lines);
            }
        }

        let lines: Arc<[String]> = self
            .content
            .text()
            .split('\n')
            .map(String::from)
            .collect::<Vec<_>>()
            .into();

        self.line_cache = Some(CachedLines {
            revision: self.revision,
            lines: Arc::clone(&lines),
        });

        lines
    }

    pub fn visible_unwrapped_gutter_text(&self, start_row: usize, end_row: usize) -> String {
        let line_count = self.content.line_count().max(1);
        if start_row >= end_row || start_row >= line_count {
            return String::new();
        }

        let end_row = end_row.min(line_count);
        let width = viewport::line_number_digits_width(line_count);
        let mut gutter_text = String::with_capacity((end_row - start_row) * (width + 2));

        for line_no in (start_row + 1)..=end_row {
            let _ = write!(gutter_text, "{line_no:>width$} ", width = width);
            if line_no < end_row {
                gutter_text.push('\n');
            }
        }

        gutter_text
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
        let mut line_start_visual_row = Vec::with_capacity(line_count + 1);
        let mut total_visual_rows = 0usize;

        for line_idx in 0..line_count {
            line_start_visual_row.push(total_visual_rows);

            let visual_rows = content.line(line_idx).map_or(1, |line| {
                viewport::visual_line_count(line.text.as_ref(), wrap_cols)
            });

            total_visual_rows += visual_rows;
        }

        line_start_visual_row.push(total_visual_rows);

        Self {
            wrap_cols,
            line_start_visual_row,
            total_visual_rows: total_visual_rows.max(1),
        }
    }

    pub fn visible_gutter_text(
        &self,
        line_count: usize,
        start_row: usize,
        end_row: usize,
    ) -> String {
        if start_row >= end_row || start_row >= self.total_visual_rows || line_count == 0 {
            return String::new();
        }

        let end_row = end_row.min(self.total_visual_rows);
        let width = viewport::line_number_digits_width(line_count);
        let start_line = self
            .line_start_visual_row
            .partition_point(|&row| row <= start_row)
            .saturating_sub(1)
            .min(line_count.saturating_sub(1));
        let mut gutter_text = String::with_capacity((end_row - start_row) * (width + 2));
        let mut first_row = true;

        for line_idx in start_line..line_count {
            let line_start = self.line_start_visual_row[line_idx];
            if line_start >= end_row {
                break;
            }

            let line_end = self.line_start_visual_row[line_idx + 1];
            let visible_start = start_row.max(line_start);
            let visible_end = end_row.min(line_end);

            for row in visible_start..visible_end {
                if !first_row {
                    gutter_text.push('\n');
                }
                first_row = false;

                if row == line_start {
                    let _ = write!(gutter_text, "{:>width$} ", line_idx + 1, width = width);
                }
            }
        }

        gutter_text
    }
}

#[cfg(all(test, feature = "internal-invariants"))]
mod tests {
    use super::*;

    #[cfg(feature = "internal-invariants")]
    #[test]
    fn layout_cache_tracks_prefix_rows_and_visible_wrapped_gutter() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "abcd\nef");

        let cache = tab.ensure_layout_cache(2);

        assert_eq!(cache.line_start_visual_row, vec![0, 2, 3]);
        assert_eq!(cache.total_visual_rows, 3);
        assert_eq!(cache.visible_gutter_text(2, 0, 3), "   1 \n\n   2 ");
    }

    #[cfg(feature = "internal-invariants")]
    #[test]
    fn touching_content_invalidates_cached_layout_revision() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "one");
        let initial_revision = tab.revision();

        tab.ensure_layout_cache(8);
        tab.touch_content();

        assert!(tab.layout_cache_for(8).is_none());
        assert_ne!(tab.revision(), initial_revision);
    }

    #[cfg(feature = "internal-invariants")]
    #[test]
    fn line_cache_reuses_revision_and_invalidates_after_touch() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "one\ntwo");

        let first = tab.lines();
        let second = tab.lines();

        assert!(Arc::ptr_eq(&first, &second));

        tab.content = text_editor::Content::with_text("one\ntwo\nthree");
        tab.touch_content();

        let third = tab.lines();

        assert!(!Arc::ptr_eq(&first, &third));
        assert_eq!(third.as_ref(), ["one", "two", "three"]);
    }

    #[test]
    fn visible_unwrapped_gutter_text_uses_current_line_count() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "one");

        assert_eq!(tab.visible_unwrapped_gutter_text(0, 1), "   1 ");

        tab.content = text_editor::Content::with_text("one\ntwo");
        tab.touch_content();

        assert_eq!(tab.visible_unwrapped_gutter_text(0, 2), "   1 \n   2 ");
    }

    #[test]
    fn wrapped_visible_gutter_text_only_builds_requested_rows() {
        let mut tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "abcd\nef");

        let cache = tab.ensure_layout_cache(2);

        assert_eq!(cache.visible_gutter_text(2, 1, 3), "\n   2 ");
    }

    #[test]
    fn unwrapped_visible_gutter_text_only_builds_requested_rows() {
        let tab = Tab::from_path(PathBuf::from("/tmp/test.txt"), "one\ntwo\nthree");

        assert_eq!(tab.visible_unwrapped_gutter_text(1, 3), "   2 \n   3 ");
    }
}
