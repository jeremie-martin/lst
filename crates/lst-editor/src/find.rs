use crate::{
    document::{char_to_position, position_to_char, EditKind, UndoBoundary},
    selection::{
        cell_partition_by_byte, cells_of_str, line_display_text, Position, Selection, SelectionSet,
    },
    tab::EditorTab,
    transaction::{EditRequest, SelectionAfter, TextChange, TextChangeSet},
    TabId,
};
use regex::{Regex, RegexBuilder};
use ropey::Rope;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchPos {
    pub line: usize,
    pub col: usize,
    pub char_len: usize,
}

impl MatchPos {
    pub(crate) fn char_range_in(self, buffer: &Rope) -> Range<usize> {
        let start = position_to_char(
            buffer,
            Position {
                line: self.line,
                column: self.col,
            },
        );
        let end = position_to_char(
            buffer,
            Position {
                line: self.line,
                column: self.col + self.char_len,
            },
        );
        start..end
    }
}

// `Selection` freezes a char range captured at toggle-on. Edits before
// the captured range do not shift it — the user re-toggles to refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindScope {
    Document,
    Selection {
        tab_id: TabId,
        start_char: usize,
        end_char: usize,
    },
}

impl FindScope {
    pub fn is_selection_for(self, tab_id: TabId) -> bool {
        self.selection_range_for(tab_id).is_some()
    }

    fn selection_range_for(self, tab_id: TabId) -> Option<Range<usize>> {
        match self {
            FindScope::Selection {
                tab_id: owner,
                start_char,
                end_char,
            } if owner == tab_id => Some(start_char..end_char),
            FindScope::Document | FindScope::Selection { .. } => None,
        }
    }
}

#[derive(Clone)]
pub struct FindState {
    pub visible: bool,
    pub show_replace: bool,
    pub query: String,
    pub replacement: String,
    pub matches: Vec<MatchPos>,
    pub active: Option<usize>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub scope: FindScope,
    pub error: Option<String>,
    indexed_revision: Option<u64>,
}

impl FindState {
    pub fn new() -> Self {
        Self {
            visible: false,
            show_replace: false,
            query: String::new(),
            replacement: String::new(),
            matches: Vec::new(),
            active: None,
            case_sensitive: false,
            whole_word: false,
            use_regex: false,
            scope: FindScope::Document,
            error: None,
            indexed_revision: None,
        }
    }

    fn clear_results(&mut self) {
        self.matches.clear();
        self.active = None;
        self.error = None;
        self.indexed_revision = None;
    }

    // Single source of truth for query interpretation — keeps
    // `compute_matches_in_text` and the replace paths in lock-step.
    fn build_regex(&self) -> Result<Regex, regex::Error> {
        build_query_regex(
            &self.query,
            self.case_sensitive,
            self.whole_word,
            self.use_regex,
        )
    }

