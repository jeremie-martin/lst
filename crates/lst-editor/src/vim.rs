use crate::selection::Position;
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
    pub const COMMAND: Self = Self { command: true, control: false };
    pub const CONTROL: Self = Self { command: false, control: true };
    pub fn command(self) -> bool {
        self.command
    }
    pub fn control(self) -> bool {
        self.control
    }
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum VimCommand {
    MoveTo(Position),
    Select { anchor: Position, head: Position },
    DeleteLines { first: usize, last: usize },
    IndentLines { first: usize, last: usize },
    OutdentLines { first: usize, last: usize },
    YankLines { first: usize, last: usize },
    EnterInsert,
    Undo,
    Redo,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    SurroundRange { from: Position, to: Position, open: char, close: char },
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

#[derive(Default)]
struct Pending {
    text: String,
}

pub struct VimState {
    pub mode: Mode,
    pub register: Register,
    pub visual_anchor: Option<Position>,
    pending: Pending,
}

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}

#[rustfmt::skip]
impl VimState {
    pub fn new() -> Self { Self { mode: Mode::Insert, register: Register::Empty, visual_anchor: None, pending: Pending::default() } }
    pub fn pending_display(&self) -> String { self.pending.text.clone() }

    pub fn on_tab_switch(&mut self) {
        self.clear_pending(); self.visual_anchor = None;
        if self.mode != Mode::Insert { self.mode = Mode::Normal; }
    }

    pub fn handle_key(&mut self, key: &Key, mods: Modifiers, text: &TextSnapshot) -> Vec<VimCommand> {
        match self.mode {
            Mode::Insert => Vec::new(),
            Mode::Normal => self.handle_normal(key, mods, text),
            Mode::Visual | Mode::VisualLine => self.handle_visual(key, mods, text),
        }
    }

    pub fn enter_normal_from_escape(&mut self, cursor: Position, text: &TextSnapshot) -> Vec<VimCommand> {
        self.clear_pending(); self.visual_anchor = None;
        match self.mode {
            Mode::Insert => { self.mode = Mode::Normal; move_if_changed(cursor, Position::new(cursor.line, cursor.column.saturating_sub(1).min(line_last_cursor_col(text, cursor.line)))) }
            Mode::Visual | Mode::VisualLine => { self.mode = Mode::Normal; vec![VimCommand::MoveTo(cursor)] }
            Mode::Normal => vec![VimCommand::Noop],
        }
    }

    pub fn selection_command(&self, head: Position, text: &TextSnapshot) -> VimCommand {
        let anchor = self.visual_anchor.unwrap_or(text.cursor);
        if self.mode == Mode::VisualLine {
            VimCommand::Select { anchor: Position::new(anchor.line.min(head.line), 0), head: line_last_position(text, anchor.line.max(head.line)) }
        } else {
            VimCommand::Select { anchor, head }
        }
    }

    fn handle_normal(&mut self, key: &Key, mods: Modifiers, text: &TextSnapshot) -> Vec<VimCommand> {
        if mods.command() && char_key(key) == Some('r') { self.clear_pending(); return vec![VimCommand::Redo]; }
        if mods.control() {
            self.clear_pending();
            return match char_key(key) { Some('d') => vec![VimCommand::HalfPageDown], Some('u') => vec![VimCommand::HalfPageUp], Some('f') => vec![VimCommand::PageDown], Some('b') => vec![VimCommand::PageUp], _ => vec![VimCommand::Noop] };
        }
        if let Key::Named(named) = key { self.clear_pending(); return self.normal_named(*named, text); }
        let Some(c) = char_key(key) else { self.clear_pending(); return vec![VimCommand::Noop]; };
        if !self.pending.text.is_empty() { return self.resolve_pending(c, text); }
        match c {
            '0' => move_if_changed(text.cursor, Position::new(text.cursor.line, 0)),
            'h' => move_if_changed(text.cursor, Position::new(text.cursor.line, text.cursor.column.saturating_sub(1))),
            'l' => move_if_changed(text.cursor, Position::new(text.cursor.line, (text.cursor.column + 1).min(line_last_cursor_col(text, text.cursor.line)))),
            'j' => vec![VimCommand::MoveTo(Position::new((text.cursor.line + 1).min(text.line_count().saturating_sub(1)), text.cursor.column))],
            'k' => vec![VimCommand::MoveTo(Position::new(text.cursor.line.saturating_sub(1), text.cursor.column))],
            'i' => { self.mode = Mode::Insert; vec![VimCommand::EnterInsert] }
            'a' => { self.mode = Mode::Insert; vec![VimCommand::MoveTo(Position::new(text.cursor.line, (text.cursor.column + 1).min(line_len(text, text.cursor.line)))), VimCommand::EnterInsert] }
            'A' => { self.mode = Mode::Insert; vec![VimCommand::MoveTo(Position::new(text.cursor.line, line_len(text, text.cursor.line))), VimCommand::EnterInsert] }
            'I' => { self.mode = Mode::Insert; vec![VimCommand::MoveTo(Position::new(text.cursor.line, first_non_blank_col(text, text.cursor.line))), VimCommand::EnterInsert] }
            'v' => self.enter_visual(text, false), 'V' => self.enter_visual(text, true), 'u' => vec![VimCommand::Undo],
            'g' | 'd' | 'y' | '>' | '<' => { self.pending.text.push(c); vec![VimCommand::Noop] }
            _ => vec![VimCommand::Noop],
        }
    }

