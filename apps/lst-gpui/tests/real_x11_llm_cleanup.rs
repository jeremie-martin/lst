//! Real-display tests for the LLM cleanup action (Ctrl+Shift+R). The
//! editor binary is launched with `LST_LLM_FAKE_RESPONSE` exported, which
//! activates an in-process fake `LlmClient` so the test never reaches
//! the real DeepSeek API. Coverage:
//!
//! - whole-buffer cleanup replaces inline; one Ctrl+Z restores the original
//! - selection-only cleanup replaces just the selection; Ctrl+Z restores
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
        editor.keys("<C-S-r>")?;
        editor.wait_state("cleanup applied", secs(5), |record| {
            record.revision > before.revision
        })?;
        editor.save_then_expect_file(&path, canned)?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
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
        editor.keys("<C-S-r>")?;
        editor.wait_state("cleanup applied", secs(5), |record| {
            record.revision > before.revision
        })?;
        editor.save_then_expect_file(&path, cleaned)?;

        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}
