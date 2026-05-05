//! Real-display tests for the multi-cursor surface. Drives the editor
//! through key sequences and asserts on the autosaved file.
//!
//! Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

mod support;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_adds_occurrences_and_literal_input_replaces_them() -> TestResult {
    // Type "foo bar foo baz foo" in Insert, drop to Normal, return to the
    // first character, re-enter Insert at column 0, then add the next two
    // occurrences with Ctrl+D and replace them all by typing "qux".
    //
    // The first Ctrl+D selects "foo" at the cursor (word-under-cursor
    // fallback). The next two Ctrl+D's add the second and third
    // occurrences. Typing "qux" replaces every selection through the
    // multi-cursor edit path, so the autosave file ends up as
    // "qux bar qux baz qux".
    support::run_x11_test("multi-cursor-ctrl-d", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo bar foo baz foo<esc>0i<C-d><C-d><C-d>qux")?;
        editor.save_then_expect_file(&path, "qux bar qux baz qux")?;
        Ok(())
    })
}
