//! Public Vim vocabulary and state.

use crate::selection::Position;

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
pub(crate) struct CharFind {
    pub target: char,
    pub forward: bool,
    pub till: bool,
}

pub struct VimState {
    pub mode: Mode,
    pub register: Register,
    pub(crate) visual: Option<VisualState>,
    pub(crate) pending: String,
    pub(crate) char_find: Option<CharFind>,
    pub(crate) search_backward: bool,
}

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
            visual: None,
            pending: String::new(),
            char_find: None,
            search_backward: false,
        }
    }

    pub fn visual_state(&self) -> Option<VisualState> {
        self.visual
            .filter(|_| matches!(self.mode, Mode::Visual | Mode::VisualLine))
    }

    pub fn pending_display(&self) -> String {
        self.pending.clone()
    }

    pub(crate) fn clear_transient(&mut self) {
        self.visual = None;
        self.pending.clear();
        self.char_find = None;
    }

    pub(crate) fn on_tab_switch(&mut self) {
        self.pending.clear();
        self.char_find = None;
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
            self.mode = Mode::Normal;
            self.visual = None;
        }
    }
}
