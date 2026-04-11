use lst_core::{
    document::{EditKind, Tab, UndoBoundary},
    find::FindState,
    position::Position,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    InsertText(String),
    NewTab,
    CloseTab(usize),
    SetActiveTab(usize),
    SelectAll,
    OpenFind { show_replace: bool },
    CloseFind,
    SetFindQuery(String),
    SetFindReplacement(String),
    FindNext,
    FindPrev,
    ReplaceAll,
    OpenGotoLine,
    CloseGotoLine,
    SetGotoLine(String),
    SubmitGotoLine,
    Undo,
    Redo,
}

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
        self.active_tab_mut().preferred_column = None;
        self.sync_find_with_active_document();
        true
    }

    pub fn queue_focus(&mut self, target: FocusTarget) {
        self.effects.push(EditorEffect::Focus(target));
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
        self.reindex_find_matches();
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

    pub fn reindex_find_matches(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
            return;
        }
        let text = self.active_tab().buffer_text();
        self.find.compute_matches_in_text(&text);
        self.find.finish_reindex(self.active_tab().revision());
    }

    fn sync_find_with_active_document(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
        } else {
            self.reindex_find_matches();
        }
    }

    pub fn find_next(&mut self) -> bool {
        self.reindex_find_matches();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.next();
        self.select_current_find_match()
    }

    pub fn find_prev(&mut self) -> bool {
        self.reindex_find_matches();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.prev();
        self.select_current_find_match()
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
        self.sync_find_with_active_document();
        true
    }

    pub fn submit_goto_line(&mut self) -> bool {
        let Some(text) = self.goto_line.clone() else {
            return false;
        };
        let Ok(line_one_based) = text.trim().parse::<usize>() else {
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
        true
    }

    pub fn close_tab_at(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = self.new_empty_tab();
            self.set_active_tab(0);
            self.queue_focus(FocusTarget::Editor);
            return true;
        }

        let should_refocus = should_refocus_editor_after_tab_close(self.active, index);
        let next_active = next_active_after_tab_close(self.tabs.len(), self.active, index);
        self.tabs.remove(index);
        self.set_active_tab(next_active);
        if should_refocus {
            self.queue_focus(FocusTarget::Editor);
        }
        true
    }

    fn select_current_find_match(&mut self) -> bool {
        let Some((start, end)) = self.find.current_match_range() else {
            return false;
        };
        self.active_tab_mut().set_cursor_position(end, Some(start));
        true
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
                self.sync_find_with_active_document();
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
                self.set_active_tab(index);
            }
            EditorCommand::SelectAll => {
                self.active_tab_mut().select_all();
            }
            EditorCommand::OpenFind { show_replace } => {
                let selected = self.active_tab().selected_text();
                self.open_find(show_replace, selected);
            }
            EditorCommand::CloseFind => self.close_find(),
            EditorCommand::SetFindQuery(text) => self.set_find_query(text),
            EditorCommand::SetFindReplacement(text) => self.set_find_replacement(text),
            EditorCommand::FindNext => {
                self.find_next();
            }
            EditorCommand::FindPrev => {
                self.find_prev();
            }
            EditorCommand::ReplaceAll => {
                self.replace_all_matches();
            }
            EditorCommand::OpenGotoLine => self.open_goto_line(),
            EditorCommand::CloseGotoLine => self.close_goto_line(),
            EditorCommand::SetGotoLine(text) => self.set_goto_line(text),
            EditorCommand::SubmitGotoLine => {
                self.submit_goto_line();
            }
            EditorCommand::Undo => {
                if self.active_tab_mut().undo() {
                    self.sync_find_with_active_document();
                }
            }
            EditorCommand::Redo => {
                if self.active_tab_mut().redo() {
                    self.sync_find_with_active_document();
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub active: usize,
    pub tab_count: usize,
    pub text: String,
    pub cursor: usize,
    pub selection: Range<usize>,
    pub find_visible: bool,
    pub find_show_replace: bool,
    pub find_query: String,
    pub find_matches: usize,
    pub status: String,
}

impl EditorModel {
    pub fn snapshot(&self) -> EditorSnapshot {
        let active = self.active_tab();
        EditorSnapshot {
            active: self.active,
            tab_count: self.tabs.len(),
            text: active.buffer_text(),
            cursor: active.cursor_char(),
            selection: active.selected_range(),
            find_visible: self.find.visible,
            find_show_replace: self.find.show_replace,
            find_query: self.find.query.clone(),
            find_matches: self.find.matches.len(),
            status: self.status.clone(),
        }
    }
}
