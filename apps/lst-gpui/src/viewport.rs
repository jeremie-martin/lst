use gpui::{
    fill, point, px, rgb, size, App, Bounds, Pixels, ScrollHandle, ShapedLine, SharedString,
    TextRun, Window,
};
use lst_core::wrap::{
    cursor_visual_row_in_line, visual_line_count, wrap_columns_with_gutter, wrap_segments,
    WrappedSegment,
};
use lst_ui::{
    COLOR_ACCENT, COLOR_BORDER, COLOR_CARET, COLOR_CURRENT_LINE, COLOR_GREEN, COLOR_GUTTER,
    COLOR_LAVENDER, COLOR_MAUVE, COLOR_MUTED, COLOR_PEACH, COLOR_PINK, COLOR_SAPPHIRE,
    COLOR_SELECTION, COLOR_SURFACE1, COLOR_TEXT, COLOR_YELLOW,
};
use ropey::Rope;
use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    path::PathBuf,
    rc::Rc,
    sync::LazyLock,
};
use tree_sitter_highlight::{
    Highlight as TreeSitterHighlight, HighlightConfiguration,
    HighlightEvent as TreeSitterHighlightEvent, Highlighter as TreeSitterHighlighter,
};

use crate::{
    vim, EditorTab, CODE_FONT_SIZE, CURSOR_WIDTH, EDITOR_LEFT_PAD, EDITOR_RIGHT_PAD,
    GUTTER_LEFT_PAD, GUTTER_SEPARATOR_WIDTH, GUTTER_WIDTH, ROW_HEIGHT, VIEWPORT_OVERSCAN_LINES,
    WINDOW_HEIGHT, WRAP_CHAR_WIDTH_FALLBACK,
};

const TREE_SITTER_CAPTURE_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constructor",
    "escape",
    "function",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "string",
    "type",
    "variable",
];

static TREE_SITTER_RUST_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    let mut config = HighlightConfiguration::new(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
    .expect("embedded tree-sitter Rust highlight query should be valid");
    config.configure(TREE_SITTER_CAPTURE_NAMES);
    config
});

