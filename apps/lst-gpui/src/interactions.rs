use gpui::{
    point, px, Bounds, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window,
};
use lst_editor::{
    selection::{drag_selection_range, line_range_at_char, word_range_at_char},
    RevealIntent,
};

use crate::viewport::scroll_top_for;
use std::ops::Range;

use crate::{ui::theme::metrics, LstGpuiApp};

#[derive(Clone, Debug)]
pub(crate) enum DragSelectionMode {
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
}

impl LstGpuiApp {
    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.persistent_overlay_focus = None;
        window.focus(&self.focus_handle);
        self.drag_last_point = Some(event.position);
        let index = self.active_char_index_for_point(event.position);
        if event.click_count >= 3 {
            let line_range = line_range_at_char(self.active_tab().buffer(), index);
            self.drag_selecting = Some(DragSelectionMode::Line(line_range.clone()));
            self.select_active_range(line_range, cx);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            let word_range = word_range_at_char(self.active_tab().buffer(), index);
            self.drag_selecting = Some(DragSelectionMode::Word(word_range.clone()));
            self.select_active_range(word_range, cx);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }

        self.drag_selecting = Some(DragSelectionMode::Character);
        self.update_model(cx, true, |model| {
            model.move_to_char(index, event.modifiers.shift, None);
        });
        self.schedule_drag_autoscroll(window, cx);
        cx.notify();
    }

    pub(crate) fn on_middle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.persistent_overlay_focus = None;
        window.focus(&self.focus_handle);
        self.drag_selecting = None;
        self.drag_last_point = None;
        self.drag_autoscroll_active = false;

        let index = self.active_char_index_for_point(event.position);
        match cx.read_from_primary().and_then(|item| item.text()) {
            Some(text) => {
                self.update_model(cx, true, |model| {
                    model.move_to_char(index, false, None);
                    model.paste_text(text);
                });
            }
            None => {
                self.update_model(cx, true, |model| {
                    model.clipboard_unavailable();
                });
            }
        }
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_last_point = Some(event.position);
        if !self.apply_drag_selection_at_point(event.position, cx) {
            return;
        }
        self.schedule_drag_autoscroll(window, cx);
        cx.notify();
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_selecting = None;
        self.drag_last_point = None;
        self.drag_autoscroll_active = false;
        self.sync_primary_selection(cx);
        cx.notify();
    }

    fn apply_drag_selection_at_point(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = self.active_char_index_for_point(position);
        match self.drag_selecting.clone() {
            Some(DragSelectionMode::Character) => {
                self.update_model(cx, true, |model| {
                    model.move_to_char(index, true, None);
                });
            }
            Some(DragSelectionMode::Word(anchor)) => {
                let current = word_range_at_char(self.active_tab().buffer(), index);
                self.select_active_drag_range(anchor, current, cx);
            }
            Some(DragSelectionMode::Line(anchor)) => {
                let current = line_range_at_char(self.active_tab().buffer(), index);
                self.select_active_drag_range(anchor, current, cx);
            }
            None => return false,
        }
        self.reveal_active_cursor(RevealIntent::NearestEdge);
        true
    }

    fn schedule_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag_autoscroll_active || self.drag_autoscroll_target().is_none() {
            return;
        }
        self.drag_autoscroll_active = true;
        cx.on_next_frame(window, |this, window, cx| {
            this.run_drag_autoscroll(window, cx);
        });
    }

    fn run_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drag_autoscroll_active = false;
        if self.drag_selecting.is_none() {
            self.drag_last_point = None;
            return;
        }

        if let Some(target) = self.drag_autoscroll_target() {
            self.active_view()
                .scroll
                .set_offset(point(px(0.0), -target));
            if let Some(position) = self.drag_last_point {
                self.apply_drag_selection_at_point(position, cx);
            }
            cx.notify();
        }
        self.schedule_drag_autoscroll(window, cx);
    }

    fn drag_autoscroll_target(&self) -> Option<Pixels> {
        let position = self.drag_last_point?;
        let geometry = self.active_view().geometry.borrow();
        let bounds = geometry.bounds?;
        let delta = drag_autoscroll_delta(position, bounds, self.ui_scale())?;
        let view = self.active_view();
        let current = scroll_top_for(&view.scroll);
        let max = view.scroll.max_offset().height.max(px(0.0));
        let target = (current + delta).max(px(0.0)).min(max);
        (target != current).then_some(target)
    }

    fn select_active_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.update_model(cx, true, |model| {
            model.set_selection(range, false);
        });
    }

    fn select_active_drag_range(
        &mut self,
        anchor: Range<usize>,
        current: Range<usize>,
        cx: &mut Context<Self>,
    ) {
        let (selection, reversed) = drag_selection_range(anchor, current);
        self.update_model(cx, true, |model| {
            model.set_selection(selection, reversed);
        });
    }
}

pub(crate) fn drag_autoscroll_delta(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    scale: f32,
) -> Option<Pixels> {
    const EDGE_PX: f32 = 36.0;
    let edge = metrics::px_for_scale(EDGE_PX, scale);
    let top_edge = bounds.top() + edge;
    let bottom_edge = bounds.bottom() - edge;

    if position.y < top_edge {
        let distance = ((top_edge - position.y) / px(1.0)).min(EDGE_PX * scale * 2.0);
        let rows = 0.5 + distance / (EDGE_PX * scale);
        Some(-metrics::px_for_scale(
            (metrics::ROW_HEIGHT * rows).min(metrics::ROW_HEIGHT * 3.0),
            scale,
        ))
    } else if position.y > bottom_edge {
        let distance = ((position.y - bottom_edge) / px(1.0)).min(EDGE_PX * scale * 2.0);
        let rows = 0.5 + distance / (EDGE_PX * scale);
        Some(metrics::px_for_scale(
            (metrics::ROW_HEIGHT * rows).min(metrics::ROW_HEIGHT * 3.0),
            scale,
        ))
    } else {
        None
    }
}
