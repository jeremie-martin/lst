use crate::{
    document::{char_to_position, line_indent_prefix, EditKind, UndoBoundary},
    language::LanguageConfig,
    multi_selection::{self, request_for_each, SelectionEdit},
    selection::{
        ceil_grapheme_boundary, display_line_char_len, floor_grapheme_boundary, is_identifier_char,
        next_grapheme_boundary, next_word_boundary, previous_grapheme_boundary, previous_word_boundary, Selection,
        SelectionSet,
    },
    tab::EditorTab,
    transaction::{EditRequest, SelectionAfter, TextChange, TextChangeSet},
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
    if let Some((edit_range, replacement, new_selection)) = auto_pair_surround_edit(tab, &resolved_range, &text) {
        let reversed = tab.selection_reversed();
        let relative_selection =
            new_selection.start.saturating_sub(edit_range.start)..new_selection.end.saturating_sub(edit_range.start);
        return TextInputAction::Edit {
            request: EditRequest::single(EditKind::Insert, UndoBoundary::Break, edit_range, replacement)
                .with_selection_after(SelectionAfter::InsertedRange {
                    range: relative_selection,
                    reversed,
                }),
            align_find_current: true,
        };
    }
    if let Some((edit_range, replacement, caret)) = auto_pair_insert_edit(tab, &resolved_range, &text) {
        let relative_caret = caret.saturating_sub(edit_range.start);
        return TextInputAction::Edit {
            request: EditRequest::single(EditKind::Insert, UndoBoundary::Break, edit_range, replacement)
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
pub(crate) fn replace_request(
    tab: &EditorTab,
    range: Option<Range<usize>>,
    text: String,
    boundary: UndoBoundary,
) -> EditRequest {
    if range.is_none() {
        if let Some(request) = multi_selection::replacement_request(tab, text.clone(), boundary) {
            return request;
        }
    }
    let kind = if text.is_empty() {
        EditKind::Delete
    } else {
        EditKind::Insert
    };
    EditRequest::single(kind, boundary, resolve_range(tab, range), text)
}
pub(crate) fn delete_selected_or_previous_request(tab: &EditorTab) -> Option<EditRequest> {
    delete_request(tab, UndoBoundary::Merge, |tab, cursor| {
        (cursor > 0).then(|| {
            soft_tab_backspace_range_at(tab, cursor)
                .unwrap_or_else(|| previous_grapheme_boundary(tab.buffer(), cursor)..cursor)
        })
    })
}

pub(crate) fn delete_selected_or_previous_pair_aware_request(tab: &EditorTab) -> Option<EditRequest> {
    let has_empty_pair = tab
        .selection_set()
        .as_slice()
        .iter()
        .any(|selection| !selection.has_selection() && empty_auto_pair_range_at(tab, selection.cursor()).is_some());
    if !has_empty_pair {
        return delete_selected_or_previous_request(tab);
    }

    delete_request(tab, UndoBoundary::Break, |tab, cursor| {
        empty_auto_pair_range_at(tab, cursor).or_else(|| {
            (cursor > 0).then(|| {
                soft_tab_backspace_range_at(tab, cursor)
                    .unwrap_or_else(|| previous_grapheme_boundary(tab.buffer(), cursor)..cursor)
            })
        })
    })
}

pub(crate) fn delete_selected_or_next_request(tab: &EditorTab) -> Option<EditRequest> {
    delete_request(tab, UndoBoundary::Merge, |tab, cursor| {
        (cursor < tab.len_chars()).then(|| cursor..next_grapheme_boundary(tab.buffer(), cursor))
    })
}

pub(crate) fn delete_selected_or_word_request(tab: &EditorTab, backward: bool) -> Option<EditRequest> {
    delete_request(tab, UndoBoundary::Break, |tab, cursor| {
        delete_word_range_at(tab, cursor, backward)
    })
}
fn delete_request<F>(tab: &EditorTab, boundary: UndoBoundary, cursor_range: F) -> Option<EditRequest>
where
    F: Fn(&EditorTab, usize) -> Option<Range<usize>>,
{
    if tab.selection_set().has_multiple() {
        return multi_selection::delete_request(tab, boundary, cursor_range);
    }
    let range = if tab.has_selection() {
        tab.selected_range()
    } else {
        cursor_range(tab, tab.cursor_char())?
    };
    Some(EditRequest::single(EditKind::Delete, boundary, range, String::new()))
}
pub(crate) fn newline_request(tab: &EditorTab) -> EditRequest {
    let newline = preferred_newline(tab);
    let buffer = tab.buffer();
    let len_chars = tab.len_chars();
    let replacements: Vec<String> = tab
        .selection_set()
        .as_slice()
        .iter()
        .map(|selection| {
            format!(
                "{newline}{}",
                carried_indent(buffer, selection.range().start.min(len_chars))
            )
        })
        .collect();
    multi_selection::replacement_request_by_index(tab, |index| replacements[index].clone(), UndoBoundary::Break)
        .unwrap_or_else(|| {
            EditRequest::single(
                EditKind::Insert,
                UndoBoundary::Break,
                resolve_range(tab, None),
                replacements[tab.selection_set().primary_index()].clone(),
            )
        })
}

pub(crate) fn smart_newline_request(tab: &EditorTab) -> EditRequest {
    if let Some(request) = request_for_each(tab, EditKind::Insert, UndoBoundary::Break, |_index, selection| {
        let (replacement, caret) = newline_replacement(tab, &selection.range(), true);
        Some(SelectionEdit::replace_with_inserted_range(
            selection.range(),
            replacement,
            caret..caret,
            false,
        ))
    }) {
        return request;
    }

    let range = resolve_range(tab, None);
    let (replacement, caret) = newline_replacement(tab, &range, true);
    EditRequest::single(EditKind::Insert, UndoBoundary::Break, range, replacement).with_selection_after(
        SelectionAfter::InsertedRange {
            range: caret..caret,
            reversed: false,
        },
    )
}

pub(crate) fn tab_request(tab: &EditorTab) -> EditRequest {
    let replacements = tab
        .selection_set()
        .as_slice()
        .iter()
        .map(|selection| tab_text_at(tab, selection.cursor()))
        .collect::<Vec<_>>();
    multi_selection::replacement_request_by_index(tab, |index| replacements[index].clone(), UndoBoundary::Break)
        .unwrap_or_else(|| {
            EditRequest::single(
                EditKind::Insert,
                UndoBoundary::Break,
                resolve_range(tab, None),
                replacements[tab.selection_set().primary_index()].clone(),
            )
        })
}

pub(crate) fn linewise_paste_request(tab: &EditorTab, mut text: String) -> EditRequest {
    if !text.ends_with(['\n', '\r']) {
        text.push_str(preferred_newline(tab));
    }
    let line_starts = tab
        .selection_set()
        .as_slice()
        .iter()
        .map(|selection| {
            let line = tab.buffer().char_to_line(selection.cursor().min(tab.len_chars()));
            tab.buffer().line_to_char(line)
        })
        .collect::<Vec<_>>();
    let mut unique_starts = line_starts.clone();
    unique_starts.sort_unstable();
    unique_starts.dedup();
    let primary_start = line_starts[tab.selection_set().primary_index()];
    let primary_change = unique_starts
        .binary_search(&primary_start)
        .expect("primary cursor line is included in linewise paste changes");
    let changes = TextChangeSet::new(
        unique_starts
            .into_iter()
            .map(|start| TextChange::insert(start, text.clone()))
            .collect(),
        primary_change,
    );
    let selections = line_starts
        .into_iter()
        .map(|start| Selection::collapsed(changes.map_offset_to_inserted_end(start)))
        .collect();
    let selection_after =
        SelectionSet::from_selections_coalescing_cursors(selections, tab.selection_set().primary_index())
            .expect("linewise paste preserves a valid selection set");
    EditRequest::from_changes(EditKind::Insert, UndoBoundary::Break, changes)
        .with_selection_after(SelectionAfter::Exact(selection_after))
}
pub(crate) fn transpose_request(tab: &EditorTab) -> Option<EditRequest> {
    let buffer = tab.buffer();
    let cursor = tab.cursor_char();
    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    let line_end = line_start + display_line_char_len(buffer, line);

    let mid = if cursor == line_start {
        let mid = next_grapheme_boundary(buffer, line_start);
        if mid >= line_end {
            return None;
        }
        mid
    } else if cursor >= line_end {
        let mid = previous_grapheme_boundary(buffer, line_end);
        if mid <= line_start {
            return None;
        }
        mid
    } else {
        cursor
    };
    let left_start = previous_grapheme_boundary(buffer, mid);
    let right_end = next_grapheme_boundary(buffer, mid);
    if left_start < line_start || right_end > line_end {
        return None;
    }

    let first = buffer.slice(left_start..mid).to_string();
    let second = buffer.slice(mid..right_end).to_string();
    Some(
        EditRequest::single(
            EditKind::Other,
            UndoBoundary::Break,
            left_start..right_end,
            format!("{second}{first}"),
        )
        .with_selection_after(SelectionAfter::CursorPosition(char_to_position(buffer, right_end))),
    )
}

fn multi_edit_action(
    tab: &EditorTab,
    range: Option<&Range<usize>>,
    text: &str,
    boundary: UndoBoundary,
) -> Option<TextInputAction> {
    if range.is_some_and(|range| *range != tab.selected_range()) {
        return None;
    }

    let edit = |request, align_find_current| TextInputAction::Edit {
        request,
        align_find_current,
    };

    if let Some(request) = request_for_each(tab, EditKind::Other, UndoBoundary::Merge, |_index, selection| {
        // Overtype skips a closer rather than inserting text. The empty change
        // keeps each selection in the change-set delta accounting; the cursor
        // is then redirected past the existing closer via AbsoluteCursor.
        let cursor = auto_pair_overtype_cursor(tab, &selection.range(), text)?;
        Some(SelectionEdit::insert_with_absolute_cursor(selection.cursor(), cursor))
    }) {
        return Some(edit(request, true));
    }
    if let Some(request) = request_for_each(tab, EditKind::Insert, UndoBoundary::Break, |_index, selection| {
        let range = auto_dedent_close_brace_range(tab, &selection.range(), text)?;
        Some(SelectionEdit::replace_with_collapsed_end(range, text.to_string()))
    }) {
        return Some(edit(request, false));
    }
    if let Some(request) = request_for_each(tab, EditKind::Insert, UndoBoundary::Break, |_index, selection| {
        let (edit_range, replacement, new_selection) = auto_pair_surround_edit(tab, &selection.range(), text)?;
        let relative_selection =
            new_selection.start.saturating_sub(edit_range.start)..new_selection.end.saturating_sub(edit_range.start);
        Some(SelectionEdit::replace_with_inserted_range(
            edit_range,
            replacement,
            relative_selection,
            selection.is_reversed(),
        ))
    }) {
        return Some(edit(request, true));
    }
    if let Some(request) = request_for_each(tab, EditKind::Insert, UndoBoundary::Break, |_index, selection| {
        let (edit_range, replacement, caret) = auto_pair_insert_edit(tab, &selection.range(), text)?;
        let relative_caret = caret.saturating_sub(edit_range.start);
        Some(SelectionEdit::replace_with_inserted_range(
            edit_range,
            replacement,
            relative_caret..relative_caret,
            false,
        ))
    }) {
        return Some(edit(request, true));
    }

    let request = multi_selection::replacement_request(tab, text.to_string(), boundary)?;
    Some(edit(request, false))
}

pub(crate) fn resolve_range(tab: &EditorTab, range: Option<Range<usize>>) -> Range<usize> {
    let range = range
        .or_else(|| tab.marked_range().cloned())
        .unwrap_or_else(|| tab.selected_range());
    floor_grapheme_boundary(tab.buffer(), range.start)..ceil_grapheme_boundary(tab.buffer(), range.end)
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
        .map(|range| SelectionAfter::InsertedRange { range, reversed: false })
        .unwrap_or(SelectionAfter::CollapseToInsertedEnd);
    let request =
        EditRequest::single(EditKind::Other, UndoBoundary::Break, range, text).with_selection_after(selection_after);
    if inserted_chars == 0 {
        request
    } else {
        request.with_marked_range_after(0..inserted_chars)
    }
}
pub(crate) fn preferred_newline(tab: &EditorTab) -> &'static str {
    let mut chars = tab.buffer().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            return if chars.peek() == Some(&'\n') { "\r\n" } else { "\n" };
        }
        if ch == '\n' {
            return "\n";
        }
    }
    "\n"
}
fn soft_tab_backspace_range_at(tab: &EditorTab, cursor: usize) -> Option<Range<usize>> {
    let cfg = tab.language_config();
    if cfg.indent.uses_tabs() {
        return None;
    }
    let unit = cfg.indent.width();
    let buffer = tab.buffer();
    let line = buffer.char_to_line(cursor);
    let col = cursor - buffer.line_to_char(line);
    if unit == 0 || col == 0 || !col.is_multiple_of(unit) {
        return None;
    }
    let prefix_len = buffer.line(line).chars().take_while(|ch| *ch == ' ').take(col).count();
    (col <= prefix_len).then_some((cursor - unit)..cursor)
}

