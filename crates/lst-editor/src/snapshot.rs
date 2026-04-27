use crate::position::Position;
use crate::{vim, EditorModel, EditorTab, Selection, TabId};
use std::{ops::Range, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub active: usize,
    pub tab_count: usize,
    pub active_tab_id: TabId,
    pub tab_ids: Vec<TabId>,
    pub tab_titles: Vec<String>,
    pub tab_modified: Vec<bool>,
    pub tab_scratchpad: Vec<bool>,
    pub text: String,
    pub cursor: usize,
    pub cursor_position: Position,
    pub selection: Selection,
    pub active_path: Option<PathBuf>,
    pub active_revision: u64,
    pub show_wrap: bool,
    pub show_gutter: bool,
    pub find_visible: bool,
    pub find_show_replace: bool,
    pub find_query: String,
    pub find_replacement: String,
    pub find_matches: usize,
    pub find_current: Option<usize>,
    pub find_match_ranges: Vec<Range<usize>>,
    pub find_active_match: Option<Range<usize>>,
    pub find_case_sensitive: bool,
    pub find_whole_word: bool,
    pub find_use_regex: bool,
    pub find_in_selection: bool,
    pub find_error: Option<String>,
    pub goto_line: Option<String>,
    pub vim_mode: vim::Mode,
    pub vim_pending: String,
    pub status: String,
}

impl EditorModel {
    pub fn snapshot(&self) -> EditorSnapshot {
        let active = self.active_tab();
        EditorSnapshot {
            active: self.active_index(),
            tab_count: self.tabs.len(),
            active_tab_id: active.id(),
            tab_ids: self.tabs.iter().map(EditorTab::id).collect(),
            tab_titles: self.tabs.iter().map(|tab| tab.display_name()).collect(),
            tab_modified: self.tabs.iter().map(EditorTab::modified).collect(),
            tab_scratchpad: self.tabs.iter().map(EditorTab::is_scratchpad).collect(),
            text: active.buffer_text(),
            cursor: active.cursor_char(),
            cursor_position: active.cursor_position(),
            selection: active.selection(),
            active_path: active.path().cloned(),
            active_revision: active.revision(),
            show_wrap: self.show_wrap,
            show_gutter: self.show_gutter,
            find_visible: self.find.visible,
            find_show_replace: self.find.show_replace,
            find_query: self.find.query.clone(),
            find_replacement: self.find.replacement.clone(),
            find_matches: self.find.matches.len(),
            find_current: self.find.active,
            find_match_ranges: self.find_match_ranges(),
            find_active_match: self.active_find_match_range(),
            find_case_sensitive: self.find.case_sensitive,
            find_whole_word: self.find.whole_word,
            find_use_regex: self.find.use_regex,
            find_in_selection: self.find.scope.is_selection_for(active.id()),
            find_error: self.find.error.clone(),
            goto_line: self.goto_line.clone(),
            vim_mode: self.vim.mode,
            vim_pending: self.vim.pending_display(),
            status: self.status.clone(),
        }
    }
}
