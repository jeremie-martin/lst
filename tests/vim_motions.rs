mod common;

use common::*;
use lst::app::App;

// ── Mode transitions ────────────────────────────────────────────────────────

#[test]
fn vim_starts_in_insert_mode() {
    let app = App::test("hello");
    assert_eq!(app.snapshot().vim_mode, "INSERT");
}

#[test]
fn escape_enters_normal_mode() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

#[test]
fn i_enters_insert_from_normal() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'i');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
}

#[test]
fn a_enters_insert_after_cursor() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    let col_before = app.snapshot().cursor_column;
    vim_key(&mut app, 'a');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    assert_eq!(app.snapshot().cursor_column, col_before + 1);
}

#[test]
fn shift_a_enters_insert_at_end_of_line() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'A');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    assert_eq!(app.snapshot().cursor_column, 5);
}

#[test]
fn shift_i_enters_insert_at_first_nonblank() {
    let mut app = App::test("  hello");
    // Move cursor to middle of the word
    move_to_end(&mut app);
    enter_normal(&mut app);
    vim_key(&mut app, 'I');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    assert_eq!(app.snapshot().cursor_column, 2);
}

#[test]
fn o_opens_line_below_and_enters_insert() {
    let mut app = App::test("hello\nworld");
    enter_normal(&mut app);
    vim_key(&mut app, 'o');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    assert_eq!(app.snapshot().cursor_line, 1);
    assert_eq!(app.snapshot().text, "hello\n\nworld");
}

#[test]
fn shift_o_opens_line_above_and_enters_insert() {
    let mut app = App::test("hello\nworld");
    enter_normal(&mut app);
    vim_key(&mut app, 'O');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    assert_eq!(app.snapshot().cursor_line, 0);
    assert_eq!(app.snapshot().text, "\nhello\nworld");
}

#[test]
fn v_enters_visual_mode() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    assert_eq!(app.snapshot().vim_mode, "VISUAL");
}

#[test]
fn shift_v_enters_visual_line_mode() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    assert_eq!(app.snapshot().vim_mode, "V-LINE");
}

#[test]
fn escape_from_visual_returns_to_normal() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    assert_eq!(app.snapshot().vim_mode, "VISUAL");
    escape(&mut app);
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

#[test]
fn escape_from_visual_line_returns_to_normal() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    assert_eq!(app.snapshot().vim_mode, "V-LINE");
    escape(&mut app);
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

// ── Cursor behavior on mode switch ──────────────────────────────────────────

#[test]
fn cursor_moves_left_entering_normal() {
    let mut app = App::test("hello");
    move_to_end(&mut app); // cursor at col 5
    enter_normal(&mut app);
    // Normal mode moves cursor left by 1 (can't be past last char)
    assert_eq!(app.snapshot().cursor_column, 4);
}

#[test]
fn cursor_stays_at_zero_entering_normal() {
    let mut app = App::test("hello");
    // cursor starts at col 0
    enter_normal(&mut app);
    assert_eq!(app.snapshot().cursor_column, 0);
}

// ── Basic motions (hjkl) ────────────────────────────────────────────────────

#[test]
fn h_moves_cursor_left() {
    let mut app = App::test("hello");
    move_to_end(&mut app);
    enter_normal(&mut app); // cursor at col 4
    vim_key(&mut app, 'h');
    assert_eq!(app.snapshot().cursor_column, 3);
}

#[test]
fn l_moves_cursor_right() {
    let mut app = App::test("hello");
    enter_normal(&mut app); // cursor at col 0
    vim_key(&mut app, 'l');
    assert_eq!(app.snapshot().cursor_column, 1);
}

#[test]
fn j_moves_cursor_down() {
    let mut app = App::test("hello\nworld");
    enter_normal(&mut app);
    vim_key(&mut app, 'j');
    assert_eq!(app.snapshot().cursor_line, 1);
}

#[test]
fn k_moves_cursor_up() {
    let mut app = App::test("hello\nworld");
    move_down(&mut app);
    enter_normal(&mut app);
    vim_key(&mut app, 'k');
    assert_eq!(app.snapshot().cursor_line, 0);
}

// ── Word motions ────────────────────────────────────────────────────────────

#[test]
fn w_moves_to_next_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'w');
    assert_eq!(app.snapshot().cursor_column, 6);
}

#[test]
fn b_moves_to_previous_word() {
    let mut app = App::test("hello world");
    move_to_end(&mut app);
    enter_normal(&mut app);
    vim_key(&mut app, 'b');
    assert_eq!(app.snapshot().cursor_column, 6);
}

