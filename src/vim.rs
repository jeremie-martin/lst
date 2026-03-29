//! Vim mode state machine.
//!
//! Pure keystroke → command translation. The caller (main.rs) executes
//! commands via iced's text_editor primitives.

use iced::keyboard;
use iced::widget::text_editor;

type Position = text_editor::Position;

fn pos(line: usize, column: usize) -> Position {
    Position { line, column }
}

// ── Public types ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

pub struct VimState {
    pub mode: Mode,
    pub register: Register,
    pub visual_anchor: Option<Position>,
    pending: Pending,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum VimCommand {
    MoveTo(Position),
    Select { anchor: Position, head: Position },
    DeleteRange { from: Position, to: Position },
    DeleteLines { first: usize, last: usize },
    ChangeRange { from: Position, to: Position },
    ChangeLines { first: usize, last: usize },
    YankRange { from: Position, to: Position },
    YankLines { first: usize, last: usize },
    EnterInsert,
    EnterNormal,
    PasteAfter,
    PasteBefore,
    OpenLineBelow,
    OpenLineAbove,
    JoinLines { count: usize },
    ReplaceChar(char),
    Undo,
    Redo,
    OpenFind,
    FindNext,
    FindPrev,
    Noop,
}

pub struct TextSnapshot {
    pub lines: Vec<String>,
    pub cursor: Position,
}

impl TextSnapshot {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
}

#[derive(Clone, Debug)]
pub enum Register {
    Empty,
    Char(String),
    Line(String),
}

// ── Private types ───────────────────────────────────────────────────────────

#[derive(Default)]
struct Pending {
    count: Option<usize>,
    operator: Option<Operator>,
    operator_count: Option<usize>,
    partial: Option<char>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Delete,
    Change,
    Yank,
}

enum Motion {
    Left,
    Right,
    Down,
    Up,
    WordForward,
    WordBackward,
    WordEnd,
    BigWordForward,
    BigWordBackward,
    BigWordEnd,
    LineStart,
    LineEnd,
    FirstNonBlank,
    DocumentStart,
    DocumentEnd,
    FindChar(char),
    TillChar(char),
    FindCharBack(char),
    TillCharBack(char),
    MatchBracket,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Punct,
    Space,
}

// ── VimState ────────────────────────────────────────────────────────────────

impl VimState {
    pub fn new() -> Self {
        Self {
            mode: Mode::Insert,
            register: Register::Empty,
            visual_anchor: None,
            pending: Pending::default(),
        }
    }

    pub fn handle_key(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        match self.mode {
            Mode::Normal => self.handle_normal(key, mods, text),
            Mode::Insert => vec![], // caller lets iced handle it
            Mode::Visual | Mode::VisualLine => self.handle_visual(key, mods, text),
        }
    }

    pub fn pending_display(&self) -> String {
        let mut s = String::new();
        if let Some(n) = self.pending.operator_count {
            s.push_str(&n.to_string());
        }
        match self.pending.operator {
            Some(Operator::Delete) => s.push('d'),
            Some(Operator::Change) => s.push('c'),
            Some(Operator::Yank) => s.push('y'),
            None => {}
        }
        if let Some(n) = self.pending.count {
            s.push_str(&n.to_string());
        }
        if let Some(p) = self.pending.partial {
            s.push(p);
        }
        s
    }

    pub fn clear_pending(&mut self) {
        self.pending = Pending::default();
    }

    pub fn enter_normal_from_escape(
        &mut self,
        cursor: Position,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        match self.mode {
            Mode::Insert => {
                self.mode = Mode::Normal;
                self.clear_pending();
                let col = if line_len(text, cursor.line) > 0 {
                    cursor
                        .column
                        .min(line_len(text, cursor.line).saturating_sub(1))
                } else {
                    0
                };
                if col != cursor.column {
                    vec![VimCommand::MoveTo(pos(cursor.line, col))]
                } else {
                    vec![VimCommand::Noop]
                }
            }
            Mode::Visual | Mode::VisualLine => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.clear_pending();
                // Clear selection by moving to current cursor
                vec![VimCommand::MoveTo(cursor)]
            }
            Mode::Normal => {
                self.clear_pending();
                vec![VimCommand::Noop]
            }
        }
    }

    // ── Normal mode ─────────────────────────────────────────────────────

