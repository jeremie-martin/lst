mod document;
mod editor_ops;
mod effect;
pub mod find;
pub mod language;
mod model_io;
pub mod position;
pub mod selection;
mod snapshot;
mod tab;
mod tab_set;
pub mod viewport;
pub mod vim;
pub mod wrap;

pub use document::{EditKind, UndoBoundary};
pub use effect::{EditorEffect, FocusTarget, RevealIntent};
pub use language::{IndentStyle, Language, LanguageConfig};
pub use snapshot::EditorSnapshot;
pub use tab::{EditorTab, FileStamp, TabId};
pub use viewport::Viewport;

use crate::{
    document::{char_to_position, line_indent_prefix, position_to_char},
    find::{FindState, MatchPos},
    position::Position,
    selection::{
        is_identifier_char, line_range_at_char, next_grapheme_boundary, next_subword_boundary,
        next_word_boundary, previous_grapheme_boundary, previous_subword_boundary,
        previous_word_boundary,
    },
    tab_set::TabSet,
};
use std::{ops::Range, path::PathBuf, sync::Arc};

pub const UNTITLED_PREFIX: &str = "untitled";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCloseRequest {
    Close { tab_id: TabId },
    SaveAndClose { tab_id: TabId },
}

pub struct EditorModel {
    tabs: TabSet,
    next_untitled_id: usize,
    show_gutter: bool,
    show_wrap: bool,
    find: FindState,
    goto_line: Option<String>,
    status: String,
    vim: vim::VimState,
    viewport: Viewport,
    effects: Vec<EditorEffect>,
}

impl EditorModel {
    pub fn from_tab(tab: EditorTab, status: String) -> Self {
        Self::from_tabs(tab, Vec::new(), status)
    }

