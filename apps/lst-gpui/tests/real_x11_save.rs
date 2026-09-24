//! Accepted real-X11 behavior for saving files: explicit saves, save-time
//! text policies, the autosave policy, and save failures.
//!
//! `[files] trim_trailing_whitespace` strips trailing spaces and tabs from
//! every line, and `[files] ensure_final_newline` appends one `\n` to a
//! non-empty document that lacks it. Both default off, preserving Markdown
//! hard breaks and intentional trailing whitespace.
//!
//! These tests edit regular files, which are not autosaved by default, and
//! every expected result differs from the file on disk, so the file can only
//! match after a save wrote it.

mod support;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;

use support::{path_text, secs, EditorTestExt, TestResult};

const TRIM_SETTINGS: &str = "version = 1\n[files]\ntrim_trailing_whitespace = true\n";
const FINAL_NEWLINE_SETTINGS: &str = "version = 1\n[files]\nensure_final_newline = true\n";

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn default_save_keeps_trailing_whitespace_and_missing_final_newline() -> TestResult {
    support::run_x11_test("save-policies-default-off", |session| {
        let path = session.seed_file("notes.md", "")?;
        let mut editor = session.open_file("save-policies-default-off", &path)?;

        editor.keys("hard break  <enter>last")?;
        editor.save_then_expect_file(&path, "hard break  \nlast")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn trim_trailing_whitespace_strips_spaces_and_tabs_per_line() -> TestResult {
    support::run_x11_test("save-trim-strips", |session| {
        session.seed_settings(TRIM_SETTINGS)?;
        let path = session.seed_file("trim.txt", "")?;
        let mut editor = session.open_file("save-trim-strips", &path)?;

        editor.keys("alpha   <enter>beta\t\t<enter>gamma")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\ngamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn trim_on_save_updates_the_visible_buffer_before_followup_typing() -> TestResult {
    support::run_x11_test("save-trim-visible-buffer", |session| {
        session.seed_settings(TRIM_SETTINGS)?;
        let path = session.seed_file("trim.txt", "")?;
        let mut editor = session.open_file("save-trim-visible-buffer", &path)?;

        editor.keys("alpha   ")?;
        editor.save_then_expect_file(&path, "alpha")?;
        // The buffer itself must hold the trimmed text; a stale buffer would
        // save "alpha   X" here.
        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alphaX")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ensure_final_newline_appends_exactly_one_when_missing() -> TestResult {
    support::run_x11_test("save-final-newline-append", |session| {
        session.seed_settings(FINAL_NEWLINE_SETTINGS)?;
        let path = session.seed_file("final-newline.txt", "")?;
        let mut editor = session.open_file("save-final-newline-append", &path)?;

        editor.keys("alpha<enter>beta")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ensure_final_newline_keeps_an_existing_terminator_single() -> TestResult {
    support::run_x11_test("save-final-newline-idempotent", |session| {
        session.seed_settings(FINAL_NEWLINE_SETTINGS)?;
        let path = session.seed_file("final-newline.txt", "old")?;
        let mut editor = session.open_file("save-final-newline-idempotent", &path)?;

        editor.keys("<C-a>alpha<enter>beta<enter>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ensure_final_newline_leaves_an_emptied_document_empty() -> TestResult {
    support::run_x11_test("save-final-newline-empty", |session| {
        session.seed_settings(FINAL_NEWLINE_SETTINGS)?;
        let path = session.seed_file("final-newline.txt", "old\n")?;
        let mut editor = session.open_file("save-final-newline-empty", &path)?;

        editor.keys("<C-a><delete>")?;
        editor.save_then_expect_file(&path, "")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn file_edit_save_quit_and_reopen_preserves_disk_contents() -> TestResult {
    support::run_x11_test("save-file-reopen", |session| {
        let path = session.seed_file("roundtrip.txt", "original contents\n")?;

        let mut editor = session.open_file("first-open", &path)?;
        editor.keys("<C-a>saved through the real window")?;
        editor.save_then_expect_file(&path, "saved through the real window")?;
        editor.quit_default()?;

        // Appending proves the reopened buffer holds the saved text.
        let mut editor = session.open_file("second-open", &path)?;
        editor.keys("<C-end>X")?;
        editor.save_then_expect_file(&path, "saved through the real windowX")?;
        editor.quit_default()?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn undo_after_save_marks_buffer_dirty_again() -> TestResult {
    support::run_x11_test("save-undo-dirty", |session| {
        let path = session.seed_file("save-undo-dirty.txt", "old")?;
        let mut editor = session.open_file("save-undo-dirty", &path)?;

        editor.keys("new ")?;
        editor.save()?;
        editor.wait_state("save clears dirty", secs(5), |record| !record.active_tab_modified)?;
        editor.keys("<C-z>")?;
        editor.wait_state("undo after save dirties buffer", secs(5), |record| {
            record.active_tab_modified
        })?;
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_preserves_existing_executable_mode() -> TestResult {
    support::run_x11_test("save-preserves-mode", |session| {
        let path = session.seed_file("script.sh", "#!/bin/sh\necho hi\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        let mut editor = session.open_file("save-preserves-mode", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("#")?;
        editor.save_then_expect_file(&path, "##!/bin/sh\necho hi\n")?;
        let mode = fs::metadata(&path)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_through_symlink_updates_target_without_replacing_link() -> TestResult {
    support::run_x11_test("save-symlink", |session| {
        let target = session.seed_file("symlink-target.txt", "target\n")?;
        let link = session.root().join("symlink-link.txt");
        symlink(&target, &link)?;
        let mut editor = session.open_file("save-symlink", &link)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("linked ")?;
        editor.save_then_expect_file(&target, "linked target\n")?;
        assert!(fs::symlink_metadata(&link)?.file_type().is_symlink());
        Ok(())
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_save_reports_the_error_and_keeps_the_file_and_the_edits() -> TestResult {
    support::run_x11_test("save-failure", |session| {
        let dir = session.root().join("locked");
        fs::create_dir(&dir)?;
        let path = dir.join("note.txt");
        fs::write(&path, "old\n")?;
        let failure = format!("Failed to save {}", path_text(&path));
        let mut editor = session.open_file("save-failure", &path)?;

        // A safe save writes a sibling temp file, so an unwritable directory
        // fails it even though the file itself is writable.
        let locked = support::RestorePermissions::set_mode(&dir, 0o555)?;
        editor.place_cursor_at_document_start()?;
        editor.keys("new ")?;
        editor.save()?;
        editor.wait_state("save failure reported", secs(5), |record| {
            record.status_message.starts_with(&failure) && record.active_tab_modified
        })?;
        assert_eq!(fs::read_to_string(&path)?, "old\n");

        drop(locked);
        editor.save_then_expect_file(&path, "new old\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ordinary_files_do_not_autosave_by_default() -> TestResult {
    support::run_x11_test("save-no-file-autosave", |session| {
        let path = session.seed_file("manual-save.txt", "original")?;
        let path_string = path_text(&path);
        let mut editor = session.open_file("manual-save", &path)?;

        editor.keys("<C-end> changed")?;
        editor.wait_state("file edited", secs(5), |record| record.active_tab_modified)?;
        // A scratchpad edited after the file autosaves on a later tick, and
        // that tick also saw the file idle, so it would have saved the file
        // too if ordinary files were autosaved.
        editor.keys("<C-n>")?;
        let opened = editor.wait_state("scratchpad opened", secs(5), |record| {
            record
                .active_tab_path
                .as_deref()
                .is_some_and(|active| active != path_string)
        })?;
        let scratchpad = PathBuf::from(opened.active_tab_path.ok_or("scratchpad has no path")?);
        editor.keys("tick")?;
        editor.expect_file(&scratchpad, "tick")?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("file still dirty", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(path_string.as_str()) && record.active_tab_modified
        })?;
        assert_eq!(fs::read_to_string(&path)?, "original");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn autosave_all_setting_autosaves_ordinary_files() -> TestResult {
    support::run_x11_test("save-autosave-all", |session| {
        session.seed_settings("version = 1\n[files]\nautosave = \"all\"\n")?;
        let path = session.seed_file("autosaved.txt", "original")?;
        let mut editor = session.open_file("autosave-all", &path)?;

        editor.keys("<C-end> changed")?;
        editor.expect_file(&path, "original changed")?;
        editor.wait_state("autosave clears dirty", secs(5), |record| !record.active_tab_modified)?;
        Ok(())
    })
}
