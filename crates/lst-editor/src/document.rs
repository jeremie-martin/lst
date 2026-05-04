use crate::{
    position::Position,
    selection::{self, line_display_text},
};
use ropey::Rope;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    Delete,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoBoundary {
    Merge,
    Break,
}

pub fn char_to_position(buffer: &Rope, char_offset: usize) -> Position {
    let char_offset = char_offset.min(buffer.len_chars());
    let line = buffer.char_to_line(char_offset);
    let line_start = buffer.line_to_char(line);
    Position {
        line,
        column: char_offset - line_start,
    }
}

pub fn position_to_char(buffer: &Rope, position: Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let line_len = buffer
        .line(line)
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .count();
    line_start + position.column.min(line_len)
}

pub(crate) fn position_range(buffer: &Rope, from: Position, to: Position) -> Option<Range<usize>> {
    if from.line >= buffer.len_lines() || to.line >= buffer.len_lines() {
        return None;
    }
    let (from, to) = if (to.line, to.column) < (from.line, from.column) {
        (to, from)
    } else {
        (from, to)
    };
    let start = position_start_char(buffer, from);
    let end = inclusive_position_to_exclusive_char(buffer, to);
    Some(start..end.max(start))
}

pub(crate) fn position_start_char(buffer: &Rope, position: Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let cells = selection::cells_of_str(&body);
    if cells.is_empty() {
        return line_start;
    }
    let cell = selection::cell_partition_by_char(&cells, position.column);
    line_start
        + cells
            .get(cell)
            .map_or(body.chars().count(), |cell| cell.char_start)
}

pub(crate) fn inclusive_position_to_exclusive_char(buffer: &Rope, position: Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let cells = selection::cells_of_str(&body);
    if cells.is_empty() {
        return line_start;
    }
    let end_cell = selection::cell_containing_char(&cells, position.column) + 1;
    line_start
        + cells
            .get(end_cell)
            .map_or_else(|| body.chars().count(), |cell| cell.char_start)
}

pub fn line_indent_prefix(buffer: &Rope, line_ix: usize) -> String {
    buffer
        .line(line_ix.min(buffer.len_lines().saturating_sub(1)))
        .chars()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .take_while(|ch| ch.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_conversion_clamps_to_line_width() {
        let buffer = Rope::from_str("abc\ndef");
        assert_eq!(
            position_to_char(
                &buffer,
                Position {
                    line: 0,
                    column: 99
                }
            ),
            3
        );
        assert_eq!(
            char_to_position(&buffer, 5),
            Position { line: 1, column: 1 }
        );
    }
}
