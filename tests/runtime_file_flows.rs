mod common;

use common::*;
use lst::app::{App, Message, RuntimeMode};

#[test]
fn open_reads_selected_file_through_dialog_and_filesystem() {
    let mut app = AppHarness::new("first");
    app.fs.seed_file("/tmp/opened.txt", "opened");
    app.dialogs.push_open("/tmp/opened.txt");

    app.send(Message::Open);

    let snap = app.snapshot();
    assert_eq!(app.dialogs.open_requests(), 1);
    assert_eq!(snap.tab_count, 2);
    assert_eq!(snap.text, "opened");
}

#[test]
fn open_cancel_is_noop() {
    let mut app = AppHarness::new("first");
    app.dialogs.cancel_open();

    app.send(Message::Open);

    let snap = app.snapshot();
    assert_eq!(app.dialogs.open_requests(), 1);
    assert_eq!(snap.tab_count, 1);
    assert_eq!(snap.text, "first");
}

#[test]
fn save_writes_current_buffer_to_existing_path() {
    let mut app = AppHarness::new("hello");
    move_to_end(&mut app.app);
    type_text(&mut app.app, " world");

    app.send(Message::Save);

    assert_eq!(
        app.fs.file_text("/tmp/test.txt"),
        Some("hello world".to_string())
    );
    assert!(!app.snapshot().modified);
}

#[test]
fn save_failure_keeps_modified_flag_set() {
    let mut app = AppHarness::new("hello");
    move_to_end(&mut app.app);
    type_text(&mut app.app, " world");
    app.fs.set_write_failure("/tmp/test.txt");

    app.send(Message::Save);

    assert!(app.snapshot().modified);
}

#[test]
fn save_as_uses_dialog_destination_and_updates_title() {
    let mut app = AppHarness::new("hello");
    app.dialogs.push_save("/tmp/named.txt");

    app.send(Message::SaveAs);

    let snap = app.snapshot();
    assert_eq!(app.dialogs.save_requests(), 1);
    assert_eq!(app.dialogs.save_suggestions(), vec!["untitled.txt"]);
    assert_eq!(
        app.fs.file_text("/tmp/named.txt"),
        Some("hello".to_string())
    );
    assert_eq!(snap.tab_titles, vec!["named.txt"]);
    assert!(!snap.modified);
}

#[test]
fn autosave_tick_writes_modified_tabs() {
    let mut app = AppHarness::new("first");
    app.fs.seed_file("/tmp/second.txt", "second");
    app.dialogs.push_open("/tmp/second.txt");
    app.send(Message::Open);

    move_to_end(&mut app.app);
    type_text(&mut app.app, "x");
    app.send(Message::TabSelect(0));
    move_to_end(&mut app.app);
    type_text(&mut app.app, "y");

    app.send(Message::AutosaveTick);

    assert_eq!(
        app.fs.file_text("/tmp/test.txt"),
        Some("firsty".to_string())
    );
    assert_eq!(
        app.fs.file_text("/tmp/second.txt"),
        Some("secondx".to_string())
    );
    assert!(!app.snapshot().modified);
    app.send(Message::TabSelect(1));
    assert!(!app.snapshot().modified);
}

#[test]
fn closing_empty_scratchpad_removes_backing_file() {
    let mut app = AppHarness::boot_with(
        &["--scratchpad-dir", "/tmp/notes"],
        "2026-04-09_12-00-00",
        |_, _| {},
    );

    app.send(Message::CloseActiveTab);

    assert_eq!(
        app.fs.removed_files(),
        vec![std::path::PathBuf::from(
            "/tmp/notes/2026-04-09_12-00-00.md"
        )]
    );
}

#[test]
fn update_inner_executes_save_as_inline_for_test_apps() {
    let (_clipboard, fs, dialogs, services) = runtime_services_with_options(
        "2026-04-09_12-00-00",
        RuntimeMode::Inline,
        Some("/tmp/lst-inline-home"),
    );
    dialogs.push_save("/tmp/inline.txt");
    let mut app = App::test_with_services("hello", services);

    app.update_inner(Message::SaveAs);

    assert_eq!(dialogs.save_requests(), 1);
    assert_eq!(fs.file_text("/tmp/inline.txt"), Some("hello".to_string()));
    assert_eq!(app.snapshot().tab_titles, vec!["inline.txt"]);
    assert!(!app.snapshot().modified);
}
