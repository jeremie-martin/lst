//! Real-display tests for common whole-editor workflows that cross input,
//! runtime effects, and file-backed state.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_workflows --run-ignored only

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, Key, KeyChord, Selection};
use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use support::{EditorTestExt, FileConflictAction, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn file_edit_save_quit_and_reopen_preserves_disk_contents() -> TestResult {
    support::run_x11_test("workflow-file-reopen", |session| {
        let path = session.seed_file("roundtrip.txt", "original contents\n")?;

        let mut editor = session.open_file("first-open", &path)?;
        editor.keys("<C-a>saved through the real window")?;
        editor.save_then_expect_file(&path, "saved through the real window")?;
        editor.quit_default()?;

        let mut editor = session.open_file("second-open", &path)?;
        editor.keys("<C-a>verified after reopen")?;
        editor.save_then_expect_file(&path, "verified after reopen")?;
        editor.quit_default()?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_v_pastes_system_clipboard_into_editor() -> TestResult {
    support::run_x11_test("workflow-clipboard-paste", |session| {
        let (mut editor, path) = session.open("scratch")?;

        write_clipboard_text(Selection::Clipboard, "clipboard paste\nsecond line")?;
        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "clipboard paste\nsecond line")?;
        Ok(())
    })
}

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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_w_dirty_file_tab_saves_before_close() -> TestResult {
    support::run_x11_test("workflow-dirty-close-save", |session| {
        let path = session.seed_file("dirty-close.txt", "original")?;
        let path_string = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("dirty-close", &path)?;

        editor.keys("<C-a>saved before close")?;
        editor.keys("<C-n>")?;
        editor.wait_state("sibling tab focused", support::secs(2), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("dirty file tab focused", support::secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&path_string)
        })?;

        editor.keys("<C-w><enter>")?;
        editor.wait_state("dirty file tab closed", support::secs(5), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.expect_file(&path, "saved before close")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_q_dirty_file_saves_before_exit() -> TestResult {
    support::run_x11_test("workflow-dirty-file-quit-save", |session| {
        let path = session.seed_file("dirty-quit.txt", "original")?;
        let mut editor = session.open_file("dirty-file-quit", &path)?;

        editor.keys("<C-a>saved before quit")?;
        editor.keys("<C-q>")?;
        let identity = path.to_string_lossy().into_owned();
        editor.wait_state("dirty file quit prompt", support::secs(2), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
                && record.close_prompt_status.as_deref() == Some("reviewing")
        })?;
        editor.press(KeyChord::Key(Key::Enter))?;
        editor.wait_for_successful_exit(support::secs(10))?;

        assert_eq!(fs::read_to_string(&path)?, "saved before quit");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_q_dirty_scratchpad_saves_before_exit() -> TestResult {
    support::run_x11_test("workflow-dirty-scratchpad-quit-save", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("scratchpad before quit")?;
        editor.quit_default()?;

        assert_eq!(fs::read_to_string(&path)?, "scratchpad before quit");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn external_change_uses_a_non_modal_per_tab_banner_and_dismiss_is_version_scoped() -> TestResult {
    support::run_x11_test("workflow-external-conflict-banner", |session| {
        let path = session.seed_file("external-conflict.txt", "original\n")?;
        let identity = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("external-conflict", &path)?;

        editor.keys("local edit")?;
        fs::write(&path, "external version\n")?;
        editor.wait_state("external conflict banner", support::secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.dismiss.is_some()
        })?;

        editor.click_file_conflict_action(FileConflictAction::Dismiss)?;
        editor.wait_state("conflict dismissed", support::secs(2), |record| {
            record.file_conflict_path.is_none() && record.active_tab_modified
        })?;
        editor.expect_file(&path, "external version\n")?;

        // Dismiss prevents background overwrite for this disk version; a
        // later explicit Save is the user's intentional overwrite.
        editor.save_then_expect_file(&path, "original\nlocal edit")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn external_change_banner_reload_action_replaces_local_edits() -> TestResult {
    support::run_x11_test("workflow-external-conflict-reload", |session| {
        let path = session.seed_file("external-reload.txt", "original\n")?;
        let identity = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("external-reload", &path)?;

        editor.keys("local edit")?;
        fs::write(&path, "disk wins\nsecond line\n")?;
        editor.wait_state("external conflict banner", support::secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.reload.is_some()
        })?;
        editor.click_file_conflict_action(FileConflictAction::Reload)?;
        editor.wait_state("disk version reloaded", support::secs(4), |record| {
            record.file_conflict_path.is_none() && !record.active_tab_modified && record.line_count == 3
        })?;

        editor.keys("<C-a>verified reload")?;
        editor.save_then_expect_file(&path, "verified reload")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn external_change_banner_keep_mine_overwrites_only_the_observed_disk_version() -> TestResult {
    support::run_x11_test("workflow-external-conflict-keep-mine", |session| {
        let path = session.seed_file("external-keep-mine.txt", "original\n")?;
        let identity = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("external-keep-mine", &path)?;

        editor.keys("local edit")?;
        fs::write(&path, "external version\n")?;
        editor.wait_state("external conflict banner", support::secs(4), |record| {
            record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.keep_mine.is_some()
        })?;
        editor.click_file_conflict_action(FileConflictAction::KeepMine)?;
        editor.expect_file(&path, "original\nlocal edit")?;
        editor.wait_state("local version saved", support::secs(4), |record| {
            record.file_conflict_path.is_none() && !record.active_tab_modified
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn clean_external_change_reloads_in_place_without_a_prompt() -> TestResult {
    support::run_x11_test("workflow-clean-external-reload", |session| {
        let path = session.seed_file("clean-external-reload.txt", "original\n")?;
        let mut editor = session.open_file("clean-external-reload", &path)?;

        fs::write(&path, "disk version\nsecond line\n")?;
        editor.wait_state("clean external reload", support::secs(4), |record| {
            record.file_conflict_path.is_none()
                && !record.active_tab_modified
                && record.line_count == 3
                && record.status_message.starts_with("Reloaded ")
        })?;
        editor.keys("<C-a>verified clean reload")?;
        editor.save_then_expect_file(&path, "verified clean reload")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn deleted_backing_file_requires_explicit_save_or_discard_on_exit() -> TestResult {
    support::run_x11_test("workflow-deleted-backing-file", |session| {
        let path = session.seed_file("deleted-backing-file.txt", "only copy in the editor\n")?;
        let identity = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("deleted-backing-file", &path)?;

        fs::remove_file(&path)?;
        editor.wait_state("missing backing file", support::secs(4), |record| {
            record.active_tab_backing_file_missing
                && record.status_message.contains("was deleted")
                && !record.active_tab_modified
        })?;
        editor.keys("<C-q>")?;
        editor.wait_state("missing file quit prompt", support::secs(2), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
                && record.close_prompt_status.as_deref() == Some("reviewing")
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(support::secs(10))?;
        assert!(!path.exists());
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn reappearing_deleted_file_conflicts_instead_of_replacing_the_editor_copy() -> TestResult {
    support::run_x11_test("workflow-reappearing-backing-file", |session| {
        let path = session.seed_file("reappearing-backing-file.txt", "only copy in the editor\n")?;
        let identity = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("reappearing-backing-file", &path)?;

        fs::remove_file(&path)?;
        editor.wait_state("missing backing file", support::secs(4), |record| {
            record.active_tab_backing_file_missing
        })?;
        fs::write(&path, "a different recreated file\n")?;
        editor.wait_state("reappeared file conflict", support::secs(4), |record| {
            record.active_tab_backing_file_missing
                && record.file_conflict_path.as_deref() == Some(identity.as_str())
                && record.file_conflict_button_bounds_px.keep_mine.is_some()
        })?;
        assert_eq!(fs::read_to_string(&path)?, "a different recreated file\n");

        editor.click_file_conflict_action(FileConflictAction::KeepMine)?;
        editor.expect_file(&path, "only copy in the editor\n")?;
        editor.wait_state("editor copy saved", support::secs(4), |record| {
            !record.active_tab_backing_file_missing
                && record.file_conflict_path.is_none()
                && !record.active_tab_modified
        })?;
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display"]
fn discard_after_failed_scratchpad_quit_save_exits_without_retrying_forever() -> TestResult {
    support::run_x11_test("workflow-scratchpad-save-failure-discard", |session| {
        let (mut editor, path) = session.open("scratch")?;
        editor.keys("the clipboard remains the fallback copy")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;

        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_state("scratchpad save failure", support::secs(5), |record| {
            record.close_prompt_status.as_deref() == Some("failed")
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(support::secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn clipboard_owner_retry_preserves_completed_discard_decisions() -> TestResult {
    support::run_x11_test("workflow-clipboard-owner-retry", |session| {
        let path = session.seed_file("discard-before-clipboard-retry.txt", "disk original\n")?;
        let identity = path.to_string_lossy().into_owned();
        let mut editor = session.open_file_with_env(
            "clipboard-owner-retry",
            &path,
            &[(OsStr::new("LST_TEST_CLIPBOARD_OWNER_FAILURE"), OsStr::new("1"))],
        )?;

        editor.keys("<C-a>discard this local edit")?;
        editor.press(KeyChord::Ctrl(Key::Char('n')))?;
        let scratchpad = editor.wait_state("new scratchpad", support::secs(4), |record| {
            record.active_tab_path.as_deref() != Some(identity.as_str())
        })?;
        let scratchpad_path = scratchpad
            .active_tab_path
            .map(PathBuf::from)
            .ok_or("scratchpad should have a backing path")?;
        editor.keys("scratchpad clipboard fallback")?;

        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_state("regular-file discard prompt", support::secs(4), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_state("clipboard owner warning", support::secs(8), |record| {
            record.close_prompt_file.is_none()
                && !record.quit_review_open
                && record.status_message.contains("Press Ctrl+Q again to quit anyway")
        })?;

        // Changing the exact payload makes this a new quit attempt. It must
        // retry clipboard ownership rather than carrying the old bypass into
        // different scratchpad content.
        editor.keys(" changed after warning")?;
        editor.wait_state("changed clipboard payload", support::secs(4), |record| {
            record.active_tab_modified && !record.status_message.contains("Press Ctrl+Q again")
        })?;
        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_state(
            "discard is reviewed again for a new quit attempt",
            support::secs(4),
            |record| record.close_prompt_file.as_deref() == Some(identity.as_str()),
        )?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_state("changed payload retries clipboard owner", support::secs(8), |record| {
            record.close_prompt_file.is_none()
                && !record.quit_review_open
                && record.status_message.contains("Press Ctrl+Q again to quit anyway")
        })?;
        assert_eq!(
            fs::read_to_string(&scratchpad_path)?,
            "scratchpad clipboard fallback changed after warning"
        );

        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_for_successful_exit(support::secs(10))?;
        assert_eq!(fs::read_to_string(path)?, "disk original\n");
        Ok(())
    })
}