    fn handle_normal(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        if mods.command() {
            if let keyboard::Key::Character(c) = key {
                if c.as_str() == "r" {
                    self.clear_pending();
                    return vec![VimCommand::Redo];
                }
            }
            return vec![VimCommand::Noop];
        }

        if let keyboard::Key::Named(named) = key {
            if let Some(m) = named_key_to_motion(named) {
                if let Some(op) = self.pending.operator.take() {
                    return self.operator_with_computed_motion(op, m, text);
                }
                return self.motion_move(m, text);
            }
            return vec![VimCommand::Noop];
        }

        let c = match key {
            keyboard::Key::Character(s) => match s.as_str().chars().next() {
                Some(c) => c,
                None => return vec![VimCommand::Noop],
            },
            _ => return vec![VimCommand::Noop],
        };

        if let Some(partial) = self.pending.partial.take() {
            return self.resolve_partial(partial, c, text);
        }

        if c == '0' && self.pending.count.is_none() {
            // 0 with no count in progress = line start motion
            if let Some(op) = self.pending.operator.take() {
                return self.operator_with_computed_motion(op, Motion::LineStart, text);
            }
            return self.motion_move(Motion::LineStart, text);
        }
        if c.is_ascii_digit() {
            let digit = c.to_digit(10).unwrap() as usize;
            self.pending.count = Some(self.pending.count.unwrap_or(0) * 10 + digit);
            return vec![VimCommand::Noop];
        }

        if let Some(op) = self.pending.operator {
            // Text object prefixes
            if c == 'i' || c == 'a' {
                self.pending.partial = Some(c);
                return vec![VimCommand::Noop];
            }

            let doubled = matches!(
                (op, c),
                (Operator::Delete, 'd') | (Operator::Change, 'c') | (Operator::Yank, 'y')
            );
            if doubled {
                let count = self.motion_count().unwrap_or(1);
                self.pending.operator = None;
                let last = (text.cursor.line + count - 1).min(text.line_count().saturating_sub(1));
                return self.line_operator(op, text.cursor.line, last);
            }

            // Try as motion
            if let Some(motion) = char_to_motion(c) {
                return self.operator_with_computed_motion(op, motion, text);
            }

            // Two-char sequence starters
            if matches!(c, 'g' | 'f' | 't' | 'F' | 'T') {
                self.pending.partial = Some(c);
                return vec![VimCommand::Noop];
            }

            // Unknown — cancel
            self.clear_pending();
            return vec![VimCommand::Noop];
        }

        // No operator pending — try motion
        if let Some(motion) = char_to_motion(c) {
            return self.motion_move(motion, text);
        }

        // Operators
        if matches!(c, 'd' | 'c' | 'y') {
            self.pending.operator = Some(match c {
                'd' => Operator::Delete,
                'c' => Operator::Change,
                'y' => Operator::Yank,
                _ => unreachable!(),
            });
            self.pending.operator_count = self.pending.count.take();
            return vec![VimCommand::Noop];
        }

        // Two-char sequence starters
        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T' | 'r') {
            self.pending.partial = Some(c);
            return vec![VimCommand::Noop];
        }

        let count = self.motion_count().unwrap_or(1);
        self.clear_pending();

