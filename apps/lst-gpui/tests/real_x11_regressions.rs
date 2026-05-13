//! Real-display regression tests for keyboard bugs that cut across one-off
//! feature suites. Add focused cases here when a review or production bug
//! needs true X11 delivery coverage before the fix.
//!
//! Run with
//!
//!     cargo nextest run --profile x11-regression -p lst-gpui --run-ignored only

mod support;

use lst_x11_harness::{ChordMods, Key, KeyChord};

use support::{secs, EditorTestExt, TestResult};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
fn ignored_insert_mode_recent_ctrl_does_not_poison_next_vim_key() -> TestResult {
    support::run_x11_test("regression-ignored-insert-ctrl-clears", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.keys("abc")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        editor.keys("<esc>d")?;
        let record = editor.wait_state("plain vim d pending", secs(2), |record| {
            record.vim_mode == "NORMAL" && record.vim_pending == "d"
        })?;
        assert_eq!(record.vim_pending, "d", "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_trim_updates_visible_buffer_before_followup_typing() -> TestResult {
    support::run_x11_test("regression-save-trim-visible-buffer", |session| {
        let env = [(
            std::ffi::OsStr::new("LST_SAVE_TRIM_TRAILING_WS"),
            std::ffi::OsStr::new("1"),
        )];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys("alpha   ")?;
        editor.save_then_expect_file(&path, "alpha")?;
        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alphaX")?;
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_preserves_existing_executable_mode() -> TestResult {
    support::run_x11_test("regression-save-preserves-mode", |session| {
        let path = session.seed_file("script.sh", "#!/bin/sh\necho hi\n")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        let mut editor = session.open_file("regression-save-preserves-mode", &path)?;

        editor.keys("#")?;
        editor.save_then_expect_file(&path, "##!/bin/sh\necho hi\n")?;
        let mode = std::fs::metadata(&path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_reopen_drops_bad_entry_so_older_closed_tab_can_reopen() -> TestResult {
    support::run_x11_test("regression-reopen-failed-advances", |session| {
        let older = session.seed_file("older.txt", "older\n")?;
        let missing = session.seed_file("missing.txt", "missing\n")?;
        let anchor = session.seed_file("anchor.txt", "anchor\n")?;
        let older_string = older.to_string_lossy().into_owned();
        let missing_string = missing.to_string_lossy().into_owned();
        let anchor_string = anchor.to_string_lossy().into_owned();
        let mut editor = session.open_files(
            "regression-reopen-failed-advances",
            &[older.clone(), missing.clone(), anchor.clone()],
        )?;

        editor.wait_state("older active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&older_string)
        })?;
        editor.keys("<C-w>")?;
        editor.wait_state("missing active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&missing_string)
        })?;
        editor.keys("<C-w>")?;
        editor.wait_state("anchor active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&anchor_string)
        })?;

        std::fs::remove_file(&missing)?;
        editor.keys("<C-S-t>")?;
        editor.wait_state("failed reopen leaves anchor active", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&anchor_string)
        })?;
        editor.keys("<C-S-t>")?;
        editor.wait_state("older tab reopened", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&older_string)
        })?;
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
