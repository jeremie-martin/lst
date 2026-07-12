//! Real-display specifications for the multi-document quit transaction.

mod support;

use lst_x11_harness::{Key, KeyChord};
use std::{fs, os::unix::fs::PermissionsExt};

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn multi_dirty_quit_review_owns_focus_and_saves_every_selected_file() -> TestResult {
    support::run_x11_test("quit-review-save-selected", |session| {
        let first = session.seed_file("first.txt", "first original")?;
        let second = session.seed_file("second.txt", "second original")?;
        let first_identity = first.to_string_lossy().into_owned();
        let second_identity = second.to_string_lossy().into_owned();
        let mut editor = session.open_files("quit-review-save", &[first.clone(), second.clone()])?;

        editor.keys("<C-a>first edited<C-tab><C-a>second edited<C-q>")?;
        let review = editor.wait_state("multi dirty quit review", support::secs(3), |record| {
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

        let revision = review.revision;
        editor.send_keys_settle("x")?;
        editor.press(KeyChord::Ctrl(Key::Char('n')))?;
        editor.press(KeyChord::Ctrl(Key::Tab))?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        let blocked = editor.read_state()?;
        assert_eq!(
            blocked.revision, revision,
            "document changed behind quit review: {blocked:?}"
        );
        assert_eq!(blocked.active_tab_id, review.active_tab_id, "{review:?} -> {blocked:?}");
        assert!(blocked.quit_review_open, "{blocked:?}");
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");

        editor.send_keys_settle("<S-d><A-d><C-d><S-enter><A-enter><C-enter>")?;
        let modified_keys_blocked = editor.read_state()?;
        assert!(modified_keys_blocked.quit_review_open, "{modified_keys_blocked:?}");
        assert!(
            modified_keys_blocked
                .quit_review_items
                .iter()
                .all(|item| item.decision == "save" && item.status == "pending"),
            "modified discard/save keys changed the quit review: {modified_keys_blocked:?}"
        );
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");

        editor.press(KeyChord::Key(Key::Enter))?;
        editor.wait_for_successful_exit(support::secs(10))?;
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
        editor.wait_state("first item toggled to discard", support::secs(3), |record| {
            record.quit_review_open
                && record.quit_review_selected_index == Some(0)
                && record
                    .quit_review_items
                    .first()
                    .is_some_and(|item| item.decision == "discard")
        })?;

        editor.keys("<esc>")?;
        editor.wait_state("quit review cancelled", support::secs(3), |record| {
            !record.quit_review_open && record.focused_input == "editor"
        })?;
        assert_eq!(fs::read_to_string(&first)?, "first original");
        assert_eq!(fs::read_to_string(&second)?, "second original");

        editor.keys("<C-q>")?;
        editor.wait_state("quit review reopened with fresh defaults", support::secs(3), |record| {
            record.quit_review_open && record.quit_review_items.iter().all(|item| item.decision == "save")
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(support::secs(10))?;
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
        editor.wait_state("quit review ready", support::secs(3), |record| record.quit_review_open)?;

        let original_permissions = fs::metadata(&root)?.permissions();
        let mut read_only = original_permissions.clone();
        read_only.set_mode(0o500);
        fs::set_permissions(&root, read_only)?;
        let restore = RestorePermissions(root, original_permissions);

        editor.keys("<enter>")?;
        let failed = editor.wait_state("quit review save failure", support::secs(5), |record| {
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
        editor.wait_state("failed review cancelled", support::secs(3), |record| {
            !record.quit_review_open
        })?;
        editor.keys("<C-q>")?;
        editor.wait_state("review reopened after failure", support::secs(3), |record| {
            record.quit_review_open
        })?;
        editor.press(KeyChord::Key(Key::Char('d')))?;
        editor.wait_for_successful_exit(support::secs(10))?;
        Ok(())
    })
}

struct RestorePermissions(std::path::PathBuf, fs::Permissions);

impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, self.1.clone());
    }
}
