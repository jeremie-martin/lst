use lst_editor::position::Position;
use lst_editor::{
    vim::{
        Key as VimKey, Mode as VimMode, Modifiers as VimModifiers, NamedKey as VimNamedKey,
        TextSnapshot as VimTextSnapshot, VimCommand, VimState,
    },
    EditorEffect, EditorModel, EditorTab, FileStamp, FocusTarget, RevealIntent, TabId,
    UndoBoundary,
};

mod common;
use common::model_with_tabs;

fn enter_vim_normal(model: &mut EditorModel) {
    model.handle_vim_escape();
    let _ = model.drain_effects();
}

fn dummy_stamp() -> FileStamp {
    FileStamp::from_raw(0, None)
}

#[test]
fn new_tab_switches_active_with_stable_tab_identity() {
    let mut model = EditorModel::empty();
    let first = model.active_tab().id();
    model.new_tab();

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 1);
    assert_eq!(snapshot.tab_count, 2);
    assert_ne!(first, model.active_tab().id());
}

#[test]
fn find_open_uses_selected_single_line_text_and_emits_focus() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );

    model.set_selection(0..3, false);
    model.open_find_panel(false);

    let snapshot = model.snapshot();
    assert!(snapshot.find_visible);
    assert!(!snapshot.find_show_replace);
    assert_eq!(snapshot.find_query, "one");
    assert_eq!(snapshot.find_matches, 2);
    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::Focus(FocusTarget::FindQuery)]
    );
}

#[test]
fn text_edit_is_real_document_behavior() {
    let mut model = EditorModel::empty();

    model.insert_text("abc".into());
    model.replace_text(Some(3..3), "def".into(), UndoBoundary::Merge);

    assert_eq!(model.snapshot().text, "abcdef");
    model.undo();
    assert_eq!(model.snapshot().text, "");
    model.redo();
    assert_eq!(model.snapshot().text, "abcdef");
}

#[test]
fn find_query_reindexes_real_active_document() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );

    model.open_find_panel(false);
    model.update_find_query("one".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_query, "one");
    assert_eq!(snapshot.find_matches, 2);
}

#[test]
fn active_tab_switch_reindexes_find_against_the_new_document() {
    let mut model = model_with_tabs(
        vec![
            EditorTab::from_text(TabId::from_raw(1), "matches".into(), None, "one two one"),
            EditorTab::from_text(TabId::from_raw(2), "none".into(), None, "zero"),
        ],
        "Ready.".into(),
    );

    model.update_find_query("one".into());
    assert_eq!(model.snapshot().find_matches, 2);

    model.set_active_tab(1);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 1);
    assert_eq!(snapshot.find_matches, 0);
}

#[test]
fn opening_find_only_after_replace_clears_replace_mode() {
    let mut model = EditorModel::empty();

    model.open_find_panel(true);
    assert!(model.snapshot().find_show_replace);

    model.open_find_panel(false);

    let snapshot = model.snapshot();
    assert!(snapshot.find_visible);
    assert!(!snapshot.find_show_replace);
}

#[test]
fn find_next_activates_the_next_observable_match_without_document_selection() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );

    model.update_find_query("one".into());
    model.find_next_match();

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_matches, 2);
    assert_eq!(snapshot.find_current, Some(1));
    assert_eq!(snapshot.find_active_match, Some(8..11));
    assert_eq!(snapshot.cursor, 8);
    assert_eq!(snapshot.selection, 8..8);
}

#[test]
fn no_match_find_query_does_not_create_or_extend_document_selection() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta",
        )],
        "Ready.".into(),
    );
    model.move_to_char(6, false, None);

    model.open_find_panel(false);
    model.update_find_query_and_activate("zzz".into());
    model.move_word(false, false);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_matches, 0);
    assert_eq!(snapshot.find_current, None);
    assert_eq!(snapshot.find_active_match, None);
    assert_eq!(snapshot.selection, snapshot.cursor..snapshot.cursor);
}

#[test]
fn replace_all_changes_real_document_text() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );

    model.update_find_query("one".into());
    model.update_find_replacement("three".into());
    model.replace_all_matches_in_document();

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "three two three");
    assert_eq!(snapshot.find_matches, 0);
}

#[test]
fn replace_all_is_undoable_document_behavior() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );

    model.update_find_query("one".into());
    model.update_find_replacement("three".into());
    model.replace_all_matches_in_document();
    assert_eq!(model.snapshot().text, "three two three");

    model.undo();

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "one two one");
    assert_eq!(snapshot.find_matches, 2);
}

#[test]
fn inserting_text_refreshes_active_find_results() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "a",
        )],
        "Ready.".into(),
    );

    model.update_find_query("a".into());
    assert_eq!(model.snapshot().find_matches, 1);

    model.insert_text("a".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "aa");
    assert_eq!(snapshot.find_matches, 2);
}

#[test]
fn replace_all_with_regex_capture_groups_rewrites_document() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alice@example bob@host",
        )],
        "Ready.".into(),
    );

    model.toggle_find_regex();
    model.update_find_query(r"(\w+)@(\w+)".into());
    model.update_find_replacement("${2}_${1}".into());
    model.replace_all_matches_in_document();

    assert_eq!(model.snapshot().text, "example_alice host_bob");
}

#[test]
fn replace_all_with_case_insensitive_rewrites_all_variants() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "Foo foo FOO",
        )],
        "Ready.".into(),
    );

    // Smart-case: lowercase query implies case-insensitive matching.
    model.update_find_query("foo".into());
    model.update_find_replacement("bar".into());
    model.replace_all_matches_in_document();

    assert_eq!(model.snapshot().text, "bar bar bar");
}

#[test]
fn find_in_selection_only_finds_within_captured_range() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "foo bar foo bar foo",
        )],
        "Ready.".into(),
    );

    // Select the middle "bar foo bar" (chars 4..15) and scope to it.
    model.set_selection(4..15, false);
    model.toggle_find_in_selection();
    assert!(model.snapshot().find_in_selection);

    model.update_find_query("foo".into());
    // Only the middle `foo` (at char 8) is inside [4, 15); the other two are outside.
    assert_eq!(model.snapshot().find_matches, 1);

    model.toggle_find_in_selection();
    assert!(!model.snapshot().find_in_selection);
    assert_eq!(model.snapshot().find_matches, 3);
}

#[test]
fn vim_search_word_flips_whole_word_and_case_sensitive_in_snapshot() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "foo foobar foo",
        )],
        "Ready.".into(),
    );
    enter_vim_normal(&mut model);

    // Press `*` on the first word (`foo` at column 0).
    model.handle_vim_key(
        VimKey::Character("*".into()),
        VimModifiers::default(),
        0,
    );

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_query, "foo");
    assert!(snapshot.find_whole_word);
    assert!(snapshot.find_case_sensitive);
    assert!(!snapshot.find_use_regex);
    // `foobar` is rejected by whole-word; only the two standalone `foo`s match.
    assert_eq!(snapshot.find_matches, 2);
}

#[test]
fn replace_all_does_not_re_replace_inside_expanded_replacement() {
    // Replacement text contains the query as a substring. Naive
    // implementations re-scan and run away; we splice each pre-computed
    // match span exactly once.
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "foo foo",
        )],
        "Ready.".into(),
    );

    model.update_find_query("foo".into());
    model.update_find_replacement("xfoox".into());
    model.replace_all_matches_in_document();

    assert_eq!(model.snapshot().text, "xfoox xfoox");
}

#[test]
fn replace_all_regex_with_alternation_respects_whole_word_boundaries() {
    // Validates that the whole-word wrapper `(?:\b(?:foo|bar)\b)` binds
    // alternation tightly — without parens the pattern `\bfoo|bar\b`
    // would reduce to "starts-with-foo OR ends-with-bar", matching far
    // too much.
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "foobar foo barfoo bar",
        )],
        "Ready.".into(),
    );

    model.toggle_find_regex();
    model.toggle_find_whole_word();
    model.update_find_query("foo|bar".into());

    // Only `foo` (idx 7..10) and `bar` (idx 18..21) stand alone.
    assert_eq!(model.snapshot().find_matches, 2);
}

