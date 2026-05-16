//! Real-display tests for mouse-driven selection and cursor placement.
//! Uses the text-coordinate mouse API (`click_at_text`, `drag_text`, etc.)
//! so the assertions are robust against font / gutter / padding changes.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_mouse --run-ignored only

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, ChordMods, Selection};
use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn single_click_positions_caret_at_text_column() -> TestResult {
    support::run_x11_test("mouse-single-click", |session| {
        let path = session.seed_file("click.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("click", &path)?;

        editor.click_at_text(1, 3)?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 1, "{record:?}");
        let cursor = record.cursors[0];
        assert!(cursor.is_collapsed(), "{cursor:?}");
        assert_eq!(cursor.head_line, 1, "{cursor:?}");
        assert_eq!(cursor.head_col, 3, "{cursor:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn double_click_selects_word_at_text_position() -> TestResult {
    support::run_x11_test("mouse-double-click-word", |session| {
        let path = session.seed_file("double.txt", "alpha bravo charlie")?;
        let mut editor = session.open_file("double", &path)?;

        // Click anywhere inside "bravo" (cols 6..11).
        editor.double_click_at_text(0, 8)?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 1, "{record:?}");
        let cursor = record.cursors[0];
        let lo = cursor.anchor_char.min(cursor.head_char);
        let hi = cursor.anchor_char.max(cursor.head_char);
        assert_eq!((lo, hi), (6, 11), "double-click should select \"bravo\": {cursor:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn triple_click_selects_line_at_text_position() -> TestResult {
    support::run_x11_test("mouse-triple-click-line", |session| {
        let path = session.seed_file("triple.txt", "alpha\nbravo charlie\ndelta")?;
        let mut editor = session.open_file("triple", &path)?;

        editor.triple_click_at_text(1, 5)?;
        let record = editor.read_state()?;
        let cursor = record.cursors[0];
        let lo = cursor.anchor_char.min(cursor.head_char);
        let hi = cursor.anchor_char.max(cursor.head_char);
        // Line 1 is "bravo charlie", char range [6, 19]. Some editors
        // include the trailing newline, others don't — accept either.
        assert!(lo == 6, "triple-click should anchor at line start: got {lo}");
        assert!(
            hi == 19 || hi == 20,
            "triple-click should select to line end (with or without \\n): got {hi}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quad_click_selects_paragraph_at_text_position() -> TestResult {
    support::run_x11_test("mouse-quad-click-paragraph", |session| {
        let path = session.seed_file("quad.txt", "para one a\npara one b\n\npara two a\npara two b")?;
        let mut editor = session.open_file("quad", &path)?;

        // Click inside the first paragraph (lines 0–1).
        editor.quad_click_at_text(0, 2)?;
        let record = editor.read_state()?;
        let cursor = record.cursors[0];
        let lo = cursor.anchor_char.min(cursor.head_char);
        let hi = cursor.anchor_char.max(cursor.head_char);
        // The first paragraph spans chars 0..21 ("para one a\npara one b").
        assert_eq!(lo, 0, "{cursor:?}");
        assert!(
            hi >= 21,
            "quad-click should select through the first paragraph: got {cursor:?}"
        );
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
        let record = editor.read_state()?;
        let cursor = record.cursors[0];
        assert_eq!(cursor.anchor_char, 6, "{cursor:?}");
        assert_eq!(cursor.head_char, 11, "{cursor:?}");
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
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 2, "{record:?}");
        let heads: Vec<(usize, usize)> = record.cursors.iter().map(|c| (c.head_line, c.head_col)).collect();
        assert!(
            heads.contains(&(0, 3)) && heads.contains(&(2, 4)),
            "expected cursors at (0,3) and (2,4); got {heads:?}"
        );
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
        let with_two = editor.read_state()?;
        assert_eq!(with_two.cursors.len(), 2, "{with_two:?}");

        // Alt-click again at the same position to toggle off.
        editor.alt_click_at_text(2, 0)?;
        let with_one = editor.read_state()?;
        assert_eq!(
            with_one.cursors.len(),
            1,
            "alt-click on an existing cursor should remove it: {with_one:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn drag_text_selects_range() -> TestResult {
    support::run_x11_test("mouse-drag-select", |session| {
        let path = session.seed_file("drag.txt", "alpha bravo charlie\ndelta")?;
        let mut editor = session.open_file("drag", &path)?;

        editor.drag_text((0, 6), (0, 11), ChordMods::default())?;
        let record = editor.read_state()?;
        let cursor = record.cursors[0];
        assert_eq!(cursor.anchor_char, 6, "{cursor:?}");
        assert_eq!(cursor.head_char, 11, "{cursor:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn drag_text_followed_by_typing_replaces_selection() -> TestResult {
    // End-to-end sanity: text-coordinate drag interacts with the rest of
    // the editor pipeline the same way a pixel-coordinate drag would.
    support::run_x11_test("mouse-drag-replace", |session| {
        let path = session.seed_file("drag-replace.txt", "alpha bravo charlie")?;
        let mut editor = session.open_file("drag-replace", &path)?;

        editor.drag_text((0, 6), (0, 11), ChordMods::default())?;
        editor.keys("BRAVO")?;
        editor.save_then_expect_file(&path, "alpha BRAVO charlie")?;
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
