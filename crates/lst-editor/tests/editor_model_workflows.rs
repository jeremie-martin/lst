mod support;

use lst_editor::{
    BufferDelta, EditorCommand, EditorEffect, FileStamp, InputMode, Language, LanguageMode, Position, SaveExpectation,
    Selection, SelectionSet, TabCloseRequest, TabId, UndoBoundary,
};
use ropey::Rope;
use std::path::PathBuf;
use support::ModelHarness;

#[test]
fn missing_backing_file_requires_an_explicit_save_or_discard() {
    let mut harness = ModelHarness::new("only surviving copy\n");
    let tab_id = harness.model.active_tab_id();

    assert!(harness.model.mark_tab_backing_file_missing(tab_id));
    assert!(harness.model.active_tab().backing_file_missing());
    assert_eq!(
        harness.model.close_request_for_tab(tab_id),
        Some(TabCloseRequest::SaveAndClose { tab_id })
    );

    harness.model.request_save_tab(tab_id);
    assert!(matches!(
        harness.model.drain_effects().as_slice(),
        [EditorEffect::SaveFile {
            tab_id: effect_tab,
            expectation: SaveExpectation::Absent,
            ..
        }] if *effect_tab == tab_id
    ));
}

#[test]
fn stale_save_completion_keeps_an_undone_buffer_dirty_against_the_committed_body() {
    let mut harness = ModelHarness::new("initial\n");
    let tab_id = harness.model.active_tab_id();
    let path = PathBuf::from("model-spec.rs");
    let initial_len = harness.model.active_tab().len_chars();
    harness.model.replace_text(
        Some(0..initial_len),
        "body already sent to disk\n".to_string(),
        UndoBoundary::Break,
    );
    let requested_revision = harness.model.active_tab().revision();
    let requested_body = harness.model.active_tab().buffer_text();

    harness.execute(EditorCommand::Undo);
    assert_eq!(harness.text(), "initial\n");
    assert!(!harness.model.active_tab().modified());

    let committed_stamp = FileStamp::from_raw(requested_body.len() as u64, Some(42));
    assert!(!harness
        .model
        .save_finished_for_tab(tab_id, path, requested_revision, committed_stamp, requested_body,));
    assert!(
        harness.model.active_tab().modified(),
        "the editor copy differs from the body that actually reached disk"
    );
    assert_eq!(harness.model.active_tab().file_stamp(), Some(committed_stamp));
}

#[test]
fn explicit_language_survives_save_as_and_auto_redetects_from_the_new_path() {
    let mut harness = ModelHarness::new("print('hello')\n");
    assert_eq!(harness.model.active_tab().language(), Some(Language::Rust));

    harness
        .model
        .set_active_language_mode(LanguageMode::Language(Language::Markdown));
    let tab_id = harness.model.active_tab_id();
    let revision = harness.model.active_tab().revision();
    let body = harness.model.active_tab().buffer_text();
    assert!(harness.model.save_as_finished_for_tab(
        tab_id,
        PathBuf::from("renamed.py"),
        revision,
        FileStamp::from_raw(body.len() as u64, Some(1)),
        body,
    ));
    assert_eq!(harness.model.active_tab().language(), Some(Language::Markdown));
    assert_eq!(
        harness.model.active_tab().language_mode(),
        LanguageMode::Language(Language::Markdown)
    );

    harness.model.set_active_language_mode(LanguageMode::Auto);
    assert_eq!(harness.model.active_tab().language(), Some(Language::Python));
    harness.model.set_active_language_mode(LanguageMode::PlainText);
    assert_eq!(harness.model.active_tab().language(), None);
}

#[test]
fn page_down_counts_wrapped_display_rows() {
    // At 10 columns the first line takes three display rows, so a page of
    // three rows lands on the second logical line.
    let mut wrapped = ModelHarness::new("abcdefghijklmnopqrstuvwxyz\nsecond\n");
    wrapped.configure_viewport(5, 0);
    wrapped.execute(EditorCommand::Page(true, false, 10));
    assert_eq!(wrapped.cursor(), Position::new(1, 0));
    wrapped.execute(EditorCommand::Page(false, false, 10));
    assert_eq!(wrapped.cursor(), Position::new(0, 0));
}

