use crate::ui::theme::{metrics, role, typography};
use gpui::{
    fill, point, px, rgb, size, App, Bounds, Pixels, ScrollHandle, ShapedLine, SharedString,
    TextRun, Window,
};
use lst_editor::wrap::{
    build_wrap_layout, cursor_visual_row_in_line, line_for_visual_row, wrap_segments, WrapLayout,
    WrappedSegment,
};
use lst_editor::{vim, EditorTab};
use ropey::Rope;
use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
};

use crate::syntax::{CachedSyntaxHighlights, SyntaxMode, SyntaxSpan};

#[derive(Clone)]
struct CachedShapedLine {
    text: SharedString,
    style_key: u64,
    shaped: ShapedLine,
}

#[derive(Default)]
pub(crate) struct ViewportCache {
    code_lines: HashMap<(usize, usize, usize), CachedShapedLine>,
    gutter_lines: HashMap<usize, CachedShapedLine>,
    pub(crate) syntax_highlights: Option<CachedSyntaxHighlights>,
    pub(crate) syntax_highlight_inflight: Option<crate::syntax::SyntaxHighlightJobKey>,
    pub(crate) wrap_layout: Option<CachedWrapLayout>,
    pub(crate) max_line_chars: Option<(u64, usize)>,
}

impl ViewportCache {
    pub(crate) fn clear_code_lines(&mut self) {
        self.code_lines.clear();
    }
}

#[derive(Clone)]
pub(crate) struct PaintedRow {
    pub(crate) row_top: Pixels,
    pub(crate) line_start_char: usize,
    pub(crate) display_end_char: usize,
    pub(crate) logical_end_char: usize,
    pub(crate) cursor_end_inclusive: bool,
    pub(crate) code_line: Option<ShapedLine>,
    pub(crate) gutter_line: Option<ShapedLine>,
}

pub(crate) struct ViewportPaintState {
    pub(crate) rows: Vec<PaintedRow>,
}

#[derive(Default)]
pub(crate) struct ViewportGeometry {
    pub(crate) bounds: Option<Bounds<Pixels>>,
    pub(crate) rows: Vec<PaintedRow>,
    pub(crate) scroll_top_at_paint: Pixels,
    pub(crate) painted_wrap_columns: Option<usize>,
    /// Lets the reveal handler translate logical columns to pixels without
    /// requiring a `&mut Window` to re-shape a probe line.
    pub(crate) painted_char_width: Pixels,
}

#[derive(Clone)]
pub(crate) struct CachedWrapLayout {
    pub(crate) revision: u64,
    pub(crate) layout: WrapLayout,
}

pub(crate) struct WrapLayoutInput<'a> {
    pub(crate) lines: &'a [String],
    pub(crate) revision: u64,
    pub(crate) viewport_width: Pixels,
    pub(crate) char_width: Pixels,
    pub(crate) show_gutter: bool,
    pub(crate) show_wrap: bool,
    pub(crate) scale: f32,
}

pub(crate) struct ViewportPreparation<'a> {
    pub(crate) buffer: &'a Rope,
    pub(crate) lines: &'a [String],
    pub(crate) revision: u64,
    pub(crate) syntax_mode: SyntaxMode,
    pub(crate) show_gutter: bool,
    pub(crate) show_wrap: bool,
    pub(crate) viewport_scroll: &'a ScrollHandle,
    pub(crate) viewport_cache: &'a Rc<RefCell<ViewportCache>>,
    pub(crate) viewport_geometry: &'a Rc<RefCell<ViewportGeometry>>,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) char_width: Pixels,
    pub(crate) scale: f32,
}

pub(crate) struct ViewportPaintInput<'a> {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) show_gutter: bool,
    pub(crate) selection: Range<usize>,
    pub(crate) search_matches: &'a [Range<usize>],
    pub(crate) active_search_match: Option<&'a Range<usize>>,
    pub(crate) cursor_char: usize,
    pub(crate) vim_mode: vim::Mode,
    pub(crate) focused: bool,
    pub(crate) paint_state: ViewportPaintState,
    pub(crate) scale: f32,
    pub(crate) horizontal_scroll: Pixels,
}

pub(crate) fn buffer_content_height(visual_rows: usize, scale: f32) -> Pixels {
    metrics::px_for_scale((visual_rows.max(1) as f32) * metrics::ROW_HEIGHT, scale)
}

