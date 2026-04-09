mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn new_tab_is_not_modified() {
    let app = App::test("hello");
    assert!(!app.tabs[0].modified);
}

#[test]
fn edit_sets_modified_flag() {
    let mut app = App::test("hello");
    type_text(&mut app, "x");
    assert!(app.tabs[0].modified);
}

#[test]
fn line_op_sets_modified_flag() {
    let mut app = App::test("hello\nworld");
    app.update_inner(Message::DeleteLine);
    assert!(app.tabs[0].modified);
}

#[test]
fn replace_all_sets_modified_flag() {
    let mut app = App::test("foo bar foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    app.update_inner(Message::FindReplaceChanged("baz".to_string()));
    app.update_inner(Message::ReplaceAll);
    assert!(app.tabs[0].modified);
}
