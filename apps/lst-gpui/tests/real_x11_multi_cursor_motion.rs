//! Accepted real-display behavior for moving and extending existing
//! multi-cursor sets. Every test builds the cursor set first, checks it, then
//! asserts each cursor's anchor and head after the motion under test.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_multi_cursor_motion --run-ignored only

mod support;

use support::{EditorTestExt, SelectionTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn right_motion_moves_every_cursor_before_literal_input() -> TestResult {
    support::run_x11_test("multi-cursor-right-motion", |session| {
        let path = session.seed_file("right-motion.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("right-motion", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<right><right>")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;

        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alXpha\nbeXta\ngaXmma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn end_moves_each_cursor_to_own_line_end() -> TestResult {
    support::run_x11_test("multi-cursor-end-line-end", |session| {
        let path = session.seed_file("end-line-end.txt", "a\nabcd\nabcdef")?;
        let mut editor = session.open_file("end-line-end", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<end>")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 4), (2, 6)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn home_toggles_each_cursor_between_first_non_blank_and_column_zero() -> TestResult {
    support::run_x11_test("multi-cursor-home-toggle", |session| {
        let path = session.seed_file("home-toggle.txt", "    alpha\n  beta\n      gamma")?;
        let mut editor = session.open_file("home-toggle", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 9), (1, 6), (2, 11)])?;

        editor.keys("<home>")?;
        editor.expect_cursor_heads(&[(0, 4), (1, 2), (2, 6)])?;

        editor.keys("<home>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<home>")?;
        editor.expect_cursor_heads(&[(0, 4), (1, 2), (2, 6)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_home_preserves_each_anchor_while_heads_toggle_home_targets() -> TestResult {
    support::run_x11_test("multi-cursor-shift-home-anchor", |session| {
        let path = session.seed_file("shift-home-anchor.txt", "    aa\n  bbbb\n    cc")?;
        let mut editor = session.open_file("shift-home-anchor", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 6), (1, 6), (2, 6)])?;

        editor.keys("<S-home>")?;
        editor.expect_selections(&[((0, 6), (0, 4)), ((1, 6), (1, 2)), ((2, 6), (2, 4))])?;

        editor.keys("<S-home>")?;
        editor.expect_selections(&[((0, 6), (0, 0)), ((1, 6), (1, 0)), ((2, 6), (2, 0))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_end_extends_each_cursor_to_own_line_end() -> TestResult {
    support::run_x11_test("multi-cursor-shift-end", |session| {
        let path = session.seed_file("shift-end.txt", "alpha\nbeta\ncharlie")?;
        let mut editor = session.open_file("shift-end", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;

        editor.keys("<S-end>")?;
        editor.expect_selections(&[((0, 2), (0, 5)), ((1, 2), (1, 4)), ((2, 2), (2, 7))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_left_after_shift_right_shrinks_each_selection_from_head() -> TestResult {
    support::run_x11_test("multi-cursor-shift-left-shrink", |session| {
        let path = session.seed_file("shift-left-shrink.txt", "abc\nabc\nabc")?;
        let mut editor = session.open_file("shift-left-shrink", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<S-right><S-right>")?;
        editor.expect_selections(&[((0, 0), (0, 2)), ((1, 0), (1, 2)), ((2, 0), (2, 2))])?;

        editor.keys("<S-left>")?;
        editor.expect_selections(&[((0, 0), (0, 1)), ((1, 0), (1, 1)), ((2, 0), (2, 1))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_right_moves_and_ctrl_shift_right_extends_every_cursor_by_word() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-right", |session| {
        let path = session.seed_file("ctrl-right.txt", "aa bb\naa bb\naa bb")?;
        let mut editor = session.open_file("ctrl-right", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-right>")?;
        editor.expect_selections(&[((0, 2), (0, 2)), ((1, 2), (1, 2)), ((2, 2), (2, 2))])?;

        editor.keys("<C-S-right>")?;
        editor.expect_selections(&[((0, 2), (0, 5)), ((1, 2), (1, 5)), ((2, 2), (2, 5))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_right_expands_and_shift_alt_left_shrinks_each_selection() -> TestResult {
    support::run_x11_test("multi-cursor-smart-select", |session| {
        let path = session.seed_file("smart-select.txt", "(foo)\n(foo)")?;
        let mut editor = session.open_file("smart-select", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><C-S-l>")?;
        editor.expect_selection_ranges(&[(1, 4), (7, 10)])?;

        // Like VS Code, expanding `foo` inside `(foo)` selects the whole
        // parenthesised expression.
        editor.keys("<S-A-right>")?;
        editor.expect_selection_ranges(&[(0, 5), (6, 11)])?;

        editor.keys("<S-A-left>")?;
        editor.expect_selection_ranges(&[(1, 4), (7, 10)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn down_moves_each_cursor_one_line_and_coalesces_duplicate_edge_targets() -> TestResult {
    support::run_x11_test("multi-cursor-down-coalesce-edge", |session| {
        let path = session.seed_file("down-coalesce-edge.txt", "aa\nb")?;
        let mut editor = session.open_file("down-coalesce-edge", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 1)])?;

        editor.keys("<down>")?;
        editor.expect_cursor_heads(&[(1, 1)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vertical_motion_keeps_each_cursor_preferred_column() -> TestResult {
    support::run_x11_test("multi-cursor-preferred-column", |session| {
        let path = session.seed_file("preferred.txt", "abcdefghij\nshort\nabcdefghij\nabcdefghij")?;
        let mut editor = session.open_file("preferred", &path)?;

        editor.keys(&format!("<C-home>{}", "<right>".repeat(8)))?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        // Ctrl+Alt+Down adds a cursor below each cursor at the goal column,
        // clamped to the short middle line.
        editor.keys("<C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 8), (1, 5)])?;

        // Moving down puts the first cursor at the end of the short line
        // (char 16), where it is drawn and reported at its goal column 8;
        // the second cursor keeps column 8 on the long line below.
        editor.keys("<down>")?;
        let clamped = editor.expect_cursor_heads(&[(1, 8), (2, 8)])?;
        assert_eq!(clamped.cursors[0].head_char, 16, "{clamped:?}");
        // Moving down again lands the first cursor on its goal column 8.
        editor.keys("<down>")?;
        editor.expect_cursor_heads(&[(2, 8), (3, 8)])?;
        editor.keys("<up><up>")?;
        editor.expect_cursor_heads(&[(0, 8), (1, 8)])?;
        Ok(())
    })
}
