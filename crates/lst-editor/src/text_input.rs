use crate::{
    document::{EditKind, UndoBoundary},
    language::LanguageConfig,
    multi_selection,
    selection::{display_line_char_len, is_identifier_char, Selection, SelectionSet},
    tab::EditorTab,
    transaction::{offset_with_delta, EditRequest, SelectionAfter, TextChange, TextChangeSet},
};
use std::ops::Range;

pub(crate) enum TextInputAction {
    MoveCursor(usize),
    Edit {
        request: EditRequest,
        align_find_current: bool,
    },
}

pub(crate) fn edit_action(
    tab: &EditorTab,
    range: Option<Range<usize>>,
    text: String,
    boundary: UndoBoundary,
) -> TextInputAction {
    if let Some(action) = multi_edit_action(tab, range.as_ref(), &text, boundary) {
        return action;
    }

    let resolved_range = resolve_range(tab, range);
    if let Some(new_cursor) = auto_pair_overtype_cursor(tab, &resolved_range, &text) {
        return TextInputAction::MoveCursor(new_cursor);
    }
    if let Some(dedent_range) = auto_dedent_close_brace_range(tab, &resolved_range, &text) {
        return TextInputAction::Edit {
            request: EditRequest::single(EditKind::Insert, UndoBoundary::Break, dedent_range, text),
            align_find_current: false,
        };
    }
    if let Some((edit_range, replacement, new_selection)) =
        auto_pair_surround_edit(tab, &resolved_range, &text)
    {
        let reversed = tab.selection_reversed();
        let relative_selection = new_selection.start.saturating_sub(edit_range.start)
            ..new_selection.end.saturating_sub(edit_range.start);
        return TextInputAction::Edit {
            request: EditRequest::single(
                EditKind::Insert,
                UndoBoundary::Break,
                edit_range,
                replacement,
            )
            .with_selection_after(SelectionAfter::InsertedRange {
                range: relative_selection,
                reversed,
            }),
            align_find_current: true,
        };
    }
    if let Some((edit_range, replacement, caret)) =
        auto_pair_insert_edit(tab, &resolved_range, &text)
    {
        let relative_caret = caret.saturating_sub(edit_range.start);
        return TextInputAction::Edit {
            request: EditRequest::single(
                EditKind::Insert,
                UndoBoundary::Break,
                edit_range,
                replacement,
            )
            .with_selection_after(SelectionAfter::InsertedRange {
                range: relative_caret..relative_caret,
                reversed: false,
            }),
            align_find_current: true,
        };
    }

    let kind = if text.is_empty() {
        EditKind::Delete
    } else {
        EditKind::Insert
    };
    TextInputAction::Edit {
        request: EditRequest::single(kind, boundary, resolved_range, text),
        align_find_current: false,
    }
}