#[test]
fn replace_one_with_regex_expands_capture_groups() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alice@example bob@host",
        )],
        "Ready.".into(),
    );

    model.toggle_find_regex();
    model.update_find_query(r"(\w+)@(\w+)".into());
    model.update_find_replacement("${2}_${1}".into());
    model.replace_current_match();

    // First match is replaced with captures swapped; second is untouched.
    assert_eq!(model.snapshot().text, "example_alice bob@host");
}

#[test]
fn invalid_regex_query_surfaces_error_in_snapshot() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "[abc] [def]",
        )],
        "Ready.".into(),
    );

    model.toggle_find_regex();
    model.update_find_query("[".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_matches, 0);
    assert!(
        snapshot.find_error.is_some(),
        "invalid regex must populate find_error",
    );
}

#[test]
fn goto_line_submit_clamps_to_existing_lines() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta\ngamma",
        )],
        "Ready.".into(),
    );

    model.open_goto_line_panel();
    model.update_goto_line("99".into());
    model.submit_goto_line_input();

    assert_eq!(model.snapshot().cursor, "alpha\nbeta\n".chars().count());
}

#[test]
fn goto_line_submit_accepts_line_and_column() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta\ngamma",
        )],
        "Ready.".into(),
    );

    model.open_goto_line_panel();
    model.update_goto_line("2:3".into());
    model.submit_goto_line_input();

    let snapshot = model.snapshot();
    assert_eq!(snapshot.cursor_position, Position { line: 1, column: 2 });
    assert_eq!(snapshot.cursor, "alpha\nbe".chars().count());
}

#[test]
fn closing_active_tab_preserves_neighbor_as_active() {
    let mut model = EditorModel::empty();
    model.new_tab();
    model.new_tab();
    assert_eq!(model.snapshot().active, 2);

    let third_id = model.tab(2).map(EditorTab::id).expect("third tab exists");
    assert!(model.close_clean_tab_by_id(third_id));

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 1);
    assert_eq!(snapshot.tab_count, 2);
    assert_eq!(
        model.drain_effects().last(),
        Some(&EditorEffect::Focus(FocusTarget::Editor))
    );
}

#[test]
fn closing_inactive_tab_does_not_request_editor_focus() {
    let mut model = EditorModel::empty();
    model.new_tab();
    model.new_tab();
    model.set_active_tab(0);
    model.drain_effects();

    let second_id = model.tab(1).map(EditorTab::id).expect("second tab exists");
    assert!(model.close_clean_tab_by_id(second_id));

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 0);
    assert_eq!(snapshot.tab_count, 2);
    assert_eq!(model.drain_effects(), Vec::<EditorEffect>::new());
}

#[test]
fn movement_and_selection_are_behavioral_commands() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta\ngamma",
        )],
        "Ready.".into(),
    );

    model.move_document_boundary(true, false);
    assert_eq!(model.snapshot().cursor, "alpha beta\ngamma".chars().count());

    model.move_word(true, true);
    assert_eq!(model.snapshot().selection, 11..16);
    assert!(model
        .drain_effects()
        .contains(&EditorEffect::Reveal(RevealIntent::NearestEdge)));
}

#[test]
fn subword_motion_moves_through_identifier_chunks() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "camelCase snake_case HTTPServer version2Alpha",
        )],
        "Ready.".into(),
    );

    for expected in [5, 9, 15, 20, 25, 31, 39, 40, 45] {
        model.move_subword(false, false);
        assert_eq!(model.snapshot().cursor, expected);
    }

    for expected in [40, 39, 32, 25, 21, 16, 10, 5, 0] {
        model.move_subword(true, false);
        assert_eq!(model.snapshot().cursor, expected);
    }
}

#[test]
fn subword_selection_extends_from_anchor() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "camelCase",
        )],
        "Ready.".into(),
    );

    model.move_subword(false, true);
    let first = model.snapshot();
    assert_eq!(first.selection, 0..5);
    assert_eq!(first.cursor, 5);

    model.move_subword(false, true);
    let second = model.snapshot();
    assert_eq!(second.selection, 0..9);
    assert_eq!(second.cursor, 9);
}

#[test]
fn subword_motion_collapses_existing_selection() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "camelCase",
        )],
        "Ready.".into(),
    );

    model.set_selection(5..9, false);
    model.move_subword(true, false);
    assert_eq!(model.snapshot().selection, 5..5);

    model.set_selection(5..9, false);
    model.move_subword(false, false);
    assert_eq!(model.snapshot().selection, 9..9);
}

#[test]
fn whole_word_motion_still_uses_whole_identifier() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "camelCase snake_case version2Alpha",
        )],
        "Ready.".into(),
    );

    for expected in [9, 20, 34] {
        model.move_word(false, false);
        assert_eq!(model.snapshot().cursor, expected);
    }
}

#[test]
fn logical_row_motion_snaps_to_document_edges() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );

    model.move_to_char(2, false, None);
    model.move_logical_rows(-1, false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 0 }
    );

    model.move_to_char("alpha\nbe".chars().count(), false, None);
    model.move_logical_rows(1, false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 1, column: 4 }
    );
}

#[test]
fn logical_row_edge_snap_extends_selection() {
    let mut top = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );
    top.move_to_char(2, false, None);
    top.move_logical_rows(-1, true);
    let top_snapshot = top.snapshot();
    assert_eq!(top_snapshot.selection, 0..2);
    assert_eq!(top_snapshot.cursor, 0);

    let mut bottom = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );
    bottom.move_to_char("alpha\nbe".chars().count(), false, None);
    bottom.move_logical_rows(1, true);
    let bottom_snapshot = bottom.snapshot();
    assert_eq!(
        bottom_snapshot.selection,
        "alpha\nbe".chars().count().."alpha\nbeta".chars().count()
    );
    assert_eq!(bottom_snapshot.cursor, "alpha\nbeta".chars().count());
}

#[test]
fn logical_row_edge_noop_collapses_selection() {
    let mut top = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );
    top.set_selection(0..2, true);
    top.move_logical_rows(-1, false);
    let top_snapshot = top.snapshot();
    assert_eq!(top_snapshot.selection, 0..0);
    assert_eq!(top_snapshot.cursor, 0);

    let mut bottom = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );
    let eof = "alpha\nbeta".chars().count();
    bottom.set_selection((eof - 2)..eof, false);
    bottom.move_logical_rows(1, false);
    let bottom_snapshot = bottom.snapshot();
    assert_eq!(bottom_snapshot.selection, eof..eof);
    assert_eq!(bottom_snapshot.cursor, eof);
}

#[test]
fn logical_row_edge_snap_preserves_preferred_column() {
    let mut top = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "abcd\nefghijkl",
        )],
        "Ready.".into(),
    );
    top.move_to_char(2, false, Some(6));
    top.move_logical_rows(-1, false);
    assert_eq!(
        top.snapshot().cursor_position,
        Position { line: 0, column: 0 }
    );
    top.move_logical_rows(1, false);
    assert_eq!(
        top.snapshot().cursor_position,
        Position { line: 1, column: 6 }
    );

    let mut bottom = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "efghijkl\nabcd",
        )],
        "Ready.".into(),
    );
    bottom.move_to_char("efghijkl\nab".chars().count(), false, Some(6));
    bottom.move_logical_rows(1, false);
    assert_eq!(
        bottom.snapshot().cursor_position,
        Position { line: 1, column: 4 }
    );
    bottom.move_logical_rows(-1, false);
    assert_eq!(
        bottom.snapshot().cursor_position,
        Position { line: 0, column: 6 }
    );
}

#[test]
fn smart_home_toggles_between_first_non_blank_and_line_start() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "  alpha",
        )],
        "Ready.".into(),
    );

    model.move_to_char(5, false, None);
    let _ = model.drain_effects();

    model.smart_home(false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 2 }
    );

    model.smart_home(false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 0 }
    );

    model.smart_home(false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 2 }
    );
}

#[test]
fn smart_home_selection_tracks_the_selection_head() {
    let mut forward = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "  alpha",
        )],
        "Ready.".into(),
    );
    forward.set_selection(0..7, false);
    forward.smart_home(true);
    let forward_snapshot = forward.snapshot();
    assert_eq!(forward_snapshot.selection, 0..2);
    assert_eq!(forward_snapshot.cursor, 2);

    let mut reversed = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "  alpha",
        )],
        "Ready.".into(),
    );
    reversed.set_selection(2..7, true);
    reversed.smart_home(true);
    let reversed_snapshot = reversed.snapshot();
    assert_eq!(reversed_snapshot.selection, 0..7);
    assert_eq!(reversed_snapshot.cursor, 0);
}

