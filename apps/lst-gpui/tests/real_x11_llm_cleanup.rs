//! Real-display tests for the command-palette LLM cleanup action. The editor binary is launched with
//! `LST_LLM_FAKE_RESPONSE` exported, which activates an in-process fake
//! `LlmClient` so the test never reaches the real DeepSeek API. Coverage:
//!
//! - whole-buffer cleanup requires an explicit in-app confirmation
//! - selection-only cleanup replaces just the selection; Ctrl+Z restores
//! - cancelling whole-buffer cleanup leaves the document untouched
//! - quitting waits for an in-flight cleanup instead of racing its result
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_llm_cleanup --run-ignored only

mod support;

use std::ffi::OsStr;

use support::{secs, EditorTestExt, TestResult};

const FAKE_ENV: &str = "LST_LLM_FAKE_RESPONSE";

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cleanup_replaces_whole_buffer_inline_with_atomic_undo() -> TestResult {
    support::run_x11_test("llm-cleanup-whole", |session| {
        let original = "um, hello, world";
        let canned = "Hello, world.";
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(FAKE_ENV), OsStr::new(canned))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys(original)?;
        editor.save_then_expect_file(&path, original)?;

        let before = editor.read_state()?;
        editor.keys("<C-S-p>clean up text<enter>")?;
        let confirmation = editor.wait_state("whole-document cleanup confirmation", secs(3), |record| {
            record.cleanup_confirmation_open && record.focused_input == "cleanup_confirmation"
        })?;
        editor.press(lst_x11_harness::KeyChord::Ctrl(lst_x11_harness::Key::Char('n')))?;
        editor.send_keys_settle("<S-enter>")?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        let blocked = editor.read_state()?;
        assert!(blocked.cleanup_confirmation_open, "{blocked:?}");
        assert_eq!(blocked.active_tab_id, confirmation.active_tab_id, "{blocked:?}");
        editor.keys("<enter>")?;
        editor.wait_state("cleanup applied", secs(5), |record| record.revision > before.revision)?;
        editor.save_then_expect_file(&path, canned)?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn whole_document_cleanup_confirmation_can_be_cancelled_without_editing() -> TestResult {
    support::run_x11_test("llm-cleanup-cancel", |session| {
        let original = "um, hello, world";
        let canned = "Hello, world.";
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(FAKE_ENV), OsStr::new(canned))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys(original)?;
        editor.save_then_expect_file(&path, original)?;

        let before = editor.read_state()?;
        editor.keys("<C-S-p>clean up text<enter>")?;
        editor.wait_state("whole-document cleanup confirmation", secs(3), |record| {
            record.cleanup_confirmation_open && record.focused_input == "cleanup_confirmation"
        })?;
        editor.keys("<escape>")?;
        editor.wait_state("cleanup confirmation cancelled", secs(3), |record| {
            !record.cleanup_confirmation_open && record.revision == before.revision
        })?;
        editor.expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cleanup_replaces_only_the_selection_with_atomic_undo() -> TestResult {
    support::run_x11_test("llm-cleanup-selection", |session| {
        let original = "before\num middle\nafter";
        let cleaned = "before\nmiddle\nafter";
        let canned = "middle";
        let env: [(&OsStr, &OsStr); 1] = [(OsStr::new(FAKE_ENV), OsStr::new(canned))];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys("before<enter>um middle<enter>after")?;
        editor.save_then_expect_file(&path, original)?;

        // Select line 2 ("um middle") without including the surrounding newlines.
        editor.keys("<C-home><down><S-end>")?;

        let before = editor.read_state()?;
        editor.keys("<C-S-p>clean up text<enter>")?;
        editor.wait_state("cleanup applied", secs(5), |record| record.revision > before.revision)?;
        editor.save_then_expect_file(&path, cleaned)?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quitting_waits_for_inflight_cleanup_and_keeps_its_result_visible() -> TestResult {
    support::run_x11_test("llm-cleanup-quit-race", |session| {
        let original = "um, keep this result";
        let canned = "Keep this result.";
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new(FAKE_ENV), OsStr::new(canned)),
            (OsStr::new("LST_LLM_FAKE_DELAY_MS"), OsStr::new("1500")),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys(original)?;
        editor.save_then_expect_file(&path, original)?;
        editor.keys("<C-a><C-S-p>clean up text<enter>")?;
        let before = editor.wait_state("cleanup started", secs(3), |record| {
            record.status_message.contains("Cleaning")
        })?;

        editor.press(lst_x11_harness::KeyChord::Ctrl(lst_x11_harness::Key::Char('q')))?;
        let blocked = editor.wait_state("quit waits for cleanup", secs(3), |record| {
            record.status_message == "Wait for text cleanup to finish before quitting."
        })?;
        assert_eq!(blocked.revision, before.revision, "{blocked:?}");
        assert!(blocked.close_prompt_file.is_none(), "{blocked:?}");
        assert!(!blocked.quit_review_open, "{blocked:?}");

        editor.wait_state("cleanup result remains visible", secs(5), |record| {
            record.revision > before.revision
        })?;
        editor.save_then_expect_file(&path, canned)?;
        Ok(())
    })
}
