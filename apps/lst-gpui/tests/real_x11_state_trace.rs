//! Real-display tests for the state-trace channel itself, plus the new
//! state-only assertions that the channel unlocks. Each test drives the
//! editor through the harness and inspects state through
//! `Editor::read_state` / `drain_state_records` rather than the autosave
//! file. That isolates "what state did the editor end up in?" from "what
//! did typing produce?".
//!
//! Run with
//!
//!     cargo test -p lst-gpui --tests -- --ignored --test-threads=1 --nocapture

mod support;

use std::time::Duration;

use support::{EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn state_trace_records_each_settled_keystroke_in_sequence() -> TestResult {
    support::run_x11_test("state-trace-per-keystroke", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        // Drain any startup records (focus click, initial paint) so the
        // assertion below sees only the records produced by our typing.
        editor.drain_state_records()?;

        editor.keys("abc")?;
        let new = editor.drain_state_records()?;
        assert!(
            new.len() >= 3,
            "expected at least 3 trace records for 3 keystrokes; got {}",
            new.len()
        );
        for window in new.windows(2) {
            assert_eq!(
                window[1].seq,
                window[0].seq + 1,
                "trace seq must be monotonically increasing: {:?} → {:?}",
                window[0].seq,
                window[1].seq
            );
        }
        let last = new.last().expect("at least one record");
        assert!(
            last.revision > new[0].revision,
            "revision should advance over typed characters: {} → {}",
            new[0].revision,
            last.revision
        );
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
        let (mut editor, _path) = session.open("scratch")?;

        editor.keys("foo bar foo<enter>foo baz<esc>")?;
        editor.keys("<C-f>foo")?;
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
        let path = session.seed_file("status.txt", "alpha\nbeta\ngamma")?;
        let mut editor = session.open_file("status", &path)?;

        editor.keys("<C-home><C-A-down><C-A-down>")?;
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
fn dirty_flag_clears_after_save() -> TestResult {
    support::run_x11_test("state-trace-dirty", |session| {
        let path = session.seed_file("dirty.txt", "original\n")?;
        let mut editor = session.open_file("dirty", &path)?;

        editor.keys("a more")?;
        let after_typing = editor.read_state()?;
        assert!(
            after_typing.active_tab_modified,
            "buffer should be modified after typing: {after_typing:?}"
        );

        editor.save()?;
        // The save path emits a model effect; wait briefly for the
        // post-save record to land before reading.
        editor.wait_quiet(
            std::time::Duration::from_millis(75),
            std::time::Duration::from_secs(2),
        )?;
        let after_save = editor.read_state()?;
        assert!(
            !after_save.active_tab_modified,
            "save should clear modified flag: {after_save:?}"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_d_on_buffer_without_occurrence_is_a_noop_in_state() -> TestResult {
    support::run_x11_test("state-trace-ctrl-d-noop", |session| {
        let (mut editor, _path) = session.open("scratch")?;

        // Establish a baseline: the scratchpad starts in Insert with one
        // collapsed cursor at the empty buffer's only position.
        editor.keys("a")?;
        editor.keys("<bs>")?;
        let baseline = editor.read_state()?;
        assert_eq!(baseline.cursors.len(), 1, "{baseline:?}");

        // Ctrl+D with no current word and no current selection should be
        // a no-op. Asserting no paint via `send_keys_expect_quiet` proves
        // the no-op contract directly; the state remains the baseline.
        editor.send_keys_expect_quiet("<C-d>", Duration::from_millis(250))?;

        // The latest record is still the baseline (no record emitted for
        // the no-op).
        let after = editor.read_state()?;
        assert_eq!(
            after.cursors.len(),
            1,
            "Ctrl+D without an occurrence should not change cursor count: {after:?}"
        );
        assert_eq!(
            after.seq, baseline.seq,
            "no-op should not emit a new trace record"
        );
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_s_on_unmodified_buffer_does_not_emit_a_repaint_record() -> TestResult {
    support::run_x11_test("state-trace-ctrl-s-clean", |session| {
        let path = session.seed_file("clean.txt", "untouched\n")?;
        let mut editor = session.open_file("clean", &path)?;

        // Drain startup records so we measure only the Ctrl+S behaviour.
        editor.drain_state_records()?;

        editor.send_keys_expect_quiet("<C-s>", Duration::from_millis(250))?;
        let after = editor.drain_state_records()?;
        assert!(
            after.is_empty(),
            "Ctrl+S on an unmodified buffer should not emit a state record; got {} records",
            after.len()
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
