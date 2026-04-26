use crate::ui::{
    scrollbar::{
        horizontal_scrollbar_layout, paint_horizontal_scrollbar, paint_vertical_scrollbar,
        scroll_left_for_thumb_drag, scroll_left_for_track_click, scroll_top_for_thumb_drag,
        scroll_top_for_track_click, vertical_scrollbar_layout,
    },
    theme::{metrics, role, typography},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, Context, CursorStyle, ElementInputHandler,
    InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Render, ScrollHandle, StatefulInteractiveElement, Styled, Window,
};

use crate::actions::attach_workspace_actions;
use crate::syntax::syntax_mode_for_language;
use crate::viewport::{
    buffer_content_height, code_origin_pad, paint_viewport, prepare_viewport_paint_state,
    scroll_left_for, scroll_top_for, ViewportPaintInput, ViewportPreparation, WrapLayoutInput,
};
use crate::{
    code_char_width, ensure_wrap_layout, EditorHorizontalScrollbarDrag, EditorScrollbarDrag,
    FocusTarget, LstGpuiApp,
};

impl LstGpuiApp {
    fn render_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.model.tab(ix).expect("rendered tab index must exist");
        let active = ix == self.model.active_index();
        let show_close = active || self.hovered_tab == Some(ix);
        let close_button: Option<IconButton> = show_close.then(|| {
            IconButton::new(("tab-close", ix), IconKind::Close)
                .emphasized(active)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.request_close_tab_at(ix, cx);
                    cx.stop_propagation();
                }))
        });

        UiTab::new(("tab", ix))
            .active(active)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_tab = Some(ix);
                } else if this.hovered_tab == Some(ix) {
                    this.hovered_tab = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_focus(FocusTarget::Editor);
                this.update_model(cx, true, |model| {
                    model.set_active_tab(ix);
                });
                window.focus(&this.focus_handle);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                    this.set_focus(FocusTarget::Editor);
                    this.request_close_tab_at(ix, cx);
                    window.focus(&this.focus_handle);
                    cx.stop_propagation();
                }),
            )
            .end_slot(close_button.map(IntoElement::into_any_element))
            .child(tab.display_name())
    }

    fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let mut items = (0..self.model.tab_count())
            .map(|ix| self.render_tab(ix, cx).into_any_element())
            .collect::<Vec<_>>();
        items.push(
            div()
                .flex()
                .flex_none()
                .h(metrics::px_for_scale(metrics::TAB_HEIGHT, scale))
                .px_2()
                .items_center()
                .border_r_1()
                .border_color(rgb(role::BORDER))
                .child(
                    IconButton::new("new-tab-button", IconKind::Plus).on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.request_new_tab(cx);
                            cx.stop_propagation();
                        },
                    )),
                )
                .into_any_element(),
        );

        TabBar::new("editor-tabs")
            .track_scroll(&self.tab_bar_scroll)
            .children(items)
    }

    fn render_find_bar(&mut self) -> impl IntoElement {
        let scale = self.ui_scale();
        let find = self.model.find();
        let match_label = if find.matches.is_empty() {
            "0/0".to_string()
        } else {
            let active = find.active.map_or(0, |index| index + 1);
            format!("{}/{}", active, find.matches.len())
        };

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(role::PANEL_BG))
            .border_1()
            .border_color(rgb(role::BORDER))
            .child(
                div()
                    .flex_none()
                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                    .text_color(rgb(role::TEXT_SUBTLE))
                    .child("Find"),
            )
            .child(div().w(px(280.0)).child(self.find_query_input.clone()))
            .when(find.show_replace, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                        .text_color(rgb(role::TEXT_SUBTLE))
                        .child("Replace"),
                )
                .child(div().w(px(280.0)).child(self.find_replace_input.clone()))
            })
            .child(
                div()
                    .flex_none()
                    .font(typography::primary_font())
                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                    .text_color(rgb(role::TEXT_MUTED))
                    .child(match_label),
            )
    }

    fn render_goto_bar(&mut self) -> impl IntoElement {
        let scale = self.ui_scale();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(role::PANEL_BG))
            .border_1()
            .border_color(rgb(role::BORDER))
            .child(
                div()
                    .flex_none()
                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                    .text_color(rgb(role::TEXT_SUBTLE))
                    .child("Line"),
            )
            .child(div().w(px(180.0)).child(self.goto_line_input.clone()))
    }

    fn render_editor_overlays(&mut self) -> impl IntoElement {
        let scale = self.ui_scale();
        let mut overlays: Vec<AnyElement> = Vec::new();
        if self.model.find().visible {
            overlays.push(self.render_find_bar().into_any_element());
        }
        if self.model.goto_line().is_some() {
            overlays.push(self.render_goto_bar().into_any_element());
        }

        div()
            .id("editor-overlays")
            .absolute()
            .top(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .right(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .flex()
            .flex_col()
            .gap_2()
            .children(overlays)
    }

    fn render_editor_scrollbar(
        &mut self,
        viewport_scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scale = self.ui_scale();
        let track_width = metrics::px_for_scale(metrics::SCROLLBAR_TRACK_WIDTH, scale);
        let has_overflow = viewport_scroll.max_offset().height > px(0.0);
        let prepare_scroll = viewport_scroll.clone();
        let paint_scroll = viewport_scroll;
        let entity = cx.entity();

        div()
            .id("editor-scrollbar")
            .absolute()
            .top_0()
            .right_0()
            .h_full()
            .w(track_width)
            .when(has_overflow, |bar| bar.cursor(CursorStyle::Arrow))
            .child(
                canvas(
                    move |bounds, _, _| {
                        vertical_scrollbar_layout(
                            bounds,
                            scroll_top_for(&prepare_scroll),
                            prepare_scroll.max_offset().height.max(px(0.0)),
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
                                app.editor_scrollbar_drag.is_some(),
                                app.editor_scrollbar_hovered
                                    || layout.thumb_bounds.contains(&window.mouse_position()),
                            )
                        };
                        paint_vertical_scrollbar(&layout, active, hovered, scale, window);

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
                            let current = scroll_top_for(&scroll_for_down);
                            let on_thumb = layout.thumb_bounds.contains(&event.position);
                            let drag = if on_thumb {
                                let grab_offset_y = event.position.y - layout.thumb_bounds.top();
                                Some(EditorScrollbarDrag { grab_offset_y })
                            } else {
                                let target =
                                    scroll_top_for_track_click(&layout, event.position.y, current);
                                let current_x = scroll_for_down.offset().x;
                                scroll_for_down.set_offset(gpui::point(current_x, -target));
                                None
                            };
                            entity_for_down.update(cx, |this, _| {
                                this.set_focus(FocusTarget::Editor);
                                this.selection_drag = None;
                                this.editor_scrollbar_hovered = on_thumb;
                                this.editor_scrollbar_drag = drag;
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

                            let drag = entity_for_move.read(cx).editor_scrollbar_drag;
                            if let Some(drag) = drag {
                                if event.dragging() {
                                    let target = scroll_top_for_thumb_drag(
                                        &layout,
                                        event.position.y,
                                        drag.grab_offset_y,
                                    );
                                    let current_x = scroll_for_move.offset().x;
                                    scroll_for_move.set_offset(gpui::point(current_x, -target));
                                    entity_for_move.update(cx, |this, _| {
                                        this.editor_scrollbar_hovered = true;
                                    });
                                    cx.stop_propagation();
                                    cx.notify(entity_for_move.entity_id());
                                } else {
                                    entity_for_move.update(cx, |this, _| {
                                        this.editor_scrollbar_drag = None;
                                    });
                                    cx.notify(entity_for_move.entity_id());
                                }
                                return;
                            }

                            let hovered = layout.thumb_bounds.contains(&event.position);
                            if entity_for_move.read(cx).editor_scrollbar_hovered != hovered {
                                entity_for_move.update(cx, |this, _| {
                                    this.editor_scrollbar_hovered = hovered;
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
                                entity_for_up.read(cx).editor_scrollbar_drag.is_some();
                            if was_dragging || layout.track_bounds.contains(&event.position) {
                                entity_for_up.update(cx, |this, _| {
                                    this.editor_scrollbar_drag = None;
                                    this.editor_scrollbar_hovered =
                                        layout.thumb_bounds.contains(&event.position);
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

    fn render_editor_horizontal_scrollbar(
        &mut self,
        viewport_scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scale = self.ui_scale();
        let track_height = metrics::px_for_scale(metrics::SCROLLBAR_TRACK_WIDTH, scale);
        let has_overflow = viewport_scroll.max_offset().width > px(0.0);
        let prepare_scroll = viewport_scroll.clone();
        let paint_scroll = viewport_scroll;
        let entity = cx.entity();

        div()
            .id("editor-horizontal-scrollbar")
            .absolute()
            .left_0()
            .bottom_0()
            .right(track_height)
            .h(track_height)
            .when(has_overflow, |bar| bar.cursor(CursorStyle::Arrow))
            .child(
                canvas(
                    move |bounds, _, _| {
                        horizontal_scrollbar_layout(
                            bounds,
                            scroll_left_for(&prepare_scroll),
                            prepare_scroll.max_offset().width.max(px(0.0)),
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
                                app.editor_horizontal_scrollbar_drag.is_some(),
                                app.editor_horizontal_scrollbar_hovered
                                    || layout.thumb_bounds.contains(&window.mouse_position()),
                            )
                        };
                        paint_horizontal_scrollbar(&layout, active, hovered, scale, window);

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
                            let current = scroll_left_for(&scroll_for_down);
                            let on_thumb = layout.thumb_bounds.contains(&event.position);
                            let drag = if on_thumb {
                                let grab_offset_x = event.position.x - layout.thumb_bounds.left();
                                Some(EditorHorizontalScrollbarDrag { grab_offset_x })
                            } else {
                                let target =
                                    scroll_left_for_track_click(&layout, event.position.x, current);
                                let current_y = scroll_for_down.offset().y;
                                scroll_for_down.set_offset(gpui::point(-target, current_y));
                                None
                            };
                            entity_for_down.update(cx, |this, _| {
                                this.set_focus(FocusTarget::Editor);
                                this.selection_drag = None;
                                this.editor_horizontal_scrollbar_hovered = on_thumb;
                                this.editor_horizontal_scrollbar_drag = drag;
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

                            let drag = entity_for_move.read(cx).editor_horizontal_scrollbar_drag;
                            if let Some(drag) = drag {
                                if event.dragging() {
                                    let target = scroll_left_for_thumb_drag(
                                        &layout,
                                        event.position.x,
                                        drag.grab_offset_x,
                                    );
                                    let current_y = scroll_for_move.offset().y;
                                    scroll_for_move.set_offset(gpui::point(-target, current_y));
                                    entity_for_move.update(cx, |this, _| {
                                        this.editor_horizontal_scrollbar_hovered = true;
                                    });
                                    cx.stop_propagation();
                                    cx.notify(entity_for_move.entity_id());
                                } else {
                                    entity_for_move.update(cx, |this, _| {
                                        this.editor_horizontal_scrollbar_drag = None;
                                    });
                                    cx.notify(entity_for_move.entity_id());
                                }
                                return;
                            }

                            let hovered = layout.thumb_bounds.contains(&event.position);
                            if entity_for_move.read(cx).editor_horizontal_scrollbar_hovered
                                != hovered
                            {
                                entity_for_move.update(cx, |this, _| {
                                    this.editor_horizontal_scrollbar_hovered = hovered;
                                });
                                cx.notify(entity_for_move.entity_id());
                            }
                        });

                        let entity_for_up = entity.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if !phase.bubble() || event.button != MouseButton::Left {
                                return;
                            }

                            let was_dragging = entity_for_up
                                .read(cx)
                                .editor_horizontal_scrollbar_drag
                                .is_some();
                            if was_dragging || layout.track_bounds.contains(&event.position) {
                                entity_for_up.update(cx, |this, _| {
                                    this.editor_horizontal_scrollbar_drag = None;
                                    this.editor_horizontal_scrollbar_hovered =
                                        layout.thumb_bounds.contains(&event.position);
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

    fn render_status_bar(&self) -> impl IntoElement {
        let scale = self.ui_scale();
        div()
            .flex_none()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .px_3()
            .py(metrics::px_for_scale(metrics::STATUS_HEIGHT_PAD, scale))
            .bg(rgb(role::PANEL_BG))
            .border_1()
            .border_color(rgb(role::BORDER))
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .text_color(rgb(role::TEXT_SUBTLE))
                    .child(self.model.status().to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .font(typography::primary_font())
                    .text_size(metrics::px_for_scale(12.0, scale))
                    .text_color(rgb(role::TEXT_MUTED))
                    .child(self.status_details()),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.model.goto_line().is_some() {
                self.update_model(cx, true, |model| {
                    model.close_goto_line_panel();
                });
                cx.stop_propagation();
                return;
            }
            if self.model.find().visible {
                self.update_model(cx, true, |model| {
                    model.close_find_panel();
                });
                cx.stop_propagation();
                return;
            }
        }

        let _ = self.maybe_handle_vim_key(event, window, cx);
    }
}

impl Render for LstGpuiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_active_syntax_highlights(cx);

        let show_gutter = self.model.show_gutter();
        let show_wrap = self.model.show_wrap();
        let (active_scroll, active_cache, active_geometry) = {
            let active_view = self.active_view();
            (
                active_view.scroll.clone(),
                active_view.cache.clone(),
                active_view.geometry.clone(),
            )
        };
        let viewport_width = active_geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| {
                metrics::px_for_scale(metrics::WINDOW_WIDTH - 48.0, self.ui_scale())
            });
        let char_width = code_char_width(window, self.ui_scale());
        let show_search_decorations = self.model.find().visible;
        let (
            revision,
            syntax_mode,
            buffer,
            selection,
            search_matches,
            active_search_match,
            cursor_char,
        ) = {
            let active_tab = self.model.active_tab();
            (
                active_tab.revision(),
                syntax_mode_for_language(active_tab.language()),
                active_tab.buffer_clone(),
                active_tab.selected_range(),
                if show_search_decorations {
                    self.model.find_match_ranges()
                } else {
                    Vec::new()
                },
                show_search_decorations
                    .then(|| self.model.active_find_match_range())
                    .flatten(),
                active_tab.cursor_char(),
            )
        };
        let line_texts = self.model.active_tab_lines();
        let total_content_height = {
            let mut cache = active_cache.borrow_mut();
            let layout = ensure_wrap_layout(
                &mut cache,
                WrapLayoutInput {
                    lines: line_texts.as_ref(),
                    revision,
                    viewport_width,
                    char_width,
                    show_gutter,
                    show_wrap,
                    scale: self.ui_scale(),
                },
            );
            buffer_content_height(layout.total_rows, self.ui_scale())
        };
        let total_content_width = (!show_wrap).then(|| {
            let max_chars = {
                let mut cache = active_cache.borrow_mut();
                match cache.max_line_chars {
                    Some((cached_revision, n)) if cached_revision == revision => n,
                    _ => {
                        let n = line_texts
                            .iter()
                            .map(|line| line.chars().count())
                            .max()
                            .unwrap_or(0);
                        cache.max_line_chars = Some((revision, n));
                        n
                    }
                }
            };
            let pad = code_origin_pad(show_gutter, self.ui_scale());
            pad + char_width * (max_chars as f32 + 2.0)
        });
        let viewport_scroll = active_scroll;
        let scrollbar_scroll = viewport_scroll.clone();
        let h_scrollbar_scroll = viewport_scroll.clone();
        let viewport_cache = active_cache;
        let viewport_geometry = active_geometry;
        let focus_handle = self.focus_handle.clone();
        let entity = cx.entity();
        let prepare_entity = entity.clone();
        let vim_mode = self.model.vim_mode();
        let ui_scale = self.ui_scale();

        let root = attach_workspace_actions(div().flex().flex_col().key_context("Workspace"), cx)
            .size_full()
            .bg(rgb(role::APP_BG))
            .text_color(rgb(role::TEXT))
            .font(typography::primary_font())
            .child(
                div()
                    .flex_grow()
                    .flex()
                    .flex_col()
                    .px(metrics::px_for_scale(
                        metrics::SHELL_EDGE_PAD,
                        self.ui_scale(),
                    ))
                    .py(metrics::px_for_scale(
                        metrics::SHELL_EDGE_PAD,
                        self.ui_scale(),
                    ))
                    .gap_2()
                    .child(self.render_tab_strip(cx))
                    .child(
                        div()
                            .flex_grow()
                            .track_focus(&self.focus_handle)
                            .key_context("Editor")
                            .on_key_down(cx.listener(Self::on_key_down))
                            .child(
                                div()
                                    .id("buffer-viewport")
                                    .relative()
                                    .h_full()
                                    .w_full()
                                    .overflow_hidden()
                                    .border_1()
                                    .border_color(rgb(role::BORDER))
                                    .bg(rgb(role::EDITOR_BG))
                                    .font(typography::primary_font())
                                    .text_size(metrics::px_for_scale(
                                        metrics::CODE_FONT_SIZE,
                                        self.ui_scale(),
                                    ))
                                    .line_height(metrics::px_for_scale(
                                        metrics::ROW_HEIGHT,
                                        self.ui_scale(),
                                    ))
                                    .child(
                                        div()
                                            .id("buffer-scroll")
                                            .absolute()
                                            .left_0()
                                            .top_0()
                                            .size_full()
                                            .overflow_x_scroll()
                                            .overflow_y_scroll()
                                            .track_scroll(&viewport_scroll)
                                            .child(match total_content_width {
                                                Some(width) => {
                                                    div().h(total_content_height).w(width)
                                                }
                                                None => div().h(total_content_height).w_full(),
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id("buffer-overlay")
                                            .absolute()
                                            .left_0()
                                            .top_0()
                                            .size_full()
                                            .cursor(CursorStyle::IBeam)
                                            .block_mouse_except_scroll()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(Self::on_mouse_down),
                                            )
                                            .on_mouse_down(
                                                MouseButton::Middle,
                                                cx.listener(Self::on_middle_mouse_down),
                                            )
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(Self::on_mouse_up),
                                            )
                                            .on_mouse_up_out(
                                                MouseButton::Left,
                                                cx.listener(Self::on_mouse_up),
                                            )
                                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                                            .child(
                                                canvas(
                                                    {
                                                        let viewport_scroll =
                                                            viewport_scroll.clone();
                                                        move |bounds, window, cx| {
                                                            let previous_wrap_columns =
                                                                viewport_geometry
                                                                    .borrow()
                                                                    .painted_wrap_columns;
                                                            let paint_state =
                                                                prepare_viewport_paint_state(
                                                                    ViewportPreparation {
                                                                        buffer: &buffer,
                                                                        lines: line_texts.as_ref(),
                                                                        revision,
                                                                        syntax_mode,
                                                                        show_gutter,
                                                                        show_wrap,
                                                                        viewport_scroll:
                                                                            &viewport_scroll,
                                                                        viewport_cache:
                                                                            &viewport_cache,
                                                                        viewport_geometry:
                                                                            &viewport_geometry,
                                                                        bounds,
                                                                        char_width,
                                                                        scale: ui_scale,
                                                                    },
                                                                    window,
                                                                );
                                                            if previous_wrap_columns
                                                                != viewport_geometry
                                                                    .borrow()
                                                                    .painted_wrap_columns
                                                            {
                                                                cx.notify(
                                                                    prepare_entity.entity_id(),
                                                                );
                                                            }
                                                            paint_state
                                                        }
                                                    },
                                                    move |bounds, paint_state, window, cx| {
                                                        window.handle_input(
                                                            &focus_handle,
                                                            ElementInputHandler::new(
                                                                bounds,
                                                                entity.clone(),
                                                            ),
                                                            cx,
                                                        );
                                                        let horizontal_scroll = if show_wrap {
                                                            px(0.0)
                                                        } else {
                                                            scroll_left_for(&viewport_scroll)
                                                        };
                                                        paint_viewport(
                                                            ViewportPaintInput {
                                                                bounds,
                                                                show_gutter,
                                                                selection: selection.clone(),
                                                                search_matches: &search_matches,
                                                                active_search_match:
                                                                    active_search_match.as_ref(),
                                                                cursor_char,
                                                                vim_mode,
                                                                focused: focus_handle
                                                                    .is_focused(window),
                                                                paint_state,
                                                                scale: ui_scale,
                                                                horizontal_scroll,
                                                            },
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )
                                                .size_full(),
                                            ),
                                    )
                                    .child(self.render_editor_scrollbar(scrollbar_scroll, cx))
                                    .when(!show_wrap, |viewport| {
                                        viewport.child(self.render_editor_horizontal_scrollbar(
                                            h_scrollbar_scroll,
                                            cx,
                                        ))
                                    })
                                    .when(
                                        self.model.find().visible
                                            || self.model.goto_line().is_some(),
                                        |viewport| viewport.child(self.render_editor_overlays()),
                                    ),
                            ),
                    )
                    .child(self.render_status_bar()),
            );
        self.schedule_pending_reveal(window, cx);
        self.apply_focus(window, cx);
        root
    }
}