#[test]
fn smart_home_clears_preferred_column_and_skips_noop_reveal() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "  alpha\n0123456789\n    ",
        )],
        "Ready.".into(),
    );

    model.move_to_char(5, false, Some(8));
    let _ = model.drain_effects();

    model.smart_home(false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 2 }
    );
    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::Reveal(RevealIntent::NearestEdge)]
    );

    model.move_logical_rows(1, false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 1, column: 2 }
    );

    model.move_document_boundary(true, false);
    model.move_line_boundary(false, false);
    let _ = model.drain_effects();
    model.smart_home(false);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 2, column: 0 }
    );
    assert_eq!(model.drain_effects(), Vec::<EditorEffect>::new());
}

fn make_model(text: &str) -> EditorModel {
    model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            text,
        )],
        "Ready.".into(),
    )
}

#[test]
fn tab_with_no_selection_inserts_four_spaces() {
    let mut model = make_model("hello");
    model.move_to_char(2, false, None);
    let _ = model.drain_effects();

    model.insert_tab_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "he    llo");
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 6 }
    );
}

#[test]
fn tab_with_single_line_selection_replaces_selection_with_four_spaces() {
    let mut model = make_model("alpha beta gamma");
    model.set_selection(6..10, false);
    model.insert_tab_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "alpha      gamma");
    assert_eq!(
        model.snapshot().cursor_position,
        Position {
            line: 0,
            column: 10
        }
    );
}

#[test]
fn tab_with_multi_line_selection_indents_each_line_and_keeps_selection() {
    let mut model = make_model("alpha\nbeta\ngamma");
    // Select from "lpha" on line 0 through "ga" on line 2.
    let start = 1; // line 0, col 1
    let end = "alpha\nbeta\nga".chars().count(); // line 2, col 2
    model.set_selection(start..end, false);

    model.insert_tab_at_cursor();
    assert_eq!(
        model.active_tab().buffer_text(),
        "    alpha\n    beta\n    gamma"
    );
    let snapshot = model.snapshot();
    // Each endpoint's column shifted by +4.
    assert_eq!(snapshot.cursor_position, Position { line: 2, column: 6 });
    let new_start = "    a".chars().count();
    let new_end = "    alpha\n    beta\n    ga".chars().count();
    assert_eq!(snapshot.selection, new_start..new_end);
}

#[test]
fn tab_does_not_indent_line_after_selection_ending_at_column_zero() {
    let mut model = make_model("alpha\nbeta\ngamma");
    // Select from start of line 0 to start of line 2 (col 0).
    let start = 0;
    let end = "alpha\nbeta\n".chars().count();
    model.set_selection(start..end, false);

    model.insert_tab_at_cursor();
    assert_eq!(
        model.active_tab().buffer_text(),
        "    alpha\n    beta\ngamma"
    );
    let snapshot = model.snapshot();
    let new_end = "    alpha\n    beta\n".chars().count();
    assert_eq!(snapshot.selection, 0..new_end);
}

#[test]
fn tab_indents_selection_crossing_newline_to_next_line_column_zero() {
    let mut model = make_model("alpha\nbeta");
    let start = 1;
    let end = "alpha\n".chars().count();
    model.set_selection(start..end, false);

    model.insert_tab_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "    alpha\nbeta");
    let snapshot = model.snapshot();
    let new_start = "    a".chars().count();
    let new_end = "    alpha\n".chars().count();
    assert_eq!(snapshot.selection, new_start..new_end);
}

#[test]
fn shift_tab_no_selection_outdents_current_line() {
    let mut model = make_model("      hello");
    model.move_to_char(8, false, None); // cursor on 'e' of "hello"
    model.outdent_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "  hello");
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 0, column: 4 }
    );
}

#[test]
fn shift_tab_outdents_partial_or_zero_whitespace() {
    let mut partial = make_model("  hi");
    partial.move_to_char(3, false, None);
    partial.outdent_at_cursor();
    assert_eq!(partial.active_tab().buffer_text(), "hi");
    assert_eq!(
        partial.snapshot().cursor_position,
        Position { line: 0, column: 1 }
    );

    let mut empty = make_model("hi");
    empty.move_to_char(1, false, None);
    let revision_before = empty.active_tab().revision();
    empty.outdent_at_cursor();
    assert_eq!(empty.active_tab().buffer_text(), "hi");
    assert_eq!(empty.active_tab().revision(), revision_before);
}

#[test]
fn shift_tab_multi_line_selection_outdents_each_line_independently() {
    let mut model = make_model("        eight\n  two\nzero");
    let end = "        eight\n  two\nzero".chars().count();
    model.set_selection(0..end, false);

    model.outdent_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "    eight\ntwo\nzero");
    let snapshot = model.snapshot();
    let new_end = "    eight\ntwo\nzero".chars().count();
    assert_eq!(snapshot.selection, 0..new_end);
}

#[test]
fn indent_then_undo_restores_original_text_and_selection() {
    let mut model = make_model("alpha\nbeta\ngamma");
    let start = 0;
    let end = "alpha\nbeta\ng".chars().count();
    model.set_selection(start..end, false);
    let before_text = model.active_tab().buffer_text();
    let before_selection = model.snapshot().selection;

    model.insert_tab_at_cursor();
    assert_ne!(model.active_tab().buffer_text(), before_text);

    model.undo();
    assert_eq!(model.active_tab().buffer_text(), before_text);
    assert_eq!(model.snapshot().selection, before_selection);
}

#[test]
fn tab_preserves_reversed_selection_flag() {
    let mut model = make_model("alpha\nbeta\ngamma");
    let start = "a".chars().count();
    let end = "alpha\nbeta\nga".chars().count();
    model.set_selection(start..end, true); // reversed: cursor at start

    model.insert_tab_at_cursor();
    let snapshot = model.snapshot();
    let new_start = "    a".chars().count();
    let new_end = "    alpha\n    beta\n    ga".chars().count();
    assert_eq!(snapshot.selection, new_start..new_end);
    // Cursor sits at the start (reversed).
    assert_eq!(snapshot.cursor, new_start);
}

#[test]
fn shift_tab_shifts_selection_columns_within_touched_range() {
    let mut model = make_model("        eight\n  two\nzero");
    // Select from col 6 of line 0 (the 'i' of "eight" is at col 9; col 6 is
    // mid-whitespace) through col 3 of line 1 (the 'o' of "two" — col 2 is
    // 't', col 3 is 'w').
    let start = 6;
    let end = "        eight\n  t".chars().count();
    model.set_selection(start..end, false);

    model.outdent_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "    eight\ntwo\nzero");
    let snapshot = model.snapshot();
    // Line 0 lost 4 leading spaces → start col 6 saturates to col 2.
    let new_start = 2;
    // Line 1 lost 2 leading spaces → end col 3 saturates to col 1.
    let new_end = "    eight\nt".chars().count();
    assert_eq!(snapshot.selection, new_start..new_end);
}

#[test]
fn shift_tab_does_not_outdent_line_after_selection_ending_at_column_zero() {
    let mut model = make_model("    alpha\n    beta\n    gamma");
    let start = 0;
    let end = "    alpha\n    beta\n".chars().count();
    model.set_selection(start..end, false);

    model.outdent_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "alpha\nbeta\n    gamma");
    let snapshot = model.snapshot();
    let new_end = "alpha\nbeta\n".chars().count();
    assert_eq!(snapshot.selection, 0..new_end);
}

#[test]
fn shift_tab_preserves_reversed_selection_flag() {
    let mut model = make_model("    alpha\n    beta\n    gamma");
    let start = "    a".chars().count(); // line 0 col 5
    let end = "    alpha\n    beta\n    ga".chars().count(); // line 2 col 6
    model.set_selection(start..end, true); // reversed: cursor at start

    model.outdent_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "alpha\nbeta\ngamma");
    let snapshot = model.snapshot();
    let new_start = "a".chars().count();
    let new_end = "alpha\nbeta\nga".chars().count();
    assert_eq!(snapshot.selection, new_start..new_end);
    assert_eq!(snapshot.cursor, new_start);
    assert!(model.active_tab().selection_reversed());
}

