mod common;
use common::*;
use lst::app::{App, Message};

use iced::widget::text_editor;

#[test]
fn insert_character_modifies_content() {
    let mut app = App::test("hello");
    app.tabs[0]
        .content
        .perform(text_editor::Action::Move(text_editor::Motion::End));
    type_text(&mut app, "!");
    assert_eq!(active_text(&app), "hello!");
}

#[test]
fn undo_restores_previous_state() {
    let mut app = App::test("hello");
    app.tabs[0]
        .content
        .perform(text_editor::Action::Move(text_editor::Motion::End));
    type_text(&mut app, " world");
    assert_eq!(active_text(&app), "hello world");

    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "hello");
}

#[test]
fn redo_after_undo_reapplies() {
    let mut app = App::test("abc");
    app.tabs[0]
        .content
        .perform(text_editor::Action::Move(text_editor::Motion::End));
    type_text(&mut app, "d");
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "abc");

    app.update_inner(Message::Redo);
    assert_eq!(active_text(&app), "abcd");
}
