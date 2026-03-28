use iced::widget::text_editor;

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
        }
    }

    /// Recompute match positions from the full document text.
    pub fn compute_matches(&mut self, text: &str) {
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
}
