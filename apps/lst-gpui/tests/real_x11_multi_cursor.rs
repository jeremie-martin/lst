//! Accepted real-display behavior for creating and removing cursors in
//! standard mode: occurrence selection, select-all-occurrences, adjacent-line
//! cursors, line-end cursors, column drags, and Escape. Linux shortcuts follow
//! VS Code conventions where `lst` implements the same workflow.
//!
//! Moving existing cursors lives in `real_x11_multi_cursor_motion`; editing at
//! every cursor lives in `real_x11_multi_cursor_editing`.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_multi_cursor --run-ignored only

mod support;

use lst_x11_harness::ChordMods;

use support::{secs, EditorTestExt, SelectionTestExt, TestResult};

const ALT_SHIFT: ChordMods = ChordMods {
    ctrl: false,
    alt: true,
    shift: true,
    platform: false,
};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_adds_occurrences_and_literal_input_replaces_them() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-d", |session| {
        let path = session.seed_file("ctrl-d.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_file("ctrl-d", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-d>")?;
        editor.expect_selection_ranges(&[(0, 3)])?;
        editor.keys("<C-d><C-d>")?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11), (16, 19)])?;

        editor.keys("qux")?;
        editor.save_then_expect_file(&path, "qux bar qux baz qux")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_uses_explicit_selection_text_not_word_boundaries() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-d-selection-text", |session| {
        let path = session.seed_file("ctrl-d-selection.txt", "ab xx ab yy ab xx")?;
        let mut editor = session.open_file("ctrl-d-selection", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys(&"<S-right>".repeat(5))?;
        editor.expect_selection_ranges(&[(0, 5)])?;
        editor.keys("<C-d>")?;
        editor.expect_selection_ranges(&[(0, 5), (12, 17)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_k_ctrl_d_skips_current_occurrence_and_adds_the_next() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-k-ctrl-d", |session| {
        let path = session.seed_file("ctrl-k-ctrl-d.txt", "foo foo foo")?;
        let mut editor = session.open_file("ctrl-k-ctrl-d", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-d><C-d>")?;
        editor.expect_selection_ranges(&[(0, 3), (4, 7)])?;
        editor.keys("<C-k><C-d>")?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11)])?;

        editor.keys("bar")?;
        editor.save_then_expect_file(&path, "bar foo bar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_u_pops_last_added_occurrence_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-u-pop", |session| {
        let path = session.seed_file("ctrl-u-pop.txt", "foo foo foo")?;
        let mut editor = session.open_file("ctrl-u-pop", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-d><C-d><C-d>")?;
        editor.expect_selection_ranges(&[(0, 3), (4, 7), (8, 11)])?;
        editor.keys("<C-u>")?;
        editor.expect_selection_ranges(&[(0, 3), (4, 7)])?;

        editor.keys("bar")?;
        editor.save_then_expect_file(&path, "bar bar foo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_l_selects_all_occurrences_and_replaces_them() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-shift-l", |session| {
        let path = session.seed_file("ctrl-shift-l.txt", "foo bar foo\nfoo baz")?;
        let mut editor = session.open_file("ctrl-shift-l", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11), (12, 15)])?;
        editor.wait_state("status bar cursor count", secs(5), |record| {
            record.status_bar.contains("3 cursors")
        })?;

        editor.keys("qux")?;
        editor.save_then_expect_file(&path, "qux bar qux\nqux baz")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_l_uses_explicit_selection_text_not_word_boundaries() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-shift-l-selection-text", |session| {
        let path = session.seed_file("ctrl-shift-l-selection.txt", "ab xx ab yy ab xx")?;
        let mut editor = session.open_file("ctrl-shift-l-selection", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys(&"<S-right>".repeat(5))?;
        editor.expect_selection_ranges(&[(0, 5)])?;
        editor.keys("<C-S-l>")?;
        editor.expect_selection_ranges(&[(0, 5), (12, 17)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_f2_selects_all_occurrences_of_current_word() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-f2", |session| {
        let path = session.seed_file("ctrl-f2.txt", "foo bar foo\nfoo baz")?;
        let mut editor = session.open_file("ctrl-f2", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-f2>")?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11), (12, 15)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_enter_selects_all_current_find_matches() -> TestResult {
    support::run_x11_test("multi-cursor-find-alt-enter", |session| {
        let path = session.seed_file("find-alt-enter.txt", "foo bar foo\nfoo baz")?;
        let mut editor = session.open_file("find-alt-enter", &path)?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus", secs(2), |record| {
            record.focused_input == "find_query"
        })?;
        editor.keys("foo")?;
        editor.expect_find_state("foo", 3)?;

        editor.keys("<A-enter>")?;
        editor.wait_state("editor focus after Alt-Enter", secs(5), |record| {
            record.focused_input == "editor"
        })?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11), (12, 15)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_down_adds_adjacent_line_cursors_for_literal_input() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-alt-down", |session| {
        let path = session.seed_file("columns.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("columns", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_selections(&[((0, 0), (0, 0)), ((1, 0), (1, 0)), ((2, 0), (2, 0))])?;

        editor.keys("X")?;
        editor.save_then_expect_file(&path, "Xalpha\nXbeta\nXgamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_up_adds_adjacent_line_cursors_above() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-alt-up", |session| {
        let path = session.seed_file("ctrl-alt-up.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("ctrl-alt-up", &path)?;

        editor.keys("<C-home><down><down>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;

        editor.keys("<C-A-up><C-A-up>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_down_clamps_added_cursors_to_short_line_ends() -> TestResult {
    support::run_x11_test("multi-cursor-short-line-clamp", |session| {
        let path = session.seed_file("short-line-clamp.txt", "abcdef\nx\nabcdef")?;
        let mut editor = session.open_file("short-line-clamp", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right>")?;
        editor.expect_cursor_heads(&[(0, 4)])?;

        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 4), (1, 1), (2, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_down_stops_at_document_end_without_duplicate_cursors() -> TestResult {
    support::run_x11_test("multi-cursor-boundary", |session| {
        let path = session.seed_file("boundary.txt", "alpha\nbeta")?;
        let mut editor = session.open_file("boundary", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_shift_down_and_up_add_adjacent_line_cursors() -> TestResult {
    support::run_x11_test("multi-cursor-alt-shift-arrows", |session| {
        let path = session.seed_file("alt-shift-arrows.txt", "aaa\nbbb\nccc")?;
        let mut editor = session.open_file("alt-shift-arrows", &path)?;

        editor.keys("<C-home><down>")?;
        editor.expect_cursor_heads(&[(1, 0)])?;
        editor.keys("<A-S-down>")?;
        editor.expect_cursor_heads(&[(1, 0), (2, 0)])?;
        editor.keys("<A-S-up>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("x")?;
        editor.save_then_expect_file(&path, "xaaa\nxbbb\nxccc")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_i_adds_cursor_at_end_of_each_selected_line() -> TestResult {
    support::run_x11_test("multi-cursor-shift-alt-i", |session| {
        let path = session.seed_file("shift-alt-i.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("shift-alt-i", &path)?;

        editor.keys("<C-a><A-S-i>")?;
        editor.expect_selections(&[((0, 5), (0, 5)), ((1, 4), (1, 4)), ((2, 5), (2, 5))])?;

        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alphaX\nbetaX\ngammaX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn column_selection_inserts_text_on_each_touched_line() -> TestResult {
    support::run_x11_test("multi-cursor-column-insert", |session| {
        let path = session.seed_file("column-insert.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("column-insert", &path)?;

        editor.drag_text((0, 2), (2, 2), ALT_SHIFT)?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;

        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alXpha\nbrXavo\nchXarlie")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn escape_collapses_selections_then_drops_secondary_cursors() -> TestResult {
    support::run_x11_test("multi-cursor-escape", |session| {
        let path = session.seed_file("escape.txt", "foo foo foo")?;
        let mut editor = session.open_file("escape", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.expect_selection_ranges(&[(0, 3), (4, 7), (8, 11)])?;

        editor.keys("<esc>")?;
        editor.expect_selections(&[((0, 3), (0, 3)), ((0, 7), (0, 7)), ((0, 11), (0, 11))])?;

        editor.keys("<esc>")?;
        editor.expect_cursor_heads(&[(0, 3)])?;
        Ok(())
    })
}
