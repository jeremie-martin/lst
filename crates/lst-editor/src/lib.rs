pub mod vim;

use lst_core::{
    document::{
        char_to_position, line_indent_prefix, position_to_char, EditKind, Tab, UndoBoundary,
    },
    editor_ops,
    find::FindState,
    position::Position,
    selection::{next_word_boundary, previous_word_boundary},
    wrap,
};
use std::{
    ops::{Deref, DerefMut, Range},
    path::PathBuf,
};

pub const UNTITLED_PREFIX: &str = "untitled";

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    Editor,
    FindQuery,
    FindReplace,
    GotoLine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEffect {
    Focus(FocusTarget),
    RevealCursor,
    WriteClipboard(String),
    WritePrimary(String),
    ReadClipboard,
    OpenFiles,
    SaveFile {
        path: PathBuf,
        body: String,
    },
    SaveFileAs {
        suggested_name: String,
        body: String,
    },
    AutosaveFile {
        path: PathBuf,
        body: String,
        revision: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertText(String),
    ReplaceText {
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    },
    ReplaceTextFromInput {
        range: Option<Range<usize>>,
        text: String,
    },
    ReplaceAndMarkText {
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    },
    ClearMarkedText,
    NewTab,
    CloseTab(usize),
    SetActiveTab(usize),
    NextTab,
    PrevTab,
    ToggleWrap,
    SelectAll,
    MoveHorizontal {
        delta: isize,
        select: bool,
    },
    MoveHorizontalCollapse {
        backward: bool,
    },
    MoveVertical {
        delta: isize,
        select: bool,
    },
    MoveDisplayRows {
        delta: isize,
        select: bool,
        wrap_columns: usize,
    },
    MovePage {
        rows: usize,
        down: bool,
        select: bool,
    },
    MoveWord {
        backward: bool,
        select: bool,
    },
    MoveLineBoundary {
        to_end: bool,
        select: bool,
    },
    MoveDocumentBoundary {
        to_end: bool,
        select: bool,
    },
    MoveToChar {
        offset: usize,
        select: bool,
        preferred_column: Option<usize>,
    },
    SetSelection {
        range: Range<usize>,
        reversed: bool,
    },
    Backspace,
    DeleteForward,
    DeleteWord {
        backward: bool,
    },
    InsertNewline,
    InsertTab,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    DuplicateLine,
    ToggleComment,
    CopySelection,
    CutSelection,
    RequestPaste,
    PasteText(String),
    OpenFind {
        show_replace: bool,
    },
    ToggleFind {
        show_replace: bool,
    },
    CloseFind,
    SetFindQuery(String),
    SetFindQueryAndSelect(String),
    SetFindReplacement(String),
    FindNext,
    FindPrev,
    ReplaceOne,
    ReplaceAll,
    OpenGotoLine,
    ToggleGotoLine,
    CloseGotoLine,
    SetGotoLine(String),
    SubmitGotoLine,
    RequestOpenFiles,
    OpenFiles(Vec<(PathBuf, String)>),
    OpenFileFailed {
        path: PathBuf,
        message: String,
    },
    RequestSave,
    RequestSaveAs,
    SaveFinished {
        path: PathBuf,
    },
    SaveFailed {
        path: PathBuf,
        message: String,
    },
    AutosaveTick,
    AutosaveFinished {
        path: PathBuf,
        revision: u64,
    },
    AutosaveFailed {
        path: PathBuf,
        message: String,
    },
    Undo,
    Redo,
}

#[derive(Clone)]
pub struct EditorTab {
    id: TabId,
    doc: Tab,
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

impl Deref for EditorTab {
    type Target = Tab;

    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}

impl DerefMut for EditorTab {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.doc
    }
}

pub struct EditorModel {
    pub tabs: Vec<EditorTab>,
    pub active: usize,
    pub next_untitled_id: usize,
    pub show_gutter: bool,
    pub show_wrap: bool,
    pub find: FindState,
    pub goto_line: Option<String>,
    pub status: String,
    pub vim: vim::VimState,
    next_tab_id: u64,
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
            effects: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        let tab = EditorTab::empty(TabId(1), format!("{UNTITLED_PREFIX}-1"));
        Self::new(vec![tab], "Ready.".to_string())
    }

    pub fn alloc_tab_id(&mut self) -> TabId {
        let id = TabId(self.next_tab_id);
        self.next_tab_id = self.next_tab_id.saturating_add(1);
        id
    }

    pub fn active_tab(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    pub fn active_tab_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active]
    }

    pub fn new_empty_tab(&mut self) -> EditorTab {
        let name = format!("{UNTITLED_PREFIX}-{}", self.next_untitled_id);
        self.next_untitled_id += 1;
        let id = self.alloc_tab_id();
        EditorTab::empty(id, name)
    }

    pub fn push_tab(&mut self, tab: EditorTab) {
        self.tabs.push(tab);
    }

    pub fn set_active_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.active = index;
        self.vim.on_tab_switch();
        self.active_tab_mut().preferred_column = None;
        self.sync_find_with_active_document();
        true
    }

    fn move_to_char(
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

    fn set_selection(&mut self, range: Range<usize>, reversed: bool) {
        let end = self.active_tab().len_chars();
        let start = range.start.min(end);
        let finish = range.end.min(end);
        let tab = self.active_tab_mut();
        tab.selection = start.min(finish)..start.max(finish);
        tab.selection_reversed = reversed;
        tab.preferred_column = None;
        tab.marked_range = None;
    }

    pub fn queue_focus(&mut self, target: FocusTarget) {
        self.effects.push(EditorEffect::Focus(target));
    }

    fn queue_effect(&mut self, effect: EditorEffect) {
        self.effects.push(effect);
    }

    fn queue_reveal_cursor(&mut self) {
        self.queue_effect(EditorEffect::RevealCursor);
    }

    pub fn drain_effects(&mut self) -> Vec<EditorEffect> {
        self.effects.drain(..).collect()
    }

    pub fn open_find(&mut self, show_replace: bool, selected_text: Option<String>) {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(text) = selected_text {
            if !text.contains('\n') {
                self.find.query = text;
                self.reindex_find_matches();
            }
        }
        self.queue_focus(FocusTarget::FindQuery);
    }

    pub fn close_find(&mut self) {
        self.find.visible = false;
        self.find.show_replace = false;
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn open_goto_line(&mut self) {
        self.goto_line = Some(String::new());
        self.queue_focus(FocusTarget::GotoLine);
    }

    pub fn close_goto_line(&mut self) {
        self.goto_line = None;
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn set_find_query(&mut self, text: String) {
        self.find.query = text;
        self.reindex_find_matches_to_nearest();
    }

    pub fn set_find_query_and_select(&mut self, text: String) {
        self.set_find_query(text);
        if self.select_current_find_match() {
            self.queue_reveal_cursor();
        }
    }

    pub fn set_find_replacement(&mut self, text: String) {
        self.find.replacement = text;
    }

    pub fn set_goto_line(&mut self, text: String) {
        self.goto_line = Some(text);
    }

    pub fn active_cursor_position(&self) -> Position {
        self.active_tab().cursor_position()
    }

    pub fn active_tab_revision(&self) -> u64 {
        self.active_tab().revision()
    }

    pub fn reindex_find_matches(&mut self) {
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
        Some(char_to_position(&tab.buffer, selected.start))
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

    pub fn reindex_find_matches_to_nearest(&mut self) {
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

    pub fn find_next(&mut self) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.next();
        self.select_current_find_match()
    }

    pub fn find_prev(&mut self) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.prev();
        self.select_current_find_match()
    }

    pub fn replace_one(&mut self) -> bool {
        self.ensure_find_matches_current();
        let Some((start, end)) = self.find.current_match_range() else {
            return false;
        };
        let replacement = self.find.replacement.clone();
        let range = {
            let tab = self.active_tab();
            position_to_char(&tab.buffer, start)..position_to_char(&tab.buffer, end)
        };
        self.active_tab_mut()
            .edit(EditKind::Other, UndoBoundary::Break, range, &replacement);
        self.sync_find_after_edit();
        self.select_current_find_match();
        true
    }

    pub fn replace_all_matches(&mut self) -> bool {
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

    pub fn submit_goto_line(&mut self) -> bool {
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

    pub fn close_tab_at(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.tabs[index].modified {
            self.status = format!(
                "Unsaved changes in {}. Save or Save As before closing this tab.",
                self.tabs[index].display_name()
            );
            return false;
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = self.new_empty_tab();
            self.set_active_tab(0);
            self.queue_focus(FocusTarget::Editor);
            self.status = "Closed tab.".to_string();
            return true;
        }

        let should_refocus = should_refocus_editor_after_tab_close(self.active, index);
        let next_active = next_active_after_tab_close(self.tabs.len(), self.active, index);
        self.tabs.remove(index);
        self.set_active_tab(next_active);
        if should_refocus {
            self.queue_focus(FocusTarget::Editor);
        }
        self.status = "Closed tab.".to_string();
        true
    }

    fn select_current_find_match(&mut self) -> bool {
        let Some((start, end)) = self.find.current_match_range() else {
            return false;
        };
        self.active_tab_mut().set_cursor_position(end, Some(start));
        true
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
        self.queue_reveal_cursor();
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

    fn replace_and_mark_text(
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
        self.queue_reveal_cursor();
    }

    fn delete_selection_or_word_range(tab: &EditorTab, backward: bool) -> Option<Range<usize>> {
        if tab.has_selection() {
            return Some(tab.selected_range());
        }
        let cursor = tab.cursor_char();
        let target = if backward {
            previous_word_boundary(&tab.buffer, cursor)
        } else {
            next_word_boundary(&tab.buffer, cursor)
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

    fn move_word_boundary(&mut self, backward: bool, select: bool) -> bool {
        let target = {
            let tab = self.active_tab();
            if !select && tab.has_selection() {
                if backward {
                    tab.selection.start
                } else {
                    tab.selection.end
                }
            } else if backward {
                previous_word_boundary(&tab.buffer, tab.cursor_char())
            } else {
                next_word_boundary(&tab.buffer, tab.cursor_char())
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

    fn move_vertical(&mut self, delta: isize, select: bool) -> bool {
        let tab = self.active_tab_mut();
        let cursor = tab.cursor_char();
        let position = tab.cursor_position();
        let preferred = tab.preferred_column.unwrap_or(position.column);
        let target_line = if delta.is_negative() {
            position.line.saturating_sub(delta.unsigned_abs())
        } else {
            (position.line + delta as usize).min(tab.line_count().saturating_sub(1))
        };
        let target_column = preferred.min(display_line_char_len(tab, target_line));
        let target = tab.buffer.line_to_char(target_line) + target_column;
        tab.preferred_column = Some(preferred);
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        target != cursor
    }

    fn move_display_rows(&mut self, delta: isize, select: bool, wrap_columns: usize) -> bool {
        if !self.show_wrap {
            return self.move_vertical(delta, select);
        }

        let target = {
            let tab = self.active_tab_mut();
            let lines = tab.lines();
            let position = tab.cursor_position();
            let layout = wrap::build_wrap_layout(lines.as_ref(), wrap_columns, true);
            wrap::display_row_target(
                lines.as_ref(),
                position.line,
                position.column,
                tab.preferred_column,
                delta,
                &layout,
            )
        };

        let Some(target) = target else {
            return false;
        };

        let target_char = {
            let tab = self.active_tab();
            position_to_char(
                &tab.buffer,
                Position {
                    line: target.line,
                    column: target.column,
                },
            )
        };
        let cursor = self.active_tab().cursor_char();
        let tab = self.active_tab_mut();
        if select {
            tab.select_to(target_char);
        } else {
            tab.move_to(target_char);
        }
        tab.preferred_column = Some(target.preferred_column);
        target_char != cursor || select
    }

    fn move_line_boundary(&mut self, to_end: bool, select: bool) -> bool {
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

    fn move_document_boundary(&mut self, to_end: bool, select: bool) -> bool {
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
                &tab.buffer,
                Position {
                    line: cursor_line,
                    column: cursor_col,
                },
            );
            tab.move_to(cursor);
        }
        self.sync_find_after_edit();
        self.queue_reveal_cursor();
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
            self.queue_reveal_cursor();
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
                .buffer
                .char_to_line(tab.cursor_char().min(tab.len_chars()));
            (
                preferred_newline_for_active_tab(tab),
                line_indent_prefix(&tab.buffer, line),
            )
        };
        self.replace_text_in_range(None, format!("{newline}{indent}"), UndoBoundary::Break);
    }

    fn copy_selection(&mut self) -> bool {
        let Some(text) = self.active_tab().selected_text() else {
            return false;
        };
        self.queue_effect(EditorEffect::WriteClipboard(text.clone()));
        self.queue_effect(EditorEffect::WritePrimary(text));
        self.status = "Copied selection.".to_string();
        true
    }

    fn cut_selection(&mut self) -> bool {
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

    pub fn handle_vim_key(&mut self, key: vim::Key, mods: vim::Modifiers) -> bool {
        let snapshot = self.vim_snapshot();
        let commands = self.vim.handle_key(&key, mods, &snapshot);
        self.execute_vim_commands(commands)
    }

    pub fn handle_vim_escape(&mut self) -> bool {
        let snapshot = self.vim_snapshot();
        let commands = self
            .vim
            .enter_normal_from_escape(snapshot.cursor, &snapshot);
        self.execute_vim_commands(commands)
    }

    fn execute_vim_commands(&mut self, commands: Vec<vim::VimCommand>) -> bool {
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
            }
        }

        if changed {
            self.queue_reveal_cursor();
            self.queue_primary_selection();
        }
        true
    }

    fn queue_primary_selection(&mut self) {
        if let Some(text) = self.active_tab().selected_text() {
            self.queue_effect(EditorEffect::WritePrimary(text));
        }
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
        self.find.current = index;
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
        self.find.current = index;
        let m = self.find.matches[index];
        Some(Position {
            line: m.line,
            column: m.col,
        })
    }

    fn apply_vim_select(&mut self, anchor: Position, head: Position) {
        let tab = self.active_tab_mut();
        let anchor_char = position_to_char(&tab.buffer, anchor);
        let head_char = position_to_char(&tab.buffer, head);
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

    pub fn apply(&mut self, command: EditorCommand) {
        match command {
            EditorCommand::InsertText(text) => {
                let range = self
                    .active_tab()
                    .marked_range
                    .clone()
                    .unwrap_or_else(|| self.active_tab().selected_range());
                self.active_tab_mut()
                    .edit(EditKind::Insert, UndoBoundary::Break, range, &text);
                self.sync_find_after_edit();
                self.queue_reveal_cursor();
            }
            EditorCommand::ReplaceText {
                range,
                text,
                boundary,
            } => self.replace_text_in_range(range, text, boundary),
            EditorCommand::ReplaceTextFromInput { range, text } => {
                let boundary = if text.chars().any(char::is_whitespace) {
                    UndoBoundary::Break
                } else {
                    UndoBoundary::Merge
                };
                self.replace_text_in_range(range, text, boundary);
            }
            EditorCommand::ReplaceAndMarkText {
                range,
                text,
                selected_range,
            } => self.replace_and_mark_text(range, text, selected_range),
            EditorCommand::ClearMarkedText => {
                self.active_tab_mut().marked_range = None;
            }
            EditorCommand::ToggleWrap => {
                self.show_wrap = !self.show_wrap;
                self.status = if self.show_wrap {
                    "Soft wrap enabled.".to_string()
                } else {
                    "Soft wrap disabled.".to_string()
                };
                self.queue_reveal_cursor();
            }
            EditorCommand::NewTab => {
                let tab = self.new_empty_tab();
                self.push_tab(tab);
                let last = self.tabs.len().saturating_sub(1);
                self.set_active_tab(last);
                self.status = "Created a new tab.".to_string();
                self.queue_focus(FocusTarget::Editor);
            }
            EditorCommand::CloseTab(index) => {
                self.close_tab_at(index);
            }
            EditorCommand::SetActiveTab(index) => {
                if self.set_active_tab(index) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::NextTab => {
                if self.tabs.len() > 1 {
                    self.set_active_tab((self.active + 1) % self.tabs.len());
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::PrevTab => {
                if self.tabs.len() > 1 {
                    let prev = if self.active == 0 {
                        self.tabs.len() - 1
                    } else {
                        self.active - 1
                    };
                    self.set_active_tab(prev);
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::SelectAll => {
                self.active_tab_mut().select_all();
            }
            EditorCommand::MoveHorizontal { delta, select } => {
                self.move_horizontal(delta, select);
                self.queue_reveal_cursor();
            }
            EditorCommand::MoveHorizontalCollapse { backward } => {
                if self.move_horizontal_collapse(backward) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::MoveVertical { delta, select } => {
                if self.move_vertical(delta, select) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::MoveDisplayRows {
                delta,
                select,
                wrap_columns,
            } => {
                if self.move_display_rows(delta, select, wrap_columns) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::MovePage { rows, down, select } => {
                let delta = if down {
                    rows as isize
                } else {
                    -(rows as isize)
                };
                if self.move_vertical(delta, select) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::MoveWord { backward, select } => {
                self.move_word_boundary(backward, select);
                self.queue_reveal_cursor();
            }
            EditorCommand::MoveLineBoundary { to_end, select } => {
                if self.move_line_boundary(to_end, select) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::MoveDocumentBoundary { to_end, select } => {
                if self.move_document_boundary(to_end, select) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::MoveToChar {
                offset,
                select,
                preferred_column,
            } => {
                if self.move_to_char(offset, select, preferred_column) {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::SetSelection { range, reversed } => {
                self.set_selection(range, reversed);
            }
            EditorCommand::Backspace => {
                self.delete_selected_or_previous();
            }
            EditorCommand::DeleteForward => {
                self.delete_selected_or_next();
            }
            EditorCommand::DeleteWord { backward } => {
                self.delete_selected_or_word(backward);
            }
            EditorCommand::InsertNewline => self.insert_newline(),
            EditorCommand::InsertTab => {
                self.replace_text_in_range(None, "    ".to_string(), UndoBoundary::Break);
            }
            EditorCommand::DeleteLine => {
                let pos = self.active_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let line = editor_ops::delete_line(lines, pos.line);
                    Some(((), line, pos.column))
                });
            }
            EditorCommand::MoveLineUp => {
                let pos = self.active_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let line = editor_ops::move_line_up(lines, pos.line)?;
                    Some(((), line, pos.column))
                });
            }
            EditorCommand::MoveLineDown => {
                let pos = self.active_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let line = editor_ops::move_line_down(lines, pos.line)?;
                    Some(((), line, pos.column))
                });
            }
            EditorCommand::DuplicateLine => {
                let pos = self.active_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let line = editor_ops::duplicate_line(lines, pos.line);
                    Some(((), line, pos.column))
                });
            }
            EditorCommand::ToggleComment => {
                let prefix = self
                    .active_tab()
                    .path
                    .as_ref()
                    .and_then(|path| path.extension())
                    .and_then(|ext| editor_ops::comment_prefix(ext.to_string_lossy().as_ref()))
                    .unwrap_or("//");
                let selected = self.active_tab().selected_range();
                let cursor = self.active_cursor_position();
                let start = char_to_position(&self.active_tab().buffer, selected.start);
                let end = char_to_position(&self.active_tab().buffer, selected.end);
                let first = start.line.min(end.line);
                let last = start.line.max(end.line);
                let _ = self.apply_line_edit(|lines| {
                    let (line, col) = editor_ops::toggle_comment(
                        lines,
                        first,
                        last,
                        cursor.line,
                        cursor.column,
                        prefix,
                    );
                    Some(((), line, col))
                });
            }
            EditorCommand::CopySelection => {
                self.copy_selection();
            }
            EditorCommand::CutSelection => {
                self.cut_selection();
            }
            EditorCommand::RequestPaste => self.queue_effect(EditorEffect::ReadClipboard),
            EditorCommand::PasteText(text) => {
                self.replace_text_in_range(None, text.clone(), UndoBoundary::Break);
                self.status = format!("Pasted {} line(s).", text.lines().count());
            }
            EditorCommand::OpenFind { show_replace } => {
                let selected = self.active_tab().selected_text();
                self.open_find(show_replace, selected);
            }
            EditorCommand::ToggleFind { show_replace } => {
                if self.find.visible && self.find.show_replace == show_replace {
                    self.close_find();
                } else {
                    let selected = self.active_tab().selected_text();
                    self.open_find(show_replace, selected);
                }
            }
            EditorCommand::CloseFind => self.close_find(),
            EditorCommand::SetFindQuery(text) => self.set_find_query(text),
            EditorCommand::SetFindQueryAndSelect(text) => self.set_find_query_and_select(text),
            EditorCommand::SetFindReplacement(text) => self.set_find_replacement(text),
            EditorCommand::FindNext => {
                if self.find_next() {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::FindPrev => {
                if self.find_prev() {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::ReplaceOne => {
                if self.replace_one() {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::ReplaceAll => {
                if self.replace_all_matches() {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::OpenGotoLine => self.open_goto_line(),
            EditorCommand::ToggleGotoLine => {
                if self.goto_line.is_some() {
                    self.close_goto_line();
                } else {
                    self.open_goto_line();
                }
            }
            EditorCommand::CloseGotoLine => self.close_goto_line(),
            EditorCommand::SetGotoLine(text) => self.set_goto_line(text),
            EditorCommand::SubmitGotoLine => {
                if self.submit_goto_line() {
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::RequestOpenFiles => self.queue_effect(EditorEffect::OpenFiles),
            EditorCommand::OpenFiles(files) => {
                let start_len = self.tabs.len();
                for (path, text) in files {
                    let id = self.alloc_tab_id();
                    self.tabs.push(EditorTab::from_path(id, path, &text));
                }
                if self.tabs.len() > start_len {
                    self.set_active_tab(self.tabs.len() - 1);
                    self.status = format!("Opened {} tab(s).", self.tabs.len() - start_len);
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::OpenFileFailed { path, message } => {
                self.status = format!("Failed to open {}: {message}", path.display());
            }
            EditorCommand::RequestSave => {
                let body = self.active_tab().buffer_text();
                if let Some(path) = self.active_tab().path.clone() {
                    self.queue_effect(EditorEffect::SaveFile { path, body });
                } else {
                    self.queue_effect(EditorEffect::SaveFileAs {
                        suggested_name: self.active_tab().display_name(),
                        body,
                    });
                }
            }
            EditorCommand::RequestSaveAs => {
                self.queue_effect(EditorEffect::SaveFileAs {
                    suggested_name: self.active_tab().display_name(),
                    body: self.active_tab().buffer_text(),
                });
            }
            EditorCommand::SaveFinished { path } => {
                let tab = self.active_tab_mut();
                tab.path = Some(path.clone());
                tab.modified = false;
                self.status = format!("Saved {}.", path.display());
            }
            EditorCommand::SaveFailed { path, message } => {
                self.status = format!("Failed to save {}: {message}", path.display());
            }
            EditorCommand::AutosaveTick => {
                let jobs = self
                    .tabs
                    .iter()
                    .filter(|tab| tab.modified)
                    .filter_map(|tab| {
                        let path = tab.path.clone()?;
                        let open_tabs_for_path = self
                            .tabs
                            .iter()
                            .filter(|candidate| candidate.path.as_ref() == Some(&path))
                            .take(2)
                            .count();
                        if open_tabs_for_path != 1 {
                            return None;
                        }
                        Some((path, tab.buffer_text(), tab.revision()))
                    })
                    .collect::<Vec<_>>();
                for (path, body, revision) in jobs {
                    self.queue_effect(EditorEffect::AutosaveFile {
                        path,
                        body,
                        revision,
                    });
                }
            }
            EditorCommand::AutosaveFinished { path, revision } => {
                for tab in &mut self.tabs {
                    if tab.path.as_ref() == Some(&path) && tab.revision() == revision {
                        tab.modified = false;
                    }
                }
                if self.active_tab().path.as_ref() == Some(&path)
                    && self.active_tab().revision() == revision
                {
                    self.status = format!("Autosaved {}.", path.display());
                }
            }
            EditorCommand::AutosaveFailed { path, message } => {
                if self.active_tab().path.as_ref() == Some(&path) {
                    self.status = format!("Autosave failed for {}: {message}", path.display());
                }
            }
            EditorCommand::Undo => {
                if self.active_tab_mut().undo() {
                    self.sync_find_after_edit();
                    self.queue_reveal_cursor();
                }
            }
            EditorCommand::Redo => {
                if self.active_tab_mut().redo() {
                    self.sync_find_after_edit();
                    self.queue_reveal_cursor();
                }
            }
        }
    }
}

pub fn next_active_after_tab_close(len: usize, active_index: usize, closed_index: usize) -> usize {
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

pub fn should_refocus_editor_after_tab_close(active_index: usize, closed_index: usize) -> bool {
    active_index == closed_index
}

fn display_line_char_len(tab: &EditorTab, line_ix: usize) -> usize {
    tab.buffer
        .line(line_ix.min(tab.buffer.len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count()
}

fn preferred_newline_for_active_tab(tab: &EditorTab) -> &'static str {
    let mut chars = tab.buffer.chars().peekable();
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
    let line = position.line.min(tab.buffer.len_lines().saturating_sub(1));
    let line_start = tab.buffer.line_to_char(line);
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub active: usize,
    pub tab_count: usize,
    pub active_tab_id: TabId,
    pub tab_ids: Vec<TabId>,
    pub tab_titles: Vec<String>,
    pub tab_modified: Vec<bool>,
    pub text: String,
    pub cursor: usize,
    pub cursor_position: Position,
    pub selection: Range<usize>,
    pub active_path: Option<PathBuf>,
    pub active_revision: u64,
    pub show_wrap: bool,
    pub show_gutter: bool,
    pub find_visible: bool,
    pub find_show_replace: bool,
    pub find_query: String,
    pub find_replacement: String,
    pub find_matches: usize,
    pub find_current: usize,
    pub goto_line: Option<String>,
    pub vim_mode: vim::Mode,
    pub vim_pending: String,
    pub status: String,
}

impl EditorModel {
    pub fn snapshot(&self) -> EditorSnapshot {
        let active = self.active_tab();
        EditorSnapshot {
            active: self.active,
            tab_count: self.tabs.len(),
            active_tab_id: active.id(),
            tab_ids: self.tabs.iter().map(EditorTab::id).collect(),
            tab_titles: self.tabs.iter().map(|tab| tab.display_name()).collect(),
            tab_modified: self.tabs.iter().map(|tab| tab.modified).collect(),
            text: active.buffer_text(),
            cursor: active.cursor_char(),
            cursor_position: active.cursor_position(),
            selection: active.selected_range(),
            active_path: active.path.clone(),
            active_revision: active.revision(),
            show_wrap: self.show_wrap,
            show_gutter: self.show_gutter,
            find_visible: self.find.visible,
            find_show_replace: self.find.show_replace,
            find_query: self.find.query.clone(),
            find_replacement: self.find.replacement.clone(),
            find_matches: self.find.matches.len(),
            find_current: self.find.current,
            goto_line: self.goto_line.clone(),
            vim_mode: self.vim.mode,
            vim_pending: self.vim.pending_display(),
            status: self.status.clone(),
        }
    }
}
