use lst::app::{App, Message};

#[test]
fn word_wrap_starts_enabled() {
    let app = App::test("hello");
    assert!(app.snapshot().word_wrap);
}

#[test]
fn toggle_word_wrap_disables() {
    let mut app = App::test("hello");
    app.update_inner(Message::ToggleWordWrap);
    assert!(!app.snapshot().word_wrap);
}

#[test]
fn toggle_word_wrap_twice_restores() {
    let mut app = App::test("hello");
    app.update_inner(Message::ToggleWordWrap);
    app.update_inner(Message::ToggleWordWrap);
    assert!(app.snapshot().word_wrap);
}
