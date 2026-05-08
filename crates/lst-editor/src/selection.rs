use ropey::Rope;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Editor selection as an `(anchor, head)` pair of char offsets.
///
/// `anchor` is the fixed end of the selection (where it started); `head` is
/// the active end where the cursor lives. When `anchor == head` the selection
/// is collapsed and acts as a plain cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    anchor: usize,
    head: usize,
}

impl Selection {
    pub fn collapsed(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn from_range(range: Range<usize>, reversed: bool) -> Self {
        if reversed {
            Self {
                anchor: range.end,
                head: range.start,
            }
        } else {
            Self {
                anchor: range.start,
                head: range.end,
            }
        }
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn head(&self) -> usize {
        self.head
    }

    pub fn cursor(&self) -> usize {
        self.head
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }

    pub fn move_to(&mut self, offset: usize) {
        self.anchor = offset;
        self.head = offset;
    }

    pub fn select_to(&mut self, offset: usize) {
        self.head = offset;
    }
}

/// Programmatic multi-cursor selection state for the editor model.
///
/// A `SelectionSet` is always non-empty, sorted by selected range, and
/// non-overlapping. One selection is primary; commands that intentionally
/// collapse multi-cursor state use that primary selection as the surviving
/// cursor. This type is part of the model API so tests and host applications
/// can exercise multi-cursor behavior before the GPUI gesture surface exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSet {
    selections: Vec<Selection>,
    primary: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorGoal {
    Column(usize),
    LineEnd,
}

impl CursorGoal {
    pub(crate) fn column(self) -> Option<usize> {
        match self {
            Self::Column(column) => Some(column),
            Self::LineEnd => None,
        }
    }

    pub(crate) fn resolve(self, line_len: usize) -> usize {
        match self {
            Self::Column(column) => column.min(line_len),
            Self::LineEnd => line_len,
        }
    }
}

/// Complete selection state owned by a tab.
///
/// `SelectionSet` owns the structural cursor/selection invariant. This wrapper
/// keeps movement metadata in lockstep with that set so callers cannot retain a
/// preferred-column vector that no longer matches the active selections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionState {
    set: SelectionSet,
    goals: CursorGoals,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelectionTransform {
    pub(crate) selection: Selection,
    pub(crate) movement_goal: Option<CursorGoal>,
    pub(crate) visible_column: Option<usize>,
}

impl SelectionTransform {
    pub(crate) fn new(selection: Selection) -> Self {
        Self {
            selection,
            movement_goal: None,
            visible_column: None,
        }
    }

    pub(crate) fn with_columns(
        selection: Selection,
        movement_goal: CursorGoal,
        visible_column: Option<usize>,
    ) -> Self {
        Self {
            selection,
            movement_goal: Some(movement_goal),
            visible_column,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct CursorGoals {
    movement: Option<Vec<CursorGoal>>,
    visible: Option<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionSetError {
    Empty,
    InvalidPrimary,
    Unordered,
    Overlapping,
}

impl SelectionSet {
    pub fn single(selection: Selection) -> Self {
        Self {
            selections: vec![selection],
            primary: 0,
        }
    }

    /// Builds a selection set after validating ordering, overlap, and primary
    /// index. Use `from_selections_coalescing_cursors` internally when an edit
    /// can legitimately produce duplicate collapsed cursors.
    pub fn from_selections(
        selections: Vec<Selection>,
        primary: usize,
    ) -> Result<Self, SelectionSetError> {
        validate_selection_set(&selections, primary)?;
        Ok(Self {
            selections,
            primary,
        })
    }

    pub fn primary(&self) -> Selection {
        self.selections[self.primary]
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.selections
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn is_single(&self) -> bool {
        self.selections.len() == 1
    }

    pub(crate) fn has_multiple(&self) -> bool {
        self.selections.len() > 1
    }

    pub(crate) fn with_added_selection(&self, selection: Selection) -> Self {
        self.with_added_selections([selection])
    }

    /// Returns a set with `additions` merged in. The most recently added
    /// selection becomes primary; on overlap with an existing selection the
    /// addition is silently dropped and the original set is returned, so
    /// callers can safely ignore "no change" results. Duplicate collapsed
    /// cursors are coalesced.
    pub(crate) fn with_added_selections<I>(&self, additions: I) -> Self
    where
        I: IntoIterator<Item = Selection>,
    {
        let mut selections: Vec<(Selection, SelectionOrigin)> = self
            .selections
            .iter()
            .copied()
            .enumerate()
            .map(|(index, selection)| {
                let origin = if index == self.primary {
                    SelectionOrigin::ExistingPrimary
                } else {
                    SelectionOrigin::Existing
                };
                (selection, origin)
            })
            .collect();
        selections.extend(
            additions
                .into_iter()
                .map(|selection| (selection, SelectionOrigin::Added)),
        );
        Self::from_unordered_intent(selections).unwrap_or_else(|| self.clone())
    }

    pub(crate) fn from_selections_coalescing_cursors(
        selections: Vec<Selection>,
        primary: usize,
    ) -> Result<Self, SelectionSetError> {
        validate_primary(&selections, primary)?;

        let mut coalesced = Vec::with_capacity(selections.len());
        let mut coalesced_primary = None;
        for (index, selection) in selections.into_iter().enumerate() {
            let duplicate_cursor = !selection.has_selection()
                && coalesced.last().is_some_and(|last: &Selection| {
                    !last.has_selection() && last.cursor() == selection.cursor()
                });
            if duplicate_cursor {
                if index == primary {
                    coalesced_primary = coalesced.len().checked_sub(1);
                }
                continue;
            }

            if index == primary {
                coalesced_primary = Some(coalesced.len());
            }
            coalesced.push(selection);
        }

        let primary = coalesced_primary.unwrap_or(0);
        Self::from_selections(coalesced, primary)
    }

    fn from_unordered_intent(mut selections: Vec<(Selection, SelectionOrigin)>) -> Option<Self> {
        if selections.is_empty() {
            return None;
        }
        selections.sort_by_key(|(selection, origin)| {
            let range = selection.range();
            (range.start, range.end, origin.sort_key())
        });

        // Prefer the most-recently-added non-duplicate Added as primary —
        // that's the new edge of a multi-step extension. A naive
        // `rposition Added` would pick the duplicate that gets coalesced
        // away, leaving primary stuck on the previous row. When every
        // Added is a duplicate, fall back to the duplicate's index so the
        // coalescer redirects primary onto the targeted existing cursor.
        let added_index = selections
            .iter()
            .enumerate()
            .rev()
            .find_map(|(i, (_, origin))| {
                if *origin != SelectionOrigin::Added {
                    return None;
                }
                let range = selections[i].0.range();
                let prev_dup = i > 0
                    && selections[i - 1].0.range() == range
                    && selections[i - 1].1 != SelectionOrigin::Added;
                let next_dup = i + 1 < selections.len()
                    && selections[i + 1].0.range() == range
                    && selections[i + 1].1 != SelectionOrigin::Added;
                if prev_dup || next_dup {
                    None
                } else {
                    Some(i)
                }
            })
            .or_else(|| {
                selections
                    .iter()
                    .rposition(|(_, origin)| *origin == SelectionOrigin::Added)
            });
        if let Some(added_index) = added_index {
            if selection_overlaps_neighbor(&selections, added_index) {
                return None;
            }
        }

        let primary = added_index
            .or_else(|| {
                selections
                    .iter()
                    .position(|(_, origin)| *origin == SelectionOrigin::ExistingPrimary)
            })
            .unwrap_or(0);
        let selections: Vec<Selection> = selections
            .into_iter()
            .map(|(selection, _)| selection)
            .collect();
        Self::from_selections_coalescing_cursors(selections, primary).ok()
    }

    pub(crate) fn set_single(&mut self, selection: Selection) {
        self.selections.clear();
        self.selections.push(selection);
        self.primary = 0;
    }

    /// Returns a set with the selection at `index` removed. Returns `None`
    /// when the removal would empty the set (single-selection case) or the
    /// index is out of range. When the primary is removed, the next
    /// selection takes over (or the previous one if removing the last).
    pub(crate) fn with_removed_at(&self, index: usize) -> Option<Self> {
        if index >= self.selections.len() || self.selections.len() <= 1 {
            return None;
        }
        let mut selections = self.selections.clone();
        selections.remove(index);
        let primary = if self.primary == index {
            index.min(selections.len() - 1)
        } else if self.primary > index {
            self.primary - 1
        } else {
            self.primary
        };
        Some(Self {
            selections,
            primary,
        })
    }

    pub(crate) fn clamped_to_len(&self, len: usize) -> Self {
        let selections: Vec<Selection> = self
            .selections
            .iter()
            .map(|selection| clamped_selection(*selection, len))
            .collect();
        Self::from_selections_coalescing_cursors(selections, self.primary)
            .expect("clamping a valid selection set preserves selection-set invariants")
    }
}

impl SelectionState {
    pub fn single(selection: Selection) -> Self {
        Self {
            set: SelectionSet::single(selection),
            goals: CursorGoals::default(),
        }
    }

    pub fn from_set(set: SelectionSet) -> Self {
        Self {
            set,
            goals: CursorGoals::default(),
        }
    }

    pub fn selection_set(&self) -> &SelectionSet {
        &self.set
    }

    pub fn primary(&self) -> Selection {
        self.set.primary()
    }

    pub fn as_slice(&self) -> &[Selection] {
        self.set.as_slice()
    }

    pub fn primary_index(&self) -> usize {
        self.set.primary_index()
    }

    pub fn is_single(&self) -> bool {
        self.set.is_single()
    }

    pub(crate) fn has_multiple(&self) -> bool {
        self.set.has_multiple()
    }

    pub(crate) fn set_single(&mut self, selection: Selection) {
        self.set.set_single(selection);
        self.goals.clear();
    }

    pub(crate) fn replace_set(&mut self, set: SelectionSet) {
        self.set = set;
        self.goals.clear();
    }

    pub(crate) fn with_added_selection(&self, selection: Selection) -> Self {
        self.with_added_selections([selection])
    }

    pub(crate) fn with_added_selections<I>(&self, additions: I) -> Self
    where
        I: IntoIterator<Item = Selection>,
    {
        let next = self.set.with_added_selections(additions);
        if next == self.set {
            self.clone()
        } else {
            Self::from_set(next)
        }
    }

    pub(crate) fn with_removed_at(&self, index: usize) -> Option<Self> {
        self.set.with_removed_at(index).map(Self::from_set)
    }

    pub(crate) fn clamped_to_len(&self, len: usize) -> Self {
        let next = self.set.clamped_to_len(len);
        if next == self.set {
            Self {
                set: next,
                goals: self.goals.clone(),
            }
        } else {
            Self::from_set(next)
        }
    }

    pub(crate) fn movement_goal_for(&self, selection_index: usize) -> Option<CursorGoal> {
        self.goals.movement_for(selection_index)
    }

    pub(crate) fn movement_column_for(&self, selection_index: usize) -> Option<usize> {
        self.movement_goal_for(selection_index)
            .and_then(CursorGoal::column)
    }

    pub(crate) fn visible_column_for(&self, selection_index: usize) -> Option<usize> {
        self.goals.visible_for(selection_index)
    }

    pub(crate) fn set_all_movement_goals(&mut self, goal: Option<CursorGoal>) {
        self.goals.set_all_movement(self.set.as_slice().len(), goal);
    }

    pub(crate) fn clear_goals(&mut self) {
        self.goals.clear();
    }

    pub(crate) fn map<F>(&self, mut f: F) -> Option<Self>
    where
        F: FnMut(usize, Selection) -> SelectionTransform,
    {
        let mut entries = self
            .set
            .as_slice()
            .iter()
            .copied()
            .enumerate()
            .map(|(index, selection)| {
                let transform = f(index, selection);
                MappedSelection {
                    transform,
                    is_primary: index == self.set.primary_index(),
                }
            })
            .collect::<Vec<_>>();
        SelectionState::from_mapped_entries(&mut entries)
    }

    fn from_mapped_entries(entries: &mut [MappedSelection]) -> Option<Self> {
        entries.sort_by_key(|entry| {
            let range = entry.transform.selection.range();
            (range.start, range.end, !entry.is_primary)
        });

        let mut selections = Vec::with_capacity(entries.len());
        let mut movement_goals: Vec<Option<CursorGoal>> = Vec::with_capacity(entries.len());
        let mut visible_columns: Vec<Option<usize>> = Vec::with_capacity(entries.len());
        let mut primary = None;

        for entry in entries.iter().copied() {
            let duplicate_cursor = !entry.transform.selection.has_selection()
                && selections.last().is_some_and(|last: &Selection| {
                    !last.has_selection() && last.cursor() == entry.transform.selection.cursor()
                });
            if duplicate_cursor {
                if entry.is_primary {
                    primary = selections.len().checked_sub(1);
                    if let Some(index) = primary {
                        movement_goals[index] = entry.transform.movement_goal;
                        visible_columns[index] = entry.transform.visible_column;
                    }
                }
                continue;
            }

            if entry.is_primary {
                primary = Some(selections.len());
            }
            selections.push(entry.transform.selection);
            movement_goals.push(entry.transform.movement_goal);
            visible_columns.push(entry.transform.visible_column);
        }

        let primary = primary.unwrap_or(0);
        let set = SelectionSet::from_selections(selections, primary).ok()?;
        let goals = CursorGoals::from_optional_columns(
            set.as_slice().len(),
            movement_goals,
            visible_columns,
        );
        Some(Self { set, goals })
    }
}

#[derive(Clone, Copy)]
struct MappedSelection {
    transform: SelectionTransform,
    is_primary: bool,
}

impl CursorGoals {
    fn movement_for(&self, selection_index: usize) -> Option<CursorGoal> {
        self.movement
            .as_ref()
            .and_then(|goals| goals.get(selection_index))
            .copied()
    }

    fn visible_for(&self, selection_index: usize) -> Option<usize> {
        self.visible
            .as_ref()
            .and_then(|columns| columns.get(selection_index))
            .copied()
    }

    fn set_all_movement(&mut self, len: usize, goal: Option<CursorGoal>) {
        self.movement = goal.map(|goal| vec![goal; len]);
        self.visible = None;
    }

    fn clear(&mut self) {
        self.movement = None;
        self.visible = None;
    }

    fn from_optional_columns(
        len: usize,
        movement: Vec<Option<CursorGoal>>,
        visible: Vec<Option<usize>>,
    ) -> Self {
        let movement = (movement.len() == len)
            .then(|| movement.into_iter().collect::<Option<Vec<_>>>())
            .flatten();
        let visible = (visible.len() == len)
            .then(|| visible.into_iter().collect::<Option<Vec<_>>>())
            .flatten();
        Self { movement, visible }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionOrigin {
    Existing,
    ExistingPrimary,
    Added,
}

impl SelectionOrigin {
    fn sort_key(self) -> u8 {
        match self {
            Self::Existing => 0,
            Self::ExistingPrimary => 1,
            Self::Added => 2,
        }
    }
}

fn selection_overlaps_neighbor(selections: &[(Selection, SelectionOrigin)], index: usize) -> bool {
    let selection = selections[index].0;
    let range = selection.range();
    if index > 0 {
        let previous = selections[index - 1].0;
        let previous_range = previous.range();
        if previous_range.end > range.start || previous_range == range {
            if selection_duplicate_collapsed_cursor(selection, previous) {
                return false;
            }
            return true;
        }
    }
    if let Some((next, _)) = selections.get(index + 1) {
        let next_range = next.range();
        if range.end > next_range.start || range == next_range {
            if selection_duplicate_collapsed_cursor(selection, *next) {
                return false;
            }
            return true;
        }
    }
    false
}

fn selection_duplicate_collapsed_cursor(left: Selection, right: Selection) -> bool {
    !left.has_selection() && !right.has_selection() && left.cursor() == right.cursor()
}

fn validate_primary(selections: &[Selection], primary: usize) -> Result<(), SelectionSetError> {
    if selections.is_empty() {
        return Err(SelectionSetError::Empty);
    }
    if primary >= selections.len() {
        return Err(SelectionSetError::InvalidPrimary);
    }
    Ok(())
}

fn validate_selection_set(
    selections: &[Selection],
    primary: usize,
) -> Result<(), SelectionSetError> {
    validate_primary(selections, primary)?;

    let mut previous: Option<Range<usize>> = None;
    for selection in selections {
        let range = selection.range();
        if let Some(previous) = previous {
            if (range.start, range.end) < (previous.start, previous.end) {
                return Err(SelectionSetError::Unordered);
            }
            if previous.end > range.start || previous == range {
                return Err(SelectionSetError::Overlapping);
            }
        }
        previous = Some(range);
    }
    Ok(())
}

fn clamped_selection(selection: Selection, len: usize) -> Selection {
    Selection::new(selection.anchor().min(len), selection.head().min(len))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenClass {
    Whitespace,
    Word,
    Symbol,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SubwordClass {
    Lower,
    Upper,
    Alpha,
    Digit,
}

fn token_class(ch: char) -> TokenClass {
    if ch.is_whitespace() {
        TokenClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        TokenClass::Word
    } else {
        TokenClass::Symbol
    }
}

// Vim's "big word" (`W`/`B`/`E`) collapses Symbol into Word — only whitespace
// breaks a big-word run. With `big = false` this matches `token_class`.
pub(crate) fn vim_token_class(ch: char, big: bool) -> TokenClass {
    if big && !ch.is_whitespace() {
        TokenClass::Word
    } else {
        token_class(ch)
    }
}

pub(crate) fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_symbol_char(ch: char) -> bool {
    !ch.is_whitespace() && !is_identifier_char(ch)
}

fn subword_class(ch: char) -> Option<SubwordClass> {
    if ch == '_' || !is_identifier_char(ch) {
        None
    } else if ch.is_numeric() {
        Some(SubwordClass::Digit)
    } else if ch.is_uppercase() {
        Some(SubwordClass::Upper)
    } else if ch.is_lowercase() {
        Some(SubwordClass::Lower)
    } else {
        Some(SubwordClass::Alpha)
    }
}

// A single extended grapheme cluster within its source text.
//
// Boundary helpers walk by `GraphemeCell` instead of by `char`, so cursor
// positions and selection endpoints always land on cluster boundaries.
// Combining marks and ZWJ joiners ride along with their base scalar; each
// cluster is classified by `repr` (the first scalar) — base wins, matching
// Helix and Zed.
#[derive(Clone, Copy)]
pub(crate) struct GraphemeCell {
    pub(crate) byte_start: usize,
    pub(crate) char_start: usize,
    pub(crate) char_len: u8,
    pub(crate) repr: char,
}

pub(crate) fn cells_of_str(text: &str) -> Vec<GraphemeCell> {
    let mut cells = Vec::new();
    let mut char_start = 0usize;
    for (byte_start, cluster) in text.grapheme_indices(true) {
        let mut chars = cluster.chars();
        let Some(repr) = chars.next() else {
            continue;
        };
        let char_len = 1 + chars.count();
        debug_assert!(char_len <= u8::MAX as usize);
        cells.push(GraphemeCell {
            byte_start,
            char_start,
            char_len: char_len as u8,
            repr,
        });
        char_start += char_len;
    }
    cells
}

fn cells_of_rope(buffer: &Rope) -> Vec<GraphemeCell> {
    cells_of_str(&buffer.to_string())
}

fn cells_of_rope_line(buffer: &Rope, line: usize) -> (usize, Vec<GraphemeCell>) {
    let line_ix = line.min(buffer.len_lines().saturating_sub(1));
    let line_start_char = buffer.line_to_char(line_ix);
    let body = line_display_text(buffer, line_ix);
    (line_start_char, cells_of_str(&body))
}

// First cell whose `char_start >= char_index`. Mid-cluster offsets round up to
// the next cluster boundary. Returns `cells.len()` if `char_index` is past end.
pub(crate) fn cell_partition_by_char(cells: &[GraphemeCell], char_index: usize) -> usize {
    cells.partition_point(|cell| cell.char_start < char_index)
}

pub(crate) fn cell_partition_by_byte(cells: &[GraphemeCell], byte_offset: usize) -> usize {
    cells.partition_point(|cell| cell.byte_start < byte_offset)
}

// Index of the cell containing `char_index`. Mid-cluster offsets land on the
// containing cluster; offsets past end clamp to the last cluster. Callers
// MUST pre-check `cells.is_empty()` — when there are no cells this returns 0
// and indexing the result would panic.
pub(crate) fn cell_containing_char(cells: &[GraphemeCell], char_index: usize) -> usize {
    cells
        .partition_point(|cell| cell.char_start <= char_index)
        .saturating_sub(1)
}

fn cell_containing_byte(cells: &[GraphemeCell], byte_offset: usize) -> usize {
    cells
        .partition_point(|cell| cell.byte_start <= byte_offset)
        .saturating_sub(1)
}

fn char_index_at_cell(cells: &[GraphemeCell], cell_ix: usize, total_chars: usize) -> usize {
    cells
        .get(cell_ix)
        .map(|cell| cell.char_start)
        .unwrap_or(total_chars)
}

fn byte_offset_at_cell(cells: &[GraphemeCell], cell_ix: usize, total_bytes: usize) -> usize {
    cells
        .get(cell_ix)
        .map(|cell| cell.byte_start)
        .unwrap_or(total_bytes)
}

fn identifier_run_start_cells(cells: &[GraphemeCell], index: usize) -> usize {
    let mut start = index.min(cells.len());
    while start > 0 && is_identifier_char(cells[start - 1].repr) {
        start -= 1;
    }
    start
}

fn identifier_run_end_cells(cells: &[GraphemeCell], index: usize) -> usize {
    let mut end = index.min(cells.len());
    while end < cells.len() && is_identifier_char(cells[end].repr) {
        end += 1;
    }
    end
}

fn subword_chunk_end(cells: &[GraphemeCell], start: usize, run_end: usize) -> usize {
    let Some(class) = subword_class(cells[start].repr) else {
        return (start + 1).min(run_end);
    };

    match class {
        SubwordClass::Digit => {
            let mut end = start + 1;
            while end < run_end && subword_class(cells[end].repr) == Some(SubwordClass::Digit) {
                end += 1;
            }
            end
        }
        SubwordClass::Lower | SubwordClass::Alpha => {
            let mut end = start + 1;
            while end < run_end {
                match subword_class(cells[end].repr) {
                    Some(SubwordClass::Lower | SubwordClass::Alpha) => end += 1,
                    _ => break,
                }
            }
            end
        }
        SubwordClass::Upper => {
            if start + 1 < run_end {
                if let Some(SubwordClass::Lower | SubwordClass::Alpha) =
                    subword_class(cells[start + 1].repr)
                {
                    let mut end = start + 2;
                    while end < run_end {
                        match subword_class(cells[end].repr) {
                            Some(SubwordClass::Lower | SubwordClass::Alpha) => end += 1,
                            _ => break,
                        }
                    }
                    return end;
                }
            }

            let mut end = start + 1;
            while end < run_end && subword_class(cells[end].repr) == Some(SubwordClass::Upper) {
                if end + 1 < run_end {
                    if let Some(SubwordClass::Lower | SubwordClass::Alpha) =
                        subword_class(cells[end + 1].repr)
                    {
                        break;
                    }
                }
                end += 1;
            }
            end
        }
    }
}

fn subword_chunks(cells: &[GraphemeCell]) -> Vec<Range<usize>> {
    let mut chunks = Vec::new();
    let mut index = 0usize;
    while index < cells.len() {
        while index < cells.len() && cells[index].repr == '_' {
            index += 1;
        }
        if index >= cells.len() {
            break;
        }
        let end = subword_chunk_end(cells, index, cells.len());
        chunks.push(index..end);
        index = end;
    }
    chunks
}

// All of these `_cells` helpers take a cell index (not a char index / byte
// offset) and return a cell index. Callers translate to char indices via
// `char_index_at_cell` or to byte offsets via `byte_offset_at_cell`.

fn previous_word_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index > 0 && token_class(cells[index - 1].repr) == TokenClass::Whitespace {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    let class = token_class(cells[index - 1].repr);
    while index > 0 && token_class(cells[index - 1].repr) == class {
        index -= 1;
    }
    index
}

fn next_word_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index < cells.len() && token_class(cells[index].repr) == TokenClass::Whitespace {
        index += 1;
    }
    if index == cells.len() {
        return cells.len();
    }

    let class = token_class(cells[index].repr);
    while index < cells.len() && token_class(cells[index].repr) == class {
        index += 1;
    }
    index
}

fn previous_subword_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index > 0 && cells[index - 1].repr.is_whitespace() {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    if is_symbol_char(cells[index - 1].repr) {
        while index > 0 && is_symbol_char(cells[index - 1].repr) {
            index -= 1;
        }
        return index;
    }

    let run_start = identifier_run_start_cells(cells, index - 1);
    let run_end = identifier_run_end_cells(cells, index - 1);
    let chunks = subword_chunks(&cells[run_start..run_end]);
    let relative = index - run_start;
    chunks
        .iter()
        .rfind(|chunk| chunk.start < relative)
        .map_or(run_start, |chunk| run_start + chunk.start)
}

fn next_subword_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index < cells.len() && cells[index].repr.is_whitespace() {
        index += 1;
    }
    if index == cells.len() {
        return cells.len();
    }

    if is_symbol_char(cells[index].repr) {
        while index < cells.len() && is_symbol_char(cells[index].repr) {
            index += 1;
        }
        return index;
    }

    while index < cells.len() && cells[index].repr == '_' {
        index += 1;
    }
    if index == cells.len() {
        return cells.len();
    }

    let run_start = identifier_run_start_cells(cells, index);
    let run_end = identifier_run_end_cells(cells, index);
    let chunks = subword_chunks(&cells[run_start..run_end]);
    let relative = index - run_start;
    chunks
        .iter()
        .find(|chunk| chunk.start <= relative && relative < chunk.end)
        .map_or(run_end, |chunk| run_start + chunk.end)
}

pub fn previous_word_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target = previous_word_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn previous_subword_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target =
        previous_subword_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn next_word_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target = next_word_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn next_subword_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target = next_subword_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn word_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let (line_start, cells) = cells_of_rope_line(buffer, buffer.char_to_line(clamped));
    if cells.is_empty() {
        return clamped..clamped;
    }
    let local = clamped.saturating_sub(line_start);
    let cell_ix = cell_containing_char(&cells, local);
    let class = token_class(cells[cell_ix].repr);
    let mut start = cell_ix;
    while start > 0 && token_class(cells[start - 1].repr) == class {
        start -= 1;
    }
    let mut end = cell_ix + 1;
    while end < cells.len() && token_class(cells[end].repr) == class {
        end += 1;
    }
    let line_chars: usize = cells.iter().map(|c| c.char_len as usize).sum();
    let start_char = cells[start].char_start;
    let end_char = char_index_at_cell(&cells, end, line_chars);
    (line_start + start_char)..(line_start + end_char)
}

pub fn next_grapheme_column(line: &str, column: usize) -> usize {
    let total = line.chars().count();
    if column >= total {
        return total;
    }
    let local_byte = byte_of_char_index(line, column);
    let next_byte = line
        .grapheme_indices(true)
        .find_map(|(b, _)| (b > local_byte).then_some(b))
        .unwrap_or(line.len());
    line[..next_byte].chars().count()
}

pub fn previous_grapheme_column(line: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }
    let total = line.chars().count();
    let column = column.min(total);
    let local_byte = if column == total {
        line.len()
    } else {
        byte_of_char_index(line, column)
    };
    let prev_byte = line
        .grapheme_indices(true)
        .rev()
        .find_map(|(b, _)| (b < local_byte).then_some(b))
        .unwrap_or(0);
    line[..prev_byte].chars().count()
}

pub fn last_grapheme_column(line: &str) -> usize {
    line.grapheme_indices(true)
        .next_back()
        .map_or(0, |(b, _)| line[..b].chars().count())
}

pub fn next_grapheme_boundary(buffer: &Rope, char_index: usize) -> usize {
    let total = buffer.len_chars();
    let ci = char_index.min(total);
    if ci == total {
        return total;
    }
    let line = buffer.char_to_line(ci);
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    if local_ci >= body.chars().count() {
        return (ci + 1).min(total);
    }
    line_start + next_grapheme_column(&body, local_ci)
}

pub fn previous_grapheme_boundary(buffer: &Rope, char_index: usize) -> usize {
    let total = buffer.len_chars();
    let ci = char_index.min(total);
    if ci == 0 {
        return 0;
    }
    let line = buffer.char_to_line(ci);
    let line_start = buffer.line_to_char(line);
    if ci == line_start {
        return ci - 1;
    }
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    if local_ci > body.chars().count() {
        return ci - 1;
    }
    line_start + previous_grapheme_column(&body, local_ci)
}

pub fn line_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let line = buffer.char_to_line(clamped);
    let start = buffer.line_to_char(line);
    let end = if line + 1 < buffer.len_lines() {
        buffer.line_to_char(line + 1)
    } else {
        buffer.len_chars()
    };
    start..end
}

/// Maximal run of lines around `char_index` whose blank/non-blank state matches
/// the cursor line. Blank = line content is whitespace-only (excluding the
/// terminator). Includes the trailing newline of the last line in the run when
/// one exists.
pub fn paragraph_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let total_lines = buffer.len_lines();
    let clamped = char_index.min(buffer.len_chars());
    let cur = buffer.char_to_line(clamped);
    let on_blank = is_blank_line(buffer, cur);

    let mut first = cur;
    while first > 0 && is_blank_line(buffer, first - 1) == on_blank {
        first -= 1;
    }
    let mut last = cur;
    while last + 1 < total_lines && is_blank_line(buffer, last + 1) == on_blank {
        last += 1;
    }
    let start = buffer.line_to_char(first);
    let end = if last + 1 < total_lines {
        buffer.line_to_char(last + 1)
    } else {
        buffer.len_chars()
    };
    start..end
}

fn is_blank_line(buffer: &Rope, line_ix: usize) -> bool {
    if line_ix >= buffer.len_lines() {
        return true;
    }
    buffer.line(line_ix).chars().all(char::is_whitespace)
}

pub fn previous_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = previous_word_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn previous_subword_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = previous_subword_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn next_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = next_word_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn next_subword_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = next_subword_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn word_range_in_text(text: &str, offset: usize) -> Range<usize> {
    let cells = cells_of_str(text);
    if cells.is_empty() {
        return 0..0;
    }
    let local = if offset >= text.len() {
        cells.len() - 1
    } else {
        cell_containing_byte(&cells, offset)
    };
    let class = token_class(cells[local].repr);
    let mut start = local;
    while start > 0 && token_class(cells[start - 1].repr) == class {
        start -= 1;
    }
    let mut end = local + 1;
    while end < cells.len() && token_class(cells[end].repr) == class {
        end += 1;
    }
    let start_byte = cells[start].byte_start;
    let end_byte = byte_offset_at_cell(&cells, end, text.len());
    start_byte..end_byte
}

pub fn drag_selection_range(anchor: Range<usize>, current: Range<usize>) -> (Range<usize>, bool) {
    if current.start < anchor.start {
        (current.start..anchor.end.max(current.end), true)
    } else {
        (
            anchor.start.min(current.start)..current.end.max(anchor.end),
            false,
        )
    }
}

fn byte_of_char_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

pub(crate) fn line_display_text(buffer: &Rope, line_ix: usize) -> String {
    let mut line = buffer
        .line(line_ix.min(buffer.len_lines().saturating_sub(1)))
        .to_string();
    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    line
}

pub(crate) fn display_line_char_len(buffer: &Rope, line_ix: usize) -> usize {
    buffer
        .line(line_ix.min(buffer.len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count()
}

pub(crate) fn char_at_line_column(buffer: &Rope, line_ix: usize, column: usize) -> usize {
    let line = line_ix.min(buffer.len_lines().saturating_sub(1));
    buffer.line_to_char(line) + column.min(display_line_char_len(buffer, line))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_word_ranges_group_words_symbols_and_whitespace() {
        let buffer = Rope::from_str("alpha beta::gamma");

        assert_eq!(word_range_at_char(&buffer, 7), 6..10);
        assert_eq!(word_range_at_char(&buffer, 10), 10..12);
        assert_eq!(word_range_at_char(&buffer, 5), 5..6);
    }

    #[test]
    fn rope_word_boundaries_skip_whitespace() {
        let buffer = Rope::from_str("alpha beta.gamma");

        assert_eq!(next_word_boundary(&buffer, 0), 5);
        assert_eq!(next_word_boundary(&buffer, 5), 10);
        assert_eq!(previous_word_boundary(&buffer, 11), 10);
        assert_eq!(previous_word_boundary(&buffer, 10), 6);
    }

    #[test]
    fn line_range_includes_trailing_newline_when_present() {
        let buffer = Rope::from_str("one\ntwo\nthree");

        assert_eq!(line_range_at_char(&buffer, 1), 0..4);
        assert_eq!(line_range_at_char(&buffer, 5), 4..8);
        assert_eq!(line_range_at_char(&buffer, 10), 8..13);
    }

    #[test]
    fn paragraph_range_groups_consecutive_non_blank_lines() {
        let buffer = Rope::from_str("alpha\nbeta\n\ngamma\ndelta\n");

        // Cursor on first paragraph: "alpha\nbeta\n".
        assert_eq!(paragraph_range_at_char(&buffer, 0), 0..11);
        assert_eq!(paragraph_range_at_char(&buffer, 7), 0..11);
        // Cursor on second paragraph: "gamma\ndelta\n".
        assert_eq!(paragraph_range_at_char(&buffer, 12), 12..24);
    }

    #[test]
    fn paragraph_range_groups_blank_lines_when_cursor_blank() {
        let buffer = Rope::from_str("alpha\n\n\nbeta\n");

        // Cursor on the blank run at line 1 → covers lines 1 and 2.
        assert_eq!(paragraph_range_at_char(&buffer, 6), 6..8);
    }

    #[test]
    fn text_word_ranges_group_words_symbols_and_whitespace() {
        let text = "alpha beta::gamma";

        assert_eq!(word_range_in_text(text, 7), 6..10);
        assert_eq!(word_range_in_text(text, 10), 10..12);
        assert_eq!(word_range_in_text(text, 5), 5..6);
    }

    #[test]
    fn text_word_boundaries_are_utf8_safe() {
        let text = "one γamma two";

        assert_eq!(next_word_boundary_in_text(text, 0), 3);
        assert_eq!(next_word_boundary_in_text(text, 3), "one γamma".len());
        assert_eq!(
            previous_word_boundary_in_text(text, "one γamma".len()),
            "one ".len()
        );
    }

    #[test]
    fn rope_subword_boundaries_split_camel_snake_and_digits() {
        let buffer = Rope::from_str("camelCase snake_case HTTPServer version2Alpha");

        assert_eq!(next_subword_boundary(&buffer, 0), 5);
        assert_eq!(next_subword_boundary(&buffer, 5), 9);
        assert_eq!(next_subword_boundary(&buffer, 10), 15);
        assert_eq!(next_subword_boundary(&buffer, 15), 20);
        assert_eq!(next_subword_boundary(&buffer, 21), 25);
        assert_eq!(next_subword_boundary(&buffer, 25), 31);
        assert_eq!(next_subword_boundary(&buffer, 32), 39);
        assert_eq!(next_subword_boundary(&buffer, 39), 40);
        assert_eq!(next_subword_boundary(&buffer, 40), 45);

        assert_eq!(previous_subword_boundary(&buffer, 9), 5);
        assert_eq!(previous_subword_boundary(&buffer, 5), 0);
        assert_eq!(previous_subword_boundary(&buffer, 20), 16);
        assert_eq!(previous_subword_boundary(&buffer, 16), 10);
        assert_eq!(previous_subword_boundary(&buffer, 15), 10);
        assert_eq!(previous_subword_boundary(&buffer, 31), 25);
        assert_eq!(previous_subword_boundary(&buffer, 25), 21);
        assert_eq!(previous_subword_boundary(&buffer, 45), 40);
        assert_eq!(previous_subword_boundary(&buffer, 40), 39);
        assert_eq!(previous_subword_boundary(&buffer, 39), 32);
    }

    #[test]
    fn subword_boundaries_handle_single_char_snake_segments() {
        let buffer = Rope::from_str("a_b_c");

        assert_eq!(next_subword_boundary(&buffer, 0), 1);
        assert_eq!(next_subword_boundary(&buffer, 1), 3);
        assert_eq!(next_subword_boundary(&buffer, 3), 5);

        assert_eq!(previous_subword_boundary(&buffer, 5), 4);
        assert_eq!(previous_subword_boundary(&buffer, 4), 2);
        assert_eq!(previous_subword_boundary(&buffer, 2), 0);
    }

    #[test]
    fn subword_boundaries_keep_symbol_runs_as_stops() {
        let buffer = Rope::from_str("foo.barBaz alpha::Beta");

        assert_eq!(next_subword_boundary(&buffer, 0), 3);
        assert_eq!(next_subword_boundary(&buffer, 3), 4);
        assert_eq!(next_subword_boundary(&buffer, 4), 7);
        assert_eq!(next_subword_boundary(&buffer, 7), 10);
        assert_eq!(next_subword_boundary(&buffer, 11), 16);
        assert_eq!(next_subword_boundary(&buffer, 16), 18);
        assert_eq!(next_subword_boundary(&buffer, 18), 22);

        assert_eq!(previous_subword_boundary(&buffer, 10), 7);
        assert_eq!(previous_subword_boundary(&buffer, 7), 4);
        assert_eq!(previous_subword_boundary(&buffer, 4), 3);
        assert_eq!(previous_subword_boundary(&buffer, 22), 18);
        assert_eq!(previous_subword_boundary(&buffer, 18), 16);
    }

    #[test]
    fn text_subword_boundaries_are_utf8_safe() {
        let text = "one ΓammaΔelta HTTPServer42";

        assert_eq!(next_subword_boundary_in_text(text, 0), "one".len());
        assert_eq!(
            next_subword_boundary_in_text(text, "one ".len() + 1),
            "one Γamma".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one ".len()),
            "one Γamma".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one Γamma".len()),
            "one ΓammaΔelta".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one ΓammaΔelta ".len()),
            "one ΓammaΔelta HTTP".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one ΓammaΔelta HTTP".len()),
            "one ΓammaΔelta HTTPServer".len()
        );
        assert_eq!(
            previous_subword_boundary_in_text(text, "one ΓammaΔelta HTTPServer".len()),
            "one ΓammaΔelta HTTP".len()
        );
        assert_eq!(
            previous_subword_boundary_in_text(text, "one Γa".len()),
            "one ".len()
        );
        assert_eq!(
            previous_subword_boundary_in_text(text, "one Γamma".len()),
            "one ".len()
        );
    }

    #[test]
    fn rope_word_boundary_skips_full_combining_acute_cluster() {
        // "naïve word" with NFD ï = i + U+0308. 11 chars, 10 graphemes.
        // The combining mark sits at char index 3.
        let buffer = Rope::from_str("nai\u{0308}ve word");

        assert_eq!(next_word_boundary(&buffer, 0), 6);
        // Mid-cluster cursor still lands past the cluster, never inside it.
        assert_eq!(next_word_boundary(&buffer, 3), 6);
        assert_eq!(next_word_boundary(&buffer, 6), 11);
        assert_eq!(previous_word_boundary(&buffer, 11), 7);
        assert_eq!(previous_word_boundary(&buffer, 7), 0);
    }

    #[test]
    fn rope_word_boundary_treats_regional_indicator_pair_as_one_cluster() {
        // "a🇫🇷b cc" — regional indicator pair (4 bytes each) is one grapheme.
        // Char layout: [a, 🇫, 🇷, b, ' ', c, c] = 7 chars, 6 graphemes.
        let buffer = Rope::from_str("a\u{1F1EB}\u{1F1F7}b cc");

        assert_eq!(next_word_boundary(&buffer, 0), 1);
        // From the first regional indicator, jump past the second to `b` start.
        assert_eq!(next_word_boundary(&buffer, 1), 3);
        assert_eq!(next_word_boundary(&buffer, 3), 4);
        assert_eq!(previous_word_boundary(&buffer, 4), 3);
        // From after `b`, walking back lands at the regional pair start, not between them.
        assert_eq!(previous_word_boundary(&buffer, 3), 1);
        assert_eq!(previous_word_boundary(&buffer, 1), 0);
    }

    #[test]
    fn rope_subword_boundary_skips_combining_acute_cluster() {
        // "naïveCase" NFD: n, a, i, U+0308, v, e, C, a, s, e = 10 chars, 9 graphemes.
        let buffer = Rope::from_str("nai\u{0308}veCase");

        // From start, first subword spans the lowercase-only run before `C`.
        assert_eq!(next_subword_boundary(&buffer, 0), 6);
        assert_eq!(next_subword_boundary(&buffer, 6), 10);
        assert_eq!(previous_subword_boundary(&buffer, 10), 6);
        assert_eq!(previous_subword_boundary(&buffer, 6), 0);
    }

    #[test]
    fn rope_word_range_groups_full_combining_acute_cluster() {
        let buffer = Rope::from_str("nai\u{0308}ve word");

        // Click anywhere on the cluster — including on the combining mark — and
        // the whole `naïve` token comes back.
        assert_eq!(word_range_at_char(&buffer, 0), 0..6);
        assert_eq!(word_range_at_char(&buffer, 3), 0..6);
        assert_eq!(word_range_at_char(&buffer, 5), 0..6);
        assert_eq!(word_range_at_char(&buffer, 7), 7..11);
    }

    #[test]
    fn text_word_boundary_skips_full_combining_acute_cluster() {
        let text = "nai\u{0308}ve word";

        let after_naive = "nai\u{0308}ve".len();
        let space = "nai\u{0308}ve ".len();
        let i_byte = "na".len();
        let combining_byte = "nai".len();

        assert_eq!(next_word_boundary_in_text(text, 0), after_naive);
        assert_eq!(
            next_word_boundary_in_text(text, combining_byte),
            after_naive
        );
        assert_eq!(previous_word_boundary_in_text(text, text.len()), space);
        assert_eq!(previous_word_boundary_in_text(text, after_naive), 0);
        // Mid-cluster offset rounds out to the same cluster boundary as `i` start.
        assert_eq!(
            previous_word_boundary_in_text(text, combining_byte),
            previous_word_boundary_in_text(text, i_byte)
        );
    }

    #[test]
    fn text_subword_boundary_skips_full_combining_acute_cluster() {
        let text = "nai\u{0308}veCase";

        let after_naive = "nai\u{0308}ve".len();
        assert_eq!(next_subword_boundary_in_text(text, 0), after_naive);
        assert_eq!(next_subword_boundary_in_text(text, after_naive), text.len());
        assert_eq!(
            previous_subword_boundary_in_text(text, text.len()),
            after_naive
        );
        assert_eq!(previous_subword_boundary_in_text(text, after_naive), 0);
    }

    #[test]
    fn text_word_range_groups_full_regional_indicator_cluster() {
        let text = "a\u{1F1EB}\u{1F1F7}b cc";
        let flag_start = "a".len();
        let after_flag = "a\u{1F1EB}\u{1F1F7}".len();
        // Click on either regional indicator — the range covers the full cluster.
        assert_eq!(word_range_in_text(text, flag_start), flag_start..after_flag);
        assert_eq!(
            word_range_in_text(text, flag_start + "\u{1F1EB}".len()),
            flag_start..after_flag,
        );
    }

    #[test]
    fn drag_selection_extends_from_anchor_token() {
        let (selection, reversed) = drag_selection_range(6..10, 13..18);

        assert_eq!(selection, 6..18);
        assert!(!reversed);

        let (selection, reversed) = drag_selection_range(6..10, 0..5);
        assert_eq!(selection, 0..10);
        assert!(reversed);
    }

    #[test]
    fn selection_set_rejects_invalid_shapes() {
        assert_eq!(
            SelectionSet::from_selections(Vec::new(), 0).unwrap_err(),
            SelectionSetError::Empty
        );
        assert_eq!(
            SelectionSet::from_selections(vec![Selection::collapsed(0)], 1).unwrap_err(),
            SelectionSetError::InvalidPrimary
        );
        assert_eq!(
            SelectionSet::from_selections(
                vec![Selection::collapsed(4), Selection::collapsed(2)],
                0,
            )
            .unwrap_err(),
            SelectionSetError::Unordered
        );
        assert_eq!(
            SelectionSet::from_selections(
                vec![
                    Selection::from_range(1..4, false),
                    Selection::from_range(3..5, false),
                ],
                0,
            )
            .unwrap_err(),
            SelectionSetError::Overlapping
        );
        assert_eq!(
            SelectionSet::from_selections(
                vec![Selection::collapsed(2), Selection::collapsed(2)],
                0,
            )
            .unwrap_err(),
            SelectionSetError::Overlapping
        );
    }

    #[test]
    fn selection_set_preserves_primary_index_for_valid_ordered_ranges() {
        let set = SelectionSet::from_selections(
            vec![
                Selection::collapsed(1),
                Selection::from_range(3..5, false),
                Selection::collapsed(7),
            ],
            1,
        )
        .expect("valid ordered non-overlapping selections");

        assert_eq!(set.primary(), Selection::from_range(3..5, false));
        assert_eq!(set.primary_index(), 1);
    }

    #[test]
    fn selection_set_coalesces_duplicate_batch_cursors_and_preserves_primary() {
        let set = SelectionSet::from_selections_coalescing_cursors(
            vec![
                Selection::collapsed(1),
                Selection::collapsed(1),
                Selection::collapsed(3),
            ],
            1,
        )
        .expect("duplicate collapsed cursors are coalesced");

        assert_eq!(
            set.as_slice(),
            &[Selection::collapsed(1), Selection::collapsed(3)]
        );
        assert_eq!(set.primary_index(), 0);
        assert_eq!(set.primary(), Selection::collapsed(1));
    }

    #[test]
    fn selection_set_clamping_coalesces_duplicate_cursors() {
        let set = SelectionSet::from_selections(
            vec![Selection::collapsed(2), Selection::collapsed(4)],
            1,
        )
        .expect("valid cursors");

        let clamped = set.clamped_to_len(1);

        assert_eq!(clamped.as_slice(), &[Selection::collapsed(1)]);
        assert_eq!(clamped.primary_index(), 0);
    }
}