/// GPUI's `ScrollHandle::offset().y` is negative when scrolled away from the
/// top; this helper returns the non-negative "pixels scrolled from the top."
pub(crate) fn scroll_top_for(scroll: &ScrollHandle) -> Pixels {
    (-scroll.offset().y).max(px(0.0))
}

/// Mirror of `scroll_top_for` for the horizontal axis: returns non-negative
/// "pixels scrolled from the left."
pub(crate) fn scroll_left_for(scroll: &ScrollHandle) -> Pixels {
    (-scroll.offset().x).max(px(0.0))
}

fn trim_display_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

pub(crate) fn line_display_text(buffer: &Rope, line_ix: usize) -> SharedString {
    let mut line = buffer.line(line_ix).to_string();
    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    SharedString::from(line)
}

fn char_to_byte_index(text: &str, char_ix: usize) -> usize {
    if char_ix == 0 {
        return 0;
    }

    text.char_indices()
        .nth(char_ix)
        .map(|(byte_ix, _)| byte_ix)
        .unwrap_or(text.len())
}

fn line_syntax_spans(
    cache: &mut ViewportCache,
    revision: u64,
    line_ix: usize,
    syntax_mode: SyntaxMode,
) -> Vec<SyntaxSpan> {
    match syntax_mode {
        SyntaxMode::Plain => Vec::new(),
        SyntaxMode::TreeSitter(language) => cache
            .syntax_highlights
            .as_ref()
            .filter(|highlights| highlights.revision == revision && highlights.language == language)
            .and_then(|highlights| highlights.lines.get(line_ix))
            .cloned()
            .unwrap_or_default(),
    }
}

fn text_runs_for_segment(
    line_text: &str,
    segment_start_col: usize,
    segment_end_col: usize,
    spans: &[SyntaxSpan],
    base_run: &TextRun,
) -> (Vec<TextRun>, u64) {
    let segment_start = char_to_byte_index(line_text, segment_start_col);
    let segment_end = char_to_byte_index(line_text, segment_end_col);
    let segment_len = segment_end.saturating_sub(segment_start);

    let mut local_spans = Vec::new();
    for span in spans {
        let start = span.start.max(segment_start);
        let end = span.end.min(segment_end);
        if start < end {
            local_spans.push(SyntaxSpan {
                start: start - segment_start,
                end: end - segment_start,
                color: span.color,
            });
        }
    }

    let mut hasher = DefaultHasher::new();
    local_spans.hash(&mut hasher);
    let style_key = hasher.finish();

    let mut runs = Vec::new();
    let mut cursor = 0;
    for span in local_spans {
        if cursor < span.start {
            runs.push(TextRun {
                len: span.start - cursor,
                ..base_run.clone()
            });
        }
        runs.push(TextRun {
            len: span.end - span.start,
            color: rgb(span.color).into(),
            ..base_run.clone()
        });
        cursor = span.end;
    }

    if cursor < segment_len {
        runs.push(TextRun {
            len: segment_len - cursor,
            ..base_run.clone()
        });
    }

    if runs.is_empty() {
        runs.push(TextRun {
            len: segment_len,
            ..base_run.clone()
        });
    }

    (runs, style_key)
}

pub(crate) fn code_origin_pad(show_gutter: bool, scale: f32) -> Pixels {
    if show_gutter {
        metrics::px_for_scale(metrics::GUTTER_WIDTH, scale)
    } else {
        metrics::px_for_scale(metrics::EDITOR_LEFT_PAD, scale)
    }
}