        match c {
            'i' => vec![VimCommand::EnterInsert],
            'a' => {
                let col = (text.cursor.column + 1).min(line_len(text, text.cursor.line));
                vec![
                    VimCommand::MoveTo(pos(text.cursor.line, col)),
                    VimCommand::EnterInsert,
                ]
            }
            'I' => {
                let col = first_non_blank(text, text.cursor.line);
                vec![
                    VimCommand::MoveTo(pos(text.cursor.line, col)),
                    VimCommand::EnterInsert,
                ]
            }
            'A' => {
                let col = line_len(text, text.cursor.line);
                vec![
                    VimCommand::MoveTo(pos(text.cursor.line, col)),
                    VimCommand::EnterInsert,
                ]
            }
            'o' => vec![VimCommand::OpenLineBelow],
            'O' => vec![VimCommand::OpenLineAbove],
            'x' => {
                let ll = line_len(text, text.cursor.line);
                if ll == 0 {
                    return vec![VimCommand::Noop];
                }
                let end = (text.cursor.column + count - 1).min(ll.saturating_sub(1));
                vec![VimCommand::DeleteRange {
                    from: text.cursor,
                    to: pos(text.cursor.line, end),
                }]
            }
            'X' => {
                if text.cursor.column == 0 {
                    return vec![VimCommand::Noop];
                }
                let start = text.cursor.column.saturating_sub(count);
                vec![VimCommand::DeleteRange {
                    from: pos(text.cursor.line, start),
                    to: pos(text.cursor.line, text.cursor.column - 1),
                }]
            }
            's' => {
                let ll = line_len(text, text.cursor.line);
                if ll == 0 {
                    return vec![VimCommand::EnterInsert];
                }
                let end = (text.cursor.column + count - 1).min(ll.saturating_sub(1));
                vec![VimCommand::ChangeRange {
                    from: text.cursor,
                    to: pos(text.cursor.line, end),
                }]
            }
            'D' => {
                let ll = line_len(text, text.cursor.line);
                if ll == 0 || text.cursor.column >= ll {
                    return vec![VimCommand::Noop];
                }
                vec![VimCommand::DeleteRange {
                    from: text.cursor,
                    to: pos(text.cursor.line, ll - 1),
                }]
            }
            'C' => {
                let ll = line_len(text, text.cursor.line);
                if ll == 0 || text.cursor.column >= ll {
                    return vec![VimCommand::EnterInsert];
                }
                vec![VimCommand::ChangeRange {
                    from: text.cursor,
                    to: pos(text.cursor.line, ll - 1),
                }]
            }
            'J' => vec![VimCommand::JoinLines { count }],
            'p' => vec![VimCommand::PasteAfter],
            'P' => vec![VimCommand::PasteBefore],
            'u' => vec![VimCommand::Undo],
            'v' => {
                self.mode = Mode::Visual;
                self.visual_anchor = Some(text.cursor);
                vec![VimCommand::Noop]
            }
            'V' => {
                self.mode = Mode::VisualLine;
                self.visual_anchor = Some(text.cursor);
                let ll = line_len(text, text.cursor.line);
                vec![VimCommand::Select {
                    anchor: pos(text.cursor.line, 0),
                    head: pos(text.cursor.line, ll.saturating_sub(1)),
                }]
            }
            '/' => vec![VimCommand::OpenFind],
            'n' => vec![VimCommand::FindNext],
            'N' => vec![VimCommand::FindPrev],
            _ => vec![VimCommand::Noop],
        }
    }

    // ── Visual mode ─────────────────────────────────────────────────────

    fn handle_visual(
        &mut self,
        key: &keyboard::Key,
        mods: keyboard::Modifiers,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        if mods.command() {
            if let keyboard::Key::Character(c) = key {
                if c.as_str() == "r" {
                    return vec![VimCommand::Redo];
                }
            }
            return vec![VimCommand::Noop];
        }

        if let keyboard::Key::Named(named) = key {
            if let Some(m) = named_key_to_motion(named) {
                let count = self.pending.count.take();
                let target = compute_motion(&m, text, count);
                return self.visual_select(target, text);
            }
            return vec![VimCommand::Noop];
        }

        let c = match key {
            keyboard::Key::Character(s) => match s.as_str().chars().next() {
                Some(c) => c,
                None => return vec![VimCommand::Noop],
            },
            _ => return vec![VimCommand::Noop],
        };

        if let Some(partial) = self.pending.partial.take() {
            // In visual mode, partial sequences are just motions (f, t, g)
            let motion = match partial {
                'f' => Some(Motion::FindChar(c)),
                't' => Some(Motion::TillChar(c)),
                'F' => Some(Motion::FindCharBack(c)),
                'T' => Some(Motion::TillCharBack(c)),
                'g' if c == 'g' => Some(Motion::DocumentStart),
                _ => None,
            };
            if let Some(m) = motion {
                let target = compute_motion(&m, text, None);
                return self.visual_select(target, text);
            }
            return vec![VimCommand::Noop];
        }

        // Digits for count
        if c.is_ascii_digit() && (c != '0' || self.pending.count.is_some()) {
            let digit = c.to_digit(10).unwrap() as usize;
            self.pending.count = Some(self.pending.count.unwrap_or(0) * 10 + digit);
            return vec![VimCommand::Noop];
        }

        let anchor = self.visual_anchor.unwrap_or(text.cursor);
        let is_line = self.mode == Mode::VisualLine;

        // Operators on selection
        match c {
            'd' | 'x' => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.clear_pending();
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![VimCommand::DeleteLines { first, last }];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![VimCommand::DeleteRange { from, to }];
            }
            'c' | 's' => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.clear_pending();
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![VimCommand::ChangeLines { first, last }];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![VimCommand::ChangeRange { from, to }];
            }
            'y' => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                self.clear_pending();
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![VimCommand::YankLines { first, last }];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![VimCommand::YankRange { from, to }];
            }
            'v' => {
                if self.mode == Mode::Visual {
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                    self.clear_pending();
                    return vec![VimCommand::MoveTo(text.cursor)];
                } else {
                    self.mode = Mode::Visual;
                    return vec![VimCommand::Select {
                        anchor,
                        head: text.cursor,
                    }];
                }
            }
            'V' => {
                if self.mode == Mode::VisualLine {
                    self.mode = Mode::Normal;
                    self.visual_anchor = None;
                    self.clear_pending();
                    return vec![VimCommand::MoveTo(text.cursor)];
                } else {
                    self.mode = Mode::VisualLine;
                    return self.visual_select(text.cursor, text);
                }
            }
            _ => {}
        }

        // Two-char sequence starters
        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T') {
            self.pending.partial = Some(c);
            return vec![VimCommand::Noop];
        }

        // Try as motion — extend selection
        if c == '0' && self.pending.count.is_none() {
            let target = compute_motion(&Motion::LineStart, text, None);
            return self.visual_select(target, text);
        }
        if let Some(motion) = char_to_motion(c) {
            let count = self.pending.count.take();
            let target = compute_motion(&motion, text, count);
            return self.visual_select(target, text);
        }

        // Search
        match c {
            '/' => return vec![VimCommand::OpenFind],
            'n' => return vec![VimCommand::FindNext],
            'N' => return vec![VimCommand::FindPrev],
            'u' => return vec![VimCommand::Undo],
            _ => {}
        }

        vec![VimCommand::Noop]
    }

    fn visual_select(&self, head: Position, text: &TextSnapshot) -> Vec<VimCommand> {
        let anchor = self.visual_anchor.unwrap_or(text.cursor);
        if self.mode == Mode::VisualLine {
            let (first, last) = ordered_lines(anchor.line, head.line);
            let last_col = line_len(text, last).saturating_sub(1);
            vec![VimCommand::Select {
                anchor: pos(first, 0),
                head: pos(last, last_col),
            }]
        } else {
            vec![VimCommand::Select { anchor, head }]
        }
    }

    // ── Partial resolution ──────────────────────────────────────────────

    fn resolve_partial(&mut self, partial: char, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        match partial {
            'g' if c == 'g' => {
                if let Some(op) = self.pending.operator.take() {
                    return self.operator_with_computed_motion(op, Motion::DocumentStart, text);
                }
                self.motion_move(Motion::DocumentStart, text)
            }
            'f' => self.resolve_motion_partial(Motion::FindChar(c), text),
            't' => self.resolve_motion_partial(Motion::TillChar(c), text),
            'F' => self.resolve_motion_partial(Motion::FindCharBack(c), text),
            'T' => self.resolve_motion_partial(Motion::TillCharBack(c), text),
            'r' => {
                self.clear_pending();
                if c == '\n' || line_len(text, text.cursor.line) == 0 {
                    vec![VimCommand::Noop]
                } else {
                    vec![VimCommand::ReplaceChar(c)]
                }
            }
            'i' | 'a' => {
                // Text object after operator
                let inner = partial == 'i';
                if let Some(range) = text_object(text, c, inner) {
                    if let Some(op) = self.pending.operator.take() {
                        self.pending.count = None;
                        self.pending.operator_count = None;
                        return self.range_operator(op, range.0, range.1);
                    }
                }
                self.clear_pending();
                vec![VimCommand::Noop]
            }
            _ => {
                self.clear_pending();
                vec![VimCommand::Noop]
            }
        }
    }

    fn resolve_motion_partial(&mut self, motion: Motion, text: &TextSnapshot) -> Vec<VimCommand> {
        if let Some(op) = self.pending.operator.take() {
            self.operator_with_computed_motion(op, motion, text)
        } else {
            self.motion_move(motion, text)
        }
    }

    // ── Motion + operator helpers ───────────────────────────────────────

    fn motion_count(&mut self) -> Option<usize> {
        let oc = self.pending.operator_count.take();
        let mc = self.pending.count.take();
        match (oc, mc) {
            (None, None) => None,
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (Some(a), Some(b)) => Some(a * b),
        }
    }

    fn motion_move(&mut self, motion: Motion, text: &TextSnapshot) -> Vec<VimCommand> {
        let count = self.motion_count();
        self.clear_pending();
        let target = compute_motion(&motion, text, count);
        vec![VimCommand::MoveTo(target)]
    }

    fn operator_with_computed_motion(
        &mut self,
        op: Operator,
        motion: Motion,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        let count = self.motion_count();
        self.clear_pending();

        if is_line_wise(&motion) {
            let target = compute_motion(&motion, text, count);
            let (first, last) = ordered_lines(text.cursor.line, target.line);
            return self.line_operator(op, first, last);
        }

        let target = compute_motion(&motion, text, count);
        if target == text.cursor {
            return vec![VimCommand::Noop];
        }

        let (from, mut to) = ordered(text.cursor, target);

        // Exclusive motions: don't include the target character
        if !is_inclusive(&motion) {
            // Shrink `to` by one character
            if to.column > 0 {
                to.column -= 1;
            } else if to.line > from.line {
                to.line -= 1;
                to.column = line_len(text, to.line).saturating_sub(1);
            }
            // If range collapsed, it's a noop
            if pos_lt(&to, &from) {
                return vec![VimCommand::Noop];
            }
        }

        self.range_operator(op, from, to)
    }

    fn line_operator(&self, op: Operator, first: usize, last: usize) -> Vec<VimCommand> {
        match op {
            Operator::Delete => vec![VimCommand::DeleteLines { first, last }],
            Operator::Change => vec![VimCommand::ChangeLines { first, last }],
            Operator::Yank => vec![VimCommand::YankLines { first, last }],
        }
    }

    fn range_operator(&self, op: Operator, from: Position, to: Position) -> Vec<VimCommand> {
        match op {
            Operator::Delete => vec![VimCommand::DeleteRange { from, to }],
            Operator::Change => vec![VimCommand::ChangeRange { from, to }],
            Operator::Yank => vec![VimCommand::YankRange { from, to }, VimCommand::MoveTo(from)],
        }
    }
}

