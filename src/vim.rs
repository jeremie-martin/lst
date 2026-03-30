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
    last_find: Option<Motion>, // for ; and ,
    preferred_column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VimCommand {
    MoveTo(Position),
    Select {
        anchor: Position,
        head: Position,
    },
    DeleteRange {
        from: Position,
        to: Position,
    },
    DeleteLines {
        first: usize,
        last: usize,
    },
    ChangeRange {
        from: Position,
        to: Position,
    },
    ChangeLines {
        first: usize,
        last: usize,
    },
    YankRange {
        from: Position,
        to: Position,
    },
    YankLines {
        first: usize,
        last: usize,
    },
    EnterInsert,
    PasteAfter,
    PasteBefore,
    OpenLineBelow,
    OpenLineAbove,
    JoinLines {
        count: usize,
    },
    ReplaceChar {
        ch: char,
        count: usize,
    },
    Undo,
    Redo,
    OpenFind,
    FindNext,
    FindPrev,
    TransformCaseRange {
        from: Position,
        to: Position,
        uppercase: bool,
    },
    TransformCaseLines {
        first: usize,
        last: usize,
        uppercase: bool,
    },
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

#[derive(Clone)]
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
    Percent,
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
            last_find: None,
            preferred_column: None,
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

    pub fn clear_preferred_column(&mut self) {
        self.preferred_column = None;
    }

    pub fn on_tab_switch(&mut self) {
        self.clear_pending();
        self.clear_preferred_column();
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
        }
    }

    fn exit_visual(&mut self) {
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.clear_pending();
        self.clear_preferred_column();
    }

    fn repeat_find(&self, c: char) -> Option<Motion> {
        if c != ';' && c != ',' {
            return None;
        }
        let last = self.last_find.as_ref()?;
        Some(if c == ';' {
            last.clone()
        } else {
            reverse_find(last)
        })
    }

    fn resolve_find_partial(&mut self, partial: char, c: char) -> Option<Motion> {
        let motion = match partial {
            'f' => Motion::FindChar(c),
            't' => Motion::TillChar(c),
            'F' => Motion::FindCharBack(c),
            'T' => Motion::TillCharBack(c),
            'g' if c == 'g' => return Some(Motion::DocumentStart),
            _ => return None,
        };
        self.last_find = Some(motion.clone());
        Some(motion)
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
                self.clear_preferred_column();
                // vim: cursor moves left by 1 when leaving Insert (unless at col 0)
                let ll = line_len(text, cursor.line);
                let col = if cursor.column > 0 {
                    (cursor.column - 1).min(ll.saturating_sub(1))
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
                self.exit_visual();
                vec![VimCommand::MoveTo(cursor)]
            }
            Mode::Normal => {
                self.clear_pending();
                self.clear_preferred_column();
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
                    self.clear_preferred_column();
                    return vec![VimCommand::Redo];
                }
            }
            return vec![VimCommand::Noop];
        }

        if let keyboard::Key::Named(named) = key {
            if let Some(m) = named_key_to_motion(named) {
                return self.apply_motion(m, text);
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
            return self.apply_motion(Motion::LineStart, text);
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
                return self.apply_motion(motion, text);
            }

            // Two-char sequence starters
            if matches!(c, 'g' | 'f' | 't' | 'F' | 'T') {
                self.pending.partial = Some(c);
                return vec![VimCommand::Noop];
            }

            if let Some(motion) = self.repeat_find(c) {
                return self.apply_motion(motion, text);
            }

            // Unknown — cancel
            self.clear_pending();
            return vec![VimCommand::Noop];
        }

        if let Some(motion) = char_to_motion(c) {
            return self.apply_motion(motion, text);
        }

        if let Some(motion) = self.repeat_find(c) {
            return self.apply_motion(motion, text);
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
        self.clear_preferred_column();

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
            'o' => vec![VimCommand::OpenLineBelow, VimCommand::EnterInsert],
            'O' => vec![VimCommand::OpenLineAbove, VimCommand::EnterInsert],
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
            'J' => {
                // vim: J = join 2 lines (1 op), 3J = join 3 lines (2 ops)
                let joins = if count <= 1 { 1 } else { count - 1 };
                vec![VimCommand::JoinLines { count: joins }]
            }
            'S' => {
                let last = (text.cursor.line + count - 1).min(text.line_count().saturating_sub(1));
                self.line_operator(Operator::Change, text.cursor.line, last)
            }
            'p' => vec![VimCommand::PasteAfter],
            'P' => vec![VimCommand::PasteBefore],
            'u' => vec![VimCommand::Undo],
            'v' => {
                self.mode = Mode::Visual;
                self.visual_anchor = Some(text.cursor);
                vec![VimCommand::Select {
                    anchor: text.cursor,
                    head: text.cursor,
                }]
            }
            'V' => {
                self.mode = Mode::VisualLine;
                self.visual_anchor = Some(text.cursor);
                self.visual_select(text.cursor, text)
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
                    self.exit_visual();
                    return vec![VimCommand::Redo];
                }
            }
            return vec![VimCommand::Noop];
        }

        if let keyboard::Key::Named(named) = key {
            if let Some(m) = named_key_to_motion(named) {
                return self.apply_motion(m, text);
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
            if let Some(motion) = self.resolve_find_partial(partial, c) {
                return self.apply_motion(motion, text);
            }
            // Text objects in Visual mode (viw, vi", vab, etc.)
            if partial == 'i' || partial == 'a' {
                let count = self.pending.count.take();
                if let Some((from, to)) = text_object(text, c, partial == 'i', count) {
                    self.visual_anchor = Some(from);
                    self.clear_preferred_column();
                    return vec![VimCommand::Select {
                        anchor: from,
                        head: to,
                    }];
                }
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
                self.exit_visual();
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![VimCommand::DeleteLines { first, last }];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![VimCommand::DeleteRange { from, to }];
            }
            'c' | 's' => {
                self.exit_visual();
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![VimCommand::ChangeLines { first, last }, VimCommand::EnterInsert];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![VimCommand::ChangeRange { from, to }, VimCommand::EnterInsert];
            }
            'y' => {
                self.exit_visual();
                let (from, to) = ordered(anchor, text.cursor);
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![
                        VimCommand::YankLines { first, last },
                        VimCommand::MoveTo(pos(first, 0)),
                    ];
                }
                return vec![VimCommand::YankRange { from, to }, VimCommand::MoveTo(from)];
            }
            'v' => {
                if self.mode == Mode::Visual {
                    self.exit_visual();
                    return vec![VimCommand::MoveTo(text.cursor)];
                } else {
                    self.mode = Mode::Visual;
                    self.clear_preferred_column();
                    return vec![VimCommand::Select {
                        anchor,
                        head: text.cursor,
                    }];
                }
            }
            'V' => {
                if self.mode == Mode::VisualLine {
                    self.exit_visual();
                    return vec![VimCommand::MoveTo(text.cursor)];
                } else {
                    self.mode = Mode::VisualLine;
                    self.clear_preferred_column();
                    return self.visual_select(text.cursor, text);
                }
            }
            'u' | 'U' => {
                self.exit_visual();
                let uppercase = c == 'U';
                if is_line {
                    let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                    return vec![VimCommand::TransformCaseLines {
                        first,
                        last,
                        uppercase,
                    }];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![VimCommand::TransformCaseRange {
                    from,
                    to,
                    uppercase,
                }];
            }
            _ => {}
        }

        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T' | 'i' | 'a') {
            self.pending.partial = Some(c);
            return vec![VimCommand::Noop];
        }

        // Try as motion — extend selection
        if c == '0' && self.pending.count.is_none() {
            return self.apply_motion(Motion::LineStart, text);
        }
        if let Some(motion) = char_to_motion(c) {
            return self.apply_motion(motion, text);
        }
        if let Some(motion) = self.repeat_find(c) {
            return self.apply_motion(motion, text);
        }

        // Search
        match c {
            '/' => {
                self.clear_preferred_column();
                return vec![VimCommand::OpenFind];
            }
            'n' => {
                self.clear_preferred_column();
                return vec![VimCommand::FindNext];
            }
            'N' => {
                self.clear_preferred_column();
                return vec![VimCommand::FindPrev];
            }
            _ => {}
        }

        vec![VimCommand::Noop]
    }

    pub fn selection_command(&self, head: Position, text: &TextSnapshot) -> VimCommand {
        let anchor = self.visual_anchor.unwrap_or(text.cursor);
        if self.mode == Mode::VisualLine {
            let (first, last) = ordered_lines(anchor.line, head.line);
            let last_col = line_len(text, last).saturating_sub(1);
            VimCommand::Select {
                anchor: pos(first, 0),
                head: pos(last, last_col),
            }
        } else {
            VimCommand::Select { anchor, head }
        }
    }

    fn visual_select(&self, head: Position, text: &TextSnapshot) -> Vec<VimCommand> {
        vec![self.selection_command(head, text)]
    }

    // ── Partial resolution ──────────────────────────────────────────────

    fn resolve_partial(&mut self, partial: char, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        if let Some(motion) = self.resolve_find_partial(partial, c) {
            return self.apply_motion(motion, text);
        }
        match partial {
            'r' => {
                let count = self.motion_count().unwrap_or(1);
                self.clear_pending();
                self.clear_preferred_column();
                if c == '\n' || line_len(text, text.cursor.line) == 0 {
                    vec![VimCommand::Noop]
                } else {
                    vec![VimCommand::ReplaceChar { ch: c, count }]
                }
            }
            'i' | 'a' => {
                let inner = partial == 'i';
                let count = self.motion_count();
                if let Some(op) = self.pending.operator.take() {
                    if let Some(range) = text_object(text, c, inner, count) {
                        self.clear_pending();
                        self.clear_preferred_column();
                        return self.range_operator(op, range.0, range.1);
                    }
                }
                self.clear_pending();
                self.clear_preferred_column();
                vec![VimCommand::Noop]
            }
            _ => {
                self.clear_pending();
                self.clear_preferred_column();
                vec![VimCommand::Noop]
            }
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

    fn cursor_motion_target(
        &mut self,
        motion: &Motion,
        text: &TextSnapshot,
        count: Option<usize>,
    ) -> Position {
        let preferred_column = if matches!(motion, Motion::Down | Motion::Up) {
            let preferred = self.preferred_column.unwrap_or(text.cursor.column);
            self.preferred_column = Some(preferred);
            Some(preferred)
        } else {
            self.clear_preferred_column();
            None
        };
        compute_motion(motion, text, count, preferred_column)
    }

    fn apply_motion(&mut self, motion: Motion, text: &TextSnapshot) -> Vec<VimCommand> {
        if let Some(op) = self.pending.operator.take() {
            self.operator_with_computed_motion(op, motion, text)
        } else {
            let count = self.motion_count();
            self.clear_pending();
            let target = self.cursor_motion_target(&motion, text, count);
            if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                self.visual_select(target, text)
            } else {
                vec![VimCommand::MoveTo(target)]
            }
        }
    }

    fn operator_with_computed_motion(
        &mut self,
        op: Operator,
        motion: Motion,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        // vim special case: cw/cW behave like ce/cE (don't eat trailing whitespace)
        let motion = if op == Operator::Change {
            match motion {
                Motion::WordForward => Motion::WordEnd,
                Motion::BigWordForward => Motion::BigWordEnd,
                _ => motion,
            }
        } else {
            motion
        };

        let count = self.motion_count();
        self.clear_pending();
        self.clear_preferred_column();

        if motion_is_linewise(&motion, count) {
            let target = compute_motion(&motion, text, count, None);
            let (first, last) = ordered_lines(text.cursor.line, target.line);
            return self.line_operator(op, first, last);
        }

        let target = compute_motion(&motion, text, count, None);
        if target == text.cursor {
            return vec![VimCommand::Noop];
        }

        // vim: dw at end of line stops at EOL (doesn't eat newline), becomes inclusive
        let mut eol_clamped = false;
        let target = if op == Operator::Delete
            && target.line > text.cursor.line
            && matches!(motion, Motion::WordForward | Motion::BigWordForward)
        {
            eol_clamped = true;
            let ll = line_len(text, text.cursor.line);
            pos(text.cursor.line, ll.saturating_sub(1))
        } else {
            target
        };

        let (from, to) = if eol_clamped && target == text.cursor {
            // dw on last char of line: delete just that character
            (text.cursor, text.cursor)
        } else {
            ordered(text.cursor, target)
        };
        let mut to = to;

        // Exclusive motions: don't include the target character (skip if clamped to EOL)
        if !motion_is_inclusive(&motion, count) && !eol_clamped {
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
            Operator::Change => vec![
                VimCommand::ChangeLines { first, last },
                VimCommand::EnterInsert,
            ],
            Operator::Yank => vec![VimCommand::YankLines { first, last }],
        }
    }

    fn range_operator(&self, op: Operator, from: Position, to: Position) -> Vec<VimCommand> {
        match op {
            Operator::Delete => vec![VimCommand::DeleteRange { from, to }],
            Operator::Change => vec![
                VimCommand::ChangeRange { from, to },
                VimCommand::EnterInsert,
            ],
            Operator::Yank => vec![VimCommand::YankRange { from, to }, VimCommand::MoveTo(from)],
        }
    }
}

