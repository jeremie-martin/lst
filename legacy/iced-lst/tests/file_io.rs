mod common;

use common::*;
use lst::app::{App, Error, Message};
use std::path::PathBuf;

// ── Open ────────────────────────────────────────────────────────────────────

#[test]
fn opened_ok_creates_new_tab() {
    let mut app = App::test("first");
    app.update_inner(Message::Opened(Ok((
        PathBuf::from("/tmp/second.txt"),
        "second".to_string(),
    ))));
    let snap = app.snapshot();
    assert_eq!(snap.tab_count, 2);
    assert_eq!(snap.text, "second"); // active tab is the newly opened one
}

#[test]
fn opened_replaces_empty_scratchpad() {
    // Create app, add scratchpad via New, close the original tab to leave only scratchpad
    let mut app = App::test("");
    app.update_inner(Message::New); // scratchpad at index 1
    app.update_inner(Message::TabClose(0)); // remove original, scratchpad is now the only tab
    assert_eq!(app.snapshot().tab_count, 1);

    app.update_inner(Message::Opened(Ok((
        PathBuf::from("/tmp/real.txt"),
        "real content".to_string(),
    ))));
    let snap = app.snapshot();
    assert_eq!(snap.tab_count, 1); // replaced, not added
    assert_eq!(snap.text, "real content");
}

#[test]
fn opened_does_not_replace_nonempty_scratchpad() {
    // Create scratchpad with content — Opened should add a new tab, not replace
    let mut app = App::test("");
    app.update_inner(Message::New); // scratchpad at index 1, now active
    type_text(&mut app, "some text"); // scratchpad has content
    app.update_inner(Message::TabClose(0)); // remove original
    assert_eq!(app.snapshot().tab_count, 1);

    app.update_inner(Message::Opened(Ok((
        PathBuf::from("/tmp/new.txt"),
        "new".to_string(),
    ))));
    assert_eq!(app.snapshot().tab_count, 2); // added, not replaced
}

#[test]
fn opened_err_is_noop() {
    let mut app = App::test("hello");
    app.update_inner(Message::Opened(Err(Error::DialogClosed)));
    assert_eq!(app.snapshot().tab_count, 1);
    assert_eq!(app.snapshot().text, "hello");
}

#[test]
fn multiple_opens_create_multiple_tabs() {
    let mut app = App::test("first");
    app.update_inner(Message::Opened(Ok((
        PathBuf::from("/tmp/a.txt"),
        "aaa".to_string(),
    ))));
    app.update_inner(Message::Opened(Ok((
        PathBuf::from("/tmp/b.txt"),
        "bbb".to_string(),
    ))));
    assert_eq!(app.snapshot().tab_count, 3);
}

// ── Save ────────────────────────────────────────────────────────────────────

#[test]
fn saved_ok_clears_modified() {
    let mut app = App::test("hello");
    type_text(&mut app, "x"); // mark modified
    assert!(app.snapshot().modified);
    app.update_inner(Message::Saved(Ok(PathBuf::from("/tmp/test.txt"))));
    assert!(!app.snapshot().modified);
}

#[test]
fn saved_ok_updates_tab_title() {
    let mut app = App::test("hello");
    app.update_inner(Message::Saved(Ok(PathBuf::from("/tmp/saved_file.rs"))));
    let titles = app.snapshot().tab_titles;
    assert!(
        titles.iter().any(|t| t.contains("saved_file")),
        "Expected 'saved_file' in tab titles, got {titles:?}"
    );
}

#[test]
fn saved_err_keeps_modified() {
    let mut app = App::test("hello");
    type_text(&mut app, "x");
    assert!(app.snapshot().modified);
    app.update_inner(Message::Saved(Err(Error::Io)));
    assert!(app.snapshot().modified); // still modified
}

// ── Autosave ────────────────────────────────────────────────────────────────

#[test]
fn autosave_complete_clears_modified() {
    let mut app = App::test("hello");
    type_text(&mut app, "x");
    assert!(app.snapshot().modified);
    // Simulate successful autosave completion for the tab's path
    app.update_inner(Message::AutosaveComplete(Ok(PathBuf::from(
        "/tmp/test.txt",
    ))));
    assert!(!app.snapshot().modified);
}

#[test]
fn autosave_complete_err_is_noop() {
    let mut app = App::test("hello");
    type_text(&mut app, "x");
    assert!(app.snapshot().modified);
    app.update_inner(Message::AutosaveComplete(Err(Error::Io)));
    assert!(app.snapshot().modified); // still modified
}

#[test]
fn autosave_only_clears_matching_tab() {
    let mut app = App::test("first");
    // Open a second tab
    app.update_inner(Message::Opened(Ok((
        PathBuf::from("/tmp/second.txt"),
        "second".to_string(),
    ))));
    // Modify both tabs
    type_text(&mut app, "x"); // modifies active (second) tab
    app.update_inner(Message::TabSelect(0));
    type_text(&mut app, "x"); // modifies first tab

    // Autosave completes for second tab only
    app.update_inner(Message::AutosaveComplete(Ok(PathBuf::from(
        "/tmp/second.txt",
    ))));

    // First tab (active) should still be modified
    assert!(app.snapshot().modified);
    // Switch to second tab — should no longer be modified
    app.update_inner(Message::TabSelect(1));
    assert!(!app.snapshot().modified);
}

// ── New / Scratchpad ────────────────────────────────────────────────────────

#[test]
fn new_creates_tab() {
    let mut app = App::test("first");
    let before = app.snapshot().tab_count;
    app.update_inner(Message::New);
    assert_eq!(app.snapshot().tab_count, before + 1);
}

#[test]
fn new_tab_has_empty_content() {
    let mut app = App::test("first");
    app.update_inner(Message::New);
    // New tab becomes active
    assert_eq!(app.snapshot().text, "");
}

#[test]
fn save_clears_scratchpad_title() {
    let mut app = App::test("first");
    app.update_inner(Message::New); // creates scratchpad, switches to it
    let snap = app.snapshot();
    let scratchpad_title = snap.tab_titles[snap.active_tab].clone();

    // Save the scratchpad with a real name
    app.update_inner(Message::Saved(Ok(PathBuf::from("/tmp/named_file.md"))));

    let snap = app.snapshot();
    let new_title = snap.tab_titles[snap.active_tab].clone();
    assert_ne!(
        new_title, scratchpad_title,
        "Title should change after save"
    );
    assert!(
        new_title.contains("named_file"),
        "Expected 'named_file' in title, got '{new_title}'"
    );
}
