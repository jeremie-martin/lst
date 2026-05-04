use crate::{
    document::{char_to_position, position_to_char, EditKind, UndoBoundary},
    position::Position,
    selection::{line_display_text, line_range_at_char},
    tab::EditorTab,
    transaction::{EditRequest, SelectionAfter, TextChange},
};

pub(crate) enum LineEditAction {
    MoveCursor(Position),
    Edit(EditRequest),
}

struct LineSelectionContext {
    start: Position,
    end: Position,
    reversed: bool,
    had_selection: bool,
    cursor: Position,
}

impl LineSelectionContext {
    fn from_tab(tab: &EditorTab) -> Self {
        let selection = tab.selected_range();
        Self {
            start: char_to_position(tab.buffer(), selection.start),
            end: char_to_position(tab.buffer(), selection.end),
            reversed: tab.selection_reversed(),
            had_selection: tab.has_selection(),
            cursor: tab.cursor_position(),
        }
    }
}

fn span_change(
    buffer: &ropey::Rope,
    newline: &str,
    first: usize,
    last: usize,
    new_lines: &[String],
) -> Option<TextChange> {
    let (range, prefix_newline, trailing_newline) = span_range(buffer, first, last)?;
    let mut replacement = String::new();
    if !new_lines.is_empty() {
        if prefix_newline {
            replacement.push_str(newline);
        }
        replacement.push_str(&new_lines.join(newline));
        if trailing_newline {
            replacement.push_str(newline);
        }
    }
    Some(TextChange::replace(range, replacement))
}

fn insert_change(
    buffer: &ropey::Rope,
    newline: &str,
    insert_at: usize,
    new_lines: &[String],
) -> Option<TextChange> {
    if new_lines.is_empty() {
        return None;
    }

    let line_count = buffer.len_lines().max(1);
    let mut replacement = new_lines.join(newline);
    let offset = if insert_at < line_count {
        replacement.push_str(newline);
        buffer.line_to_char(insert_at)
    } else {
        if insert_at > 0 {
            replacement.insert_str(0, newline);
        }
        buffer.len_chars()
    };
    Some(TextChange::insert(offset, replacement))
}

pub(super) fn replace_lines_change(
    tab: &EditorTab,
    first: usize,
    last: usize,
    new_lines: &[String],
) -> Option<TextChange> {
    span_change(
        tab.buffer(),
        super::preferred_newline_for_active_tab(tab),
        first,
        last,
        new_lines,
    )
}

pub(super) fn insert_lines_change(
    tab: &EditorTab,
    insert_at: usize,
    new_lines: &[String],
) -> Option<TextChange> {
    insert_change(
        tab.buffer(),
        super::preferred_newline_for_active_tab(tab),
        insert_at,
        new_lines,
    )
}

pub(super) fn clamped_line_span(tab: &EditorTab, first: usize, last: usize) -> (usize, usize) {
    let last_line = tab.line_count().saturating_sub(1);
    (first.min(last_line), last.min(last_line))
}

pub(crate) fn indent_request(tab: &EditorTab, first: usize, last: usize) -> Option<EditRequest> {
    if first > last || last >= tab.line_count() {
        return None;
    }

    let selection = LineSelectionContext::from_tab(tab);
    let unit = tab.language_config().indent.indent_unit();
    let unit_chars = unit.chars().count();
    let shift = |pos: Position| -> Position {
        if (first..=last).contains(&pos.line) && pos.column > 0 {
            Position::new(pos.line, pos.column + unit_chars)
        } else {
            pos
        }
    };
    let new_start = shift(selection.start);
    let new_end = shift(selection.end);

    let changes: Vec<TextChange> = (first..=last)
        .map(|line| {
            let line_start = tab.buffer().line_to_char(line);
            TextChange::insert(line_start, unit.clone())
        })
        .collect();
    Some(EditRequest::other_with_selection(
        changes,
        SelectionAfter::PositionRange {
            start: new_start,
            end: new_end,
            reversed: selection.reversed,
        },
    ))
}

