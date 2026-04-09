#![allow(dead_code)]

use lst::app::{App, Message};

use iced::widget::text_editor;

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

pub fn goto_line(app: &mut App, line: usize) {
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged(line.to_string()));
    app.update_inner(Message::GotoLineSubmit);
}

pub fn make_multiline_doc(lines: usize) -> String {
    (1..=lines).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}
