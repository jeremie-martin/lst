use iced::widget::text_editor;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchPos {
    pub line: usize,
    pub col: usize,
}

pub struct FindState {
    pub visible: bool,
    pub show_replace: bool,
    pub query: String,
    pub replacement: String,
    pub matches: Vec<MatchPos>,
    pub current: usize,
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
            current: 0,
            indexed_revision: None,
            dirty_since: None,
        }
    }

    pub fn clear_results(&mut self) {
        self.matches.clear();
        self.current = 0;
        self.indexed_revision = None;
        self.dirty_since = None;
    }

    /// Recompute match positions from the full document text.
    pub fn compute_matches(&mut self, text: &str) {
        self.compute_matches_in_lines(text.lines());
    }

    /// Recompute match positions from cached document lines without rebuilding a full document string.
    pub fn compute_matches_lines(&mut self, lines: &[String]) {
        self.compute_matches_in_lines(lines.iter().map(String::as_str));
    }

    /// Move cursor to the current match and select it.
    pub fn navigate_to_current(&self, content: &mut text_editor::Content) {
        if self.matches.is_empty() {
            return;
        }
        let m = &self.matches[self.current];
        let start = text_editor::Position {
            line: m.line,
            column: m.col,
        };
        let end = text_editor::Position {
            line: m.line,
            column: m.col + self.query.chars().count(),
        };
        content.move_to(text_editor::Cursor {
            position: end,
            selection: Some(start),
        });
    }

    pub fn vim_next_from_cursor(
        &mut self,
        position: &text_editor::Position,
    ) -> Option<text_editor::Position> {
        let index = self
            .matches
            .iter()
            .position(|m| {
                m.line > position.line || (m.line == position.line && m.col > position.column)
            })
            .or_else(|| (!self.matches.is_empty()).then_some(0))?;
        self.current = index;
        Some(self.match_start(index))
    }

    pub fn vim_prev_from_cursor(
        &mut self,
        position: &text_editor::Position,
    ) -> Option<text_editor::Position> {
        let index = self
            .matches
            .iter()
            .rposition(|m| {
                m.line < position.line || (m.line == position.line && m.col < position.column)
            })
            .or_else(|| self.matches.len().checked_sub(1))?;
        self.current = index;
        Some(self.match_start(index))
    }

    pub fn next(&mut self) {
        if !self.matches.is_empty() {
            self.current = (self.current + 1) % self.matches.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.matches.is_empty() {
            self.current = if self.current == 0 {
                self.matches.len() - 1
            } else {
                self.current - 1
            };
        }
    }

    /// Jump to the nearest match at or after the given cursor position.
    pub fn find_nearest(&mut self, position: &text_editor::Position) {
        for (i, m) in self.matches.iter().enumerate() {
            if m.line > position.line || (m.line == position.line && m.col >= position.column) {
                self.current = i;
                return;
            }
        }
        self.current = 0;
    }

    pub fn select_exact(&mut self, position: &text_editor::Position) -> bool {
        let Some(index) = self
            .matches
            .iter()
            .position(|m| m.line == position.line && m.col == position.column)
        else {
            return false;
        };

        self.current = index;
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

    pub fn dirty_since(&self) -> Option<Instant> {
        self.dirty_since
    }

    pub fn indexed_revision(&self) -> Option<u64> {
        self.indexed_revision
    }

    pub fn is_stale(&self, revision: u64) -> bool {
        !self.query.is_empty() && (self.is_dirty() || self.indexed_revision != Some(revision))
    }

    fn match_start(&self, index: usize) -> text_editor::Position {
        let m = &self.matches[index];
        text_editor::Position {
            line: m.line,
            column: m.col,
        }
    }

    fn compute_matches_in_lines<'a, I>(&mut self, lines: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }

        let query = self.query.as_str();
        let query_is_ascii = query.is_ascii();

        for (line_idx, line) in lines.into_iter().enumerate() {
            let mut start = 0;
            let ascii_columns = query_is_ascii && line.is_ascii();

            while let Some(byte_pos) = line[start..].find(query) {
                let abs_byte = start + byte_pos;
                let col = if ascii_columns {
                    abs_byte
                } else {
                    line[..abs_byte].chars().count()
                };

                self.matches.push(MatchPos {
                    line: line_idx,
                    col,
                });
                start = abs_byte + query.len();
            }
        }

        if self.matches.is_empty() {
            self.current = 0;
        } else {
            self.current = self.current.min(self.matches.len() - 1);
        }
    }
}

#[cfg(all(test, feature = "internal-invariants"))]
mod tests {
    use super::*;

    fn pos(line: usize, column: usize) -> text_editor::Position {
        text_editor::Position { line, column }
    }

    #[test]
    fn vim_next_is_cursor_relative_and_wraps() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.compute_matches("foo bar foo");

        assert_eq!(find.vim_next_from_cursor(&pos(0, 0)), Some(pos(0, 8)));
        assert_eq!(find.current, 1);
        assert_eq!(find.vim_next_from_cursor(&pos(0, 8)), Some(pos(0, 0)));
        assert_eq!(find.current, 0);
    }

    #[test]
    fn vim_prev_is_cursor_relative_and_wraps() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.compute_matches("foo bar foo");

        assert_eq!(find.vim_prev_from_cursor(&pos(0, 8)), Some(pos(0, 0)));
        assert_eq!(find.current, 0);
        assert_eq!(find.vim_prev_from_cursor(&pos(0, 0)), Some(pos(0, 8)));
        assert_eq!(find.current, 1);
    }

    #[test]
    fn vim_search_advances_from_cursor_not_stale_index() {
        let mut find = FindState::new();
        find.query = "foo".into();
        find.compute_matches("foo\nbar\nfoo");
        find.current = 1;

        assert_eq!(find.vim_next_from_cursor(&pos(0, 0)), Some(pos(2, 0)));
        assert_eq!(find.current, 1);
    }

    #[test]
    fn compute_matches_lines_matches_text_results() {
        let mut from_text = FindState::new();
        from_text.query = "foo".into();
        from_text.compute_matches("foo\nbar foo");

        let mut from_lines = FindState::new();
        from_lines.query = "foo".into();
        let lines = vec!["foo".to_string(), "bar foo".to_string()];
        from_lines.compute_matches_lines(&lines);

        assert_eq!(from_lines.matches, from_text.matches);
    }

    #[test]
    fn compute_matches_lines_preserves_unicode_columns() {
        let mut find = FindState::new();
        find.query = "é".into();
        let lines = vec!["aéa".to_string()];

        find.compute_matches_lines(&lines);

        assert_eq!(find.matches, vec![MatchPos { line: 0, col: 1 }]);
    }
}