// ── Motion computation ──────────────────────────────────────────────────────

fn compute_motion(motion: &Motion, text: &TextSnapshot, count: Option<usize>) -> Position {
    let n = count.unwrap_or(1);
    match motion {
        Motion::Left => {
            let col = text.cursor.column.saturating_sub(n);
            pos(text.cursor.line, col)
        }
        Motion::Right => {
            let max = line_len(text, text.cursor.line).saturating_sub(1);
            pos(text.cursor.line, (text.cursor.column + n).min(max))
        }
        Motion::Down => {
            let line = (text.cursor.line + n).min(text.line_count().saturating_sub(1));
            let col = text
                .cursor
                .column
                .min(line_len(text, line).saturating_sub(1));
            pos(line, col)
        }
        Motion::Up => {
            let line = text.cursor.line.saturating_sub(n);
            let col = text
                .cursor
                .column
                .min(line_len(text, line).saturating_sub(1));
            pos(line, col)
        }
        Motion::WordForward => {
            let (mut l, mut c) = (text.cursor.line, text.cursor.column);
            for _ in 0..n {
                let (nl, nc) = word_forward(text, l, c, false);
                l = nl;
                c = nc;
            }
            pos(l, c)
        }
        Motion::WordBackward => {
            let (mut l, mut c) = (text.cursor.line, text.cursor.column);
            for _ in 0..n {
                let (nl, nc) = word_backward(text, l, c, false);
                l = nl;
                c = nc;
            }
            pos(l, c)
        }
        Motion::WordEnd => {
            let (mut l, mut c) = (text.cursor.line, text.cursor.column);
            for _ in 0..n {
                let (nl, nc) = word_end(text, l, c, false);
                l = nl;
                c = nc;
            }
            pos(l, c)
        }
        Motion::BigWordForward => {
            let (mut l, mut c) = (text.cursor.line, text.cursor.column);
            for _ in 0..n {
                let (nl, nc) = word_forward(text, l, c, true);
                l = nl;
                c = nc;
            }
            pos(l, c)
        }
        Motion::BigWordBackward => {
            let (mut l, mut c) = (text.cursor.line, text.cursor.column);
            for _ in 0..n {
                let (nl, nc) = word_backward(text, l, c, true);
                l = nl;
                c = nc;
            }
            pos(l, c)
        }
        Motion::BigWordEnd => {
            let (mut l, mut c) = (text.cursor.line, text.cursor.column);
            for _ in 0..n {
                let (nl, nc) = word_end(text, l, c, true);
                l = nl;
                c = nc;
            }
            pos(l, c)
        }
        Motion::LineStart => pos(text.cursor.line, 0),
        Motion::LineEnd => {
            let ll = line_len(text, text.cursor.line);
            pos(text.cursor.line, ll.saturating_sub(1))
        }
        Motion::FirstNonBlank => pos(text.cursor.line, first_non_blank(text, text.cursor.line)),
        Motion::DocumentStart => match count {
            Some(n) => {
                let line = n.saturating_sub(1).min(text.line_count().saturating_sub(1));
                pos(line, first_non_blank(text, line))
            }
            None => pos(0, first_non_blank(text, 0)),
        },
        Motion::DocumentEnd => match count {
            Some(n) => {
                let line = n.saturating_sub(1).min(text.line_count().saturating_sub(1));
                pos(line, first_non_blank(text, line))
            }
            None => {
                let line = text.line_count().saturating_sub(1);
                pos(line, first_non_blank(text, line))
            }
        },
        Motion::FindChar(ch) => {
            let chars = line_chars(text, text.cursor.line);
            let mut found = 0;
            for (i, &c) in chars.iter().enumerate().skip(text.cursor.column + 1) {
                if c == *ch {
                    found += 1;
                    if found == n {
                        return pos(text.cursor.line, i);
                    }
                }
            }
            text.cursor
        }
        Motion::TillChar(ch) => {
            let chars = line_chars(text, text.cursor.line);
            let mut found = 0;
            for (i, &c) in chars.iter().enumerate().skip(text.cursor.column + 1) {
                if c == *ch {
                    found += 1;
                    if found == n {
                        return pos(
                            text.cursor.line,
                            i.saturating_sub(1).max(text.cursor.column),
                        );
                    }
                }
            }
            text.cursor
        }
        Motion::FindCharBack(ch) => {
            let chars = line_chars(text, text.cursor.line);
            let mut found = 0;
            for i in (0..text.cursor.column).rev() {
                if chars[i] == *ch {
                    found += 1;
                    if found == n {
                        return pos(text.cursor.line, i);
                    }
                }
            }
            text.cursor
        }
        Motion::TillCharBack(ch) => {
            let chars = line_chars(text, text.cursor.line);
            let mut found = 0;
            for i in (0..text.cursor.column).rev() {
                if chars[i] == *ch {
                    found += 1;
                    if found == n {
                        return pos(text.cursor.line, (i + 1).min(text.cursor.column));
                    }
                }
            }
            text.cursor
        }
        Motion::MatchBracket => match_bracket(text).unwrap_or(text.cursor),
    }
}

