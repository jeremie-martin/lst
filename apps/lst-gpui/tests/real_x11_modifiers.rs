//! Real-display tests for single-cursor editing chords: select-all, undo and
//! both redo chords, the redo-branch swap, modified Enter, moving and
//! duplicating lines, and modifier state that must not leak between keys.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_modifiers --run-ignored only

mod support;

use lst_x11_harness::{ChordMods, Key, KeyChord};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_a_select_all_then_type_replaces_buffer() -> TestResult {
    support::run_x11_test("modifier-ctrl-a", |session| {
        let path = session.seed_file("seed.txt", "old content here\nstill old\n")?;
        let mut editor = session.open_file("file", &path)?;

        editor.keys("<C-a>this should replace the existing text")?;
        editor.save_then_expect_file(&path, "this should replace the existing text")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_z_undoes_and_ctrl_y_and_ctrl_shift_z_redo_a_typed_run() -> TestResult {
    // A single word coalesces into one undo group. Each step saves, and each
    // expected text differs from the file the previous step saved.
    support::run_x11_test("modifier-undo-redo", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("hello")?;
        editor.save_then_expect_file(&path, "hello")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "")?;
        editor.keys("<C-y>")?;
        editor.save_then_expect_file(&path, "hello")?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, "")?;
        editor.keys("<C-S-z>")?;
        editor.save_then_expect_file(&path, "hello")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_y_swaps_to_the_abandoned_redo_branch() -> TestResult {
    // Typing after an undo keeps the abandoned redo path as a branch. Ctrl+Y
    // redoes the newer branch; Ctrl+Alt+Y swaps the older one back in.
    support::run_x11_test("modifier-swap-redo-branch", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("a<C-z>b<C-z>")?;
        editor.save_then_expect_file(&path, "")?;

        editor.keys("<C-A-y>")?;
        editor.wait_state("redo branch swapped", secs(5), |record| {
            record.status_message.starts_with("Switched to alternate redo branch")
        })?;
        editor.keys("<C-y>")?;
        editor.save_then_expect_file(&path, "a")?;

        editor.keys("<C-z><C-A-y><C-y>")?;
        editor.save_then_expect_file(&path, "b")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn modified_enter_inserts_an_indented_newline() -> TestResult {
    // Shift+, Ctrl+ and Alt+Enter insert a newline through the smart-indent
    // path, like plain Enter, rather than being dropped or inserting a raw
    // newline at column 0.
    support::run_x11_test("modifier-enter", |session| {
        let path = session.seed_file("indent.txt", "    alpha")?;
        let mut editor = session.open_file("indent", &path)?;

        editor.keys("<end><S-enter>bravo<C-enter>charlie<A-enter>delta")?;
        editor.save_then_expect_file(&path, "    alpha\n    bravo\n    charlie\n    delta")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn alt_down_and_alt_up_move_the_cursor_line() -> TestResult {
    support::run_x11_test("modifier-move-line", |session| {
        let path = session.seed_file("move-line.txt", "one\ntwo\nthree")?;
        let mut editor = session.open_file("move-line", &path)?;

        editor.keys("<C-home><right>")?;
        editor.expect_cursor_heads(&[(0, 1)])?;

        editor.keys("<A-down>")?;
        editor.save_then_expect_file(&path, "two\none\nthree")?;
        editor.expect_cursor_heads(&[(1, 1)])?;

        editor.keys("<A-down>")?;
        editor.save_then_expect_file(&path, "two\nthree\none")?;
        editor.expect_cursor_heads(&[(2, 1)])?;

        editor.keys("<A-up><A-up>")?;
        editor.save_then_expect_file(&path, "one\ntwo\nthree")?;
        editor.expect_cursor_heads(&[(0, 1)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn platform_shift_tab_does_not_run_shift_only_outdent() -> TestResult {
    support::run_x11_test("regression-platform-shift-tab", |session| {
        let path = session.seed_file("platform-shift-tab.txt", "    alpha")?;
        let mut editor = session.open_file("platform-shift-tab", &path)?;

        editor.send_keys_settle("<cmd-S-tab>")?;
        // The file already holds the indented line; the marker proves the
        // buffer still does.
        editor.keys("<end>X")?;
        editor.save_then_expect_file(&path, "    alphaX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn consumed_ctrl_action_does_not_leave_recent_ctrl_for_next_text_key() -> TestResult {
    support::run_x11_test("regression-consumed-action-stale-ctrl", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo foo")?;
        editor.press(KeyChord::Ctrl(Key::Char('s')))?;
        editor.keys("d")?;
        editor.save_then_expect_file(&path, "foo food")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn released_ctrl_then_plain_d_in_insert_mode_inserts_d() -> TestResult {
    support::run_x11_test("regression-released-ctrl-plain-d", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("abc")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        editor.save_then_expect_file(&path, "abcd")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn qwertz_layout_types_literal_z_and_y() -> TestResult {
    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    let _layout = EnvGuard::set("LST_X11_HARNESS_LAYOUT", "de");
    support::run_x11_test("regression-qwertz-literals", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("zy")?;
        editor.save_then_expect_file(&path, "zy")?;
        Ok(())
    })
}

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
