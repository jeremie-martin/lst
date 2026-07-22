use crate::{
    diagnostics,
    ui::theme::{metrics, typography, Theme},
};
use gpui::{fill, point, px, rgb, size, App, Bounds, Pixels, ScrollHandle, ShapedLine, SharedString, TextRun, Window};
use lst_editor::wrap::{
    build_wrap_layout, cursor_visual_row_in_line, line_for_visual_row, visual_line_count, wrap_segments, WrapLayout,
    WrappedSegment,
};
use lst_editor::{
    selection::{
        exact_text_occurrence_ranges_in_text, identifier_occurrence_ranges_in_text, is_identifier_occurrence_char,
    },
    vim, DisplayLine, EditorTab, GutterMode, Selection, SelectionSet,
};
use ropey::Rope;
use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
    time::Instant,
};

use crate::syntax::{CachedSyntaxHighlights, SyntaxInvalidation, SyntaxMode, SyntaxSpan};

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
    pub(crate) wrap_layout: Option<CachedWrapLayout>,
    max_unwrapped_line_width: Option<CachedUnwrappedLineWidth>,
    code_char_width: Option<CachedCodeCharWidth>,
    occurrence_highlights: Option<CachedOccurrenceHighlights>,
    selection_match_highlights: Option<CachedSelectionMatchHighlights>,
}

#[derive(Clone)]
struct CachedOccurrenceHighlights {
    revision: u64,
    query: String,
    scan_windows: Vec<Range<usize>>,
    ranges: Rc<[Range<usize>]>,
}

#[derive(Clone)]
struct CachedSelectionMatchHighlights {
    revision: u64,
    query: String,
    visible_windows: Vec<Range<usize>>,
    selected_ranges: Vec<Range<usize>>,
    ranges: Rc<[Range<usize>]>,
}

impl ViewportCache {
    pub(crate) fn clear_code_lines(&mut self) {
        self.code_lines.clear();
    }

    pub(crate) fn clear_shaped_lines(&mut self) {
        self.code_lines.clear();
        self.gutter_lines.clear();
    }

    /// Invalidate revision-dependent layout and shaping while retaining the
    /// previous per-line syntax snapshot. The syntax synchronizer patches the
    /// affected line window immediately after this call.
    pub(crate) fn invalidate_content_layout(&mut self) {
        self.code_lines.clear();
        self.gutter_lines.clear();
        self.max_unwrapped_line_width = None;
    }

    /// Invalidate every cache whose output depends on the configured editor
    /// font. Content edits deliberately retain the measured character width;
    /// a font-family change must not.
    pub(crate) fn invalidate_typography(&mut self) {
        self.clear_shaped_lines();
        self.max_unwrapped_line_width = None;
        self.code_char_width = None;
    }

    /// Advance a cached wrapped-row index after a same-line-topology edit.
    /// Only changed lines are remeasured; later row starts receive one cheap
    /// integer shift instead of re-tokenizing every line in the document.
    pub(crate) fn patch_wrap_layout(&mut self, buffer: &Rope, revision: u64, invalidation: &SyntaxInvalidation) {
        if invalidation.is_full() {
            self.wrap_layout = None;
            return;
        }
        let Some(mut cached) = self.wrap_layout.take() else {
            return;
        };
        let line_count = buffer.len_lines();
        if cached.layout.line_row_starts.len() != line_count.saturating_add(1) {
            return;
        }
        let lines = invalidation.line_range(line_count);
        if lines.is_empty() {
            cached.revision = revision;
            self.wrap_layout = Some(cached);
            return;
        }

        let old_end = cached.layout.line_row_starts[lines.end];
        let mut next_start = cached.layout.line_row_starts[lines.start];
        for line_ix in lines.clone() {
            cached.layout.line_row_starts[line_ix] = next_start;
            let row_count = if cached.layout.show_wrap {
                let mut line = buffer.line(line_ix).to_string();
                while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                visual_line_count(&line, cached.layout.wrap_columns)
            } else {
                1
            };
            next_start = next_start.saturating_add(row_count);
        }
        cached.layout.line_row_starts[lines.end] = next_start;
        let row_delta = next_start as isize - old_end as isize;
        for row_start in &mut cached.layout.line_row_starts[lines.end.saturating_add(1)..] {
            *row_start = row_start.saturating_add_signed(row_delta);
        }
        cached.layout.total_rows = cached.layout.total_rows.saturating_add_signed(row_delta).max(1);
        cached.revision = revision;
        self.wrap_layout = Some(cached);
    }
}

#[derive(Clone, Copy)]
struct CachedUnwrappedLineWidth {
    revision: u64,
    char_width: Pixels,
    font_size: Pixels,
    width: Pixels,
}

#[derive(Clone, Copy)]
struct CachedCodeCharWidth {
    font_size: Pixels,
    theme_key: u64,
    width: Pixels,
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
    pub(crate) gutter_text: Option<String>,
}

pub(crate) struct ViewportPaintState {
    pub(crate) rows: Vec<PaintedRow>,
    pub(crate) occurrence_highlights: Rc<[Range<usize>]>,
    pub(crate) selection_match_highlights: Rc<[Range<usize>]>,
}

