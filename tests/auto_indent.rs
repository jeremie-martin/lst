mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn auto_indent_copies_leading_whitespace() {
    let mut app = App::test("");
    type_text(&mut app, "    hello");
    app.update_inner(Message::AutoIndent);
    type_text(&mut app, "world");
    assert_eq!(active_text(&app), "    hello\n    world");
}

#[test]
fn auto_indent_no_indent_on_unindented_line() {
    let mut app = App::test("");
    type_text(&mut app, "hello");
    app.update_inner(Message::AutoIndent);
    type_text(&mut app, "world");
    assert_eq!(active_text(&app), "hello\nworld");
}

#[test]
fn auto_indent_preserves_tabs() {
    let mut app = App::test("");
    type_text(&mut app, "\thello");
    app.update_inner(Message::AutoIndent);
    type_text(&mut app, "world");
    assert_eq!(active_text(&app), "\thello\n\tworld");
}
