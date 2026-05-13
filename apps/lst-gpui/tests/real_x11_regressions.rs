//! Real-display regression tests for keyboard bugs that cut across one-off
//! feature suites. Add focused cases here when a review or production bug
//! needs true X11 delivery coverage before the fix.
//!
//! Run with
//!
//!     cargo nextest run --profile x11-regression -p lst-gpui --run-ignored only

mod support;

use lst_x11_harness::{ChordMods, FileWaitOpts, Key, KeyChord};

use support::{secs, EditorTestExt, TestResult};

#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
#[cfg(unix)]
use std::time::Duration;

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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn undo_after_save_marks_buffer_dirty_again() -> TestResult {
    support::run_x11_test("regression-save-undo-dirty", |session| {
        let path = session.seed_file("save-undo-dirty.txt", "old")?;
        let mut editor = session.open_file("regression-save-undo-dirty", &path)?;

        editor.keys("new ")?;
        editor.save()?;
        editor.wait_state("save clears dirty", secs(5), |record| {
            !record.active_tab_modified
        })?;
        editor.keys("<C-z>")?;
        editor.wait_state("undo after save dirties buffer", secs(5), |record| {
            record.active_tab_modified
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn crlf_line_ending_is_not_split_by_right_motion_and_insert() -> TestResult {
    support::run_x11_test("regression-crlf-motion-insert", |session| {
        let path = session.seed_file("crlf.txt", "a\r\nb")?;
        let mut editor = session.open_file("regression-crlf-motion-insert", &path)?;

        editor.keys("<end><right>X")?;
        editor.save_then_expect_file(&path, "a\r\nXb")?;
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

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_through_symlink_updates_target_without_replacing_link() -> TestResult {
    support::run_x11_test("regression-save-symlink", |session| {
        let target = session.seed_file("symlink-target.txt", "target\n")?;
        let link = session.root().join("symlink-link.txt");
        symlink(&target, &link)?;
        let mut editor = session.open_file("regression-save-symlink", &link)?;

        editor.keys("linked ")?;
        editor.save_then_expect_file(&target, "linked target\n")?;
        assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_safe_save_keeps_existing_file_contents() -> TestResult {
    support::run_x11_test("regression-save-failure-preserves-file", |session| {
        let dir = session.root().join("locked");
        std::fs::create_dir(&dir)?;
        let path = dir.join("note.txt");
        std::fs::write(&path, "old\n")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))?;
        let mut editor = session.open_file("regression-save-failure-preserves-file", &path)?;

        editor.keys("new ")?;
        editor.save()?;
        editor.wait_file_text(
            &path,
            "old\n",
            FileWaitOpts::new(secs(2), Duration::from_millis(300)),
        )?;
        let record = editor.read_state()?;
        let text = std::fs::read_to_string(&path)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))?;

        assert_eq!(text, "old\n");
        assert!(record.active_tab_modified, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn recent_panel_open_and_query_are_visible_in_state_trace() -> TestResult {
    support::run_x11_test("regression-recent-trace", |session| {
        let path = session.seed_file("recent-trace.txt", "recent body\n")?;
        let mut editor = session.open_file("regression-recent-trace", &path)?;

        editor.keys("<C-r>")?;
        editor.wait_state("recent panel trace opens", secs(5), |record| {
            record.recent_panel_open && record.focused_input == "recent_query"
        })?;
        editor.keys("recent")?;
        editor.wait_state("recent panel query traces", secs(5), |record| {
            record.recent_panel_open && record.recent_panel_query.as_deref() == Some("recent")
        })?;
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
