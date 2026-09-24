//! Real-display specifications for closing tabs and quitting: dirty-document
//! prompts, the multi-document quit review, save failures during a quit, and
//! what closing a scratchpad leaves on disk and in the X11 selections.

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Key, KeyChord, Selection,
};
use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use support::{close_active_tab_keeping_window, path_text, secs, EditorTestExt, TestResult};

const SCRATCHPAD_TEXT: &str = "quit clipboard smoke";

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn multi_dirty_quit_review_owns_focus_and_saves_every_selected_file() -> TestResult {
    support::run_x11_test("quit-review-save-selected", |session| {
        let first = session.seed_file("first.txt", "first original")?;
        let second = session.seed_file("second.txt", "second original")?;
        let first_identity = path_text(&first);
        let second_identity = path_text(&second);
        let mut editor = session.open_files("quit-review-save", &[first.clone(), second.clone()])?;

        editor.keys("<C-a>first edited<C-tab><C-a>second edited<C-q>")?;
        let review = editor.wait_state("multi dirty quit review", secs(3), |record| {
            record.quit_review_open
                && record.focused_input == "quit_review"
                && record.quit_review_items.len() == 2
                && record.quit_review_items.iter().all(|item| item.decision == "save")
                && record.quit_review_items.iter().all(|item| item.status == "pending")
        })?;
        assert_eq!(
            review
                .quit_review_items
                .iter()
                .map(|item| item.identity.as_str())
                .collect::<Vec<_>>(),
            vec![first_identity.as_str(), second_identity.as_str()]
        );

        let untouched = |record: &lst_x11_harness::StateTraceRecord| {
            record.revision == review.revision
                && record.active_tab_id == review.active_tab_id
                && record.quit_review_open
                && record
                    .quit_review_items
                    .iter()
                    .all(|item| item.decision == "save" && item.status == "pending")
        };
        editor.expect_keys_ignored(
            "x<C-n><C-tab>",
            "<down>",
            |record| record.quit_review_selected_index == Some(1),
            untouched,
        )?;
        // Only plain d, s, and Enter decide; modified variants must not.
        editor.expect_keys_ignored(
            "<S-d><A-d><C-d><S-enter><A-enter><C-enter>",
            "<up>",
            |record| record.quit_review_selected_index == Some(0),
            untouched,
        )?;
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");

        editor.press(KeyChord::Key(Key::Enter))?;
        editor.wait_for_successful_exit(secs(10))?;
        assert_eq!(fs::read_to_string(&first)?, "first edited");
        assert_eq!(fs::read_to_string(&second)?, "second edited");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quit_review_choices_can_be_toggled_cancelled_and_discarded() -> TestResult {
    support::run_x11_test("quit-review-decisions", |session| {
        let first = session.seed_file("first.txt", "first original")?;
        let second = session.seed_file("second.txt", "second original")?;
        let mut editor = session.open_files("quit-review-decisions", &[first.clone(), second.clone()])?;

        editor.keys("<C-a>first edited<C-tab><C-a>second edited<C-q><space>")?;
        editor.wait_state("first item toggled to discard", secs(3), |record| {
            record.quit_review_open
                && record.quit_review_selected_index == Some(0)
                && record
                    .quit_review_items
                    .first()
                    .is_some_and(|item| item.decision == "discard")
        })?;

        editor.keys("<esc>")?;
        editor.wait_state("quit review cancelled", secs(3), |record| {
            !record.quit_review_open && record.focused_input == "editor"
        })?;
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");

        editor.keys("<C-q>")?;
        editor.wait_state("quit review reopened with fresh defaults", secs(3), |record| {
            record.quit_review_open && record.quit_review_items.iter().all(|item| item.decision == "save")
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(secs(10))?;
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_failure_keeps_quit_review_visible_and_documents_open() -> TestResult {
    support::run_x11_test("quit-review-save-failure", |session| {
        let first = session.seed_file("first.txt", "first original")?;
        let second = session.seed_file("second.txt", "second original")?;
        let root = session.root().to_path_buf();
        let mut editor = session.open_files("quit-review-failure", &[first.clone(), second.clone()])?;
        editor.keys("<C-a>first edited<C-tab><C-a>second edited<C-q>")?;
        editor.wait_state("quit review ready", secs(3), |record| record.quit_review_open)?;

        let restore = support::RestorePermissions::set_mode(&root, 0o500)?;

        editor.keys("<enter>")?;
        let failed = editor.wait_state("quit review save failure", secs(5), |record| {
            record.quit_review_open
                && record.focused_input == "quit_review"
                && record
                    .quit_review_items
                    .iter()
                    .any(|item| item.status == "failed" && item.error.is_some())
        })?;
        assert!(failed.quit_review_message.is_some(), "{failed:?}");
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");

        drop(restore);
        editor.keys("<esc>")?;
        editor.wait_state("failed review cancelled", secs(3), |record| !record.quit_review_open)?;
        editor.keys("<C-q>")?;
        editor.wait_state("review reopened after failure", secs(3), |record| {
            record.quit_review_open
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_a_dirty_file_requires_an_explicit_decision() -> TestResult {
    support::run_x11_test("close-prompt", |session| {
        let path = session.seed_file("close-me.txt", "body")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("close-prompt", &path)?;

        editor.keys(" changed<C-w>")?;
        let prompted = editor.wait_state("dirty close prompt", secs(2), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
                && record.close_prompt_status.as_deref() == Some("reviewing")
                && record.active_tab_modified
        })?;

        // Only plain d, s, and Enter decide; modified variants must neither
        // save, discard, nor leave the prompt before Escape cancels it.
        editor.expect_keys_ignored(
            "<S-d><A-d><C-d><S-enter><A-enter><C-enter>",
            "<esc>",
            |record| record.close_prompt_file.is_none(),
            |record| {
                record.active_tab_id == prompted.active_tab_id
                    && record.revision == prompted.revision
                    && record.active_tab_modified
                    && (record.close_prompt_file.is_none()
                        || record.close_prompt_status.as_deref() == Some("reviewing"))
            },
        )?;
        assert_eq!(fs::read_to_string(&path)?, "body");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_w_dirty_file_tab_saves_before_close() -> TestResult {
    support::run_x11_test("close-dirty-tab-save", |session| {
        let path = session.seed_file("dirty-close.txt", "original")?;
        let path_string = path_text(&path);
        let mut editor = session.open_file("dirty-close", &path)?;

        editor.keys("<C-a>saved before close")?;
        editor.keys("<C-n>")?;
        editor.wait_state("sibling tab focused", secs(2), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("dirty file tab focused", secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&path_string)
        })?;

        editor.keys("<C-w><enter>")?;
        editor.wait_state("dirty file tab closed", secs(5), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.expect_file(&path, "saved before close")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_q_dirty_file_saves_before_exit() -> TestResult {
    support::run_x11_test("quit-dirty-file-save", |session| {
        let path = session.seed_file("dirty-quit.txt", "original")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("dirty-file-quit", &path)?;

        editor.keys("<C-a>saved before quit")?;
        editor.keys("<C-q>")?;
        editor.wait_state("dirty file quit prompt", secs(2), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
                && record.close_prompt_status.as_deref() == Some("reviewing")
        })?;
        editor.press(KeyChord::Key(Key::Enter))?;
        editor.wait_for_successful_exit(secs(10))?;

        assert_eq!(fs::read_to_string(&path)?, "saved before quit");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_q_dirty_scratchpad_saves_before_exit() -> TestResult {
    support::run_x11_test("quit-dirty-scratchpad-save", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("scratchpad before quit")?;
        editor.quit_default()?;

        assert_eq!(fs::read_to_string(&path)?, "scratchpad before quit");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn deleted_backing_file_requires_explicit_save_or_discard_on_exit() -> TestResult {
    support::run_x11_test("quit-deleted-backing-file", |session| {
        let path = session.seed_file("deleted-backing-file.txt", "only copy in the editor\n")?;
        let identity = path_text(&path);
        let mut editor = session.open_file("deleted-backing-file", &path)?;

        fs::remove_file(&path)?;
        editor.wait_state("missing backing file", secs(4), |record| {
            record.active_tab_backing_file_missing
                && record.status_message.contains("was deleted")
                && !record.active_tab_modified
        })?;
        editor.keys("<C-q>")?;
        editor.wait_state("missing file quit prompt", secs(2), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
                && record.close_prompt_status.as_deref() == Some("reviewing")
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(secs(10))?;
        assert!(!path.exists());
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display"]
fn discard_after_failed_scratchpad_quit_save_exits_without_retrying_forever() -> TestResult {
    support::run_x11_test("quit-scratchpad-save-failure-discard", |session| {
        let (mut editor, path) = session.open("scratch")?;
        editor.keys("the clipboard remains the fallback copy")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;

        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_state("scratchpad save failure", secs(5), |record| {
            record.close_prompt_status.as_deref() == Some("failed")
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display"]
fn clipboard_owner_retry_preserves_completed_discard_decisions() -> TestResult {
    support::run_x11_test("quit-clipboard-owner-retry", |session| {
        let path = session.seed_file("discard-before-clipboard-retry.txt", "disk original\n")?;
        let identity = path_text(&path);
        let mut editor = session.open_file_with_env(
            "clipboard-owner-retry",
            &path,
            &[(OsStr::new("LST_TEST_CLIPBOARD_OWNER_FAILURE"), OsStr::new("1"))],
        )?;

        editor.keys("<C-a>discard this local edit")?;
        editor.press(KeyChord::Ctrl(Key::Char('n')))?;
        let scratchpad = editor.wait_state("new scratchpad", secs(4), |record| {
            record.active_tab_path.as_deref() != Some(identity.as_str())
        })?;
        let scratchpad_path = scratchpad
            .active_tab_path
            .map(PathBuf::from)
            .ok_or("scratchpad should have a backing path")?;
        editor.keys("scratchpad clipboard fallback")?;

        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_state("regular-file discard prompt", secs(4), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_state("clipboard owner warning", secs(8), |record| {
            record.close_prompt_file.is_none()
                && !record.quit_review_open
                && record.status_message.contains("Press Ctrl+Q again to quit anyway")
        })?;

        // Changing the exact payload makes this a new quit attempt. It must
        // retry clipboard ownership rather than carrying the old bypass into
        // different scratchpad content.
        editor.keys(" changed after warning")?;
        editor.wait_state("changed clipboard payload", secs(4), |record| {
            record.active_tab_modified && !record.status_message.contains("Press Ctrl+Q again")
        })?;
        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_state("discard is reviewed again for a new quit attempt", secs(4), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_state("changed payload retries clipboard owner", secs(8), |record| {
            record.close_prompt_file.is_none()
                && !record.quit_review_open
                && record.status_message.contains("Press Ctrl+Q again to quit anyway")
        })?;
        assert_eq!(
            fs::read_to_string(&scratchpad_path)?,
            "scratchpad clipboard fallback changed after warning"
        );

        editor.press(KeyChord::Ctrl(Key::Char('q')))?;
        editor.wait_for_successful_exit(secs(10))?;
        assert_eq!(fs::read_to_string(path)?, "disk original\n");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_an_empty_scratchpad_removes_its_file() -> TestResult {
    support::run_x11_test("close-empty-scratchpad", |session| {
        let scratchpad_dir = session.root().join("scratch");
        let (mut editor, path) = session.open("scratch")?;
        write_clipboard_text(Selection::Clipboard, "empty clipboard sentinel")?;
        write_clipboard_text(Selection::Primary, "empty primary sentinel")?;

        editor.press(KeyChord::Ctrl(Key::Char('w')))?;
        editor.wait_for_successful_exit(secs(10))?;

        assert!(
            !path.exists(),
            "closing an empty scratchpad should remove {}",
            path.display()
        );
        assert_eq!(support::count_files(&scratchpad_dir)?, 0);
        wait_clipboard_text(Selection::Clipboard, "empty clipboard sentinel", secs(10))?;
        wait_clipboard_text(Selection::Primary, "empty primary sentinel", secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quitting_a_regular_file_preserves_clipboard_and_primary() -> TestResult {
    support::run_x11_test("quit-regular-clipboard", |session| {
        let path = session.seed_file("quit-source.txt", SCRATCHPAD_TEXT)?;
        let editor = session.open_file("text", &path)?;
        write_clipboard_text(Selection::Clipboard, "clipboard sentinel")?;
        write_clipboard_text(Selection::Primary, "primary sentinel")?;

        editor.quit_default()?;

        wait_clipboard_text(Selection::Clipboard, "clipboard sentinel", secs(10))?;
        wait_clipboard_text(Selection::Primary, "primary sentinel", secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quitting_a_nonempty_scratchpad_copies_it_to_both_selections() -> TestResult {
    support::run_x11_test("quit-scratchpad-clipboard", |session| {
        let (mut editor, _path) = session.open("scratch")?;
        editor.keys(SCRATCHPAD_TEXT)?;
        editor.quit_default()?;

        wait_clipboard_text(Selection::Clipboard, SCRATCHPAD_TEXT, secs(10))?;
        wait_clipboard_text(Selection::Primary, SCRATCHPAD_TEXT, secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_a_scratchpad_tab_copies_it_while_other_tabs_remain_open() -> TestResult {
    support::run_x11_test("close-scratchpad-tab-clipboard", |session| {
        let (mut editor, _path) = session.open("scratch")?;
        editor.keys(SCRATCHPAD_TEXT)?;
        close_active_tab_keeping_window(&mut editor)?;

        wait_clipboard_text(Selection::Clipboard, SCRATCHPAD_TEXT, secs(10))?;
        wait_clipboard_text(Selection::Primary, SCRATCHPAD_TEXT, secs(10))?;
        Ok(())
    })
}
