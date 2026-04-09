mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn find_open_makes_bar_visible() {
    let mut app = App::test("hello world");
    assert!(!app.find.visible);

    app.update_inner(Message::FindOpen);
    assert!(app.find.visible);
    assert!(!app.find.show_replace);
}

#[test]
fn find_open_replace_shows_both_bars() {
    let mut app = App::test("hello world");
    app.update_inner(Message::FindOpenReplace);
    assert!(app.find.visible);
    assert!(app.find.show_replace);
}

#[test]
fn find_close_hides_bar() {
    let mut app = App::test("hello world");
    app.update_inner(Message::FindOpen);
    assert!(app.find.visible);

    app.update_inner(Message::FindClose);
    assert!(!app.find.visible);
}

#[test]
fn find_query_populates_matches() {
    let mut app = App::test("foo bar foo baz foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    assert_eq!(app.find.matches.len(), 3);
}

#[test]
fn find_next_advances_selected_match() {
    let mut app = App::test("aaa bbb aaa bbb aaa");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("aaa".to_string()));
    let first = app.find.current;

    app.update_inner(Message::FindNext);
    let second = app.find.current;
    assert_ne!(first, second);
}

#[test]
fn replace_one_substitutes_current_match() {
    let mut app = App::test("foo bar foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    app.update_inner(Message::FindReplaceChanged("baz".to_string()));
    app.update_inner(Message::ReplaceOne);
    let text = active_text(&app);
    assert!(text.contains("baz"));
}

#[test]
fn replace_all_substitutes_every_match() {
    let mut app = App::test("foo bar foo baz foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    app.update_inner(Message::FindReplaceChanged("qux".to_string()));
    app.update_inner(Message::ReplaceAll);
    let text = active_text(&app);
    assert!(!text.contains("foo"));
    assert_eq!(text.matches("qux").count(), 3);
}

#[test]
fn find_prev_goes_backward() {
    let mut app = App::test("aaa bbb aaa bbb aaa");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("aaa".to_string()));
    let first = app.find.current;

    app.update_inner(Message::FindNext);
    assert_ne!(app.find.current, first);

    app.update_inner(Message::FindPrev);
    assert_eq!(app.find.current, first);
}

#[test]
fn find_open_when_visible_closes() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    assert!(app.find.visible);

    app.update_inner(Message::FindOpen);
    assert!(!app.find.visible);
}

#[test]
fn find_open_replace_upgrades_from_find_only() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    assert!(app.find.visible);
    assert!(!app.find.show_replace);

    app.update_inner(Message::FindOpenReplace);
    assert!(app.find.visible);
    assert!(app.find.show_replace);
}
