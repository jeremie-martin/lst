//! Under-review executable specs for `Ctrl+Shift+T` reopening the most
//! recently closed tab.
//!
//! Pinned contract:
//!
//! - `Ctrl+Shift+T` (matching VS Code / browser conventions) reopens the
//!   most recently closed tab. The reopened tab points at the same file
//!   path and the caret is restored to where it was at close time.
//! - Pressing the chord with no closed-tab history is a visible no-op:
//!   the active tab and caret remain unchanged.
//!
//! Specs run under the `x11-tdd` profile. Promote to
//! `real_x11_recently_closed.rs` once green.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_recently_closed_tdd --run-ignored only

mod support;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_shift_t_reopens_most_recently_closed_file_with_cursor_position() -> TestResult {
    support::run_x11_test("recently-closed-reopen", |session| {
        let contents = "line0\nline1\nline2\nline3\nline4\nline5\nline6\n";
        let path = session.seed_file("reopen-target.txt", contents)?;
        let mut editor = session.open_file("recently-closed-reopen", &path)?;

        let path_string = path.to_string_lossy().into_owned();

        // Move caret to line 5, leave the buffer clean so close does not
        // trigger a save prompt.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down><down><down><down>")?;
        editor.expect_cursor_heads(&[(5, 0)])?;

        // Open a sibling untitled tab so closing the file's tab does not
        // trigger the single-tab quit shortcut. Then switch back to the
        // file so it is the active tab when we close it.
        editor.keys("<C-n>")?;
        editor.wait_state("sibling tab focused", secs(2), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("file tab refocused", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&path_string)
        })?;
        editor.expect_cursor_heads(&[(5, 0)])?;

        // Close the file's tab (Ctrl+W). The sibling untitled tab becomes
        // the active buffer.
        editor.keys("<C-w>")?;
        editor.wait_state("tab closed", secs(2), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;

        // Reopen via Ctrl+Shift+T; the active buffer must return to the
        // closed file with caret position restored.
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
        let (mut editor, _path) = session.open("scratch")?;

        // Capture baseline before pressing the chord.
        let before = editor.read_state()?;
        let before_path = before.active_tab_path.clone();
        let before_cursor = before.cursors[0].head_pos();

        editor.keys("<C-S-t>")?;
        editor.wait_quiet(std::time::Duration::from_millis(75), std::time::Duration::from_secs(2))?;

        let after = editor.read_state()?;
        assert_eq!(
            after.active_tab_path, before_path,
            "active tab path must not change with empty history: {:?} vs {:?}",
            before_path, after.active_tab_path
        );
        assert_eq!(
            after.cursors[0].head_pos(),
            before_cursor,
            "caret must not move with empty history: {:?} vs {:?}",
            before_cursor,
            after.cursors[0].head_pos()
        );
        Ok(())
    })
}