pub(crate) fn code_char_width(window: &mut Window, scale: f32) -> Pixels {
    let font_size = metrics::px_for_scale(metrics::CODE_FONT_SIZE, scale);
    let font = typography::primary_font();
    let probe = SharedString::from("00000000");
    let shaped = window.text_system().shape_line(
        probe.clone(),
        font_size,
        &[TextRun {
            len: probe.len(),
            font,
            color: rgb(role::TEXT).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );

    if shaped.width > px(0.0) {
        shaped.width / probe.chars().count() as f32
    } else {
        metrics::px_for_scale(metrics::WRAP_CHAR_WIDTH_FALLBACK, scale)
    }
}

fn wrap_columns_for_viewport(
    viewport_width: Pixels,
    char_width: Pixels,
    show_gutter: bool,
    show_wrap: bool,
    scale: f32,
) -> usize {
    if !show_wrap {
        return usize::MAX;
    }

    let content_width = (viewport_width
        - code_origin_pad(show_gutter, scale)
        - metrics::px_for_scale(metrics::CURSOR_WIDTH, scale))
    .max(px(1.0));
    let char_width = (char_width / px(1.0)).max(metrics::WRAP_CHAR_WIDTH_FALLBACK * scale);
    ((content_width / px(1.0)) / char_width).floor().max(1.0) as usize
}

pub(crate) fn ensure_wrap_layout(
    cache: &mut ViewportCache,
    input: WrapLayoutInput<'_>,
) -> WrapLayout {
    let WrapLayoutInput {
        lines,
        revision,
        viewport_width,
        char_width,
        show_gutter,
        show_wrap,
        scale,
    } = input;
    let wrap_columns =
        wrap_columns_for_viewport(viewport_width, char_width, show_gutter, show_wrap, scale);
    if let Some(layout) = cache.wrap_layout.as_ref() {
        if layout.revision == revision
            && layout.layout.wrap_columns == wrap_columns
            && layout.layout.show_wrap == show_wrap
            && layout.layout.line_row_starts.len() == lines.len() + 1
        {
            return layout.layout.clone();
        }
    }

    cache.code_lines.clear();

    let layout = build_wrap_layout(lines, wrap_columns, show_wrap);
    cache.wrap_layout = Some(CachedWrapLayout {
        revision,
        layout: layout.clone(),
    });
    layout
}

fn visible_visual_row_range(
    scroll_top: Pixels,
    viewport_height: Pixels,
    total_rows: usize,
    row_height: Pixels,
) -> std::ops::Range<usize> {
    let start = ((scroll_top / row_height).floor() as usize)
        .saturating_sub(metrics::VIEWPORT_OVERSCAN_LINES);
    let end = (((scroll_top + viewport_height) / row_height).ceil() as usize)
        .saturating_add(metrics::VIEWPORT_OVERSCAN_LINES)
        .min(total_rows.max(1));
    start..end.max(start.saturating_add(1))
}

fn shape_cached_line(
    cache: &mut HashMap<usize, CachedShapedLine>,
    line_ix: usize,
    text: SharedString,
    style_key: u64,
    base_run: &TextRun,
    font_size: Pixels,
    window: &mut Window,
) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(&line_ix) {
        if cached.text == text && cached.style_key == style_key {
            return Some(cached.shaped.clone());
        }
    }

    let shaped = window.text_system().shape_line(
        text.clone(),
        font_size,
        &[TextRun {
            len: text.len(),
            ..base_run.clone()
        }],
        None,
    );

    cache.insert(
        line_ix,
        CachedShapedLine {
            text,
            style_key,
            shaped: shaped.clone(),
        },
    );
    Some(shaped)
}

fn shape_cached_segment(
    cache: &mut HashMap<(usize, usize, usize), CachedShapedLine>,
    key: (usize, usize, usize),
    text: SharedString,
    runs: &[TextRun],
    style_key: u64,
    font_size: Pixels,
    window: &mut Window,
) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(&key) {
        if cached.text == text && cached.style_key == style_key {
            return Some(cached.shaped.clone());
        }
    }

    let shaped = window
        .text_system()
        .shape_line(text.clone(), font_size, runs, None);

    cache.insert(
        key,
        CachedShapedLine {
            text,
            style_key,
            shaped: shaped.clone(),
        },
    );
    Some(shaped)
}

