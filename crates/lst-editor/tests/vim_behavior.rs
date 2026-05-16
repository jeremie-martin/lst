mod support;

use lst_editor::{vim, EditorEffect, FocusTarget, RevealIntent};

use support::{run_cursor_cases, run_text_cases, run_text_cases_expect_normal, VimHarness};

#[test]
fn x11_vim_smoke_specs_run_through_the_editor_model() {
    let cases = [
        ("top line delete", "A<enter>B<enter>C<enter><esc>ggdd", "B\nC\n"),
        (
            "visual line indent",
            "alpha<enter>beta<enter>gamma<esc>gg0Vjj><esc>",
            "  alpha\n  beta\n  gamma",
        ),
        ("change inner word", "hello world<esc>0ciwHEY<esc>", "HEY world"),
        (
            "normal open join replace",
            "foo<enter>bar<esc>ggOtop<esc>jJ0rx",
            "top\nxoo bar",
        ),
        (
            "linewise paste",
            "one<enter>two<enter>three<esc>ggyyGp",
            "one\ntwo\nthree\none",
        ),
        ("visual text object case", "hello world<esc>0viwU", "HELLO world"),
    ];

    for (name, keys, expected) in cases {
        let mut harness = VimHarness::new("");
        harness.keys(keys);
        assert_eq!(harness.text(), expected, "{name}");
    }
}

#[test]
fn normal_motions_cover_words_lines_char_search_and_brackets() {
    let cases = [
        ("line start", "abc def", (0, 4), "0", (0, 0)),
        ("first nonblank", "  abc", (0, 4), "^", (0, 2)),
        ("line end", "abc", (0, 0), "$", (0, 2)),
        ("word forward", "  alpha beta", (0, 0), "w", (0, 2)),
        ("word end", "alpha beta", (0, 0), "e", (0, 4)),
        ("word backward", "alpha beta", (0, 8), "b", (0, 6)),
        ("big word end", "a+b c", (0, 0), "E", (0, 2)),
        ("document start", "  a\n b\nc", (2, 0), "gg", (0, 0)),
        ("counted gg", "a\n  b\nc", (0, 0), "2gg", (1, 0)),
        ("document end", "a\n  b\n c", (0, 0), "G", (2, 0)),
        ("counted G", "a\n  b\n c", (0, 0), "2G", (1, 0)),
        ("matching bracket", "call(foo)", (0, 4), "%", (0, 8)),
        ("line percentage", "a\nb\nc\nd", (0, 0), "50%", (1, 0)),
        ("find char", "abc abc", (0, 0), "fc", (0, 2)),
        ("till char", "abc abc", (0, 0), "tc", (0, 1)),
        ("counted find char", "abc abc", (0, 0), "2fb", (0, 5)),
        ("find char backward", "abc abc", (0, 6), "Fb", (0, 5)),
        ("till char backward", "abc abc", (0, 6), "T ", (0, 4)),
        ("repeat and reverse char search", "abcabc", (0, 0), "fc;,", (0, 2)),
    ];

    run_cursor_cases(&cases);
}

