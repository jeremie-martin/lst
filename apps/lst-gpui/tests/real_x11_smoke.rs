//! Real-display smoke tests. Spawn the editor against `DISPLAY`, drive
//! input with `lst-x11-harness`, and assert on autosaved file contents
//! and clipboard state.
//!
//! Gated by `#[ignore]` because the default `cargo test` gate is biased
//! toward in-process behavioral tests; these need a real Xorg session and
//! `xclip` on PATH. Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_smoke --run-ignored only

mod support;

use std::fs;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Key, KeyChord, Selection,
};

use support::{secs, EditorTestExt, ScratchpadSession, SupportResult, TestResult};

const TEXT: &str = "quit clipboard smoke";
const PRIMARY_TEXT: &str = "middle paste smoke";

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_an_empty_scratchpad_removes_its_file() -> TestResult {
    support::run_x11_test("smoke-empty", |session| {
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
    support::run_x11_test("smoke-quit-regular-clipboard", |session| {
        let text_path = with_seed_file(session, "quit-source.txt", TEXT)?;
        let editor = session.open_file("text", &text_path)?;
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
    support::run_x11_test("smoke-quit-scratchpad-clipboard", |session| {
        let (mut editor, _path) = session.open("scratch")?;
        editor.keys(TEXT)?;
        editor.quit_default()?;

        wait_clipboard_text(Selection::Clipboard, TEXT, secs(10))?;
        wait_clipboard_text(Selection::Primary, TEXT, secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_a_scratchpad_tab_copies_it_while_other_tabs_remain_open() -> TestResult {
    support::run_x11_test("smoke-close-scratchpad-tab-clipboard", |session| {
        let (mut editor, _path) = session.open("scratch")?;
        editor.keys(TEXT)?;
        editor.keys("<C-n><C-S-tab><C-w>")?;

        editor.wait_state("scratchpad tab closed", secs(5), |record| {
            record.status_message == "Closed tab." && record.line_count == 1
        })?;
        wait_clipboard_text(Selection::Clipboard, TEXT, secs(10))?;
        wait_clipboard_text(Selection::Primary, TEXT, secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn primary_selection_round_trips_via_middle_click() -> TestResult {
    support::run_x11_test("smoke-primary-paste", |session| {
        let (mut editor, path) = session.open("scratch")?;

        if !editor.is_viewable()? {
            eprintln!("skipping middle-click PRIMARY paste check because the X11 window is not viewable");
            return Ok(());
        }

        // Pin the click to the top-left of the empty scratchpad's text
        // area via the text-coordinate API. This is robust to font /
        // gutter / padding changes; the older pixel-magic `(160, 170)`
        // version drifted when those changed.
        write_clipboard_text(Selection::Primary, PRIMARY_TEXT)?;
        editor.middle_click_at_text(0, 0)?;
        editor.save_then_expect_file(&path, PRIMARY_TEXT)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn copying_without_a_selection_pastes_a_complete_line_before_the_cursor_line() -> TestResult {
    support::run_x11_test("smoke-linewise-copy-paste", |session| {
        let path = session.seed_file("linewise-copy.txt", "alpha\nbeta\n")?;
        let mut editor = session.open_file("linewise-copy", &path)?;

        editor.keys("<C-home><C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "alpha\n", secs(10))?;
        editor.keys("<down><C-v>")?;
        editor.save_then_expect_file(&path, "alpha\nalpha\nbeta\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn keyboard_selection_updates_x11_primary() -> TestResult {
    support::run_x11_test("smoke-keyboard-selection-primary", |session| {
        let path = session.seed_file("keyboard-primary.txt", "alpha beta")?;
        let mut editor = session.open_file("keyboard-primary", &path)?;
        write_clipboard_text(Selection::Primary, "primary sentinel")?;

        editor.keys("<C-home><S-right>")?;
        wait_clipboard_text(Selection::Primary, "a", secs(10))?;
        Ok(())
    })
}

fn with_seed_file(session: &ScratchpadSession, name: &str, contents: &str) -> SupportResult<std::path::PathBuf> {
    let path = session.root().join(name);
    fs::write(&path, contents)?;
    Ok(path)
}