#[test]
fn tab_indents_blank_lines_in_the_middle_of_selection() {
    let mut model = make_model("alpha\n\ngamma");
    let end = "alpha\n\nga".chars().count();
    model.set_selection(0..end, false);

    model.insert_tab_at_cursor();
    assert_eq!(
        model.active_tab().buffer_text(),
        "    alpha\n    \n    gamma"
    );
}

#[test]
fn shift_tab_multi_line_undo_is_a_single_step() {
    let mut model = make_model("    alpha\n    beta\n    gamma");
    let end = "    alpha\n    beta\n    gamma".chars().count();
    model.set_selection(0..end, false);
    let before_text = model.active_tab().buffer_text();
    let before_selection = model.snapshot().selection;

    model.outdent_at_cursor();
    assert_eq!(model.active_tab().buffer_text(), "alpha\nbeta\ngamma");

    model.undo();
    assert_eq!(model.active_tab().buffer_text(), before_text);
    assert_eq!(model.snapshot().selection, before_selection);
}

#[test]
fn horizontal_collapse_commands_collapse_active_selection() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "abcdef",
        )],
        "Ready.".into(),
    );

    model.set_selection(2..5, false);
    model.move_horizontal_collapsed(true);
    assert_eq!(model.snapshot().selection, 2..2);

    model.set_selection(2..5, false);
    model.move_horizontal_collapsed(false);
    assert_eq!(model.snapshot().selection, 5..5);
}

#[test]
fn selecting_horizontal_movement_still_extends_selection() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "abcdef",
        )],
        "Ready.".into(),
    );

    model.move_to_char(2, false, None);
    model.move_horizontal_by(1, true);

    assert_eq!(model.snapshot().selection, 2..3);
}

#[test]
fn display_row_movement_uses_wrapped_rows_behaviorally() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta gamma\nshort",
        )],
        "Ready.".into(),
    );

    model.move_to_char(1, false, None);
    model.move_display_rows_by(1, false, 6);
    assert_eq!(model.snapshot().cursor, 7);

    model.move_display_rows_by(1, false, 6);
    assert_eq!(model.snapshot().cursor, 12);

    model.move_display_rows_by(1, false, 6);
    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 1, column: 1 }
    );
}

#[test]
fn display_row_selection_extends_from_anchor() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta gamma",
        )],
        "Ready.".into(),
    );

    model.move_to_char(1, false, None);
    model.move_display_rows_by(1, true, 6);

    assert_eq!(model.snapshot().selection, 1..7);
}

#[test]
fn display_row_motion_snaps_to_document_edges() {
    let mut top = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta gamma\nshort",
        )],
        "Ready.".into(),
    );
    top.move_to_char(2, false, None);
    top.move_display_rows_by(-1, false, 6);
    assert_eq!(
        top.snapshot().cursor_position,
        Position { line: 0, column: 0 }
    );

    let mut bottom = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "short\nabcdefghijkl",
        )],
        "Ready.".into(),
    );
    bottom.move_to_char("short\nabcdefghi".chars().count(), false, None);
    bottom.move_display_rows_by(1, false, 4);
    assert_eq!(
        bottom.snapshot().cursor_position,
        Position {
            line: 1,
            column: "abcdefghijkl".chars().count(),
        }
    );
}

#[test]
fn display_row_edge_noop_collapses_selection() {
    let mut top = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta gamma\nshort",
        )],
        "Ready.".into(),
    );
    top.set_selection(0..2, true);
    top.move_display_rows_by(-1, false, 6);
    let top_snapshot = top.snapshot();
    assert_eq!(top_snapshot.selection, 0..0);
    assert_eq!(top_snapshot.cursor, 0);

    let mut bottom = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "short\nabcdefghijkl",
        )],
        "Ready.".into(),
    );
    let eof = "short\nabcdefghijkl".chars().count();
    bottom.set_selection((eof - 3)..eof, false);
    bottom.move_display_rows_by(1, false, 4);
    let bottom_snapshot = bottom.snapshot();
    assert_eq!(bottom_snapshot.selection, eof..eof);
    assert_eq!(bottom_snapshot.cursor, eof);
}

#[test]
fn display_row_movement_clamps_and_falls_back_when_wrap_is_disabled() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta gamma\nshort",
        )],
        "Ready.".into(),
    );

    model.move_to_char(2, false, None);
    model.move_display_rows_by(-1, false, 6);
    assert_eq!(model.snapshot().cursor, 0);

    model.toggle_wrap();
    model.move_to_char(1, false, None);
    model.move_display_rows_by(1, false, 6);

    assert_eq!(
        model.snapshot().cursor_position,
        Position { line: 1, column: 1 }
    );
}

#[test]
fn input_text_replacement_owns_undo_grouping_policy() {
    let mut model = EditorModel::empty();

    model.replace_text_from_input(None, "a".into());
    model.replace_text_from_input(None, "b".into());
    assert_eq!(model.snapshot().text, "ab");

    model.undo();
    assert_eq!(model.snapshot().text, "");

    model.replace_text_from_input(None, "a".into());
    model.replace_text_from_input(None, " ".into());
    assert_eq!(model.snapshot().text, "a ");

    model.undo();
    assert_eq!(model.snapshot().text, "a");
}

#[test]
fn insert_text_auto_dedents_close_brace_on_four_space_blank_line() {
    let mut model = make_model("    ");
    model.move_to_char(4, false, None);

    model.insert_text("}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "}");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn input_text_auto_dedents_close_brace_by_one_indent_level() {
    let mut model = make_model("        ");
    model.move_to_char(8, false, None);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "    }");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 5 });
}

#[test]
fn input_text_auto_dedent_clamps_when_indent_is_less_than_width() {
    let mut model = make_model("  ");
    model.move_to_char(2, false, None);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "}");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn input_text_auto_dedent_replaces_whitespace_only_selection() {
    let mut model = make_model("        ");
    model.set_selection(2..6, false);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "}  ");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn input_text_does_not_auto_dedent_close_brace_on_non_blank_line() {
    let mut model = make_model("    alpha");
    model.move_to_char(4, false, None);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "    }alpha");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 5 });
}

#[test]
fn input_text_auto_dedent_only_applies_to_single_close_brace() {
    let mut bracket = make_model("    ");
    bracket.move_to_char(4, false, None);
    bracket.replace_text_from_input(None, "]".into());
    assert_eq!(bracket.snapshot().text, "    ]");

    let mut paren = make_model("    ");
    paren.move_to_char(4, false, None);
    paren.replace_text_from_input(None, ")".into());
    assert_eq!(paren.snapshot().text, "    )");

    let mut multi = make_model("    ");
    multi.move_to_char(4, false, None);
    multi.replace_text_from_input(None, "}}".into());
    assert_eq!(multi.snapshot().text, "    }}");
}

#[test]
fn input_text_does_not_auto_dedent_tab_or_mixed_indentation() {
    let mut tab = make_model("\t");
    tab.move_to_char(1, false, None);
    tab.replace_text_from_input(None, "}".into());
    assert_eq!(tab.snapshot().text, "\t}");

    let mut mixed = make_model("  \t");
    mixed.move_to_char(3, false, None);
    mixed.replace_text_from_input(None, "}".into());
    assert_eq!(mixed.snapshot().text, "  \t}");
}

#[test]
fn input_text_auto_dedent_respects_caret_at_line_start_and_middle() {
    let mut start = make_model("        ");
    start.move_to_char(0, false, None);
    start.replace_text_from_input(None, "}".into());
    let start_snapshot = start.snapshot();
    assert_eq!(start_snapshot.text, "}        ");
    assert_eq!(
        start_snapshot.cursor_position,
        Position { line: 0, column: 1 }
    );

    let mut middle = make_model("        ");
    middle.move_to_char(2, false, None);
    middle.replace_text_from_input(None, "}".into());
    let middle_snapshot = middle.snapshot();
    assert_eq!(middle_snapshot.text, "}      ");
    assert_eq!(
        middle_snapshot.cursor_position,
        Position { line: 0, column: 1 }
    );
}

#[test]
fn input_text_does_not_auto_dedent_multi_line_replacement_ranges() {
    let mut model = make_model("    \nnext");
    let end = "    \n".chars().count();
    model.set_selection(0..end, false);

    model.replace_text_from_input(None, "}".into());

    assert_eq!(model.snapshot().text, "}next");
}

