mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn move_tab_right_swaps_with_neighbor() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    app.update_inner(Message::New);
    // 3 tabs, active = 2 (last created)
    app.update_inner(Message::TabSelect(0));
    assert_eq!(app.snapshot().active_tab, 0);

    app.update_inner(Message::MoveTabRight);
    assert_eq!(app.snapshot().active_tab, 1);
}

#[test]
fn move_tab_left_swaps_with_neighbor() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    app.update_inner(Message::New);
    app.update_inner(Message::TabSelect(2));

    app.update_inner(Message::MoveTabLeft);
    assert_eq!(app.snapshot().active_tab, 1);
}

#[test]
fn move_tab_left_at_start_is_noop() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    app.update_inner(Message::TabSelect(0));

    app.update_inner(Message::MoveTabLeft);
    assert_eq!(app.snapshot().active_tab, 0);
}

#[test]
fn move_tab_right_at_end_is_noop() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    // Active is last tab (1)
    assert_eq!(app.snapshot().active_tab, 1);

    app.update_inner(Message::MoveTabRight);
    assert_eq!(app.snapshot().active_tab, 1);
}

#[test]
fn move_tab_preserves_content() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    type_text(&mut app, "second");
    app.update_inner(Message::TabSelect(0));

    app.update_inner(Message::MoveTabRight);
    assert_eq!(app.snapshot().text, "first");
}
