mod common;

use common::*;
use lst::app::App;

#[test]
fn insert_mode_no_selection() {
    let app = App::test("hello");
    assert_eq!(app.snapshot().selection, None);
}

#[test]
fn normal_mode_selection_is_none() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    // Block cursor (1-char selection) is filtered out in ViewSnapshot
    assert_eq!(app.snapshot().selection, None);
}

#[test]
fn insert_from_normal_does_not_replace_block_cursor() {
    let mut app = App::test("abc");
    enter_normal(&mut app); // cursor at col 0, block cursor selects col 0-1
    vim_key(&mut app, 'i'); // enter Insert — should collapse block cursor, not replace
    type_text(&mut app, "x");
    assert_eq!(app.snapshot().text, "xabc"); // x inserted, nothing replaced
}