    fn compute_matches_in_text(&mut self, text: &str) {
        let previous_active = self.active;
        self.matches.clear();
        self.error = None;
        if self.query.is_empty() {
            self.active = None;
            return;
        }

        let regex = match self.build_regex() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("regex: {e}"));
                self.active = None;
                return;
            }
        };

        for (line_idx, line) in text.lines().enumerate() {
            let cells = cells_of_str(line);
            let line_byte_len = line.len();
            let line_char_len = cells
                .last()
                .map(|c| c.char_start + c.char_len as usize)
                .unwrap_or(0);
            for m in regex.find_iter(line) {
                let abs_byte = m.start();
                let end_byte = m.end();
                if abs_byte == end_byte {
                    // Skip zero-width matches (e.g. /^/, /\b/) — they have no
                    // selectable span and cause infinite loops in find/replace.
                    continue;
                }
                let start_idx = cell_partition_by_byte(&cells, abs_byte);
                let end_idx = cell_partition_by_byte(&cells, end_byte);
                let start_aligned = cells
                    .get(start_idx)
                    .map_or(abs_byte == line_byte_len, |c| c.byte_start == abs_byte);
                let end_aligned = cells
                    .get(end_idx)
                    .map_or(end_byte == line_byte_len, |c| c.byte_start == end_byte);
                if !(start_aligned && end_aligned) {
                    continue;
                }
                let col = cells.get(start_idx).map_or(line_char_len, |c| c.char_start);
                let end_col = cells.get(end_idx).map_or(line_char_len, |c| c.char_start);
                self.matches.push(MatchPos {
                    line: line_idx,
                    col,
                    char_len: end_col - col,
                });
            }
        }
        self.active = if self.matches.is_empty() {
            None
        } else {
            Some(previous_active.unwrap_or(0).min(self.matches.len() - 1))
        };
    }

    pub fn current_match_range(&self) -> Option<(Position, Position)> {
        let m = self.matches.get(self.active?)?;
        Some((
            Position {
                line: m.line,
                column: m.col,
            },
            Position {
                line: m.line,
                column: m.col + m.char_len,
            },
        ))
    }

    pub fn next(&mut self) {
        let len = self.matches.len();
        if len > 0 {
            self.active = Some(self.active.map_or(0, |current| (current + 1) % len));
        }
    }

    pub fn prev(&mut self) {
        let len = self.matches.len();
        if len > 0 {
            self.active = Some(match self.active {
                Some(0) | None => len - 1,
                Some(current) => current - 1,
            });
        }
    }

    fn find_nearest(&mut self, position: &Position) {
        if self.matches.is_empty() {
            self.active = None;
            return;
        }
        for (i, m) in self.matches.iter().enumerate() {
            if m.line > position.line || (m.line == position.line && m.col >= position.column) {
                self.active = Some(i);
                return;
            }
        }
        self.active = Some(0);
    }

    fn select_exact(&mut self, position: &Position) -> bool {
        let Some(index) = self
            .matches
            .iter()
            .position(|m| m.line == position.line && m.col == position.column)
        else {
            return false;
        };
        self.active = Some(index);
        true
    }

    fn is_stale(&self, revision: u64) -> bool {
        !self.query.is_empty() && self.indexed_revision != Some(revision)
    }

    pub(crate) fn reindex_for_tab(&mut self, tab: &EditorTab) {
        if self.query.is_empty() {
            self.clear_results();
            return;
        }
        self.compute_matches_in_text(&tab.buffer_text());
        if let Some(scope) = self.scope.selection_range_for(tab.id()) {
            let buffer = tab.buffer();
            let len = buffer.len_chars();
            let scope = scope.start.min(len)..scope.end.min(len);
            self.matches
                .retain(|m| scope_contains(&scope, &m.char_range_in(buffer)));
            match (self.matches.is_empty(), self.active) {
                (true, _) => self.active = None,
                (false, Some(index)) => self.active = Some(index.min(self.matches.len() - 1)),
                _ => {}
            }
        }
        self.indexed_revision = Some(tab.revision());
    }

    pub(crate) fn reindex_to_nearest(&mut self, tab: &EditorTab) {
        self.reindex_for_tab(tab);
        if !self.matches.is_empty() {
            self.align_to_visible_match(tab);
        }
    }

    pub(crate) fn ensure_current(&mut self, tab: &EditorTab) {
        if self.is_stale(tab.revision()) {
            self.reindex_for_tab(tab);
        }
    }

    pub(crate) fn sync_with_tab(&mut self, tab: &EditorTab) {
        if self.query.is_empty() {
            self.clear_results();
        } else {
            self.reindex_to_nearest(tab);
        }
    }

    pub(crate) fn sync_after_edit(&mut self, tab: &EditorTab) {
        if !self.query.is_empty() {
            self.reindex_to_nearest(tab);
        }
    }

    pub(crate) fn active_selection_set(&self, tab: &EditorTab) -> Option<SelectionSet> {
        if self.matches.is_empty() {
            return None;
        }
        let buffer = tab.buffer();
        let selections = self
            .matches
            .iter()
            .map(|m| Selection::from_range(m.char_range_in(buffer), false))
            .collect::<Vec<_>>();
        let primary = self.active.unwrap_or(0).min(selections.len() - 1);
        SelectionSet::from_selections(selections, primary).ok()
    }

    pub(crate) fn next_from(&mut self, position: Position) -> Option<Position> {
        self.select_relative_from(position, true)
    }

    pub(crate) fn prev_from(&mut self, position: Position) -> Option<Position> {
        self.select_relative_from(position, false)
    }

    pub(crate) fn search_word_from(
        &mut self,
        tab: &EditorTab,
        word: String,
        position: Position,
        forward: bool,
    ) -> Option<Position> {
        self.query = word;
        self.whole_word = true;
        self.case_sensitive = true;
        self.use_regex = false;
        self.scope = FindScope::Document;
        self.reindex_for_tab(tab);
        if forward {
            self.next_from(position)
        } else {
            self.prev_from(position)
        }
    }

    fn align_to_visible_match(&mut self, tab: &EditorTab) {
        if let Some(start) = self.selected_match_start(tab) {
            if self.select_exact(&start) {
                return;
            }
        }
        self.find_nearest(&tab.cursor_position());
    }

    fn selected_match_start(&self, tab: &EditorTab) -> Option<Position> {
        if self.query.is_empty() || !tab.has_selection() {
            return None;
        }
        let selected = tab.selected_range();
        if selected.end.saturating_sub(selected.start) != self.query.chars().count() {
            return None;
        }
        Some(char_to_position(tab.buffer(), selected.start))
    }

    fn select_relative_from(&mut self, position: Position, forward: bool) -> Option<Position> {
        let index = if forward {
            self.matches
                .iter()
                .position(|m| {
                    m.line > position.line || (m.line == position.line && m.col > position.column)
                })
                .or_else(|| (!self.matches.is_empty()).then_some(0))
        } else {
            self.matches
                .iter()
                .rposition(|m| {
                    m.line < position.line || (m.line == position.line && m.col < position.column)
                })
                .or_else(|| self.matches.len().checked_sub(1))
        }?;
        self.active = Some(index);
        let m = self.matches[index];
        Some(Position::new(m.line, m.col))
    }
}

