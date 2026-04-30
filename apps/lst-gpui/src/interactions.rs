use gpui::{
    point, px, Bounds, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window,
};
use lst_editor::{
    selection::{
        drag_selection_range, line_range_at_char, paragraph_range_at_char, word_range_at_char,
    },
    RevealIntent, Selection,
};

use crate::viewport::scroll_top_for;
use std::ops::Range;

use crate::{ui::theme::metrics, FocusTarget, LstGpuiApp};

#[derive(Clone, Debug)]
pub(crate) enum DragSelectionMode {
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
    Paragraph(Range<usize>),
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveDragSelection {
    mode: DragSelectionMode,
    last_point: Point<Pixels>,
    autoscroll_active: bool,
}

impl ActiveDragSelection {
    fn new(mode: DragSelectionMode, last_point: Point<Pixels>) -> Self {
        Self {
            mode,
            last_point,
            autoscroll_active: false,
        }
    }
}

impl LstGpuiApp {
    pub(crate) fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_focus(FocusTarget::Editor);
        window.focus(&self.focus_handle);
        let index = self.active_char_index_for_point(event.position);
        if event.click_count >= 4 {
            let para_range = paragraph_range_at_char(self.active_tab().buffer(), index);
            self.start_drag_selection(
                DragSelectionMode::Paragraph(para_range.clone()),
                event.position,
            );
            self.select_active_range(para_range, cx);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }
        if event.click_count == 3 {
            let line_range = line_range_at_char(self.active_tab().buffer(), index);
            self.start_drag_selection(DragSelectionMode::Line(line_range.clone()), event.position);
            self.select_active_range(line_range, cx);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            let word_range = word_range_at_char(self.active_tab().buffer(), index);
            self.start_drag_selection(DragSelectionMode::Word(word_range.clone()), event.position);
            self.select_active_range(word_range, cx);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }

        self.start_drag_selection(DragSelectionMode::Character, event.position);
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
        self.set_focus(FocusTarget::Editor);
        window.focus(&self.focus_handle);
        self.cancel_drag_selection();

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
        self.update_drag_selection(event, window, cx);
    }

    pub(crate) fn on_mouse_up(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_drag_selection(cx);
    }

    fn start_drag_selection(&mut self, mode: DragSelectionMode, point: Point<Pixels>) {
        self.selection_drag = Some(ActiveDragSelection::new(mode, point));
    }

    fn update_drag_selection(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            self.cancel_drag_selection();
            return;
        }

        let Some(drag) = self.selection_drag.as_mut() else {
            return;
        };
        drag.last_point = event.position;

        if !self.apply_drag_selection_at_point(event.position, cx) {
            return;
        }
        self.schedule_drag_autoscroll(window, cx);
        cx.notify();
    }

    fn finish_drag_selection(&mut self, cx: &mut Context<Self>) {
        self.cancel_drag_selection();
        self.sync_primary_selection(cx);
        cx.notify();
    }

    fn cancel_drag_selection(&mut self) {
        self.selection_drag = None;
    }

    fn apply_drag_selection_at_point(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = self.active_char_index_for_point(position);
        let mode = self.selection_drag.as_ref().map(|drag| drag.mode.clone());
        match mode {
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
            Some(DragSelectionMode::Paragraph(anchor)) => {
                let current = paragraph_range_at_char(self.active_tab().buffer(), index);
                self.select_active_drag_range(anchor, current, cx);
            }
            None => return false,
        }
        self.queue_cursor_reveal(RevealIntent::NearestEdge);
        true
    }

    fn schedule_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.selection_drag.as_ref() else {
            return;
        };
        if drag.autoscroll_active || self.drag_autoscroll_target().is_none() {
            return;
        }
        if let Some(drag) = self.selection_drag.as_mut() {
            drag.autoscroll_active = true;
        }
        cx.on_next_frame(window, |this, window, cx| {
            this.run_drag_autoscroll(window, cx);
        });
    }

    fn run_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.selection_drag.as_mut() else {
            return;
        };
        drag.autoscroll_active = false;

        if let Some(target) = self.drag_autoscroll_target() {
            let current_x = self.active_view().scroll.offset().x;
            self.active_view()
                .scroll
                .set_offset(point(current_x, -target));
            if let Some(position) = self.selection_drag.as_ref().map(|drag| drag.last_point) {
                self.apply_drag_selection_at_point(position, cx);
            }
            cx.notify();
        }
        self.schedule_drag_autoscroll(window, cx);
    }

    fn drag_autoscroll_target(&self) -> Option<Pixels> {
        let position = self.selection_drag.as_ref()?.last_point;
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
            model.set_selection(Selection::from_range(range, false));
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
            model.set_selection(Selection::from_range(selection, reversed));
        });
    }

    #[cfg(test)]
    pub(crate) fn force_stale_drag_selection_for_test(&mut self, point: Point<Pixels>) {
        self.start_drag_selection(DragSelectionMode::Character, point);
    }

    #[cfg(test)]
    pub(crate) fn has_active_drag_selection_for_test(&self) -> bool {
        self.selection_drag.is_some()
    }

    #[cfg(test)]
    pub(crate) fn run_drag_autoscroll_once_for_test(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_drag_autoscroll(window, cx);
        self.cancel_drag_selection();
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
