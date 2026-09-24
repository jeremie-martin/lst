//! Vim behavior the Neovim oracle (`vim_oracle.rs`) cannot express: grapheme-aware
//! editing, lst-specific policy, viewport-dependent motions, and `g;`/`gi`. ASCII
//! Normal and Visual parity cases belong in `scripts/generate_vim_oracle_fixtures.py`.

mod support;

use lst_editor::{vim, EditorEffect, FocusTarget, RevealIntent};

use support::{run_cursor_cases, run_text_cases, VimHarness};

#[test]
fn modes_state_pending_and_escape_follow_vim_contracts() {
    let mut harness = VimHarness::new("");
    harness.keys("abc<esc>");
    harness.expect_text("abc");
    harness.expect_mode(vim::Mode::Normal);
    harness.expect_cursor(0, 2);

    let mut harness = VimHarness::new("");
    harness.keys("a\u{301}b<esc>");
    harness.expect_cursor(0, 2);
    harness.keys("h");
    harness.expect_cursor(0, 0);

    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    harness.keys("vww");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("alpha beta");
    harness.keys("<esc>");
    harness.expect_mode(vim::Mode::Normal);
    harness.expect_no_selection();

    let mut harness = VimHarness::with_two_tabs("alpha beta", "other");
    harness.keys("<esc>vww");
    harness.expect_mode(vim::Mode::Visual);
    let second = harness.model.tab_id_at(1).unwrap();
    harness.model.set_active_tab(second);
    harness.sync_effects();
    harness.expect_mode(vim::Mode::Normal);
    harness.expect_no_selection();

    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    for (keys, pending) in [("2", "2"), ("d", "2d"), ("3", "2d3"), ("<esc>", "")] {
        harness.keys(keys);
        harness.expect_pending(pending);
    }
    harness.keys("y");
    harness.expect_pending("y");
    harness.keys("s");
    harness.expect_pending("");

    let mut harness = VimHarness::normal_at("abcdef", 0, 0);
    harness.keys("df<right>");
    harness.expect_text("abcdef");
    harness.expect_cursor(0, 1);
    harness.expect_pending("");
}

#[test]
fn page_motions_move_by_half_and_full_viewports() {
    // Eight rows keep half-page and full-page distances apart.
    let text = (0..20).map(|line| line.to_string()).collect::<Vec<_>>().join("\n");
    let mut harness = VimHarness::normal_at(&text, 0, 0);
    harness.model.set_viewport_rows(8);
    harness.keys("<C-d>");
    harness.expect_cursor(4, 0);
    harness.keys("<C-u>");
    harness.expect_cursor(0, 0);
    harness.keys("<C-f>");
    harness.expect_cursor(6, 0);
    harness.keys("<C-b>");
    harness.expect_cursor(0, 0);
    harness.keys("<pagedown>");
    harness.expect_cursor(6, 0);
    harness.keys("<pageup>");
    harness.expect_cursor(0, 0);
}

#[test]
fn command_modified_named_keys_are_left_for_the_caller() {
    let mut harness = VimHarness::normal_at("abcdef\nline 2\nline 3\nline 4", 2, 3);

    harness.keys("d<cmd-left>");
    harness.expect_cursor(2, 3);
    harness.expect_pending("d");

    harness.keys("<cmd-pageup>");
    harness.expect_cursor(2, 3);
    harness.expect_pending("d");
}

#[test]
fn unicode_motions_move_by_grapheme() {
    let cases = [
        (
            "unicode word motion is grapheme aligned",
            "éclair cafe",
            (0, 0),
            "e",
            (0, 5),
        ),
        ("combining grapheme horizontal right", "a\u{301}bc", (0, 0), "l", (0, 2)),
        ("combining grapheme horizontal left", "a\u{301}bc", (0, 2), "h", (0, 0)),
    ];

    run_cursor_cases(&cases);
}

