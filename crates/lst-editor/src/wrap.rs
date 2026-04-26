use crate::selection::{cells_of_str, GraphemeCell};

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

pub fn build_wrap_layout(lines: &[String], wrap_columns: usize, show_wrap: bool) -> WrapLayout {
    let wrap_columns = wrap_columns.max(1);
    let mut line_row_starts = Vec::with_capacity(lines.len() + 1);
    let mut total_rows = 0usize;
    line_row_starts.push(0);

    for line in lines {
        let display = trim_display_line(line);
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

pub fn line_for_visual_row(layout: &WrapLayout, visual_row: usize) -> usize {
    layout
        .line_row_starts
        .partition_point(|start| *start <= visual_row)
        .saturating_sub(1)
        .min(layout.line_row_starts.len().saturating_sub(2))
}

pub fn visual_row_for_position(
    lines: &[String],
    line: usize,
    column: usize,
    layout: &WrapLayout,
) -> Option<usize> {
    let line_start_row = layout.line_row_starts.get(line).copied()?;
    let display_text = trim_display_line(lines.get(line)?);
    let display_column = column.min(display_text.chars().count());
    let row_in_line = if layout.show_wrap {
        cursor_visual_row_in_line(display_text, display_column, layout.wrap_columns)
    } else {
        0
    };
    Some(line_start_row + row_in_line)
}

pub fn display_row_target(
    lines: &[String],
    line: usize,
    column: usize,
    preferred_column: Option<usize>,
    delta: isize,
    layout: &WrapLayout,
) -> Option<DisplayRowTarget> {
    if lines.is_empty() || !layout.show_wrap {
        return None;
    }

    let display_text = trim_display_line(lines.get(line)?);
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
    let preferred_column =
        preferred_column.unwrap_or_else(|| column.saturating_sub(current_segment.start_col));
    let target_line = line_for_visual_row(layout, target_visual_row);
    let target_text = trim_display_line(lines.get(target_line)?);
    let target_segments = wrap_segments(target_text, layout.wrap_columns);
    let target_row_in_line = target_visual_row - layout.line_row_starts[target_line];
    let target_segment = target_segments
        .get(target_row_in_line)
        .or_else(|| target_segments.last())
        .expect("wrap_segments always returns at least one segment");
    let target_column =
        target_segment.start_col + preferred_column.min(target_segment.text.chars().count());

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
    fn wrap_segments_follow_visual_rows() {
        let segments = wrap_segments("alpha beta gamma", 6);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "alpha ");
        assert_eq!(segments[0].start_col, 0);
        assert_eq!(segments[0].end_col, 6);
        assert_eq!(segments[1].text, "beta ");
        assert_eq!(segments[1].start_col, 6);
        assert_eq!(segments[1].end_col, 11);
        assert_eq!(segments[2].text, "gamma");
        assert_eq!(segments[2].start_col, 11);
        assert_eq!(segments[2].end_col, 16);
        assert_eq!(visual_line_count("alpha beta gamma", 6), 3);
    }

    #[test]
    fn wrap_layout_maps_visual_rows_to_lines() {
        let lines = vec!["alpha beta gamma".to_string(), "short".to_string()];
        let layout = build_wrap_layout(&lines, 6, true);

        assert_eq!(layout.total_rows, 4);
        assert_eq!(line_for_visual_row(&layout, 0), 0);
        assert_eq!(line_for_visual_row(&layout, 2), 0);
        assert_eq!(line_for_visual_row(&layout, 3), 1);
    }

    #[test]
    fn display_row_target_preserves_visual_column() {
        let lines = vec!["alpha beta gamma".to_string(), "short".to_string()];
        let layout = build_wrap_layout(&lines, 6, true);

        assert_eq!(
            display_row_target(&lines, 0, 1, None, 1, &layout),
            Some(DisplayRowTarget {
                line: 0,
                column: 7,
                preferred_column: 1,
            })
        );
        assert_eq!(
            display_row_target(&lines, 0, 1, Some(1), 3, &layout),
            Some(DisplayRowTarget {
                line: 1,
                column: 1,
                preferred_column: 1,
            })
        );
    }

    #[test]
    fn wrap_keeps_grapheme_cluster_intact() {
        // "naïve" decomposed: n, a, i, U+0308 (combining diaeresis), v, e — 6 chars / 5 clusters.
        let line = "na\u{0069}\u{0308}ve";
        let segments = wrap_segments(line, 3);

        for segment in &segments {
            // No segment should ever begin with a combining mark — that would mean a cluster was split.
            assert_ne!(segment.text.chars().next(), Some('\u{0308}'));
        }
        // Combining mark stays glued to its base char in cursor_rows.
        let layout = line_layout(line, 3);
        assert_eq!(layout.cursor_rows[2], layout.cursor_rows[3]);
    }
}
