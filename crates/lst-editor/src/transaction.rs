use crate::{
    document::{EditKind, UndoBoundary},
    selection::{Position, SelectionSet},
};
use ropey::Rope;
use std::{cell::OnceCell, ops::Range};

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

#[derive(Clone, Debug)]
pub(crate) struct TextChangeSet {
    changes: Vec<TextChange>,
    primary: usize,
    offset_map: OnceCell<TextChangeOffsetMap>,
}

impl PartialEq for TextChangeSet {
    fn eq(&self, other: &Self) -> bool {
        self.changes == other.changes && self.primary == other.primary
    }
}
impl Eq for TextChangeSet {}

/// Position-only mapping for one simultaneous edit batch. The before/after
/// ranges are ordered in their respective documents; replacement text is not
/// retained. History can invert this same map without reconstructing edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TextChangeOffsetMap {
    Single((Range<usize>, Range<usize>)),
    Batch(Vec<(Range<usize>, Range<usize>)>),
}

impl TextChangeOffsetMap {
    fn for_changes(changes: &[TextChange]) -> Self {
        if let [change] = changes {
            let start = change.range.start;
            return Self::Single((change.range.clone(), start..start + change.replacement.chars().count()));
        }
        let mut delta = 0isize;
        Self::Batch(
            changes
                .iter()
                .map(|change| {
                    let inserted = change.replacement.chars().count();
                    let start = offset_with_delta(change.range.start, delta);
                    delta += inserted as isize - change.range.len() as isize;
                    (change.range.clone(), start..start + inserted)
                })
                .collect(),
        )
    }

    fn ranges(&self) -> &[(Range<usize>, Range<usize>)] {
        match self {
            Self::Single(change) => std::slice::from_ref(change),
            Self::Batch(changes) => changes,
        }
    }

    pub(crate) fn map_offset_before(&self, offset: usize) -> usize {
        let end = self.ranges().partition_point(|(before, _)| before.start < offset);
        let Some(index) = end.checked_sub(1) else {
            return offset;
        };
        let (before, result) = &self.ranges()[index];
        if offset < before.end {
            result.start
        } else {
            result.end + (offset - before.end)
        }
    }

    pub(crate) fn map_offset(&self, offset: usize) -> usize {
        let end = self.ranges().partition_point(|(before, _)| before.start <= offset);
        let Some(index) = end.checked_sub(1) else {
            return offset;
        };
        let (before, result) = &self.ranges()[index];
        result.end + offset.saturating_sub(before.end)
    }

    pub(crate) fn inverted(&self) -> Self {
        match self {
            Self::Single((before, after)) => Self::Single((after.clone(), before.clone())),
            Self::Batch(changes) => Self::Batch(
                changes
                    .iter()
                    .map(|(before, after)| (after.clone(), before.clone()))
                    .collect(),
            ),
        }
    }

    /// Streaming typing/backspace can be retained as one position change.
    /// Other edits keep their batch boundaries and endpoint affinities.
    pub(crate) fn extend_streaming(&mut self, next: &Self) -> bool {
        let (Self::Single((before, after)), Self::Single((next_before, next_after))) = (self, next) else {
            return false;
        };
        if before.is_empty() && next_before.is_empty() && next_before.start == after.end {
            after.end = next_after.end;
            true
        } else if after.is_empty() && next_after.is_empty() && next_before.start == before.start {
            before.end += next_before.len();
            true
        } else if after.is_empty() && next_after.is_empty() && next_before.end == before.start {
            before.start = next_before.start;
            *after = next_after.clone();
            true
        } else {
            false
        }
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::Single(_) => 0,
                Self::Batch(changes) => changes.capacity() * std::mem::size_of::<(Range<usize>, Range<usize>)>(),
            }
    }
}

impl TextChangeSet {
    pub(crate) fn single(change: TextChange) -> Self {
        Self {
            changes: vec![change.with_ordered_range()],
            primary: 0,
            offset_map: OnceCell::new(),
        }
    }

