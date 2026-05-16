//! Vim mode state machine.
//!
//! Pure keystroke to command translation. The caller executes commands against
//! whatever editor surface owns the document state.

use crate::selection::{
    cell_containing_char, cell_partition_by_char, cells_of_str, display_line_char_len, is_identifier_char,
    last_grapheme_column, next_grapheme_column, previous_grapheme_column, vim_token_class, GraphemeCell, Position,
    TokenClass,
};
use crate::RevealIntent;
use ropey::{Rope, RopeSlice};

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
    visual_head: Option<Position>,
    pending: Pending,
    last_find: Option<Motion>, // for ; and ,
    last_search_backward: bool,
    preferred_column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VimCommand {
    MoveTo(Position),
    Select(VisualState),
    Delete(RangeTarget),
    Change(RangeTarget),
    Yank(RangeTarget, bool),
    SetRegister(Register),
    Shift(RangeTarget, bool, bool),
    PasteSelection(RangeTarget, bool),
    TransformCase(RangeTarget, bool),
    EnterInsert,
    Paste(bool),
    OpenLine(bool),
    JoinLines(usize),
    ReplaceChar(char, usize),
    Undo,
    Redo,
    OpenFind(bool),
    Find(bool),
    SearchWordUnderCursor(String, bool),
    Page(bool, bool),
    MoveToScreen(ScreenRow),
    ScrollCursor(RevealIntent),
    JumpToLastEdit(bool),
}

pub struct VimText<'a> {
    pub buffer: &'a Rope,
    pub cached_lines: Option<&'a [String]>,
    pub cursor: Position,
}

