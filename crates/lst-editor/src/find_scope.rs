use crate::{
    selection::SelectionSet,
    transaction::{TextChangeOffsetMap, TextChangeSet},
};
use std::ops::Range;

/// One capture, independent of the selections subsequently used for navigation.
/// Empty ranges may result from deletion; they never turn into document scope.
#[derive(Clone, Debug)]
pub(crate) struct CapturedFindScope {
    ranges: Vec<Range<usize>>,
}

impl CapturedFindScope {
    pub(crate) fn capture(selections: &SelectionSet) -> Option<Self> {
        let ranges: Vec<_> = selections
            .as_slice()
            .iter()
            .map(|s| s.range())
            .filter(|r| !r.is_empty())
            .collect();
        if ranges.is_empty() {
            return None;
        }
        let mut scope = Self { ranges };
        scope.coalesce();
        Some(scope)
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.ranges
            .capacity()
            .saturating_mul(std::mem::size_of::<Range<usize>>())
    }

    pub(crate) fn clear(&mut self) {
        self.ranges.clear();
    }

    pub(crate) fn contains(&self, range: &Range<usize>) -> bool {
        let after = self.ranges.partition_point(|r| r.start <= range.start);
        after.checked_sub(1).is_some_and(|i| range.end <= self.ranges[i].end)
    }

    pub(crate) fn map_edit(&mut self, changes: &TextChangeSet) {
        self.map_offsets(changes.offset_map());
    }

    fn map_offsets(&mut self, map: &TextChangeOffsetMap) {
        for range in &mut self.ranges {
            *range = map.map_offset_before(range.start)..map.map_offset(range.end);
        }
        self.coalesce();
    }

    pub(crate) fn map_history(&mut self, changes: &[TextChangeOffsetMap], forward: bool) {
        if forward {
            for change in changes {
                self.map_offsets(change);
            }
        } else {
            for change in changes.iter().rev() {
                self.map_offsets(&change.inverted());
            }
        }
    }

    fn coalesce(&mut self) {
        let mut count = 0;
        for i in 0..self.ranges.len() {
            let range = self.ranges[i].clone();
            if count > 0 && range.start <= self.ranges[count - 1].end {
                self.ranges[count - 1].end = self.ranges[count - 1].end.max(range.end);
            } else {
                self.ranges[count] = range;
                count += 1;
            }
        }
        self.ranges.truncate(count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Selection;

    #[test]
    fn captures_coalesce_touching_ranges_ignore_carets_and_require_whole_matches() {
        let selections = SelectionSet::from_selections(
            vec![
                Selection::new(0, 2),
                Selection::new(4, 2),
                Selection::collapsed(5),
                Selection::new(6, 8),
            ],
            2,
        )
        .unwrap();
        let scope = CapturedFindScope::capture(&selections).unwrap();
        assert_eq!(scope.ranges, vec![0..4, 6..8]);
        for range in [0..0, 2..2, 4..4, 0..4, 6..8, 8..8] {
            assert!(scope.contains(&range), "{range:?}");
        }
        for range in [3..5, 4..6, 5..5, 7..9] {
            assert!(!scope.contains(&range), "{range:?}");
        }
        assert!(CapturedFindScope::capture(&SelectionSet::single(Selection::collapsed(0))).is_none());
    }
}