#[test]
fn lowering_multi_cursor_limit_clamps_inactive_tabs_immediately() {
    let first_id = TabId::from_raw(1);
    let second_id = TabId::from_raw(2);
    let ModelHarness { mut model, .. } = ModelHarness::with_two_tabs("first", "second");
    model.set_active_tab(second_id);
    model.set_selection_set(
        SelectionSet::from_selections((0..4).map(Selection::collapsed).collect(), 3)
            .expect("fixture selections are ordered"),
    );
    model.set_active_tab(first_id);

    model.set_multi_cursor_limit(2);

    let limited = model
        .tab_by_id(second_id)
        .expect("second tab remains open")
        .selection_set();
    assert_eq!(limited.as_slice(), &[Selection::collapsed(0), Selection::collapsed(3)]);
    assert_eq!(limited.primary_index(), 1);
}

#[test]
fn visual_line_boundaries_use_each_cursors_wrapped_row() {
    let text = "abcdefghij\nklmnopqrst";
    let cursors = [6, 17];

    let mut home = ModelHarness::new(text);
    home.model.set_selection_set(
        SelectionSet::from_selections(cursors.map(Selection::collapsed).to_vec(), 0)
            .expect("one cursor on each wrapped line is valid"),
    );
    home.model.move_visual_line_boundary(false, false, 4);
    assert_eq!(
        home.model.selection_set().as_slice(),
        &[Selection::collapsed(4), Selection::collapsed(15)]
    );

    let mut end = ModelHarness::new(text);
    end.model.set_selection_set(
        SelectionSet::from_selections(cursors.map(Selection::collapsed).to_vec(), 0)
            .expect("one cursor on each wrapped line is valid"),
    );
    end.model.move_visual_line_boundary(true, true, 4);
    assert_eq!(
        end.model.selection_set().as_slice(),
        &[Selection::new(6, 8), Selection::new(17, 19)]
    );

    end.execute(EditorCommand::ToggleWrap);
    end.model.move_visual_line_boundary(true, false, 4);
    assert_eq!(
        end.model.selection_set().as_slice(),
        &[Selection::collapsed(10), Selection::collapsed(21)]
    );
}

#[test]
fn standard_pair_backspace_and_smart_enter_are_single_conventional_edits() {
    let mut pair = ModelHarness::new("");
    pair.model.replace_text_from_input(None, "(".to_string());
    pair.sync_effects();
    assert_eq!(pair.text(), "()");
    assert_eq!(pair.cursor(), Position::new(0, 1));

    pair.execute(EditorCommand::Backspace);
    assert_eq!(pair.text(), "");
    pair.execute(EditorCommand::Undo);
    assert_eq!(pair.text(), "()");
    assert_eq!(pair.cursor(), Position::new(0, 1));

    let mut block = ModelHarness::new("    fn main() {}");
    block.set_cursor(Position::new(0, 15));
    block.execute(EditorCommand::InsertNewline);
    assert_eq!(block.text(), "    fn main() {\n        \n    }");
    assert_eq!(block.cursor(), Position::new(1, 8));
    block.execute(EditorCommand::Undo);
    assert_eq!(block.text(), "    fn main() {}");

    let mut crlf = ModelHarness::new("{}\r\n");
    crlf.set_cursor(Position::new(0, 1));
    crlf.execute(EditorCommand::InsertNewline);
    assert_eq!(crlf.text(), "{\r\n    \r\n}\r\n");
    assert_eq!(crlf.cursor(), Position::new(1, 4));
}

