use crate::style::{EDITOR_PAD, LINE_HEIGHT_PX};
use iced::widget::scrollable;
use std::ops::Range;

const MIN_LINE_NUMBER_DIGITS: usize = 4;
const LINE_NUMBER_GAP_CHARS: usize = 1;
const TAB_WIDTH: usize = 8;

pub const LINE_NUMBER_LEFT_PAD: f32 = 4.0;
pub const GUTTER_SEPARATOR_WIDTH: f32 = 1.0;
pub const EDITOR_RIGHT_PAD: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevealIntent {
    #[default]
    None,
    RevealCaret,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ViewportState {
    width: f32,
    height: f32,
    content_height: f32,
    scroll_y: f32,
}

impl ViewportState {
    pub fn update(&mut self, viewport: scrollable::Viewport) {
        self.width = viewport.bounds().width;
        self.height = viewport.bounds().height;
        self.content_height = viewport.content_bounds().height;
        self.scroll_y = viewport.absolute_offset().y;
    }

    pub fn from_metrics(width: f32, height: f32, content_height: f32, scroll_y: f32) -> Self {
        Self {
            width,
            height,
            content_height,
            scroll_y,
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }

    pub fn set_scroll_y(&mut self, scroll_y: f32) {
        self.scroll_y = scroll_y;
    }

    pub fn can_reveal(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    pub fn with_content_height(mut self, content_height: f32) -> Self {
        self.content_height = content_height;
        self
    }

    pub fn reveal_offset(&self, caret_top: f32, caret_height: f32, margin: f32) -> Option<f32> {
        if !self.can_reveal() {
            return None;
        }

        let top_edge = self.scroll_y + margin;
        let bottom_edge = self.scroll_y + self.height - margin;
        let target = if caret_top < top_edge {
            caret_top - margin
        } else if caret_top + caret_height > bottom_edge {
            caret_top + caret_height + margin - self.height
        } else {
            return None;
        };

        let target = target.clamp(0.0, self.max_scroll_y());
        if (target - self.scroll_y).abs() < f32::EPSILON {
            None
        } else {
            Some(target)
        }
    }

    fn max_scroll_y(&self) -> f32 {
        (self.content_height - self.height).max(0.0)
    }
}

pub fn visible_row_range(scroll_y: f32, viewport_height: f32, total_rows: usize) -> Range<usize> {
    if total_rows == 0 {
        return 0..0;
    }

    let top = (scroll_y - EDITOR_PAD).max(0.0);
    let bottom = (scroll_y + viewport_height - EDITOR_PAD).max(0.0);
    let start = (top / LINE_HEIGHT_PX).floor() as usize;
    let start = start.min(total_rows.saturating_sub(1));
    let end = ((bottom / LINE_HEIGHT_PX).ceil() as usize).clamp(start + 1, total_rows);

    start..end
}

pub fn line_number_digits_width(line_count: usize) -> usize {
    line_count
        .max(1)
        .to_string()
        .len()
        .max(MIN_LINE_NUMBER_DIGITS)
}

pub fn continuation_prefix(line_count: usize) -> String {
    " ".repeat(line_number_digits_width(line_count) + LINE_NUMBER_GAP_CHARS)
}

pub fn line_number_gutter_width(line_count: usize, char_width: f32) -> f32 {
    char_width * (line_number_digits_width(line_count) + LINE_NUMBER_GAP_CHARS) as f32
        + LINE_NUMBER_LEFT_PAD
}

pub fn wrap_columns(viewport_width: f32, char_width: f32, line_count: usize) -> usize {
    let editor_width = editor_text_width(viewport_width, char_width, line_count);
    (editor_width / char_width).floor().max(1.0) as usize
}

pub fn visual_line_count(line: &str, max_cols: usize) -> usize {
    if line.is_empty() || max_cols == 0 {
        return 1;
    }

    // Fast path: line fits in one row when no char is wider than 1 column.
    // char_width() returns 1 for every non-tab character, so display width
    // equals the char count.  For valid UTF-8, chars().count() <= len(),
    // so checking len() is a conservative (never wrong) byte-level test.
    if line.len() <= max_cols && !line.as_bytes().contains(&b'\t') {
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

pub fn content_height(viewport_height: f32, visual_lines: usize) -> f32 {
    let overscroll = viewport_height * 0.4;
    let text_height =
        visual_lines as f32 * crate::style::LINE_HEIGHT_PX + EDITOR_PAD * 2.0 + overscroll;
    text_height.max(viewport_height)
}

fn editor_text_width(viewport_width: f32, char_width: f32, line_count: usize) -> f32 {
    let gutter_width = line_number_gutter_width(line_count, char_width);
    (viewport_width - gutter_width - GUTTER_SEPARATOR_WIDTH - EDITOR_PAD - EDITOR_RIGHT_PAD)
        .max(char_width)
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

#[cfg(all(test, feature = "internal-invariants"))]
mod tests {
    use super::*;

    #[test]
    fn reveal_offset_is_none_when_caret_is_visible() {
        let viewport = ViewportState::from_metrics(800.0, 100.0, 400.0, 40.0);

        assert_eq!(viewport.reveal_offset(80.0, 20.0, 40.0), None);
    }

    #[test]
    fn reveal_offset_scrolls_up_with_margin() {
        let viewport = ViewportState::from_metrics(800.0, 100.0, 400.0, 120.0);

        assert_eq!(viewport.reveal_offset(80.0, 20.0, 40.0), Some(40.0));
    }

    #[test]
    fn reveal_offset_scrolls_down_with_margin() {
        let viewport = ViewportState::from_metrics(800.0, 100.0, 400.0, 20.0);

        assert_eq!(viewport.reveal_offset(120.0, 20.0, 40.0), Some(80.0));
    }

    #[test]
    fn reveal_offset_clamps_to_content_bounds() {
        let viewport = ViewportState::from_metrics(800.0, 100.0, 140.0, 0.0);

        assert_eq!(viewport.reveal_offset(150.0, 20.0, 40.0), Some(40.0));
    }

    #[test]
    fn wrap_count_and_cursor_row_stay_consistent() {
        let line = "alpha beta gamma";

        assert_eq!(visual_line_count(line, 6), 3);
        assert_eq!(cursor_visual_row_in_line(line, 0, 6), 0);
        assert_eq!(cursor_visual_row_in_line(line, 7, 6), 1);
        assert_eq!(cursor_visual_row_in_line(line, line.chars().count(), 6), 2);
    }

    #[test]
    fn long_word_cursor_row_tracks_hard_wraps() {
        let line = "abcdefghij";

        assert_eq!(visual_line_count(line, 4), 3);
        assert_eq!(cursor_visual_row_in_line(line, 3, 4), 0);
        assert_eq!(cursor_visual_row_in_line(line, 4, 4), 1);
        assert_eq!(cursor_visual_row_in_line(line, 5, 4), 1);
        assert_eq!(cursor_visual_row_in_line(line, 9, 4), 2);
    }

    #[test]
    fn tab_width_depends_on_visual_column() {
        let line = "aa\tbb";

        assert_eq!(visual_line_count(line, 4), 4);
        assert_eq!(cursor_visual_row_in_line(line, 2, 4), 1);
        assert_eq!(cursor_visual_row_in_line(line, 3, 4), 3);
        assert_eq!(cursor_visual_row_in_line(line, line.chars().count(), 4), 3);
    }

    #[test]
    fn visible_row_range_accounts_for_padding_and_partial_rows() {
        assert_eq!(visible_row_range(0.0, 40.0, 10), 0..2);
        assert_eq!(visible_row_range(28.0, 40.0, 10), 1..3);
    }

    #[test]
    fn visible_row_range_clamps_to_last_row() {
        assert_eq!(visible_row_range(500.0, 100.0, 3), 2..3);
    }
}
