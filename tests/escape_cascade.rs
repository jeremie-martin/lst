use lst::app::{App, Message};

#[test]
fn goto_line_close_dismisses_goto_first() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::GotoLineOpen);
    assert!(app.find.visible);
    assert!(app.goto_line.is_some());

    // GotoLineClose should dismiss goto-line first, leave find visible
    app.update_inner(Message::GotoLineClose);
    assert!(app.goto_line.is_none());
    assert!(app.find.visible);
}

#[test]
fn goto_line_close_dismisses_find_when_no_goto() {
    let mut app = App::test("hello");
    app.update_inner(Message::FindOpen);
    assert!(app.find.visible);
    assert!(app.goto_line.is_none());

    // No goto-line open, so GotoLineClose should dismiss find
    app.update_inner(Message::GotoLineClose);
    assert!(!app.find.visible);
}
