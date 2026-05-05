//! Real-display smoke tests. Spawn the editor against `DISPLAY`, drive
//! input with `lst-x11-harness`, and assert on autosaved file contents
//! and clipboard state.
//!
//! Gated by `#[ignore]` because the default `cargo test` gate is biased
//! toward in-process behavioral tests; these need a real Xorg session and
//! `xclip` on PATH. Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

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

        editor.press(KeyChord::Ctrl(Key::Char('w')))?;
        editor.wait_for_successful_exit(secs(10))?;

        assert!(
            !path.exists(),
            "closing an empty scratchpad should remove {}",
            path.display()
        );
        assert_eq!(support::count_files(&scratchpad_dir)?, 0);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn quit_persists_buffer_into_clipboard_and_primary() -> TestResult {
    support::run_x11_test("smoke-quit-clipboard", |session| {
        let text_path = with_seed_file(session, "quit-source.txt", TEXT)?;
        let editor = session.open_file("text", &text_path)?;

        // Ctrl+Q is an editor accelerator; the spawn already focused the window
        // for us, so the synthesized chord lands in the editor.
        editor.quit_default()?;

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
            eprintln!(
                "skipping middle-click PRIMARY paste check because the X11 window is not viewable"
            );
            return Ok(());
        }

        write_clipboard_text(Selection::Primary, PRIMARY_TEXT)?;
        editor.middle_click_at(160, 170)?;
        editor.save_then_expect_file(&path, PRIMARY_TEXT)?;
        Ok(())
    })
}

fn with_seed_file(
    session: &ScratchpadSession,
    name: &str,
    contents: &str,
) -> SupportResult<std::path::PathBuf> {
    let path = session.root().join(name);
    fs::write(&path, contents)?;
    Ok(path)
}
