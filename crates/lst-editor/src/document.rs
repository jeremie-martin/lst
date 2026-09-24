use crate::selection::{self, line_display_text, Position};
use ropey::Rope;

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

pub(crate) fn position_start_char(buffer: &Rope, position: Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let cells = selection::cells_of_str(&body);
    if cells.is_empty() {
        return line_start;
    }
    let cell = selection::cell_partition_by_char(&cells, position.column);
    line_start + cells.get(cell).map_or(body.chars().count(), |cell| cell.char_start)
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

/// A covering line window before and after an edit. The unchanged suffix
/// determines the old window from the new window and both document sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineChange {
    before: std::ops::Range<usize>,
    after: std::ops::Range<usize>,
}

impl LineChange {
    pub fn new(before_count: usize, after_count: usize, after: std::ops::Range<usize>) -> Option<Self> {
        if after.start > after.end || after.end > after_count {
            return None;
        }
        let before_end = if before_count >= after_count {
            after.end.checked_add(before_count - after_count)?
        } else {
            after.end.checked_sub(after_count - before_count)?
        };
        if after.start > before_end || before_end > before_count {
            return None;
        }
        Some(Self {
            before: after.start..before_end,
            after,
        })
    }

    pub fn before(&self) -> std::ops::Range<usize> {
        self.before.clone()
    }
    pub fn after(&self) -> std::ops::Range<usize> {
        self.after.clone()
    }
}

/// Visit logical lines with Ropey's exact line boundaries and terminators.
/// Lines within a rope chunk stay borrowed; only cross-chunk lines use a
/// reusable buffer. Includes the final empty line after a trailing break.
/// This avoids computing RopeSlice character metadata when a caller only
/// needs each line's text.
pub fn for_each_rope_line(buffer: &Rope, visit: impl FnMut(usize, &str)) {
    for_each_chunk_line(buffer.chunks(), visit);
}

/// Visit a validated logical-line window without measuring RopeSlice metadata
/// for every line. Callback indexes remain relative to the whole document.
pub fn for_each_rope_line_in(buffer: &Rope, lines: std::ops::Range<usize>, mut visit: impl FnMut(usize, &str)) {
    let slice = buffer.slice(buffer.line_to_char(lines.start)..buffer.line_to_char(lines.end));
    for_each_chunk_line(slice.chunks(), |index, line| {
        if index < lines.len() {
            visit(lines.start + index, line);
        }
    });
}

fn for_each_chunk_line<'a>(chunks: impl Iterator<Item = &'a str>, mut visit: impl FnMut(usize, &str)) {
    let mut index = 0;
    let mut visit_line = |line: &str| {
        visit(index, line);
        index += 1;
    };
    // Ropey chunks end at character boundaries and never split CRLF pairs.
    let mut partial_line = String::new();
    for chunk in chunks {
        // Prove once per chunk that LF is its only possible line separator.
        // Keep Ropey's parser for CRLF, other ASCII breaks, and Unicode.
        let lf_only = chunk.is_ascii() && memchr::memchr3(b'\r', b'\x0b', b'\x0c', chunk.as_bytes()).is_none();
        let ends_with_break = chunk.char_indices().next_back().is_some_and(|(start, _)| {
            let tail = &chunk[start..];
            ropey::str_utils::byte_to_line_idx(tail, tail.len()) != 0
        });
        let mut remaining = chunk;
        while !remaining.is_empty() {
            let end = if lf_only {
                memchr::memchr(b'\n', remaining.as_bytes()).map_or(remaining.len(), |newline| newline + 1)
            } else {
                ropey::str_utils::line_to_byte_idx(remaining, 1)
            };
            let complete = end < remaining.len() || ends_with_break;
            let (line, rest) = remaining.split_at(end);
            if complete && partial_line.is_empty() {
                visit_line(line);
            } else {
                partial_line.push_str(line);
                if complete {
                    visit_line(&partial_line);
                    partial_line.clear();
                }
            }
            remaining = rest;
        }
    }
    // A trailing line break still leaves one final empty logical line.
    visit_line(&partial_line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_change_windows_preserve_prefix_and_suffix_lengths() {
        for before_count in 0..12 {
            for after_count in 0..12 {
                for start in 0..14 {
                    for end in 0..14 {
                        let valid = start <= end
                            && end <= after_count
                            && after_count - end <= before_count
                            && start <= before_count - (after_count - end);
                        let change = LineChange::new(before_count, after_count, start..end);
                        assert_eq!(change.is_some(), valid);
                        if let Some(change) = change {
                            assert_eq!(change.before().start, change.after().start);
                            assert_eq!(before_count - change.before().end, after_count - change.after().end);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn borrowed_lines_match_rope_lines_after_fragmented_edits() {
        let fragments = [
            "",
            "a",
            "\n",
            "\r",
            "\r\n",
            "\u{b}",
            "\u{c}",
            "\u{85}",
            "\u{2028}",
            "\u{2029}",
            "界e\u{301}\t",
        ];
        let check = |buffer: &Rope| {
            let expected: Vec<_> = buffer.lines().map(|line| line.to_string()).collect();
            let mut actual = Vec::new();
            for_each_rope_line(buffer, |index, line| {
                assert_eq!(index, actual.len());
                actual.push(line.to_owned());
            });
            assert_eq!(actual, expected);
            let count = buffer.len_lines();
            for lines in [0..0, 0..count, count / 3..count * 2 / 3, count - 1..count, count..count] {
                let mut window = Vec::new();
                for_each_rope_line_in(buffer, lines.clone(), |index, text| {
                    window.push((index, text.to_owned()))
                });
                let expected_window = lines.map(|index| (index, expected[index].clone())).collect::<Vec<_>>();
                assert_eq!(window, expected_window);
            }
        };
        for text in fragments {
            check(&Rope::from_str(text));
        }
        for text in [
            "alpha\nbeta\n".repeat(4096),
            "a".repeat(4096),
            format!("{}界\r\n{}\u{2028}tail", "ascii\n".repeat(512), "more\n".repeat(512)),
        ] {
            check(&Rope::from_str(&text));
        }
        let mut buffer = Rope::from_str(&"alpha界\r\nbeta\u{2028}gamma\n".repeat(128));
        for step in 0..1024 {
            let at = step * 97 % (buffer.len_chars() + 1);
            if step % 3 == 0 && at < buffer.len_chars() {
                buffer.remove(at..(at + step % 23 + 1).min(buffer.len_chars()));
            } else {
                buffer.insert(at, fragments[step % fragments.len()]);
            }
            check(&buffer);
        }
        check(&Rope::from_str(&"界e\u{301}".repeat(4096)));
    }
}
