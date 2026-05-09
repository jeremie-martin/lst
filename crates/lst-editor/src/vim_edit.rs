use crate::{
    document::{
        inclusive_position_to_exclusive_char, line_indent_prefix, position_range,
        position_start_char, position_to_char,
    },
    line_edit::{clamped_line_span, insert_lines_change, replace_lines_change},
    position::Position,
    selection::{self, line_display_text},
    tab::EditorTab,
    text_input,
    transaction::{EditRequest, SelectionAfter, TextChange, TextChangeSet},
    vim,
};

pub(crate) enum VimEditAction {
    MoveCursor(Position),
    Edit(EditRequest),
}

pub(crate) type DeletedEdit = (String, EditRequest);

pub(crate) fn extract_range(tab: &EditorTab, from: Position, to: Position) -> String {
    position_range(tab.buffer(), from, to)
        .map(|range| tab.buffer().slice(range).to_string())
        .unwrap_or_default()
}

pub(crate) fn extract_lines(tab: &EditorTab, first: usize, last: usize) -> String {
    let (first, last) = clamped_line_span(tab, first, last);
    (first..=last)
        .map(|line| line_display_text(tab.buffer(), line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn delete_range(tab: &EditorTab, from: Position, to: Position) -> Option<DeletedEdit> {
    let range = position_range(tab.buffer(), from, to)?;
    let deleted = extract_range(tab, from, to);
    let change = TextChange::delete(range);
    Some((
        deleted,
        EditRequest::other_break(TextChangeSet::single(change))
            .with_selection_after(SelectionAfter::CursorPositionBeforeLineEnd(from)),
    ))
}

pub(crate) fn delete_lines(tab: &EditorTab, first: usize, last: usize) -> Option<DeletedEdit> {
    let (first, last) = clamped_line_span(tab, first, last);
    let deleted = extract_lines(tab, first, last);
    let change = replace_lines_change(tab, first, last, &[])?;
    Some((
        deleted,
        EditRequest::single_other_at_position(change, Position::new(first, 0)),
    ))
}

pub(crate) fn change_lines(tab: &EditorTab, first: usize, last: usize) -> Option<DeletedEdit> {
    let (first, last) = clamped_line_span(tab, first, last);
    let indent = line_indent_prefix(tab.buffer(), first);
    let deleted = extract_lines(tab, first, last);
    let change = replace_lines_change(tab, first, last, std::slice::from_ref(&indent))?;
    Some((
        deleted,
        EditRequest::single_other_at_position(change, Position::new(first, indent.chars().count())),
    ))
}

pub(crate) fn surround_range(
    tab: &EditorTab,
    from: Position,
    to: Position,
    open: char,
    close: char,
) -> Option<EditRequest> {
    if from.line >= tab.line_count() || to.line >= tab.line_count() {
        return None;
    }
    let open_char = position_start_char(tab.buffer(), from);
    let close_char = inclusive_position_to_exclusive_char(tab.buffer(), to);
    let changes = vec![
        TextChange::insert(open_char, open),
        TextChange::insert(close_char, close),
    ];
    Some(EditRequest::other_at_position(changes, from))
}

pub(crate) fn delete_surround(
    tab: &EditorTab,
    open_pos: Position,
    close_pos: Position,
) -> Option<EditRequest> {
    let open_range = position_range(tab.buffer(), open_pos, open_pos)?;
    let close_range = position_range(tab.buffer(), close_pos, close_pos)?;
    let changes = vec![
        TextChange::delete(open_range),
        TextChange::delete(close_range),
    ];
    Some(EditRequest::other_at_position(changes, open_pos))
}

pub(crate) fn change_surround(
    tab: &EditorTab,
    open_pos: Position,
    close_pos: Position,
    to_open: char,
    to_close: char,
) -> Option<EditRequest> {
    let open_range = position_range(tab.buffer(), open_pos, open_pos)?;
    let close_range = position_range(tab.buffer(), close_pos, close_pos)?;
    let changes = vec![
        TextChange::replace(open_range, to_open),
        TextChange::replace(close_range, to_close),
    ];
    Some(EditRequest::other_at_position(changes, open_pos))
}

pub(crate) fn paste(
    tab: &EditorTab,
    cursor: Position,
    register: &vim::Register,
    before: bool,
) -> Option<EditRequest> {
    match register {
        vim::Register::Empty => None,
        vim::Register::Char(paste_text) => paste_chars(tab, cursor, paste_text, before),
        vim::Register::Line(paste_text) => paste_lines(tab, cursor, paste_text, before),
    }
}

pub(crate) fn open_line(tab: &EditorTab, pos: Position, above: bool) -> Option<EditRequest> {
    let indent = line_indent_prefix(tab.buffer(), pos.line);
    let idx = if above { pos.line } else { pos.line + 1 };
    let change = insert_lines_change(tab, idx, std::slice::from_ref(&indent))?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(idx, indent.chars().count()),
    ))
}

pub(crate) fn join_lines(tab: &EditorTab, pos: Position, count: usize) -> Option<EditRequest> {
    if pos.line + 1 >= tab.line_count() {
        return None;
    }

    let join_end = (pos.line + count).min(tab.line_count() - 1);
    let mut joined = line_display_text(tab.buffer(), pos.line)
        .trim_end()
        .to_string();
    let join_col = joined.chars().count();
    for line in (pos.line + 1)..=join_end {
        let line = line_display_text(tab.buffer(), line);
        let trimmed = line.trim_start();
        if !trimmed.is_empty() {
            joined.push(' ');
            joined.push_str(trimmed);
        }
    }
    let change = replace_lines_change(tab, pos.line, join_end, std::slice::from_ref(&joined))?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(pos.line, join_col),
    ))
}