pub(crate) fn prepare_viewport_paint_state(
    input: ViewportPreparation<'_>,
    window: &mut Window,
) -> ViewportPaintState {
    let ViewportPreparation {
        buffer,
        lines,
        revision,
        syntax_mode,
        show_gutter,
        show_wrap,
        viewport_scroll,
        viewport_cache,
        viewport_geometry,
        bounds,
        char_width,
        scale,
    } = input;
    let row_height = metrics::px_for_scale(metrics::ROW_HEIGHT, scale);
    let viewport_height = if bounds.size.height > px(0.0) {
        bounds.size.height
    } else {
        metrics::px_for_scale(metrics::WINDOW_HEIGHT, scale)
    };
    let scroll_top = scroll_top_for(viewport_scroll);
    let font_size = metrics::px_for_scale(metrics::CODE_FONT_SIZE, scale);
    let font = typography::primary_font();
    let code_run = TextRun {
        len: 0,
        font: font.clone(),
        color: rgb(role::TEXT).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let gutter_run = TextRun {
        len: 0,
        font,
        color: rgb(role::TEXT_MUTED).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let mut cache = viewport_cache.borrow_mut();
    let layout = ensure_wrap_layout(
        &mut cache,
        WrapLayoutInput {
            lines,
            revision,
            viewport_width: bounds.size.width,
            char_width,
            show_gutter,
            show_wrap,
            scale,
        },
    );
    let visible_rows =
        visible_visual_row_range(scroll_top, viewport_height, layout.total_rows, row_height);
    let first_line = line_for_visual_row(&layout, visible_rows.start);
    let last_visible_line = line_for_visual_row(&layout, visible_rows.end.saturating_sub(1));
    cache
        .code_lines
        .retain(|(line_ix, _, _), _| *line_ix >= first_line && *line_ix <= last_visible_line);
    cache.gutter_lines.retain(|line_ix, _| {
        show_gutter && *line_ix >= first_line && *line_ix <= last_visible_line
    });

    let mut rows = Vec::new();
    for (line_ix, line) in lines
        .iter()
        .enumerate()
        .take(last_visible_line.saturating_add(1))
        .skip(first_line)
    {
        let display_source = trim_display_line(line);
        let highlight_spans = line_syntax_spans(&mut cache, revision, line_ix, syntax_mode);
        let display_len = display_source.chars().count();
        let logical_end_char = if line_ix + 1 < buffer.len_lines() {
            buffer.line_to_char(line_ix + 1)
        } else {
            buffer.len_chars()
        };
        let line_start_char = buffer.line_to_char(line_ix);
        let segments = if show_wrap {
            wrap_segments(display_source, layout.wrap_columns)
        } else {
            vec![WrappedSegment {
                start_col: 0,
                end_col: display_len,
                text: display_source.to_string(),
            }]
        };
        let segment_count = segments.len();

        for (segment_ix, segment) in segments.into_iter().enumerate() {
            let visual_row = layout.line_row_starts[line_ix] + segment_ix;
            if !visible_rows.contains(&visual_row) {
                continue;
            }

            let row_top = bounds.top() + row_height * visual_row as f32 - scroll_top;
            let segment_start_char = line_start_char + segment.start_col;
            let segment_end_char = line_start_char + segment.end_col;
            let (code_runs, style_key) = text_runs_for_segment(
                display_source,
                segment.start_col,
                segment.end_col,
                &highlight_spans,
                &code_run,
            );
            let code_line = shape_cached_segment(
                &mut cache.code_lines,
                (line_ix, segment.start_col, segment.end_col),
                SharedString::from(segment.text),
                &code_runs,
                style_key,
                font_size,
                window,
            );
            let gutter_line = if show_gutter && segment_ix == 0 {
                shape_cached_line(
                    &mut cache.gutter_lines,
                    line_ix,
                    SharedString::from(format!("{:>3}", line_ix + 1)),
                    0,
                    &gutter_run,
                    font_size,
                    window,
                )
            } else {
                None
            };

            rows.push(PaintedRow {
                row_top,
                line_start_char: segment_start_char,
                display_end_char: segment_end_char,
                logical_end_char: if segment_ix + 1 == segment_count {
                    logical_end_char
                } else {
                    segment_end_char
                },
                cursor_end_inclusive: segment_ix + 1 == segment_count
                    && logical_end_char == segment_end_char,
                code_line,
                gutter_line,
            });
        }
    }

    *viewport_geometry.borrow_mut() = ViewportGeometry {
        bounds: Some(bounds),
        rows: rows.clone(),
        scroll_top_at_paint: scroll_top,
        painted_wrap_columns: show_wrap.then_some(layout.wrap_columns),
        painted_char_width: char_width,
    };

    ViewportPaintState { rows }
}

fn paint_range_background(
    row: &PaintedRow,
    range: &Range<usize>,
    code_origin_x: Pixels,
    row_height: Pixels,
    scale: f32,
    color: u32,
    window: &mut Window,
) {
    if range.start == range.end
        || range.end <= row.line_start_char
        || range.start >= row.logical_end_char
    {
        return;
    }

    let start = range
        .start
        .max(row.line_start_char)
        .min(row.display_end_char);
    let end = range.end.min(row.display_end_char);
    if end <= start {
        return;
    }

    let start_x = code_origin_x + x_for_global_char(row, start).unwrap_or_else(|| px(0.0));
    let end_x = code_origin_x + x_for_global_char(row, end).unwrap_or_else(|| px(0.0));
    window.paint_quad(fill(
        Bounds::from_corners(
            point(start_x, row.row_top),
            point(
                end_x.max(start_x + metrics::px_for_scale(metrics::CURSOR_WIDTH, scale)),
                row.row_top + row_height,
            ),
        ),
        rgb(color),
    ));
}

fn search_matches_for_row<'a>(
    search_matches: &'a [Range<usize>],
    row: &PaintedRow,
) -> &'a [Range<usize>] {
    // FindState emits document-order, non-overlapping ranges.
    let first = search_matches.partition_point(|range| range.end <= row.line_start_char);
    let last =
        first + search_matches[first..].partition_point(|range| range.start < row.logical_end_char);
    &search_matches[first..last]
}

