use crate::{
    document::{char_to_position, position_to_char, EditKind, UndoBoundary},
    position::Position,
    selection::{
        display_line_char_len, line_display_text, line_range_at_char, Selection, SelectionSet,
    },
    tab::EditorTab,
    transaction::{apply_change_to_buffer, EditRequest, SelectionAfter, TextChange, TextChangeSet},
};
use std::ops::Range;

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

fn replace_lines_in_place_change(
    tab: &EditorTab,
    first: usize,
    last: usize,
    new_lines: &[String],
) -> Option<TextChange> {
    let (range, trailing_newline) = line_span_without_prefix(tab.buffer(), first, last)?;
    let mut replacement = new_lines.join(super::preferred_newline_for_active_tab(tab));
    if trailing_newline && !replacement.is_empty() {
        replacement.push_str(super::preferred_newline_for_active_tab(tab));
    }
    Some(TextChange::replace(range, replacement))
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

pub(crate) fn outdent_selection_set_request(tab: &EditorTab) -> Option<EditRequest> {
    let lines = selection_set_touched_lines(tab);
    if lines.is_empty() {
        return None;
    }

    let unit = tab.language_config().indent.indent_unit();
    let removed_by_line = lines
        .into_iter()
        .map(|line| (line, outdent_prefix_len(tab, line, &unit)))
        .collect::<Vec<_>>();
    let changes = removed_by_line
        .iter()
        .filter(|(_, removed)| *removed > 0)
        .map(|(line, removed)| {
            let line_start = tab.buffer().line_to_char(*line);
            TextChange::delete(line_start..line_start + *removed)
        })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return None;
    }

    let selections_after = tab
        .selection_set()
        .as_slice()
        .iter()
        .map(|selection| {
            let anchor = map_outdented_endpoint(tab, selection.anchor(), &removed_by_line);
            let head = map_outdented_endpoint(tab, selection.head(), &removed_by_line);
            Selection::new(anchor, head)
        })
        .collect::<Vec<_>>();
    let selection_after = SelectionSet::from_selections_coalescing_cursors(
        selections_after,
        tab.selection_set().primary_index(),
    )
    .expect("line outdent preserves a valid selection set");

    Some(
        EditRequest::other_break(TextChangeSet::new(changes, 0))
            .with_selection_after(SelectionAfter::Exact(selection_after)),
    )
}

pub(crate) fn delete_line_request(tab: &EditorTab, pos: Position) -> Option<EditRequest> {
    let line = pos.line.min(tab.line_count().saturating_sub(1));
    let change = replace_lines_change(tab, line, line, &[])?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(line, pos.column),
    ))
}

pub(crate) fn delete_touched_lines_request(tab: &EditorTab) -> Option<EditRequest> {
    let lines = selection_set_touched_lines(tab);
    if lines.is_empty() {
        return None;
    }
    let changes = line_clusters(&lines)
        .into_iter()
        .filter_map(|cluster| replace_lines_change(tab, cluster.start, cluster.end - 1, &[]))
        .collect::<Vec<_>>();
    request_with_mapped_selection(tab, changes)
}

pub(crate) fn delete_lines_request(tab: &EditorTab, pos: Position) -> Option<EditRequest> {
    if tab.selection_set().has_multiple() {
        if let Some(request) = delete_touched_lines_request(tab) {
            return Some(request);
        }
    }
    delete_line_request(tab, pos)
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
    let change = replace_lines_in_place_change(tab, first, second, &[second_text, first_text])?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(cursor_line, pos.column),
    ))
}

pub(crate) fn move_touched_line_clusters_request(tab: &EditorTab, up: bool) -> Option<EditRequest> {
    let lines = selection_set_touched_lines(tab);
    let clusters = line_clusters(&lines);
    if clusters.is_empty() {
        return None;
    }

    let mut changes = Vec::with_capacity(clusters.len());
    for cluster in &clusters {
        let (first, last, replacement_lines) = if up {
            if cluster.start == 0 {
                return None;
            }
            let mut lines = cluster
                .clone()
                .map(|line| line_display_text(tab.buffer(), line))
                .collect::<Vec<_>>();
            lines.push(line_display_text(tab.buffer(), cluster.start - 1));
            (cluster.start - 1, cluster.end - 1, lines)
        } else {
            if cluster.end >= tab.line_count() {
                return None;
            }
            let mut lines = vec![line_display_text(tab.buffer(), cluster.end)];
            lines.extend(
                cluster
                    .clone()
                    .map(|line| line_display_text(tab.buffer(), line)),
            );
            (cluster.start, cluster.end, lines)
        };
        let change = replace_lines_in_place_change(tab, first, last, &replacement_lines)?;
        changes.push(change);
    }
    request_with_line_move_selection(tab, changes, &clusters, up)
}

