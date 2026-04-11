use crate::position::Position;
use ropey::Rope;

#[derive(Clone, Copy, PartialEq, Eq)]
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
