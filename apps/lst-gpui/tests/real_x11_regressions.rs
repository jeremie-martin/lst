//! Real-display regression tests for keyboard bugs that cut across one-off
//! feature suites. Add focused cases here when a review or production bug
//! needs true X11 delivery coverage before the fix.
//!
//! Run with
//!
//!     cargo nextest run --profile x11-regression -p lst-gpui --run-ignored only

mod support;

use std::{env, ffi::OsString};

use lst_x11_harness::{ChordMods, Key};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_ctrl_d_in_normal_mode_keeps_vim_half_page_motion() -> TestResult {
    support::run_x11_test("regression-vim-ctrl-d", |session| {
        let text = (0..80)
            .map(|line| format!("foo line {line:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = session.seed_file("vim-ctrl-d.txt", &text)?;
        let mut editor = session.open_file("vim-ctrl-d", &path)?;

        editor.click_at_text(0, 0)?;
        editor.send_keys_settle("<esc>")?;
        editor.expect_vim_mode("NORMAL")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        let record = editor.wait_state("vim Ctrl-D moved down", secs(5), |record| {
            record.vim_mode == "NORMAL"
                && matches!(record.cursors.as_slice(), [cursor] if cursor.is_collapsed() && cursor.head_line > 0)
        })?;
        assert_eq!(record.vim_mode, "NORMAL", "{record:?}");
        assert_eq!(record.cursors.len(), 1, "{record:?}");
        assert!(record.cursors[0].is_collapsed(), "{record:?}");
        assert!(record.cursors[0].head_line > 0, "{record:?}");
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
        editor.save_then_expect_file(&path, "    alpha")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip and setxkbmap"]
fn recent_shift_state_does_not_rewrite_committed_ascii_punctuation() -> TestResult {
    let _layout = HarnessLayoutGuard::set_layout("us");
    support::run_x11_test("regression-recent-shift-hyphen", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.key_after_released_modifiers(ChordMods::SHIFT, Key::Char('-'))?;
        editor.save_then_expect_file(&path, "-")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip and setxkbmap"]
fn azerty_shift_digit_preserves_committed_digit() -> TestResult {
    let _layout = HarnessLayoutGuard::set_layout("fr");
    support::run_x11_test("regression-azerty-shift-digit", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.send_keys_settle("7")?;
        editor.save_then_expect_file(&path, "7")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip and setxkbmap"]
fn qwertz_shift_digit_preserves_committed_slash() -> TestResult {
    let _layout = HarnessLayoutGuard::set_layout("de");
    support::run_x11_test("regression-qwertz-shift-slash", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.send_keys_settle("/")?;
        editor.save_then_expect_file(&path, "/")?;
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
        let record = editor.read_state()?;
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
        assert_eq!(ranges, vec![(0, 1), (4, 5)], "{record:?}");
        Ok(())
    })
}

struct HarnessLayoutGuard {
    original: Option<OsString>,
}

impl HarnessLayoutGuard {
    fn set_layout(layout: &str) -> Self {
        let original = env::var_os("LST_X11_HARNESS_LAYOUT");
        env::set_var("LST_X11_HARNESS_LAYOUT", layout);
        Self { original }
    }
}

impl Drop for HarnessLayoutGuard {
    fn drop(&mut self) {
        if let Some(original) = self.original.take() {
            env::set_var("LST_X11_HARNESS_LAYOUT", original);
        } else {
            env::remove_var("LST_X11_HARNESS_LAYOUT");
        }
    }
}