fn named_key_to_motion(named: &keyboard::key::Named) -> Option<Motion> {
    match named {
        keyboard::key::Named::ArrowLeft => Some(Motion::Left),
        keyboard::key::Named::ArrowRight => Some(Motion::Right),
        keyboard::key::Named::ArrowUp => Some(Motion::Up),
        keyboard::key::Named::ArrowDown => Some(Motion::Down),
        keyboard::key::Named::Home => Some(Motion::LineStart),
        keyboard::key::Named::End => Some(Motion::LineEnd),
        _ => None,
    }
}

fn char_to_motion(c: char) -> Option<Motion> {
    match c {
        'h' => Some(Motion::Left),
        'l' => Some(Motion::Right),
        'j' => Some(Motion::Down),
        'k' => Some(Motion::Up),
        'w' => Some(Motion::WordForward),
        'b' => Some(Motion::WordBackward),
        'e' => Some(Motion::WordEnd),
        'W' => Some(Motion::BigWordForward),
        'B' => Some(Motion::BigWordBackward),
        'E' => Some(Motion::BigWordEnd),
        '$' => Some(Motion::LineEnd),
        '^' => Some(Motion::FirstNonBlank),
        'G' => Some(Motion::DocumentEnd),
        '%' => Some(Motion::MatchBracket),
        _ => None,
    }
}

