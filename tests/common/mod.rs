#![allow(dead_code)]

use lst::app::{App, Message};
use lst::clipboard::Clipboard;

use iced::keyboard;
use iced::widget::text_editor;
use std::sync::{Arc, Mutex};

// ── Action helpers (drive the app through its message interface) ─────────────

pub fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.update_inner(Message::Edit(text_editor::Action::Edit(
            text_editor::Edit::Insert(ch),
        )));
    }
}

pub fn backspace(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Edit(
        text_editor::Edit::Backspace,
    )));
}

pub fn move_to_end(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Move(
        text_editor::Motion::End,
    )));
}

pub fn move_down(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Move(
        text_editor::Motion::Down,
    )));
}

pub fn move_right(app: &mut App) {
    app.update_inner(Message::Edit(text_editor::Action::Move(
        text_editor::Motion::Right,
    )));
}

pub fn goto_line(app: &mut App, line: usize) {
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged(line.to_string()));
    app.update_inner(Message::GotoLineSubmit);
}

pub fn make_multiline_doc(lines: usize) -> String {
    (1..=lines)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Vim helpers ─────────────────────────────────────────────────────────────

/// Send Escape — cascades through: close goto-line → close find → vim mode transition.
/// In a clean test state (no overlays), this enters Normal mode from Insert.
pub fn escape(app: &mut App) {
    app.update_inner(Message::GotoLineClose);
}

/// Alias for `escape` — enters Normal mode from Insert (when no overlays are open).
pub fn enter_normal(app: &mut App) {
    escape(app);
}

/// Send a single character as a VimKey message (no modifiers).
pub fn vim_key(app: &mut App, c: char) {
    app.update_inner(Message::VimKey(
        keyboard::Key::Character(c.to_string().into()),
        keyboard::Modifiers::default(),
    ));
}

/// Send a sequence of characters as VimKey messages.
pub fn vim_keys(app: &mut App, keys: &str) {
    for c in keys.chars() {
        vim_key(app, c);
    }
}

/// Send a VimKey with Ctrl/Cmd modifier (e.g., Ctrl+R for redo).
pub fn vim_ctrl(app: &mut App, c: char) {
    app.update_inner(Message::VimKey(
        keyboard::Key::Character(c.to_string().into()),
        keyboard::Modifiers::COMMAND,
    ));
}

// ── MemoryClipboard ─────────────────────────────────────────────────────────

/// A test clipboard that stores text in memory for inspection.
/// Clone-able — the test keeps a clone sharing the same Arc buffers.
#[derive(Clone)]
pub struct MemoryClipboard {
    clipboard: Arc<Mutex<String>>,
    primary: Arc<Mutex<String>>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(String::new())),
            primary: Arc::new(Mutex::new(String::new())),
        }
    }

    pub fn with_primary(text: &str) -> Self {
        Self {
            clipboard: Arc::new(Mutex::new(String::new())),
            primary: Arc::new(Mutex::new(text.to_string())),
        }
    }

    pub fn get_clipboard(&self) -> String {
        self.clipboard.lock().unwrap().clone()
    }

    pub fn get_primary(&self) -> String {
        self.primary.lock().unwrap().clone()
    }
}

impl Clipboard for MemoryClipboard {
    fn copy(&self, text: &str) {
        *self.clipboard.lock().unwrap() = text.to_string();
        self.copy_primary(text);
    }

    fn copy_primary(&self, text: &str) {
        *self.primary.lock().unwrap() = text.to_string();
    }

    fn read_primary(&self) -> Option<String> {
        let s = self.primary.lock().unwrap();
        if s.is_empty() {
            None
        } else {
            Some(s.clone())
        }
    }
}

// ── App builders with MemoryClipboard ───────────────────────────────────────

pub fn app_with_clipboard(text: &str) -> (App, MemoryClipboard) {
    let clip = MemoryClipboard::new();
    let mut app = App::test(text);
    app.clipboard = Box::new(clip.clone());
    (app, clip)
}

pub fn app_with_primary(text: &str, primary: &str) -> (App, MemoryClipboard) {
    let clip = MemoryClipboard::with_primary(primary);
    let mut app = App::test(text);
    app.clipboard = Box::new(clip.clone());
    (app, clip)
}