    pub(crate) fn new(changes: Vec<TextChange>, primary: usize) -> Self {
        Self::try_new(changes, primary).expect("text changes must be non-empty, sorted, and disjoint")
    }

    /// Returns `None` when `changes` is empty, when ranges overlap or are
    /// out of order, or when `primary` is out of range. Reversed ranges are
    /// silently normalized.
    pub(crate) fn try_new(changes: Vec<TextChange>, primary: usize) -> Option<Self> {
        let changes: Vec<TextChange> = changes.into_iter().map(TextChange::with_ordered_range).collect();
        (primary < changes.len() && valid_change_order(&changes)).then_some(Self {
            changes,
            primary,
            offset_map: OnceCell::new(),
        })
    }

    pub(crate) fn as_slice(&self) -> &[TextChange] {
        &self.changes
    }

    pub(crate) fn offset_map(&self) -> &TextChangeOffsetMap {
        self.offset_map
            .get_or_init(|| TextChangeOffsetMap::for_changes(&self.changes))
    }

    pub(crate) fn normalized_for_len(mut self, len: usize) -> Self {
        // Ordered disjoint ranges make the final endpoint the maximum.
        if self.changes.last().expect("non-empty changes").range.end > len {
            for change in &mut self.changes {
                change.range = clamped_range(change.range.clone(), len);
            }
            self.offset_map.take();
        }
        self
    }

    pub(crate) fn primary_inserted_range(&self) -> Range<usize> {
        self.offset_map().ranges()[self.primary].1.clone()
    }

