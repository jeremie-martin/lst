mod document;
mod effect;
pub mod find;
mod history;
pub mod language;
mod line_edit;
mod model_io;
mod multi_selection;
pub mod position;
pub mod selection;
mod selection_edit;
mod snapshot;
mod tab;
mod tab_set;
mod text_input;
mod transaction;
pub mod viewport;
pub mod vim;
mod vim_edit;
pub mod wrap;

pub use document::{EditKind, UndoBoundary};
pub use effect::{EditorEffect, FocusTarget, RevealIntent};
pub use language::{IndentStyle, Language, LanguageConfig};
pub use selection::{CursorGoal, Selection, SelectionSet, SelectionSetError, SelectionState};
pub use snapshot::EditorSnapshot;
pub use tab::{EditorTab, FileStamp, TabId};
pub use viewport::Viewport;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GutterMode {
    #[default]
    Absolute,
    Relative,
    Hybrid,
}

impl GutterMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Absolute => "Absolute",
            Self::Relative => "Relative",
            Self::Hybrid => "Hybrid",
        }
    }

    fn cycle(self) -> Self {
        match self {
            Self::Absolute => Self::Relative,
            Self::Relative => Self::Hybrid,
            Self::Hybrid => Self::Absolute,
        }
    }

    /// Formats the gutter label for `line_ix`. `cursor_line` anchors
    /// Relative mode (distance is measured from the primary cursor's
    /// line); `cursor_lines` lists every line that hosts a cursor head
    /// (sorted, may contain just `cursor_line`) so Hybrid mode can mark
    /// each multi-cursor row with its absolute number.
    pub fn format(self, line_ix: usize, cursor_line: usize, cursor_lines: &[usize]) -> String {
        match self {
            Self::Absolute => format!("{:>3}", line_ix + 1),
            Self::Relative => format!("{:>3}", line_ix.abs_diff(cursor_line)),
            Self::Hybrid if cursor_lines.contains(&line_ix) => format!("{:>3}", line_ix + 1),
            Self::Hybrid => format!("{:>3}", line_ix.abs_diff(cursor_line)),
        }
    }
}

