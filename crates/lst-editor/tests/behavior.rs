use lst_editor::{
    EditorCommand as Command, EditorEffect, EditorModel, EditorTab, FileStamp, Selection,
    SelectionSet, TabId, UndoBoundary,
};
use std::path::PathBuf;

mod common;
use common::model_with_tabs;

fn model_with_text(text: &str) -> EditorModel {
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
fn text_edit_undo_redo_round_trips_through_public_model() {
    let mut model = EditorModel::empty();

    model.insert_text("abc".into());
    model.replace_text(Some(3..3), "def".into(), UndoBoundary::Merge);

    assert_eq!(model.snapshot().text, "abcdef");
    model.execute(Command::Undo);
    assert_eq!(model.snapshot().text, "");
    model.execute(Command::Redo);
    assert_eq!(model.snapshot().text, "abcdef");
}

#[test]
fn multi_cursor_insert_is_one_public_model_transaction() {
    let mut model = model_with_text("alpha\nbeta");
    let second_line = model.active_tab().buffer().line_to_char(1);
    let set = SelectionSet::from_selections(
        vec![Selection::collapsed(0), Selection::collapsed(second_line)],
        1,
    )
    .expect("valid multi-cursor set");
    model.set_selection_set(set);

    model.insert_text("> ".into());

    assert_eq!(model.snapshot().text, "> alpha\n> beta");
    model.execute(Command::Undo);
    assert_eq!(model.snapshot().text, "alpha\nbeta");
}

#[test]
fn multi_cursor_newline_places_each_cursor_at_inherited_indent() {
    let mut model = model_with_text("    alpha!\n        be");
    let set =
        SelectionSet::from_selections(vec![Selection::collapsed(10), Selection::collapsed(21)], 1)
            .expect("valid multi-cursor set");
    model.set_selection_set(set);

    model.execute(Command::InsertNewline);

    assert_eq!(
        model.snapshot().text,
        "    alpha!\n    \n        be\n        "
    );
    let snapshot = model.snapshot();
    let head_cols: Vec<usize> = snapshot
        .selection_set
        .as_slice()
        .iter()
        .map(|selection| char_column(&snapshot.text, selection.head()))
        .collect();
    assert_eq!(head_cols, vec![4, 8]);
}

fn char_column(text: &str, offset: usize) -> usize {
    let mut column = 0;
    for (index, ch) in text.chars().enumerate() {
        if index == offset {
            break;
        }
        if ch == '\n' {
            column = 0;
        } else {
            column += 1;
        }
    }
    column
}

#[test]
fn find_replace_changes_observable_document_text() {
    let mut model = model_with_text("one two one");
    model.update_find_query("one".into());
    model.update_find_replacement("three".into());

    model.execute(Command::ReplaceAllMatches);

    assert_eq!(model.snapshot().text, "three two three");
}

#[test]
fn grapheme_backspace_deletes_the_whole_cluster() {
    let mut model = model_with_text("e\u{301}🙂");
    model.set_selection(Selection::collapsed(2));

    model.execute(Command::Backspace);

    assert_eq!(model.snapshot().text, "🙂");
    assert_eq!(model.selection().cursor(), 0);
}

#[test]
fn public_edit_ranges_expand_to_grapheme_boundaries() {
    let mut model = model_with_text("e\u{301}x");

    model.replace_text(Some(1..1), "A".into(), UndoBoundary::Break);

    assert_eq!(model.snapshot().text, "Ax");
}

#[test]
fn regex_replace_all_expands_capture_groups_once_per_match() {
    let mut model = model_with_text("alpha-one beta-two");
    model.update_find_query(r"(\w+)-(\w+)".into());
    model.execute(Command::ToggleFindRegex);
    model.update_find_replacement("$2:$1".into());

    model.execute(Command::ReplaceAllMatches);

    assert_eq!(model.snapshot().text, "one:alpha two:beta");
}

#[test]
fn invalid_regex_replace_all_is_a_noop() {
    let mut model = model_with_text("[abc] [def]");
    model.update_find_query("[".into());
    model.execute(Command::ToggleFindRegex);
    model.update_find_replacement("x".into());

    model.execute(Command::ReplaceAllMatches);

    assert_eq!(model.snapshot().text, "[abc] [def]");
    assert!(model.find().error.is_some());
}

#[test]
fn whole_word_replace_all_skips_identifier_substrings() {
    let mut model = model_with_text("foobar foo_bar foo bar");
    model.update_find_query("foo".into());
    model.execute(Command::ToggleFindWholeWord);
    model.update_find_replacement("baz".into());

    model.execute(Command::ReplaceAllMatches);

    assert_eq!(model.snapshot().text, "foobar foo_bar baz bar");
}

#[test]
fn case_sensitive_replace_all_disables_smart_case() {
    let mut model = model_with_text("Foo foo FOO");
    model.update_find_query("foo".into());
    model.execute(Command::ToggleFindCaseSensitive);
    model.update_find_replacement("bar".into());

    model.execute(Command::ReplaceAllMatches);

    assert_eq!(model.snapshot().text, "Foo bar FOO");
}

#[test]
fn tab_reorder_preserves_active_tab_identity() {
    let mut model = model_with_tabs(
        vec![
            EditorTab::from_text(TabId::from_raw(1), "one".into(), None, "1"),
            EditorTab::from_text(TabId::from_raw(2), "two".into(), None, "2"),
            EditorTab::from_text(TabId::from_raw(3), "three".into(), None, "3"),
        ],
        "Ready.".into(),
    );
    model.set_active_tab(TabId::from_raw(3));

    model.move_tab(0, 2);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.tab_titles, ["two", "three", "one"]);
    assert_eq!(snapshot.active, 1);
    assert_eq!(model.active_tab_id(), TabId::from_raw(3));
}

