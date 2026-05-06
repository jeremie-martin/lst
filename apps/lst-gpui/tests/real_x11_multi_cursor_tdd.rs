//! Under-review executable specs for multi-cursor and multi-selection behavior.
//!
//! Specs in this file are product decisions first. They run in the `x11-tdd`
//! profile while the behavior is being discussed or implemented. Once accepted
//! and green, move them into the blocking real-display suite.

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, ChordMods, Selection};

use support::{EditorTestExt, TestResult};

fn anchor_head_positions(
    record: &lst_x11_harness::StateTraceRecord,
) -> Vec<((usize, usize), (usize, usize))> {
    record
        .cursors
        .iter()
        .map(|cursor| (cursor.anchor_pos(), cursor.head_pos()))
        .collect()
}

fn selection_widths(record: &lst_x11_harness::StateTraceRecord) -> Vec<usize> {
    record
        .cursors
        .iter()
        .map(|cursor| cursor.head_char.abs_diff(cursor.anchor_char))
        .collect()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn end_moves_each_cursor_to_own_line_end() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-end-line-end", |session| {
        let path = session.seed_file("end-line-end.txt", "a\nabcd\nabcdef")?;
        let mut editor = session.open_file("end-line-end", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<end>")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 4), (2, 6)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn home_toggles_each_cursor_between_first_non_blank_and_column_zero() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-home-toggle", |session| {
        let path = session.seed_file("home-toggle.txt", "    alpha\n  beta\n      gamma")?;
        let mut editor = session.open_file("home-toggle", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><S-A-down><S-A-down>")?;

        editor.keys("<home>")?;
        editor.expect_cursor_heads(&[(0, 4), (1, 2), (2, 6)])?;

        editor.keys("<home>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_home_preserves_each_anchor_while_heads_toggle_home_targets() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-home-anchor", |session| {
        let path = session.seed_file("shift-home-anchor.txt", "    aa\n  bbbb\n    cc")?;
        let mut editor = session.open_file("shift-home-anchor", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 6), (1, 6), (2, 6)])?;

        editor.keys("<S-home>")?;
        let first = editor.read_state()?;
        assert_eq!(
            anchor_head_positions(&first),
            vec![((0, 6), (0, 4)), ((1, 6), (1, 2)), ((2, 6), (2, 4))],
            "{first:?}"
        );

        editor.keys("<S-home>")?;
        let second = editor.read_state()?;
        assert_eq!(
            anchor_head_positions(&second),
            vec![((0, 6), (0, 0)), ((1, 6), (1, 0)), ((2, 6), (2, 0))],
            "{second:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_end_extends_each_cursor_to_own_line_end() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-end", |session| {
        let path = session.seed_file("shift-end.txt", "alpha\nbeta\ncharlie")?;
        let mut editor = session.open_file("shift-end", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;

        editor.keys("<S-end>")?;
        let record = editor.read_state()?;
        assert_eq!(
            anchor_head_positions(&record),
            vec![((0, 2), (0, 5)), ((1, 2), (1, 4)), ((2, 2), (2, 7))],
            "{record:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_right_extends_each_selection_to_next_word_boundary() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-shift-right", |session| {
        let path = session.seed_file("ctrl-shift-right.txt", "aa bb\naa bb\naa bb")?;
        let mut editor = session.open_file("ctrl-shift-right", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-S-right>")?;
        let record = editor.read_state()?;
        assert_eq!(
            anchor_head_positions(&record),
            vec![((0, 0), (0, 2)), ((1, 0), (1, 2)), ((2, 0), (2, 2))],
            "{record:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_left_after_shift_right_shrinks_each_selection_from_head() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-shift-left-shrink", |session| {
        let path = session.seed_file("shift-left-shrink.txt", "abc\nabc\nabc")?;
        let mut editor = session.open_file("shift-left-shrink", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down><S-right><S-right>")?;
        let expanded = editor.read_state()?;
        assert_eq!(selection_widths(&expanded), vec![2, 2, 2], "{expanded:?}");

        editor.keys("<S-left>")?;
        let shrunk = editor.read_state()?;
        assert_eq!(
            anchor_head_positions(&shrunk),
            vec![((0, 0), (0, 1)), ((1, 0), (1, 1)), ((2, 0), (2, 1))],
            "{shrunk:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn down_moves_each_cursor_one_line_and_coalesces_duplicate_edge_targets() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-down-coalesce-edge", |session| {
        let path = session.seed_file("down-coalesce-edge.txt", "aa\nb")?;
        let mut editor = session.open_file("down-coalesce-edge", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 1)])?;

        editor.keys("<down>")?;
        editor.expect_cursor_heads(&[(1, 1)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_deletes_full_words_before_each_cursor_in_one_undo_step() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-backspace-full-word", |session| {
        let original = "alpha camelCase\nbravo snake_case";
        let path = session.seed_file("ctrl-backspace-full-word.txt", original)?;
        let mut editor = session.open_file("ctrl-backspace-full-word", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 15), (1, 16)])?;

        editor.keys("<C-backspace>")?;
        editor.save_then_expect_file(&path, "alpha \nbravo ")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_deletes_full_words_after_each_cursor_in_one_undo_step() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-delete-full-word", |session| {
        let original = "camelCase alpha\nsnake_case bravo";
        let path = session.seed_file("ctrl-delete-full-word.txt", original)?;
        let mut editor = session.open_file("ctrl-delete-full-word", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, " alpha\n bravo")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_deletes_single_space_and_following_word() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-delete-single-space", |session| {
        let original = "foo bar\nbaz qux";
        let path = session.seed_file("ctrl-delete-single-space.txt", original)?;
        let mut editor = session.open_file("ctrl-delete-single-space", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 3), (1, 3)])?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, "foo\nbaz")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_deletes_trailing_whitespace_before_previous_word() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-backspace-whitespace", |session| {
        let original = "foo bar   \nbaz qux   ";
        let path = session.seed_file("ctrl-backspace-whitespace.txt", original)?;
        let mut editor = session.open_file("ctrl-backspace-whitespace", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 10), (1, 10)])?;

        editor.keys("<C-backspace>")?;
        editor.save_then_expect_file(&path, "foo bar\nbaz qux")?;

        editor.keys("<C-backspace>")?;
        editor.save_then_expect_file(&path, "foo \nbaz ")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "foo bar\nbaz qux")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_deletes_leading_whitespace_before_next_word() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-delete-whitespace", |session| {
        let original = "foo   bar\nbaz   qux";
        let path = session.seed_file("ctrl-delete-whitespace.txt", original)?;
        let mut editor = session.open_file("ctrl-delete-whitespace", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 3), (1, 3)])?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, "foobar\nbazqux")?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, "foo\nbaz")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "foobar\nbazqux")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_treats_word_separators_as_boundaries() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-backspace-separators", |session| {
        let path = session.seed_file("ctrl-backspace-separators.txt", "foo.bar\nzip.qux")?;
        let mut editor = session.open_file("ctrl-backspace-separators", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 7), (1, 7)])?;

        editor.keys("<C-backspace>")?;
        editor.save_then_expect_file(&path, "foo.\nzip.")?;

        editor.keys("<C-backspace>")?;
        editor.save_then_expect_file(&path, "foo\nzip")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_treats_word_separators_as_boundaries() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-delete-separators", |session| {
        let path = session.seed_file("ctrl-delete-separators.txt", "foo.bar\nzip.qux")?;
        let mut editor = session.open_file("ctrl-delete-separators", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, ".bar\n.qux")?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, "bar\nqux")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_at_line_start_joins_previous_line_for_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-backspace-line-start", |session| {
        let original = "a\nb\nc\nd";
        let path = session.seed_file("ctrl-backspace-line-start.txt", original)?;
        let mut editor = session.open_file("ctrl-backspace-line-start", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down><S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(1, 0), (2, 0), (3, 0)])?;

        editor.keys("<C-backspace>")?;
        editor.save_then_expect_file(&path, "abcd")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_at_line_end_joins_to_next_word_for_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-ctrl-delete-line-end", |session| {
        let original = "a\n  b\nc\n  d";
        let path = session.seed_file("ctrl-delete-line-end.txt", original)?;
        let mut editor = session.open_file("ctrl-delete-line-end", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down><end>")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 3), (2, 1)])?;

        editor.keys("<C-delete>")?;
        editor.save_then_expect_file(&path, "abcd")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn backspace_at_line_start_joins_previous_line_for_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-backspace-line-start", |session| {
        let path = session.seed_file("backspace-line-start.txt", "a\nb\nc\nd")?;
        let mut editor = session.open_file("backspace-line-start", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down><S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(1, 0), (2, 0), (3, 0)])?;

        editor.keys("<bs>")?;
        editor.save_then_expect_file(&path, "abcd")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn delete_at_line_end_joins_next_line_for_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-delete-line-end", |session| {
        let path = session.seed_file("delete-line-end.txt", "a\nb\nc\nd")?;
        let mut editor = session.open_file("delete-line-end", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 1), (2, 1)])?;

        editor.keys("<delete>")?;
        editor.save_then_expect_file(&path, "abcd")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn auto_pair_inserts_matching_pair_at_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-auto-pair-insert", |session| {
        let path = session.seed_file("auto-pair-insert.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("auto-pair-insert", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("(")?;
        editor.save_then_expect_file(&path, "()alpha\n()beta\n()gamma")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 1), (2, 1)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn auto_pair_surrounds_each_non_empty_selection() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-auto-pair-surround", |session| {
        let path = session.seed_file("auto-pair-surround.txt", "foo\nfoo")?;
        let mut editor = session.open_file("auto-pair-surround", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        let selected = editor.read_state()?;
        assert_eq!(selection_widths(&selected), vec![3, 3], "{selected:?}");

        editor.keys("(")?;
        editor.save_then_expect_file(&path, "(foo)\n(foo)")?;
        let surrounded = editor.read_state()?;
        assert_eq!(selection_widths(&surrounded), vec![3, 3], "{surrounded:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_mismatched_multiline_clipboard_broadcasts_whole_text_to_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-paste-mismatch-broadcast", |session| {
        let path = session.seed_file("paste-mismatch-broadcast.txt", "A\nB")?;
        let mut editor = session.open_file("paste-mismatch-broadcast", &path)?;

        write_clipboard_text(Selection::Clipboard, "x\ny\nz")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;

        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "x\ny\nzA\nx\ny\nzB")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_distribute_is_one_undo_step() -> TestResult {
    support::run_x11_test("multi-cursor-tdd-paste-distribute-undo", |session| {
        let path = session.seed_file("paste-distribute-undo.txt", "A\nB\nC")?;
        let mut editor = session.open_file("paste-distribute-undo", &path)?;

        write_clipboard_text(Selection::Clipboard, "red\ngreen\nblue")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "redA\ngreenB\nblueC")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "A\nB\nC")?;
        Ok(())
    })
}

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