pub(crate) fn outdent_request(tab: &EditorTab, first: usize, last: usize) -> Option<EditRequest> {
    if first > last || last >= tab.line_count() {
        return None;
    }

    let selection = LineSelectionContext::from_tab(tab);
    let unit = tab.language_config().indent.indent_unit();
    let mut removed = Vec::with_capacity(last - first + 1);
    let mut changes = Vec::new();
    for line in first..=last {
        let removed_for_line = outdent_prefix_len(tab, line, &unit);
        removed.push(removed_for_line);
        if removed_for_line > 0 {
            let line_start = tab.buffer().line_to_char(line);
            changes.push(TextChange::delete(
                line_start..line_start + removed_for_line,
            ));
        }
    }
    if changes.is_empty() {
        return None;
    }

    let shift = |pos: Position| -> Position {
        if (first..=last).contains(&pos.line) {
            let removed_for_line = removed.get(pos.line - first).copied().unwrap_or(0);
            Position::new(pos.line, pos.column.saturating_sub(removed_for_line))
        } else {
            pos
        }
    };
    let selection_after = if selection.had_selection {
        SelectionAfter::PositionRange {
            start: shift(selection.start),
            end: shift(selection.end),
            reversed: selection.reversed,
        }
    } else {
        let cursor_line = selection.cursor.line;
        let new_cursor_col = if (first..=last).contains(&cursor_line) {
            let removed_for_cursor = removed.get(cursor_line - first).copied().unwrap_or(0);
            selection.cursor.column.saturating_sub(removed_for_cursor)
        } else {
            selection.cursor.column
        };
        SelectionAfter::CursorPosition(Position::new(cursor_line, new_cursor_col))
    };
    Some(EditRequest::other_with_selection(changes, selection_after))
}

pub(crate) fn delete_line_request(tab: &EditorTab, pos: Position) -> Option<EditRequest> {
    let line = pos.line.min(tab.line_count().saturating_sub(1));
    let change = replace_lines_change(tab, line, line, &[])?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(line, pos.column),
    ))
}

pub(crate) fn line_swap_request(tab: &EditorTab, pos: Position, up: bool) -> Option<EditRequest> {
    let line = pos.line.min(tab.line_count().saturating_sub(1));
    let (first, second, cursor_line) = if up {
        if line == 0 {
            return None;
        }
        (line - 1, line, line - 1)
    } else {
        if line + 1 >= tab.line_count() {
            return None;
        }
        (line, line + 1, line + 1)
    };
    let first_text = line_display_text(tab.buffer(), first);
    let second_text = line_display_text(tab.buffer(), second);
    let change = replace_lines_change(tab, first, second, &[second_text, first_text])?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(cursor_line, pos.column),
    ))
}

pub(crate) fn duplicate_line_request(tab: &EditorTab, pos: Position) -> Option<EditRequest> {
    let line = pos.line.min(tab.line_count().saturating_sub(1));
    let text = line_display_text(tab.buffer(), line);
    let insert_at = line + 1;
    let change = insert_lines_change(tab, insert_at, std::slice::from_ref(&text))?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(insert_at, pos.column),
    ))
}

pub(crate) fn duplicate_selection_request(tab: &EditorTab) -> Option<EditRequest> {
    let text = tab.selected_text()?;
    let range = tab.selected_range();
    let char_len = range.end - range.start;
    let inserted_start = range.end;
    Some(
        EditRequest::single(
            EditKind::Other,
            UndoBoundary::Break,
            inserted_start..inserted_start,
            text,
        )
        .with_selection_after(SelectionAfter::InsertedRange {
            range: 0..char_len,
            reversed: false,
        }),
    )
}