pub(crate) fn replace_char(
    tab: &EditorTab,
    pos: Position,
    ch: char,
    count: usize,
) -> Option<EditRequest> {
    let line = line_display_text(tab.buffer(), pos.line);
    let cells = selection::cells_of_str(&line);
    if cells.is_empty() {
        return None;
    }
    let start_cell = selection::cell_partition_by_char(&cells, pos.column);
    let end_cell = start_cell.checked_add(count)?;
    if end_cell > cells.len() {
        return None;
    }
    let line_start = tab.buffer().line_to_char(pos.line);
    let start = line_start + cells[start_cell].char_start;
    let end = line_start
        + cells
            .get(end_cell)
            .map_or_else(|| line.chars().count(), |cell| cell.char_start);
    let replacement: String = std::iter::repeat_n(ch, count).collect();
    let cursor_col = cells[start_cell].char_start + count - 1;
    let change = TextChange::replace(start..end, replacement);
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(pos.line, cursor_col),
    ))
}

pub(crate) fn transform_case_range(
    tab: &EditorTab,
    from: Position,
    to: Position,
    uppercase: bool,
) -> Option<VimEditAction> {
    let range = position_range(tab.buffer(), from, to)?;
    let text = tab.buffer().slice(range.clone()).to_string();
    let replacement = transform_case_text(&text, uppercase);
    if text == replacement {
        return Some(VimEditAction::MoveCursor(from));
    }
    Some(VimEditAction::Edit(EditRequest::single_other_at_position(
        TextChange::replace(range, replacement),
        from,
    )))
}

pub(crate) fn transform_case_lines(
    tab: &EditorTab,
    first: usize,
    last: usize,
    uppercase: bool,
) -> VimEditAction {
    let (first, last) = clamped_line_span(tab, first, last);
    let mut changes = Vec::new();
    for line in first..=last {
        let start = tab.buffer().line_to_char(line);
        let len = selection::display_line_char_len(tab.buffer(), line);
        let range = start..start + len;
        let text = tab.buffer().slice(range.clone()).to_string();
        let replacement = transform_case_text(&text, uppercase);
        if text != replacement {
            changes.push(TextChange::replace(range, replacement));
        }
    }
    if changes.is_empty() {
        return VimEditAction::MoveCursor(Position::new(first, 0));
    }
    VimEditAction::Edit(EditRequest::other_at_position(
        changes,
        Position::new(first, 0),
    ))
}

fn paste_chars(
    tab: &EditorTab,
    cursor: Position,
    paste_text: &str,
    before: bool,
) -> Option<EditRequest> {
    let line_len = selection::display_line_char_len(tab.buffer(), cursor.line);
    let insert_col = if before {
        cursor.column.min(line_len)
    } else {
        (cursor.column + 1).min(line_len)
    };
    let insert_at = position_to_char(tab.buffer(), Position::new(cursor.line, insert_col));
    let paste_lines: Vec<&str> = paste_text.split('\n').collect();
    let replacement = paste_lines.join(text_input::preferred_newline(tab));
    let cursor_position = if paste_lines.len() == 1 {
        Position::new(
            cursor.line,
            insert_col + paste_lines[0].chars().count().saturating_sub(1),
        )
    } else {
        Position::new(
            cursor.line + paste_lines.len() - 1,
            paste_lines
                .last()
                .unwrap_or(&"")
                .chars()
                .count()
                .saturating_sub(1),
        )
    };
    let change = TextChange::insert(insert_at, replacement);
    Some(EditRequest::single_other_at_position(
        change,
        cursor_position,
    ))
}

fn paste_lines(
    tab: &EditorTab,
    cursor: Position,
    paste_text: &str,
    before: bool,
) -> Option<EditRequest> {
    let insert_at = if before { cursor.line } else { cursor.line + 1 };
    let inserted_lines: Vec<String> = paste_text.split('\n').map(String::from).collect();
    let indent = inserted_lines.first().map_or(0, |line| {
        line.chars().take_while(|c| c.is_whitespace()).count()
    });
    let change = insert_lines_change(tab, insert_at, &inserted_lines)?;
    Some(EditRequest::single_other_at_position(
        change,
        Position::new(insert_at, indent),
    ))
}

fn transform_case_text(text: &str, uppercase: bool) -> String {
    if uppercase {
        text.to_uppercase()
    } else {
        text.to_lowercase()
    }
}