#[test]
fn enter_carries_only_the_indent_behind_the_cursor() {
    let mut at_start = ModelHarness::new("    foo");
    at_start.set_cursor(Position::new(0, 0));
    at_start.execute(EditorCommand::InsertNewline);
    assert_eq!(at_start.text(), "\n    foo");
    assert_eq!(at_start.cursor(), Position::new(1, 0));

    let mut inside_indent = ModelHarness::new("    foo");
    inside_indent.set_cursor(Position::new(0, 2));
    inside_indent.execute(EditorCommand::InsertNewline);
    assert_eq!(inside_indent.text(), "  \n    foo");
    assert_eq!(inside_indent.cursor(), Position::new(1, 2));

    let mut at_end = ModelHarness::new("    foo");
    at_end.set_cursor(Position::new(0, 7));
    at_end.execute(EditorCommand::InsertNewline);
    assert_eq!(at_end.text(), "    foo\n    ");
    assert_eq!(at_end.cursor(), Position::new(1, 4));
}

#[test]
fn vim_insert_mode_backspace_and_enter_skip_standard_pair_editing() {
    let mut vim = ModelHarness::new("{}");
    vim.model.set_input_mode(InputMode::Vim);
    vim.set_cursor(Position::new(0, 1));
    vim.execute(EditorCommand::Backspace);
    assert_eq!(vim.text(), "}");

    let mut vim_newline = ModelHarness::new("{}");
    vim_newline.model.set_input_mode(InputMode::Vim);
    vim_newline.set_cursor(Position::new(0, 1));
    vim_newline.execute(EditorCommand::InsertNewline);
    assert_eq!(vim_newline.text(), "{\n}");
    assert_eq!(vim_newline.cursor(), Position::new(1, 0));
}

#[test]
fn standard_tab_targets_the_next_stop_and_any_selection_indents_lines() {
    let mut stop = ModelHarness::new("  value");
    stop.set_cursor(Position::new(0, 2));
    stop.execute(EditorCommand::InsertTab);
    assert_eq!(stop.text(), "    value");
    assert_eq!(stop.cursor(), Position::new(0, 4));

    let mut later_stop = ModelHarness::new("value");
    later_stop.set_cursor(Position::new(0, 5));
    later_stop.execute(EditorCommand::InsertTab);
    assert_eq!(later_stop.text(), "value   ");
    assert_eq!(later_stop.cursor(), Position::new(0, 8));

    let mut selected = ModelHarness::new("alpha\nbeta\n");
    selected.model.set_selection(Selection::from_range(1..4, false));
    selected.execute(EditorCommand::InsertTab);
    assert_eq!(selected.text(), "    alpha\nbeta\n");
}

#[test]
fn tab_inserts_at_each_cursor_on_the_same_line() {
    let mut tab = ModelHarness::new("foo foo");
    tab.model.set_selection_set(
        SelectionSet::from_selections(vec![Selection::collapsed(3), Selection::collapsed(7)], 0)
            .expect("same-line cursors are valid"),
    );
    tab.execute(EditorCommand::InsertTab);
    assert_eq!(tab.text(), "foo  foo ");
}

#[test]
fn no_selection_copy_and_cut_keep_linewise_semantics_inside_lst() {
    let mut across_tabs = ModelHarness::with_two_tabs("alpha\nbeta\n", "omega\n");
    across_tabs.execute(EditorCommand::CopySelection);
    assert_eq!(across_tabs.clipboard_text(), Some("alpha\n"));
    across_tabs.execute(EditorCommand::NextTab);
    across_tabs.execute(EditorCommand::RequestPaste);
    assert_eq!(across_tabs.tab_text(1), "alpha\nomega\n");

    let mut final_line = ModelHarness::new("alpha\nbeta");
    final_line.set_cursor(Position::new(1, 2));
    final_line.execute(EditorCommand::CopySelection);
    assert_eq!(final_line.clipboard_text(), Some("beta\n"));

    final_line.execute(EditorCommand::CutSelection);
    assert_eq!(final_line.text(), "alpha");
    final_line.execute(EditorCommand::RequestPaste);
    assert_eq!(final_line.text(), "beta\nalpha");

    let mut multi = ModelHarness::new("a\nb\n");
    multi.model.set_selection_set(
        SelectionSet::from_selections(vec![Selection::collapsed(0), Selection::collapsed(2)], 0)
            .expect("two line cursors are valid"),
    );
    multi.execute(EditorCommand::CopySelection);
    multi.execute(EditorCommand::RequestPaste);
    assert_eq!(multi.text(), "a\na\na\nb\n");
    assert_eq!(multi.selection_count(), 2);

    let mut external = ModelHarness::new("alpha\n");
    external.execute(EditorCommand::CopySelection);
    external.set_clipboard("external");
    external.set_cursor(Position::new(0, 2));
    external.execute(EditorCommand::RequestPaste);
    assert_eq!(external.text(), "alexternalpha\n");
}

