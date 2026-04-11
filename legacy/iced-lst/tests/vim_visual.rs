mod common;

use common::*;
use lst::app::App;

// ── Visual mode activation ──────────────────────────────────────────────────

#[test]
fn v_toggles_visual_off() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    assert_eq!(app.snapshot().vim_mode, "VISUAL");
    vim_key(&mut app, 'v'); // toggle off
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

#[test]
fn shift_v_toggles_visual_line_off() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    assert_eq!(app.snapshot().vim_mode, "V-LINE");
    vim_key(&mut app, 'V'); // toggle off
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

#[test]
fn v_to_shift_v_switches_mode() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    assert_eq!(app.snapshot().vim_mode, "VISUAL");
    vim_key(&mut app, 'V');
    assert_eq!(app.snapshot().vim_mode, "V-LINE");
}

#[test]
fn shift_v_to_v_switches_mode() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    assert_eq!(app.snapshot().vim_mode, "V-LINE");
    vim_key(&mut app, 'v');
    assert_eq!(app.snapshot().vim_mode, "VISUAL");
}

// ── Visual selection observation ────────────────────────────────────────────

#[test]
fn visual_l_selects_chars() {
    let mut app = App::test("hello");
    enter_normal(&mut app); // cursor at col 0
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'l');
    let sel = app.snapshot().selection;
    assert!(sel.is_some(), "Expected a selection");
    // iced uses exclusive-end selection; vim visual is inclusive but ViewSnapshot
    // shows iced's view, so v + l from col 0 → head at col 1 → 1 char shown
    assert!(!sel.unwrap().is_empty());
}

#[test]
fn visual_w_selects_word() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'w');
    let sel = app.snapshot().selection.unwrap();
    assert!(
        sel.contains("hello"),
        "Selection should contain 'hello', got '{sel}'"
    );
}

#[test]
fn visual_j_extends_to_next_line() {
    let mut app = App::test("hello\nworld");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'j');
    let sel = app.snapshot().selection;
    assert!(sel.is_some(), "Expected a selection across lines");
    let sel = sel.unwrap();
    assert!(
        sel.contains('\n'),
        "Selection should span lines, got '{sel}'"
    );
}

#[test]
fn visual_line_selects_full_line() {
    let mut app = App::test("hello\nworld");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    let sel = app.snapshot().selection;
    assert!(sel.is_some(), "Expected a line selection");
    let sel = sel.unwrap();
    // V-LINE selects from col 0 to last char; iced's exclusive end clips the last char
    assert!(
        sel.contains("hell"),
        "Line selection should include most of 'hello', got '{sel}'"
    );
}

#[test]
fn visual_line_j_selects_two_lines() {
    let mut app = App::test("aaa\nbbb\nccc");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    vim_key(&mut app, 'j');
    let sel = app.snapshot().selection.unwrap();
    assert!(sel.contains("aaa"), "Selection should contain first line");
    // iced's exclusive-end may clip the last char of "bbb", but the newline should be there
    assert!(
        sel.contains('\n'),
        "Selection should span multiple lines, got '{sel}'"
    );
    assert!(
        sel.contains("bb"),
        "Selection should include most of second line, got '{sel}'"
    );
}

// ── Operators on visual selection ───────────────────────────────────────────

#[test]
fn visual_d_deletes_selection() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'e'); // select "hello"
    vim_key(&mut app, 'd');
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
    let text = app.snapshot().text;
    assert!(
        !text.contains("hello"),
        "Expected 'hello' deleted, got '{text}'"
    );
}

#[test]
fn visual_c_changes_selection() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'e'); // select "hello"
    vim_key(&mut app, 'c');
    assert_eq!(app.snapshot().vim_mode, "INSERT");
    type_text(&mut app, "goodbye");
    let text = app.snapshot().text;
    assert!(
        text.contains("goodbye"),
        "Expected 'goodbye' in text, got '{text}'"
    );
}

#[test]
fn visual_y_yanks_selection_and_returns_to_normal() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'e'); // select to end of word
    vim_key(&mut app, 'y');
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
    // Text unchanged after yank
    assert_eq!(app.snapshot().text, "hello world");
    // Paste to verify yank worked — p inserts after cursor
    vim_key(&mut app, 'p');
    let text = app.snapshot().text;
    assert!(
        text.len() > "hello world".len(),
        "Paste should have added text, got '{text}'"
    );
}

#[test]
fn visual_line_d_deletes_lines() {
    let mut app = App::test("aaa\nbbb\nccc");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    vim_key(&mut app, 'j'); // select first two lines
    vim_key(&mut app, 'd');
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
    assert_eq!(app.snapshot().text, "ccc");
}

#[test]
fn visual_escape_clears_selection() {
    let mut app = App::test("hello");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, 'l');
    assert!(app.snapshot().selection.is_some());
    escape(&mut app);
    assert_eq!(app.snapshot().selection, None);
}

// ── Case transforms in visual ───────────────────────────────────────────────

#[test]
fn visual_u_lowercases_selection() {
    let mut app = App::test("HELLO WORLD");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, '$'); // select entire line
    vim_key(&mut app, 'u');
    assert_eq!(app.snapshot().text, "hello world");
    assert_eq!(app.snapshot().vim_mode, "NORMAL");
}

#[test]
fn visual_shift_u_uppercases_selection() {
    let mut app = App::test("hello world");
    enter_normal(&mut app);
    vim_key(&mut app, 'v');
    vim_key(&mut app, '$');
    vim_key(&mut app, 'U');
    assert_eq!(app.snapshot().text, "HELLO WORLD");
}

#[test]
fn visual_line_u_lowercases_lines() {
    let mut app = App::test("HELLO\nWORLD");
    enter_normal(&mut app);
    vim_key(&mut app, 'V');
    vim_key(&mut app, 'j');
    vim_key(&mut app, 'u');
    assert_eq!(app.snapshot().text, "hello\nworld");
}