    fn normal_named(&mut self, named: NamedKey, text: &TextSnapshot) -> Vec<VimCommand> {
        match named {
            NamedKey::ArrowLeft => move_if_changed(text.cursor, Position::new(text.cursor.line, text.cursor.column.saturating_sub(1))),
            NamedKey::ArrowRight => move_if_changed(text.cursor, Position::new(text.cursor.line, (text.cursor.column + 1).min(line_last_cursor_col(text, text.cursor.line)))),
            NamedKey::ArrowUp => vec![VimCommand::MoveTo(Position::new(text.cursor.line.saturating_sub(1), text.cursor.column))],
            NamedKey::ArrowDown => vec![VimCommand::MoveTo(Position::new((text.cursor.line + 1).min(text.line_count().saturating_sub(1)), text.cursor.column))],
            NamedKey::Home => move_if_changed(text.cursor, Position::new(text.cursor.line, 0)),
            NamedKey::End => move_if_changed(text.cursor, Position::new(text.cursor.line, line_last_cursor_col(text, text.cursor.line))),
            NamedKey::PageDown => vec![VimCommand::PageDown], NamedKey::PageUp => vec![VimCommand::PageUp],
            _ => vec![VimCommand::Noop],
        }
    }

    fn enter_visual(&mut self, text: &TextSnapshot, linewise: bool) -> Vec<VimCommand> {
        self.clear_pending(); self.mode = if linewise { Mode::VisualLine } else { Mode::Visual }; self.visual_anchor = Some(text.cursor);
        let head = if linewise { line_last_position(text, text.cursor.line) } else { text.cursor };
        vec![self.selection_command(head, text)]
    }

    fn handle_visual(&mut self, key: &Key, mods: Modifiers, text: &TextSnapshot) -> Vec<VimCommand> {
        if mods.control() || mods.command() { self.clear_pending(); return vec![VimCommand::Noop]; }
        if let Key::Named(named) = key {
            let head = match named {
                NamedKey::ArrowUp => Position::new(text.cursor.line.saturating_sub(1), text.cursor.column),
                NamedKey::ArrowDown => Position::new((text.cursor.line + 1).min(text.line_count().saturating_sub(1)), text.cursor.column),
                NamedKey::ArrowLeft => Position::new(text.cursor.line, text.cursor.column.saturating_sub(1)),
                NamedKey::ArrowRight => Position::new(text.cursor.line, (text.cursor.column + 1).min(line_last_cursor_col(text, text.cursor.line))),
                _ => text.cursor,
            };
            return vec![self.selection_command(head, text)];
        }
        match char_key(key) {
            Some('j') => vec![self.selection_command(Position::new((text.cursor.line + 1).min(text.line_count().saturating_sub(1)), text.cursor.column), text)],
            Some('k') => vec![self.selection_command(Position::new(text.cursor.line.saturating_sub(1), text.cursor.column), text)],
            Some('>') | Some('<') => {
                let outdent = char_key(key) == Some('<');
                let (first, last) = visual_line_span(self.visual_anchor.unwrap_or(text.cursor), text.cursor);
                self.mode = Mode::Normal; self.visual_anchor = None;
                if outdent { vec![VimCommand::OutdentLines { first, last }, VimCommand::MoveTo(Position::new(first, 0))] } else { vec![VimCommand::IndentLines { first, last }, VimCommand::MoveTo(Position::new(first, 0))] }
            }
            _ => vec![VimCommand::Noop],
        }
    }