use crate::{
    document::{
        char_to_position, inclusive_position_to_exclusive_char, line_indent_prefix,
        position_to_char,
    },
    find::{FindScope, FindState, MatchPos},
    position::Position,
    selection::{
        char_at_line_column, display_line_char_len as buffer_display_line_char_len,
        line_range_at_char, next_grapheme_boundary, next_subword_boundary, next_word_boundary,
        paragraph_range_at_char, previous_grapheme_boundary, previous_subword_boundary,
        previous_word_boundary, word_range_at_char, SelectionTransform,
    },
    tab_set::TabSet,
    transaction::{EditOutcome, EditRequest, SelectionAfter},
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
    gutter_mode: GutterMode,
    find: FindState,
    goto_line: Option<String>,
    status: String,
    vim: vim::VimState,
    viewport: Viewport,
    effects: Vec<EditorEffect>,
    /// Overtype mode replaces the character to the right of the caret on
    /// each printable insert instead of pushing it forward. Persists across
    /// motion until toggled off.
    overtype: bool,
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
            gutter_mode: GutterMode::Absolute,
            find: FindState::new(),
            goto_line: None,
            status,
            vim: vim::VimState::new(),
            viewport: Viewport::default(),
            effects: Vec::new(),
            overtype: false,
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

    pub fn gutter_mode(&self) -> GutterMode {
        self.gutter_mode
    }

    pub fn cycle_gutter_mode(&mut self) {
        self.gutter_mode = self.gutter_mode.cycle();
        self.status = format!("Line numbers: {}", self.gutter_mode.label());
    }

    pub fn find(&self) -> &FindState {
        &self.find
    }

    pub fn find_match_ranges(&self) -> Vec<Range<usize>> {
        let buffer = self.active_tab().buffer();
        self.find
            .matches
            .iter()
            .copied()
            .map(|m| m.char_range_in(buffer))
            .collect()
    }

    pub fn active_find_match_range(&self) -> Option<Range<usize>> {
        let active = self.find.active?;
        let m = *self.find.matches.get(active)?;
        Some(m.char_range_in(self.active_tab().buffer()))
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

    pub fn overtype(&self) -> bool {
        self.overtype
    }

    /// Toggle overtype mode. When on, plain printable insertion replaces
    /// the char to the right of the caret instead of pushing it forward.
    pub fn toggle_overtype(&mut self) {
        self.overtype = !self.overtype;
        self.status = if self.overtype {
            "Overtype on.".to_string()
        } else {
            "Overtype off.".to_string()
        };
    }

    fn new_empty_tab(&mut self) -> EditorTab {
        let name = format!("{UNTITLED_PREFIX}-{}", self.next_untitled_id);
        self.next_untitled_id += 1;
        let id = self.alloc_tab_id();
        EditorTab::empty(id, name)
    }

    fn activate_tab(&mut self, index: usize) -> bool {
        if !self.tabs.activate(index) {
            return false;
        }
        self.active_tab_changed();
        self.status = format!("Switched to {}.", self.active_tab().display_name());
        true
    }

    fn active_tab_changed(&mut self) {
        self.vim.on_tab_switch();
        self.active_tab_mut().clear_preferred_column();
        self.sync_find_with_active_document();
    }

    pub fn move_to_char(&mut self, offset: usize, select: bool, preferred_column: Option<usize>) {
        let end = self.active_tab().len_chars();
        let target = offset.min(end);
        let cursor = self.active_tab().cursor_char();
        {
            let tab = self.active_tab_mut();
            if select {
                tab.select_to(target);
            } else {
                tab.move_to(target);
            }
            tab.set_preferred_column(preferred_column);
        }
        if target != cursor || select {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    fn assign_selection(&mut self, selection: Selection) {
        self.active_tab_mut().set_selection(selection);
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

    pub fn toggle_find_case_sensitive(&mut self) {
        self.find.case_sensitive = !self.find.case_sensitive;
        self.reindex_find_matches_to_nearest();
    }

    pub fn toggle_find_whole_word(&mut self) {
        self.find.whole_word = !self.find.whole_word;
        self.reindex_find_matches_to_nearest();
    }

    pub fn toggle_find_regex(&mut self) {
        self.find.use_regex = !self.find.use_regex;
        self.reindex_find_matches_to_nearest();
    }

    // No-op when toggling on without a selection (UI grays the chip).
    pub fn toggle_find_in_selection(&mut self) {
        let active_tab_id = self.active_tab_id();
        if self.find.scope.is_selection_for(active_tab_id) {
            self.find.scope = FindScope::Document;
            self.reindex_find_matches_to_nearest();
            return;
        }

        let sel = self.active_tab().selected_range();
        if sel.start < sel.end {
            self.find.scope = FindScope::Selection {
                tab_id: active_tab_id,
                start_char: sel.start,
                end_char: sel.end,
            };
            self.reindex_find_matches_to_nearest();
        }
    }

    pub fn update_goto_line(&mut self, text: String) {
        self.goto_line = Some(text);
    }

    fn active_cursor_position(&self) -> Position {
        self.active_tab().cursor_position()
    }

    fn reindex_find_matches(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
            return;
        }
        let text = self.active_tab().buffer_text();
        self.find.compute_matches_in_text(&text);
        if let Some(scope) = self.find.scope.selection_range_for(self.active_tab_id()) {
            let buffer = self.active_tab().buffer();
            let buffer_len = buffer.len_chars();
            let scope_start = scope.start.min(buffer_len);
            let scope_end = scope.end.min(buffer_len);
            let kept: Vec<MatchPos> = self
                .find
                .matches
                .iter()
                .copied()
                .filter(|m| {
                    let r = m.char_range_in(buffer);
                    r.start >= scope_start && r.end <= scope_end
                })
                .collect();
            self.find.matches = kept;
            if self.find.matches.is_empty() {
                self.find.active = None;
            } else if let Some(idx) = self.find.active {
                self.find.active = Some(idx.min(self.find.matches.len() - 1));
            }
        }
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
        if self.find.is_stale(self.active_tab().revision()) {
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

    fn finish_active_text_mutation(&mut self, outcome: EditOutcome, reveal: Option<RevealIntent>) {
        if outcome.text_changed {
            self.sync_find_after_edit();
        }
        if outcome.changed() {
            if let Some(intent) = reveal {
                self.queue_reveal(intent);
            }
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
        let Some(request) = find::replace_one_request(self.active_tab(), &self.find) else {
            return false;
        };
        self.apply_active_edit_request(request, None);
        self.move_to_current_find_match();
        self.queue_reveal(RevealIntent::Center);
        true
    }

    fn replace_all_matches(&mut self) -> bool {
        self.reindex_find_matches();
        let cursor = self.active_cursor_position();
        let Some(request) = find::replace_all_request(self.active_tab(), &self.find, cursor) else {
            return false;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::Center));
        true
    }

    pub fn submit_goto_line_input(&mut self) {
        let Some(text) = self.goto_line.clone() else {
            return;
        };
        let trimmed = text.trim();
        let (line_text, column_text) = match trimmed.split_once([':', ';']) {
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
            self.active_tab_changed();
            self.queue_focus(FocusTarget::Editor);
            self.status = "Closed tab.".to_string();
            return true;
        }

        if self.tabs.remove(index) {
            self.active_tab_changed();
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

    fn apply_single_text_edit(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        text: impl Into<String>,
    ) {
        let request = EditRequest::single(kind, boundary, range, text.into());
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn apply_active_edit_request(
        &mut self,
        request: EditRequest,
        reveal: Option<RevealIntent>,
    ) -> EditOutcome {
        let outcome = self.active_tab_mut().apply_edit_request(request);
        self.finish_active_text_mutation(outcome, reveal);
        outcome
    }

    fn apply_multi_selection_replacement(&mut self, text: String, boundary: UndoBoundary) -> bool {
        let Some(request) = multi_selection::replacement_request(self.active_tab(), text, boundary)
        else {
            return false;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        true
    }

    /// Programmatic replacement entry point. Multi-cursor distributes the same
    /// `text` to every selection; auto-pair semantics are intentionally NOT
    /// applied here (callers like `paste_text`, `insert_newline`, and
    /// `insert_tab_at_cursor` want literal insertion). Keyboard input goes
    /// through `apply_text_input` instead, which dispatches multi-cursor
    /// auto-pair handlers before falling through to the same plain replacement.
    pub fn replace_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    ) {
        if range.is_none() && self.apply_multi_selection_replacement(text.clone(), boundary) {
            return;
        }
        let range = text_input::resolve_range(self.active_tab(), range);
        let kind = if text.is_empty() {
            EditKind::Delete
        } else {
            EditKind::Insert
        };
        self.apply_single_text_edit(kind, boundary, range, text);
    }

    pub fn replace_and_mark_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    ) {
        let request =
            text_input::marked_text_request(self.active_tab(), range, text, selected_range);
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn delete_selection_or_word_range(tab: &EditorTab, backward: bool) -> Option<Range<usize>> {
        if tab.has_selection() {
            return Some(tab.selected_range());
        }
        delete_word_range_at(tab, tab.cursor_char(), backward)
    }

    fn delete_selected_or_word(&mut self, backward: bool) -> bool {
        if self.active_tab().selection_set().has_multiple() {
            return self.apply_multi_selection_delete(UndoBoundary::Break, |tab, cursor| {
                delete_word_range_at(tab, cursor, backward)
            });
        }
        let Some(range) = Self::delete_selection_or_word_range(self.active_tab(), backward) else {
            return false;
        };
        self.apply_single_text_edit(EditKind::Delete, UndoBoundary::Break, range, "");
        true
    }

    fn insert_newline(&mut self) {
        let newline = preferred_newline_for_active_tab(self.active_tab());

        // Multi-cursor: each cursor inherits its own line's indent.
        // `replacement_request_by_index` returns `None` during IME
        // composition or for single-selection sets, so the
        // `if let Some(request)` falls through to single-cursor in those
        // cases without us re-checking those conditions here.
        let replacements: Vec<String> = {
            let tab = self.active_tab();
            let buffer = tab.buffer();
            let len_chars = tab.len_chars();
            tab.selection_set()
                .as_slice()
                .iter()
                .map(|selection| {
                    // Sample indent from the selection's range start —
                    // that's where the newline lands after the selection
                    // is deleted, regardless of cursor direction. Using
                    // `cursor()` would pull the wrong line for reverse-
                    // direction selections.
                    let line = buffer.char_to_line(selection.range().start.min(len_chars));
                    format!("{newline}{}", line_indent_prefix(buffer, line))
                })
                .collect()
        };
        let request = multi_selection::replacement_request_by_index(
            self.active_tab(),
            |index| replacements[index].clone(),
            UndoBoundary::Break,
        );
        if let Some(request) = request {
            self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
            return;
        }

        let primary_replacement = replacements
            .get(self.active_tab().selection_set().primary_index())
            .cloned()
            .unwrap_or_else(|| newline.to_string());
        self.replace_text(None, primary_replacement, UndoBoundary::Break);
    }

    fn apply_selection_motion<F>(&mut self, preferred_column: Option<usize>, mut motion: F) -> bool
    where
        F: FnMut(&EditorTab, Selection) -> Selection,
    {
        let Some((before, after)) = (|| {
            let tab = self.active_tab();
            let before = tab.selection_state().clone();
            let goal = preferred_column.map(CursorGoal::Column);
            let after = before.map(|_, selection| {
                let selection = motion(tab, selection);
                match goal {
                    Some(goal) => SelectionTransform::with_columns(
                        selection,
                        goal,
                        (!selection.has_selection()).then_some(preferred_column.unwrap()),
                    ),
                    None => SelectionTransform::new(selection),
                }
            })?;
            Some((before, after))
        })() else {
            return false;
        };
        if after == before {
            return false;
        }

        self.active_tab_mut().set_selection_state(after);
        true
    }

    fn apply_selection_motion_with_columns<F>(&mut self, mut motion: F) -> bool
    where
        F: FnMut(&EditorTab, usize, Selection) -> SelectionMotion,
    {
        let Some((before, after)) = (|| {
            let tab = self.active_tab();
            let before = tab.selection_state().clone();
            let after = before.map(|index, selection| {
                let motion = motion(tab, index, selection);
                SelectionTransform::with_columns(
                    motion.selection,
                    motion
                        .movement_goal
                        .expect("selection motion must carry a movement goal"),
                    motion.visible_column,
                )
            })?;
            Some((before, after))
        })() else {
            return false;
        };
        if after == before {
            return false;
        }

        self.active_tab_mut().set_selection_state(after);
        true
    }

    fn move_horizontal(&mut self, delta: isize, select: bool) -> bool {
        self.apply_selection_motion(None, |tab, selection| {
            let mut target = if !select && selection.has_selection() {
                let range = selection.range();
                if delta.is_negative() {
                    range.start
                } else {
                    range.end
                }
            } else {
                selection.cursor()
            };
            for _ in 0..delta.unsigned_abs() {
                let next = if delta.is_negative() {
                    previous_grapheme_boundary(tab.buffer(), target)
                } else {
                    next_grapheme_boundary(tab.buffer(), target)
                };
                if next == target {
                    break;
                }
                target = next;
            }
            if select {
                Selection::new(selection.anchor(), target)
            } else {
                Selection::collapsed(target)
            }
        })
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
            tab.clear_preferred_column();
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
        self.apply_selection_motion(None, |tab, selection| {
            let target = if !select && selection.has_selection() {
                let range = selection.range();
                if backward {
                    range.start
                } else {
                    range.end
                }
            } else if backward {
                prev_fn(tab.buffer(), selection.cursor())
            } else {
                next_fn(tab.buffer(), selection.cursor())
            };
            if select {
                Selection::new(selection.anchor(), target)
            } else {
                Selection::collapsed(target)
            }
        })
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
        tab.set_preferred_column(Some(preferred_column));
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
        if self.active_tab().selection_set().has_multiple() {
            return self.apply_selection_motion_with_columns(|tab, index, selection| {
                let position = char_to_position(tab.buffer(), selection.cursor());
                let goal = tab
                    .preferred_goal_for_selection(index)
                    .unwrap_or(CursorGoal::Column(position.column));
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
                    let target_column = goal.resolve(display_line_char_len(tab, target_line));
                    tab.buffer().line_to_char(target_line) + target_column
                };
                let target_position = char_to_position(tab.buffer(), target);
                if select {
                    let selection = Selection::new(selection.anchor(), target);
                    SelectionMotion::with_goal(
                        selection,
                        goal,
                        visible_column_for_motion(selection, goal, target_position.column),
                    )
                } else {
                    let selection = Selection::collapsed(target);
                    SelectionMotion::with_goal(
                        selection,
                        goal,
                        visible_column_for_motion(selection, goal, target_position.column),
                    )
                }
            });
        }

        let (target, preferred) = {
            let tab = self.active_tab();
            let position = tab.cursor_position();
            let preferred = tab.preferred_column().unwrap_or(position.column);
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

        if self.active_tab().selection_set().has_multiple() {
            let lines = self.active_tab_lines();
            let layout = wrap::build_wrap_layout(lines.as_ref(), wrap_columns, true);
            return self.apply_selection_motion_with_columns(|tab, index, selection| {
                let position = char_to_position(tab.buffer(), selection.cursor());
                let goal = tab
                    .preferred_goal_for_selection(index)
                    .unwrap_or(CursorGoal::Column(position.column));
                let preferred = match goal {
                    CursorGoal::Column(column) => column,
                    CursorGoal::LineEnd => display_line_char_len(tab, position.line),
                };
                let row_target = wrap::display_row_target(
                    lines.as_ref(),
                    position.line,
                    position.column,
                    Some(preferred),
                    delta,
                    &layout,
                );
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
                }
                .unwrap_or_else(|| selection.cursor());
                let target_position = char_to_position(tab.buffer(), target);
                if select {
                    let selection = Selection::new(selection.anchor(), target);
                    SelectionMotion::with_goal(
                        selection,
                        goal,
                        visible_column_for_motion(selection, goal, target_position.column),
                    )
                } else {
                    let selection = Selection::collapsed(target);
                    SelectionMotion::with_goal(
                        selection,
                        goal,
                        visible_column_for_motion(selection, goal, target_position.column),
                    )
                }
            });
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
                tab.preferred_column(),
                delta,
                &layout,
            );
            let preferred = row_target
                .map(|target| target.preferred_column)
                .or(tab.preferred_column())
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
                tab.preferred_column(),
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
        tab.set_preferred_column(Some(preferred_column));
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
        if self.apply_selection_motion(None, |tab, selection| {
            let line = tab
                .buffer()
                .char_to_line(selection.cursor().min(tab.len_chars()));
            let target = if to_end {
                tab.buffer().line_to_char(line) + display_line_char_len(tab, line)
            } else {
                tab.buffer().line_to_char(line)
            };
            if select {
                Selection::new(selection.anchor(), target)
            } else {
                Selection::collapsed(target)
            }
        }) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn smart_home(&mut self, select: bool) {
        if self.apply_selection_motion(None, |tab, selection| {
            let cursor = selection.cursor();
            let line = tab.buffer().char_to_line(cursor.min(tab.len_chars()));
            let line_start = tab.buffer().line_to_char(line);
            let first_non_blank = line_start + first_non_blank_column(tab, line);
            let target = if cursor == first_non_blank {
                line_start
            } else {
                first_non_blank
            };
            if select {
                Selection::new(selection.anchor(), target)
            } else {
                Selection::collapsed(target)
            }
        }) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn move_document_boundary(&mut self, to_end: bool, select: bool) {
        let target = if to_end {
            self.active_tab().len_chars()
        } else {
            0
        };
        if self.apply_selection_motion(None, |_tab, selection| {
            if select {
                Selection::new(selection.anchor(), target)
            } else {
                Selection::collapsed(target)
            }
        }) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
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

    fn delete_selected_or_previous(&mut self) -> bool {
        if self.active_tab().selection_set().has_multiple() {
            return self.apply_multi_selection_delete(UndoBoundary::Merge, |tab, cursor| {
                if cursor == 0 {
                    return None;
                }
                Some(
                    soft_tab_backspace_range_at(tab, cursor).unwrap_or_else(|| {
                        previous_grapheme_boundary(tab.buffer(), cursor)..cursor
                    }),
                )
            });
        }
        let range = {
            let tab = self.active_tab();
            if tab.has_selection() {
                tab.selected_range()
            } else {
                let cursor = tab.cursor_char();
                if cursor == 0 {
                    return false;
                }
                soft_tab_backspace_range(tab)
                    .unwrap_or_else(|| previous_grapheme_boundary(tab.buffer(), cursor)..cursor)
            }
        };
        self.apply_single_text_edit(EditKind::Delete, UndoBoundary::Merge, range, "");
        true
    }

    fn apply_multi_selection_delete<F>(&mut self, boundary: UndoBoundary, cursor_range: F) -> bool
    where
        F: Fn(&EditorTab, usize) -> Option<Range<usize>>,
    {
        let Some(request) = ({
            let tab = self.active_tab();
            multi_selection::delete_request(tab, boundary, cursor_range)
        }) else {
            return false;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        true
    }

    fn delete_selected_or_next(&mut self) -> bool {
        if self.active_tab().selection_set().has_multiple() {
            return self.apply_multi_selection_delete(UndoBoundary::Merge, |tab, cursor| {
                (cursor < tab.len_chars())
                    .then(|| cursor..next_grapheme_boundary(tab.buffer(), cursor))
            });
        }
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
        self.apply_single_text_edit(EditKind::Delete, UndoBoundary::Merge, range, "");
        true
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
        if let Some(text) = multi_selection::selected_text_joined(self.active_tab()) {
            if text.is_empty() {
                return;
            }
            self.queue_clipboard_copy(text);
            self.status = "Copied selections.".to_string();
            return;
        }

        let (_range, text, whole_line) = self.selection_or_current_line();
        if text.is_empty() {
            return;
        }
        self.queue_clipboard_copy(text);
        self.status = if whole_line {
            "Copied line.".to_string()
        } else {
            "Copied selection.".to_string()
        };
    }

    pub fn cut_selection(&mut self) {
        if let Some(text) = multi_selection::selected_text_joined(self.active_tab()) {
            if text.is_empty() {
                return;
            }
            self.queue_clipboard_copy(text);
            let deleted =
                self.apply_multi_selection_delete(UndoBoundary::Break, |_tab, _cursor| None);
            if deleted {
                self.status = "Cut selections.".to_string();
            }
            return;
        }

        let (range, text, whole_line) = self.selection_or_current_line();
        if text.is_empty() {
            return;
        }
        self.queue_clipboard_copy(text);
        self.apply_single_text_edit(EditKind::Delete, UndoBoundary::Break, range, "");
        self.status = if whole_line {
            "Cut line.".to_string()
        } else {
            "Cut selection.".to_string()
        };
    }

    fn queue_clipboard_copy(&mut self, text: String) {
        self.queue_effect(EditorEffect::WriteClipboard(text.clone()));
        self.queue_effect(EditorEffect::WritePrimary(text));
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
                    self.undo_active_text_mutation(None);
                    changed = true;
                }
                vim::VimCommand::Redo => {
                    self.redo_active_text_mutation(None);
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
                    // Vim `*` / `#` are whole-word, case-sensitive, literal.
                    self.find.whole_word = true;
                    self.find.case_sensitive = true;
                    self.find.use_regex = false;
                    self.find.scope = FindScope::Document;
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
                vim::VimCommand::SurroundRange {
                    from,
                    to,
                    open,
                    close,
                } => {
                    self.vim_surround_range(from, to, open, close);
                    changed = true;
                }
                vim::VimCommand::DeleteSurround { open } => {
                    self.vim_delete_surround(open);
                    changed = true;
                }
                vim::VimCommand::ChangeSurround { from_open, to_open } => {
                    self.vim_change_surround(from_open, to_open);
                    changed = true;
                }
                vim::VimCommand::JumpToLastEdit { enter_insert } => {
                    if let Some(target) = self.active_tab().last_edit_position() {
                        self.active_tab_mut().move_to(target);
                        if enter_insert {
                            self.vim.mode = vim::Mode::Insert;
                        }
                        changed = true;
                    }
                }
                vim::VimCommand::IndentLines { first, last } => {
                    self.indent_selected_lines(first, last);
                    changed = true;
                }
                vim::VimCommand::OutdentLines { first, last } => {
                    self.outdent_selected_lines(first, last);
                    changed = true;
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
        let anchor_end = inclusive_position_to_exclusive_char(tab.buffer(), anchor);
        let head_end = inclusive_position_to_exclusive_char(tab.buffer(), head);
        if (head.line, head.column) < (anchor.line, anchor.column) {
            tab.set_selection(Selection::from_range(
                head_char..anchor_end.max(head_char),
                true,
            ));
        } else {
            tab.set_selection(Selection::from_range(
                anchor_char..head_end.max(anchor_char),
                false,
            ));
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
        let Some((deleted, request)) = vim_edit::delete_range(self.active_tab(), from, to) else {
            return String::new();
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        deleted
    }

    fn vim_delete_lines(&mut self, first: usize, last: usize) -> String {
        let Some((deleted, request)) = vim_edit::delete_lines(self.active_tab(), first, last)
        else {
            return String::new();
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        deleted
    }

    fn vim_change_lines(&mut self, first: usize, last: usize) -> String {
        let Some((deleted, request)) = vim_edit::change_lines(self.active_tab(), first, last)
        else {
            return String::new();
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        deleted
    }

    fn vim_surround_range(&mut self, from: Position, to: Position, open: char, close: char) {
        let Some(request) = vim_edit::surround_range(self.active_tab(), from, to, open, close)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_delete_surround(&mut self, open: char) {
        let (open, close) =
            vim::surround_pair_for_char(open).expect("validated by resolve_surround");
        let snapshot = self.vim_snapshot();
        let Some((open_pos, close_pos)) = vim::find_surround_pair(&snapshot, open, close) else {
            self.status = "No surrounding pair.".to_string();
            return;
        };
        let Some(request) = vim_edit::delete_surround(self.active_tab(), open_pos, close_pos)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_change_surround(&mut self, from_open: char, to_open: char) {
        let (from_open, from_close) =
            vim::surround_pair_for_char(from_open).expect("validated by resolve_surround");
        let (to_open, to_close) =
            vim::surround_pair_for_char(to_open).expect("validated by resolve_surround");
        let snapshot = self.vim_snapshot();
        let Some((open_pos, close_pos)) = vim::find_surround_pair(&snapshot, from_open, from_close)
        else {
            self.status = "No surrounding pair.".to_string();
            return;
        };
        let Some(request) =
            vim_edit::change_surround(self.active_tab(), open_pos, close_pos, to_open, to_close)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_extract_range(&mut self, from: Position, to: Position) -> String {
        vim_edit::extract_range(self.active_tab(), from, to)
    }

    fn vim_extract_lines(&mut self, first: usize, last: usize) -> String {
        vim_edit::extract_lines(self.active_tab(), first, last)
    }

    fn vim_paste(&mut self, before: bool) {
        let cursor = self.active_cursor_position();
        let Some(request) = vim_edit::paste(self.active_tab(), cursor, &self.vim.register, before)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_open_line(&mut self, above: bool) {
        let pos = self.active_cursor_position();
        let Some(request) = vim_edit::open_line(self.active_tab(), pos, above) else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_join_lines(&mut self, count: usize) {
        let pos = self.active_cursor_position();
        let Some(request) = vim_edit::join_lines(self.active_tab(), pos, count) else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_replace_char(&mut self, ch: char, count: usize) {
        let pos = self.active_cursor_position();
        let Some(request) = vim_edit::replace_char(self.active_tab(), pos, ch, count) else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn vim_transform_case_range(&mut self, from: Position, to: Position, uppercase: bool) {
        match vim_edit::transform_case_range(self.active_tab(), from, to, uppercase) {
            Some(vim_edit::VimEditAction::Edit(request)) => {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
            }
            Some(vim_edit::VimEditAction::MoveCursor(position)) => {
                self.move_active_cursor(position.line, position.column, false);
                self.queue_reveal(RevealIntent::NearestEdge);
            }
            None => {}
        }
    }

    fn vim_transform_case_lines(&mut self, first: usize, last: usize, uppercase: bool) {
        match vim_edit::transform_case_lines(self.active_tab(), first, last, uppercase) {
            vim_edit::VimEditAction::Edit(request) => {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
            }
            vim_edit::VimEditAction::MoveCursor(position) => {
                self.move_active_cursor(position.line, position.column, false);
                self.queue_reveal(RevealIntent::NearestEdge);
            }
        }
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
        self.active_tab_mut().clear_marked_range();
    }

    /// EOL fall-through (the cursor is past the last char) keeps users
    /// from getting stuck unable to extend a line. Multi-cursor and IME
    /// composition skip overtype intentionally.
    fn expand_range_for_overtype(
        &self,
        range: Option<Range<usize>>,
        text: &str,
    ) -> Option<Range<usize>> {
        if !self.overtype {
            return range;
        }
        if text.is_empty() || text.contains('\n') || text.contains('\r') {
            return range;
        }
        let tab = self.active_tab();
        if tab.selection_set().has_multiple() || tab.marked_range().is_some() {
            return range;
        }
        let resolved = text_input::resolve_range(tab, range.clone());
        if resolved.start != resolved.end {
            return range;
        }
        let buffer = tab.buffer();
        let cursor = resolved.start;
        let line = buffer.char_to_line(cursor);
        let line_start = buffer.line_to_char(line);
        let line_chars = display_line_char_len(tab, line);
        if cursor >= line_start + line_chars {
            // EOL: no char to overwrite, fall back to insert so the user
            // can still extend the line.
            return range;
        }
        let next = selection::next_grapheme_boundary(buffer, cursor);
        Some(cursor..next)
    }

    fn apply_text_input(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        boundary: UndoBoundary,
    ) {
        let range = self.expand_range_for_overtype(range, &text);
        match text_input::edit_action(self.active_tab(), range, text, boundary) {
            text_input::TextInputAction::MoveCursor(new_cursor) => {
                self.assign_selection(Selection::collapsed(new_cursor));
                self.queue_reveal(RevealIntent::NearestEdge);
            }
            text_input::TextInputAction::Edit {
                request,
                align_find_current,
            } => {
                let outcome = self.apply_active_edit_request(request, None);
                if align_find_current && outcome.text_changed {
                    self.align_find_current_to_visible_match();
                }
                self.queue_reveal(RevealIntent::NearestEdge);
            }
        }
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
        let index = self.tabs.push(tab);
        self.activate_tab(index);
        self.status = "Created a new tab.".to_string();
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn new_scratchpad_tab(&mut self, path: PathBuf, file_stamp: FileStamp) {
        let id = self.alloc_tab_id();
        let tab = EditorTab::scratchpad_with_stamp(id, path, file_stamp);
        let index = self.tabs.push(tab);
        self.activate_tab(index);
        self.status = "Created a new scratchpad.".to_string();
        self.queue_focus(FocusTarget::Editor);
    }

    pub fn close_request_for_tab(&self, tab_id: TabId) -> Option<TabCloseRequest> {
        let tab = self.tab_by_id(tab_id)?;
        if tab.modified() {
            Some(TabCloseRequest::SaveAndClose { tab_id: tab.id() })
        } else {
            Some(TabCloseRequest::Close { tab_id: tab.id() })
        }
    }

    pub fn first_dirty_tab_index(&self) -> Option<usize> {
        self.tabs.iter().position(EditorTab::modified)
    }

    pub fn tab_id_at(&self, index: usize) -> Option<TabId> {
        self.tabs.get(index).map(EditorTab::id)
    }

    pub fn close_clean_tab(&mut self, tab_id: TabId) -> bool {
        let Some(index) = self.tab_index_by_id(tab_id) else {
            return false;
        };
        if self.tabs[index].modified() {
            return false;
        }
        self.close_tab_at_unchecked(index)
    }

    pub fn discard_close_tab(&mut self, tab_id: TabId) -> bool {
        let Some(index) = self.tab_index_by_id(tab_id) else {
            return false;
        };
        self.close_tab_at_unchecked(index)
    }

    pub fn set_active_tab(&mut self, tab_id: TabId) {
        let Some(index) = self.tab_index_by_id(tab_id) else {
            return;
        };
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

    pub fn move_tab(&mut self, from: usize, to: usize) {
        if self.tabs.reorder(from, to) {
            self.status = format!("Reordered tab to position {}.", to + 1);
        }
    }

    pub fn move_active_tab(&mut self, delta: isize) {
        let len = self.tabs.len();
        if len < 2 {
            return;
        }
        let from = self.active_index();
        let to = (from as isize + delta).rem_euclid(len as isize) as usize;
        self.move_tab(from, to);
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

    pub fn smart_expand_selection(&mut self) {
        if self.apply_selection_motion(None, |tab, selection| {
            smart_expanded_selection(tab.buffer(), selection)
        }) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn smart_shrink_selection(&mut self) {
        if self.apply_selection_motion(None, |tab, selection| {
            smart_shrunk_selection(tab.buffer(), selection)
        }) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn set_selection(&mut self, selection: Selection) {
        self.assign_selection(selection);
    }

    /// Replaces the active selection state with a validated multi-cursor set.
    ///
    /// This is the public model-level entry point for programmatic
    /// multi-cursor behavior. Commands that do not yet define multi-cursor
    /// semantics collapse through the set's primary selection.
    pub fn set_selection_set(&mut self, selection_set: SelectionSet) {
        self.active_tab_mut().set_selection_set(selection_set);
    }

    pub fn selection(&self) -> Selection {
        self.active_tab().selection()
    }

    pub fn selection_set(&self) -> &SelectionSet {
        self.active_tab().selection_set()
    }

    /// Collapses multi-cursor / extended-selection state toward a single
    /// caret. Stage 1: any selection with extent collapses to its head,
    /// keeping every cursor; otherwise, stage 2 drops every secondary
    /// cursor and keeps only the primary. Two presses of Esc therefore
    /// always reach a single collapsed cursor.
    pub fn collapse_to_primary(&mut self) -> bool {
        let set = self.active_tab().selection_state();
        let any_extent = set.as_slice().iter().any(Selection::has_selection);
        if any_extent {
            let collapsed: Vec<Selection> = set
                .as_slice()
                .iter()
                .map(|selection| Selection::collapsed(selection.cursor()))
                .collect();
            let primary = set.primary_index();
            let next = SelectionSet::from_selections_coalescing_cursors(collapsed, primary)
                .expect("collapsing each selection to a caret preserves invariants");
            self.active_tab_mut().set_selection_set(next);
            self.queue_reveal(RevealIntent::NearestEdge);
            return true;
        }
        if set.has_multiple() {
            let primary = set.primary();
            self.active_tab_mut()
                .set_selection_set(SelectionSet::single(primary));
            self.queue_reveal(RevealIntent::NearestEdge);
            return true;
        }
        false
    }

    pub fn add_cursor_at_char(&mut self, offset: usize) {
        let offset = offset.min(self.active_tab().len_chars());
        self.add_selection_to_active_set(Selection::collapsed(offset));
    }

    /// Removes the selection whose range contains (or whose cursor sits at)
    /// `offset`. Returns `true` when a removal happened. Refuses to remove
    /// when the set has a single selection so the model invariant
    /// "non-empty `SelectionSet`" is preserved — alt-click on the only
    /// cursor is a no-op rather than a collapse.
    pub fn remove_cursor_at_char(&mut self, offset: usize) -> bool {
        let offset = offset.min(self.active_tab().len_chars());
        let set = self.active_tab().selection_state();
        if !set.has_multiple() {
            return false;
        }
        let index = set.as_slice().iter().position(|selection| {
            let range = selection.range();
            selection.cursor() == offset || (range.start <= offset && offset < range.end)
        });
        let Some(index) = index else {
            return false;
        };
        let Some(after) = set.with_removed_at(index) else {
            return false;
        };
        self.active_tab_mut().set_selection_state(after);
        self.queue_reveal(RevealIntent::NearestEdge);
        true
    }

    pub fn add_selection_range(&mut self, range: Range<usize>, reversed: bool) {
        let len = self.active_tab().len_chars();
        let range = range.start.min(len)..range.end.min(len);
        self.add_selection_to_active_set(Selection::from_range(range, reversed));
    }

    fn add_selection_to_active_set(&mut self, selection: Selection) {
        let before = self.active_tab().selection_state().clone();
        let after = before.with_added_selection(selection);
        if after != before {
            self.active_tab_mut().set_selection_state(after);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn add_cursor_above(&mut self) {
        self.add_cursor_on_adjacent_line(-1);
    }

    pub fn add_cursor_below(&mut self) {
        self.add_cursor_on_adjacent_line(1);
    }

    fn add_cursor_on_adjacent_line(&mut self, delta: isize) {
        let (additions, preferred_goal) = {
            let tab = self.active_tab();
            let current_position = tab.cursor_position();
            let current_line_end = display_line_char_len(tab, current_position.line);
            let line_end_sticky = tab.preferred_goal() == Some(CursorGoal::LineEnd)
                || (!tab.selection_set().has_multiple()
                    && current_line_end > 0
                    && current_position.column == current_line_end);
            let preferred_goal = if line_end_sticky {
                Some(CursorGoal::LineEnd)
            } else {
                tab.preferred_goal().or_else(|| {
                    (!tab.selection_set().has_multiple())
                        .then_some(CursorGoal::Column(current_position.column))
                })
            };
            let last_line = tab.line_count().saturating_sub(1);
            let additions: Vec<Selection> = tab
                .selection_set()
                .as_slice()
                .iter()
                .filter_map(|selection| {
                    let position = char_to_position(tab.buffer(), selection.cursor());
                    let target_line = if delta.is_negative() {
                        position.line.checked_sub(delta.unsigned_abs())?
                    } else {
                        (position.line + delta as usize <= last_line)
                            .then_some(position.line + delta as usize)?
                    };
                    let column = preferred_goal
                        .unwrap_or(CursorGoal::Column(position.column))
                        .resolve(display_line_char_len(tab, target_line));
                    let target = char_at_line_column(tab.buffer(), target_line, column);
                    Some(Selection::collapsed(target))
                })
                .collect();
            (additions, preferred_goal)
        };
        if additions.is_empty() {
            return;
        }

        let before = self.active_tab().selection_state().clone();
        let after = before.with_added_selections(additions);
        if after != before {
            let tab = self.active_tab_mut();
            tab.set_selection_state(after);
            tab.set_preferred_goal(preferred_goal);
            // Center the new primary so an extension that outgrows the
            // viewport surfaces the ▲/▼ off-screen indicator instead of
            // pinning to one edge.
            self.queue_reveal(RevealIntent::Center);
        }
    }

    pub fn set_rectangular_column_selection(&mut self, anchor: usize, head: usize) {
        let Some(selection_set) =
            multi_selection::rectangular_selection_set(self.active_tab(), anchor, head)
        else {
            return;
        };
        if *self.active_tab().selection_set() != selection_set {
            self.active_tab_mut().set_selection_set(selection_set);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }

    pub fn select_next_occurrence(&mut self) {
        let tab = self.active_tab();
        if !tab.selection().has_selection() {
            let range = word_range_at_char(tab.buffer(), tab.cursor_char());
            if range.start == range.end {
                return;
            }
            self.assign_selection(Selection::from_range(range, false));
            self.queue_reveal(RevealIntent::NearestEdge);
            return;
        }
        if let Some(selection) = multi_selection::next_occurrence_addition(tab, &self.find) {
            self.add_selection_to_active_set(selection);
        }
    }

    pub fn select_all_occurrences(&mut self) {
        let Some(selection_set) =
            multi_selection::all_occurrences_set(self.active_tab(), &self.find)
        else {
            return;
        };
        self.active_tab_mut().set_selection_set(selection_set);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn select_all_find_matches(&mut self) {
        self.ensure_find_matches_current();
        let Some(selection_set) = self.find_match_selection_set() else {
            return;
        };
        self.active_tab_mut().set_selection_set(selection_set);
        self.queue_focus(FocusTarget::Editor);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    fn find_match_selection_set(&self) -> Option<SelectionSet> {
        if self.find.matches.is_empty() {
            return None;
        }
        let tab = self.active_tab();
        let buffer = tab.buffer();
        let selections = self
            .find
            .matches
            .iter()
            .map(|m| Selection::from_range(m.char_range_in(buffer), false))
            .collect::<Vec<_>>();
        let primary = self.find.active.unwrap_or(0).min(selections.len() - 1);
        SelectionSet::from_selections(selections, primary).ok()
    }

    pub fn skip_next_occurrence(&mut self) {
        let skipped = self.active_tab().selection_set().primary();
        let Some(addition) =
            multi_selection::next_occurrence_addition(self.active_tab(), &self.find)
        else {
            return;
        };
        let with_addition = self
            .active_tab()
            .selection_set()
            .with_added_selection(addition);
        let Some(skipped_index) = with_addition
            .as_slice()
            .iter()
            .position(|selection| *selection == skipped)
        else {
            return;
        };
        let Some(after) = with_addition.with_removed_at(skipped_index) else {
            return;
        };
        self.active_tab_mut().set_selection_set(after);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn pop_primary_selection_cursor(&mut self) {
        let set = self.active_tab().selection_set();
        let Some(after) = set.with_removed_at(set.primary_index()) else {
            return;
        };
        self.active_tab_mut().set_selection_set(after);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn add_cursors_to_selected_line_ends(&mut self) {
        let tab = self.active_tab();
        let (first, last) = selection_line_span(tab)
            .map(|(first, last, _)| (first, last))
            .unwrap_or_else(|| {
                let line = tab.cursor_position().line;
                (line, line)
            });
        let selections = (first..=last)
            .map(|line| {
                let end = tab.buffer().line_to_char(line) + display_line_char_len(tab, line);
                Selection::collapsed(end)
            })
            .collect::<Vec<_>>();
        if selections.is_empty() {
            return;
        }
        let primary = selections.len() - 1;
        let selection_set = SelectionSet::from_selections_coalescing_cursors(selections, primary)
            .expect("line-end cursors are sorted and non-overlapping");
        self.active_tab_mut().set_selection_set(selection_set);
        self.queue_reveal(RevealIntent::NearestEdge);
    }

    pub fn select_current_line(&mut self) {
        let tab = self.active_tab();
        let range = line_range_at_char(tab.buffer(), tab.cursor_char());
        self.assign_selection(Selection::from_range(range, false));
    }

    pub fn select_current_paragraph(&mut self) {
        let tab = self.active_tab();
        let range = paragraph_range_at_char(tab.buffer(), tab.cursor_char());
        self.assign_selection(Selection::from_range(range, false));
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
        if self.active_tab().selection_set().has_multiple() {
            let lines = line_edit::selection_set_touched_lines(self.active_tab());
            if lines.len() == 1 {
                self.indent_selected_lines(lines[0], lines[0]);
                return;
            }
        }
        match selection_line_span(self.active_tab()) {
            Some((first, last, true)) => self.indent_selected_lines(first, last),
            _ => {
                let unit = self.active_tab().language_config().indent.indent_unit();
                self.replace_text(None, unit, UndoBoundary::Break);
            }
        }
    }

    pub fn outdent_at_cursor(&mut self) {
        if self.active_tab().selection_set().has_multiple() {
            if let Some(request) = line_edit::outdent_selection_set_request(self.active_tab()) {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
                return;
            }
        }
        let (first, last) = selection_line_span(self.active_tab())
            .map(|(first, last, _)| (first, last))
            .unwrap_or_else(|| {
                let line = self.active_cursor_position().line;
                (line, line)
            });
        self.outdent_selected_lines(first, last);
    }

    fn indent_selected_lines(&mut self, first: usize, last: usize) {
        let Some(request) = line_edit::indent_request(self.active_tab(), first, last) else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    fn outdent_selected_lines(&mut self, first: usize, last: usize) {
        let Some(request) = line_edit::outdent_request(self.active_tab(), first, last) else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    pub fn delete_line(&mut self) {
        if self.active_tab().selection_set().has_multiple() {
            if let Some(request) = line_edit::delete_touched_lines_request(self.active_tab()) {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
                return;
            }
        }

        let Some(request) =
            line_edit::delete_line_request(self.active_tab(), self.active_cursor_position())
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    pub fn move_line_up(&mut self) {
        if self.active_tab().selection_set().has_multiple() {
            if let Some(request) =
                line_edit::move_touched_line_clusters_up_request(self.active_tab())
            {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
                return;
            }
        }

        let Some(request) =
            line_edit::line_swap_request(self.active_tab(), self.active_cursor_position(), true)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    pub fn move_line_down(&mut self) {
        if self.active_tab().selection_set().has_multiple() {
            if let Some(request) =
                line_edit::move_touched_line_clusters_down_request(self.active_tab())
            {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
                return;
            }
        }

        let Some(request) =
            line_edit::line_swap_request(self.active_tab(), self.active_cursor_position(), false)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    pub fn duplicate_line(&mut self) {
        if self.active_tab().selection_set().has_multiple() {
            if let Some(request) = line_edit::duplicate_touched_lines_request(self.active_tab()) {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
                return;
            }
        }

        if let Some(request) = line_edit::duplicate_selection_request(self.active_tab()) {
            self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
            return;
        }

        let Some(request) =
            line_edit::duplicate_line_request(self.active_tab(), self.active_cursor_position())
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    pub fn toggle_comment(&mut self) {
        let Some(prefix) = self.active_tab().language_config().line_comment else {
            self.status = "No line-comment syntax for this language.".to_string();
            return;
        };
        match line_edit::toggle_comment_action(self.active_tab(), prefix) {
            Some(line_edit::LineEditAction::MoveCursor(position)) => {
                self.move_active_cursor(position.line, position.column, false);
                self.queue_reveal(RevealIntent::NearestEdge);
            }
            Some(line_edit::LineEditAction::Edit(request)) => {
                self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
            }
            None => {}
        }
    }

    pub fn toggle_block_comment(&mut self) {
        let Some((open, close)) = self.active_tab().language_config().block_comment else {
            self.status = "No block-comment syntax for this language.".to_string();
            return;
        };
        let Some(request) = line_edit::toggle_block_comment_request(self.active_tab(), open, close)
        else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }

    pub fn request_paste(&mut self) {
        self.queue_effect(EditorEffect::ReadClipboard);
    }

    pub fn clipboard_unavailable(&mut self) {
        self.status = "Clipboard does not currently contain plain text.".to_string();
    }

    pub fn paste_text(&mut self, text: String) {
        if let Some(request) =
            multi_selection::paste_request(self.active_tab(), text.clone(), UndoBoundary::Break)
        {
            self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        } else {
            self.replace_text(None, text.clone(), UndoBoundary::Break);
        }
        self.status = format!("Pasted {} line(s).", text.lines().count());
    }

    /// Move the primary cursor to `(line, column)` on the active tab,
    /// clamping to the buffer's logical extent.
    pub fn set_active_cursor_position(&mut self, line: usize, column: usize) {
        self.move_active_cursor(line, column, false);
        self.queue_reveal(RevealIntent::Center);
    }

    /// Toggle a bookmark on the line that hosts the primary cursor.
    pub fn toggle_bookmark(&mut self) {
        let added = self.active_tab_mut().toggle_bookmark_at_cursor();
        self.status = if added {
            "Bookmark added.".to_string()
        } else {
            "Bookmark cleared.".to_string()
        };
    }

    pub fn jump_next_bookmark(&mut self) {
        let cursor_line = self.active_cursor_position().line;
        let Some(target) = self.active_tab().next_bookmark_line(cursor_line) else {
            return;
        };
        self.move_cursor_to_bookmarked_line(target);
    }

    pub fn jump_previous_bookmark(&mut self) {
        let cursor_line = self.active_cursor_position().line;
        let Some(target) = self.active_tab().previous_bookmark_line(cursor_line) else {
            return;
        };
        self.move_cursor_to_bookmarked_line(target);
    }

    fn move_cursor_to_bookmarked_line(&mut self, line: usize) {
        self.active_tab_mut()
            .set_cursor_position(Position { line, column: 0 }, None);
        self.queue_reveal(RevealIntent::Center);
    }

    /// Emacs `C-t`-style transpose: swap the two graphemes around the
    /// caret. At BOL/EOL, swap the line's first/last two graphemes
    /// instead — never crossing the newline.
    pub fn transpose_chars(&mut self) {
        let Some(request) = transpose_request(self.active_tab()) else {
            return;
        };
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
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
        self.replace_one();
    }

    pub fn replace_all_matches_in_document(&mut self) {
        self.replace_all_matches();
    }

    pub fn toggle_goto_line_panel(&mut self) {
        if self.goto_line.is_some() {
            self.close_goto_line_panel();
        } else {
            self.open_goto_line_panel();
        }
    }

    fn undo_active_text_mutation(&mut self, reveal: Option<RevealIntent>) -> bool {
        if self.active_tab_mut().undo() {
            self.sync_find_after_edit();
            if let Some(intent) = reveal {
                self.queue_reveal(intent);
            }
            true
        } else {
            false
        }
    }

    fn redo_active_text_mutation(&mut self, reveal: Option<RevealIntent>) -> bool {
        if self.active_tab_mut().redo() {
            self.sync_find_after_edit();
            if let Some(intent) = reveal {
                self.queue_reveal(intent);
            }
            true
        } else {
            false
        }
    }

    pub fn undo(&mut self) {
        self.undo_active_text_mutation(Some(RevealIntent::NearestEdge));
    }

    pub fn redo(&mut self) {
        self.redo_active_text_mutation(Some(RevealIntent::NearestEdge));
    }

    pub fn swap_redo_branch(&mut self) {
        if self.active_tab_mut().swap_redo_branch() {
            self.status = format!(
                "Switched to alternate redo branch ({} more saved).",
                self.active_tab().redo_branch_count(),
            );
        } else {
            self.status = "No saved redo branches.".to_string();
        }
    }
}

/// When the cursor sits in aligned leading indent of a space-indent language,
/// collapse one full indent unit instead of one grapheme. Returns `None` to
/// fall back to the regular grapheme delete.
fn soft_tab_backspace_range(tab: &EditorTab) -> Option<Range<usize>> {
    soft_tab_backspace_range_at(tab, tab.cursor_char())
}

fn soft_tab_backspace_range_at(tab: &EditorTab, cursor: usize) -> Option<Range<usize>> {
    let cfg = tab.language_config();
    if cfg.indent.uses_tabs() {
        return None;
    }
    let unit = cfg.indent.width();
    if unit == 0 {
        return None;
    }
    let buffer = tab.buffer();
    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    let col = cursor - line_start;
    if col == 0 || !col.is_multiple_of(unit) {
        return None;
    }
    // Space-indent languages should only collapse real spaces; hard tabs fall
    // back to the regular grapheme delete even when they appear in indentation.
    let prefix_len = buffer
        .line(line)
        .chars()
        .take_while(|ch| *ch == ' ')
        .take(col)
        .count();
    if col > prefix_len {
        return None;
    }
    Some((cursor - unit)..cursor)
}

fn delete_word_range_at(tab: &EditorTab, cursor: usize, backward: bool) -> Option<Range<usize>> {
    if backward {
        delete_word_backward_range(tab.buffer(), cursor)
    } else {
        delete_word_forward_range(tab.buffer(), cursor)
    }
}

fn delete_word_backward_range(buffer: &ropey::Rope, cursor: usize) -> Option<Range<usize>> {
    let cursor = cursor.min(buffer.len_chars());
    if cursor == 0 {
        return None;
    }

    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    if cursor == line_start {
        let start = previous_line_break_start(buffer, cursor)?;
        return Some(start..cursor);
    }

    if buffer.char(cursor - 1).is_whitespace() {
        let mut start = cursor;
        while start > line_start && buffer.char(start - 1).is_whitespace() {
            start -= 1;
        }
        return (start < cursor).then_some(start..cursor);
    }

    let target = previous_word_boundary(buffer, cursor);
    (target != cursor).then_some(target..cursor)
}

fn delete_word_forward_range(buffer: &ropey::Rope, cursor: usize) -> Option<Range<usize>> {
    let cursor = cursor.min(buffer.len_chars());
    if cursor >= buffer.len_chars() {
        return None;
    }

    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    let line_end = line_start + buffer_display_line_char_len(buffer, line);
    if cursor >= line_end {
        let end = next_line_join_whitespace_end(buffer, cursor)?;
        return (end > cursor).then_some(cursor..end);
    }

    if buffer.char(cursor).is_whitespace() {
        let mut end = cursor;
        while end < line_end && buffer.char(end).is_whitespace() {
            end += 1;
        }
        if end > cursor {
            if end - cursor == 1 {
                let target = next_word_boundary(buffer, cursor);
                return (target != cursor).then_some(cursor..target);
            }
            return Some(cursor..end);
        }
    }

    let target = next_word_boundary(buffer, cursor);
    (target != cursor).then_some(cursor..target)
}

fn previous_line_break_start(buffer: &ropey::Rope, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    let mut start = cursor - 1;
    if buffer.char(start) == '\n' && start > 0 && buffer.char(start - 1) == '\r' {
        start -= 1;
    }
    Some(start)
}

fn next_line_join_whitespace_end(buffer: &ropey::Rope, cursor: usize) -> Option<usize> {
    let len = buffer.len_chars();
    let mut end = cursor;
    if end >= len {
        return None;
    }

    match buffer.char(end) {
        '\r' => {
            end += 1;
            if end < len && buffer.char(end) == '\n' {
                end += 1;
            }
        }
        '\n' => {
            end += 1;
        }
        ch if ch.is_whitespace() => {
            while end < len {
                let ch = buffer.char(end);
                if ch.is_whitespace() && ch != '\n' && ch != '\r' {
                    end += 1;
                } else {
                    break;
                }
            }
            return Some(end);
        }
        _ => return None,
    }

    while end < len {
        let ch = buffer.char(end);
        if ch.is_whitespace() && ch != '\n' && ch != '\r' {
            end += 1;
        } else {
            break;
        }
    }
    Some(end)
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

#[derive(Clone, Copy, Debug)]
struct SelectionMotion {
    selection: Selection,
    movement_goal: Option<CursorGoal>,
    visible_column: Option<usize>,
}

impl SelectionMotion {
    fn with_goal(selection: Selection, goal: CursorGoal, visible_column: Option<usize>) -> Self {
        Self {
            selection,
            movement_goal: Some(goal),
            visible_column,
        }
    }
}

fn visible_column_for_motion(
    selection: Selection,
    goal: CursorGoal,
    actual_column: usize,
) -> Option<usize> {
    if selection.has_selection() {
        None
    } else {
        Some(match goal {
            CursorGoal::Column(column) => column,
            CursorGoal::LineEnd => actual_column,
        })
    }
}

fn smart_expanded_selection(buffer: &ropey::Rope, selection: Selection) -> Selection {
    let range = selection.range();
    let reversed = selection.is_reversed();
    if range.start < range.end {
        if let Some(expanded) = expand_to_adjacent_pair(buffer, range.clone()) {
            return Selection::from_range(expanded, reversed);
        }
        return selection;
    }

    let word = word_range_at_char(buffer, selection.cursor());
    if word.start < word.end {
        Selection::from_range(word, false)
    } else {
        selection
    }
}

fn smart_shrunk_selection(buffer: &ropey::Rope, selection: Selection) -> Selection {
    let range = selection.range();
    if range.end.saturating_sub(range.start) < 2 {
        return selection;
    }
    let Some(open) = rope_char(buffer, range.start) else {
        return selection;
    };
    let Some(close) = rope_char(buffer, range.end - 1) else {
        return selection;
    };
    if matching_delimiter(open, close) {
        Selection::from_range(range.start + 1..range.end - 1, selection.is_reversed())
    } else {
        selection
    }
}

fn expand_to_adjacent_pair(buffer: &ropey::Rope, range: Range<usize>) -> Option<Range<usize>> {
    if range.start == 0 || range.end >= buffer.len_chars() {
        return None;
    }
    let open = rope_char(buffer, range.start - 1)?;
    let close = rope_char(buffer, range.end)?;
    matching_delimiter(open, close).then_some(range.start - 1..range.end + 1)
}

fn rope_char(buffer: &ropey::Rope, char_index: usize) -> Option<char> {
    (char_index < buffer.len_chars()).then(|| buffer.char(char_index))
}

fn matching_delimiter(open: char, close: char) -> bool {
    matches!(
        (open, close),
        ('(', ')') | ('[', ']') | ('{', '}') | ('"', '"') | ('\'', '\'') | ('`', '`')
    )
}

fn display_line_char_len(tab: &EditorTab, line_ix: usize) -> usize {
    buffer_display_line_char_len(tab.buffer(), line_ix)
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

fn transpose_request(tab: &EditorTab) -> Option<EditRequest> {
    let buffer = tab.buffer();
    let cursor = tab.cursor_char();
    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    let line_end = line_start + display_line_char_len(tab, line);

    // Walk by grapheme boundaries — naked-char swaps would split combining
    // marks off their base character.
    let mid = if cursor == line_start {
        let m = next_grapheme_boundary(buffer, line_start);
        if m >= line_end {
            return None;
        }
        m
    } else if cursor >= line_end {
        let m = previous_grapheme_boundary(buffer, line_end);
        if m <= line_start {
            return None;
        }
        m
    } else {
        cursor
    };
    let left_start = previous_grapheme_boundary(buffer, mid);
    let right_end = next_grapheme_boundary(buffer, mid);
    if left_start < line_start || right_end > line_end {
        return None;
    }
    let first = buffer.slice(left_start..mid).to_string();
    let second = buffer.slice(mid..right_end).to_string();
    let mut replacement = String::with_capacity(first.len() + second.len());
    replacement.push_str(&second);
    replacement.push_str(&first);

    Some(
        EditRequest::single(
            EditKind::Other,
            UndoBoundary::Break,
            left_start..right_end,
            replacement,
        )
        .with_selection_after(SelectionAfter::CursorPosition(char_to_position(
            buffer, right_end,
        ))),
    )
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

        model.set_active_tab(model.tab_id_at(1).unwrap());
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
        model.set_active_tab(model.tab_id_at(1).unwrap());

        let active_id = model.active_tab_id();
        assert!(model.close_clean_tab(active_id));

        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["one.txt"]);
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.status, "Closed tab.");
    }

    #[test]
    fn gutter_mode_renders_each_kind() {
        assert_eq!(GutterMode::Absolute.format(4, 7, &[7]), "  5");
        assert_eq!(GutterMode::Relative.format(4, 7, &[7]), "  3");
        assert_eq!(GutterMode::Relative.format(7, 4, &[4]), "  3");
        // Hybrid: cursor row shows the absolute number, others show distance.
        assert_eq!(GutterMode::Hybrid.format(7, 7, &[7]), "  8");
        assert_eq!(GutterMode::Hybrid.format(2, 7, &[7]), "  5");
    }

    #[test]
    fn hybrid_gutter_mode_marks_every_multi_cursor_line() {
        // With cursors on lines 2 and 5, Hybrid shows the absolute number
        // on each cursor row and the distance-from-primary elsewhere.
        let cursor_lines = [2usize, 5];
        assert_eq!(GutterMode::Hybrid.format(2, 5, &cursor_lines), "  3");
        assert_eq!(GutterMode::Hybrid.format(5, 5, &cursor_lines), "  6");
        assert_eq!(GutterMode::Hybrid.format(4, 5, &cursor_lines), "  1");
    }

    #[test]
    fn cycle_gutter_mode_advances_through_three_modes() {
        let mut model = model_with_tabs(vec![tab(1, "one.txt", "")], "Ready.".to_string());
        assert_eq!(model.gutter_mode(), GutterMode::Absolute);
        model.cycle_gutter_mode();
        assert_eq!(model.gutter_mode(), GutterMode::Relative);
        model.cycle_gutter_mode();
        assert_eq!(model.gutter_mode(), GutterMode::Hybrid);
        model.cycle_gutter_mode();
        assert_eq!(model.gutter_mode(), GutterMode::Absolute);
    }

    #[test]
    fn vim_gi_returns_to_line_edit_cursor_not_buffer_end() {
        let mut model = model_with_tabs(
            vec![EditorTab::from_path(
                TabId::from_raw(1),
                std::path::PathBuf::from("example.rs"),
                "alpha\nbeta\ngamma",
            )],
            "Ready.".to_string(),
        );
        model.handle_vim_escape();
        model.move_to_char("alpha\n".chars().count(), false, None);

        press_vim_chars(&mut model, ">>");
        model.move_document_boundary(true, false);
        press_vim_chars(&mut model, "gi");

        let snapshot = model.snapshot();
        assert_eq!(snapshot.vim_mode, vim::Mode::Insert);
        assert_eq!(snapshot.cursor_position, Position { line: 1, column: 0 });
    }

    #[test]
    fn move_active_tab_keeps_focus_on_dragged_tab() {
        let mut model = model_with_tabs(
            vec![
                tab(1, "one.txt", "1"),
                tab(2, "two.txt", "2"),
                tab(3, "three.txt", "3"),
            ],
            "Ready.".to_string(),
        );
        model.set_active_tab(model.tab_id_at(0).unwrap());

        model.move_active_tab(1);
        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["two.txt", "one.txt", "three.txt"]);
        assert_eq!(snapshot.active, 1);

        model.move_active_tab(-1);
        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["one.txt", "two.txt", "three.txt"]);
        assert_eq!(snapshot.active, 0);
    }

    #[test]
    fn move_active_tab_wraps_around_when_delta_overshoots() {
        let mut model = model_with_tabs(
            vec![
                tab(1, "one.txt", "1"),
                tab(2, "two.txt", "2"),
                tab(3, "three.txt", "3"),
            ],
            "Ready.".to_string(),
        );

        model.set_active_tab(model.tab_id_at(2).unwrap());
        model.move_active_tab(1);
        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["three.txt", "one.txt", "two.txt"]);
        assert_eq!(snapshot.active, 0);

        model.move_active_tab(-1);
        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["one.txt", "two.txt", "three.txt"]);
        assert_eq!(snapshot.active, 2);
    }

    #[test]
    fn move_tab_shifts_other_tabs_without_changing_active_content() {
        let mut model = model_with_tabs(
            vec![
                tab(1, "one.txt", "1"),
                tab(2, "two.txt", "2"),
                tab(3, "three.txt", "3"),
            ],
            "Ready.".to_string(),
        );
        model.set_active_tab(model.tab_id_at(2).unwrap());

        model.move_tab(0, 2);

        let snapshot = model.snapshot();
        assert_eq!(snapshot.tab_titles, ["two.txt", "three.txt", "one.txt"]);
        assert_eq!(snapshot.active, 1);
    }

    #[test]
    fn select_all_queues_primary_selection() {
        let mut model = model_with_tabs(vec![tab(1, "one.txt", "hello")], "Ready.".to_string());

        model.select_all();

        assert_eq!(model.snapshot().selection.range(), 0..5);
        assert_eq!(
            model.drain_effects(),
            vec![EditorEffect::WritePrimary("hello".to_string())]
        );
    }

    fn press_vim_chars(model: &mut EditorModel, keys: &str) {
        for ch in keys.chars() {
            model.handle_vim_key(
                vim::Key::Character(ch.to_string()),
                vim::Modifiers::default(),
                80,
            );
        }
    }
}
