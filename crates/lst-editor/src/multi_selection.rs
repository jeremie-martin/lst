use std::ops::Range;

use crate::{
    document::{EditKind, UndoBoundary},
    selection::{Selection, SelectionSet},
    tab::EditorTab,
    transaction::{offset_with_delta, EditRequest, SelectionAfter, TextChange, TextChangeSet},
};

pub(crate) fn replacement_request(
    tab: &EditorTab,
    text: String,
    boundary: UndoBoundary,
) -> Option<EditRequest> {
    let selection_set = tab.selection_set();
    if !selection_set.has_multiple() || tab.marked_range().is_some() {
        return None;
    }

    let replacement_len = text.chars().count();
    let mut delta = 0isize;
    let mut changes = Vec::with_capacity(selection_set.as_slice().len());
    let mut selections_after = Vec::with_capacity(selection_set.as_slice().len());
    for selection in selection_set.as_slice() {
        let range = selection.range();
        let inserted_start = offset_with_delta(range.start, delta);
        selections_after.push(Selection::collapsed(inserted_start + replacement_len));
        delta += replacement_len as isize - (range.end - range.start) as isize;
        changes.push(TextChange::replace(range, text.clone()));
    }

    let changes = TextChangeSet::new(changes, selection_set.primary_index());
    let selection_after = SelectionSet::from_selections_coalescing_cursors(
        selections_after,
        selection_set.primary_index(),
    )
    .expect("multi-selection replacement preserves a valid selection set");
    let kind = if text.is_empty() {
        EditKind::Delete
    } else {
        EditKind::Insert
    };
    Some(
        EditRequest::from_changes(kind, boundary, changes)
            .with_selection_after(SelectionAfter::Exact(selection_after)),
    )
}

pub(crate) fn delete_request<F>(
    tab: &EditorTab,
    boundary: UndoBoundary,
    cursor_range: F,
) -> Option<EditRequest>
where
    F: Fn(&EditorTab, usize) -> Option<Range<usize>>,
{
    let selection_set = tab.selection_set();
    if !selection_set.has_multiple() || tab.marked_range().is_some() {
        return None;
    }

    let primary = selection_set.primary_index();
    let mut requested_ranges = Vec::new();
    let mut caret_offsets = Vec::with_capacity(selection_set.as_slice().len());

    for (selection_index, selection) in selection_set.as_slice().iter().copied().enumerate() {
        let range = if selection.has_selection() {
            Some(selection.range())
        } else {
            cursor_range(tab, selection.cursor())
        };

        if let Some(range) = range.filter(|range| range.start < range.end) {
            caret_offsets.push(range.start);
            requested_ranges.push((selection_index, range));
        } else {
            caret_offsets.push(selection.cursor());
        }
    }

    if requested_ranges.is_empty() {
        return None;
    }

    requested_ranges.sort_by_key(|(_, range)| (range.start, range.end));
    let merged_ranges =
        merge_delete_ranges(requested_ranges.iter().map(|(_, range)| range.clone()));
    let changes: Vec<TextChange> = merged_ranges
        .iter()
        .cloned()
        .map(TextChange::delete)
        .collect();
    let primary_change = requested_ranges
        .iter()
        .find(|(selection_index, _)| *selection_index == primary)
        .and_then(|(_, range)| {
            merged_ranges
                .iter()
                .position(|merged| merged.start <= range.start && range.end <= merged.end)
        })
        .unwrap_or(0);

    let changes = TextChangeSet::new(changes, primary_change);
    let selections_after: Vec<Selection> = caret_offsets
        .into_iter()
        .map(|offset| Selection::collapsed(changes.map_offset_to_inserted_end(offset)))
        .collect();
    let selection_after =
        SelectionSet::from_selections_coalescing_cursors(selections_after, primary)
            .expect("multi-selection delete preserves a valid selection set");
    Some(
        EditRequest::from_changes(EditKind::Delete, boundary, changes)
            .with_selection_after(SelectionAfter::Exact(selection_after)),
    )
}

fn merge_delete_ranges(ranges: impl IntoIterator<Item = Range<usize>>) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut() {
            if range.start <= last.end {
                last.end = last.end.max(range.end);
                continue;
            }
        }
        merged.push(range);
    }
    merged
}
