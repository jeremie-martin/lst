mod common;

use common::*;
use lst::app::{App, Message};

// ── Delete operations ───────────────────────────────────────────────────────

#[test]
fn dd_deletes_current_line() {
    let mut app = App::test("aaa\nbbb\nccc");
    enter_normal(&mut app);
    vim_keys(&mut app, "dd");
    assert_eq!(app.snapshot().text, "bbb\nccc");
}

#[test]
fn dw_deletes_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "dw");
    assert_eq!(app.snapshot().text, "world");
}

#[test]
fn d_dollar_deletes_to_end_of_line() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "d$");
    assert_eq!(app.snapshot().text, "");
}

#[test]
fn x_deletes_char_under_cursor() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'x');
    assert_eq!(app.snapshot().text, "ello");
}

#[test]
fn shift_x_deletes_char_before_cursor() {
    let mut app = App::test("hello");
    move_to_end(&mut app);
    enter_normal(&mut app); // col 4
    vim_key(&mut app, 'X');
    assert_eq!(app.snapshot().text, "helo");
}

#[test]
fn shift_d_deletes_to_end_of_line() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "llD"); // move to col 2, then D
    assert_eq!(app.snapshot().text, "he");
}

#[test]
fn two_dd_deletes_two_lines() {
    let mut app = App::test("aaa\nbbb\nccc");
    enter_normal(&mut app);
    vim_keys(&mut app, "2dd");
    assert_eq!(app.snapshot().text, "ccc");
}

#[test]
fn d2w_deletes_two_words() {
    let mut app = App::test("one two three");
    enter_normal(&mut app);
    vim_keys(&mut app, "d2w");
    assert_eq!(app.snapshot().text, "three");
}

// ── Change operations ───────────────────────────────────────────────────────

#[test]
fn cw_changes_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "cw");
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "goodbye");
    assert_eq!(app.snapshot().text, "goodbye world");
}

#[test]
fn cc_changes_entire_line() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "cc");
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "new line");
    assert_eq!(app.snapshot().text, "new line");
}

#[test]
fn shift_c_changes_to_end_of_line() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'C'); // at col 0, deletes entire line
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "new");
    assert_eq!(app.snapshot().text, "new");
}

#[test]
fn s_substitutes_char() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 's');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "y");
    assert_eq!(app.snapshot().text, "yello");
}

#[test]
fn shift_s_changes_whole_line() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'S');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "world");
    assert_eq!(app.snapshot().text, "world");
}

// ── Yank and paste ──────────────────────────────────────────────────────────

#[test]
fn yy_p_duplicates_line() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "yyp");
    assert_eq!(app.snapshot().text, "hello\nhello");
}

#[test]
fn dd_p_restores_deleted_line_below() {
    let mut app = App::test("aaa\nbbb");
    enter_normal(&mut app);
    vim_keys(&mut app, "ddp");
    assert_eq!(app.snapshot().text, "bbb\naaa");
}

#[test]
fn shift_p_pastes_before() {
    let mut app = App::test("aaa\nbbb");
    enter_normal(&mut app);
    vim_keys(&mut app, "yyP");
    // P pastes line above current line
    assert_eq!(app.snapshot().text, "aaa\naaa\nbbb");
}

#[test]
fn yw_p_yanks_and_pastes_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "yw"); // yank "hello "
    vim_key(&mut app, '$'); // go to end
    vim_key(&mut app, 'p'); // paste after cursor
    let text = app.snapshot().text;
    // "hello " should appear somewhere after "world"
    assert!(text.contains("hello "), "Expected 'hello ' in '{text}'");
    assert!(
        text.starts_with("hello world"),
        "Expected text to start with 'hello world', got '{text}'"
    );
}

#[test]
fn yy_preserves_text() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "yy");
    // yy should not modify the text
    assert_eq!(app.snapshot().text, "hello");
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

// ── Text objects ────────────────────────────────────────────────────────────

#[test]
fn diw_deletes_inner_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "diw");
    assert_eq!(app.snapshot().text, " world");
}

