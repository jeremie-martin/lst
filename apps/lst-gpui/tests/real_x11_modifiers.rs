//! Real-display tests for Ctrl/Shift modifier chords. Each test is a
//! self-contained "do inputs, assert output" scenario, exercising one
//! modifier-driven behaviour through the fixture's key helper so we
//! verify the chord notation (`<C-a>`, `<C-z>`, `<C-y>`) actually drives
//! the editor end-to-end.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_modifiers --run-ignored only

mod support;

use std::fs;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_a_select_all_then_type_replaces_buffer() -> TestResult {
    // Open a file with pre-seeded content, select all with Ctrl+A, then
    // type a fresh string. The active selection makes the next literal
    // input replace the entire buffer in one transaction. This is the
    // canonical "I want to start over" gesture and the simplest possible
    // proof that modifier chords mid-`send_keys` reach the editor.
    support::run_x11_test("modifier-ctrl-a", |session| {
        let seed_path = session.root().join("seed.txt");
        fs::write(&seed_path, "old content here\nstill old\n")?;
        let mut editor = session.open_file("file", &seed_path)?;

        editor.keys("<C-a>this should replace the existing text")?;
        editor.save_then_expect_file(&seed_path, "this should replace the existing text")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_z_undoes_a_typed_run() -> TestResult {
    // Type a single word — no internal word boundaries, so the editor
    // coalesces it into one undo group — then press Ctrl+Z. The buffer
    // should return to the empty state of a fresh scratchpad, which we
    // verify by saving and asserting the autosave path is empty.
    support::run_x11_test("modifier-ctrl-z", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("hello")?;
        editor.save_then_expect_file(&path, "hello")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_y_redoes_after_ctrl_z() -> TestResult {
    // Round-trip: type → undo → redo. The redo must restore the original
    // text exactly. Single word so the typing coalesces into one undo
    // group, mirroring `ctrl_z_undoes_a_typed_run`.
    support::run_x11_test("modifier-ctrl-y", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("hello<C-z><C-y>")?;
        editor.save_then_expect_file(&path, "hello")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_enter_in_insert_mode_inserts_newline() -> TestResult {
    // Shift+Enter (and Ctrl+Enter / Alt+Enter) should insert a literal
    // newline while editing — many keyboards send modified Enter from
    // chorded shortcuts, and a text editor must never silently drop them.
    // Default startup mode is INSERT, so we can type straight away.
    support::run_x11_test("modifier-shift-enter", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("alpha<S-enter>bravo")?;
        editor.save_then_expect_file(&path, "alpha\nbravo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_enter_in_insert_mode_inserts_newline() -> TestResult {
    support::run_x11_test("modifier-ctrl-enter", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("alpha<C-enter>bravo")?;
        editor.save_then_expect_file(&path, "alpha\nbravo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_enter_in_insert_mode_inserts_newline() -> TestResult {
    support::run_x11_test("modifier-alt-enter", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("alpha<A-enter>bravo")?;
        editor.save_then_expect_file(&path, "alpha\nbravo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_enter_preserves_indent_like_plain_enter() -> TestResult {
    // Modified Enter must go through the smart-indent path, not raw '\n' —
    // otherwise the new line lands at column 0 inside indented code.
    support::run_x11_test("modifier-shift-enter-indent", |session| {
        let path = session.seed_file("indent.txt", "    alpha")?;
        let mut editor = session.open_file("indent", &path)?;

        // Move to end-of-line, then Shift+Enter — the inserted line must
        // carry the four-space indent, matching plain Enter's behavior.
        editor.keys("<end><S-enter>bravo")?;
        editor.save_then_expect_file(&path, "    alpha\n    bravo")?;
        Ok(())
    })
}
