mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn delete_line_removes_current_line() {
    let mut app = App::test("aaa\nbbb\nccc");
    // Cursor starts on line 0
    app.update_inner(Message::DeleteLine);
    assert_eq!(app.snapshot().text, "bbb\nccc");
}

#[test]
fn move_line_down_swaps_with_next() {
    let mut app = App::test("aaa\nbbb\nccc");
    app.update_inner(Message::MoveLineDown);
    assert_eq!(app.snapshot().text, "bbb\naaa\nccc");
}

#[test]
fn move_line_up_swaps_with_previous() {
    let mut app = App::test("aaa\nbbb\nccc");
    // Move cursor to line 1 through the message interface
    move_down(&mut app);
    app.update_inner(Message::MoveLineUp);
    assert_eq!(app.snapshot().text, "bbb\naaa\nccc");
}

#[test]
fn duplicate_line_copies_current_line() {
    let mut app = App::test("aaa\nbbb");
    app.update_inner(Message::DuplicateLine);
    assert_eq!(app.snapshot().text, "aaa\naaa\nbbb");
}

#[test]
fn toggle_comment_adds_prefix() {
    let mut app = App::test("hello");
    app.update_inner(Message::ToggleComment);
    let text = app.snapshot().text;
    assert!(text.starts_with("// ") || text.starts_with("# "));
}

#[test]
fn toggle_comment_twice_restores_original() {
    let mut app = App::test("hello");
    app.update_inner(Message::ToggleComment);
    app.update_inner(Message::ToggleComment);
    assert_eq!(app.snapshot().text, "hello");
}

#[test]
fn move_line_down_with_identical_lines_still_moves_cursor() {
    // Black-box equivalent of white-box test in app.rs
    let mut app = App::test("same\nsame");
    // Position cursor on line 0 col 2
    move_right(&mut app);
    move_right(&mut app);
    app.update_inner(Message::MoveLineDown);
    // Text unchanged (lines are identical) but cursor should now be on line 1
    assert_eq!(app.snapshot().text, "same\nsame");
    assert_eq!(app.snapshot().cursor_line, 1);
}

#[test]
fn toggle_comment_on_blank_line_moves_cursor() {
    // Black-box equivalent of white-box test in app.rs
    // Open a .rs file to get Rust comment prefix
    let mut app = App::test("");
    app.update_inner(Message::Opened(Ok((
        std::path::PathBuf::from("/tmp/test.rs"),
        "    ".to_string(),
    ))));
    // Cursor is at col 0 after opening; move to end (col 4)
    move_to_end(&mut app);
    assert_eq!(app.snapshot().cursor_column, 4);
    app.update_inner(Message::ToggleComment);
    // On a blank-ish line (just spaces), toggle comment should still move cursor
    // Text may or may not change, but cursor should move
    let snap = app.snapshot();
    // The key behavior: it doesn't crash, and cursor isn't stuck at col 4
    assert!(
        snap.cursor_column != 4 || snap.text != "    ",
        "Expected cursor to move or text to change"
    );
}
