mod common;

use common::*;
use iced::event;
use iced::keyboard;
use lst::app::Message;

#[test]
fn command_s_routes_to_save() {
    let mut app = AppHarness::new("hello");
    move_to_end(&mut app.app);
    type_text(&mut app.app, "!");

    app.shortcut_char('s', keyboard::Modifiers::COMMAND);

    assert_eq!(
        app.fs.file_text("/tmp/test.txt"),
        Some("hello!".to_string())
    );
    assert!(!app.snapshot().modified);
}

#[test]
fn command_shift_s_routes_to_save_as() {
    let mut app = AppHarness::new("hello");
    app.dialogs.push_save("/tmp/renamed.txt");

    app.shortcut_char(
        's',
        keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
    );

    assert_eq!(
        app.fs.file_text("/tmp/renamed.txt"),
        Some("hello".to_string())
    );
    assert_eq!(app.snapshot().tab_titles, vec!["renamed.txt"]);
}

#[test]
fn command_o_routes_to_open() {
    let mut app = AppHarness::new("first");
    app.fs.seed_file("/tmp/opened.txt", "opened");
    app.dialogs.push_open("/tmp/opened.txt");

    app.shortcut_char('o', keyboard::Modifiers::COMMAND);

    assert_eq!(app.snapshot().text, "opened");
}

#[test]
fn command_g_routes_to_goto_line() {
    let mut app = AppHarness::new("hello");

    app.shortcut_char('g', keyboard::Modifiers::COMMAND);

    let snap = app.snapshot();
    assert!(snap.goto_line_visible);
    assert_eq!(snap.goto_line_text, "");
}

#[test]
fn command_h_routes_to_find_replace() {
    let mut app = AppHarness::new("hello");

    app.shortcut_char('h', keyboard::Modifiers::COMMAND);

    let snap = app.snapshot();
    assert!(snap.find_visible);
    assert!(snap.find_replace_visible);
}

#[test]
fn command_tab_and_command_shift_tab_cycle_tabs() {
    let mut app = AppHarness::new("first");
    app.send(Message::New);
    app.send(Message::New);

    app.shortcut_named(keyboard::key::Named::Tab, keyboard::Modifiers::COMMAND);
    assert_eq!(app.snapshot().active_tab, 0);

    app.shortcut_named(
        keyboard::key::Named::Tab,
        keyboard::Modifiers::COMMAND | keyboard::Modifiers::SHIFT,
    );
    assert_eq!(app.snapshot().active_tab, 2);
}

#[test]
fn alt_z_routes_to_wrap_toggle() {
    let mut app = AppHarness::new("hello");
    assert!(app.snapshot().word_wrap);

    app.shortcut_char('z', keyboard::Modifiers::ALT);

    assert!(!app.snapshot().word_wrap);
}

#[test]
fn close_requested_routes_to_quit() {
    let mut app = AppHarness::new("hello world");

    app.close_requested();

    assert_eq!(app.clipboard.get_clipboard(), "hello world");
}

#[test]
fn escape_routes_even_when_event_is_captured() {
    let mut app = AppHarness::new("hello");
    app.send(Message::FindOpen);
    app.send(Message::GotoLineOpen);

    app.dispatch_event(
        named_key_event(keyboard::key::Named::Escape, keyboard::Modifiers::default()),
        event::Status::Captured,
    );
    let snap = app.snapshot();
    assert!(!snap.goto_line_visible);
    assert!(snap.find_visible);

    app.dispatch_event(
        named_key_event(keyboard::key::Named::Escape, keyboard::Modifiers::default()),
        event::Status::Captured,
    );
    assert!(!app.snapshot().find_visible);
}
