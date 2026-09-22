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
gate = os.environ.get('LST_TEST_PROMPT_GATE')
while gate and os.path.exists(gate):
    time.sleep(0.01)
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
fn polish_button_replaces_whole_buffer_inline_with_atomic_undo() -> TestResult {
    support::run_x11_test("llm-cleanup-whole", |session| {
        let screenshot = session.artifacts().join("prompt-confirmation.ppm");
        let button_screenshot = session.artifacts().join("prompt-button.ppm");
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
        editor.screenshot()?.write_ppm(&button_screenshot)?;
        editor.click_cleanup_button()?;
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
        let review = editor.wait_state("prompt review ready", secs(5), |record| {
            record.prompt_review_view.as_deref() == Some("changes") && record.focused_input == "prompt_review"
        })?;
        assert_eq!(review.revision, before.revision, "Review must preserve the original");
        editor.expect_file(&path, original)?;
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
        editor.click_cleanup_button()?;
        editor.wait_state("whole-document cleanup confirmation", secs(3), |record| {
            record.cleanup_confirmation_open && record.focused_input == "cleanup_confirmation"
        })?;
        editor.keys("<escape>")?;
        editor.wait_state("cleanup confirmation cancelled", secs(3), |record| {
            !record.cleanup_confirmation_open && record.revision == before.revision
        })?;
        editor.expect_file(&path, original)?;
        editor.keys("!")?;
        editor.save_then_expect_file(&path, "um, hello, world!")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn polish_button_replaces_only_the_selection_with_atomic_undo() -> TestResult {
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
        editor.click_cleanup_button()?;
        let review = editor.wait_state("prompt review ready", secs(5), |record| {
            record.prompt_review_view.as_deref() == Some("changes") && record.focused_input == "prompt_review"
        })?;
        assert_eq!(review.revision, before.revision, "Review must preserve the original");
        editor.expect_file(&path, original)?;
        editor.keys("<enter>")?;
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
            record.prompt_review_view.is_some()
        })?;
        editor.keys("<enter>")?;
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
        editor.wait_state("review ready with history warning", secs(5), |record| {
            record.prompt_review_view.is_some()
        })?;
        editor.keys("<enter>")?;
        editor.wait_state("history warning visible", secs(5), |record| {
            record.status_message.contains("history could not be saved")
        })?;
        editor.save_then_expect_file(&path, "Polished request.")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn prompt_review_switches_views_and_discard_keeps_original() -> TestResult {
    support::run_x11_test("prompt-review", |session| {
        let artifacts = session.artifacts().to_path_buf();
        let filter_path = install_filter(session)?;
        let original = "Please, um, review the search panel and keep its current keyboard shortcuts.\n\nKeep the current keyboard shortcuts and don't change how selection works. I want a small fix, not a redesign.\n\nFirst understand why it is slow, then fix that and check that it works.";
        let result = "Please review the search panel and keep its current keyboard shortcuts.\n\nKeep the current keyboard shortcuts and selection behavior. Make a small, focused fix rather than redesigning the panel.\n\nUnderstand the cause before changing the code, then verify the fix and the nearby behavior.";
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new(result)),
        ];
        let path = session.seed_file("prompt.txt", original)?;
        let mut editor = session.open_file_with_env("prompt", &path, &env)?;
        editor.keys("<C-a>")?;
        editor.click_cleanup_button()?;
        let review = editor.wait_state("changes view", secs(5), |r| {
            r.prompt_review_view.as_deref() == Some("changes")
        })?;
        editor.wait_quiet(std::time::Duration::from_millis(150), secs(3))?;
        editor
            .screenshot()?
            .write_ppm(artifacts.join("prompt-review-changes.ppm"))?;
        editor.send_keys_settle("typing must not edit<C-z><C-n>")?;
        let blocked = editor.read_state()?;
        assert_eq!(blocked.revision, review.revision);
        assert_eq!(blocked.active_tab_id, review.active_tab_id);
        editor.keys("<tab>")?;
        editor.wait_state("clean result view", secs(3), |r| {
            r.prompt_review_view.as_deref() == Some("result")
        })?;
        editor.wait_quiet(std::time::Duration::from_millis(150), secs(3))?;
        editor
            .screenshot()?
            .write_ppm(artifacts.join("prompt-review-result.ppm"))?;
        editor.keys("<tab>")?;
        editor.wait_state("changes view restored", secs(3), |r| {
            r.prompt_review_view.as_deref() == Some("changes")
        })?;
        editor.keys("<escape>")?;
        editor.wait_state("review discarded", secs(3), |r| {
            r.prompt_review_view.is_none() && r.focused_input == "editor"
        })?;
        editor.save_then_expect_file(&path, original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn document_changed_on_disk_cannot_be_overwritten_by_review() -> TestResult {
    support::run_x11_test("prompt-review-stale", |session| {
        let filter_path = install_filter(session)?;
        let path = session.seed_file("prompt.txt", "original")?;
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new("old suggestion")),
        ];
        let mut editor = session.open_file_with_env("prompt", &path, &env)?;
        editor.keys("<C-a>")?;
        editor.click_cleanup_button()?;
        let review = editor.wait_state("review ready", secs(5), |r| r.prompt_review_view.is_some())?;
        std::fs::write(&path, "changed externally")?;
        editor.wait_state("external change reloaded", secs(5), |r| r.revision != review.revision)?;
        editor.send_keys_settle("<enter>")?;
        editor.wait_state("stale apply refused", secs(3), |r| {
            r.prompt_review_view.is_some() && r.status_message.contains("cannot be applied")
        })?;
        editor.keys("<escape>")?;
        editor.save_then_expect_file(&path, "changed externally")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn long_prompt_review_scrolls_and_applies_the_complete_result() -> TestResult {
    support::run_x11_test("prompt-review-long", |session| {
        session.seed_settings("version = 1\n[appearance]\ntheme = \"dark\"\n")?;
        let artifacts = session.artifacts().to_path_buf();
        let filter_path = install_filter(session)?;
        let original: String = (0..200)
            .map(|i| format!("Request {i}: please, um, review this part carefully.\n\n"))
            .collect();
        let result = original.replace("please, um, review", "review");
        let path = session.seed_file("prompt.txt", &original)?;
        let env: [(&OsStr, &OsStr); 2] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new(&result)),
        ];
        let mut editor = session.open_file_with_env("prompt", &path, &env)?;
        editor.resize(1000, 760)?;
        editor.keys("<C-a>")?;
        editor.click_cleanup_button()?;
        editor.wait_state("long review ready", secs(5), |r| r.prompt_review_view.is_some())?;
        editor.wait_quiet(std::time::Duration::from_millis(150), secs(3))?;
        let top = editor.screenshot()?;
        top.write_ppm(artifacts.join("prompt-review-dark.ppm"))?;
        editor.send_keys_settle("<end>")?;
        editor.wait_quiet(std::time::Duration::from_millis(150), secs(3))?;
        let bottom = editor.screenshot()?;
        assert!(!top.diff(&bottom)?.is_exact(), "End must scroll the review");
        bottom.write_ppm(artifacts.join("prompt-review-end.ppm"))?;
        editor.keys("<tab>")?;
        editor.wait_state("result selected", secs(3), |r| {
            r.prompt_review_view.as_deref() == Some("result")
        })?;
        editor.send_keys_settle("<pagedown><home>")?;
        editor.keys("<enter>")?;
        editor.save_then_expect_file(&path, &result)?;
        editor.keys("<C-z>")?;
        editor.save_then_expect_file(&path, &original)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn completed_prompt_review_dismisses_competing_surfaces_before_accepting_input() -> TestResult {
    for surface in ["settings", "command_palette", "recent"] {
        support::run_x11_test(&format!("prompt-review-{surface}"), |session| {
            let filter_path = install_filter(session)?;
            let gate = session.seed_file("gate", "wait")?;
            let path = session.seed_file("prompt.txt", "original")?;
            let env: [(&OsStr, &OsStr); 3] = [
                (OsStr::new("PATH"), &filter_path),
                (OsStr::new(FAKE_ENV), OsStr::new("polished")),
                (OsStr::new("LST_TEST_PROMPT_GATE"), gate.as_os_str()),
            ];
            let mut editor = session.open_file_with_env("prompt", &path, &env)?;
            editor.keys("<C-a>")?;
            editor.click_cleanup_button()?;
            let before = editor.wait_state("filter running", secs(3), |r| r.status_message.contains("Polishing"))?;
            match surface {
                "settings" => {
                    editor.keys("<C-,>")?;
                    editor.send_keys_settle("rulers")?;
                    editor.keys("<tab><enter>")?;
                    editor.wait_state("settings value editor open", secs(3), |r| {
                        r.settings_value_editor_item.is_some()
                    })?;
                }
                "command_palette" => {
                    editor.keys("<C-S-p>")?;
                    editor.wait_state("palette open", secs(3), |r| r.workspace_surface == "command_palette")?;
                }
                _ => {
                    editor.keys("<C-p>")?;
                    editor.wait_state("recent open", secs(3), |r| r.recent_panel_open)?;
                }
            }
            std::fs::remove_file(&gate)?;
            let review = editor.wait_state("review owns visible surface and focus", secs(5), |r| {
                r.prompt_review_view.is_some()
                    && r.focused_input == "prompt_review"
                    && r.workspace_surface == "none"
                    && !r.recent_panel_open
                    && r.settings_value_editor_item.is_none()
            })?;
            assert_eq!(review.revision, before.revision);
            editor.expect_file(&path, "original")?;
            editor.keys("<enter>")?;
            editor.save_then_expect_file(&path, "polished")?;
            Ok(())
        })?;
    }
    Ok(())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn prompt_timeout_preserves_text_allows_retry_and_unblocks_quitting() -> TestResult {
    support::run_x11_test("prompt-timeout", |session| {
        let filter_path = install_filter(session)?;
        let gate = session.seed_file("gate", "wait")?;
        let path = session.seed_file("prompt.txt", "original")?;
        let env: [(&OsStr, &OsStr); 3] = [
            (OsStr::new("PATH"), &filter_path),
            (OsStr::new(FAKE_ENV), OsStr::new("polished")),
            (OsStr::new("LST_TEST_PROMPT_GATE"), gate.as_os_str()),
        ];
        let mut editor = session.open_file_with_env("prompt", &path, &env)?;
        editor.keys("<C-a>")?;
        editor.click_cleanup_button()?;
        let before = editor.wait_state("filter running", secs(3), |r| r.status_message.contains("Polishing"))?;
        let failed = editor.wait_state("production timeout reported", secs(65), |r| {
            r.status_message.contains("timed out after 60 seconds")
        })?;
        assert_eq!(before.revision, failed.revision);
        assert!(failed.prompt_review_view.is_none());
        editor.expect_file(&path, "original")?;
        std::fs::remove_file(gate)?;
        editor.click_cleanup_button()?;
        editor.wait_state("retry produces review", secs(5), |r| r.prompt_review_view.is_some())?;
        editor.keys("<escape>")?;
        assert!(editor.quit(secs(5))?.success());
        assert_eq!(std::fs::read_to_string(path)?, "original");
        Ok(())
    })
}
