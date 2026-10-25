//! Accepted real-X11 behavior for "Replace All" honoring find-in-selection.
//!
//! Pinned chords:
//!
//! - `Alt+S` — toggle find-in-selection scope (`ToggleFindInSelection`).
//!   When toggled with an active text selection, the scope captures the
//!   union of nonempty selection ranges.
//! - `Ctrl+Alt+Enter` — invoke `ReplaceAll`. With selection scope, only
//!   matches inside the captured range are rewritten. With the default
//!   document scope, every match in the buffer is rewritten.
//!
//! `Ctrl+H` already opens the replace panel with focus on the find query
//! input. Typing populates the query; `Tab` advances focus to the replace
//! input.
//!
mod support;

use lst_x11_harness::{clipboard::write_clipboard_text, Editor, Selection};

use support::{secs, EditorTestExt, FindChip, SelectionTestExt, SupportResult, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn replace_all_with_document_scope_mutates_every_match() -> TestResult {
    support::run_x11_test("replace-scope-document", |session| {
        let path = session.seed_file("replace-scope-document.txt", "x foo\ny foo foo\nz foo\n")?;
        let mut editor = session.open_file("replace-scope-document", &path)?;

        // No prior selection, no in-selection toggle. Default scope is
        // document — Replace All rewrites every "foo".
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-h>")?;
        editor.wait_state("find query focus", secs(2), |record| {
            record.focused_input == "find_query" && record.find.show_replace && record.find.scope == "document"
        })?;

        editor.keys("foo")?;
        editor.expect_find_state("foo", 4)?;
        editor.keys("<tab>bar<C-A-enter>")?;
        editor.expect_find_state("foo", 0)?;
        editor.save_then_expect_file(&path, "x bar\ny bar bar\nz bar\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn replace_all_in_selection_preserves_outside_text_byte_for_byte() -> TestResult {
    support::run_x11_test("replace-scope-preserves-outside", |session| {
        // Mixed content: matches inside the selection range and matches
        // outside it. Replace All in selection must rewrite only the
        // selected matches; outside lines stay byte-identical, including
        // the matches and the surrounding whitespace/newlines.
        let fixture = "leading foo  \nselected foo and foo\ntrailing foo  \n";
        let path = session.seed_file("replace-scope-preserves.txt", fixture)?;
        let mut editor = session.open_file("replace-scope-preserves", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<down><S-end>")?;
        editor.expect_cursor_heads(&[(1, 20)])?;

        editor.keys("<C-h>")?;
        editor.wait_state("find query focus", secs(2), |record| {
            record.focused_input == "find_query" && record.find.show_replace
        })?;

        editor.keys("<A-s>")?;
        editor.wait_state("scope captured", secs(2), |record| record.find.scope == "selection")?;

        editor.keys("foo")?;
        editor.expect_find_state("foo", 2)?;
        editor.keys("<tab>bar<C-A-enter>")?;
        editor.expect_find_state("foo", 0)?;
        // Inside-selection "foo" instances become "bar". Trailing two spaces
        // on the outside lines must survive — this catches an implementation
        // that accidentally normalizes trailing whitespace as part of the
        // replace pipeline.
        editor.save_then_expect_file(&path, "leading foo  \nselected bar and bar\ntrailing foo  \n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn captured_scope_keeps_disjoint_ranges_through_navigation_edits_and_history() -> TestResult {
    support::run_x11_test("scope-disjoint-history", |session| {
        let path = session.seed_file("scope.txt", "foo foo foo")?;
        let mut editor = session.open_file("scope-disjoint-history", &path)?;
        editor.keys("<C-home><C-d><C-d><C-k><C-d>")?;
        editor.expect_selection_ranges(&[(0, 3), (8, 11)])?;
        editor.keys("<C-h><A-s><C-a>foo")?;
        expect_scoped_matches(&mut editor, "foo", 2)?;
        editor.keys("<enter><enter><enter>")?;
        expect_scoped_matches(&mut editor, "foo", 2)?;
        editor.keys("<esc><C-home>prefix ")?;
        expect_scoped_matches(&mut editor, "foo", 2)?;
        editor.keys("<C-h><tab>longer<C-A-enter>")?;
        expect_scoped_matches(&mut editor, "foo", 0)?;
        editor.save_then_expect_file(&path, "prefix longer foo longer")?;
        editor.keys("<C-f><C-a>longer")?;
        expect_scoped_matches(&mut editor, "longer", 2)?;
        editor.keys("<esc><C-z>")?;
        expect_scoped_matches(&mut editor, "longer", 0)?;
        editor.save_then_expect_file(&path, "prefix foo foo foo")?;
        editor.keys("<C-y>")?;
        expect_scoped_matches(&mut editor, "longer", 2)?;
        editor.save_then_expect_file(&path, "prefix longer foo longer")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn captured_scope_tracks_history_from_before_capture_and_restores_deleted_ranges() -> TestResult {
    support::run_x11_test("scope-old-history", |session| {
        let path = session.seed_file("scope.txt", "foo foo foo")?;
        let mut editor = session.open_file("scope-old-history", &path)?;
        write_clipboard_text(Selection::Clipboard, "prefix ")?;
        editor.keys("<C-home><C-v><C-g>1:12<enter><S-right><S-right><S-right>")?;
        editor.expect_selection_ranges(&[(11, 14)])?;
        editor.keys("<C-h><A-s><esc><C-z>")?;
        expect_scoped_matches(&mut editor, "foo", 1)?;
        editor.keys("<f3>")?;
        editor.expect_cursor_heads(&[(0, 4)])?;
        editor.save_then_expect_file(&path, "foo foo foo")?;
        editor.keys("<C-y><f3>")?;
        editor.expect_cursor_heads(&[(0, 11)])?;
        editor.save_then_expect_file(&path, "prefix foo foo foo")?;
        editor.keys("<S-right><S-right><S-right><bs>")?;
        expect_scoped_matches(&mut editor, "foo", 0)?;
        editor.save_then_expect_file(&path, "prefix foo  foo")?;
        editor.keys("<C-z>")?;
        expect_scoped_matches(&mut editor, "foo", 1)?;
        editor.save_then_expect_file(&path, "prefix foo foo foo")?;
        editor.keys("<C-y>")?;
        expect_scoped_matches(&mut editor, "foo", 0)?;
        editor.keys("<C-f><A-s><esc><C-z>")?;
        editor.wait_state("undo does not reactivate a disabled scope", secs(5), |record| {
            record.find.scope == "document" && record.find.match_count == 3
        })?;
        editor.keys("<C-g>1:8<enter><S-right><S-right><S-right><C-h><A-s><esc><C-y>")?;
        expect_scoped_matches(&mut editor, "foo", 1)?;
        editor.keys("<f3>")?;
        editor.expect_cursor_heads(&[(0, 7)])?;
        editor.save_then_expect_file(&path, "prefix foo  foo")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn captured_scope_clears_on_tab_switch_and_cannot_activate_without_selected_text() -> TestResult {
    support::run_x11_test("scope-tab-switch", |session| {
        let a = session.seed_file("a.txt", "foo foo")?;
        let b = session.seed_file("b.txt", "foo foo foo")?;
        let mut editor = session.open_files("scope-tab-switch", &[a, b])?;
        editor.keys("<C-home><C-d><C-h><A-s>")?;
        expect_scoped_matches(&mut editor, "foo", 1)?;
        editor.keys("<esc><C-tab>")?;
        editor.wait_state("tab switch clears captured scope", secs(5), |record| {
            record.active_tab_index == 1 && record.find.scope == "document" && record.find.match_count == 3
        })?;
        editor.keys("<C-f><A-s><end>!")?;
        editor.wait_state("empty selections cannot enable scope", secs(5), |record| {
            record.find.query == "foo!" && record.find.scope == "document"
        })?;
        editor.keys("<C-a>foo<esc><C-S-tab>")?;
        editor.wait_state("returning does not resurrect the old capture", secs(5), |record| {
            record.active_tab_index == 0 && record.find.scope == "document" && record.find.match_count == 2
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn captured_scope_includes_shared_empty_regex_boundaries_once_and_excludes_partial_matches() -> TestResult {
    support::run_x11_test("scope-regex-boundaries", |session| {
        let path = session.seed_file("scope.txt", "aaa")?;
        let mut editor = session.open_file("scope-regex-boundaries", &path)?;
        editor.keys("<C-home><S-right><C-d>")?;
        editor.expect_selection_ranges(&[(0, 1), (1, 2)])?;
        editor.keys("<C-h><A-s><C-a>aaa")?;
        expect_scoped_matches(&mut editor, "aaa", 0)?;
        editor.keys("<C-a>(?:)")?;
        editor.click_find_chip(FindChip::Regex)?;
        expect_scoped_matches(&mut editor, "(?:)", 3)?;
        editor.keys("<tab>|<C-A-enter>")?;
        editor.save_then_expect_file(&path, "|a|a|a")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn captured_scope_follows_an_alternate_redo_branch_from_a_different_fork_position() -> TestResult {
    support::run_x11_test("scope-redo-branch", |session| {
        let path = session.seed_file("scope.txt", "foo foo")?;
        let mut editor = session.open_file("scope-redo-branch", &path)?;
        write_clipboard_text(Selection::Clipboard, "old ")?;
        editor.keys("<C-home><C-v><C-z>")?;
        write_clipboard_text(Selection::Clipboard, "newer ")?;
        editor.keys("<C-v><C-g>1:11<enter><S-right><S-right><S-right>")?;
        editor.expect_selection_ranges(&[(10, 13)])?;
        editor.keys("<C-h><A-s><esc><C-A-y><C-y>")?;
        expect_scoped_matches(&mut editor, "foo", 1)?;
        editor.keys("<f3>")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.save_then_expect_file(&path, "old foo foo")?;
        editor.keys("<C-z><f3>")?;
        editor.expect_cursor_heads(&[(0, 10)])?;
        editor.save_then_expect_file(&path, "newer foo foo")?;
        editor.keys("<C-y><f3>")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.save_then_expect_file(&path, "old foo foo")?;
        Ok(())
    })
}

fn expect_scoped_matches(editor: &mut Editor<'_>, query: &str, count: usize) -> SupportResult<()> {
    editor.wait_state("captured Find results", secs(5), |record| {
        record.find.scope == "selection" && record.find.query == query && record.find.match_count == count
    })?;
    Ok(())
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn captured_scope_can_use_a_secondary_selection_when_the_primary_is_a_caret() -> TestResult {
    support::run_x11_test("scope-secondary-selection", |session| {
        let path = session.seed_file("scope.txt", "foo bar")?;
        let screenshot = session.root().join("captured-scope.ppm");
        let mut editor = session.open_file("scope-secondary-selection", &path)?;
        editor.keys("<C-home><C-d>")?;
        editor.alt_click_at_text(0, 6)?;
        editor.wait_state("secondary text selection and primary caret", secs(5), |record| {
            record.cursors.len() == 2
                && record.cursors[0].anchor_char == 0
                && record.cursors[0].head_char == 3
                && record.cursors[record.primary_cursor_index].anchor_char
                    == record.cursors[record.primary_cursor_index].head_char
        })?;
        editor.keys("<C-h>")?;
        editor.click_find_chip(FindChip::Scope)?;
        editor.keys("<C-a>foo")?;
        expect_scoped_matches(&mut editor, "foo", 1)?;
        editor.wait_quiet(std::time::Duration::from_millis(75), secs(5))?;
        editor.screenshot()?.write_ppm(screenshot)?;
        editor.keys("<tab>changed<C-A-enter>")?;
        editor.save_then_expect_file(&path, "changed bar")?;
        Ok(())
    })
}