#[test]
fn input_text_auto_dedent_close_brace_is_undoable() {
    let mut model = make_model("        ");
    model.move_to_char(8, false, None);

    model.replace_text_from_input(None, "}".into());
    assert_eq!(model.snapshot().text, "    }");

    model.undo();
    assert_eq!(model.snapshot().text, "        ");
}

#[test]
fn input_text_auto_dedents_close_brace_on_second_line() {
    let mut model = make_model("foo\n        ");
    let caret = "foo\n        ".chars().count();
    model.move_to_char(caret, false, None);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "foo\n    }");
    assert_eq!(snapshot.cursor_position, Position { line: 1, column: 5 });
}

#[test]
fn input_text_auto_dedent_close_brace_undo_is_distinct_from_prior_typing() {
    let mut model = make_model("");
    model.replace_text_from_input(None, "        ".into());
    assert_eq!(model.snapshot().text, "        ");

    model.replace_text_from_input(None, "}".into());
    assert_eq!(model.snapshot().text, "    }");

    model.undo();
    assert_eq!(model.snapshot().text, "        ");
}

#[test]
fn insert_text_auto_pairs_open_paren() {
    let mut model = make_model("");
    model.insert_text("(".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "()");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn input_text_auto_pairs_each_bracket_pair() {
    for (opener, expected) in [('(', "()"), ('[', "[]"), ('{', "{}")] {
        let mut model = make_model("");
        model.replace_text_from_input(None, opener.to_string());
        let snapshot = model.snapshot();
        assert_eq!(snapshot.text, expected, "opener {opener}");
        assert_eq!(
            snapshot.cursor_position,
            Position { line: 0, column: 1 },
            "opener {opener}"
        );
    }
}

#[test]
fn input_text_auto_pairs_each_quote() {
    for (quote, expected) in [('"', "\"\""), ('\'', "''"), ('`', "``")] {
        let mut model = make_model("");
        model.replace_text_from_input(None, quote.to_string());
        let snapshot = model.snapshot();
        assert_eq!(snapshot.text, expected, "quote {quote}");
        assert_eq!(
            snapshot.cursor_position,
            Position { line: 0, column: 1 },
            "quote {quote}"
        );
    }
}

#[test]
fn input_text_third_repeated_quote_or_backtick_inserts_literally() {
    for (quote, expected) in [('"', "\"\"\""), ('\'', "'''"), ('`', "```")] {
        let mut model = make_model("");
        model.replace_text_from_input(None, quote.to_string());
        model.replace_text_from_input(None, quote.to_string());
        model.replace_text_from_input(None, quote.to_string());

        let snapshot = model.snapshot();
        assert_eq!(snapshot.text, expected, "quote {quote}");
        assert_eq!(
            snapshot.cursor_position,
            Position { line: 0, column: 3 },
            "quote {quote}"
        );
    }
}

#[test]
fn input_text_does_not_auto_pair_quote_after_identifier_char() {
    let mut model = make_model("don");
    model.move_to_char(3, false, None);

    model.replace_text_from_input(None, "'".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "don'");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 4 });
}

#[test]
fn input_text_does_not_auto_pair_quote_after_backslash() {
    for (quote, expected) in [('"', "\\\""), ('\'', "\\'"), ('`', "\\`")] {
        let mut model = make_model("\\");
        model.move_to_char(1, false, None);

        model.replace_text_from_input(None, quote.to_string());

        let snapshot = model.snapshot();
        assert_eq!(snapshot.text, expected, "quote {quote}");
        assert_eq!(
            snapshot.cursor_position,
            Position { line: 0, column: 2 },
            "quote {quote}"
        );
    }
}

#[test]
fn input_text_does_not_auto_pair_quote_before_identifier_char() {
    let mut model = make_model("abc");
    model.move_to_char(0, false, None);

    model.replace_text_from_input(None, "\"".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "\"abc");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn input_text_auto_pairs_quote_between_non_word_chars() {
    let mut model = make_model("  ");
    model.move_to_char(1, false, None);

    model.replace_text_from_input(None, "'".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, " '' ");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 2 });
}

#[test]
fn input_text_overtypes_matching_closer_when_next_char_matches() {
    let mut model = make_model("");
    model.replace_text_from_input(None, "(".into());
    assert_eq!(model.snapshot().text, "()");

    model.replace_text_from_input(None, ")".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "()");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 2 });
}

#[test]
fn input_text_overtypes_matching_quote() {
    let mut model = make_model("");
    model.replace_text_from_input(None, "\"".into());
    assert_eq!(model.snapshot().text, "\"\"");

    model.replace_text_from_input(None, "\"".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "\"\"");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 2 });
}

#[test]
fn input_text_closer_with_no_char_ahead_inserts_literally() {
    let mut model = make_model("");
    model.replace_text_from_input(None, ")".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, ")");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn input_text_does_not_overtype_mismatched_closer() {
    let mut model = make_model("(]");
    model.move_to_char(1, false, None);

    model.replace_text_from_input(None, ")".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "()]");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 2 });
}

#[test]
fn input_text_surrounds_selection_with_pair() {
    let mut model = make_model("abc");
    model.set_selection(0..3, false);

    model.replace_text_from_input(None, "(".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "(abc)");
    assert_eq!(snapshot.selection, 1..4);
}

#[test]
fn input_text_surrounds_selection_with_quote() {
    let mut model = make_model("abc");
    model.set_selection(0..3, false);

    model.replace_text_from_input(None, "\"".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "\"abc\"");
    assert_eq!(snapshot.selection, 1..4);
}

#[test]
fn input_text_surround_preserves_reversed_selection_flag() {
    let mut model = make_model("abc");
    model.set_selection(0..3, true);

    model.replace_text_from_input(None, "(".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "(abc)");
    assert_eq!(snapshot.selection, 1..4);
    assert_eq!(snapshot.cursor, 1);
    assert!(model.active_tab().selection_reversed());
}

#[test]
fn input_text_auto_pair_realigns_find_after_restoring_caret() {
    let mut model = make_model(")");
    model.update_find_query(")".into());
    model.move_to_char(0, false, None);

    model.replace_text_from_input(None, "(".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "())");
    assert_eq!(snapshot.selection, 1..1);
    assert_eq!(snapshot.find_matches, 2);
    assert_eq!(snapshot.find_current, Some(0));
    assert_eq!(snapshot.find_active_match, Some(1..2));
}

#[test]
fn input_text_surround_realigns_find_after_restoring_selection() {
    let mut model = make_model("))");
    model.update_find_query(")".into());
    model.set_selection(0..1, false);

    model.replace_text_from_input(None, "(".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "()))");
    assert_eq!(snapshot.selection, 1..2);
    assert_eq!(snapshot.find_matches, 3);
    assert_eq!(snapshot.find_current, Some(0));
    assert_eq!(snapshot.find_active_match, Some(1..2));
}

#[test]
fn input_text_surround_preserves_multi_line_selection() {
    let mut model = make_model("abc\ndef");
    model.set_selection(0..7, false);

    model.replace_text_from_input(None, "(".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "(abc\ndef)");
    assert_eq!(snapshot.selection, 1..8);
}

#[test]
fn input_text_auto_pair_is_single_undo_step() {
    let mut model = make_model("");
    model.replace_text_from_input(None, "(".into());
    assert_eq!(model.snapshot().text, "()");

    model.undo();
    assert_eq!(model.snapshot().text, "");
}

#[test]
fn input_text_surround_is_single_undo_step() {
    let mut model = make_model("abc");
    model.set_selection(0..3, false);

    model.replace_text_from_input(None, "(".into());
    assert_eq!(model.snapshot().text, "(abc)");

    model.undo();
    assert_eq!(model.snapshot().text, "abc");
}

#[test]
fn input_text_auto_pair_overtype_takes_precedence_over_dedent() {
    let mut model = make_model("        }");
    model.move_to_char(8, false, None);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "        }");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 9 });
}

#[test]
fn input_text_auto_dedent_still_fires_when_no_matching_closer_ahead() {
    let mut model = make_model("        ");
    model.move_to_char(8, false, None);

    model.replace_text_from_input(None, "}".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "    }");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 5 });
}