pub(crate) fn paint_viewport(input: ViewportPaintInput<'_>, window: &mut Window, cx: &mut App) {
    let ViewportPaintInput {
        bounds,
        show_gutter,
        selection,
        search_matches,
        active_search_match,
        cursor_char,
        vim_mode,
        focused,
        paint_state,
        scale,
        horizontal_scroll,
    } = input;
    let line_height = window.line_height();
    let row_height = metrics::px_for_scale(metrics::ROW_HEIGHT, scale);
    let gutter_origin_x = bounds.left() + metrics::px_for_scale(metrics::GUTTER_LEFT_PAD, scale);
    let gutter_width = metrics::px_for_scale(
        metrics::GUTTER_WIDTH - metrics::GUTTER_LEFT_PAD - 8.0,
        scale,
    );
    let code_origin_x = bounds.left() + code_origin_pad(show_gutter, scale) - horizontal_scroll;

    for row in paint_state.rows {
        let cursor_in_row = row_contains_cursor(&row, cursor_char);
        let row_bounds = Bounds::new(
            point(bounds.left(), row.row_top),
            size(bounds.size.width, row_height),
        );
        window.paint_quad(fill(
            row_bounds,
            if cursor_in_row {
                rgb(role::CURRENT_LINE_BG)
            } else {
                rgb(role::EDITOR_BG)
            },
        ));

        for search_match in search_matches_for_row(search_matches, &row) {
            paint_range_background(
                &row,
                search_match,
                code_origin_x,
                row_height,
                scale,
                role::SEARCH_MATCH_BG,
                window,
            );
        }

        if let Some(active_search_match) = active_search_match {
            paint_range_background(
                &row,
                active_search_match,
                code_origin_x,
                row_height,
                scale,
                role::SEARCH_ACTIVE_MATCH_BG,
                window,
            );
        }

        paint_range_background(
            &row,
            &selection,
            code_origin_x,
            row_height,
            scale,
            role::SELECTION_BG,
            window,
        );

        if let Some(code_line) = row.code_line.as_ref() {
            let _ = code_line.paint(point(code_origin_x, row.row_top), line_height, window, cx);
        }

        if focused && selection.start == selection.end && cursor_in_row {
            let cursor_x = code_origin_x
                + x_for_global_char(&row, cursor_char.min(row.display_end_char))
                    .unwrap_or_else(|| px(0.0));
            let cursor_width = if vim_mode == vim::Mode::Normal {
                let next_x = code_origin_x
                    + x_for_global_char(
                        &row,
                        (cursor_char + 1).min(row.display_end_char.max(cursor_char + 1)),
                    )
                    .unwrap_or_else(|| {
                        cursor_x + metrics::px_for_scale(metrics::CODE_FONT_SIZE * 0.55, scale)
                    });
                (next_x - cursor_x).max(metrics::px_for_scale(metrics::CURSOR_WIDTH * 2.0, scale))
            } else {
                metrics::px_for_scale(metrics::CURSOR_WIDTH, scale)
            };
            window.paint_quad(fill(
                Bounds::new(point(cursor_x, row.row_top), size(cursor_width, row_height)),
                if vim_mode == vim::Mode::Normal {
                    rgb(role::SELECTION_BG)
                } else {
                    rgb(role::CARET)
                },
            ));
        }

        if show_gutter {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left(), row.row_top),
                    size(
                        metrics::px_for_scale(metrics::GUTTER_WIDTH, scale),
                        row_height,
                    ),
                ),
                rgb(role::GUTTER_BG),
            ));
            if let Some(gutter_line) = row.gutter_line.as_ref() {
                let gutter_x = gutter_origin_x + (gutter_width - gutter_line.width);
                let _ = gutter_line.paint(point(gutter_x, row.row_top), line_height, window, cx);
            }
        }
    }
}