#[derive(Clone)]
struct CachedShapedLine {
    text: SharedString,
    style_key: u64,
    shaped: ShapedLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SyntaxMode {
    Plain,
    TreeSitterRust,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SyntaxSpan {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) color: u32,
}

#[derive(Clone)]
pub(crate) struct CachedRustHighlights {
    pub(crate) revision: u64,
    pub(crate) lines: Vec<Vec<SyntaxSpan>>,
}

pub(crate) struct ViewportCache {
    code_lines: HashMap<(usize, usize, usize), CachedShapedLine>,
    gutter_lines: HashMap<usize, CachedShapedLine>,
    pub(crate) rust_highlights: Option<CachedRustHighlights>,
    pub(crate) rust_highlight_inflight_revision: Option<u64>,
    pub(crate) wrap_layout: Option<WrapLayout>,
}

impl Default for ViewportCache {
    fn default() -> Self {
        Self {
            code_lines: HashMap::new(),
            gutter_lines: HashMap::new(),
            rust_highlights: None,
            rust_highlight_inflight_revision: None,
            wrap_layout: None,
        }
    }
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
}

#[derive(Clone)]
pub(crate) struct WrapLayout {
    pub(crate) revision: u64,
    pub(crate) show_wrap: bool,
    pub(crate) wrap_columns: usize,
    pub(crate) line_row_starts: Vec<usize>,
    pub(crate) total_rows: usize,
}

pub(crate) fn buffer_content_height(visual_rows: usize) -> Pixels {
    px((visual_rows.max(1) as f32) * ROW_HEIGHT)
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

pub(crate) fn line_display_char_len(buffer: &Rope, line_ix: usize) -> usize {
    line_display_text(buffer, line_ix).chars().count()
}

pub(crate) fn syntax_mode_for_path(path: Option<&PathBuf>) -> SyntaxMode {
    match path
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
    {
        Some("rs") => SyntaxMode::TreeSitterRust,
        _ => SyntaxMode::Plain,
    }
}

fn tree_sitter_color_for_capture(index: usize) -> Option<u32> {
    match TREE_SITTER_CAPTURE_NAMES.get(index).copied() {
        Some("attribute") => Some(COLOR_YELLOW),
        Some("comment") => Some(COLOR_MUTED),
        Some("constant") => Some(COLOR_PEACH),
        Some("constructor") => Some(COLOR_SAPPHIRE),
        Some("escape") => Some(COLOR_PINK),
        Some("function") => Some(COLOR_ACCENT),
        Some("keyword") => Some(COLOR_MAUVE),
        Some("module") => Some(COLOR_LAVENDER),
        Some("number") => Some(COLOR_PEACH),
        Some("operator") => Some(COLOR_SAPPHIRE),
        Some("property") => Some(COLOR_LAVENDER),
        Some("punctuation") => Some(COLOR_BORDER),
        Some("string") => Some(COLOR_GREEN),
        Some("type") => Some(COLOR_YELLOW),
        Some("variable") => None,
        _ => None,
    }
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

fn push_rust_highlight_span(
    lines: &mut [Vec<SyntaxSpan>],
    line_starts: &[usize],
    display_ends: &[usize],
    mut start: usize,
    end: usize,
    color: u32,
) {
    while start < end {
        let line_ix = line_starts
            .partition_point(|offset| *offset <= start)
            .saturating_sub(1);
        let line_start = line_starts[line_ix];
        let display_end = display_ends[line_ix];
        let next_line_start = line_starts.get(line_ix + 1).copied().unwrap_or(end);
        let visible_end = end.min(display_end);

        if start < visible_end {
            lines[line_ix].push(SyntaxSpan {
                start: start - line_start,
                end: visible_end - line_start,
                color,
            });
        }

        if end <= next_line_start {
            break;
        }
        start = next_line_start;
    }
}

fn highlight_rust_source(source: &str) -> Vec<Vec<SyntaxSpan>> {
    let mut line_starts = vec![0usize];
    let mut display_ends = Vec::new();
    let bytes = source.as_bytes();
    let mut ix = 0usize;
    let mut line_start = 0usize;

    while ix < bytes.len() {
        if bytes[ix] == b'\n' {
            let display_end = if ix > line_start && bytes[ix - 1] == b'\r' {
                ix - 1
            } else {
                ix
            };
            display_ends.push(display_end);
            ix += 1;
            line_start = ix;
            line_starts.push(line_start);
            continue;
        }
        ix += 1;
    }
    display_ends.push(source.strip_suffix('\r').map_or(source.len(), str::len));

    let mut lines = vec![Vec::new(); line_starts.len()];
    let mut highlighter = TreeSitterHighlighter::new();
    let Ok(events) =
        highlighter.highlight(&TREE_SITTER_RUST_CONFIG, source.as_bytes(), None, |_| None)
    else {
        return lines;
    };

    let mut stack: Vec<TreeSitterHighlight> = Vec::new();
    for event in events {
        match event {
            Ok(TreeSitterHighlightEvent::HighlightStart(highlight)) => stack.push(highlight),
            Ok(TreeSitterHighlightEvent::HighlightEnd) => {
                let _ = stack.pop();
            }
            Ok(TreeSitterHighlightEvent::Source { start, end }) if start < end => {
                let Some(color) = stack
                    .last()
                    .and_then(|highlight| tree_sitter_color_for_capture(highlight.0))
                else {
                    continue;
                };
                push_rust_highlight_span(
                    &mut lines,
                    &line_starts,
                    &display_ends,
                    start,
                    end,
                    color,
                );
            }
            Ok(TreeSitterHighlightEvent::Source { .. }) => {}
            Err(_) => return vec![Vec::new(); line_starts.len()],
        }
    }

    lines
}

pub(crate) fn compute_rust_highlights(source: &str) -> Vec<Vec<SyntaxSpan>> {
    highlight_rust_source(source)
}

fn line_syntax_spans(
    cache: &mut ViewportCache,
    revision: u64,
    line_ix: usize,
    syntax_mode: SyntaxMode,
) -> Vec<SyntaxSpan> {
    match syntax_mode {
        SyntaxMode::Plain => Vec::new(),
        SyntaxMode::TreeSitterRust => cache
            .rust_highlights
            .as_ref()
            .filter(|highlights| highlights.revision == revision)
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

pub(crate) fn code_origin_pad(show_gutter: bool) -> Pixels {
    if show_gutter {
        px(GUTTER_WIDTH)
    } else {
        px(EDITOR_LEFT_PAD)
    }
}

pub(crate) fn code_char_width(window: &mut Window) -> Pixels {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let probe = SharedString::from("00000000");
    let shaped = window.text_system().shape_line(
        probe.clone(),
        font_size,
        &[TextRun {
            len: probe.len(),
            font: style.font(),
            color: rgb(COLOR_TEXT).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );

    if shaped.width > px(0.0) {
        shaped.width / probe.chars().count() as f32
    } else {
        px(WRAP_CHAR_WIDTH_FALLBACK)
    }
}

fn wrap_columns_for_viewport(
    viewport_width: Pixels,
    line_count: usize,
    char_width: Pixels,
    show_gutter: bool,
    show_wrap: bool,
) -> usize {
    if !show_wrap {
        return usize::MAX;
    }

    wrap_columns_with_gutter(
        viewport_width / px(1.0),
        (char_width / px(1.0)).max(WRAP_CHAR_WIDTH_FALLBACK),
        line_count,
        show_gutter,
        EDITOR_LEFT_PAD,
        EDITOR_RIGHT_PAD,
        GUTTER_LEFT_PAD,
        GUTTER_SEPARATOR_WIDTH,
    )
}

pub(crate) fn ensure_wrap_layout(
    cache: &mut ViewportCache,
    lines: &[String],
    revision: u64,
    viewport_width: Pixels,
    char_width: Pixels,
    show_gutter: bool,
    show_wrap: bool,
) -> WrapLayout {
    let wrap_columns = wrap_columns_for_viewport(
        viewport_width,
        lines.len(),
        char_width,
        show_gutter,
        show_wrap,
    );
    if let Some(layout) = cache.wrap_layout.as_ref() {
        if layout.revision == revision
            && layout.wrap_columns == wrap_columns
            && layout.show_wrap == show_wrap
            && layout.line_row_starts.len() == lines.len() + 1
        {
            return layout.clone();
        }
    }

    cache.code_lines.clear();

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

    let layout = WrapLayout {
        revision,
        show_wrap,
        wrap_columns,
        line_row_starts,
        total_rows: total_rows.max(1),
    };
    cache.wrap_layout = Some(layout.clone());
    layout
}

fn visible_visual_row_range(
    scroll_top: Pixels,
    viewport_height: Pixels,
    total_rows: usize,
) -> std::ops::Range<usize> {
    let start =
        ((scroll_top / px(ROW_HEIGHT)).floor() as usize).saturating_sub(VIEWPORT_OVERSCAN_LINES);
    let end = (((scroll_top + viewport_height) / px(ROW_HEIGHT)).ceil() as usize)
        .saturating_add(VIEWPORT_OVERSCAN_LINES)
        .min(total_rows.max(1));
    start..end.max(start.saturating_add(1))
}

pub(crate) fn line_for_visual_row(layout: &WrapLayout, visual_row: usize) -> usize {
    layout
        .line_row_starts
        .partition_point(|start| *start <= visual_row)
        .saturating_sub(1)
        .min(layout.line_row_starts.len().saturating_sub(2))
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
    buffer: &Rope,
    lines: &[String],
    revision: u64,
    syntax_mode: SyntaxMode,
    show_gutter: bool,
    show_wrap: bool,
    viewport_scroll: &ScrollHandle,
    viewport_cache: &Rc<RefCell<ViewportCache>>,
    viewport_geometry: &Rc<RefCell<ViewportGeometry>>,
    bounds: Bounds<Pixels>,
    char_width: Pixels,
    window: &mut Window,
) -> ViewportPaintState {
    let viewport_height = if bounds.size.height > px(0.0) {
        bounds.size.height
    } else {
        px(WINDOW_HEIGHT)
    };
    let scroll_top = {
        let offset_y = -viewport_scroll.offset().y;
        if offset_y > px(0.0) {
            offset_y
        } else {
            px(0.0)
        }
    };
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let code_run = TextRun {
        len: 0,
        font: style.font(),
        color: rgb(COLOR_TEXT).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let gutter_run = TextRun {
        len: 0,
        font: style.font(),
        color: rgb(COLOR_MUTED).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let mut cache = viewport_cache.borrow_mut();
    let layout = ensure_wrap_layout(
        &mut cache,
        lines,
        revision,
        bounds.size.width,
        char_width,
        show_gutter,
        show_wrap,
    );
    let visible_rows = visible_visual_row_range(scroll_top, viewport_height, layout.total_rows);
    let first_line = line_for_visual_row(&layout, visible_rows.start);
    let last_visible_line = line_for_visual_row(&layout, visible_rows.end.saturating_sub(1));
    cache
        .code_lines
        .retain(|(line_ix, _, _), _| *line_ix >= first_line && *line_ix <= last_visible_line);
    cache.gutter_lines.retain(|line_ix, _| {
        show_gutter && *line_ix >= first_line && *line_ix <= last_visible_line
    });

    let mut rows = Vec::new();
    for line_ix in first_line..=last_visible_line {
        let display_source = trim_display_line(&lines[line_ix]);
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

            let row_top = bounds.top() + px((visual_row as f32) * ROW_HEIGHT) - scroll_top;
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
                    SharedString::from(format!("{:>6}", line_ix + 1)),
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
    };

    ViewportPaintState { rows }
}

pub(crate) fn paint_viewport(
    bounds: Bounds<Pixels>,
    show_gutter: bool,
    selection: std::ops::Range<usize>,
    cursor_char: usize,
    vim_mode: vim::Mode,
    focused: bool,
    paint_state: ViewportPaintState,
    window: &mut Window,
    cx: &mut App,
) {
    let line_height = window.line_height();
    let gutter_origin_x = bounds.left() + px(GUTTER_LEFT_PAD);
    let gutter_width = px(GUTTER_WIDTH - GUTTER_LEFT_PAD - 8.0);
    let code_origin_x = bounds.left() + code_origin_pad(show_gutter);

    for row in paint_state.rows {
        let cursor_in_row = row_contains_cursor(&row, cursor_char);
        let row_bounds = Bounds::new(
            point(bounds.left(), row.row_top),
            size(bounds.size.width, px(ROW_HEIGHT)),
        );
        window.paint_quad(fill(
            row_bounds,
            if cursor_in_row {
                rgb(COLOR_CURRENT_LINE)
            } else {
                rgb(COLOR_SURFACE1)
            },
        ));

        if show_gutter {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left(), row.row_top),
                    size(px(GUTTER_WIDTH), px(ROW_HEIGHT)),
                ),
                rgb(COLOR_GUTTER),
            ));
        }

        if selection.start != selection.end
            && selection.end > row.line_start_char
            && selection.start < row.logical_end_char
        {
            let start = selection
                .start
                .max(row.line_start_char)
                .min(row.display_end_char);
            let end = selection.end.min(row.display_end_char);
            if end > start {
                let start_x =
                    code_origin_x + x_for_global_char(&row, start).unwrap_or_else(|| px(0.0));
                let end_x = code_origin_x + x_for_global_char(&row, end).unwrap_or_else(|| px(0.0));
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(start_x, row.row_top),
                        point(
                            end_x.max(start_x + px(CURSOR_WIDTH)),
                            row.row_top + px(ROW_HEIGHT),
                        ),
                    ),
                    rgb(COLOR_SELECTION),
                ));
            }
        }

        if let Some(gutter_line) = row.gutter_line.as_ref() {
            let gutter_x = gutter_origin_x + (gutter_width - gutter_line.width);
            let _ = gutter_line.paint(point(gutter_x, row.row_top), line_height, window, cx);
        }

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
                    .unwrap_or_else(|| cursor_x + px(CODE_FONT_SIZE * 0.55));
                (next_x - cursor_x).max(px(CURSOR_WIDTH * 2.0))
            } else {
                px(CURSOR_WIDTH)
            };
            window.paint_quad(fill(
                Bounds::new(
                    point(cursor_x, row.row_top),
                    size(cursor_width, px(ROW_HEIGHT)),
                ),
                if vim_mode == vim::Mode::Normal {
                    rgb(COLOR_SELECTION)
                } else {
                    rgb(COLOR_CARET)
                },
            ));
        }
    }
}

pub(crate) fn visual_row_for_char(tab: &EditorTab, layout: &WrapLayout) -> Option<usize> {
    let cursor = tab.cursor_char().min(tab.buffer.len_chars());
    let line = tab.buffer.char_to_line(cursor);
    let line_start = tab.buffer.line_to_char(line);
    let display_text = line_display_text(&tab.buffer, line);
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