#[test]
fn pasting_a_linewise_copy_over_a_selection_replaces_the_selection() {
    let mut model = ModelHarness::new("alpha\nbeta\n");
    model.execute(EditorCommand::CopySelection);
    assert_eq!(model.clipboard_text(), Some("alpha\n"));

    model.model.set_selection(Selection::from_range(6..10, false));
    model.execute(EditorCommand::RequestPaste);
    assert_eq!(model.text(), "alpha\nalpha\n\n");
}

#[test]
fn end_and_home_on_wrapped_rows_neither_walk_the_line_nor_stall_at_boundaries() {
    let mut wrapped = ModelHarness::new("aaaaabbbbbccccc");
    wrapped.set_cursor(Position::new(0, 2));

    wrapped.model.move_visual_line_boundary(true, false, 5);
    assert_eq!(wrapped.cursor(), Position::new(0, 5));
    wrapped.model.move_visual_line_boundary(true, false, 5);
    assert_eq!(wrapped.cursor(), Position::new(0, 5));

    wrapped.model.move_visual_line_boundary(false, false, 5);
    assert_eq!(wrapped.cursor(), Position::new(0, 0));
}

#[test]
fn undo_reaches_back_through_150_separate_edits() {
    let mut history = ModelHarness::new("");
    for _ in 0..150 {
        let end = history.model.active_tab().len_chars();
        history
            .model
            .replace_text(Some(end..end), "x".to_string(), UndoBoundary::Break);
    }
    history.sync_effects();
    assert_eq!(history.text().len(), 150);

    for _ in 0..150 {
        history.execute(EditorCommand::Undo);
    }
    assert_eq!(history.text(), "");
}

#[test]
fn selection_drag_move_and_copy_are_atomic_and_reselect_the_destination() {
    let mut moved = ModelHarness::new("abcdef");
    moved.model.set_selection(Selection::from_range(1..3, false));
    let token = moved.model.selection_drag_token_at(1).unwrap();
    assert!(moved.model.drag_selection_token_to(&token, 6, false));
    moved.sync_effects();
    assert_eq!(moved.text(), "adefbc");
    assert_eq!(moved.model.selection().range(), 4..6);
    moved.execute(EditorCommand::Undo);
    assert_eq!(moved.text(), "abcdef");
    assert_eq!(moved.model.selection().range(), 1..3);

    let mut copied = ModelHarness::new("abcdef");
    copied.model.set_selection(Selection::from_range(1..3, false));
    let token = copied.model.selection_drag_token_at(1).unwrap();
    assert!(copied.model.drag_selection_token_to(&token, 6, true));
    copied.sync_effects();
    assert_eq!(copied.text(), "abcdefbc");
    assert_eq!(copied.model.selection().range(), 6..8);
    copied.execute(EditorCommand::Undo);
    assert_eq!(copied.text(), "abcdef");

    let mut moved_earlier = ModelHarness::new("abcdef");
    moved_earlier.model.set_selection(Selection::from_range(3..5, false));
    let token = moved_earlier.model.selection_drag_token_at(3).unwrap();
    assert!(moved_earlier.model.drag_selection_token_to(&token, 1, false));
    assert_eq!(moved_earlier.text(), "adebcf");
    assert_eq!(moved_earlier.model.selection().range(), 1..3);

    let mut inside = ModelHarness::new("abcdef");
    inside.model.set_selection(Selection::from_range(1..4, false));
    let token = inside.model.selection_drag_token_at(1).unwrap();
    assert!(!inside.model.drag_selection_token_to(&token, 2, false));
    assert!(!inside.model.drag_selection_token_to(&token, 4, true));
    assert_eq!(inside.text(), "abcdef");
    assert_eq!(inside.model.selection().range(), 1..4);
}

