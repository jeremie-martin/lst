use ropey::Rope;
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenClass {
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

// Vim's "big word" (`W`/`B`/`E`) collapses Symbol into Word — only whitespace
// breaks a big-word run. With `big = false` this matches `token_class`.
pub(crate) fn vim_token_class(ch: char, big: bool) -> TokenClass {
    if big && !ch.is_whitespace() {
        TokenClass::Word
    } else {
        token_class(ch)
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

// A single extended grapheme cluster within its source text.
//
// Boundary helpers walk by `GraphemeCell` instead of by `char`, so cursor
// positions and selection endpoints always land on cluster boundaries.
// Combining marks and ZWJ joiners ride along with their base scalar; each
// cluster is classified by `repr` (the first scalar) — base wins, matching
// Helix and Zed.
#[derive(Clone, Copy)]
pub(crate) struct GraphemeCell {
    pub(crate) byte_start: usize,
    pub(crate) char_start: usize,
    pub(crate) char_len: u8,
    pub(crate) repr: char,
}

pub(crate) fn cells_of_str(text: &str) -> Vec<GraphemeCell> {
    let mut cells = Vec::new();
    let mut char_start = 0usize;
    for (byte_start, cluster) in text.grapheme_indices(true) {
        let mut chars = cluster.chars();
        let Some(repr) = chars.next() else {
            continue;
        };
        let char_len = 1 + chars.count();
        debug_assert!(char_len <= u8::MAX as usize);
        cells.push(GraphemeCell {
            byte_start,
            char_start,
            char_len: char_len as u8,
            repr,
        });
        char_start += char_len;
    }
    cells
}

fn cells_of_rope(buffer: &Rope) -> Vec<GraphemeCell> {
    cells_of_str(&buffer.to_string())
}

fn cells_of_rope_line(buffer: &Rope, line: usize) -> (usize, Vec<GraphemeCell>) {
    let line_ix = line.min(buffer.len_lines().saturating_sub(1));
    let line_start_char = buffer.line_to_char(line_ix);
    let body = line_display_text(buffer, line_ix);
    (line_start_char, cells_of_str(&body))
}

// First cell whose `char_start >= char_index`. Mid-cluster offsets round up to
// the next cluster boundary. Returns `cells.len()` if `char_index` is past end.
pub(crate) fn cell_partition_by_char(cells: &[GraphemeCell], char_index: usize) -> usize {
    cells.partition_point(|cell| cell.char_start < char_index)
}

fn cell_partition_by_byte(cells: &[GraphemeCell], byte_offset: usize) -> usize {
    cells.partition_point(|cell| cell.byte_start < byte_offset)
}

// Index of the cell containing `char_index`. Mid-cluster offsets land on the
// containing cluster; offsets past end clamp to the last cluster. Callers
// MUST pre-check `cells.is_empty()` — when there are no cells this returns 0
// and indexing the result would panic.
pub(crate) fn cell_containing_char(cells: &[GraphemeCell], char_index: usize) -> usize {
    cells
        .partition_point(|cell| cell.char_start <= char_index)
        .saturating_sub(1)
}

fn cell_containing_byte(cells: &[GraphemeCell], byte_offset: usize) -> usize {
    cells
        .partition_point(|cell| cell.byte_start <= byte_offset)
        .saturating_sub(1)
}

fn char_index_at_cell(cells: &[GraphemeCell], cell_ix: usize, total_chars: usize) -> usize {
    cells
        .get(cell_ix)
        .map(|cell| cell.char_start)
        .unwrap_or(total_chars)
}

fn byte_offset_at_cell(cells: &[GraphemeCell], cell_ix: usize, total_bytes: usize) -> usize {
    cells
        .get(cell_ix)
        .map(|cell| cell.byte_start)
        .unwrap_or(total_bytes)
}

fn identifier_run_start_cells(cells: &[GraphemeCell], index: usize) -> usize {
    let mut start = index.min(cells.len());
    while start > 0 && is_identifier_char(cells[start - 1].repr) {
        start -= 1;
    }
    start
}

fn identifier_run_end_cells(cells: &[GraphemeCell], index: usize) -> usize {
    let mut end = index.min(cells.len());
    while end < cells.len() && is_identifier_char(cells[end].repr) {
        end += 1;
    }
    end
}

fn subword_chunk_end(cells: &[GraphemeCell], start: usize, run_end: usize) -> usize {
    let Some(class) = subword_class(cells[start].repr) else {
        return (start + 1).min(run_end);
    };

    match class {
        SubwordClass::Digit => {
            let mut end = start + 1;
            while end < run_end && subword_class(cells[end].repr) == Some(SubwordClass::Digit) {
                end += 1;
            }
            end
        }
        SubwordClass::Lower | SubwordClass::Alpha => {
            let mut end = start + 1;
            while end < run_end {
                match subword_class(cells[end].repr) {
                    Some(SubwordClass::Lower | SubwordClass::Alpha) => end += 1,
                    _ => break,
                }
            }
            end
        }
        SubwordClass::Upper => {
            if start + 1 < run_end {
                if let Some(SubwordClass::Lower | SubwordClass::Alpha) =
                    subword_class(cells[start + 1].repr)
                {
                    let mut end = start + 2;
                    while end < run_end {
                        match subword_class(cells[end].repr) {
                            Some(SubwordClass::Lower | SubwordClass::Alpha) => end += 1,
                            _ => break,
                        }
                    }
                    return end;
                }
            }

            let mut end = start + 1;
            while end < run_end && subword_class(cells[end].repr) == Some(SubwordClass::Upper) {
                if end + 1 < run_end {
                    if let Some(SubwordClass::Lower | SubwordClass::Alpha) =
                        subword_class(cells[end + 1].repr)
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

fn subword_chunks(cells: &[GraphemeCell]) -> Vec<Range<usize>> {
    let mut chunks = Vec::new();
    let mut index = 0usize;
    while index < cells.len() {
        while index < cells.len() && cells[index].repr == '_' {
            index += 1;
        }
        if index >= cells.len() {
            break;
        }
        let end = subword_chunk_end(cells, index, cells.len());
        chunks.push(index..end);
        index = end;
    }
    chunks
}

// All of these `_cells` helpers take a cell index (not a char index / byte
// offset) and return a cell index. Callers translate to char indices via
// `char_index_at_cell` or to byte offsets via `byte_offset_at_cell`.

fn previous_word_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index > 0 && token_class(cells[index - 1].repr) == TokenClass::Whitespace {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    let class = token_class(cells[index - 1].repr);
    while index > 0 && token_class(cells[index - 1].repr) == class {
        index -= 1;
    }
    index
}

fn next_word_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index < cells.len() && token_class(cells[index].repr) == TokenClass::Whitespace {
        index += 1;
    }
    if index == cells.len() {
        return cells.len();
    }

    let class = token_class(cells[index].repr);
    while index < cells.len() && token_class(cells[index].repr) == class {
        index += 1;
    }
    index
}

fn previous_subword_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index > 0 && cells[index - 1].repr.is_whitespace() {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    if is_symbol_char(cells[index - 1].repr) {
        while index > 0 && is_symbol_char(cells[index - 1].repr) {
            index -= 1;
        }
        return index;
    }

    let run_start = identifier_run_start_cells(cells, index - 1);
    let run_end = identifier_run_end_cells(cells, index - 1);
    let chunks = subword_chunks(&cells[run_start..run_end]);
    let relative = index - run_start;
    chunks
        .iter()
        .rfind(|chunk| chunk.start < relative)
        .map_or(run_start, |chunk| run_start + chunk.start)
}

fn next_subword_boundary_cells(cells: &[GraphemeCell], cell_index: usize) -> usize {
    let mut index = cell_index.min(cells.len());
    while index < cells.len() && cells[index].repr.is_whitespace() {
        index += 1;
    }
    if index == cells.len() {
        return cells.len();
    }

    if is_symbol_char(cells[index].repr) {
        while index < cells.len() && is_symbol_char(cells[index].repr) {
            index += 1;
        }
        return index;
    }

    while index < cells.len() && cells[index].repr == '_' {
        index += 1;
    }
    if index == cells.len() {
        return cells.len();
    }

    let run_start = identifier_run_start_cells(cells, index);
    let run_end = identifier_run_end_cells(cells, index);
    let chunks = subword_chunks(&cells[run_start..run_end]);
    let relative = index - run_start;
    chunks
        .iter()
        .find(|chunk| chunk.start <= relative && relative < chunk.end)
        .map_or(run_end, |chunk| run_start + chunk.end)
}

pub fn previous_word_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target = previous_word_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn previous_subword_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target =
        previous_subword_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn next_word_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target = next_word_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn next_subword_boundary(buffer: &Rope, char_index: usize) -> usize {
    let cells = cells_of_rope(buffer);
    let total_chars = buffer.len_chars();
    let target = next_subword_boundary_cells(&cells, cell_partition_by_char(&cells, char_index));
    char_index_at_cell(&cells, target, total_chars)
}

pub fn word_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let (line_start, cells) = cells_of_rope_line(buffer, buffer.char_to_line(clamped));
    if cells.is_empty() {
        return clamped..clamped;
    }
    let local = clamped.saturating_sub(line_start);
    let cell_ix = cell_containing_char(&cells, local);
    let class = token_class(cells[cell_ix].repr);
    let mut start = cell_ix;
    while start > 0 && token_class(cells[start - 1].repr) == class {
        start -= 1;
    }
    let mut end = cell_ix + 1;
    while end < cells.len() && token_class(cells[end].repr) == class {
        end += 1;
    }
    let line_chars: usize = cells.iter().map(|c| c.char_len as usize).sum();
    let start_char = cells[start].char_start;
    let end_char = char_index_at_cell(&cells, end, line_chars);
    (line_start + start_char)..(line_start + end_char)
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
    let cells = cells_of_str(text);
    let target = previous_word_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn previous_subword_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = previous_subword_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn next_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = next_word_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn next_subword_boundary_in_text(text: &str, offset: usize) -> usize {
    let cells = cells_of_str(text);
    let target = next_subword_boundary_cells(&cells, cell_partition_by_byte(&cells, offset));
    byte_offset_at_cell(&cells, target, text.len())
}

pub fn word_range_in_text(text: &str, offset: usize) -> Range<usize> {
    let cells = cells_of_str(text);
    if cells.is_empty() {
        return 0..0;
    }
    let local = if offset >= text.len() {
        cells.len() - 1
    } else {
        cell_containing_byte(&cells, offset)
    };
    let class = token_class(cells[local].repr);
    let mut start = local;
    while start > 0 && token_class(cells[start - 1].repr) == class {
        start -= 1;
    }
    let mut end = local + 1;
    while end < cells.len() && token_class(cells[end].repr) == class {
        end += 1;
    }
    let start_byte = cells[start].byte_start;
    let end_byte = byte_offset_at_cell(&cells, end, text.len());
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
    fn rope_word_boundary_skips_full_combining_acute_cluster() {
        // "naïve word" with NFD ï = i + U+0308. 11 chars, 10 graphemes.
        // The combining mark sits at char index 3.
        let buffer = Rope::from_str("nai\u{0308}ve word");

        assert_eq!(next_word_boundary(&buffer, 0), 6);
        // Mid-cluster cursor still lands past the cluster, never inside it.
        assert_eq!(next_word_boundary(&buffer, 3), 6);
        assert_eq!(next_word_boundary(&buffer, 6), 11);
        assert_eq!(previous_word_boundary(&buffer, 11), 7);
        assert_eq!(previous_word_boundary(&buffer, 7), 0);
    }

    #[test]
    fn rope_word_boundary_treats_regional_indicator_pair_as_one_cluster() {
        // "a🇫🇷b cc" — regional indicator pair (4 bytes each) is one grapheme.
        // Char layout: [a, 🇫, 🇷, b, ' ', c, c] = 7 chars, 6 graphemes.
        let buffer = Rope::from_str("a\u{1F1EB}\u{1F1F7}b cc");

        assert_eq!(next_word_boundary(&buffer, 0), 1);
        // From the first regional indicator, jump past the second to `b` start.
        assert_eq!(next_word_boundary(&buffer, 1), 3);
        assert_eq!(next_word_boundary(&buffer, 3), 4);
        assert_eq!(previous_word_boundary(&buffer, 4), 3);
        // From after `b`, walking back lands at the regional pair start, not between them.
        assert_eq!(previous_word_boundary(&buffer, 3), 1);
        assert_eq!(previous_word_boundary(&buffer, 1), 0);
    }

    #[test]
    fn rope_subword_boundary_skips_combining_acute_cluster() {
        // "naïveCase" NFD: n, a, i, U+0308, v, e, C, a, s, e = 10 chars, 9 graphemes.
        let buffer = Rope::from_str("nai\u{0308}veCase");

        // From start, first subword spans the lowercase-only run before `C`.
        assert_eq!(next_subword_boundary(&buffer, 0), 6);
        assert_eq!(next_subword_boundary(&buffer, 6), 10);
        assert_eq!(previous_subword_boundary(&buffer, 10), 6);
        assert_eq!(previous_subword_boundary(&buffer, 6), 0);
    }

    #[test]
    fn rope_word_range_groups_full_combining_acute_cluster() {
        let buffer = Rope::from_str("nai\u{0308}ve word");

        // Click anywhere on the cluster — including on the combining mark — and
        // the whole `naïve` token comes back.
        assert_eq!(word_range_at_char(&buffer, 0), 0..6);
        assert_eq!(word_range_at_char(&buffer, 3), 0..6);
        assert_eq!(word_range_at_char(&buffer, 5), 0..6);
        assert_eq!(word_range_at_char(&buffer, 7), 7..11);
    }

    #[test]
    fn text_word_boundary_skips_full_combining_acute_cluster() {
        let text = "nai\u{0308}ve word";

        let after_naive = "nai\u{0308}ve".len();
        let space = "nai\u{0308}ve ".len();
        let i_byte = "na".len();
        let combining_byte = "nai".len();

        assert_eq!(next_word_boundary_in_text(text, 0), after_naive);
        assert_eq!(
            next_word_boundary_in_text(text, combining_byte),
            after_naive
        );
        assert_eq!(previous_word_boundary_in_text(text, text.len()), space);
        assert_eq!(previous_word_boundary_in_text(text, after_naive), 0);
        // Mid-cluster offset rounds out to the same cluster boundary as `i` start.
        assert_eq!(
            previous_word_boundary_in_text(text, combining_byte),
            previous_word_boundary_in_text(text, i_byte)
        );
    }

    #[test]
    fn text_subword_boundary_skips_full_combining_acute_cluster() {
        let text = "nai\u{0308}veCase";

        let after_naive = "nai\u{0308}ve".len();
        assert_eq!(next_subword_boundary_in_text(text, 0), after_naive);
        assert_eq!(next_subword_boundary_in_text(text, after_naive), text.len());
        assert_eq!(
            previous_subword_boundary_in_text(text, text.len()),
            after_naive
        );
        assert_eq!(previous_subword_boundary_in_text(text, after_naive), 0);
    }

    #[test]
    fn text_word_range_groups_full_regional_indicator_cluster() {
        let text = "a\u{1F1EB}\u{1F1F7}b cc";
        let flag_start = "a".len();
        let after_flag = "a\u{1F1EB}\u{1F1F7}".len();
        // Click on either regional indicator — the range covers the full cluster.
        assert_eq!(word_range_in_text(text, flag_start), flag_start..after_flag);
        assert_eq!(
            word_range_in_text(text, flag_start + "\u{1F1EB}".len()),
            flag_start..after_flag,
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