#[test]
fn replace_text_does_not_auto_pair() {
    let mut model = make_model("");
    model.replace_text(None, "(".into(), UndoBoundary::Break);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "(");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn replace_and_mark_text_does_not_auto_pair() {
    let mut model = make_model("");
    model.replace_and_mark_text(None, "(".into(), None);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "(");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn delete_word_and_line_ops_are_undoable_document_behavior() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta\ngamma",
        )],
        "Ready.".into(),
    );

    model.move_word(false, false);
    model.delete_word(false);
    assert_eq!(model.snapshot().text, "alpha\ngamma");

    model.duplicate_line();
    assert_eq!(model.snapshot().text, "alpha\nalpha\ngamma");

    model.undo();
    model.undo();
    assert_eq!(model.snapshot().text, "alpha beta\ngamma");
}

#[test]
fn duplicate_line_duplicates_active_selection_and_selects_the_copy() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta",
        )],
        "Ready.".into(),
    );

    model.set_selection(0..5, false);
    model.duplicate_line();

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "alphaalpha beta");
    assert_eq!(snapshot.selection, 5..10);
    assert_eq!(snapshot.cursor, 10);
}

#[test]
fn duplicate_selection_is_a_separate_undo_step_from_typed_input() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta",
        )],
        "Ready.".into(),
    );

    model.set_selection(0..5, false);
    model.duplicate_line();
    model.replace_text_from_input(None, "x".into());
    assert_eq!(model.snapshot().text, "alphax beta");

    model.undo();
    assert_eq!(model.snapshot().text, "alphaalpha beta");

    model.undo();
    assert_eq!(model.snapshot().text, "alpha beta");
}

#[test]
fn delete_word_commands_delete_active_selection_before_word_boundaries() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "hello world",
        )],
        "Ready.".into(),
    );

    model.set_selection(0..5, false);
    model.delete_word(true);
    assert_eq!(model.snapshot().text, " world");

    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "hello world",
        )],
        "Ready.".into(),
    );
    model.set_selection(6..11, false);
    model.delete_word(false);
    assert_eq!(model.snapshot().text, "hello ");
}

#[test]
fn clipboard_commands_emit_boundary_effects_without_fakes() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "hello world",
        )],
        "Ready.".into(),
    );
    model.set_selection(0..5, false);

    model.copy_selection();
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("hello".into()),
            EditorEffect::WritePrimary("hello".into())
        ]
    );

    model.cut_selection();
    assert_eq!(model.snapshot().text, " world");
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("hello".into()),
            EditorEffect::WritePrimary("hello".into()),
            EditorEffect::Reveal(RevealIntent::NearestEdge)
        ]
    );
}

#[test]
fn clipboard_commands_fall_back_to_the_current_line_without_selection() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta\n",
        )],
        "Ready.".into(),
    );

    model.move_logical_rows(1, false);
    let _ = model.drain_effects();

    model.copy_selection();
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("beta\n".into()),
            EditorEffect::WritePrimary("beta\n".into())
        ]
    );

    model.cut_selection();
    assert_eq!(model.snapshot().text, "alpha\n");
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("beta\n".into()),
            EditorEffect::WritePrimary("beta\n".into()),
            EditorEffect::Reveal(RevealIntent::NearestEdge)
        ]
    );
}

#[test]
fn clipboard_commands_treat_the_last_unterminated_line_as_linewise() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );

    model.move_logical_rows(1, false);
    let _ = model.drain_effects();

    model.copy_selection();
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("\nbeta".into()),
            EditorEffect::WritePrimary("\nbeta".into())
        ]
    );

    model.cut_selection();
    assert_eq!(model.snapshot().text, "alpha");
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("\nbeta".into()),
            EditorEffect::WritePrimary("\nbeta".into()),
            EditorEffect::Reveal(RevealIntent::NearestEdge)
        ]
    );
}

#[test]
fn clipboard_commands_include_the_trailing_blank_line_at_eof() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\n",
        )],
        "Ready.".into(),
    );

    let end = model.active_tab().len_chars();
    model.move_to_char(end, false, None);
    let _ = model.drain_effects();

    model.copy_selection();
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("\n".into()),
            EditorEffect::WritePrimary("\n".into())
        ]
    );

    model.cut_selection();
    assert_eq!(model.snapshot().text, "alpha");
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("\n".into()),
            EditorEffect::WritePrimary("\n".into()),
            EditorEffect::Reveal(RevealIntent::NearestEdge)
        ]
    );
}

#[test]
fn clipboard_commands_preserve_crlf_when_cutting_the_trailing_blank_line() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\r\n",
        )],
        "Ready.".into(),
    );

    let end = model.active_tab().len_chars();
    model.move_to_char(end, false, None);
    let _ = model.drain_effects();

    model.cut_selection();
    assert_eq!(model.snapshot().text, "alpha");
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("\r\n".into()),
            EditorEffect::WritePrimary("\r\n".into()),
            EditorEffect::Reveal(RevealIntent::NearestEdge)
        ]
    );
}

#[test]
fn file_commands_emit_runtime_effects_and_apply_results() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example.txt".into(),
            Some(path.clone()),
            "hello",
        )],
        "Ready.".into(),
    );
    model.insert_text(" world".into());
    model.drain_effects();

    model.request_save();
    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::SaveFile {
            tab_id: TabId::from_raw(1),
            path: path.clone(),
            body: " worldhello".into(),
            expected_stamp: None,
        }]
    );

    let tab_id = model.active_tab_id();
    model.save_finished_for_tab(tab_id, path.clone(), dummy_stamp());
    let snapshot = model.snapshot();
    assert!(!snapshot.tab_modified[0]);
    assert_eq!(snapshot.status, format!("Saved {}.", path.display()));
}

#[test]
fn save_effects_can_target_inactive_tabs_by_id() {
    let first_path = std::path::PathBuf::from("/tmp/first.txt");
    let second_path = std::path::PathBuf::from("/tmp/second.txt");
    let second_id = TabId::from_raw(2);
    let mut second = EditorTab::from_text(
        second_id,
        "second.txt".into(),
        Some(second_path.clone()),
        "second",
    );
    second.replace_char_range(0..0, "edited ");
    let mut model = model_with_tabs(
        vec![
            EditorTab::from_text(
                TabId::from_raw(1),
                "first.txt".into(),
                Some(first_path),
                "first",
            ),
            second,
        ],
        "Ready.".into(),
    );

    model.request_save_tab(second_id);

    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::SaveFile {
            tab_id: second_id,
            path: second_path,
            body: "edited second".into(),
            expected_stamp: None,
        }]
    );
}

#[test]
fn save_finished_for_tab_does_not_clear_the_active_tab_by_accident() {
    let first_path = std::path::PathBuf::from("/tmp/first.txt");
    let second_path = std::path::PathBuf::from("/tmp/second.txt");
    let mut first = EditorTab::from_text(
        TabId::from_raw(1),
        "first.txt".into(),
        Some(first_path),
        "first",
    );
    let mut second = EditorTab::from_text(
        TabId::from_raw(2),
        "second.txt".into(),
        Some(second_path.clone()),
        "second",
    );
    first.replace_char_range(0..0, "edited ");
    second.replace_char_range(0..0, "saved ");
    let mut model = model_with_tabs(vec![first, second], "Ready.".into());

    model.save_finished_for_tab(
        TabId::from_raw(2),
        second_path,
        lst_editor::FileStamp::from_raw(10, Some(20)),
    );

    assert_eq!(model.snapshot().tab_modified, [true, false]);
}

#[test]
fn dirty_tab_close_request_signals_save_and_close() {
    let mut model = EditorModel::empty();
    let tab_id = model.active_tab().id();
    model.insert_text("unsaved".into());

    assert_eq!(
        model.close_request_for_tab(0),
        Some(lst_editor::TabCloseRequest::SaveAndClose { tab_id })
    );

    assert!(model.discard_close_tab_by_id(tab_id));
    let snapshot = model.snapshot();
    assert_eq!(snapshot.tab_count, 1);
    assert_eq!(snapshot.text, "");
    assert!(!snapshot.tab_modified[0]);
}

#[test]
fn autosave_tick_emits_modified_file_backed_tabs() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example.txt".into(),
            Some(path.clone()),
            "hello",
        )],
        "Ready.".into(),
    );
    model.insert_text("!".into());
    let revision = model.snapshot().active_revision;
    model.drain_effects();

    model.autosave_tick();

    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::AutosaveFile {
            tab_id: TabId::from_raw(1),
            path: path.clone(),
            body: "!hello".into(),
            revision,
            expected_stamp: None,
        }]
    );

    let tab_id = model.active_tab_id();
    model.autosave_finished_for_tab(tab_id, path, revision, dummy_stamp());
    assert!(!model.snapshot().tab_modified[0]);
}

