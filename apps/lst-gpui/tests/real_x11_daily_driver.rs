//! Real-display acceptance coverage for line duplication. `run_x11_nested.py
//! --probe` runs this test as its standard-mode editing check.

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_shift_arrows_duplicate_the_line_above_and_below() -> TestResult {
    support::run_x11_test("daily-driver-duplicate-up-down", |session| {
        let path = session.seed_file("duplicate.txt", "alpha\nbeta")?;
        let mut editor = session.open_file("duplicate", &path)?;

        // The copies are identical, so the caret tells the directions apart:
        // it stays on the upper copy for "above" and follows the lower one
        // for "below".
        editor.click_at_text(1, 2)?;
        editor.keys("<C-A-S-up>")?;
        editor.expect_cursor_heads(&[(1, 2)])?;
        editor.keys("<C-A-S-down>")?;
        editor.expect_cursor_heads(&[(2, 2)])?;
        editor.save_then_expect_file(&path, "alpha\nbeta\nbeta\nbeta")?;
        Ok(())
    })
}
