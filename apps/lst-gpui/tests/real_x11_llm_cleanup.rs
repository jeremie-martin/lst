//! Exercises the production prompt-add subprocess boundary with a local executable fixture.

mod support;

use std::ffi::OsStr;

use support::{secs, EditorTestExt, TestResult};

const FAKE_ENV: &str = "LST_TEST_PROMPT_RESPONSE";

fn install_filter(session: &support::ScratchpadSession) -> support::SupportResult<std::ffi::OsString> {
    use std::os::unix::fs::PermissionsExt;
    let path = session.seed_file(
        "prompt-add",
        r#"#!/usr/bin/python3
import os, sys, time
assert sys.argv[1:] == ['-', '--label', 'lst', '--no-clipboard']
source = sys.stdin.read()
assert source
expected = os.environ.get('LST_TEST_PROMPT_SOURCE')
if expected is not None:
    assert source == expected
if os.environ.get('LST_TEST_PROMPT_DELAY'):
    time.sleep(1.5)
if os.environ.get('LST_TEST_PROMPT_FAIL'):
    print('provider unavailable', file=sys.stderr)
    print('partial output must not be applied')
    sys.exit(1)
sys.stdout.write(os.environ['LST_TEST_PROMPT_RESPONSE'])
if os.environ.get('LST_TEST_PROMPT_WARNING'):
    print('history could not be saved', file=sys.stderr)
"#,
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    let mut paths = vec![path.parent().unwrap().to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    Ok(std::env::join_paths(paths)?)
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn cleanup_replaces_whole_buffer_inline_with_atomic_undo() -> TestResult {
    support::run_x11_test("llm-cleanup-whole", |session| {
        let screenshot = session.artifacts().join("prompt-confirmation.ppm");
        let original = "um, hello, world";
        let canned = "Hello, world.\n";
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new(canned)),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys(original)?;
        editor.save_then_expect_file(&path, original)?;

        let before = editor.read_state()?;
        editor.keys("<C-S-p>polish agent prompt<enter>")?;
        let confirmation = editor.wait_state("whole-document cleanup confirmation", secs(3), |record| {
            record.cleanup_confirmation_open && record.focused_input == "cleanup_confirmation"
        })?;
        editor.press(lst_x11_harness::KeyChord::Ctrl(lst_x11_harness::Key::Char('n')))?;
        editor.send_keys_settle("<S-enter>")?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        let blocked = editor.read_state()?;
        assert!(blocked.cleanup_confirmation_open, "{blocked:?}");
        assert_eq!(blocked.active_tab_id, confirmation.active_tab_id, "{blocked:?}");
        editor.screenshot()?.write_ppm(&screenshot)?;
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
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new(canned)),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys(original)?;
        editor.save_then_expect_file(&path, original)?;

        let before = editor.read_state()?;
        editor.keys("<C-S-p>polish agent prompt<enter>")?;
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
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 3] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new("LST_TEST_PROMPT_SOURCE"), OsStr::new("um middle")),
            (OsStr::new(FAKE_ENV), OsStr::new(canned)),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys("before<enter>um middle<enter>after")?;
        editor.save_then_expect_file(&path, original)?;

        // Select line 2 ("um middle") without including the surrounding newlines.
        editor.keys("<C-home><down><S-end>")?;

        let before = editor.read_state()?;
        editor.keys("<C-S-p>polish agent prompt<enter>")?;
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
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 3] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new(canned)),
            (OsStr::new("LST_TEST_PROMPT_DELAY"), OsStr::new("1500")),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;

        editor.keys(original)?;
        editor.save_then_expect_file(&path, original)?;
        editor.keys("<C-a><C-S-p>polish agent prompt<enter>")?;
        let before = editor.wait_state("cleanup started", secs(3), |record| {
            record.status_message.contains("Polishing")
        })?;

        editor.press(lst_x11_harness::KeyChord::Ctrl(lst_x11_harness::Key::Char('q')))?;
        let blocked = editor.wait_state("quit waits for cleanup", secs(3), |record| {
            record.status_message == "Wait for prompt polishing to finish before quitting."
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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_prompt_filter_preserves_text_and_shows_diagnostic() -> TestResult {
    support::run_x11_test("prompt-failure", |session| {
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new("LST_TEST_PROMPT_FAIL"), OsStr::new("1")),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;
        editor.keys("keep my request<C-a><C-S-p>polish agent prompt<enter>")?;
        editor.wait_state("filter failure", secs(5), |record| {
            record.status_message.contains("provider unavailable")
        })?;
        editor.save_then_expect_file(&path, "keep my request")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn editing_during_prompt_polishing_preserves_new_text() -> TestResult {
    support::run_x11_test("prompt-stale", |session| {
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 3] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new("stale result")),
            (OsStr::new("LST_TEST_PROMPT_DELAY"), OsStr::new("1")),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;
        editor.keys("original<C-a><C-S-p>polish agent prompt<enter>")?;
        editor.wait_state("filter running", secs(3), |record| {
            record.status_message.contains("Polishing")
        })?;
        editor.keys("new request")?;
        editor.wait_state("stale result refused", secs(5), |record| {
            record.status_message.contains("result discarded")
        })?;
        editor.save_then_expect_file(&path, "new request")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn empty_prompt_output_preserves_original_text() -> TestResult {
    support::run_x11_test("prompt-empty", |session| {
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new(" \n")),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;
        editor.keys("original<C-a><C-S-p>polish agent prompt<enter>")?;
        editor.wait_state("empty result refused", secs(5), |record| {
            record.status_message.contains("empty message")
        })?;
        editor.save_then_expect_file(&path, "original")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn history_warning_does_not_hide_successful_prompt_rewrite() -> TestResult {
    support::run_x11_test("prompt-warning", |session| {
        let filter_path = install_filter(session)?;
        let env: [(&OsStr, &OsStr); 3] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new("Polished request.")),
            (OsStr::new("LST_TEST_PROMPT_WARNING"), OsStr::new("1")),
        ];
        let (mut editor, path) = session.open_with_env("scratch", &env)?;
        editor.keys("original<C-a><C-S-p>polish agent prompt<enter>")?;
        editor.wait_state("history warning visible", secs(5), |record| {
            record.status_message.contains("history could not be saved")
        })?;
        editor.save_then_expect_file(&path, "Polished request.")?;
        Ok(())
    })
}
