#![allow(dead_code)]

use lst::app::{App, Message};

use iced::widget::text_editor;

pub fn active_text(app: &App) -> String {
    app.tabs[app.active].content.text()
}

pub fn tab_count(app: &App) -> usize {
    app.tabs.len()
}

pub fn active_tab(app: &App) -> usize {
    app.active
}

pub fn cursor_pos(app: &App) -> text_editor::Position {
    app.tabs[app.active].content.cursor().position
}

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

pub fn make_multiline_doc(lines: usize) -> String {
    (1..=lines).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
}

pub fn goto_line(app: &mut App, line: usize) {
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged(line.to_string()));
    app.update_inner(Message::GotoLineSubmit);
}
