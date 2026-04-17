use lst_editor::position::Position;
use lst_editor::{EditorModel, EditorTab, FileStamp, Language, TabId};
use std::path::PathBuf;

fn model_with_path(path: &str, text: &str) -> EditorModel {
    EditorModel::new(
        vec![EditorTab::from_path(
            TabId::from_raw(1),
            PathBuf::from(path),
            text,
        )],
        "Ready.".into(),
    )
}

fn model_with_language(language: Option<Language>, text: &str) -> EditorModel {
    let mut model = EditorModel::new(
        vec![EditorTab::from_text(
            TabId::from_raw(1),
            "scratch".into(),
            None,
            text,
        )],
        "Ready.".into(),
    );
    model.set_tab_language(TabId::from_raw(1), language);
    model
}

#[test]
fn tab_carries_detected_language_for_known_extensions() {
    let rs = model_with_path("example.rs", "");
    assert_eq!(rs.active_tab().language(), Some(Language::Rust));

    let py = model_with_path("example.py", "");
    assert_eq!(py.active_tab().language(), Some(Language::Python));

    let tsx = model_with_path("example.tsx", "");
    assert_eq!(tsx.active_tab().language(), Some(Language::Tsx));

    let makefile = model_with_path("Makefile", "");
    assert_eq!(makefile.active_tab().language(), Some(Language::Makefile));

    let make_include = model_with_path("rules.mk", "");
    assert_eq!(
        make_include.active_tab().language(),
        Some(Language::Makefile)
    );

    let unknown = model_with_path("example.xyz", "");
    assert_eq!(unknown.active_tab().language(), None);
}

#[test]
fn shebang_detects_language_when_extension_absent() {
    let python_script = model_with_path("build", "#!/usr/bin/env python3\nprint('hi')\n");
    assert_eq!(
        python_script.active_tab().language(),
        Some(Language::Python)
    );

    let node_script = model_with_path("server", "#!/usr/bin/env node\nconsole.log('hi');\n");
    assert_eq!(
        node_script.active_tab().language(),
        Some(Language::JavaScript)
    );
}

#[test]
fn toggle_comment_uses_language_prefix_for_python() {
    let mut model = model_with_path("example.py", "x = 1");
    model.toggle_comment();
    assert_eq!(model.snapshot().text, "# x = 1");
}

#[test]
fn toggle_comment_uses_language_prefix_for_rust() {
    let mut model = model_with_path("example.rs", "let x = 1;");
    model.toggle_comment();
    assert_eq!(model.snapshot().text, "// let x = 1;");
}

#[test]
fn toggle_comment_uses_language_prefix_for_lua() {
    let mut model = model_with_path("example.lua", "print(1)");
    model.toggle_comment();
    assert_eq!(model.snapshot().text, "-- print(1)");
}

#[test]
fn toggle_comment_uses_makefile_prefix_for_mk_files() {
    let mut model = model_with_path("rules.mk", "target:");
    model.toggle_comment();
    assert_eq!(model.snapshot().text, "# target:");
}

#[test]
fn toggle_comment_is_noop_when_language_has_no_line_comment() {
    let mut model = model_with_path("example.json", "{}");
    let before = model.snapshot().text.clone();
    model.toggle_comment();
    assert_eq!(model.snapshot().text, before);
}

#[test]
fn auto_pair_angle_brackets_fire_for_tsx() {
    let mut model = model_with_path("component.tsx", "");
    model.replace_text_from_input(None, "<".into());

    let snapshot = model.snapshot();
    assert_eq!(snapshot.text, "<>");
    assert_eq!(snapshot.cursor_position, Position { line: 0, column: 1 });
}

#[test]
fn auto_pair_angle_brackets_fire_for_html() {
    let mut model = model_with_path("page.html", "");
    model.replace_text_from_input(None, "<".into());
    assert_eq!(model.snapshot().text, "<>");
}

#[test]
fn auto_pair_angle_brackets_do_not_fire_for_rust() {
    let mut model = model_with_path("main.rs", "");
    model.replace_text_from_input(None, "<".into());
    // No auto-pair: just a literal `<` inserted.
    assert_eq!(model.snapshot().text, "<");
}

#[test]
fn auto_pair_angle_brackets_do_not_fire_for_typescript() {
    let mut model = model_with_path("module.ts", "");
    model.replace_text_from_input(None, "<".into());
    assert_eq!(model.snapshot().text, "<");
}

#[test]
fn auto_pair_suppresses_single_quote_in_rust() {
    let mut model = model_with_path("main.rs", "");
    model.replace_text_from_input(None, "'".into());
    // Single quote should not auto-close in Rust because it commonly denotes lifetimes.
    assert_eq!(model.snapshot().text, "'");
}

#[test]
fn auto_pair_single_quote_still_fires_for_javascript() {
    let mut model = model_with_path("main.js", "");
    model.replace_text_from_input(None, "'".into());
    assert_eq!(model.snapshot().text, "''");
}

