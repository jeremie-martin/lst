use std::time::Instant;

use gpui::{
    canvas, div, point, prelude::*, px, App, Bounds, ClipboardItem, Context, CursorStyle, InteractiveElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, ScrollDelta, ScrollHandle,
    ScrollWheelEvent, Styled, Window,
};
use lst_editor::{EditorCommand as Command, EditorTab as ModelEditorTab, RevealIntent};

use crate::{
    char_to_line_col, diagnostics,
    ui::{
        scrollbar::{paint_scrollbar, scroll_for_thumb_drag, scroll_for_track_click, scrollbar_layout, ScrollbarAxis},
        theme::metrics,
    },
    viewport::{
        byte_index_to_char, code_char_width, ensure_wrap_layout, line_display_text, max_scroll_left, max_scroll_top,
        scroll_left_for, scroll_to_left, scroll_to_top, scroll_top_for, visual_row_for_char, x_for_display_char,
        ViewportLayoutMetrics, WrapLayoutInput,
    },
    EditorScrollbarDrag, EditorTabView, FocusTarget, LstGpuiApp, SmoothScroll,
};

/// Remaining distance shrinks by e⁻¹ every tau; ~90% of a detent lands
/// within two taus (~70ms), keeping the motion visibly smooth while the
/// response stays snappy.
const SMOOTH_SCROLL_TAU_S: f32 = 0.03;
/// Snap once the remainder drops under half a physical pixel.
const SMOOTH_SCROLL_SNAP_PX: f32 = 0.5;

/// One frame of exponential approach toward `target`. Deriving the blend
/// factor from elapsed time keeps the feel identical at 60Hz and 144Hz.
fn smooth_scroll_step(current: Point<Pixels>, target: Point<Pixels>, dt_s: f32) -> (Point<Pixels>, bool) {
    let alpha = 1.0 - (-dt_s / SMOOTH_SCROLL_TAU_S).exp();
    let next = point(
        current.x + (target.x - current.x) * alpha,
        current.y + (target.y - current.y) * alpha,
    );
    let snap = px(SMOOTH_SCROLL_SNAP_PX);
    if (target.x - next.x).abs() < snap && (target.y - next.y).abs() < snap {
        (target, true)
    } else {
        (next, false)
    }
}

impl LstGpuiApp {
    pub(crate) fn active_tab(&self) -> &ModelEditorTab {
        self.model.active_tab()
    }

    pub(crate) fn record_find_metrics(&self, reindex_ms: f64) {
        diagnostics::record_ms("find_reindex_ms", reindex_ms);
        diagnostics::record_usize("find_match_count", self.model.find().matches.len());
        diagnostics::record_usize("find_query_len", self.model.find().query.chars().count());
    }

    pub(crate) fn record_operation(&self, label: &'static str, clipboard_read_ms: Option<f64>, apply_ms: f64) {
        let tab = self.active_tab();
        diagnostics::record_operation(
            label,
            tab.buffer().len_bytes(),
            tab.line_count(),
            clipboard_read_ms,
            apply_ms,
        );
    }

    pub(crate) fn active_view(&self) -> &EditorTabView {
        self.tab_views
            .get(&self.model.active_tab_id())
            .expect("active tab must have a tab view")
    }

    fn active_cursor_line_col(&self) -> (usize, usize) {
        char_to_line_col(self.active_tab().buffer(), self.active_tab().cursor_char())
    }

