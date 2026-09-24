//! Real-display acceptance coverage for selection commands: syntax-aware
//! expansion, column selection, and the configured cursor limit.

mod support;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn syntax_selection_expands_in_layers_and_shrinks_the_exact_history() -> TestResult {
    support::run_x11_test("polish-smart-selection", |session| {
        let path = session.seed_file("smart.rs", "fn main() { let camelCase = call(1); }\n")?;
        let mut editor = session.open_file("polish-smart-selection", &path)?;
        editor.click_at_text(0, 22)?;

        editor.keys("<S-A-right>")?;
        editor.wait_state("subword selected", secs(2), |record| selection_width(record) == 4)?;

        editor.keys("<S-A-right>")?;
        editor.wait_state("word selected", secs(2), |record| selection_width(record) == 9)?;

        editor.keys("<S-A-right>")?;
        editor.wait_state("syntax node selected", secs(2), |record| selection_width(record) > 9)?;

        editor.keys("<S-A-left><S-A-left>")?;
        editor.wait_state("shrunk back to the subword", secs(2), |record| {
            selection_width(record) == 4
        })?;
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