fn empty_auto_pair_range_at(tab: &EditorTab, cursor: usize) -> Option<Range<usize>> {
    if cursor == 0 || cursor >= tab.len_chars() {
        return None;
    }
    let opener = tab.buffer().char(cursor - 1);
    let closer = tab.buffer().char(cursor);
    tab.language_config()
        .auto_pairs
        .contains(&(opener, closer))
        .then_some((cursor - 1)..(cursor + 1))
}

/// The indentation a newline inserted at `at` carries to the next line: the
/// line's leading whitespace, truncated at the cursor so pressing Enter
/// inside the indent does not duplicate the part behind the cursor.
fn carried_indent(buffer: &ropey::Rope, at: usize) -> String {
    let at = at.min(buffer.len_chars());
    let line = buffer.char_to_line(at);
    let column = at - buffer.line_to_char(line);
    line_indent_prefix(buffer, line).chars().take(column).collect()
}

fn newline_replacement(tab: &EditorTab, range: &Range<usize>, smart_pair: bool) -> (String, usize) {
    let newline = preferred_newline(tab);
    let buffer = tab.buffer();
    let outer_indent = carried_indent(buffer, range.start);
    if smart_pair && range.start == range.end && non_quote_auto_pair_at(tab, range.start).is_some() {
        let inner_indent = format!("{outer_indent}{}", tab.language_config().indent.indent_unit());
        let replacement = format!("{newline}{inner_indent}{newline}{outer_indent}");
        let caret = newline.chars().count() + inner_indent.chars().count();
        return (replacement, caret);
    }

    let replacement = format!("{newline}{outer_indent}");
    let caret = replacement.chars().count();
    (replacement, caret)
}