    fn resolve_pending(&mut self, c: char, text: &TextSnapshot) -> Vec<VimCommand> {
        let mut seq = std::mem::take(&mut self.pending.text); seq.push(c);
        match seq.as_str() {
            "gg" => vec![VimCommand::MoveTo(Position::new(0, 0))],
            "dd" => vec![VimCommand::DeleteLines { first: text.cursor.line, last: text.cursor.line }],
            "yy" => vec![VimCommand::YankLines { first: text.cursor.line, last: text.cursor.line }],
            ">>" => vec![VimCommand::IndentLines { first: text.cursor.line, last: text.cursor.line }],
            "<<" => vec![VimCommand::OutdentLines { first: text.cursor.line, last: text.cursor.line }],
            "ys" | "ysi" | "ysiw" => { self.pending.text = seq; vec![VimCommand::Noop] }
            _ if seq.starts_with("ysiw") => {
                let Some((open, close)) = surround_pair_for_char(c) else { return vec![VimCommand::Noop]; };
                let (from, to) = inner_word_positions(text);
                vec![VimCommand::SurroundRange { from, to, open, close }]
            }
            _ => vec![VimCommand::Noop],
        }
    }

    fn clear_pending(&mut self) { self.pending.text.clear(); }
}

fn char_key(key: &Key) -> Option<char> {
    match key {
        Key::Character(s) => s.chars().next(),
        Key::Named(_) => None,
    }
}
fn move_if_changed(from: Position, to: Position) -> Vec<VimCommand> {
    if from == to {
        vec![VimCommand::Noop]
    } else {
        vec![VimCommand::MoveTo(to)]
    }
}
fn line_len(text: &TextSnapshot, line: usize) -> usize {
    text.lines.get(line).map(|s| s.chars().count()).unwrap_or(0)
}
fn line_last_cursor_col(text: &TextSnapshot, line: usize) -> usize {
    line_len(text, line).saturating_sub(1)
}
fn line_last_position(text: &TextSnapshot, line: usize) -> Position {
    Position::new(line, line_last_cursor_col(text, line))
}
fn first_non_blank_col(text: &TextSnapshot, line: usize) -> usize {
    text.lines.get(line).map(|s| s.chars().position(|ch| !ch.is_whitespace()).unwrap_or(0)).unwrap_or(0)
}
fn visual_line_span(anchor: Position, head: Position) -> (usize, usize) {
    if anchor.line <= head.line {
        (anchor.line, head.line)
    } else {
        (head.line, anchor.line)
    }
}

fn inner_word_positions(text: &TextSnapshot) -> (Position, Position) {
    let line = text.cursor.line;
    let chars: Vec<char> = text.lines.get(line).map(|s| s.chars().collect()).unwrap_or_default();
    if chars.is_empty() {
        return (Position::new(line, 0), Position::new(line, 0));
    }
    let mut start = text.cursor.column.min(chars.len().saturating_sub(1));
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = text.cursor.column.min(chars.len().saturating_sub(1));
    while end + 1 < chars.len() && !chars[end + 1].is_whitespace() {
        end += 1;
    }
    (Position::new(line, start), Position::new(line, end))
}

pub fn surround_pair_for_char(c: char) -> Option<(char, char)> {
    Some(match c {
        '(' | ')' => ('(', ')'),
        '[' | ']' => ('[', ']'),
        '{' | '}' => ('{', '}'),
        '<' | '>' => ('<', '>'),
        '"' => ('"', '"'),
        '\'' => ('\'', '\''),
        '`' => ('`', '`'),
        _ => return None,
    })
}
