mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn goto_line_open_shows_input() {
    let mut app = App::test("hello");
    assert!(!app.snapshot().goto_line_visible);
    app.update_inner(Message::GotoLineOpen);
    let s = app.snapshot();
    assert!(s.goto_line_visible);
    assert_eq!(s.goto_line_text, "");
}

#[test]
fn goto_line_open_toggles() {
    let mut app = App::test("hello");
    app.update_inner(Message::GotoLineOpen);
    assert!(app.snapshot().goto_line_visible);
    app.update_inner(Message::GotoLineOpen);
    assert!(!app.snapshot().goto_line_visible);
}

#[test]
fn goto_line_changed_updates_text() {
    let mut app = App::test("hello");
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged("5".to_string()));
    assert_eq!(app.snapshot().goto_line_text, "5");
}

#[test]
fn goto_line_submit_moves_cursor() {
    let doc = make_multiline_doc(10);
    let mut app = App::test(&doc);
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged("5".to_string()));
    app.update_inner(Message::GotoLineSubmit);
    let s = app.snapshot();
    assert_eq!(s.cursor_line, 4); // 1-based input → 0-based cursor
    assert!(!s.goto_line_visible); // Input dismissed after submit
}

#[test]
fn goto_line_submit_clamps_to_last_line() {
    let mut app = App::test("one\ntwo\nthree");
    app.update_inner(Message::GotoLineOpen);
    app.update_inner(Message::GotoLineChanged("999".to_string()));
    app.update_inner(Message::GotoLineSubmit);
    assert_eq!(app.snapshot().cursor_line, 2); // Last line
}

#[test]
fn goto_line_close_clears_input() {
    let mut app = App::test("hello");
    app.update_inner(Message::GotoLineOpen);
    assert!(app.snapshot().goto_line_visible);
    app.update_inner(Message::GotoLineClose);
    assert!(!app.snapshot().goto_line_visible);
}
