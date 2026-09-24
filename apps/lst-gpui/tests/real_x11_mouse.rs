//! Real-display tests for mouse-driven selection and cursor placement.
//! Uses the text-coordinate mouse API (`click_at_text`, `drag_text`, etc.)
//! so the assertions are robust against font / gutter / padding changes.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_mouse --run-ignored only

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, ChordMods, Selection};
use support::{EditorTestExt, SelectionTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn single_click_positions_caret_at_text_column() -> TestResult {
    support::run_x11_test("mouse-single-click", |session| {
        let path = session.seed_file("click.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("click", &path)?;

        editor.click_at_text(1, 3)?;
        editor.expect_selections(&[((1, 3), (1, 3))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn double_click_selects_word_at_text_position() -> TestResult {
    support::run_x11_test("mouse-double-click-word", |session| {
        let path = session.seed_file("double.txt", "alpha bravo charlie")?;
        let mut editor = session.open_file("double", &path)?;

        // Click inside "bravo" (chars 6..11).
        editor.double_click_at_text(0, 8)?;
        editor.expect_selection_ranges(&[(6, 11)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn triple_click_selects_line_at_text_position() -> TestResult {
    support::run_x11_test("mouse-triple-click-line", |session| {
        let path = session.seed_file("triple.txt", "alpha\nbravo charlie\ndelta")?;
        let mut editor = session.open_file("triple", &path)?;

        // Line 1 is "bravo charlie" at chars 6..19; the selection includes
        // its newline.
        editor.triple_click_at_text(1, 5)?;
        editor.expect_selection_ranges(&[(6, 20)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quad_click_selects_paragraph_at_text_position() -> TestResult {
    support::run_x11_test("mouse-quad-click-paragraph", |session| {
        let path = session.seed_file("quad.txt", "para one a\npara one b\n\npara two a\npara two b")?;
        let mut editor = session.open_file("quad", &path)?;

        // The first paragraph "para one a\npara one b" spans chars 0..21; the
        // selection includes its trailing newline.
        editor.quad_click_at_text(0, 2)?;
        editor.expect_selection_ranges(&[(0, 22)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn shift_click_extends_selection_from_caret() -> TestResult {
    support::run_x11_test("mouse-shift-click-extend", |session| {
        let path = session.seed_file("extend.txt", "alpha bravo charlie\ndelta")?;
        let mut editor = session.open_file("extend", &path)?;

        editor.click_at_text(0, 6)?;
        editor.shift_click_at_text(0, 11)?;
        editor.expect_selections(&[((0, 6), (0, 11))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_click_adds_secondary_cursor_at_click_point() -> TestResult {
    support::run_x11_test("mouse-alt-click-add", |session| {
        let path = session.seed_file("alt-add.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("alt-add", &path)?;

        editor.click_at_text(0, 3)?;
        editor.alt_click_at_text(2, 4)?;
        editor.expect_cursor_heads(&[(0, 3), (2, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_click_on_existing_cursor_removes_it() -> TestResult {
    support::run_x11_test("mouse-alt-click-remove", |session| {
        let path = session.seed_file("alt-rm.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("alt-rm", &path)?;

        editor.click_at_text(0, 0)?;
        editor.alt_click_at_text(2, 0)?;
        editor.expect_cursor_heads(&[(0, 0), (2, 0)])?;

        // Alt-clicking the added cursor removes it. Handling the gesture as a
        // plain click would instead leave one cursor at (2, 0).
        editor.alt_click_at_text(2, 0)?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn drag_text_selects_range_that_typing_replaces() -> TestResult {
    support::run_x11_test("mouse-drag-select", |session| {
        let path = session.seed_file("drag.txt", "alpha bravo charlie\ndelta")?;
        let mut editor = session.open_file("drag", &path)?;

        editor.drag_text((0, 6), (0, 11), ChordMods::default())?;
        editor.expect_selections(&[((0, 6), (0, 11))])?;

        editor.keys("BRAVO")?;
        editor.save_then_expect_file(&path, "alpha BRAVO charlie\ndelta")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_drag_adds_a_freeform_selection_without_replacing_existing_selections() -> TestResult {
    support::run_x11_test("mouse-alt-drag-add", |session| {
        let path = session.seed_file("alt-drag.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("alt-drag-add", &path)?;

        editor.drag_text((0, 1), (0, 4), ChordMods::default())?;
        editor.expect_selections(&[((0, 1), (0, 4))])?;

        editor.drag_text((1, 1), (1, 4), ChordMods::ALT)?;
        let record = editor.expect_selections(&[((0, 1), (0, 4)), ((1, 1), (1, 4))])?;
        assert_eq!(record.primary_cursor_index, 1, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn dragging_a_secondary_selection_moves_only_that_target_and_preserves_the_set() -> TestResult {
    support::run_x11_test("mouse-drag-secondary-selection", |session| {
        let path = session.seed_file("drag-secondary.txt", "one two three four")?;
        let mut editor = session.open_file("drag-secondary", &path)?;

        editor.drag_text((0, 0), (0, 3), ChordMods::default())?;
        editor.drag_text((0, 8), (0, 13), ChordMods::ALT)?;
        editor.expect_selection_ranges(&[(0, 3), (8, 13)])?;

        editor.drag_text((0, 10), (0, 18), ChordMods::default())?;
        editor.save_then_expect_file(&path, "one two  fourthree")?;
        editor.expect_selection_ranges(&[(0, 3), (13, 18)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn dragging_selected_text_moves_it_as_one_undoable_edit() -> TestResult {
    support::run_x11_test("mouse-drag-move-selection", |session| {
        let path = session.seed_file("drag-move.txt", "abcdef")?;
        let mut editor = session.open_file("drag-move", &path)?;

        editor.drag_text((0, 1), (0, 3), ChordMods::default())?;
        editor.drag_text((0, 2), (0, 6), ChordMods::default())?;
        editor.save_then_expect_file(&path, "adefbc")?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "abcdef")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_dragging_selected_text_copies_it() -> TestResult {
    support::run_x11_test("mouse-drag-copy-selection", |session| {
        let path = session.seed_file("drag-copy.txt", "abcdef")?;
        let mut editor = session.open_file("drag-copy", &path)?;

        editor.drag_text((0, 1), (0, 3), ChordMods::default())?;
        editor.drag_text((0, 2), (0, 6), ChordMods::CTRL)?;
        editor.save_then_expect_file(&path, "abcdefbc")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn clicking_inside_selection_without_drag_collapses_the_caret() -> TestResult {
    support::run_x11_test("mouse-selection-click-collapse", |session| {
        let path = session.seed_file("selection-click.txt", "abcdef")?;
        let mut editor = session.open_file("selection-click", &path)?;

        editor.drag_text((0, 1), (0, 3), ChordMods::default())?;
        editor.expect_selections(&[((0, 1), (0, 3))])?;

        // A click that never moves must not start a text move, so the buffer
        // stays unmodified.
        editor.click_at_text(0, 2)?;
        let record = editor.expect_selections(&[((0, 2), (0, 2))])?;
        assert!(!record.active_tab_modified, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn middle_click_at_text_pastes_primary_selection_at_click_point() -> TestResult {
    support::run_x11_test("mouse-middle-click-paste", |session| {
        let path = session.seed_file("middle.txt", "alpha bravo")?;
        let mut editor = session.open_file("middle", &path)?;

        write_clipboard_text(Selection::Primary, "PASTED")?;
        editor.middle_click_at_text(0, 6)?;
        editor.save_then_expect_file(&path, "alpha PASTEDbravo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn click_below_last_line_moves_caret_to_document_end() -> TestResult {
    // Clicking the empty area below the last line jumps the caret to the end
    // of the document, as in other editors.
    support::run_x11_test("mouse-click-below-last-line", |session| {
        let path = session.seed_file("below.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("below", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.click_below_last_row()?;
        editor.expect_selections(&[((2, 7), (2, 7))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn gutter_click_selects_a_line_and_shift_gutter_click_extends_by_lines() -> TestResult {
    support::run_x11_test("mouse-gutter-click", |session| {
        let path = session.seed_file("gutter.txt", "alpha\nbravo\ncharlie\ndelta")?;
        let mut editor = session.open_file("gutter", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.click_gutter(1, false)?;
        editor.expect_selections(&[((1, 0), (2, 0))])?;

        editor.click_gutter(3, true)?;
        editor.expect_selections(&[((1, 0), (3, 5))])?;

        editor.click_gutter(0, true)?;
        editor.expect_selections(&[((2, 0), (0, 0))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn double_click_drag_extends_by_words_and_triple_click_drag_by_lines() -> TestResult {
    support::run_x11_test("mouse-multi-click-drag", |session| {
        let path = session.seed_file("multi-click-drag.txt", "alpha bravo charlie\ndelta echo\nfoxtrot")?;
        let mut editor = session.open_file("multi-click-drag", &path)?;

        // A word drag from inside "bravo" to inside "charlie" covers both
        // words.
        editor.multi_click_drag_text(2, (0, 8), (0, 14))?;
        editor.expect_selection_ranges(&[(6, 19)])?;

        // A line drag from line 0 into line 1 covers both lines and the
        // newline after line 1.
        editor.multi_click_drag_text(3, (0, 2), (1, 3))?;
        editor.expect_selection_ranges(&[(0, 31)])?;
        Ok(())
    })
}
