use std::ops::Range;

use crate::{
    document::{EditKind, UndoBoundary},
    selection::{Selection, SelectionSet},
    tab::EditorTab,
    transaction::{offset_with_delta, EditRequest, SelectionAfter, TextChange, TextChangeSet},
};

pub(crate) enum SelectionEditAfter {
    AbsoluteCursor(usize),
    InsertedRange(Range<usize>, bool),
}

pub(crate) struct SelectionEdit {
    pub(crate) change: TextChange,
    pub(crate) selection_after: SelectionEditAfter,
}

impl SelectionEdit {
    pub(crate) fn insert_with_absolute_cursor(offset: usize, cursor: usize) -> Self {
        Self {
            change: TextChange::insert(offset, ""),
            selection_after: SelectionEditAfter::AbsoluteCursor(cursor),
        }
    }

    pub(crate) fn replace_with_collapsed_end(range: Range<usize>, replacement: String) -> Self {
        let len = replacement.chars().count();
        Self::replace_with_inserted_range(range, replacement, len..len, false)
    }

    pub(crate) fn replace_with_inserted_range(
        range: Range<usize>,
        replacement: String,
        selection_after: Range<usize>,
        reversed: bool,
    ) -> Self {
        Self {
            change: TextChange::replace(range, replacement),
            selection_after: SelectionEditAfter::InsertedRange(selection_after, reversed),
        }
    }
}

pub(crate) fn request_for_each<F>(
    tab: &EditorTab,
    kind: EditKind,
    boundary: UndoBoundary,
    per_selection: F,
) -> Option<EditRequest>
where
    F: Fn(usize, Selection) -> Option<SelectionEdit>,
{
    let selection_set = tab.selection_set();
    if !selection_set.has_multiple() || tab.marked_range().is_some() {
        return None;
    }

    let edits = selection_set
        .as_slice()
        .iter()
        .copied()
        .enumerate()
        .map(|(index, selection)| per_selection(index, selection))
        .collect::<Option<Vec<_>>>()?;

    let mut delta = 0isize;
    let mut changes = Vec::with_capacity(edits.len());
    let mut selections_after = Vec::with_capacity(edits.len());

    for edit in edits {
        let inserted_start = offset_with_delta(edit.change.range.start, delta);
        let inserted_len = edit.change.replacement.chars().count();
        let selection = match edit.selection_after {
            SelectionEditAfter::AbsoluteCursor(cursor) => Selection::collapsed(cursor),
            SelectionEditAfter::InsertedRange(range, reversed) => {
                let start = range.start.min(inserted_len);
                let end = range.end.min(inserted_len);
                Selection::from_range(inserted_start + start..inserted_start + end, reversed)
            }
        };

        delta += inserted_len as isize - (edit.change.range.end - edit.change.range.start) as isize;
        changes.push(edit.change);
        selections_after.push(selection);
    }

    let changes = TextChangeSet::try_new(changes, selection_set.primary_index())?;
    let selections_after = SelectionSet::from_selections_coalescing_cursors(
        selections_after,
        selection_set.primary_index(),
    )
    .ok()?;

    Some(
        EditRequest::from_changes(kind, boundary, changes)
            .with_selection_after(SelectionAfter::Exact(selections_after)),
    )
}
