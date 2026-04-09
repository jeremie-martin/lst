mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn insert_character_modifies_content() {
    let mut app = App::test("hello");
    move_to_end(&mut app);
    type_text(&mut app, "!");
    assert_eq!(app.snapshot().text, "hello!");
}

#[test]
fn undo_restores_previous_state() {
    let mut app = App::test("hello");
    move_to_end(&mut app);
    type_text(&mut app, " world");
    assert_eq!(app.snapshot().text, "hello world");

    app.update_inner(Message::Undo);
    assert_eq!(app.snapshot().text, "hello");
}

#[test]
fn redo_after_undo_reapplies() {
    let mut app = App::test("abc");
    move_to_end(&mut app);
    type_text(&mut app, "d");
    app.update_inner(Message::Undo);
    assert_eq!(app.snapshot().text, "abc");

    app.update_inner(Message::Redo);
    assert_eq!(app.snapshot().text, "abcd");
}
