use ropey::Rope;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

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

/// A `SelectionSet` is always non-empty, sorted by selected range, and
/// non-overlapping, with one primary selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSet {
    selections: Vec<Selection>,
    primary: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorGoal {
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

/// Selection state plus movement metadata owned by a tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectionState {
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
            if coalesced
                .last()
                .is_some_and(|last| duplicate_cursor(*last, selection))
            {
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

        let added_index = (0..selections.len())
            .rev()
            .find(|&i| {
                if selections[i].1 != SelectionOrigin::Added {
                    return false;
                }
                let range = selections[i].0.range();
                let prev_dup = i.checked_sub(1).is_some_and(|j| {
                    selections[j].0.range() == range && selections[j].1 != SelectionOrigin::Added
                });
                let next_dup = selections.get(i + 1).is_some_and(|(selection, origin)| {
                    selection.range() == range && *origin != SelectionOrigin::Added
                });
                !(prev_dup || next_dup)
            })
            .or_else(|| {
                selections
                    .iter()
                    .rposition(|(_, origin)| *origin == SelectionOrigin::Added)
            });

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
    pub(crate) fn single(selection: Selection) -> Self {
        Self {
            set: SelectionSet::single(selection),
            goals: CursorGoals::default(),
        }
    }

    pub(crate) fn single_with_transform(transform: SelectionTransform) -> Self {
        Self {
            set: SelectionSet::single(transform.selection),
            goals: CursorGoals {
                movement: transform.movement_goal.map(|goal| vec![goal]),
                visible: transform.visible_column.map(|column| vec![column]),
            },
        }
    }

    pub(crate) fn from_set(set: SelectionSet) -> Self {
        Self {
            set,
            goals: CursorGoals::default(),
        }
    }

    pub(crate) fn selection_set(&self) -> &SelectionSet {
        &self.set
    }

    pub(crate) fn primary(&self) -> Selection {
        self.set.primary()
    }

    pub(crate) fn as_slice(&self) -> &[Selection] {
        self.set.as_slice()
    }

    pub(crate) fn primary_index(&self) -> usize {
        self.set.primary_index()
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

        let mut mapped = Vec::with_capacity(entries.len());
        let mut primary = None;

        for entry in entries.iter().copied() {
            if mapped.last().is_some_and(|last: &MappedSelection| {
                duplicate_cursor(last.transform.selection, entry.transform.selection)
            }) {
                if entry.is_primary {
                    primary = mapped.len().checked_sub(1);
                    if let Some(index) = primary {
                        mapped[index] = entry;
                    }
                }
                continue;
            }

            if entry.is_primary {
                primary = Some(mapped.len());
            }
            mapped.push(entry);
        }

        let primary = primary.unwrap_or(0);
        let selections = mapped
            .iter()
            .map(|entry| entry.transform.selection)
            .collect();
        let set = SelectionSet::from_selections(selections, primary).ok()?;
        let goals = CursorGoals {
            movement: mapped
                .iter()
                .map(|entry| entry.transform.movement_goal)
                .collect(),
            visible: mapped
                .iter()
                .map(|entry| entry.transform.visible_column)
                .collect(),
        };
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

fn duplicate_cursor(left: Selection, right: Selection) -> bool {
    !left.has_selection() && !right.has_selection() && left.cursor() == right.cursor()
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
        return if line + 1 < buffer.len_lines() {
            buffer.line_to_char(line + 1)
        } else {
            total
        };
    }
    line_start + next_grapheme_column(&body, local_ci)
}

pub(crate) fn floor_grapheme_boundary(buffer: &Rope, char_index: usize) -> usize {
    let total = buffer.len_chars();
    let ci = char_index.min(total);
    if ci == total {
        return total;
    }
    let line = buffer.char_to_line(ci);
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    let body_chars = body.chars().count();
    if local_ci >= body_chars {
        return ci;
    }

    let mut char_start = 0usize;
    let mut best = 0usize;
    for cluster in body.graphemes(true) {
        if char_start > local_ci {
            break;
        }
        best = char_start;
        char_start += cluster.chars().count();
    }
    line_start + best
}

pub(crate) fn ceil_grapheme_boundary(buffer: &Rope, char_index: usize) -> usize {
    let total = buffer.len_chars();
    let ci = char_index.min(total);
    if ci == total {
        return total;
    }
    let line = buffer.char_to_line(ci);
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    let body_chars = body.chars().count();
    if local_ci >= body_chars {
        return ci;
    }

    let mut char_start = 0usize;
    for cluster in body.graphemes(true) {
        let next = char_start + cluster.chars().count();
        if local_ci == char_start {
            return ci;
        }
        if local_ci < next {
            return line_start + next;
        }
        char_start = next;
    }
    line_start + body_chars
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
        return if line == 0 {
            0
        } else {
            let previous_line = line - 1;
            buffer.line_to_char(previous_line) + display_line_char_len(buffer, previous_line)
        };
    }
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    if local_ci > body.chars().count() {
        return line_start + body.chars().count();
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
    fn word_motion_does_not_split_combining_cluster() {
        let buffer = Rope::from_str("nai\u{0308}ve word");

        assert_eq!(next_word_boundary(&buffer, 3), 6);
        assert_eq!(previous_word_boundary(&buffer, 7), 0);
        assert_eq!(word_range_at_char(&buffer, 3), 0..6);
    }

    #[test]
    fn subword_motion_splits_common_identifier_shapes() {
        let buffer = Rope::from_str("camelCase snake_case HTTPServer");

        assert_eq!(next_subword_boundary(&buffer, 0), 5);
        assert_eq!(next_subword_boundary(&buffer, 10), 15);
        assert_eq!(previous_subword_boundary(&buffer, 31), 25);
    }

    #[test]
    fn selection_set_coalesces_duplicate_cursors() {
        let set = SelectionSet::from_selections_coalescing_cursors(
            vec![
                Selection::collapsed(1),
                Selection::collapsed(1),
                Selection::collapsed(3),
            ],
            1,
        )
        .expect("duplicate cursors coalesce");

        assert_eq!(
            set.as_slice(),
            &[Selection::collapsed(1), Selection::collapsed(3)]
        );
        assert_eq!(set.primary_index(), 0);
    }
}