#[derive(Default)]
pub(crate) struct ViewportGeometry {
    pub(crate) bounds: Option<Bounds<Pixels>>,
    /// Buffer revision the `rows` below were painted at. Geometry is carried
    /// across edits (see `invalidate_visual_state`), so consumers that read
    /// per-sample char offsets out of `rows` must check this against the
    /// active tab's current revision and treat a mismatch as "not yet
    /// painted for this content" — otherwise they compare a fresh cursor
    /// position against last revision's char ranges.
    pub(crate) painted_revision: u64,
    pub(crate) rows: Vec<PaintedRow>,
    pub(crate) scroll_top_at_paint: Pixels,
    pub(crate) scroll_left_at_paint: Pixels,
    pub(crate) painted_wrap_columns: Option<usize>,
    /// Lets the reveal handler translate logical columns to pixels without
    /// requiring a `&mut Window` to re-shape a probe line.
    pub(crate) painted_char_width: Pixels,
    /// Vertical pitch between consecutive painted rows. Captured at paint
    /// time so the state-trace channel and other consumers can convert
    /// (line, col) → pixels without `window.line_height()`.
    pub(crate) painted_row_height: Pixels,
    /// Width reserved for the gutter at paint time. Zero when line numbers
    /// are hidden. Mouse input and tracing consume this captured value rather
    /// than independently reconstructing layout.
    pub(crate) gutter_width_at_paint: Pixels,
    /// Horizontal padding before code, including the dynamic gutter when it
    /// is visible.
    pub(crate) code_origin_pad_at_paint: Pixels,
    /// Window-local x where the first code character of an unwrapped line
    /// is painted (i.e., `bounds.left() + gutter_pad - horizontal_scroll`).
    /// Captured at paint time so consumers can convert (col) → window x via
    /// `code_origin_x_at_paint + col * painted_char_width`.
    pub(crate) code_origin_x_at_paint: Pixels,
    /// Passive identifier occurrences that were visible in the prepared row
    /// window. Stored with geometry because they are observable paint state.
    pub(crate) occurrence_highlights: Rc<[Range<usize>]>,
    /// Exact matches for the current explicit selection, excluding every
    /// selected range. Stored separately from passive caret occurrences so
    /// focus and edit triggers cannot blur the two interaction contracts.
    pub(crate) selection_match_highlights: Rc<[Range<usize>]>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GutterLayout {
    pub(crate) width: Pixels,
    pub(crate) text_right: Pixels,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewportLayoutMetrics {
    gutter: Option<GutterLayout>,
    code_origin_pad: Pixels,
}

impl ViewportLayoutMetrics {
    pub(crate) fn new(show_gutter: bool, line_count: usize, char_width: Pixels, scale: f32) -> Self {
        let gutter = show_gutter.then(|| {
            let digits = decimal_digits(line_count).max(metrics::GUTTER_MIN_DIGITS);
            let left_pad = metrics::px_for_scale(metrics::GUTTER_LEFT_PAD, scale);
            let right_pad = metrics::px_for_scale(metrics::GUTTER_RIGHT_PAD, scale);
            let text_width = char_width * digits as f32;
            let width = left_pad + text_width + right_pad;
            GutterLayout {
                width,
                text_right: width - right_pad,
            }
        });
        let code_origin_pad = gutter.map_or_else(
            || metrics::px_for_scale(metrics::EDITOR_LEFT_PAD, scale),
            |gutter| gutter.width,
        );
        Self {
            gutter,
            code_origin_pad,
        }
    }

    pub(crate) fn gutter(self) -> Option<GutterLayout> {
        self.gutter
    }

    pub(crate) fn gutter_width(self) -> Pixels {
        self.gutter.map_or(px(0.0), |gutter| gutter.width)
    }

    pub(crate) fn code_origin_pad(self) -> Pixels {
        self.code_origin_pad
    }

    pub(crate) fn code_origin_x(self, element_left: Pixels, horizontal_scroll: Pixels) -> Pixels {
        element_left + self.code_origin_pad - horizontal_scroll
    }
}

fn decimal_digits(mut value: usize) -> usize {
    value = value.max(1);
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

#[derive(Clone)]
pub(crate) struct CachedWrapLayout {
    pub(crate) revision: u64,
    pub(crate) layout: WrapLayout,
}

pub(crate) struct WrapLayoutInput<'a> {
    pub(crate) lines: &'a [DisplayLine],
    pub(crate) revision: u64,
    pub(crate) viewport_width: Pixels,
    pub(crate) char_width: Pixels,
    pub(crate) layout_metrics: ViewportLayoutMetrics,
    pub(crate) show_wrap: bool,
    pub(crate) scale: f32,
}

pub(crate) struct ViewportPreparation<'a> {
    pub(crate) buffer: &'a Rope,
    pub(crate) lines: &'a [DisplayLine],
    pub(crate) revision: u64,
    pub(crate) syntax_mode: SyntaxMode,
    pub(crate) layout_metrics: ViewportLayoutMetrics,
    pub(crate) gutter_mode: GutterMode,
    pub(crate) cursor_line: usize,
    pub(crate) cursor_lines: &'a [usize],
    pub(crate) occurrence_query: Option<&'a str>,
    pub(crate) selection_match_query: Option<&'a str>,
    pub(crate) selected_match_ranges: &'a [Range<usize>],
    pub(crate) show_wrap: bool,
    pub(crate) viewport_scroll: &'a ScrollHandle,
    pub(crate) viewport_cache: &'a Rc<RefCell<ViewportCache>>,
    pub(crate) viewport_geometry: &'a Rc<RefCell<ViewportGeometry>>,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) char_width: Pixels,
    pub(crate) scale: f32,
    pub(crate) theme: Theme,
}

pub(crate) struct ViewportPaintInput<'a> {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) layout_metrics: ViewportLayoutMetrics,
    pub(crate) selection_set: SelectionSet,
    pub(crate) search_matches: &'a [Range<usize>],
    pub(crate) active_search_match: Option<&'a Range<usize>>,
    pub(crate) vim_mode: vim::Mode,
    pub(crate) focused: bool,
    pub(crate) cursor_visible: bool,
    pub(crate) drop_cursor: Option<usize>,
    pub(crate) paint_state: ViewportPaintState,
    pub(crate) scale: f32,
    pub(crate) horizontal_scroll: Pixels,
    pub(crate) theme: Theme,
}
pub(crate) fn buffer_content_height(visual_rows: usize, scale: f32) -> Pixels {
    metrics::px_for_scale((visual_rows.max(1) as f32) * metrics::row_height(), scale)
}

