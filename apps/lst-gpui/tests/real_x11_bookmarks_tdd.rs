//! Under-review executable specs for line bookmarks.
//!
//! Pinned chords (matching the VS Code Bookmarks extension):
//!
//! - `Ctrl+Alt+K` — toggle a bookmark on the current line
//! - `Ctrl+Alt+L` — jump to the next bookmark (wraps to the first)
//! - `Ctrl+Alt+J` — jump to the previous bookmark (wraps to the last)
//!
//! Bookmarks are per-buffer. Navigation lands the caret on the bookmarked
//! line at column 0. Toggling a bookmark on an already-marked line clears
//! the mark.
//!
//! These specs assert only through `expect_cursor_heads` — internal mark
//! state is observed indirectly through the navigation outcome, so no new
//! state-trace fields are required.
//!
//! Specs run under the `x11-tdd` profile. Promote to
//! `real_x11_bookmarks.rs` once green.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_bookmarks_tdd --run-ignored only

mod support;

use support::{EditorTestExt, TestResult};

fn ten_line_fixture() -> String {
    (0..10)
        .map(|i| format!("line{i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_k_toggles_bookmark_and_ctrl_alt_l_jumps_back_to_it() -> TestResult {
    support::run_x11_test("bookmarks-toggle-and-jump", |session| {
        let path = session.seed_file("bookmarks-toggle-and-jump.txt", &ten_line_fixture())?;
        let mut editor = session.open_file("bookmarks-toggle-and-jump", &path)?;

        // Toggle a bookmark on line 5.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down><down><down><down>")?;
        editor.expect_cursor_heads(&[(5, 0)])?;
        editor.keys("<C-A-k>")?;

        // Move away, then jump-next; cursor should land on the marked line.
        editor.keys("<C-home>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;

        editor.keys("<C-A-l>")?;
        editor.expect_cursor_heads(&[(5, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_l_cycles_forward_through_marks_in_document_order() -> TestResult {
    support::run_x11_test("bookmarks-cycle-forward", |session| {
        let path = session.seed_file("bookmarks-cycle-forward.txt", &ten_line_fixture())?;
        let mut editor = session.open_file("bookmarks-cycle-forward", &path)?;

        // Mark line 2.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;
        editor.keys("<C-A-k>")?;

        // Mark line 7.
        editor.keys("<down><down><down><down><down>")?;
        editor.expect_cursor_heads(&[(7, 0)])?;
        editor.keys("<C-A-k>")?;

        // Jump-next from line 0 cycles 2 → 7 → 2 in document order.
        editor.keys("<C-home>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;

        editor.keys("<C-A-l>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;

        editor.keys("<C-A-l>")?;
        editor.expect_cursor_heads(&[(7, 0)])?;

        editor.keys("<C-A-l>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_j_cycles_backward_through_marks_with_wrap() -> TestResult {
    support::run_x11_test("bookmarks-cycle-backward", |session| {
        let path = session.seed_file("bookmarks-cycle-backward.txt", &ten_line_fixture())?;
        let mut editor = session.open_file("bookmarks-cycle-backward", &path)?;

        // Mark lines 2 and 7.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down>")?;
        editor.keys("<C-A-k>")?;
        editor.keys("<down><down><down><down><down>")?;
        editor.keys("<C-A-k>")?;

        // From line 0, jump-prev wraps to the highest mark first.
        editor.keys("<C-home>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;

        editor.keys("<C-A-j>")?;
        editor.expect_cursor_heads(&[(7, 0)])?;

        editor.keys("<C-A-j>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;

        editor.keys("<C-A-j>")?;
        editor.expect_cursor_heads(&[(7, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_k_on_marked_line_clears_the_mark() -> TestResult {
    support::run_x11_test("bookmarks-toggle-clears", |session| {
        let path = session.seed_file("bookmarks-toggle-clears.txt", &ten_line_fixture())?;
        let mut editor = session.open_file("bookmarks-toggle-clears", &path)?;

        // Mark line 5, then toggle again to clear it.
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><down><down><down><down>")?;
        editor.keys("<C-A-k>")?;
        editor.keys("<C-A-k>")?;

        // From line 0, jump-next has nothing to find. Caret stays put.
        editor.keys("<C-home>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;

        editor.keys("<C-A-l>")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn bookmarks_track_inserted_lines_and_restore_on_undo() -> TestResult {
    support::run_x11_test("bookmarks-track-edits", |session| {
        let path = session.seed_file("bookmarks-track-edits.txt", "a\nb\nc")?;
        let mut editor = session.open_file("bookmarks-track-edits", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down><C-A-k><C-home><enter>")?;

        editor.keys("<C-home><C-A-l>")?;
        editor.expect_cursor_heads(&[(2, 0)])?;

        editor.keys("<C-z><C-home><C-A-l>")?;
        editor.expect_cursor_heads(&[(1, 0)])?;
        editor.save_then_expect_file(&path, "a\nb\nc")?;
        Ok(())
    })
}
