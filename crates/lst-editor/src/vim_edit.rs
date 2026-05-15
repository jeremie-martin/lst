use crate::{
    document::{inclusive_position_to_exclusive_char, position_start_char},
    line_edit::{clamped_line_span, replace_lines_change},
    selection::{line_display_text, Position},
    tab::EditorTab,
    transaction::{EditRequest, SelectionAfter, TextChange, TextChangeSet},
};

pub(crate) type DeletedEdit = (String, EditRequest);

pub(crate) fn extract_lines(tab: &EditorTab, first: usize, last: usize) -> String {
    let (first, last) = clamped_line_span(tab, first, last);
    (first..=last).map(|line| line_display_text(tab.buffer(), line)).collect::<Vec<_>>().join("\n")
}

pub(crate) fn delete_lines(tab: &EditorTab, first: usize, last: usize) -> Option<DeletedEdit> {
    let (first, last) = clamped_line_span(tab, first, last);
    let deleted = extract_lines(tab, first, last);
    let change = replace_lines_change(tab, first, last, &[])?;
    Some((deleted, EditRequest::other_break(TextChangeSet::single(change)).with_selection_after(SelectionAfter::CursorPositionBeforeLineEnd(Position::new(first, 0)))))
}

pub(crate) fn surround_range(tab: &EditorTab, from: Position, to: Position, open: char, close: char) -> Option<EditRequest> {
    if from.line >= tab.line_count() || to.line >= tab.line_count() {
        return None;
    }
    let open_char = position_start_char(tab.buffer(), from);
    let close_char = inclusive_position_to_exclusive_char(tab.buffer(), to);
    Some(EditRequest::other_at_position(vec![TextChange::insert(open_char, open), TextChange::insert(close_char, close)], from))
}
