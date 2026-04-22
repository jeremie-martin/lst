use lst_editor::position::Position;
use lst_editor::{
    vim::{
        Key as VimKey, Mode as VimMode, Modifiers as VimModifiers, NamedKey as VimNamedKey,
        TextSnapshot as VimTextSnapshot, VimCommand, VimState,
    },
    EditorEffect, EditorModel, EditorTab, FocusTarget, RevealIntent, TabId, UndoBoundary,
};

fn enter_vim_normal(model: &mut EditorModel) {
    model.handle_vim_escape();
    let _ = model.drain_effects();
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
fn goto_line_submit_clamps_to_existing_lines() {
    let mut model = EditorModel::new(
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
fn closing_active_tab_preserves_neighbor_as_active() {
    let mut model = EditorModel::empty();
    model.new_tab();
    model.new_tab();
    assert_eq!(model.snapshot().active, 2);

    model.close_tab(2);

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

    model.close_tab(1);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 0);
    assert_eq!(snapshot.tab_count, 2);
    assert_eq!(model.drain_effects(), Vec::<EditorEffect>::new());
}

#[test]
fn movement_and_selection_are_behavioral_commands() {
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut top = EditorModel::new(
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

    let mut bottom = EditorModel::new(
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
    let mut top = EditorModel::new(
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

    let mut bottom = EditorModel::new(
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
    let mut top = EditorModel::new(
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

    let mut bottom = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut forward = EditorModel::new(
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

    let mut reversed = EditorModel::new(
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
    let mut model = EditorModel::new(
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

#[test]
fn horizontal_collapse_commands_collapse_active_selection() {
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut top = EditorModel::new(
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

    let mut bottom = EditorModel::new(
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
    let mut top = EditorModel::new(
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

    let mut bottom = EditorModel::new(
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
    let mut model = EditorModel::new(
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
fn delete_word_and_line_ops_are_undoable_document_behavior() {
    let mut model = EditorModel::new(
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
fn delete_word_commands_delete_active_selection_before_word_boundaries() {
    let mut model = EditorModel::new(
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

    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
fn file_commands_emit_runtime_effects_and_apply_results() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = EditorModel::new(
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

    model.save_finished(path.clone());
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(vec![first, second], "Ready.".into());

    model.save_finished_for_tab(
        TabId::from_raw(2),
        second_path,
        Some(lst_editor::FileStamp::from_raw(10, Some(20))),
    );

    assert_eq!(model.snapshot().tab_modified, [true, false]);
}

#[test]
fn dirty_tab_close_requires_confirmation_and_discard_can_close_it() {
    let mut model = EditorModel::empty();
    let tab_id = model.active_tab().id();
    model.insert_text("unsaved".into());

    assert!(matches!(
        model.close_request_for_tab(0),
        Some(lst_editor::TabCloseRequest::Unsaved(tab)) if tab.tab_id == tab_id
    ));

    assert!(model.discard_close_tab_by_id(tab_id));
    let snapshot = model.snapshot();
    assert_eq!(snapshot.tab_count, 1);
    assert_eq!(snapshot.text, "");
    assert!(!snapshot.tab_modified[0]);
}

#[test]
fn autosave_tick_emits_modified_file_backed_tabs() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = EditorModel::new(
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

    model.autosave_finished(path, revision);
    assert!(!model.snapshot().tab_modified[0]);
}

#[test]
fn scratchpad_tabs_are_path_backed_and_save_without_save_as() {
    let path = std::path::PathBuf::from("/tmp/2026-04-11_12-13-14.md");
    let stamp = lst_editor::FileStamp::from_raw(0, Some(1));
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
        Some(lst_editor::FileStamp::from_raw(4, Some(2))),
    );

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active_path, Some(saved_path));
    assert_eq!(snapshot.tab_scratchpad, [false]);
    assert!(!snapshot.tab_modified[0]);
}

#[test]
fn autosave_tick_skips_paths_open_in_multiple_tabs() {
    let path = std::path::PathBuf::from("/tmp/example.txt");
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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

    model.autosave_finished(path.clone(), stale_revision);
    assert!(model.snapshot().tab_modified[0]);

    model.autosave_finished(path, current_revision);
    assert!(!model.snapshot().tab_modified[0]);
}

#[test]
fn direct_cursor_and_selection_commands_are_model_behavior() {
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
    let mut model = EditorModel::new(
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
