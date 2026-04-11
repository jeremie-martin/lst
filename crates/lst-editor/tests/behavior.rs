use lst_core::document::{EditKind, UndoBoundary};
use lst_editor::{EditorCommand, EditorEffect, EditorModel, EditorTab, FocusTarget, TabId};

#[test]
fn new_tab_switches_active_with_stable_tab_identity() {
    let mut model = EditorModel::empty();
    let first = model.active_tab().id();
    model.apply(EditorCommand::NewTab);

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

    model.active_tab_mut().selection = 0..3;
    model.apply(EditorCommand::OpenFind {
        show_replace: false,
    });

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

    model.apply(EditorCommand::InsertText("abc".into()));
    model
        .active_tab_mut()
        .edit(EditKind::Insert, UndoBoundary::Merge, 3..3, "def");

    assert_eq!(model.snapshot().text, "abcdef");
    model.apply(EditorCommand::Undo);
    assert_eq!(model.snapshot().text, "");
    model.apply(EditorCommand::Redo);
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

    model.apply(EditorCommand::OpenFind {
        show_replace: false,
    });
    model.apply(EditorCommand::SetFindQuery("one".into()));

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

    model.apply(EditorCommand::SetFindQuery("one".into()));
    assert_eq!(model.snapshot().find_matches, 2);

    model.apply(EditorCommand::SetActiveTab(1));

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 1);
    assert_eq!(snapshot.find_matches, 0);
}

#[test]
fn opening_find_only_after_replace_clears_replace_mode() {
    let mut model = EditorModel::empty();

    model.apply(EditorCommand::OpenFind { show_replace: true });
    assert!(model.snapshot().find_show_replace);

    model.apply(EditorCommand::OpenFind {
        show_replace: false,
    });

    let snapshot = model.snapshot();
    assert!(snapshot.find_visible);
    assert!(!snapshot.find_show_replace);
}

#[test]
fn find_next_selects_the_next_observable_match() {
    let mut model = EditorModel::new(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "example".into(),
            None,
            "one two one",
        )],
        "Ready.".into(),
    );

    model.apply(EditorCommand::SetFindQuery("one".into()));
    model.apply(EditorCommand::FindNext);

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_matches, 2);
    assert_eq!(snapshot.selection, 8..11);
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

    model.apply(EditorCommand::SetFindQuery("one".into()));
    model.apply(EditorCommand::SetFindReplacement("three".into()));
    model.apply(EditorCommand::ReplaceAll);

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

    model.apply(EditorCommand::SetFindQuery("one".into()));
    model.apply(EditorCommand::SetFindReplacement("three".into()));
    model.apply(EditorCommand::ReplaceAll);
    assert_eq!(model.snapshot().text, "three two three");

    model.apply(EditorCommand::Undo);

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

    model.apply(EditorCommand::SetFindQuery("a".into()));
    assert_eq!(model.snapshot().find_matches, 1);

    model.apply(EditorCommand::InsertText("a".into()));

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

    model.apply(EditorCommand::OpenGotoLine);
    model.apply(EditorCommand::SetGotoLine("99".into()));
    model.apply(EditorCommand::SubmitGotoLine);

    assert_eq!(model.snapshot().cursor, "alpha\nbeta\n".chars().count());
}

#[test]
fn closing_active_tab_preserves_neighbor_as_active() {
    let mut model = EditorModel::empty();
    model.apply(EditorCommand::NewTab);
    model.apply(EditorCommand::NewTab);
    assert_eq!(model.snapshot().active, 2);

    model.apply(EditorCommand::CloseTab(2));

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
    model.apply(EditorCommand::NewTab);
    model.apply(EditorCommand::NewTab);
    model.apply(EditorCommand::SetActiveTab(0));
    model.drain_effects();

    model.apply(EditorCommand::CloseTab(1));

    let snapshot = model.snapshot();
    assert_eq!(snapshot.active, 0);
    assert_eq!(snapshot.tab_count, 2);
    assert_eq!(model.drain_effects(), Vec::<EditorEffect>::new());
}
