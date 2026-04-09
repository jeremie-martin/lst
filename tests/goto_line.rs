mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn goto_line_open_shows_input() {
    let mut app = App::test("hello");
    assert!(app.goto_line.is_none());
    app.update_inner(Message::GotoLineOpen);
    assert_eq!(app.goto_line, Some(String::new()));
}

#[test]
fn goto_line_open_toggles() {
    let mut app = App::test("hello");
    app.update_inner(Message::GotoLineOpen);
    assert!(app.goto_line.is_some());
    app.update_inner(Message::GotoLineOpen);
    assert!(app.goto_line.is_none());
}

#[test]
fn goto_line_changed_updates_text() {
    let mut app = App::test("hello");
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged("5".to_string()));
    assert_eq!(app.goto_line, Some("5".to_string()));
}

#[test]
fn goto_line_submit_moves_cursor() {
    let doc = make_multiline_doc(10);
    let mut app = App::test(&doc);
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged("5".to_string()));
    app.update_inner(Message::GotoLineSubmit);
    assert_eq!(cursor_pos(&app).line, 4); // 1-based input → 0-based cursor
    assert!(app.goto_line.is_none()); // Input dismissed after submit
}

#[test]
fn goto_line_submit_clamps_to_last_line() {
    let mut app = App::test("one\ntwo\nthree");
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged("999".to_string()));
    app.update_inner(Message::GotoLineSubmit);
    assert_eq!(cursor_pos(&app).line, 2); // Last line
}

#[test]
fn goto_line_close_clears_input() {
    let mut app = App::test("hello");
    app.update_inner(Message::GotoLineOpen);
    assert!(app.goto_line.is_some());
    app.update_inner(Message::GotoLineClose);
    assert!(app.goto_line.is_none());
}
