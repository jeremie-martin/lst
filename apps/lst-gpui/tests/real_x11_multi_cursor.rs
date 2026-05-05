//! Real-display tests for the multi-cursor surface. These assertions follow
//! `docs/editor-behaviors-checklist.md` as the behavior spec; some are
//! intentionally ahead of the current implementation.
//!
//! Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Key, KeyChord, Selection,
};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_adds_occurrences_and_literal_input_replaces_them() -> TestResult {
    // Type "foo bar foo baz foo" in Insert, drop to Normal, return to the
    // first character, re-enter Insert at column 0, then add the next two
    // occurrences with Ctrl+D and replace them all by typing "qux".
    //
    // The first Ctrl+D selects "foo" at the cursor (word-under-cursor
    // fallback). The next two Ctrl+D's add the second and third
    // occurrences. Typing "qux" replaces every selection through the
    // multi-cursor edit path, so the autosave file ends up as
    // "qux bar qux baz qux".
    support::run_x11_test("multi-cursor-ctrl-d", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo bar foo baz foo<esc>0i<C-d><C-d><C-d>qux")?;
        editor.save_then_expect_file(&path, "qux bar qux baz qux")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_l_selects_all_occurrences_and_replaces_them() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-shift-l", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo bar foo<enter>foo baz<esc>gg0i<C-S-l>qux")?;
        editor.save_then_expect_file(&path, "qux bar qux\nqux baz")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_down_adds_adjacent_line_cursors_for_literal_input() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-alt-down", |session| {
        let path = session.seed_file("columns.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("columns", &path)?;

        editor.keys("<C-home><C-A-down><C-A-down>X")?;
        editor.save_then_expect_file(&path, "Xalpha\nXbeta\nXgamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn backspace_deletes_before_every_cursor_added_by_adjacent_line_commands() -> TestResult {
    support::run_x11_test("multi-cursor-backspace", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("alpha<enter>bravo<enter>gamma<esc>gg0i<end><C-A-down><C-A-down><bs>")?;
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

        editor.keys("<C-home><C-A-down><C-A-down><delete>")?;
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
        editor.keys("<C-home><C-A-down><C-A-down><C-v>")?;
        editor.save_then_expect_file(&path, "redA\ngreenB\nblueC")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn copy_collects_multi_selection_fragments_in_document_order() -> TestResult {
    support::run_x11_test("multi-cursor-copy-fragments", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.keys("foo bar foo baz foo<esc>0i<C-S-l>")?;
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

        editor.keys("<C-home><end><C-A-down><enter>")?;
        editor.save_then_expect_file(&path, "    alpha!\n    \n        be\n        ")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn escape_collapses_non_empty_multi_selections_to_cursors_before_input() -> TestResult {
    support::run_x11_test("multi-cursor-escape-collapse", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo foo foo<esc>0i<C-S-l><esc>X")?;
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

        editor.keys("<C-home><C-A-down><C-A-down><right><right>X")?;
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

        editor.keys("<C-home><C-A-down><C-S-d>")?;
        editor.save_then_expect_file(&path, "alpha\nalpha\nbeta\nbeta\ngamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_k_ctrl_d_skips_current_occurrence_and_adds_the_next() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-k-ctrl-d", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo foo foo<esc>0i<C-d><C-d><C-k><C-d>bar")?;
        editor.save_then_expect_file(&path, "bar foo bar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_u_pops_last_added_occurrence_cursor() -> TestResult {
    support::run_x11_test("multi-cursor-ctrl-u-pop", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo foo foo<esc>0i<C-d><C-d><C-d><C-u>bar")?;
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
