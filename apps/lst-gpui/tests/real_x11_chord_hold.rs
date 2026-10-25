//! Real-display tests for the chord-hold notation (`<C-{k d}>`). These
//! validate that the harness emits the correct XTEST event train when a
//! modifier is held across multiple keystrokes — important for
//! chord-prefix bindings like VSCode's `Ctrl+K Ctrl+D` style sequences, and
//! that an unrelated shortcut clears a pending `Ctrl+K` prefix.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_chord_hold --run-ignored only

mod support;

use lst_x11_harness::{clipboard::wait_clipboard_text, Key, KeyChord, Selection};

use support::{secs, EditorTestExt, HeldKeys, TestResult};

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

        editor.keys("foo foo foo<C-home><C-d><C-d>")?;
        // Two cursors at this point. Ctrl+K Ctrl+D should skip the
        // current match (the second "foo") and add the third occurrence's
        // selection. Replacing all selections with "bar" then yields
        // "bar foo bar".
        editor.keys("<C-{k d}>bar")?;
        editor.save_then_expect_file(&path, "bar foo bar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn held_control_shift_repeats_a_shifted_symbol_shortcut() -> TestResult {
    // Observe every transition while Control and Shift remain held: the final
    // position alone cannot distinguish three successful presses from one.
    support::run_x11_test("chord-hold-shifted-symbol", |session| {
        let path = session.seed_file("brackets.txt", "x(abc)y")?;
        let mut editor = session.open_file("chord-hold-shifted-symbol", &path)?;
        editor.keys("<C-home><right><C-S-\\>")?;
        editor.expect_cursor_heads(&[(0, 5)])?;

        let mut held = HeldKeys::new()?;
        held.press(0xffe3)?; // Control_L
        held.press(0xffe1)?; // Shift_L
        for column in [1, 5, 1] {
            editor.press(KeyChord::Key(Key::Char('\\')))?;
            editor.expect_cursor_heads(&[(0, column)])?;
        }
        held.release_all()?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_k_prefix_is_cleared_by_unrelated_selection_shortcut() -> TestResult {
    support::run_x11_test("regression-ctrl-k-stale-prefix", |session| {
        let path = session.seed_file("ctrl-k-stale-prefix.txt", "foo foo foo")?;
        let mut editor = session.open_file("ctrl-k-stale-prefix", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-k><S-right><C-d>")?;
        editor.wait_state("Ctrl+D adds the next match of the selection", secs(5), |record| {
            let ranges = record
                .cursors
                .iter()
                .map(|cursor| {
                    (
                        cursor.anchor_char.min(cursor.head_char),
                        cursor.anchor_char.max(cursor.head_char),
                    )
                })
                .collect::<Vec<_>>();
            ranges == [(0, 1), (4, 5)]
        })?;
        Ok(())
    })
}
