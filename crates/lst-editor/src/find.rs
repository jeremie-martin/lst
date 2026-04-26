use crate::position::Position;
use crate::selection::{cell_partition_by_byte, cells_of_str};
use crate::TabId;
use regex::{Regex, RegexBuilder};
use std::ops::Range;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchPos {
    pub line: usize,
    pub col: usize,
    pub char_len: usize,
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

    pub fn selection_range_for(self, tab_id: TabId) -> Option<Range<usize>> {
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
    dirty_since: Option<Instant>,
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
            dirty_since: None,
        }
    }

    pub fn clear_results(&mut self) {
        self.matches.clear();
        self.active = None;
        self.error = None;
        self.indexed_revision = None;
        self.dirty_since = None;
    }

    // Single source of truth for query interpretation — keeps
    // `compute_matches_in_text` and the replace paths in lock-step.
    pub fn build_regex(&self) -> Result<Regex, regex::Error> {
        let ignore_case =
            !self.case_sensitive && !self.query.chars().any(|c| c.is_uppercase());
        let core = if self.use_regex {
            self.query.clone()
        } else {
            regex::escape(&self.query)
        };
        let pattern = if self.whole_word {
            format!(r"(?:\b(?:{core})\b)")
        } else {
            core
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(ignore_case)
            .build()
    }

    pub fn compute_matches_in_text(&mut self, text: &str) {
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
                let col = cells
                    .get(start_idx)
                    .map_or(line_char_len, |c| c.char_start);
                let end_col = cells
                    .get(end_idx)
                    .map_or(line_char_len, |c| c.char_start);
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

    pub fn find_nearest(&mut self, position: &Position) {
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

    pub fn select_exact(&mut self, position: &Position) -> bool {
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

    pub fn mark_dirty(&mut self) {
        self.mark_dirty_at(Instant::now());
    }

    pub fn mark_dirty_at(&mut self, when: Instant) {
        self.dirty_since = Some(when);
    }

    pub fn finish_reindex(&mut self, revision: u64) {
        self.indexed_revision = Some(revision);
        self.dirty_since = None;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    pub fn is_stale(&self, revision: u64) -> bool {
        !self.query.is_empty() && (self.is_dirty() || self.indexed_revision != Some(revision))
    }
}

impl Default for FindState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grapheme_boundary_filters_mid_cluster_match() {
        // First line has the composed form `é` (U+00E9, single codepoint).
        // Second line has the decomposed form `e` + U+0301 (combining acute).
        // Querying the combining mark alone must NOT match — the only place
        // where the byte sequence appears is mid-cluster on the second line.
        let mut find = FindState::new();
        find.query = "\u{0301}".into();
        find.compute_matches_in_text("caf\u{00E9}\ncafe\u{0301}");
        assert_eq!(find.matches.len(), 0, "mid-cluster match must be filtered");

        // Querying the full decomposed cluster matches the second line only.
        find.query = "e\u{0301}".into();
        find.compute_matches_in_text("caf\u{00E9}\ncafe\u{0301}");
        assert_eq!(find.matches.len(), 1);
        assert_eq!(find.matches[0].line, 1);
        assert_eq!(find.matches[0].col, 3);
        assert_eq!(find.matches[0].char_len, 2);
    }

    #[test]
    fn smart_case_lowercase_query_matches_mixed_case() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.compute_matches_in_text("Foo foo FOO");
        assert_eq!(find.matches.len(), 3);
    }

    #[test]
    fn smart_case_uppercase_query_is_strict() {
        let mut find = FindState::new();
        find.query = "Foo".into();
        find.compute_matches_in_text("Foo foo FOO");
        assert_eq!(find.matches.len(), 1);
        assert_eq!(find.matches[0].col, 0);
    }

    #[test]
    fn case_sensitive_flag_disables_smart_case() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.case_sensitive = true;
        find.compute_matches_in_text("Foo foo FOO");
        assert_eq!(find.matches.len(), 1);
        assert_eq!(find.matches[0].col, 4);
    }

    #[test]
    fn whole_word_rejects_substring_inside_identifier() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.whole_word = true;
        find.compute_matches_in_text("foobar foo_bar foo bar");
        // `foobar` and `foo_bar` are rejected; only the standalone `foo` matches.
        assert_eq!(find.matches.len(), 1);
        assert_eq!(find.matches[0].line, 0);
        assert_eq!(find.matches[0].col, 15);
    }

    #[test]
    fn whole_word_accepts_at_punctuation_boundary() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.whole_word = true;
        find.compute_matches_in_text("(foo) foo!");
        assert_eq!(find.matches.len(), 2);
    }

    #[test]
    fn regex_capture_groups_match_with_correct_char_len() {
        let mut find = FindState::new();
        find.query = r"(\w+)@(\w+)".into();
        find.use_regex = true;
        find.compute_matches_in_text("alice@example bob@host");
        assert_eq!(find.matches.len(), 2);
        assert_eq!(find.matches[0].col, 0);
        assert_eq!(find.matches[0].char_len, "alice@example".len());
        assert_eq!(find.matches[1].col, 14);
        assert_eq!(find.matches[1].char_len, "bob@host".len());
    }

    #[test]
    fn regex_invalid_pattern_sets_error_and_clears_matches() {
        let mut find = FindState::new();
        find.query = "[".into();
        find.use_regex = true;
        find.compute_matches_in_text("[abc] [def]");
        assert!(find.matches.is_empty());
        assert!(find.error.is_some(), "invalid regex must populate error");
        assert!(find.active.is_none());
    }

    #[test]
    fn computes_matches_and_current_range() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.compute_matches_in_text("foo bar\nbaz foo");

        assert_eq!(find.matches.len(), 2);
        assert_eq!(
            find.current_match_range(),
            Some((
                Position { line: 0, column: 0 },
                Position { line: 0, column: 3 }
            ))
        );
        find.next();
        assert_eq!(
            find.current_match_range(),
            Some((
                Position { line: 1, column: 4 },
                Position { line: 1, column: 7 }
            ))
        );
    }
}