    pub fn from_tabs(first: EditorTab, rest: Vec<EditorTab>, status: String) -> Self {
        Self {
            tabs: TabSet::new(first, rest),
            next_untitled_id: 2,
            show_gutter: true,
            show_wrap: true,
            find: FindState::new(),
            goto_line: None,
            status,
            vim: vim::VimState::new(),
            viewport: Viewport::default(),
            effects: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        let tab = EditorTab::empty(TabId::from_raw(1), format!("{UNTITLED_PREFIX}-1"));
        Self::from_tab(tab, "Ready.".to_string())
    }

    fn alloc_tab_id(&mut self) -> TabId {
        self.tabs.alloc_tab_id()
    }

    pub fn active_tab(&self) -> &EditorTab {
        self.tabs.active()
    }

    fn active_tab_mut(&mut self) -> &mut EditorTab {
        self.tabs.active_mut()
    }

    pub fn active_tab_id(&self) -> TabId {
        self.active_tab().id()
    }

    pub fn active_tab_lines(&mut self) -> Arc<[String]> {
        self.active_tab_mut().lines()
    }

    pub fn tabs(&self) -> &[EditorTab] {
        &self.tabs
    }

    pub fn tab(&self, index: usize) -> Option<&EditorTab> {
        self.tabs.get(index)
    }

    pub fn tab_by_id(&self, tab_id: TabId) -> Option<&EditorTab> {
        self.tabs.tab_by_id(tab_id)
    }

    fn tab_mut_by_id(&mut self, tab_id: TabId) -> Option<&mut EditorTab> {
        self.tabs.tab_mut_by_id(tab_id)
    }

    pub fn set_tab_language(&mut self, tab_id: TabId, language: Option<Language>) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            tab.set_language(language);
        }
    }

    fn tab_index_by_id(&self, tab_id: TabId) -> Option<usize> {
        self.tabs.index_by_id(tab_id)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_index(&self) -> usize {
        self.tabs.active_index()
    }

    pub fn show_gutter(&self) -> bool {
        self.show_gutter
    }

    pub fn show_wrap(&self) -> bool {
        self.show_wrap
    }

    pub fn find(&self) -> &FindState {
        &self.find
    }

    pub fn find_match_ranges(&self) -> Vec<Range<usize>> {
        self.find
            .matches
            .iter()
            .copied()
            .map(|m| self.find_match_char_range(m))
            .collect()
    }

    pub fn active_find_match_range(&self) -> Option<Range<usize>> {
        let active = self.find.active?;
        self.find
            .matches
            .get(active)
            .copied()
            .map(|m| self.find_match_char_range(m))
    }

    pub fn goto_line(&self) -> Option<&str> {
        self.goto_line.as_deref()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn vim_mode(&self) -> vim::Mode {
        self.vim.mode
    }

    pub fn vim_pending_display(&self) -> String {
        self.vim.pending_display()
    }

    fn new_empty_tab(&mut self) -> EditorTab {
        let name = format!("{UNTITLED_PREFIX}-{}", self.next_untitled_id);
        self.next_untitled_id += 1;
        let id = self.alloc_tab_id();
        EditorTab::empty(id, name)
    }

    fn push_tab(&mut self, tab: EditorTab) {
        self.tabs.push(tab);
    }

    fn activate_tab(&mut self, index: usize) -> bool {
        if !self.tabs.activate(index) {
            return false;
        }
        self.vim.on_tab_switch();
        self.active_tab_mut().preferred_column = None;
        self.sync_find_with_active_document();
        self.status = format!("Switched to {}.", self.active_tab().display_name());
        true
    }

    pub fn move_to_char(&mut self, offset: usize, select: bool, preferred_column: Option<usize>) {
        let end = self.active_tab().len_chars();
        let target = offset.min(end);
        let cursor = self.active_tab().cursor_char();
        {
            let tab = self.active_tab_mut();
            tab.preferred_column = preferred_column;
            if select {
                tab.select_to(target);
            } else {
                tab.move_to(target);
            }
            tab.marked_range = None;
        }
        if target != cursor || select {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    fn assign_selection(&mut self, range: Range<usize>, reversed: bool) {
        self.active_tab_mut().set_selection_range(range, reversed);
    }

    fn queue_focus(&mut self, target: FocusTarget) {
        self.effects.push(EditorEffect::Focus(target));
    }

    fn queue_effect(&mut self, effect: EditorEffect) {
        self.effects.push(effect);
    }

    fn queue_reveal(&mut self, intent: RevealIntent) {
        self.queue_effect(EditorEffect::Reveal(intent));
    }

    pub fn drain_effects(&mut self) -> Vec<EditorEffect> {
        self.effects.drain(..).collect()
    }

    pub fn open_find_panel(&mut self, show_replace: bool) {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(text) = self.active_tab().selected_text() {
            if !text.contains('\n') {
                self.find.query = text;
                self.reindex_find_matches_to_nearest();
            }
        }
        self.queue_focus(FocusTarget::FindQuery);
    }

    pub fn close_find_panel(&mut self) {
        self.find.visible = false;
        self.find.show_replace = false;
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn open_goto_line_panel(&mut self) {
        self.goto_line = Some(String::new());
        self.queue_focus(FocusTarget::GotoLine);
    }

    pub fn close_goto_line_panel(&mut self) {
        self.goto_line = None;
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn update_find_query(&mut self, text: String) {
        self.find.query = text;
        self.reindex_find_matches_to_nearest();
    }

    pub fn update_find_query_and_activate(&mut self, text: String) {
        self.update_find_query(text);
        if self.move_to_current_find_match() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn update_find_replacement(&mut self, text: String) {
        self.find.replacement = text;
    }

    pub fn update_goto_line(&mut self, text: String) {
        self.goto_line = Some(text);
    }

    fn active_cursor_position(&self) -> Position {
        self.active_tab().cursor_position()
    }

    fn active_tab_revision(&self) -> u64 {
        self.active_tab().revision()
    }

    fn reindex_find_matches(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
            return;
        }
        let text = self.active_tab().buffer_text();
        self.find.compute_matches_in_text(&text);
        self.find.finish_reindex(self.active_tab().revision());
    }

    fn selected_find_match_start(&self) -> Option<Position> {
        if self.find.query.is_empty() {
            return None;
        }
        let tab = self.active_tab();
        if !tab.has_selection() {
            return None;
        }
        let selected = tab.selected_range();
        if selected.end.saturating_sub(selected.start) != self.find.query.chars().count() {
            return None;
        }
        Some(char_to_position(tab.buffer(), selected.start))
    }

    fn align_find_current_to_visible_match(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }
        if let Some(start) = self.selected_find_match_start() {
            if self.find.select_exact(&start) {
                return;
            }
        }
        let pos = self.active_cursor_position();
        self.find.find_nearest(&pos);
    }

    fn reindex_find_matches_to_nearest(&mut self) {
        self.reindex_find_matches();
        if !self.find.matches.is_empty() {
            self.align_find_current_to_visible_match();
        }
    }

    fn ensure_find_matches_current(&mut self) {
        if self.find.is_stale(self.active_tab_revision()) {
            self.reindex_find_matches();
        }
    }

    fn sync_find_with_active_document(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
        } else {
            self.reindex_find_matches_to_nearest();
        }
    }

    fn sync_find_after_edit(&mut self) {
        if !self.find.query.is_empty() {
            self.reindex_find_matches_to_nearest();
        }
    }

    fn find_next(&mut self) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.next();
        self.move_to_current_find_match()
    }

    fn find_prev(&mut self) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.prev();
        self.move_to_current_find_match()
    }

    fn replace_one(&mut self) -> bool {
        self.ensure_find_matches_current();
        let Some((start, end)) = self.find.current_match_range() else {
            return false;
        };
        let replacement = self.find.replacement.clone();
        let range = {
            let tab = self.active_tab();
            position_to_char(tab.buffer(), start)..position_to_char(tab.buffer(), end)
        };
        self.active_tab_mut()
            .edit(EditKind::Other, UndoBoundary::Break, range, &replacement);
        self.sync_find_after_edit();
        self.move_to_current_find_match();
        true
    }

    fn replace_all_matches(&mut self) -> bool {
        self.reindex_find_matches();
        if self.find.query.is_empty() {
            return false;
        }

        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        let text = self.active_tab().buffer_text();
        let new_text = text.replace(&query, &replacement);
        if new_text == text {
            return false;
        }

        let cursor = self.active_cursor_position();
        let range = 0..self.active_tab().len_chars();
        {
            let tab = self.active_tab_mut();
            tab.push_undo_snapshot(EditKind::Other, UndoBoundary::Break);
            tab.replace_char_range(range, &new_text);
            tab.set_cursor_position(cursor, None);
        }
        self.sync_find_after_edit();
        true
    }

    pub fn submit_goto_line_input(&mut self) {
        let Some(text) = self.goto_line.clone() else {
            return;
        };
        let trimmed = text.trim();
        let (line_text, column_text) = match trimmed.split_once(':') {
            Some((line, column)) => (line.trim(), Some(column.trim()).filter(|s| !s.is_empty())),
            None => (trimmed, None),
        };
        let Ok(line_one_based) = line_text.parse::<usize>() else {
            self.close_goto_line_panel();
            return;
        };
        let target_line = line_one_based
            .saturating_sub(1)
            .min(self.active_tab().line_count().saturating_sub(1));
        let target_column = match column_text {
            Some(column_text) => {
                let Ok(column_one_based) = column_text.parse::<usize>() else {
                    self.close_goto_line_panel();
                    return;
                };
                column_one_based
                    .saturating_sub(1)
                    .min(display_line_char_len(self.active_tab(), target_line))
            }
            None => 0,
        };
        self.active_tab_mut().set_cursor_position(
            Position {
                line: target_line,
                column: target_column,
            },
            None,
        );
        self.close_goto_line_panel();
        self.queue_reveal(RevealIntent::Center);
    }

    fn close_tab_at_unchecked(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.tabs.len() == 1 {
            let tab = self.new_empty_tab();
            self.tabs.replace_only(tab);
            self.activate_tab(0);
            self.queue_focus(FocusTarget::Editor);
            self.status = "Closed tab.".to_string();
            return true;
        }

        let active = self.active_index();
        let should_refocus = should_refocus_editor_after_tab_close(active, index);
        let next_active = next_active_after_tab_close(self.tabs.len(), active, index);
        self.tabs.remove(index);
        self.activate_tab(next_active);
        if should_refocus {
            self.queue_focus(FocusTarget::Editor);
        }
        self.status = "Closed tab.".to_string();
        true
    }

    fn move_to_current_find_match(&mut self) -> bool {
        let Some((start, _end)) = self.find.current_match_range() else {
            return false;
        };
        self.active_tab_mut().set_cursor_position(start, None);
        true
    }

    fn find_match_char_range(&self, m: MatchPos) -> Range<usize> {
        let query_len = self.find.query.chars().count();
        let tab = self.active_tab();
        let start = position_to_char(
            tab.buffer(),
            Position {
                line: m.line,
                column: m.col,
            },
        );
        let end = position_to_char(
            tab.buffer(),
            Position {
                line: m.line,
                column: m.col + query_len,
            },
        );
        start..end
    }

    fn edit_active(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        text: &str,
    ) {
        self.active_tab_mut().edit(kind, boundary, range, text);
        self.sync_find_after_edit();
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn replace_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    ) {
        let range = self.resolve_text_input_range(range);
        let kind = if text.is_empty() {
            EditKind::Delete
        } else {
            EditKind::Insert
        };
        self.edit_active(kind, boundary, range, &text);
    }

    pub fn replace_and_mark_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    ) {
        let range = self.resolve_text_input_range(range);
        let inserted_start = range.start;
        self.active_tab_mut()
            .edit(EditKind::Other, UndoBoundary::Break, range, &text);
        {
            let tab = self.active_tab_mut();
            let marked_range = if text.is_empty() {
                None
            } else {
                Some(inserted_start..inserted_start + text.chars().count())
            };
            let selection = selected_range
                .map(|range| inserted_start + range.start..inserted_start + range.end)
                .unwrap_or_else(|| {
                    let cursor = inserted_start + text.chars().count();
                    cursor..cursor
                });
            tab.set_selection_range(selection, false);
            tab.marked_range = marked_range;
        }
        self.sync_find_after_edit();
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    fn delete_selection_or_word_range(tab: &EditorTab, backward: bool) -> Option<Range<usize>> {
        if tab.has_selection() {
            return Some(tab.selected_range());
        }
        let cursor = tab.cursor_char();
        let target = if backward {
            previous_word_boundary(tab.buffer(), cursor)
        } else {
            next_word_boundary(tab.buffer(), cursor)
        };
        (target != cursor).then_some(target.min(cursor)..target.max(cursor))
    }

    fn move_horizontal(&mut self, delta: isize, select: bool) -> bool {
        let tab = self.active_tab_mut();
        let cursor = tab.cursor_char();
        let mut target = cursor;
        let steps = delta.unsigned_abs();
        if delta.is_negative() {
            for _ in 0..steps {
                let next = previous_grapheme_boundary(tab.buffer(), target);
                if next == target {
                    break;
                }
                target = next;
            }
        } else {
            for _ in 0..steps {
                let next = next_grapheme_boundary(tab.buffer(), target);
                if next == target {
                    break;
                }
                target = next;
            }
        }
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        target != cursor || select
    }

    pub fn move_horizontal_collapsed(&mut self, backward: bool) {
        let selection = self.active_tab().selected_range();
        if selection.start != selection.end {
            let target = if backward {
                selection.start
            } else {
                selection.end
            };
            let tab = self.active_tab_mut();
            tab.preferred_column = None;
            tab.move_to(target);
            self.queue_reveal(RevealIntent::NearestEdge);
            return;
        }

        if self.move_horizontal(if backward { -1 } else { 1 }, false) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    fn move_boundary(
        &mut self,
        backward: bool,
        select: bool,
        prev_fn: fn(&ropey::Rope, usize) -> usize,
        next_fn: fn(&ropey::Rope, usize) -> usize,
    ) -> bool {
        let cursor = self.active_tab().cursor_char();
        let target = {
            let tab = self.active_tab();
            if !select && tab.has_selection() {
                if backward {
                    tab.selected_range().start
                } else {
                    tab.selected_range().end
                }
            } else if backward {
                prev_fn(tab.buffer(), cursor)
            } else {
                next_fn(tab.buffer(), cursor)
            }
        };

        let tab = self.active_tab_mut();
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        target != cursor || select
    }

    fn apply_vertical_motion_target(
        &mut self,
        target: usize,
        preferred_column: usize,
        select: bool,
    ) -> bool {
        let cursor = self.active_tab().cursor_char();
        let tab = self.active_tab_mut();
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        tab.preferred_column = Some(preferred_column);
        target != cursor
    }

    fn vertical_boundary_target(tab: &EditorTab, delta: isize) -> Option<usize> {
        if delta < 0 {
            Some(tab.buffer().line_to_char(0))
        } else if delta > 0 {
            let last_line = tab.line_count().saturating_sub(1);
            Some(tab.buffer().line_to_char(last_line) + display_line_char_len(tab, last_line))
        } else {
            None
        }
    }

    fn move_vertical(&mut self, delta: isize, select: bool, snap_to_document_edges: bool) -> bool {
        let (target, preferred) = {
            let tab = self.active_tab();
            let position = tab.cursor_position();
            let preferred = tab.preferred_column.unwrap_or(position.column);
            let last_line = tab.line_count().saturating_sub(1);
            let at_edge =
                (delta < 0 && position.line == 0) || (delta > 0 && position.line == last_line);
            let boundary_target = (snap_to_document_edges && at_edge)
                .then(|| Self::vertical_boundary_target(tab, delta))
                .flatten();
            let target = if let Some(target) = boundary_target {
                target
            } else {
                let target_line = if delta.is_negative() {
                    position.line.saturating_sub(delta.unsigned_abs())
                } else {
                    (position.line + delta as usize).min(last_line)
                };
                let target_column = preferred.min(display_line_char_len(tab, target_line));
                tab.buffer().line_to_char(target_line) + target_column
            };
            (target, preferred)
        };

        self.apply_vertical_motion_target(target, preferred, select)
    }

    fn move_display_rows(
        &mut self,
        delta: isize,
        select: bool,
        wrap_columns: usize,
        snap_to_document_edges: bool,
    ) -> bool {
        if !self.show_wrap {
            return self.move_vertical(delta, select, snap_to_document_edges);
        }

        let (target, preferred) = {
            let tab = self.active_tab_mut();
            let lines = tab.lines();
            let position = tab.cursor_position();
            let layout = wrap::build_wrap_layout(lines.as_ref(), wrap_columns, true);
            let row_target = wrap::display_row_target(
                lines.as_ref(),
                position.line,
                position.column,
                tab.preferred_column,
                delta,
                &layout,
            );
            let preferred = row_target
                .map(|target| target.preferred_column)
                .or(tab.preferred_column)
                .unwrap_or_else(|| {
                    let current_visual_row = wrap::visual_row_for_position(
                        lines.as_ref(),
                        position.line,
                        position.column,
                        &layout,
                    )
                    .unwrap_or(layout.line_row_starts[position.line]);
                    let current_row_in_line =
                        current_visual_row.saturating_sub(layout.line_row_starts[position.line]);
                    let current_line = lines
                        .get(position.line)
                        .map(String::as_str)
                        .unwrap_or_default();
                    let segments = wrap::wrap_segments(current_line, layout.wrap_columns);
                    let current_segment = segments
                        .get(current_row_in_line)
                        .or_else(|| segments.last())
                        .expect("wrap_segments always returns at least one segment");
                    position.column.saturating_sub(current_segment.start_col)
                });
            let target = if let Some(rt) = row_target {
                Some(position_to_char(
                    tab.buffer(),
                    Position {
                        line: rt.line,
                        column: rt.column,
                    },
                ))
            } else if snap_to_document_edges {
                Self::vertical_boundary_target(tab, delta)
            } else {
                None
            };
            (target, preferred)
        };

        let Some(target) = target else {
            return false;
        };

        self.apply_vertical_motion_target(target, preferred, select)
    }

    fn move_to_visual_row(&mut self, target: usize, select: bool, wrap_columns: usize) -> bool {
        if !self.show_wrap {
            let current = self.active_tab().cursor_position().line;
            if target == current {
                return false;
            }
            return self.move_vertical(target as isize - current as isize, select, true);
        }

        // Build the wrap layout once and reuse it for both the current-row
        // lookup and the delta application; going through `move_display_rows`
        // would build it again.
        let cursor = self.active_tab().cursor_char();
        let (target_char, preferred_column) = {
            let tab = self.active_tab_mut();
            let lines = tab.lines();
            let position = tab.cursor_position();
            let layout = wrap::build_wrap_layout(lines.as_ref(), wrap_columns, true);
            let current = wrap::visual_row_for_position(
                lines.as_ref(),
                position.line,
                position.column,
                &layout,
            )
            .unwrap_or(position.line);
            if target == current {
                return false;
            }
            let Some(row_target) = wrap::display_row_target(
                lines.as_ref(),
                position.line,
                position.column,
                tab.preferred_column,
                target as isize - current as isize,
                &layout,
            ) else {
                return false;
            };
            let target_char = position_to_char(
                tab.buffer(),
                Position {
                    line: row_target.line,
                    column: row_target.column,
                },
            );
            (target_char, row_target.preferred_column)
        };

        let tab = self.active_tab_mut();
        if select {
            tab.select_to(target_char);
        } else {
            tab.move_to(target_char);
        }
        tab.preferred_column = Some(preferred_column);
        target_char != cursor || select
    }

    fn move_paged(
        &mut self,
        delta: isize,
        select: bool,
        wrap_columns: usize,
        snap_to_document_edges: bool,
    ) {
        if self.move_display_rows(delta, select, wrap_columns, snap_to_document_edges) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_line_boundary(&mut self, to_end: bool, select: bool) {
        let tab = self.active_tab_mut();
        let cursor = tab.cursor_char();
        let line = tab.buffer.char_to_line(cursor.min(tab.len_chars()));
        let target = if to_end {
            tab.buffer.line_to_char(line) + display_line_char_len(tab, line)
        } else {
            tab.buffer.line_to_char(line)
        };
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        if target != cursor {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn smart_home(&mut self, select: bool) {
        let tab = self.active_tab_mut();
        let cursor = tab.cursor_char();
        let line = tab.buffer.char_to_line(cursor.min(tab.len_chars()));
        let line_start = tab.buffer.line_to_char(line);
        let first_non_blank = line_start + first_non_blank_column(tab, line);
        let target = if cursor == first_non_blank {
            line_start
        } else {
            first_non_blank
        };
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        if target != cursor {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_document_boundary(&mut self, to_end: bool, select: bool) {
        let target = if to_end {
            self.active_tab().len_chars()
        } else {
            0
        };
        let cursor = self.active_tab().cursor_char();
        let tab = self.active_tab_mut();
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        if target != cursor {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    fn replace_active_lines(&mut self, lines: Vec<String>, cursor_line: usize, cursor_col: usize) {
        let newline = preferred_newline_for_active_tab(self.active_tab());
        {
            let tab = self.active_tab_mut();
            tab.set_text(&lines.join(newline));
            tab.mark_modified();
            let cursor = position_to_char(
                tab.buffer(),
                Position {
                    line: cursor_line,
                    column: cursor_col,
                },
            );
            tab.move_to(cursor);
        }
        self.sync_find_after_edit();
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    fn move_active_cursor(&mut self, cursor_line: usize, cursor_col: usize, select: bool) {
        let position = Position {
            line: cursor_line,
            column: cursor_col,
        };
        let anchor = if select {
            Some(self.active_cursor_position())
        } else {
            None
        };
        self.active_tab_mut().set_cursor_position(position, anchor);
    }

    fn apply_line_edit<R, F>(&mut self, edit: F) -> Option<R>
    where
        F: FnOnce(&mut Vec<String>) -> Option<(R, usize, usize)>,
    {
        let cached_lines = self.active_tab_mut().lines();
        let mut lines: Vec<String> = cached_lines.iter().cloned().collect();
        let (result, cursor_line, cursor_col) = edit(&mut lines)?;
        if lines.as_slice() == cached_lines.as_ref() {
            let cursor = self.active_cursor_position();
            if cursor.line == cursor_line && cursor.column == cursor_col {
                return None;
            }
            self.move_active_cursor(cursor_line, cursor_col, false);
            self.queue_reveal(RevealIntent::NearestEdge);
            return Some(result);
        }

        self.active_tab_mut()
            .push_undo_snapshot(EditKind::Other, UndoBoundary::Break);
        self.replace_active_lines(lines, cursor_line, cursor_col);
        Some(result)
    }

    fn delete_selected_or_previous(&mut self) -> bool {
        let range = {
            let tab = self.active_tab();
            if tab.has_selection() {
                tab.selected_range()
            } else {
                let cursor = tab.cursor_char();
                if cursor == 0 {
                    return false;
                }
                previous_grapheme_boundary(tab.buffer(), cursor)..cursor
            }
        };
        self.edit_active(EditKind::Delete, UndoBoundary::Merge, range, "");
        true
    }

    fn delete_selected_or_next(&mut self) -> bool {
        let range = {
            let tab = self.active_tab();
            if tab.has_selection() {
                tab.selected_range()
            } else {
                let cursor = tab.cursor_char();
                if cursor >= tab.len_chars() {
                    return false;
                }
                cursor..next_grapheme_boundary(tab.buffer(), cursor)
            }
        };
        self.edit_active(EditKind::Delete, UndoBoundary::Merge, range, "");
        true
    }

    fn delete_selected_or_word(&mut self, backward: bool) -> bool {
        let Some(range) = Self::delete_selection_or_word_range(self.active_tab(), backward) else {
            return false;
        };
        self.edit_active(EditKind::Delete, UndoBoundary::Break, range, "");
        true
    }

    fn insert_newline(&mut self) {
        let (newline, indent) = {
            let tab = self.active_tab();
            let line = tab
                .buffer()
                .char_to_line(tab.cursor_char().min(tab.len_chars()));
            (
                preferred_newline_for_active_tab(tab),
                line_indent_prefix(tab.buffer(), line),
            )
        };
        self.replace_text(None, format!("{newline}{indent}"), UndoBoundary::Break);
    }

    fn selection_or_current_line(&self) -> (Range<usize>, String, bool) {
        let tab = self.active_tab();
        let use_current_line = !tab.has_selection();
        let range = if use_current_line {
            linewise_range_at_char(tab.buffer(), tab.cursor_char())
        } else {
            tab.selected_range()
        };
        let text = tab.buffer().slice(range.clone()).to_string();
        (range, text, use_current_line)
    }

    pub fn copy_selection(&mut self) {
        let (_range, text, whole_line) = self.selection_or_current_line();
        if text.is_empty() {
            return;
        }
        self.queue_effect(EditorEffect::WriteClipboard(text.clone()));
        self.queue_effect(EditorEffect::WritePrimary(text));
        self.status = if whole_line {
            "Copied line.".to_string()
        } else {
            "Copied selection.".to_string()
        };
    }

    pub fn cut_selection(&mut self) {
        let (range, text, whole_line) = self.selection_or_current_line();
        if text.is_empty() {
            return;
        }
        self.queue_effect(EditorEffect::WriteClipboard(text.clone()));
        self.queue_effect(EditorEffect::WritePrimary(text));
        self.edit_active(EditKind::Delete, UndoBoundary::Break, range, "");
        self.status = if whole_line {
            "Cut line.".to_string()
        } else {
            "Cut selection.".to_string()
        };
    }

    fn vim_snapshot(&mut self) -> vim::TextSnapshot {
        let cursor = self.active_cursor_position();
        let lines = self.active_tab_mut().lines();
        vim::TextSnapshot { lines, cursor }
    }

    pub fn handle_vim_key(
        &mut self,
        key: vim::Key,
        mods: vim::Modifiers,
        wrap_columns: usize,
    ) -> bool {
        let snapshot = self.vim_snapshot();
        let commands = self.vim.handle_key(&key, mods, &snapshot);
        self.execute_vim_commands(commands, wrap_columns)
    }

    pub fn handle_vim_escape(&mut self) -> bool {
        let snapshot = self.vim_snapshot();
        let commands = self
            .vim
            .enter_normal_from_escape(snapshot.cursor, &snapshot);
        self.execute_vim_commands(commands, 0)
    }

    fn execute_vim_commands(
        &mut self,
        commands: Vec<vim::VimCommand>,
        wrap_columns: usize,
    ) -> bool {
        if commands.is_empty() {
            return false;
        }

        let mut changed = false;
        for cmd in commands {
            match cmd {
                vim::VimCommand::Noop => {}
                vim::VimCommand::MoveTo(position) => {
                    self.active_tab_mut().set_cursor_position(position, None);
                    changed = true;
                }
                vim::VimCommand::Select { anchor, head } => {
                    self.apply_vim_select(anchor, head);
                    changed = true;
                }
                vim::VimCommand::DeleteRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                    changed = true;
                }
                vim::VimCommand::DeleteLines { first, last } => {
                    let deleted = self.vim_delete_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                    changed = true;
                }
                vim::VimCommand::ChangeRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                    self.vim.mode = vim::Mode::Insert;
                    changed = true;
                }
                vim::VimCommand::ChangeLines { first, last } => {
                    let deleted = self.vim_change_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                    self.vim.mode = vim::Mode::Insert;
                    changed = true;
                }
                vim::VimCommand::YankRange { from, to } => {
                    self.vim.register = vim::Register::Char(self.vim_extract_range(from, to));
                    changed = true;
                }
                vim::VimCommand::YankLines { first, last } => {
                    self.vim.register = vim::Register::Line(self.vim_extract_lines(first, last));
                    changed = true;
                }
                vim::VimCommand::EnterInsert => {
                    self.vim.mode = vim::Mode::Insert;
                    changed = true;
                }
                vim::VimCommand::PasteAfter => {
                    self.vim_paste(false);
                    changed = true;
                }
                vim::VimCommand::PasteBefore => {
                    self.vim_paste(true);
                    changed = true;
                }
                vim::VimCommand::OpenLineBelow => {
                    self.vim_open_line(false);
                    self.vim.mode = vim::Mode::Insert;
                    changed = true;
                }
                vim::VimCommand::OpenLineAbove => {
                    self.vim_open_line(true);
                    self.vim.mode = vim::Mode::Insert;
                    changed = true;
                }
                vim::VimCommand::JoinLines { count } => {
                    self.vim_join_lines(count);
                    changed = true;
                }
                vim::VimCommand::ReplaceChar { ch, count } => {
                    self.vim_replace_char(ch, count);
                    changed = true;
                }
                vim::VimCommand::Undo => {
                    if self.active_tab_mut().undo() {
                        self.sync_find_after_edit();
                    }
                    changed = true;
                }
                vim::VimCommand::Redo => {
                    if self.active_tab_mut().redo() {
                        self.sync_find_after_edit();
                    }
                    changed = true;
                }
                vim::VimCommand::OpenFind => {
                    self.open_find_panel(false);
                    changed = true;
                }
                vim::VimCommand::FindNext => {
                    self.ensure_find_matches_current();
                    if let Some(target) =
                        self.vim_find_next_from_cursor(self.active_cursor_position())
                    {
                        self.move_to_vim_search_target(target);
                    }
                    changed = true;
                }
                vim::VimCommand::FindPrev => {
                    self.ensure_find_matches_current();
                    if let Some(target) =
                        self.vim_find_prev_from_cursor(self.active_cursor_position())
                    {
                        self.move_to_vim_search_target(target);
                    }
                    changed = true;
                }
                vim::VimCommand::SearchWordUnderCursor { word, forward } => {
                    self.find.query = word;
                    self.reindex_find_matches();
                    let cursor = self.active_cursor_position();
                    let target = if forward {
                        self.vim_find_next_from_cursor(cursor)
                    } else {
                        self.vim_find_prev_from_cursor(cursor)
                    };
                    if let Some(target) = target {
                        self.move_to_vim_search_target(target);
                    }
                    changed = true;
                }
                vim::VimCommand::TransformCaseRange {
                    from,
                    to,
                    uppercase,
                } => {
                    self.vim_transform_case_range(from, to, uppercase);
                    changed = true;
                }
                vim::VimCommand::TransformCaseLines {
                    first,
                    last,
                    uppercase,
                } => {
                    self.vim_transform_case_lines(first, last, uppercase);
                    changed = true;
                }
                vim::VimCommand::HalfPageDown => {
                    let delta = self.viewport.half_page() as isize;
                    self.move_paged(delta, self.vim_in_visual(), wrap_columns, false);
                    changed = true;
                }
                vim::VimCommand::HalfPageUp => {
                    let delta = -(self.viewport.half_page() as isize);
                    self.move_paged(delta, self.vim_in_visual(), wrap_columns, false);
                    changed = true;
                }
                vim::VimCommand::PageDown => {
                    let delta = self.viewport.page() as isize;
                    self.move_paged(delta, self.vim_in_visual(), wrap_columns, false);
                    changed = true;
                }
                vim::VimCommand::PageUp => {
                    let delta = -(self.viewport.page() as isize);
                    self.move_paged(delta, self.vim_in_visual(), wrap_columns, false);
                    changed = true;
                }
                vim::VimCommand::MoveToScreenTop => {
                    self.screen_top(self.vim_in_visual(), wrap_columns);
                    changed = true;
                }
                vim::VimCommand::MoveToScreenMiddle => {
                    self.screen_middle(self.vim_in_visual(), wrap_columns);
                    changed = true;
                }
                vim::VimCommand::MoveToScreenBottom => {
                    self.screen_bottom(self.vim_in_visual(), wrap_columns);
                    changed = true;
                }
                vim::VimCommand::ScrollCursor(intent) => {
                    self.queue_reveal(intent);
                }
            }
        }

        if changed {
            self.queue_reveal(RevealIntent::NearestEdge);
            self.queue_primary_selection();
        }
        true
    }

    fn queue_primary_selection(&mut self) {
        if let Some(text) = self.active_tab().selected_text() {
            self.queue_effect(EditorEffect::WritePrimary(text));
        }
    }

    fn vim_in_visual(&self) -> bool {
        matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine)
    }

    fn vim_find_next_from_cursor(&mut self, position: Position) -> Option<Position> {
        let index = self
            .find
            .matches
            .iter()
            .position(|m| {
                m.line > position.line || (m.line == position.line && m.col > position.column)
            })
            .or_else(|| (!self.find.matches.is_empty()).then_some(0))?;
        self.find.active = Some(index);
        let m = self.find.matches[index];
        Some(Position {
            line: m.line,
            column: m.col,
        })
    }

    fn vim_find_prev_from_cursor(&mut self, position: Position) -> Option<Position> {
        let index = self
            .find
            .matches
            .iter()
            .rposition(|m| {
                m.line < position.line || (m.line == position.line && m.col < position.column)
            })
            .or_else(|| self.find.matches.len().checked_sub(1))?;
        self.find.active = Some(index);
        let m = self.find.matches[index];
        Some(Position {
            line: m.line,
            column: m.col,
        })
    }

    fn apply_vim_select(&mut self, anchor: Position, head: Position) {
        let tab = self.active_tab_mut();
        let anchor_char = position_to_char(tab.buffer(), anchor);
        let head_char = position_to_char(tab.buffer(), head);
        let anchor_end = inclusive_position_to_exclusive_char(tab, anchor);
        let head_end = inclusive_position_to_exclusive_char(tab, head);
        if vim_position_lt(head, anchor) {
            tab.set_selection_range(head_char..anchor_end.max(head_char), true);
        } else {
            tab.set_selection_range(anchor_char..head_end.max(anchor_char), false);
        }
    }

    fn move_to_vim_search_target(&mut self, target: Position) {
        if matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine) {
            let snapshot = self.vim_snapshot();
            if let vim::VimCommand::Select { anchor, head } =
                self.vim.selection_command(target, &snapshot)
            {
                self.apply_vim_select(anchor, head);
            }
        } else {
            self.active_tab_mut().set_cursor_position(target, None);
        }
    }

    fn vim_delete_range(&mut self, from: Position, to: Position) -> String {
        self.apply_line_edit(|lines| {
            let deleted = extract_text_range(lines, &from, &to);
            remove_text_range(lines, &from, &to);
            let cursor_col = from.column.min(
                lines
                    .get(from.line)
                    .map_or(0, |line| line.chars().count().saturating_sub(1)),
            );
            Some((deleted, from.line, cursor_col))
        })
        .unwrap_or_default()
    }

    fn vim_delete_lines(&mut self, first: usize, last: usize) -> String {
        self.apply_line_edit(|lines| {
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            let deleted = lines[first..=last].join("\n");
            lines.drain(first..=last);
            if lines.is_empty() {
                lines.push(String::new());
            }
            let cursor_line = first.min(lines.len().saturating_sub(1));
            Some((deleted, cursor_line, 0))
        })
        .unwrap_or_default()
    }

    fn vim_change_lines(&mut self, first: usize, last: usize) -> String {
        self.apply_line_edit(|lines| {
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            let indent: String = lines[first]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            let deleted = lines[first..=last].join("\n");
            lines.drain(first..=last);
            lines.insert(first, indent.clone());
            Some((deleted, first, indent.chars().count()))
        })
        .unwrap_or_default()
    }

    fn vim_extract_range(&mut self, from: Position, to: Position) -> String {
        let lines = self.active_tab_mut().lines();
        extract_text_range(lines.as_ref(), &from, &to)
    }

    fn vim_extract_lines(&mut self, first: usize, last: usize) -> String {
        let lines = self.active_tab_mut().lines();
        let first = first.min(lines.len().saturating_sub(1));
        let last = last.min(lines.len().saturating_sub(1));
        lines[first..=last].join("\n")
    }

    fn vim_paste(&mut self, before: bool) {
        match self.vim.register.clone() {
            vim::Register::Empty => {}
            vim::Register::Char(paste_text) => {
                let cursor = self.active_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let line_chars: Vec<char> = lines[cursor.line].chars().collect();
                    let insert_col = if before {
                        cursor.column.min(line_chars.len())
                    } else {
                        (cursor.column + 1).min(line_chars.len())
                    };
                    let prefix: String = line_chars[..insert_col].iter().collect();
                    let suffix: String = line_chars[insert_col..].iter().collect();
                    let paste_lines: Vec<&str> = paste_text.split('\n').collect();
                    if paste_lines.len() == 1 {
                        lines[cursor.line] = format!("{prefix}{}{suffix}", paste_lines[0]);
                        let cursor_col =
                            insert_col + paste_lines[0].chars().count().saturating_sub(1);
                        return Some(((), cursor.line, cursor_col));
                    }

                    let first_new = format!("{prefix}{}", paste_lines[0]);
                    let last_new = format!("{}{suffix}", paste_lines.last().unwrap_or(&""));
                    let mut new_lines: Vec<String> = lines[..cursor.line].to_vec();
                    new_lines.push(first_new);
                    for paste_line in &paste_lines[1..paste_lines.len() - 1] {
                        new_lines.push((*paste_line).to_string());
                    }
                    new_lines.push(last_new);
                    new_lines.extend(lines[cursor.line + 1..].iter().cloned());
                    let cursor_line = cursor.line + paste_lines.len() - 1;
                    let cursor_col = paste_lines
                        .last()
                        .unwrap_or(&"")
                        .chars()
                        .count()
                        .saturating_sub(1);
                    *lines = new_lines;
                    Some(((), cursor_line, cursor_col))
                });
            }
            vim::Register::Line(paste_text) => {
                let cursor = self.active_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let insert_at = if before { cursor.line } else { cursor.line + 1 };
                    lines.splice(
                        insert_at..insert_at,
                        paste_text.split('\n').map(String::from),
                    );
                    let indent = lines.get(insert_at).map_or(0, |line| {
                        line.chars().take_while(|c| c.is_whitespace()).count()
                    });
                    Some(((), insert_at, indent))
                });
            }
        }
    }

    fn vim_open_line(&mut self, above: bool) {
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let indent: String = lines.get(pos.line).map_or(String::new(), |line| {
                line.chars().take_while(|c| c.is_whitespace()).collect()
            });
            let idx = if above { pos.line } else { pos.line + 1 };
            lines.insert(idx, indent.clone());
            Some(((), idx, indent.chars().count()))
        });
    }

    fn vim_join_lines(&mut self, count: usize) {
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            if pos.line + 1 >= lines.len() {
                return None;
            }

            let join_end = (pos.line + count).min(lines.len() - 1);
            let mut joined = lines[pos.line].trim_end().to_string();
            let join_col = joined.chars().count();
            for line in lines.drain((pos.line + 1)..=join_end) {
                let trimmed = line.trim_start();
                if !trimmed.is_empty() {
                    joined.push(' ');
                    joined.push_str(trimmed);
                }
            }
            lines[pos.line] = joined;
            Some(((), pos.line, join_col))
        });
    }

    fn vim_replace_char(&mut self, ch: char, count: usize) {
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let line = lines.get(pos.line)?;
            let cells = selection::cells_of_str(line);
            if cells.is_empty() {
                return None;
            }
            let start_cell = selection::cell_partition_by_char(&cells, pos.column);
            let end_cell = start_cell.checked_add(count)?;
            if end_cell > cells.len() {
                return None;
            }
            let start_byte = cells[start_cell].byte_start;
            let end_byte = cells
                .get(end_cell)
                .map_or(line.len(), |cell| cell.byte_start);
            let replacement: String = std::iter::repeat_n(ch, count).collect();
            let mut new_line = String::with_capacity(line.len());
            new_line.push_str(&line[..start_byte]);
            new_line.push_str(&replacement);
            new_line.push_str(&line[end_byte..]);
            lines[pos.line] = new_line;
            // Cursor lands at the start of the last replaced cluster, which —
            // since `ch` is a single scalar — is one char per replacement.
            let cursor_col = cells[start_cell].char_start + count - 1;
            Some(((), pos.line, cursor_col))
        });
    }

    fn vim_transform_case_range(&mut self, from: Position, to: Position, uppercase: bool) {
        let _ = self.apply_line_edit(|lines| {
            editor_ops::transform_case_range(
                lines,
                from.line,
                from.column,
                to.line,
                to.column,
                uppercase,
            );
            Some(((), from.line, from.column))
        });
    }

    fn vim_transform_case_lines(&mut self, first: usize, last: usize, uppercase: bool) {
        let _ = self.apply_line_edit(|lines| {
            if lines.is_empty() {
                return None;
            }
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            for line in &mut lines[first..=last] {
                *line = if uppercase {
                    line.to_uppercase()
                } else {
                    line.to_lowercase()
                };
            }
            Some(((), first, 0))
        });
    }

    pub fn insert_text(&mut self, text: String) {
        self.apply_text_input(None, text, UndoBoundary::Break);
    }

    pub fn replace_text_from_input(&mut self, range: Option<Range<usize>>, text: String) {
        let boundary = if text.chars().any(char::is_whitespace) {
            UndoBoundary::Break
        } else {
            UndoBoundary::Merge
        };
        self.apply_text_input(range, text, boundary);
    }

    pub fn clear_marked_text(&mut self) {
        self.active_tab_mut().marked_range = None;
    }

    fn apply_text_input(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    ) {
        let resolved_range = self.resolve_text_input_range(range);
        if let Some(new_cursor) = self.auto_pair_overtype_cursor(&resolved_range, &text) {
            self.assign_selection(new_cursor..new_cursor, false);
            self.queue_reveal(RevealIntent::NearestEdge);
            return;
        }
        if let Some(dedent_range) = self.auto_dedent_close_brace_range(&resolved_range, &text) {
            self.edit_active(EditKind::Insert, UndoBoundary::Break, dedent_range, &text);
            return;
        }
        if let Some((edit_range, replacement, new_selection)) =
            self.auto_pair_surround_edit(&resolved_range, &text)
        {
            // Keep the caret on whichever end the user anchored so reversed
            // selections stay reversed after the surround.
            let reversed = self.active_tab().selection_reversed();
            self.edit_active(
                EditKind::Insert,
                UndoBoundary::Break,
                edit_range,
                &replacement,
            );
            self.assign_selection(new_selection, reversed);
            self.align_find_current_to_visible_match();
            return;
        }
        if let Some((edit_range, replacement, caret)) =
            self.auto_pair_insert_edit(&resolved_range, &text)
        {
            self.edit_active(
                EditKind::Insert,
                UndoBoundary::Break,
                edit_range,
                &replacement,
            );
            self.assign_selection(caret..caret, false);
            self.align_find_current_to_visible_match();
            return;
        }
        let kind = if text.is_empty() {
            EditKind::Delete
        } else {
            EditKind::Insert
        };
        self.edit_active(kind, boundary, resolved_range, &text);
    }

    fn resolve_text_input_range(&self, range: Option<Range<usize>>) -> Range<usize> {
        let tab = self.active_tab();
        range
            .or_else(|| tab.marked_range.clone())
            .unwrap_or_else(|| tab.selected_range())
    }

    fn auto_dedent_close_brace_range(
        &self,
        range: &Range<usize>,
        text: &str,
    ) -> Option<Range<usize>> {
        let ch = single_char(text)?;
        let tab = self.active_tab();
        let config = tab.language_config();
        if !config.auto_dedent_closers.contains(&ch) {
            return None;
        }
        if config.indent.uses_tabs() {
            return None;
        }

        let buffer = tab.buffer();
        let line = buffer.char_to_line(range.start);
        if line != buffer.char_to_line(range.end) {
            return None;
        }

        let line_start = buffer.line_to_char(line);
        let line_end = line_start + display_line_char_len(tab, line);
        if !buffer
            .slice(line_start..line_end)
            .chars()
            .all(|ch| ch == ' ')
        {
            return None;
        }

        let width = config.indent.width();
        let dedent_start = range.start.saturating_sub(width).max(line_start);
        if dedent_start == range.start {
            return None;
        }
        Some(dedent_start..range.end)
    }

    fn auto_pair_overtype_cursor(&self, range: &Range<usize>, text: &str) -> Option<usize> {
        if range.start != range.end {
            return None;
        }
        let ch = single_char(text)?;
        let config = self.active_tab().language_config();
        let (_, closer) = auto_pair_pair_for(config, ch)?;
        if ch != closer {
            return None;
        }
        let tab = self.active_tab();
        let buffer = tab.buffer();
        if range.end >= buffer.len_chars() {
            return None;
        }
        if buffer.char(range.end) != closer {
            return None;
        }
        Some(range.end + 1)
    }

    fn auto_pair_surround_edit(
        &self,
        range: &Range<usize>,
        text: &str,
    ) -> Option<(Range<usize>, String, Range<usize>)> {
        if range.start >= range.end {
            return None;
        }
        let ch = single_char(text)?;
        let config = self.active_tab().language_config();
        let (opener, closer) = auto_pair_pair_for(config, ch)?;
        if ch != opener {
            return None;
        }
        let tab = self.active_tab();
        let selected = tab.buffer().slice(range.clone()).to_string();
        let mut replacement = String::with_capacity(selected.len() + 2);
        replacement.push(opener);
        replacement.push_str(&selected);
        replacement.push(closer);
        Some((
            range.clone(),
            replacement,
            (range.start + 1)..(range.end + 1),
        ))
    }

    fn auto_pair_insert_edit(
        &self,
        range: &Range<usize>,
        text: &str,
    ) -> Option<(Range<usize>, String, usize)> {
        if range.start != range.end {
            return None;
        }
        let ch = single_char(text)?;
        let config = self.active_tab().language_config();
        let (opener, closer) = auto_pair_pair_for(config, ch)?;
        if ch != opener {
            return None;
        }

        if is_auto_pair_quote(ch) {
            let buffer = self.active_tab().buffer();
            if range.start > 0 {
                let prev = buffer.char(range.start - 1);
                if prev == '\\' || prev == ch || is_identifier_char(prev) {
                    return None;
                }
            }
            if range.end < buffer.len_chars() {
                let next = buffer.char(range.end);
                if next == ch || is_identifier_char(next) {
                    return None;
                }
            }
        }

        let mut replacement = String::with_capacity(2);
        replacement.push(opener);
        replacement.push(closer);
        Some((range.clone(), replacement, range.start + 1))
    }

    pub fn toggle_wrap(&mut self) {
        self.show_wrap = !self.show_wrap;
        self.status = if self.show_wrap {
            "Soft wrap enabled.".to_string()
        } else {
            "Soft wrap disabled.".to_string()
        };
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn new_tab(&mut self) {
        let tab = self.new_empty_tab();
        self.push_tab(tab);
        let last = self.tabs.len().saturating_sub(1);
        self.activate_tab(last);
        self.status = "Created a new tab.".to_string();
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn new_scratchpad_tab(&mut self, path: PathBuf, file_stamp: FileStamp) {
        let id = self.alloc_tab_id();
        let tab = EditorTab::scratchpad_with_stamp(id, path, file_stamp);
        self.push_tab(tab);
        let last = self.tabs.len().saturating_sub(1);
        self.activate_tab(last);
        self.status = "Created a new scratchpad.".to_string();
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn close_request_for_tab(&self, index: usize) -> Option<TabCloseRequest> {
        let tab = self.tabs.get(index)?;
        if tab.modified() {
            Some(TabCloseRequest::SaveAndClose { tab_id: tab.id() })
        } else {
            Some(TabCloseRequest::Close { tab_id: tab.id() })
        }
    }

    pub fn first_dirty_tab_index(&self) -> Option<usize> {
        self.tabs.iter().position(EditorTab::modified)
    }

    pub fn close_clean_tab_by_id(&mut self, tab_id: TabId) -> bool {
        let Some(index) = self.tab_index_by_id(tab_id) else {
            return false;
        };
        if self.tabs[index].modified() {
            return false;
        }
        self.close_tab_at_unchecked(index)
    }

    pub fn discard_close_tab_by_id(&mut self, tab_id: TabId) -> bool {
        let Some(index) = self.tab_index_by_id(tab_id) else {
            return false;
        };
        self.close_tab_at_unchecked(index)
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if self.activate_tab(index) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.activate_tab((self.active_index() + 1) % self.tabs.len());
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            let active = self.active_index();
            let prev = if active == 0 {
                self.tabs.len() - 1
            } else {
                active - 1
            };
            self.activate_tab(prev);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn select_all(&mut self) {
        self.active_tab_mut().select_all();
        if let Some(text) = self.active_tab().selected_text() {
            self.queue_effect(EditorEffect::WritePrimary(text));
        }
    }

    pub fn move_horizontal_by(&mut self, delta: isize, select: bool) {
        if self.move_horizontal(delta, select) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_logical_rows(&mut self, delta: isize, select: bool) {
        if self.move_vertical(delta, select, true) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_display_rows_by(&mut self, delta: isize, select: bool, wrap_columns: usize) {
        self.move_paged(delta, select, wrap_columns, true);
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn set_viewport_rows(&mut self, rows: usize) {
        self.viewport.rows = rows.max(1);
    }

    pub fn set_viewport_top(&mut self, row: usize) {
        self.viewport.top_visual_row = row;
    }

    pub fn page_down(&mut self, select: bool, wrap_columns: usize) {
        let delta = self.viewport.page() as isize;
        self.move_paged(delta, select, wrap_columns, true);
    }

    pub fn page_up(&mut self, select: bool, wrap_columns: usize) {
        let delta = -(self.viewport.page() as isize);
        self.move_paged(delta, select, wrap_columns, true);
    }

    pub fn half_page_down(&mut self, select: bool, wrap_columns: usize) {
        let delta = self.viewport.half_page() as isize;
        self.move_paged(delta, select, wrap_columns, true);
    }

    pub fn half_page_up(&mut self, select: bool, wrap_columns: usize) {
        let delta = -(self.viewport.half_page() as isize);
        self.move_paged(delta, select, wrap_columns, true);
    }

    pub fn screen_top(&mut self, select: bool, wrap_columns: usize) {
        let target = self.viewport.screen_top_row();
        if self.move_to_visual_row(target, select, wrap_columns) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn screen_middle(&mut self, select: bool, wrap_columns: usize) {
        let target = self.viewport.screen_middle_row();
        if self.move_to_visual_row(target, select, wrap_columns) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn screen_bottom(&mut self, select: bool, wrap_columns: usize) {
        let target = self.viewport.screen_bottom_row();
        if self.move_to_visual_row(target, select, wrap_columns) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn scroll_to_center(&mut self) {
        self.queue_reveal(RevealIntent::Center);
    }

    pub fn scroll_to_top(&mut self) {
        self.queue_reveal(RevealIntent::Top);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.queue_reveal(RevealIntent::Bottom);
    }

    pub fn move_word(&mut self, backward: bool, select: bool) {
        if self.move_boundary(backward, select, previous_word_boundary, next_word_boundary) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_subword(&mut self, backward: bool, select: bool) {
        if self.move_boundary(
            backward,
            select,
            previous_subword_boundary,
            next_subword_boundary,
        ) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn set_selection(&mut self, range: Range<usize>, reversed: bool) {
        self.assign_selection(range, reversed);
    }

    pub fn backspace(&mut self) {
        self.delete_selected_or_previous();
    }

    pub fn delete_forward(&mut self) {
        self.delete_selected_or_next();
    }

    pub fn delete_word(&mut self, backward: bool) {
        self.delete_selected_or_word(backward);
    }

    pub fn insert_newline_at_cursor(&mut self) {
        self.insert_newline();
    }

    pub fn insert_tab_at_cursor(&mut self) {
        match selection_line_span(self.active_tab()) {
            Some((first, last, true)) => self.indent_selected_lines(first, last),
            _ => {
                let unit = self.active_tab().language_config().indent.indent_unit();
                self.replace_text(None, unit, UndoBoundary::Break);
            }
        }
    }

    pub fn outdent_at_cursor(&mut self) {
        let (first, last) = selection_line_span(self.active_tab())
            .map(|(first, last, _)| (first, last))
            .unwrap_or_else(|| {
                let line = self.active_cursor_position().line;
                (line, line)
            });
        self.outdent_selected_lines(first, last);
    }

    fn indent_selected_lines(&mut self, first: usize, last: usize) {
        let unit = self.active_tab().language_config().indent.indent_unit();
        let unit_chars = unit.chars().count();
        let (start_pos, end_pos, reversed) = self.selection_endpoints();
        // Endpoints at column 0 of a touched line stay at column 0 so the
        // new leading whitespace falls inside the selection — matches VS
        // Code / Sublime "keep the same logical lines selected" behavior.
        let shift = |pos: Position| -> Position {
            if (first..=last).contains(&pos.line) && pos.column > 0 {
                Position {
                    line: pos.line,
                    column: pos.column + unit_chars,
                }
            } else {
                pos
            }
        };
        let new_start = shift(start_pos);
        let new_end = shift(end_pos);

        // The cursor passed here is overwritten by `restore_selection` below,
        // so we just hand in a valid position on the edited range.
        let result = self.apply_line_edit(|lines| {
            editor_ops::indent_lines(lines, first, last, &unit);
            Some(((), new_end.line, new_end.column))
        });
        if result.is_none() {
            return;
        }

        self.restore_selection(new_start, new_end, reversed);
    }

    fn outdent_selected_lines(&mut self, first: usize, last: usize) {
        let unit = self.active_tab().language_config().indent.indent_unit();
        let (start_pos, end_pos, reversed) = self.selection_endpoints();
        let had_selection = self.active_tab().has_selection();
        let cursor_pos = self.active_cursor_position();
        let cursor_line = cursor_pos.line;

        let result = self.apply_line_edit(|lines| {
            let removed = editor_ops::outdent_lines(lines, first, last, &unit);
            let new_cursor_col = if (first..=last).contains(&cursor_line) {
                let removed_for_cursor = removed.get(cursor_line - first).copied().unwrap_or(0);
                cursor_pos.column.saturating_sub(removed_for_cursor)
            } else {
                cursor_pos.column
            };
            Some((removed, cursor_line, new_cursor_col))
        });
        let Some(removed) = result else {
            return;
        };
        if !had_selection {
            return;
        }

        let shift = |pos: Position| -> Position {
            if (first..=last).contains(&pos.line) {
                let removed_for_line = removed.get(pos.line - first).copied().unwrap_or(0);
                Position {
                    line: pos.line,
                    column: pos.column.saturating_sub(removed_for_line),
                }
            } else {
                pos
            }
        };
        self.restore_selection(shift(start_pos), shift(end_pos), reversed);
    }

    fn selection_endpoints(&self) -> (Position, Position, bool) {
        let tab = self.active_tab();
        let selection = tab.selected_range();
        let buffer = tab.buffer();
        let start = char_to_position(buffer, selection.start);
        let end = char_to_position(buffer, selection.end);
        (start, end, tab.selection_reversed())
    }

    fn restore_selection(&mut self, start: Position, end: Position, reversed: bool) {
        let buffer = self.active_tab().buffer();
        let start_char = position_to_char(buffer, start);
        let end_char = position_to_char(buffer, end);
        self.assign_selection(start_char..end_char, reversed);
    }

    pub fn delete_line(&mut self) {
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let line = editor_ops::delete_line(lines, pos.line);
            Some(((), line, pos.column))
        });
    }

    pub fn move_line_up(&mut self) {
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let line = editor_ops::move_line_up(lines, pos.line)?;
            Some(((), line, pos.column))
        });
    }

    pub fn move_line_down(&mut self) {
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let line = editor_ops::move_line_down(lines, pos.line)?;
            Some(((), line, pos.column))
        });
    }

    pub fn duplicate_line(&mut self) {
        let tab = self.active_tab();
        if let Some(text) = tab.selected_text() {
            let range = tab.selected_range();
            let char_len = range.end - range.start;
            let inserted_start = range.end;
            self.edit_active(
                EditKind::Other,
                UndoBoundary::Break,
                inserted_start..inserted_start,
                &text,
            );
            self.assign_selection(inserted_start..inserted_start + char_len, false);
            return;
        }

        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let line = editor_ops::duplicate_line(lines, pos.line);
            Some(((), line, pos.column))
        });
    }

    pub fn toggle_comment(&mut self) {
        let Some(prefix) = self.active_tab().language_config().line_comment else {
            self.status = "No line-comment syntax for this language.".to_string();
            return;
        };
        let selected = self.active_tab().selected_range();
        let cursor = self.active_cursor_position();
        let start = char_to_position(self.active_tab().buffer(), selected.start);
        let end = char_to_position(self.active_tab().buffer(), selected.end);
        let first = start.line.min(end.line);
        let last = start.line.max(end.line);
        let _ = self.apply_line_edit(|lines| {
            let (line, col) =
                editor_ops::toggle_comment(lines, first, last, cursor.line, cursor.column, prefix);
            Some(((), line, col))
        });
    }

    pub fn request_paste(&mut self) {
        self.queue_effect(EditorEffect::ReadClipboard);
    }

    pub fn clipboard_unavailable(&mut self) {
        self.status = "Clipboard does not currently contain plain text.".to_string();
    }

    pub fn paste_text(&mut self, text: String) {
        self.replace_text(None, text.clone(), UndoBoundary::Break);
        self.status = format!("Pasted {} line(s).", text.lines().count());
    }

    pub fn toggle_find_panel(&mut self, show_replace: bool) {
        if self.find.visible && self.find.show_replace == show_replace {
            self.close_find_panel();
        } else {
            self.open_find_panel(show_replace);
        }
    }

    pub fn find_next_match(&mut self) {
        if self.find_next() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn find_prev_match(&mut self) {
        if self.find_prev() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn replace_current_match(&mut self) {
        if self.replace_one() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn replace_all_matches_in_document(&mut self) {
        if self.replace_all_matches() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn toggle_goto_line_panel(&mut self) {
        if self.goto_line.is_some() {
            self.close_goto_line_panel();
        } else {
            self.open_goto_line_panel();
        }
    }

    pub fn undo(&mut self) {
        if self.active_tab_mut().undo() {
            self.sync_find_after_edit();
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn redo(&mut self) {
        if self.active_tab_mut().redo() {
            self.sync_find_after_edit();
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }
}

fn next_active_after_tab_close(len: usize, active_index: usize, closed_index: usize) -> usize {
    debug_assert!(len > 0);
    debug_assert!(closed_index < len);
    debug_assert!(active_index < len);

    if len == 1 {
        return 0;
    }
    if closed_index < active_index {
        active_index - 1
    } else if closed_index == active_index {
        active_index.min(len - 2)
    } else {
        active_index
    }
}

fn should_refocus_editor_after_tab_close(active_index: usize, closed_index: usize) -> bool {
    active_index == closed_index
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(ch)
}

fn auto_pair_pair_for(config: &LanguageConfig, ch: char) -> Option<(char, char)> {
    if is_auto_pair_quote(ch) && config.auto_pair_suppress_quotes.contains(&ch) {
        return None;
    }
    config
        .auto_pairs
        .iter()
        .copied()
        .find(|(opener, closer)| *opener == ch || *closer == ch)
}

fn is_auto_pair_quote(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}

/// Returns `(first, last, spans_multiple_lines)` for the active selection,
/// or `None` for a collapsed cursor. `last` excludes a trailing line whose
/// only contribution is a column-0 anchor after a preceding newline.
fn selection_line_span(tab: &EditorTab) -> Option<(usize, usize, bool)> {
    let selection = tab.selected_range();
    if selection.start == selection.end {
        return None;
    }
    let buffer = tab.buffer();
    let start = char_to_position(buffer, selection.start);
    let end = char_to_position(buffer, selection.end);
    let spans = start.line != end.line;
    let last = if end.column == 0 && end.line > start.line {
        end.line - 1
    } else {
        end.line
    };
    Some((start.line, last.max(start.line), spans))
}

fn display_line_char_len(tab: &EditorTab, line_ix: usize) -> usize {
    tab.buffer()
        .line(line_ix.min(tab.buffer().len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count()
}

fn first_non_blank_column(tab: &EditorTab, line_ix: usize) -> usize {
    tab.buffer()
        .line(line_ix.min(tab.buffer().len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .position(|ch| !ch.is_whitespace())
        .unwrap_or(0)
}

fn linewise_range_at_char(buffer: &ropey::Rope, char_index: usize) -> Range<usize> {
    let range = line_range_at_char(buffer, char_index);
    // If the line already owns its terminator we can return as-is.
    let ends_in_newline =
        range.end > range.start && matches!(buffer.char(range.end - 1), '\n' | '\r');
    if ends_in_newline {
        return range;
    }
    // Only try to pull a terminator from the preceding line when the cursor
    // sits on the buffer's final line (either the trailing unterminated line
    // or an empty trailing row after the last `\n`).
    if range.end != buffer.len_chars() || range.start == 0 {
        return range;
    }
    let mut start = range.start;
    match buffer.char(start - 1) {
        '\n' => {
            start -= 1;
            if start > 0 && buffer.char(start - 1) == '\r' {
                start -= 1;
            }
            start..range.end
        }
        '\r' => (start - 1)..range.end,
        _ => range,
    }
}

fn preferred_newline_for_active_tab(tab: &EditorTab) -> &'static str {
    let mut chars = tab.buffer().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                return "\r\n";
            }
            return "\n";
        }
        if ch == '\n' {
            return "\n";
        }
    }
    "\n"
}

fn vim_position_lt(a: Position, b: Position) -> bool {
    (a.line, a.column) < (b.line, b.column)
}

fn inclusive_position_to_exclusive_char(tab: &EditorTab, position: Position) -> usize {
    let line = position
        .line
        .min(tab.buffer().len_lines().saturating_sub(1));
    let line_start = tab.buffer().line_to_char(line);
    let display_len = display_line_char_len(tab, line);
    if display_len == 0 {
        return line_start;
    }
    line_start + (position.column.min(display_len.saturating_sub(1)) + 1).min(display_len)
}

// Inclusive char-column range `[from_col, to_col]` as a byte range on `line`.
// Saturates `from_col` past EOL to `line.len()` and rounds `to_col` to its
// cluster's far edge, so multi-char clusters can't be split.
fn line_byte_range(line: &str, from_col: usize, to_col: usize) -> (usize, usize) {
    let cells = selection::cells_of_str(line);
    if cells.is_empty() {
        return (0, 0);
    }
    let start_cell = selection::cell_partition_by_char(&cells, from_col);
    let start_byte = cells.get(start_cell).map_or(line.len(), |c| c.byte_start);
    let end_cell = selection::cell_containing_char(&cells, to_col) + 1;
    let end_byte = cells.get(end_cell).map_or(line.len(), |c| c.byte_start);
    (start_byte, end_byte.max(start_byte))
}

fn extract_text_range(lines: &[String], from: &Position, to: &Position) -> String {
    if from.line >= lines.len() || to.line >= lines.len() {
        return String::new();
    }
    if from.line == to.line {
        let (s, e) = line_byte_range(&lines[from.line], from.column, to.column);
        return lines[from.line][s..e].to_string();
    }
    let (first_start, _) = line_byte_range(&lines[from.line], from.column, usize::MAX);
    let (_, last_end) = line_byte_range(&lines[to.line], 0, to.column);
    let mut result = String::new();
    result.push_str(&lines[from.line][first_start..]);
    for line in lines.iter().take(to.line).skip(from.line + 1) {
        result.push('\n');
        result.push_str(line);
    }
    result.push('\n');
    result.push_str(&lines[to.line][..last_end]);
    result
}

fn remove_text_range(lines: &mut Vec<String>, from: &Position, to: &Position) {
    if from.line >= lines.len() || to.line >= lines.len() {
        return;
    }
    if from.line == to.line {
        let (s, e) = line_byte_range(&lines[from.line], from.column, to.column);
        let line = &lines[from.line];
        lines[from.line] = format!("{}{}", &line[..s], &line[e..]);
    } else {
        let (first_start, _) = line_byte_range(&lines[from.line], from.column, usize::MAX);
        let (_, last_end) = line_byte_range(&lines[to.line], 0, to.column);
        let prefix = lines[from.line][..first_start].to_string();
        let suffix = lines[to.line][last_end..].to_string();
        lines[from.line] = format!("{prefix}{suffix}");
        if from.line < to.line {
            lines.drain((from.line + 1)..=to.line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64, title: &str, text: &str) -> EditorTab {
        EditorTab::from_text(TabId::from_raw(id), title.to_string(), None, text)
    }

    fn model_with_tabs(tabs: Vec<EditorTab>, status: String) -> EditorModel {
        let mut tabs = tabs.into_iter();
        let first = tabs.next().expect("test model needs at least one tab");
        EditorModel::from_tabs(first, tabs.collect(), status)
    }

    #[test]
    fn tab_switch_commands_own_switch_status() {
        let mut model = model_with_tabs(
            vec![tab(1, "one.txt", "one"), tab(2, "two.txt", "two")],
            "Ready.".to_string(),
        );

        model.set_active_tab(1);
        assert_eq!(model.snapshot().status, "Switched to two.txt.");

        model.prev_tab();
        assert_eq!(model.snapshot().status, "Switched to one.txt.");

        model.next_tab();
        assert_eq!(model.snapshot().status, "Switched to two.txt.");
    }

    #[test]
    fn close_active_tab_command_closes_current_tab() {
        let mut model = model_with_tabs(
            vec![tab(1, "one.txt", "one"), tab(2, "two.txt", "two")],
            "Ready.".to_string(),
        );
        model.set_active_tab(1);

        let active_id = model.active_tab_id();
        assert!(model.close_clean_tab_by_id(active_id));

        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["one.txt"]);
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.status, "Closed tab.");
    }

    #[test]
    fn select_all_queues_primary_selection() {
        let mut model = model_with_tabs(vec![tab(1, "one.txt", "hello")], "Ready.".to_string());

        model.select_all();

        assert_eq!(model.snapshot().selection, 0..5);
        assert_eq!(
            model.drain_effects(),
            vec![EditorEffect::WritePrimary("hello".to_string())]
        );
    }
}