impl VimText<'_> {
    fn line_count(&self) -> usize {
        self.cached_lines
            .map_or(self.buffer.len_lines().max(1), <[String]>::len)
    }

    fn cached_line(&self, line: usize) -> Option<&str> {
        self.cached_lines.and_then(|lines| lines.get(line)).map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Register {
    Empty,
    Char(String),
    Line(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualState {
    pub anchor: Position,
    pub head: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenRow {
    Top,
    Middle,
    Bottom,
}

// -- Supporting types --------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeTarget {
    Range { from: Position, to: Position },
    Lines { first: usize, last: usize },
}

impl RangeTarget {
    fn charwise(anchor: Position, head: Position) -> Self {
        Self::Range { from: anchor, to: head }
    }

    fn linewise(first: usize, last: usize) -> Self {
        let (first, last) = ordered_lines(first, last);
        Self::Lines { first, last }
    }

    fn visual(anchor: Position, head: Position, linewise: bool) -> Self {
        if linewise {
            Self::linewise(anchor.line, head.line)
        } else {
            Self::charwise(anchor, head)
        }
    }

    fn forward_chars(text: &VimText<'_>, count: usize) -> Option<Self> {
        let ll = line_len(text, text.cursor.line);
        (ll > 0).then(|| {
            let end = (text.cursor.column + count - 1).min(ll.saturating_sub(1));
            Self::charwise(text.cursor, pos(text.cursor.line, end))
        })
    }

    fn backward_chars(text: &VimText<'_>, count: usize) -> Option<Self> {
        (text.cursor.column > 0).then(|| {
            let start = text.cursor.column.saturating_sub(count);
            Self::charwise(
                pos(text.cursor.line, start),
                pos(text.cursor.line, text.cursor.column - 1),
            )
        })
    }

    fn line_tail(text: &VimText<'_>) -> Option<Self> {
        let ll = line_len(text, text.cursor.line);
        (ll > 0 && text.cursor.column < ll).then(|| Self::charwise(text.cursor, pos(text.cursor.line, ll - 1)))
    }

    fn operator_motion(op: Operator, motion: &Motion, text: &VimText<'_>, count: Option<usize>) -> Option<Self> {
        let semantics = motion_semantics(op, motion, count);

        if semantics.linewise {
            let target = compute_motion(motion, text, count, None);
            return Some(Self::linewise(text.cursor.line, target.line));
        }

        let target = compute_motion(motion, text, count, None);

        if target == text.cursor && semantics.noop_on_same {
            return None;
        }

        let backward = target < text.cursor;

        let mut eol_clamped = false;
        let target = if matches!(motion, Motion::Word(WordMotion::Forward, _)) {
            if op == Operator::Delete && target.line > text.cursor.line {
                eol_clamped = true;
                let ll = line_len(text, text.cursor.line);
                pos(text.cursor.line, ll.saturating_sub(1))
            } else if target.line == text.line_count().saturating_sub(1)
                && line_len(text, target.line) > 0
                && target.column == line_len(text, target.line).saturating_sub(1)
            {
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

        if (!semantics.inclusive || backward) && !eol_clamped {
            if to.column > 0 {
                to.column -= 1;
            } else if to.line > from.line {
                to.line -= 1;
                to.column = line_len(text, to.line).saturating_sub(1);
            }
            if to < from {
                return None;
            }
        }

        Some(Self::charwise(from, to))
    }

    fn ordered(self) -> Self {
        match self {
            Self::Range { from, to } => {
                let (from, to) = ordered(from, to);
                Self::Range { from, to }
            }
            Self::Lines { first, last } => Self::linewise(first, last),
        }
    }

    fn selection(self, text: &VimText<'_>) -> VisualState {
        match self {
            Self::Range { from: anchor, to: head } => VisualState { anchor, head },
            Self::Lines { first, last } => {
                let last_col = line_len(text, last).saturating_sub(1);
                VisualState {
                    anchor: pos(first, 0),
                    head: pos(last, last_col),
                }
            }
        }
    }

    fn operator(self, op: Operator, move_after_yank: bool) -> Vec<VimCommand> {
        operator_commands(op, self.ordered(), move_after_yank)
    }

    fn paste(self, preserve_register: bool) -> Vec<VimCommand> {
        vec![VimCommand::PasteSelection(self.ordered(), preserve_register)]
    }

    fn shift(self, indent: bool, move_after: bool) -> Vec<VimCommand> {
        vec![VimCommand::Shift(self.ordered(), indent, move_after)]
    }

    fn transform_case(self, uppercase: bool) -> Vec<VimCommand> {
        vec![VimCommand::TransformCase(self.ordered(), uppercase)]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Pending {
    #[default]
    Empty,
    Count(usize),
    Prefix {
        count: Option<usize>,
        prefix: Prefix,
    },
    Operator(OperatorPending),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OperatorPending {
    op: Operator,
    op_count: Option<usize>,
    count: Option<usize>,
    prefix: Option<Prefix>,
}

impl Pending {
    fn display(self) -> String {
        let mut s = String::new();
        match self {
            Pending::Empty => {}
            Pending::Count(count) => s.push_str(&count.to_string()),
            Pending::Prefix { count, prefix } => {
                push_count(&mut s, count);
                s.push(prefix.as_char());
            }
            Pending::Operator(pending) => {
                push_count(&mut s, pending.op_count);
                s.push(pending.op.as_char());
                push_count(&mut s, pending.count);
                if let Some(prefix) = pending.prefix {
                    s.push(prefix.as_char());
                }
            }
        }
        s
    }

    fn has_count(self) -> bool {
        matches!(
            self,
            Pending::Count(_) | Pending::Operator(OperatorPending { count: Some(_), .. })
        )
    }

    fn add_digit(&mut self, digit: usize) {
        match self {
            Pending::Empty => *self = Pending::Count(digit),
            Pending::Count(count) => *count = *count * 10 + digit,
            Pending::Operator(pending) if pending.prefix.is_none() => {
                pending.count = Some(pending.count.unwrap_or(0) * 10 + digit);
            }
            _ => {}
        }
    }

    fn start_normal_prefix(&mut self, prefix: Prefix) {
        let count = self.take_count();
        *self = Pending::Prefix { count, prefix };
    }

    fn start_operator(&mut self, op: Operator) {
        let op_count = self.take_count();
        *self = Pending::Operator(OperatorPending {
            op,
            op_count,
            count: None,
            prefix: None,
        });
    }

    fn start_operator_prefix(&mut self, prefix: Prefix) {
        if let Pending::Operator(pending) = self {
            pending.prefix = Some(prefix);
        }
    }

    fn take_prefix(&mut self) -> Option<Prefix> {
        match self {
            Pending::Prefix { count, prefix } => {
                let count = *count;
                let prefix = *prefix;
                *self = count.map(Pending::Count).unwrap_or_default();
                Some(prefix)
            }
            Pending::Operator(pending) => pending.prefix.take(),
            _ => None,
        }
    }

    fn operator(self) -> Option<Operator> {
        match self {
            Pending::Operator(OperatorPending { op, prefix: None, .. }) => Some(op),
            _ => None,
        }
    }

    fn take_count(&mut self) -> Option<usize> {
        match *self {
            Pending::Count(count) => {
                *self = Pending::Empty;
                Some(count)
            }
            _ => None,
        }
    }

    fn take_operator_count(&mut self) -> Option<usize> {
        match *self {
            Pending::Operator(pending) if pending.prefix.is_none() => {
                *self = Pending::Empty;
                combine_counts(pending.op_count, pending.count)
            }
            _ => None,
        }
    }

    fn take_operator(&mut self) -> Option<(Operator, Option<usize>)> {
        match *self {
            Pending::Operator(pending) if pending.prefix.is_none() => {
                *self = Pending::Empty;
                Some((pending.op, combine_counts(pending.op_count, pending.count)))
            }
            _ => None,
        }
    }
}

fn push_count(s: &mut String, count: Option<usize>) {
    if let Some(count) = count {
        s.push_str(&count.to_string());
    }
}

fn combine_counts(operator_count: Option<usize>, motion_count: Option<usize>) -> Option<usize> {
    operator_count.map_or(motion_count, |count| Some(count * motion_count.unwrap_or(1)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Delete,
    Change,
    Yank,
}

impl Operator {
    fn from_char(c: char) -> Option<Self> {
        match c {
            'd' => Some(Self::Delete),
            'c' => Some(Self::Change),
            'y' => Some(Self::Yank),
            _ => None,
        }
    }

    fn as_char(self) -> char {
        match self {
            Operator::Delete => 'd',
            Operator::Change => 'c',
            Operator::Yank => 'y',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefix {
    Go,
    FindChar,
    TillChar,
    FindCharBack,
    TillCharBack,
    Replace,
    Scroll,
    Indent,
    Outdent,
    TextObjectInner,
    TextObjectAround,
}

impl Prefix {
    fn start(c: char, context: PrefixContext) -> Option<Self> {
        Some(match c {
            'g' => Self::Go,
            'f' => Self::FindChar,
            't' => Self::TillChar,
            'F' => Self::FindCharBack,
            'T' => Self::TillCharBack,
            'r' if context == PrefixContext::Normal => Self::Replace,
            'z' if context != PrefixContext::Operator => Self::Scroll,
            '>' if context == PrefixContext::Normal => Self::Indent,
            '<' if context == PrefixContext::Normal => Self::Outdent,
            'i' if context != PrefixContext::Normal => Self::TextObjectInner,
            'a' if context != PrefixContext::Normal => Self::TextObjectAround,
            _ => return None,
        })
    }

    fn as_char(self) -> char {
        match self {
            Prefix::Go => 'g',
            Prefix::FindChar => 'f',
            Prefix::TillChar => 't',
            Prefix::FindCharBack => 'F',
            Prefix::TillCharBack => 'T',
            Prefix::Replace => 'r',
            Prefix::Scroll => 'z',
            Prefix::Indent => '>',
            Prefix::Outdent => '<',
            Prefix::TextObjectInner => 'i',
            Prefix::TextObjectAround => 'a',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrefixContext {
    Normal,
    Operator,
    Visual,
}

#[derive(Clone, Copy)]
enum Motion {
    Left,
    Right,
    Down,
    Up,
    Word(WordMotion, bool),
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
enum WordMotion {
    Forward,
    Backward,
    End,
}

enum KeyAction {
    Command(VimCommand),
    Motion(Motion),
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
            visual_head: None,
            pending: Pending::default(),
            last_find: None,
            last_search_backward: false,
            preferred_column: None,
        }
    }

    pub fn handle_key(&mut self, key: &Key, mods: Modifiers, text: &VimText<'_>) -> Vec<VimCommand> {
        match self.mode {
            Mode::Normal => self.handle_normal(key, mods, text),
            Mode::Insert => vec![], // caller handles text input
            Mode::Visual | Mode::VisualLine => self.handle_visual(key, mods, text),
        }
    }

    pub fn snapshot_cursor(&self, editor_cursor: Position) -> Position {
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.visual_head.unwrap_or(editor_cursor)
        } else {
            editor_cursor
        }
    }

    pub fn visual_state(&self) -> Option<VisualState> {
        matches!(self.mode, Mode::Visual | Mode::VisualLine).then_some(VisualState {
            anchor: self.visual_anchor?,
            head: self.visual_head?,
        })
    }

    pub fn pending_display(&self) -> String {
        self.pending.display()
    }
    fn clear_pending(&mut self) {
        self.pending = Pending::default();
    }
    fn clear_preferred_column(&mut self) {
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
            self.visual_head = None;
        }
    }

    fn exit_visual(&mut self) {
        self.mode = Mode::Normal;
        self.visual_anchor = None;
        self.visual_head = None;
        self.clear_command_state();
    }

    fn exit_visual_with(&mut self, commands: Vec<VimCommand>) -> Vec<VimCommand> {
        self.exit_visual();
        commands
    }

    fn repeat_find(&self, c: char) -> Option<Motion> {
        if c != ';' && c != ',' {
            return None;
        }
        let last = self.last_find?;
        Some(if c == ';' { last } else { reverse_find(last) })
    }

    fn char_motion(&self, c: char) -> Option<Motion> {
        char_to_motion(c).or_else(|| self.repeat_find(c))
    }

    fn apply_key_action(&mut self, action: KeyAction, text: &VimText<'_>, clear_commands: bool) -> Vec<VimCommand> {
        match action {
            KeyAction::Command(cmd) => {
                if clear_commands {
                    self.clear_command_state();
                }
                vec![cmd]
            }
            KeyAction::Motion(motion) => self.apply_motion(motion, text),
        }
    }

    fn search_command(&mut self, c: char) -> Option<VimCommand> {
        match c {
            '/' | '?' => {
                self.last_search_backward = c == '?';
                Some(VimCommand::OpenFind(c == '?'))
            }
            'n' => Some(VimCommand::Find(!self.last_search_backward)),
            'N' => Some(VimCommand::Find(self.last_search_backward)),
            _ => None,
        }
    }

    fn resolve_prefix_motion(&mut self, prefix: Prefix, c: char) -> Option<Motion> {
        let motion = match prefix {
            Prefix::FindChar => Motion::FindChar(c),
            Prefix::TillChar => Motion::TillChar(c),
            Prefix::FindCharBack => Motion::FindCharBack(c),
            Prefix::TillCharBack => Motion::TillCharBack(c),
            Prefix::Go if c == 'g' => return Some(Motion::DocumentStart),
            _ => return None,
        };
        self.last_find = Some(motion);
        Some(motion)
    }

    pub fn enter_normal_from_escape(&mut self, cursor: Position, text: &VimText<'_>) -> Vec<VimCommand> {
        match self.mode {
            Mode::Insert => {
                self.mode = Mode::Normal;
                self.clear_command_state();
                // vim: cursor moves left by 1 when leaving Insert (unless at col 0).
                // Step by grapheme cluster, then clamp to the start of the last
                // cluster so we never land mid-cluster on a multi-char grapheme.
                let col = if cursor.column > 0 {
                    with_line_str(text, cursor.line, |line_text| {
                        previous_grapheme_column(line_text, cursor.column).min(last_grapheme_column(line_text))
                    })
                } else {
                    0
                };
                if col != cursor.column {
                    vec![VimCommand::MoveTo(pos(cursor.line, col))]
                } else {
                    vec![]
                }
            }
            Mode::Visual | Mode::VisualLine => {
                self.exit_visual();
                vec![VimCommand::MoveTo(cursor)]
            }
            Mode::Normal => {
                self.clear_command_state();
                vec![]
            }
        }
    }

    // -- Normal mode -----------------------------------------------------

    fn handle_normal(&mut self, key: &Key, mods: Modifiers, text: &VimText<'_>) -> Vec<VimCommand> {
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
            return vec![];
        }

        if let Some(action) = named_key_action(key) {
            return self.apply_key_action(action, text, true);
        }

        let Some(c) = key_char(key) else {
            return vec![];
        };

        if let Some(prefix) = self.pending.take_prefix() {
            return self.resolve_partial(prefix, c, text);
        }

        if c == '0' && !self.pending.has_count() {
            return self.apply_motion(Motion::LineStart, text);
        }
        if c.is_ascii_digit() {
            let digit = c.to_digit(10).unwrap() as usize;
            self.pending.add_digit(digit);
            return vec![];
        }

        if let Some(op) = self.pending.operator() {
            if Some(op) == Operator::from_char(c) {
                let count = self.pending.take_operator_count().unwrap_or(1);
                let last = (text.cursor.line + count - 1).min(text.line_count().saturating_sub(1));
                return RangeTarget::linewise(text.cursor.line, last).operator(op, false);
            }

            if let Some(motion) = self.char_motion(c) {
                return self.apply_motion(motion, text);
            }

            if let Some(prefix) = Prefix::start(c, PrefixContext::Operator) {
                self.pending.start_operator_prefix(prefix);
                return vec![];
            }

            self.clear_pending();
            return vec![];
        }

        if let Some(motion) = self.char_motion(c) {
            return self.apply_motion(motion, text);
        }

        if let Some(op) = Operator::from_char(c) {
            self.pending.start_operator(op);
            return vec![];
        }

        if let Some(prefix) = Prefix::start(c, PrefixContext::Normal) {
            self.pending.start_normal_prefix(prefix);
            return vec![];
        }

        let count = self.pending.take_count().unwrap_or(1);
        self.clear_command_state();
        if let Some(cmd) = self.search_command(c) {
            return vec![cmd];
        }

        match c {
            'H' => vec![VimCommand::MoveToScreen(ScreenRow::Top)],
            'M' => vec![VimCommand::MoveToScreen(ScreenRow::Middle)],
            'L' => vec![VimCommand::MoveToScreen(ScreenRow::Bottom)],
            'i' => vec![VimCommand::EnterInsert],
            'a' => insert_at(pos(
                text.cursor.line,
                (text.cursor.column + 1).min(line_len(text, text.cursor.line)),
            )),
            'I' => insert_at(pos(text.cursor.line, first_non_blank(text, text.cursor.line))),
            'A' => insert_at(pos(text.cursor.line, line_len(text, text.cursor.line))),
            'o' => vec![VimCommand::OpenLine(false)],
            'O' => vec![VimCommand::OpenLine(true)],
            'x' => operator_or(RangeTarget::forward_chars(text, count), Operator::Delete, None),
            'X' => operator_or(RangeTarget::backward_chars(text, count), Operator::Delete, None),
            's' => operator_or(
                RangeTarget::forward_chars(text, count),
                Operator::Change,
                Some(VimCommand::EnterInsert),
            ),
            'D' => operator_or(RangeTarget::line_tail(text), Operator::Delete, None),
            'C' => operator_or(
                RangeTarget::line_tail(text),
                Operator::Change,
                Some(VimCommand::EnterInsert),
            ),
            'J' => {
                // vim: J = join 2 lines (1 op), 3J = join 3 lines (2 ops)
                let joins = if count <= 1 { 1 } else { count - 1 };
                vec![VimCommand::JoinLines(joins)]
            }
            'S' => {
                let last = (text.cursor.line + count - 1).min(text.line_count().saturating_sub(1));
                RangeTarget::linewise(text.cursor.line, last).operator(Operator::Change, false)
            }
            'p' => vec![VimCommand::Paste(false)],
            'P' => vec![VimCommand::Paste(true)],
            'u' => vec![VimCommand::Undo],
            'v' => self.enter_visual(Mode::Visual, text),
            'V' => self.enter_visual(Mode::VisualLine, text),
            '*' | '#' => {
                if let Some(word) = word_under_cursor(text) {
                    self.last_search_backward = c == '#';
                    vec![VimCommand::SearchWordUnderCursor(word, c == '*')]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    // -- Visual mode -----------------------------------------------------

    fn handle_visual(&mut self, key: &Key, mods: Modifiers, text: &VimText<'_>) -> Vec<VimCommand> {
        if let Some(cmd) = ctrl_page_command(key, mods) {
            return vec![cmd];
        }

        if mods.command() {
            if let Key::Character(c) = key {
                if c.as_str() == "r" {
                    return self.exit_visual_with(vec![VimCommand::Redo]);
                }
            }
            return vec![];
        }

        if let Some(action) = named_key_action(key) {
            return self.apply_key_action(action, text, false);
        }

        let Some(c) = key_char(key) else {
            return vec![];
        };

        if let Some(prefix) = self.pending.take_prefix() {
            return self.resolve_partial(prefix, c, text);
        }

        if c.is_ascii_digit() && (c != '0' || self.pending.has_count()) {
            let digit = c.to_digit(10).unwrap() as usize;
            self.pending.add_digit(digit);
            return vec![];
        }

        let anchor = self.visual_anchor.unwrap_or(text.cursor);
        let head = self.visual_head.unwrap_or(text.cursor);
        let target = RangeTarget::visual(anchor, head, self.mode == Mode::VisualLine);

        match c {
            'd' | 'x' => return self.exit_visual_with(target.operator(Operator::Delete, true)),
            'c' | 's' => return self.exit_visual_with(target.operator(Operator::Change, true)),
            'y' => return self.exit_visual_with(target.operator(Operator::Yank, true)),
            'p' | 'P' => return self.exit_visual_with(target.paste(c == 'P')),
            '>' => return self.exit_visual_with(target.shift(true, true)),
            '<' => return self.exit_visual_with(target.shift(false, true)),
            'v' => return self.toggle_visual(Mode::Visual, text),
            'V' => return self.toggle_visual(Mode::VisualLine, text),
            'u' | 'U' => return self.exit_visual_with(target.transform_case(c == 'U')),
            _ => {}
        }

        if let Some(prefix) = Prefix::start(c, PrefixContext::Visual) {
            self.pending.start_normal_prefix(prefix);
            return vec![];
        }

        match c {
            'H' => return vec![VimCommand::MoveToScreen(ScreenRow::Top)],
            'M' => return vec![VimCommand::MoveToScreen(ScreenRow::Middle)],
            'L' => return vec![VimCommand::MoveToScreen(ScreenRow::Bottom)],
            _ => {}
        }

        if c == '0' && !self.pending.has_count() {
            return self.apply_motion(Motion::LineStart, text);
        }
        if let Some(motion) = self.char_motion(c) {
            return self.apply_motion(motion, text);
        }

        if let Some(cmd) = self.search_command(c) {
            self.clear_preferred_column();
            return vec![cmd];
        }

        vec![]
    }

    pub fn selection_command(&mut self, head: Position, text: &VimText<'_>) -> VimCommand {
        let anchor = self.visual_anchor.unwrap_or(text.cursor);
        self.visual_head = Some(head);
        VimCommand::Select(RangeTarget::visual(anchor, head, self.mode == Mode::VisualLine).selection(text))
    }

    fn enter_visual(&mut self, mode: Mode, text: &VimText<'_>) -> Vec<VimCommand> {
        self.mode = mode;
        self.visual_anchor = Some(text.cursor);
        self.visual_head = Some(text.cursor);
        vec![self.selection_command(text.cursor, text)]
    }

    fn toggle_visual(&mut self, mode: Mode, text: &VimText<'_>) -> Vec<VimCommand> {
        if self.mode == mode {
            self.exit_visual_with(vec![VimCommand::MoveTo(text.cursor)])
        } else {
            self.mode = mode;
            self.clear_preferred_column();
            vec![self.selection_command(text.cursor, text)]
        }
    }

    // -- Partial resolution ----------------------------------------------

    fn resolve_partial(&mut self, prefix: Prefix, c: char, text: &VimText<'_>) -> Vec<VimCommand> {
        if let Some(motion) = self.resolve_prefix_motion(prefix, c) {
            return self.apply_motion(motion, text);
        }
        if prefix == Prefix::Scroll {
            self.clear_command_state();
            return match resolve_z_intent(c) {
                Some(intent) => vec![VimCommand::ScrollCursor(intent)],
                None => vec![],
            };
        }
        match prefix {
            Prefix::Replace => {
                let count = self.pending.take_count().unwrap_or(1);
                self.clear_command_state();
                if c == '\n' || line_len(text, text.cursor.line) == 0 {
                    vec![]
                } else {
                    vec![VimCommand::ReplaceChar(c, count)]
                }
            }
            Prefix::Go => {
                self.clear_command_state();
                match c {
                    ';' => vec![VimCommand::JumpToLastEdit(false)],
                    'i' => vec![VimCommand::JumpToLastEdit(true)],
                    _ => vec![],
                }
            }
            Prefix::Indent | Prefix::Outdent if c == prefix.as_char() => {
                let count = self.pending.take_count().unwrap_or(1);
                self.clear_command_state();
                let last = (text.cursor.line + count - 1).min(text.line_count().saturating_sub(1));
                RangeTarget::linewise(text.cursor.line, last).shift(prefix == Prefix::Indent, false)
            }
            Prefix::TextObjectInner | Prefix::TextObjectAround => {
                let inner = prefix == Prefix::TextObjectInner;
                if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                    let count = self.pending.take_count();
                    if let Some((from, to)) = text_object(text, c, inner, count) {
                        self.clear_preferred_column();
                        let range = RangeTarget::charwise(from, to);
                        self.visual_anchor = Some(from);
                        self.visual_head = Some(to);
                        return vec![VimCommand::Select(range.selection(text))];
                    }
                    self.clear_command_state();
                    return vec![];
                }
                if let Some((op, count)) = self.pending.take_operator() {
                    if let Some(range) = text_object(text, c, inner, count) {
                        self.clear_command_state();
                        // Paragraph text objects are linewise
                        if c == 'p' {
                            return RangeTarget::linewise(range.0.line, range.1.line).operator(op, false);
                        }
                        return RangeTarget::charwise(range.0, range.1).operator(op, false);
                    }
                    if inner && op == Operator::Change {
                        if let Some(at) = empty_inner_text_object_position(text, c) {
                            self.clear_command_state();
                            return vec![
                                VimCommand::SetRegister(Register::Char(String::new())),
                                VimCommand::MoveTo(at),
                                VimCommand::EnterInsert,
                            ];
                        }
                    }
                }
                self.clear_command_state();
                vec![]
            }
            _ => {
                self.clear_command_state();
                vec![]
            }
        }
    }

    // -- Motion + operator helpers ---------------------------------------

    fn cursor_motion_target(&mut self, motion: &Motion, text: &VimText<'_>, count: Option<usize>) -> Position {
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

    fn apply_motion(&mut self, motion: Motion, text: &VimText<'_>) -> Vec<VimCommand> {
        if let Some((op, count)) = self.pending.take_operator() {
            self.operator_with_computed_motion(op, motion, text, count)
        } else {
            let count = self.pending.take_count();
            self.clear_pending();
            let target = self.cursor_motion_target(&motion, text, count);
            if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                vec![self.selection_command(target, text)]
            } else {
                vec![VimCommand::MoveTo(target)]
            }
        }
    }

    fn operator_with_computed_motion(
        &mut self,
        op: Operator,
        motion: Motion,
        text: &VimText<'_>,
        count: Option<usize>,
    ) -> Vec<VimCommand> {
        let motion = match motion {
            Motion::Word(WordMotion::Forward, big) if op == Operator::Change && cursor_on_non_space(text) => {
                Motion::Word(WordMotion::End, big)
            }
            _ => motion,
        };

        self.clear_command_state();

        RangeTarget::operator_motion(op, &motion, text, count)
            .map_or_else(Vec::new, |target| target.operator(op, false))
    }
}

fn operator_commands(op: Operator, target: RangeTarget, move_after_yank: bool) -> Vec<VimCommand> {
    vec![match op {
        Operator::Delete => VimCommand::Delete(target),
        Operator::Change => VimCommand::Change(target),
        Operator::Yank => VimCommand::Yank(target, move_after_yank),
    }]
}

fn insert_at(target: Position) -> Vec<VimCommand> {
    vec![VimCommand::MoveTo(target), VimCommand::EnterInsert]
}

fn operator_or(target: Option<RangeTarget>, op: Operator, fallback: Option<VimCommand>) -> Vec<VimCommand> {
    target.map_or_else(|| fallback.into_iter().collect(), |target| target.operator(op, false))
}

fn cursor_on_non_space(text: &VimText<'_>) -> bool {
    let chars = line_chars(text, text.cursor.line);
    let col = text.cursor.column.min(chars.len().saturating_sub(1));
    !chars.is_empty() && !chars[col].is_whitespace()
}

#[derive(Clone, Copy)]
struct MotionSemantics {
    linewise: bool,
    inclusive: bool,
    noop_on_same: bool,
}

fn motion_semantics(op: Operator, motion: &Motion, count: Option<usize>) -> MotionSemantics {
    MotionSemantics {
        linewise: matches!(
            motion,
            Motion::Down | Motion::Up | Motion::DocumentStart | Motion::DocumentEnd
        ) || matches!(motion, Motion::LineEnd) && count.unwrap_or(1) > 1 && op == Operator::Delete
            || matches!(motion, Motion::Percent) && count.is_some(),
        inclusive: matches!(
            motion,
            Motion::Word(WordMotion::End, _)
                | Motion::LineEnd
                | Motion::FindChar(_)
                | Motion::FindCharBack(_)
                | Motion::TillChar(_)
                | Motion::TillCharBack(_)
        ) || matches!(motion, Motion::Percent) && count.is_none(),
        noop_on_same: matches!(
            motion,
            Motion::Left
                | Motion::Word(WordMotion::Backward, _)
                | Motion::LineStart
                | Motion::FirstNonBlank
                | Motion::FindChar(_)
                | Motion::TillChar(_)
                | Motion::FindCharBack(_)
                | Motion::TillCharBack(_)
                | Motion::Percent
        ),
    }
}

// -- Motion computation ------------------------------------------------------

fn compute_motion(
    motion: &Motion,
    text: &VimText<'_>,
    count: Option<usize>,
    preferred_column: Option<usize>,
) -> Position {
    let n = count.unwrap_or(1);
    match motion {
        Motion::Left => {
            let mut col = text.cursor.column;
            with_line_str(text, text.cursor.line, |line_text| {
                for _ in 0..n {
                    let next = previous_grapheme_column(line_text, col);
                    if next == col {
                        break;
                    }
                    col = next;
                }
            });
            pos(text.cursor.line, col)
        }
        Motion::Right => {
            let mut col = text.cursor.column;
            with_line_str(text, text.cursor.line, |line_text| {
                let last = last_grapheme_column(line_text);
                col = col.min(last);
                for _ in 0..n {
                    let next = next_grapheme_column(line_text, col);
                    if next == col || next > last {
                        break;
                    }
                    col = next;
                }
            });
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
        Motion::Word(kind, big) => word_motion(text, *kind, *big, n),
        Motion::LineStart => pos(text.cursor.line, 0),
        Motion::LineEnd => {
            let line = (text.cursor.line + n.saturating_sub(1)).min(text.line_count().saturating_sub(1));
            let ll = line_len(text, line);
            pos(line, ll.saturating_sub(1))
        }
        Motion::FirstNonBlank => pos(text.cursor.line, first_non_blank(text, text.cursor.line)),
        Motion::DocumentStart | Motion::DocumentEnd => {
            let last = text.line_count().saturating_sub(1);
            let default = if matches!(motion, Motion::DocumentStart) {
                0
            } else {
                last
            };
            let line = count.map_or(default, |n| n.saturating_sub(1).min(last));
            pos(line, text.cursor.column.min(line_len(text, line).saturating_sub(1)))
        }
        Motion::FindChar(ch) => find_char(text, *ch, n, true, false),
        Motion::TillChar(ch) => find_char(text, *ch, n, true, true),
        Motion::FindCharBack(ch) => find_char(text, *ch, n, false, false),
        Motion::TillCharBack(ch) => find_char(text, *ch, n, false, true),
        Motion::Percent => match count {
            Some(n) => {
                let total = text.line_count().max(1);
                let pct = n.clamp(1, 100);
                let line = ((pct * total).saturating_add(99) / 100).saturating_sub(1);
                pos(line, text.cursor.column.min(line_len(text, line).saturating_sub(1)))
            }
            None => match_bracket(text).unwrap_or(text.cursor),
        },
    }
}

fn find_char(text: &VimText<'_>, ch: char, n: usize, forward: bool, till: bool) -> Position {
    let chars = line_chars(text, text.cursor.line);
    let col = text.cursor.column;
    let mut found = 0;
    let positions: Box<dyn Iterator<Item = usize>> = if forward {
        Box::new(col.saturating_add(1)..chars.len())
    } else {
        Box::new((0..col.min(chars.len())).rev())
    };
    for i in positions {
        if chars[i] == ch {
            found += 1;
            if found == n {
                let target_col = if !till {
                    i
                } else if forward {
                    i.saturating_sub(1).max(col)
                } else {
                    (i + 1).min(col)
                };
                return pos(text.cursor.line, target_col);
            }
        }
    }
    text.cursor
}

fn key_char(key: &Key) -> Option<char> {
    match key {
        Key::Character(s) => s.as_str().chars().next(),
        _ => None,
    }
}

fn named_key_action(key: &Key) -> Option<KeyAction> {
    let Key::Named(named) = key else {
        return None;
    };
    Some(match named {
        NamedKey::ArrowLeft => KeyAction::Motion(Motion::Left),
        NamedKey::ArrowRight => KeyAction::Motion(Motion::Right),
        NamedKey::ArrowUp => KeyAction::Motion(Motion::Up),
        NamedKey::ArrowDown => KeyAction::Motion(Motion::Down),
        NamedKey::Home => KeyAction::Motion(Motion::LineStart),
        NamedKey::End => KeyAction::Motion(Motion::LineEnd),
        NamedKey::PageDown => KeyAction::Command(VimCommand::Page(false, true)),
        NamedKey::PageUp => KeyAction::Command(VimCommand::Page(false, false)),
        _ => return None,
    })
}

fn ctrl_page_command(key: &Key, mods: Modifiers) -> Option<VimCommand> {
    if !mods.control() {
        return None;
    }
    let Key::Character(c) = key else {
        return None;
    };
    match c.as_str() {
        "d" => Some(VimCommand::Page(true, true)),
        "u" => Some(VimCommand::Page(true, false)),
        "f" => Some(VimCommand::Page(false, true)),
        "b" => Some(VimCommand::Page(false, false)),
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
        'w' => Some(Motion::Word(WordMotion::Forward, false)),
        'b' => Some(Motion::Word(WordMotion::Backward, false)),
        'e' => Some(Motion::Word(WordMotion::End, false)),
        'W' => Some(Motion::Word(WordMotion::Forward, true)),
        'B' => Some(Motion::Word(WordMotion::Backward, true)),
        'E' => Some(Motion::Word(WordMotion::End, true)),
        '$' => Some(Motion::LineEnd),
        '^' => Some(Motion::FirstNonBlank),
        'G' => Some(Motion::DocumentEnd),
        '%' => Some(Motion::Percent),
        _ => None,
    }
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

#[derive(Clone, Copy)]
struct TokenRun {
    start: usize,
    end: usize,
    class: TokenClass,
}

struct TokenLine(usize, Vec<GraphemeCell>, bool);

impl TokenLine {
    fn run(&self, ix: usize) -> TokenRun {
        let class = vim_token_class(self.1[ix].repr, self.2);
        let token_class = |i: usize| vim_token_class(self.1[i].repr, self.2);
        let start = (0..ix).rev().find(|&i| token_class(i) != class).map_or(0, |i| i + 1);
        let end = (ix + 1..self.1.len())
            .find(|&i| token_class(i) != class)
            .map_or(self.1.len() - 1, |i| i - 1);
        TokenRun { start, end, class }
    }

    fn non_ws_run(&self, start: usize, forward: bool) -> Option<TokenRun> {
        let class = |i: usize| vim_token_class(self.1[i].repr, self.2);
        let ix = if forward {
            (start..self.1.len()).find(|&i| class(i) != TokenClass::Whitespace)?
        } else {
            (0..start).rev().find(|&i| class(i) != TokenClass::Whitespace)?
        };
        Some(self.run(ix))
    }

    fn positions(&self, start: usize, end: usize) -> (Position, Position) {
        (
            pos(self.0, self.1[start].char_start),
            pos(self.0, self.1[end].char_start),
        )
    }
}

fn word_motion(text: &VimText<'_>, kind: WordMotion, big: bool, n: usize) -> Position {
    let (mut line, mut col) = (text.cursor.line, text.cursor.column);
    let step = match kind {
        WordMotion::Forward => word_forward,
        WordMotion::Backward => word_backward,
        WordMotion::End => word_end,
    };
    for _ in 0..n {
        (line, col) = step(text, line, col, big);
    }
    pos(line, col)
}

fn word_forward(text: &VimText<'_>, mut line: usize, col: usize, big: bool) -> (usize, usize) {
    let mut tokens = TokenLine(line, line_cells(text, line), big);
    if tokens.1.is_empty() {
        if line + 1 < text.line_count() {
            return (line + 1, 0);
        }
        return (line, 0);
    }

    let mut ix = if col >= line_len(text, line) {
        tokens.1.len()
    } else {
        cell_containing_char(&tokens.1, col)
    };

    if ix < tokens.1.len() && vim_token_class(tokens.1[ix].repr, big) != TokenClass::Whitespace {
        ix = tokens.run(ix).end + 1;
    }

    loop {
        if let Some(run) = tokens.non_ws_run(ix, true) {
            return (tokens.0, tokens.1[run.start].char_start);
        }
        if line + 1 < text.line_count() {
            line += 1;
            tokens = TokenLine(line, line_cells(text, line), big);
            ix = 0;
            if tokens.1.is_empty() {
                return (line, 0);
            }
        } else {
            return (line, last_cluster_col(text, line));
        }
    }
}

fn word_backward(text: &VimText<'_>, mut line: usize, col: usize, big: bool) -> (usize, usize) {
    let mut tokens = TokenLine(line, line_cells(text, line), big);
    let mut ix = if tokens.1.is_empty() {
        None
    } else {
        cell_partition_by_char(&tokens.1, col).checked_sub(1)
    };

    if ix.is_none() {
        if line == 0 {
            return (0, 0);
        }
        line -= 1;
        tokens = TokenLine(line, line_cells(text, line), big);
        if tokens.1.is_empty() {
            return (line, 0);
        }
        ix = Some(tokens.1.len() - 1);
    }
    let mut ix = ix.unwrap();

    loop {
        if let Some(run) = tokens.non_ws_run(ix + 1, false) {
            return (tokens.0, tokens.1[run.start].char_start);
        }
        if line == 0 {
            return (0, 0);
        }
        line -= 1;
        tokens = TokenLine(line, line_cells(text, line), big);
        if tokens.1.is_empty() {
            return (line, 0);
        }
        ix = tokens.1.len() - 1;
    }
}

fn word_end(text: &VimText<'_>, mut line: usize, col: usize, big: bool) -> (usize, usize) {
    let mut tokens = TokenLine(line, line_cells(text, line), big);
    let mut ix = if tokens.1.is_empty() {
        if line + 1 < text.line_count() {
            line += 1;
            tokens = TokenLine(line, line_cells(text, line), big);
            0
        } else {
            return (line, 0);
        }
    } else {
        let containing = cell_containing_char(&tokens.1, col);
        if containing + 1 < tokens.1.len() {
            containing + 1
        } else if line + 1 < text.line_count() {
            line += 1;
            tokens = TokenLine(line, line_cells(text, line), big);
            0
        } else {
            return (line, last_cluster_col(text, line));
        }
    };

    loop {
        if let Some(run) = tokens.non_ws_run(ix, true) {
            return (tokens.0, tokens.1[run.end].char_start);
        }
        if line + 1 < text.line_count() {
            line += 1;
            tokens = TokenLine(line, line_cells(text, line), big);
            ix = 0;
        } else {
            return (line, last_cluster_col(text, line));
        }
    }
}

// -- Text objects ------------------------------------------------------------

fn text_object(text: &VimText<'_>, obj: char, inner: bool, count: Option<usize>) -> Option<(Position, Position)> {
    delimited(text, obj, inner, true).or_else(|| match obj {
        'w' => word_object(text, inner, false, count.unwrap_or(1)),
        'W' => word_object(text, inner, true, count.unwrap_or(1)),
        'p' => paragraph_object(text, inner),
        _ => None,
    })
}

fn delimited(text: &VimText<'_>, obj: char, inner: bool, around: bool) -> Option<(Position, Position)> {
    match obj {
        '(' | ')' | 'b' => pair_object(text, '(', ')', inner),
        '{' | '}' | 'B' => pair_object(text, '{', '}', inner),
        '[' | ']' => pair_object(text, '[', ']', inner),
        '<' | '>' => pair_object(text, '<', '>', inner),
        '"' if around => quote_text_object(text, '"', inner),
        '\'' if around => quote_text_object(text, '\'', inner),
        '`' if around => quote_text_object(text, '`', inner),
        '"' => quote_object(text, '"', inner),
        '\'' => quote_object(text, '\'', inner),
        '`' => quote_object(text, '`', inner),
        _ => None,
    }
}

fn empty_inner_text_object_position(text: &VimText<'_>, obj: char) -> Option<Position> {
    let outer = delimited(text, obj, false, false)?;
    let from = advance_pos(text, outer.0)?;
    let to = retreat_pos(text, outer.1)?;
    (to < from).then_some(from)
}

fn word_object(text: &VimText<'_>, inner: bool, big: bool, count: usize) -> Option<(Position, Position)> {
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

fn word_object_at(text: &VimText<'_>, cursor: Position, inner: bool, big: bool) -> Option<(Position, Position)> {
    let tokens = TokenLine(cursor.line, line_cells(text, cursor.line), big);
    if tokens.1.is_empty() {
        return None;
    }
    let col = cursor.column.min(line_len(text, cursor.line).saturating_sub(1));
    let run = tokens.run(cell_containing_char(&tokens.1, col));

    let (start, end) = if inner {
        (run.start, run.end)
    } else if run.class == TokenClass::Whitespace {
        if let Some(next) = tokens.non_ws_run(run.end + 1, true) {
            (run.start, next.end)
        } else if let Some(prev) = tokens.non_ws_run(run.start, false) {
            (prev.start, run.end)
        } else {
            (run.start, run.end)
        }
    } else {
        let (mut start, mut end) = (run.start, run.end);
        if end + 1 < tokens.1.len() && vim_token_class(tokens.1[end + 1].repr, big) == TokenClass::Whitespace {
            end = tokens.run(end + 1).end;
        } else if start > 0 && vim_token_class(tokens.1[start - 1].repr, big) == TokenClass::Whitespace {
            start = tokens.run(start - 1).start;
        }
        (start, end)
    };

    Some(tokens.positions(start, end))
}

fn paragraph_object(text: &VimText<'_>, inner: bool) -> Option<(Position, Position)> {
    let total = text.line_count();
    if total == 0 {
        return None;
    }
    let cur = text.cursor.line;
    let is_blank = |l: usize| with_line_str(text, l, |line| line.trim().is_empty());
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

fn pair_object(text: &VimText<'_>, open: char, close: char, inner: bool) -> Option<(Position, Position)> {
    let start_col = text
        .cursor
        .column
        .min(line_len(text, text.cursor.line).saturating_sub(1));
    let start_chars = line_chars(text, text.cursor.line);
    let depth = if start_chars.get(start_col) == Some(&close) {
        -1
    } else {
        0
    };
    let open_pos = scan_pair(text, pos(text.cursor.line, start_col), close, open, false, true, depth)?;
    let close_pos = scan_pair(text, open_pos, open, close, true, false, 0)?;

    if inner {
        let from = advance_pos(text, open_pos)?;
        let to = retreat_pos(text, close_pos)?;
        (from <= to).then_some((from, to))
    } else {
        Some((open_pos, close_pos))
    }
}

fn quote_object(text: &VimText<'_>, quote: char, inner: bool) -> Option<(Position, Position)> {
    let line = text.cursor.line;
    let chars = line_chars(text, line);
    let col = text.cursor.column;

    let mut previous_quote = None;
    let mut best = None;
    for (end, &ch) in chars.iter().enumerate() {
        if ch != quote || is_escaped_quote(&chars, end) {
            continue;
        }
        if let Some(start) = previous_quote {
            let contains_cursor = start <= col && col <= end;
            let shortest = best.is_none_or(|(best_start, best_end)| end - start < best_end - best_start);
            if contains_cursor && quote_can_open(&chars, start) && quote_can_close(&chars, end) && shortest {
                best = Some((start, end));
            }
        }
        previous_quote = Some(end);
    }

    let (start, end) = best?;

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

fn quote_text_object(text: &VimText<'_>, quote: char, inner: bool) -> Option<(Position, Position)> {
    let (from, to) = quote_object(text, quote, inner)?;
    if inner {
        return Some((from, to));
    }

    let chars = line_chars(text, from.line);
    let mut start = from.column;
    let mut end = to.column;
    let extend_right = end + 1 < chars.len() && chars[end + 1].is_whitespace();
    while extend_right && end + 1 < chars.len() && chars[end + 1].is_whitespace() {
        end += 1;
    }
    while !extend_right && start > 0 && chars[start - 1].is_whitespace() {
        start -= 1;
    }
    Some((pos(from.line, start), pos(to.line, end)))
}

fn is_escaped_quote(chars: &[char], idx: usize) -> bool {
    chars[..idx].iter().rev().take_while(|&&ch| ch == '\\').count() % 2 == 1
}

fn quote_can_open(chars: &[char], idx: usize) -> bool {
    idx == 0 || !is_identifier_char(chars[idx - 1])
}

fn quote_can_close(chars: &[char], idx: usize) -> bool {
    idx + 1 >= chars.len() || !is_identifier_char(chars[idx + 1])
}

// -- Bracket matching --------------------------------------------------------

fn match_bracket(text: &VimText<'_>) -> Option<Position> {
    let line = text.cursor.line;
    let chars = line_chars(text, line);
    let col = (text.cursor.column..chars.len()).find(|&col| matches!(chars[col], '(' | ')' | '[' | ']' | '{' | '}'))?;
    let bracket = chars[col];

    let (inc, dec, forward) = match bracket {
        '(' => ('(', ')', true),
        ')' => (')', '(', false),
        '[' => ('[', ']', true),
        ']' => (']', '[', false),
        '{' => ('{', '}', true),
        '}' => ('}', '{', false),
        _ => return None,
    };

    scan_pair(text, pos(line, col), inc, dec, forward, false, 0)
}

fn scan_pair(
    text: &VimText<'_>,
    start: Position,
    inc: char,
    dec: char,
    forward: bool,
    return_before_dec: bool,
    initial_depth: i32,
) -> Option<Position> {
    let mut depth = initial_depth;
    let mut line = start.line;
    let mut chars = line_chars(text, line);
    let mut col = start.column;

    loop {
        if col < chars.len() {
            let ch = chars[col];
            if ch == inc {
                depth += 1;
            }
            if ch == dec {
                if return_before_dec && depth == 0 {
                    return Some(pos(line, col));
                }
                depth -= 1;
            }
            if !return_before_dec && depth == 0 {
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

// -- Helpers -----------------------------------------------------------------
fn line_len(text: &VimText<'_>, line: usize) -> usize {
    if line >= text.line_count() {
        return 0;
    }
    text.cached_line(line)
        .map_or_else(|| display_line_char_len(text.buffer, line), |line| line.chars().count())
}

fn line_body<'a>(text: &'a VimText<'_>, line: usize) -> RopeSlice<'a> {
    let line = line.min(text.buffer.len_lines().saturating_sub(1));
    let len = display_line_char_len(text.buffer, line);
    text.buffer.line(line).slice(..len)
}

fn with_line_str<R>(text: &VimText<'_>, line: usize, f: impl FnOnce(&str) -> R) -> R {
    if line >= text.line_count() {
        return f("");
    }
    if let Some(line) = text.cached_line(line) {
        return f(line);
    }
    let body = line_body(text, line);
    if let Some(line) = body.as_str() {
        f(line)
    } else {
        f(&body.to_string())
    }
}
fn line_chars(text: &VimText<'_>, line: usize) -> Vec<char> {
    with_line_str(text, line, |line| line.chars().collect())
}
fn line_cells(text: &VimText<'_>, line: usize) -> Vec<GraphemeCell> {
    with_line_str(text, line, cells_of_str)
}

fn last_cluster_col(text: &VimText<'_>, line: usize) -> usize {
    with_line_str(text, line, last_grapheme_column)
}

fn word_under_cursor(text: &VimText<'_>) -> Option<String> {
    if text.cursor.line >= text.line_count() {
        return None;
    }
    with_line_str(text, text.cursor.line, |line| {
        let cells = cells_of_str(line);
        if cells.is_empty() {
            return None;
        }
        let tokens = TokenLine(text.cursor.line, cells, false);
        let col = text.cursor.column.min(line.chars().count().saturating_sub(1));
        let run = tokens.run(cell_containing_char(&tokens.1, col));
        if run.class != TokenClass::Word {
            return None;
        }
        let start_byte = tokens.1[run.start].byte_start;
        let end_byte = tokens.1.get(run.end + 1).map_or(line.len(), |c| c.byte_start);
        Some(line[start_byte..end_byte].to_string())
    })
}

fn first_non_blank(text: &VimText<'_>, line: usize) -> usize {
    let chars = line_chars(text, line);
    chars.iter().position(|c| !c.is_whitespace()).unwrap_or(0)
}
fn ordered(a: Position, b: Position) -> (Position, Position) {
    (a.min(b), a.max(b))
}
fn ordered_lines(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

fn advance_pos(text: &VimText<'_>, p: Position) -> Option<Position> {
    let ll = line_len(text, p.line);
    if p.column + 1 < ll {
        Some(pos(p.line, p.column + 1))
    } else if p.line + 1 < text.line_count() {
        Some(pos(p.line + 1, 0))
    } else {
        None
    }
}

fn retreat_pos(text: &VimText<'_>, p: Position) -> Option<Position> {
    if p.column > 0 {
        Some(pos(p.line, p.column - 1))
    } else if p.line > 0 {
        let prev = p.line - 1;
        Some(pos(prev, line_len(text, prev).saturating_sub(1)))
    } else {
        None
    }
}
