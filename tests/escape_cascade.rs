use lst::app::{App, Message};

#[test]
fn goto_line_close_dismisses_goto_first() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::GotoLineOpen);
    let s = app.snapshot();
    assert!(s.find_visible);
    assert!(s.goto_line_visible);

    // GotoLineClose should dismiss goto-line first, leave find visible
    app.update_inner(Message::GotoLineClose);
    let s = app.snapshot();
    assert!(!s.goto_line_visible);
    assert!(s.find_visible);
}

#[test]
fn goto_line_close_dismisses_find_when_no_goto() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    let s = app.snapshot();
    assert!(s.find_visible);
    assert!(!s.goto_line_visible);

    // No goto-line open, so GotoLineClose should dismiss find
    app.update_inner(Message::GotoLineClose);
    assert!(!app.snapshot().find_visible);
}