#[test]
fn auto_dedent_does_not_fire_in_python() {
    let mut model = model_with_path("script.py", "        ");
    model.move_to_char(8, false, None);
    model.replace_text_from_input(None, "}".into());

    // Python has no auto-dedent closers; `}` is a literal character.
    assert_eq!(model.snapshot().text, "        }");
}

#[test]
fn auto_dedent_fires_in_rust_with_four_space_step() {
    let mut model = model_with_path("main.rs", "        ");
    model.move_to_char(8, false, None);
    model.replace_text_from_input(None, "}".into());

    assert_eq!(model.snapshot().text, "    }");
}

#[test]
fn auto_dedent_fires_in_javascript_with_two_space_step() {
    let mut model = model_with_path("main.js", "    ");
    model.move_to_char(4, false, None);
    model.replace_text_from_input(None, "}".into());

    // JavaScript defaults to 2-space indent, so four leading spaces dedent to two.
    assert_eq!(model.snapshot().text, "  }");
}

#[test]
fn tab_inserts_language_indent_unit_rust() {
    let mut model = model_with_path("main.rs", "");
    model.insert_tab_at_cursor();
    assert_eq!(model.snapshot().text, "    ");
}

#[test]
fn tab_inserts_language_indent_unit_javascript() {
    let mut model = model_with_path("main.js", "");
    model.insert_tab_at_cursor();
    assert_eq!(model.snapshot().text, "  ");
}

#[test]
fn tab_inserts_tab_character_for_go() {
    let mut model = model_with_path("main.go", "");
    model.insert_tab_at_cursor();
    assert_eq!(model.snapshot().text, "\t");
}

#[test]
fn set_tab_language_overrides_detection_and_retunes_behavior() {
    let mut model = model_with_path("example.py", "x = 1");
    assert_eq!(model.active_tab().language(), Some(Language::Python));

    // Override Python with Rust; subsequent comment-toggle uses Rust's `//`.
    let id = model.active_tab_id();
    model.set_tab_language(id, Some(Language::Rust));
    assert_eq!(model.active_tab().language(), Some(Language::Rust));

    model.toggle_comment();
    assert_eq!(model.snapshot().text, "// x = 1");
}

#[test]
fn set_tab_language_to_none_falls_back_to_defaults() {
    let mut model = model_with_path("example.rs", "");
    let id = model.active_tab_id();
    model.set_tab_language(id, None);
    assert_eq!(model.active_tab().language(), None);

    // Default config still auto-pairs single quotes (Rust's suppression is gone).
    model.replace_text_from_input(None, "'".into());
    assert_eq!(model.snapshot().text, "''");
}

#[test]
fn scratchpad_language_is_none_by_default() {
    let model = model_with_language(None, "");
    assert_eq!(model.active_tab().language(), None);
}

#[test]
fn path_backed_empty_buffer_detects_language_from_extension() {
    // A scratchpad / newly-saved tab with no content should still pick up the
    // language from its path so Tab, comment toggle, etc. behave correctly
    // from the first keystroke.
    let model = model_with_path("notes.md", "");
    assert_eq!(model.active_tab().language(), Some(Language::Markdown));

    let model = model_with_path("scratch.rs", "");
    assert_eq!(model.active_tab().language(), Some(Language::Rust));
}

#[test]
fn save_as_recomputes_language_from_new_path() {
    let mut model = model_with_path("example.py", "x = 1");
    assert_eq!(model.active_tab().language(), Some(Language::Python));

    let id = model.active_tab_id();
    model.save_as_finished_for_tab(
        id,
        PathBuf::from("main.rs"),
        Some(FileStamp::from_raw(5, None)),
    );
    assert_eq!(model.active_tab().language(), Some(Language::Rust));

    model.toggle_comment();
    assert_eq!(model.snapshot().text, "// x = 1");
}

#[test]
fn saved_untitled_tab_detects_language_from_saved_path() {
    let mut model = EditorModel::empty();
    let id = model.active_tab_id();

    model.save_as_finished_for_tab(id, PathBuf::from("main.rs"), None);
    assert_eq!(model.active_tab().language(), Some(Language::Rust));
}

#[test]
fn indent_selection_uses_language_unit() {
    let mut model = model_with_path("main.js", "alpha\nbeta\n");
    model.set_selection(0..10, false);
    model.insert_tab_at_cursor();

    assert_eq!(model.snapshot().text, "  alpha\n  beta\n");
}

#[test]
fn outdent_selection_uses_language_unit_for_tabs() {
    let mut model = model_with_path("main.go", "\talpha\n\tbeta\n");
    model.set_selection(0..12, false);
    model.outdent_at_cursor();

    assert_eq!(model.snapshot().text, "alpha\nbeta\n");
}
