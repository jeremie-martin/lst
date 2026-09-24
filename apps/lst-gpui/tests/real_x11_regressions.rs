//! Real-display regression tests for keyboard delivery bugs: stale modifier
//! state, chord prefixes, and layout-dependent keys.

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
        let mut editor = session.open_vim_file("vim-ctrl-d", &path)?;

        editor.click_at_text(0, 0)?;
        editor.send_keys_settle("<esc>")?;
        editor.expect_vim_mode("NORMAL")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        editor.wait_state("vim Ctrl-D moved down", secs(5), |record| {
            record.vim_mode == "NORMAL"
                && matches!(record.cursors.as_slice(), [cursor] if cursor.is_collapsed() && cursor.head_line > 0)
        })?;
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
        // The file already holds the indented line; the marker proves the
        // buffer still does.
        editor.keys("<end>X")?;
        editor.save_then_expect_file(&path, "    alphaX")?;
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
        editor.wait_state("Ctrl+D adds the next match of the selection", secs(5), |record| {
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
            ranges == [(0, 1), (4, 5)]
        })?;
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

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn released_ctrl_then_plain_d_in_insert_mode_inserts_d() -> TestResult {
    support::run_x11_test("regression-released-ctrl-plain-d", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("abc")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        editor.save_then_expect_file(&path, "abcd")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn ignored_insert_mode_recent_ctrl_does_not_poison_next_vim_key() -> TestResult {
    support::run_x11_test("regression-ignored-insert-ctrl-clears", |session| {
        let (mut editor, _path) = session.open_vim("scratch")?;

        editor.keys("abc")?;
        editor.key_after_released_modifiers(ChordMods::CTRL, Key::Char('d'))?;
        editor.keys("<esc>d")?;
        editor.wait_state("plain vim d pending", secs(2), |record| {
            record.vim_mode == "NORMAL" && record.vim_pending == "d"
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn crlf_line_ending_is_not_split_by_right_motion_and_insert() -> TestResult {
    support::run_x11_test("regression-crlf-motion-insert", |session| {
        let path = session.seed_file("crlf.txt", "a\r\nb")?;
        let mut editor = session.open_file("regression-crlf-motion-insert", &path)?;

        editor.place_cursor_at_document_start()?;
        editor.keys("<end><right>X")?;
        editor.save_then_expect_file(&path, "a\r\nXb")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn qwertz_layout_types_literal_z_and_y() -> TestResult {
    struct EnvGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    let _layout = EnvGuard::set("LST_X11_HARNESS_LAYOUT", "de");
    support::run_x11_test("regression-qwertz-literals", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("zy")?;
        editor.save_then_expect_file(&path, "zy")?;
        Ok(())
    })
}
