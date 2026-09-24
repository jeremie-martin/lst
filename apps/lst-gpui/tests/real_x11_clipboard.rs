//! Real-display specs for the X11 CLIPBOARD and PRIMARY selections: copying,
//! pasting, selection ownership, and pastes from slow or huge sources.

#[path = "support/stalled_clipboard.rs"]
mod stalled_clipboard;
mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Key, KeyChord, Selection,
};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn copying_without_a_selection_pastes_a_complete_line_before_the_cursor_line() -> TestResult {
    support::run_x11_test("clipboard-linewise-copy-paste", |session| {
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
    support::run_x11_test("clipboard-keyboard-selection-primary", |session| {
        let path = session.seed_file("keyboard-primary.txt", "alpha beta")?;
        let mut editor = session.open_file("keyboard-primary", &path)?;

        editor.keys("<C-home><S-right>")?;
        wait_clipboard_text(Selection::Primary, "a", secs(10))?;
        editor.keys("<C-a>")?;
        wait_clipboard_text(Selection::Primary, "alpha beta", secs(10))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_v_pastes_system_clipboard_into_editor() -> TestResult {
    support::run_x11_test("clipboard-paste", |session| {
        let (mut editor, path) = session.open("scratch")?;

        write_clipboard_text(Selection::Clipboard, "clipboard paste\nsecond line")?;
        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "clipboard paste\nsecond line")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn stalled_external_clipboard_transfer_returns_control_to_the_editor() -> TestResult {
    support::run_x11_test("clipboard-stalled-transfer", |session| {
        let (mut editor, path) = session.open("scratch")?;
        let mut owner = stalled_clipboard::StalledClipboard::new()?;
        editor.press(KeyChord::Ctrl(Key::Char('v')))?;
        owner.wait_started()?;
        // Queue real input while the clipboard owner remains alive but silent.
        // The bounded selection wait must end so typing and saving can resume.
        editor.press(KeyChord::Key(Key::Char('X')))?;
        editor.press(KeyChord::Ctrl(Key::Char('s')))?;
        editor.expect_file(&path, "X")?;
        owner.finish()?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn large_markdown_paste_accepts_end_edit_while_syntax_builds() -> TestResult {
    support::run_x11_test("clipboard-large-markdown-paste", |session| {
        let (mut editor, path) = session.open("scratch")?;
        let payload = format!("before\n\n<pre>\n{}", "x".repeat(300 * 1024));
        let expected = format!("{payload}T");

        write_clipboard_text(Selection::Clipboard, &payload)?;
        editor.keys("<C-v>")?;
        editor.keys("T")?;
        editor.save_then_expect_file(&path, &expected)?;
        Ok(())
    })
}
