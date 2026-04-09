mod common;
use common::*;
use lst::app::{App, Message};

use iced::widget::text_editor;

#[test]
fn delete_line_removes_current_line() {
    let mut app = App::test("aaa\nbbb\nccc");
    // Cursor starts on line 0
    app.update_inner(Message::DeleteLine);
    assert_eq!(active_text(&app), "bbb\nccc");
}

#[test]
fn move_line_down_swaps_with_next() {
    let mut app = App::test("aaa\nbbb\nccc");
    app.update_inner(Message::MoveLineDown);
    assert_eq!(active_text(&app), "bbb\naaa\nccc");
}

#[test]
fn move_line_up_swaps_with_previous() {
    let mut app = App::test("aaa\nbbb\nccc");
    // Move cursor to line 1
    app.tabs[0]
        .content
        .perform(text_editor::Action::Move(text_editor::Motion::Down));
    app.update_inner(Message::MoveLineUp);
    assert_eq!(active_text(&app), "bbb\naaa\nccc");
}

#[test]
fn duplicate_line_copies_current_line() {
    let mut app = App::test("aaa\nbbb");
    app.update_inner(Message::DuplicateLine);
    assert_eq!(active_text(&app), "aaa\naaa\nbbb");
}

#[test]
fn toggle_comment_adds_prefix() {
    let mut app = App::test("hello");
    app.update_inner(Message::ToggleComment);
    assert!(active_text(&app).starts_with("// ") || active_text(&app).starts_with("# "));
}

#[test]
fn toggle_comment_twice_restores_original() {
    let mut app = App::test("hello");
    app.update_inner(Message::ToggleComment);
    app.update_inner(Message::ToggleComment);
    assert_eq!(active_text(&app), "hello");
}
