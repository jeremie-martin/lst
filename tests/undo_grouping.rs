mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn consecutive_inserts_grouped_into_single_undo() {
    let mut app = App::test("");
    type_text(&mut app, "abc");
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "");
}

#[test]
fn whitespace_breaks_insert_group() {
    let mut app = App::test("");
    type_text(&mut app, "ab cd");
    // Undo removes "cd" and the space (snapshot was taken before the space)
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "ab");
    // Undo again removes "ab"
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "");
}

#[test]
fn delete_after_insert_starts_new_group() {
    let mut app = App::test("xyz");
    type_text(&mut app, "abc");
    backspace(&mut app);
    // Undo the backspace
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "abcxyz");
    // Undo the inserts
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "xyz");
}

#[test]
fn line_op_is_single_undo_step() {
    let mut app = App::test("hello\nworld");
    app.update_inner(Message::DeleteLine);
    assert!(!active_text(&app).starts_with("hello"));
    app.update_inner(Message::Undo);
    assert_eq!(active_text(&app), "hello\nworld");
}