fn non_quote_auto_pair_at(tab: &EditorTab, cursor: usize) -> Option<(char, char)> {
    if cursor == 0 || cursor >= tab.len_chars() {
        return None;
    }
    let pair = (tab.buffer().char(cursor - 1), tab.buffer().char(cursor));
    (pair.0 != pair.1 && tab.language_config().auto_pairs.contains(&pair)).then_some(pair)
}

fn tab_text_at(tab: &EditorTab, cursor: usize) -> String {
    let indent = tab.language_config().indent;
    if indent.uses_tabs() {
        return "\t".to_string();
    }
    let width = indent.width();
    if width == 0 {
        return String::new();
    }

    let cursor = cursor.min(tab.len_chars());
    let line = tab.buffer().char_to_line(cursor);
    let line_start = tab.buffer().line_to_char(line);
    let visual_column = tab.buffer().slice(line_start..cursor).chars().fold(0, |column, ch| {
        if ch == '\t' {
            column + width - (column % width)
        } else {
            column + 1
        }
    });
    " ".repeat(width - (visual_column % width))
}
fn delete_word_range_at(tab: &EditorTab, cursor: usize, backward: bool) -> Option<Range<usize>> {
    if backward {
        delete_word_backward_range(tab.buffer(), cursor)
    } else {
        delete_word_forward_range(tab.buffer(), cursor)
    }
}
fn delete_word_backward_range(buffer: &ropey::Rope, cursor: usize) -> Option<Range<usize>> {
    let cursor = cursor.min(buffer.len_chars());
    if cursor == 0 {
        return None;
    }

    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    if cursor == line_start {
        return Some(previous_line_break_start(buffer, cursor)?..cursor);
    }
    if buffer.char(cursor - 1).is_whitespace() {
        let mut start = cursor;
        while start > line_start && buffer.char(start - 1).is_whitespace() {
            start -= 1;
        }
        return (start < cursor).then_some(start..cursor);
    }

    let target = previous_word_boundary(buffer, cursor);
    (target != cursor).then_some(target..cursor)
}
fn delete_word_forward_range(buffer: &ropey::Rope, cursor: usize) -> Option<Range<usize>> {
    let cursor = cursor.min(buffer.len_chars());
    if cursor >= buffer.len_chars() {
        return None;
    }

    let line = buffer.char_to_line(cursor);
    let line_start = buffer.line_to_char(line);
    let line_end = line_start + display_line_char_len(buffer, line);
    if cursor >= line_end {
        let end = next_line_join_whitespace_end(buffer, cursor)?;
        return (end > cursor).then_some(cursor..end);
    }
    if buffer.char(cursor).is_whitespace() {
        let mut end = cursor;
        while end < line_end && buffer.char(end).is_whitespace() {
            end += 1;
        }
        if end > cursor {
            return if end - cursor == 1 {
                let target = next_word_boundary(buffer, cursor);
                (target != cursor).then_some(cursor..target)
            } else {
                Some(cursor..end)
            };
        }
    }

    let target = next_word_boundary(buffer, cursor);
    (target != cursor).then_some(cursor..target)
}

