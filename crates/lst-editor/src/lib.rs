mod document;
mod editor_ops;
mod effect;
pub mod find;
pub mod position;
pub mod selection;
mod snapshot;
mod tab;
pub mod viewport;
pub mod vim;
pub mod wrap;

pub use document::{EditKind, UndoBoundary};
pub use effect::{EditorEffect, FocusTarget, RevealIntent};
pub use snapshot::EditorSnapshot;
pub use tab::{EditorTab, FileStamp, TabId};
pub use viewport::Viewport;

use crate::{
    document::{char_to_position, line_indent_prefix, position_to_char},
    find::{FindState, MatchPos},
    position::Position,
    selection::{
        next_subword_boundary, next_word_boundary, previous_subword_boundary,
        previous_word_boundary,
    },
};
use std::{ops::Range, path::PathBuf, sync::Arc};

pub const UNTITLED_PREFIX: &str = "untitled";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsavedTab {
    pub index: usize,
    pub tab_id: TabId,
    pub title: String,
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabCloseRequest {
    Close { tab_id: TabId },
    Unsaved(UnsavedTab),
}

pub struct EditorModel {
    tabs: Vec<EditorTab>,
    active: usize,
    next_untitled_id: usize,
    show_gutter: bool,
    show_wrap: bool,
    find: FindState,
    goto_line: Option<String>,
    status: String,
    vim: vim::VimState,
    next_tab_id: u64,
    viewport: Viewport,
    effects: Vec<EditorEffect>,
}

impl EditorModel {
    pub fn new(tabs: Vec<EditorTab>, status: String) -> Self {
        let next_tab_id = tabs
            .iter()
            .map(|tab| tab.id().get())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Self {
            tabs,
            active: 0,
            next_untitled_id: 2,
            show_gutter: true,
            show_wrap: true,
            find: FindState::new(),
            goto_line: None,
            status,
            vim: vim::VimState::new(),
            next_tab_id,
            viewport: Viewport::default(),
            effects: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        let tab = EditorTab::empty(TabId::from_raw(1), format!("{UNTITLED_PREFIX}-1"));
        Self::new(vec![tab], "Ready.".to_string())
    }

    fn alloc_tab_id(&mut self) -> TabId {
        let id = TabId::from_raw(self.next_tab_id);
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        id
    }

    pub fn active_tab(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    fn active_tab_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active]
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
        self.tabs.iter().find(|tab| tab.id() == tab_id)
    }

    fn tab_mut_by_id(&mut self, tab_id: TabId) -> Option<&mut EditorTab> {
        self.tabs.iter_mut().find(|tab| tab.id() == tab_id)
    }

    fn tab_index_by_id(&self, tab_id: TabId) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id() == tab_id)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_index(&self) -> usize {
        self.active
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
        if index >= self.tabs.len() {
            return false;
        }
        self.active = index;
        self.vim.on_tab_switch();
        self.active_tab_mut().preferred_column = None;
        self.sync_find_with_active_document();
        self.status = format!("Switched to {}.", self.active_tab().display_name());
        true
    }

    fn move_to_char_inner(
        &mut self,
        offset: usize,
        select: bool,
        preferred_column: Option<usize>,
    ) -> bool {
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
        target != cursor || select
    }

    fn assign_selection(&mut self, range: Range<usize>, reversed: bool) {
        let end = self.active_tab().len_chars();
        let start = range.start.min(end);
        let finish = range.end.min(end);
        let tab = self.active_tab_mut();
        tab.selection = start.min(finish)..start.max(finish);
        tab.selection_reversed = reversed;
        tab.preferred_column = None;
        tab.marked_range = None;
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

    fn open_find(&mut self, show_replace: bool, selected_text: Option<String>) {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(text) = selected_text {
            if !text.contains('\n') {
                self.find.query = text;
                self.reindex_find_matches_to_nearest();
            }
        }
        self.queue_focus(FocusTarget::FindQuery);
    }

    fn close_find(&mut self) {
        self.find.visible = false;
        self.find.show_replace = false;
        self.queue_focus(FocusTarget::Editor);
    }

    fn open_goto_line(&mut self) {
        self.goto_line = Some(String::new());
        self.queue_focus(FocusTarget::GotoLine);
    }

    fn close_goto_line(&mut self) {
        self.goto_line = None;
        self.queue_focus(FocusTarget::Editor);
    }

    fn set_find_query(&mut self, text: String) {
        self.find.query = text;
        self.reindex_find_matches_to_nearest();
    }

    fn set_find_query_and_activate(&mut self, text: String) {
        self.set_find_query(text);
        if self.move_to_current_find_match() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    fn set_find_replacement(&mut self, text: String) {
        self.find.replacement = text;
    }

    fn set_goto_line(&mut self, text: String) {
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

    fn submit_goto_line(&mut self) -> bool {
        let Some(text) = self.goto_line.clone() else {
            return false;
        };
        let Ok(line_one_based) = text.trim().parse::<usize>() else {
            self.close_goto_line();
            return false;
        };
        let target = line_one_based
            .saturating_sub(1)
            .min(self.active_tab().line_count().saturating_sub(1));
        self.active_tab_mut().set_cursor_position(
            Position {
                line: target,
                column: 0,
            },
            None,
        );
        self.close_goto_line();
        true
    }

    fn close_tab_at(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.tabs[index].modified() {
            self.status = format!(
                "Unsaved changes in {}. Save or Save As before closing this tab.",
                self.tabs[index].display_name()
            );
            return false;
        }
        self.close_tab_at_unchecked(index)
    }

    fn close_tab_at_unchecked(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = self.new_empty_tab();
            self.activate_tab(0);
            self.queue_focus(FocusTarget::Editor);
            self.status = "Closed tab.".to_string();
            return true;
        }

        let should_refocus = should_refocus_editor_after_tab_close(self.active, index);
        let next_active = next_active_after_tab_close(self.tabs.len(), self.active, index);
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

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    ) {
        let range = {
            let tab = self.active_tab();
            range
                .or_else(|| tab.marked_range.clone())
                .unwrap_or_else(|| tab.selected_range())
        };
        let kind = if text.is_empty() {
            EditKind::Delete
        } else {
            EditKind::Insert
        };
        self.edit_active(kind, boundary, range, &text);
    }

    fn replace_and_mark_text_inner(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    ) {
        let range = {
            let tab = self.active_tab();
            range
                .or_else(|| tab.marked_range.clone())
                .unwrap_or_else(|| tab.selected_range())
        };
        let inserted_start = range.start;
        self.active_tab_mut()
            .edit(EditKind::Other, UndoBoundary::Break, range, &text);
        {
            let tab = self.active_tab_mut();
            if text.is_empty() {
                tab.marked_range = None;
            } else {
                tab.marked_range = Some(inserted_start..inserted_start + text.chars().count());
            }
            tab.selection = selected_range
                .map(|range| inserted_start + range.start..inserted_start + range.end)
                .unwrap_or_else(|| {
                    let cursor = inserted_start + text.chars().count();
                    cursor..cursor
                });
            tab.selection_reversed = false;
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
        let target = if delta.is_negative() {
            tab.cursor_char().saturating_sub(delta.unsigned_abs())
        } else {
            (tab.cursor_char() + delta as usize).min(tab.len_chars())
        };
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        true
    }

    fn move_horizontal_collapse(&mut self, backward: bool) -> bool {
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
            return true;
        }

        let delta = if backward { -1 } else { 1 };
        self.move_horizontal(delta, false)
    }

    fn move_boundary(
        &mut self,
        backward: bool,
        select: bool,
        prev_fn: fn(&ropey::Rope, usize) -> usize,
        next_fn: fn(&ropey::Rope, usize) -> usize,
    ) -> bool {
        let target = {
            let tab = self.active_tab();
            if !select && tab.has_selection() {
                if backward {
                    tab.selection().start
                } else {
                    tab.selection().end
                }
            } else if backward {
                prev_fn(tab.buffer(), tab.cursor_char())
            } else {
                next_fn(tab.buffer(), tab.cursor_char())
            }
        };

        let tab = self.active_tab_mut();
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        true
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

    fn move_line_boundary_inner(&mut self, to_end: bool, select: bool) -> bool {
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
        target != cursor
    }

    fn move_smart_home_inner(&mut self, select: bool) -> bool {
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
        target != cursor
    }

    fn move_document_boundary_inner(&mut self, to_end: bool, select: bool) -> bool {
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
        target != cursor
    }

    fn replace_active_lines(&mut self, lines: Vec<String>, cursor_line: usize, cursor_col: usize) {
        let newline = preferred_newline_for_active_tab(self.active_tab());
        {
            let tab = self.active_tab_mut();
            tab.set_text(&lines.join(newline));
            tab.modified = true;
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
                cursor - 1..cursor
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
                cursor..cursor + 1
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
        self.replace_text_in_range(None, format!("{newline}{indent}"), UndoBoundary::Break);
    }

    fn copy_selection_inner(&mut self) -> bool {
        let Some(text) = self.active_tab().selected_text() else {
            return false;
        };
        self.queue_effect(EditorEffect::WriteClipboard(text.clone()));
        self.queue_effect(EditorEffect::WritePrimary(text));
        self.status = "Copied selection.".to_string();
        true
    }

    fn cut_selection_inner(&mut self) -> bool {
        let Some(text) = self.active_tab().selected_text() else {
            return false;
        };
        self.queue_effect(EditorEffect::WriteClipboard(text.clone()));
        self.queue_effect(EditorEffect::WritePrimary(text));
        let range = self.active_tab().selected_range();
        self.edit_active(EditKind::Delete, UndoBoundary::Break, range, "");
        self.status = "Cut selection.".to_string();
        true
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
                    let selected = self.active_tab().selected_text();
                    self.open_find(false, selected);
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
            tab.selection = head_char..anchor_end.max(head_char);
            tab.selection_reversed = true;
        } else {
            tab.selection = anchor_char..head_end.max(anchor_char);
            tab.selection_reversed = false;
        }
        tab.marked_range = None;
        tab.preferred_column = None;
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
            let chars: Vec<char> = lines
                .get(pos.line)
                .map_or(Vec::new(), |line| line.chars().collect());
            if pos.column + count > chars.len() {
                return None;
            }
            let mut new_chars = chars;
            for ix in 0..count {
                new_chars[pos.column + ix] = ch;
            }
            lines[pos.line] = new_chars.into_iter().collect();
            Some(((), pos.line, pos.column + count - 1))
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
        let range = self
            .active_tab()
            .marked_range
            .clone()
            .unwrap_or_else(|| self.active_tab().selected_range());
        self.active_tab_mut()
            .edit(EditKind::Insert, UndoBoundary::Break, range, &text);
        self.sync_find_after_edit();
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn replace_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    ) {
        self.replace_text_in_range(range, text, boundary);
    }

    pub fn replace_text_from_input(&mut self, range: Option<Range<usize>>, text: String) {
        let boundary = if text.chars().any(char::is_whitespace) {
            UndoBoundary::Break
        } else {
            UndoBoundary::Merge
        };
        self.replace_text_in_range(range, text, boundary);
    }

    pub fn replace_and_mark_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    ) {
        self.replace_and_mark_text_inner(range, text, selected_range);
    }

    pub fn clear_marked_text(&mut self) {
        self.active_tab_mut().marked_range = None;
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

    pub fn close_active_tab(&mut self) {
        self.close_tab_at(self.active);
    }

    pub fn close_tab(&mut self, index: usize) {
        self.close_tab_at(index);
    }

    pub fn close_request_for_tab(&self, index: usize) -> Option<TabCloseRequest> {
        let tab = self.tabs.get(index)?;
        if tab.modified() {
            Some(TabCloseRequest::Unsaved(UnsavedTab {
                index,
                tab_id: tab.id(),
                title: tab.display_name(),
                path: tab.path().cloned(),
            }))
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

    pub fn close_cancelled(&mut self) {
        self.status = "Close cancelled.".to_string();
    }

    pub fn set_active_tab(&mut self, index: usize) {
        if self.activate_tab(index) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.activate_tab((self.active + 1) % self.tabs.len());
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            let prev = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
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
        self.move_horizontal(delta, select);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn move_horizontal_collapsed(&mut self, backward: bool) {
        if self.move_horizontal_collapse(backward) {
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
        self.move_boundary(backward, select, previous_word_boundary, next_word_boundary);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn move_subword(&mut self, backward: bool, select: bool) {
        self.move_boundary(
            backward,
            select,
            previous_subword_boundary,
            next_subword_boundary,
        );
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn move_line_boundary(&mut self, to_end: bool, select: bool) {
        if self.move_line_boundary_inner(to_end, select) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn smart_home(&mut self, select: bool) {
        if self.move_smart_home_inner(select) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_document_boundary(&mut self, to_end: bool, select: bool) {
        if self.move_document_boundary_inner(to_end, select) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_to_char(&mut self, offset: usize, select: bool, preferred_column: Option<usize>) {
        if self.move_to_char_inner(offset, select, preferred_column) {
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
        self.replace_text_in_range(None, "    ".to_string(), UndoBoundary::Break);
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
        let pos = self.active_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let line = editor_ops::duplicate_line(lines, pos.line);
            Some(((), line, pos.column))
        });
    }

    pub fn toggle_comment(&mut self) {
        let prefix = self
            .active_tab()
            .path()
            .and_then(|path| path.extension())
            .and_then(|ext| editor_ops::comment_prefix(ext.to_string_lossy().as_ref()))
            .unwrap_or("//");
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

    pub fn copy_selection(&mut self) {
        self.copy_selection_inner();
    }

    pub fn cut_selection(&mut self) {
        self.cut_selection_inner();
    }

    pub fn request_paste(&mut self) {
        self.queue_effect(EditorEffect::ReadClipboard);
    }

    pub fn clipboard_unavailable(&mut self) {
        self.status = "Clipboard does not currently contain plain text.".to_string();
    }

    pub fn paste_text(&mut self, text: String) {
        self.replace_text_in_range(None, text.clone(), UndoBoundary::Break);
        self.status = format!("Pasted {} line(s).", text.lines().count());
    }

    pub fn open_find_panel(&mut self, show_replace: bool) {
        let selected = self.active_tab().selected_text();
        self.open_find(show_replace, selected);
    }

    pub fn toggle_find_panel(&mut self, show_replace: bool) {
        if self.find.visible && self.find.show_replace == show_replace {
            self.close_find();
        } else {
            let selected = self.active_tab().selected_text();
            self.open_find(show_replace, selected);
        }
    }

    pub fn close_find_panel(&mut self) {
        self.close_find();
    }

    pub fn update_find_query(&mut self, text: String) {
        self.set_find_query(text);
    }

    pub fn update_find_query_and_activate(&mut self, text: String) {
        self.set_find_query_and_activate(text);
    }

    pub fn update_find_replacement(&mut self, text: String) {
        self.set_find_replacement(text);
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

    pub fn open_goto_line_panel(&mut self) {
        self.open_goto_line();
    }

    pub fn toggle_goto_line_panel(&mut self) {
        if self.goto_line.is_some() {
            self.close_goto_line();
        } else {
            self.open_goto_line();
        }
    }

    pub fn close_goto_line_panel(&mut self) {
        self.close_goto_line();
    }

    pub fn update_goto_line(&mut self, text: String) {
        self.set_goto_line(text);
    }

    pub fn submit_goto_line_input(&mut self) {
        if self.submit_goto_line() {
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn request_open_files(&mut self) {
        self.queue_effect(EditorEffect::OpenFiles);
    }

    pub fn open_files(&mut self, files: Vec<(PathBuf, String)>) {
        self.open_files_with_stamps(
            files
                .into_iter()
                .map(|(path, text)| (path, text, None))
                .collect(),
        );
    }

    pub fn open_files_with_stamps(&mut self, files: Vec<(PathBuf, String, Option<FileStamp>)>) {
        let start_len = self.tabs.len();
        for (path, text, file_stamp) in files {
            let id = self.alloc_tab_id();
            self.tabs
                .push(EditorTab::from_path_with_stamp(id, path, &text, file_stamp));
        }
        if self.tabs.len() > start_len {
            self.activate_tab(self.tabs.len() - 1);
            self.status = format!("Opened {} tab(s).", self.tabs.len() - start_len);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn open_file_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to open {}: {message}", path.display());
    }

    pub fn request_save(&mut self) {
        self.request_save_tab(self.active_tab_id());
    }

    pub fn request_save_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        let body = tab.buffer_text();
        if let Some(path) = tab.path().cloned() {
            self.queue_effect(EditorEffect::SaveFile {
                tab_id,
                path,
                body,
                expected_stamp: tab.file_stamp(),
            });
        } else {
            let previous_scratchpad_path = if tab.is_scratchpad() {
                tab.path().cloned()
            } else {
                None
            };
            self.queue_effect(EditorEffect::SaveFileAs {
                tab_id,
                suggested_name: tab.display_name(),
                body,
                previous_scratchpad_path,
            });
        }
    }

    pub fn request_save_as(&mut self) {
        self.request_save_as_tab(self.active_tab_id());
    }

    pub fn request_save_as_tab(&mut self, tab_id: TabId) {
        let Some(tab) = self.tab_by_id(tab_id) else {
            return;
        };
        let previous_scratchpad_path = if tab.is_scratchpad() {
            tab.path().cloned()
        } else {
            None
        };
        self.queue_effect(EditorEffect::SaveFileAs {
            tab_id,
            suggested_name: tab.display_name(),
            body: tab.buffer_text(),
            previous_scratchpad_path,
        });
    }

    pub fn save_finished(&mut self, path: PathBuf) {
        let tab_id = self.active_tab_id();
        self.save_finished_for_tab(tab_id, path, None);
    }

    pub fn save_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        file_stamp: Option<FileStamp>,
    ) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            if let Some(file_stamp) = file_stamp {
                tab.mark_saved(path.clone(), file_stamp);
            } else {
                tab.path = Some(path.clone());
                tab.modified = false;
            }
            self.status = format!("Saved {}.", path.display());
        }
    }

    pub fn save_as_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        file_stamp: Option<FileStamp>,
    ) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            if let Some(file_stamp) = file_stamp {
                tab.mark_saved_as(path.clone(), file_stamp);
            } else {
                tab.path = Some(path.clone());
                tab.modified = false;
                tab.is_scratchpad = false;
            }
            self.status = format!("Saved {}.", path.display());
        }
    }

    pub fn save_failed(&mut self, path: PathBuf, message: String) {
        self.status = format!("Failed to save {}: {message}", path.display());
    }

    pub fn save_failed_for_tab(&mut self, tab_id: TabId, path: PathBuf, message: String) {
        if self.tab_by_id(tab_id).is_some() {
            self.status = format!("Failed to save {}: {message}", path.display());
        }
    }

    pub fn autosave_tick(&mut self) {
        let jobs = self
            .tabs
            .iter()
            .filter(|tab| tab.modified())
            .filter_map(|tab| {
                let path = tab.path().cloned()?;
                let open_tabs_for_path = self
                    .tabs
                    .iter()
                    .filter(|candidate| candidate.path() == Some(&path))
                    .take(2)
                    .count();
                if open_tabs_for_path != 1 {
                    return None;
                }
                Some((
                    tab.id(),
                    path,
                    tab.buffer_text(),
                    tab.revision(),
                    tab.file_stamp(),
                ))
            })
            .collect::<Vec<_>>();
        for (tab_id, path, body, revision, expected_stamp) in jobs {
            self.queue_effect(EditorEffect::AutosaveFile {
                tab_id,
                path,
                body,
                revision,
                expected_stamp,
            });
        }
    }

    pub fn autosave_finished(&mut self, path: PathBuf, revision: u64) {
        let Some(tab_id) = self
            .tabs
            .iter()
            .find(|tab| tab.path() == Some(&path) && tab.revision() == revision)
            .map(EditorTab::id)
        else {
            return;
        };
        self.autosave_finished_for_tab(tab_id, path, revision, None);
    }

    pub fn autosave_finished_for_tab(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        revision: u64,
        file_stamp: Option<FileStamp>,
    ) {
        for tab in &mut self.tabs {
            if tab.id() == tab_id && tab.path() == Some(&path) && tab.revision() == revision {
                tab.modified = false;
                if let Some(file_stamp) = file_stamp {
                    tab.file_stamp = Some(file_stamp);
                    tab.suppressed_conflict_stamp = None;
                }
            }
        }
        if self.active_tab().id() == tab_id
            && self.active_tab().path() == Some(&path)
            && self.active_tab().revision() == revision
        {
            self.status = format!("Autosaved {}.", path.display());
        }
    }

    pub fn autosave_failed(&mut self, path: PathBuf, message: String) {
        if self.active_tab().path() == Some(&path) {
            self.status = format!("Autosave failed for {}: {message}", path.display());
        }
    }

    pub fn autosave_failed_for_tab(&mut self, tab_id: TabId, path: PathBuf, message: String) {
        if self.active_tab().id() == tab_id {
            self.status = format!("Autosave failed for {}: {message}", path.display());
        }
    }

    pub fn reload_tab_from_disk(
        &mut self,
        tab_id: TabId,
        path: PathBuf,
        text: String,
        file_stamp: FileStamp,
    ) -> bool {
        let Some(tab) = self.tab_mut_by_id(tab_id) else {
            return false;
        };
        tab.path = Some(path.clone());
        tab.reset_from_disk(&text, file_stamp);
        self.sync_find_with_active_document();
        self.status = format!("Reloaded {}.", path.display());
        true
    }

    pub fn reload_failed(&mut self, tab_id: TabId, path: PathBuf, message: String) {
        if self.tab_by_id(tab_id).is_some() {
            self.status = format!("Failed to reload {}: {message}", path.display());
        }
    }

    pub fn suppress_file_conflict(&mut self, tab_id: TabId, path: PathBuf, stamp: FileStamp) {
        if let Some(tab) = self.tab_mut_by_id(tab_id) {
            tab.suppress_file_conflict(stamp);
            self.status = format!("Kept local changes for {}.", path.display());
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

fn extract_text_range(lines: &[String], from: &Position, to: &Position) -> String {
    if from.line >= lines.len() || to.line >= lines.len() {
        return String::new();
    }
    if from.line == to.line {
        let chars: Vec<char> = lines[from.line].chars().collect();
        let start = from.column.min(chars.len());
        let end = (to.column + 1).min(chars.len());
        if start >= end {
            return String::new();
        }
        chars[start..end].iter().collect()
    } else {
        let mut result = String::new();
        let first: Vec<char> = lines[from.line].chars().collect();
        result.extend(&first[from.column.min(first.len())..]);
        for line in lines.iter().take(to.line).skip(from.line + 1) {
            result.push('\n');
            result.push_str(line);
        }
        result.push('\n');
        let last: Vec<char> = lines[to.line].chars().collect();
        result.extend(&last[..(to.column + 1).min(last.len())]);
        result
    }
}

fn remove_text_range(lines: &mut Vec<String>, from: &Position, to: &Position) {
    if from.line >= lines.len() || to.line >= lines.len() {
        return;
    }
    if from.line == to.line {
        let chars: Vec<char> = lines[from.line].chars().collect();
        let start = from.column.min(chars.len());
        let end = (to.column + 1).min(chars.len());
        let remaining: String = chars[..start].iter().chain(chars[end..].iter()).collect();
        lines[from.line] = remaining;
    } else {
        let first: Vec<char> = lines[from.line].chars().collect();
        let last: Vec<char> = lines[to.line].chars().collect();
        let prefix: String = first[..from.column.min(first.len())].iter().collect();
        let suffix: String = last[(to.column + 1).min(last.len())..].iter().collect();
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

    #[test]
    fn tab_switch_commands_own_switch_status() {
        let mut model = EditorModel::new(
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
        let mut model = EditorModel::new(
            vec![tab(1, "one.txt", "one"), tab(2, "two.txt", "two")],
            "Ready.".to_string(),
        );
        model.set_active_tab(1);

        model.close_active_tab();

        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["one.txt"]);
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.status, "Closed tab.");
    }

    #[test]
    fn select_all_queues_primary_selection() {
        let mut model = EditorModel::new(vec![tab(1, "one.txt", "hello")], "Ready.".to_string());

        model.select_all();

        assert_eq!(model.snapshot().selection, 0..5);
        assert_eq!(
            model.drain_effects(),
            vec![EditorEffect::WritePrimary("hello".to_string())]
        );
    }
}