#[test]
fn daw_deletes_a_word_with_space() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "daw");
    assert_eq!(app.snapshot().text, "world");
}

#[test]
fn di_quote_deletes_inside_quotes() {
    let mut app = App::test("say \"hello\" end");
    // Move cursor inside the quotes
    enter_normal(&mut app);
    vim_keys(&mut app, "f\"l"); // move to opening quote, then one right (inside)
    vim_keys(&mut app, "di\"");
    assert_eq!(app.snapshot().text, "say \"\" end");
}

#[test]
fn ci_paren_changes_inside_parens() {
    let mut app = App::test("fn(hello)");
    enter_normal(&mut app);
    vim_keys(&mut app, "f("); // move to '('
    vim_keys(&mut app, "ci(");
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "world");
    assert_eq!(app.snapshot().text, "fn(world)");
}

#[test]
fn da_brace_deletes_around_braces() {
    let mut app = App::test("x {hello} y");
    enter_normal(&mut app);
    vim_keys(&mut app, "f{"); // move to '{'
    vim_keys(&mut app, "da{");
    let text = app.snapshot().text;
    assert!(
        !text.contains('{'),
        "Braces should be deleted, got '{text}'"
    );
    assert!(
        !text.contains("hello"),
        "Content should be deleted, got '{text}'"
    );
}

// ── Special commands ────────────────────────────────────────────────────────

#[test]
fn shift_j_joins_lines() {
    let mut app = App::test("hello\nworld");
    enter_normal(&mut app);
    vim_key(&mut app, 'J');
    assert_eq!(app.snapshot().text, "hello world");
}

#[test]
fn r_replaces_single_char() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "ra");
    assert_eq!(app.snapshot().text, "aello");
    assert_eq!(app.snapshot().vim_mode, "NORMAL"); // stays in Normal
}

// ── Undo / redo ─────────────────────────────────────────────────────────────

#[test]
fn u_undoes_last_change() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "dd"); // delete line
    assert_eq!(app.snapshot().text, "");
    vim_key(&mut app, 'u');
    assert_eq!(app.snapshot().text, "hello");
}

#[test]
fn ctrl_r_redoes_after_undo() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "dd");
    vim_key(&mut app, 'u');
    assert_eq!(app.snapshot().text, "hello");
    vim_ctrl(&mut app, 'r');
    assert_eq!(app.snapshot().text, "");
}

// ── Search integration ──────────────────────────────────────────────────────

#[test]
fn slash_opens_find_bar() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, '/');
    assert!(app.snapshot().find_visible);
}

#[test]
fn n_navigates_to_next_match() {
    let mut app = App::test("foo bar foo baz foo");
    // Set up find query through the message interface
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    app.update_inner(Message::FindNext); // go to first match
    app.update_inner(Message::FindClose);

    enter_normal(&mut app);
    let col_before = app.snapshot().cursor_column;
    vim_key(&mut app, 'n');
    let col_after = app.snapshot().cursor_column;
    assert!(
        col_after > col_before,
        "n should advance to next match: before={col_before}, after={col_after}"
    );
}

#[test]
fn shift_n_navigates_to_prev_match() {
    let mut app = App::test("foo bar foo baz foo");
    app.update_inner(Message::FindOpen);
    app.update_inner(Message::FindQueryChanged("foo".to_string()));
    // Navigate to the last match
    app.update_inner(Message::FindNext);
    app.update_inner(Message::FindNext);
    app.update_inner(Message::FindNext);
    app.update_inner(Message::FindClose);

    enter_normal(&mut app);
    let col_before = app.snapshot().cursor_column;
    vim_key(&mut app, 'N');
    let col_after = app.snapshot().cursor_column;
    assert!(
        col_after < col_before,
        "N should go to previous match: before={col_before}, after={col_after}"
    );
}

// ── Count with operators ────────────────────────────────────────────────────

#[test]
fn three_x_deletes_three_chars() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "3x");
    assert_eq!(app.snapshot().text, "lo");
}

#[test]
fn count_with_r_replaces_multiple() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "3ra");
    assert_eq!(app.snapshot().text, "aaalo");
}
