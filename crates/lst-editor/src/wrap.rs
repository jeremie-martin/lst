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
    let display = trim_rope_display_line(line);
    if display.len_chars() <= max_cols
        && display
            .chunks()
            .all(|chunk| chunk.bytes().all(|byte| byte.is_ascii() && byte != b'\t'))
    {
        1
    } else {
        visual_line_count(&display.to_string(), max_cols)
    }
}

fn trim_rope_display_line(line: RopeSlice<'_>) -> RopeSlice<'_> {
    let mut end = line.len_chars();
    while end > 0 && matches!(line.char(end - 1), '\n' | '\r') {
        end -= 1;
    }
    line.slice(..end)
}

pub fn line_for_visual_row(layout: &WrapLayout, visual_row: usize) -> usize {
    layout
        .line_row_starts
        .partition_point(|start| *start <= visual_row)
        .saturating_sub(1)
        .min(layout.line_row_starts.len().saturating_sub(2))
}

pub fn visual_row_for_position<T: AsRef<str>>(
    lines: &[T],
    line: usize,
    column: usize,
    layout: &WrapLayout,
) -> Option<usize> {
    let line_start_row = layout.line_row_starts.get(line).copied()?;
    let display_text = trim_display_line(lines.get(line)?.as_ref());
    let display_column = column.min(display_text.chars().count());
    let row_in_line = if layout.show_wrap {
        cursor_visual_row_in_line(display_text, display_column, layout.wrap_columns)
    } else {
        0
    };
    Some(line_start_row + row_in_line)
}

pub fn visual_row_for_position_in_rope(
    buffer: &Rope,
    line: usize,
    column: usize,
    layout: &WrapLayout,
) -> Option<usize> {
    let line_start_row = layout.line_row_starts.get(line).copied()?;
    let display_text = crate::selection::line_display_text(buffer, line);
    let display_column = column.min(display_text.chars().count());
    let row_in_line = if layout.show_wrap {
        cursor_visual_row_in_line(&display_text, display_column, layout.wrap_columns)
    } else {
        0
    };
    Some(line_start_row + row_in_line)
}

pub fn display_row_target<T: AsRef<str>>(
    lines: &[T],
    line: usize,
    column: usize,
    preferred_column: Option<usize>,
    delta: isize,
    layout: &WrapLayout,
) -> Option<DisplayRowTarget> {
    if lines.is_empty() || !layout.show_wrap {
        return None;
    }

    let display_text = trim_display_line(lines.get(line)?.as_ref());
    let column = column.min(display_text.chars().count());
    let segment_row = cursor_visual_row_in_line(display_text, column, layout.wrap_columns);
    let visual_row = layout.line_row_starts.get(line).copied()? + segment_row;
    let target_visual_row = if delta.is_negative() {
        visual_row.saturating_sub(delta.unsigned_abs())
    } else {
        (visual_row + delta as usize).min(layout.total_rows.saturating_sub(1))
    };

    if target_visual_row == visual_row {
        return None;
    }

    let segments = wrap_segments(display_text, layout.wrap_columns);
    let current_segment = segments
        .get(segment_row)
        .or_else(|| segments.last())
        .expect("wrap_segments always returns at least one segment");
    let preferred_column = preferred_column.unwrap_or_else(|| column.saturating_sub(current_segment.start_col));
    let target_line = line_for_visual_row(layout, target_visual_row);
    let target_text = trim_display_line(lines.get(target_line)?.as_ref());
    let target_segments = wrap_segments(target_text, layout.wrap_columns);
    let target_row_in_line = target_visual_row - layout.line_row_starts[target_line];
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

pub fn display_row_target_in_rope(
    buffer: &Rope,
    line: usize,
    column: usize,
    preferred_column: Option<usize>,
    delta: isize,
    layout: &WrapLayout,
) -> Option<DisplayRowTarget> {
    if buffer.len_lines() == 0 || !layout.show_wrap {
        return None;
    }

    let display_text = crate::selection::line_display_text(buffer, line);
    let column = column.min(display_text.chars().count());
    let segment_row = cursor_visual_row_in_line(&display_text, column, layout.wrap_columns);
    let visual_row = layout.line_row_starts.get(line).copied()? + segment_row;
    let target_visual_row = if delta.is_negative() {
        visual_row.saturating_sub(delta.unsigned_abs())
    } else {
        (visual_row + delta as usize).min(layout.total_rows.saturating_sub(1))
    };
    if target_visual_row == visual_row {
        return None;
    }

    let segments = wrap_segments(&display_text, layout.wrap_columns);
    let current_segment = segments
        .get(segment_row)
        .or_else(|| segments.last())
        .expect("wrap_segments always returns at least one segment");
    let preferred_column = preferred_column.unwrap_or_else(|| column.saturating_sub(current_segment.start_col));
    let target_line = line_for_visual_row(layout, target_visual_row);
    let target_text = crate::selection::line_display_text(buffer, target_line);
    let target_segments = wrap_segments(&target_text, layout.wrap_columns);
    let target_row_in_line = target_visual_row - layout.line_row_starts[target_line];
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
