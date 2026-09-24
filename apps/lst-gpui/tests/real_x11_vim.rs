//! Real-display tests for vim-mode behaviors. Drive the editor through
//! key sequences in vim notation and assert on the saved file or the
//! mode, pending-command, and cursor state in the trace.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_vim --run-ignored only

mod support;

use std::thread;

use lst_x11_harness::{ChordMods, Key};

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_top_line_delete_round_trips_to_autosaved_file() -> TestResult {
    // Type three lines in Insert, leave Insert, jump to top, delete first
    // line. Standard vim semantics: result is "B\nC\n".
    support::run_x11_test("vim-top-line-delete", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("A<enter>B<enter>C<enter><esc>ggdd")?;
        editor.save_then_expect_file(&path, "B\nC\n")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_visual_line_indent_indents_block_by_one_unit() -> TestResult {
    // Type three lines, escape to Normal, return to top of buffer, enter
    // Visual-line over all three lines, indent. Scratchpads are saved as
    // `.md`, so the active indent unit is two spaces.
    support::run_x11_test("vim-visual-indent", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("alpha<enter>beta<enter>gamma<esc>gg0Vjj><esc>")?;
        editor.save_then_expect_file(&path, "  alpha\n  beta\n  gamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_change_inner_word_replaces_text_object_and_enters_insert() -> TestResult {
    support::run_x11_test("vim-change-inner-word", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("hello world<esc>0ciwHEY<esc>")?;
        editor.save_then_expect_file(&path, "HEY world")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_normal_open_join_and_replace_commands_edit_observable_text() -> TestResult {
    support::run_x11_test("vim-open-join-replace", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("foo<enter>bar<esc>ggOtop<esc>jJ0rx")?;
        editor.save_then_expect_file(&path, "top\nxoo bar")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_linewise_yank_pastes_after_target_line() -> TestResult {
    support::run_x11_test("vim-linewise-paste", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("one<enter>two<enter>three<esc>ggyyGp")?;
        editor.save_then_expect_file(&path, "one\ntwo\nthree\none")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_visual_text_object_uppercases_inner_word() -> TestResult {
    support::run_x11_test("vim-visual-text-object-case", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("hello world<esc>0viwU")?;
        editor.save_then_expect_file(&path, "HELLO world")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_star_and_navigate_find_word_under_cursor() -> TestResult {
    support::run_x11_test("vim-star-search", |session| {
        let path = session.seed_file("vim-star-search.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_vim_file("vim-star-search", &path)?;

        editor.keys("<esc>0*")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.keys("n")?;
        editor.expect_cursor_heads(&[(0, 16)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_question_search_repeats_backward() -> TestResult {
    support::run_x11_test("vim-question-search", |session| {
        let path = session.seed_file("vim-question-search.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_vim_file("vim-question-search", &path)?;

        editor.keys("<esc>gg$")?;
        editor.expect_cursor_heads(&[(0, 18)])?;
        editor.keys("?")?;
        editor.wait_state("vim question find query focus", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("foo<enter>")?;
        editor.wait_state("vim question search submitted", secs(5), |record| {
            !record.find.visible
                && record.focused_input == "editor"
                && matches!(record.cursors.as_slice(), [cursor] if cursor.head_pos() == (0, 16))
        })?;
        editor.keys("n")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.keys("N")?;
        editor.expect_cursor_heads(&[(0, 16)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_slash_search_moves_to_the_next_match_after_the_cursor() -> TestResult {
    support::run_x11_test("vim-slash-search", |session| {
        let path = session.seed_file("vim-slash-search.txt", "foo bar foo baz foo")?;
        let mut editor = session.open_vim_file("vim-slash-search", &path)?;

        editor.keys("<esc>0")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        editor.keys("/")?;
        editor.wait_state("vim slash find query focus", secs(2), |record| {
            record.find.visible && record.focused_input == "find_query"
        })?;
        editor.keys("foo<enter>")?;
        editor.wait_state("vim slash search submitted", secs(5), |record| {
            !record.find.visible && record.focused_input == "editor"
        })?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        editor.keys("n")?;
        editor.expect_cursor_heads(&[(0, 16)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_slash_search_replaces_the_previous_query() -> TestResult {
    support::run_x11_test("vim-slash-replaces-query", |session| {
        let path = session.seed_file("vim-slash-empty.txt", "foo bar foo bar")?;
        let mut editor = session.open_vim_file("vim-slash-empty", &path)?;

        editor.keys("<esc>0*")?;
        editor.expect_cursor_heads(&[(0, 8)])?;
        // The prompt opens with "foo" from * selected, so typing replaces it
        // instead of searching for "foobar".
        editor.keys("/bar<enter>")?;
        editor.expect_cursor_heads(&[(0, 12)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_search_that_finds_nothing_or_is_cancelled_keeps_the_cursor() -> TestResult {
    support::run_x11_test("vim-search-miss-cancel", |session| {
        let path = session.seed_file("vim-search-miss.txt", "abc def zebra foo")?;
        let mut editor = session.open_vim_file("vim-search-miss", &path)?;

        editor.keys("<esc>0w")?;
        editor.expect_cursor_heads(&[(0, 4)])?;
        // While typing, "z" alone matches "zebra"; the full query matches
        // nothing.
        editor.keys("/zzz<enter>")?;
        editor.wait_state("vim missed search closes", secs(5), |record| {
            !record.find.visible && record.focused_input == "editor"
        })?;
        editor.expect_cursor_heads(&[(0, 4)])?;

        editor.keys("/foo<esc>")?;
        editor.wait_state("vim cancelled search closes", secs(5), |record| {
            !record.find.visible && record.focused_input == "editor"
        })?;
        editor.expect_cursor_heads(&[(0, 4)])?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_change_undo_is_single_step() -> TestResult {
    support::run_x11_test("vim-change-undo", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("alpha beta<esc>0cwX<esc>u")?;
        editor.save_then_expect_file(&path, "alpha beta")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_compound_commands_survive_long_pauses_between_keystrokes() -> TestResult {
    // **Harness invariant — do not remove this test.**
    //
    // Vim's compound commands (`gg`, `dd`, `yy`, `ciw`, …) have no
    // wall-clock timeout. A user can press the first half of a compound,
    // get distracted for ten seconds, then press the second half and the
    // editor still treats the two keystrokes as one command. That is
    // part of the implicit contract between a text editor and a
    // keyboard, and our harness must preserve it.
    //
    // `send_keys` is paint-synchronized, but it must never coalesce nor
    // deduplicate events, and arbitrary host-side delays *between*
    // `send_keys` calls must remain invisible to the editor: typing
    // slowly should produce the exact same observable outcome as typing
    // fast. If this test ever starts failing because the editor saw only
    // a single `g` or because the second half of `gg` was discarded,
    // either the harness has regressed (likely accidentally throttling
    // events again) or the editor has grown a real timeout it shouldn't
    // have. Investigate the regression — do not "fix" the test by
    // shortening the pauses.
    support::run_x11_test("vim-slow-paced", |session| {
        let (mut editor, path) = session.open_vim("scratch")?;

        editor.keys("first<enter>second<enter>third<esc>")?;
        thread::sleep(secs(1));
        editor.keys("g")?;
        thread::sleep(secs(1));
        editor.keys("g")?;
        thread::sleep(secs(1));
        editor.keys("dd")?;
        // No trailing newline — we typed "first<enter>second<enter>third" with
        // no final <enter>, so the buffer is "first\nsecond\nthird" and `dd`
        // on line 1 leaves the lines below intact, exactly as a real user
        // would observe.
        editor.save_then_expect_file(&path, "second\nthird")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_escape_visual_and_visual_line_keys_switch_modes() -> TestResult {
    support::run_x11_test("vim-mode-transitions", |session| {
        let (mut editor, _path) = session.open_vim("scratch")?;

        // Scratchpads start in Insert.
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
fn vim_pending_prefix_shows_until_escape_drops_it_or_the_command_completes() -> TestResult {
    support::run_x11_test("vim-pending", |session| {
        let (mut editor, _path) = session.open_vim("scratch")?;

        editor.keys("first<enter>second<esc>")?;
        editor.expect_vim_mode("NORMAL")?;
        editor.expect_cursor_heads(&[(1, 5)])?;

        editor.keys("g")?;
        editor.wait_state("pending g", secs(2), |record| record.vim_pending == "g")?;
        editor.keys("<esc>")?;
        editor.wait_state("escape drops pending g", secs(2), |record| {
            record.vim_pending.is_empty()
        })?;
        editor.expect_cursor_heads(&[(1, 5)])?;

        editor.keys("g")?;
        editor.wait_state("pending g again", secs(2), |record| record.vim_pending == "g")?;
        editor.keys("g")?;
        editor.wait_state("gg moves to the first line", secs(2), |record| {
            record.vim_pending.is_empty() && matches!(record.cursors.as_slice(), [cursor] if cursor.head_line == 0)
        })?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_normal_mode_ctrl_f_b_d_u_page_instead_of_running_editor_shortcuts() -> TestResult {
    support::run_x11_test("vim-ctrl-paging", |session| {
        let text = (0..80)
            .map(|line| format!("line {line:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let path = session.seed_file("vim-ctrl-paging.txt", &text)?;
        let mut editor = session.open_vim_file("vim-ctrl-paging", &path)?;

        editor.keys("<esc>gg0")?;
        editor.expect_cursor_heads(&[(0, 0)])?;
        for (keys, forward) in [("<C-f>", true), ("<C-b>", false), ("<C-d>", true), ("<C-u>", false)] {
            editor.keys(keys)?;
            editor.wait_state(keys, secs(3), |record| {
                let line = record.cursors.first().map_or(0, |cursor| cursor.head_line);
                record.vim_mode == "NORMAL"
                    && !record.find.visible
                    && record.cursors.len() == 1
                    && (line > 0) == forward
            })?;
        }
        Ok(())
    })
}

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