pub(crate) fn toggle_comment_action(tab: &EditorTab, prefix: &str) -> Option<LineEditAction> {
    let selected = tab.selected_range();
    let cursor = tab.cursor_position();
    let start = char_to_position(tab.buffer(), selected.start);
    let end = char_to_position(tab.buffer(), selected.end);
    let first = start.line.min(end.line);
    let last = start.line.max(end.line);
    if first > last || last >= tab.line_count() {
        return None;
    }

    let all_commented = (first..=last).all(|line| {
        let line_text = line_display_text(tab.buffer(), line);
        let trimmed = line_text.trim_start();
        trimmed.is_empty() || trimmed.starts_with(prefix)
    });
    let mut changes = Vec::new();
    let prefix_len = prefix.chars().count();
    for line in first..=last {
        let line_text = line_display_text(tab.buffer(), line);
        let trimmed = line_text.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let line_start = tab.buffer().line_to_char(line);
        let indent_len = line_text
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .count();
        if all_commented {
            let after_prefix = line_text.chars().nth(indent_len + prefix_len);
            let remove_len = prefix_len + usize::from(after_prefix == Some(' '));
            changes.push(TextChange::delete(
                line_start + indent_len..line_start + indent_len + remove_len,
            ));
        } else {
            changes.push(TextChange::insert(
                line_start + indent_len,
                format!("{prefix} "),
            ));
        }
    }

    let delta = prefix_len + 1;
    let cursor_col = if all_commented {
        cursor.column.saturating_sub(delta)
    } else {
        cursor.column + delta
    };
    if changes.is_empty() {
        return Some(LineEditAction::MoveCursor(Position::new(
            cursor.line,
            cursor_col,
        )));
    }
    Some(LineEditAction::Edit(EditRequest::other_at_position(
        changes,
        Position::new(cursor.line, cursor_col),
    )))
}

pub(crate) fn toggle_block_comment_request(
    tab: &EditorTab,
    open: &str,
    close: &str,
) -> Option<EditRequest> {
    let buffer = tab.buffer();
    let range = if tab.has_selection() {
        tab.selected_range()
    } else {
        let mut range = line_range_at_char(buffer, tab.cursor_char());
        while range.end > range.start && matches!(buffer.char(range.end - 1), '\n' | '\r') {
            range.end -= 1;
        }
        range
    };
    let from = char_to_position(buffer, range.start);
    let to = char_to_position(buffer, range.end);
    if from.line >= tab.line_count() || to.line >= tab.line_count() {
        return None;
    }
    if from.line > to.line || (from.line == to.line && from.column > to.column) {
        return None;
    }

    let open_len = open.chars().count();
    let close_len = close.chars().count();
    let from_line = line_display_text(buffer, from.line);
    let to_line = line_display_text(buffer, to.line);
    let starts_at_open = line_has_at(&from_line, from.column, open);
    let ends_at_close =
        to.column >= close_len && line_has_at(&to_line, to.column - close_len, close);

    let from_char = position_to_char(buffer, from);
    let to_char = position_to_char(buffer, to);
    let (changes, cursor) = if starts_at_open && ends_at_close {
        (
            vec![
                TextChange::delete(from_char..from_char + open_len),
                TextChange::delete(to_char - close_len..to_char),
            ],
            from,
        )
    } else {
        (
            vec![
                TextChange::insert(from_char, open),
                TextChange::insert(to_char, close),
            ],
            Position::new(from.line, from.column + open_len),
        )
    };
    Some(EditRequest::other_at_position(changes, cursor))
}

fn outdent_prefix_len(tab: &EditorTab, line: usize, unit: &str) -> usize {
    let line_text = line_display_text(tab.buffer(), line);
    if unit.starts_with('\t') {
        usize::from(line_text.starts_with('\t'))
    } else {
        line_text
            .bytes()
            .take(unit.len())
            .take_while(|byte| *byte == b' ')
            .count()
    }
}

fn line_has_at(line: &str, col: usize, needle: &str) -> bool {
    line.chars()
        .skip(col)
        .take(needle.chars().count())
        .eq(needle.chars())
}

fn span_range(
    buffer: &ropey::Rope,
    first: usize,
    last: usize,
) -> Option<(std::ops::Range<usize>, bool, bool)> {
    let line_count = buffer.len_lines().max(1);
    if first > last || first >= line_count {
        return None;
    }
    let last = last.min(line_count - 1);
    let mut start = buffer.line_to_char(first);
    let end = if last + 1 < buffer.len_lines() {
        buffer.line_to_char(last + 1)
    } else {
        buffer.len_chars()
    };

    let mut prefix_newline = false;
    if last + 1 >= buffer.len_lines() && first > 0 && start > 0 {
        match buffer.char(start - 1) {
            '\n' => {
                start -= 1;
                if start > 0 && buffer.char(start - 1) == '\r' {
                    start -= 1;
                }
                prefix_newline = true;
            }
            '\r' => {
                start -= 1;
                prefix_newline = true;
            }
            _ => {}
        }
    }

    let trailing_newline = end > start && matches!(buffer.char(end - 1), '\n' | '\r');
    Some((start..end, prefix_newline, trailing_newline))
}