#[test]
fn selection_copy_emits_clipboard_boundary_effects() {
    let mut model = model_with_text("hello");
    model.set_selection(Selection::from_range(0..5, false));
    let _ = model.drain_effects();

    model.execute(Command::CopySelection);

    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("hello".into()),
            EditorEffect::WritePrimary("hello".into()),
        ]
    );
}

#[test]
fn stale_manual_save_completion_does_not_clear_newer_edits() {
    let path = PathBuf::from("/tmp/lst-stale-save.txt");
    let stamp = FileStamp::from_raw(3, Some(1));
    let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), path.clone(), "old", Some(stamp));
    let mut model = EditorModel::from_tab(tab, "Ready.".into());

    model.replace_text(Some(3..3), " saved".into(), UndoBoundary::Break);
    let save_revision = model.active_tab().revision();
    let _ = model.drain_effects();
    model.request_save_tab(TabId::from_raw(1));
    let effects = model.drain_effects();
    assert!(
        matches!(
            effects.as_slice(),
            [EditorEffect::SaveFile { revision, .. }] if *revision == save_revision
        ),
        "{effects:?}"
    );

    model.replace_text(Some(0..0), "newer ".into(), UndoBoundary::Break);
    model.save_finished_for_tab(
        TabId::from_raw(1),
        path,
        save_revision,
        FileStamp::from_raw(9, Some(2)),
        "old saved".into(),
    );

    assert_eq!(model.snapshot().text, "newer old saved");
    assert!(model.active_tab().modified());
}

#[test]
fn typing_after_moving_cursor_starts_a_new_undo_group() {
    let mut model = model_with_text("");

    model.replace_text_from_input(None, "a".into());
    model.execute(Command::MoveHorizontal(-1, false));
    model.replace_text_from_input(None, "b".into());

    assert_eq!(model.snapshot().text, "ba");
    model.execute(Command::Undo);
    assert_eq!(model.snapshot().text, "a");
}

#[test]
fn undoing_back_to_the_saved_snapshot_clears_dirty_state() {
    let path = PathBuf::from("/tmp/lst-clean-undo.txt");
    let stamp = FileStamp::from_raw(3, Some(1));
    let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), path, "old", Some(stamp));
    let mut model = EditorModel::from_tab(tab, "Ready.".into());

    model.replace_text(Some(3..3), " dirty".into(), UndoBoundary::Break);
    assert!(model.active_tab().modified());

    model.execute(Command::Undo);

    assert_eq!(model.snapshot().text, "old");
    assert!(!model.active_tab().modified());
}

#[test]
fn undoing_away_from_newly_saved_text_marks_buffer_dirty() {
    let path = PathBuf::from("/tmp/lst-dirty-after-save-undo.txt");
    let stamp = FileStamp::from_raw(3, Some(1));
    let tab = EditorTab::from_path_with_stamp(TabId::from_raw(1), path.clone(), "old", Some(stamp));
    let mut model = EditorModel::from_tab(tab, "Ready.".into());

    model.replace_text(Some(3..3), " saved".into(), UndoBoundary::Break);
    let save_revision = model.active_tab().revision();
    assert!(model.save_finished_for_tab(
        TabId::from_raw(1),
        path,
        save_revision,
        FileStamp::from_raw(9, Some(2)),
        "old saved".into()
    ));
    assert!(!model.active_tab().modified());

    model.execute(Command::Undo);

    assert_eq!(model.snapshot().text, "old");
    assert!(model.active_tab().modified());
}

