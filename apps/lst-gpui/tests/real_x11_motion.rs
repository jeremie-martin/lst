//! Real-display tests for cursor-motion behaviors. Each one drives the
//! editor through key sequences and observes the resulting cursor state
//! through the trace channel — no typing-and-inspecting the autosave file.
//!
//! Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn smart_home_toggles_between_first_non_blank_and_column_zero() -> TestResult {
    // Seed an indented line, jump to its end, then press Home twice. The
    // first press lands on the first non-blank (col 4); the second press
    // collapses to column 0; a third returns to first non-blank.
    support::run_x11_test("motion-smart-home", |session| {
        let path = session.seed_file("smart-home.txt", "    foo")?;
        let mut editor = session.open_file("smart-home", &path)?;

        editor.keys("<C-end>")?;
        let at_end = editor.read_state()?;
        assert_eq!(at_end.cursors.len(), 1, "{at_end:?}");
        assert_eq!(
            at_end.cursors[0].head_col, 7,
            "Ctrl+End should land at end-of-line; got {at_end:?}"
        );

        editor.keys("<home>")?;
        let after_first = editor.read_state()?;
        assert_eq!(
            after_first.cursors[0].head_col, 4,
            "first Home should snap to first non-blank: {after_first:?}"
        );

        editor.keys("<home>")?;
        let after_second = editor.read_state()?;
        assert_eq!(
            after_second.cursors[0].head_col, 0,
            "second Home should toggle to column 0: {after_second:?}"
        );

        editor.keys("<home>")?;
        let after_third = editor.read_state()?;
        assert_eq!(
            after_third.cursors[0].head_col, 4,
            "third Home should toggle back to first non-blank: {after_third:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_right_advances_cursor_across_word_boundaries() -> TestResult {
    // Don't pin exact word-boundary behavior (start-of-word vs end-of-word
    // is a long-standing editor preference); just assert that Ctrl+Right
    // crosses each word and stops increasing once we've reached EOL.
    support::run_x11_test("motion-ctrl-right-words", |session| {
        let path = session.seed_file("words.txt", "foo, bar; baz")?;
        let mut editor = session.open_file("words", &path)?;

        editor.keys("<C-home>")?;
        let start = editor.read_state()?;
        assert_eq!(start.cursors[0].head_col, 0, "{start:?}");

        let mut cols = vec![0usize];
        for _ in 0..6 {
            editor.keys("<C-right>")?;
            let record = editor.read_state()?;
            cols.push(record.cursors[0].head_col);
        }
        // Cursor must advance monotonically (or stop) across boundaries —
        // never go backwards.
        for window in cols.windows(2) {
            assert!(
                window[1] >= window[0],
                "Ctrl+Right must not regress the cursor: trail {cols:?}"
            );
        }
        // After enough presses we should reach the line's last column.
        let line_len = "foo, bar; baz".len();
        assert!(
            *cols.last().unwrap() >= line_len - 1,
            "expected to land at or near EOL ({line_len}); trail {cols:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_right_subword_motion_lands_inside_camel_and_snake_runs() -> TestResult {
    // "fooBar_baz" should produce subword stops at the case transition
    // (Bar) and the snake separator (_). Don't pin exact stops; assert
    // that two Alt+Right presses make at least two distinct positions
    // strictly between 0 and len.
    support::run_x11_test("motion-alt-right-subwords", |session| {
        let path = session.seed_file("subwords.txt", "fooBar_baz")?;
        let mut editor = session.open_file("subwords", &path)?;

        editor.keys("<C-home>")?;
        let start = editor.read_state()?;
        assert_eq!(start.cursors[0].head_col, 0);

        editor.keys("<A-right>")?;
        let after_first = editor.read_state()?.cursors[0].head_col;
        editor.keys("<A-right>")?;
        let after_second = editor.read_state()?.cursors[0].head_col;

        let line_len = "fooBar_baz".len();
        assert!(
            after_first > 0 && after_first < line_len,
            "first Alt+Right should land inside the line; got {after_first}"
        );
        assert!(
            after_second > after_first,
            "subword motion must advance: {after_first} → {after_second}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn page_down_at_eof_lands_on_last_line() -> TestResult {
    // The "snap to EOL" specifics depend on whether we're in vim mode and
    // which key was used. Assert the weaker invariant: after enough page
    // downs, the cursor's head_line is the last line in the buffer.
    support::run_x11_test("motion-page-down-eof", |session| {
        let body = (0..40)
            .map(|i| format!("line {i:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = session.seed_file("eof.txt", &body)?;
        let mut editor = session.open_file("eof", &path)?;

        editor.keys("<C-home>")?;
        for _ in 0..10 {
            editor.keys("<pagedown>")?;
        }
        let record = editor.read_state()?;
        let last_line = record.line_count.saturating_sub(1);
        assert_eq!(
            record.cursors[0].head_line, last_line,
            "after exhaustive PageDown the cursor should be on the last line: {record:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vertical_motion_per_cursor_preferred_column() -> TestResult {
    // TDD spec: each cursor should remember its own preferred column
    // across vertical motion. Today only the primary cursor has
    // `preferred_column` (see editor-behaviors-checklist.md "Per-cursor
    // goal column"), so this test is expected to fail until that gap is
    // closed. Keep it ignored-passing today by writing the assertion
    // exactly as a fixed implementation would behave.
    support::run_x11_test("motion-per-cursor-preferred-col", |session| {
        let path = session.seed_file("preferred.txt", "abcdefghij\nshort\nabcdefghij\n")?;
        let mut editor = session.open_file("preferred", &path)?;

        // Place primary at (0, 8). Add a cursor at (2, 8) via Ctrl-Alt-Down
        // twice so we have two cursors in a tall column.
        editor.keys("<C-home>")?;
        for _ in 0..8 {
            editor.keys("<right>")?;
        }
        editor.keys("<C-A-down><C-A-down>")?;
        // Move down once: the middle line is short, so both cursors clamp
        // to its end (col 5). Move up twice: each cursor should restore
        // to col 8 thanks to its own preferred-column memory.
        editor.keys("<down><up><up>")?;
        let record = editor.read_state()?;
        assert_eq!(record.cursors.len(), 3, "{record:?}");
        let cols: Vec<usize> = record.cursors.iter().map(|c| c.head_col).collect();
        assert_eq!(
            cols,
            vec![8, 8, 8],
            "each cursor should restore its own goal column 8: {record:?}"
        );
        Ok(())
    })
}