#[test]
fn unicode_grapheme_vim_edits_cover_operators_registers_paste_and_case() {
    let cases = [
        (
            "delete combining grapheme under cursor",
            "a\u{301}bc",
            (0, 0),
            "x",
            "bc",
        ),
        (
            "replace combining grapheme under cursor",
            "a\u{301}bc",
            (0, 0),
            "rX",
            "Xbc",
        ),
        (
            "substitute combining grapheme under cursor",
            "a\u{301}bc",
            (0, 0),
            "sX<esc>",
            "Xbc",
        ),
        (
            "paste deleted combining grapheme",
            "a\u{301}bc",
            (0, 0),
            "x$p",
            "bca\u{301}",
        ),
        ("delete emoji grapheme under cursor", "😀abc", (0, 0), "x", "abc"),
        ("unicode inner word delete", "éclair cafe", (0, 0), "diw", " cafe"),
        (
            "unicode inner word paste",
            "éclair cafe",
            (0, 0),
            "yiw$p",
            "éclair cafeéclair",
        ),
        (
            "unicode visual uppercase word",
            "éclair cafe",
            (0, 0),
            "viwU",
            "ÉCLAIR cafe",
        ),
    ];

    run_text_cases(&cases);

    let mut harness = VimHarness::normal_at("a\u{301}bc", 0, 0);
    harness.keys("x");
    harness.expect_char_register("a\u{301}");

    let mut harness = VimHarness::normal_at("éclair cafe", 0, 0);
    harness.keys("diw");
    harness.expect_char_register("éclair");
}

// Neovim makes `dw` and `yw` on an empty line linewise (register: line ""); lst
// takes a charwise "\n". The texts below agree, but the register differs and
// `ywp` leaves Neovim's cursor on line 1, lst's on line 0. Kept out of the
// oracle until that divergence is decided.
#[test]
fn word_operators_on_an_empty_line_take_its_newline() {
    run_text_cases(&[
        ("dw on an empty line deletes the newline", "\nabc", (0, 0), "dw", "abc"),
        (
            "yw on an empty line yanks the newline",
            "\nabc",
            (0, 0),
            "ywP",
            "\n\nabc",
        ),
    ]);
}

// Neovim inserts no separator after a whitespace-only first line ("  \nbar" J
// gives "  bar") and leaves `3J` over a blank line on the separator (column 1).
// lst gives "   bar" and column 2. Kept out of the oracle until that is decided.
#[test]
fn join_keeps_indentation_only_lines_and_adds_no_space_for_blank_lines() {
    run_text_cases(&[
        (
            "join keeps indentation-only first line",
            "  \nbar",
            (0, 0),
            "J",
            "   bar",
        ),
        ("join adds no space for blank lines", "a\n\nb", (0, 0), "3J", "a b"),
    ]);
}

#[test]
fn visual_selection_follows_viewport_motions() {
    let text = (0..12)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut harness = VimHarness::normal_at(&text, 0, 0);
    harness.model.set_viewport_rows(7);
    harness.model.set_viewport_top(4);
    harness.keys("vH");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("line 0\nline 1\nline 2\nline 3\nl");
    harness.keys("<esc>");
    harness.expect_mode(vim::Mode::Normal);

    let mut harness = VimHarness::normal_at(&text, 2, 0);
    harness.model.set_viewport_rows(4);
    harness.keys("v<C-f>");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("line 2\nline 3\nl");
}

#[test]
fn search_prompt_hands_focus_back_to_the_editor() {
    let mut harness = VimHarness::normal_at("alpha beta alpha", 0, 0);
    harness.keys("/alpha<enter>n");
    assert_eq!(harness.focus, FocusTarget::Editor);

    let mut harness = VimHarness::normal_at("foo bar foo", 0, 4);
    harness.keys("/foo<esc>");
    assert_eq!(harness.focus, FocusTarget::Editor);
    harness.expect_cursor(0, 4);
}