fn scope_contains(scope: &Range<usize>, range: &Range<usize>) -> bool {
    range.start >= scope.start && range.end <= scope.end
}

impl Default for FindState {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a regex for `query` honouring case / whole-word / regex
/// toggles plus the smart-case fallback (lowercase queries are
/// case-insensitive). Shared between the find panel and cursor-add
/// gestures so flag semantics stay consistent across surfaces.
pub(crate) fn build_query_regex(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Result<Regex, regex::Error> {
    let ignore_case = !case_sensitive && !query.chars().any(|c| c.is_uppercase());
    let core = if use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let pattern = if whole_word {
        format!(r"(?:\b(?:{core})\b)")
    } else {
        core
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(ignore_case)
        .build()
}

pub(crate) fn replace_one_request(tab: &EditorTab, find: &FindState) -> Option<EditRequest> {
    let (start, end) = find.current_match_range()?;
    let regex = find.use_regex.then(|| find.build_regex().ok()).flatten();
    let template = find.replacement.clone();
    let buffer = tab.buffer();
    let range = position_to_char(buffer, start)..position_to_char(buffer, end);
    let replacement = regex.map_or(template.clone(), |re| {
        let line_start = buffer.char_to_byte(buffer.line_to_char(start.line));
        let match_start = buffer.char_to_byte(range.start);
        expand_match_replacement(
            &re,
            &line_display_text(buffer, start.line),
            match_start - line_start,
            &template,
        )
    });
    Some(EditRequest::single(
        EditKind::Other,
        UndoBoundary::Break,
        range,
        replacement,
    ))
}

pub(crate) fn replace_all_request(
    tab: &EditorTab,
    find: &FindState,
    cursor: Position,
) -> Option<EditRequest> {
    if find.query.is_empty() || find.matches.is_empty() {
        return None;
    }

    let regex = find.use_regex.then(|| find.build_regex().ok()).flatten();
    if find.use_regex && regex.is_none() {
        return None;
    }

    let buffer = tab.buffer();
    let mut changes = Vec::new();
    for m in find.matches.iter().copied() {
        let range = m.char_range_in(buffer);
        let replacement = regex.as_ref().map_or_else(
            || find.replacement.clone(),
            |re| {
                let line_start = buffer.char_to_byte(buffer.line_to_char(m.line));
                let match_start = buffer.char_to_byte(range.start);
                expand_match_replacement(
                    re,
                    &line_display_text(buffer, m.line),
                    match_start - line_start,
                    &find.replacement,
                )
            },
        );
        if buffer.slice(range.clone()) != replacement.as_str() {
            changes.push(TextChange::replace(range, replacement));
        }
    }
    if changes.is_empty() {
        return None;
    }

    let changes = TextChangeSet::new(changes, 0);
    Some(
        EditRequest::other_break(changes)
            .with_selection_after(SelectionAfter::CursorPosition(cursor)),
    )
}

fn expand_match_replacement(
    regex: &Regex,
    line: &str,
    byte_start_in_line: usize,
    template: &str,
) -> String {
    if let Some(caps) = regex.captures_at(line, byte_start_in_line) {
        if caps.get(0).is_some_and(|m| m.start() == byte_start_in_line) {
            let mut buf = String::new();
            caps.expand(template, &mut buf);
            return buf;
        }
    }
    template.to_string()
}
