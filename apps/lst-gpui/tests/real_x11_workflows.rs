//! Real-display tests for the go-to-line panel.

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn goto_line_column_moves_to_requested_column_and_clamps() -> TestResult {
    support::run_x11_test("workflow-goto-line-column-clamp", |session| {
        let path = session.seed_file("goto-column.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("goto-column", &path)?;

        // Submitting returns focus to the editor, so typing lands at the
        // requested position.
        editor.keys("<C-g>2:3<enter>X")?;
        editor.expect_cursor_heads(&[(1, 3)])?;

        editor.keys("<C-g>2:99<enter>")?;
        editor.expect_cursor_heads(&[(1, 5)])?;

        editor.keys("<C-g>99:2<enter>")?;
        editor.expect_cursor_heads(&[(2, 1)])?;
        editor.save_then_expect_file(&path, "alpha\nbeXta\ngamma")?;
        Ok(())
    })
}
