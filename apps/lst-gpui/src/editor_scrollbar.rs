use gpui::{
    canvas, div, prelude::*, px, Context, CursorStyle, InteractiveElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, ScrollHandle, Styled,
};

use crate::{
    ui::{
        scrollbar::{
            paint_scrollbar, scroll_for_thumb_drag, scroll_for_track_click, scrollbar_layout,
            ScrollbarAxis,
        },
        theme::metrics,
    },
    viewport::{scroll_left_for, scroll_to_left, scroll_to_top, scroll_top_for},
    EditorScrollbarDrag, FocusTarget, LstGpuiApp,
};

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

        bar.when(has_overflow, |bar| bar.cursor(CursorStyle::Arrow))
            .child(
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
                                app.scrollbar_hovered(axis)
                                    || layout.thumb_bounds.contains(&window.mouse_position()),
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
                                    grab_offset: pointer
                                        - axis.pointer_offset(layout.thumb_bounds.origin),
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

                            let was_dragging =
                                entity_for_up.read(cx).scrollbar_drag(axis).is_some();
                            if was_dragging || layout.track_bounds.contains(&event.position) {
                                entity_for_up.update(cx, |this, _| {
                                    this.set_scrollbar_drag(axis, None);
                                    this.set_scrollbar_hovered(
                                        axis,
                                        layout.thumb_bounds.contains(&event.position),
                                    );
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