fn is_line_wise(motion: &Motion) -> bool {
    matches!(
        motion,
        Motion::Down | Motion::Up | Motion::DocumentStart | Motion::DocumentEnd
    )
}

fn is_inclusive(motion: &Motion) -> bool {
    matches!(
        motion,
        Motion::WordEnd
            | Motion::BigWordEnd
            | Motion::LineEnd
            | Motion::FindChar(_)
            | Motion::FindCharBack(_)
            | Motion::TillChar(_)
            | Motion::TillCharBack(_)
            | Motion::MatchBracket
    )
}

// ── Word motions ────────────────────────────────────────────────────────────

fn classify(c: char, big: bool) -> CharClass {
    if big {
        if c.is_whitespace() {
            CharClass::Space
        } else {
            CharClass::Word
        }
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else if c.is_whitespace() {
        CharClass::Space
    } else {
        CharClass::Punct
    }
}

fn word_forward(text: &TextSnapshot, mut line: usize, mut col: usize, big: bool) -> (usize, usize) {
    let chars = line_chars(text, line);
    if chars.is_empty() {
        // Empty line — advance to next line
        if line + 1 < text.line_count() {
            return (line + 1, 0);
        }
        return (line, 0);
    }

    let col_clamped = col.min(chars.len() - 1);
    let start_class = classify(chars[col_clamped], big);

    // Skip current class
    if start_class != CharClass::Space {
        while col < chars.len() && classify(chars[col], big) == start_class {
            col += 1;
        }
    }

    // Skip whitespace, crossing line boundaries
    loop {
        let chars = line_chars(text, line);
        while col < chars.len() && classify(chars[col], big) == CharClass::Space {
            col += 1;
        }
        if col < chars.len() {
            return (line, col);
        }
        if line + 1 < text.line_count() {
            line += 1;
            col = 0;
        } else {
            let ll = line_len(text, line);
            return (line, ll.saturating_sub(1));
        }
    }
}

fn word_backward(
    text: &TextSnapshot,
    mut line: usize,
    mut col: usize,
    big: bool,
) -> (usize, usize) {
    // Move left by one to start
    if col > 0 {
        col -= 1;
    } else if line > 0 {
        line -= 1;
        col = line_len(text, line).saturating_sub(1);
    } else {
        return (0, 0);
    }

    // Skip whitespace backward, crossing lines
    loop {
        let chars = line_chars(text, line);
        if !chars.is_empty() {
            while col > 0 && classify(chars[col], big) == CharClass::Space {
                col -= 1;
            }
            if classify(chars[col], big) != CharClass::Space {
                break;
            }
        }
        // Still on whitespace (or empty line) — go to previous line
        if line > 0 {
            line -= 1;
            col = line_len(text, line).saturating_sub(1);
        } else {
            return (0, 0);
        }
    }

    // At end of a word — find its start
    let chars = line_chars(text, line);
    let word_class = classify(chars[col], big);
    while col > 0 && classify(chars[col - 1], big) == word_class {
        col -= 1;
    }

    (line, col)
}

fn word_end(text: &TextSnapshot, mut line: usize, mut col: usize, big: bool) -> (usize, usize) {
    // Move right by one to start
    let ll = line_len(text, line);
    if col + 1 < ll {
        col += 1;
    } else if line + 1 < text.line_count() {
        line += 1;
        col = 0;
    } else {
        return (line, ll.saturating_sub(1));
    }

    // Skip whitespace forward, crossing lines
    loop {
        let chars = line_chars(text, line);
        if !chars.is_empty() {
            while col < chars.len() && classify(chars[col], big) == CharClass::Space {
                col += 1;
            }
            if col < chars.len() {
                break;
            }
        }
        if line + 1 < text.line_count() {
            line += 1;
            col = 0;
        } else {
            return (line, line_len(text, line).saturating_sub(1));
        }
    }

    // At start of a word — find its end
    let chars = line_chars(text, line);
    let word_class = classify(chars[col], big);
    while col + 1 < chars.len() && classify(chars[col + 1], big) == word_class {
        col += 1;
    }

    (line, col)
}

// ── Text objects ────────────────────────────────────────────────────────────

fn text_object(text: &TextSnapshot, obj: char, inner: bool) -> Option<(Position, Position)> {
    match obj {
        'w' => word_object(text, inner, false),
        'W' => word_object(text, inner, true),
        '(' | ')' | 'b' => pair_object(text, '(', ')', inner),
        '{' | '}' | 'B' => pair_object(text, '{', '}', inner),
        '[' | ']' => pair_object(text, '[', ']', inner),
        '<' | '>' => pair_object(text, '<', '>', inner),
        '"' => quote_object(text, '"', inner),
        '\'' => quote_object(text, '\'', inner),
        '`' => quote_object(text, '`', inner),
        _ => None,
    }
}

fn word_object(text: &TextSnapshot, inner: bool, big: bool) -> Option<(Position, Position)> {
    let line = text.cursor.line;
    let chars = line_chars(text, line);
    if chars.is_empty() {
        return None;
    }
    let col = text.cursor.column.min(chars.len() - 1);
    let cur_class = classify(chars[col], big);

    // Find word start
    let mut start = col;
    while start > 0 && classify(chars[start - 1], big) == cur_class {
        start -= 1;
    }

    // Find word end
    let mut end = col;
    while end + 1 < chars.len() && classify(chars[end + 1], big) == cur_class {
        end += 1;
    }

    if !inner {
        // "a word" includes trailing whitespace (or leading if at end)
        if end + 1 < chars.len() && classify(chars[end + 1], big) == CharClass::Space {
            while end + 1 < chars.len() && classify(chars[end + 1], big) == CharClass::Space {
                end += 1;
            }
        } else if start > 0 && classify(chars[start - 1], big) == CharClass::Space {
            while start > 0 && classify(chars[start - 1], big) == CharClass::Space {
                start -= 1;
            }
        }
    }

    Some((pos(line, start), pos(line, end)))
}

fn pair_object(
    text: &TextSnapshot,
    open: char,
    close: char,
    inner: bool,
) -> Option<(Position, Position)> {
    // Scan backward for unmatched opener
    let open_pos = {
        let mut line = text.cursor.line;
        let mut chars = line_chars(text, line);
        let mut col = text.cursor.column.min(chars.len().saturating_sub(1));
        let mut depth = 0i32;
        loop {
            if col < chars.len() {
                let ch = chars[col];
                if ch == close {
                    depth += 1;
                }
                if ch == open {
                    if depth == 0 {
                        break Some(pos(line, col));
                    }
                    depth -= 1;
                }
            }
            if col > 0 {
                col -= 1;
            } else if line > 0 {
                line -= 1;
                chars = line_chars(text, line);
                col = chars.len().saturating_sub(1);
            } else {
                break None;
            }
        }
    }?;

    // Scan forward from opener for matching closer
    let close_pos = {
        let mut line = open_pos.line;
        let mut chars = line_chars(text, line);
        let mut col = open_pos.column;
        let mut depth = 0i32;
        loop {
            if col < chars.len() {
                let ch = chars[col];
                if ch == open {
                    depth += 1;
                }
                if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        break Some(pos(line, col));
                    }
                }
            }
            col += 1;
            if col >= chars.len() {
                line += 1;
                if line >= text.line_count() {
                    break None;
                }
                chars = line_chars(text, line);
                col = 0;
            }
        }
    }?;

    if inner {
        // Between delimiters (exclusive of delimiters)
        let from = advance_pos(text, open_pos)?;
        let to = retreat_pos(text, close_pos)?;
        if pos_le(&from, &to) {
            Some((from, to))
        } else {
            // Empty interior (e.g., `()`) — no-op
            None
        }
    } else {
        Some((open_pos, close_pos))
    }
}

