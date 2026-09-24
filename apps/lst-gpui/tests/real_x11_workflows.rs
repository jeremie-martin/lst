//! Real-display tests for common whole-editor workflows that cross input,
//! runtime effects, and file-backed state.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_workflows --run-ignored only

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn goto_line_panel_moves_focus_back_to_editor_after_submit() -> TestResult {
    support::run_x11_test("workflow-goto-line", |session| {
        let path = session.seed_file("goto.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("goto", &path)?;

        editor.keys("<C-g>2:3<enter>X")?;
        editor.save_then_expect_file(&path, "alpha\nbeXta\ngamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn goto_line_column_moves_to_requested_column_and_clamps() -> TestResult {
    support::run_x11_test("workflow-goto-line-column-clamp", |session| {
        let path = session.seed_file("goto-column.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("goto-column", &path)?;

        editor.keys("<C-g>2:3<enter>")?;
        editor.expect_cursor_heads(&[(1, 2)])?;

        editor.keys("<C-g>2:99<enter>")?;
        editor.expect_cursor_heads(&[(1, 4)])?;

        editor.keys("<C-g>99:2<enter>")?;
        editor.expect_cursor_heads(&[(2, 1)])?;
        Ok(())
    })
}
