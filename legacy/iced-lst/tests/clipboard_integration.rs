mod common;

use common::*;
use lst::app::Message;

// ── MiddleClickPaste ────────────────────────────────────────────────────────

#[test]
fn middle_click_paste_inserts_primary_text() {
    let (mut app, _clip) = app_with_primary("hello", "pasted");
    app.update_inner(Message::MiddleClickPaste);
    let text = app.snapshot().text;
    assert!(
        text.contains("pasted"),
        "Expected 'pasted' in text, got '{text}'"
    );
}

#[test]
fn middle_click_paste_empty_primary_is_noop() {
    let (mut app, _clip) = app_with_clipboard("hello");
    app.update_inner(Message::MiddleClickPaste);
    assert_eq!(app.snapshot().text, "hello");
}

#[test]
fn middle_click_paste_is_undoable() {
    let (mut app, _clip) = app_with_primary("hello", "extra");
    app.update_inner(Message::MiddleClickPaste);
    let text_after_paste = app.snapshot().text;
    assert!(text_after_paste.contains("extra"));

    app.update_inner(Message::Undo);
    assert_eq!(app.snapshot().text, "hello");
}

// ── Quit clipboard ──────────────────────────────────────────────────────────

#[test]
fn quit_copies_text_to_clipboard() {
    let (mut app, clip) = app_with_clipboard("hello world");
    app.update_inner(Message::Quit);
    assert_eq!(clip.get_clipboard(), "hello world");
}

#[test]
fn quit_empty_text_does_not_copy() {
    let (mut app, clip) = app_with_clipboard("");
    app.update_inner(Message::Quit);
    assert_eq!(clip.get_clipboard(), "");
}

// ── Vim register vs clipboard ───────────────────────────────────────────────

#[test]
fn vim_yank_paste_uses_register_not_clipboard() {
    let (mut app, clip) = app_with_clipboard("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "yyp"); // yank line, paste below
                               // Clipboard should NOT have been written (vim uses internal register)
    assert_eq!(clip.get_clipboard(), "");
    // But text should be duplicated
    let text = app.snapshot().text;
    assert!(
        text.matches("hello").count() >= 2,
        "Expected 'hello' duplicated, got '{text}'"
    );
}

#[test]
fn vim_dd_paste_uses_register_not_clipboard() {
    let (mut app, clip) = app_with_clipboard("aaa\nbbb");
    enter_normal(&mut app);
    vim_keys(&mut app, "ddp"); // delete line, paste below
    assert_eq!(clip.get_clipboard(), ""); // clipboard untouched
                                          // Text should have lines swapped
    assert_eq!(app.snapshot().text, "bbb\naaa");
}

// ── MulticlickReleased syncs to primary ─────────────────────────────────────

#[test]
fn multiclick_released_copies_selection_to_primary() {
    let (mut app, clip) = app_with_clipboard("hello world");
    // Create a selection via Edit actions (simulating mouse select)
    use iced::widget::text_editor::{Action, Motion};
    app.update_inner(Message::Edit(Action::Move(Motion::Home)));
    app.update_inner(Message::Edit(Action::Select(Motion::WordRight)));
    // Now release — should sync selection to primary
    app.update_inner(Message::MulticlickReleased);
    let primary = clip.get_primary();
    assert!(
        !primary.is_empty(),
        "Expected selection copied to primary clipboard"
    );
}
