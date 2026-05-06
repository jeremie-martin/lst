//! Real-display tests for the chord-hold notation (`<C-{k d}>`). These
//! validate that the harness emits the correct XTEST event train when a
//! modifier is held across multiple keystrokes — important for
//! chord-prefix bindings like VSCode's `Ctrl+K Ctrl+D` style sequences.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_chord_hold --run-ignored only

mod support;

use lst_x11_harness::{clipboard::wait_clipboard_text, Selection};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_held_across_two_keystrokes_dispatches_each_chord_separately() -> TestResult {
    // Sanity test for the chord-hold path: existing single-Ctrl bindings
    // must still fire when Ctrl is held continuously across two key
    // presses. `<C-{a c}>` should select-all (`Ctrl+A`) and then copy
    // (`Ctrl+C`), with the system clipboard ending up holding the buffer
    // text. If the harness sent Ctrl-up between A and C, the second
    // chord would land as a literal `c` and the clipboard would be empty.
    support::run_x11_test("chord-hold-select-all-then-copy", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.keys("hello chord-hold")?;
        editor.keys("<C-{a c}>")?;
        wait_clipboard_text(Selection::Clipboard, "hello chord-hold", secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_k_ctrl_d_skip_via_held_modifier_grows_selection_set() -> TestResult {
    // Sends the chord-hold form so the editor sees the same held-Ctrl
    // two-key prefix event train that XTEST produces.
    support::run_x11_test("chord-hold-ctrl-k-ctrl-d", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo foo foo<esc>0i<C-d><C-d>")?;
        // Two cursors at this point. Ctrl+K Ctrl+D should skip the
        // current match (the second "foo") and add the third occurrence's
        // selection. Replacing all selections with "bar" then yields
        // "bar foo bar".
        editor.keys("<C-{k d}>bar")?;
        editor.save_then_expect_file(&path, "bar foo bar")?;
        Ok(())
    })
}