#[test]
fn vertical_motions_preserve_preferred_column() {
    let mut harness = VimHarness::normal_at("abcdef\nab\nabcdef", 0, 5);

    harness.keys("j");
    harness.expect_cursor(1, 1);
    harness.keys("j");
    harness.expect_cursor(2, 5);
    harness.keys("k");
    harness.expect_cursor(1, 1);
    harness.keys("0j");
    harness.expect_cursor(2, 0);
}

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
fn named_and_page_motions_cover_keyboard_boundary_paths() {
    let mut harness = VimHarness::normal_at("abcdef\nab\nabcdef\nlast", 0, 3);
    harness.keys("<left>");
    harness.expect_cursor(0, 2);
    harness.keys("<right>");
    harness.expect_cursor(0, 3);
    harness.keys("<home>");
    harness.expect_cursor(0, 0);
    harness.keys("<end>");
    harness.expect_cursor(0, 5);
    harness.keys("<down>");
    harness.expect_cursor(1, 1);
    harness.keys("<up>");
    harness.expect_cursor(0, 5);

    let mut harness = VimHarness::normal_at("a\nb\nc\nd\ne\nf\ng\nh", 0, 0);
    harness.model.set_viewport_rows(4);
    harness.keys("<C-d>");
    harness.expect_cursor(2, 0);
    harness.keys("<C-u>");
    harness.expect_cursor(0, 0);
    harness.keys("<C-f>");
    harness.expect_cursor(2, 0);
    harness.keys("<C-b>");
    harness.expect_cursor(0, 0);
    harness.keys("<pagedown>");
    harness.expect_cursor(2, 0);
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
fn word_and_big_word_motions_cover_counts_punctuation_empty_lines_and_unicode() {
    let cases = [
        ("counted word forward", "aa bb cc", (0, 0), "2w", (0, 6)),
        ("counted word end", "aa bb cc", (0, 0), "2e", (0, 4)),
        ("counted word backward", "aa bb cc", (0, 6), "2b", (0, 0)),
        ("word crosses empty line", "aa\n\nbb", (0, 0), "w", (1, 0)),
        ("word end crosses empty line", "aa\n\nbb", (0, 0), "2e", (2, 1)),
        (
            "big word forward treats punctuation as word",
            "aa+bb cc",
            (0, 0),
            "W",
            (0, 6),
        ),
        (
            "small word forward stops after punctuation",
            "aa+bb cc",
            (0, 0),
            "w",
            (0, 2),
        ),
        (
            "big word backward treats punctuation as word",
            "aa+bb cc",
            (0, 6),
            "B",
            (0, 0),
        ),
        (
            "small word backward stops at identifier run",
            "aa+bb cc",
            (0, 6),
            "b",
            (0, 3),
        ),
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

#[test]
fn operators_cover_motion_ranges_text_objects_counts_and_lines() {
    let cases = [
        ("delete word", "alpha beta", (0, 0), "dw", "beta"),
        ("delete to word end", "alpha beta", (0, 0), "de", " beta"),
        ("delete backward word", "alpha beta", (0, 6), "db", "beta"),
        ("delete to end", "abc def\nnext", (0, 4), "D", "abc \nnext"),
        (
            "delete through multiplied counts",
            "one two three four five",
            (0, 0),
            "2d2w",
            "five",
        ),
        ("delete lines to end", "a\nb\nc\nd", (1, 0), "dG", "a"),
        ("line change", "alpha\nbeta", (0, 0), "ccX<esc>", "X\nbeta"),
        (
            "change word uses word-end semantics",
            "alpha beta",
            (0, 0),
            "cwX<esc>",
            "X beta",
        ),
        (
            "change big word uses big-word-end semantics",
            "alpha+beta gamma",
            (0, 0),
            "cWX<esc>",
            "X gamma",
        ),
        (
            "change quote inner object",
            "prefix \"alpha beta\" tail",
            (0, 9),
            "ci\"X<esc>",
            "prefix \"X\" tail",
        ),
        (
            "change quote a-object",
            "prefix \"alpha beta\" tail",
            (0, 9),
            "ca\"X<esc>",
            "prefix Xtail",
        ),
        (
            "change paren inner object",
            "call(alpha, beta)",
            (0, 7),
            "ci(X<esc>",
            "call(X)",
        ),
        (
            "change bracket a-object",
            "items[one, two] tail",
            (0, 8),
            "ca[X<esc>",
            "itemsX tail",
        ),
    ];

    run_text_cases_expect_normal(&cases);
}

#[test]
fn operators_cover_linewise_inclusive_exclusive_and_register_edges() {
    let cases = [
        ("delete down is linewise", "a\nb\nc", (0, 0), "dj", "c"),
        ("delete up is linewise", "a\nb\nc", (2, 0), "dk", "a"),
        ("delete to document start is linewise", "a\nb\nc", (2, 0), "dgg", ""),
        ("delete counted gg is linewise", "a\nb\nc\nd", (3, 0), "d2gg", "a"),
        ("delete counted line end is linewise", "a\nb\nc", (0, 0), "d2$", "c"),
        (
            "delete percentage count is linewise",
            "a\nb\nc\nd",
            (0, 0),
            "d50%",
            "c\nd",
        ),
        (
            "change backward excludes cursor char",
            "alpha beta",
            (0, 6),
            "cbX<esc>",
            "Xbeta",
        ),
        (
            "change to document start is linewise",
            "a\nb\nc",
            (2, 0),
            "cggX<esc>",
            "X",
        ),
        (
            "yank to document start pastes linewise",
            "one\ntwo\nthree",
            (2, 0),
            "yggGp",
            "one\ntwo\nthree\none\ntwo\nthree",
        ),
        ("delete char find is inclusive", "abc def", (0, 0), "dfc", " def"),
        (
            "delete till find is inclusive to previous char",
            "abc def",
            (0, 0),
            "td",
            "abc def",
        ),
        ("delete failed find is noop", "abc def", (0, 0), "dz", "abc def"),
        (
            "yank char range pastes charwise",
            "alpha beta",
            (0, 0),
            "yw$p",
            "alpha betaalpha ",
        ),
        (
            "yank line count pastes linewise",
            "one\ntwo\nthree",
            (0, 0),
            "2yyGp",
            "one\ntwo\nthree\none\ntwo",
        ),
        (
            "empty line delete records line register",
            "one\n\ntwo",
            (1, 0),
            "ddP",
            "one\n\ntwo",
        ),
        (
            "delete from end to start restores as one undo step",
            "one\ntwo\nthree",
            (0, 0),
            "Gdggu",
            "one\ntwo\nthree",
        ),
    ];

    run_text_cases(&cases);
}

#[test]
fn normal_edits_cover_insert_positions_substitute_join_replace_paste_and_indent() {
    let cases = [
        ("append after cursor", "abc", (0, 0), "aX<esc>", "aXbc"),
        ("insert at first nonblank", "  abc", (0, 4), "IX<esc>", "  Xabc"),
        ("append at line end", "abc", (0, 0), "AX<esc>", "abcX"),
        ("open below", "alpha\nbeta", (0, 0), "oX<esc>", "alpha\nX\nbeta"),
        ("open above", "alpha\nbeta", (1, 0), "OX<esc>", "alpha\nX\nbeta"),
        ("delete under cursor", "abc", (0, 0), "x", "bc"),
        ("delete count under cursor", "abcd", (0, 0), "2x", "cd"),
        ("delete before cursor", "abcd", (0, 2), "X", "acd"),
        ("substitute one char", "abc", (0, 0), "sZ<esc>", "Zbc"),
        ("substitute counted chars", "abcd", (0, 0), "2sZ<esc>", "Zcd"),
        ("change to end", "abc def", (0, 4), "CXYZ<esc>", "abc XYZ"),
        (
            "change line preserves indentation",
            "  abc\nnext",
            (0, 2),
            "Snew<esc>",
            "  new\nnext",
        ),
        ("join one following line", "alpha\n beta", (0, 0), "J", "alpha beta"),
        ("join counted lines", "a\n b\n c\nd", (0, 0), "3J", "a b c\nd"),
        ("replace char", "abc", (0, 0), "rx", "xbc"),
        ("replace counted chars", "abcd", (0, 0), "3rx", "xxxd"),
        ("indent current line", "alpha", (0, 0), ">>", "  alpha"),
        ("outdent current line", "  alpha", (0, 0), "<<", "alpha"),
        (
            "line yank paste after target",
            "one\ntwo\nthree",
            (0, 0),
            "yyGp",
            "one\ntwo\nthree\none",
        ),
        (
            "line yank paste before target",
            "one\ntwo\nthree",
            (2, 0),
            "yyggP",
            "three\none\ntwo\nthree",
        ),
    ];

    run_text_cases_expect_normal(&cases);

    let mut harness = VimHarness::normal_at("a\n b\n", 0, 0);
    harness.keys("3J");
    harness.expect_text("a b");
    harness.expect_cursor(0, 2);
}

#[test]
fn normal_edits_cover_counts_boundaries_empty_lines_and_noops() {
    let cases = [
        ("append at empty line", "", (0, 0), "aX<esc>", "X"),
        ("insert on empty line", "", (0, 0), "iX<esc>", "X"),
        (
            "open below inherits indentation",
            "  alpha",
            (0, 2),
            "oX<esc>",
            "  alpha\n  X",
        ),
        (
            "open above inherits indentation",
            "  alpha",
            (0, 2),
            "OX<esc>",
            "  X\n  alpha",
        ),
        ("delete beyond eol clamps", "abc", (0, 1), "9x", "a"),
        ("delete before bol is noop", "abc", (0, 0), "X", "abc"),
        ("substitute on empty line inserts", "", (0, 0), "sX<esc>", "X"),
        ("delete to end at eol deletes current char", "abc", (0, 2), "D", "ab"),
        (
            "change to end at eol changes current char",
            "abc",
            (0, 2),
            "CX<esc>",
            "abX",
        ),
        ("join at last line is noop", "abc", (0, 0), "J", "abc"),
        ("replace on empty line is noop", "", (0, 0), "rx", ""),
        ("counted indent lines", "a\nb\nc", (0, 0), "2>>", "  a\n  b\nc"),
        ("counted outdent lines", "  a\n  b\nc", (0, 0), "2<<", "a\nb\nc"),
    ];

    run_text_cases(&cases);
}

#[test]
fn visual_mode_covers_charwise_linewise_text_objects_case_and_indentation() {
    let cases = [
        ("delete inner word", "alpha beta", (0, 0), "viwd", " beta"),
        ("change inner word", "alpha beta", (0, 0), "viwcX<esc>", "X beta"),
        ("lowercase selection", "ALPHA beta", (0, 0), "viwu", "alpha beta"),
        ("uppercase selection", "alpha beta", (0, 0), "viwU", "ALPHA beta"),
        ("visual line delete", "alpha\nbeta\ngamma", (0, 0), "Vjd", "gamma"),
        (
            "visual line change",
            "alpha\nbeta\ngamma",
            (0, 0),
            "VjcX<esc>",
            "X\ngamma",
        ),
        (
            "visual line indent",
            "alpha\nbeta\ngamma",
            (0, 0),
            "Vj>",
            "  alpha\n  beta\ngamma",
        ),
        (
            "visual line outdent",
            "  alpha\n  beta\ngamma",
            (0, 0),
            "Vj<",
            "alpha\nbeta\ngamma",
        ),
        (
            "visual text object selects quotes",
            "a \"two words\" z",
            (0, 4),
            "vi\"U",
            "a \"TWO WORDS\" z",
        ),
    ];

    run_text_cases_expect_normal(&cases);
}

#[test]
fn visual_mode_covers_counts_reverse_selection_search_repeat_and_viewport() {
    let mut harness = VimHarness::normal_at("alpha beta gamma", 0, 0);
    harness.keys("v2w");
    harness.expect_selection("alpha beta g");
    harness.keys("y$p");
    harness.expect_text("alpha beta gammaalpha beta g");

    let mut harness = VimHarness::normal_at("alpha beta gamma", 0, 11);
    harness.keys("vbU");
    harness.expect_text("alpha BETA Gamma");

    let mut harness = VimHarness::normal_at("abc abc abc", 0, 0);
    harness.keys("vfc;");
    harness.expect_selection("abc abc");
    harness.keys(",");
    harness.expect_selection("abc");

    let mut harness = VimHarness::normal_at("alpha beta alpha beta", 0, 0);
    harness.keys("v/beta<enter>n");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("alpha beta alpha b");
    harness.keys("N");
    harness.expect_selection("alpha b");

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
fn visual_mode_tracks_anchor_head_and_cursor_shape() {
    let mut harness = VimHarness::normal_at("alpha beta gamma", 0, 0);
    harness.keys("v2w");
    harness.expect_selection("alpha beta g");
    harness.expect_visual_state((0, 0), (0, 11));

    let mut harness = VimHarness::normal_at("alpha beta gamma", 0, 11);
    harness.keys("vb");
    harness.expect_selection("beta g");
    harness.expect_visual_state((0, 11), (0, 6));

    let mut harness = VimHarness::normal_at("alpha\nbeta\ngamma", 0, 0);
    harness.keys("Vj");
    harness.expect_mode(vim::Mode::VisualLine);
    harness.expect_selection("alpha\nbeta");
    harness.expect_visual_state((0, 0), (1, 0));

    harness.keys("v");
    harness.expect_mode(vim::Mode::Visual);
    harness.expect_selection("alpha\nb");
    harness.expect_visual_state((0, 0), (1, 0));
}

#[test]
fn search_commands_cover_word_search_find_panel_and_visual_stepping() {
    let mut harness = VimHarness::normal_at("foo bar foo baz foo", 0, 0);
    harness.keys("*");
    harness.expect_cursor(0, 8);
    harness.keys("n");
    harness.expect_cursor(0, 16);
    harness.keys("N");
    harness.expect_cursor(0, 8);
    harness.keys("#");
    harness.expect_cursor(0, 0);

    let mut harness = VimHarness::normal_at("alpha beta alpha", 0, 0);
    harness.keys("/alpha<enter>n");
    harness.expect_cursor(0, 0);
    assert_eq!(harness.focus, FocusTarget::Editor);

    let mut harness = VimHarness::normal_at("alpha beta alpha", 0, 0);
    harness.keys("v/beta<enter>");
    harness.expect_mode(vim::Mode::Visual);
    assert_eq!(harness.model.active_tab().selected_text().as_deref(), Some("alpha b"));
}

#[test]
fn search_commands_cover_wrap_empty_words_and_find_query_editing() {
    let mut harness = VimHarness::normal_at("foo bar foo", 0, 8);
    harness.keys("n");
    harness.expect_cursor(0, 8);
    harness.keys("/foo<enter>N");
    harness.keys("N");
    harness.expect_cursor(0, 0);

    let mut harness = VimHarness::normal_at("foo bar foo", 0, 4);
    harness.keys("*");
    harness.expect_cursor(0, 4);
    harness.keys("#");
    harness.expect_cursor(0, 4);

    let mut harness = VimHarness::normal_at("foo bar baz", 0, 0);
    harness.keys("/baq<bs>r<enter>");
    assert_eq!(harness.model.find().query, "bar");
    harness.expect_cursor(0, 4);

    let mut harness = VimHarness::normal_at("foo bar foo baz foo", 0, 16);
    harness.keys("?foo<enter>");
    assert_eq!(harness.model.find().query, "foo");
    harness.expect_cursor(0, 8);
    harness.keys("n");
    harness.expect_cursor(0, 0);
    harness.keys("N");
    harness.expect_cursor(0, 8);

    let mut harness = VimHarness::normal_at("foo bar foo baz foo", 0, 16);
    harness.keys("#");
    assert_eq!(harness.model.find().query, "foo");
    assert!(harness.model.find().whole_word);
    harness.expect_cursor(0, 8);
    harness.keys("n");
    harness.expect_cursor(0, 0);

    let mut harness = VimHarness::normal_at("foo bar foo", 0, 4);
    harness.keys("/foo<esc>");
    assert_eq!(harness.focus, FocusTarget::Editor);
    harness.expect_cursor(0, 4);
}

#[test]
fn text_objects_cover_words_paragraphs_pairs_quotes_counts_and_escapes() {
    let cases = [
        (
            "change a word consumes following space",
            "alpha beta",
            (0, 0),
            "cawX<esc>",
            "Xbeta",
        ),
        (
            "change inner big word",
            "alpha+beta gamma",
            (0, 3),
            "ciWX<esc>",
            "X gamma",
        ),
        ("change a big word", "alpha+beta gamma", (0, 3), "caWX<esc>", "Xgamma"),
        (
            "word object count extends through following words",
            "one two three four",
            (0, 0),
            "d2aw",
            "three four",
        ),
        (
            "inner paragraph is linewise",
            "one\ntwo\n\nthree",
            (0, 0),
            "dip",
            "\nthree",
        ),
        (
            "a paragraph includes following blank line",
            "one\ntwo\n\nthree",
            (0, 0),
            "dap",
            "three",
        ),
        (
            "paren inner via close delimiter",
            "call(alpha)",
            (0, 10),
            "ci)X<esc>",
            "call(X)",
        ),
        ("paren a-object alias b", "call(alpha)", (0, 6), "dab", "call"),
        ("brace inner via B alias", "fn { alpha }", (0, 5), "ciBX<esc>", "fn {X}"),
        ("brace a-object", "fn { alpha } tail", (0, 5), "da}", "fn  tail"),
        (
            "bracket inner via close delimiter",
            "items[alpha]",
            (0, 8),
            "ci]X<esc>",
            "items[X]",
        ),
        (
            "angle inner via close delimiter",
            "tag<alpha>",
            (0, 5),
            "ci>X<esc>",
            "tag<X>",
        ),
        (
            "single quote object",
            "let 'alpha' tail",
            (0, 6),
            "ci'X<esc>",
            "let 'X' tail",
        ),
        ("backtick object", "let `alpha` tail", (0, 6), "ca`X<esc>", "let Xtail"),
        (
            "escaped quote stays inside quote object",
            "let \"a\\\"b\" tail",
            (0, 7),
            "ci\"X<esc>",
            "let \"X\" tail",
        ),
    ];

    run_text_cases(&cases);
}

#[test]
fn registers_preserve_charwise_and_linewise_paste_placement() {
    let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
    harness.keys("yiw$p");
    harness.expect_text("alpha betaalpha");

    let mut harness = VimHarness::normal_at("alpha beta", 0, 6);
    harness.keys("diwP");
    harness.expect_text("alphabeta ");

    let mut harness = VimHarness::normal_at("one\ntwo\nthree", 1, 0);
    harness.keys("ddP");
    harness.expect_text("one\ntwo\nthree");
}

#[test]
fn paste_placement_covers_charwise_linewise_before_after_and_empty_registers() {
    let cases = [
        ("empty paste after is noop", "alpha", (0, 0), "p", "alpha"),
        ("empty paste before is noop", "alpha", (0, 0), "P", "alpha"),
        (
            "char delete paste after cursor",
            "alpha beta",
            (0, 0),
            "dw$p",
            "betaalpha ",
        ),
        (
            "char delete paste before cursor",
            "alpha beta",
            (0, 0),
            "dwP",
            "alpha beta",
        ),
        ("line delete paste after last", "one\ntwo", (0, 0), "ddGp", "two\none"),
        (
            "line delete paste before first",
            "one\ntwo",
            (1, 0),
            "ddggP",
            "two\none",
        ),
    ];

    run_text_cases(&cases);
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
fn undo_redo_groups_vim_edit_families_as_single_steps() {
    let cases = [
        ("change word", "alpha beta", (0, 0), "cwX<esc>", "X beta"),
        ("change line", "alpha\nbeta", (0, 0), "ccX<esc>", "X\nbeta"),
        ("substitute char", "alpha beta", (0, 0), "sX<esc>", "Xlpha beta"),
        ("visual delete", "alpha beta gamma", (0, 0), "vwd", "eta gamma"),
        (
            "visual change",
            "alpha beta gamma",
            (0, 0),
            "viwcX<esc>",
            "X beta gamma",
        ),
        ("visual paste", "one two three", (0, 0), "yiwwviwp", "one one three"),
        ("paste", "alpha beta", (0, 0), "yiw$p", "alpha betaalpha"),
        ("join", "alpha\n beta", (0, 0), "J", "alpha beta"),
        ("indent", "alpha\nbeta", (0, 0), ">>", "  alpha\nbeta"),
        ("outdent", "  alpha\nbeta", (0, 0), "<<", "alpha\nbeta"),
    ];

    for (name, initial, cursor, keys, edited) in cases {
        let mut harness = VimHarness::normal_at(initial, cursor.0, cursor.1);
        harness.keys(keys);
        assert_eq!(harness.text(), edited, "{name} edit");
        harness.keys("u");
        assert_eq!(harness.text(), initial, "{name} undo");
        harness.keys("<cmd-r>");
        assert_eq!(harness.text(), edited, "{name} redo");
    }
}

#[test]
fn unsupported_vim_commands_are_intentional_noops() {
    for keys in [".", "q", "@", "\"", ":", "R", "m", "'", "`", "g~", "gu", "gU"] {
        let mut harness = VimHarness::normal_at("alpha beta", 0, 0);
        harness.keys(keys);
        harness.expect_text("alpha beta");
        harness.expect_mode(vim::Mode::Normal);
        harness.expect_pending("");
    }
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
