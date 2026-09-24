//! Real-display specs for visible editor chrome: the gutter, status text,
//! theme, zoom, and the tab-strip menus that own keyboard input while open.

mod support;

use lst_x11_harness::{Key, KeyChord, StateTraceRecord};

use support::{secs, EditorTestExt, TestResult};

fn visible_gutter(record: &StateTraceRecord, line: usize) -> String {
    record
        .viewport
        .rows
        .iter()
        .find(|row| row.logical_line == line)
        .and_then(|row| row.gutter_text.as_deref())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn line_number_modes_render_absolute_relative_and_hybrid_text() -> TestResult {
    support::run_x11_test("chrome-line-number-modes", |session| {
        let path = session.seed_file("line-numbers.txt", "one\ntwo\nthree\nfour\n")?;
        let mut editor = session.open_file("chrome-line-number-modes", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down>")?;
        editor.expect_cursor_heads(&[(1, 0)])?;

        let absolute = editor.wait_state("absolute gutter text", secs(2), |record| {
            visible_gutter(record, 0) == "1" && visible_gutter(record, 1) == "2" && visible_gutter(record, 2) == "3"
        })?;
        assert_eq!(visible_gutter(&absolute, 1), "2", "{absolute:?}");

        editor.keys("<A-l>")?;
        let relative = editor.wait_state("relative gutter text", secs(2), |record| {
            record.status_message == "Line numbers: Relative"
                && visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "0"
                && visible_gutter(record, 2) == "1"
        })?;
        assert_eq!(visible_gutter(&relative, 1), "0", "{relative:?}");

        editor.keys("<A-l>")?;
        let hybrid = editor.wait_state("hybrid gutter text", secs(2), |record| {
            record.status_message == "Line numbers: Hybrid"
                && visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "2"
                && visible_gutter(record, 2) == "1"
        })?;
        assert_eq!(visible_gutter(&hybrid, 1), "2", "{hybrid:?}");

        editor.keys("<A-l>")?;
        editor.wait_state("absolute gutter restored", secs(2), |record| {
            record.status_message == "Line numbers: Absolute"
                && visible_gutter(record, 0) == "1"
                && visible_gutter(record, 1) == "2"
                && visible_gutter(record, 2) == "3"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn configured_theme_is_visible_without_a_status_bar_toggle() -> TestResult {
    support::run_x11_test("chrome-configured-theme", |session| {
        session.seed_settings("version = 1\n[appearance]\ntheme = \"dark\"\n")?;
        let (mut editor, _path) = session.open("scratch")?;

        let configured = editor.wait_state("configured theme", secs(2), |record| {
            record.theme_name == "Dark" && record.theme_button_bounds_px.is_none()
        })?;
        assert_eq!(configured.theme_name, "Dark", "{configured:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn zoom_shortcuts_update_visible_zoom_status_and_reset() -> TestResult {
    support::run_x11_test("chrome-zoom-shortcuts", |session| {
        let (mut editor, _path) = session.open("scratch")?;
        let initial = editor.read_state()?;
        let glyph_width = initial.viewport.char_width_px;

        editor.press(KeyChord::Ctrl(Key::Char('=')))?;
        editor.wait_state("zoom in", secs(2), |record| {
            record.status_bar.contains("Zoom 110%") && record.viewport.char_width_px > glyph_width
        })?;

        editor.press(KeyChord::Ctrl(Key::Char('=')))?;
        editor.wait_state("zoom in again", secs(2), |record| {
            record.status_bar.contains("Zoom 121%")
        })?;

        editor.press(KeyChord::Ctrl(Key::Char('-')))?;
        editor.wait_state("zoom out", secs(2), |record| record.status_bar.contains("Zoom 110%"))?;

        editor.press(KeyChord::Ctrl(Key::Char('0')))?;
        editor.wait_state("zoom reset", secs(2), |record| {
            !record.status_bar.contains("Zoom") && record.viewport.char_width_px == glyph_width
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn gutter_width_grows_and_shrinks_by_one_glyph_at_the_four_digit_line_count() -> TestResult {
    support::run_x11_test("chrome-dynamic-gutter", |session| {
        let contents = (0..999)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = session.seed_file("dynamic-gutter.txt", &contents)?;
        let mut editor = session.open_file("chrome-dynamic-gutter", &path)?;

        let three_digits = editor.wait_state("three digit gutter", secs(5), |record| {
            record.line_count == 999 && record.viewport.gutter_width_px > 0.0
        })?;
        editor.keys("<C-end><enter>")?;
        let four_digits = editor.wait_state("four digit gutter", secs(5), |record| {
            record.line_count == 1_000 && record.viewport.gutter_width_px > three_digits.viewport.gutter_width_px
        })?;
        let growth = four_digits.viewport.gutter_width_px - three_digits.viewport.gutter_width_px;
        assert!(
            (growth - four_digits.viewport.char_width_px).abs() < 0.25,
            "one digit should add one measured glyph width: before={three_digits:?}, after={four_digits:?}"
        );

        editor.keys("<bs>")?;
        let shrunk = editor.wait_state("three digit gutter restored", secs(5), |record| {
            record.line_count == 999
                && (record.viewport.gutter_width_px - three_digits.viewport.gutter_width_px).abs() < 0.25
        })?;
        assert_eq!(shrunk.line_count, 999, "{shrunk:?}");
        Ok(())
    })
}

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
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn all_tabs_and_application_menu_are_fully_keyboard_operable() -> TestResult {
    support::run_x11_test("daily-driver-shell-menu-navigation", |session| {
        let path = session.seed_file("first.txt", "first")?;
        let path_text = support::path_text(&path);
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