    /// "▲N" / "▼N" indicator for cursors outside the painted viewport.
    /// Compares against painted char ranges (not logical lines) so soft-
    /// wrap segments don't confuse the off-screen test.
    fn off_screen_cursor_indicator(&self) -> Option<String> {
        let view = self.tab_views.get(&self.model.active_tab_id())?;
        let geometry = view.geometry.borrow();
        // `rows` carries the char ranges from the last paint. After an edit
        // the buffer revision advances but geometry is intentionally kept
        // (see `invalidate_visual_state`), so on the edit frame these ranges
        // predate the insert. Comparing the freshly-moved cursor against them
        // would spuriously report it "below" the painted region for one frame
        // — a layout-shifting flicker in the status bar, most visible in a
        // new/empty buffer where the cursor always sits at the very end. Wait
        // until the viewport repaints at the current revision.
        if geometry.painted_revision != self.active_tab().revision() {
            return None;
        }
        let first = geometry.rows.first()?;
        let last = geometry.rows.last()?;
        let painted_start_char = first.line_start_char;
        let painted_end_char = last.display_end_char;
        let mut above = 0usize;
        let mut below = 0usize;
        for selection in self.active_tab().selection_set().as_slice() {
            let head = selection.head();
            if head < painted_start_char {
                above += 1;
            } else if head > painted_end_char {
                below += 1;
            }
        }
        if above == 0 && below == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if above > 0 {
            parts.push(format!("\u{25B2}{above}"));
        }
        if below > 0 {
            parts.push(format!("\u{25BC}{below}"));
        }
        Some(parts.join(" "))
    }

    pub(crate) fn selection_summary(&self) -> Option<String> {
        let tab = self.active_tab();
        let set = tab.selection_set();
        if !set.is_single() {
            let buffer = tab.buffer();
            let (total_chars, total_lines) =
                set.as_slice()
                    .iter()
                    .fold((0usize, 0usize), |(chars, lines), selection| {
                        let range = selection.range();
                        if range.start == range.end {
                            return (chars, lines);
                        }
                        let start_line = buffer.char_to_line(range.start);
                        let end_line = buffer.char_to_line(range.end - 1);
                        (chars + range.len(), lines + (end_line - start_line + 1))
                    });
            let mut parts = vec![format!("{} cursors", set.as_slice().len())];
            if total_chars > 0 {
                parts.push(format!("Sel {total_chars}"));
                if total_lines > 1 {
                    parts.push(format!("{total_lines} lines"));
                }
            }
            return Some(parts.join(" · "));
        }
        let selected = tab.selected_range();
        (selected.start != selected.end).then(|| format!("Sel {}", selected.len()))
    }

    fn painted_wrap_columns(&self) -> Option<usize> {
        self.active_view().geometry.borrow().painted_wrap_columns
    }

    pub(crate) fn status_detail_segments(&self) -> Vec<String> {
        let tab = self.active_tab();
        let (line, column) = self.active_cursor_line_col();
        let mut parts = vec![
            format!("Ln {}", line + 1),
            format!("Col {}", column + 1),
            if self.model.show_wrap() {
                self.painted_wrap_columns()
                    .map(|columns| format!("Wrap {columns} cols"))
                    .unwrap_or_else(|| "Wrap".to_string())
            } else {
                "No Wrap".to_string()
            },
            format!("{} lines", tab.line_count()),
        ];
        if self.model.input_mode() == lst_editor::InputMode::Vim {
            parts.insert(0, self.model.vim_mode().label().to_string());
            let pending = self.model.vim_pending_display();
            if !pending.is_empty() {
                parts.push(pending);
            }
        }
        if self.model.overtype() {
            parts.push("OVR".to_string());
        }
        if let Some(indicator) = self.off_screen_cursor_indicator() {
            parts.push(indicator);
        }
        if let Some(selection) = self.selection_summary() {
            parts.push(selection);
        }
        if self.zoom_level != 0 {
            parts.push(format!("Zoom {:.0}%", self.ui_scale() * 100.0));
        }
        if self.model.find().visible {
            let current = if self.model.find().matches.is_empty() {
                0
            } else {
                self.model.find().active.map_or(0, |index| index + 1)
            };
            parts.push(format!("Match {current}/{}", self.model.find().matches.len()));
        }
        parts
    }

    pub(crate) fn status_details(&self) -> String {
        self.status_detail_segments().join("  ")
    }

    pub(crate) fn move_vertical(&mut self, delta: isize, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        let wrap_columns = self.active_wrap_columns(window, cx);
        self.execute_model_command(cx, Command::MoveDisplayRows(delta, select, wrap_columns));
    }

