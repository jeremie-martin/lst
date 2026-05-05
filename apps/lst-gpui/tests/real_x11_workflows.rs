//! Real-display tests for common whole-editor workflows that cross input,
//! runtime effects, and file-backed state.
//!
//! Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, Selection};

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn file_edit_save_quit_and_reopen_preserves_disk_contents() -> TestResult {
    support::run_x11_test("workflow-file-reopen", |session| {
        let path = session.seed_file("roundtrip.txt", "original contents\n")?;

        let mut editor = session.open_file("first-open", &path)?;
        editor.keys("<C-a>saved through the real window")?;
        editor.save_then_expect_file(&path, "saved through the real window")?;
        editor.quit_default()?;

        let mut editor = session.open_file("second-open", &path)?;
        editor.keys("<C-a>verified after reopen")?;
        editor.save_then_expect_file(&path, "verified after reopen")?;
        editor.quit_default()?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_v_pastes_system_clipboard_into_editor() -> TestResult {
    support::run_x11_test("workflow-clipboard-paste", |session| {
        let (mut editor, path) = session.open("scratch")?;

        write_clipboard_text(Selection::Clipboard, "clipboard paste\nsecond line")?;
        editor.keys("<C-v>")?;
        editor.save_then_expect_file(&path, "clipboard paste\nsecond line")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn goto_line_panel_moves_focus_back_to_editor_after_submit() -> TestResult {
    support::run_x11_test("workflow-goto-line", |session| {
        let path = session.seed_file("goto.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("goto", &path)?;

        editor.keys("<C-g>2:3<enter>X")?;
        editor.save_then_expect_file(&path, "alpha\nbeXta\ngamma")?;
        Ok(())
    })
}
