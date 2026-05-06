//! Under-review executable specs for multi-cursor and multi-selection behavior.
//!
//! Specs in this file are product decisions first. They run in the `x11-tdd`
//! profile while the behavior is being discussed or implemented. Once accepted
//! and green, move them into the blocking real-display suite.

mod support;

use lst_x11_harness::ChordMods;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_outdents_each_touched_multi_cursor_line() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-tab-each-line", |session| {
        let path =
            session.seed_file("shift-tab-each-line.txt", "    alpha\n    beta\n    gamma")?;
        let mut editor = session.open_file("shift-tab-each-line", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right><right><right>")?;
        editor.expect_cursor_heads(&[(0, 6)])?;

        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 6), (1, 6), (2, 6)])?;

        editor.keys("<S-tab>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_outdents_a_line_once_when_multiple_cursors_touch_it() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-tab-coalesce-line", |session| {
        let path = session.seed_file("shift-tab-coalesce-line.txt", "    foo foo")?;
        let mut editor = session.open_file("shift-tab-coalesce-line", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        editor.expect_cursor_heads(&[(0, 7), (0, 11)])?;

        editor.keys("<S-tab>")?;
        editor.save_then_expect_file(&path, "foo foo")?;
        editor.expect_cursor_heads(&[(0, 3), (0, 7)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_column_mode_outdents_only_lines_with_removable_indent() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-tab-column", |session| {
        let path = session.seed_file("shift-tab-column.txt", "    alpha\nbeta\n  gamma")?;
        let mut editor = session.open_file("shift-tab-column", &path)?;

        editor.drag_text(
            (0, 6),
            (2, 6),
            ChordMods {
                ctrl: false,
                alt: true,
                shift: true,
            },
        )?;
        editor.expect_cursor_heads(&[(0, 6), (1, 4), (2, 6)])?;

        editor.keys("<S-tab>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 4), (2, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_multi_cursor_outdent_is_one_undo_step() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-tab-undo", |session| {
        let path = session.seed_file("shift-tab-undo.txt", "    alpha\n    beta")?;
        let mut editor = session.open_file("shift-tab-undo", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right><right><right><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 6), (1, 6)])?;

        editor.keys("<S-tab>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "    alpha\n    beta")?;
        Ok(())
    })
}
