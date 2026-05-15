use crate::{
    selection::{line_range_at_char, next_subword_boundary, next_word_boundary, paragraph_range_at_char, previous_subword_boundary, previous_word_boundary},
    EditorModel, RevealIntent, Selection,
};

macro_rules! reveal_if {
    ($model:ident, $changed:expr, $intent:expr) => {
        if $changed {
            $model.queue_reveal($intent);
        }
    };
}

#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    RequestOpenFiles, RequestSave, RequestSaveAs,
    NextTab, PrevTab, MoveActiveTab(isize),
    ToggleWrap, CycleGutterMode, CopySelection, CutSelection, RequestPaste,
    MoveHorizontalCollapsed(bool), MoveHorizontal(isize, bool), MoveWord(bool, bool),
    MoveSubword(bool, bool), MoveDocumentBoundary(bool, bool), SmartHome(bool),
    MoveLineBoundary(bool, bool), MoveDisplayRows(isize, bool, usize), Page(bool, bool, usize),
    Backspace, DeleteForward, DeleteWord(bool),
    InsertNewline, InsertTab, Outdent, SelectAll, SmartExpandSelection, SmartShrinkSelection,
    SelectNextOccurrence, SelectAllOccurrences, SelectAllFindMatches, SkipNextOccurrence,
    PopPrimarySelectionCursor, AddCursorAbove, AddCursorBelow, AddCursorsToSelectedLineEnds,
    SelectCurrentLine, SelectCurrentParagraph, Undo, Redo, SwapRedoBranch, ToggleFindPanel(bool),
    FindNext, FindPrev, ReplaceCurrentMatch, ReplaceAllMatches,
    ToggleFindCaseSensitive, ToggleFindWholeWord, ToggleFindRegex, ToggleFindInSelection,
    ToggleGotoLinePanel, SubmitGotoLine, DeleteLine, MoveLineUp, MoveLineDown, DuplicateLine,
    ToggleComment, ToggleBlockComment, TransposeChars, ToggleOvertype, ToggleBookmark,
    JumpNextBookmark, JumpPreviousBookmark,
}