#[test]
fn e_moves_to_end_of_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'e');
    assert_eq!(app.snapshot().cursor_column, 4);
}

// ── Line motions ────────────────────────────────────────────────────────────

#[test]
fn zero_moves_to_line_start() {
    let mut app = App::test("hello");
    move_to_end(&mut app);
    enter_normal(&mut app);
    vim_key(&mut app, '0');
    assert_eq!(app.snapshot().cursor_column, 0);
}

#[test]
fn dollar_moves_to_line_end() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, '$');
    assert_eq!(app.snapshot().cursor_column, 4); // last char index
}

#[test]
fn caret_moves_to_first_nonblank() {
    let mut app = App::test("  hello");
    enter_normal(&mut app);
    vim_key(&mut app, '^');
    assert_eq!(app.snapshot().cursor_column, 2);
}

// ── Document motions ────────────────────────────────────────────────────────

#[test]
fn gg_moves_to_first_line() {
    let doc = make_multiline_doc(10);
    let mut app = App::test(&doc);
    // Move to a line in the middle
    for _ in 0..5 {
        move_down(&mut app);
    }
    enter_normal(&mut app);
    vim_keys(&mut app, "gg");
    assert_eq!(app.snapshot().cursor_line, 0);
}

#[test]
fn shift_g_moves_to_last_line() {
    let doc = make_multiline_doc(5);
    let mut app = App::test(&doc);
    enter_normal(&mut app);
    vim_key(&mut app, 'G');
    assert_eq!(app.snapshot().cursor_line, 4);
}

// ── Find char motions ───────────────────────────────────────────────────────

#[test]
fn f_moves_to_char() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "fo");
    // Should find the first 'o' at column 4
    assert_eq!(app.snapshot().cursor_column, 4);
}

#[test]
fn t_moves_before_char() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "to");
    // Should move to one before the first 'o' at column 3
    assert_eq!(app.snapshot().cursor_column, 3);
}

#[test]
fn semicolon_repeats_last_find() {
    let mut app = App::test("abacada");
    enter_normal(&mut app);
    vim_keys(&mut app, "fa"); // cursor at col 0 ('a'), finds next 'a' at col 2
    assert_eq!(app.snapshot().cursor_column, 2);
    vim_key(&mut app, ';'); // repeat find forward → next 'a' at col 4
    assert_eq!(app.snapshot().cursor_column, 4);
}

// ── Bracket match ───────────────────────────────────────────────────────────

#[test]
fn percent_moves_to_matching_bracket() {
    let mut app = App::test("(hello)");
    enter_normal(&mut app); // cursor at col 0 on '('
    vim_key(&mut app, '%');
    assert_eq!(app.snapshot().cursor_column, 6); // closing ')'
}

#[test]
fn percent_moves_back_from_closing_bracket() {
    let mut app = App::test("(hello)");
    move_to_end(&mut app);
    enter_normal(&mut app); // cursor at col 6 on ')'
    vim_key(&mut app, '%');
    assert_eq!(app.snapshot().cursor_column, 0); // opening '('
}

// ── Counts ──────────────────────────────────────────────────────────────────

#[test]
fn count_3j_moves_down_3_lines() {
    let doc = make_multiline_doc(10);
    let mut app = App::test(&doc);
    enter_normal(&mut app);
    vim_keys(&mut app, "3j");
    assert_eq!(app.snapshot().cursor_line, 3);
}

#[test]
fn count_2l_moves_right_2() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_keys(&mut app, "2l");
    assert_eq!(app.snapshot().cursor_column, 2);
}

#[test]
fn count_2w_moves_forward_2_words() {
    let mut app = App::test("one two three");
    enter_normal(&mut app);
    vim_keys(&mut app, "2w");
    assert_eq!(app.snapshot().cursor_column, 8); // start of "three"
}

// ── Pending display ─────────────────────────────────────────────────────────

#[test]
fn pressing_d_shows_pending() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'd');
    assert_eq!(app.snapshot().vim_pending, "d");
}

#[test]
fn completing_dd_clears_pending() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "dd");
    assert_eq!(app.snapshot().vim_pending, "");
}

#[test]
fn count_before_operator_shows_in_pending() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_keys(&mut app, "2d");
    assert_eq!(app.snapshot().vim_pending, "2d");
}

#[test]
fn escape_clears_pending() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'd');
    assert_eq!(app.snapshot().vim_pending, "d");
    escape(&mut app);
    assert_eq!(app.snapshot().vim_pending, "");
}