/// GPUI's `ScrollHandle::offset()` is negative when scrolled away from the
/// origin; these helpers return non-negative "pixels scrolled from the edge."
pub(crate) fn scroll_top_for(scroll: &ScrollHandle) -> Pixels {
    (-scroll.offset().y).max(px(0.0))
}
pub(crate) fn scroll_left_for(scroll: &ScrollHandle) -> Pixels {
    (-scroll.offset().x).max(px(0.0))
}
pub(crate) fn max_scroll_top(scroll: &ScrollHandle) -> Pixels {
    scroll.max_offset().height.max(px(0.0))
}
pub(crate) fn max_scroll_left(scroll: &ScrollHandle) -> Pixels {
    scroll.max_offset().width.max(px(0.0))
}

/// Sets scroll position, clamped to `[0, max_offset]`, preserving the other axis.
pub(crate) fn scroll_to_top(scroll: &ScrollHandle, top: Pixels) {
    let top = top.max(px(0.0)).min(max_scroll_top(scroll));
    scroll.set_offset(gpui::point(scroll.offset().x, -top));
}

pub(crate) fn scroll_to_left(scroll: &ScrollHandle, left: Pixels) {
    let left = left.max(px(0.0)).min(max_scroll_left(scroll));
    scroll.set_offset(gpui::point(-left, scroll.offset().y));
}
pub(crate) fn reset_scroll(scroll: &ScrollHandle) {
    scroll.set_offset(gpui::point(px(0.0), px(0.0)));
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
    text.char_indices().nth(char_ix).map(|(b, _)| b).unwrap_or(text.len())
}

/// Returns cached syntax spans for `line_ix` only when (a) the cache holds
/// highlights for the active language and (b) the line's display-byte
/// length is unchanged from the snapshot the cache was built against. The
/// byte-length guard is what lets the renderer keep showing correct
/// highlights for unedited lines after an edit invalidates the cache for
/// the lines the user actually touched, instead of blanking the whole
/// document until the next parse lands.
fn line_syntax_spans(
    cache: &mut ViewportCache,
    line_ix: usize,
    current_line_byte_len: usize,
    syntax_mode: SyntaxMode,
) -> Vec<SyntaxSpan> {
    match syntax_mode {
        SyntaxMode::Plain => Vec::new(),
        SyntaxMode::TreeSitter(language) => cache
            .syntax_highlights
            .as_ref()
            .filter(|highlights| highlights.language == language)
            .filter(|highlights| {
                highlights
                    .line_byte_lens
                    .get(line_ix)
                    .copied()
                    .is_some_and(|len| len as usize == current_line_byte_len)
            })
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
    theme: Theme,
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
                role: span.role,
            });
        }
    }

    let mut hasher = DefaultHasher::new();
    theme.style_key().hash(&mut hasher);
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
            color: rgb(theme.syntax.color(span.role)).into(),
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

