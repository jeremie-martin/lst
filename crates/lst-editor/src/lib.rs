mod command;
mod document;
pub mod find;
mod history;
pub mod language;
mod line_edit;
mod model_io;
mod motion;
mod multi_selection;
pub mod selection;
mod tab;
mod tab_set;
mod text_input;
mod transaction;
pub mod viewport;
pub mod vim;
mod vim_engine;
pub mod wrap;
pub use command::EditorCommand;
pub use document::{EditKind, UndoBoundary};
pub use language::{IndentStyle, Language, LanguageConfig};
pub use selection::{Position, Selection, SelectionSet, SelectionSetError};
pub use tab::{EditorTab, FileStamp, TabId};
pub use viewport::Viewport;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    Editor,
    FindQuery,
    FindReplace,
    GotoLine,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealIntent {
    NearestEdge,
    Center,
    Top,
    Bottom,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEffect {
    Focus(FocusTarget),
    Reveal(RevealIntent),
    WriteClipboard(String),
    WritePrimary(String),
    ReadClipboard,
    OpenFiles,
    SaveFile {
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expected_stamp: Option<FileStamp>,
    },
    SaveFileAs {
        tab_id: TabId,
        suggested_name: String,
        body: String,
        revision: u64,
        previous_scratchpad_path: Option<PathBuf>,
    },
    AutosaveFile {
        tab_id: TabId,
        path: PathBuf,
        body: String,
        revision: u64,
        expected_stamp: Option<FileStamp>,
    },
}
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
    document::char_to_position,
    find::{FindScope, FindState},
    selection::{
        char_at_line_column, display_line_char_len as buffer_display_line_char_len, line_range_at_char,
        word_range_at_char, CursorGoal, SelectionTransform,
    },
    tab_set::TabSet,
    transaction::{EditOutcome, EditRequest},
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
    find_submit: FindSubmit,
    goto_line: Option<String>,
    status: String,
    vim: vim::VimState,
    viewport: Viewport,
    effects: Vec<EditorEffect>,
    overtype: bool,
    wrap_layout_cache: Option<ModelWrapLayoutCache>,
}
struct ModelWrapLayoutCache {
    tab_id: TabId,
    revision: u64,
    wrap_columns: usize,
    layout: wrap::WrapLayout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FindSubmit {
    Panel,
    Vim { backward: bool },
}
impl EditorModel {
    pub fn from_tabs(first: EditorTab, rest: Vec<EditorTab>, status: String) -> Self {
        Self {
            tabs: TabSet::new(first, rest),
            next_untitled_id: 2,
            show_gutter: true,
            show_wrap: true,
            gutter_mode: GutterMode::Absolute,
            find: FindState::new(),
            find_submit: FindSubmit::Panel,
            goto_line: None,
            status,
            vim: vim::VimState::new(),
            viewport: Viewport::default(),
            effects: Vec::new(),
            overtype: false,
            wrap_layout_cache: None,
        }
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
    fn ensure_active_wrap_layout(&mut self, wrap_columns: usize, lines: &[String]) {
        let tab_id = self.active_tab_id();
        let revision = self.active_tab().revision();
        if self.wrap_layout_cache.as_ref().is_some_and(|cache| {
            cache.tab_id == tab_id && cache.revision == revision && cache.wrap_columns == wrap_columns
        }) {
            return;
        }
        self.wrap_layout_cache = Some(ModelWrapLayoutCache {
            tab_id,
            revision,
            wrap_columns,
            layout: wrap::build_wrap_layout(lines, wrap_columns, true),
        });
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
    pub fn vim_register(&self) -> &vim::Register {
        &self.vim.register
    }
    pub fn vim_visual_state(&self) -> Option<vim::VisualState> {
        self.vim.visual_state()
    }
    pub fn vim_pending_display(&self) -> String {
        self.vim.pending_display()
    }
    pub fn overtype(&self) -> bool {
        self.overtype
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
        self.find_submit = FindSubmit::Panel;
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
        self.find_submit = FindSubmit::Panel;
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
    pub fn update_find_query_and_activate(&mut self, text: String) {
        self.find.query = text;
        self.reindex_find_matches_to_nearest();
        if self.move_to_current_find_match() {
            self.queue_reveal(RevealIntent::Center);
        }
    }
    pub fn submit_find_query(&mut self) {
        match self.find_submit {
            FindSubmit::Panel => {
                self.execute(EditorCommand::FindNext);
            }
            FindSubmit::Vim { backward } => {
                if !matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine) {
                    if backward {
                        self.execute(EditorCommand::FindPrev);
                    } else {
                        self.execute(EditorCommand::FindNext);
                    }
                }
                self.close_find_panel();
            }
        }
    }
    pub fn update_find_replacement(&mut self, text: String) {
        self.find.replacement = text;
    }
    // No-op when toggling on without a selection (UI grays the chip).
    fn toggle_find_in_selection(&mut self) {
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
        self.find.reindex_for_tab(self.tabs.active());
    }
    fn reindex_find_matches_to_nearest(&mut self) {
        self.find.reindex_to_nearest(self.tabs.active());
    }
    fn ensure_find_matches_current(&mut self) {
        self.find.ensure_current(self.tabs.active());
    }
    fn sync_find_with_active_document(&mut self) {
        self.find.sync_with_tab(self.tabs.active());
    }
    fn sync_find_after_edit(&mut self) {
        self.find.sync_after_edit(self.tabs.active());
    }
    fn find_step(&mut self, forward: bool) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        if forward {
            self.find.next();
        } else {
            self.find.prev();
        }
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
    fn submit_goto_line_input(&mut self) {
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
        if self.vim_in_visual() {
            self.move_to_vim_search_target(start);
        } else {
            self.active_tab_mut().set_cursor_position(start, None);
        }
        true
    }
    fn apply_active_edit_request(&mut self, request: EditRequest, reveal: Option<RevealIntent>) -> EditOutcome {
        let outcome = self.active_tab_mut().apply_edit_request(request);
        if outcome.text_changed {
            self.sync_find_after_edit();
        }
        if outcome.changed() {
            if let Some(intent) = reveal {
                self.queue_reveal(intent);
            }
        }
        outcome
    }
    fn apply_optional_edit_request(&mut self, request: Option<EditRequest>, reveal: Option<RevealIntent>) -> bool {
        let Some(request) = request else { return false };
        self.apply_active_edit_request(request, reveal);
        true
    }
    pub fn replace_text(&mut self, range: Option<Range<usize>>, text: String, boundary: UndoBoundary) {
        let request = text_input::replace_request(self.active_tab(), range, text, boundary);
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }
    pub fn replace_and_mark_text(
        &mut self,
        range: Option<Range<usize>>,
        text: String,
        selected_range: Option<Range<usize>>,
    ) {
        let request = text_input::marked_text_request(self.active_tab(), range, text, selected_range);
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }
    fn delete_selected_or_word(&mut self, backward: bool) -> bool {
        self.apply_optional_edit_request(
            text_input::delete_selected_or_word_request(self.active_tab(), backward),
            Some(RevealIntent::NearestEdge),
        )
    }
    fn insert_newline(&mut self) {
        let request = text_input::newline_request(self.active_tab());
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
    }
    fn apply_selection_motion<F>(&mut self, preferred_column: Option<usize>, mut motion: F) -> bool
    where
        F: FnMut(&EditorTab, Selection) -> Selection,
    {
        let goal = preferred_column.map(CursorGoal::Column);
        self.apply_selection_transform(|tab, _, selection| {
            let selection = motion(tab, selection);
            match goal {
                Some(goal) => SelectionTransform::with_columns(
                    selection,
                    goal,
                    (!selection.has_selection()).then_some(preferred_column.unwrap()),
                ),
                None => SelectionTransform::new(selection),
            }
        })
    }
    fn apply_selection_transform<F>(&mut self, mut transform: F) -> bool
    where
        F: FnMut(&EditorTab, usize, Selection) -> SelectionTransform,
    {
        let Some((before, after)) = (|| {
            let tab = self.active_tab();
            let before = tab.selection_state().clone();
            let after = before.map(|index, selection| transform(tab, index, selection))?;
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
    fn apply_selection_state(&mut self, after: Option<selection::SelectionState>) -> bool {
        let Some(after) = after else {
            return false;
        };
        self.active_tab_mut().set_selection_state(after);
        true
    }
    fn move_horizontal(&mut self, delta: isize, select: bool) -> bool {
        self.apply_selection_state(motion::horizontal(self.active_tab(), delta, select))
    }
    fn move_boundary(
        &mut self,
        backward: bool,
        select: bool,
        prev_fn: fn(&ropey::Rope, usize) -> usize,
        next_fn: fn(&ropey::Rope, usize) -> usize,
    ) -> bool {
        self.apply_selection_state(motion::boundary(self.active_tab(), backward, select, prev_fn, next_fn))
    }
    fn move_display_rows(&mut self, delta: isize, select: bool, wrap_columns: usize, snap: bool) -> bool {
        if !self.show_wrap {
            return self.apply_selection_state(motion::vertical(self.active_tab(), delta, select, snap));
        }

        let lines = self.active_tab_lines();
        self.ensure_active_wrap_layout(wrap_columns, lines.as_ref());
        let state = {
            let layout = &self
                .wrap_layout_cache
                .as_ref()
                .expect("wrap layout cache was just populated")
                .layout;
            motion::display_rows_with_layout(self.active_tab(), lines.as_ref(), layout, delta, select, snap)
        };
        self.apply_selection_state(state)
    }
    fn move_paged(&mut self, delta: isize, select: bool, wrap_columns: usize, snap: bool) {
        if self.move_display_rows(delta, select, wrap_columns, snap) {
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }
    fn move_with_reveal(&mut self, state: Option<selection::SelectionState>) {
        if self.apply_selection_state(state) {
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
        self.apply_optional_edit_request(
            text_input::delete_selected_or_previous_request(self.active_tab()),
            Some(RevealIntent::NearestEdge),
        )
    }
    fn delete_selected_or_next(&mut self) -> bool {
        self.apply_optional_edit_request(
            text_input::delete_selected_or_next_request(self.active_tab()),
            Some(RevealIntent::NearestEdge),
        )
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
    fn copy_selection(&mut self) {
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
    fn cut_selection(&mut self) {
        if let Some(text) = multi_selection::selected_text_joined(self.active_tab()) {
            if text.is_empty() {
                return;
            }
            self.queue_clipboard_copy(text);
            let deleted = self.apply_optional_edit_request(
                multi_selection::delete_request(self.active_tab(), UndoBoundary::Break, |_tab, _| None),
                Some(RevealIntent::NearestEdge),
            );
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
        let request = text_input::replace_request(self.active_tab(), Some(range), String::new(), UndoBoundary::Break);
        self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
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
    pub fn handle_vim_key(&mut self, key: vim::Key, mods: vim::Modifiers, wrap_columns: usize) -> bool {
        self.vim_handle_key(key, mods, wrap_columns)
    }
    pub fn handle_vim_escape(&mut self) -> bool {
        self.vim_escape()
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
    fn expand_range_for_overtype(&self, range: Option<Range<usize>>, text: &str) -> Option<Range<usize>> {
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
            return range;
        }
        let next = selection::next_grapheme_boundary(buffer, cursor);
        Some(cursor..next)
    }
    fn apply_text_input(&mut self, range: Option<Range<usize>>, text: String, boundary: UndoBoundary) {
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
                    self.find.reindex_to_nearest(self.tabs.active());
                }
                self.queue_reveal(RevealIntent::NearestEdge);
            }
        }
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
        let tab_id = tab.id();
        Some(if tab.modified() {
            TabCloseRequest::SaveAndClose { tab_id }
        } else {
            TabCloseRequest::Close { tab_id }
        })
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
    fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.activate_tab((self.active_index() + 1) % self.tabs.len());
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }
    fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            let active = self.active_index();
            let prev = if active == 0 { self.tabs.len() - 1 } else { active - 1 };
            self.activate_tab(prev);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if self.tabs.reorder(from, to) {
            self.status = format!("Reordered tab to position {}.", to + 1);
        }
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
    pub fn set_selection(&mut self, selection: Selection) {
        self.assign_selection(selection);
    }
    pub fn set_selection_set(&mut self, selection_set: SelectionSet) {
        self.active_tab_mut().set_selection_set(selection_set);
    }
    pub fn selection(&self) -> Selection {
        self.active_tab().selection()
    }
    pub fn selection_set(&self) -> &SelectionSet {
        self.active_tab().selection_set()
    }
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
            self.active_tab_mut().set_selection_set(SelectionSet::single(primary));
            self.queue_reveal(RevealIntent::NearestEdge);
            return true;
        }
        false
    }
    pub fn add_cursor_at_char(&mut self, offset: usize) {
        let offset = selection::floor_grapheme_boundary(self.active_tab().buffer(), offset);
        self.add_selection_to_active_set(Selection::collapsed(offset));
    }
    pub fn remove_cursor_at_char(&mut self, offset: usize) -> bool {
        let offset = selection::floor_grapheme_boundary(self.active_tab().buffer(), offset);
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
        let buffer = self.active_tab().buffer();
        let range = selection::floor_grapheme_boundary(buffer, range.start)
            ..selection::ceil_grapheme_boundary(buffer, range.end);
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
                    (!tab.selection_set().has_multiple()).then_some(CursorGoal::Column(current_position.column))
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
                        (position.line + delta as usize <= last_line).then_some(position.line + delta as usize)?
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
            self.queue_reveal(RevealIntent::Center);
        }
    }
    pub fn set_rectangular_column_selection(&mut self, anchor: usize, head: usize) {
        let Some(selection_set) = multi_selection::rectangular_selection_set(self.active_tab(), anchor, head) else {
            return;
        };
        if *self.active_tab().selection_set() != selection_set {
            self.active_tab_mut().set_selection_set(selection_set);
            self.queue_reveal(RevealIntent::NearestEdge);
        }
    }
    fn select_next_occurrence(&mut self) {
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
    fn select_all_occurrences(&mut self) {
        let Some(selection_set) = multi_selection::all_occurrences_set(self.active_tab(), &self.find) else {
            return;
        };
        if selection_set.already_normalized {
            self.active_tab_mut().set_normalized_selection_set(selection_set.set);
        } else {
            self.active_tab_mut().set_selection_set(selection_set.set);
        }
        self.queue_reveal(RevealIntent::NearestEdge);
    }
    fn select_all_find_matches(&mut self) {
        self.ensure_find_matches_current();
        let Some(selection_set) = self.find.active_selection_set(self.active_tab()) else {
            return;
        };
        self.active_tab_mut().set_normalized_selection_set(selection_set);
        self.queue_focus(FocusTarget::Editor);
        self.queue_reveal(RevealIntent::NearestEdge);
    }
    fn skip_next_occurrence(&mut self) {
        let skipped = self.active_tab().selection_set().primary();
        let Some(addition) = multi_selection::next_occurrence_addition(self.active_tab(), &self.find) else {
            return;
        };
        let with_addition = self.active_tab().selection_set().with_added_selection(addition);
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
    fn pop_primary_selection_cursor(&mut self) {
        let set = self.active_tab().selection_set();
        let Some(after) = set.with_removed_at(set.primary_index()) else {
            return;
        };
        self.active_tab_mut().set_selection_set(after);
        self.queue_reveal(RevealIntent::NearestEdge);
    }
    fn add_cursors_to_selected_line_ends(&mut self) {
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
    fn insert_tab_at_cursor(&mut self) {
        if self.active_tab().selection_set().has_multiple()
            && self.apply_optional_edit_request(
                line_edit::indent_selection_set_request(self.active_tab()),
                Some(RevealIntent::NearestEdge),
            )
        {
            return;
        }
        match selection_line_span(self.active_tab()) {
            Some((first, last, true)) => self.indent_selected_lines(first, last),
            _ => {
                let unit = self.active_tab().language_config().indent.indent_unit();
                self.replace_text(None, unit, UndoBoundary::Break);
            }
        }
    }
    fn outdent_at_cursor(&mut self) {
        if self.active_tab().selection_set().has_multiple()
            && self.apply_optional_edit_request(
                line_edit::outdent_selection_set_request(self.active_tab()),
                Some(RevealIntent::NearestEdge),
            )
        {
            return;
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
        self.apply_optional_edit_request(
            line_edit::indent_request(self.active_tab(), first, last),
            Some(RevealIntent::NearestEdge),
        );
    }
    fn outdent_selected_lines(&mut self, first: usize, last: usize) {
        self.apply_optional_edit_request(
            line_edit::outdent_request(self.active_tab(), first, last),
            Some(RevealIntent::NearestEdge),
        );
    }
    fn delete_line(&mut self) {
        self.apply_optional_edit_request(
            line_edit::delete_lines_request(self.active_tab(), self.active_cursor_position()),
            Some(RevealIntent::NearestEdge),
        );
    }
    fn move_line(&mut self, up: bool) {
        self.apply_optional_edit_request(
            line_edit::move_lines_request(self.active_tab(), self.active_cursor_position(), up),
            Some(RevealIntent::NearestEdge),
        );
    }
    fn duplicate_line(&mut self) {
        self.apply_optional_edit_request(
            line_edit::duplicate_lines_request(self.active_tab(), self.active_cursor_position()),
            Some(RevealIntent::NearestEdge),
        );
    }
    fn toggle_comment(&mut self) {
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
    fn toggle_block_comment(&mut self) {
        let Some((open, close)) = self.active_tab().language_config().block_comment else {
            self.status = "No block-comment syntax for this language.".to_string();
            return;
        };
        self.apply_optional_edit_request(
            line_edit::toggle_block_comment_request(self.active_tab(), open, close),
            Some(RevealIntent::NearestEdge),
        );
    }
    pub fn clipboard_unavailable(&mut self) {
        self.status = "Clipboard does not currently contain plain text.".to_string();
    }
    pub fn paste_text(&mut self, text: String) {
        if let Some(request) = multi_selection::paste_request(self.active_tab(), text.clone(), UndoBoundary::Break) {
            self.apply_active_edit_request(request, Some(RevealIntent::NearestEdge));
        } else {
            self.replace_text(None, text.clone(), UndoBoundary::Break);
        }
        self.status = format!("Pasted {} line(s).", text.lines().count());
    }
    pub fn set_active_cursor_position(&mut self, line: usize, column: usize) {
        self.move_active_cursor(line, column, false);
        self.queue_reveal(RevealIntent::Center);
    }
    fn toggle_bookmark(&mut self) {
        let added = self.active_tab_mut().toggle_bookmark_at_cursor();
        self.status = if added {
            "Bookmark added.".to_string()
        } else {
            "Bookmark cleared.".to_string()
        };
    }
    fn jump_bookmark(&mut self, next: bool) {
        let cursor_line = self.active_cursor_position().line;
        let target = if next {
            self.active_tab().next_bookmark_line(cursor_line)
        } else {
            self.active_tab().previous_bookmark_line(cursor_line)
        };
        let Some(line) = target else {
            return;
        };
        self.active_tab_mut()
            .set_cursor_position(Position { line, column: 0 }, None);
        self.queue_reveal(RevealIntent::Center);
    }
    fn undo_or_redo(&mut self, redo: bool, reveal: Option<RevealIntent>) -> bool {
        let changed = if redo {
            self.active_tab_mut().redo()
        } else {
            self.active_tab_mut().undo()
        };
        if !changed {
            return false;
        }
        self.sync_find_after_edit();
        if let Some(intent) = reveal {
            self.queue_reveal(intent);
        }
        true
    }
    fn swap_redo_branch(&mut self) {
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
fn linewise_range_at_char(buffer: &ropey::Rope, char_index: usize) -> Range<usize> {
    let range = line_range_at_char(buffer, char_index);
    let ends_in_newline = range.end > range.start && matches!(buffer.char(range.end - 1), '\n' | '\r');
    if ends_in_newline {
        return range;
    }
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
