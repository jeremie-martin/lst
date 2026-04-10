const MIN_LINE_NUMBER_DIGITS: usize = 4;
const LINE_NUMBER_GAP_CHARS: usize = 1;
const TAB_WIDTH: usize = 8;

pub fn line_number_digits_width(line_count: usize) -> usize {
    line_count
        .max(1)
        .to_string()
        .len()
        .max(MIN_LINE_NUMBER_DIGITS)
}

pub fn wrap_columns_with_gutter(
    viewport_width: f32,
    char_width: f32,
    line_count: usize,
    show_gutter: bool,
    editor_pad: f32,
    right_pad: f32,
    gutter_left_pad: f32,
    gutter_separator_width: f32,
) -> usize {
    let gutter_width = if show_gutter {
        char_width * (line_number_digits_width(line_count) + LINE_NUMBER_GAP_CHARS) as f32
            + gutter_left_pad
    } else {
        0.0
    };
    let separator_width = if show_gutter {
        gutter_separator_width
    } else {
        0.0
    };
    let editor_width =
        (viewport_width - gutter_width - separator_width - editor_pad - right_pad).max(char_width);
    (editor_width / char_width).floor().max(1.0) as usize
}

pub fn visual_line_count(line: &str, max_cols: usize) -> usize {
    if line.is_empty() || max_cols == 0 {
        return 1;
    }

    let mut lines = 1usize;
    let mut col = 0usize;
    let mut chars = line.chars().peekable();

    while chars.peek().is_some() {
        let (token_len, token_width) = token_shape(chars.clone(), col);

        if col > 0 && token_width > max_cols.saturating_sub(col) {
            lines += 1;
            col = 0;
        }

        for _ in 0..token_len {
            let ch = chars.next().expect("token length came from this iterator");
            let mut width = char_width(ch, col);
            if col > 0 && col + width > max_cols {
                lines += 1;
                col = 0;
                width = char_width(ch, col);
            }

            col += width;
            while col > max_cols {
                lines += 1;
                col -= max_cols;
            }
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

    let chars: Vec<char> = line.chars().collect();
    let layout = line_layout(line, max_cols);
    let mut segments = Vec::new();
    let mut start = 0usize;
    let mut row = 0usize;
    for idx in 1..=chars.len() {
        let current_row = layout.cursor_rows[idx];
        if current_row != row {
            segments.push(WrappedSegment {
                start_col: start,
                end_col: idx - 1,
                text: chars[start..idx].iter().collect(),
            });
            start = idx;
            row = current_row;
        }
    }
    segments.push(WrappedSegment {
        start_col: start,
        end_col: chars.len(),
        text: chars[start..].iter().collect(),
    });
    segments
}

struct LineLayout {
    cursor_rows: Vec<usize>,
}

fn line_layout(line: &str, max_cols: usize) -> LineLayout {
    if line.is_empty() || max_cols == 0 {
        return LineLayout {
            cursor_rows: vec![0],
        };
    }

    let chars: Vec<char> = line.chars().collect();
    let mut cursor_rows = vec![0; chars.len() + 1];
    let mut row = 0usize;
    let mut col = 0usize;
    let mut index = 0usize;

    while index < chars.len() {
        let token_end = token_end(&chars, index);

        if col > 0 && span_width(&chars[index..token_end], col) > max_cols.saturating_sub(col) {
            row += 1;
            col = 0;
            cursor_rows[index] = row;
        }

        while index < token_end {
            cursor_rows[index] = row;

            let mut width = char_width(chars[index], col);
            if col > 0 && col + width > max_cols {
                row += 1;
                col = 0;
                cursor_rows[index] = row;
                width = char_width(chars[index], col);
            }

            col += width;
            while col > max_cols {
                row += 1;
                col -= max_cols;
            }

            index += 1;
            cursor_rows[index] = row;
        }
    }

    LineLayout { cursor_rows }
}

fn token_end(chars: &[char], start: usize) -> usize {
    let mut end = start;

    if chars[start].is_whitespace() {
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
    } else {
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
    }

    end
}

fn span_width(chars: &[char], start_col: usize) -> usize {
    let mut col = start_col;
    for ch in chars {
        col += char_width(*ch, col);
    }
    col - start_col
}

fn token_shape(
    mut chars: std::iter::Peekable<std::str::Chars<'_>>,
    start_col: usize,
) -> (usize, usize) {
    let Some(first) = chars.peek().copied() else {
        return (0, 0);
    };

    let whitespace = first.is_whitespace();
    let mut token_len = 0usize;
    let mut col = start_col;
    let mut in_trailing_whitespace = false;

    while let Some(ch) = chars.peek().copied() {
        if whitespace {
            if !ch.is_whitespace() {
                break;
            }
        } else if !in_trailing_whitespace {
            if ch.is_whitespace() {
                in_trailing_whitespace = true;
            }
        } else if !ch.is_whitespace() {
            break;
        }

        col += char_width(ch, col);
        token_len += 1;
        chars.next();
    }

    (token_len, col - start_col)
}

fn char_width(ch: char, col: usize) -> usize {
    if ch == '\t' {
        let tab_stop = TAB_WIDTH - (col % TAB_WIDTH);
        tab_stop.max(1)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_segments_follow_visual_rows() {
        let segments = wrap_segments("alpha beta gamma", 6);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "alpha ");
        assert_eq!(segments[1].text, "beta ");
        assert_eq!(segments[2].text, "gamma");
        assert_eq!(visual_line_count("alpha beta gamma", 6), 3);
    }
}