    pub(crate) fn active_wrap_columns(&mut self, window: &mut Window, cx: &App) -> usize {
        if !self.model.show_wrap() {
            return usize::MAX;
        }

        let (geometry, cache) = {
            let active_view = self.active_view();
            (active_view.geometry.clone(), active_view.cache.clone())
        };
        let viewport_width = geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| self.ui_px(metrics::WINDOW_WIDTH - 48.0));
        let revision = self.model.active_tab().revision();
        let buffer = self.model.active_tab().buffer().clone();
        let layout = {
            let mut cache = cache.borrow_mut();
            let char_width = code_char_width(&mut cache, window, self.ui_scale(), self.theme(cx));
            let layout_metrics = ViewportLayoutMetrics::new(
                self.model.show_gutter(),
                self.model.active_tab().line_count(),
                char_width,
                self.ui_scale(),
            );
            ensure_wrap_layout(
                &mut cache,
                WrapLayoutInput {
                    buffer: &buffer,
                    revision,
                    viewport_width,
                    char_width,
                    layout_metrics,
                    show_wrap: self.model.show_wrap(),
                    scale: self.ui_scale(),
                },
            )
        };
        layout.wrap_columns
    }

    pub(crate) fn move_page(&mut self, down: bool, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        let wrap_columns = self.active_wrap_columns(window, cx);
        self.execute_model_command(cx, Command::Page(down, select, wrap_columns));
    }

    pub(crate) fn move_visual_line_boundary(&mut self, end: bool, select: bool, cx: &mut Context<Self>) {
        let wrap_columns = self
            .active_view()
            .geometry
            .borrow()
            .painted_wrap_columns
            .unwrap_or(usize::MAX);
        self.update_model(cx, true, |model| {
            model.move_visual_line_boundary(end, select, wrap_columns);
        });
    }

    pub(crate) fn scroll_editor_lines(&mut self, delta: isize, cx: &mut Context<Self>) {
        let view = self.active_view();
        let current = scroll_top_for(&view.scroll);
        let target = current + self.ui_px(metrics::row_height()) * delta as f32;
        scroll_to_top(&view.scroll, target);
        self.sync_viewport_state();
        cx.notify();
    }

    /// Wheel detents animate toward an accumulated target instead of jumping
    /// a full detent (3 rows) in one frame. Trackpads emit fine-grained pixel
    /// deltas that are already smooth, so those keep GPUI's stock instant
    /// handling: the event is left to propagate to the scroll container, and
    /// any running animation yields to it.
    pub(crate) fn on_editor_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ScrollDelta::Lines(_) = event.delta else {
            self.smooth_scroll = None;
            return;
        };
        let delta = event.delta.pixel_delta(self.ui_px(metrics::row_height()));
        let tab_id = self.model.active_tab_id();
        let scroll = &self.active_view().scroll;
        let current = point(scroll_left_for(scroll), scroll_top_for(scroll));
        let base = match self.smooth_scroll {
            Some(anim) if anim.tab_id == tab_id => anim.target,
            _ => current,
        };
        // Wheel-down is a negative delta while targets count pixels from the
        // origin. Clamping the target keeps detents past the edge from
        // banking distance that would have to unwind before a reversal moves.
        let target = point(
            (base.x - delta.x).clamp(px(0.0), max_scroll_left(scroll)),
            (base.y - delta.y).clamp(px(0.0), max_scroll_top(scroll)),
        );
        self.smooth_scroll = Some(SmoothScroll {
            tab_id,
            target,
            last_applied: current,
            last_tick: Instant::now(),
        });
        self.schedule_smooth_scroll(window, cx);
        cx.stop_propagation();
    }

    fn schedule_smooth_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.smooth_scroll.is_none() || self.smooth_scroll_scheduled {
            return;
        }
        self.smooth_scroll_scheduled = true;
        cx.on_next_frame(window, |this, window, cx| {
            this.smooth_scroll_scheduled = false;
            this.tick_smooth_scroll(window, cx);
        });
        cx.notify();
    }

    fn tick_smooth_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut anim) = self.smooth_scroll else {
            return;
        };
        if anim.tab_id != self.model.active_tab_id() {
            self.smooth_scroll = None;
            return;
        }
        let scroll = self.active_view().scroll.clone();
        let current = point(scroll_left_for(&scroll), scroll_top_for(&scroll));
        if current != anim.last_applied {
            self.smooth_scroll = None;
            return;
        }
        let now = Instant::now();
        let (next, done) = smooth_scroll_step(current, anim.target, (now - anim.last_tick).as_secs_f32());
        scroll_to_left(&scroll, next.x);
        scroll_to_top(&scroll, next.y);
        if done {
            self.smooth_scroll = None;
        } else {
            anim.last_applied = point(scroll_left_for(&scroll), scroll_top_for(&scroll));
            anim.last_tick = now;
            self.smooth_scroll = Some(anim);
        }
        self.sync_viewport_state();
        cx.notify();
        self.schedule_smooth_scroll(window, cx);
    }

    pub(crate) fn sync_viewport_state(&mut self) {
        let bounds = self.active_view().geometry.borrow().bounds;
        let Some(bounds) = bounds else {
            return;
        };
        let row_height = self.ui_px(metrics::row_height());
        if row_height <= px(0.0) || bounds.size.height <= px(0.0) {
            return;
        }
        let rows = ((bounds.size.height / row_height).floor() as usize).max(1);
        let scroll_top = scroll_top_for(&self.active_view().scroll);
        let top = (scroll_top / row_height).floor() as usize;
        self.model.set_viewport_rows(rows);
        self.model.set_viewport_top(top);
    }

    pub(crate) fn queue_cursor_reveal(&mut self, intent: RevealIntent) {
        self.pending_reveal = Some(intent);
    }

    pub(crate) fn schedule_pending_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_reveal.is_none() || self.reveal_scheduled {
            return;
        }

        self.reveal_scheduled = true;
        cx.on_next_frame(window, |this, window, cx| {
            this.reveal_scheduled = false;
            this.flush_pending_reveal(window, cx);
        });
        cx.notify();
    }

    fn flush_pending_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(intent) = self.pending_reveal.take() else {
            return;
        };

        if self.try_reveal_active_cursor(intent, window, cx) {
            cx.notify();
        } else {
            self.pending_reveal = Some(intent);
            self.schedule_pending_reveal(window, cx);
        }
    }

    fn active_cursor_visual_row(&self) -> Option<usize> {
        let tab = self.active_tab();
        let view = self.active_view();

        if self.model.show_wrap() {
            let cache = view.cache.borrow();
            let cached = cache.wrap_layout.as_ref()?;
            if cached.revision != tab.revision() || !cached.layout.show_wrap {
                return None;
            }
            visual_row_for_char(tab, &cached.layout)
        } else {
            Some(tab.buffer().char_to_line(tab.cursor_char()))
        }
    }

    fn try_reveal_active_cursor(&self, intent: RevealIntent, window: &mut Window, cx: &App) -> bool {
        let view = self.active_view();
        let viewport_bounds = {
            let geometry = view.geometry.borrow();
            let Some(bounds) = geometry.bounds else {
                return false;
            };
            bounds
        };
        if viewport_bounds.size.height <= px(0.) {
            return false;
        }

        let Some(visual_row) = self.active_cursor_visual_row() else {
            return false;
        };

        let row_height = self.ui_px(metrics::row_height());
        let caret_top = row_height * visual_row as f32;
        let caret_bottom = caret_top + row_height;
        let scroll_top = scroll_top_for(&view.scroll);
        let viewport_height = viewport_bounds.size.height;
        let margin = row_height * self.model.viewport().effective_scrolloff() as f32;

        let target = match intent {
            RevealIntent::NearestEdge => {
                if caret_top < scroll_top + margin {
                    Some(caret_top - margin)
                } else if caret_bottom > scroll_top + viewport_height - margin {
                    Some(caret_bottom + margin - viewport_height)
                } else {
                    None
                }
            }
            RevealIntent::Center => Some(caret_top - (viewport_height - row_height) / 2.0),
            RevealIntent::Top => Some(caret_top - margin),
            RevealIntent::Bottom => Some(caret_bottom + margin - viewport_height),
        };

        if let Some(target) = target {
            let max_top = max_scroll_top(&view.scroll);
            scroll_to_top(&view.scroll, target);
            if target > max_top && caret_bottom > max_top + viewport_height {
                return false;
            }
        }

        if !self.model.show_wrap() {
            return self.try_reveal_active_cursor_horizontally(view, viewport_bounds, window, cx);
        }
        true
    }

    fn try_reveal_active_cursor_horizontally(
        &self,
        view: &EditorTabView,
        viewport_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &App,
    ) -> bool {
        let geometry = view.geometry.borrow();
        let char_width = geometry.painted_char_width;
        if char_width <= px(0.0) {
            return false;
        }

        let scroll_left = scroll_left_for(&view.scroll);
        let pad = geometry.code_origin_pad_at_paint;
        let visible_width = (viewport_bounds.size.width - pad).max(px(0.0));
        if visible_width <= px(0.0) {
            return false;
        }

        let visible_cols = ((visible_width / px(1.0)) / (char_width / px(1.0))).floor() as usize;
        if visible_cols == 0 {
            return false;
        }

        let cursor_x = self.active_cursor_rendered_x(char_width, window, cx);

        let raw_margin = self.model.viewport().sidescrolloff;
        let margin_cols = if visible_cols <= 1 {
            0
        } else {
            raw_margin.min((visible_cols - 1) / 2)
        };
        let margin = char_width * margin_cols as f32;

        let target_x = if cursor_x < scroll_left + margin {
            Some(cursor_x - margin)
        } else if cursor_x > scroll_left + visible_width - margin {
            Some(cursor_x + margin - visible_width)
        } else {
            None
        };

        if let Some(target_x) = target_x {
            if target_x > px(0.0) && max_scroll_left(&view.scroll) <= px(0.0) {
                return false;
            }
            drop(geometry);
            scroll_to_left(&view.scroll, target_x);
        }
        true
    }

    fn active_cursor_rendered_x(&self, char_width: Pixels, window: &mut Window, cx: &App) -> Pixels {
        let tab = self.active_tab();
        let cursor = tab.cursor_char().min(tab.buffer().len_chars());
        let line = tab.buffer().char_to_line(cursor);
        let line_start = tab.buffer().line_to_char(line);
        let display_text = line_display_text(tab.buffer(), line);
        let column = cursor
            .saturating_sub(line_start)
            .min(display_text.as_ref().chars().count());
        x_for_display_char(
            display_text.as_ref(),
            column,
            char_width,
            self.ui_scale(),
            self.theme(cx),
            window,
        )
    }

    pub(crate) fn sync_primary_selection(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.active_tab().selected_text() {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
    }

    pub(crate) fn active_char_index_for_point(&self, point: Point<Pixels>) -> usize {
        let active_view = self.active_view();
        let geometry = active_view.geometry.borrow();
        let Some(bounds) = geometry.bounds else {
            return self.active_tab().cursor_char();
        };
        const SCROLL_STALE_THRESHOLD: f32 = 0.5;
        let current_scroll_top = scroll_top_for(&active_view.scroll);
        let current_scroll_left = scroll_left_for(&active_view.scroll);
        if self.selection_drag.is_none()
            && (geometry.painted_revision != self.active_tab().revision()
                || (current_scroll_top - geometry.scroll_top_at_paint).abs() > px(SCROLL_STALE_THRESHOLD)
                || (current_scroll_left - geometry.scroll_left_at_paint).abs() > px(SCROLL_STALE_THRESHOLD))
        {
            // Geometry rows carry char offsets from the last paint; after an
            // edit (revision bumped) they predate the buffer until the next
            // paint, and a click landing in that pre-repaint frame would map to
            // a stale offset. Fall back to the known cursor position, matching
            // the scroll-staleness handling above.
            return self.active_tab().cursor_char();
        }
        let code_origin_x = bounds.left() + geometry.code_origin_pad_at_paint;

        let row_height = self.ui_px(metrics::row_height());
        let row = if geometry.rows.is_empty() {
            return 0;
        } else if point.y <= geometry.rows[0].row_top {
            &geometry.rows[0]
        } else if let Some(row) = geometry
            .rows
            .iter()
            .find(|row| point.y >= row.row_top && point.y < row.row_top + row_height)
        {
            row
        } else {
            let last_row = geometry.rows.last().expect("checked above");
            return last_row.display_end_char;
        };

        let x = if point.x >= code_origin_x {
            point.x - code_origin_x + current_scroll_left
        } else {
            px(0.0)
        };

        if let Some(code_line) = row.code_line.as_ref() {
            let hit_x = (x - geometry.painted_char_width * 0.5).max(px(0.0));
            let byte_index = code_line.closest_index_for_x(hit_x);
            let line_char = byte_index_to_char(code_line.text.as_ref(), byte_index);
            (row.line_start_char + line_char).min(row.display_end_char)
        } else {
            row.line_start_char
        }
    }
}

