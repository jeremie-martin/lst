//! Real-display tests for the multi-cursor surface. These assertions follow
//! `docs/editor-behaviors-checklist.md` as the behavior spec; some are
//! intentionally ahead of the current implementation. The spec target is
//! VS Code's default Linux behavior.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_multi_cursor --run-ignored only

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Key, KeyChord, Selection,
};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_adds_occurrences_and_literal_input_replaces_them() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-d", |session| {
        let path = session.seed_file("ctrl-d.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_file("ctrl-d", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-d><C-d><C-d>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 3, "{:?}", record.cursors);

        editor.keys("qux")?;
        editor.save_then_expect_file(&path, "qux bar qux baz qux")?;
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
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 3, "{:?}", record.cursors);

        editor.keys("qux")?;
        editor.save_then_expect_file(&path, "qux bar qux\nqux baz")?;
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
        let record = editor.wait_state("Ctrl-F2 selections", secs(5), |record| {
            record.cursors.len() == 3
                && record.cursors.iter().all(|cursor| {
                    cursor.head_char.max(cursor.anchor_char) - cursor.head_char.min(cursor.anchor_char) == 3
                })
        })?;
        assert_eq!(record.cursors.len(), 3, "{record:?}");
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
        let record = editor.wait_state("Alt-Enter find selections", secs(5), |record| {
            record.cursors.len() == 3
                && record.cursors.iter().all(|cursor| {
                    cursor.head_char.max(cursor.anchor_char) - cursor.head_char.min(cursor.anchor_char) == 3
                })
        })?;
        assert_eq!(record.cursors.len(), 3, "{record:?}");
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
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("X")?;
        editor.save_then_expect_file(&path, "Xalpha\nXbeta\nXgamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn backspace_deletes_before_every_cursor_added_by_adjacent_line_commands() -> TestResult {
    support::run_x11_test("multi-cursor-backspace", |session| {
        let path = session.seed_file("backspace.txt", "alpha\nbravo\ngamma")?;
        let mut editor = session.open_file("backspace", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 5), (1, 5), (2, 5)])?;

        editor.keys("<bs>")?;
        editor.save_then_expect_file(&path, "alph\nbrav\ngamm")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn delete_forward_deletes_after_every_cursor_added_by_adjacent_line_commands() -> TestResult {
    support::run_x11_test("multi-cursor-delete-forward", |session| {
        let path = session.seed_file("delete-forward.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("delete-forward", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<delete>")?;
        editor.save_then_expect_file(&path, "lpha\nravo\nharlie")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_distributes_clipboard_lines_to_matching_cursor_count() -> TestResult {
    support::run_x11_test("multi-cursor-paste-distribute", |session| {
        let path = session.seed_file("paste.txt", "A\nB\nC")?;
        let mut editor = session.open_file("paste", &path)?;

        write_clipboard_text(Selection::Clipboard, "red\ngreen\nblue")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "redA\ngreenB\nblueC")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn copy_collects_multi_selection_fragments_in_document_order() -> TestResult {
    support::run_x11_test("multi-cursor-copy-fragments", |session| {
        let path = session.seed_file("copy-fragments.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_file("copy-fragments", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.press(KeyChord::Ctrl(Key::Char('c')))?;
        wait_clipboard_text(Selection::Clipboard, "foo\nfoo\nfoo", secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn smart_enter_inherits_each_cursor_line_indent() -> TestResult {
    support::run_x11_test("multi-cursor-smart-enter", |session| {
        let path = session.seed_file("smart-enter.txt", "    alpha!\n        be")?;
        let mut editor = session.open_file("smart-enter", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 10), (1, 10)])?;

        editor.keys("<enter>")?;
        editor.save_then_expect_file(&path, "    alpha!\n    \n        be\n        ")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn escape_collapses_non_empty_multi_selections_to_cursors_before_input() -> TestResult {
    support::run_x11_test("multi-cursor-escape-collapse", |session| {
        let path = session.seed_file("escape-collapse.txt", "foo foo foo")?;
        let mut editor = session.open_file("escape-collapse", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        let with_selections = editor.read_state()?;
        assert_eq!(with_selections.cursors.len(), 3, "{with_selections:?}");

        editor.keys("<esc>X")?;
        editor.save_then_expect_file(&path, "fooX fooX fooX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn right_motion_moves_every_cursor_before_literal_input() -> TestResult {
    support::run_x11_test("multi-cursor-right-motion", |session| {
        let path = session.seed_file("right-motion.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("right-motion", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<right><right>X")?;
        editor.save_then_expect_file(&path, "alXpha\nbeXta\ngaXmma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn duplicate_line_applies_to_every_cursor_line() -> TestResult {
    support::run_x11_test("multi-cursor-duplicate-line", |session| {
        let path = session.seed_file("duplicate-line.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("duplicate-line", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;

        editor.keys("<C-S-A-down>")?;
        editor.save_then_expect_file(&path, "alpha\nalpha\nbeta\nbeta\ngamma")?;
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
        editor.keys("<C-d><C-d><C-k><C-d>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 2, "{record:?}");

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
        editor.keys("<C-d><C-d><C-d><C-u>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 2, "{record:?}");

        editor.keys("bar")?;
        editor.save_then_expect_file(&path, "bar bar foo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_i_adds_cursor_at_end_of_each_selected_line() -> TestResult {
    support::run_x11_test("multi-cursor-shift-alt-i", |session| {
        let path = session.seed_file("shift-alt-i.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("shift-alt-i", &path)?;

        editor.keys("<C-a><A-S-i>X")?;
        editor.save_then_expect_file(&path, "alphaX\nbetaX\ngammaX")?;
        Ok(())
    })
}

// Sibling state-only siblings of the conflated create+edit tests above.
// Each one stops just before the literal input that the original test
// uses to *observe* the cursor set, and asserts the cursor set directly
// through the state-trace channel. When a regression breaks cursor
// creation, this test fails — and it fails *for* the cursor regression,
// not because the multi-cursor edit path also happens to be broken.

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_down_adds_three_cursors_aligned_on_column_zero() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-alt-down-state", |session| {
        let path = session.seed_file("columns-state.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("columns-state", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        let record = editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;
        assert!(
            record.cursors.iter().all(|c| c.is_collapsed()),
            "all cursors should be collapsed (no selection): {:?}",
            record.cursors
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn right_motion_advances_every_cursor_independently() -> TestResult {
    support::run_x11_test("multi-cursor-right-motion-state", |session| {
        let path = session.seed_file("right-motion-state.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("right-motion-state", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<right><right>")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_grows_selection_set_to_three_occurrences_of_foo() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-d-state", |session| {
        let path = session.seed_file("ctrl-d-state.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_file("ctrl-d-state", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-d><C-d><C-d>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 3, "{:?}", record.cursors);
        for (idx, cursor) in record.cursors.iter().enumerate() {
            let span = cursor.head_char.max(cursor.anchor_char) - cursor.head_char.min(cursor.anchor_char);
            assert_eq!(span, 3, "cursor #{idx} should cover 3 chars (\"foo\"); got {cursor:?}");
        }
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_l_creates_one_selection_per_occurrence_via_state() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-shift-l-state", |session| {
        let path = session.seed_file("ctrl-shift-l-state.txt", "foo bar foo\nfoo baz")?;
        let mut editor = session.open_file("ctrl-shift-l-state", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 3, "{:?}", record.cursors);
        for cursor in &record.cursors {
            let span = cursor.head_char.max(cursor.anchor_char) - cursor.head_char.min(cursor.anchor_char);
            assert_eq!(span, 3, "{cursor:?} should select 3 chars (\"foo\")");
        }
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn escape_collapses_selections_then_drops_secondary_cursors_via_state() -> TestResult {
    support::run_x11_test("multi-cursor-escape-state", |session| {
        let path = session.seed_file("escape-state.txt", "foo foo foo")?;
        let mut editor = session.open_file("escape-state", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        let with_selections = editor.read_state()?;
        assert_eq!(with_selections.cursors.len(), 3, "{with_selections:?}");
        assert!(
            !with_selections.cursors.iter().all(|c| c.is_collapsed()),
            "selections should be non-empty before first Esc"
        );

        editor.keys("<esc>")?;
        let after_first_esc = editor.read_state()?;
        assert_eq!(
            after_first_esc.cursors.len(),
            3,
            "first Esc should keep 3 cursors but collapse selections: {after_first_esc:?}"
        );
        assert!(
            after_first_esc.cursors.iter().all(|c| c.is_collapsed()),
            "first Esc should collapse selections: {after_first_esc:?}"
        );

        editor.keys("<esc>")?;
        let after_second_esc = editor.read_state()?;
        assert_eq!(
            after_second_esc.cursors.len(),
            1,
            "second Esc should drop secondary cursors: {after_second_esc:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_distribute_preserves_cursor_count_after_insertion() -> TestResult {
    support::run_x11_test("multi-cursor-paste-state", |session| {
        let path = session.seed_file("paste-state.txt", "A\nB\nC")?;
        let mut editor = session.open_file("paste-state", &path)?;

        write_clipboard_text(Selection::Clipboard, "red\ngreen\nblue")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-v>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 3, "{record:?}");
        // Heads should land at the end of each pasted fragment.
        let head_cols: Vec<usize> = record.cursors.iter().map(|c| c.head_col).collect();
        assert_eq!(head_cols, vec![3, 5, 4], "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn smart_enter_per_cursor_indent_lands_each_cursor_at_inherited_column() -> TestResult {
    support::run_x11_test("multi-cursor-smart-enter-state", |session| {
        let path = session.seed_file("smart-enter-state.txt", "    alpha!\n        be")?;
        let mut editor = session.open_file("smart-enter-state", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 10), (1, 10)])?;

        editor.keys("<enter>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 2, "{record:?}");
        // Cursor 0 was on the 4-space-indented line and should land at col 4
        // on the new line below it; cursor 1 on the 8-space-indented line
        // should land at col 8.
        let head_cols: Vec<usize> = record.cursors.iter().map(|c| c.head_col).collect();
        assert_eq!(head_cols, vec![4, 8], "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_shift_down_adds_adjacent_line_cursors() -> TestResult {
    support::run_x11_test("multi-cursor-alt-shift-down", |session| {
        let path = session.seed_file("alt-shift-down.txt", "aaa\nbbb\nccc")?;
        let mut editor = session.open_file("alt-shift-down", &path)?;

        // alt-shift-down stacks a cursor onto each successive line below.
        editor.place_cursor_at_document_start()?;
        editor.keys("<A-S-down><A-S-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("x")?;
        editor.save_then_expect_file(&path, "xaaa\nxbbb\nxccc")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_shift_up_adds_adjacent_line_cursors() -> TestResult {
    support::run_x11_test("multi-cursor-alt-shift-up", |session| {
        let path = session.seed_file("alt-shift-up.txt", "aaa\nbbb\nccc")?;
        let mut editor = session.open_file("alt-shift-up", &path)?;

        // alt-shift-up mirrors the behavior upward from the bottom line.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down><A-S-up><A-S-up>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("x")?;
        editor.save_then_expect_file(&path, "xaaa\nxbbb\nxccc")?;
        Ok(())
    })
}
