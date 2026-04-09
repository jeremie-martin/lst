mod common;
use common::*;
use lst::app::{App, Message};

#[test]
fn page_down_moves_cursor_down() {
    let doc = make_multiline_doc(20);
    let mut app = App::test(&doc);
    assert_eq!(cursor_pos(&app).line, 0);
    app.update_inner(Message::PageDown(10, false));
    assert_eq!(cursor_pos(&app).line, 10);
}

#[test]
fn page_up_moves_cursor_up() {
    let doc = make_multiline_doc(20);
    let mut app = App::test(&doc);
    goto_line(&mut app, 16);
    assert_eq!(cursor_pos(&app).line, 15);

    app.update_inner(Message::PageUp(10, false));
    assert_eq!(cursor_pos(&app).line, 5);
}

#[test]
fn page_up_clamps_at_top() {
    let doc = make_multiline_doc(20);
    let mut app = App::test(&doc);
    goto_line(&mut app, 3);
    assert_eq!(cursor_pos(&app).line, 2);

    app.update_inner(Message::PageUp(10, false));
    assert_eq!(cursor_pos(&app).line, 0);
}

#[test]
fn page_down_clamps_at_bottom() {
    let doc = make_multiline_doc(20);
    let mut app = App::test(&doc);
    goto_line(&mut app, 18);

    app.update_inner(Message::PageDown(10, false));
    assert_eq!(cursor_pos(&app).line, 19);
}
