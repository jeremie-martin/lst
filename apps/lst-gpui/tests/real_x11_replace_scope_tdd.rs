//! Under-review executable specs for "Replace All" honoring the
//! find-in-selection scope.
//!
//! Pinned chords (currently unbound in `keymap.rs` — both are TDD policy
//! choices; both are free):
//!
//! - `Alt+S` — toggle find-in-selection scope (`ToggleFindInSelection`).
//!   When toggled with an active text selection, the scope captures that
//!   selection range.
//! - `Ctrl+Alt+Enter` — invoke `ReplaceAll`. With selection scope, only
//!   matches inside the captured range are rewritten. With the default
//!   document scope, every match in the buffer is rewritten.
//!
//! `Ctrl+H` already opens the replace panel with focus on the find query
//! input. Typing populates the query; `Tab` advances focus to the replace
//! input.
//!
//! Specs run under the `x11-tdd` profile. Promote to
//! `real_x11_replace_scope.rs` once green.
//!
//!     cargo nextest run --profile x11-tdd -p lst-gpui --test real_x11_replace_scope_tdd --run-ignored only

mod support;

use support::{secs, EditorTestExt, TestResult};

const FIXTURE: &str = "x foo\ny foo foo\nz foo\n";

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn replace_all_in_selection_only_mutates_inside_selection() -> TestResult {
    support::run_x11_test("replace-scope-in-selection", |session| {
        let path = session.seed_file("replace-scope-in-selection.txt", FIXTURE)?;
        let mut editor = session.open_file("replace-scope-in-selection", &path)?;

        // Select all of line 1: "y foo foo".
        editor.place_cursor_at_document_start()?;
        editor.keys("<down><S-end>")?;
        editor.expect_cursor_heads(&[(1, 9)])?;

        // Open replace panel. Focus lands on the find query input.
        editor.keys("<C-h>")?;
        editor.wait_state("find query focus", secs(2), |record| {
            record.focused_input == "find_query" && record.find.show_replace
        })?;

        // Engage in-selection scope before typing the query so the captured
        // range is the selection we just made (not the empty post-typing
        // caret position).
        editor.keys("<A-s>")?;
        editor.wait_state("scope captured", secs(2), |record| {
            record.find.scope == "selection"
        })?;

        // Type query, advance to replace input, type replacement, fire.
        editor.keys("foo<tab>bar<C-A-enter>")?;
        editor.save_then_expect_file(&path, "x foo\ny bar bar\nz foo\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn replace_all_with_document_scope_mutates_every_match() -> TestResult {
    support::run_x11_test("replace-scope-document", |session| {
        let path = session.seed_file("replace-scope-document.txt", FIXTURE)?;
        let mut editor = session.open_file("replace-scope-document", &path)?;

        // No prior selection, no in-selection toggle. Default scope is
        // document — Replace All rewrites every "foo".
        editor.place_cursor_at_document_start()?;
        editor.keys("<C-h>")?;
        editor.wait_state("find query focus", secs(2), |record| {
            record.focused_input == "find_query"
                && record.find.show_replace
                && record.find.scope == "document"
        })?;

        editor.keys("foo<tab>bar<C-A-enter>")?;
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
        editor.wait_state("scope captured", secs(2), |record| {
            record.find.scope == "selection"
        })?;

        editor.keys("foo<tab>bar<C-A-enter>")?;
        // Inside-selection "foo" instances become "bar". Trailing two spaces
        // on the outside lines must survive — this catches an implementation
        // that accidentally normalizes trailing whitespace as part of the
        // replace pipeline.
        editor.save_then_expect_file(
            &path,
            "leading foo  \nselected bar and bar\ntrailing foo  \n",
        )?;
        Ok(())
    })
}
