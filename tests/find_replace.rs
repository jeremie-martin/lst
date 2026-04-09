use lst::app::{App, Message};

#[test]
fn find_open_makes_bar_visible() {
    let mut app = App::test("hello world");
    assert!(!app.snapshot().find_visible);

    app.update_inner(Message::FindOpen);
    let s = app.snapshot();
    assert!(s.find_visible);
    assert!(!s.find_replace_visible);
}

#[test]
fn find_open_replace_shows_both_bars() {
    let mut app = App::test("hello world");
    app.update_inner(Message::FindOpenReplace);
    let s = app.snapshot();
    assert!(s.find_visible);
    assert!(s.find_replace_visible);
}

#[test]
fn find_close_hides_bar() {
    let mut app = App::test("hello world");
    app.update_inner(Message::FindOpen);
    assert!(app.snapshot().find_visible);

    app.update_inner(Message::FindClose);
    assert!(!app.snapshot().find_visible);
}

#[test]
fn find_query_populates_matches() {
    let mut app = App::test("foo bar foo baz foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    assert_eq!(app.snapshot().find_match_count, 3);
}

#[test]
fn find_next_advances_selected_match() {
    let mut app = App::test("aaa bbb aaa bbb aaa");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("aaa".to_string()));
    let first = app.snapshot().find_current_match;

    app.update_inner(Message::FindNext);
    let second = app.snapshot().find_current_match;
    assert_ne!(first, second);
}

#[test]
fn replace_one_substitutes_current_match() {
    let mut app = App::test("foo bar foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    app.update_inner(Message::FindReplaceChanged("baz".to_string()));
    app.update_inner(Message::ReplaceOne);
    assert!(app.snapshot().text.contains("baz"));
}

#[test]
fn replace_all_substitutes_every_match() {
    let mut app = App::test("foo bar foo baz foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    app.update_inner(Message::FindReplaceChanged("qux".to_string()));
    app.update_inner(Message::ReplaceAll);
    let s = app.snapshot();
    assert!(!s.text.contains("foo"));
    assert_eq!(s.text.matches("qux").count(), 3);
}

#[test]
fn find_prev_goes_backward() {
    let mut app = App::test("aaa bbb aaa bbb aaa");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("aaa".to_string()));
    let first = app.snapshot().find_current_match;

    app.update_inner(Message::FindNext);
    assert_ne!(app.snapshot().find_current_match, first);

    app.update_inner(Message::FindPrev);
    assert_eq!(app.snapshot().find_current_match, first);
}

#[test]
fn find_open_when_visible_closes() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    assert!(app.snapshot().find_visible);

    app.update_inner(Message::FindOpen);
    assert!(!app.snapshot().find_visible);
}

#[test]
fn find_open_replace_upgrades_from_find_only() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    let s = app.snapshot();
    assert!(s.find_visible);
    assert!(!s.find_replace_visible);

    app.update_inner(Message::FindOpenReplace);
    let s = app.snapshot();
    assert!(s.find_visible);
    assert!(s.find_replace_visible);
}