fn multi_edit_action(
    tab: &EditorTab,
    range: Option<&Range<usize>>,
    text: &str,
    boundary: UndoBoundary,
) -> Option<TextInputAction> {
    let selection_set = tab.selection_set();
    if !selection_set.has_multiple() || tab.marked_range().is_some() {
        return None;
    }
    if range.is_some_and(|range| *range != tab.selected_range()) {
        return None;
    }

    let edit = |request, align_find_current| TextInputAction::Edit {
        request,
        align_find_current,
    };

    if let Some(request) = multi_request(tab, EditKind::Other, UndoBoundary::Merge, |selection| {
        // Overtype skips a closer rather than inserting text. The empty change
        // keeps each selection in the change-set delta accounting; the cursor
        // is then redirected past the existing closer via AbsoluteCursor.
        let cursor = auto_pair_overtype_cursor(tab, &selection.range(), text)?;
        Some((
            TextChange::insert(selection.cursor(), ""),
            MultiSelectionAfter::AbsoluteCursor(cursor),
        ))
    }) {
        return Some(edit(request, true));
    }
    if let Some(request) = multi_request(tab, EditKind::Insert, UndoBoundary::Break, |selection| {
        let range = auto_dedent_close_brace_range(tab, &selection.range(), text)?;
        let inserted_chars = text.chars().count();
        Some((
            TextChange::replace(range, text.to_string()),
            MultiSelectionAfter::InsertedRange(inserted_chars..inserted_chars, false),
        ))
    }) {
        return Some(edit(request, false));
    }
    if let Some(request) = multi_request(tab, EditKind::Insert, UndoBoundary::Break, |selection| {
        let (edit_range, replacement, new_selection) =
            auto_pair_surround_edit(tab, &selection.range(), text)?;
        let relative_selection = new_selection.start.saturating_sub(edit_range.start)
            ..new_selection.end.saturating_sub(edit_range.start);
        Some((
            TextChange::replace(edit_range, replacement),
            MultiSelectionAfter::InsertedRange(relative_selection, selection.is_reversed()),
        ))
    }) {
        return Some(edit(request, true));
    }
    if let Some(request) = multi_request(tab, EditKind::Insert, UndoBoundary::Break, |selection| {
        let (edit_range, replacement, caret) =
            auto_pair_insert_edit(tab, &selection.range(), text)?;
        let relative_caret = caret.saturating_sub(edit_range.start);
        Some((
            TextChange::replace(edit_range, replacement),
            MultiSelectionAfter::InsertedRange(relative_caret..relative_caret, false),
        ))
    }) {
        return Some(edit(request, true));
    }

    let request = multi_selection::replacement_request(tab, text.to_string(), boundary)?;
    Some(edit(request, false))
}

enum MultiSelectionAfter {
    AbsoluteCursor(usize),
    InsertedRange(Range<usize>, bool),
}

/// Builds a multi-selection edit request by asking `per_selection` to produce a
/// `(TextChange, MultiSelectionAfter)` for every selection. Returns `None` as
/// soon as any selection opts out, so handlers behave atomically: either every
/// cursor participates or the dispatcher falls through to the next handler.
fn multi_request<F>(
    tab: &EditorTab,
    kind: EditKind,
    boundary: UndoBoundary,
    per_selection: F,
) -> Option<EditRequest>
where
    F: Fn(&Selection) -> Option<(TextChange, MultiSelectionAfter)>,
{
    let selection_set = tab.selection_set();
    let pairs: Vec<(TextChange, MultiSelectionAfter)> = selection_set
        .as_slice()
        .iter()
        .map(per_selection)
        .collect::<Option<_>>()?;

    let mut delta = 0isize;
    let mut text_changes = Vec::with_capacity(pairs.len());
    let mut selections_after = Vec::with_capacity(pairs.len());
    for (change, selection_after) in pairs {
        let inserted_start = offset_with_delta(change.range.start, delta);
        let inserted_len = change.replacement.chars().count();
        let selection = match selection_after {
            MultiSelectionAfter::AbsoluteCursor(cursor) => Selection::collapsed(cursor),
            MultiSelectionAfter::InsertedRange(range, reversed) => {
                let start = range.start.min(inserted_len);
                let end = range.end.min(inserted_len);
                Selection::from_range(inserted_start + start..inserted_start + end, reversed)
            }
        };
        delta += inserted_len as isize - (change.range.end - change.range.start) as isize;
        text_changes.push(change);
        selections_after.push(selection);
    }

    let text_changes = TextChangeSet::try_new(text_changes, selection_set.primary_index())?;
    let selections_after = SelectionSet::from_selections_coalescing_cursors(
        selections_after,
        selection_set.primary_index(),
    )
    .ok()?;
    Some(
        EditRequest::from_changes(kind, boundary, text_changes)
            .with_selection_after(SelectionAfter::Exact(selections_after)),
    )
}

pub(crate) fn resolve_range(tab: &EditorTab, range: Option<Range<usize>>) -> Range<usize> {
    range
        .or_else(|| tab.marked_range().cloned())
        .unwrap_or_else(|| tab.selected_range())
}

