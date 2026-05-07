//! Real-display regression tests for keyboard bugs that cut across one-off
//! feature suites. Add focused cases here when a review or production bug
//! needs true X11 delivery coverage before the fix.
//!
//! Run with
//!
//!     cargo nextest run --profile x11-regression -p lst-gpui --run-ignored only

mod support;

use lst_x11_harness::{ChordMods, Key, KeyChord};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_ctrl_d_in_normal_mode_keeps_vim_half_page_motion() -> TestResult {
    support::run_x11_test("regression-vim-ctrl-d", |session| {
        let text = (0..80)
            .map(|line| format!("foo line {line:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = session.seed_file("vim-ctrl-d.txt", &text)?;
        let mut editor = session.open_file("vim-ctrl-d", &path)?;

        editor.click_at_text(0, 0)?;
        editor.send_keys_settle("<esc>")?;
        editor.expect_vim_mode("NORMAL")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        let record = editor.wait_state("vim Ctrl-D moved down", secs(5), |record| {
            record.vim_mode == "NORMAL"
                && matches!(record.cursors.as_slice(), [cursor] if cursor.is_collapsed() && cursor.head_line > 0)
        })?;
        assert_eq!(record.vim_mode, "NORMAL", "{record:?}");
        assert_eq!(record.cursors.len(), 1, "{record:?}");
        assert!(record.cursors[0].is_collapsed(), "{record:?}");
        assert!(record.cursors[0].head_line > 0, "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn platform_shift_tab_does_not_run_shift_only_outdent() -> TestResult {
    support::run_x11_test("regression-platform-shift-tab", |session| {
        let path = session.seed_file("platform-shift-tab.txt", "    alpha")?;
        let mut editor = session.open_file("platform-shift-tab", &path)?;

        editor.send_keys_settle("<cmd-S-tab>")?;
        editor.save_then_expect_file(&path, "    alpha")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ctrl_k_prefix_is_cleared_by_unrelated_selection_shortcut() -> TestResult {
    support::run_x11_test("regression-ctrl-k-stale-prefix", |session| {
        let path = session.seed_file("ctrl-k-stale-prefix.txt", "foo foo foo")?;
        let mut editor = session.open_file("ctrl-k-stale-prefix", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<C-k><S-right><C-d>")?;
        let record = editor.read_state()?;
        let ranges = record
            .cursors
            .iter()
            .map(|cursor| {
                (
                    cursor.anchor_char.min(cursor.head_char),
                    cursor.anchor_char.max(cursor.head_char),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(ranges, vec![(0, 1), (4, 5)], "{record:?}");
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn consumed_ctrl_action_does_not_leave_recent_ctrl_for_next_text_key() -> TestResult {
    support::run_x11_test("regression-consumed-action-stale-ctrl", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("foo foo")?;
        editor.press(KeyChord::Ctrl(Key::Char('s')))?;
        editor.keys("d")?;
        editor.save_then_expect_file(&path, "foo food")?;
        Ok(())
    })
}