pub(crate) fn visual_row_for_char(tab: &EditorTab, layout: &WrapLayout) -> Option<usize> {
    let cursor = tab.cursor_char().min(tab.buffer().len_chars());
    let line = tab.buffer().char_to_line(cursor);
    let line_start = tab.buffer().line_to_char(line);
    let display_text = line_display_text(tab.buffer(), line);
    let column = cursor
        .saturating_sub(line_start)
        .min(display_text.chars().count());
    let row_in_line = if layout.show_wrap {
        cursor_visual_row_in_line(display_text.as_ref(), column, layout.wrap_columns)
    } else {
        0
    };
    layout
        .line_row_starts
        .get(line)
        .copied()
        .map(|row| row + row_in_line)
}

pub(crate) fn row_contains_cursor(row: &PaintedRow, cursor_char: usize) -> bool {
    if cursor_char < row.line_start_char {
        return false;
    }

    cursor_char < row.logical_end_char
        || (row.cursor_end_inclusive && cursor_char == row.logical_end_char)
}

pub(crate) fn x_for_global_char(row: &PaintedRow, global_char: usize) -> Option<Pixels> {
    let local_char = global_char.saturating_sub(row.line_start_char);
    let code_line = row.code_line.as_ref()?;
    let byte = char_to_byte(code_line.text.as_ref(), local_char);
    Some(code_line.x_for_index(byte))
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

pub(crate) fn byte_index_to_char(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_columns_match_painted_code_width_with_gutter() {
        let viewport_width = px(800.0);
        let char_width = px(8.0);

        let columns = wrap_columns_for_viewport(viewport_width, char_width, true, true, 1.0);

        assert_eq!(
            columns,
            ((800.0 - metrics::GUTTER_WIDTH - metrics::CURSOR_WIDTH) / 8.0).floor() as usize
        );
    }

    #[test]
    fn wrap_columns_match_painted_code_width_without_gutter() {
        let viewport_width = px(800.0);
        let char_width = px(8.0);

        let columns = wrap_columns_for_viewport(viewport_width, char_width, false, true, 1.0);

        assert_eq!(
            columns,
            ((800.0 - metrics::EDITOR_LEFT_PAD - metrics::CURSOR_WIDTH) / 8.0).floor() as usize
        );
    }

    #[test]
    fn wrap_columns_leave_right_slack_for_the_insert_caret() {
        let viewport_width = px(metrics::GUTTER_WIDTH + 10.0 * 8.0);
        let char_width = px(8.0);

        let columns = wrap_columns_for_viewport(viewport_width, char_width, true, true, 1.0);

        assert_eq!(columns, 9);
    }

    #[test]
    fn search_matches_for_row_slices_to_visible_char_range() {
        let matches = vec![0..2, 5..7, 10..12, 15..18, 22..25];
        let row = PaintedRow {
            row_top: px(0.0),
            line_start_char: 10,
            display_end_char: 20,
            logical_end_char: 20,
            cursor_end_inclusive: false,
            code_line: None,
            gutter_line: None,
        };

        assert_eq!(search_matches_for_row(&matches, &row), &[10..12, 15..18]);
    }

    #[test]
    fn search_matches_for_row_excludes_adjacent_ranges() {
        let matches = vec![0..5, 5..10, 10..15, 15..20];
        let row = PaintedRow {
            row_top: px(0.0),
            line_start_char: 10,
            display_end_char: 15,
            logical_end_char: 15,
            cursor_end_inclusive: false,
            code_line: None,
            gutter_line: None,
        };

        assert_eq!(search_matches_for_row(&matches, &row), &matches[2..3]);
    }
}