#[test]
fn scratchpad_tabs_are_path_backed_and_save_without_save_as() {
    let path = std::path::PathBuf::from("/tmp/2026-04-11_12-13-14.md");
    let stamp = lst_editor::FileStamp::from_raw(0, Some(1));
    let mut model = model_with_tabs(
        vec![EditorTab::scratchpad_with_stamp(
            TabId::from_raw(1),
            path.clone(),
            stamp,
        )],
        "Ready.".into(),
    );

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active_path, Some(path.clone()));
    assert_eq!(snapshot.tab_scratchpad, [true]);
    assert!(!snapshot.tab_modified[0]);

    model.insert_text("note".into());
    model.drain_effects();
    model.request_save();

    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::SaveFile {
            tab_id: TabId::from_raw(1),
            path,
            body: "note".into(),
            expected_stamp: Some(stamp),
        }]
    );
}

#[test]
fn save_as_marks_scratchpad_as_normal_file() {
    let scratchpad_path = std::path::PathBuf::from("/tmp/2026-04-11_12-13-14.md");
    let saved_path = std::path::PathBuf::from("/tmp/saved.md");
    let mut model = model_with_tabs(
        vec![EditorTab::scratchpad_with_stamp(
            TabId::from_raw(1),
            scratchpad_path.clone(),
            lst_editor::FileStamp::from_raw(0, Some(1)),
        )],
        "Ready.".into(),
    );

    model.request_save_as();
    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::SaveFileAs {
            tab_id: TabId::from_raw(1),
            suggested_name: "2026-04-11_12-13-14.md".into(),
            body: "".into(),
            previous_scratchpad_path: Some(scratchpad_path),
        }]
    );

    model.save_as_finished_for_tab(
        TabId::from_raw(1),
        saved_path.clone(),
        lst_editor::FileStamp::from_raw(4, Some(2)),
    );

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active_path, Some(saved_path));
    assert_eq!(snapshot.tab_scratchpad, [false]);
    assert!(!snapshot.tab_modified[0]);
}

#[test]
fn autosave_tick_skips_paths_open_in_multiple_tabs() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = model_with_tabs(
        vec![
            EditorTab::from_text(
                TabId::from_raw(1),
                "example.txt".into(),
                Some(path.clone()),
                "hello",
            ),
            EditorTab::from_text(
                TabId::from_raw(2),
                "example.txt".into(),
                Some(path),
                "other",
            ),
        ],
        "Ready.".into(),
    );
    model.insert_text("!".into());
    model.drain_effects();

    model.autosave_tick();

    assert_eq!(model.drain_effects(), Vec::<EditorEffect>::new());
}

#[test]
fn autosave_finished_only_clears_matching_revision() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example.txt".into(),
            Some(path.clone()),
            "hello",
        )],
        "Ready.".into(),
    );
    model.insert_text("!".into());
    let stale_revision = model.snapshot().active_revision;
    model.insert_text("?".into());
    let current_revision = model.snapshot().active_revision;
    model.drain_effects();

    let tab_id = model.active_tab_id();
    model.autosave_finished_for_tab(tab_id, path.clone(), stale_revision, dummy_stamp());
    assert!(model.snapshot().tab_modified[0]);

    model.autosave_finished_for_tab(tab_id, path, current_revision, dummy_stamp());
    assert!(!model.snapshot().tab_modified[0]);
}

#[test]
fn direct_cursor_and_selection_commands_are_model_behavior() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha beta",
        )],
        "Ready.".into(),
    );

    model.move_to_char(6, false, Some(4));
    assert_eq!(model.snapshot().cursor, 6);

    model.move_to_char(10, true, None);
    assert_eq!(model.snapshot().selection, 6..10);

    model.set_selection(0..5, true);
    let snapshot = model.snapshot();
    assert_eq!(snapshot.selection, 0..5);
    assert_eq!(snapshot.cursor, 0);
}

#[test]
fn ime_marked_text_replacement_remains_model_behavior() {
    let mut model = EditorModel::empty();

    model.replace_and_mark_text(None, "a🙂b".into(), Some(1..2));
    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "a🙂b");
    assert_eq!(snapshot.selection, 1..2);

    model.clear_marked_text();
    model.replace_text(None, "Z".into(), UndoBoundary::Break);

    assert_eq!(model.snapshot().text, "aZb");
}

#[test]
fn vim_delete_and_paste_execute_against_real_document() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "alpha\nbeta",
        )],
        "Ready.".into(),
    );
    enter_vim_normal(&mut model);

    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default(), 80);
    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default(), 80);
    assert_eq!(model.snapshot().text, "beta");

    model.handle_vim_key(VimKey::Character("p".into()), VimModifiers::default(), 80);
    assert_eq!(model.snapshot().text, "beta\nalpha");
}

#[test]
fn vim_search_word_under_cursor_updates_find_behaviorally() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );
    enter_vim_normal(&mut model);

    model.handle_vim_key(VimKey::Character("*".into()), VimModifiers::default(), 80);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_query, "one");
    assert_eq!(snapshot.find_matches, 2);
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 8 });
}

#[test]
fn vim_mode_and_pending_are_visible_in_snapshot() {
    let mut model = EditorModel::empty();
    enter_vim_normal(&mut model);

    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default(), 80);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.vim_mode, VimMode::Normal);
    assert_eq!(snapshot.vim_pending, "d");
}

#[test]
fn vim_key_translation_is_framework_neutral_behavior() {
    let mut vim = VimState::new();
    vim.mode = VimMode::Normal;
    let text = VimTextSnapshot {
        lines: vec!["alpha beta".to_string()].into(),
        cursor: Position { line: 0, column: 0 },
    };

    assert_eq!(
        vim.handle_key(
            &VimKey::Character("w".into()),
            VimModifiers::default(),
            &text
        ),
        vec![VimCommand::MoveTo(Position { line: 0, column: 6 })]
    );
    assert_eq!(
        vim.handle_key(
            &VimKey::Named(VimNamedKey::ArrowRight),
            VimModifiers::default(),
            &text,
        ),
        vec![VimCommand::MoveTo(Position { line: 0, column: 1 })]
    );
    assert_eq!(
        vim.handle_key(&VimKey::Character("r".into()), VimModifiers::COMMAND, &text,),
        vec![VimCommand::Redo]
    );
}

#[test]
fn arrow_right_steps_over_combining_acute() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "e\u{0301}f",
        )],
        "Ready.".into(),
    );

    model.move_to_char(0, false, None);
    model.move_horizontal_collapsed(false);

    assert_eq!(model.snapshot().selection, 2..2);
}

#[test]
fn arrow_left_steps_over_emoji() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "a\u{1F1EB}\u{1F1F7}b",
        )],
        "Ready.".into(),
    );

    model.move_to_char(3, false, None);
    model.move_horizontal_collapsed(true);

    assert_eq!(model.snapshot().selection, 1..1);
}

#[test]
fn backspace_removes_full_grapheme() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "Xe\u{0301}Y",
        )],
        "Ready.".into(),
    );

    model.move_to_char(3, false, None);
    model.backspace();

    assert_eq!(model.active_tab().buffer_text(), "XY");
    assert_eq!(model.snapshot().selection, 1..1);
}

#[test]
fn delete_forward_removes_full_emoji() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "X\u{1F1EB}\u{1F1F7}Y",
        )],
        "Ready.".into(),
    );

    model.move_to_char(1, false, None);
    model.delete_forward();

    assert_eq!(model.active_tab().buffer_text(), "XY");
    assert_eq!(model.snapshot().selection, 1..1);
}

#[test]
fn vim_l_steps_over_emoji() {
    let mut vim = VimState::new();
    vim.mode = VimMode::Normal;
    let text = VimTextSnapshot {
        lines: vec!["a\u{1F1EB}\u{1F1F7}b".to_string()].into(),
        cursor: Position { line: 0, column: 1 },
    };

    assert_eq!(
        vim.handle_key(
            &VimKey::Character("l".into()),
            VimModifiers::default(),
            &text,
        ),
        vec![VimCommand::MoveTo(Position { line: 0, column: 3 })]
    );
}