impl EditorModel {
    #[rustfmt::skip]
    pub fn execute(&mut self, command: EditorCommand) {
        use EditorCommand::*;

        match command {
            RequestOpenFiles => self.request_open_files(),
            RequestSave => self.request_save(),
            RequestSaveAs => self.request_save_as(),
            NextTab => self.next_tab(),
            PrevTab => self.prev_tab(),
            MoveActiveTab(delta) => { let len = self.tabs.len(); if len > 1 { let from = self.active_index(); self.move_tab(from, (from as isize + delta).rem_euclid(len as isize) as usize); } }
            ToggleWrap => { self.show_wrap = !self.show_wrap; self.status = if self.show_wrap { "Soft wrap enabled.".to_string() } else { "Soft wrap disabled.".to_string() }; self.queue_reveal(RevealIntent::NearestEdge); }
            CycleGutterMode => { self.gutter_mode = self.gutter_mode.cycle(); self.status = format!("Line numbers: {}", self.gutter_mode.label()); }
            CopySelection => self.copy_selection(),
            CutSelection => self.cut_selection(),
            RequestPaste => self.queue_effect(super::EditorEffect::ReadClipboard),
            MoveHorizontalCollapsed(backward) => self.move_with_reveal(super::motion::horizontal_collapsed(self.active_tab(), backward)),
            MoveHorizontal(delta, select) => reveal_if!(self, self.move_horizontal(delta, select), RevealIntent::NearestEdge),
            MoveWord(backward, select) => reveal_if!(self, self.move_boundary(backward, select, previous_word_boundary, next_word_boundary), RevealIntent::NearestEdge),
            MoveSubword(backward, select) => reveal_if!(self, self.move_boundary(backward, select, previous_subword_boundary, next_subword_boundary), RevealIntent::NearestEdge),
            MoveDocumentBoundary(to_end, select) => self.move_with_reveal(super::motion::document_boundary(self.active_tab(), to_end, select)),
            SmartHome(select) => self.move_with_reveal(super::motion::smart_home(self.active_tab(), select)),
            MoveLineBoundary(to_end, select) => self.move_with_reveal(super::motion::line_boundary(self.active_tab(), to_end, select)),
            MoveDisplayRows(delta, select, wrap_columns) => self.move_paged(delta, select, wrap_columns, true),
            Page(down, select, wrap_columns) => { let delta = self.viewport.page() as isize; self.move_paged(if down { delta } else { -delta }, select, wrap_columns, true); }
            Backspace => { self.delete_selected_or_previous(); }
            DeleteForward => { self.delete_selected_or_next(); }
            DeleteWord(backward) => { self.delete_selected_or_word(backward); }
            InsertNewline => self.insert_newline(),
            InsertTab => self.insert_tab_at_cursor(),
            Outdent => self.outdent_at_cursor(),
            SelectAll => { self.active_tab_mut().select_all(); if let Some(text) = self.active_tab().selected_text() { self.queue_effect(super::EditorEffect::WritePrimary(text)); } }
            SmartExpandSelection => reveal_if!(self, self.apply_selection_motion(None, |tab, selection| super::smart_expanded_selection(tab.buffer(), selection)), RevealIntent::NearestEdge),
            SmartShrinkSelection => reveal_if!(self, self.apply_selection_motion(None, |tab, selection| super::smart_shrunk_selection(tab.buffer(), selection)), RevealIntent::NearestEdge),
            SelectNextOccurrence => self.select_next_occurrence(),
            SelectAllOccurrences => self.select_all_occurrences(),
            SelectAllFindMatches => self.select_all_find_matches(),
            SkipNextOccurrence => self.skip_next_occurrence(),
            PopPrimarySelectionCursor => self.pop_primary_selection_cursor(),
            AddCursorAbove => self.add_cursor_on_adjacent_line(-1),
            AddCursorBelow => self.add_cursor_on_adjacent_line(1),
            AddCursorsToSelectedLineEnds => self.add_cursors_to_selected_line_ends(),
            SelectCurrentLine => { let tab = self.active_tab(); self.assign_selection(Selection::from_range(line_range_at_char(tab.buffer(), tab.cursor_char()), false)); }
            SelectCurrentParagraph => { let tab = self.active_tab(); self.assign_selection(Selection::from_range(paragraph_range_at_char(tab.buffer(), tab.cursor_char()), false)); }
            Undo => { self.undo_or_redo(false, Some(RevealIntent::NearestEdge)); }
            Redo => { self.undo_or_redo(true, Some(RevealIntent::NearestEdge)); }
            SwapRedoBranch => self.swap_redo_branch(),
            ToggleFindPanel(show_replace) => { if self.find.visible && self.find.show_replace == show_replace { self.close_find_panel(); } else { self.open_find_panel(show_replace); } }
            FindNext => reveal_if!(self, self.find_step(true), RevealIntent::Center),
            FindPrev => reveal_if!(self, self.find_step(false), RevealIntent::Center),
            ReplaceCurrentMatch => { self.replace_one(); }
            ReplaceAllMatches => { self.replace_all_matches(); }
            ToggleFindCaseSensitive => { self.find.case_sensitive = !self.find.case_sensitive; self.reindex_find_matches_to_nearest(); }
            ToggleFindWholeWord => { self.find.whole_word = !self.find.whole_word; self.reindex_find_matches_to_nearest(); }
            ToggleFindRegex => { self.find.use_regex = !self.find.use_regex; self.reindex_find_matches_to_nearest(); }
            ToggleFindInSelection => self.toggle_find_in_selection(),
            ToggleGotoLinePanel => { if self.goto_line.is_some() { self.close_goto_line_panel(); } else { self.open_goto_line_panel(); } }
            SubmitGotoLine => self.submit_goto_line_input(),
            DeleteLine => self.delete_line(),
            MoveLineUp => self.move_line(true),
            MoveLineDown => self.move_line(false),
            DuplicateLine => self.duplicate_line(),
            ToggleComment => self.toggle_comment(),
            ToggleBlockComment => self.toggle_block_comment(),
            TransposeChars => { self.apply_optional_edit_request(super::text_input::transpose_request(self.active_tab()), Some(RevealIntent::NearestEdge)); }
            ToggleOvertype => { self.overtype = !self.overtype; self.status = if self.overtype { "Overtype on.".to_string() } else { "Overtype off.".to_string() }; }
            ToggleBookmark => self.toggle_bookmark(),
            JumpNextBookmark => self.jump_bookmark(true),
            JumpPreviousBookmark => self.jump_bookmark(false),
        }
    }
}
