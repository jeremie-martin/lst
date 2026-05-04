use crate::{
    document::{EditKind, UndoBoundary},
    position::Position,
    selection::SelectionSet,
};
use ropey::Rope;
use std::ops::Range;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EditOutcome {
    pub(crate) text_changed: bool,
    pub(crate) selection_changed: bool,
    pub(crate) marked_range_changed: bool,
}

impl EditOutcome {
    pub(crate) fn changed(self) -> bool {
        self.text_changed || self.selection_changed || self.marked_range_changed
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextChange {
    pub range: Range<usize>,
    pub replacement: String,
}

impl TextChange {
    pub(crate) fn replace(range: Range<usize>, replacement: impl Into<String>) -> Self {
        Self {
            range,
            replacement: replacement.into(),
        }
    }

    pub(crate) fn insert(offset: usize, replacement: impl Into<String>) -> Self {
        Self::replace(offset..offset, replacement)
    }

    pub(crate) fn delete(range: Range<usize>) -> Self {
        Self::replace(range, String::new())
    }

    fn with_ordered_range(self) -> Self {
        Self {
            range: ordered_range(self.range.start, self.range.end),
            replacement: self.replacement,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextChangeSet {
    changes: Vec<TextChange>,
    primary: usize,
}

impl TextChangeSet {
    pub(crate) fn single(change: TextChange) -> Self {
        Self {
            changes: vec![change.with_ordered_range()],
            primary: 0,
        }
    }

    pub(crate) fn new(changes: Vec<TextChange>, primary: usize) -> Self {
        Self::try_new(changes, primary)
            .expect("text changes must be non-empty, sorted, and disjoint")
    }

    fn try_new(changes: Vec<TextChange>, primary: usize) -> Option<Self> {
        let changes: Vec<TextChange> = changes
            .into_iter()
            .map(TextChange::with_ordered_range)
            .collect();
        (primary < changes.len() && valid_change_order(&changes))
            .then_some(Self { changes, primary })
    }

    pub(crate) fn as_slice(&self) -> &[TextChange] {
        &self.changes
    }

    #[cfg(test)]
    pub(crate) fn primary_index(&self) -> usize {
        self.primary
    }

    pub(crate) fn map_offset_to_inserted_end(&self, offset: usize) -> usize {
        map_offset_to_inserted_end(&self.changes, offset)
    }

    pub(crate) fn normalized_for_len(&self, len: usize) -> Self {
        let changes = self
            .changes
            .iter()
            .map(|change| {
                TextChange::replace(
                    clamped_range(change.range.clone(), len),
                    change.replacement.clone(),
                )
            })
            .collect();
        Self {
            changes,
            primary: self.primary,
        }
    }

    pub(crate) fn primary_inserted_range(&self) -> Range<usize> {
        inserted_range_for_change(&self.changes, self.primary)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditRequest {
    pub(crate) kind: EditKind,
    pub(crate) boundary: UndoBoundary,
    pub(crate) changes: TextChangeSet,
    pub(crate) selection_after: SelectionAfter,
    pub(crate) marked_range_after: Option<Range<usize>>,
}

impl EditRequest {
    pub(crate) fn single(
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        replacement: String,
    ) -> Self {
        Self::from_changes(
            kind,
            boundary,
            TextChangeSet::single(TextChange::replace(range, replacement)),
        )
    }

    pub(crate) fn from_changes(
        kind: EditKind,
        boundary: UndoBoundary,
        changes: TextChangeSet,
    ) -> Self {
        Self {
            kind,
            boundary,
            changes,
            selection_after: SelectionAfter::CollapseToInsertedEnd,
            marked_range_after: None,
        }
    }

    pub(crate) fn other_break(changes: TextChangeSet) -> Self {
        Self::from_changes(EditKind::Other, UndoBoundary::Break, changes)
    }

    pub(crate) fn other_with_selection(
        changes: Vec<TextChange>,
        selection_after: SelectionAfter,
    ) -> Self {
        Self::other_break(TextChangeSet::new(changes, 0)).with_selection_after(selection_after)
    }

    pub(crate) fn other_at_position(changes: Vec<TextChange>, position: Position) -> Self {
        Self::other_with_selection(changes, SelectionAfter::CursorPosition(position))
    }

    pub(crate) fn single_other_at_position(change: TextChange, position: Position) -> Self {
        Self::other_break(TextChangeSet::single(change))
            .with_selection_after(SelectionAfter::CursorPosition(position))
    }

    pub(crate) fn with_selection_after(mut self, selection_after: SelectionAfter) -> Self {
        self.selection_after = selection_after;
        self
    }

    pub(crate) fn with_marked_range_after(mut self, range: Range<usize>) -> Self {
        self.marked_range_after = Some(range);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SelectionAfter {
    /// Caret at the end of the primary inserted span.
    CollapseToInsertedEnd,
    /// Final selection set in post-transaction character coordinates.
    Exact(SelectionSet),
    /// Final collapsed cursor in post-transaction line/column coordinates.
    CursorPosition(Position),
    /// Final collapsed cursor clamped to the last displayed char on its line.
    CursorPositionBeforeLineEnd(Position),
    /// Final selection range in post-transaction line/column coordinates.
    PositionRange {
        start: Position,
        end: Position,
        reversed: bool,
    },
    /// Char offsets relative to the start of the primary inserted text.
    InsertedRange { range: Range<usize>, reversed: bool },
}

fn valid_change_order(changes: &[TextChange]) -> bool {
    !changes.is_empty()
        && changes.windows(2).all(|pair| {
            pair[0].range.start <= pair[1].range.start && pair[0].range.end <= pair[1].range.start
        })
}

pub(crate) fn apply_change_to_buffer(buffer: &mut Rope, change: &TextChange) {
    if change.range.start != change.range.end {
        buffer.remove(change.range.clone());
    }
    if !change.replacement.is_empty() {
        buffer.insert(change.range.start, &change.replacement);
    }
}

pub(crate) fn offset_with_delta(offset: usize, delta: isize) -> usize {
    if delta >= 0 {
        offset + delta as usize
    } else {
        offset.saturating_sub(delta.unsigned_abs())
    }
}

fn map_offset_to_inserted_end(changes: &[TextChange], offset: usize) -> usize {
    let mut delta = 0isize;
    let mut mapped = None;
    for change in changes {
        if offset < change.range.start {
            break;
        }

        let inserted_len = change.replacement.chars().count();
        if offset <= change.range.end {
            mapped = Some(offset_with_delta(change.range.start, delta) + inserted_len);
        }

        let removed_len = change.range.end - change.range.start;
        delta += inserted_len as isize - removed_len as isize;
    }
    mapped.unwrap_or_else(|| offset_with_delta(offset, delta))
}

fn inserted_range_for_change(changes: &[TextChange], primary: usize) -> Range<usize> {
    let mut delta = 0isize;
    for change in &changes[..primary] {
        let inserted_len = change.replacement.chars().count();
        let removed_len = change.range.end - change.range.start;
        delta += inserted_len as isize - removed_len as isize;
    }

    let change = &changes[primary];
    let inserted_start = offset_with_delta(change.range.start, delta);
    let inserted_len = change.replacement.chars().count();
    inserted_start..inserted_start + inserted_len
}

pub(crate) fn clamped_range(range: Range<usize>, len: usize) -> Range<usize> {
    ordered_range(range.start.min(len), range.end.min(len))
}

pub(crate) fn inserted_relative_range(
    inserted_range: Range<usize>,
    range: Range<usize>,
) -> Range<usize> {
    let inserted_len = inserted_range.end - inserted_range.start;
    let range = ordered_range(range.start.min(inserted_len), range.end.min(inserted_len));
    inserted_range.start + range.start..inserted_range.start + range.end
}

pub(crate) fn ordered_range(start: usize, end: usize) -> Range<usize> {
    if start <= end {
        start..end
    } else {
        end..start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_normalizes_ranges_and_preserves_primary() {
        let set = TextChangeSet::try_new(
            vec![
                TextChange::delete(0..0),
                TextChange::delete(Range { start: 4, end: 2 }),
                TextChange::delete(6..6),
            ],
            1,
        )
        .expect("valid sorted changes after normalizing reversed ranges");

        assert_eq!(set.as_slice().len(), 3);
        assert_eq!(set.as_slice()[1].range, 2..4);
        assert_eq!(set.primary_index(), 1);
    }

    #[test]
    fn validated_preserves_same_offset_insert_order_and_mapping() {
        let set = TextChangeSet::try_new(
            vec![
                TextChange::insert(1, "A"),
                TextChange::insert(1, "B"),
                TextChange::replace(1..3, "X"),
                TextChange::insert(3, "Y"),
            ],
            0,
        )
        .expect("valid same-offset insert batch");

        assert_eq!(set.as_slice()[0].replacement, "A");
        assert_eq!(set.as_slice()[1].replacement, "B");
        assert_eq!(set.map_offset_to_inserted_end(1), 4);
        assert_eq!(set.map_offset_to_inserted_end(3), 5);
    }

    #[cfg(feature = "internal-invariants")]
    #[test]
    fn validated_rejects_empty_out_of_order_or_overlapping_change_sets() {
        assert!(TextChangeSet::try_new(Vec::new(), 0).is_none());
        assert!(TextChangeSet::try_new(vec![TextChange::delete(1..1)], 1).is_none());
        assert!(TextChangeSet::try_new(
            vec![TextChange::delete(0..3), TextChange::delete(2..4)],
            0
        )
        .is_none());
        assert!(TextChangeSet::try_new(
            vec![TextChange::delete(4..5), TextChange::delete(1..2)],
            0
        )
        .is_none());
    }
}
