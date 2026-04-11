use lst_core::document::{EditKind, UndoBoundary};
use lst_core::position::Position;
use lst_editor::{
    vim::{
        Key as VimKey, Mode as VimMode, Modifiers as VimModifiers, NamedKey as VimNamedKey,
        TextSnapshot as VimTextSnapshot, VimCommand, VimState,
    },
    EditorCommand, EditorEffect, EditorModel, EditorTab, FocusTarget, TabId,
};

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

    model.apply(EditorCommand::MoveDocumentBoundary {
        to_end: true,
        select: false,
    });
    assert_eq!(model.snapshot().cursor, "alpha beta\ngamma".chars().count());

    model.apply(EditorCommand::MoveWord {
        backward: true,
        select: true,
    });
    assert_eq!(model.snapshot().selection, 11..16);
    assert!(model.drain_effects().contains(&EditorEffect::RevealCursor));
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

    model.apply(EditorCommand::MoveWord {
        backward: false,
        select: false,
    });
    model.apply(EditorCommand::DeleteWord { backward: false });
    assert_eq!(model.snapshot().text, "alpha\ngamma");

    model.apply(EditorCommand::DuplicateLine);
    assert_eq!(model.snapshot().text, "alpha\nalpha\ngamma");

    model.apply(EditorCommand::Undo);
    model.apply(EditorCommand::Undo);
    assert_eq!(model.snapshot().text, "alpha beta\ngamma");
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
    model.active_tab_mut().selection = 0..5;

    model.apply(EditorCommand::CopySelection);
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("hello".into()),
            EditorEffect::WritePrimary("hello".into())
        ]
    );

    model.apply(EditorCommand::CutSelection);
    assert_eq!(model.snapshot().text, " world");
    assert_eq!(
        model.drain_effects(),
        vec![
            EditorEffect::WriteClipboard("hello".into()),
            EditorEffect::WritePrimary("hello".into()),
            EditorEffect::RevealCursor
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
    model.apply(EditorCommand::InsertText(" world".into()));
    model.drain_effects();

    model.apply(EditorCommand::RequestSave);
    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::SaveFile {
            path: path.clone(),
            body: " worldhello".into()
        }]
    );

    model.apply(EditorCommand::SaveFinished { path: path.clone() });
    let snapshot = model.snapshot();
    assert!(!snapshot.tab_modified[0]);
    assert_eq!(snapshot.status, format!("Saved {}.", path.display()));
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
    model.apply(EditorCommand::InsertText("!".into()));
    let revision = model.snapshot().active_revision;
    model.drain_effects();

    model.apply(EditorCommand::AutosaveTick);

    assert_eq!(
        model.drain_effects(),
        vec![EditorEffect::AutosaveFile {
            path: path.clone(),
            body: "!hello".into(),
            revision,
        }]
    );

    model.apply(EditorCommand::AutosaveFinished { path, revision });
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

    model.apply(EditorCommand::MoveToChar {
        offset: 6,
        select: false,
        preferred_column: Some(4),
    });
    assert_eq!(model.snapshot().cursor, 6);

    model.apply(EditorCommand::MoveToChar {
        offset: 10,
        select: true,
        preferred_column: None,
    });
    assert_eq!(model.snapshot().selection, 6..10);

    model.apply(EditorCommand::SetSelection {
        range: 0..5,
        reversed: true,
    });
    let snapshot = model.snapshot();
    assert_eq!(snapshot.selection, 0..5);
    assert_eq!(snapshot.cursor, 0);
}

#[test]
fn ime_marked_text_replacement_remains_model_behavior() {
    let mut model = EditorModel::empty();

    model.apply(EditorCommand::ReplaceAndMarkText {
        range: None,
        text: "a🙂b".into(),
        selected_range: Some(1..2),
    });
    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "a🙂b");
    assert_eq!(snapshot.selection, 1..2);

    model.apply(EditorCommand::ClearMarkedText);
    model.apply(EditorCommand::ReplaceText {
        range: None,
        text: "Z".into(),
        boundary: UndoBoundary::Break,
    });

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
    model.vim.mode = VimMode::Normal;

    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default());
    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default());
    assert_eq!(model.snapshot().text, "beta");

    model.handle_vim_key(VimKey::Character("p".into()), VimModifiers::default());
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
    model.vim.mode = VimMode::Normal;

    model.handle_vim_key(VimKey::Character("*".into()), VimModifiers::default());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.find_query, "one");
    assert_eq!(snapshot.find_matches, 2);
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 8 });
}

#[test]
fn vim_mode_and_pending_are_visible_in_snapshot() {
    let mut model = EditorModel::empty();
    model.vim.mode = VimMode::Normal;

    model.handle_vim_key(VimKey::Character("d".into()), VimModifiers::default());

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
