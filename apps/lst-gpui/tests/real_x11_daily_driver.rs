//! Real-display acceptance coverage for the standard daily-driver workflow.
//!
//! Run with:
//!
//!     DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --test real_x11_daily_driver --run-ignored only

mod support;

use std::{fs, thread};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn standard_mode_is_default_and_workspace_surfaces_are_keyboard_reachable() -> TestResult {
    support::run_x11_test("daily-driver-surfaces", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        let initial = editor.read_state()?;
        assert_eq!(initial.input_mode, "standard", "{initial:?}");

        editor.keys("<C-S-p>")?;
        editor.wait_state("command palette opens", secs(2), |record| {
            record.workspace_surface == "command_palette" && record.focused_input == "command_palette"
        })?;
        editor.keys("<esc><C-,>")?;
        editor.wait_state("settings opens", secs(2), |record| {
            record.workspace_surface == "settings" && record.focused_input == "settings"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn standard_alt_shift_up_duplicates_line_above() -> TestResult {
    support::run_x11_test("daily-driver-duplicate-up", |session| {
        let path = session.seed_file("duplicate.txt", "alpha\nbeta")?;
        let mut editor = session.open_file("duplicate", &path)?;

        editor.click_at_text(1, 2)?;
        editor.keys("<A-S-up>")?;
        editor.save_then_expect_file(&path, "alpha\nbeta\nbeta")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ordinary_files_do_not_autosave_by_default() -> TestResult {
    support::run_x11_test("daily-driver-no-file-autosave", |session| {
        let path = session.seed_file("manual-save.txt", "original")?;
        let mut editor = session.open_file("manual-save", &path)?;

        editor.keys(" changed")?;
        thread::sleep(secs(2));
        assert_eq!(fs::read_to_string(&path)?, "original");
        let state = editor.read_state()?;
        assert!(state.active_tab_modified, "{state:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn closing_a_dirty_file_requires_an_explicit_decision() -> TestResult {
    support::run_x11_test("daily-driver-close-prompt", |session| {
        let path = session.seed_file("close-me.txt", "body")?;
        let mut editor = session.open_file("close-prompt", &path)?;

        editor.keys(" changed<C-w>")?;
        let prompted = editor.wait_state("dirty close prompt", secs(2), |record| {
            record.close_prompt_file.as_deref() == Some("close-me.txt")
        })?;
        assert!(prompted.active_tab_modified, "{prompted:?}");

        editor.keys("<esc>")?;
        editor.wait_state("close prompt cancelled", secs(2), |record| {
            record.close_prompt_file.is_none() && record.active_tab_modified
        })?;
        assert_eq!(fs::read_to_string(&path)?, "body");
        Ok(())
    })
}
