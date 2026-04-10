use gpui::Keystroke;
use lst_ui::COLOR_MUTED;

use crate::viewport::PaintedRow;
use crate::*;

fn has_binding<A: gpui::Action + 'static>(keystroke: &str) -> bool {
    let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
    editor_keybindings().iter().any(|binding| {
        binding.match_keystrokes(&typed) == Some(false) && binding.action().as_any().is::<A>()
    })
}

fn has_binding_in_context<A: gpui::Action + 'static>(keystroke: &str, context: &str) -> bool {
    let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
    editor_keybindings().iter().any(|binding| {
        binding.match_keystrokes(&typed) == Some(false)
            && binding.action().as_any().is::<A>()
            && binding
                .predicate()
                .as_ref()
                .map(ToString::to_string)
                .as_deref()
                == Some(context)
    })
}

#[test]
fn autosave_revision_requires_a_unique_matching_tab() {
    let path = PathBuf::from("/tmp/example.rs");
    let tab = EditorTab::from_path(path.clone(), "fn main() {}\n");

    assert!(autosave_revision_is_current(&[tab], &path, 0));

    let mut stale_tab = EditorTab::from_path(path.clone(), "fn main() {}\n");
    stale_tab.replace_char_range(0..0, "// ");
    assert!(!autosave_revision_is_current(&[stale_tab], &path, 0));

    let first = EditorTab::from_path(path.clone(), "one\n");
    let second = EditorTab::from_path(path.clone(), "two\n");
    assert!(!autosave_revision_is_current(&[first, second], &path, 0));
}

#[test]
fn rust_highlighting_keeps_multiline_comment_context() {
    let lines = compute_rust_highlights("/* first line\nsecond line */\nlet x = 1;\n");

    assert!(lines[0].iter().any(|span| span.color == COLOR_MUTED));
    assert!(lines[1].iter().any(|span| span.color == COLOR_MUTED));
    assert!(lines[2].iter().all(|span| span.color != COLOR_MUTED));
}

#[test]
fn drag_selection_range_extends_forward_from_anchor_token() {
    let (selection, reversed) = drag_selection_range(6..11, 12..17);

    assert_eq!(selection, 6..17);
    assert!(!reversed);
}

#[test]
fn drag_selection_range_extends_backward_from_anchor_token() {
    let (selection, reversed) = drag_selection_range(6..11, 0..5);

    assert_eq!(selection, 0..11);
    assert!(reversed);
}

#[test]
fn ctrl_arrow_aliases_expand_vertical_selection() {
    assert!(has_binding::<SelectUp>("ctrl-up"));
    assert!(has_binding::<SelectDown>("ctrl-down"));
}

#[test]
fn find_shortcuts_stay_available_from_workspace_context() {
    assert!(has_binding_in_context::<FindOpen>("ctrl-f", "Workspace"));
    assert!(has_binding_in_context::<FindOpenReplace>(
        "ctrl-h",
        "Workspace"
    ));
    assert!(has_binding_in_context::<FindNext>("f3", "Workspace"));
    assert!(has_binding_in_context::<FindPrev>("shift-f3", "Workspace"));
    assert!(has_binding_in_context::<GotoLineOpen>(
        "ctrl-g",
        "Workspace"
    ));
}

#[test]
fn closing_other_tab_does_not_force_editor_focus() {
    assert!(!should_refocus_editor_after_tab_close(2, 1));
    assert!(!should_refocus_editor_after_tab_close(2, 3));
    assert!(should_refocus_editor_after_tab_close(2, 2));
}

#[test]
fn wrapped_row_boundaries_assign_cursor_to_one_row() {
    let first = PaintedRow {
        row_top: px(0.0),
        line_start_char: 0,
        display_end_char: 5,
        logical_end_char: 5,
        cursor_end_inclusive: false,
        code_line: None,
        gutter_line: None,
    };
    let second = PaintedRow {
        row_top: px(0.0),
        line_start_char: 5,
        display_end_char: 10,
        logical_end_char: 10,
        cursor_end_inclusive: true,
        code_line: None,
        gutter_line: None,
    };

    assert!(!row_contains_cursor(&first, 5));
    assert!(row_contains_cursor(&second, 5));
}

#[test]
fn eof_cursor_is_allowed_on_last_empty_row() {
    let row = PaintedRow {
        row_top: px(0.0),
        line_start_char: 0,
        display_end_char: 0,
        logical_end_char: 0,
        cursor_end_inclusive: true,
        code_line: None,
        gutter_line: None,
    };

    assert!(row_contains_cursor(&row, 0));
}