// ── Motion computation ──────────────────────────────────────────────────────

fn compute_motion(
    motion: &Motion,
    text: &TextSnapshot,
    count: Option<usize>,
    preferred_column: Option<usize>,
) -> Position {
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
            let col = preferred_column
                .unwrap_or(text.cursor.column)
                .min(line_len(text, line).saturating_sub(1));
            pos(line, col)
        }
        Motion::Up => {
            let line = text.cursor.line.saturating_sub(n);
            let col = preferred_column
                .unwrap_or(text.cursor.column)
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
            let line =
                (text.cursor.line + n.saturating_sub(1)).min(text.line_count().saturating_sub(1));
            let ll = line_len(text, line);
            pos(line, ll.saturating_sub(1))
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
            for i in (0..text.cursor.column.min(chars.len())).rev() {
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
            for i in (0..text.cursor.column.min(chars.len())).rev() {
                if chars[i] == *ch {
                    found += 1;
                    if found == n {
                        return pos(text.cursor.line, (i + 1).min(text.cursor.column));
                    }
                }
            }
            text.cursor
        }
        Motion::Percent => match count {
            Some(n) => {
                let total = text.line_count().max(1);
                let pct = n.clamp(1, 100);
                let line = ((pct * total).saturating_add(99) / 100).saturating_sub(1);
                pos(line, first_non_blank(text, line))
            }
            None => match_bracket(text).unwrap_or(text.cursor),
        },
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
        '%' => Some(Motion::Percent),
        _ => None,
    }
}

