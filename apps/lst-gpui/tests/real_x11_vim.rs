//! Real-display tests for vim-mode behaviors. Drive the editor through
//! key sequences in vim notation and assert on the autosaved file.
//!
//! Run with
//!
//!     cargo nextest run --profile x11 -p lst-gpui --test real_x11_vim --run-ignored only

mod support;

use std::thread;

use support::{secs, EditorTestExt, TestResult};

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_top_line_delete_round_trips_to_autosaved_file() -> TestResult {
    // Type three lines in Insert, leave Insert, jump to top, delete first
    // line. Standard vim semantics: result is "B\nC\n".
    support::run_x11_test("vim-top-line-delete", |session| {
        let (mut editor, path) = session.open("scratch")?;

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
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("alpha<enter>beta<enter>gamma<esc>gg0Vjj><esc>")?;
        editor.save_then_expect_file(&path, "  alpha\n  beta\n  gamma")?;
        Ok(())
    })
}

#[test]
#[ignore = "requires a real X11 display plus xclip"]
fn vim_surround_inner_word_with_parentheses() -> TestResult {
    // ysiw)  → "you-surround inner-word with )". With "hello" as the only
    // word in the buffer, the result is "(hello)".
    support::run_x11_test("vim-surround-iw", |session| {
        let (mut editor, path) = session.open("scratch")?;

        editor.keys("hello<esc>0ysiw)")?;
        editor.save_then_expect_file(&path, "(hello)")?;
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
        let (mut editor, path) = session.open("scratch")?;

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