#[test]
fn hash_search_sets_a_whole_word_find_query() {
    let mut harness = VimHarness::normal_at("foo bar foo baz foo", 0, 16);
    harness.keys("#");
    assert_eq!(harness.model.find().query, "foo");
    assert!(harness.model.find().whole_word);
}

#[test]
fn undo_redo_and_last_edit_jump_track_vim_edits() {
    let mut harness = VimHarness::new("");
    harness.keys("abc<esc>u<cmd-r>");
    harness.expect_text("abc");

    let mut harness = VimHarness::normal_at("alpha\nbeta", 0, 0);
    harness.keys("A!<esc>ggg;");
    harness.expect_cursor(0, 6);
    harness.keys("gi?<esc>");
    harness.expect_text("alpha!?\nbeta");
}

#[test]
fn cmd_r_redoes_each_vim_edit_family_as_one_step() {
    // The oracle pins each edit and its undo against Neovim; lst binds redo to
    // <cmd-r> rather than <C-r>, so the redo half stays here.
    let cases = [
        ("change word", "alpha beta", "cwX<esc>"),
        ("change line", "alpha\nbeta", "ccX<esc>"),
        ("substitute char", "alpha beta", "sX<esc>"),
        ("visual delete", "alpha beta gamma", "vwd"),
        ("visual change", "alpha beta gamma", "viwcX<esc>"),
        ("visual paste", "one two three", "yiwwviwp"),
        ("paste", "alpha beta", "yiw$p"),
        ("join", "alpha\n beta", "J"),
        ("indent", "alpha\nbeta", ">>"),
        ("outdent", "  alpha\nbeta", "<<"),
    ];

    for (name, initial, keys) in cases {
        let mut harness = VimHarness::normal_at(initial, 0, 0);
        harness.keys(keys);
        let edited = harness.text();
        assert_ne!(edited, initial, "{name} edit");
        harness.keys("u");
        assert_eq!(harness.text(), initial, "{name} undo");
        harness.keys("<cmd-r>");
        assert_eq!(harness.text(), edited, "{name} redo");
    }
}

#[test]
fn unsupported_vim_commands_are_intentional_noops() {
    for keys in ["q", "@", "\"", ":", "R", "m", "'", "`", "g~", "gu", "gU"] {
        let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
        harness.keys(keys);
        harness.expect_text("alpha beta");
        harness.expect_mode(vim::Mode::Normal);
        harness.expect_pending("");
    }

    // Dot-repeat is unsupported, so it must not replay the previous change.
    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    harness.keys("x.");
    harness.expect_text("lpha beta");
    harness.expect_pending("");
}

#[test]
fn viewport_commands_emit_reveal_effects_and_move_to_visible_rows() {
    let text = (0..24)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut harness = VimHarness::normal_at(&text, 0, 0);
    harness.model.set_viewport_rows(11);
    harness.model.set_viewport_top(10);
    harness.clear_effects();

    harness.keys("H");
    harness.expect_cursor(10, 0);
    harness.keys("M");
    harness.expect_cursor(15, 0);
    harness.keys("L");
    harness.expect_cursor(20, 0);

    harness.clear_effects();
    harness.keys("zzztzb");
    let effects = harness.take_effects();
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::Center)));
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::Top)));
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::Bottom)));
}

#[test]
fn viewport_page_motions_preserve_visual_state() {
    let text = (0..16)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut harness = VimHarness::normal_at(&text, 3, 0);
    harness.model.set_viewport_rows(6);
    harness.keys("v<C-d>");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("line 3\nline 4\nline 5\nl");
    harness.keys("<C-u>");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("l");
    harness.keys("<esc>");

    harness.clear_effects();
    harness.keys("<pagedown><pageup>");
    let effects = harness.take_effects();
    assert!(effects.contains(&EditorEffect::Reveal(RevealIntent::NearestEdge)));
}
