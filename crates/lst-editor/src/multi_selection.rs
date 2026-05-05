use std::ops::Range;

use crate::{
    document::{char_to_position, EditKind, UndoBoundary},
    find::{build_query_regex, FindState},
    selection::{char_at_line_column, word_range_at_char, Selection, SelectionSet},
    tab::EditorTab,
    transaction::{offset_with_delta, EditRequest, SelectionAfter, TextChange, TextChangeSet},
};

pub(crate) fn replacement_request(
    tab: &EditorTab,
    text: String,
    boundary: UndoBoundary,
) -> Option<EditRequest> {
    replacement_request_by_index(tab, |_| text.clone(), boundary)
}

pub(crate) fn paste_request(
    tab: &EditorTab,
    text: String,
    boundary: UndoBoundary,
) -> Option<EditRequest> {
    let selection_count = tab.selection_set().as_slice().len();
    if selection_count <= 1 {
        return None;
    }

    let lines = clipboard_lines_for_distribution(&text);
    if lines.len() == selection_count {
        replacement_request_by_index(tab, |index| lines[index].clone(), boundary)
    } else {
        replacement_request_by_index(tab, |_| text.clone(), boundary)
    }
}

/// Multi-cursor replacement where the inserted text varies per selection.
/// `replacement_for(i)` is called for each selection in document order.
/// Returns `None` for single-selection sets and during IME composition,
/// matching `replacement_request`'s contract.
pub(crate) fn replacement_request_by_index<F>(
    tab: &EditorTab,
    replacement_for: F,
    boundary: UndoBoundary,
) -> Option<EditRequest>
where
    F: Fn(usize) -> String,
{
    let selection_set = tab.selection_set();
    if !selection_set.has_multiple() || tab.marked_range().is_some() {
        return None;
    }

    let mut delta = 0isize;
    let mut changes = Vec::with_capacity(selection_set.as_slice().len());
    let mut selections_after = Vec::with_capacity(selection_set.as_slice().len());
    for (index, selection) in selection_set.as_slice().iter().enumerate() {
        let replacement = replacement_for(index);
        let replacement_len = replacement.chars().count();
        let range = selection.range();
        let inserted_start = offset_with_delta(range.start, delta);
        selections_after.push(Selection::collapsed(inserted_start + replacement_len));
        delta += replacement_len as isize - (range.end - range.start) as isize;
        changes.push(TextChange::replace(range, replacement));
    }

    let changes = TextChangeSet::new(changes, selection_set.primary_index());
    let selection_after = SelectionSet::from_selections_coalescing_cursors(
        selections_after,
        selection_set.primary_index(),
    )
    .expect("multi-selection replacement preserves a valid selection set");
    let kind = if changes
        .as_slice()
        .iter()
        .all(|change| change.replacement.is_empty())
    {
        EditKind::Delete
    } else {
        EditKind::Insert
    };
    Some(
        EditRequest::from_changes(kind, boundary, changes)
            .with_selection_after(SelectionAfter::Exact(selection_after)),
    )
}

pub(crate) fn selected_text_joined(tab: &EditorTab) -> Option<String> {
    let selection_set = tab.selection_set();
    if !selection_set.has_multiple()
        || !selection_set
            .as_slice()
            .iter()
            .any(Selection::has_selection)
    {
        return None;
    }

    // Cursor-only selections contribute an empty fragment so that the
    // resulting `\n`-joined clipboard has exactly one line per selection.
    // Pasting back into the same set then round-trips through
    // `paste_request`'s line-count equality branch.
    let mut joined = String::new();
    for (index, selection) in selection_set.as_slice().iter().enumerate() {
        if index > 0 {
            joined.push('\n');
        }
        joined.push_str(&tab.buffer().slice(selection.range()).to_string());
    }
    Some(joined)
}

fn clipboard_lines_for_distribution(text: &str) -> Vec<String> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
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

/// Builds a column block selection between `anchor` and `head`. Each line in
/// the block contributes one selection clamped to its display width; lines
/// shorter than `start_column` collapse to a cursor at the line end. The
/// primary tracks the line containing `head`.
pub(crate) fn rectangular_selection_set(
    tab: &EditorTab,
    anchor: usize,
    head: usize,
) -> Option<SelectionSet> {
    let buffer = tab.buffer();
    let anchor = char_to_position(buffer, anchor);
    let head = char_to_position(buffer, head);
    let first_line = anchor.line.min(head.line);
    let last_line = anchor.line.max(head.line);
    let start_column = anchor.column.min(head.column);
    let end_column = anchor.column.max(head.column);
    let reversed = head.column < anchor.column;
    let primary_line = head.line.clamp(first_line, last_line);

    let mut selections = Vec::with_capacity(last_line - first_line + 1);
    let mut primary = 0;
    for line in first_line..=last_line {
        let start = char_at_line_column(buffer, line, start_column);
        let end = char_at_line_column(buffer, line, end_column);
        if line == primary_line {
            primary = selections.len();
        }
        selections.push(Selection::from_range(start..end, reversed));
    }
    SelectionSet::from_selections_coalescing_cursors(selections, primary).ok()
}

