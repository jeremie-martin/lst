//! Real-display acceptance coverage for the standard daily-driver workflow.
//!
//! Run with:
//!
//!     DISPLAY=:0 cargo nextest run --profile x11 -p lst-gpui --test real_x11_daily_driver --run-ignored only

mod support;

use std::{fs, thread};

use lst_x11_harness::{Key, KeyChord};
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
        let before_search = editor.read_state()?;
        editor.send_keys_settle("save as")?;
        let after_search = editor.read_state()?;
        assert_eq!(after_search.workspace_surface, "settings", "{after_search:?}");
        assert_eq!(after_search.focused_input, "settings", "{after_search:?}");
        assert_eq!(
            after_search.revision, before_search.revision,
            "{before_search:?} -> {after_search:?}"
        );

        editor.keys("<esc>")?;
        editor.wait_state("settings search escape closes settings", secs(2), |record| {
            record.workspace_surface == "none" && record.focused_input == "editor"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn settings_search_and_rows_are_keyboard_operable_without_editing_the_document() -> TestResult {
    support::run_x11_test("daily-driver-settings-keyboard", |session| {
        let path = session.seed_file("settings-keyboard.txt", "keep me unchanged\n")?;
        let mut editor = session.open_file("settings-keyboard", &path)?;
        let before = editor.read_state()?;

        editor.keys("<C-,><tab>")?;
        editor.wait_state("first settings row selected", secs(2), |record| {
            record.workspace_surface == "settings" && record.settings_selected_item.as_deref() == Some("input_mode")
        })?;
        editor.keys("<enter>")?;
        editor.wait_state("input mode changed from settings row", secs(2), |record| {
            record.workspace_surface == "settings"
                && record.settings_selected_item.as_deref() == Some("input_mode")
                && record.input_mode == "vim"
        })?;

        editor.keys("/")?;
        editor.wait_state("settings search refocused", secs(2), |record| {
            record.workspace_surface == "settings" && record.settings_selected_item.is_none()
        })?;
        editor.send_keys_settle("word wrap")?;
        editor.keys("<tab>")?;
        editor.wait_state("filtered word-wrap row selected", secs(2), |record| {
            record.workspace_surface == "settings" && record.settings_selected_item.as_deref() == Some("word_wrap")
        })?;
        editor.keys("<space>")?;
        let changed = editor.wait_state("word wrap toggled from settings row", secs(2), |record| {
            record.workspace_surface == "settings"
                && record.settings_selected_item.as_deref() == Some("word_wrap")
                && !record.word_wrap_enabled
        })?;
        assert_eq!(changed.revision, before.revision, "{before:?} -> {changed:?}");
        assert_eq!(fs::read_to_string(path)?, "keep me unchanged\n");

        editor.keys("<esc>")?;
        editor.wait_state("settings closes from selected row", secs(2), |record| {
            record.workspace_surface == "none" && record.focused_input == "editor"
        })?;
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
        editor.wait_state("app menu owns focus", secs(2), |record| {
            record.workspace_surface == "app_menu" && record.focused_input == "app_menu"
        })?;
        editor.send_keys_settle("z<down>")?;
        editor.press(KeyChord::Ctrl(Key::Char('n')))?;
        editor.press(KeyChord::Ctrl(Key::Tab))?;
        editor.press(KeyChord::Ctrl(Key::Char('f')))?;
        thread::sleep(std::time::Duration::from_millis(200));
        let covered = editor.read_state()?;
        assert_eq!(covered.revision, before.revision, "{before:?} -> {covered:?}");
        assert_eq!(covered.active_tab_id, before.active_tab_id, "{before:?} -> {covered:?}");
        assert!(!covered.find.visible, "{covered:?}");
        assert_eq!(covered.workspace_surface, "app_menu", "{covered:?}");
        let cursor_state = |record: &lst_x11_harness::StateTraceRecord| {
            record
                .cursors
                .iter()
                .map(|cursor| (cursor.anchor_char, cursor.head_char))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            cursor_state(&covered),
            cursor_state(&before),
            "{before:?} -> {covered:?}"
        );

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
fn app_menu_does_not_dim_the_editor() -> TestResult {
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
        let (_, _, max_x, max_y) = diff.changed_bounds.ok_or("opening the app menu changed no pixels")?;
        let scale = open.viewport.scale_factor.max(1.0);

        assert!(
            f32::from(max_x) < 340.0 * scale && f32::from(max_y) < 430.0 * scale,
            "app menu changed pixels outside its bounds, indicating a tinted backdrop: {diff:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn standard_duplicate_line_above() -> TestResult {
    support::run_x11_test("daily-driver-duplicate-up", |session| {
        let path = session.seed_file("duplicate.txt", "alpha\nbeta")?;
        let mut editor = session.open_file("duplicate", &path)?;

        editor.click_at_text(1, 2)?;
        editor.keys("<C-A-S-up>")?;
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
        let identity = path.to_string_lossy().into_owned();
        let prompted = editor.wait_state("dirty close prompt", secs(2), |record| {
            record.close_prompt_file.as_deref() == Some(identity.as_str())
                && record.close_prompt_status.as_deref() == Some("reviewing")
        })?;
        assert!(prompted.active_tab_modified, "{prompted:?}");

        editor.send_keys_settle("<S-d><A-d><C-d><S-enter><A-enter><C-enter>")?;
        let blocked = editor.read_state()?;
        assert_eq!(
            blocked.close_prompt_file.as_deref(),
            Some(identity.as_str()),
            "modified discard/save keys escaped the close prompt: {blocked:?}"
        );
        assert_eq!(blocked.close_prompt_status.as_deref(), Some("reviewing"), "{blocked:?}");
        assert!(blocked.active_tab_modified, "{blocked:?}");
        assert_eq!(fs::read_to_string(&path)?, "body");

        editor.keys("<esc>")?;
        editor.wait_state("close prompt cancelled", secs(2), |record| {
            record.close_prompt_file.is_none() && record.active_tab_modified
        })?;
        assert_eq!(fs::read_to_string(&path)?, "body");
        Ok(())
    })
}
