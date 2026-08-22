mod support;

use lst_editor::{
    EditorCommand, EditorEffect, EditorModel, EditorTab, FileStamp, InputMode, Language, LanguageMode, Position,
    SaveExpectation, Selection, SelectionSet, TabCloseRequest, TabId, UndoBoundary,
};
use std::path::PathBuf;
use support::{position_of, ModelHarness};

const SAMPLE: &str = "alpha\nbeta\ngamma\n";
const FIND_TEXT: &str = "fn alpha() {}\nlet other = 1;\nfn beta() {}\nfn gamma() {}\n";

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
fn standard_input_and_explicit_language_are_stable_defaults() {
    let mut harness = ModelHarness::new("print('hello')\n");
    assert_eq!(harness.model.input_mode(), InputMode::Standard);
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
fn clipboard_workflows_preserve_current_model_results() {
    let mut select = ModelHarness::new(SAMPLE);
    select.execute(EditorCommand::SelectAll);
    assert_eq!(select.model.selection().range(), 0..SAMPLE.chars().count());
    assert_eq!(select.primary_text(), Some(SAMPLE));

    let mut copy = ModelHarness::new(SAMPLE);
    copy.execute(EditorCommand::SelectAll);
    copy.clear_transfer_buffers();
    copy.execute(EditorCommand::CopySelection);
    assert_eq!(copy.clipboard_text(), Some(SAMPLE));
    assert_eq!(copy.primary_text(), Some(SAMPLE));

    let mut paste_once = ModelHarness::new("");
    paste_once.set_clipboard(SAMPLE);
    paste_once.execute(EditorCommand::RequestPaste);
    assert_eq!(paste_once.text(), SAMPLE);

    let mut paste_three = ModelHarness::new("");
    paste_three.set_clipboard(SAMPLE);
    for _ in 0..3 {
        paste_three.execute(EditorCommand::RequestPaste);
    }
    assert_eq!(paste_three.text(), SAMPLE.repeat(3));
}

#[test]
fn repeated_whole_document_paste_preserves_current_model_results() {
    let mut same_tab = ModelHarness::new(SAMPLE);
    same_tab.execute(EditorCommand::SelectAll);
    same_tab.execute(EditorCommand::CopySelection);
    for _ in 0..3 {
        same_tab.execute(EditorCommand::RequestPaste);
    }
    assert_eq!(same_tab.text(), SAMPLE.repeat(3));

    let mut two_tabs = ModelHarness::with_two_tabs(SAMPLE, "");
    two_tabs.execute(EditorCommand::SelectAll);
    two_tabs.execute(EditorCommand::CopySelection);
    two_tabs.execute(EditorCommand::NextTab);
    for _ in 0..3 {
        two_tabs.execute(EditorCommand::RequestPaste);
    }
    assert_eq!(two_tabs.tab_text(0), SAMPLE);
    assert_eq!(two_tabs.tab_text(1), SAMPLE.repeat(3));
}

#[test]
fn find_workflows_preserve_current_model_results() {
    let mut find = ModelHarness::new(FIND_TEXT);
    find.model.update_find_query_and_activate("fn ".to_string());
    find.sync_effects();
    assert_eq!(find.find_match_count(), 3);
    assert_eq!(find.cursor(), Position::new(0, 0));

    for _ in 0..5 {
        find.execute(EditorCommand::FindNext);
    }
    assert_eq!(find.cursor(), Position::new(3, 0));

    find.execute(EditorCommand::SelectAllFindMatches);
    assert_eq!(find.selection_count(), 3);

    let mut replace = ModelHarness::new(FIND_TEXT);
    replace.model.update_find_replacement("fn".to_string());
    replace.model.update_find_query_and_activate("fn ".to_string());
    replace.sync_effects();
    replace.execute(EditorCommand::ReplaceAllMatches);
    assert_eq!(
        replace.text(),
        "fnalpha() {}\nlet other = 1;\nfnbeta() {}\nfngamma() {}\n"
    );
    assert_eq!(replace.find_match_count(), 0);
}

#[test]
fn page_movement_preserves_current_model_results() {
    let mut wrapped = ModelHarness::new("abcdefghijklmnopqrstuvwxyz\nsecond\n");
    wrapped.configure_viewport(5, 0);
    wrapped.execute(EditorCommand::Page(true, false, 10));
    assert_eq!(wrapped.cursor(), Position::new(1, 0));
    wrapped.execute(EditorCommand::Page(false, false, 10));
    assert_eq!(wrapped.cursor(), Position::new(0, 0));

    let mut unwrapped = ModelHarness::new("l0\nl1\nl2\nl3\nl4\nl5\n");
    unwrapped.configure_viewport(5, 0);
    unwrapped.execute(EditorCommand::ToggleWrap);
    unwrapped.execute(EditorCommand::Page(true, false, 80));
    assert_eq!(unwrapped.cursor(), Position::new(3, 0));
    unwrapped.execute(EditorCommand::Page(false, false, 80));
    assert_eq!(unwrapped.cursor(), Position::new(0, 0));
}

#[test]
fn line_edit_and_multi_cursor_workflows_preserve_current_model_results() {
    let mut indent = ModelHarness::new("fn main() {\nlet value = 1;\n}\n");
    indent.execute(EditorCommand::SelectAll);
    indent.execute(EditorCommand::InsertTab);
    assert_eq!(indent.text(), "    fn main() {\n    let value = 1;\n    }\n");

    let occurrence_text = "selected one selected two\nidle\nselected three\n";
    let mut occurrences = ModelHarness::new(occurrence_text);
    occurrences.set_cursor(position_of(occurrence_text, "selected"));
    occurrences.execute(EditorCommand::SelectAllOccurrences);
    assert_eq!(occurrences.selection_count(), 3);

    let mut line_ends = ModelHarness::new("aa\nbbbb\nc\n");
    line_ends.select_first_lines(3);
    line_ends.execute(EditorCommand::AddCursorsToSelectedLineEnds);
    assert_eq!(line_ends.selection_count(), 3);
    line_ends.paste_text("!");
    assert_eq!(line_ends.text(), "aa!\nbbbb!\nc!\n");
}

#[test]
fn column_selection_restores_its_preferred_column_after_short_lines() {
    let mut harness = ModelHarness::new("abcdef\nx\nabcdef");
    harness.set_cursor(Position::new(0, 5));

    harness.execute(EditorCommand::ColumnSelectDown);
    harness.execute(EditorCommand::ColumnSelectDown);

    assert_eq!(
        harness.model.selection_set().as_slice(),
        &[
            Selection::collapsed(5),
            Selection::collapsed(8),
            Selection::collapsed(14)
        ]
    );
}

#[test]
fn lowering_multi_cursor_limit_clamps_inactive_tabs_immediately() {
    let first_id = TabId::from_raw(1);
    let second_id = TabId::from_raw(2);
    let first = EditorTab::from_path_with_stamp(first_id, PathBuf::from("first.txt"), "first", None);
    let second = EditorTab::from_path_with_stamp(second_id, PathBuf::from("second.txt"), "second", None);
    let mut model = EditorModel::from_tabs(first, vec![second], "Ready.".to_string());
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
fn visual_line_boundaries_move_every_cursor_and_preserve_each_selection_anchor() {
    let text = "    aa\n  bbbb\n    cc";
    let line_starts = [0, 7, 14];
    let line_ends = [6, 13, 20];
    let mut home = ModelHarness::new(text);
    home.model.set_selection_set(
        SelectionSet::from_selections(line_ends.map(Selection::collapsed).to_vec(), 0)
            .expect("one cursor at each line end is valid"),
    );

    home.model.move_visual_line_boundary(false, false, 80);
    assert_eq!(
        home.model.selection_set().as_slice(),
        &[
            Selection::collapsed(4),
            Selection::collapsed(9),
            Selection::collapsed(18)
        ]
    );
    home.model.move_visual_line_boundary(false, false, 80);
    assert_eq!(
        home.model.selection_set().as_slice(),
        &line_starts.map(Selection::collapsed)
    );
    home.model.move_visual_line_boundary(true, false, 80);
    assert_eq!(
        home.model.selection_set().as_slice(),
        &line_ends.map(Selection::collapsed)
    );

    let mut shifted_home = ModelHarness::new(text);
    shifted_home.model.set_selection_set(
        SelectionSet::from_selections(line_ends.map(Selection::collapsed).to_vec(), 0)
            .expect("one cursor at each line end is valid"),
    );
    shifted_home.model.move_visual_line_boundary(false, true, 80);
    assert_eq!(
        shifted_home.model.selection_set().as_slice(),
        &[Selection::new(6, 4), Selection::new(13, 9), Selection::new(20, 18)]
    );
    shifted_home.model.move_visual_line_boundary(false, true, 80);
    assert_eq!(
        shifted_home.model.selection_set().as_slice(),
        &[Selection::new(6, 0), Selection::new(13, 7), Selection::new(20, 14)]
    );

    let mut shifted_end = ModelHarness::new(text);
    shifted_end.model.set_selection_set(
        SelectionSet::from_selections(line_starts.map(Selection::collapsed).to_vec(), 0)
            .expect("one cursor at each line start is valid"),
    );
    shifted_end.model.move_visual_line_boundary(true, true, 80);
    assert_eq!(
        shifted_end.model.selection_set().as_slice(),
        &[Selection::new(0, 6), Selection::new(7, 13), Selection::new(14, 20)]
    );
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
fn new_standard_pair_edits_do_not_change_vim_insert_semantics() {
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
fn explicit_line_indent_coalesces_cursors_while_tab_inserts_at_each_cursor() {
    let cursors = || {
        SelectionSet::from_selections(vec![Selection::collapsed(3), Selection::collapsed(7)], 0)
            .expect("same-line cursors are valid")
    };

    let mut tab = ModelHarness::new("foo foo");
    tab.model.set_selection_set(cursors());
    tab.execute(EditorCommand::InsertTab);
    assert_eq!(tab.text(), "foo  foo ");

    let mut indent = ModelHarness::new("foo foo");
    indent.model.set_selection_set(cursors());
    indent.execute(EditorCommand::IndentLines);
    assert_eq!(indent.text(), "    foo foo");
    assert_eq!(indent.selection_count(), 2);
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
    wrapped.model.set_show_wrap(true);
    wrapped.set_cursor(Position::new(0, 2));

    wrapped.model.move_visual_line_boundary(true, false, 5);
    assert_eq!(wrapped.cursor(), Position::new(0, 5));
    wrapped.model.move_visual_line_boundary(true, false, 5);
    assert_eq!(wrapped.cursor(), Position::new(0, 5));

    wrapped.model.move_visual_line_boundary(false, false, 5);
    assert_eq!(wrapped.cursor(), Position::new(0, 0));
}

#[test]
fn active_undo_history_retains_more_than_the_old_hundred_group_limit() {
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