fn scrollbar_id(axis: ScrollbarAxis) -> &'static str {
    match axis {
        ScrollbarAxis::Vertical => "editor-scrollbar",
        ScrollbarAxis::Horizontal => "editor-horizontal-scrollbar",
    }
}

fn scrollbar_max_offset(axis: ScrollbarAxis, scroll: &ScrollHandle) -> Pixels {
    match axis {
        ScrollbarAxis::Vertical => scroll.max_offset().height.max(px(0.0)),
        ScrollbarAxis::Horizontal => scroll.max_offset().width.max(px(0.0)),
    }
}

fn scrollbar_current_offset(axis: ScrollbarAxis, scroll: &ScrollHandle) -> Pixels {
    match axis {
        ScrollbarAxis::Vertical => scroll_top_for(scroll),
        ScrollbarAxis::Horizontal => scroll_left_for(scroll),
    }
}

fn scroll_editor_to(axis: ScrollbarAxis, scroll: &ScrollHandle, target: Pixels) {
    match axis {
        ScrollbarAxis::Vertical => scroll_to_top(scroll, target),
        ScrollbarAxis::Horizontal => scroll_to_left(scroll, target),
    }
}

impl LstGpuiApp {
    pub(crate) fn render_editor_scrollbar(
        &mut self,
        axis: ScrollbarAxis,
        viewport_scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let track_size = metrics::px_for_scale(metrics::SCROLLBAR_TRACK_WIDTH, scale);
        let has_overflow = scrollbar_max_offset(axis, &viewport_scroll) > px(0.0);
        let prepare_scroll = viewport_scroll.clone();
        let paint_scroll = viewport_scroll;
        let entity = cx.entity();

        let bar = match axis {
            ScrollbarAxis::Vertical => div()
                .id(scrollbar_id(axis))
                .absolute()
                .top_0()
                .right_0()
                .h_full()
                .w(track_size),
            ScrollbarAxis::Horizontal => div()
                .id(scrollbar_id(axis))
                .absolute()
                .left_0()
                .bottom_0()
                .right(track_size)
                .h(track_size),
        };

        bar.when(has_overflow, |bar| bar.cursor(CursorStyle::Arrow)).child(
            canvas(
                move |bounds, _, _| {
                    scrollbar_layout(
                        axis,
                        bounds,
                        scrollbar_current_offset(axis, &prepare_scroll),
                        scrollbar_max_offset(axis, &prepare_scroll),
                        scale,
                    )
                },
                move |_, layout, window, cx| {
                    let Some(layout) = layout else {
                        return;
                    };

                    let (active, hovered) = {
                        let app = entity.read(cx);
                        (
                            app.scrollbar_drag(axis).is_some(),
                            app.scrollbar_hovered(axis) || layout.thumb_bounds.contains(&window.mouse_position()),
                        )
                    };
                    paint_scrollbar(&layout, active, hovered, scale, theme, window);

                    let entity_for_down = entity.clone();
                    let scroll_for_down = paint_scroll.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if !phase.bubble()
                            || event.button != MouseButton::Left
                            || !layout.track_bounds.contains(&event.position)
                        {
                            return;
                        }

                        let focus_handle = entity_for_down.read(cx).focus_handle.clone();
                        window.focus(&focus_handle);
                        let pointer = axis.pointer_offset(event.position);
                        let current = scrollbar_current_offset(axis, &scroll_for_down);
                        let on_thumb = layout.thumb_bounds.contains(&event.position);
                        let drag = if on_thumb {
                            Some(EditorScrollbarDrag {
                                grab_offset: pointer - axis.pointer_offset(layout.thumb_bounds.origin),
                            })
                        } else {
                            scroll_editor_to(
                                axis,
                                &scroll_for_down,
                                scroll_for_track_click(&layout, pointer, current),
                            );
                            None
                        };
                        entity_for_down.update(cx, |this, _| {
                            this.set_focus(FocusTarget::Editor);
                            this.selection_drag = None;
                            this.set_scrollbar_hovered(axis, on_thumb);
                            this.set_scrollbar_drag(axis, drag);
                        });
                        cx.stop_propagation();
                        cx.notify(entity_for_down.entity_id());
                    });

                    let entity_for_move = entity.clone();
                    let scroll_for_move = paint_scroll.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if !phase.bubble() {
                            return;
                        }

                        let drag = entity_for_move.read(cx).scrollbar_drag(axis);
                        if let Some(drag) = drag {
                            if event.dragging() {
                                let target = scroll_for_thumb_drag(
                                    &layout,
                                    axis.pointer_offset(event.position),
                                    drag.grab_offset,
                                );
                                scroll_editor_to(axis, &scroll_for_move, target);
                                entity_for_move.update(cx, |this, _| {
                                    this.set_scrollbar_hovered(axis, true);
                                });
                                cx.stop_propagation();
                                cx.notify(entity_for_move.entity_id());
                            } else {
                                entity_for_move.update(cx, |this, _| {
                                    this.set_scrollbar_drag(axis, None);
                                });
                                cx.notify(entity_for_move.entity_id());
                            }
                            return;
                        }

                        let hovered = layout.thumb_bounds.contains(&event.position);
                        if entity_for_move.read(cx).scrollbar_hovered(axis) != hovered {
                            entity_for_move.update(cx, |this, _| {
                                this.set_scrollbar_hovered(axis, hovered);
                            });
                            cx.notify(entity_for_move.entity_id());
                        }
                    });

