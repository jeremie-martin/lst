use std::ops::Range;

use crate::{
    document::{char_to_position, EditKind, UndoBoundary},
    find::{build_query_regex, FindState},
    selection::{char_at_line_column, word_range_at_char, Selection, SelectionSet},
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
    let selections_after =
        SelectionSet::from_selections_coalescing_cursors(selections_after, selection_set.primary_index()).ok()?;

    Some(
        EditRequest::from_changes(kind, boundary, changes)
            .with_selection_after(SelectionAfter::Exact(selections_after)),
    )
}

pub(crate) fn replacement_request(tab: &EditorTab, text: String, boundary: UndoBoundary) -> Option<EditRequest> {
    replacement_request_by_index(tab, |_| text.clone(), boundary)
}

pub(crate) fn paste_request(tab: &EditorTab, text: String, boundary: UndoBoundary) -> Option<EditRequest> {
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

pub(crate) fn replacement_request_by_index<F>(
    tab: &EditorTab,
    replacement_for: F,
    boundary: UndoBoundary,
) -> Option<EditRequest>
where
    F: Fn(usize) -> String,
{
    let selection_set = tab.selection_set();
    let replacements = (0..selection_set.as_slice().len())
        .map(replacement_for)
        .collect::<Vec<_>>();
    let kind = if replacements.iter().all(String::is_empty) {
        EditKind::Delete
    } else {
        EditKind::Insert
    };
    request_for_each(tab, kind, boundary, |index, selection| {
        Some(SelectionEdit::replace_with_collapsed_end(
            selection.range(),
            replacements[index].clone(),
        ))
    })
}

pub(crate) fn selected_text_joined(tab: &EditorTab) -> Option<String> {
    let selections = tab.selection_set().as_slice();
    if selections.len() <= 1 || !selections.iter().any(Selection::has_selection) {
        return None;
    }

    Some(
        selections
            .iter()
            .map(|selection| tab.buffer().slice(selection.range()).to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn clipboard_lines_for_distribution(text: &str) -> Vec<String> {
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

pub(crate) fn delete_request<F>(tab: &EditorTab, boundary: UndoBoundary, cursor_range: F) -> Option<EditRequest>
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
    let merged_ranges = merge_delete_ranges(requested_ranges.iter().map(|(_, range)| range.clone()));
    let changes: Vec<TextChange> = merged_ranges.iter().cloned().map(TextChange::delete).collect();
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
    let selection_after = SelectionSet::from_selections_coalescing_cursors(selections_after, primary)
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

pub(crate) fn rectangular_selection_set(tab: &EditorTab, anchor: usize, head: usize) -> Option<SelectionSet> {
    let buffer = tab.buffer();
    let anchor = char_to_position(buffer, anchor);
    let head = char_to_position(buffer, head);
    let first_line = anchor.line.min(head.line);
    let last_line = anchor.line.max(head.line);
    let start_column = anchor.column.min(head.column);
    let end_column = anchor.column.max(head.column);
    let reversed = head.column < anchor.column;
    let primary = head.line.clamp(first_line, last_line) - first_line;
    let selections = (first_line..=last_line)
        .map(|line| {
            let start = char_at_line_column(buffer, line, start_column);
            let end = char_at_line_column(buffer, line, end_column);
            Selection::from_range(start..end, reversed)
        })
        .collect();
    SelectionSet::from_selections_coalescing_cursors(selections, primary).ok()
}

pub(crate) fn next_occurrence_addition(tab: &EditorTab, find: &FindState) -> Option<Selection> {
    let (query, _) = occurrence_query(tab)?;
    let regex = build_query_regex(&query, find.case_sensitive, find.whole_word, false).ok()?;
    let text = tab.buffer_text();
    let primary_end = tab.selection().range().end;
    let selection_set = tab.selection_set();

    let mut wrap_candidate: Option<Range<usize>> = None;
    for range in regex_char_ranges(&text, &regex) {
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

pub(crate) fn all_occurrences_set(tab: &EditorTab, find: &FindState) -> Option<SelectionSet> {
    let (query, query_range) = occurrence_query(tab)?;
    let ranges = occurrence_ranges(&tab.buffer_text(), &query, find);
    if ranges.is_empty() {
        return None;
    }
    let primary = ranges.iter().position(|range| *range == query_range).unwrap_or(0);
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
        occurrence_word_range_at_cursor(tab)
    };
    if range.start == range.end {
        return None;
    }
    Some((tab.buffer().slice(range.clone()).to_string(), range))
}

fn occurrence_word_range_at_cursor(tab: &EditorTab) -> Range<usize> {
    let buffer = tab.buffer();
    let mut range = word_range_at_char(buffer, tab.cursor_char());
    if range.start == range.end || !range_is_whitespace(buffer, &range) {
        return range;
    }

    let mut next = range.end;
    while next < buffer.len_chars() && buffer.char(next).is_whitespace() {
        next += 1;
    }
    if next < buffer.len_chars() {
        range = word_range_at_char(buffer, next);
    }
    range
}

fn range_is_whitespace(buffer: &ropey::Rope, range: &Range<usize>) -> bool {
    range.start < range.end && buffer.slice(range.clone()).chars().all(char::is_whitespace)
}

fn occurrence_ranges(text: &str, query: &str, find: &FindState) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    if !find.whole_word && query.is_ascii() && text.is_ascii() {
        let ignore_case = !find.case_sensitive && !query.chars().any(|c| c.is_uppercase());
        return ascii_literal_ranges(text.as_bytes(), query.as_bytes(), ignore_case);
    }
    // Selection-as-query stays literal; only case-related flags apply.
    let regex = match build_query_regex(query, find.case_sensitive, find.whole_word, false) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };

    regex_char_ranges(text, &regex).collect()
}

fn ascii_literal_ranges(text: &[u8], query: &[u8], ignore_case: bool) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let first = query[0];
    while start + query.len() <= text.len() {
        let end = start + query.len();
        let first_matches = if ignore_case {
            text[start].eq_ignore_ascii_case(&first)
        } else {
            text[start] == first
        };
        let matched = first_matches
            && if ignore_case {
                text[start..end].eq_ignore_ascii_case(query)
            } else {
                &text[start..end] == query
            };
        if matched {
            ranges.push(start..end);
            start = end;
        } else {
            start += 1;
        }
    }
    ranges
}

fn regex_char_ranges<'a>(text: &'a str, regex: &'a regex::Regex) -> impl Iterator<Item = Range<usize>> + 'a {
    let mut last_match_end_byte = 0usize;
    let mut last_match_end_char = 0usize;
    regex.find_iter(text).filter_map(move |m| {
        let start_byte = m.start();
        let end_byte = m.end();
        if start_byte == end_byte {
            return None;
        }
        let chars_in_gap = text[last_match_end_byte..start_byte].chars().count();
        let match_start_char = last_match_end_char + chars_in_gap;
        let chars_in_match = text[start_byte..end_byte].chars().count();
        let match_end_char = match_start_char + chars_in_match;
        last_match_end_byte = end_byte;
        last_match_end_char = match_end_char;
        Some(match_start_char..match_end_char)
    })
}

fn range_overlaps_selection_set(range: &Range<usize>, selection_set: &SelectionSet) -> bool {
    selection_set.as_slice().iter().any(|selection| {
        let selected = selection.range();
        range.start < selected.end && selected.start < range.end
    })
}
