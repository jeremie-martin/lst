mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn new_tab_increases_count_and_switches() {
    let mut app = App::test("first");
    assert_eq!(tab_count(&app), 1);
    assert_eq!(active_tab(&app), 0);

    app.update_inner(Message::New);
    assert_eq!(tab_count(&app), 2);
    assert_eq!(active_tab(&app), 1);
}

#[test]
fn tab_select_switches_active() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    assert_eq!(active_tab(&app), 1);

    app.update_inner(Message::TabSelect(0));
    assert_eq!(active_tab(&app), 0);
}

#[test]
fn close_tab_reduces_count() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    type_text(&mut app, "second");
    assert_eq!(tab_count(&app), 2);

    app.update_inner(Message::TabClose(0));
    assert_eq!(tab_count(&app), 1);
    assert!(active_text(&app).contains("second"));
}

#[test]
fn next_prev_tab_cycles() {
    let mut app = App::test("a");
    app.update_inner(Message::New);
    app.update_inner(Message::New);
    assert_eq!(active_tab(&app), 2);

    app.update_inner(Message::PrevTab);
    assert_eq!(active_tab(&app), 1);

    app.update_inner(Message::NextTab);
    assert_eq!(active_tab(&app), 2);

    // Wraps around
    app.update_inner(Message::NextTab);
    assert_eq!(active_tab(&app), 0);
}
