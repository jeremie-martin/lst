//! Editor-side guarantees consumed by the LLM cleanup feature in lst-gpui.
//!
//! The cleanup action runs an LLM round-trip in the app layer and then calls
//! `model.replace_text(Some(range), cleaned, UndoBoundary::Break)`. These
//! tests pin the contract that one `undo()` restores the original text
//! byte-for-byte, for both the whole-buffer and selection paths.

use lst_editor::{EditorModel, EditorTab, Selection, TabId, UndoBoundary};

mod common;
use common::model_with_tabs;

fn model_with_text(text: &str) -> EditorModel {
    model_with_tabs(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "scratch".into(),
            None,
            text,
        )],
        "Ready.".into(),
    )
}

#[test]
fn whole_buffer_replacement_is_atomic_undo() {
    let original = "um, so I think we, you know, should ship it";
    let cleaned = "I think we should ship it.";

    let mut model = model_with_text(original);
    let len = model.active_tab().buffer().len_chars();

    model.replace_text(Some(0..len), cleaned.into(), UndoBoundary::Break);
    assert_eq!(model.active_tab().buffer_text(), cleaned);

    model.undo();
    assert_eq!(model.active_tab().buffer_text(), original);
}

#[test]
fn selection_range_replacement_is_atomic_undo() {
    let original = "intro\num, so I think we should ship it\noutro";
    let selection_start = "intro\n".chars().count();
    let selection_end = original.chars().count() - "\noutro".chars().count();
    let cleaned_paragraph = "I think we should ship it.";

    let mut model = model_with_text(original);
    model.set_selection(Selection::from_range(selection_start..selection_end, false));

    model.replace_text(
        Some(selection_start..selection_end),
        cleaned_paragraph.into(),
        UndoBoundary::Break,
    );
    let expected = format!("intro\n{cleaned_paragraph}\noutro");
    assert_eq!(model.active_tab().buffer_text(), expected);

    model.undo();
    assert_eq!(model.active_tab().buffer_text(), original);
}

#[test]
fn break_boundary_separates_cleanup_from_prior_typing() {
    // Simulates: user types, then triggers cleanup. The two edits must be
    // distinct undo steps so undoing the cleanup doesn't also rewind typing.
    let mut model = model_with_text("");
    model.replace_text(None, "first ".into(), UndoBoundary::Break);
    model.replace_text(None, "draft".into(), UndoBoundary::Break);
    assert_eq!(model.active_tab().buffer_text(), "first draft");

    let len = model.active_tab().buffer().len_chars();
    model.replace_text(Some(0..len), "First Draft".into(), UndoBoundary::Break);
    assert_eq!(model.active_tab().buffer_text(), "First Draft");

    model.undo();
    assert_eq!(model.active_tab().buffer_text(), "first draft");
}
