mod common;
use common::*;
use lst::app::App;

#[test]
fn open_paren_auto_closes() {
    let mut app = App::test("");
    type_text(&mut app, "(");
    assert_eq!(active_text(&app), "()");
    assert_eq!(cursor_pos(&app).column, 1);
}

#[test]
fn open_brace_auto_closes() {
    let mut app = App::test("");
    type_text(&mut app, "{");
    assert_eq!(active_text(&app), "{}");
}

#[test]
fn open_bracket_auto_closes() {
    let mut app = App::test("");
    type_text(&mut app, "[");
    assert_eq!(active_text(&app), "[]");
}

#[test]
fn close_paren_overtypes_existing() {
    let mut app = App::test("");
    type_text(&mut app, "(");
    // Now we have "()" with cursor between
    type_text(&mut app, ")");
    // Should overtype, not insert duplicate
    assert_eq!(active_text(&app), "()");
    assert_eq!(cursor_pos(&app).column, 2);
}

#[test]
fn backspace_deletes_pair() {
    let mut app = App::test("");
    type_text(&mut app, "(");
    // Cursor is between ()
    backspace(&mut app);
    assert_eq!(active_text(&app), "");
}

#[test]
fn quote_auto_closes() {
    let mut app = App::test("");
    type_text(&mut app, "\"");
    assert_eq!(active_text(&app), "\"\"");
    assert_eq!(cursor_pos(&app).column, 1);
}

#[test]
fn quote_no_autoclose_after_word_char() {
    let mut app = App::test("");
    type_text(&mut app, "hello\"");
    assert_eq!(active_text(&app), "hello\"");
}

#[test]
fn quote_overtypes_existing() {
    let mut app = App::test("");
    type_text(&mut app, "\"");
    // Now we have "" with cursor between
    type_text(&mut app, "\"");
    // Should overtype
    assert_eq!(active_text(&app), "\"\"");
    assert_eq!(cursor_pos(&app).column, 2);
}

#[test]
fn single_quote_auto_closes() {
    let mut app = App::test("");
    type_text(&mut app, "'");
    assert_eq!(active_text(&app), "''");
    assert_eq!(cursor_pos(&app).column, 1);
}