/// Selection to add for the next occurrence of the active tab's selected
/// query, skipping ranges that already overlap the selection set. Wraps to
/// the first non-overlapping match if no match exists past the primary.
/// Honours the find panel's case / whole-word flags (and the smart-case
/// heuristic) via [`FindState`]; the selection text is always treated
/// literally — `use_regex` only applies to the find panel itself.
pub(crate) fn next_occurrence_addition(tab: &EditorTab, find: &FindState) -> Option<Selection> {
    let (query, _) = occurrence_query(tab)?;
    let regex = build_query_regex(&query, find.case_sensitive, find.whole_word, false).ok()?;
    let text = tab.buffer_text();
    let primary_end = tab.selection().range().end;
    let selection_set = tab.selection_set();

    // Stream regex matches and stop on the first non-overlapping match
    // past the primary's end. Only fall back to a wrap-around match
    // (first non-overlapping match anywhere) when the forward scan finds
    // none — saves walking the buffer to completion for the common
    // Ctrl-D case.
    let mut last_match_end_byte = 0usize;
    let mut last_match_end_char = 0usize;
    let mut wrap_candidate: Option<Range<usize>> = None;
    for m in regex.find_iter(&text) {
        let start_byte = m.start();
        let end_byte = m.end();
        if start_byte == end_byte {
            continue;
        }
        let chars_in_gap = text[last_match_end_byte..start_byte].chars().count();
        let match_start_char = last_match_end_char + chars_in_gap;
        let chars_in_match = text[start_byte..end_byte].chars().count();
        let match_end_char = match_start_char + chars_in_match;
        last_match_end_byte = end_byte;
        last_match_end_char = match_end_char;

        let range = match_start_char..match_end_char;
        if range_overlaps_selection_set(&range, selection_set) {
            continue;
        }
        if range.start >= primary_end {
            return Some(Selection::from_range(range, false));
        }
        if wrap_candidate.is_none() {
            wrap_candidate = Some(range);
        }
    }
    wrap_candidate.map(|range| Selection::from_range(range, false))
}

/// Selection set spanning every occurrence of the active tab's selection
/// (or the word under the cursor when there's no selection). The primary
/// is the match equal to the original query range when present, otherwise
/// the first match. Same flag semantics as [`next_occurrence_addition`].
pub(crate) fn all_occurrences_set(tab: &EditorTab, find: &FindState) -> Option<SelectionSet> {
    let (query, query_range) = occurrence_query(tab)?;
    let ranges = occurrence_ranges(&tab.buffer_text(), &query, find);
    if ranges.is_empty() {
        return None;
    }
    let primary = ranges
        .iter()
        .position(|range| *range == query_range)
        .unwrap_or(0);
    let selections = ranges
        .into_iter()
        .map(|range| Selection::from_range(range, false))
        .collect();
    SelectionSet::from_selections(selections, primary).ok()
}

fn occurrence_query(tab: &EditorTab) -> Option<(String, Range<usize>)> {
    let range = if tab.selection().has_selection() {
        tab.selection().range()
    } else {
        word_range_at_char(tab.buffer(), tab.cursor_char())
    };
    if range.start == range.end {
        return None;
    }
    Some((tab.buffer().slice(range.clone()).to_string(), range))
}

fn occurrence_ranges(text: &str, query: &str, find: &FindState) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    // Selection-as-query stays literal; only case-related flags apply.
    let regex = match build_query_regex(query, find.case_sensitive, find.whole_word, false) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };

    let mut ranges = Vec::new();
    let mut last_match_end_byte = 0usize;
    let mut last_match_end_char = 0usize;
    for m in regex.find_iter(text) {
        let start_byte = m.start();
        let end_byte = m.end();
        if start_byte == end_byte {
            // Skip zero-width matches; cursor-add has no use for them.
            continue;
        }
        let chars_in_gap = text[last_match_end_byte..start_byte].chars().count();
        let match_start_char = last_match_end_char + chars_in_gap;
        let chars_in_match = text[start_byte..end_byte].chars().count();
        let match_end_char = match_start_char + chars_in_match;
        ranges.push(match_start_char..match_end_char);
        last_match_end_byte = end_byte;
        last_match_end_char = match_end_char;
    }
    ranges
}

fn range_overlaps_selection_set(range: &Range<usize>, selection_set: &SelectionSet) -> bool {
    selection_set.as_slice().iter().any(|selection| {
        let selected = selection.range();
        range.start < selected.end && selected.start < range.end
    })
}