#[test]
fn crlf_line_ending_is_one_horizontal_delete_boundary() {
    let mut model = model_with_text("a\r\nb");

    model.set_selection(Selection::collapsed(1));
    model.execute(Command::MoveHorizontal(1, false));
    assert_eq!(model.selection().cursor(), 3);

    model.execute(Command::Backspace);
    assert_eq!(model.snapshot().text, "ab");
}

#[test]
fn multi_selection_move_line_up_at_document_boundary_is_a_noop() {
    let mut model = model_with_text("a\nb\nc\n");
    let set =
        SelectionSet::from_selections(vec![Selection::collapsed(0), Selection::collapsed(4)], 1)
            .expect("valid multi-cursor set");
    model.set_selection_set(set);

    model.execute(Command::MoveLineUp);

    assert_eq!(model.snapshot().text, "a\nb\nc\n");
    assert_eq!(model.selection_set().as_slice().len(), 2);
}

#[test]
fn moving_a_full_line_selection_down_keeps_the_moved_line_selected() {
    let mut model = model_with_text("a\nb\nc\n");
    model.set_selection(Selection::from_range(0..2, false));

    model.execute(Command::MoveLineDown);

    assert_eq!(model.snapshot().text, "b\na\nc\n");
    assert_eq!(model.selection().range(), 2..4);
}

#[test]
fn insert_tab_indents_each_line_touched_by_multiple_selections() {
    let mut model = model_with_text("a\nb\nc");
    let set = SelectionSet::from_selections(
        vec![
            Selection::from_range(0..1, false),
            Selection::from_range(4..5, false),
        ],
        1,
    )
    .expect("valid multi-cursor set");
    model.set_selection_set(set);

    model.execute(Command::InsertTab);

    assert_eq!(model.snapshot().text, "    a\nb\n    c");
    assert_eq!(model.selection_set().as_slice().len(), 2);
}

#[test]
fn model_construction_repairs_duplicate_tab_ids() {
    let first = EditorTab::from_text(TabId::from_raw(1), "one".into(), None, "1");
    let second = EditorTab::from_text(TabId::from_raw(1), "two".into(), None, "2");

    let model = EditorModel::from_tabs(first, vec![second], "Ready.".into());

    let ids = model
        .tabs()
        .iter()
        .map(|tab| tab.id().get())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn public_selection_offsets_are_normalized_to_grapheme_boundaries() {
    let mut model = model_with_text("e\u{301}x");

    model.set_selection(Selection::collapsed(1));
    assert_eq!(model.selection().cursor(), 0);

    model.add_selection_range(1..2, false);
    let ranges = model
        .selection_set()
        .as_slice()
        .iter()
        .map(Selection::range)
        .collect::<Vec<_>>();
    assert_eq!(ranges, vec![0..0, 0..2]);
}

#[test]
fn public_selection_set_offsets_are_coalesced_after_grapheme_normalization() {
    let mut model = model_with_text("e\u{301}x");
    let set = SelectionSet::from_selections(
        vec![
            Selection::from_range(0..1, false),
            Selection::from_range(1..2, false),
        ],
        1,
    )
    .expect("adjacent public selections are valid");

    model.set_selection_set(set);

    let ranges = model
        .selection_set()
        .as_slice()
        .iter()
        .map(Selection::range)
        .collect::<Vec<_>>();
    assert_eq!(ranges, vec![0..2]);
}

#[test]
fn bookmarks_shift_with_inserted_lines_before_them() {
    let mut model = model_with_text("a\nb\nc");
    model.set_selection(Selection::collapsed(2));
    model.execute(Command::ToggleBookmark);
    assert_eq!(model.active_tab().bookmarks(), &[1]);

    model.set_selection(Selection::collapsed(0));
    model.insert_text("new\n".into());

    assert_eq!(model.active_tab().bookmarks(), &[2]);
}

#[test]
fn bookmarks_follow_their_line_when_inserting_at_line_start() {
    let mut model = model_with_text("a\nb\nc");
    model.set_selection(Selection::collapsed(2));
    model.execute(Command::ToggleBookmark);

    model.insert_text("new\n".into());

    assert_eq!(model.active_tab().bookmarks(), &[2]);
}

#[test]
fn undo_restores_bookmarks_with_text_snapshot() {
    let mut model = model_with_text("a\nb\nc");
    model.set_selection(Selection::collapsed(2));
    model.execute(Command::ToggleBookmark);
    model.set_selection(Selection::collapsed(0));
    model.insert_text("new\n".into());
    assert_eq!(model.active_tab().bookmarks(), &[2]);

    model.execute(Command::Undo);

    assert_eq!(model.snapshot().text, "a\nb\nc");
    assert_eq!(model.active_tab().bookmarks(), &[1]);
}