                    let entity_for_up = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if !phase.bubble() || event.button != MouseButton::Left {
                            return;
                        }

                        let was_dragging = entity_for_up.read(cx).scrollbar_drag(axis).is_some();
                        if was_dragging || layout.track_bounds.contains(&event.position) {
                            entity_for_up.update(cx, |this, _| {
                                this.set_scrollbar_drag(axis, None);
                                this.set_scrollbar_hovered(axis, layout.thumb_bounds.contains(&event.position));
                            });
                            cx.stop_propagation();
                            cx.notify(entity_for_up.entity_id());
                        }
                    });
                },
            )
            .size_full(),
        )
    }

    fn scrollbar_drag(&self, axis: ScrollbarAxis) -> Option<EditorScrollbarDrag> {
        match axis {
            ScrollbarAxis::Vertical => self.editor_scrollbar_drag,
            ScrollbarAxis::Horizontal => self.editor_horizontal_scrollbar_drag,
        }
    }

    fn set_scrollbar_drag(&mut self, axis: ScrollbarAxis, drag: Option<EditorScrollbarDrag>) {
        match axis {
            ScrollbarAxis::Vertical => self.editor_scrollbar_drag = drag,
            ScrollbarAxis::Horizontal => self.editor_horizontal_scrollbar_drag = drag,
        }
    }

    fn scrollbar_hovered(&self, axis: ScrollbarAxis) -> bool {
        match axis {
            ScrollbarAxis::Vertical => self.editor_scrollbar_hovered,
            ScrollbarAxis::Horizontal => self.editor_horizontal_scrollbar_hovered,
        }
    }

    fn set_scrollbar_hovered(&mut self, axis: ScrollbarAxis, hovered: bool) {
        match axis {
            ScrollbarAxis::Vertical => self.editor_scrollbar_hovered = hovered,
            ScrollbarAxis::Horizontal => self.editor_horizontal_scrollbar_hovered = hovered,
        }
    }
}
