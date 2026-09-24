//! Real-display acceptance coverage for the standard daily-driver workflow.
//!
//! Run with:
//!
//!     DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --test real_x11_daily_driver --run-ignored only

mod support;

use std::fs;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn workspace_surfaces_own_input_instead_of_editing_behind_them() -> TestResult {
    support::run_x11_test("daily-driver-surface-input-ownership", |session| {
        let path = session.seed_file("surface-focus.txt", "first\nsecond")?;
        let mut editor = session.open_file("surface-focus", &path)?;
        let before = editor.read_state()?;

        editor.click_app_menu_button()?;
        let menu = editor.wait_state("app menu owns focus", secs(2), |record| {
            record.workspace_surface == "app_menu" && record.focused_input == "app_menu"
        })?;
        let cursors = |record: &lst_x11_harness::StateTraceRecord| {
            record
                .cursors
                .iter()
                .map(|cursor| (cursor.anchor_char, cursor.head_char))
                .collect::<Vec<_>>()
        };
        editor.expect_keys_ignored(
            "z<C-n><C-tab><C-f>",
            "<down>",
            |record| record.workspace_surface_selected_index != menu.workspace_surface_selected_index,
            |record| {
                record.revision == before.revision
                    && record.active_tab_id == before.active_tab_id
                    && !record.find.visible
                    && record.workspace_surface == "app_menu"
                    && cursors(record) == cursors(&before)
            },
        )?;

        editor.keys("<esc>")?;
        editor.wait_state("editor focus restored", secs(2), |record| {
            record.workspace_surface == "none" && record.focused_input == "editor"
        })?;
        assert_eq!(fs::read_to_string(path)?, "first\nsecond");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn all_tabs_and_application_menu_are_fully_keyboard_operable() -> TestResult {
    support::run_x11_test("daily-driver-shell-menu-navigation", |session| {
        let path = session.seed_file("first.txt", "first")?;
        let path_text = path.to_string_lossy().into_owned();
        let mut editor = session.open_file("shell-menu-navigation", &path)?;

        editor.keys("<C-n>")?;
        editor.wait_state("second tab active", secs(2), |record| record.active_tab_index == 1)?;
        editor.keys("<C-n>")?;
        editor.wait_state("third tab active", secs(2), |record| record.active_tab_index == 2)?;

        editor.click_all_tabs_button()?;
        editor.wait_state("all tabs list owns focus", secs(2), |record| {
            record.workspace_surface == "tab_list"
                && record.focused_input == "tab_list"
                && record.workspace_surface_selected_index == Some(2)
        })?;
        editor.keys("<home><enter>")?;
        editor.wait_state("first tab activated from all tabs", secs(2), |record| {
            record.workspace_surface == "none"
                && record.focused_input == "editor"
                && record.active_tab_index == 0
                && record.active_tab_path.as_deref() == Some(path_text.as_str())
        })?;

        editor.click_app_menu_button()?;
        editor.wait_state("application menu starts on first row", secs(2), |record| {
            record.workspace_surface == "app_menu"
                && record.focused_input == "app_menu"
                && record.workspace_surface_selected_index == Some(0)
        })?;
        editor.keys("<enter>")?;
        editor.wait_state("application menu enter activates new scratchpad", secs(2), |record| {
            record.workspace_surface == "none"
                && record.focused_input == "editor"
                && record.active_tab_index == 3
                && record.active_tab_path.as_deref() != Some(path_text.as_str())
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn app_menu_does_not_add_a_backdrop_beyond_the_inactive_current_line() -> TestResult {
    support::run_x11_test("daily-driver-menu-backdrop", |session| {
        session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n")?;
        let (mut editor, _path) = session.open("scratch")?;
        editor.wait_quiet(secs(1), secs(5))?;
        let before = editor.screenshot()?;

        editor.click_app_menu_button()?;
        let open = editor.wait_state("app menu opens", secs(2), |record| {
            record.workspace_surface == "app_menu"
        })?;
        editor.wait_quiet(secs(1), secs(5))?;
        let after = editor.screenshot()?;
        let diff = after.diff(&before)?;
        diff.changed_bounds.ok_or("opening the app menu changed no pixels")?;
        let scale = open.viewport.scale_factor.max(1.0);
        let cursor_line = open.cursors[open.primary_cursor_index].head_line;
        let row = open
            .viewport
            .first_row_for_line(cursor_line)
            .ok_or("current cursor line is outside the painted viewport")?;
        let menu_right = (340.0 * scale).ceil() as usize;
        let menu_bottom = (430.0 * scale).ceil() as usize;
        let line_top = (row.top_px * scale).floor().max(0.0) as usize;
        let line_bottom = ((row.top_px + open.viewport.line_height_px) * scale).ceil() as usize;
        let screenshot_width = usize::from(after.width);
        let outside_allowed = before
            .rgb_pixels
            .chunks_exact(3)
            .zip(after.rgb_pixels.chunks_exact(3))
            .enumerate()
            .filter(|(index, (before_pixel, after_pixel))| {
                if before_pixel == after_pixel {
                    return false;
                }
                let x = index % screenshot_width;
                let y = index / screenshot_width;
                let in_menu = x < menu_right && y < menu_bottom;
                let in_current_line = (line_top..line_bottom).contains(&y);
                !(in_menu || in_current_line)
            })
            .count();

        assert!(
            outside_allowed == 0,
            "app menu changed {outside_allowed} pixels outside its surface and the intentional inactive current-line row, indicating a tinted backdrop: {diff:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_alt_shift_arrows_duplicate_the_line_above_and_below() -> TestResult {
    support::run_x11_test("daily-driver-duplicate-up-down", |session| {
        let path = session.seed_file("duplicate.txt", "alpha\nbeta")?;
        let mut editor = session.open_file("duplicate", &path)?;

        // The copies are identical, so the caret tells the directions apart:
        // it stays on the upper copy for "above" and follows the lower one
        // for "below".
        editor.click_at_text(1, 2)?;
        editor.keys("<C-A-S-up>")?;
        editor.expect_cursor_heads(&[(1, 2)])?;
        editor.keys("<C-A-S-down>")?;
        editor.expect_cursor_heads(&[(2, 2)])?;
        editor.save_then_expect_file(&path, "alpha\nbeta\nbeta\nbeta")?;
        Ok(())
    })
}
