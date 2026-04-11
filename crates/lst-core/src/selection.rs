use ropey::Rope;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Whitespace,
    Word,
    Symbol,
}

fn token_class(ch: char) -> TokenClass {
    if ch.is_whitespace() {
        TokenClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        TokenClass::Word
    } else {
        TokenClass::Symbol
    }
}

pub fn previous_word_boundary(buffer: &Rope, char_index: usize) -> usize {
    let chars: Vec<char> = buffer.chars().collect();
    let mut index = char_index.min(chars.len());
    while index > 0 && token_class(chars[index - 1]) == TokenClass::Whitespace {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    let class = token_class(chars[index - 1]);
    while index > 0 && token_class(chars[index - 1]) == class {
        index -= 1;
    }
    index
}

pub fn next_word_boundary(buffer: &Rope, char_index: usize) -> usize {
    let chars: Vec<char> = buffer.chars().collect();
    let mut index = char_index.min(chars.len());
    while index < chars.len() && token_class(chars[index]) == TokenClass::Whitespace {
        index += 1;
    }
    if index == chars.len() {
        return chars.len();
    }

    let class = token_class(chars[index]);
    while index < chars.len() && token_class(chars[index]) == class {
        index += 1;
    }
    index
}

pub fn word_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let line = buffer.char_to_line(clamped);
    let line_start = buffer.line_to_char(line);
    let display_text = line_display_text(buffer, line);
    let chars: Vec<char> = display_text.chars().collect();
    if chars.is_empty() {
        return clamped..clamped;
    }

    let local = clamped
        .saturating_sub(line_start)
        .min(chars.len().saturating_sub(1));
    let class = token_class(chars[local]);
    let mut start = local;
    while start > 0 && token_class(chars[start - 1]) == class {
        start -= 1;
    }
    let mut end = local + 1;
    while end < chars.len() && token_class(chars[end]) == class {
        end += 1;
    }
    (line_start + start)..(line_start + end)
}

pub fn line_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let line = buffer.char_to_line(clamped);
    let start = buffer.line_to_char(line);
    let end = if line + 1 < buffer.len_lines() {
        buffer.line_to_char(line + 1)
    } else {
        buffer.len_chars()
    };
    start..end
}

pub fn previous_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = chars.partition_point(|(byte, _)| *byte < offset.min(text.len()));
    while index > 0 && token_class(chars[index - 1].1) == TokenClass::Whitespace {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    let class = token_class(chars[index - 1].1);
    while index > 0 && token_class(chars[index - 1].1) == class {
        index -= 1;
    }
    chars.get(index).map_or(0, |(byte, _)| *byte)
}

pub fn next_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = chars.partition_point(|(byte, _)| *byte < offset.min(text.len()));
    while index < chars.len() && token_class(chars[index].1) == TokenClass::Whitespace {
        index += 1;
    }
    if index == chars.len() {
        return text.len();
    }

    let class = token_class(chars[index].1);
    while index < chars.len() && token_class(chars[index].1) == class {
        index += 1;
    }
    chars.get(index).map_or(text.len(), |(byte, _)| *byte)
}

pub fn word_range_in_text(text: &str, offset: usize) -> Range<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let Some(local) = char_index_containing_offset(text, offset) else {
        return 0..0;
    };

    let class = token_class(chars[local].1);
    let mut start = local;
    while start > 0 && token_class(chars[start - 1].1) == class {
        start -= 1;
    }
    let mut end = local + 1;
    while end < chars.len() && token_class(chars[end].1) == class {
        end += 1;
    }

    let start_byte = chars[start].0;
    let end_byte = chars.get(end).map_or(text.len(), |(byte, _)| *byte);
    start_byte..end_byte
}

pub fn drag_selection_range(anchor: Range<usize>, current: Range<usize>) -> (Range<usize>, bool) {
    if current.start < anchor.start {
        (current.start..anchor.end.max(current.end), true)
    } else {
        (
            anchor.start.min(current.start)..current.end.max(anchor.end),
            false,
        )
    }
}

fn char_index_containing_offset(text: &str, offset: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return None;
    }

    let offset = offset.min(text.len());
    if offset == text.len() {
        return Some(chars.len() - 1);
    }

    chars.iter().enumerate().find_map(|(index, (start, _))| {
        let end = chars.get(index + 1).map_or(text.len(), |(byte, _)| *byte);
        (offset >= *start && offset < end).then_some(index)
    })
}

fn line_display_text(buffer: &Rope, line_ix: usize) -> String {
    let mut line = buffer
        .line(line_ix.min(buffer.len_lines().saturating_sub(1)))
        .to_string();
    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_word_ranges_group_words_symbols_and_whitespace() {
        let buffer = Rope::from_str("alpha beta::gamma");

        assert_eq!(word_range_at_char(&buffer, 7), 6..10);
        assert_eq!(word_range_at_char(&buffer, 10), 10..12);
        assert_eq!(word_range_at_char(&buffer, 5), 5..6);
    }

    #[test]
    fn rope_word_boundaries_skip_whitespace() {
        let buffer = Rope::from_str("alpha beta.gamma");

        assert_eq!(next_word_boundary(&buffer, 0), 5);
        assert_eq!(next_word_boundary(&buffer, 5), 10);
        assert_eq!(previous_word_boundary(&buffer, 11), 10);
        assert_eq!(previous_word_boundary(&buffer, 10), 6);
    }

    #[test]
    fn line_range_includes_trailing_newline_when_present() {
        let buffer = Rope::from_str("one\ntwo\nthree");

        assert_eq!(line_range_at_char(&buffer, 1), 0..4);
        assert_eq!(line_range_at_char(&buffer, 5), 4..8);
        assert_eq!(line_range_at_char(&buffer, 10), 8..13);
    }

    #[test]
    fn text_word_ranges_group_words_symbols_and_whitespace() {
        let text = "alpha beta::gamma";

        assert_eq!(word_range_in_text(text, 7), 6..10);
        assert_eq!(word_range_in_text(text, 10), 10..12);
        assert_eq!(word_range_in_text(text, 5), 5..6);
    }

    #[test]
    fn text_word_boundaries_are_utf8_safe() {
        let text = "one γamma two";

        assert_eq!(next_word_boundary_in_text(text, 0), 3);
        assert_eq!(next_word_boundary_in_text(text, 3), "one γamma".len());
        assert_eq!(
            previous_word_boundary_in_text(text, "one γamma".len()),
            "one ".len()
        );
    }

    #[test]
    fn drag_selection_extends_from_anchor_token() {
        let (selection, reversed) = drag_selection_range(6..10, 13..18);

        assert_eq!(selection, 6..18);
        assert!(!reversed);

        let (selection, reversed) = drag_selection_range(6..10, 0..5);
        assert_eq!(selection, 0..10);
        assert!(reversed);
    }
}
