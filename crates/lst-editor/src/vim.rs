//! Vim mode state machine.
//!
//! Pure keystroke to command translation. The caller executes commands against
//! whatever editor surface owns the document state.

use crate::effect::RevealIntent;
use crate::position::Position;
use crate::selection::{last_grapheme_column, next_grapheme_column, previous_grapheme_column};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Character(String),
    Named(NamedKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
    Tab,
    Enter,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub command: bool,
    pub control: bool,
}

impl Modifiers {
    pub const COMMAND: Self = Self {
        command: true,
        control: false,
    };
    pub const CONTROL: Self = Self {
        command: false,
        control: true,
    };

    pub fn command(self) -> bool {
        self.command
    }

    pub fn control(self) -> bool {
        self.control
    }
}

fn pos(line: usize, column: usize) -> Position {
    Position { line, column }
}

// -- Public types ------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
        }
    }
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
    SearchWordUnderCursor {
        word: String,
        forward: bool,
    },
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
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    MoveToScreenTop,
    MoveToScreenMiddle,
    MoveToScreenBottom,
    ScrollCursor(RevealIntent),
    Noop,
}

pub struct TextSnapshot {
    pub lines: Arc<[String]>,
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

// -- Private types -----------------------------------------------------------

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

// -- VimState ----------------------------------------------------------------

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}

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
        key: &Key,
        mods: Modifiers,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        match self.mode {
            Mode::Normal => self.handle_normal(key, mods, text),
            Mode::Insert => vec![], // caller lets the editor surface handle text input
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

    // -- Normal mode -----------------------------------------------------

    fn handle_normal(
        &mut self,
        key: &Key,
        mods: Modifiers,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        if let Some(cmd) = ctrl_page_command(key, mods) {
            self.clear_pending();
            self.clear_preferred_column();
            return vec![cmd];
        }

        if mods.command() {
            if let Key::Character(c) = key {
                if c.as_str() == "r" {
                    self.clear_pending();
                    self.clear_preferred_column();
                    return vec![VimCommand::Redo];
                }
            }
            return vec![VimCommand::Noop];
        }

        if let Key::Named(named) = key {
            if let Some(m) = named_key_to_motion(named) {
                return self.apply_motion(m, text);
            }
            return vec![VimCommand::Noop];
        }

        let c = match key {
            Key::Character(s) => match s.as_str().chars().next() {
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

            // Unknown - cancel
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
        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T' | 'r' | 'z') {
            self.pending.partial = Some(c);
            return vec![VimCommand::Noop];
        }

        let count = self.motion_count().unwrap_or(1);
        self.clear_pending();
        self.clear_preferred_column();

        match c {
            'H' => vec![VimCommand::MoveToScreenTop],
            'M' => vec![VimCommand::MoveToScreenMiddle],
            'L' => vec![VimCommand::MoveToScreenBottom],
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
                vec![
                    VimCommand::ChangeRange {
                        from: text.cursor,
                        to: pos(text.cursor.line, end),
                    },
                    VimCommand::EnterInsert,
                ]
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
                vec![
                    VimCommand::ChangeRange {
                        from: text.cursor,
                        to: pos(text.cursor.line, ll - 1),
                    },
                    VimCommand::EnterInsert,
                ]
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
            '*' | '#' => {
                if let Some(word) = word_under_cursor(text) {
                    vec![VimCommand::SearchWordUnderCursor {
                        word,
                        forward: c == '*',
                    }]
                } else {
                    vec![VimCommand::Noop]
                }
            }
            _ => vec![VimCommand::Noop],
        }
    }

    // -- Visual mode -----------------------------------------------------

    fn handle_visual(
        &mut self,
        key: &Key,
        mods: Modifiers,
        text: &TextSnapshot,
    ) -> Vec<VimCommand> {
        if let Some(cmd) = ctrl_page_command(key, mods) {
            return vec![cmd];
        }

        if mods.command() {
            if let Key::Character(c) = key {
                if c.as_str() == "r" {
                    self.exit_visual();
                    return vec![VimCommand::Redo];
                }
            }
            return vec![VimCommand::Noop];
        }

        if let Key::Named(named) = key {
            if let Some(m) = named_key_to_motion(named) {
                return self.apply_motion(m, text);
            }
            return vec![VimCommand::Noop];
        }

        let c = match key {
            Key::Character(s) => match s.as_str().chars().next() {
                Some(c) => c,
                None => return vec![VimCommand::Noop],
            },
            _ => return vec![VimCommand::Noop],
        };

        if let Some(partial) = self.pending.partial.take() {
            if let Some(motion) = self.resolve_find_partial(partial, c) {
                return self.apply_motion(motion, text);
            }
            if partial == 'z' {
                self.clear_preferred_column();
                return match resolve_z_intent(c) {
                    Some(intent) => vec![VimCommand::ScrollCursor(intent)],
                    None => vec![VimCommand::Noop],
                };
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
                    return vec![
                        VimCommand::ChangeLines { first, last },
                        VimCommand::EnterInsert,
                    ];
                }
                let (from, to) = ordered(anchor, text.cursor);
                return vec![
                    VimCommand::ChangeRange { from, to },
                    VimCommand::EnterInsert,
                ];
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

        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T' | 'i' | 'a' | 'z') {
            self.pending.partial = Some(c);
            return vec![VimCommand::Noop];
        }

        match c {
            'H' => return vec![VimCommand::MoveToScreenTop],
            'M' => return vec![VimCommand::MoveToScreenMiddle],
            'L' => return vec![VimCommand::MoveToScreenBottom],
            _ => {}
        }

        // Try as motion - extend selection
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

    // -- Partial resolution ----------------------------------------------

    fn resolve_partial(&mut self, partial: char, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        if let Some(motion) = self.resolve_find_partial(partial, c) {
            return self.apply_motion(motion, text);
        }
        if partial == 'z' {
            self.clear_pending();
            self.clear_preferred_column();
            return match resolve_z_intent(c) {
                Some(intent) => vec![VimCommand::ScrollCursor(intent)],
                None => vec![VimCommand::Noop],
            };
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
                        // Paragraph text objects are linewise
                        if c == 'p' {
                            return self.line_operator(op, range.0.line, range.1.line);
                        }
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

    // -- Motion + operator helpers ---------------------------------------

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
        // vim: cw/cW behave like ce/cE when cursor is on a non-whitespace char
        let motion = if op == Operator::Change {
            let cursor_on_non_space = {
                let chars = line_chars(text, text.cursor.line);
                let col = text.cursor.column.min(chars.len().saturating_sub(1));
                !chars.is_empty() && !chars[col].is_whitespace()
            };
            if cursor_on_non_space {
                match motion {
                    Motion::WordForward => Motion::WordEnd,
                    Motion::BigWordForward => Motion::BigWordEnd,
                    _ => motion,
                }
            } else {
                motion
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

        // Motions that return cursor on failure (no match / no bracket) are true no-ops.
        // Forward motions that return cursor due to clamping (l at EOL, e at EOF, etc.)
        // should still operate on the char at cursor.
        if target == text.cursor && motion_noop_on_same_pos(&motion) {
            return vec![VimCommand::Noop];
        }

        let backward = pos_lt(&target, &text.cursor);

        // vim: dw at end of line stops at EOL (doesn't eat newline)
        // vim: w at end of file can't find next word start - treat as inclusive
        let mut eol_clamped = false;
        let target = if matches!(motion, Motion::WordForward | Motion::BigWordForward) {
            if op == Operator::Delete && target.line > text.cursor.line {
                eol_clamped = true;
                let ll = line_len(text, text.cursor.line);
                pos(text.cursor.line, ll.saturating_sub(1))
            } else if target.line == text.line_count().saturating_sub(1)
                && line_len(text, target.line) > 0
                && target.column == line_len(text, target.line).saturating_sub(1)
            {
                // w landed at last char of last line - no next word exists
                eol_clamped = true;
                target
            } else {
                target
            }
        } else {
            target
        };

        let (from, to) = if eol_clamped && target == text.cursor {
            (text.cursor, text.cursor)
        } else {
            ordered(text.cursor, target)
        };
        let mut to = to;

        // Shrink `to` by one character when:
        // - exclusive motions (standard vim rule), OR
        // - backward motions (cursor char is never included for backward ops)
        // Skip when eol_clamped (already adjusted to be inclusive)
        if (!motion_is_inclusive(&motion, count) || backward) && !eol_clamped {
            if to.column > 0 {
                to.column -= 1;
            } else if to.line > from.line {
                to.line -= 1;
                to.column = line_len(text, to.line).saturating_sub(1);
            }
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

// -- Motion computation ------------------------------------------------------

fn compute_motion(
    motion: &Motion,
    text: &TextSnapshot,
    count: Option<usize>,
    preferred_column: Option<usize>,
) -> Position {
    let n = count.unwrap_or(1);
    match motion {
        Motion::Left => {
            let line_text = text
                .lines
                .get(text.cursor.line)
                .map(String::as_str)
                .unwrap_or("");
            let mut col = text.cursor.column;
            for _ in 0..n {
                let next = previous_grapheme_column(line_text, col);
                if next == col {
                    break;
                }
                col = next;
            }
            pos(text.cursor.line, col)
        }
        Motion::Right => {
            let line_text = text
                .lines
                .get(text.cursor.line)
                .map(String::as_str)
                .unwrap_or("");
            let last = last_grapheme_column(line_text);
            let mut col = text.cursor.column.min(last);
            for _ in 0..n {
                let next = next_grapheme_column(line_text, col);
                if next == col || next > last {
                    break;
                }
                col = next;
            }
            pos(text.cursor.line, col)
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

fn named_key_to_motion(named: &NamedKey) -> Option<Motion> {
    match named {
        NamedKey::ArrowLeft => Some(Motion::Left),
        NamedKey::ArrowRight => Some(Motion::Right),
        NamedKey::ArrowUp => Some(Motion::Up),
        NamedKey::ArrowDown => Some(Motion::Down),
        NamedKey::Home => Some(Motion::LineStart),
        NamedKey::End => Some(Motion::LineEnd),
        _ => None,
    }
}

fn ctrl_page_command(key: &Key, mods: Modifiers) -> Option<VimCommand> {
    if !mods.control() {
        return None;
    }
    let Key::Character(c) = key else {
        return None;
    };
    match c.as_str() {
        "d" => Some(VimCommand::HalfPageDown),
        "u" => Some(VimCommand::HalfPageUp),
        "f" => Some(VimCommand::PageDown),
        "b" => Some(VimCommand::PageUp),
        _ => None,
    }
}

fn resolve_z_intent(c: char) -> Option<RevealIntent> {
    match c {
        'z' => Some(RevealIntent::Center),
        't' => Some(RevealIntent::Top),
        'b' => Some(RevealIntent::Bottom),
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

/// Motions where target == cursor means "failed to find" (no-op), NOT "clamped at boundary".
/// Forward motions clamped at boundary (l at EOL, e at EOF, $ at end) should still operate
/// on the cursor character, so they are NOT listed here.
fn motion_noop_on_same_pos(motion: &Motion) -> bool {
    matches!(
        motion,
        Motion::Left
            | Motion::WordBackward
            | Motion::BigWordBackward
            | Motion::LineStart
            | Motion::FirstNonBlank
            | Motion::FindChar(_)
            | Motion::TillChar(_)
            | Motion::FindCharBack(_)
            | Motion::TillCharBack(_)
            | Motion::Percent
    )
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

// -- Word motions ------------------------------------------------------------

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
        // Empty line - advance to next line
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

    // At end of a word - find its start
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

    // At start of a word - find its end
    let chars = line_chars(text, line);
    let word_class = classify(chars[col], big);
    while col + 1 < chars.len() && classify(chars[col + 1], big) == word_class {
        col += 1;
    }

    (line, col)
}

// -- Text objects ------------------------------------------------------------

fn text_object(
    text: &TextSnapshot,
    obj: char,
    inner: bool,
    count: Option<usize>,
) -> Option<(Position, Position)> {
    match obj {
        'w' => word_object(text, inner, false, count.unwrap_or(1)),
        'W' => word_object(text, inner, true, count.unwrap_or(1)),
        'p' => paragraph_object(text, inner),
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

fn paragraph_object(text: &TextSnapshot, inner: bool) -> Option<(Position, Position)> {
    let total = text.line_count();
    if total == 0 {
        return None;
    }
    let cur = text.cursor.line;
    let is_blank = |l: usize| text.lines.get(l).is_none_or(|s| s.trim().is_empty());
    let on_blank = is_blank(cur);
    let same = |l: usize| is_blank(l) == on_blank;

    let mut first = cur;
    while first > 0 && same(first - 1) {
        first -= 1;
    }
    let mut last = cur;
    while last + 1 < total && same(last + 1) {
        last += 1;
    }
    if !inner {
        while last + 1 < total && !same(last + 1) {
            last += 1;
        }
    }
    let last_col = line_len(text, last).saturating_sub(1);
    Some((pos(first, 0), pos(last, last_col)))
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
            // Empty interior (e.g., `()`) - no-op
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

// -- Bracket matching --------------------------------------------------------

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

// -- Helpers -----------------------------------------------------------------

fn line_len(text: &TextSnapshot, line: usize) -> usize {
    text.lines.get(line).map_or(0, |l| l.chars().count())
}

fn line_chars(text: &TextSnapshot, line: usize) -> Vec<char> {
    text.lines
        .get(line)
        .map_or(Vec::new(), |l| l.chars().collect())
}

fn word_under_cursor(text: &TextSnapshot) -> Option<String> {
    let chars = line_chars(text, text.cursor.line);
    if chars.is_empty() {
        return None;
    }
    let col = text.cursor.column.min(chars.len().saturating_sub(1));
    if !chars[col].is_alphanumeric() && chars[col] != '_' {
        return None;
    }
    let mut start = col;
    while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < chars.len() && (chars[end + 1].is_alphanumeric() || chars[end + 1] == '_') {
        end += 1;
    }
    Some(chars[start..=end].iter().collect())
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

#[cfg(all(test, feature = "internal-invariants"))]
mod tests {
    use super::*;

    // -- Helpers -------------------------------------------------------------

    fn snapshot(lines: &[&str], line: usize, column: usize) -> TextSnapshot {
        TextSnapshot {
            lines: lines
                .iter()
                .map(|line| (*line).to_string())
                .collect::<Vec<_>>()
                .into(),
            cursor: pos(line, column),
        }
    }

    fn key(c: char) -> Key {
        Key::Character(c.to_string())
    }

    fn normal() -> VimState {
        let mut v = VimState::new();
        v.mode = Mode::Normal;
        v
    }

    fn mods() -> Modifiers {
        Modifiers::default()
    }

    fn press(vim: &mut VimState, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        vim.handle_key(&key(c), mods(), text)
    }

    /// Press a sequence of character keys, returning the result of the last one.
    fn press_keys(vim: &mut VimState, keys: &str, text: &TextSnapshot) -> Vec<VimCommand> {
        let mut result = vec![VimCommand::Noop];
        for c in keys.chars() {
            result = vim.handle_key(&key(c), mods(), text);
        }
        result
    }

    fn press_named(vim: &mut VimState, n: NamedKey, text: &TextSnapshot) -> Vec<VimCommand> {
        vim.handle_key(&Key::Named(n), mods(), text)
    }

    fn press_ctrl(vim: &mut VimState, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        vim.handle_key(&key(c), Modifiers::COMMAND, text)
    }

    // -- Motions: horizontal (h, l) ------------------------------------------

    #[test]
    fn h_moves_left() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'h', &snapshot(&["hello"], 0, 3)),
            vec![VimCommand::MoveTo(pos(0, 2))]
        );
    }

    #[test]
    fn h_clamps_at_column_zero() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'h', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn l_moves_right() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'l', &snapshot(&["hello"], 0, 2)),
            vec![VimCommand::MoveTo(pos(0, 3))]
        );
    }

    #[test]
    fn l_clamps_at_last_char() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'l', &snapshot(&["hello"], 0, 4)),
            vec![VimCommand::MoveTo(pos(0, 4))]
        );
    }

    #[test]
    fn h_on_empty_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'h', &snapshot(&[""], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn l_on_empty_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'l', &snapshot(&[""], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    // -- Motions: vertical (j, k) --------------------------------------------

    #[test]
    fn j_moves_down() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'j', &snapshot(&["abc", "xyz"], 0, 0)),
            vec![VimCommand::MoveTo(pos(1, 0))]
        );
    }

    #[test]
    fn j_clamps_at_last_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'j', &snapshot(&["abc"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn j_clamps_column_to_shorter_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'j', &snapshot(&["abcdef", "xy"], 0, 4)),
            vec![VimCommand::MoveTo(pos(1, 1))]
        );
    }

    #[test]
    fn k_moves_up() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'k', &snapshot(&["abc", "xyz"], 1, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn k_clamps_at_first_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'k', &snapshot(&["abc"], 0, 2)),
            vec![VimCommand::MoveTo(pos(0, 2))]
        );
    }

    // -- Motions: word (w, b, e, W, B, E) ------------------------------------

    #[test]
    fn w_to_next_word() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["foo bar"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 4))]
        );
    }

    #[test]
    fn w_stops_at_punctuation() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["foo.bar"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 3))]
        );
    }

    #[test]
    fn w_crosses_line_boundary() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["foo", "bar"], 0, 0)),
            vec![VimCommand::MoveTo(pos(1, 0))]
        );
    }

    #[test]
    fn w_stops_at_empty_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["foo", "", "bar"], 0, 0)),
            vec![VimCommand::MoveTo(pos(1, 0))]
        );
    }

    #[test]
    fn w_at_end_of_file() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["foo"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 2))]
        );
    }

    #[test]
    fn b_to_prev_word() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'b', &snapshot(&["foo bar"], 0, 4)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn b_crosses_line_boundary() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'b', &snapshot(&["foo", "bar"], 1, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn b_at_start_of_file() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'b', &snapshot(&["foo"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn e_to_word_end() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'e', &snapshot(&["foo bar"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 2))]
        );
    }

    #[test]
    fn e_skips_to_next_word_end() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'e', &snapshot(&["foo bar"], 0, 2)),
            vec![VimCommand::MoveTo(pos(0, 6))]
        );
    }

    #[test]
    fn e_crosses_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'e', &snapshot(&["foo", "bar"], 0, 2)),
            vec![VimCommand::MoveTo(pos(1, 2))]
        );
    }

    #[test]
    fn big_w_skips_punctuation() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'W', &snapshot(&["foo.bar baz"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 8))]
        );
    }

    #[test]
    fn big_b_skips_punctuation() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'B', &snapshot(&["foo.bar baz"], 0, 8)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn big_e_skips_punctuation() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'E', &snapshot(&["foo.bar baz"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 6))]
        );
    }

    // -- Motions: line position (0, $, ^) ------------------------------------

    #[test]
    fn zero_to_line_start() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '0', &snapshot(&["  hello"], 0, 5)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn dollar_to_line_end() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '$', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 4))]
        );
    }

    #[test]
    fn dollar_on_empty_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '$', &snapshot(&[""], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn caret_to_first_non_blank() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '^', &snapshot(&["  hello"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 2))]
        );
    }

    #[test]
    fn caret_no_indent() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '^', &snapshot(&["hello"], 0, 3)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    // -- Motions: document (gg, G) -------------------------------------------

    #[test]
    fn gg_to_document_start() {
        let mut v = normal();
        let text = snapshot(&["  aaa", "bbb", "ccc", "ddd", "eee"], 3, 2);
        assert_eq!(
            press_keys(&mut v, "gg", &text),
            vec![VimCommand::MoveTo(pos(0, 2))]
        );
    }

    #[test]
    fn gg_with_count_to_line_n() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "ccc", "ddd", "eee"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "3gg", &text),
            vec![VimCommand::MoveTo(pos(2, 0))]
        );
    }

    #[test]
    fn gg_count_clamps_to_last_line() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "ccc"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "99gg", &text),
            vec![VimCommand::MoveTo(pos(2, 0))]
        );
    }

    #[test]
    fn big_g_to_document_end() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "  ccc"], 0, 0);
        assert_eq!(
            press(&mut v, 'G', &text),
            vec![VimCommand::MoveTo(pos(2, 2))]
        );
    }

    #[test]
    fn big_g_with_count() {
        let mut v = normal();
        let text = snapshot(&["aaa", "  bbb", "ccc"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "2G", &text),
            vec![VimCommand::MoveTo(pos(1, 2))]
        );
    }

    // -- Motions: find char (f, t, F, T) -------------------------------------

    #[test]
    fn f_finds_char_forward() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "fw", &snapshot(&["hello world"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 6))]
        );
    }

    #[test]
    fn f_no_match_stays() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "fz", &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn f_with_count() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "2fa", &snapshot(&["banana"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 3))]
        );
    }

    #[test]
    fn t_till_before_char() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "tw", &snapshot(&["hello world"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 5))]
        );
    }

    #[test]
    fn big_f_finds_backward() {
        let mut v = normal();
        // "hello world": o at col 4 and col 7. From col 10, nearest backward 'o' is col 7.
        assert_eq!(
            press_keys(&mut v, "Fo", &snapshot(&["hello world"], 0, 10)),
            vec![VimCommand::MoveTo(pos(0, 7))]
        );
    }

    #[test]
    fn big_t_till_after_backward() {
        let mut v = normal();
        // From col 10, Fo finds 'o' at col 7, To stops one after = col 8.
        assert_eq!(
            press_keys(&mut v, "To", &snapshot(&["hello world"], 0, 10)),
            vec![VimCommand::MoveTo(pos(0, 8))]
        );
    }

    // -- Motions: bracket match (%) ------------------------------------------

    #[test]
    fn percent_matches_forward() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '%', &snapshot(&["(foo)"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 4))]
        );
    }

    #[test]
    fn percent_matches_backward() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '%', &snapshot(&["(foo)"], 0, 4)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn percent_nested() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '%', &snapshot(&["((()))"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 5))]
        );
    }

    #[test]
    fn percent_scans_forward_for_bracket() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '%', &snapshot(&["foo(bar)"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 7))]
        );
    }

    #[test]
    fn percent_no_bracket_stays() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '%', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn percent_cross_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '%', &snapshot(&["(", "foo", ")"], 0, 0)),
            vec![VimCommand::MoveTo(pos(2, 0))]
        );
    }

    #[test]
    fn percent_with_count_is_file_percentage() {
        let mut v = normal();
        let lines: Vec<&str> = (0..10).map(|_| "x").collect();
        assert_eq!(
            press_keys(&mut v, "50%", &snapshot(&lines, 0, 0)),
            vec![VimCommand::MoveTo(pos(4, 0))]
        );
    }

    // -- Motions: arrow keys -------------------------------------------------

    #[test]
    fn arrow_keys_match_hjkl() {
        let text = snapshot(&["abc", "xyz"], 0, 1);
        let mut v1 = normal();
        let mut v2 = normal();
        assert_eq!(
            press_named(&mut v1, NamedKey::ArrowLeft, &text),
            press(&mut v2, 'h', &text),
        );
        let mut v1 = normal();
        let mut v2 = normal();
        assert_eq!(
            press_named(&mut v1, NamedKey::ArrowRight, &text),
            press(&mut v2, 'l', &text),
        );
        let mut v1 = normal();
        let mut v2 = normal();
        assert_eq!(
            press_named(&mut v1, NamedKey::ArrowDown, &text),
            press(&mut v2, 'j', &text),
        );
        let mut v1 = normal();
        let mut v2 = normal();
        let text_k = snapshot(&["abc", "xyz"], 1, 1);
        assert_eq!(
            press_named(&mut v1, NamedKey::ArrowUp, &text_k),
            press(&mut v2, 'k', &text_k),
        );
    }

    #[test]
    fn home_end_match_zero_dollar() {
        let text = snapshot(&["hello"], 0, 2);
        let mut v1 = normal();
        let mut v2 = normal();
        assert_eq!(
            press_named(&mut v1, NamedKey::Home, &text),
            press(&mut v2, '0', &text),
        );
        let mut v1 = normal();
        let mut v2 = normal();
        assert_eq!(
            press_named(&mut v1, NamedKey::End, &text),
            press(&mut v2, '$', &text),
        );
    }

    // -- Motions: counts -----------------------------------------------------

    #[test]
    fn counted_h() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "3h", &snapshot(&["hello"], 0, 4)),
            vec![VimCommand::MoveTo(pos(0, 1))]
        );
    }

    #[test]
    fn counted_j() {
        let mut v = normal();
        let text = snapshot(&["a", "b", "c", "d", "e"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "3j", &text),
            vec![VimCommand::MoveTo(pos(3, 0))]
        );
    }

    #[test]
    fn counted_w() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "2w", &snapshot(&["foo bar baz"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 8))]
        );
    }

    #[test]
    fn multi_digit_count() {
        let mut v = normal();
        let lines: Vec<&str> = (0..20).map(|_| "x").collect();
        assert_eq!(
            press_keys(&mut v, "12j", &snapshot(&lines, 0, 0)),
            vec![VimCommand::MoveTo(pos(12, 0))]
        );
    }

    #[test]
    fn count_clamps() {
        let mut v = normal();
        let text = snapshot(&["a", "b", "c"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "99j", &text),
            vec![VimCommand::MoveTo(pos(2, 0))]
        );
    }

    #[test]
    fn zero_is_motion_not_digit_when_no_count() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '0', &snapshot(&["hello"], 0, 3)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn zero_is_digit_after_nonzero_digit() {
        let mut v = normal();
        let text = snapshot(
            &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"],
            0,
            0,
        );
        assert_eq!(
            press_keys(&mut v, "10j", &text),
            vec![VimCommand::MoveTo(pos(10, 0))]
        );
    }

    // -- Motions: preferred column -------------------------------------------

    #[test]
    fn preferred_column_survives_short_line() {
        let mut v = normal();
        let t1 = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 0, 7);
        assert_eq!(press(&mut v, 'j', &t1), vec![VimCommand::MoveTo(pos(1, 0))]);
        let t2 = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 1, 0);
        assert_eq!(press(&mut v, 'j', &t2), vec![VimCommand::MoveTo(pos(2, 7))]);
    }

    #[test]
    fn preferred_column_cleared_by_horizontal_motion() {
        let mut v = normal();
        let t1 = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 0, 7);
        press(&mut v, 'j', &t1); // sets preferred_column = 7
        let t2 = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 1, 0);
        press(&mut v, 'l', &t2); // should clear preferred_column
        let t3 = snapshot(&["ABCDEFGHIJ", "x", "ABCDEFGHIJ"], 1, 0);
        // After l, preferred_column is cleared, so j uses current column
        assert_eq!(press(&mut v, 'j', &t3), vec![VimCommand::MoveTo(pos(2, 0))]);
    }

    // -- Operators: delete + motion ------------------------------------------

    #[test]
    fn dw_deletes_word() {
        let mut v = normal();
        let text = snapshot(&["foo bar"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dw", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 3)
            }]
        );
    }

    #[test]
    fn de_deletes_to_word_end_inclusive() {
        let mut v = normal();
        let text = snapshot(&["foo bar"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "de", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 2)
            }]
        );
    }

    #[test]
    fn d_dollar_deletes_to_end_inclusive() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        assert_eq!(
            press_keys(&mut v, "d$", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 1),
                to: pos(0, 4)
            }]
        );
    }

    #[test]
    fn d0_deletes_to_start() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 3);
        assert_eq!(
            press_keys(&mut v, "d0", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 2)
            }]
        );
    }

    #[test]
    fn dj_is_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dj", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 1 }]
        );
    }

    #[test]
    fn dk_is_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 2, 0);
        assert_eq!(
            press_keys(&mut v, "dk", &text),
            vec![VimCommand::DeleteLines { first: 1, last: 2 }]
        );
    }

    #[test]
    fn dgg_is_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123", "456"], 3, 0);
        assert_eq!(
            press_keys(&mut v, "dgg", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 3 }]
        );
    }

    #[test]
    fn d_big_g_is_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dG", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 2 }]
        );
    }

    #[test]
    fn df_is_inclusive() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dfw", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 6)
            }]
        );
    }

    #[test]
    fn dt_is_inclusive() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dtw", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 5)
            }]
        );
    }

    #[test]
    fn d_percent_no_count_is_charwise() {
        let mut v = normal();
        let text = snapshot(&["(foo)"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "d%", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 4)
            }]
        );
    }

    #[test]
    fn dw_at_eol_clamps_to_line() {
        let mut v = normal();
        // cursor on "bar", w would cross to next line; vim clamps to EOL
        let text = snapshot(&["foo bar", "baz"], 0, 4);
        assert_eq!(
            press_keys(&mut v, "dw", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 4),
                to: pos(0, 6)
            }]
        );
    }

    // -- Operators: change + motion ------------------------------------------

    #[test]
    fn cw_becomes_ce() {
        let mut v = normal();
        let text = snapshot(&["foo bar"], 0, 0);
        // cw remapped to ce: inclusive to word end
        assert_eq!(
            press_keys(&mut v, "cw", &text),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 0),
                    to: pos(0, 2)
                },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn c_big_w_becomes_c_big_e() {
        let mut v = normal();
        let text = snapshot(&["foo.bar baz"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "cW", &text),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 0),
                    to: pos(0, 6)
                },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn c_dollar_changes_to_end() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        assert_eq!(
            press_keys(&mut v, "c$", &text),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 1),
                    to: pos(0, 4)
                },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn cj_is_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "cj", &text),
            vec![
                VimCommand::ChangeLines { first: 0, last: 1 },
                VimCommand::EnterInsert
            ],
        );
    }

    // -- Operators: yank + motion --------------------------------------------

    #[test]
    fn yw_yanks_and_stays() {
        let mut v = normal();
        let text = snapshot(&["foo bar"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "yw", &text),
            vec![
                VimCommand::YankRange {
                    from: pos(0, 0),
                    to: pos(0, 3)
                },
                VimCommand::MoveTo(pos(0, 0))
            ],
        );
    }

    #[test]
    fn ye_inclusive() {
        let mut v = normal();
        let text = snapshot(&["foo bar"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "ye", &text),
            vec![
                VimCommand::YankRange {
                    from: pos(0, 0),
                    to: pos(0, 2)
                },
                VimCommand::MoveTo(pos(0, 0))
            ],
        );
    }

    #[test]
    fn yj_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "yj", &text),
            vec![VimCommand::YankLines { first: 0, last: 1 }],
        );
    }

    // -- Operators: count multiplication --------------------------------------

    #[test]
    fn operator_count_times_motion_count() {
        // 2d3j = delete lines 0..6 (2*3=6 lines down)
        let mut v = normal();
        let lines: Vec<&str> = (0..10).map(|_| "x").collect();
        let text = snapshot(&lines, 0, 0);
        assert_eq!(
            press_keys(&mut v, "2d3j", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 6 }]
        );
    }

    #[test]
    fn count_before_operator() {
        // 3dw = delete 3 words (operator_count=3, motion_count=None -> 3)
        let mut v = normal();
        let text = snapshot(&["aaa bbb ccc ddd"], 0, 0);
        let result = press_keys(&mut v, "3dw", &text);
        // 3 words forward from col 0: aaa->bbb->ccc->ddd, so target is (0,12)
        // exclusive: to = (0,11)
        assert_eq!(
            result,
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 11)
            }]
        );
    }

    #[test]
    fn count_after_operator() {
        // d3w = same as 3dw
        let mut v = normal();
        let text = snapshot(&["aaa bbb ccc ddd"], 0, 0);
        let result = press_keys(&mut v, "d3w", &text);
        assert_eq!(
            result,
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 11)
            }]
        );
    }

    // -- Operators: doubled (dd, cc, yy) -------------------------------------

    #[test]
    fn dd_deletes_current_line() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 1, 0);
        assert_eq!(
            press_keys(&mut v, "dd", &text),
            vec![VimCommand::DeleteLines { first: 1, last: 1 }]
        );
    }

    #[test]
    fn cc_changes_current_line() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "cc", &text),
            vec![
                VimCommand::ChangeLines { first: 0, last: 0 },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn yy_yanks_current_line() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "yy", &text),
            vec![VimCommand::YankLines { first: 0, last: 0 }]
        );
    }

    #[test]
    fn counted_dd() {
        let mut v = normal();
        let text = snapshot(&["a", "b", "c", "d", "e"], 1, 0);
        assert_eq!(
            press_keys(&mut v, "3dd", &text),
            vec![VimCommand::DeleteLines { first: 1, last: 3 }]
        );
    }

    #[test]
    fn dd_clamps_at_end_of_file() {
        let mut v = normal();
        let text = snapshot(&["a", "b", "c"], 1, 0);
        assert_eq!(
            press_keys(&mut v, "5dd", &text),
            vec![VimCommand::DeleteLines { first: 1, last: 2 }]
        );
    }

    #[test]
    fn counted_line_end_motion_is_linewise() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 0, 0);
        // d2$ - count > 1 makes $ linewise
        assert_eq!(
            press_keys(&mut v, "d2$", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 1 }]
        );
    }

    #[test]
    fn counted_percent_operator_is_linewise() {
        let mut v = normal();
        let lines: Vec<&str> = (0..10).map(|_| "x").collect();
        let text = snapshot(&lines, 0, 0);
        assert_eq!(
            press_keys(&mut v, "d50%", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 4 }]
        );
    }

    // -- Single-key actions --------------------------------------------------

    #[test]
    fn i_enters_insert() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'i', &snapshot(&["hello"], 0, 2)),
            vec![VimCommand::EnterInsert]
        );
    }

    #[test]
    fn a_moves_right_and_inserts() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'a', &snapshot(&["hello"], 0, 2)),
            vec![VimCommand::MoveTo(pos(0, 3)), VimCommand::EnterInsert],
        );
    }

    #[test]
    fn a_at_end_of_line() {
        let mut v = normal();
        // col 4 is last char of "hello" (len=5), a moves to col 5 (= line_len)
        assert_eq!(
            press(&mut v, 'a', &snapshot(&["hello"], 0, 4)),
            vec![VimCommand::MoveTo(pos(0, 5)), VimCommand::EnterInsert],
        );
    }

    #[test]
    fn a_on_empty_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'a', &snapshot(&[""], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0)), VimCommand::EnterInsert],
        );
    }

    #[test]
    fn big_i_to_first_non_blank() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'I', &snapshot(&["  hello"], 0, 5)),
            vec![VimCommand::MoveTo(pos(0, 2)), VimCommand::EnterInsert],
        );
    }

    #[test]
    fn big_a_to_end_of_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'A', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 5)), VimCommand::EnterInsert],
        );
    }

    #[test]
    fn o_opens_line_below() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'o', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::OpenLineBelow, VimCommand::EnterInsert],
        );
    }

    #[test]
    fn big_o_opens_line_above() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'O', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::OpenLineAbove, VimCommand::EnterInsert],
        );
    }

    #[test]
    fn x_deletes_char() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'x', &snapshot(&["hello"], 0, 2)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 2),
                to: pos(0, 2)
            }],
        );
    }

    #[test]
    fn x_on_empty_line_is_noop() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'x', &snapshot(&[""], 0, 0)),
            vec![VimCommand::Noop]
        );
    }

    #[test]
    fn counted_x() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "3x", &snapshot(&["hello"], 0, 1)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 1),
                to: pos(0, 3)
            }],
        );
    }

    #[test]
    fn x_clamps_at_line_end() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "99x", &snapshot(&["hi"], 0, 0)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 1)
            }],
        );
    }

    #[test]
    fn big_x_deletes_before_cursor() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'X', &snapshot(&["hello"], 0, 3)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 2),
                to: pos(0, 2)
            }],
        );
    }

    #[test]
    fn big_x_at_column_zero_is_noop() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'X', &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::Noop]
        );
    }

    #[test]
    fn s_changes_char() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 's', &snapshot(&["hello"], 0, 2)),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 2),
                    to: pos(0, 2),
                },
                VimCommand::EnterInsert,
            ],
        );
    }

    #[test]
    fn s_on_empty_line_enters_insert() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 's', &snapshot(&[""], 0, 0)),
            vec![VimCommand::EnterInsert]
        );
    }

    #[test]
    fn big_d_deletes_to_eol() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'D', &snapshot(&["hello"], 0, 1)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 1),
                to: pos(0, 4)
            }],
        );
    }

    #[test]
    fn big_d_on_empty_line_is_noop() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'D', &snapshot(&[""], 0, 0)),
            vec![VimCommand::Noop]
        );
    }

    #[test]
    fn big_c_changes_to_eol() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'C', &snapshot(&["hello"], 0, 1)),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 1),
                    to: pos(0, 4),
                },
                VimCommand::EnterInsert,
            ],
        );
    }

    #[test]
    fn big_c_on_empty_line_enters_insert() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'C', &snapshot(&[""], 0, 0)),
            vec![VimCommand::EnterInsert]
        );
    }

    #[test]
    fn big_j_joins_two_lines() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'J', &snapshot(&["a", "b"], 0, 0)),
            vec![VimCommand::JoinLines { count: 1 }]
        );
    }

    #[test]
    fn counted_big_j() {
        let mut v = normal();
        // 3J joins 3 lines = 2 join operations
        assert_eq!(
            press_keys(&mut v, "3J", &snapshot(&["a", "b", "c", "d"], 0, 0)),
            vec![VimCommand::JoinLines { count: 2 }]
        );
    }

    #[test]
    fn big_s_changes_line() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 0);
        assert_eq!(
            press(&mut v, 'S', &text),
            vec![
                VimCommand::ChangeLines { first: 0, last: 0 },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn p_paste_after() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'p', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::PasteAfter]
        );
    }

    #[test]
    fn big_p_paste_before() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'P', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::PasteBefore]
        );
    }

    #[test]
    fn u_undo() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'u', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::Undo]
        );
    }

    #[test]
    fn ctrl_r_redo() {
        let mut v = normal();
        assert_eq!(
            press_ctrl(&mut v, 'r', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::Redo]
        );
    }

    #[test]
    fn r_replaces_char() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "ra", &snapshot(&["hello"], 0, 2)),
            vec![VimCommand::ReplaceChar { ch: 'a', count: 1 }],
        );
    }

    #[test]
    fn r_on_empty_line_is_noop() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "ra", &snapshot(&[""], 0, 0)),
            vec![VimCommand::Noop]
        );
    }

    #[test]
    fn counted_r() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "3ra", &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::ReplaceChar { ch: 'a', count: 3 }],
        );
    }

    #[test]
    fn slash_opens_find() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '/', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::OpenFind]
        );
    }

    #[test]
    fn n_find_next() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'n', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::FindNext]
        );
    }

    #[test]
    fn big_n_find_prev() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'N', &snapshot(&["x"], 0, 0)),
            vec![VimCommand::FindPrev]
        );
    }

    // -- Visual mode: entering and exiting -----------------------------------

    #[test]
    fn v_enters_visual() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        let result = press(&mut v, 'v', &text);
        assert_eq!(v.mode, Mode::Visual);
        assert_eq!(v.visual_anchor, Some(pos(0, 2)));
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 2),
                head: pos(0, 2)
            }]
        );
    }

    #[test]
    fn big_v_enters_visual_line() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        let result = press(&mut v, 'V', &text);
        assert_eq!(v.mode, Mode::VisualLine);
        assert_eq!(v.visual_anchor, Some(pos(0, 2)));
        // VisualLine selects full line via selection_command
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 4)
            }]
        );
    }

    #[test]
    fn v_in_visual_exits() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        press(&mut v, 'v', &text); // enter Visual
        let result = press(&mut v, 'v', &text); // exit
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(v.visual_anchor, None);
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 2))]);
    }

    #[test]
    fn big_v_in_visual_line_exits() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        press(&mut v, 'V', &text);
        let result = press(&mut v, 'V', &text);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 2))]);
    }

    #[test]
    fn v_in_visual_line_switches_to_charwise() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        press(&mut v, 'V', &text); // enter VisualLine
        let result = press(&mut v, 'v', &text); // switch to Visual
        assert_eq!(v.mode, Mode::Visual);
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 2),
                head: pos(0, 2)
            }]
        );
    }

    #[test]
    fn big_v_in_visual_switches_to_linewise() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        press(&mut v, 'v', &text); // enter Visual
        let result = press(&mut v, 'V', &text); // switch to VisualLine
        assert_eq!(v.mode, Mode::VisualLine);
        // VisualLine expands to full line
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 4)
            }]
        );
    }

    // -- Visual mode: motions extend selection -------------------------------

    #[test]
    fn visual_l_extends_selection() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, 'l', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 1),
                head: pos(0, 2)
            }],
        );
    }

    #[test]
    fn visual_j_extends_selection() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 1);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, 'j', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 1),
                head: pos(1, 1)
            }],
        );
    }

    #[test]
    fn visual_w_extends_selection() {
        let mut v = normal();
        let text = snapshot(&["foo bar baz"], 0, 0);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, 'w', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 4)
            }],
        );
    }

    #[test]
    fn visual_dollar_extends_to_eol() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, '$', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 1),
                head: pos(0, 4)
            }],
        );
    }

    #[test]
    fn visual_gg_extends_to_top() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "ccc"], 2, 1);
        press(&mut v, 'v', &text);
        assert_eq!(
            press_keys(&mut v, "gg", &text),
            vec![VimCommand::Select {
                anchor: pos(2, 1),
                head: pos(0, 0)
            }],
        );
    }

    #[test]
    fn visual_big_g_extends_to_bottom() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "ccc"], 0, 0);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, 'G', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(2, 0)
            }],
        );
    }

    #[test]
    fn visual_f_extends_selection() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 0);
        press(&mut v, 'v', &text);
        assert_eq!(
            press_keys(&mut v, "fw", &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 6)
            }],
        );
    }

    #[test]
    fn visual_percent_extends_selection() {
        let mut v = normal();
        let text = snapshot(&["(foo)"], 0, 0);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, '%', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 4)
            }],
        );
    }

    #[test]
    fn visual_line_j_extends_full_lines() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 0, 1);
        press(&mut v, 'V', &text);
        let result = press(&mut v, 'j', &text);
        // VisualLine: full first to last line
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(1, 2)
            }]
        );
    }

    #[test]
    fn visual_count_motion() {
        let mut v = normal();
        let text = snapshot(&["abcdefghij"], 0, 0);
        press(&mut v, 'v', &text);
        assert_eq!(
            press_keys(&mut v, "3l", &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 3)
            }],
        );
    }

    #[test]
    fn visual_find_partial_uses_counts() {
        let mut v = normal();
        let text = snapshot(&["foo bar baz boom"], 0, 0);
        press(&mut v, 'v', &text);
        assert_eq!(
            press_keys(&mut v, "3fb", &text),
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 12)
            }],
        );
    }

    #[test]
    fn visual_zero_is_motion_not_digit() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 3);
        press(&mut v, 'v', &text);
        assert_eq!(
            press(&mut v, '0', &text),
            vec![VimCommand::Select {
                anchor: pos(0, 3),
                head: pos(0, 0)
            }],
        );
    }

    // -- Visual mode: operators on selection ----------------------------------

    #[test]
    fn visual_d_deletes_charwise() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 2);
        press(&mut v, 'v', &text); // anchor = (0,2)
                                   // Simulate cursor at col 4 after motions
        let text2 = snapshot(&["hello world"], 0, 4);
        let result = press(&mut v, 'd', &text2);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(
            result,
            vec![VimCommand::DeleteRange {
                from: pos(0, 2),
                to: pos(0, 4)
            }]
        );
    }

    #[test]
    fn visual_x_same_as_d() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        press(&mut v, 'v', &text); // anchor = (0,1)
        let text2 = snapshot(&["hello"], 0, 2);
        let result = press(&mut v, 'x', &text2);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(
            result,
            vec![VimCommand::DeleteRange {
                from: pos(0, 1),
                to: pos(0, 2)
            }]
        );
    }

    #[test]
    fn visual_c_changes_and_enters_insert() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        press(&mut v, 'v', &text); // anchor = (0,1)
        let text2 = snapshot(&["hello"], 0, 2);
        let result = press(&mut v, 'c', &text2);
        assert_eq!(v.mode, Mode::Normal); // exit_visual sets Normal, then EnterInsert changes to Insert in main.rs
        assert_eq!(
            result,
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 1),
                    to: pos(0, 2)
                },
                VimCommand::EnterInsert
            ]
        );
    }

    #[test]
    fn visual_s_same_as_c() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        press(&mut v, 'v', &text); // anchor = (0,1)
        let text2 = snapshot(&["hello"], 0, 2);
        let result = press(&mut v, 's', &text2);
        assert_eq!(
            result,
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 1),
                    to: pos(0, 2)
                },
                VimCommand::EnterInsert
            ]
        );
    }

    #[test]
    fn visual_y_yanks_and_moves_to_start() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 1);
        press(&mut v, 'v', &text);
        // Simulate cursor moved to col 3 by pressing 'l' twice
        let text_end = snapshot(&["hello"], 0, 3);
        let result = press(&mut v, 'y', &text_end);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(
            result,
            vec![
                VimCommand::YankRange {
                    from: pos(0, 1),
                    to: pos(0, 3)
                },
                VimCommand::MoveTo(pos(0, 1))
            ]
        );
    }

    #[test]
    fn visual_line_d_deletes_lines() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz", "123"], 0, 0);
        press(&mut v, 'V', &text);
        let text2 = snapshot(&["abc", "xyz", "123"], 1, 0);
        press(&mut v, 'j', &text2);
        let result = press(&mut v, 'd', &text2);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(result, vec![VimCommand::DeleteLines { first: 0, last: 1 }]);
    }

    #[test]
    fn visual_line_c_changes_lines_and_inserts() {
        let mut v = normal();
        let text = snapshot(&["abc", "xyz"], 0, 0);
        press(&mut v, 'V', &text);
        let result = press(&mut v, 'c', &text);
        assert_eq!(
            result,
            vec![
                VimCommand::ChangeLines { first: 0, last: 0 },
                VimCommand::EnterInsert
            ]
        );
    }

    #[test]
    fn visual_line_y_moves_to_col_zero() {
        let mut v = normal();
        let text = snapshot(&["  abc", "  xyz"], 0, 3);
        press(&mut v, 'V', &text);
        let text2 = snapshot(&["  abc", "  xyz"], 1, 2);
        press(&mut v, 'j', &text2);
        let result = press(&mut v, 'y', &text2);
        assert_eq!(
            result,
            vec![
                VimCommand::YankLines { first: 0, last: 1 },
                VimCommand::MoveTo(pos(0, 0))
            ],
        );
    }

    // -- Visual mode: case transform -----------------------------------------

    #[test]
    fn visual_u_lowercases() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["ABC DEF"], 0, 2);
        assert_eq!(
            press(&mut v, 'u', &text),
            vec![VimCommand::TransformCaseRange {
                from: pos(0, 0),
                to: pos(0, 2),
                uppercase: false
            }],
        );
    }

    #[test]
    fn visual_big_u_uppercases() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["abc def"], 0, 2);
        assert_eq!(
            press(&mut v, 'U', &text),
            vec![VimCommand::TransformCaseRange {
                from: pos(0, 0),
                to: pos(0, 2),
                uppercase: true
            }],
        );
    }

    #[test]
    fn visual_line_u_lowercases_lines() {
        let mut v = normal();
        v.mode = Mode::VisualLine;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["ABC", "DEF"], 1, 0);
        assert_eq!(
            press(&mut v, 'u', &text),
            vec![VimCommand::TransformCaseLines {
                first: 0,
                last: 1,
                uppercase: false
            }],
        );
    }

    // -- Visual mode: search -------------------------------------------------

    #[test]
    fn visual_slash_opens_find_stays_visual() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["hello"], 0, 2);
        assert_eq!(press(&mut v, '/', &text), vec![VimCommand::OpenFind]);
        assert_eq!(v.mode, Mode::Visual);
    }

    #[test]
    fn visual_n_find_next_stays_visual() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["hello"], 0, 2);
        assert_eq!(press(&mut v, 'n', &text), vec![VimCommand::FindNext]);
        assert_eq!(v.mode, Mode::Visual);
        assert_eq!(v.visual_anchor, Some(pos(0, 0)));
    }

    // -- Visual mode: text objects -------------------------------------------

    #[test]
    fn visual_iw_selects_word() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 2);
        press(&mut v, 'v', &text);
        let result = press_keys(&mut v, "iw", &text);
        assert_eq!(v.visual_anchor, Some(pos(0, 0)));
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 4)
            }]
        );
    }

    #[test]
    fn visual_ib_selects_inside_parens() {
        let mut v = normal();
        let text = snapshot(&["foo(bar)baz"], 0, 5);
        press(&mut v, 'v', &text);
        let result = press_keys(&mut v, "ib", &text);
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 4),
                head: pos(0, 6)
            }]
        );
    }

    // -- Visual mode: ctrl+r -------------------------------------------------

    #[test]
    fn visual_ctrl_r_exits_and_redoes() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["hello"], 0, 2);
        let result = press_ctrl(&mut v, 'r', &text);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(v.visual_anchor, None);
        assert_eq!(result, vec![VimCommand::Redo]);
    }

    // -- Find repeat (; and ,) -----------------------------------------------

    #[test]
    fn semicolon_repeats_f() {
        let mut v = normal();
        let text = snapshot(&["abcabc"], 0, 0);
        press_keys(&mut v, "fa", &text); // find first 'a' (stays at 0 since first match is at col 0+skip)
                                         // Actually fa from col 0 finds 'a' at col 3 (skips current col)
        let text2 = snapshot(&["abcabc"], 0, 3);
        let result = press(&mut v, ';', &text2);
        // repeats FindChar('a'), from col 3 finds nothing after col 3... wait
        // "abcabc": chars are a(0) b(1) c(2) a(3) b(4) c(5)
        // fa from col 0: skip col 0, check col 1 (b), col 2 (c), col 3 (a) -> match! MoveTo(0,3)
        // ; from col 3: FindChar('a'), skip col 3, check col 4 (b), col 5 (c) -> no match, stays
        // So this test should show the cursor staying at (0,3)
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 3))]);
    }

    #[test]
    fn semicolon_repeats_t() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 0);
        press_keys(&mut v, "tw", &text); // till 'w' -> col 5
        let text2 = snapshot(&["hello world"], 0, 5);
        let result = press(&mut v, ';', &text2);
        // TillChar('w') from col 5: finds 'w' at col 6, till = col 5. Stays.
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 5))]);
    }

    #[test]
    fn comma_reverses_f() {
        let mut v = normal();
        let text = snapshot(&["abcabc"], 0, 0);
        press_keys(&mut v, "fa", &text); // sets last_find to FindChar('a')
        let text2 = snapshot(&["abcabc"], 0, 3);
        let result = press(&mut v, ',', &text2);
        // , reverses to FindCharBack('a'): from col 3, scan backward cols 2,1,0. Col 0 is 'a' -> match
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 0))]);
    }

    #[test]
    fn comma_reverses_big_f() {
        let mut v = normal();
        let text = snapshot(&["abcabc"], 0, 5);
        press_keys(&mut v, "Fa", &text); // FindCharBack('a'), finds 'a' at col 3
        let text2 = snapshot(&["abcabc"], 0, 3);
        let result = press(&mut v, ',', &text2);
        // , reverses FindCharBack to FindChar: from col 3, skip col 3+1=4, find 'a'... no more 'a' after col 4
        // stays at (0,3)
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 3))]);
    }

    #[test]
    fn semicolon_without_prior_find_is_noop() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        // no prior find, repeat_find returns None, falls through to Noop
        assert_eq!(press(&mut v, ';', &text), vec![VimCommand::Noop]);
    }

    #[test]
    fn find_persists_across_other_commands() {
        let mut v = normal();
        let text = snapshot(&["abcabc"], 0, 0);
        press_keys(&mut v, "fa", &text); // find 'a' at col 3
        let text2 = snapshot(&["abcabc"], 0, 3);
        press(&mut v, 'l', &text2); // some other motion
        let text3 = snapshot(&["abcabc"], 0, 4);
        // ; should still repeat the find
        // repeat_find returns FindChar('a'), but from col 4: skip to 5, no more 'a' -> stays
        let result = press(&mut v, ';', &text3);
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 4))]);
    }

    #[test]
    fn find_repeat_with_operator() {
        let mut v = normal();
        let text = snapshot(&["abcabc"], 0, 0);
        press_keys(&mut v, "fa", &text); // sets last_find
        let text2 = snapshot(&["abcabc"], 0, 3);
        // d; should delete to next 'a' using repeat find
        // But from col 3, FindChar('a') finds no match after col 3+1=4 (cols 4=b, 5=c)
        // target == cursor -> Noop
        let result = press_keys(&mut v, "d;", &text2);
        assert_eq!(result, vec![VimCommand::Noop]);
    }

    #[test]
    fn find_repeat_in_visual_extends_selection() {
        let mut v = normal();
        let text = snapshot(&["aXbXcXd"], 0, 0);
        press_keys(&mut v, "fX", &text); // find 'X' at col 1
        let text2 = snapshot(&["aXbXcXd"], 0, 1);
        press(&mut v, 'v', &text2); // enter Visual at col 1
        let result = press(&mut v, ';', &text2); // repeat find, extends selection
                                                 // FindChar('X') from col 1: skip col 1+1=2, col 2 is 'b', col 3 is 'X' -> match
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 1),
                head: pos(0, 3)
            }]
        );
    }

    #[test]
    fn visual_find_updates_last_find() {
        // Regression test: visual mode f/t must update last_find for ; to work
        let mut v = normal();
        let text = snapshot(&["aXbXcXd"], 0, 0);
        press(&mut v, 'v', &text); // enter Visual
        press_keys(&mut v, "fX", &text); // find 'X' in visual mode
                                         // last_find should now be set to FindChar('X')
        let text2 = snapshot(&["aXbXcXd"], 0, 1);
        let result = press(&mut v, ';', &text2); // repeat should work
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(0, 3)
            }]
        );
    }

    // -- Text objects: word (iw, aw) -----------------------------------------

    #[test]
    fn iw_selects_word() {
        let text = snapshot(&["hello world"], 0, 2);
        assert_eq!(
            text_object(&text, 'w', true, None),
            Some((pos(0, 0), pos(0, 4)))
        );
    }

    #[test]
    fn aw_includes_trailing_space() {
        let text = snapshot(&["hello world"], 0, 2);
        assert_eq!(
            text_object(&text, 'w', false, None),
            Some((pos(0, 0), pos(0, 5)))
        );
    }

    #[test]
    fn iw_on_whitespace() {
        let text = snapshot(&["foo   bar"], 0, 4);
        assert_eq!(
            text_object(&text, 'w', true, None),
            Some((pos(0, 3), pos(0, 5)))
        );
    }

    #[test]
    fn aw_on_whitespace_includes_following_word() {
        let text = snapshot(&["foo   bar"], 0, 4);
        assert_eq!(
            text_object(&text, 'w', false, None),
            Some((pos(0, 3), pos(0, 8)))
        );
    }

    #[test]
    fn iw_on_empty_line() {
        let text = snapshot(&[""], 0, 0);
        assert_eq!(text_object(&text, 'w', true, None), None);
    }

    #[test]
    fn counted_iw() {
        let text = snapshot(&["foo bar baz"], 0, 0);
        assert_eq!(
            text_object(&text, 'w', true, Some(2)),
            Some((pos(0, 0), pos(0, 3)))
        );
    }

    #[test]
    fn big_iw() {
        let text = snapshot(&["foo.bar baz"], 0, 2);
        assert_eq!(
            text_object(&text, 'W', true, None),
            Some((pos(0, 0), pos(0, 6)))
        );
    }

    // -- Text objects: pairs (ib, ab, iB, aB, etc.) --------------------------

    #[test]
    fn ib_inside_parens() {
        let text = snapshot(&["foo(bar)baz"], 0, 5);
        assert_eq!(
            text_object(&text, 'b', true, None),
            Some((pos(0, 4), pos(0, 6)))
        );
    }

    #[test]
    fn ab_around_parens() {
        let text = snapshot(&["foo(bar)baz"], 0, 5);
        assert_eq!(
            text_object(&text, 'b', false, None),
            Some((pos(0, 3), pos(0, 7)))
        );
    }

    #[test]
    fn ib_empty_parens_is_none() {
        let text = snapshot(&["()"], 0, 0);
        assert_eq!(text_object(&text, 'b', true, None), None);
    }

    #[test]
    fn ib_nested() {
        let text = snapshot(&["((foo))"], 0, 3);
        assert_eq!(
            text_object(&text, 'b', true, None),
            Some((pos(0, 2), pos(0, 4)))
        );
    }

    #[test]
    fn ib_cursor_on_close_paren() {
        let text = snapshot(&["(foo)"], 0, 4);
        assert_eq!(
            text_object(&text, 'b', true, None),
            Some((pos(0, 1), pos(0, 3)))
        );
    }

    #[test]
    fn i_brace_inside_braces() {
        let text = snapshot(&["{foo}"], 0, 2);
        assert_eq!(
            text_object(&text, 'B', true, None),
            Some((pos(0, 1), pos(0, 3)))
        );
    }

    #[test]
    fn i_bracket() {
        let text = snapshot(&["[foo]"], 0, 2);
        assert_eq!(
            text_object(&text, '[', true, None),
            Some((pos(0, 1), pos(0, 3)))
        );
    }

    #[test]
    fn i_angle() {
        let text = snapshot(&["<foo>"], 0, 2);
        assert_eq!(
            text_object(&text, '<', true, None),
            Some((pos(0, 1), pos(0, 3)))
        );
    }

    #[test]
    fn pair_cross_line() {
        let text = snapshot(&["(", "foo", ")"], 1, 1);
        assert_eq!(
            text_object(&text, 'b', true, None),
            Some((pos(1, 0), pos(1, 2)))
        );
    }

    #[test]
    fn pair_no_match() {
        let text = snapshot(&["foo(bar"], 0, 5);
        // Unclosed paren: forward scan finds no match
        assert_eq!(text_object(&text, 'b', true, None), None);
    }

    // -- Text objects: quotes (i", a", i', a', i`, a`) -----------------------

    #[test]
    fn i_double_quote() {
        let text = snapshot(&["say \"hello\" ok"], 0, 6);
        assert_eq!(
            text_object(&text, '"', true, None),
            Some((pos(0, 5), pos(0, 9)))
        );
    }

    #[test]
    fn a_double_quote() {
        let text = snapshot(&["say \"hello\" ok"], 0, 6);
        assert_eq!(
            text_object(&text, '"', false, None),
            Some((pos(0, 4), pos(0, 10)))
        );
    }

    #[test]
    fn i_single_quote() {
        let text = snapshot(&["say 'hi' ok"], 0, 6);
        assert_eq!(
            text_object(&text, '\'', true, None),
            Some((pos(0, 5), pos(0, 6)))
        );
    }

    #[test]
    fn i_backtick() {
        let text = snapshot(&["say `hi` ok"], 0, 6);
        assert_eq!(
            text_object(&text, '`', true, None),
            Some((pos(0, 5), pos(0, 6)))
        );
    }

    #[test]
    fn quote_escaped() {
        let text = snapshot(&["let s = \"a\\\"b\""], 0, 12);
        assert_eq!(
            text_object(&text, '"', true, None),
            Some((pos(0, 9), pos(0, 12)))
        );
    }

    #[test]
    fn quote_nearest_pair() {
        let text = snapshot(&["\"foo \"bar\""], 0, 7);
        assert_eq!(
            text_object(&text, '"', true, None),
            Some((pos(0, 6), pos(0, 8)))
        );
    }

    #[test]
    fn quote_gap_between_strings() {
        let text = snapshot(&["\"one\" \"two\""], 0, 5);
        assert_eq!(text_object(&text, '"', true, None), None);
    }

    #[test]
    fn quote_empty_inner_is_none() {
        let text = snapshot(&["\"\""], 0, 0);
        assert_eq!(text_object(&text, '"', true, None), None);
    }

    // -- Text objects: with operators ----------------------------------------

    #[test]
    fn diw() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 2);
        assert_eq!(
            press_keys(&mut v, "diw", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 4)
            }],
        );
    }

    #[test]
    fn ciw() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 2);
        assert_eq!(
            press_keys(&mut v, "ciw", &text),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 0),
                    to: pos(0, 4)
                },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn yiw() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 2);
        assert_eq!(
            press_keys(&mut v, "yiw", &text),
            vec![
                VimCommand::YankRange {
                    from: pos(0, 0),
                    to: pos(0, 4)
                },
                VimCommand::MoveTo(pos(0, 0))
            ],
        );
    }

    #[test]
    fn dib() {
        let mut v = normal();
        let text = snapshot(&["foo(bar)"], 0, 5);
        assert_eq!(
            press_keys(&mut v, "dib", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 4),
                to: pos(0, 6)
            }],
        );
    }

    // -- Escape handling -----------------------------------------------------

    #[test]
    fn escape_from_insert_moves_cursor_left() {
        let mut v = VimState::new(); // starts in Insert
        let text = snapshot(&["hello"], 0, 3);
        let result = v.enter_normal_from_escape(text.cursor, &text);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 2))]);
    }

    #[test]
    fn escape_from_insert_at_col_zero_stays() {
        let mut v = VimState::new();
        let text = snapshot(&["hello"], 0, 0);
        let result = v.enter_normal_from_escape(text.cursor, &text);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(result, vec![VimCommand::Noop]);
    }

    #[test]
    fn escape_from_visual_clears_selection() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["hello"], 0, 3);
        let result = v.enter_normal_from_escape(text.cursor, &text);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(v.visual_anchor, None);
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 3))]);
    }

    #[test]
    fn escape_from_visual_line_clears_selection() {
        let mut v = normal();
        v.mode = Mode::VisualLine;
        v.visual_anchor = Some(pos(0, 0));
        let text = snapshot(&["hello"], 0, 3);
        let result = v.enter_normal_from_escape(text.cursor, &text);
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(v.visual_anchor, None);
        assert_eq!(result, vec![VimCommand::MoveTo(pos(0, 3))]);
    }

    #[test]
    fn escape_in_normal_clears_pending() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        press(&mut v, 'd', &text); // set pending operator
        let result = v.enter_normal_from_escape(text.cursor, &text);
        assert_eq!(v.pending_display(), "");
        assert_eq!(result, vec![VimCommand::Noop]);
    }

    // -- Pending state -------------------------------------------------------

    #[test]
    fn f_waits_for_char() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        assert_eq!(press(&mut v, 'f', &text), vec![VimCommand::Noop]);
        assert_eq!(v.pending_display(), "f");
    }

    #[test]
    fn g_waits_for_second_g() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb"], 1, 0);
        assert_eq!(press(&mut v, 'g', &text), vec![VimCommand::Noop]);
        assert_eq!(v.pending_display(), "g");
    }

    #[test]
    fn g_then_non_g_cancels() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb"], 1, 0);
        press(&mut v, 'g', &text);
        let result = press(&mut v, 'x', &text);
        assert_eq!(result, vec![VimCommand::Noop]);
        assert_eq!(v.pending_display(), "");
    }

    #[test]
    fn r_waits_for_char() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 2);
        assert_eq!(press(&mut v, 'r', &text), vec![VimCommand::Noop]);
        assert_eq!(v.pending_display(), "r");
    }

    #[test]
    fn d_then_unknown_cancels() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        press(&mut v, 'd', &text);
        let result = press(&mut v, 'z', &text);
        assert_eq!(result, vec![VimCommand::Noop]);
        assert_eq!(v.pending_display(), "");
    }

    #[test]
    fn d_f_x_deletes_to_char() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dfo", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 4)
            }],
        );
    }

    #[test]
    fn d_gg_deletes_to_top() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "ccc"], 2, 0);
        assert_eq!(
            press_keys(&mut v, "dgg", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 2 }],
        );
    }

    #[test]
    fn d_i_w_text_object() {
        let mut v = normal();
        let text = snapshot(&["hello world"], 0, 2);
        assert_eq!(
            press_keys(&mut v, "diw", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 4)
            }],
        );
    }

    #[test]
    fn d_i_unknown_cancels() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        let result = press_keys(&mut v, "diz", &text);
        assert_eq!(result, vec![VimCommand::Noop]);
        assert_eq!(v.pending_display(), "");
    }

    #[test]
    fn pending_display_shows_count_operator_count_partial() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        press(&mut v, '3', &text);
        assert_eq!(v.pending_display(), "3");
        press(&mut v, 'd', &text);
        assert_eq!(v.pending_display(), "3d");
        press(&mut v, '2', &text);
        assert_eq!(v.pending_display(), "3d2");
        press(&mut v, 'f', &text);
        assert_eq!(v.pending_display(), "3d2f");
    }

    #[test]
    fn pending_display_empty_by_default() {
        let v = normal();
        assert_eq!(v.pending_display(), "");
    }

    // -- Tab switch ----------------------------------------------------------

    #[test]
    fn tab_switch_clears_pending_operator() {
        let mut v = normal();
        let text = snapshot(&["foo bar"], 0, 0);
        press(&mut v, 'd', &text);
        v.on_tab_switch();
        assert_eq!(
            press(&mut v, 'w', &text),
            vec![VimCommand::MoveTo(pos(0, 4))]
        );
    }

    #[test]
    fn tab_switch_exits_visual() {
        let mut v = normal();
        v.mode = Mode::Visual;
        v.visual_anchor = Some(pos(0, 0));
        v.on_tab_switch();
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(v.visual_anchor, None);
    }

    #[test]
    fn tab_switch_preserves_insert() {
        let mut v = VimState::new(); // starts Insert
        v.on_tab_switch();
        assert_eq!(v.mode, Mode::Insert);
    }

    // -- Insert mode passthrough ---------------------------------------------

    #[test]
    fn insert_mode_returns_empty() {
        let mut v = VimState::new(); // Insert mode
        let text = snapshot(&["hello"], 0, 0);
        assert_eq!(v.handle_key(&key('x'), mods(), &text), vec![]);
    }

    // -- Ctrl passthrough in normal mode -------------------------------------

    #[test]
    fn ctrl_non_r_is_noop() {
        let mut v = normal();
        let text = snapshot(&["hello"], 0, 0);
        assert_eq!(press_ctrl(&mut v, 'x', &text), vec![VimCommand::Noop]);
    }

    // -- Edge cases ----------------------------------------------------------

    #[test]
    fn w_from_empty_line_to_next() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["", "foo"], 0, 0)),
            vec![VimCommand::MoveTo(pos(1, 0))]
        );
    }

    #[test]
    fn b_to_empty_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'b', &snapshot(&["", "foo"], 1, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn x_on_single_char_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'x', &snapshot(&["a"], 0, 0)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 0)
            }],
        );
    }

    #[test]
    fn dollar_on_single_char_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, '$', &snapshot(&["a"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn l_on_single_char_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'l', &snapshot(&["a"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn big_g_on_single_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'G', &snapshot(&["foo"], 0, 0)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn gg_on_single_line() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "gg", &snapshot(&["foo"], 0, 2)),
            vec![VimCommand::MoveTo(pos(0, 0))]
        );
    }

    #[test]
    fn dw_at_last_word_of_file_deletes() {
        let mut v = normal();
        // vim: dw on last word of file deletes to end (w can't find next word, becomes inclusive)
        assert_eq!(
            press_keys(&mut v, "dw", &snapshot(&["foo"], 0, 2)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 2),
                to: pos(0, 2)
            }],
        );
    }

    #[test]
    fn dw_on_only_word_deletes_all() {
        let mut v = normal();
        // dw from start of sole word: deletes entire word
        assert_eq!(
            press_keys(&mut v, "dw", &snapshot(&["hello"], 0, 0)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 4)
            }],
        );
    }

    #[test]
    fn d_dollar_on_single_char_deletes() {
        let mut v = normal();
        // d$ on "a" at col 0: $ returns col 0, still deletes the char
        assert_eq!(
            press_keys(&mut v, "d$", &snapshot(&["a"], 0, 0)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 0)
            }],
        );
    }

    #[test]
    fn dl_on_single_char_deletes() {
        let mut v = normal();
        // dl on "a": l clamped to col 0, but still deletes (vim exception for 0-char motions)
        assert_eq!(
            press_keys(&mut v, "dl", &snapshot(&["a"], 0, 0)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 0)
            }],
        );
    }

    #[test]
    fn de_at_end_of_file_deletes() {
        let mut v = normal();
        assert_eq!(
            press_keys(&mut v, "de", &snapshot(&["x"], 0, 0)),
            vec![VimCommand::DeleteRange {
                from: pos(0, 0),
                to: pos(0, 0)
            }],
        );
    }

    #[test]
    fn w_on_whitespace_only_line() {
        let mut v = normal();
        assert_eq!(
            press(&mut v, 'w', &snapshot(&["   ", "foo"], 0, 0)),
            vec![VimCommand::MoveTo(pos(1, 0))]
        );
    }

    // -- Backward inclusive motion fixes ---------------------------------

    #[test]
    fn d_big_f_excludes_cursor_char() {
        let mut v = normal();
        // "hello world", cursor at col 7 ('o' in world), dFo finds 'o' at col 4
        // vim: deletes cols 4-6 ("o w"), NOT including cursor char at col 7
        let text = snapshot(&["hello world"], 0, 7);
        assert_eq!(
            press_keys(&mut v, "dFo", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 4),
                to: pos(0, 6)
            }],
        );
    }

    #[test]
    fn d_big_t_excludes_cursor_char() {
        let mut v = normal();
        // "hello world", cursor at col 7, dTo: T finds 'o' at col 4, stops at col 5
        // vim: deletes cols 5-6 (" w")
        let text = snapshot(&["hello world"], 0, 7);
        assert_eq!(
            press_keys(&mut v, "dTo", &text),
            vec![VimCommand::DeleteRange {
                from: pos(0, 5),
                to: pos(0, 6)
            }],
        );
    }

    // -- cw on whitespace ------------------------------------------------

    #[test]
    fn cw_on_whitespace_does_not_remap_to_ce() {
        let mut v = normal();
        // "foo   bar" cursor on space at col 3: cw should change only spaces (to col 5),
        // NOT remap to ce which would change spaces + "bar"
        let text = snapshot(&["foo   bar"], 0, 3);
        assert_eq!(
            press_keys(&mut v, "cw", &text),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 3),
                    to: pos(0, 5)
                },
                VimCommand::EnterInsert
            ],
        );
    }

    #[test]
    fn cw_on_word_still_remaps_to_ce() {
        let mut v = normal();
        // "foo bar" cursor on 'f': cw should remap to ce, changing only "foo"
        let text = snapshot(&["foo bar"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "cw", &text),
            vec![
                VimCommand::ChangeRange {
                    from: pos(0, 0),
                    to: pos(0, 2)
                },
                VimCommand::EnterInsert
            ],
        );
    }

    // -- Paragraph text object (ip/ap) --------------------------------------

    #[test]
    fn dip_deletes_inner_paragraph() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "", "ccc"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dip", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 1 }],
        );
    }

    #[test]
    fn dap_deletes_paragraph_with_trailing_blank() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "", "ccc"], 0, 0);
        assert_eq!(
            press_keys(&mut v, "dap", &text),
            vec![VimCommand::DeleteLines { first: 0, last: 2 }],
        );
    }

    #[test]
    fn dip_on_blank_line_deletes_blank_region() {
        let mut v = normal();
        let text = snapshot(&["aaa", "", "", "bbb"], 1, 0);
        assert_eq!(
            press_keys(&mut v, "dip", &text),
            vec![VimCommand::DeleteLines { first: 1, last: 2 }],
        );
    }

    #[test]
    fn dap_on_blank_line_includes_following_paragraph() {
        let mut v = normal();
        let text = snapshot(&["aaa", "", "", "bbb", "ccc"], 1, 0);
        assert_eq!(
            press_keys(&mut v, "dap", &text),
            vec![VimCommand::DeleteLines { first: 1, last: 4 }],
        );
    }

    #[test]
    fn yip_yanks_paragraph() {
        let mut v = normal();
        let text = snapshot(&["", "aaa", "bbb", "", "ccc"], 2, 0);
        assert_eq!(
            press_keys(&mut v, "yip", &text),
            vec![VimCommand::YankLines { first: 1, last: 2 }],
        );
    }

    #[test]
    fn visual_ip_selects_paragraph() {
        let mut v = normal();
        let text = snapshot(&["aaa", "bbb", "", "ccc"], 0, 1);
        press(&mut v, 'v', &text);
        let result = press_keys(&mut v, "ip", &text);
        assert_eq!(
            result,
            vec![VimCommand::Select {
                anchor: pos(0, 0),
                head: pos(1, 2)
            }]
        );
    }

    // -- * and # (search word under cursor) ------------------------------

    #[test]
    fn star_searches_word_under_cursor() {
        let mut v = normal();
        let text = snapshot(&["foo bar foo"], 0, 0);
        assert_eq!(
            press(&mut v, '*', &text),
            vec![VimCommand::SearchWordUnderCursor {
                word: "foo".into(),
                forward: true
            }],
        );
    }

    #[test]
    fn hash_searches_word_backward() {
        let mut v = normal();
        let text = snapshot(&["foo bar foo"], 0, 8);
        assert_eq!(
            press(&mut v, '#', &text),
            vec![VimCommand::SearchWordUnderCursor {
                word: "foo".into(),
                forward: false
            }],
        );
    }

    #[test]
    fn star_on_non_word_is_noop() {
        let mut v = normal();
        let text = snapshot(&["  "], 0, 0);
        assert_eq!(press(&mut v, '*', &text), vec![VimCommand::Noop]);
    }

    #[test]
    fn star_mid_word() {
        let mut v = normal();
        let text = snapshot(&["hello_world"], 0, 3);
        assert_eq!(
            press(&mut v, '*', &text),
            vec![VimCommand::SearchWordUnderCursor {
                word: "hello_world".into(),
                forward: true
            }],
        );
    }
}
