//! Real-display specs for accepted find-panel behavior.

mod support;

use lst_x11_harness::{
    clipboard::{wait_clipboard_text, write_clipboard_text},
    Selection,
};
use support::{secs, EditorTestExt, FindChip, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_navigates_and_replaces_after_every_document_line_separator() -> TestResult {
    support::run_x11_test("find-document-line-separators", |session| {
        let separators = ["\r", "\r\n", "\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"];
        let mut text = String::from("prefix");
        for separator in separators {
            text.push_str(separator);
            text.push_str("needle");
        }
        let path = session.seed_file("line-separators.txt", &text)?;
        let mut editor = session.open_file("find-line-separators", &path)?;
        editor.keys("<C-f>needle")?;
        editor.expect_find_state("needle", separators.len())?;
        for line in 1..=separators.len() {
            editor.expect_cursor_heads(&[(line, 0)])?;
            editor.keys("<enter>")?;
        }
        // Submitting past the last match wraps to the first.
        editor.expect_cursor_heads(&[(1, 0)])?;
        editor.keys("<esc><S-end><C-c>")?;
        wait_clipboard_text(Selection::Clipboard, "needle", secs(5))?;
        editor.keys("<C-h><C-a>needle<tab>found<C-A-enter>")?;
        editor.save_then_expect_file(&path, &text.replace("needle", "found"))?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn lowercase_find_query_uses_smart_case() -> TestResult {
    support::run_x11_test("find-smart-case-lowercase", |session| {
        let path = session.seed_file("smart-case-lowercase.txt", "Foo foo FOO")?;
        let mut editor = session.open_file("find-smart-case-lowercase", &path)?;

        editor.keys("<C-f>foo")?;
        let record = editor.expect_find_state("foo", 3)?;
        assert!(!record.find.case_sensitive, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn uppercase_find_query_is_case_sensitive() -> TestResult {
    support::run_x11_test("find-smart-case-uppercase", |session| {
        let path = session.seed_file("smart-case-uppercase.txt", "foo Foo FOO")?;
        let mut editor = session.open_file("find-smart-case-uppercase", &path)?;

        editor.keys("<C-f>Foo")?;
        editor.expect_find_state("Foo", 1)?;
        editor.expect_cursor_heads(&[(0, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn submitting_find_query_advances_to_next_match() -> TestResult {
    support::run_x11_test("find-submit-next-match", |session| {
        let path = session.seed_file("find-next.txt", "bar foo\nbaz foo")?;
        let mut editor = session.open_file("find-submit-next-match", &path)?;

        editor.keys("<C-f>foo")?;
        editor.expect_find_state("foo", 2)?;
        editor.expect_cursor_heads(&[(0, 4)])?;

        editor.keys("<enter>")?;
        editor.wait_state("second find match", secs(5), |record| {
            record.find.visible
                && record.find.query == "foo"
                && record.find.match_count == 2
                && record.find.active_index == Some(1)
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_pos() == (1, 4))
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn repeated_ctrl_f_refocuses_without_closing_or_collapsing_replace() -> TestResult {
    support::run_x11_test("find-idempotent-open", |session| {
        let path = session.seed_file("find-idempotent.txt", "alpha beta alpha")?;
        let mut editor = session.open_file("find-idempotent", &path)?;

        editor.keys("<C-h>")?;
        editor.wait_state("replace opens expanded", secs(2), |record| {
            record.find.visible && record.find.show_replace && record.focused_input == "find_query"
        })?;
        editor.click_at_text(0, 0)?;
        editor.wait_state("editor focused with replace still open", secs(2), |record| {
            record.find.visible && record.find.show_replace && record.focused_input == "editor"
        })?;
        // No keys are ignored here: every record until Ctrl+F refocuses the
        // query must keep the expanded panel, so a close-and-reopen toggle
        // fails even though it ends in the same state.
        editor.expect_keys_ignored(
            "",
            "<C-f>",
            |record| record.focused_input == "find_query",
            |record| record.find.visible && record.find.show_replace,
        )?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn workspace_shortcuts_work_while_find_inputs_are_focused() -> TestResult {
    support::run_x11_test("find-workspace-shortcuts", |session| {
        let first = session.seed_file("find-shortcuts-first.txt", "first")?;
        let second = session.seed_file("find-shortcuts-second.txt", "second")?;
        let second_path = second.to_string_lossy().into_owned();
        let mut editor = session.open_files("find-workspace-shortcuts-close", &[first, second])?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus before close", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("<C-w>")?;
        editor.wait_state("close tab from find focus", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&second_path)
                && record.status_message == "Closed tab."
                && record.focused_input == "editor"
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus before new tab", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("<C-n>")?;
        editor.wait_state("new tab from find focus", secs(5), |record| {
            record.active_tab_path.as_deref() != Some(&second_path)
                && record.active_tab_index == 1
                && record.status_message == "Created a new scratchpad."
                && record.focused_input == "editor"
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus before next tab", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("<C-tab>")?;
        editor.wait_state("next tab from find focus", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&second_path)
                && record.active_tab_index == 0
                && record.find.visible
                && record.focused_input == "editor"
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus before previous tab", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("<C-S-tab>")?;
        editor.wait_state("previous tab from find focus", secs(5), |record| {
            record.active_tab_path.as_deref() != Some(&second_path)
                && record.active_tab_index == 1
                && record.find.visible
                && record.focused_input == "editor"
        })?;

        editor.keys("<C-h>")?;
        editor.wait_state("replace focus before tab switch", secs(2), |record| {
            record.find.visible && record.find.show_replace && record.focused_input == "find_replace"
        })?;
        editor.keys("<C-tab>")?;
        editor.wait_state("next tab from replace focus", secs(5), |record| {
            record.active_tab_path.as_deref() == Some(&second_path)
                && record.active_tab_index == 0
                && record.find.visible
                && record.find.show_replace
                && record.focused_input == "editor"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn tab_strip_buttons_work_while_find_query_is_focused() -> TestResult {
    support::run_x11_test("find-tab-strip-buttons", |session| {
        let recent = session.seed_file("find-recent-button-target.txt", "recent target\n")?;
        let recent_path = recent.to_string_lossy().into_owned();
        session.seed_recent_files(std::slice::from_ref(&recent))?;
        let (mut editor, _scratchpad) = session.open("find-tab-strip-buttons")?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus before new-tab click", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.click_new_tab_button()?;
        editor.wait_state("new tab button from find focus", secs(5), |record| {
            record.find.visible
                && record.status_message == "Created a new scratchpad."
                && record.focused_input == "editor"
        })?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus before recent click", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.click_recent_files_button()?;
        editor.wait_state("recent button from find focus", secs(5), |record| {
            record.recent_panel_open
                && record.focused_input == "recent_query"
                && record.recent_panel_selected_path.as_deref() == Some(recent_path.as_str())
        })?;
        editor.keys("<escape>")?;
        editor.wait_state("recent panel closed", secs(5), |record| {
            !record.recent_panel_open && record.focused_input == "editor"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn case_sensitive_chip_disables_smart_case() -> TestResult {
    support::run_x11_test("find-chip-case-sensitive", |session| {
        let path = session.seed_file("find-chip-case.txt", "Foo foo FOO")?;
        let mut editor = session.open_file("find-chip-case-sensitive", &path)?;

        editor.keys("<C-f>foo")?;
        editor.expect_find_state("foo", 3)?;

        editor.click_find_chip(FindChip::CaseSensitive)?;
        editor.wait_state("case-sensitive find", secs(5), |record| {
            record.find.visible
                && record.find.query == "foo"
                && record.find.case_sensitive
                && record.find.match_count == 1
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn whole_word_chip_restricts_matches_to_word_boundaries() -> TestResult {
    support::run_x11_test("find-chip-whole-word", |session| {
        let path = session.seed_file("find-chip-word.txt", "foobar foo_bar foo (foo) foo!")?;
        let mut editor = session.open_file("find-chip-whole-word", &path)?;

        editor.keys("<C-f>foo")?;
        editor.expect_find_state("foo", 5)?;

        editor.click_find_chip(FindChip::WholeWord)?;
        editor.wait_state("whole-word find", secs(5), |record| {
            record.find.visible && record.find.query == "foo" && record.find.whole_word && record.find.match_count == 3
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn regex_chip_treats_query_as_pattern() -> TestResult {
    support::run_x11_test("find-chip-regex-pattern", |session| {
        let path = session.seed_file("find-chip-regex.txt", "foo foa fo.")?;
        let mut editor = session.open_file("find-chip-regex-pattern", &path)?;

        editor.keys("<C-f>fo.")?;
        editor.expect_find_state("fo.", 1)?;

        editor.click_find_chip(FindChip::Regex)?;
        editor.wait_state("regex find", secs(5), |record| {
            record.find.visible && record.find.query == "fo." && record.find.use_regex && record.find.match_count == 3
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn invalid_regex_query_reports_error_and_clears_matches() -> TestResult {
    support::run_x11_test("find-chip-invalid-regex", |session| {
        let path = session.seed_file("find-chip-invalid-regex.txt", "[abc] [def]")?;
        let mut editor = session.open_file("find-chip-invalid-regex", &path)?;

        editor.keys("<C-f>[")?;
        editor.expect_find_state("[", 2)?;

        editor.click_find_chip(FindChip::Regex)?;
        editor.wait_state("invalid regex find", secs(5), |record| {
            record.find.visible
                && record.find.query == "["
                && record.find.use_regex
                && record.find.match_count == 0
                && record.find.active_index.is_none()
                && record
                    .find
                    .error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("regex:"))
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_query_ctrl_a_replaces_existing_query() -> TestResult {
    support::run_x11_test("find-query-ctrl-a-replace", |session| {
        let path = session.seed_file("find-query-replace.txt", "alpha beta\nalpha beta")?;
        let mut editor = session.open_file("find-query-ctrl-a-replace", &path)?;

        editor.keys("<C-f>alpha")?;
        editor.expect_find_state("alpha", 2)?;

        editor.keys("<C-a>beta")?;
        editor.wait_state("query replaced", secs(5), |record| {
            record.find.visible
                && record.find.query == "beta"
                && record.find.match_count == 2
                && record.focused_input == "find_query"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_respects_grapheme_boundaries_for_combining_clusters() -> TestResult {
    support::run_x11_test("find-grapheme-boundaries", |session| {
        let path = session.seed_file("find-grapheme.txt", "cafe\u{0301}\ncafe")?;
        let mut editor = session.open_file("find-grapheme-boundaries", &path)?;

        editor.keys("<C-f>e")?;
        editor.expect_find_state("e", 1)?;
        editor.expect_cursor_heads(&[(1, 3)])?;

        write_clipboard_text(Selection::Clipboard, "e\u{0301}")?;
        editor.keys("<C-a><C-v>")?;
        editor.expect_find_state("e\u{0301}", 1)?;
        editor.expect_cursor_heads(&[(0, 3)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn replace_current_match_only_rewrites_active_match() -> TestResult {
    support::run_x11_test("find-replace-one-active-match", |session| {
        let path = session.seed_file("replace-one.txt", "foo foo foo")?;
        let mut editor = session.open_file("find-replace-one-active-match", &path)?;

        editor.keys("<C-h>foo")?;
        editor.expect_find_state("foo", 3)?;
        editor.keys("<enter>")?;
        editor.wait_state("second match active", secs(5), |record| {
            record.find.active_index == Some(1) && record.focused_input == "find_query"
        })?;
        editor.keys("<tab>bar<enter>")?;
        editor.save_then_expect_file(&path, "foo bar foo")?;
        Ok(())
    })
}
