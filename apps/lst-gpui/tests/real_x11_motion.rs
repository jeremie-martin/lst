//! Real-display tests for single-cursor motion and keyboard selection. Each
//! one drives the editor through key sequences and observes the resulting
//! cursor state through the trace channel.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_motion --run-ignored only

mod support;

use std::time::Duration;

use lst_x11_harness::{clipboard::wait_clipboard_text, Selection};
use support::{secs, EditorTestExt, HeldKeys, SelectionTestExt, TestResult, XK_DOWN, XK_LEFT, XK_RIGHT, XK_UP};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn home_and_end_target_the_current_wrapped_visual_row() -> TestResult {
    support::run_x11_test("motion-wrapped-visual-boundaries", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n")?;
        let text = "alpha beta gamma delta ".repeat(30);
        let path = session.seed_file("wrapped-boundaries.txt", &text)?;
        let mut editor = session.open_file("wrapped-visual-boundaries", &path)?;

        editor.wait_state("multiple wrapped rows", secs(5), |record| {
            record.viewport.rows.iter().filter(|row| row.logical_line == 0).count() >= 2
        })?;
        // Window discovery can observe the requested launch geometry before
        // the nested window manager applies its final tile. Wait for a real
        // DAMAGE-quiet interval, then consume the most recent trace so the
        // click target and asserted boundaries describe one settled layout.
        editor.wait_quiet(Duration::from_millis(500), secs(5))?;
        let wrapped = editor.read_state()?;
        let segments = wrapped
            .viewport
            .rows
            .iter()
            .filter(|row| row.logical_line == 0)
            .map(|row| (row.line_start_char, row.display_end_char))
            .collect::<Vec<_>>();
        assert!(segments.len() >= 2, "{wrapped:?}");
        let logical_start = segments[0].0;
        let second_start = segments[1].0 - logical_start;
        let second_end = segments[1].1 - logical_start;
        assert!(
            second_end >= second_start + 2,
            "unexpected wrapped segments: {segments:?}"
        );
        let inside_second = second_start + 2;

        editor.click_at_text(0, inside_second)?;
        editor.keys("<home>")?;
        editor.expect_cursor_heads(&[(0, second_start)])?;

        editor.click_at_text(0, inside_second)?;
        editor.keys("<end>")?;
        editor.expect_cursor_heads(&[(0, second_end)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_right_stops_after_each_word_and_punctuation_run() -> TestResult {
    support::run_x11_test("motion-ctrl-right-words", |session| {
        let path = session.seed_file("words.txt", "foo, bar; baz")?;
        let mut editor = session.open_file("words", &path)?;

        editor.place_cursor_at_document_start()?;
        for column in [3, 4, 8, 9, 13] {
            editor.keys("<C-right>")?;
            editor.expect_cursor_heads(&[(0, column)])?;
        }
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_right_crosses_decomposed_grapheme_word_without_splitting_it() -> TestResult {
    support::run_x11_test("motion-ctrl-right-grapheme-word", |session| {
        let path = session.seed_file("grapheme-word.txt", "nai\u{0308}ve word")?;
        let mut editor = session.open_file("motion-ctrl-right-grapheme-word", &path)?;

        editor.keys("<C-home><C-right>")?;
        editor.expect_cursor_heads(&[(0, 6)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn selecting_and_replacing_a_long_grapheme_keeps_the_following_text() -> TestResult {
    support::run_x11_test("motion-long-grapheme", |session| {
        let cluster = format!("a{}", "\u{301}".repeat(300));
        let path = session.seed_file("long-grapheme.txt", &format!("{cluster} tail"))?;
        let mut editor = session.open_file("long-grapheme", &path)?;

        editor.keys("<C-home><S-right>")?;
        editor.expect_cursor_heads(&[(0, 301)])?;
        editor.keys("<C-c>")?;
        wait_clipboard_text(Selection::Clipboard, &cluster, secs(5))?;
        editor.keys("b")?;
        editor.save_then_expect_file(&path, "b tail")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, &format!("{cluster} tail"))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn word_and_subword_motion_cross_blank_crlf_lines_and_keep_graphemes_intact() -> TestResult {
    support::run_x11_test("motion-words-across-crlf", |session| {
        let path = session.seed_file("words.txt", "alpha\r\n \t\r\ncafe\u{301}HTTP42_beta\r\nomega")?;
        let mut editor = session.open_file("motion-words-across-crlf", &path)?;

        editor.keys("<C-home><C-right>")?;
        editor.expect_cursor_heads(&[(0, 5)])?;
        editor.keys("<C-right>")?;
        editor.expect_cursor_heads(&[(2, 16)])?;
        for column in [12, 9, 5, 0] {
            editor.keys("<A-left>")?;
            editor.expect_cursor_heads(&[(2, column)])?;
        }
        editor.keys("<C-left>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        editor.keys("<C-right><A-right>")?;
        editor.expect_cursor_heads(&[(2, 5)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_right_stops_at_camel_case_and_snake_case_subwords() -> TestResult {
    support::run_x11_test("motion-alt-right-subwords", |session| {
        let path = session.seed_file("subwords.txt", "fooBar_baz")?;
        let mut editor = session.open_file("subwords", &path)?;

        editor.place_cursor_at_document_start()?;
        // Stops after "foo" and "Bar"; the next press skips the underscore
        // and stops after "baz".
        for column in [3, 6, 10] {
            editor.keys("<A-right>")?;
            editor.expect_cursor_heads(&[(0, column)])?;
        }
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn page_down_at_eof_lands_on_last_line() -> TestResult {
    support::run_x11_test("motion-page-down-eof", |session| {
        let body = (0..40).map(|i| format!("line {i:02}")).collect::<Vec<_>>().join("\n");
        let path = session.seed_file("eof.txt", &body)?;
        let mut editor = session.open_file("eof", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys(&"<pagedown>".repeat(10))?;
        editor.wait_state(
            "cursor on the last line",
            secs(5),
            |record| matches!(record.cursors.as_slice(), [cursor] if cursor.head_line == 39),
        )?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn keyboard_selection_extends_by_lines_and_to_document_edges() -> TestResult {
    support::run_x11_test("motion-keyboard-selection", |session| {
        let path = session.seed_file("keyboard-selection.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("keyboard-selection", &path)?;

        editor.keys("<C-home><right><right>")?;
        editor.expect_selections(&[((0, 2), (0, 2))])?;

        editor.keys("<S-down>")?;
        editor.expect_selections(&[((0, 2), (1, 2))])?;
        editor.keys("<S-down>")?;
        editor.expect_selections(&[((0, 2), (2, 2))])?;
        editor.keys("<S-up>")?;
        editor.expect_selections(&[((0, 2), (1, 2))])?;

        editor.keys("<C-S-end>")?;
        editor.expect_selections(&[((0, 2), (2, 5))])?;
        editor.keys("<C-S-home>")?;
        editor.expect_selections(&[((0, 2), (0, 0))])?;

        // Ctrl+L selects the caret's whole line, including its newline.
        editor.keys("<right><down><C-l>")?;
        editor.expect_selections(&[((1, 0), (2, 0))])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn smooth_cursor_diagonal_navigation_lands_and_stops_repainting() -> TestResult {
    support::run_x11_test("smooth-cursor-diagonal", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\nsmooth_cursor = true\n")?;
        let text = "A deliberately longer first line to exercise diagonal cursor movement.\nshort\n";
        let path = session.seed_file("diagonal.txt", text)?;
        let mut editor = session.open_file("smooth-cursor-diagonal", &path)?;
        editor.keys("<C-home><end>")?;
        editor.wait_quiet(Duration::from_millis(200), secs(5))?;
        editor.press(lst_x11_harness::KeyChord::Key(lst_x11_harness::Key::Down))?;
        if let Some(directory) = std::env::var_os("LST_CURSOR_CAPTURE_DIR") {
            // Optional visual sampling at successive animation times, not input
            // synchronization. Normal acceptance runs use the state/quiet waits.
            for frame in 0..12 {
                editor
                    .screenshot()?
                    .write_ppm(std::path::Path::new(&directory).join(format!("diagonal-{frame:02}.ppm")))?;
                std::thread::sleep(Duration::from_millis(16));
            }
        }
        editor.wait_state("diagonal reaches shorter next line", secs(2), |state| {
            state.cursors[0].head_char == text.find("short").unwrap() + 5
        })?;
        editor.wait_quiet(Duration::from_millis(200), secs(5))?;
        editor.keys("X")?;
        editor.save_then_expect_file(&path, &text.replace("short", "shortX"))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn held_perpendicular_arrows_repeat_on_both_axes_and_release_cleanly() -> TestResult {
    support::run_x11_test("held-diagonal-arrows", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\nsmooth_cursor = true\n")?;
        let text = format!("{}\n", "x".repeat(100)).repeat(80);
        let path = session.seed_file("held-arrows.txt", &text)?;
        let mut editor = session.open_file("held-arrows", &path)?;
        for (horizontal, dx) in [(XK_LEFT, -1isize), (XK_RIGHT, 1)] {
            for (vertical, dy) in [(XK_UP, -1isize), (XK_DOWN, 1)] {
                for horizontal_first in [true, false] {
                    editor.keys(&format!("<C-home>{}{}", "<down>".repeat(15), "<right>".repeat(15)))?;
                    let initial = editor.expect_cursor_heads(&[(15, 15)])?.cursors[0];
                    let (first, second) = if horizontal_first {
                        (horizontal, vertical)
                    } else {
                        (vertical, horizontal)
                    };
                    let mut held = HeldKeys::new()?;
                    held.press(first)?;
                    // Add the second key while the first is already repeating,
                    // exercising both press orders rather than one chord.
                    let repeating = editor.wait_state("first held arrow repeats", secs(3), |state| {
                        let after = &state.cursors[0];
                        if horizontal_first {
                            (after.head_col as isize - initial.head_col as isize) * dx >= 2
                        } else {
                            (after.head_line as isize - initial.head_line as isize) * dy >= 2
                        }
                    })?;
                    let before = repeating.cursors[0];
                    held.press(second)?;
                    editor.wait_state("both held axes repeat", secs(3), |state| {
                        let after = &state.cursors[0];
                        (after.head_line as isize - before.head_line as isize) * dy >= 3
                            && (after.head_col as isize - before.head_col as isize) * dx >= 3
                    })?;
                    held.release_all()?;
                    // Once released, nothing repeats: one more arrow moves
                    // exactly one column from where the cursor stopped.
                    editor.wait_quiet(Duration::from_millis(200), secs(5))?;
                    let released = editor.read_state()?.cursors[0];
                    editor.keys("<right>")?;
                    editor.expect_cursor_heads(&[(released.head_line, released.head_col + 1)])?;
                }
            }
        }
        let record = editor.read_state()?;
        assert!(!record.active_tab_modified, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn held_diagonal_arrows_cross_blank_lines_without_cancelling_vertical_motion() -> TestResult {
    support::run_x11_test("held-arrows-blank-lines", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\nsmooth_cursor = true\n")?;
        let text = format!("{}\n\n", "x".repeat(100)).repeat(20);
        let path = session.seed_file("paragraphs.txt", &text)?;
        let mut editor = session.open_file("held-arrows-blank-lines", &path)?;
        editor.keys(&format!("<C-home>{}{}", "<down>".repeat(12), "<right>".repeat(15)))?;
        editor.expect_cursor_heads(&[(12, 15)])?;

        let mut held = HeldKeys::new()?;
        held.press(XK_RIGHT)?;
        held.press(XK_UP)?;
        editor.wait_state("diagonal crosses multiple blank lines", secs(3), |state| {
            state.cursors[0].head_line <= 8 && state.cursors[0].head_col >= 18
        })?;
        held.release_all()?;
        let record = editor.read_state()?;
        assert!(!record.active_tab_modified, "{record:?}");
        Ok(())
    })
}
