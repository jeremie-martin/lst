//! Vim mode state machine.
//!
//! Pure keystroke to command translation. The caller executes commands against
//! whatever editor surface owns the document state.

use crate::effect::RevealIntent;
use crate::position::Position;
use crate::selection::{
    cell_containing_char, cell_partition_by_char, cells_of_str, is_identifier_char,
    last_grapheme_column, next_grapheme_column, previous_grapheme_column, vim_token_class,
    GraphemeCell, TokenClass,
};
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
    IndentLines {
        first: usize,
        last: usize,
    },
    OutdentLines {
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
    SurroundRange {
        from: Position,
        to: Position,
        open: char,
        close: char,
    },
    DeleteSurround {
        open: char,
    },
    ChangeSurround {
        from_open: char,
        to_open: char,
    },
    JumpToLastEdit {
        enter_insert: bool,
    },
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
    surround: Option<SurroundPhase>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Delete,
    Change,
    Yank,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SurroundPhase {
    AddAwaitMotion,
    AddAwaitInner(char),
    AddAwaitDelim { from: Position, to: Position },
    DeleteAwaitDelim,
    ChangeAwaitFrom,
    ChangeAwaitTo { from_open: char },
}

#[derive(Clone, Copy)]
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
        match &self.pending.surround {
            Some(SurroundPhase::AddAwaitMotion) => s.push_str("ys"),
            Some(SurroundPhase::AddAwaitInner(c)) => {
                s.push_str("ys");
                s.push(*c);
            }
            Some(SurroundPhase::AddAwaitDelim { .. }) => s.push_str("ys…"),
            Some(SurroundPhase::DeleteAwaitDelim) => s.push_str("ds"),
            Some(SurroundPhase::ChangeAwaitFrom) => s.push_str("cs"),
            Some(SurroundPhase::ChangeAwaitTo { .. }) => s.push_str("cs…"),
            None => {}
        }
        s
    }

    pub fn clear_pending(&mut self) {
        self.pending = Pending::default();
    }

    pub fn clear_preferred_column(&mut self) {
        self.preferred_column = None;
    }

    fn clear_command_state(&mut self) {
        self.clear_pending();
        self.clear_preferred_column();
    }

    pub fn on_tab_switch(&mut self) {
        self.clear_command_state();
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
        }
    }

    fn exit_visual(&mut self) {
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.clear_command_state();
    }

    fn repeat_find(&self, c: char) -> Option<Motion> {
        if c != ';' && c != ',' {
            return None;
        }
        let last = self.last_find?;
        Some(if c == ';' { last } else { reverse_find(last) })
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
        self.last_find = Some(motion);
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
                self.clear_command_state();
                // vim: cursor moves left by 1 when leaving Insert (unless at col 0).
                // Step by grapheme cluster, then clamp to the start of the last
                // cluster so we never land mid-cluster on a multi-char grapheme.
                let line_text = text
                    .lines
                    .get(cursor.line)
                    .map(String::as_str)
                    .unwrap_or("");
                let col = if cursor.column > 0 {
                    previous_grapheme_column(line_text, cursor.column)
                        .min(last_cluster_col(text, cursor.line))
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
                self.clear_command_state();
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
            self.clear_command_state();
            return vec![cmd];
        }

        if mods.command() {
            if let Key::Character(c) = key {
                if c.as_str() == "r" {
                    self.clear_command_state();
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

        if self.pending.surround.is_some() {
            return self.resolve_surround(c, text);
        }

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

            // Surround: `ys{motion}{char}`, `ds{char}`, `cs{from}{to}`.
            if c == 's' {
                self.pending.operator = None;
                self.pending.operator_count = None;
                self.pending.count = None;
                self.pending.surround = Some(match op {
                    Operator::Yank => SurroundPhase::AddAwaitMotion,
                    Operator::Delete => SurroundPhase::DeleteAwaitDelim,
                    Operator::Change => SurroundPhase::ChangeAwaitFrom,
                });
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
        if matches!(c, 'g' | 'f' | 't' | 'F' | 'T' | 'r' | 'z' | '>' | '<') {
            self.pending.partial = Some(c);
            return vec![VimCommand::Noop];
        }

        let count = self.motion_count().unwrap_or(1);
        self.clear_command_state();

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
            '>' => {
                self.exit_visual();
                let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                return vec![
                    VimCommand::IndentLines { first, last },
                    VimCommand::MoveTo(pos(first, 0)),
                ];
            }
            '<' => {
                self.exit_visual();
                let (first, last) = ordered_lines(anchor.line, text.cursor.line);
                return vec![
                    VimCommand::OutdentLines { first, last },
                    VimCommand::MoveTo(pos(first, 0)),
                ];
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
            self.clear_command_state();
            return match resolve_z_intent(c) {
                Some(intent) => vec![VimCommand::ScrollCursor(intent)],
                None => vec![VimCommand::Noop],
            };
        }
        match partial {
            'r' => {
                let count = self.motion_count().unwrap_or(1);
                self.clear_command_state();
                if c == '\n' || line_len(text, text.cursor.line) == 0 {
                    vec![VimCommand::Noop]
                } else {
                    vec![VimCommand::ReplaceChar { ch: c, count }]
                }
            }
            'g' => {
                self.clear_command_state();
                match c {
                    ';' => vec![VimCommand::JumpToLastEdit {
                        enter_insert: false,
                    }],
                    'i' => vec![VimCommand::JumpToLastEdit { enter_insert: true }],
                    _ => vec![VimCommand::Noop],
                }
            }
            '>' | '<' if c == partial => {
                let count = self.motion_count().unwrap_or(1);
                self.clear_command_state();
                let last = (text.cursor.line + count - 1).min(text.line_count().saturating_sub(1));
                if partial == '>' {
                    vec![VimCommand::IndentLines {
                        first: text.cursor.line,
                        last,
                    }]
                } else {
                    vec![VimCommand::OutdentLines {
                        first: text.cursor.line,
                        last,
                    }]
                }
            }
            'i' | 'a' => {
                let inner = partial == 'i';
                let count = self.motion_count();
                if let Some(op) = self.pending.operator.take() {
                    if let Some(range) = text_object(text, c, inner, count) {
                        self.clear_command_state();
                        // Paragraph text objects are linewise
                        if c == 'p' {
                            return self.line_operator(op, range.0.line, range.1.line);
                        }
                        return self.range_operator(op, range.0, range.1);
                    }
                }
                self.clear_command_state();
                vec![VimCommand::Noop]
            }
            _ => {
                self.clear_command_state();
                vec![VimCommand::Noop]
            }
        }
    }

    // -- Surround --------------------------------------------------------

    fn resolve_surround(&mut self, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        let phase = self.pending.surround.take();
        match phase {
            Some(SurroundPhase::AddAwaitMotion) => {
                if c == 'i' || c == 'a' {
                    self.pending.surround = Some(SurroundPhase::AddAwaitInner(c));
                    return vec![VimCommand::Noop];
                }
                if let Some(motion) = char_to_motion(c) {
                    let count = self.motion_count();
                    self.clear_preferred_column();
                    let target = compute_motion(&motion, text, count, None);
                    let (from, to) = surround_motion_endpoints(&motion, text.cursor, target, text);
                    self.pending.surround = Some(SurroundPhase::AddAwaitDelim { from, to });
                    return vec![VimCommand::Noop];
                }
                vec![VimCommand::Noop]
            }
            Some(SurroundPhase::AddAwaitInner(prefix)) => {
                let inner = prefix == 'i';
                if let Some((from, to)) = text_object(text, c, inner, self.motion_count()) {
                    self.pending.surround = Some(SurroundPhase::AddAwaitDelim { from, to });
                    return vec![VimCommand::Noop];
                }
                self.clear_pending();
                vec![VimCommand::Noop]
            }
            Some(SurroundPhase::AddAwaitDelim { from, to }) => {
                self.clear_command_state();
                let Some((open, close)) = surround_pair_for_char(c) else {
                    return vec![VimCommand::Noop];
                };
                vec![VimCommand::SurroundRange {
                    from,
                    to,
                    open,
                    close,
                }]
            }
            Some(SurroundPhase::DeleteAwaitDelim) => {
                self.clear_command_state();
                let Some((open, _)) = surround_pair_for_char(c) else {
                    return vec![VimCommand::Noop];
                };
                vec![VimCommand::DeleteSurround { open }]
            }
            Some(SurroundPhase::ChangeAwaitFrom) => {
                let Some((from_open, _)) = surround_pair_for_char(c) else {
                    self.clear_pending();
                    return vec![VimCommand::Noop];
                };
                self.pending.surround = Some(SurroundPhase::ChangeAwaitTo { from_open });
                vec![VimCommand::Noop]
            }
            Some(SurroundPhase::ChangeAwaitTo { from_open }) => {
                self.clear_command_state();
                let Some((to_open, _)) = surround_pair_for_char(c) else {
                    return vec![VimCommand::Noop];
                };
                vec![VimCommand::ChangeSurround { from_open, to_open }]
            }
            None => vec![VimCommand::Noop],
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
        self.clear_command_state();

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

fn reverse_find(motion: Motion) -> Motion {
    match motion {
        Motion::FindChar(c) => Motion::FindCharBack(c),
        Motion::FindCharBack(c) => Motion::FindChar(c),
        Motion::TillChar(c) => Motion::TillCharBack(c),
        Motion::TillCharBack(c) => Motion::TillChar(c),
        other => other,
    }
}

// -- Word motions ------------------------------------------------------------

fn word_forward(text: &TextSnapshot, mut line: usize, col: usize, big: bool) -> (usize, usize) {
    let mut cells = line_cells(text, line);
    if cells.is_empty() {
        if line + 1 < text.line_count() {
            return (line + 1, 0);
        }
        return (line, 0);
    }

    let containing = cell_containing_char(&cells, col);
    let start_class = vim_token_class(cells[containing].repr, big);
    // If the cursor is past EOL we start advancing from one-past-end so the
    // class-skip loop falls through to the next line; otherwise advance from
    // the containing cluster.
    let mut cell_ix = if col >= line_len(text, line) {
        cells.len()
    } else {
        containing
    };

    if start_class != TokenClass::Whitespace {
        while cell_ix < cells.len() && vim_token_class(cells[cell_ix].repr, big) == start_class {
            cell_ix += 1;
        }
    }

    loop {
        while cell_ix < cells.len()
            && vim_token_class(cells[cell_ix].repr, big) == TokenClass::Whitespace
        {
            cell_ix += 1;
        }
        if cell_ix < cells.len() {
            return (line, cells[cell_ix].char_start);
        }
        if line + 1 < text.line_count() {
            line += 1;
            cells = line_cells(text, line);
            cell_ix = 0;
            if cells.is_empty() {
                return (line, 0);
            }
        } else {
            return (line, last_cluster_col(text, line));
        }
    }
}

fn word_backward(text: &TextSnapshot, mut line: usize, col: usize, big: bool) -> (usize, usize) {
    let mut cells = line_cells(text, line);

    // Step left by one cluster, possibly crossing to the previous line. A
    // mid-cluster column rounds back to the cluster start (defensive: cursors
    // are normally grapheme-aligned).
    let mut cell_ix = if cells.is_empty() {
        None
    } else {
        cell_partition_by_char(&cells, col).checked_sub(1)
    };

    if cell_ix.is_none() {
        if line == 0 {
            return (0, 0);
        }
        line -= 1;
        cells = line_cells(text, line);
        if cells.is_empty() {
            return (line, 0);
        }
        cell_ix = Some(cells.len() - 1);
    }
    let mut cell_ix = cell_ix.unwrap();

    loop {
        if !cells.is_empty() {
            while cell_ix > 0 && vim_token_class(cells[cell_ix].repr, big) == TokenClass::Whitespace
            {
                cell_ix -= 1;
            }
            if vim_token_class(cells[cell_ix].repr, big) != TokenClass::Whitespace {
                break;
            }
        }
        if line == 0 {
            return (0, 0);
        }
        line -= 1;
        cells = line_cells(text, line);
        if cells.is_empty() {
            return (line, 0);
        }
        cell_ix = cells.len() - 1;
    }

    let word_class = vim_token_class(cells[cell_ix].repr, big);
    while cell_ix > 0 && vim_token_class(cells[cell_ix - 1].repr, big) == word_class {
        cell_ix -= 1;
    }

    (line, cells[cell_ix].char_start)
}

fn word_end(text: &TextSnapshot, mut line: usize, col: usize, big: bool) -> (usize, usize) {
    let mut cells = line_cells(text, line);

    // Step right by one cluster, possibly crossing to the next line. The next
    // cell is the one just past the cluster currently containing the cursor.
    let mut cell_ix = if cells.is_empty() {
        if line + 1 < text.line_count() {
            line += 1;
            cells = line_cells(text, line);
            0
        } else {
            return (line, 0);
        }
    } else {
        let containing = cell_containing_char(&cells, col);
        if containing + 1 < cells.len() {
            containing + 1
        } else if line + 1 < text.line_count() {
            line += 1;
            cells = line_cells(text, line);
            0
        } else {
            return (line, last_cluster_col(text, line));
        }
    };

    loop {
        if !cells.is_empty() {
            while cell_ix < cells.len()
                && vim_token_class(cells[cell_ix].repr, big) == TokenClass::Whitespace
            {
                cell_ix += 1;
            }
            if cell_ix < cells.len() {
                break;
            }
        }
        if line + 1 < text.line_count() {
            line += 1;
            cells = line_cells(text, line);
            cell_ix = 0;
        } else {
            return (line, last_cluster_col(text, line));
        }
    }

    let word_class = vim_token_class(cells[cell_ix].repr, big);
    while cell_ix + 1 < cells.len() && vim_token_class(cells[cell_ix + 1].repr, big) == word_class {
        cell_ix += 1;
    }

    // The cursor sits on the cluster, so the "end of word" target is the
    // cluster's start column — never an interior char column.
    (line, cells[cell_ix].char_start)
}

// -- Text objects ------------------------------------------------------------

fn surround_motion_endpoints(
    motion: &Motion,
    cursor: Position,
    target: Position,
    text: &TextSnapshot,
) -> (Position, Position) {
    if cursor == target {
        return (cursor, cursor);
    }
    let (from, mut to) = ordered(cursor, target);
    let backward = pos_lt(&target, &cursor);
    if (!motion_is_inclusive(motion, None) || backward) && to != from {
        if to.column > 0 {
            to.column -= 1;
        } else if to.line > from.line {
            to.line -= 1;
            to.column = line_len(text, to.line).saturating_sub(1);
        }
    }
    (from, to)
}

pub fn surround_pair_for_char(c: char) -> Option<(char, char)> {
    Some(match c {
        '(' | ')' | 'b' => ('(', ')'),
        '{' | '}' | 'B' => ('{', '}'),
        '[' | ']' => ('[', ']'),
        '<' | '>' => ('<', '>'),
        '"' | '\'' | '`' => (c, c),
        _ => return None,
    })
}

pub fn find_surround_pair(
    text: &TextSnapshot,
    open: char,
    close: char,
) -> Option<(Position, Position)> {
    if open == close {
        quote_object(text, open, false)
    } else {
        pair_object(text, open, close, false)
    }
}

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
    let cells = line_cells(text, line);
    if cells.is_empty() {
        return None;
    }
    let col = cursor.column.min(line_len(text, line).saturating_sub(1));
    let cell_ix = cell_containing_char(&cells, col);
    let cur_class = vim_token_class(cells[cell_ix].repr, big);

    let mut start = cell_ix;
    while start > 0 && vim_token_class(cells[start - 1].repr, big) == cur_class {
        start -= 1;
    }

    let mut end = cell_ix;
    while end + 1 < cells.len() && vim_token_class(cells[end + 1].repr, big) == cur_class {
        end += 1;
    }

    if inner {
        return Some((
            pos(line, cells[start].char_start),
            pos(line, cells[end].char_start),
        ));
    }

    if cur_class == TokenClass::Whitespace {
        if let Some((_, next_end)) = next_non_space_range(&cells, end + 1, big) {
            return Some((
                pos(line, cells[start].char_start),
                pos(line, cells[next_end].char_start),
            ));
        }
        if let Some((prev_start, _)) = prev_non_space_range(&cells, start, big) {
            return Some((
                pos(line, cells[prev_start].char_start),
                pos(line, cells[end].char_start),
            ));
        }
        return Some((
            pos(line, cells[start].char_start),
            pos(line, cells[end].char_start),
        ));
    }

    if end + 1 < cells.len() && vim_token_class(cells[end + 1].repr, big) == TokenClass::Whitespace
    {
        while end + 1 < cells.len()
            && vim_token_class(cells[end + 1].repr, big) == TokenClass::Whitespace
        {
            end += 1;
        }
    } else if start > 0 && vim_token_class(cells[start - 1].repr, big) == TokenClass::Whitespace {
        while start > 0 && vim_token_class(cells[start - 1].repr, big) == TokenClass::Whitespace {
            start -= 1;
        }
    }

    Some((
        pos(line, cells[start].char_start),
        pos(line, cells[end].char_start),
    ))
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

fn next_non_space_range(
    cells: &[GraphemeCell],
    mut start: usize,
    big: bool,
) -> Option<(usize, usize)> {
    while start < cells.len() && vim_token_class(cells[start].repr, big) == TokenClass::Whitespace {
        start += 1;
    }
    if start >= cells.len() {
        return None;
    }
    let class = vim_token_class(cells[start].repr, big);
    let mut end = start;
    while end + 1 < cells.len() && vim_token_class(cells[end + 1].repr, big) == class {
        end += 1;
    }
    Some((start, end))
}

fn prev_non_space_range(cells: &[GraphemeCell], start: usize, big: bool) -> Option<(usize, usize)> {
    if start == 0 {
        return None;
    }
    let mut end = start - 1;
    loop {
        if vim_token_class(cells[end].repr, big) != TokenClass::Whitespace {
            break;
        }
        if end == 0 {
            return None;
        }
        end -= 1;
    }

    let class = vim_token_class(cells[end].repr, big);
    let mut range_start = end;
    while range_start > 0 && vim_token_class(cells[range_start - 1].repr, big) == class {
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

// Used by bracket / quote / paragraph helpers, which target ASCII chars and
// don't need grapheme awareness (every match is a single-cluster ASCII char).
// Word/text-object motion uses `line_cells` instead.
fn line_chars(text: &TextSnapshot, line: usize) -> Vec<char> {
    text.lines
        .get(line)
        .map_or(Vec::new(), |l| l.chars().collect())
}

fn line_cells(text: &TextSnapshot, line: usize) -> Vec<GraphemeCell> {
    text.lines.get(line).map_or(Vec::new(), |l| cells_of_str(l))
}

// Char column of the start of the last grapheme cluster on `line`, or 0 for
// an empty line. The canonical "EOL clamp" for normal-mode cursors — using
// `line_len - 1` would land mid-cluster on multi-char clusters.
fn last_cluster_col(text: &TextSnapshot, line: usize) -> usize {
    text.lines.get(line).map_or(0, |l| last_grapheme_column(l))
}

fn word_under_cursor(text: &TextSnapshot) -> Option<String> {
    let line = text.lines.get(text.cursor.line)?;
    let cells = cells_of_str(line);
    if cells.is_empty() {
        return None;
    }
    let col = text
        .cursor
        .column
        .min(line_len(text, text.cursor.line).saturating_sub(1));
    let cell_ix = cell_containing_char(&cells, col);
    if !is_identifier_char(cells[cell_ix].repr) {
        return None;
    }
    let mut start = cell_ix;
    while start > 0 && is_identifier_char(cells[start - 1].repr) {
        start -= 1;
    }
    let mut end = cell_ix;
    while end + 1 < cells.len() && is_identifier_char(cells[end + 1].repr) {
        end += 1;
    }
    let start_byte = cells[start].byte_start;
    let end_byte = cells.get(end + 1).map_or(line.len(), |c| c.byte_start);
    Some(line[start_byte..end_byte].to_string())
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