pub(crate) fn marked_text_request(
    tab: &EditorTab,
    range: Option<Range<usize>>,
    text: String,
    selected_range: Option<Range<usize>>,
) -> EditRequest {
    let range = resolve_range(tab, range);
    let inserted_chars = text.chars().count();
    let selection_after = selected_range
        .map(|range| SelectionAfter::InsertedRange {
            range,
            reversed: false,
        })
        .unwrap_or(SelectionAfter::CollapseToInsertedEnd);
    let request = EditRequest::single(EditKind::Other, UndoBoundary::Break, range, text)
        .with_selection_after(selection_after);
    if inserted_chars == 0 {
        request
    } else {
        request.with_marked_range_after(0..inserted_chars)
    }
}

fn auto_dedent_close_brace_range(
    tab: &EditorTab,
    range: &Range<usize>,
    text: &str,
) -> Option<Range<usize>> {
    let ch = single_char(text)?;
    let config = tab.language_config();
    if !config.auto_dedent_closers.contains(&ch) {
        return None;
    }
    if config.indent.uses_tabs() {
        return None;
    }

    let buffer = tab.buffer();
    let line = buffer.char_to_line(range.start);
    if line != buffer.char_to_line(range.end) {
        return None;
    }

    let line_start = buffer.line_to_char(line);
    let line_end = line_start + display_line_char_len(buffer, line);
    if !buffer
        .slice(line_start..line_end)
        .chars()
        .all(|ch| ch == ' ')
    {
        return None;
    }

    let width = config.indent.width();
    let dedent_start = range.start.saturating_sub(width).max(line_start);
    if dedent_start == range.start {
        return None;
    }
    Some(dedent_start..range.end)
}

fn auto_pair_overtype_cursor(tab: &EditorTab, range: &Range<usize>, text: &str) -> Option<usize> {
    if range.start != range.end {
        return None;
    }
    let ch = single_char(text)?;
    let (_, closer) = auto_pair_pair_for(tab.language_config(), ch)?;
    if ch != closer {
        return None;
    }
    let buffer = tab.buffer();
    if range.end >= buffer.len_chars() {
        return None;
    }
    if buffer.char(range.end) != closer {
        return None;
    }
    Some(range.end + 1)
}

fn auto_pair_surround_edit(
    tab: &EditorTab,
    range: &Range<usize>,
    text: &str,
) -> Option<(Range<usize>, String, Range<usize>)> {
    if range.start >= range.end {
        return None;
    }
    let ch = single_char(text)?;
    let (opener, closer) = auto_pair_pair_for(tab.language_config(), ch)?;
    if ch != opener {
        return None;
    }
    let selected = tab.buffer().slice(range.clone()).to_string();
    let mut replacement = String::with_capacity(selected.len() + 2);
    replacement.push(opener);
    replacement.push_str(&selected);
    replacement.push(closer);
    Some((
        range.clone(),
        replacement,
        (range.start + 1)..(range.end + 1),
    ))
}

fn auto_pair_insert_edit(
    tab: &EditorTab,
    range: &Range<usize>,
    text: &str,
) -> Option<(Range<usize>, String, usize)> {
    if range.start != range.end {
        return None;
    }
    let ch = single_char(text)?;
    let (opener, closer) = auto_pair_pair_for(tab.language_config(), ch)?;
    if ch != opener {
        return None;
    }

    if is_auto_pair_quote(ch) {
        let buffer = tab.buffer();
        if range.start > 0 {
            let prev = buffer.char(range.start - 1);
            if prev == '\\' || prev == ch || is_identifier_char(prev) {
                return None;
            }
        }
        if range.end < buffer.len_chars() {
            let next = buffer.char(range.end);
            if next == ch || is_identifier_char(next) {
                return None;
            }
        }
    }

    let mut replacement = String::with_capacity(2);
    replacement.push(opener);
    replacement.push(closer);
    Some((range.clone(), replacement, range.start + 1))
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(ch)
}

fn auto_pair_pair_for(config: &LanguageConfig, ch: char) -> Option<(char, char)> {
    if is_auto_pair_quote(ch) && config.auto_pair_suppress_quotes.contains(&ch) {
        return None;
    }
    config
        .auto_pairs
        .iter()
        .copied()
        .find(|(opener, closer)| *opener == ch || *closer == ch)
}

fn is_auto_pair_quote(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '`')
}
