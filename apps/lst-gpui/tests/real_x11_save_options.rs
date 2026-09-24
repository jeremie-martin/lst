//! Accepted real-X11 behavior for save-time text policies.
//!
//! `[files] trim_trailing_whitespace` strips trailing spaces and tabs from
//! every line, and `[files] ensure_final_newline` appends one `\n` to a
//! non-empty document that lacks it. Both default off, preserving Markdown
//! hard breaks and intentional trailing whitespace.
//!
//! These tests edit regular files, which are not autosaved, and every seed
//! differs from the expected result, so the file can only match after the
//! explicit save wrote it.

mod support;

use support::{EditorTestExt, TestResult};

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
