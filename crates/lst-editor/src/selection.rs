use ropey::Rope;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Whitespace,
    Word,
    Symbol,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SubwordClass {
    Lower,
    Upper,
    Alpha,
    Digit,
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

pub(crate) fn is_identifier_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_symbol_char(ch: char) -> bool {
    !ch.is_whitespace() && !is_identifier_char(ch)
}

fn subword_class(ch: char) -> Option<SubwordClass> {
    if ch == '_' || !is_identifier_char(ch) {
        None
    } else if ch.is_numeric() {
        Some(SubwordClass::Digit)
    } else if ch.is_uppercase() {
        Some(SubwordClass::Upper)
    } else if ch.is_lowercase() {
        Some(SubwordClass::Lower)
    } else {
        Some(SubwordClass::Alpha)
    }
}

fn identifier_run_start(chars: &[char], index: usize) -> usize {
    let mut start = index.min(chars.len());
    while start > 0 && is_identifier_char(chars[start - 1]) {
        start -= 1;
    }
    start
}

fn identifier_run_end(chars: &[char], index: usize) -> usize {
    let mut end = index.min(chars.len());
    while end < chars.len() && is_identifier_char(chars[end]) {
        end += 1;
    }
    end
}

fn subword_chunk_end(chars: &[char], start: usize, run_end: usize) -> usize {
    let Some(class) = subword_class(chars[start]) else {
        return (start + 1).min(run_end);
    };

    match class {
        SubwordClass::Digit => {
            let mut end = start + 1;
            while end < run_end && subword_class(chars[end]) == Some(SubwordClass::Digit) {
                end += 1;
            }
            end
        }
        SubwordClass::Lower | SubwordClass::Alpha => {
            let mut end = start + 1;
            while end < run_end {
                match subword_class(chars[end]) {
                    Some(SubwordClass::Lower | SubwordClass::Alpha) => end += 1,
                    _ => break,
                }
            }
            end
        }
        SubwordClass::Upper => {
            if start + 1 < run_end {
                if let Some(SubwordClass::Lower | SubwordClass::Alpha) =
                    subword_class(chars[start + 1])
                {
                    let mut end = start + 2;
                    while end < run_end {
                        match subword_class(chars[end]) {
                            Some(SubwordClass::Lower | SubwordClass::Alpha) => end += 1,
                            _ => break,
                        }
                    }
                    return end;
                }
            }

            let mut end = start + 1;
            while end < run_end && subword_class(chars[end]) == Some(SubwordClass::Upper) {
                if end + 1 < run_end {
                    if let Some(SubwordClass::Lower | SubwordClass::Alpha) =
                        subword_class(chars[end + 1])
                    {
                        break;
                    }
                }
                end += 1;
            }
            end
        }
    }
}

fn subword_chunks(chars: &[char]) -> Vec<Range<usize>> {
    let mut chunks = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        while index < chars.len() && chars[index] == '_' {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }
        let end = subword_chunk_end(chars, index, chars.len());
        chunks.push(index..end);
        index = end;
    }
    chunks
}

