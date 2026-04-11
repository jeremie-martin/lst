use gpui::Keystroke;
use lst_ui::{COLOR_GREEN, COLOR_MUTED};

use crate::syntax::{
    compute_syntax_highlights, syntax_mode_for_path, SyntaxHighlightJobKey, SyntaxLanguage,
    SyntaxMode,
};
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
fn launch_args_accept_benchmark_window_title() {
    let args =
        crate::launch::parse_launch_args_from(["--title", "lst-bench-window", "/tmp/example.rs"])
            .expect("args should parse");

    assert_eq!(args.window_title.as_deref(), Some("lst-bench-window"));
    assert_eq!(args.files, [PathBuf::from("/tmp/example.rs")]);
}

#[test]
fn launch_args_require_title_value() {
    let error = crate::launch::parse_launch_args_from(["--title"]).expect_err("missing title");

    assert!(matches!(
        error,
        crate::launch::LaunchArgError::Message(message) if message == "missing value for --title"
    ));
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
    let lines = compute_syntax_highlights(
        SyntaxLanguage::Rust,
        "/* first line\nsecond line */\nlet x = 1;\n",
    );

    assert!(lines[0].iter().any(|span| span.color == COLOR_MUTED));
    assert!(lines[1].iter().any(|span| span.color == COLOR_MUTED));
    assert!(lines[2].iter().all(|span| span.color != COLOR_MUTED));
}

#[test]
fn syntax_mode_maps_core_extensions() {
    let cases = [
        ("example.rs", SyntaxLanguage::Rust),
        ("example.py", SyntaxLanguage::Python),
        ("example.pyw", SyntaxLanguage::Python),
        ("example.js", SyntaxLanguage::JavaScript),
        ("example.mjs", SyntaxLanguage::JavaScript),
        ("example.cjs", SyntaxLanguage::JavaScript),
        ("example.jsx", SyntaxLanguage::Jsx),
        ("example.ts", SyntaxLanguage::TypeScript),
        ("example.tsx", SyntaxLanguage::Tsx),
        ("example.json", SyntaxLanguage::Json),
        ("example.toml", SyntaxLanguage::Toml),
        ("example.yaml", SyntaxLanguage::Yaml),
        ("example.yml", SyntaxLanguage::Yaml),
        ("example.md", SyntaxLanguage::Markdown),
        ("example.markdown", SyntaxLanguage::Markdown),
        ("example.html", SyntaxLanguage::Html),
        ("example.htm", SyntaxLanguage::Html),
        ("example.css", SyntaxLanguage::Css),
    ];

    for (path, language) in cases {
        assert_eq!(
            syntax_mode_for_path(Some(&PathBuf::from(path))),
            SyntaxMode::TreeSitter(language)
        );
    }
    assert_eq!(
        syntax_mode_for_path(Some(&PathBuf::from("example.txt"))),
        SyntaxMode::Plain
    );
}

#[test]
fn broad_highlighting_produces_spans_for_representative_languages() {
    let cases = [
        (
            SyntaxLanguage::Python,
            "value = \"\"\"first\nsecond\"\"\"\nprint(value)\n",
        ),
        (
            SyntaxLanguage::JavaScript,
            "/* first\nsecond */\nconst value = `template ${1}`;\n",
        ),
        (
            SyntaxLanguage::Jsx,
            "const element = <div className=\"editor\">{value}</div>;\n",
        ),
        (
            SyntaxLanguage::TypeScript,
            "interface Item { name: string }\nconst item: Item = { name: \"lst\" };\n",
        ),
        (
            SyntaxLanguage::Tsx,
            "const element: JSX.Element = <div className=\"editor\">{value}</div>;\n",
        ),
        (
            SyntaxLanguage::Json,
            "{\n  \"name\": \"lst\",\n  \"enabled\": true\n}\n",
        ),
        (SyntaxLanguage::Toml, "[package]\nname = \"lst\"\n"),
        (SyntaxLanguage::Yaml, "name: lst\nenabled: true\n"),
        (
            SyntaxLanguage::Markdown,
            "# Title\n\n```rust\nfn main() {}\n```\n",
        ),
        (
            SyntaxLanguage::Html,
            "<style>.editor { color: red; }</style>\n",
        ),
        (SyntaxLanguage::Css, ".editor { color: red; }\n"),
    ];

    for (language, source) in cases {
        let lines = compute_syntax_highlights(language, source);
        assert!(
            lines.iter().flatten().next().is_some(),
            "{language:?} should produce at least one syntax span"
        );
    }
}

#[test]
fn python_highlighting_keeps_multiline_string_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::Python,
        "value = \"\"\"first\nsecond\"\"\"\nprint(value)\n",
    );

    assert!(lines[0].iter().any(|span| span.color == COLOR_GREEN));
    assert!(lines[1].iter().any(|span| span.color == COLOR_GREEN));
}

#[test]
fn javascript_highlighting_keeps_multiline_comment_context() {
    let lines = compute_syntax_highlights(
        SyntaxLanguage::JavaScript,
        "/* first\nsecond */\nconst value = 1;\n",
    );

    assert!(lines[0].iter().any(|span| span.color == COLOR_MUTED));
    assert!(lines[1].iter().any(|span| span.color == COLOR_MUTED));
    assert!(lines[2].iter().all(|span| span.color != COLOR_MUTED));
}