#[test]
fn ctrl_right_steps_over_combining_acute_word_boundary() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "nai\u{0308}ve word",
        )],
        "Ready.".into(),
    );

    model.move_to_char(0, false, None);
    model.move_word(false, false);

    // Cursor lands on the space after the cluster, never inside it (would be 3).
    assert_eq!(model.snapshot().selection, 6..6);
}

#[test]
fn ctrl_right_steps_over_regional_indicator() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "a\u{1F1EB}\u{1F1F7}b cc",
        )],
        "Ready.".into(),
    );

    model.move_to_char(0, false, None);
    model.move_word(false, false);
    let first = model.snapshot().selection;
    model.move_word(false, false);
    let second = model.snapshot().selection;
    model.move_word(false, false);
    let third = model.snapshot().selection;

    // First Ctrl+Right: end of `a`, start of the regional pair.
    assert_eq!(first, 1..1);
    // Second Ctrl+Right skips the entire regional cluster as one Symbol run and
    // lands at the start of `b` — char 3, never the mid-cluster char-2.
    assert_eq!(second, 3..3);
    // Third Ctrl+Right walks past `b` to the space.
    assert_eq!(third, 4..4);
}

#[test]
fn alt_right_subword_steps_over_combining_mark() {
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "nai\u{0308}veCase",
        )],
        "Ready.".into(),
    );

    model.move_to_char(0, false, None);
    model.move_subword(false, false);

    // The subword run is `naïve` (6 chars, 5 graphemes); next subword stops at `C`.
    assert_eq!(model.snapshot().selection, 6..6);
}

#[test]
fn vim_w_steps_over_combining_acute() {
    let mut vim = VimState::new();
    vim.mode = VimMode::Normal;
    let text = VimTextSnapshot {
        lines: vec!["nai\u{0308}ve word".to_string()].into(),
        cursor: Position { line: 0, column: 0 },
    };

    // `w` skips the whole `naïve` cluster run and lands on `w`, never inside
    // the NFD combining mark.
    assert_eq!(
        vim.handle_key(
            &VimKey::Character("w".into()),
            VimModifiers::default(),
            &text,
        ),
        vec![VimCommand::MoveTo(Position { line: 0, column: 7 })]
    );
}

#[test]
fn vim_e_lands_on_full_emoji_cluster_start() {
    let mut vim = VimState::new();
    vim.mode = VimMode::Normal;
    let text = VimTextSnapshot {
        lines: vec!["a\u{1F1EB}\u{1F1F7} b".to_string()].into(),
        cursor: Position { line: 0, column: 0 },
    };

    // `e` lands at the regional cluster's *start* (col 1), since the cursor
    // sits on the cluster as a whole — never at the mid-cluster col 2.
    assert_eq!(
        vim.handle_key(
            &VimKey::Character("e".into()),
            VimModifiers::default(),
            &text,
        ),
        vec![VimCommand::MoveTo(Position { line: 0, column: 1 })]
    );
}

#[test]
fn vim_daw_keeps_emoji_cluster_intact() {
    let mut vim = VimState::new();
    vim.mode = VimMode::Normal;
    // "x 🇫🇷naïve y" with NFD ï. The 🇫🇷 cluster sits at chars 2..4 and the
    // identifier "naïve" at chars 4..10 (5 graphemes). With cursor inside
    // `naïve`, `daw` covers the whole identifier word plus surrounding space.
    let text = VimTextSnapshot {
        lines: vec!["x \u{1F1EB}\u{1F1F7}nai\u{0308}ve y".to_string()].into(),
        cursor: Position { line: 0, column: 5 },
    };

    // `d` and `a` set the operator + text-object pending; emit Noop until the
    // motion target arrives.
    vim.handle_key(
        &VimKey::Character("d".into()),
        VimModifiers::default(),
        &text,
    );
    vim.handle_key(
        &VimKey::Character("a".into()),
        VimModifiers::default(),
        &text,
    );
    let cmds = vim.handle_key(
        &VimKey::Character("w".into()),
        VimModifiers::default(),
        &text,
    );

    let Some(VimCommand::DeleteRange { from, to }) = cmds.iter().find_map(|c| {
        if matches!(c, VimCommand::DeleteRange { .. }) {
            Some(c.clone())
        } else {
            None
        }
    }) else {
        panic!("expected DeleteRange, got {:?}", cmds);
    };

    // Both endpoints land on cluster starts (no mid-cluster char-3 of the
    // regional pair, no mid-cluster char-7 of the NFD ï).
    assert_ne!(from.column, 3);
    assert_ne!(to.column, 3);
    assert_ne!(from.column, 7);
    assert_ne!(to.column, 7);
    // Concretely: `daw` with cursor in `naïve` deletes `naïve` plus the
    // trailing space — the regional cluster is a separate word and stays.
    assert_eq!(from, Position { line: 0, column: 4 });
    assert_eq!(
        to,
        Position {
            line: 0,
            column: 10
        }
    );
}

#[test]
fn vim_star_extracts_full_grapheme_word() {
    let mut vim = VimState::new();
    vim.mode = VimMode::Normal;
    let text = VimTextSnapshot {
        lines: vec!["nai\u{0308}ve nai\u{0308}ve".to_string()].into(),
        cursor: Position { line: 0, column: 0 },
    };

    let cmds = vim.handle_key(
        &VimKey::Character("*".into()),
        VimModifiers::default(),
        &text,
    );

    let (word, forward) = cmds
        .iter()
        .find_map(|c| match c {
            VimCommand::SearchWordUnderCursor { word, forward } => Some((word.clone(), *forward)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected SearchWordUnderCursor, got {:?}", cmds));

    assert!(forward);
    // The extracted pattern must be the full NFD `naïve`, not `na` (which
    // would happen if extraction stopped at the combining mark).
    assert_eq!(word, "nai\u{0308}ve");
}

#[test]
fn vim_de_removes_trailing_combining_mark_with_word() {
    // "cafe\u{0301}" — NFD café. Without grapheme-aware deletion, `de` at col 0
    // would remove only "cafe" via `to.column + 1`, leaving the orphan
    // combining mark.
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "cafe\u{0301}",
        )],
        "Ready.".into(),
    );

    enter_vim_normal(&mut model);
    model.move_to_char(0, false, None);
    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default(), 80);
    model.handle_vim_key(VimKey::Character("e".into()), VimModifiers::default(), 80);

    assert_eq!(model.active_tab().buffer_text(), "");
}

#[test]
fn vim_escape_from_insert_lands_on_cluster_start() {
    // Cursor in Insert at the past-EOL column of "cafe\u{0301}" (ll = 5). Vim
    // moves left by 1 on Escape; without grapheme awareness that lands at col
    // 4 (the combining mark), mid-cluster. The fix lands at col 3 (start of é).
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "cafe\u{0301}",
        )],
        "Ready.".into(),
    );

    model.move_to_char(5, false, None);
    enter_vim_normal(&mut model);

    assert_eq!(model.snapshot().cursor, 3);
}

#[test]
fn vim_r_replaces_full_grapheme_cluster() {
    // "cafe\u{0301}" with cursor on the é NFD cluster (col 3). Vim `r X` must
    // replace the whole cluster with `X`, not just the base `e`.
    let mut model = model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "cafe\u{0301}",
        )],
        "Ready.".into(),
    );

    enter_vim_normal(&mut model);
    model.move_to_char(3, false, None);
    model.handle_vim_key(VimKey::Character("r".into()), VimModifiers::default(), 80);
    model.handle_vim_key(VimKey::Character("X".into()), VimModifiers::default(), 80);

    assert_eq!(model.active_tab().buffer_text(), "cafX");
}

#[test]
fn double_click_on_combining_mark_selects_full_grapheme() {
    use lst_editor::selection::word_range_at_char;
    use ropey::Rope;

    let buffer = Rope::from_str("nai\u{0308}ve word");

    // Click on the base `i`, the combining mark, and the trailing `e` — all
    // produce the same `naïve` token range, with the cluster intact.
    assert_eq!(word_range_at_char(&buffer, 2), 0..6);
    assert_eq!(word_range_at_char(&buffer, 3), 0..6);
    assert_eq!(word_range_at_char(&buffer, 5), 0..6);
}
