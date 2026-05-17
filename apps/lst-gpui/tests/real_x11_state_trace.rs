//! Real-display tests for the state-trace channel itself, plus the new
//! state-only assertions that the channel unlocks. Each test drives the
//! editor through the harness and inspects state through
//! `Editor::read_state` / `drain_state_records` rather than the autosave
//! file. That isolates "what state did the editor end up in?" from "what
//! did typing produce?".
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_state_trace --run-ignored only

mod support;

use std::time::Duration;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn state_trace_exposes_final_state_after_text_input() -> TestResult {
    support::run_x11_test("state-trace-text-input-state", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        let baseline = editor.read_state()?;

        editor.keys("abc")?;
        let after = editor.wait_state(
            "text input reaches final trace state",
            Duration::from_secs(2),
            |record| matches!(record.cursors.as_slice(), [cursor] if cursor.head_char == 3),
        )?;
        assert!(
            after.seq > baseline.seq,
            "trace should advance after text input: {baseline:?} -> {after:?}"
        );
        assert!(
            after.revision > baseline.revision,
            "revision should advance after text input: {baseline:?} -> {after:?}"
        );
        assert_eq!(after.cursors[0].head_char, 3, "{after:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_mode_transitions_visible_in_trace() -> TestResult {
    support::run_x11_test("state-trace-vim-mode", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        // Scratchpads start in Insert. Type something so we have a buffer
        // to enter Visual on, then walk through modes.
        editor.keys("hello")?;
        editor.expect_vim_mode("INSERT")?;

        editor.keys("<esc>")?;
        editor.expect_vim_mode("NORMAL")?;

        editor.keys("v")?;
        editor.expect_vim_mode("VISUAL")?;

        editor.keys("<esc>")?;
        editor.expect_vim_mode("NORMAL")?;

        editor.keys("V")?;
        editor.expect_vim_mode("V-LINE")?;

        editor.keys("<esc>")?;
        editor.expect_vim_mode("NORMAL")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_pending_is_visible_mid_compound_command() -> TestResult {
    // The status bar shows the pending vim prefix while a compound
    // command is in progress. That makes `vim_pending` part of the
    // settled state for each keystroke, so we can observe `g` halfway
    // through `gg` without typing the second `g`.
    support::run_x11_test("state-trace-vim-pending", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.keys("first<enter>second<esc>")?;
        editor.expect_vim_mode("NORMAL")?;

        editor.keys("g")?;
        let record = editor.read_state()?;
        assert_eq!(
            record.vim_pending, "g",
            "expected pending 'g' after one half of `gg`; got {record:?}"
        );

        // Complete the compound; pending clears.
        editor.keys("g")?;
        let after = editor.read_state()?;
        assert_eq!(
            after.vim_pending, "",
            "pending should clear once the compound resolves; got {after:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn find_panel_state_round_trips_through_trace() -> TestResult {
    support::run_x11_test("state-trace-find", |session| {
        let path = session.seed_file("find.txt", "foo bar foo\nfoo baz")?;
        let mut editor = session.open_file("find", &path)?;

        editor.keys("<C-f>")?;
        editor.wait_state("find query focus", Duration::from_secs(2), |record| {
            record.focused_input == "find_query"
        })?;
        editor.keys("foo")?;
        editor.expect_find_state("foo", 3)?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn goto_line_panel_input_visible_before_submit() -> TestResult {
    support::run_x11_test("state-trace-goto-line", |session| {
        let path = session.seed_file("goto.txt", "alpha\nbeta\ngamma\ndelta\nepsilon")?;
        let mut editor = session.open_file("goto", &path)?;

        editor.keys("<C-g>3")?;
        let record = editor.read_state()?;
        assert_eq!(record.goto_line_input.as_deref(), Some("3"), "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn status_bar_reports_multi_cursor_summary() -> TestResult {
    support::run_x11_test("state-trace-status-bar", |session| {
        let path = session.seed_file("status.txt", "foo foo foo")?;
        let mut editor = session.open_file("status", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-S-l><esc>")?;
        let record = editor.read_state()?;
        assert!(
            record.status_bar.contains("3 cursors"),
            "status bar should mention 3 cursors; got {:?}",
            record.status_bar
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn save_leaves_buffer_clean_after_edit() -> TestResult {
    support::run_x11_test("state-trace-dirty", |session| {
        let path = session.seed_file("dirty.txt", "original\n")?;
        let mut editor = session.open_file("dirty", &path)?;

        editor.keys("a more")?;
        editor.save()?;
        editor.wait_state("save clears dirty flag", Duration::from_secs(2), |record| {
            !record.active_tab_modified
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_on_buffer_without_occurrence_leaves_cursor_state_unchanged() -> TestResult {
    support::run_x11_test("state-trace-ctrl-d-noop", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        // Establish a baseline: the scratchpad starts in Insert with one
        // collapsed cursor at the empty buffer's only position.
        editor.keys("a")?;
        editor.keys("<bs>")?;
        let baseline = editor.read_state()?;
        assert_eq!(baseline.cursors.len(), 1, "{baseline:?}");

        // Ctrl+D with no current word and no current selection should leave
        // the visible cursor set unchanged. A repaint is allowed; that is not
        // part of the user-facing no-op contract.
        editor.press(lst_x11_harness::KeyChord::Ctrl(lst_x11_harness::Key::Char('d')))?;
        editor.wait_quiet(Duration::from_millis(75), Duration::from_secs(2))?;

        // The visible cursor state remains the baseline.
        let after = editor.read_state()?;
        assert_eq!(
            after.cursors.len(),
            1,
            "Ctrl+D without an occurrence should not change cursor count: {after:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_s_on_unmodified_buffer_leaves_visible_state_unchanged() -> TestResult {
    support::run_x11_test("state-trace-ctrl-s-clean", |session| {
        let path = session.seed_file("clean.txt", "untouched\n")?;
        let mut editor = session.open_file("clean", &path)?;

        let baseline = editor.read_state()?;

        editor.press(lst_x11_harness::KeyChord::Ctrl(lst_x11_harness::Key::Char('s')))?;
        editor.wait_quiet(Duration::from_millis(75), Duration::from_secs(2))?;
        let after = editor.read_state()?;
        assert_eq!(
            after.active_tab_modified, baseline.active_tab_modified,
            "Ctrl+S on a clean buffer should not dirty the buffer: {after:?}"
        );
        assert_eq!(
            after.cursors[0].head_pos(),
            baseline.cursors[0].head_pos(),
            "Ctrl+S on a clean buffer should not move the cursor: {after:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_pending_clears_when_escape_drops_the_compound_prefix() -> TestResult {
    support::run_x11_test("state-trace-pending-escape", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        editor.keys("first<esc>")?;
        editor.expect_vim_mode("NORMAL")?;
        editor.keys("g")?;
        let pending = editor.read_state()?;
        assert_eq!(pending.vim_pending, "g", "{pending:?}");

        editor.keys("<esc>")?;
        let cleared = editor.read_state()?;
        assert_eq!(
            cleared.vim_pending, "",
            "Esc should drop the pending compound prefix: {cleared:?}"
        );
        Ok(())
    })
}
