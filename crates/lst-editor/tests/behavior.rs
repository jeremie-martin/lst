use lst_editor::{
    EditorEffect, EditorModel, EditorTab, Selection, SelectionSet, TabId, UndoBoundary,
};

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
    model.undo();
    assert_eq!(model.snapshot().text, "");
    model.redo();
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
    model.undo();
    assert_eq!(model.snapshot().text, "alpha\nbeta");
}

#[test]
fn find_replace_changes_observable_document_text() {
    let mut model = model_with_text("one two one");
    model.update_find_query("one".into());
    model.update_find_replacement("three".into());

    model.replace_all_matches_in_document();

    assert_eq!(model.snapshot().text, "three two three");
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

    model.copy_selection();

    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("hello".into()),
            EditorEffect::WritePrimary("hello".into()),
        ]
    );
}
