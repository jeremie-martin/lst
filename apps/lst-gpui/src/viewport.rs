use crate::{
    diagnostics,
    settings::{GuideMode, MatchBracketsSetting, RenderWhitespaceSetting},
    ui::theme::{metrics, typography, Theme},
};
use gpui::{fill, point, px, rgb, size, App, Bounds, Pixels, ScrollHandle, ShapedLine, SharedString, TextRun, Window};
use lst_editor::wrap::{
    build_wrap_layout_for_rope, cursor_visual_row_in_line, line_for_visual_row, visual_line_count_for_rope_line,
    wrap_segments, WrapLayout, WrappedSegment,
};
use lst_editor::{
    selection::{
        exact_text_occurrence_ranges_in_text, identifier_occurrence_ranges_in_text, is_identifier_occurrence_char,
    },
    vim, EditorTab, GutterMode, Selection, SelectionSet,
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

use crate::syntax::{
    CachedSyntaxHighlights, StructuralPair, StructuralSnapshot, StructuralToken, SyntaxInvalidation, SyntaxMode,
    SyntaxSpan, TabSyntaxState,
};

/// Keep a bounded shaping working set around the viewport. This is large
/// enough to make short scroll reversals and page-up/page-down reuse glyph
/// layouts, while still bounding per-tab cache memory on huge documents.
const SHAPED_LINE_CACHE_MARGIN: usize = 1_024;

#[derive(Clone)]
struct CachedShapedLine {
    text: SharedString,
    style_key: u64,
    shaped: ShapedLine,
}

#[derive(Default)]
pub(crate) struct ViewportCache {
    code_lines: HashMap<(usize, usize, usize), CachedShapedLine>,
    display_lines: HashMap<usize, SharedString>,
    wrapped_lines: HashMap<(usize, Option<usize>), Rc<[WrappedSegment]>>,
    gutter_lines: HashMap<usize, CachedShapedLine>,
    pub(crate) syntax_highlights: Option<CachedSyntaxHighlights>,
    pub(crate) wrap_layout: Option<CachedWrapLayout>,
    max_unwrapped_line_width: Option<CachedUnwrappedLineWidth>,
    unwrapped_line_width_invalidation: Option<PendingLineWidthInvalidation>,
    code_char_width: Option<CachedCodeCharWidth>,
    occurrence_highlights: Option<CachedOccurrenceHighlights>,
    selection_match_highlights: Option<CachedSelectionMatchHighlights>,
    marker_lines: HashMap<usize, CachedShapedLine>,
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

    pub(crate) fn clear_code_lines_in(&mut self, lines: Range<usize>) {
        if lines.is_empty() {
            return;
        }
        self.code_lines.retain(|(line_ix, _, _), _| !lines.contains(line_ix));
    }

    pub(crate) fn clear_text_lines_in(&mut self, lines: Range<usize>) {
        if lines.is_empty() {
            return;
        }
        self.display_lines.retain(|line_ix, _| !lines.contains(line_ix));
        self.wrapped_lines.retain(|(line_ix, _), _| !lines.contains(line_ix));
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
        self.display_lines.clear();
        self.wrapped_lines.clear();
        self.max_unwrapped_line_width = None;
        self.unwrapped_line_width_invalidation = None;
        self.invalidate_content_layout_preserving_code_lines();
    }

    /// A failed parser setup falls back to plain rendering. Every cached text
    /// and layout artifact may have been produced for an older revision or
    /// language, so the fallback transition owns their full invalidation.
    pub(crate) fn invalidate_after_parser_failure(&mut self) {
        self.syntax_highlights = None;
        self.invalidate_content_layout();
        self.wrap_layout = None;
    }

    /// Invalidate revision-dependent measurements after an edit, retaining
    /// shaped code rows until syntax synchronization identifies the exact
    /// semantic line window that changed.
    pub(crate) fn invalidate_content_layout_preserving_code_lines(&mut self) {
        self.gutter_lines.clear();
    }

    /// Invalidate every cache whose output depends on the configured editor
    /// font. Content edits deliberately retain the measured character width;
    /// a font-family change must not.
    pub(crate) fn invalidate_typography(&mut self) {
        self.clear_shaped_lines();
        self.max_unwrapped_line_width = None;
        self.unwrapped_line_width_invalidation = None;
        self.code_char_width = None;
    }

    /// Preserve the horizontal extent across ordinary edits. If the previous
    /// widest line was untouched, no-wrap mode can remeasure only this small
    /// line window instead of rescanning the whole document.
    pub(crate) fn patch_unwrapped_line_width(
        &mut self,
        revision: u64,
        invalidation: &SyntaxInvalidation,
        line_count: usize,
    ) {
        if invalidation.is_full() {
            self.max_unwrapped_line_width = None;
            self.unwrapped_line_width_invalidation = None;
            return;
        }
        let Some(cached) = self.max_unwrapped_line_width else {
            return;
        };
        let lines = invalidation.line_range(line_count);
        if lines.is_empty() {
            if let Some(pending) = self.unwrapped_line_width_invalidation.as_mut() {
                pending.revision = revision;
            } else {
                self.max_unwrapped_line_width = Some(CachedUnwrappedLineWidth { revision, ..cached });
            }
            return;
        }
        match self.unwrapped_line_width_invalidation.as_mut() {
            Some(pending) if pending.base_revision == cached.revision => {
                pending.revision = revision;
                pending.lines.start = pending.lines.start.min(lines.start);
                pending.lines.end = pending.lines.end.max(lines.end);
            }
            _ => {
                self.unwrapped_line_width_invalidation = Some(PendingLineWidthInvalidation {
                    base_revision: cached.revision,
                    revision,
                    lines,
                });
            }
        }
    }

    /// Advance a cached wrapped-row index after a same-line-topology edit.
    /// Only changed lines are remeasured; later row starts receive one cheap
    /// integer shift instead of re-tokenizing every line in the document.
    pub(crate) fn patch_wrap_layout(&mut self, buffer: &Rope, revision: u64, invalidation: &SyntaxInvalidation) {
        let started = diagnostics::trace_enabled().then(Instant::now);
        self.patch_wrap_layout_inner(buffer, revision, invalidation);
        if let Some(started) = started {
            diagnostics::record_ms("wrap_layout_patch_ms", started.elapsed().as_secs_f64() * 1000.0);
        }
    }

    fn patch_wrap_layout_inner(&mut self, buffer: &Rope, revision: u64, invalidation: &SyntaxInvalidation) {
        if invalidation.is_full() {
            self.wrap_layout = None;
            return;
        }
        let Some(mut cached) = self.wrap_layout.take() else {
            return;
        };
        let layout = Rc::make_mut(&mut cached.layout);
        let line_count = buffer.len_lines();
        if layout.line_row_starts.len() != line_count.saturating_add(1) {
            return;
        }
        let lines = invalidation.line_range(line_count);
        if lines.is_empty() {
            cached.revision = revision;
            self.wrap_layout = Some(cached);
            return;
        }

        let old_end = layout.line_row_starts[lines.end];
        let mut next_start = layout.line_row_starts[lines.start];
        for line_ix in lines.clone() {
            layout.line_row_starts[line_ix] = next_start;
            let row_count = if layout.show_wrap {
                visual_line_count_for_rope_line(buffer.line(line_ix), layout.wrap_columns)
            } else {
                1
            };
            next_start = next_start.saturating_add(row_count);
        }
        layout.line_row_starts[lines.end] = next_start;
        let row_delta = next_start as isize - old_end as isize;
        for row_start in &mut layout.line_row_starts[lines.end.saturating_add(1)..] {
            *row_start = row_start.saturating_add_signed(row_delta);
        }
        layout.total_rows = layout.total_rows.saturating_add_signed(row_delta).max(1);
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
    line_ix: usize,
}

struct PendingLineWidthInvalidation {
    base_revision: u64,
    revision: u64,
    lines: Range<usize>,
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
    pub(crate) rows: Rc<[PaintedRow]>,
    pub(crate) occurrence_highlights: Rc<[Range<usize>]>,
    pub(crate) selection_match_highlights: Rc<[Range<usize>]>,
    structure: StructurePaintState,
}

#[derive(Clone, Default)]
struct StructurePaintState {
    bracket_matches: Rc<[Range<usize>]>,
    guides: Rc<[PaintGuide]>,
    markers: Rc<[PaintMarker]>,
}

#[derive(Clone)]
struct PaintGuide {
    row_top: Pixels,
    start_column: f32,
    end_column: f32,
    horizontal: bool,
    active: bool,
}

#[derive(Clone)]
struct PaintMarker {
    at: usize,
    shaped: ShapedLine,
    whitespace: bool,
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
    pub(crate) rows: Rc<[PaintedRow]>,
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
    pub(crate) bracket_matches: Rc<[Range<usize>]>,
    pub(crate) structural_pair_count: usize,
    pub(crate) unmatched_bracket_count: usize,
    pub(crate) guide_count: usize,
    pub(crate) whitespace_marker_count: usize,
    pub(crate) control_marker_count: usize,
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
    pub(crate) layout: Rc<WrapLayout>,
}

pub(crate) struct WrapLayoutInput<'a> {
    pub(crate) buffer: &'a Rope,
    pub(crate) revision: u64,
    pub(crate) viewport_width: Pixels,
    pub(crate) char_width: Pixels,
    pub(crate) layout_metrics: ViewportLayoutMetrics,
    pub(crate) show_wrap: bool,
    pub(crate) scale: f32,
}

pub(crate) struct ViewportPreparation<'a> {
    pub(crate) buffer: &'a Rope,
    pub(crate) revision: u64,
    pub(crate) syntax_mode: SyntaxMode,
    pub(crate) syntax_state: Option<&'a TabSyntaxState>,
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
    pub(crate) selection_set: &'a SelectionSet,
    pub(crate) structure: &'a StructuralSnapshot,
    pub(crate) match_brackets: MatchBracketsSetting,
    pub(crate) bracket_pair_colorization: bool,
    pub(crate) bracket_pair_guides: GuideMode,
    pub(crate) bracket_pair_horizontal_guides: GuideMode,
    pub(crate) indent_guides: bool,
    pub(crate) highlight_active_indent_guide: bool,
    pub(crate) indent_width: usize,
    pub(crate) render_whitespace: RenderWhitespaceSetting,
    pub(crate) render_control_characters: bool,
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
    pub(crate) rulers: &'a [u16],
    pub(crate) char_width: Pixels,
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

fn cached_line_display_text(cache: &mut ViewportCache, buffer: &Rope, line_ix: usize) -> SharedString {
    cache
        .display_lines
        .entry(line_ix)
        .or_insert_with(|| line_display_text(buffer, line_ix))
        .clone()
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
            .filter(|highlights| highlights.valid_lines.get(line_ix).copied().unwrap_or(false))
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

pub(crate) fn ensure_syntax_cache_for_lines(
    cache: &mut ViewportCache,
    state: &TabSyntaxState,
    revision: u64,
    requested: Range<usize>,
) {
    let Some(highlights) = cache.syntax_highlights.as_ref() else {
        return;
    };
    if highlights.language != state.language
        || highlights.revision != revision
        || state.revision != revision
        || highlights.lines.len() != highlights.valid_lines.len()
    {
        return;
    }

    let line_count = highlights.lines.len();
    let requested = requested.start.min(line_count)..requested.end.min(line_count);
    let mut missing = Vec::new();
    let mut line = requested.start;
    while line < requested.end {
        if highlights.valid_lines[line] {
            line += 1;
            continue;
        }
        let start = line;
        line += 1;
        while line < requested.end && !highlights.valid_lines[line] {
            line += 1;
        }
        missing.push(start..line);
    }
    if missing.is_empty() {
        return;
    }

    let started = diagnostics::trace_enabled().then(Instant::now);
    let highlights = cache
        .syntax_highlights
        .as_mut()
        .expect("syntax cache was validated above");
    for range in missing {
        let (lines, line_byte_lens) = state.compute_spans_for_lines(range.clone());
        highlights.lines[range.clone()].clone_from_slice(&lines);
        highlights.line_byte_lens[range.clone()].clone_from_slice(&line_byte_lens);
        highlights.valid_lines[range].fill(true);
    }
    if let Some(started) = started {
        diagnostics::record_ms("syntax_highlight_cache_ms", started.elapsed().as_secs_f64() * 1000.0);
    }
}

fn bracket_role(token: StructuralToken) -> crate::ui::theme::SyntaxRole {
    use crate::ui::theme::SyntaxRole;
    if !token.matched {
        return SyntaxRole::Error;
    }
    const ROLES: [SyntaxRole; 6] = [
        SyntaxRole::Function,
        SyntaxRole::Emphasis,
        SyntaxRole::Type,
        SyntaxRole::Keyword,
        SyntaxRole::String,
        SyntaxRole::Constant,
    ];
    ROLES[usize::from(token.depth) % ROLES.len()]
}

fn overlay_bracket_spans(
    line_text: &str,
    line_start_char: usize,
    spans: Vec<SyntaxSpan>,
    structure: &StructuralSnapshot,
) -> Vec<SyntaxSpan> {
    let line_chars = line_text.chars().count();
    let line_end_char = line_start_char + line_chars;
    let first = structure.token_index_at_or_after(line_start_char);
    let last = structure.token_index_at_or_after(line_end_char);
    if first == last {
        return spans;
    }

    let mut overrides = Vec::with_capacity(last - first);
    let mut span_index = 0usize;
    for token_index in first..last {
        let token = structure.tokens[token_index];
        let local_char = structure.token_position(token_index) - line_start_char;
        let start = char_to_byte_index(line_text, local_char);
        let end = char_to_byte_index(line_text, local_char + 1);
        while span_index < spans.len() && spans[span_index].end <= start {
            span_index += 1;
        }
        let existing = spans
            .get(span_index)
            .filter(|span| span.start <= start && start < span.end)
            .map(|span| span.role);
        if existing.is_none_or(|role| {
            matches!(
                role,
                crate::ui::theme::SyntaxRole::Punctuation | crate::ui::theme::SyntaxRole::Operator
            )
        }) {
            overrides.push(SyntaxSpan {
                start,
                end,
                role: bracket_role(token),
            });
        }
    }
    if overrides.is_empty() {
        return spans;
    }

    let mut merged = Vec::with_capacity(spans.len() + overrides.len() * 2);
    let mut override_index = 0usize;
    for span in spans {
        while override_index < overrides.len() && overrides[override_index].end <= span.start {
            push_syntax_span(&mut merged, overrides[override_index].clone());
            override_index += 1;
        }
        let mut cursor = span.start;
        while override_index < overrides.len() && overrides[override_index].start < span.end {
            let override_span = &overrides[override_index];
            if cursor < override_span.start {
                push_syntax_span(
                    &mut merged,
                    SyntaxSpan {
                        start: cursor,
                        end: override_span.start,
                        role: span.role,
                    },
                );
            }
            push_syntax_span(&mut merged, override_span.clone());
            cursor = override_span.end;
            override_index += 1;
        }
        if cursor < span.end {
            push_syntax_span(
                &mut merged,
                SyntaxSpan {
                    start: cursor,
                    end: span.end,
                    role: span.role,
                },
            );
        }
    }
    for override_span in overrides.into_iter().skip(override_index) {
        push_syntax_span(&mut merged, override_span);
    }
    merged
}

fn push_syntax_span(spans: &mut Vec<SyntaxSpan>, span: SyntaxSpan) {
    if let Some(previous) = spans
        .last_mut()
        .filter(|previous| previous.end == span.start && previous.role == span.role)
    {
        previous.end = span.end;
    } else {
        spans.push(span);
    }
}

fn text_runs_for_segment(
    line_text: &str,
    segment_start_col: usize,
    segment_end_col: usize,
    spans: &[SyntaxSpan],
    base_run: &TextRun,
    theme: Theme,
) -> Vec<TextRun> {
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

    runs
}

fn code_style_key(theme: Theme, syntax_mode: SyntaxMode, bracket_pair_colorization: bool) -> u64 {
    let mut hasher = DefaultHasher::new();
    theme.style_key().hash(&mut hasher);
    syntax_mode.hash(&mut hasher);
    bracket_pair_colorization.hash(&mut hasher);
    hasher.finish()
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
    buffer: &Rope,
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
        if cached.char_width == char_width
            && cached.font_size == font_size
            && cache.unwrapped_line_width_invalidation.as_ref().is_some_and(|pending| {
                pending.base_revision == cached.revision
                    && pending.revision == revision
                    && !pending.lines.contains(&cached.line_ix)
            })
        {
            let pending = cache
                .unwrapped_line_width_invalidation
                .take()
                .expect("checked pending line-width invalidation");
            let mut updated = CachedUnwrappedLineWidth { revision, ..cached };
            for line_ix in pending.lines {
                let line_width = unwrapped_rope_line_width(buffer.line(line_ix), char_width, scale, theme, window);
                if line_width > updated.width {
                    updated.width = line_width;
                    updated.line_ix = line_ix;
                }
            }
            cache.max_unwrapped_line_width = Some(updated);
            return updated.width;
        }
    }

    let mut width = px(0.0);
    let mut widest_line = 0;
    for (line_ix, line) in buffer.lines().enumerate() {
        let line_width = unwrapped_rope_line_width(line, char_width, scale, theme, window);
        if line_width > width {
            width = line_width;
            widest_line = line_ix;
        }
    }

    cache.max_unwrapped_line_width = Some(CachedUnwrappedLineWidth {
        revision,
        char_width,
        font_size,
        width,
        line_ix: widest_line,
    });
    cache.unwrapped_line_width_invalidation = None;
    width
}

fn unwrapped_rope_line_width(
    line: ropey::RopeSlice<'_>,
    char_width: Pixels,
    scale: f32,
    theme: Theme,
    window: &mut Window,
) -> Pixels {
    let mut end = line.len_chars();
    while end > 0 && matches!(line.char(end - 1), '\n' | '\r') {
        end -= 1;
    }
    let display = line.slice(..end);
    if display
        .chunks()
        .all(|chunk| chunk.bytes().all(|byte| byte.is_ascii() && byte != b'\t'))
    {
        char_width * display.len_chars() as f32
    } else {
        let display = display.to_string();
        shape_display_line(&display, scale, theme, window).map_or(px(0.0), |line| line.width)
    }
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

pub(crate) fn ensure_wrap_layout(cache: &mut ViewportCache, input: WrapLayoutInput<'_>) -> Rc<WrapLayout> {
    let WrapLayoutInput {
        buffer,
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
            && layout.layout.line_row_starts.len() == buffer.len_lines() + 1
        {
            return layout.layout.clone();
        }
    }

    cache.code_lines.clear();
    cache.wrapped_lines.clear();

    let started = diagnostics::trace_enabled().then(Instant::now);
    let layout = Rc::new(build_wrap_layout_for_rope(buffer, wrap_columns, show_wrap));
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
    text: &str,
    runs: &[TextRun],
    style_key: u64,
    font_size: Pixels,
    window: &mut Window,
) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(&key) {
        if cached.text.as_ref() == text && cached.style_key == style_key {
            return Some(cached.shaped.clone());
        }
    }

    let text = SharedString::from(text.to_string());
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

fn cached_segment(
    cache: &HashMap<(usize, usize, usize), CachedShapedLine>,
    key: (usize, usize, usize),
    text: &str,
    style_key: u64,
) -> Option<ShapedLine> {
    cache
        .get(&key)
        .filter(|cached| cached.text.as_ref() == text && cached.style_key == style_key)
        .map(|cached| cached.shaped.clone())
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

fn pair_for_head(structure: &StructuralSnapshot, head: usize, enclosing: bool) -> Option<StructuralPair> {
    let touched = |at: usize| {
        structure
            .token_index_at(at)
            .and_then(|index| structure.tokens[index].pair)
            .map(|index| structure.pair(index))
    };
    touched(head)
        .or_else(|| head.checked_sub(1).and_then(touched))
        .or_else(|| {
            if !enclosing {
                return None;
            }
            let mut index = structure.pair_index_at_or_after_open(head).checked_sub(1);
            while let Some(pair_index) = index {
                let pair = structure.pair(pair_index);
                if pair.close >= head {
                    return Some(pair);
                }
                index = pair.parent;
            }
            None
        })
}

fn visible_structure_pairs(
    structure: &StructuralSnapshot,
    visible_start: usize,
    visible_end: usize,
) -> Vec<StructuralPair> {
    let first_visible_open = structure.pair_index_at_or_after_open(visible_start);
    let after_visible_open = structure.pair_index_after_open(visible_end);
    let mut indices: Vec<usize> = (first_visible_open..after_visible_open).collect();
    let mut ancestor = first_visible_open.checked_sub(1);
    while let Some(index) = ancestor {
        let pair = structure.pair(index);
        if pair.close >= visible_start {
            indices.push(index);
        }
        ancestor = pair.parent;
    }
    indices.sort_unstable();
    indices.dedup();
    indices.into_iter().map(|index| structure.pair(index)).collect()
}

fn bracket_matches(
    structure: &StructuralSnapshot,
    selections: &SelectionSet,
    mode: MatchBracketsSetting,
) -> Vec<Range<usize>> {
    if mode == MatchBracketsSetting::Never {
        return Vec::new();
    }
    let enclosing = mode == MatchBracketsSetting::Always;
    let mut ranges = Vec::new();
    for selection in selections.as_slice() {
        if let Some(pair) = pair_for_head(structure, selection.head(), enclosing) {
            ranges.push(pair.open..pair.open + 1);
            ranges.push(pair.close..pair.close + 1);
        }
    }
    ranges.sort_by_key(|range| range.start);
    ranges.dedup();
    ranges
}

fn leading_visual_column(text: &str, tab_width: usize) -> usize {
    text.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .fold(0, |column, ch| {
            if ch == '\t' {
                column + tab_width - column % tab_width
            } else {
                column + 1
            }
        })
}

fn rope_line_indent(buffer: &Rope, line_ix: usize, tab_width: usize) -> Option<usize> {
    let mut column = 0;
    for ch in buffer.line(line_ix).chars() {
        match ch {
            ' ' => column += 1,
            '\t' => column += tab_width - column % tab_width,
            ch if ch.is_whitespace() => {}
            _ => return Some(column),
        }
    }
    None
}

fn char_visual_column(text: &str, char_offset: usize, tab_width: usize) -> usize {
    text.chars().take(char_offset).fold(0, |column, ch| {
        if ch == '\t' {
            column + tab_width - column % tab_width
        } else {
            column + 1
        }
    })
}

fn control_picture(ch: char) -> Option<char> {
    match ch {
        '\0'..='\u{001f}' if !matches!(ch, '\t' | '\n' | '\r') => char::from_u32(0x2400 + u32::from(ch)),
        '\u{007f}' => Some('\u{2421}'),
        '\u{0080}'..='\u{009f}' => Some('\u{25c7}'),
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}' => Some('\u{25c7}'),
        '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => Some('\u{21c4}'),
        _ => None,
    }
}

fn whitespace_positions(text: &str, mode: RenderWhitespaceSetting) -> Vec<(usize, char)> {
    if mode == RenderWhitespaceSetting::None {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let first_non_whitespace = chars.iter().position(|ch| !matches!(ch, ' ' | '\t'));
    let last_non_whitespace = chars.iter().rposition(|ch| !matches!(ch, ' ' | '\t'));
    let mut positions = Vec::new();
    for (index, &ch) in chars.iter().enumerate() {
        if !matches!(ch, ' ' | '\t') {
            continue;
        }
        let in_run = ch == ' '
            && ((index > 0 && chars[index - 1] == ' ') || (index + 1 < chars.len() && chars[index + 1] == ' '));
        let leading = first_non_whitespace.is_none_or(|first| index < first);
        let trailing = last_non_whitespace.is_none_or(|last| index > last);
        let visible = match mode {
            RenderWhitespaceSetting::None => false,
            RenderWhitespaceSetting::Selection | RenderWhitespaceSetting::All => true,
            RenderWhitespaceSetting::Trailing => trailing,
            RenderWhitespaceSetting::Boundary => ch == '\t' || leading || trailing || in_run,
        };
        if visible {
            positions.push((index, if ch == '\t' { '\u{2192}' } else { '\u{00b7}' }));
        }
    }
    positions
}

fn selection_set_contains(selection_set: &SelectionSet, at: usize) -> bool {
    let selections = selection_set.as_slice();
    let candidate = selections.partition_point(|selection| selection.range().end <= at);
    selections
        .get(candidate)
        .is_some_and(|selection| selection.range().contains(&at))
}

pub(crate) fn prepare_viewport_paint_state(input: ViewportPreparation<'_>, window: &mut Window) -> ViewportPaintState {
    let ViewportPreparation {
        buffer,
        revision,
        syntax_mode,
        syntax_state,
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
        selection_set,
        structure,
        match_brackets,
        bracket_pair_colorization,
        bracket_pair_guides,
        bracket_pair_horizontal_guides,
        indent_guides,
        highlight_active_indent_guide,
        indent_width,
        render_whitespace,
        render_control_characters,
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
    let code_style_key = code_style_key(theme, syntax_mode, bracket_pair_colorization);

    let mut cache = viewport_cache.borrow_mut();
    let rows_started = diagnostics::trace_enabled().then(Instant::now);
    let layout = ensure_wrap_layout(
        &mut cache,
        WrapLayoutInput {
            buffer,
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
    if let Some(syntax_state) = syntax_state {
        ensure_syntax_cache_for_lines(
            &mut cache,
            syntax_state,
            revision,
            first_line..last_visible_line.saturating_add(1),
        );
    }
    let cached_first_line = first_line.saturating_sub(SHAPED_LINE_CACHE_MARGIN);
    let cached_last_line = last_visible_line.saturating_add(SHAPED_LINE_CACHE_MARGIN);
    cache
        .code_lines
        .retain(|(line_ix, _, _), _| *line_ix >= cached_first_line && *line_ix <= cached_last_line);
    cache
        .wrapped_lines
        .retain(|(line_ix, _), _| *line_ix >= cached_first_line && *line_ix <= cached_last_line);
    cache
        .display_lines
        .retain(|line_ix, _| *line_ix >= cached_first_line && *line_ix <= cached_last_line);
    cache
        .gutter_lines
        .retain(|line_ix, _| show_gutter && *line_ix >= cached_first_line && *line_ix <= cached_last_line);

    let mut rows = Vec::new();
    for line_ix in first_line..last_visible_line.saturating_add(1).min(buffer.len_lines()) {
        let line = cached_line_display_text(&mut cache, buffer, line_ix);
        let display_source = line.as_ref();
        let line_start_char = buffer.line_to_char(line_ix);
        let display_len = display_source.chars().count();
        let logical_end_char = if line_ix + 1 < buffer.len_lines() {
            buffer.line_to_char(line_ix + 1)
        } else {
            buffer.len_chars()
        };
        let segment_key = (line_ix, show_wrap.then_some(layout.wrap_columns));
        let segments = cache
            .wrapped_lines
            .entry(segment_key)
            .or_insert_with(|| {
                if show_wrap {
                    wrap_segments(display_source, layout.wrap_columns).into()
                } else {
                    Rc::from([WrappedSegment {
                        start_col: 0,
                        end_col: display_len,
                        text: display_source.to_string(),
                    }])
                }
            })
            .clone();
        let segment_count = segments.len();
        let mut highlight_spans = None;

        for (segment_ix, segment) in segments.iter().enumerate() {
            let visual_row = layout.line_row_starts[line_ix] + segment_ix;
            if !visible_rows.contains(&visual_row) {
                continue;
            }

            let row_top = bounds.top() + row_height * visual_row as f32 - scroll_top;
            let segment_start_char = line_start_char + segment.start_col;
            let segment_end_char = line_start_char + segment.end_col;
            let segment_cache_key = (line_ix, segment.start_col, segment.end_col);
            let code_line = cached_segment(&cache.code_lines, segment_cache_key, &segment.text, code_style_key)
                .or_else(|| {
                    let highlight_spans = highlight_spans.get_or_insert_with(|| {
                        let mut spans = line_syntax_spans(&mut cache, line_ix, display_source.len(), syntax_mode);
                        if bracket_pair_colorization && structure.revision == revision {
                            spans = overlay_bracket_spans(display_source, line_start_char, spans, structure);
                        }
                        spans
                    });
                    let code_runs = text_runs_for_segment(
                        display_source,
                        segment.start_col,
                        segment.end_col,
                        highlight_spans,
                        &code_run,
                        theme,
                    );
                    shape_cached_segment(
                        &mut cache.code_lines,
                        segment_cache_key,
                        &segment.text,
                        &code_runs,
                        code_style_key,
                        font_size,
                        window,
                    )
                });
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
    if let Some(started) = rows_started {
        diagnostics::record_ms("viewport_rows_ms", started.elapsed().as_secs_f64() * 1000.0);
    }

    let structure_started = diagnostics::trace_enabled().then(Instant::now);
    let structure_current = structure.revision == revision;
    let primary_head = selection_set.primary().head();
    let active_pair = structure_current
        .then(|| pair_for_head(structure, primary_head, true))
        .flatten();
    let mut guides = Vec::new();
    let mut indent_cache = HashMap::new();
    let primary_line = buffer.char_to_line(primary_head.min(buffer.len_chars()));
    let before_visible = (0..first_line)
        .rev()
        .find_map(|line| rope_line_indent(buffer, line, indent_width));
    let after_visible =
        (last_visible_line + 1..buffer.len_lines()).find_map(|line| rope_line_indent(buffer, line, indent_width));
    let raw_visible: Vec<Option<usize>> = (first_line..=last_visible_line)
        .map(|line| rope_line_indent(buffer, line, indent_width))
        .collect();
    let mut previous = before_visible;
    let mut previous_indents = Vec::with_capacity(raw_visible.len());
    for raw in &raw_visible {
        if raw.is_some() {
            previous = *raw;
        }
        previous_indents.push(previous);
    }
    let mut next = after_visible;
    let mut next_indents = vec![None; raw_visible.len()];
    for (index, raw) in raw_visible.iter().enumerate().rev() {
        if raw.is_some() {
            next = *raw;
        }
        next_indents[index] = next;
    }
    for (offset, raw) in raw_visible.into_iter().enumerate() {
        let indent = raw.unwrap_or_else(|| match (previous_indents[offset], next_indents[offset]) {
            (Some(before), Some(after)) => before.min(after),
            (Some(indent), None) | (None, Some(indent)) => indent,
            (None, None) => 0,
        });
        indent_cache.insert(first_line + offset, indent);
    }
    let primary_indent = indent_cache.get(&primary_line).copied().unwrap_or_else(|| {
        let text = cached_line_display_text(&mut cache, buffer, primary_line);
        leading_visual_column(text.as_ref(), indent_width)
    });
    if indent_guides && indent_width > 0 {
        for row in &rows {
            let line_ix = buffer.char_to_line(row.line_start_char.min(buffer.len_chars()));
            let indent = indent_cache.get(&line_ix).copied().unwrap_or(0);
            for column in (indent_width..=indent).step_by(indent_width) {
                guides.push(PaintGuide {
                    row_top: row.row_top,
                    start_column: column as f32,
                    end_column: column as f32,
                    horizontal: false,
                    active: highlight_active_indent_guide && line_ix == primary_line && column == primary_indent,
                });
            }
        }
    }

    let visible_char_start = rows.first().map_or(0, |row| row.line_start_char);
    let visible_char_end = rows.last().map_or(0, |row| row.logical_end_char);
    let visible_pairs = visible_structure_pairs(structure, visible_char_start, visible_char_end);

    let guide_pairs: Vec<(StructuralPair, bool)> = if !structure_current {
        Vec::new()
    } else {
        match bracket_pair_guides {
            GuideMode::Off => Vec::new(),
            GuideMode::Active => active_pair.into_iter().map(|pair| (pair, true)).collect(),
            GuideMode::All => visible_pairs
                .iter()
                .copied()
                .map(|pair| (pair, active_pair == Some(pair)))
                .collect(),
        }
    };
    for (pair, active) in guide_pairs {
        let open_line = buffer.char_to_line(pair.open);
        let close_line = buffer.char_to_line(pair.close);
        let open_text = cached_line_display_text(&mut cache, buffer, open_line);
        let open_column = char_visual_column(
            open_text.as_ref(),
            pair.open.saturating_sub(buffer.line_to_char(open_line)),
            indent_width,
        );
        for row in &rows {
            let line_ix = buffer.char_to_line(row.line_start_char.min(buffer.len_chars()));
            if open_line <= line_ix && line_ix <= close_line {
                guides.push(PaintGuide {
                    row_top: row.row_top,
                    start_column: open_column as f32,
                    end_column: open_column as f32,
                    horizontal: false,
                    active,
                });
            }
        }
    }

    let horizontal_pairs: Vec<(StructuralPair, bool)> = if !structure_current {
        Vec::new()
    } else {
        match bracket_pair_horizontal_guides {
            GuideMode::Off => Vec::new(),
            GuideMode::Active => active_pair.into_iter().map(|pair| (pair, true)).collect(),
            GuideMode::All => visible_pairs
                .iter()
                .copied()
                .map(|pair| (pair, active_pair == Some(pair)))
                .collect(),
        }
    };
    for (pair, active) in horizontal_pairs {
        let close_line = buffer.char_to_line(pair.close);
        let close_text = cached_line_display_text(&mut cache, buffer, close_line);
        let close_column = char_visual_column(
            close_text.as_ref(),
            pair.close.saturating_sub(buffer.line_to_char(close_line)),
            indent_width,
        );
        let open_line = buffer.char_to_line(pair.open);
        let open_text = cached_line_display_text(&mut cache, buffer, open_line);
        let open_column = char_visual_column(
            open_text.as_ref(),
            pair.open.saturating_sub(buffer.line_to_char(open_line)),
            indent_width,
        );
        for row in rows.iter().filter(|row| {
            buffer.char_to_line(row.line_start_char.min(buffer.len_chars())) == close_line
                && row_contains_cursor(row, pair.close)
        }) {
            guides.push(PaintGuide {
                row_top: row.row_top,
                start_column: open_column.min(close_column) as f32,
                end_column: open_column.max(close_column) as f32,
                horizontal: true,
                active,
            });
        }
    }

    let marker_run = TextRun {
        len: 0,
        font: typography::primary_font(),
        color: rgb(theme.role.whitespace).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut markers = Vec::new();
    for line_ix in first_line..last_visible_line.saturating_add(1).min(buffer.len_lines()) {
        let line = cached_line_display_text(&mut cache, buffer, line_ix);
        let text = line.as_ref();
        let line_start = buffer.line_to_char(line_ix);
        let mut candidates: Vec<(usize, char, bool)> = whitespace_positions(text, render_whitespace)
            .into_iter()
            .map(|(offset, glyph)| (line_start + offset, glyph, true))
            .filter(|(at, _, _)| {
                render_whitespace != RenderWhitespaceSetting::Selection || selection_set_contains(selection_set, *at)
            })
            .collect();
        if render_control_characters {
            candidates.extend(
                text.chars()
                    .enumerate()
                    .filter_map(|(offset, ch)| control_picture(ch).map(|glyph| (line_start + offset, glyph, false))),
            );
        }
        for (at, glyph, whitespace) in candidates {
            if !rows.iter().any(|row| row_contains_cursor(row, at)) {
                continue;
            }
            let text = SharedString::from(glyph.to_string());
            if let Some(shaped) = shape_cached_line(
                &mut cache.marker_lines,
                glyph as usize,
                text,
                theme.style_key(),
                &marker_run,
                font_size,
                window,
            ) {
                markers.push(PaintMarker { at, shaped, whitespace });
            }
        }
    }
    markers.sort_by_key(|marker| marker.at);
    let structural_pair_count = if structure_current { structure.pairs.len() } else { 0 };
    let unmatched_bracket_count = if structure_current {
        structure.unmatched_count()
    } else {
        0
    };
    let structure = StructurePaintState {
        bracket_matches: bracket_matches(structure, selection_set, match_brackets).into(),
        guides: guides.into(),
        markers: markers.into(),
    };
    if let Some(started) = structure_started {
        diagnostics::record_ms("structure_decorations_ms", started.elapsed().as_secs_f64() * 1000.0);
    }

    let highlights_started = diagnostics::trace_enabled().then(Instant::now);
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
    if let Some(started) = highlights_started {
        diagnostics::record_ms(
            "viewport_visible_highlights_ms",
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }

    let rows: Rc<[PaintedRow]> = rows.into();
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
        bracket_matches: structure.bracket_matches.clone(),
        structural_pair_count,
        unmatched_bracket_count,
        guide_count: structure.guides.len(),
        whitespace_marker_count: structure.markers.iter().filter(|marker| marker.whitespace).count(),
        control_marker_count: structure.markers.iter().filter(|marker| !marker.whitespace).count(),
    };

    ViewportPaintState {
        rows,
        occurrence_highlights,
        selection_match_highlights,
        structure,
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

fn range_bounds(
    row: &PaintedRow,
    range: &Range<usize>,
    code_origin_x: Pixels,
    row_height: Pixels,
    scale: f32,
) -> Option<Bounds<Pixels>> {
    if range.start == range.end || range.end <= row.line_start_char || range.start >= row.logical_end_char {
        return None;
    }
    let start = range.start.max(row.line_start_char).min(row.display_end_char);
    let end = range.end.min(row.display_end_char);
    if end <= start {
        return None;
    }
    let start_x = code_origin_x + x_for_global_char(row, start)?;
    let end_x = code_origin_x + x_for_global_char(row, end)?;
    Some(Bounds::from_corners(
        point(start_x, row.row_top),
        point(
            end_x.max(start_x + metrics::px_for_scale(metrics::CURSOR_WIDTH, scale)),
            row.row_top + row_height,
        ),
    ))
}

fn paint_outline(bounds: Bounds<Pixels>, width: Pixels, color: u32, window: &mut Window) {
    window.paint_quad(fill(
        Bounds::new(bounds.origin, size(bounds.size.width, width)),
        rgb(color),
    ));
    window.paint_quad(fill(
        Bounds::new(
            point(bounds.left(), bounds.bottom() - width),
            size(bounds.size.width, width),
        ),
        rgb(color),
    ));
    window.paint_quad(fill(
        Bounds::new(bounds.origin, size(width, bounds.size.height)),
        rgb(color),
    ));
    window.paint_quad(fill(
        Bounds::new(
            point(bounds.right() - width, bounds.top()),
            size(width, bounds.size.height),
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

fn cursors_in_row<'a>(cursors: &'a [PaintCursor], row: &PaintedRow) -> &'a [PaintCursor] {
    let first = cursors.partition_point(|cursor| cursor.char < row.line_start_char);
    let last = first
        + cursors[first..].partition_point(|cursor| {
            cursor.char < row.logical_end_char || (row.cursor_end_inclusive && cursor.char == row.logical_end_char)
        });
    &cursors[first..last]
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
        rulers,
        char_width,
    } = input;
    let ViewportPaintState {
        rows,
        occurrence_highlights,
        selection_match_highlights,
        structure,
    } = paint_state;
    let line_height = window.line_height();
    let row_height = metrics::px_for_scale(metrics::row_height(), scale);
    let gutter = layout_metrics.gutter();
    let code_origin_x = layout_metrics.code_origin_x(bounds.left(), horizontal_scroll);
    let selections = selection_set.as_slice();
    let cursors = paint_cursors(&selection_set);

    for &column in rulers {
        let x = code_origin_x + char_width * f32::from(column);
        window.paint_quad(fill(
            Bounds::new(
                point(x, bounds.top()),
                size(metrics::px_for_scale(1.0, scale), bounds.size.height),
            ),
            rgb(theme.role.ruler),
        ));
    }
    for guide in structure.guides.iter() {
        let color = if guide.active {
            theme.role.guide_active
        } else {
            theme.role.guide
        };
        if guide.horizontal {
            let start = code_origin_x + char_width * guide.start_column;
            let end = code_origin_x + char_width * guide.end_column;
            window.paint_quad(fill(
                Bounds::new(
                    point(start, guide.row_top + row_height - metrics::px_for_scale(1.0, scale)),
                    size(
                        (end - start).max(metrics::px_for_scale(1.0, scale)),
                        metrics::px_for_scale(1.0, scale),
                    ),
                ),
                rgb(color),
            ));
        } else {
            let x = code_origin_x + char_width * guide.start_column;
            window.paint_quad(fill(
                Bounds::new(
                    point(x, guide.row_top),
                    size(metrics::px_for_scale(1.0, scale), row_height),
                ),
                rgb(color),
            ));
        }
    }

    for row in rows.iter() {
        let row_cursors = cursors_in_row(&cursors, row);
        let selection_head_in_row = !row_cursors.is_empty();
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

        for occurrence in items_overlapping_row(occurrence_highlights.as_ref(), row, Clone::clone) {
            paint_range_background(
                row,
                occurrence,
                code_origin_x,
                row_height,
                scale,
                theme.role.occurrence_match_bg,
                window,
            );
        }

        for selection_match in items_overlapping_row(selection_match_highlights.as_ref(), row, Clone::clone) {
            paint_range_background(
                row,
                selection_match,
                code_origin_x,
                row_height,
                scale,
                theme.role.selection_match_bg,
                window,
            );
        }

        for bracket in items_overlapping_row(structure.bracket_matches.as_ref(), row, Clone::clone) {
            paint_range_background(
                row,
                bracket,
                code_origin_x,
                row_height,
                scale,
                theme.role.bracket_match_bg,
                window,
            );
        }

        for search_match in items_overlapping_row(search_matches, row, Clone::clone) {
            paint_range_background(
                row,
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
                row,
                active_search_match,
                code_origin_x,
                row_height,
                scale,
                theme.role.search_active_match_bg,
                window,
            );
        }

        for selection in items_overlapping_row(selections, row, Selection::range) {
            paint_range_background(
                row,
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

        let marker_first = structure
            .markers
            .partition_point(|marker| marker.at < row.line_start_char);
        let marker_last = marker_first
            + structure.markers[marker_first..].partition_point(|marker| row_contains_cursor(row, marker.at));
        for marker in &structure.markers[marker_first..marker_last] {
            let x = code_origin_x + x_for_global_char(row, marker.at).unwrap_or_else(|| px(0.0));
            let _ = marker.shaped.paint(point(x, row.row_top), line_height, window, cx);
        }

        for bracket in items_overlapping_row(structure.bracket_matches.as_ref(), row, Clone::clone) {
            if let Some(bounds) = range_bounds(row, bracket, code_origin_x, row_height, scale) {
                paint_outline(
                    bounds,
                    metrics::px_for_scale(1.0, scale),
                    theme.role.bracket_match_outline,
                    window,
                );
            }
        }

        if focused && cursor_visible {
            for cursor in row_cursors {
                let cursor_char = cursor.char;
                let block_cursor = vim_mode == vim::Mode::Normal && cursor.collapsed;
                let cursor_x = code_origin_x
                    + x_for_global_char(row, cursor_char.min(row.display_end_char)).unwrap_or_else(|| px(0.0));
                let cursor_width = if block_cursor {
                    let next_x = code_origin_x
                        + x_for_global_char(row, (cursor_char + 1).min(row.display_end_char.max(cursor_char + 1)))
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

        if let Some(drop_cursor) = drop_cursor.filter(|drop| row_contains_cursor(row, *drop)) {
            let cursor_x = code_origin_x
                + x_for_global_char(row, drop_cursor.min(row.display_end_char)).unwrap_or_else(|| px(0.0));
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
        let valid_lines = vec![true; lines.len()];
        ViewportCache {
            syntax_highlights: Some(CachedSyntaxHighlights {
                language: SyntaxLanguage::Rust,
                revision: 0,
                lines,
                line_byte_lens,
                valid_lines,
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
    fn syntax_cache_materializes_only_requested_lines() {
        let source = "let first = 1;\nfn second() {}\nlet third = 3;\n";
        let buffer = Rope::from_str(source);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, &buffer, 7).unwrap();
        let line_count = buffer.len_lines();
        let mut cache = ViewportCache {
            syntax_highlights: Some(CachedSyntaxHighlights {
                language: SyntaxLanguage::Rust,
                revision: 7,
                lines: vec![Vec::new(); line_count],
                line_byte_lens: vec![0; line_count],
                valid_lines: vec![false; line_count],
            }),
            ..Default::default()
        };

        ensure_syntax_cache_for_lines(&mut cache, &state, 7, 1..2);

        let highlights = cache.syntax_highlights.unwrap();
        assert_eq!(highlights.valid_lines, [false, true, false, false]);
        assert!(highlights.lines[0].is_empty());
        assert!(highlights.lines[1].iter().any(|span| span.role == SyntaxRole::Keyword));
        assert!(highlights.lines[2].is_empty());
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
        let before_buffer = Rope::from_str(&before);
        let after_buffer = Rope::from_str(&after);
        let mut cache = ViewportCache {
            wrap_layout: Some(CachedWrapLayout {
                revision: 0,
                layout: Rc::new(build_wrap_layout_for_rope(&before_buffer, 12, true)),
            }),
            ..Default::default()
        };

        cache.patch_wrap_layout(&after_buffer, 1, &SyntaxInvalidation::Lines(99..102));

        let patched = cache.wrap_layout.expect("patch should retain layout");
        assert_eq!(patched.revision, 1);
        assert_eq!(*patched.layout, build_wrap_layout_for_rope(&after_buffer, 12, true));
    }

    #[test]
    fn unwrapped_width_cache_unions_edits_until_the_next_measurement() {
        let mut cache = ViewportCache {
            max_unwrapped_line_width: Some(CachedUnwrappedLineWidth {
                revision: 7,
                char_width: px(8.0),
                font_size: px(13.0),
                width: px(800.0),
                line_ix: 50,
            }),
            ..Default::default()
        };

        cache.patch_unwrapped_line_width(8, &SyntaxInvalidation::Lines(10..12), 100);
        cache.patch_unwrapped_line_width(8, &SyntaxInvalidation::Lines(0..0), 100);
        assert_eq!(
            cache
                .max_unwrapped_line_width
                .expect("cached width should remain")
                .revision,
            7,
            "an empty render-path sync must not hide pending line measurements"
        );
        cache.patch_unwrapped_line_width(9, &SyntaxInvalidation::Lines(20..22), 100);

        let pending = cache
            .unwrapped_line_width_invalidation
            .as_ref()
            .expect("edits should remain pending until no-wrap width is requested");
        assert_eq!(pending.base_revision, 7);
        assert_eq!(pending.revision, 9);
        assert_eq!(pending.lines, 10..22);

        cache.patch_unwrapped_line_width(10, &SyntaxInvalidation::Full, 100);
        assert!(cache.max_unwrapped_line_width.is_none());
        assert!(cache.unwrapped_line_width_invalidation.is_none());
    }

    #[test]
    fn parser_failure_invalidates_cached_display_text_and_layout() {
        let mut cache = cache_with_highlights(vec![3], vec![Vec::new()]);
        cache.display_lines.insert(0, SharedString::from("old"));
        cache
            .wrapped_lines
            .insert((0, Some(80)), Rc::from(Vec::<WrappedSegment>::new()));
        cache.wrap_layout = Some(CachedWrapLayout {
            revision: 4,
            layout: Rc::new(build_wrap_layout_for_rope(&Rope::from_str("old"), 80, true)),
        });
        cache.max_unwrapped_line_width = Some(CachedUnwrappedLineWidth {
            revision: 4,
            char_width: px(8.0),
            font_size: px(13.0),
            width: px(24.0),
            line_ix: 0,
        });

        cache.invalidate_after_parser_failure();

        assert!(cache.syntax_highlights.is_none());
        assert!(cache.display_lines.is_empty());
        assert!(cache.wrapped_lines.is_empty());
        assert!(cache.wrap_layout.is_none());
        assert!(cache.max_unwrapped_line_width.is_none());
        assert!(cache.unwrapped_line_width_invalidation.is_none());
    }
}