fn quote_object(text: &TextSnapshot, quote: char, inner: bool) -> Option<(Position, Position)> {
    let line = text.cursor.line;
    let chars = line_chars(text, line);
    let col = text.cursor.column;

    // Find all quote positions on the line
    let quotes: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter(|(_, &c)| c == quote)
        .map(|(i, _)| i)
        .collect();

    // Find the pair containing the cursor
    for pair in quotes.chunks(2) {
        if pair.len() == 2 && pair[0] <= col && col <= pair[1] {
            let (start, end) = (pair[0], pair[1]);
            return if inner {
                if start < end.saturating_sub(1) {
                    Some((pos(line, start + 1), pos(line, end - 1)))
                } else {
                    Some((pos(line, start + 1), pos(line, start + 1)))
                }
            } else {
                Some((pos(line, start), pos(line, end)))
            };
        }
    }
    None
}

// ── Bracket matching ────────────────────────────────────────────────────────

fn match_bracket(text: &TextSnapshot) -> Option<Position> {
    let line = text.cursor.line;
    let chars = line_chars(text, line);

    // Find bracket at or after cursor on current line
    let mut col = text.cursor.column;
    let bracket = loop {
        if col >= chars.len() {
            return None;
        }
        if is_bracket(chars[col]) {
            break chars[col];
        }
        col += 1;
    };

    let (inc, dec, forward) = match bracket {
        '(' => ('(', ')', true),
        ')' => (')', '(', false),
        '[' => ('[', ']', true),
        ']' => (']', '[', false),
        '{' => ('{', '}', true),
        '}' => ('}', '{', false),
        _ => return None,
    };

    find_match(text, line, col, inc, dec, forward)
}

