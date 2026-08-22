//! Real-display acceptance coverage for structural and daily-driver polish.

mod support;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn guide_decorations_are_disabled_by_default() -> TestResult {
    support::run_x11_test("polish-guides-default-off", |session| {
        let path = session.seed_file("default-guides.rs", "fn main() {\n    let value = (1 + 2);\n}\n")?;
        let mut editor = session.open_file("polish-guides-default-off", &path)?;

        let state = editor.wait_state("default guide settings", secs(5), |record| {
            record.viewport.structural_pair_count >= 3
                && record.viewport.guide_count == 0
                && record.editor_polish.bracket_pair_guides == "off"
                && record.editor_polish.bracket_pair_horizontal_guides == "off"
                && !record.editor_polish.indent_guides
                && !record.editor_polish.highlight_active_indent_guide
        })?;
        assert_eq!(state.viewport.guide_count, 0);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn enclosing_brackets_are_decorated_and_the_jump_command_uses_the_same_pairs() -> TestResult {
    support::run_x11_test("polish-bracket-match", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nbracket_pair_guides = 'active'\nbracket_pair_horizontal_guides = 'active'\nindent_guides = true\nhighlight_active_indent_guide = true\n",
        )?;
        let path = session.seed_file("brackets.rs", "fn main() {\n    let value = (1 + 2);\n}\n")?;
        let mut editor = session.open_file("polish-bracket-match", &path)?;

        editor.click_at_text(1, 19)?;
        let decorated = editor.wait_state("enclosing bracket decoration", secs(2), |record| {
            record.viewport.structural_pair_count >= 3
                && record.viewport.bracket_matches.len() == 2
                && record.viewport.guide_count > 0
        })?;
        assert_eq!(decorated.editor_polish.match_brackets, "always");

        editor.click_at_text(1, 16)?;
        editor.keys("<C-S-\\>")?;
        editor.expect_cursor_heads(&[(1, 22)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn injected_brackets_stop_matching_after_selection_is_quoted() -> TestResult {
    support::run_x11_test("polish-injected-bracket-edit", |session| {
        let path = session.seed_file("injected.md", "```rust\nfn fenced() { let value = (1 + 2); }\n```\n")?;
        let mut editor = session.open_file("polish-injected-bracket-edit", &path)?;

        let initial = editor.wait_state("injected brackets parsed", secs(5), |record| {
            record.viewport.structural_pair_count == 3
        })?;
        assert_eq!(initial.viewport.structural_pair_count, 3);

        editor.click_at_text(1, 26)?;
        editor.keys("<S-right><S-right><S-right><S-right><S-right><S-right><S-right>\"")?;
        editor.wait_state("quoted injected brackets excluded", secs(5), |record| {
            record.viewport.structural_pair_count == 2
        })?;
        editor.save_then_expect_file(&path, "```rust\nfn fenced() { let value = \"(1 + 2)\"; }\n```\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn syntax_selection_expands_in_layers_and_shrinks_the_exact_history() -> TestResult {
    support::run_x11_test("polish-smart-selection", |session| {
        let path = session.seed_file("smart.rs", "fn main() { let camelCase = call(1); }\n")?;
        let mut editor = session.open_file("polish-smart-selection", &path)?;
        editor.click_at_text(0, 22)?;

        editor.keys("<S-A-right>")?;
        let subword = editor.read_state()?;
        let subword_width = selection_width(&subword);
        assert_eq!(subword_width, 4, "{subword:?}");

        editor.keys("<S-A-right>")?;
        let word = editor.read_state()?;
        assert_eq!(selection_width(&word), 9, "{word:?}");

        editor.keys("<S-A-right>")?;
        let syntax = editor.read_state()?;
        assert!(selection_width(&syntax) > 9, "{syntax:?}");

        editor.keys("<S-A-left><S-A-left>")?;
        let shrunk = editor.read_state()?;
        assert_eq!(selection_width(&shrunk), subword_width, "{shrunk:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn polish_settings_value_editor_validates_and_commits_without_touching_the_document() -> TestResult {
    support::run_x11_test("polish-settings-values", |session| {
        let path = session.seed_file("settings-values.txt", "unchanged\n")?;
        let mut editor = session.open_file("polish-settings-values", &path)?;

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
        assert_eq!(committed.revision, 0, "{committed:?}");
        assert_eq!(std::fs::read_to_string(path)?, "unchanged\n");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn configured_cursor_limit_truncates_large_selection_sets_with_status_feedback() -> TestResult {
    support::run_x11_test("polish-cursor-limit", |session| {
        session.seed_settings("version = 1\n[editor]\nmulti_cursor_limit = 3\n")?;
        let path = session.seed_file("cursor-limit.txt", "same same same same same\n")?;
        let mut editor = session.open_file("polish-cursor-limit", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l>")?;
        let limited = editor.wait_state("cursor set limited", secs(2), |record| {
            record.cursors.len() == 3 && record.status_message.contains("limit reached")
        })?;
        assert_eq!(limited.editor_polish.multi_cursor_limit, 3);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn configured_guides_rulers_whitespace_and_control_markers_reach_the_real_viewport() -> TestResult {
    support::run_x11_test("polish-viewport-markers", |session| {
        session.seed_settings(
            "version = 1\n[editor]\nbracket_pair_guides = 'all'\nbracket_pair_horizontal_guides = 'all'\nrender_whitespace = 'all'\nrender_control_characters = true\nrulers = [4, 8]\n",
        )?;
        let path = session.seed_file("markers.rs", "fn main() {\n\tlet value = 1;  \u{1}\n}\n")?;
        let mut editor = session.open_file("polish-viewport-markers", &path)?;

        let painted = editor.wait_state("polish markers painted", secs(5), |record| {
            record.editor_polish.rulers == [4, 8]
                && record.viewport.guide_count > 0
                && record.viewport.whitespace_marker_count >= 4
                && record.viewport.control_marker_count >= 1
        })?;
        assert_eq!(painted.editor_polish.render_whitespace, "all");
        assert!(painted.editor_polish.render_control_characters);
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn configured_column_selection_command_grows_one_stable_rectangle() -> TestResult {
    support::run_x11_test("polish-column-command", |session| {
        session.seed_settings("version = 1\n[keybindings]\n\"selection.column_down\" = [\"ctrl-alt-m\"]\n")?;
        let path = session.seed_file("column-command.txt", "alpha\nbravo\ncharlie")?;
        let mut editor = session.open_file("polish-column-command", &path)?;

        editor.click_at_text(0, 2)?;
        editor.keys("<C-A-m><C-A-m>")?;
        editor.expect_cursor_heads(&[(0, 2), (1, 2), (2, 2)])?;
        editor.keys("X")?;
        editor.save_then_expect_file(&path, "alXpha\nbrXavo\nchXarlie")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn keyboard_column_selection_restores_its_column_after_a_short_line() -> TestResult {
    support::run_x11_test("polish-column-short-line", |session| {
        session.seed_settings("version = 1\n[keybindings]\n\"selection.column_down\" = [\"ctrl-alt-m\"]\n")?;
        let path = session.seed_file("column-short-line.txt", "abcdef\nx\nabcdef")?;
        let mut editor = session.open_file("polish-column-short-line", &path)?;

        editor.click_at_text(0, 5)?;
        editor.keys("<C-A-m><C-A-m>")?;
        editor.expect_cursor_heads(&[(0, 5), (1, 1), (2, 5)])?;
        Ok(())
    })
}

fn selection_width(record: &lst_x11_harness::StateTraceRecord) -> usize {
    let selection = &record.cursors[record.primary_cursor_index];
    selection.anchor_char.abs_diff(selection.head_char)
}