fn motion_is_linewise(motion: &Motion, count: Option<usize>) -> bool {
    matches!(
        motion,
        Motion::Down | Motion::Up | Motion::DocumentStart | Motion::DocumentEnd
    ) || matches!(motion, Motion::LineEnd) && count.unwrap_or(1) > 1
        || matches!(motion, Motion::Percent) && count.is_some()
}

fn motion_is_inclusive(motion: &Motion, count: Option<usize>) -> bool {
    matches!(
        motion,
        Motion::WordEnd
            | Motion::BigWordEnd
            | Motion::LineEnd
            | Motion::FindChar(_)
            | Motion::FindCharBack(_)
            | Motion::TillChar(_)
            | Motion::TillCharBack(_)
    ) || matches!(motion, Motion::Percent) && count.is_none()
}

fn reverse_find(motion: &Motion) -> Motion {
    match motion {
        Motion::FindChar(c) => Motion::FindCharBack(*c),
        Motion::FindCharBack(c) => Motion::FindChar(*c),
        Motion::TillChar(c) => Motion::TillCharBack(*c),
        Motion::TillCharBack(c) => Motion::TillChar(*c),
        other => other.clone(),
    }
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

    // Skip whitespace, crossing line boundaries (empty lines are word boundaries)
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
            if line_len(text, line) == 0 {
                return (line, 0);
            }
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
        if line_len(text, line) == 0 {
            return (line, 0); // empty line is a word boundary
        }
        col = line_len(text, line).saturating_sub(1);
    } else {
        return (0, 0);
    }

    // Skip whitespace backward, crossing lines (empty lines are word boundaries)
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
        if line > 0 {
            line -= 1;
            if line_len(text, line) == 0 {
                return (line, 0);
            }
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

fn text_object(
    text: &TextSnapshot,
    obj: char,
    inner: bool,
    count: Option<usize>,
) -> Option<(Position, Position)> {
    match obj {
        'w' => word_object(text, inner, false, count.unwrap_or(1)),
        'W' => word_object(text, inner, true, count.unwrap_or(1)),
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

fn word_object(
    text: &TextSnapshot,
    inner: bool,
    big: bool,
    count: usize,
) -> Option<(Position, Position)> {
    let mut range = word_object_at(text, text.cursor, inner, big)?;
    for _ in 1..count.max(1) {
        let Some(next_cursor) = advance_pos(text, range.1) else {
            break;
        };
        let Some(next_range) = word_object_at(text, next_cursor, inner, big) else {
            break;
        };
        range.1 = next_range.1;
    }
    Some(range)
}

fn word_object_at(
    text: &TextSnapshot,
    cursor: Position,
    inner: bool,
    big: bool,
) -> Option<(Position, Position)> {
    let line = cursor.line;
    let chars = line_chars(text, line);
    if chars.is_empty() {
        return None;
    }
    let col = cursor.column.min(chars.len() - 1);
    let cur_class = classify(chars[col], big);

    let mut start = col;
    while start > 0 && classify(chars[start - 1], big) == cur_class {
        start -= 1;
    }

    let mut end = col;
    while end + 1 < chars.len() && classify(chars[end + 1], big) == cur_class {
        end += 1;
    }

    if inner {
        return Some((pos(line, start), pos(line, end)));
    }

    if cur_class == CharClass::Space {
        if let Some((_, next_end)) = next_non_space_range(&chars, end + 1, big) {
            return Some((pos(line, start), pos(line, next_end)));
        }
        if let Some((prev_start, _)) = prev_non_space_range(&chars, start, big) {
            return Some((pos(line, prev_start), pos(line, end)));
        }
        return Some((pos(line, start), pos(line, end)));
    }

    if end + 1 < chars.len() && classify(chars[end + 1], big) == CharClass::Space {
        while end + 1 < chars.len() && classify(chars[end + 1], big) == CharClass::Space {
            end += 1;
        }
    } else if start > 0 && classify(chars[start - 1], big) == CharClass::Space {
        while start > 0 && classify(chars[start - 1], big) == CharClass::Space {
            start -= 1;
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
    // If cursor is on the close delimiter, skip it (we're looking for its match)
    let open_pos = {
        let mut line = text.cursor.line;
        let mut chars = line_chars(text, line);
        let mut col = text.cursor.column.min(chars.len().saturating_sub(1));
        let on_close = col < chars.len() && chars[col] == close;
        let mut depth = if on_close { -1 } else { 0i32 };
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

    let quotes: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter(|(i, &c)| c == quote && !is_escaped_quote(&chars, *i))
        .map(|(i, _)| i)
        .collect();

    let (start, end) = quotes
        .windows(2)
        .filter_map(|pair| {
            let start = pair[0];
            let end = pair[1];
            if start <= col
                && col <= end
                && quote_can_open(&chars, start)
                && quote_can_close(&chars, end)
            {
                Some((start, end))
            } else {
                None
            }
        })
        .min_by_key(|(start, end)| end - start)?;

    if inner {
        if start + 1 < end {
            Some((pos(line, start + 1), pos(line, end - 1)))
        } else {
            None
        }
    } else {
        Some((pos(line, start), pos(line, end)))
    }
}

fn next_non_space_range(chars: &[char], mut start: usize, big: bool) -> Option<(usize, usize)> {
    while start < chars.len() && classify(chars[start], big) == CharClass::Space {
        start += 1;
    }
    if start >= chars.len() {
        return None;
    }
    let class = classify(chars[start], big);
    let mut end = start;
    while end + 1 < chars.len() && classify(chars[end + 1], big) == class {
        end += 1;
    }
    Some((start, end))
}

fn prev_non_space_range(chars: &[char], start: usize, big: bool) -> Option<(usize, usize)> {
    if start == 0 {
        return None;
    }
    let mut end = start - 1;
    loop {
        if classify(chars[end], big) != CharClass::Space {
            break;
        }
        if end == 0 {
            return None;
        }
        end -= 1;
    }

    let class = classify(chars[end], big);
    let mut range_start = end;
    while range_start > 0 && classify(chars[range_start - 1], big) == class {
        range_start -= 1;
    }
    Some((range_start, end))
}

fn is_escaped_quote(chars: &[char], idx: usize) -> bool {
    let mut backslashes = 0;
    let mut i = idx;
    while i > 0 {
        i -= 1;
        if chars[i] == '\\' {
            backslashes += 1;
        } else {
            break;
        }
    }
    backslashes % 2 == 1
}

fn quote_can_open(chars: &[char], idx: usize) -> bool {
    idx == 0 || !quote_neighbor_is_wordish(chars[idx - 1])
}

fn quote_can_close(chars: &[char], idx: usize) -> bool {
    idx + 1 >= chars.len() || !quote_neighbor_is_wordish(chars[idx + 1])
}

fn quote_neighbor_is_wordish(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(lines: &[&str], line: usize, column: usize) -> TextSnapshot {
        TextSnapshot {
            lines: lines.iter().map(|line| (*line).to_string()).collect(),
            cursor: pos(line, column),
        }
    }

    fn key(c: char) -> keyboard::Key {
        keyboard::Key::Character(c.to_string().into())
    }

    #[test]
    fn counted_line_end_motion_moves_across_lines() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;
        let text = snapshot(&["abc", "xyz", "123"], 0, 0);

        assert_eq!(
            vim.handle_key(&key('2'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('$'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::MoveTo(pos(1, 2))]
        );
    }

    #[test]
    fn counted_line_end_operator_becomes_linewise() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;
        let text = snapshot(&["abc", "xyz", "123"], 0, 0);

        assert_eq!(
            vim.handle_key(&key('d'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('2'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('$'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::DeleteLines { first: 0, last: 1 }]
        );
    }

    #[test]
    fn counted_percent_motion_jumps_by_file_percentage() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;
        let lines = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
        let text = snapshot(&lines, 0, 0);

        assert_eq!(
            vim.handle_key(&key('5'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('0'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('%'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::MoveTo(pos(4, 0))]
        );
    }

    #[test]
    fn counted_percent_operator_is_linewise() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;
        let lines = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];
        let text = snapshot(&lines, 0, 0);

        assert_eq!(
            vim.handle_key(&key('d'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('5'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('0'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('%'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::DeleteLines { first: 0, last: 4 }]
        );
    }

    #[test]
    fn visual_find_partial_uses_counts() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;
        let text = snapshot(&["foo bar baz boom"], 0, 0);

        assert_eq!(
            vim.handle_key(&key('v'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 0),
            }]
        );
        assert_eq!(
            vim.handle_key(&key('3'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('f'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        assert_eq!(
            vim.handle_key(&key('b'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 12),
            }]
        );
    }

    #[test]
    fn vertical_motions_preserve_preferred_column() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;

        let first = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 0, 7);
        assert_eq!(
            vim.handle_key(
                &keyboard::Key::Named(keyboard::key::Named::ArrowDown),
                keyboard::Modifiers::default(),
                &first,
            ),
            vec![VimCommand::MoveTo(pos(1, 0))]
        );

        let second = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 1, 0);
        assert_eq!(
            vim.handle_key(
                &keyboard::Key::Named(keyboard::key::Named::ArrowDown),
                keyboard::Modifiers::default(),
                &second,
            ),
            vec![VimCommand::MoveTo(pos(2, 7))]
        );
    }

    #[test]
    fn tab_switch_clears_pending_operator() {
        let mut vim = VimState::new();
        vim.mode = Mode::Normal;
        let text = snapshot(&["foo bar"], 0, 0);

        assert_eq!(
            vim.handle_key(&key('d'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::Noop]
        );
        vim.on_tab_switch();
        assert_eq!(
            vim.handle_key(&key('w'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::MoveTo(pos(0, 4))]
        );
    }

    #[test]
    fn around_word_on_whitespace_selects_following_word() {
        let text = snapshot(&["foo   bar"], 0, 4);
        assert_eq!(
            text_object(&text, 'w', false, None),
            Some((pos(0, 3), pos(0, 8)))
        );
    }

    #[test]
    fn counted_inner_word_objects_expand_across_adjacent_objects() {
        let text = snapshot(&["foo bar baz"], 0, 0);
        assert_eq!(
            text_object(&text, 'w', true, Some(2)),
            Some((pos(0, 0), pos(0, 3)))
        );
    }

    #[test]
    fn counted_around_word_objects_expand_from_whitespace() {
        let text = snapshot(&["foo   bar baz"], 0, 4);
        assert_eq!(
            text_object(&text, 'w', false, Some(2)),
            Some((pos(0, 3), pos(0, 12)))
        );
    }

    #[test]
    fn quote_object_ignores_escaped_quotes() {
        let text = snapshot(&["let s = \"a\\\"b\""], 0, 12);
        assert_eq!(
            text_object(&text, '"', true, None),
            Some((pos(0, 9), pos(0, 12)))
        );
    }

    #[test]
    fn quote_object_uses_nearest_surrounding_pair() {
        let text = snapshot(&["\"foo \"bar\""], 0, 7);
        assert_eq!(
            text_object(&text, '"', true, None),
            Some((pos(0, 6), pos(0, 8)))
        );
    }

    #[test]
    fn quote_object_rejects_gap_between_adjacent_strings() {
        let text = snapshot(&["\"one\" \"two\""], 0, 5);
        assert_eq!(text_object(&text, '"', true, None), None);
    }

    #[test]
    fn visual_u_maps_to_case_transform() {
        let mut vim = VimState::new();
        vim.mode = Mode::Visual;
        vim.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["ABC DEF"], 0, 2);

        assert_eq!(
            vim.handle_key(&key('u'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::TransformCaseRange {
                from: pos(0, 0),
                to: pos(0, 2),
                uppercase: false,
            }]
        );
    }

    #[test]
    fn visual_search_repeat_keeps_visual_mode() {
        let mut vim = VimState::new();
        vim.mode = Mode::Visual;
        vim.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["foo bar foo"], 0, 2);

        assert_eq!(
            vim.handle_key(&key('n'), keyboard::Modifiers::default(), &text),
            vec![VimCommand::FindNext]
        );
        assert_eq!(vim.mode, Mode::Visual);
        assert_eq!(vim.visual_anchor, Some(pos(0, 0)));
    }
}