fn find_match(
    text: &TextSnapshot,
    start_line: usize,
    start_col: usize,
    inc: char,
    dec: char,
    forward: bool,
) -> Option<Position> {
    let mut depth = 0i32;
    let mut line = start_line;
    let mut chars = line_chars(text, line);
    let mut col = start_col;

    loop {
        if col < chars.len() {
            let ch = chars[col];
            if ch == inc {
                depth += 1;
            }
            if ch == dec {
                depth -= 1;
            }
            if depth == 0 {
                return Some(pos(line, col));
            }
        }

        if forward {
            col += 1;
            if col >= chars.len() {
                line += 1;
                if line >= text.line_count() {
                    return None;
                }
                chars = line_chars(text, line);
                col = 0;
            }
        } else if col > 0 {
            col -= 1;
        } else if line > 0 {
            line -= 1;
            chars = line_chars(text, line);
            if chars.is_empty() {
                continue;
            }
            col = chars.len() - 1;
        } else {
            return None;
        }
    }
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}')
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn line_len(text: &TextSnapshot, line: usize) -> usize {
    text.lines.get(line).map_or(0, |l| l.chars().count())
}

fn line_chars(text: &TextSnapshot, line: usize) -> Vec<char> {
    text.lines
        .get(line)
        .map_or(Vec::new(), |l| l.chars().collect())
}

fn first_non_blank(text: &TextSnapshot, line: usize) -> usize {
    let chars = line_chars(text, line);
    chars.iter().position(|c| !c.is_whitespace()).unwrap_or(0)
}

fn pos_le(a: &Position, b: &Position) -> bool {
    a.line < b.line || (a.line == b.line && a.column <= b.column)
}

fn pos_lt(a: &Position, b: &Position) -> bool {
    a.line < b.line || (a.line == b.line && a.column < b.column)
}

fn ordered(a: Position, b: Position) -> (Position, Position) {
    if pos_le(&a, &b) {
        (a, b)
    } else {
        (b, a)
    }
}

fn ordered_lines(a: usize, b: usize) -> (usize, usize) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Advance position by one character (possibly to next line).
fn advance_pos(text: &TextSnapshot, p: Position) -> Option<Position> {
    let ll = line_len(text, p.line);
    if p.column + 1 < ll {
        Some(pos(p.line, p.column + 1))
    } else if p.line + 1 < text.line_count() {
        Some(pos(p.line + 1, 0))
    } else {
        None
    }
}

/// Retreat position by one character (possibly to previous line).
fn retreat_pos(text: &TextSnapshot, p: Position) -> Option<Position> {
    if p.column > 0 {
        Some(pos(p.line, p.column - 1))
    } else if p.line > 0 {
        let prev = p.line - 1;
        Some(pos(prev, line_len(text, prev).saturating_sub(1)))
    } else {
        None
    }
}
