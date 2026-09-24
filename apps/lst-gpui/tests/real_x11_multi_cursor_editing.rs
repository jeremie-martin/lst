//! Accepted real-X11 behavior for edits and clipboard commands applied at
//! every cursor of a multi-cursor set: deletion, line commands, indentation,
//! auto-pairs, paste distribution, copy and cut, and their undo steps.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_multi_cursor_editing --run-ignored only

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    ChordMods, Selection,
};

use support::{secs, EditorTestExt, SelectionSpan, SelectionTestExt, TestResult};

/// Seed `original`, build a cursor set from the document start with `setup`,
/// check that its heads land on `heads`, then press each step's keys and
/// save, expecting the step's file text.
fn run_edit_steps(
    label: &str,
    original: &str,
    setup: &str,
    heads: &[(usize, usize)],
    steps: &[(&str, &str)],
) -> TestResult {
    support::run_x11_test(label, |session| {
        let path = session.seed_file(&format!("{label}.txt"), original)?;
        let mut editor = session.open_file(label, &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys(setup)?;
        editor.expect_cursor_heads(heads)?;

        for (keys, expected) in steps {
            editor.keys(keys)?;
            editor.save_then_expect_file(&path, expected)?;
        }
        Ok(())
    })
}

/// Collapsed selections at `heads`, for checking that Escape dropped the
/// selections `Ctrl+Shift+L` made.
fn carets(heads: &[(usize, usize)]) -> Vec<SelectionSpan> {
    heads.iter().map(|&head| (head, head)).collect()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn backspace_deletes_before_every_cursor_added_by_adjacent_line_commands() -> TestResult {
    run_edit_steps(
        "multi-cursor-backspace",
        "alpha\nbravo\ngamma",
        "<end><C-A-down><C-A-down>",
        &[(0, 5), (1, 5), (2, 5)],
        &[("<bs>", "alph\nbrav\ngamm")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn delete_forward_deletes_after_every_cursor_added_by_adjacent_line_commands() -> TestResult {
    run_edit_steps(
        "multi-cursor-delete-forward",
        "alpha\nbravo\ncharlie",
        "<C-A-down><C-A-down>",
        &[(0, 0), (1, 0), (2, 0)],
        &[("<delete>", "lpha\nravo\nharlie")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_deletes_full_words_before_each_cursor_in_one_undo_step() -> TestResult {
    let original = "alpha camelCase\nbravo snake_case";
    run_edit_steps(
        "multi-cursor-ctrl-backspace-full-word",
        original,
        "<end><C-A-down>",
        &[(0, 15), (1, 16)],
        &[("<C-backspace>", "alpha \nbravo "), ("<C-z>", original)],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_deletes_full_words_after_each_cursor_in_one_undo_step() -> TestResult {
    let original = "camelCase alpha\nsnake_case bravo";
    run_edit_steps(
        "multi-cursor-ctrl-delete-full-word",
        original,
        "<C-A-down>",
        &[(0, 0), (1, 0)],
        &[("<C-delete>", " alpha\n bravo"), ("<C-z>", original)],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_deletes_single_space_and_following_word() -> TestResult {
    let original = "foo bar\nbaz qux";
    run_edit_steps(
        "multi-cursor-ctrl-delete-single-space",
        original,
        "<right><right><right><C-A-down>",
        &[(0, 3), (1, 3)],
        &[("<C-delete>", "foo\nbaz"), ("<C-z>", original)],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_deletes_trailing_whitespace_before_previous_word() -> TestResult {
    let original = "foo bar   \nbaz qux   ";
    run_edit_steps(
        "multi-cursor-ctrl-backspace-whitespace",
        original,
        "<end><C-A-down>",
        &[(0, 10), (1, 10)],
        &[
            ("<C-backspace>", "foo bar\nbaz qux"),
            ("<C-backspace>", "foo \nbaz "),
            ("<C-z>", "foo bar\nbaz qux"),
            ("<C-z>", original),
        ],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_deletes_leading_whitespace_before_next_word() -> TestResult {
    let original = "foo   bar\nbaz   qux";
    run_edit_steps(
        "multi-cursor-ctrl-delete-whitespace",
        original,
        "<right><right><right><C-A-down>",
        &[(0, 3), (1, 3)],
        &[
            ("<C-delete>", "foobar\nbazqux"),
            ("<C-delete>", "foo\nbaz"),
            ("<C-z>", "foobar\nbazqux"),
            ("<C-z>", original),
        ],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_treats_word_separators_as_boundaries() -> TestResult {
    run_edit_steps(
        "multi-cursor-ctrl-backspace-separators",
        "foo.bar\nzip.qux",
        "<end><C-A-down>",
        &[(0, 7), (1, 7)],
        &[("<C-backspace>", "foo.\nzip."), ("<C-backspace>", "foo\nzip")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_treats_word_separators_as_boundaries() -> TestResult {
    run_edit_steps(
        "multi-cursor-ctrl-delete-separators",
        "foo.bar\nzip.qux",
        "<C-A-down>",
        &[(0, 0), (1, 0)],
        &[("<C-delete>", ".bar\n.qux"), ("<C-delete>", "bar\nqux")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_backspace_at_line_start_joins_previous_line_for_each_cursor() -> TestResult {
    let original = "a\nb\nc\nd";
    run_edit_steps(
        "multi-cursor-ctrl-backspace-line-start",
        original,
        "<down><C-A-down><C-A-down>",
        &[(1, 0), (2, 0), (3, 0)],
        &[("<C-backspace>", "abcd"), ("<C-z>", original)],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_delete_at_line_end_joins_to_next_word_for_each_cursor() -> TestResult {
    let original = "a\n  b\nc\n  d";
    run_edit_steps(
        "multi-cursor-ctrl-delete-line-end",
        original,
        "<C-A-down><C-A-down><end>",
        &[(0, 1), (1, 3), (2, 1)],
        &[("<C-delete>", "abcd"), ("<C-z>", original)],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn backspace_at_line_start_joins_previous_line_for_each_cursor() -> TestResult {
    run_edit_steps(
        "multi-cursor-backspace-line-start",
        "a\nb\nc\nd",
        "<down><C-A-down><C-A-down>",
        &[(1, 0), (2, 0), (3, 0)],
        &[("<bs>", "abcd")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn delete_at_line_end_joins_next_line_for_each_cursor() -> TestResult {
    run_edit_steps(
        "multi-cursor-delete-line-end",
        "a\nb\nc\nd",
        "<end><C-A-down><C-A-down>",
        &[(0, 1), (1, 1), (2, 1)],
        &[("<delete>", "abcd")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn duplicate_line_applies_to_every_cursor_line() -> TestResult {
    run_edit_steps(
        "multi-cursor-duplicate-line",
        "alpha\nbeta\ngamma",
        "<C-A-down>",
        &[(0, 0), (1, 0)],
        &[("<C-S-A-down>", "alpha\nalpha\nbeta\nbeta\ngamma")],
    )
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn deleting_thousand_occurrences_keeps_cursors_and_undo_restores_text() -> TestResult {
    support::run_x11_test("multi-cursor-delete-thousand", |session| {
        let original = "selected rest\n".repeat(1_000);
        let path = session.seed_file("delete-thousand.txt", &original)?;
        let mut editor = session.open_file("delete-thousand", &path)?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><backspace>")?;
        editor.wait_state("a caret at the start of every line", secs(10), |record| {
            record.cursors.len() == 1_000
                && record
                    .cursors
                    .iter()
                    .enumerate()
                    .all(|(line, cursor)| cursor.anchor_pos() == (line, 0) && cursor.head_pos() == (line, 0))
        })?;
        editor.save_then_expect_file(&path, &" rest\n".repeat(1_000))?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, &original)?;
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
        editor.expect_cursor_heads(&[(1, 4), (3, 8)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn auto_pair_inserts_matching_pair_at_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-auto-pair-insert", |session| {
        let path = session.seed_file("auto-pair-insert.txt", " alpha\n beta\n gamma")?;
        let mut editor = session.open_file("auto-pair-insert", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("(")?;
        editor.save_then_expect_file(&path, "() alpha\n() beta\n() gamma")?;
        editor.expect_cursor_heads(&[(0, 1), (1, 1), (2, 1)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn auto_pair_surrounds_each_non_empty_selection() -> TestResult {
    support::run_x11_test("multi-cursor-auto-pair-surround", |session| {
        let path = session.seed_file("auto-pair-surround.txt", "foo\nfoo")?;
        let mut editor = session.open_file("auto-pair-surround", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.expect_selection_ranges(&[(0, 3), (4, 7)])?;

        editor.keys("(")?;
        editor.save_then_expect_file(&path, "(foo)\n(foo)")?;
        editor.expect_selection_ranges(&[(1, 4), (7, 10)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_distribute_is_one_undo_step_that_restores_the_cursor_set() -> TestResult {
    support::run_x11_test("multi-cursor-paste-distribute-undo", |session| {
        let path = session.seed_file("paste-distribute-undo.txt", "A\nB\nC")?;
        let mut editor = session.open_file("paste-distribute-undo", &path)?;

        write_clipboard_text(Selection::Clipboard, "red\ngreen\nblue")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "redA\ngreenB\nblueC")?;
        editor.expect_cursor_heads(&[(0, 3), (1, 5), (2, 4)])?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "A\nB\nC")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0), (2, 0)])?;

        editor.keys("<C-y>")?;
        editor.save_then_expect_file(&path, "redA\ngreenB\nblueC")?;
        editor.expect_cursor_heads(&[(0, 3), (1, 5), (2, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_mismatched_multiline_clipboard_broadcasts_whole_text_to_each_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-paste-mismatch-broadcast", |session| {
        let path = session.seed_file("paste-mismatch-broadcast.txt", "A\nB")?;
        let mut editor = session.open_file("paste-mismatch-broadcast", &path)?;

        write_clipboard_text(Selection::Clipboard, "x\ny\nz")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 0), (1, 0)])?;

        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "x\ny\nzA\nx\ny\nzB")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn paste_single_clipboard_fragment_broadcasts_to_every_selection() -> TestResult {
    support::run_x11_test("multi-cursor-paste-broadcast", |session| {
        let path = session.seed_file("paste-broadcast.txt", "foo\nfoo\nfoo")?;
        let mut editor = session.open_file("paste-broadcast", &path)?;

        write_clipboard_text(Selection::Clipboard, "bar")?;
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.expect_selection_ranges(&[(0, 3), (4, 7), (8, 11)])?;

        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "bar\nbar\nbar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn copy_collects_multi_selection_fragments_in_document_order() -> TestResult {
    support::run_x11_test("multi-cursor-copy-fragments", |session| {
        let path = session.seed_file("copy-fragments.txt", "one\ntwo\nthree")?;
        let mut editor = session.open_file("copy-fragments", &path)?;

        // Add the cursors bottom-up so creation order is the reverse of
        // document order.
        editor.keys("<C-end><C-A-up><C-A-up><S-home>")?;
        editor.expect_selections(&[((0, 3), (0, 0)), ((1, 3), (1, 0)), ((2, 5), (2, 0))])?;

        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "one\ntwo\nthree", secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cut_collects_fragments_and_deletes_each_selection() -> TestResult {
    support::run_x11_test("multi-cursor-cut-fragments", |session| {
        let path = session.seed_file("cut-fragments.txt", "foo bar foo")?;
        let mut editor = session.open_file("cut-fragments", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11)])?;

        editor.keys("<C-x>")?;
        wait_clipboard_text(Selection::Clipboard, "foo\nfoo", secs(10))?;
        editor.save_then_expect_file(&path, " bar ")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_outdents_each_cursor_line_in_one_undo_step() -> TestResult {
    support::run_x11_test("multi-cursor-shift-tab-each-line", |session| {
        let original = "    alpha\n    beta\n    gamma";
        let path = session.seed_file("shift-tab-each-line.txt", original)?;
        let mut editor = session.open_file("shift-tab-each-line", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right><right><right><C-A-down><C-A-down>")?;
        editor.expect_cursor_heads(&[(0, 6), (1, 6), (2, 6)])?;

        editor.keys("<S-tab>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_outdents_a_line_once_when_multiple_cursors_touch_it() -> TestResult {
    support::run_x11_test("multi-cursor-shift-tab-coalesce-line", |session| {
        let path = session.seed_file("shift-tab-coalesce-line.txt", "    foo foo")?;
        let mut editor = session.open_file("shift-tab-coalesce-line", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        editor.expect_selections(&carets(&[(0, 7), (0, 11)]))?;

        editor.keys("<S-tab>")?;
        editor.save_then_expect_file(&path, "foo foo")?;
        editor.expect_cursor_heads(&[(0, 3), (0, 7)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_tab_column_mode_outdents_only_lines_with_removable_indent() -> TestResult {
    support::run_x11_test("multi-cursor-shift-tab-column", |session| {
        let path = session.seed_file("shift-tab-column.txt", "    alpha\nbeta\n  gamma")?;
        let mut editor = session.open_file("shift-tab-column", &path)?;

        editor.drag_text(
            (0, 6),
            (2, 6),
            ChordMods {
                ctrl: false,
                alt: true,
                shift: true,
                platform: false,
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
fn indent_and_outdent_coalesce_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-indent-coalesce", |session| {
        let path = session.seed_file("indent-coalesce.rs", "foo foo")?;
        let mut editor = session.open_file("indent-coalesce", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        editor.expect_selections(&carets(&[(0, 3), (0, 7)]))?;

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
    support::run_x11_test("multi-cursor-delete-line-coalesce", |session| {
        let path = session.seed_file("delete-line-coalesce.txt", "keep\nfoo foo\nkeep2")?;
        let mut editor = session.open_file("delete-line-coalesce", &path)?;

        editor.keys("<C-home><down><C-S-l><esc>")?;
        editor.expect_selections(&carets(&[(1, 3), (1, 7)]))?;

        editor.keys("<C-S-k>")?;
        editor.save_then_expect_file(&path, "keep\nkeep2")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn duplicate_line_coalesces_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-duplicate-line-coalesce", |session| {
        let path = session.seed_file("duplicate-line-coalesce.txt", "foo foo\nbar")?;
        let mut editor = session.open_file("duplicate-line-coalesce", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        editor.expect_selections(&carets(&[(0, 3), (0, 7)]))?;

        editor.keys("<C-S-A-down>")?;
        editor.save_then_expect_file(&path, "foo foo\nfoo foo\nbar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn move_line_down_moves_adjacent_cursor_lines_as_one_cluster() -> TestResult {
    support::run_x11_test("multi-cursor-move-line-cluster", |session| {
        let path = session.seed_file("move-line-cluster.txt", "top\nfoo\nfoo\nbottom")?;
        let mut editor = session.open_file("move-line-cluster", &path)?;

        editor.keys("<C-home><down><C-S-l><esc>")?;
        editor.expect_selections(&carets(&[(1, 3), (2, 3)]))?;

        editor.keys("<A-down>")?;
        editor.save_then_expect_file(&path, "top\nbottom\nfoo\nfoo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn toggle_line_comment_coalesces_multiple_cursors_on_one_line() -> TestResult {
    support::run_x11_test("multi-cursor-comment-coalesce", |session| {
        let path = session.seed_file("comment-coalesce.rs", "let foo = foo;")?;
        let mut editor = session.open_file("comment-coalesce", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<right><right><right><right><C-S-l><esc>")?;
        editor.expect_selections(&carets(&[(0, 7), (0, 13)]))?;

        editor.keys("<C-/>")?;
        editor.save_then_expect_file(&path, "// let foo = foo;")?;
        Ok(())
    })
}
