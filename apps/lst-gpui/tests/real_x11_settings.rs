//! Real-display specs for the settings surface and `config.toml`: keyboard
//! operation, persistence, reloading the file while running, invalid files,
//! and settings whose effect is visible outside the settings surface.

mod support;

use std::fs;
use std::path::Path;

use lst_x11_harness::{clipboard::wait_clipboard_text, Selection};

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
fn clicking_settings_search_clears_row_focus_before_typing() -> TestResult {
    support::run_x11_test("settings-mouse-search-focus", |session| {
        let path = session.seed_file("settings-mouse-search.txt", "keep me unchanged\n")?;
        let mut editor = session.open_file("settings-mouse-search", &path)?;
        editor.keys("<C-,>word wrap<tab>")?;
        let record = editor.wait_state(
            "filtered settings row selected before search click",
            secs(5),
            |record| {
                record.settings_selected_item.as_deref() == Some("word_wrap")
                    && record
                        .settings_search_input
                        .as_ref()
                        .and_then(|field| field.bounds_px)
                        .is_some()
            },
        )?;
        let (x, y, width, height) = record
            .settings_search_input
            .as_ref()
            .and_then(|field| field.bounds_px)
            .ok_or("settings search bounds missing")?;
        let scale = if record.viewport.scale_factor > 0.0 {
            record.viewport.scale_factor
        } else {
            1.0
        };
        editor.click_at(
            ((x + width * 0.5) * scale).round() as i32,
            ((y + height * 0.5) * scale).round() as i32,
        )?;
        editor.wait_state("mouse focus clears the selected-row owner", secs(5), |record| {
            record.workspace_surface == "settings" && record.settings_selected_item.is_none()
        })?;
        editor.keys("_<C-a><C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "word wrap_", secs(5))?;
        editor.keys("<esc>")?;
        editor.save_then_expect_file(&path, "keep me unchanged\n")?;
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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_config_reload_is_reported_and_not_overwritten_by_settings_changes() -> TestResult {
    support::run_x11_test("settings-invalid-reload", |session| {
        let settings_path =
            session.seed_settings("version = 1\n[editor]\nword_wrap = false\nmulti_cursor_limit = 7\n")?;
        let (mut editor, _path) = session.open("settings-invalid-reload")?;

        let broken = "version = 1\n[editor\nword_wrap = false\n";
        fs::write(&settings_path, broken)?;
        editor.wait_state("reload failure reported", secs(5), |record| {
            record.status_message.starts_with("Settings reload failed")
                && !record.word_wrap_enabled
                && record.editor_polish.multi_cursor_limit == 7
        })?;

        editor.keys("<C-,><tab>")?;
        editor.wait_state("first settings row selected", secs(2), |record| {
            record.settings_selected_item.as_deref() == Some("input_mode")
        })?;
        editor.keys("<enter>")?;
        editor.wait_state("input mode changed", secs(2), |record| record.input_mode == "vim")?;
        assert_eq!(fs::read_to_string(&settings_path)?, broken);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn changing_one_setting_preserves_other_values_comments_and_keybindings() -> TestResult {
    support::run_x11_test("settings-preserve-toml", |session| {
        let original = "# personal configuration\nversion = 1\n[editor]\nfont_size = 0x11 # hexadecimal\nfont_family = 'TX-02'\nword_wrap = true # wrapping\n[appearance]\ntheme = 'light'\n[keybindings]\n# Optional shortcut:\n# 'file.save' = ['ctrl-s']\n'edit.duplicate_line' = [ 'ctrl-shift-d' ] # keep binding\n";
        let settings_path = session.seed_settings(original)?;
        let (mut editor, _) = session.open("preserve-toml")?;
        editor.keys("<C-,>word wrap<tab><space>")?;
        editor.wait_state("wrap changed", secs(5), |s| !s.word_wrap_enabled)?;
        editor.expect_file(
            &settings_path,
            &original.replace("word_wrap = true", "word_wrap = false"),
        )?;
        Ok(())
    })
}

struct RestorePermissions(std::path::PathBuf, fs::Permissions);
impl Drop for RestorePermissions {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, self.1.clone());
    }
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn failed_reset_keeps_active_settings_and_failed_saves_stay_visible_until_recovery() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    support::run_x11_test("settings-failure-recovery", |session| {
        let original =
            "version = 1\n[editor]\nword_wrap = false\nmulti_cursor_limit = 7\n[appearance]\nzoom_level = 1\n";
        let settings_path = session.seed_settings(original)?;
        let parent = settings_path.parent().unwrap().to_path_buf();
        let restore = RestorePermissions(parent.clone(), fs::metadata(&parent)?.permissions());
        let screenshot = session.root().join("settings-save-error.ppm");
        let (mut editor, _) = session.open_vim("failure-recovery")?;
        editor.wait_state("nondefault settings loaded", secs(5), |s| {
            !s.word_wrap_enabled && s.editor_polish.multi_cursor_limit == 7
        })?;
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o555))?;
        editor.keys("<C-,>reset<tab><enter><tab><enter>")?;
        let failed = editor.wait_state("reset failure", secs(5), |s| {
            s.status_message.starts_with("Settings were not reset")
        })?;
        assert_eq!(failed.editor_polish.multi_cursor_limit, 7);
        assert_eq!(failed.input_mode, "vim");
        assert!(!failed.word_wrap_enabled);
        assert!(failed
            .settings_error
            .as_deref()
            .is_some_and(|error| error.starts_with("Settings were not reset")));
        editor.expect_file(&settings_path, original)?;
        editor.keys("/<C-a>word wrap<tab><space>")?;
        editor.wait_state("active but unsaved change", secs(5), |s| {
            s.word_wrap_enabled
                && s.settings_error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("Changes are active but not saved"))
        })?;
        editor.wait_quiet(std::time::Duration::from_millis(150), secs(3))?;
        editor.screenshot()?.write_ppm(screenshot)?;
        editor.expect_file(&settings_path, original)?;
        drop(restore);
        editor.keys("<space>")?;
        editor.wait_state("persistence recovers", secs(5), |s| {
            !s.word_wrap_enabled && s.settings_error.is_none()
        })?;
        editor.keys("/<C-a>reset<tab><enter><tab><enter>")?;
        editor.wait_state("successful reset applies defaults", secs(5), |s| {
            s.word_wrap_enabled
                && s.input_mode == "standard"
                && s.editor_polish.multi_cursor_limit == 10_000
                && s.settings_error.is_none()
        })?;
        editor.expect_file(&settings_path, "version = 1\n")?;
        editor.keys("<esc>")?;
        fs::write(&settings_path, "version = 1\n[editor]\ninput_mode = 'vim'\n")?;
        editor.wait_state("mode reload still works after reset", secs(5), |s| {
            s.input_mode == "vim"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn settings_updates_follow_symlinks_and_preserve_private_permissions() -> TestResult {
    use std::os::unix::fs::{symlink, PermissionsExt};
    support::run_x11_test("settings-symlink", |session| {
        let settings_path = session.seed_settings("version = 1\n[editor]\nword_wrap = true\n")?;
        let target = settings_path.with_file_name("personal.toml");
        fs::rename(&settings_path, &target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        symlink("personal.toml", &settings_path)?;
        let (mut editor, _) = session.open("symlink")?;
        editor.keys("<C-,>word wrap<tab><space>")?;
        editor.wait_state("wrap changed", secs(5), |s| !s.word_wrap_enabled)?;
        editor.expect_file(&target, "version = 1\n[editor]\nword_wrap = false\n")?;
        assert_eq!(fs::read_link(&settings_path)?, Path::new("personal.toml"));
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(settings_path.parent().unwrap())?.count(), 2);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn home_relative_note_directory_is_resolved_and_invalid_reload_keeps_active_settings() -> TestResult {
    support::run_x11_test("settings-note-path", |session| {
        let original = "version = 1\n[editor]\nword_wrap = false\nmulti_cursor_limit = 7\n[files]\nscratchpad_directory = '~/notes' # portable\n";
        let settings_path = session.seed_settings(original)?;
        let expected = session.root().join("home/notes");
        let mut editor = session.open_files("note-path", &[])?;
        let state = editor.read_state()?;
        assert_eq!(
            Path::new(state.active_tab_path.as_deref().unwrap()).parent(),
            Some(expected.as_path())
        );
        editor.keys("<C-,>word wrap<tab><space>")?;
        editor.wait_state("wrap changed", secs(5), |s| s.word_wrap_enabled)?;
        editor.expect_file(
            &settings_path,
            &original.replace("word_wrap = false", "word_wrap = true"),
        )?;
        let invalid = "version = 1\n[files]\nscratchpad_directory = 'relative/notes'\n";
        fs::write(&settings_path, invalid)?;
        editor.wait_state("invalid path is visible and old settings survive", secs(5), |s| {
            s.settings_error
                .as_deref()
                .is_some_and(|error| error.contains("must be absolute"))
                && s.editor_polish.multi_cursor_limit == 7
                && s.word_wrap_enabled
        })?;
        editor.keys("<esc><C-n>")?;
        let created = editor.wait_state("new note uses last valid directory", secs(5), |s| {
            s.active_tab_id != state.active_tab_id
        })?;
        assert_eq!(
            Path::new(created.active_tab_path.as_deref().unwrap()).parent(),
            Some(expected.as_path())
        );
        editor.expect_file(&settings_path, invalid)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn invalid_xdg_config_roots_fall_back_to_the_absolute_home_directory() -> TestResult {
    use std::ffi::OsStr;
    support::run_x11_test("settings-xdg-root", |session| {
        let fallback = session.root().join("home/.config/lst/config.toml");
        fs::create_dir_all(fallback.parent().unwrap())?;
        fs::write(
            &fallback,
            "version = 1\n[editor]\nword_wrap = false\nmulti_cursor_limit = 9\n",
        )?;
        let path = session.seed_file("document.txt", "unchanged")?;
        for (label, root) in [("empty-xdg", ""), ("relative-xdg", "relative-config")] {
            let mut editor =
                session.open_file_with_env(label, &path, &[(OsStr::new("XDG_CONFIG_HOME"), OsStr::new(root))])?;
            editor.wait_state("fallback settings loaded", secs(5), |s| {
                !s.word_wrap_enabled && s.editor_polish.multi_cursor_limit == 9
            })?;
        }
        Ok(())
    })
}
