use gpui::{
    point, px, Bounds, Context, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Window,
};
use lst_core::selection::{drag_selection_range, line_range_at_char, word_range_at_char};
use std::ops::Range;

use crate::{LstGpuiApp, ROW_HEIGHT};

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
        window.focus(&self.focus_handle);
        self.drag_last_point = Some(event.position);
        let index = self.active_char_index_for_point(event.position);
        if event.click_count >= 3 {
            let line_range = line_range_at_char(&self.active_tab().buffer, index);
            self.drag_selecting = Some(DragSelectionMode::Line(line_range.clone()));
            self.select_active_range(line_range);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            let word_range = word_range_at_char(&self.active_tab().buffer, index);
            self.drag_selecting = Some(DragSelectionMode::Word(word_range.clone()));
            self.select_active_range(word_range);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }

        self.drag_selecting = Some(DragSelectionMode::Character);
        if event.modifiers.shift {
            self.active_tab_mut().select_to(index);
        } else {
            let tab = self.active_tab_mut();
            tab.move_to(index);
            tab.preferred_column = None;
        }
        self.reveal_active_cursor();
        self.schedule_drag_autoscroll(window, cx);
        cx.notify();
    }

    pub(crate) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drag_last_point = Some(event.position);
        if !self.apply_drag_selection_at_point(event.position) {
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

    fn apply_drag_selection_at_point(&mut self, position: Point<Pixels>) -> bool {
        let index = self.active_char_index_for_point(position);
        match self.drag_selecting.clone() {
            Some(DragSelectionMode::Character) => self.active_tab_mut().select_to(index),
            Some(DragSelectionMode::Word(anchor)) => {
                let current = word_range_at_char(&self.active_tab().buffer, index);
                self.select_active_drag_range(anchor, current);
            }
            Some(DragSelectionMode::Line(anchor)) => {
                let current = line_range_at_char(&self.active_tab().buffer, index);
                self.select_active_drag_range(anchor, current);
            }
            None => return false,
        }
        self.reveal_active_cursor();
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
            self.active_tab().scroll.set_offset(point(px(0.0), -target));
            if let Some(position) = self.drag_last_point {
                self.apply_drag_selection_at_point(position);
            }
            cx.notify();
        }
        self.schedule_drag_autoscroll(window, cx);
    }

    fn drag_autoscroll_target(&self) -> Option<Pixels> {
        let position = self.drag_last_point?;
        let geometry = self.active_tab().geometry.borrow();
        let bounds = geometry.bounds?;
        let delta = drag_autoscroll_delta(position, bounds)?;
        let tab = self.active_tab();
        let current = (-tab.scroll.offset().y).max(px(0.0));
        let max = tab.scroll.max_offset().height.max(px(0.0));
        let target = (current + delta).max(px(0.0)).min(max);
        (target != current).then_some(target)
    }

    fn select_active_range(&mut self, range: Range<usize>) {
        let tab = self.active_tab_mut();
        let end = tab.len_chars();
        tab.selection = range.start.min(end)..range.end.min(end);
        tab.selection_reversed = false;
        tab.preferred_column = None;
        tab.marked_range = None;
    }

    fn select_active_drag_range(&mut self, anchor: Range<usize>, current: Range<usize>) {
        let (selection, reversed) = drag_selection_range(anchor, current);
        let tab = self.active_tab_mut();
        let end = tab.len_chars();
        tab.selection = selection.start.min(end)..selection.end.min(end);
        tab.selection_reversed = reversed;
        tab.preferred_column = None;
        tab.marked_range = None;
    }
}

pub(crate) fn drag_autoscroll_delta(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
) -> Option<Pixels> {
    const EDGE_PX: f32 = 36.0;
    let edge = px(EDGE_PX);
    let top_edge = bounds.top() + edge;
    let bottom_edge = bounds.bottom() - edge;

    if position.y < top_edge {
        let distance = ((top_edge - position.y) / px(1.0)).min(EDGE_PX * 2.0);
        let rows = 0.5 + distance / EDGE_PX;
        Some(-px((ROW_HEIGHT * rows).min(ROW_HEIGHT * 3.0)))
    } else if position.y > bottom_edge {
        let distance = ((position.y - bottom_edge) / px(1.0)).min(EDGE_PX * 2.0);
        let rows = 0.5 + distance / EDGE_PX;
        Some(px((ROW_HEIGHT * rows).min(ROW_HEIGHT * 3.0)))
    } else {
        None
    }
}