pub(crate) fn code_char_width(cache: &mut ViewportCache, window: &mut Window, scale: f32, theme: Theme) -> Pixels {
    let font_size = metrics::px_for_scale(metrics::code_font_size(), scale);
    let theme_key = theme.style_key();
    if let Some(cached) = cache.code_char_width {
        if cached.font_size == font_size && cached.theme_key == theme_key {
            return cached.width;
        }
    }

    let font = typography::primary_font();
    let probe = SharedString::from("00000000");
    let shaped = window.text_system().shape_line(
        probe.clone(),
        font_size,
        &[TextRun {
            len: probe.len(),
            font,
            color: rgb(theme.role.text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );

    let width = if shaped.width > px(0.0) {
        shaped.width / probe.chars().count() as f32
    } else {
        metrics::px_for_scale(metrics::WRAP_CHAR_WIDTH_FALLBACK, scale)
    };
    cache.code_char_width = Some(CachedCodeCharWidth {
        font_size,
        theme_key,
        width,
    });
    width
}

pub(crate) fn x_for_display_char(
    line_text: &str,
    char_offset: usize,
    char_width: Pixels,
    scale: f32,
    theme: Theme,
    window: &mut Window,
) -> Pixels {
    let line_text = trim_display_line(line_text);
    let char_offset = char_offset.min(line_text.chars().count());
    if is_plain_monospace_text(line_text) {
        return char_width * char_offset as f32;
    }

    let Some(shaped) = shape_display_line(line_text, scale, theme, window) else {
        return px(0.0);
    };
    let byte = char_to_byte(line_text, char_offset);
    shaped.x_for_index(byte)
}

pub(crate) fn max_unwrapped_line_width(
    cache: &mut ViewportCache,
    lines: &[DisplayLine],
    revision: u64,
    char_width: Pixels,
    scale: f32,
    theme: Theme,
    window: &mut Window,
) -> Pixels {
    let font_size = metrics::px_for_scale(metrics::code_font_size(), scale);
    if let Some(cached) = cache.max_unwrapped_line_width {
        if cached.revision == revision && cached.char_width == char_width && cached.font_size == font_size {
            return cached.width;
        }
    }

    let mut width = px(0.0);
    for line in lines {
        let display_line = trim_display_line(line);
        let line_width = if is_plain_monospace_text(display_line) {
            char_width * display_line.chars().count() as f32
        } else {
            shape_display_line(display_line, scale, theme, window).map_or(px(0.0), |line| line.width)
        };
        width = width.max(line_width);
    }

    cache.max_unwrapped_line_width = Some(CachedUnwrappedLineWidth {
        revision,
        char_width,
        font_size,
        width,
    });
    width
}

fn is_plain_monospace_text(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii() && byte != b'\t')
}

fn shape_display_line(text: &str, scale: f32, theme: Theme, window: &mut Window) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    let font_size = metrics::px_for_scale(metrics::code_font_size(), scale);
    let text = SharedString::from(text.to_string());
    let font = typography::primary_font();
    Some(window.text_system().shape_line(
        text.clone(),
        font_size,
        &[TextRun {
            len: text.len(),
            font,
            color: rgb(theme.role.text).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    ))
}

fn wrap_columns_for_viewport(
    viewport_width: Pixels,
    char_width: Pixels,
    layout_metrics: ViewportLayoutMetrics,
    show_wrap: bool,
    scale: f32,
) -> usize {
    if !show_wrap {
        return usize::MAX;
    }

    let content_width =
        (viewport_width - layout_metrics.code_origin_pad() - metrics::px_for_scale(metrics::CURSOR_WIDTH, scale))
            .max(px(1.0));
    let char_width = (char_width / px(1.0)).max(metrics::WRAP_CHAR_WIDTH_FALLBACK * scale);
    ((content_width / px(1.0)) / char_width).floor().max(1.0) as usize
}

pub(crate) fn ensure_wrap_layout(cache: &mut ViewportCache, input: WrapLayoutInput<'_>) -> WrapLayout {
    let WrapLayoutInput {
        lines,
        revision,
        viewport_width,
        char_width,
        layout_metrics,
        show_wrap,
        scale,
    } = input;
    let wrap_columns = wrap_columns_for_viewport(viewport_width, char_width, layout_metrics, show_wrap, scale);
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

    let started = diagnostics::trace_enabled().then(Instant::now);
    let layout = build_wrap_layout(lines, wrap_columns, show_wrap);
    if let Some(started) = started {
        diagnostics::record_ms("wrap_layout_ms", started.elapsed().as_secs_f64() * 1000.0);
    }
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
    let start = ((scroll_top / row_height).floor() as usize).saturating_sub(metrics::VIEWPORT_OVERSCAN_LINES);
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

    let shaped = window.text_system().shape_line(text.clone(), font_size, runs, None);

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

fn expand_identifier_window(buffer: &Rope, mut window: Range<usize>) -> Range<usize> {
    window.start = window.start.min(buffer.len_chars());
    window.end = window.end.min(buffer.len_chars()).max(window.start);
    let prefix_chars = buffer
        .chars_at(window.start)
        .reversed()
        .take_while(|ch| is_identifier_occurrence_char(*ch))
        .count();
    let suffix_chars = buffer
        .chars_at(window.end)
        .take_while(|ch| is_identifier_occurrence_char(*ch))
        .count();
    window.start -= prefix_chars;
    window.end += suffix_chars;
    window
}

fn push_merged_window(windows: &mut Vec<Range<usize>>, window: Range<usize>) {
    if window.is_empty() {
        return;
    }
    if let Some(previous) = windows.last_mut() {
        if window.start <= previous.end {
            previous.end = previous.end.max(window.end);
            return;
        }
    }
    windows.push(window);
}

fn painted_character_windows(
    rows: &[PaintedRow],
    bounds: Bounds<Pixels>,
    layout_metrics: ViewportLayoutMetrics,
    horizontal_scroll: Pixels,
) -> Vec<Range<usize>> {
    let code_origin_x = layout_metrics.code_origin_x(bounds.left(), horizontal_scroll);
    let code_clip_left = bounds.left() + layout_metrics.gutter_width();
    let visible_start_x = (code_clip_left - code_origin_x).max(px(0.0));
    let visible_end_x = (bounds.right() - code_origin_x).max(visible_start_x);
    let mut windows: Vec<Range<usize>> = Vec::new();

    for row in rows {
        let Some(code_line) = row.code_line.as_ref() else {
            continue;
        };
        let text = code_line.text.as_ref();
        let text_len_chars = text.chars().count();
        let local_start = byte_index_to_char(text, code_line.closest_index_for_x(visible_start_x)).saturating_sub(1);
        let local_end = byte_index_to_char(text, code_line.closest_index_for_x(visible_end_x))
            .saturating_add(1)
            .min(text_len_chars);
        push_merged_window(
            &mut windows,
            row.line_start_char + local_start..row.line_start_char + local_end,
        );
    }

    windows
}

fn occurrence_scan_windows(buffer: &Rope, visible_windows: &[Range<usize>]) -> Vec<Range<usize>> {
    // Wrapped rows commonly describe adjacent pieces of the same logical
    // line. Merge those visible pieces before walking to identifier
    // boundaries so one pathological identifier is traversed once, not once
    // per painted row.
    let mut expanded = Vec::with_capacity(visible_windows.len());
    for window in visible_windows {
        push_merged_window(&mut expanded, expand_identifier_window(buffer, window.clone()));
    }
    expanded
}

fn text_match_scan_windows(buffer_len: usize, visible_windows: &[Range<usize>], query_len: usize) -> Vec<Range<usize>> {
    let overlap = query_len.saturating_sub(1);
    let mut expanded = Vec::with_capacity(visible_windows.len());
    for window in visible_windows {
        push_merged_window(
            &mut expanded,
            window.start.saturating_sub(overlap)..window.end.saturating_add(overlap).min(buffer_len),
        );
    }
    expanded
}

fn overlaps_any_sorted(candidate: &Range<usize>, ranges: &[Range<usize>]) -> bool {
    let index = ranges.partition_point(|range| range.end <= candidate.start);
    ranges
        .get(index)
        .is_some_and(|range| range.start < candidate.end && candidate.start < range.end)
}

fn visible_occurrence_highlights(
    cache: &mut ViewportCache,
    buffer: &Rope,
    revision: u64,
    query: Option<&str>,
    scan_windows: Vec<Range<usize>>,
) -> Rc<[Range<usize>]> {
    let Some(query) = query else {
        return Rc::from([]);
    };
    if let Some(cached) = cache.occurrence_highlights.as_ref() {
        if cached.revision == revision && cached.query == query && cached.scan_windows == scan_windows {
            return cached.ranges.clone();
        }
    }

    let started = diagnostics::trace_enabled().then(Instant::now);
    let mut ranges = Vec::new();
    for window in &scan_windows {
        let text = buffer.slice(window.clone()).to_string();
        ranges.extend(
            identifier_occurrence_ranges_in_text(&text, query)
                .into_iter()
                .map(|range| window.start + range.start..window.start + range.end),
        );
    }
    if let Some(started) = started {
        diagnostics::record_ms("occurrence_highlight_ms", started.elapsed().as_secs_f64() * 1000.0);
    }
    let ranges: Rc<[Range<usize>]> = ranges.into();
    cache.occurrence_highlights = Some(CachedOccurrenceHighlights {
        revision,
        query: query.to_string(),
        scan_windows,
        ranges: ranges.clone(),
    });
    ranges
}

fn visible_selection_match_highlights(
    cache: &mut ViewportCache,
    buffer: &Rope,
    revision: u64,
    query: Option<&str>,
    selected_ranges: &[Range<usize>],
    visible_windows: Vec<Range<usize>>,
) -> Rc<[Range<usize>]> {
    let Some(query) = query else {
        return Rc::from([]);
    };
    if let Some(cached) = cache.selection_match_highlights.as_ref() {
        if cached.revision == revision
            && cached.query == query
            && cached.visible_windows == visible_windows
            && cached.selected_ranges == selected_ranges
        {
            return cached.ranges.clone();
        }
    }

    let started = diagnostics::trace_enabled().then(Instant::now);
    let scan_windows = text_match_scan_windows(buffer.len_chars(), &visible_windows, query.chars().count());
    let mut ranges = Vec::new();
    for window in &scan_windows {
        let text = buffer.slice(window.clone()).to_string();
        ranges.extend(
            exact_text_occurrence_ranges_in_text(&text, query)
                .into_iter()
                .map(|range| window.start + range.start..window.start + range.end)
                .filter(|range| overlaps_any_sorted(range, &visible_windows))
                .filter(|range| !overlaps_any_sorted(range, selected_ranges)),
        );
    }
    if let Some(started) = started {
        diagnostics::record_ms("selection_match_highlight_ms", started.elapsed().as_secs_f64() * 1000.0);
    }
    let ranges: Rc<[Range<usize>]> = ranges.into();
    cache.selection_match_highlights = Some(CachedSelectionMatchHighlights {
        revision,
        query: query.to_string(),
        visible_windows,
        selected_ranges: selected_ranges.to_vec(),
        ranges: ranges.clone(),
    });
    ranges
}

pub(crate) fn prepare_viewport_paint_state(input: ViewportPreparation<'_>, window: &mut Window) -> ViewportPaintState {
    let ViewportPreparation {
        buffer,
        lines,
        revision,
        syntax_mode,
        layout_metrics,
        gutter_mode,
        cursor_line,
        cursor_lines,
        occurrence_query,
        selection_match_query,
        selected_match_ranges,
        show_wrap,
        viewport_scroll,
        viewport_cache,
        viewport_geometry,
        bounds,
        char_width,
        scale,
        theme,
    } = input;
    let show_gutter = layout_metrics.gutter().is_some();
    let row_height = metrics::px_for_scale(metrics::row_height(), scale);
    let viewport_height = if bounds.size.height > px(0.0) {
        bounds.size.height
    } else {
        metrics::px_for_scale(metrics::WINDOW_HEIGHT, scale)
    };
    let scroll_top = scroll_top_for(viewport_scroll);
    let scroll_left = scroll_left_for(viewport_scroll);
    let font_size = metrics::px_for_scale(metrics::code_font_size(), scale);
    let font = typography::primary_font();
    let code_run = TextRun {
        len: 0,
        font: font.clone(),
        color: rgb(theme.role.text).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let gutter_muted_run = TextRun {
        len: 0,
        font,
        color: rgb(theme.role.text_muted).into(),
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
            layout_metrics,
            show_wrap,
            scale,
        },
    );
    let visible_rows = visible_visual_row_range(scroll_top, viewport_height, layout.total_rows, row_height);
    let first_line = line_for_visual_row(&layout, visible_rows.start);
    let last_visible_line = line_for_visual_row(&layout, visible_rows.end.saturating_sub(1));
    cache
        .code_lines
        .retain(|(line_ix, _, _), _| *line_ix >= first_line && *line_ix <= last_visible_line);
    cache
        .gutter_lines
        .retain(|line_ix, _| show_gutter && *line_ix >= first_line && *line_ix <= last_visible_line);

    let mut rows = Vec::new();
    for (line_ix, line) in lines
        .iter()
        .enumerate()
        .take(last_visible_line.saturating_add(1))
        .skip(first_line)
    {
        let display_source = trim_display_line(line);
        let highlight_spans = line_syntax_spans(&mut cache, line_ix, display_source.len(), syntax_mode);
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
                theme,
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
            let gutter_text = if show_gutter && segment_ix == 0 {
                Some(gutter_mode.format(line_ix, cursor_line, cursor_lines))
            } else {
                None
            };
            let gutter_line = if let Some(gutter_text) = gutter_text.as_ref() {
                let cursor_line_number = cursor_lines.binary_search(&line_ix).is_ok();
                let gutter_run = TextRun {
                    color: rgb(if cursor_line_number {
                        theme.role.text
                    } else {
                        theme.role.text_muted
                    })
                    .into(),
                    ..gutter_muted_run.clone()
                };
                shape_cached_line(
                    &mut cache.gutter_lines,
                    line_ix,
                    SharedString::from(gutter_text.clone()),
                    theme.style_key() * 2 + u64::from(cursor_line_number),
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
                cursor_end_inclusive: segment_ix + 1 == segment_count && logical_end_char == segment_end_char,
                code_line,
                gutter_line,
                gutter_text,
            });
        }
    }

    let visible_windows = painted_character_windows(&rows, bounds, layout_metrics, scroll_left);
    let occurrence_highlights = visible_occurrence_highlights(
        &mut cache,
        buffer,
        revision,
        occurrence_query,
        occurrence_scan_windows(buffer, &visible_windows),
    );
    let selection_match_highlights = visible_selection_match_highlights(
        &mut cache,
        buffer,
        revision,
        selection_match_query,
        selected_match_ranges,
        visible_windows,
    );

    *viewport_geometry.borrow_mut() = ViewportGeometry {
        bounds: Some(bounds),
        painted_revision: revision,
        rows: rows.clone(),
        scroll_top_at_paint: scroll_top,
        scroll_left_at_paint: scroll_left,
        painted_wrap_columns: show_wrap.then_some(layout.wrap_columns),
        painted_char_width: char_width,
        painted_row_height: row_height,
        gutter_width_at_paint: layout_metrics.gutter_width(),
        code_origin_pad_at_paint: layout_metrics.code_origin_pad(),
        code_origin_x_at_paint: layout_metrics.code_origin_x(bounds.left(), scroll_left),
        occurrence_highlights: occurrence_highlights.clone(),
        selection_match_highlights: selection_match_highlights.clone(),
    };

    ViewportPaintState {
        rows,
        occurrence_highlights,
        selection_match_highlights,
    }
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
    if range.start == range.end || range.end <= row.line_start_char || range.start >= row.logical_end_char {
        return;
    }

    let start = range.start.max(row.line_start_char).min(row.display_end_char);
    let end = range.end.min(row.display_end_char);
    let selects_line_ending = row.logical_end_char > row.display_end_char
        && range.start <= row.display_end_char
        && range.end > row.display_end_char;
    if end <= start && !selects_line_ending {
        return;
    }

    let start_x = code_origin_x + x_for_global_char(row, start).unwrap_or_else(|| px(0.0));
    let mut end_x = code_origin_x + x_for_global_char(row, end).unwrap_or_else(|| px(0.0));
    if selects_line_ending {
        end_x += metrics::px_for_scale(metrics::code_font_size() * 0.55, scale);
    }
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

/// Slices the (document-order, non-overlapping) `items` down to those whose
/// char range overlaps `row`'s painted span. Shared by search matches and
/// selections, both of which uphold that ordering invariant.
fn items_overlapping_row<'a, T>(items: &'a [T], row: &PaintedRow, range_of: impl Fn(&T) -> Range<usize>) -> &'a [T] {
    let first = items.partition_point(|item| range_of(item).end <= row.line_start_char);
    let last = first + items[first..].partition_point(|item| range_of(item).start < row.logical_end_char);
    &items[first..last]
}

// One entry per selection. Collapsed cursors take the wide block-cursor in
// Vim Normal; selections with extent get a thin caret on their head over
// the existing range fill.
struct PaintCursor {
    char: usize,
    collapsed: bool,
    primary: bool,
}

fn paint_cursors(selection_set: &SelectionSet) -> Vec<PaintCursor> {
    selection_set
        .as_slice()
        .iter()
        .enumerate()
        .map(|(index, selection)| PaintCursor {
            char: selection.cursor(),
            collapsed: !selection.has_selection(),
            primary: index == selection_set.primary_index(),
        })
        .collect()
}

pub(crate) fn paint_viewport(input: ViewportPaintInput<'_>, window: &mut Window, cx: &mut App) {
    let ViewportPaintInput {
        bounds,
        layout_metrics,
        selection_set,
        search_matches,
        active_search_match,
        vim_mode,
        focused,
        cursor_visible,
        drop_cursor,
        paint_state,
        scale,
        horizontal_scroll,
        theme,
    } = input;
    let ViewportPaintState {
        rows,
        occurrence_highlights,
        selection_match_highlights,
    } = paint_state;
    let line_height = window.line_height();
    let row_height = metrics::px_for_scale(metrics::row_height(), scale);
    let gutter = layout_metrics.gutter();
    let code_origin_x = layout_metrics.code_origin_x(bounds.left(), horizontal_scroll);
    let selections = selection_set.as_slice();
    let cursors = paint_cursors(&selection_set);

    for row in rows {
        let selection_head_in_row = cursors.iter().any(|cursor| row_contains_cursor(&row, cursor.char));
        if selection_head_in_row {
            let highlight_left = bounds.left() + layout_metrics.gutter_width();
            window.paint_quad(fill(
                Bounds::new(
                    point(highlight_left, row.row_top),
                    size((bounds.right() - highlight_left).max(px(0.0)), row_height),
                ),
                rgb(if focused {
                    theme.role.current_line_bg
                } else {
                    theme.role.current_line_inactive_bg
                }),
            ));
        }

        for occurrence in items_overlapping_row(occurrence_highlights.as_ref(), &row, Clone::clone) {
            paint_range_background(
                &row,
                occurrence,
                code_origin_x,
                row_height,
                scale,
                theme.role.occurrence_match_bg,
                window,
            );
        }

        for selection_match in items_overlapping_row(selection_match_highlights.as_ref(), &row, Clone::clone) {
            paint_range_background(
                &row,
                selection_match,
                code_origin_x,
                row_height,
                scale,
                theme.role.selection_match_bg,
                window,
            );
        }

        for search_match in items_overlapping_row(search_matches, &row, Clone::clone) {
            paint_range_background(
                &row,
                search_match,
                code_origin_x,
                row_height,
                scale,
                theme.role.search_match_bg,
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
                theme.role.search_active_match_bg,
                window,
            );
        }

        for selection in items_overlapping_row(selections, &row, Selection::range) {
            paint_range_background(
                &row,
                &selection.range(),
                code_origin_x,
                row_height,
                scale,
                if focused {
                    theme.role.selection_bg
                } else {
                    theme.role.selection_inactive_bg
                },
                window,
            );
        }

        if let Some(code_line) = row.code_line.as_ref() {
            let _ = code_line.paint(point(code_origin_x, row.row_top), line_height, window, cx);
        }

        if focused && cursor_visible {
            for cursor in cursors.iter().filter(|cursor| row_contains_cursor(&row, cursor.char)) {
                let cursor_char = cursor.char;
                let block_cursor = vim_mode == vim::Mode::Normal && cursor.collapsed;
                let cursor_x = code_origin_x
                    + x_for_global_char(&row, cursor_char.min(row.display_end_char)).unwrap_or_else(|| px(0.0));
                let cursor_width = if block_cursor {
                    let next_x = code_origin_x
                        + x_for_global_char(&row, (cursor_char + 1).min(row.display_end_char.max(cursor_char + 1)))
                            .unwrap_or_else(|| {
                                cursor_x + metrics::px_for_scale(metrics::code_font_size() * 0.55, scale)
                            });
                    (next_x - cursor_x).max(metrics::px_for_scale(metrics::CURSOR_WIDTH * 2.0, scale))
                } else {
                    metrics::px_for_scale(metrics::CURSOR_WIDTH, scale)
                };
                window.paint_quad(fill(
                    Bounds::new(point(cursor_x, row.row_top), size(cursor_width, row_height)),
                    if block_cursor {
                        rgb(if cursor.primary {
                            theme.role.selection_bg
                        } else {
                            theme.role.selection_inactive_bg
                        })
                    } else {
                        rgb(if cursor.primary {
                            theme.role.caret
                        } else {
                            theme.role.caret_secondary
                        })
                    },
                ));
            }
        }

        if let Some(drop_cursor) = drop_cursor.filter(|drop| row_contains_cursor(&row, *drop)) {
            let cursor_x = code_origin_x
                + x_for_global_char(&row, drop_cursor.min(row.display_end_char)).unwrap_or_else(|| px(0.0));
            window.paint_quad(fill(
                Bounds::new(
                    point(cursor_x, row.row_top),
                    size(metrics::px_for_scale(metrics::CURSOR_WIDTH * 2.0, scale), row_height),
                ),
                rgb(theme.role.accent),
            ));
        }

        if let Some(gutter) = gutter {
            // The gutter shares the editor color, but it still owns a fixed
            // occlusion layer. Without this fill, horizontally scrolled text
            // and decorations can paint underneath the line numbers.
            window.paint_quad(fill(
                Bounds::new(point(bounds.left(), row.row_top), size(gutter.width, row_height)),
                rgb(theme.role.editor_bg),
            ));
            if let Some(gutter_line) = row.gutter_line.as_ref() {
                let gutter_x = bounds.left() + gutter.text_right - gutter_line.width;
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
    let column = cursor.saturating_sub(line_start).min(display_text.chars().count());
    let row_in_line = if layout.show_wrap {
        cursor_visual_row_in_line(display_text.as_ref(), column, layout.wrap_columns)
    } else {
        0
    };
    layout.line_row_starts.get(line).copied().map(|row| row + row_in_line)
}

pub(crate) fn row_contains_cursor(row: &PaintedRow, cursor_char: usize) -> bool {
    if cursor_char < row.line_start_char {
        return false;
    }
    cursor_char < row.logical_end_char || (row.cursor_end_inclusive && cursor_char == row.logical_end_char)
}

pub(crate) fn x_for_global_char(row: &PaintedRow, global_char: usize) -> Option<Pixels> {
    let local_char = global_char.saturating_sub(row.line_start_char);
    let code_line = row.code_line.as_ref()?;
    Some(code_line.x_for_index(char_to_byte(code_line.text.as_ref(), local_char)))
}
fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}
pub(crate) fn byte_index_to_char(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{CachedSyntaxHighlights, SyntaxLanguage};
    use crate::ui::theme::SyntaxRole;

    fn cache_with_highlights(line_byte_lens: Vec<u32>, lines: Vec<Vec<SyntaxSpan>>) -> ViewportCache {
        ViewportCache {
            syntax_highlights: Some(CachedSyntaxHighlights {
                language: SyntaxLanguage::Rust,
                revision: 0,
                lines,
                line_byte_lens,
            }),
            ..Default::default()
        }
    }

    fn rust_keyword_span(start: usize, end: usize) -> SyntaxSpan {
        SyntaxSpan {
            start,
            end,
            role: SyntaxRole::Keyword,
        }
    }

    #[test]
    fn gutter_layout_is_stable_through_three_digits_and_grows_by_one_digit() {
        let char_width = px(8.0);
        let one = ViewportLayoutMetrics::new(true, 1, char_width, 1.0);
        let nine_ninety_nine = ViewportLayoutMetrics::new(true, 999, char_width, 1.0);
        let one_thousand = ViewportLayoutMetrics::new(true, 1_000, char_width, 1.0);

        assert_eq!(one.gutter_width(), nine_ninety_nine.gutter_width());
        assert_eq!(
            one_thousand.gutter_width() - nine_ninety_nine.gutter_width(),
            char_width
        );
    }

    #[test]
    fn identifier_scan_windows_expand_only_to_the_surrounding_identifier() {
        let buffer = Rope::from_str("prefix alpha_suffix omega");

        assert_eq!(expand_identifier_window(&buffer, 10..12), 7..19);
    }

    #[test]
    fn adjacent_wrapped_windows_merge_before_identifier_expansion() {
        let buffer = Rope::from_str(&"a".repeat(30_000));
        let mut windows = Vec::new();
        for start in (0..7_224).step_by(168) {
            push_merged_window(&mut windows, start..(start + 168));
        }

        assert_eq!(windows, vec![0..7_224]);
        assert_eq!(expand_identifier_window(&buffer, windows.remove(0)), 0..30_000);
    }

    #[test]
    fn typography_invalidation_discards_the_measured_character_width() {
        let mut cache = ViewportCache {
            code_char_width: Some(CachedCodeCharWidth {
                font_size: px(13.0),
                theme_key: 1,
                width: px(8.0),
            }),
            ..ViewportCache::default()
        };

        cache.invalidate_typography();

        assert!(cache.code_char_width.is_none());
    }

    #[test]
    fn line_syntax_spans_returns_cached_when_byte_length_matches() {
        // Cached state for two lines: line 0 has 7 bytes with a keyword on
        // bytes 0..2, line 1 has 11 bytes with no spans.
        let line_lens = vec![7u32, 11u32];
        let line_spans = vec![vec![rust_keyword_span(0, 2)], Vec::new()];
        let mut cache = cache_with_highlights(line_lens, line_spans);

        let spans = line_syntax_spans(&mut cache, 0, 7, SyntaxMode::TreeSitter(SyntaxLanguage::Rust));
        assert_eq!(spans, vec![rust_keyword_span(0, 2)]);
    }

    #[test]
    fn line_syntax_spans_returns_empty_when_byte_length_differs() {
        // Cached state recorded length 7, but the line has grown to 8 (user
        // typed a character). Reusing the stale span at byte offset 0..2 is
        // safe in terms of bytes, but the byte-length-mismatch guard is what
        // forces a blank-and-reparse for the edited line so a later split
        // never falls inside a multi-byte UTF-8 character.
        let mut cache = cache_with_highlights(vec![7u32], vec![vec![rust_keyword_span(0, 2)]]);
        let spans = line_syntax_spans(&mut cache, 0, 8, SyntaxMode::TreeSitter(SyntaxLanguage::Rust));
        assert!(spans.is_empty(), "stale-length cache must return empty spans");
    }

    #[test]
    fn line_syntax_spans_returns_empty_when_language_differs() {
        let mut cache = cache_with_highlights(vec![7u32], vec![vec![rust_keyword_span(0, 2)]]);
        let spans = line_syntax_spans(&mut cache, 0, 7, SyntaxMode::TreeSitter(SyntaxLanguage::Python));
        assert!(spans.is_empty(), "language switch must invalidate cached spans");
    }

    #[test]
    fn line_syntax_spans_returns_empty_when_mode_is_plain() {
        let mut cache = cache_with_highlights(vec![7u32], vec![vec![rust_keyword_span(0, 2)]]);
        let spans = line_syntax_spans(&mut cache, 0, 7, SyntaxMode::Plain);
        assert!(spans.is_empty());
    }

    #[test]
    fn line_syntax_spans_handles_line_beyond_cache() {
        let mut cache = cache_with_highlights(vec![7u32], vec![vec![rust_keyword_span(0, 2)]]);
        let spans = line_syntax_spans(&mut cache, 9, 0, SyntaxMode::TreeSitter(SyntaxLanguage::Rust));
        assert!(spans.is_empty());
    }

    #[test]
    fn patched_wrap_layout_matches_a_full_rebuild() {
        let before = (0..200)
            .map(|line| format!("line {line} has enough words to wrap across rows"))
            .collect::<Vec<_>>()
            .join("\n");
        let after = before.replacen("line 100", "line 100 with substantially more content", 1);
        let before_lines = before.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        let after_lines = after.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
        let mut cache = ViewportCache {
            wrap_layout: Some(CachedWrapLayout {
                revision: 0,
                layout: build_wrap_layout(&before_lines, 12, true),
            }),
            ..Default::default()
        };

        cache.patch_wrap_layout(&Rope::from_str(&after), 1, &SyntaxInvalidation::Lines(99..102));

        let patched = cache.wrap_layout.expect("patch should retain layout");
        assert_eq!(patched.revision, 1);
        assert_eq!(patched.layout, build_wrap_layout(&after_lines, 12, true));
    }
}