#[test]
fn selection_drag_tokens_reject_stale_document_revisions() {
    let mut harness = ModelHarness::new("abcdef");
    harness.model.set_selection(Selection::from_range(1..3, false));
    let token = harness.model.selection_drag_token_at(1).unwrap();
    harness
        .model
        .replace_text(Some(6..6), "!".to_string(), UndoBoundary::Break);

    assert!(!harness.model.drag_selection_token_to(&token, 6, false));
    assert_eq!(harness.text(), "abcdef!");
}

#[test]
fn external_append_waits_for_ime_and_rejects_missing_targets() {
    let mut harness = ModelHarness::new("prefix ");
    let id = harness.model.active_tab_id();
    harness.model.move_to_char(7, false, None);
    harness.model.replace_and_mark_text(None, "draft".into(), None);
    let revision = harness.model.active_tab().revision();
    let marked = harness.model.active_tab().marked_range().cloned();
    assert!(marked.is_some());
    assert!(!harness.model.append_text_to_tab(id, "spoken"));
    assert_eq!(harness.model.active_tab().revision(), revision);
    assert_eq!(harness.model.active_tab().marked_range(), marked.as_ref());
    harness.model.clear_marked_text();
    assert!(harness.model.append_text_to_tab(id, "spoken"));
    assert_eq!(harness.text(), "prefix draft spoken");
    assert!(!harness.model.append_text_to_tab(TabId::from_raw(999), "lost"));
    assert_eq!(harness.text(), "prefix draft spoken");
}

#[test]
fn external_append_preserves_a_normal_mode_caret_in_an_empty_document() {
    let mut harness = ModelHarness::new("");
    harness.model.set_input_mode(InputMode::Vim);
    harness.model.handle_vim_escape();
    let id = harness.model.active_tab_id();
    assert!(harness.model.append_text_to_tab(id, "spoken"));
    assert_eq!(harness.model.vim_mode(), lst_editor::vim::Mode::Normal);
    assert_eq!(harness.model.selection(), Selection::collapsed(0));
}

/// Incremental syntax highlighting keeps its own copy of the text in sync by
/// replaying these deltas, so replaying them must reproduce the buffer after
/// every kind of edit.
#[test]
fn buffer_deltas_replay_to_the_current_text() {
    fn replay(harness: &mut ModelHarness, mirror: &mut Rope) -> BufferDelta {
        let delta = harness.model.take_active_buffer_delta();
        match &delta {
            BufferDelta::Unchanged => {}
            BufferDelta::FullReplace => *mirror = Rope::from_str(&harness.text()),
            BufferDelta::Edits(edits) => {
                for edit in edits.iter().rev() {
                    mirror.remove(edit.range.clone());
                    mirror.insert(edit.range.start, &edit.replacement);
                }
            }
        }
        assert_eq!(mirror.to_string(), harness.text());
        delta
    }

    let mut harness = ModelHarness::new("foo bar foo\nbaz foo\n");
    let mut mirror = Rope::from_str(&harness.text());

    harness.execute(EditorCommand::SelectAllOccurrences);
    harness.paste_text("longer");
    assert!(
        matches!(replay(&mut harness, &mut mirror), BufferDelta::Edits(edits) if edits.len() == 3),
        "a multi-cursor edit is one batch of edits"
    );
    assert_eq!(harness.model.take_active_buffer_delta(), BufferDelta::Unchanged);

    // A consumer that skips a tick sees one delta covering both edits.
    harness.paste_text("x");
    harness.paste_text("y");
    replay(&mut harness, &mut mirror);

    harness.execute(EditorCommand::Undo);
    assert_eq!(replay(&mut harness, &mut mirror), BufferDelta::FullReplace);
    harness.execute(EditorCommand::Redo);
    replay(&mut harness, &mut mirror);
    assert_eq!(harness.text(), "longerxy bar longerxy\nbaz longerxy\n");
}
