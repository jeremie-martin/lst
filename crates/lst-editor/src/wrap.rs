use crate::selection::{cells_of_str, GraphemeCell};
use ropey::{Rope, RopeSlice};

const TAB_WIDTH: usize = 8;

pub fn visual_line_count(line: &str, max_cols: usize) -> usize {
    if line.is_empty() || max_cols == 0 {
        return 1;
    }
    let cells = cells_of_str(line);
    if cells.is_empty() {
        return 1;
    }

    let mut lines = 1usize;
    let mut col = 0usize;
    let mut idx = 0usize;

    while idx < cells.len() {
        let token_end = token_end(&cells, idx);
        let token_width = span_width(&cells[idx..token_end], col);

        if col > 0 && token_width > max_cols.saturating_sub(col) {
            lines += 1;
            col = 0;
        }

        while idx < token_end {
            let mut width = cell_width(cells[idx].repr, col);
            if col > 0 && col + width > max_cols {
                lines += 1;
                col = 0;
                width = cell_width(cells[idx].repr, col);
            }

            col += width;
            while col > max_cols {
                lines += 1;
                col -= max_cols;
            }

            idx += 1;
        }
    }

    lines
}

pub fn cursor_visual_row_in_line(line: &str, column: usize, max_cols: usize) -> usize {
    let layout = line_layout(line, max_cols);
    let max_column = layout.cursor_rows.len().saturating_sub(1);
    layout.cursor_rows[column.min(max_column)]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapLayout {
    pub show_wrap: bool,
    pub wrap_columns: usize,
    pub line_row_starts: Vec<usize>,
    pub total_rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRowTarget {
    pub line: usize,
    pub column: usize,
    pub preferred_column: usize,
}

pub fn build_wrap_layout<T: AsRef<str>>(lines: &[T], wrap_columns: usize, show_wrap: bool) -> WrapLayout {
    let wrap_columns = wrap_columns.max(1);
    let mut line_row_starts = Vec::with_capacity(lines.len() + 1);
    let mut total_rows = 0usize;
    line_row_starts.push(0);

    for line in lines {
        let display = trim_display_line(line.as_ref());
        total_rows += if show_wrap {
            visual_line_count(display, wrap_columns)
        } else {
            1
        };
        line_row_starts.push(total_rows);
    }

    WrapLayout {
        show_wrap,
        wrap_columns,
        line_row_starts,
        total_rows: total_rows.max(1),
    }
}

pub fn build_wrap_layout_for_rope(buffer: &Rope, wrap_columns: usize, show_wrap: bool) -> WrapLayout {
    let line_count = buffer.len_lines();
    let wrap_columns = wrap_columns.max(1);
    if !show_wrap {
        return WrapLayout {
            show_wrap,
            wrap_columns,
            line_row_starts: (0..=line_count).collect(),
            total_rows: line_count.max(1),
        };
    }

    let mut line_row_starts = Vec::with_capacity(line_count.saturating_add(1));
    let mut total_rows = 0usize;
    line_row_starts.push(0);
    for line in buffer.lines() {
        total_rows = total_rows.saturating_add(visual_line_count_for_rope_line(line, wrap_columns));
        line_row_starts.push(total_rows);
    }
    WrapLayout {
        show_wrap,
        wrap_columns,
        line_row_starts,
        total_rows: total_rows.max(1),
    }
}

/// Count the wrapped rows for one rope line without materializing ordinary
/// short ASCII lines. Complex and potentially wrapping text falls back to the
/// same grapheme-aware implementation used everywhere else.
pub fn visual_line_count_for_rope_line(line: RopeSlice<'_>, max_cols: usize) -> usize {
    let max_cols = max_cols.max(1);
    let text = std::borrow::Cow::from(line);
    let display = text.trim_end_matches(['\n', '\r']);
    if display.len() <= max_cols && display.is_ascii() && !display.as_bytes().contains(&b'\t') {
        1
    } else {
        visual_line_count(display, max_cols)
    }
}

pub fn line_for_visual_row(layout: &WrapLayout, visual_row: usize) -> usize {
    layout
        .line_row_starts
        .partition_point(|start| *start <= visual_row)
        .saturating_sub(1)
        .min(layout.line_row_starts.len().saturating_sub(2))
}

/// Finds the line and column `delta` display rows away from `(line, column)`
/// by walking neighbouring lines, so the cost is proportional to the rows
/// moved rather than the document size. Rows past either end clamp to the
/// first row of the document or the last row of its last line. Returns
/// `None` when the caret would not move.
pub fn display_row_target_in_rope(
    buffer: &Rope,
    line: usize,
    column: usize,
    preferred_column: Option<usize>,
    delta: isize,
    wrap_columns: usize,
) -> Option<DisplayRowTarget> {
    if buffer.len_lines() == 0 {
        return None;
    }
    let line = line.min(buffer.len_lines() - 1);
    let display_text = crate::selection::line_display_text(buffer, line);
    let column = column.min(display_text.chars().count());
    let segments = wrap_segments(&display_text, wrap_columns);
    let segment_row = cursor_visual_row_in_line(&display_text, column, wrap_columns).min(segments.len() - 1);
    let preferred_column = preferred_column.unwrap_or_else(|| column.saturating_sub(segments[segment_row].start_col));

    let (target_line, target_row_in_line) = if delta >= 0 {
        walk_rows_down(buffer, line, segments.len(), segment_row, delta as usize, wrap_columns)
    } else {
        walk_rows_up(buffer, line, segment_row, delta.unsigned_abs(), wrap_columns)
    };
    if target_line == line && target_row_in_line == segment_row {
        return None;
    }

    let target_segments = if target_line == line {
        segments
    } else {
        wrap_segments(&crate::selection::line_display_text(buffer, target_line), wrap_columns)
    };
    let target_segment = target_segments
        .get(target_row_in_line)
        .or_else(|| target_segments.last())
        .expect("wrap_segments always returns at least one segment");
    let target_column = target_segment.start_col + preferred_column.min(target_segment.text.chars().count());
    Some(DisplayRowTarget {
        line: target_line,
        column: target_column,
        preferred_column,
    })
}

/// Walks `remaining` rows down from `row_in_line` of `line`, whose row count
/// is `line_rows`; clamps to the last row of the last line.
fn walk_rows_down(
    buffer: &Rope,
    line: usize,
    line_rows: usize,
    row_in_line: usize,
    remaining: usize,
    wrap_columns: usize,
) -> (usize, usize) {
    if row_in_line + remaining < line_rows {
        return (line, row_in_line + remaining);
    }
    let mut remaining = remaining - (line_rows - row_in_line);
    let mut current = line;
    let mut current_rows = line_rows;
    for next in line + 1..buffer.len_lines() {
        let rows = visual_line_count_for_rope_line(buffer.line(next), wrap_columns);
        if remaining < rows {
            return (next, remaining);
        }
        remaining -= rows;
        current = next;
        current_rows = rows;
    }
    (current, current_rows - 1)
}

/// Walks `remaining` rows up from `row_in_line` of `line`; clamps to the
/// first row of the document.
fn walk_rows_up(
    buffer: &Rope,
    line: usize,
    row_in_line: usize,
    remaining: usize,
    wrap_columns: usize,
) -> (usize, usize) {
    if remaining <= row_in_line {
        return (line, row_in_line - remaining);
    }
    let mut remaining = remaining - row_in_line - 1;
    for previous in (0..line).rev() {
        let rows = visual_line_count_for_rope_line(buffer.line(previous), wrap_columns);
        if remaining < rows {
            return (previous, rows - 1 - remaining);
        }
        remaining -= rows;
    }
    (0, 0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedSegment {
    pub start_col: usize,
    pub end_col: usize,
    pub text: String,
}

pub fn wrap_segments(line: &str, max_cols: usize) -> Vec<WrappedSegment> {
    if line.is_empty() || max_cols == 0 {
        return vec![WrappedSegment {
            start_col: 0,
            end_col: 0,
            text: String::new(),
        }];
    }

    let cells = cells_of_str(line);
    let layout = line_layout_from_cells(&cells, max_cols);
    let total_chars = layout.cursor_rows.len().saturating_sub(1);
    let mut segments = Vec::new();
    let mut start_char = 0usize;
    let mut start_byte = 0usize;
    let mut row = 0usize;
    // Cluster boundaries are the only places a row transition can occur, so
    // checking each cell start is enough — and slicing `line` by byte_start
    // skips the per-segment chars-collect.
    for cell in cells.iter().skip(1) {
        let cell_row = layout.cursor_rows[cell.char_start];
        if cell_row != row {
            segments.push(WrappedSegment {
                start_col: start_char,
                end_col: cell.char_start,
                text: line[start_byte..cell.byte_start].to_string(),
            });
            while row + 1 < cell_row {
                row += 1;
                segments.push(WrappedSegment {
                    start_col: cell.char_start,
                    end_col: cell.char_start,
                    text: String::new(),
                });
            }
            start_char = cell.char_start;
            start_byte = cell.byte_start;
            row = cell_row;
        }
    }
    segments.push(WrappedSegment {
        start_col: start_char,
        end_col: total_chars,
        text: line[start_byte..].to_string(),
    });
    let final_row = layout.cursor_rows[total_chars];
    while row < final_row {
        row += 1;
        segments.push(WrappedSegment {
            start_col: total_chars,
            end_col: total_chars,
            text: String::new(),
        });
    }
    segments
}

struct LineLayout {
    cursor_rows: Vec<usize>,
}

fn line_layout(line: &str, max_cols: usize) -> LineLayout {
    line_layout_from_cells(&cells_of_str(line), max_cols)
}

fn line_layout_from_cells(cells: &[GraphemeCell], max_cols: usize) -> LineLayout {
    let char_count: usize = cells.iter().map(|cell| cell.char_len as usize).sum();
    if char_count == 0 || max_cols == 0 {
        return LineLayout {
            cursor_rows: vec![0; char_count + 1],
        };
    }

    let mut cursor_rows = vec![0; char_count + 1];
    let mut row = 0usize;
    let mut col = 0usize;
    let mut idx = 0usize;

    while idx < cells.len() {
        let token_end = token_end(cells, idx);

        if col > 0 && span_width(&cells[idx..token_end], col) > max_cols.saturating_sub(col) {
            row += 1;
            col = 0;
            mark_cluster_rows(&mut cursor_rows, &cells[idx], row);
        }

        while idx < token_end {
            let cell = cells[idx];
            mark_cluster_rows(&mut cursor_rows, &cell, row);

            let mut width = cell_width(cell.repr, col);
            if col > 0 && col + width > max_cols {
                row += 1;
                col = 0;
                mark_cluster_rows(&mut cursor_rows, &cell, row);
                width = cell_width(cell.repr, col);
            }

            col += width;
            while col > max_cols {
                row += 1;
                col -= max_cols;
            }

            idx += 1;
            cursor_rows[cell.char_start + cell.char_len as usize] = row;
        }
    }

    LineLayout { cursor_rows }
}

fn mark_cluster_rows(cursor_rows: &mut [usize], cell: &GraphemeCell, row: usize) {
    for offset in 0..cell.char_len as usize {
        cursor_rows[cell.char_start + offset] = row;
    }
}

fn token_end(cells: &[GraphemeCell], start: usize) -> usize {
    let mut end = start;

    if cells[start].repr.is_whitespace() {
        while end < cells.len() && cells[end].repr.is_whitespace() {
            end += 1;
        }
    } else {
        while end < cells.len() && !cells[end].repr.is_whitespace() {
            end += 1;
        }
        while end < cells.len() && cells[end].repr.is_whitespace() {
            end += 1;
        }
    }

    end
}

fn span_width(cells: &[GraphemeCell], start_col: usize) -> usize {
    let mut col = start_col;
    for cell in cells {
        col += cell_width(cell.repr, col);
    }
    col - start_col
}

fn cell_width(repr: char, col: usize) -> usize {
    if repr == '\t' {
        let tab_stop = TAB_WIDTH - (col % TAB_WIDTH);
        tab_stop.max(1)
    } else {
        1
    }
}

fn trim_display_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rope_row_counts_match_grapheme_layout_across_chunks_and_line_endings() {
        let text = [
            "ordinary short text\n".repeat(100),
            "\tindented\r\nempty next\n\n".into(),
            "e\u{301} 👩‍💻 日本語 words\r".repeat(100),
            "long unbroken text".repeat(1000),
            "\r\nlast line".into(),
        ]
        .concat();
        let rope = Rope::from_str(&text);
        for columns in [0, 1, 4, 8, 17, 80, 1024] {
            for line in rope.lines() {
                let owned = line.to_string();
                let display = owned.trim_end_matches(['\n', '\r']);
                assert_eq!(
                    visual_line_count_for_rope_line(line, columns),
                    visual_line_count(display, columns.max(1)),
                    "columns {columns}, line {display:?}"
                );
            }
        }
    }

    /// The previous implementation: resolve rows through a full-document
    /// layout. Kept as the oracle for the local walk.
    fn display_row_target_via_layout(
        buffer: &Rope,
        line: usize,
        column: usize,
        delta: isize,
        wrap_columns: usize,
    ) -> Option<(usize, usize)> {
        let layout = build_wrap_layout_for_rope(buffer, wrap_columns, true);
        let display_text = crate::selection::line_display_text(buffer, line);
        let column = column.min(display_text.chars().count());
        let segment_row = cursor_visual_row_in_line(&display_text, column, wrap_columns);
        let visual_row = layout.line_row_starts[line] + segment_row;
        let target_visual_row = if delta.is_negative() {
            visual_row.saturating_sub(delta.unsigned_abs())
        } else {
            (visual_row + delta as usize).min(layout.total_rows.saturating_sub(1))
        };
        if target_visual_row == visual_row {
            return None;
        }
        let segments = wrap_segments(&display_text, wrap_columns);
        let current = segments.get(segment_row).or_else(|| segments.last()).unwrap();
        let preferred = column.saturating_sub(current.start_col);
        let target_line = line_for_visual_row(&layout, target_visual_row);
        let target_text = crate::selection::line_display_text(buffer, target_line);
        let target_segments = wrap_segments(&target_text, wrap_columns);
        let row_in_line = target_visual_row - layout.line_row_starts[target_line];
        let segment = target_segments
            .get(row_in_line)
            .or_else(|| target_segments.last())
            .unwrap();
        Some((
            target_line,
            segment.start_col + preferred.min(segment.text.chars().count()),
        ))
    }

    #[test]
    fn local_row_walk_matches_the_full_layout_for_every_position_and_delta() {
        let text = "short\n\nthe quick brown fox jumps over the lazy dog again and again\r\n\
                    x\n\tindented tab line with several words inside\nend";
        let buffer = Rope::from_str(text);
        for wrap_columns in [1, 4, 9, 17, 80] {
            for line in 0..buffer.len_lines() {
                let len = crate::selection::line_display_text(&buffer, line).chars().count();
                for column in 0..=len {
                    for delta in -12isize..=12 {
                        let expected = display_row_target_via_layout(&buffer, line, column, delta, wrap_columns);
                        let actual = display_row_target_in_rope(&buffer, line, column, None, delta, wrap_columns)
                            .map(|target| (target.line, target.column));
                        assert_eq!(
                            actual, expected,
                            "line {line} column {column} delta {delta} cols {wrap_columns}"
                        );
                    }
                }
            }
        }
    }
}
