mod support;

use lst_editor::{EditorCommand, Position};
use support::{position_of, ModelHarness};

const SAMPLE: &str = "alpha\nbeta\ngamma\n";
const FIND_TEXT: &str = "fn alpha() {}\nlet other = 1;\nfn beta() {}\nfn gamma() {}\n";

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