fn previous_subword_boundary_chars(chars: &[char], char_index: usize) -> usize {
    let mut index = char_index.min(chars.len());
    while index > 0 && chars[index - 1].is_whitespace() {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    if is_symbol_char(chars[index - 1]) {
        while index > 0 && is_symbol_char(chars[index - 1]) {
            index -= 1;
        }
        return index;
    }

    let run_start = identifier_run_start(chars, index - 1);
    let run_end = identifier_run_end(chars, index - 1);
    let chunks = subword_chunks(&chars[run_start..run_end]);
    let relative = index - run_start;
    chunks
        .iter()
        .rfind(|chunk| chunk.start < relative)
        .map_or(run_start, |chunk| run_start + chunk.start)
}

fn next_subword_boundary_chars(chars: &[char], char_index: usize) -> usize {
    let mut index = char_index.min(chars.len());
    while index < chars.len() && chars[index].is_whitespace() {
        index += 1;
    }
    if index == chars.len() {
        return chars.len();
    }

    if is_symbol_char(chars[index]) {
        while index < chars.len() && is_symbol_char(chars[index]) {
            index += 1;
        }
        return index;
    }

    while index < chars.len() && chars[index] == '_' {
        index += 1;
    }
    if index == chars.len() {
        return chars.len();
    }

    let run_start = identifier_run_start(chars, index);
    let run_end = identifier_run_end(chars, index);
    let chunks = subword_chunks(&chars[run_start..run_end]);
    let relative = index - run_start;
    chunks
        .iter()
        .find(|chunk| chunk.start <= relative && relative < chunk.end)
        .map_or(run_end, |chunk| run_start + chunk.end)
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

pub fn previous_subword_boundary(buffer: &Rope, char_index: usize) -> usize {
    let chars: Vec<char> = buffer.chars().collect();
    previous_subword_boundary_chars(&chars, char_index)
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

pub fn next_subword_boundary(buffer: &Rope, char_index: usize) -> usize {
    let chars: Vec<char> = buffer.chars().collect();
    next_subword_boundary_chars(&chars, char_index)
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

pub fn next_grapheme_column(line: &str, column: usize) -> usize {
    let total = line.chars().count();
    if column >= total {
        return total;
    }
    let local_byte = byte_of_char_index(line, column);
    let next_byte = line
        .grapheme_indices(true)
        .find_map(|(b, _)| (b > local_byte).then_some(b))
        .unwrap_or(line.len());
    line[..next_byte].chars().count()
}

pub fn previous_grapheme_column(line: &str, column: usize) -> usize {
    if column == 0 {
        return 0;
    }
    let total = line.chars().count();
    let column = column.min(total);
    let local_byte = if column == total {
        line.len()
    } else {
        byte_of_char_index(line, column)
    };
    let prev_byte = line
        .grapheme_indices(true)
        .rev()
        .find_map(|(b, _)| (b < local_byte).then_some(b))
        .unwrap_or(0);
    line[..prev_byte].chars().count()
}

pub fn last_grapheme_column(line: &str) -> usize {
    line.grapheme_indices(true)
        .next_back()
        .map_or(0, |(b, _)| line[..b].chars().count())
}

pub fn next_grapheme_boundary(buffer: &Rope, char_index: usize) -> usize {
    let total = buffer.len_chars();
    let ci = char_index.min(total);
    if ci == total {
        return total;
    }
    let line = buffer.char_to_line(ci);
    let line_start = buffer.line_to_char(line);
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    if local_ci >= body.chars().count() {
        return (ci + 1).min(total);
    }
    line_start + next_grapheme_column(&body, local_ci)
}

pub fn previous_grapheme_boundary(buffer: &Rope, char_index: usize) -> usize {
    let total = buffer.len_chars();
    let ci = char_index.min(total);
    if ci == 0 {
        return 0;
    }
    let line = buffer.char_to_line(ci);
    let line_start = buffer.line_to_char(line);
    if ci == line_start {
        return ci - 1;
    }
    let body = line_display_text(buffer, line);
    let local_ci = ci - line_start;
    if local_ci > body.chars().count() {
        return ci - 1;
    }
    line_start + previous_grapheme_column(&body, local_ci)
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

pub fn previous_subword_boundary_in_text(text: &str, offset: usize) -> usize {
    let (byte_offsets, chars): (Vec<usize>, Vec<char>) = text.char_indices().unzip();
    let char_index = byte_offsets.partition_point(|byte| *byte < offset.min(text.len()));
    let target = previous_subword_boundary_chars(&chars, char_index);
    byte_offsets.get(target).copied().unwrap_or(text.len())
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

pub fn next_subword_boundary_in_text(text: &str, offset: usize) -> usize {
    let (byte_offsets, chars): (Vec<usize>, Vec<char>) = text.char_indices().unzip();
    let char_index = byte_offsets.partition_point(|byte| *byte < offset.min(text.len()));
    let target = next_subword_boundary_chars(&chars, char_index);
    byte_offsets.get(target).copied().unwrap_or(text.len())
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

fn byte_of_char_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
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
    fn rope_subword_boundaries_split_camel_snake_and_digits() {
        let buffer = Rope::from_str("camelCase snake_case HTTPServer version2Alpha");

        assert_eq!(next_subword_boundary(&buffer, 0), 5);
        assert_eq!(next_subword_boundary(&buffer, 5), 9);
        assert_eq!(next_subword_boundary(&buffer, 10), 15);
        assert_eq!(next_subword_boundary(&buffer, 15), 20);
        assert_eq!(next_subword_boundary(&buffer, 21), 25);
        assert_eq!(next_subword_boundary(&buffer, 25), 31);
        assert_eq!(next_subword_boundary(&buffer, 32), 39);
        assert_eq!(next_subword_boundary(&buffer, 39), 40);
        assert_eq!(next_subword_boundary(&buffer, 40), 45);

        assert_eq!(previous_subword_boundary(&buffer, 9), 5);
        assert_eq!(previous_subword_boundary(&buffer, 5), 0);
        assert_eq!(previous_subword_boundary(&buffer, 20), 16);
        assert_eq!(previous_subword_boundary(&buffer, 16), 10);
        assert_eq!(previous_subword_boundary(&buffer, 15), 10);
        assert_eq!(previous_subword_boundary(&buffer, 31), 25);
        assert_eq!(previous_subword_boundary(&buffer, 25), 21);
        assert_eq!(previous_subword_boundary(&buffer, 45), 40);
        assert_eq!(previous_subword_boundary(&buffer, 40), 39);
        assert_eq!(previous_subword_boundary(&buffer, 39), 32);
    }

    #[test]
    fn subword_boundaries_handle_single_char_snake_segments() {
        let buffer = Rope::from_str("a_b_c");

        assert_eq!(next_subword_boundary(&buffer, 0), 1);
        assert_eq!(next_subword_boundary(&buffer, 1), 3);
        assert_eq!(next_subword_boundary(&buffer, 3), 5);

        assert_eq!(previous_subword_boundary(&buffer, 5), 4);
        assert_eq!(previous_subword_boundary(&buffer, 4), 2);
        assert_eq!(previous_subword_boundary(&buffer, 2), 0);
    }

    #[test]
    fn subword_boundaries_keep_symbol_runs_as_stops() {
        let buffer = Rope::from_str("foo.barBaz alpha::Beta");

        assert_eq!(next_subword_boundary(&buffer, 0), 3);
        assert_eq!(next_subword_boundary(&buffer, 3), 4);
        assert_eq!(next_subword_boundary(&buffer, 4), 7);
        assert_eq!(next_subword_boundary(&buffer, 7), 10);
        assert_eq!(next_subword_boundary(&buffer, 11), 16);
        assert_eq!(next_subword_boundary(&buffer, 16), 18);
        assert_eq!(next_subword_boundary(&buffer, 18), 22);

        assert_eq!(previous_subword_boundary(&buffer, 10), 7);
        assert_eq!(previous_subword_boundary(&buffer, 7), 4);
        assert_eq!(previous_subword_boundary(&buffer, 4), 3);
        assert_eq!(previous_subword_boundary(&buffer, 22), 18);
        assert_eq!(previous_subword_boundary(&buffer, 18), 16);
    }

    #[test]
    fn text_subword_boundaries_are_utf8_safe() {
        let text = "one ΓammaΔelta HTTPServer42";

        assert_eq!(next_subword_boundary_in_text(text, 0), "one".len());
        assert_eq!(
            next_subword_boundary_in_text(text, "one ".len() + 1),
            "one Γamma".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one ".len()),
            "one Γamma".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one Γamma".len()),
            "one ΓammaΔelta".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one ΓammaΔelta ".len()),
            "one ΓammaΔelta HTTP".len()
        );
        assert_eq!(
            next_subword_boundary_in_text(text, "one ΓammaΔelta HTTP".len()),
            "one ΓammaΔelta HTTPServer".len()
        );
        assert_eq!(
            previous_subword_boundary_in_text(text, "one ΓammaΔelta HTTPServer".len()),
            "one ΓammaΔelta HTTP".len()
        );
        assert_eq!(
            previous_subword_boundary_in_text(text, "one Γa".len()),
            "one ".len()
        );
        assert_eq!(
            previous_subword_boundary_in_text(text, "one Γamma".len()),
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