pub(crate) fn move_lines_request(tab: &EditorTab, pos: Position, up: bool) -> Option<EditRequest> {
    if tab.selection_set().has_multiple() {
        if let Some(request) = move_touched_line_clusters_request(tab, up) {
            return Some(request);
        }
    }
    line_swap_request(tab, pos, up)
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

pub(crate) fn duplicate_touched_lines_request(tab: &EditorTab) -> Option<EditRequest> {
    let lines = selection_set_touched_lines(tab);
    if lines.is_empty() {
        return None;
    }

    let mut changes = Vec::new();
    for line in &lines {
        let text = line_display_text(tab.buffer(), *line);
        let change = insert_lines_change(tab, line + 1, std::slice::from_ref(&text))?;
        changes.push(change);
    }
    request_with_duplicate_line_selection(tab, changes, &lines)
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

pub(crate) fn duplicate_lines_request(tab: &EditorTab, pos: Position) -> Option<EditRequest> {
    if tab.selection_set().has_multiple() {
        if let Some(request) = duplicate_touched_lines_request(tab) {
            return Some(request);
        }
    }
    duplicate_selection_request(tab).or_else(|| duplicate_line_request(tab, pos))
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

pub(crate) fn selection_set_touched_lines(tab: &EditorTab) -> Vec<usize> {
    let mut lines = Vec::new();
    for selection in tab.selection_set().as_slice() {
        let range = selection.range();
        if range.start == range.end {
            lines.push(char_to_position(tab.buffer(), selection.cursor()).line);
            continue;
        }
        let start = char_to_position(tab.buffer(), range.start);
        let end = char_to_position(tab.buffer(), range.end);
        let last = if end.column == 0 && end.line > start.line {
            end.line - 1
        } else {
            end.line
        };
        lines.extend(start.line..=last.max(start.line));
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

fn line_clusters(lines: &[usize]) -> Vec<Range<usize>> {
    let mut clusters: Vec<Range<usize>> = Vec::new();
    for line in lines.iter().copied() {
        if let Some(last) = clusters.last_mut() {
            if line == last.end {
                last.end += 1;
                continue;
            }
        }
        clusters.push(line..line + 1);
    }
    clusters
}

fn request_with_mapped_selection(tab: &EditorTab, changes: Vec<TextChange>) -> Option<EditRequest> {
    request_with_selection_map(tab, changes, |changes, _after_buffer, offset| {
        changes.map_offset_to_inserted_end(offset)
    })
}

fn request_with_line_move_selection(
    tab: &EditorTab,
    changes: Vec<TextChange>,
    clusters: &[Range<usize>],
    up: bool,
) -> Option<EditRequest> {
    request_with_selection_map(tab, changes, |_changes, after_buffer, offset| {
        map_line_move_endpoint(tab, after_buffer, offset, clusters, up)
    })
}

fn request_with_duplicate_line_selection(
    tab: &EditorTab,
    changes: Vec<TextChange>,
    lines: &[usize],
) -> Option<EditRequest> {
    request_with_selection_map(tab, changes, |_changes, after_buffer, offset| {
        map_duplicate_line_endpoint(tab, after_buffer, offset, lines)
    })
}

fn request_with_selection_map<F>(
    tab: &EditorTab,
    changes: Vec<TextChange>,
    mut map_endpoint: F,
) -> Option<EditRequest>
where
    F: FnMut(&TextChangeSet, &ropey::Rope, usize) -> usize,
{
    if changes.is_empty() {
        return None;
    }
    let changes = TextChangeSet::try_new(changes, 0)?;
    let after_buffer = buffer_after_changes(tab, &changes);

    let selections_after = tab
        .selection_set()
        .as_slice()
        .iter()
        .map(|selection| {
            let anchor = map_endpoint(&changes, &after_buffer, selection.anchor());
            let head = map_endpoint(&changes, &after_buffer, selection.head());
            Selection::new(anchor, head)
        })
        .collect::<Vec<_>>();
    let selection_after = SelectionSet::from_selections_coalescing_cursors(
        selections_after,
        tab.selection_set().primary_index(),
    )
    .ok()?;
    Some(
        EditRequest::other_break(changes)
            .with_selection_after(SelectionAfter::Exact(selection_after)),
    )
}

fn buffer_after_changes(tab: &EditorTab, changes: &TextChangeSet) -> ropey::Rope {
    let mut after_buffer = tab.buffer().clone();
    for change in changes.as_slice().iter().rev() {
        apply_change_to_buffer(&mut after_buffer, change);
    }
    after_buffer
}

fn map_line_move_endpoint(
    tab: &EditorTab,
    after_buffer: &ropey::Rope,
    offset: usize,
    clusters: &[Range<usize>],
    up: bool,
) -> usize {
    let mut position = char_to_position(tab.buffer(), offset.min(tab.len_chars()));
    for cluster in clusters {
        if cluster.contains(&position.line) {
            position.line = if up {
                position.line.saturating_sub(1)
            } else {
                position.line + 1
            };
            return position_to_char(after_buffer, position);
        }
        if up {
            if position.line + 1 == cluster.start {
                position.line = cluster.end - 1;
                return position_to_char(after_buffer, position);
            }
        } else if position.line == cluster.end {
            position.line = cluster.start;
            return position_to_char(after_buffer, position);
        }
    }
    changes_unmapped_offset(tab, after_buffer, offset)
}

fn map_duplicate_line_endpoint(
    tab: &EditorTab,
    after_buffer: &ropey::Rope,
    offset: usize,
    lines: &[usize],
) -> usize {
    let mut position = char_to_position(tab.buffer(), offset.min(tab.len_chars()));
    let mut inserted_before = 0;
    for line in lines {
        if *line == position.line {
            position.line += inserted_before + 1;
            return position_to_char(after_buffer, position);
        }
        if *line < position.line {
            inserted_before += 1;
        }
    }
    position.line += inserted_before;
    position_to_char(after_buffer, position)
}

fn changes_unmapped_offset(tab: &EditorTab, after_buffer: &ropey::Rope, offset: usize) -> usize {
    let position = char_to_position(tab.buffer(), offset.min(tab.len_chars()));
    position_to_char(after_buffer, position)
}

fn map_outdented_endpoint(
    tab: &EditorTab,
    offset: usize,
    removed_by_line: &[(usize, usize)],
) -> usize {
    let buffer = tab.buffer();
    let len = tab.len_chars();
    let offset = offset.min(len);
    let position = char_to_position(buffer, offset);
    let removed_before: usize = removed_by_line
        .iter()
        .take_while(|(line, _)| *line < position.line)
        .map(|(_, removed)| *removed)
        .sum();
    let removed_on_line = removed_by_line
        .iter()
        .find(|(line, _)| *line == position.line)
        .map(|(_, removed)| *removed)
        .unwrap_or(0);
    let new_line_start = buffer
        .line_to_char(position.line)
        .saturating_sub(removed_before);
    let new_line_len = display_line_char_len(buffer, position.line).saturating_sub(removed_on_line);
    let new_column = position
        .column
        .saturating_sub(removed_on_line)
        .min(new_line_len);
    new_line_start + new_column
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

pub(crate) fn outdent_prefix_len(tab: &EditorTab, line: usize, unit: &str) -> usize {
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

fn line_span_without_prefix(
    buffer: &ropey::Rope,
    first: usize,
    last: usize,
) -> Option<(std::ops::Range<usize>, bool)> {
    let line_count = buffer.len_lines().max(1);
    if first > last || first >= line_count {
        return None;
    }
    let last = last.min(line_count - 1);
    let start = buffer.line_to_char(first);
    let end = if last + 1 < buffer.len_lines() {
        buffer.line_to_char(last + 1)
    } else {
        buffer.len_chars()
    };
    let trailing_newline = end > start && matches!(buffer.char(end - 1), '\n' | '\r');
    Some((start..end, trailing_newline))
}