    /// Transfer both the edits and their single derived position map.
    pub(crate) fn into_parts(self) -> (Vec<TextChange>, TextChangeOffsetMap) {
        let map = self
            .offset_map
            .into_inner()
            .unwrap_or_else(|| TextChangeOffsetMap::for_changes(&self.changes));
        (self.changes, map)
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
    pub(crate) fn single(kind: EditKind, boundary: UndoBoundary, range: Range<usize>, replacement: String) -> Self {
        Self::from_changes(
            kind,
            boundary,
            TextChangeSet::single(TextChange::replace(range, replacement)),
        )
    }

    pub(crate) fn from_changes(kind: EditKind, boundary: UndoBoundary, changes: TextChangeSet) -> Self {
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

    pub(crate) fn other_with_selection(changes: Vec<TextChange>, selection_after: SelectionAfter) -> Self {
        Self::other_break(TextChangeSet::new(changes, 0)).with_selection_after(selection_after)
    }

    pub(crate) fn other_at_position(changes: Vec<TextChange>, position: Position) -> Self {
        Self::other_with_selection(changes, SelectionAfter::CursorPosition(position))
    }

    pub(crate) fn single_other_at_position(change: TextChange, position: Position) -> Self {
        Self::other_break(TextChangeSet::single(change)).with_selection_after(SelectionAfter::CursorPosition(position))
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
    /// Map the existing selections, staying before insertions at their ends.
    Map,
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
        && changes
            .windows(2)
            .all(|pair| pair[0].range.start <= pair[1].range.start && pair[0].range.end <= pair[1].range.start)
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

pub(crate) fn clamped_range(range: Range<usize>, len: usize) -> Range<usize> {
    ordered_range(range.start.min(len), range.end.min(len))
}

pub(crate) fn inserted_relative_range(inserted_range: Range<usize>, range: Range<usize>) -> Range<usize> {
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
    fn clamping_changes_refreshes_derived_positions_without_changing_value_equality() {
        let changes = TextChangeSet::single(TextChange::replace(2..8, "é"));
        let untouched = changes.clone();
        assert_eq!(changes.offset_map().map_offset(9), 4);
        assert_eq!(changes, untouched);
        let bounded = changes.normalized_for_len(4);
        assert_eq!(bounded.offset_map().map_offset(5), 4);
        assert_eq!(bounded.primary_inserted_range(), 2..3);
        assert_eq!(bounded, TextChangeSet::single(TextChange::replace(2..4, "é")));
    }

    #[test]
    fn positions_can_stay_before_insertions_and_collapse_into_deleted_text() {
        let changes = TextChangeSet::new(
            vec![
                TextChange::delete(2..4),
                TextChange::replace(6..8, "é🦀"),
                TextChange::insert(8, "\r\n"),
            ],
            0,
        );
        let map = changes.offset_map();
        let expected = [0, 1, 2, 2, 2, 3, 4, 4, 6, 9];
        for (offset, expected) in expected.into_iter().enumerate() {
            assert_eq!(map.map_offset_before(offset), expected, "{offset}");
        }
    }

    /// Linear reference for the indexed offset mapping.
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

    fn map_offset_before_linear(changes: &[TextChange], offset: usize) -> usize {
        let mut delta = 0isize;
        for change in changes {
            if offset <= change.range.start {
                break;
            }
            if offset < change.range.end {
                return offset_with_delta(change.range.start, delta);
            }
            delta += change.replacement.chars().count() as isize - change.range.len() as isize;
        }
        offset_with_delta(offset, delta)
    }

    #[test]
    fn streaming_compaction_preserves_both_affinities_in_both_directions() {
        for (first, next) in [
            (TextChange::insert(2, "ab"), TextChange::insert(4, "xyz")),
            (TextChange::delete(2..4), TextChange::delete(2..5)),
            (TextChange::delete(4..6), TextChange::delete(2..4)),
        ] {
            let first = TextChangeSet::single(first).into_parts().1;
            let next = TextChangeSet::single(next).into_parts().1;
            let mut combined = first.clone();
            assert!(combined.extend_streaming(&next));
            for offset in 0..12 {
                assert_eq!(combined.map_offset(offset), next.map_offset(first.map_offset(offset)));
                assert_eq!(
                    combined.map_offset_before(offset),
                    next.map_offset_before(first.map_offset_before(offset))
                );
                let inverse = combined.inverted();
                assert_eq!(
                    inverse.map_offset(offset),
                    first.inverted().map_offset(next.inverted().map_offset(offset))
                );
                assert_eq!(
                    inverse.map_offset_before(offset),
                    first
                        .inverted()
                        .map_offset_before(next.inverted().map_offset_before(offset))
                );
            }
        }
    }

    #[test]
    fn indexed_offsets_match_linear_mapping_at_every_boundary() {
        // Include touching replacements, repeated zero-length insertions,
        // Unicode growth, deletions, and positions before/after all changes.
        for a in 0..=4 {
            for b in a..=4 {
                for c in b..=5 {
                    for d in c..=5 {
                        for first in ["", "x", "é🦀"] {
                            for second in ["", "ab", "e\u{301}"] {
                                let changes = TextChangeSet::new(
                                    vec![
                                        TextChange::replace(a..b, first),
                                        TextChange::replace(c..d, second),
                                        TextChange::insert(d, "末"),
                                    ],
                                    1,
                                );
                                let map = changes.offset_map();
                                for offset in (0..=8).rev().chain(0..=8) {
                                    assert_eq!(
                                        map.map_offset_before(offset),
                                        map_offset_before_linear(changes.as_slice(), offset)
                                    );
                                    let inverse = map.inverted();
                                    let inverse_changes: Vec<_> = map
                                        .ranges()
                                        .iter()
                                        .map(|(before, after)| {
                                            TextChange::replace(after.clone(), "x".repeat(before.len()))
                                        })
                                        .collect();
                                    assert_eq!(
                                        inverse.map_offset(offset),
                                        map_offset_to_inserted_end(&inverse_changes, offset)
                                    );
                                    assert_eq!(
                                        inverse.map_offset_before(offset),
                                        map_offset_before_linear(&inverse_changes, offset)
                                    );
                                    assert_eq!(
                                        map.map_offset(offset),
                                        map_offset_to_inserted_end(changes.as_slice(), offset),
                                        "changes={changes:?}, offset={offset}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
