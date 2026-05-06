//! Real-display specs for multi-cursor and multi-selection edge cases. These
//! tests describe accepted product behavior and run in the blocking `x11`
//! profile.
//!
//! The target keyboard behavior is VS Code's default Linux behavior.

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    ChordMods, Key, KeyChord, Selection,
};

use support::{secs, EditorTestExt, TestResult};

fn cursor_ranges(record: &lst_x11_harness::StateTraceRecord) -> Vec<(usize, usize)> {
    record
        .cursors
        .iter()
        .map(|cursor| {
            (
                cursor.anchor_char.min(cursor.head_char),
                cursor.anchor_char.max(cursor.head_char),
            )
        })
        .collect()
}

fn selection_widths(record: &lst_x11_harness::StateTraceRecord) -> Vec<usize> {
    cursor_ranges(record)
        .into_iter()
        .map(|(start, end)| end - start)
        .collect()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_up_adds_adjacent_line_cursors_above() -> TestResult {
    support::run_x11_test("multi-cursor-spec-shift-alt-up", |session| {
        let path = session.seed_file("shift-alt-up.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("shift-alt-up", &path)?;

        editor.keys("<C-home><down><down>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;

        editor.keys("<S-A-up><S-A-up>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_down_clamps_added_cursors_to_short_line_ends() -> TestResult {
    support::run_x11_test("multi-cursor-spec-short-line-clamp", |session| {
        let path = session.seed_file("short-line-clamp.txt", "abcdef\nx\nabcdef")?;
        let mut editor = session.open_file("short-line-clamp", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right>")?;
        editor.expect_cursor_heads(&[(0, 4)])?;

        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 4), (1, 1), (2, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_down_stops_at_document_end_without_duplicate_cursors() -> TestResult {
    support::run_x11_test("multi-cursor-spec-boundary", |session| {
        let path = session.seed_file("boundary.txt", "alpha\nbeta")?;
        let mut editor = session.open_file("boundary", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_uses_explicit_selection_text_not_word_boundaries() -> TestResult {
    support::run_x11_test("multi-cursor-spec-ctrl-d-selection-text", |session| {
        let path = session.seed_file("ctrl-d-selection.txt", "ab xx ab yy ab xx")?;
        let mut editor = session.open_file("ctrl-d-selection", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-right><S-right><S-right><S-right><S-right><C-d>")?;

        let record = editor.read_state()?;
        assert_eq!(cursor_ranges(&record), vec![(0, 5), (12, 17)], "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_l_uses_explicit_selection_text_not_word_boundaries() -> TestResult {
    support::run_x11_test("multi-cursor-spec-ctrl-shift-l-selection-text", |session| {
        let path = session.seed_file("ctrl-shift-l-selection.txt", "ab xx ab yy ab xx")?;
        let mut editor = session.open_file("ctrl-shift-l-selection", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-right><S-right><S-right><S-right><S-right><C-S-l>")?;

        let record = editor.read_state()?;
        assert_eq!(cursor_ranges(&record), vec![(0, 5), (12, 17)], "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_right_extends_every_cursor_independently() -> TestResult {
    support::run_x11_test("multi-cursor-spec-shift-right", |session| {
        let path = session.seed_file("shift-right.txt", "xabc\nxdef\nxghi")?;
        let mut editor = session.open_file("shift-right", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<S-A-down><S-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<S-right>")?;
        let record = editor.read_state()?;
        assert_eq!(selection_widths(&record), vec![1, 1, 1], "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_right_moves_every_cursor_to_next_word_boundary() -> TestResult {
    support::run_x11_test("multi-cursor-spec-ctrl-right", |session| {
        let path = session.seed_file("ctrl-right.txt", "aa bb\naa bb\naa bb")?;
        let mut editor = session.open_file("ctrl-right", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc><home>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-right>")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_right_expands_and_shift_alt_left_shrinks_each_selection() -> TestResult {
    support::run_x11_test("multi-cursor-spec-smart-select", |session| {
        let path = session.seed_file("smart-select.txt", "(foo)\n(foo)")?;
        let mut editor = session.open_file("smart-select", &path)?;

        editor.keys("<right><C-S-l>")?;
        let baseline = editor.read_state()?;
        assert_eq!(selection_widths(&baseline), vec![3, 3], "{baseline:?}");

        editor.keys("<S-A-right>")?;
        let expanded = editor.read_state()?;
        // VS Code's smart-expand on `"foo"` inside `"(foo)"` lands on the
        // parenthesised expression, so each selection grows from 3 to 5
        // characters. A loose `> 3` check would also accept partial expansions
        // (just the trailing paren, just the leading paren) that don't match
        // VS Code, so pin the exact width here.
        assert_eq!(selection_widths(&expanded), vec![5, 5], "{expanded:?}");

        editor.keys("<S-A-left>")?;
        let shrunk = editor.read_state()?;
        assert_eq!(selection_widths(&shrunk), vec![3, 3], "{shrunk:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_single_clipboard_fragment_broadcasts_to_every_selection() -> TestResult {
    support::run_x11_test("multi-cursor-spec-paste-broadcast", |session| {
        let path = session.seed_file("paste-broadcast.txt", "foo\nfoo\nfoo")?;
        let mut editor = session.open_file("paste-broadcast", &path)?;

        write_clipboard_text(Selection::Clipboard, "bar")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><C-v>")?;
        editor.save_then_expect_file(&path, "bar\nbar\nbar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cut_collects_fragments_and_deletes_each_selection() -> TestResult {
    support::run_x11_test("multi-cursor-spec-cut-fragments", |session| {
        let path = session.seed_file("cut-fragments.txt", "foo bar foo")?;
        let mut editor = session.open_file("cut-fragments", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.press(KeyChord::Ctrl(Key::Char('x')))?;

        wait_clipboard_text(Selection::Clipboard, "foo\nfoo", secs(10))?;
        editor.save_then_expect_file(&path, " bar ")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn indent_and_outdent_coalesce_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-spec-indent-coalesce", |session| {
        let path = session.seed_file("indent-coalesce.rs", "foo foo")?;
        let mut editor = session.open_file("indent-coalesce", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        editor.expect_cursor_heads(&[(0, 3), (0, 7)])?;

        editor.keys("<C-]>")?;
        editor.save_then_expect_file(&path, "    foo foo")?;

        editor.keys("<C-[>")?;
        editor.save_then_expect_file(&path, "foo foo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn delete_line_coalesces_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-spec-delete-line-coalesce", |session| {
        let path = session.seed_file("delete-line-coalesce.txt", "keep\nfoo foo\nkeep2")?;
        let mut editor = session.open_file("delete-line-coalesce", &path)?;

        editor.keys("<C-home><down><C-S-l><esc>")?;
        editor.expect_cursor_heads(&[(1, 3), (1, 7)])?;

        editor.keys("<C-S-k>")?;
        editor.save_then_expect_file(&path, "keep\nkeep2")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn duplicate_line_coalesces_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-spec-duplicate-line-coalesce", |session| {
        let path = session.seed_file("duplicate-line-coalesce.txt", "foo foo\nbar")?;
        let mut editor = session.open_file("duplicate-line-coalesce", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        editor.expect_cursor_heads(&[(0, 3), (0, 7)])?;

        editor.keys("<C-S-A-down>")?;
        editor.save_then_expect_file(&path, "foo foo\nfoo foo\nbar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn move_line_down_moves_adjacent_cursor_lines_as_one_cluster() -> TestResult {
    support::run_x11_test("multi-cursor-spec-move-line-cluster", |session| {
        let path = session.seed_file("move-line-cluster.txt", "top\nfoo\nfoo\nbottom")?;
        let mut editor = session.open_file("move-line-cluster", &path)?;

        editor.keys("<C-home><down><C-S-l><esc>")?;
        editor.expect_cursor_heads(&[(1, 3), (2, 3)])?;

        editor.keys("<A-down>")?;
        editor.save_then_expect_file(&path, "top\nbottom\nfoo\nfoo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn toggle_line_comment_coalesces_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-spec-comment-coalesce", |session| {
        let path = session.seed_file("comment-coalesce.rs", "let foo = foo;")?;
        let mut editor = session.open_file("comment-coalesce", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right><C-S-l><esc>")?;
        editor.expect_cursor_heads(&[(0, 7), (0, 13)])?;

        editor.keys("<C-/>")?;
        editor.save_then_expect_file(&path, "// let foo = foo;")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_alt_drag_creates_column_cursor_set() -> TestResult {
    support::run_x11_test("multi-cursor-spec-column-drag", |session| {
        let path = session.seed_file("column-drag.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("column-drag", &path)?;

        editor.drag_text(
            (0, 2),
            (2, 2),
            ChordMods {
                ctrl: false,
                alt: true,
                shift: true,
            },
        )?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn column_selection_inserts_text_on_each_touched_line() -> TestResult {
    support::run_x11_test("multi-cursor-spec-column-insert", |session| {
        let path = session.seed_file("column-insert.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("column-insert", &path)?;

        editor.drag_text(
            (0, 2),
            (2, 2),
            ChordMods {
                ctrl: false,
                alt: true,
                shift: true,
            },
        )?;
        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alXpha\nbrXavo\nchXarlie")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn column_selection_backspace_deletes_before_each_touched_line() -> TestResult {
    support::run_x11_test("multi-cursor-spec-column-backspace", |session| {
        let path = session.seed_file("column-backspace.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("column-backspace", &path)?;

        editor.drag_text(
            (0, 2),
            (2, 2),
            ChordMods {
                ctrl: false,
                alt: true,
                shift: true,
            },
        )?;
        editor.keys("<bs>")?;
        editor.save_then_expect_file(&path, "apha\nbavo\ncarlie")?;
        Ok(())
    })
}