fn previous_line_break_start(buffer: &ropey::Rope, cursor: usize) -> Option<usize> {
    let mut start = cursor.checked_sub(1)?;
    if buffer.char(start) == '\n' && start > 0 && buffer.char(start - 1) == '\r' {
        start -= 1;
    }
    Some(start)
}
fn next_line_join_whitespace_end(buffer: &ropey::Rope, cursor: usize) -> Option<usize> {
    let len = buffer.len_chars();
    let mut end = cursor;
    if end >= len {
        return None;
    }

    match buffer.char(end) {
        '\r' => {
            end += 1;
            if end < len && buffer.char(end) == '\n' {
                end += 1;
            }
        }
        '\n' => end += 1,
        ch if ch.is_whitespace() => return Some(horizontal_whitespace_end(buffer, end, len)),
        _ => return None,
    }
    Some(horizontal_whitespace_end(buffer, end, len))
}
fn horizontal_whitespace_end(buffer: &ropey::Rope, mut end: usize, len: usize) -> usize {
    while end < len {
        let ch = buffer.char(end);
        if ch.is_whitespace() && ch != '\n' && ch != '\r' {
            end += 1;
        } else {
            break;
        }
    }
    end
}

fn auto_dedent_close_brace_range(tab: &EditorTab, range: &Range<usize>, text: &str) -> Option<Range<usize>> {
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
    if !buffer.slice(line_start..line_end).chars().all(|ch| ch == ' ') {
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
    Some((range.clone(), replacement, (range.start + 1)..(range.end + 1)))
}

fn auto_pair_insert_edit(tab: &EditorTab, range: &Range<usize>, text: &str) -> Option<(Range<usize>, String, usize)> {
    if range.start != range.end {
        return None;
    }
    let ch = single_char(text)?;
    let (opener, closer) = auto_pair_pair_for(tab.language_config(), ch)?;
    if ch != opener {
        return None;
    }

    let buffer = tab.buffer();
    if range.end < buffer.len_chars() && is_identifier_char(buffer.char(range.end)) {
        return None;
    }

    if is_auto_pair_quote(ch) {
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
