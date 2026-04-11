use crate::position::Position;
use ropey::Rope;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchPos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone)]
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

    pub fn compute_matches_in_text(&mut self, text: &str) {
        self.matches.clear();
        if self.query.is_empty() {
            return;
        }
        for (line_idx, line) in text.lines().enumerate() {
            let mut start = 0;
            while let Some(byte_pos) = line[start..].find(&self.query) {
                let abs_byte = start + byte_pos;
                let col = line[..abs_byte].chars().count();
                self.matches.push(MatchPos {
                    line: line_idx,
                    col,
                });
                start = abs_byte + self.query.len();
            }
        }
        if self.matches.is_empty() {
            self.current = 0;
        } else {
            self.current = self.current.min(self.matches.len() - 1);
        }
    }

    pub fn compute_matches_in_rope(&mut self, buffer: &Rope) {
        self.compute_matches_in_text(&buffer.to_string());
    }

    pub fn current_match_range(&self) -> Option<(Position, Position)> {
        let m = self.matches.get(self.current)?;
        Some((
            Position {
                line: m.line,
                column: m.col,
            },
            Position {
                line: m.line,
                column: m.col + self.query.chars().count(),
            },
        ))
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

    pub fn find_nearest(&mut self, position: &Position) {
        for (i, m) in self.matches.iter().enumerate() {
            if m.line > position.line || (m.line == position.line && m.col >= position.column) {
                self.current = i;
                return;
            }
        }
        self.current = 0;
    }

    pub fn select_exact(&mut self, position: &Position) -> bool {
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
