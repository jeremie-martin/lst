//! Real-display specs for the settings surface and `config.toml`: keyboard
//! operation, persistence, reloading the file while running, invalid files,
//! and settings whose effect is visible outside the settings surface.

mod support;

use std::fs;
use std::path::Path;

use support::{secs, wait_file_matching, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn standard_mode_is_default_and_workspace_surfaces_are_keyboard_reachable() -> TestResult {
    support::run_x11_test("settings-surfaces", |session| {
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
        // Typing goes to the settings search, not the document behind it.
        editor.expect_keys_ignored(
            "save as",
            "<esc>",
            |record| record.workspace_surface == "none" && record.focused_input == "editor",
            |record| record.revision == initial.revision,
        )?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn settings_search_and_rows_are_keyboard_operable_without_editing_the_document() -> TestResult {
    support::run_x11_test("settings-keyboard", |session| {
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

        editor.keys("<esc>")?;
        editor.wait_state("settings closes from selected row", secs(2), |record| {
            record.workspace_surface == "none" && record.focused_input == "editor"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn settings_categories_keep_search_global_and_changes_persistent() -> TestResult {
    support::run_x11_test("settings-categories", |session| {
        let settings_path =
            session.seed_settings("version = 1\n[editor]\ncursor_blink = false\n[appearance]\ntheme = 'light'\n")?;
        let (mut editor, _path) = session.open("settings-categories")?;
        editor.resize(1140, 820)?;
        editor.keys("<C-,>")?;
        editor.wait_state("settings opens", secs(2), |state| state.workspace_surface == "settings")?;

        // The category rail has no keyboard route and no traced bounds; at
        // this window size these points are its Appearance and Guides rows.
        editor.click_at(140, 110)?;
        editor.keys("<tab>")?;
        editor.wait_state("appearance category starts at typography", secs(2), |state| {
            state.settings_selected_item.as_deref() == Some("font_family")
        })?;

        // Search spans every category and changes persist immediately.
        editor.keys("/")?;
        editor.send_keys_settle("smooth cursor")?;
        editor.keys("<tab><space>")?;
        editor.wait_state("search reaches a different category", secs(2), |state| {
            state.settings_selected_item.as_deref() == Some("smooth_cursor")
        })?;
        wait_file_matching(&settings_path, "smooth cursor enabled", |text| {
            text.contains("smooth_cursor = true")
        })?;

        editor.keys("/")?;
        editor.send_keys_settle("<C-a>theme")?;
        editor.keys("<tab><left>")?;
        editor.wait_state("theme changed from search", secs(2), |state| state.theme_name == "Dark")?;
        wait_file_matching(&settings_path, "dark theme", |text| text.contains("theme = \"dark\""))?;

        editor.click_at(140, 144)?;
        editor.keys("<tab>")?;
        editor.wait_state("guides category is reachable", secs(2), |state| {
            state.settings_selected_item.as_deref() == Some("match_brackets")
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn settings_value_editor_validates_and_commits_without_touching_the_document() -> TestResult {
    support::run_x11_test("settings-values", |session| {
        let path = session.seed_file("settings-values.txt", "unchanged\n")?;
        let mut editor = session.open_file("settings-values", &path)?;
        let before = editor.read_state()?;

        editor.keys("<C-,>")?;
        editor.send_keys_settle("rulers")?;
        editor.keys("<tab><enter>1001<enter>")?;
        editor.wait_state("invalid rulers remain open", secs(2), |record| {
            record.settings_value_editor_item.as_deref() == Some("rulers") && record.settings_value_error.is_some()
        })?;
        editor.send_keys_settle("<C-a>120, 80, 80<enter>")?;
        let committed = editor.wait_state("rulers committed", secs(2), |record| {
            record.settings_value_editor_item.is_none() && record.editor_polish.rulers == [80, 120]
        })?;
        assert_eq!(committed.revision, before.revision, "{before:?} -> {committed:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn editing_the_config_file_while_running_applies_the_new_settings() -> TestResult {
    support::run_x11_test("settings-reload", |session| {
        let settings_path =
            session.seed_settings("version = 1\n[editor]\nword_wrap = true\nmulti_cursor_limit = 50\n")?;
        let (mut editor, _path) = session.open("settings-reload")?;
        editor.wait_state("configured settings", secs(2), |record| {
            record.word_wrap_enabled && record.editor_polish.multi_cursor_limit == 50
        })?;

        fs::write(
            &settings_path,
            "version = 1\n[editor]\nword_wrap = false\nmulti_cursor_limit = 7\n",
        )?;
        editor.wait_state("edited settings applied", secs(5), |record| {
            !record.word_wrap_enabled
                && record.editor_polish.multi_cursor_limit == 7
                && record.status_message == "Settings reloaded."
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn settings_changes_refuse_to_overwrite_an_invalid_config_file() -> TestResult {
    support::run_x11_test("settings-invalid-file", |session| {
        let broken = "version = 1\n[editor\nword_wrap = false\n";
        let settings_path = session.seed_settings(broken)?;
        let (mut editor, _path) = session.open("settings-invalid-file")?;

        editor.keys("<C-,><tab>")?;
        editor.wait_state("first settings row selected", secs(2), |record| {
            record.settings_selected_item.as_deref() == Some("input_mode")
        })?;
        editor.keys("<enter>")?;
        editor.wait_state("save refused with the parse error", secs(2), |record| {
            record.input_mode == "vim"
                && record.status_message.starts_with("Settings were not saved")
                && record.status_message.contains("Invalid settings")
        })?;
        assert_eq!(fs::read_to_string(&settings_path)?, broken);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn scratchpad_directory_setting_chooses_where_scratchpads_are_created() -> TestResult {
    support::run_x11_test("settings-scratchpad-directory", |session| {
        let dir = session.root().join("configured-scratchpads");
        fs::create_dir(&dir)?;
        session.seed_settings(&format!(
            "version = 1\n[files]\nscratchpad_directory = \"{}\"\n",
            dir.display()
        ))?;
        let in_dir = |path: Option<&str>| path.and_then(|path| Path::new(path).parent()) == Some(dir.as_path());

        let mut editor = session.open_files("settings-scratchpad-directory", &[])?;
        let launched = editor.read_state()?;
        assert!(in_dir(launched.active_tab_path.as_deref()), "{launched:?}");

        editor.keys("<C-n>")?;
        let created = editor.wait_state("new scratchpad in the configured directory", secs(5), |record| {
            record.active_tab_id != launched.active_tab_id && in_dir(record.active_tab_path.as_deref())
        })?;
        let path = created.active_tab_path.ok_or("scratchpad has no path")?;
        editor.keys("configured")?;
        editor.expect_file(Path::new(&path), "configured")?;
        Ok(())
    })
}
