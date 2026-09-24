//! Accepted real-X11 behavior for `Ctrl+Shift+T` reopening the most recently
//! closed tab.
//!
//! Pinned contract:
//!
//! - `Ctrl+Shift+T` (matching VS Code / browser conventions) reopens the
//!   most recently closed tab. The reopened tab points at the same file
//!   path and the caret is restored to where it was at close time.
//! - Pressing the chord with no closed-tab history is a visible no-op:
//!   the active tab and caret remain unchanged.
//! - A closed file that can no longer be read reports the failure and drops
//!   out of the history, so the next press reopens the tab closed before it.

mod support;

use support::{close_active_tab_keeping_window, path_text, secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_t_reopens_most_recently_closed_file_with_cursor_position() -> TestResult {
    support::run_x11_test("recently-closed-reopen", |session| {
        let contents = "line0\nline1\nline2\nline3\nline4\nline5\nline6\n";
        let path = session.seed_file("reopen-target.txt", contents)?;
        let path_string = path_text(&path);
        let mut editor = session.open_file("recently-closed-reopen", &path)?;

        // The buffer stays clean so closing it does not prompt.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down><down><down><down>")?;
        editor.expect_cursor_heads(&[(5, 0)])?;
        close_active_tab_keeping_window(&mut editor)?;

        editor.keys("<C-S-t>")?;
        editor.wait_state("tab reopened", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&path_string)
        })?;
        editor.expect_cursor_heads(&[(5, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_t_with_empty_history_is_a_visible_noop() -> TestResult {
    support::run_x11_test("recently-closed-empty-history", |session| {
        let path = session.seed_file("only-tab.txt", "alpha\nbeta\n")?;
        let mut editor = session.open_file("recently-closed-empty-history", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down><right>")?;
        let before = editor.expect_cursor_heads(&[(1, 1)])?;

        // Right is the fence: once the caret reaches column 2, Ctrl+Shift+T
        // has been handled, and no record on the way may change the tab,
        // the text, or the caret.
        editor.expect_keys_ignored(
            "<C-S-t>",
            "<right>",
            |record| record.cursors.first().is_some_and(|cursor| cursor.head_pos() == (1, 2)),
            |record| {
                record.active_tab_id == before.active_tab_id
                    && record.revision == before.revision
                    && record.cursors.len() == 1
                    && matches!(record.cursors[0].head_pos(), (1, 1) | (1, 2))
            },
        )?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_t_reopens_scratchpad_with_cursor_and_autosave_origin() -> TestResult {
    support::run_x11_test("recently-closed-scratchpad", |session| {
        let (mut editor, scratchpad) = session.open("scratch")?;
        let scratchpad_text = path_text(&scratchpad);

        editor.keys("scratch body")?;
        editor.expect_file(&scratchpad, "scratch body")?;
        editor.expect_cursor_heads(&[(0, 12)])?;
        close_active_tab_keeping_window(&mut editor)?;

        editor.keys("<C-S-t>")?;
        editor.wait_state("scratchpad reopened", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(scratchpad_text.as_str())
        })?;
        editor.expect_cursor_heads(&[(0, 12)])?;
        editor.keys("X")?;
        editor.expect_file(&scratchpad, "scratch bodyX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_reopen_drops_bad_entry_so_older_closed_tab_can_reopen() -> TestResult {
    support::run_x11_test("recently-closed-failed-reopen", |session| {
        let older = session.seed_file("older.txt", "older\n")?;
        let missing = session.seed_file("missing.txt", "missing\n")?;
        let anchor = session.seed_file("anchor.txt", "anchor\n")?;
        let older_string = path_text(&older);
        let missing_string = path_text(&missing);
        let anchor_string = path_text(&anchor);
        let mut editor = session.open_files(
            "recently-closed-failed-reopen",
            &[older.clone(), missing.clone(), anchor.clone()],
        )?;

        editor.wait_state("older active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&older_string)
        })?;
        editor.keys("<C-w>")?;
        editor.wait_state("missing active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&missing_string)
        })?;
        editor.keys("<C-w>")?;
        editor.wait_state("anchor active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&anchor_string)
        })?;

        std::fs::remove_file(&missing)?;
        editor.keys("<C-S-t>")?;
        let failure = format!("Failed to open {missing_string}");
        editor.wait_state("failed reopen reported", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&anchor_string) && record.status_message.starts_with(&failure)
        })?;
        editor.keys("<C-S-t>")?;
        editor.wait_state("older tab reopened", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&older_string)
        })?;
        Ok(())
    })
}