#[test]
fn syntax_highlight_result_requires_matching_active_revision_and_language() {
    let rust_tab = EditorTab::from_path(PathBuf::from("/tmp/example.rs"), "fn main() {}\n");
    let rust_cache = rust_tab.cache.clone();
    let rust_key = SyntaxHighlightJobKey {
        language: SyntaxLanguage::Rust,
        revision: 0,
    };
    assert!(syntax_highlight_result_is_current(
        &[rust_tab],
        0,
        &rust_cache,
        rust_key
    ));

    let mut stale_tab = EditorTab::from_path(PathBuf::from("/tmp/example.rs"), "fn main() {}\n");
    let stale_cache = stale_tab.cache.clone();
    stale_tab.replace_char_range(0..0, "// ");
    assert!(!syntax_highlight_result_is_current(
        &[stale_tab],
        0,
        &stale_cache,
        rust_key
    ));

    let python_tab = EditorTab::from_path(PathBuf::from("/tmp/example.py"), "print('lst')\n");
    let python_cache = python_tab.cache.clone();
    assert!(!syntax_highlight_result_is_current(
        &[python_tab],
        0,
        &python_cache,
        rust_key
    ));
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
fn word_range_groups_words_symbols_and_whitespace() {
    let buffer = Rope::from_str("alpha beta::gamma");

    assert_eq!(word_range_at_char(&buffer, 7), 6..10);
    assert_eq!(word_range_at_char(&buffer, 10), 10..12);
    assert_eq!(word_range_at_char(&buffer, 5), 5..6);
}

#[test]
fn line_range_includes_trailing_newline_when_present() {
    let buffer = Rope::from_str("one\ntwo\nthree");

    assert_eq!(line_range_at_char(&buffer, 1), 0..4);
    assert_eq!(line_range_at_char(&buffer, 5), 4..8);
    assert_eq!(line_range_at_char(&buffer, 10), 8..13);
}

#[test]
fn word_boundaries_skip_whitespace_before_moving() {
    let buffer = Rope::from_str("alpha beta.gamma");

    assert_eq!(next_word_boundary(&buffer, 0), 5);
    assert_eq!(next_word_boundary(&buffer, 5), 10);
    assert_eq!(previous_word_boundary(&buffer, 11), 10);
    assert_eq!(previous_word_boundary(&buffer, 10), 6);
}

#[test]
fn drag_autoscroll_delta_only_activates_at_viewport_edges() {
    let bounds = Bounds::new(point(px(0.0), px(100.0)), gpui::size(px(100.0), px(200.0)));

    assert!(drag_autoscroll_delta(point(px(50.0), px(90.0)), bounds).is_some());
    assert!(drag_autoscroll_delta(point(px(50.0), px(310.0)), bounds).is_some());
    assert!(drag_autoscroll_delta(point(px(50.0), px(200.0)), bounds).is_none());
}

#[test]
fn ctrl_arrow_aliases_expand_vertical_selection() {
    assert!(has_binding::<SelectUp>("ctrl-up"));
    assert!(has_binding::<SelectDown>("ctrl-down"));
}

#[test]
fn standard_movement_keybindings_are_registered() {
    assert!(has_binding::<MoveWordLeft>("ctrl-left"));
    assert!(has_binding::<MoveWordRight>("ctrl-right"));
    assert!(has_binding::<SelectWordLeft>("ctrl-shift-left"));
    assert!(has_binding::<SelectWordRight>("ctrl-shift-right"));
    assert!(has_binding::<MovePageUp>("pageup"));
    assert!(has_binding::<MovePageDown>("pagedown"));
    assert!(has_binding::<SelectPageUp>("shift-pageup"));
    assert!(has_binding::<SelectPageDown>("shift-pagedown"));
    assert!(has_binding::<MoveDocumentStart>("ctrl-home"));
    assert!(has_binding::<MoveDocumentEnd>("ctrl-end"));
    assert!(has_binding::<SelectDocumentStart>("ctrl-shift-home"));
    assert!(has_binding::<SelectDocumentEnd>("ctrl-shift-end"));
    assert!(has_binding::<MoveLineStart>("cmd-left"));
    assert!(has_binding::<MoveLineEnd>("cmd-right"));
    assert!(has_binding::<SelectLineStart>("cmd-shift-left"));
    assert!(has_binding::<SelectLineEnd>("cmd-shift-right"));
    assert!(has_binding::<DeleteWordBackward>("ctrl-backspace"));
    assert!(has_binding::<DeleteWordBackward>("alt-backspace"));
    assert!(has_binding::<DeleteWordForward>("ctrl-delete"));
    assert!(has_binding::<DeleteWordForward>("alt-delete"));
}

#[test]
fn explicit_undo_boundary_keeps_paste_separate_from_typing() {
    let mut tab = Tab::from_text("untitled".into(), None, "", false);
    tab.edit(EditKind::Insert, UndoBoundary::Merge, 0..0, "a");
    tab.edit(EditKind::Insert, UndoBoundary::Break, 1..1, "paste");

    assert_eq!(tab.buffer_text(), "apaste");
    assert!(tab.undo());
    assert_eq!(tab.buffer_text(), "a");
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
fn closing_tab_preserves_expected_active_index() {
    assert_eq!(next_active_after_tab_close(3, 2, 0), 1);
    assert_eq!(next_active_after_tab_close(3, 1, 2), 1);
    assert_eq!(next_active_after_tab_close(3, 2, 2), 1);
    assert_eq!(next_active_after_tab_close(1, 0, 0), 0);
}

#[test]
fn word_delete_range_uses_selection_or_word_boundary() {
    let mut tab = Tab::from_text("untitled".into(), None, "alpha beta.gamma", false);
    tab.move_to(11);
    assert_eq!(delete_selection_or_word_range(&tab, true), Some(10..11));
    assert_eq!(delete_selection_or_word_range(&tab, false), Some(11..16));

    tab.selection = 2..8;
    tab.selection_reversed = false;
    assert_eq!(delete_selection_or_word_range(&tab, true), Some(2..8));
    assert_eq!(delete_selection_or_word_range(&tab, false), Some(2..8));
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
