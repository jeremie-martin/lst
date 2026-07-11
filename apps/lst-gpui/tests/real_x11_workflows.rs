//! Real-display tests for common whole-editor workflows that cross input,
//! runtime effects, and file-backed state.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_workflows --run-ignored only

mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, Key, KeyChord, Selection};
use std::fs;

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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn goto_line_column_moves_to_requested_column_and_clamps() -> TestResult {
    support::run_x11_test("workflow-goto-line-column-clamp", |session| {
        let path = session.seed_file("goto-column.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("goto-column", &path)?;

        editor.keys("<C-g>2:3<enter>")?;
        editor.expect_cursor_heads(&[(1, 2)])?;

        editor.keys("<C-g>2:99<enter>")?;
        editor.expect_cursor_heads(&[(1, 4)])?;

        editor.keys("<C-g>99:2<enter>")?;
        editor.expect_cursor_heads(&[(2, 1)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_w_dirty_file_tab_saves_before_close() -> TestResult {
    support::run_x11_test("workflow-dirty-close-save", |session| {
        let path = session.seed_file("dirty-close.txt", "original")?;
        let path_string = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("dirty-close", &path)?;

        editor.keys("<C-a>saved before close")?;
        editor.keys("<C-n>")?;
        editor.wait_state("sibling tab focused", support::secs(2), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("dirty file tab focused", support::secs(2), |record| {
            record.active_tab_path.as_deref() == Some(&path_string)
        })?;

        editor.keys("<C-w><enter>")?;
        editor.wait_state("dirty file tab closed", support::secs(5), |record| {
            record.active_tab_path.as_deref() != Some(&path_string)
        })?;
        editor.expect_file(&path, "saved before close")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_q_dirty_file_saves_before_exit() -> TestResult {
    support::run_x11_test("workflow-dirty-file-quit-save", |session| {
        let path = session.seed_file("dirty-quit.txt", "original")?;
        let mut editor = session.open_file("dirty-file-quit", &path)?;

        editor.keys("<C-a>saved before quit")?;
        editor.keys("<C-q>")?;
        editor.wait_state("dirty file quit prompt", support::secs(2), |record| {
            record.close_prompt_file.as_deref() == Some("dirty-quit.txt")
        })?;
        editor.press(KeyChord::Key(Key::Enter))?;
        editor.wait_for_successful_exit(support::secs(10))?;

        assert_eq!(fs::read_to_string(&path)?, "saved before quit");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_q_dirty_scratchpad_saves_before_exit() -> TestResult {
    support::run_x11_test("workflow-dirty-scratchpad-quit-save", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("scratchpad before quit")?;
        editor.quit_default()?;

        assert_eq!(fs::read_to_string(&path)?, "scratchpad before quit");
        Ok(())
    })
}
