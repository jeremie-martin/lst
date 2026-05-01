use crate::ui::{
    scrollbar::{
        horizontal_scrollbar_layout, paint_horizontal_scrollbar, paint_vertical_scrollbar,
        scroll_left_for_thumb_drag, scroll_left_for_track_click, scroll_top_for_thumb_drag,
        scroll_top_for_track_click, vertical_scrollbar_layout,
    },
    theme::{metrics, typography},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, App, Bounds, Context, CursorStyle,
    ElementInputHandler, InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Window,
};

use crate::actions::attach_workspace_actions;
use crate::syntax::syntax_mode_for_language;
use crate::viewport::{
    buffer_content_height, paint_viewport, prepare_viewport_paint_state, scroll_left_for,
    scroll_top_for, unwrapped_content_width, ViewportPaintInput, ViewportPreparation,
    WrapLayoutInput,
};
use crate::{
    code_char_width, ensure_wrap_layout, EditorHorizontalScrollbarDrag, EditorScrollbarDrag,
    FocusTarget, LstGpuiApp, RecentPreviewState, RECENT_CARD_BASIS,
};

impl LstGpuiApp {
    fn render_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme();
        let tab = self.model.tab(ix).expect("rendered tab index must exist");
        let active = !self.recent_panel_visible && ix == self.model.active_index();
        let show_close = active || self.hovered_tab == Some(ix);
        let close_button: Option<IconButton> = show_close.then(|| {
            IconButton::new(("tab-close", ix), IconKind::Close, theme)
                .emphasized(active)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.request_close_tab_at(ix, cx);
                    cx.stop_propagation();
                }))
        });

        UiTab::new(("tab", ix), theme)
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
                this.close_recent_files_panel(cx);
                this.force_editor_focus = true;
                this.set_focus(FocusTarget::Editor);
                this.update_model(cx, true, |model| {
                    if let Some(id) = model.tab_id_at(ix) {
                        model.set_active_tab(id);
                    }
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
        let theme = self.theme();
        let recent_button = IconButton::new("recent-files-button", IconKind::Recent, theme)
            .emphasized(self.recent_panel_visible)
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_recent_files_panel(window, cx);
                cx.stop_propagation();
            }));
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
                .border_color(rgb(theme.role.border))
                .child(
                    IconButton::new("new-tab-button", IconKind::Plus, theme).on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.request_new_tab(cx);
                            cx.stop_propagation();
                        },
                    )),
                )
                .into_any_element(),
        );

        TabBar::new("editor-tabs", theme)
            .start_child(recent_button)
            .track_scroll(&self.tab_bar_scroll)
            .children(items)
    }

    fn render_find_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme();
        let find = self.model.find();
        let match_label = if find.matches.is_empty() {
            "0/0".to_string()
        } else {
            let active = find.active.map_or(0, |index| index + 1);
            format!("{}/{}", active, find.matches.len())
        };
        let case_sensitive = find.case_sensitive;
        let whole_word = find.whole_word;
        let use_regex = find.use_regex;
        let in_selection = find.scope.is_selection_for(self.model.active_tab_id());
        let selection_chip_enabled = in_selection || self.model.active_tab().has_selection();
        let error = find.error.clone();
        let show_replace = find.show_replace;

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(theme.role.panel_bg))
            .border_1()
            .border_color(rgb(theme.role.border))
            .child(
                div()
                    .flex_none()
                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                    .text_color(rgb(theme.role.text_subtle))
                    .child("Find"),
            )
            .child(div().w(px(280.0)).child(self.find_query_input.clone()))
            .when(show_replace, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                        .text_color(rgb(theme.role.text_subtle))
                        .child("Replace"),
                )
                .child(div().w(px(280.0)).child(self.find_replace_input.clone()))
            })
            .child(
                div()
                    .flex_none()
                    .font(typography::primary_font())
                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                    .text_color(rgb(theme.role.text_muted))
                    .child(match_label),
            )
            .when_some(error, |row, err| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                        .text_color(rgb(theme.role.error_text))
                        .child(err),
                )
            })
            .child(find_chip(
                "find-chip-case",
                "Aa",
                FindChipState {
                    active: case_sensitive,
                    enabled: true,
                },
                theme,
                scale,
                cx,
                |this, cx| {
                    this.update_model(cx, true, |m| m.toggle_find_case_sensitive());
                },
            ))
            .child(find_chip(
                "find-chip-word",
                "W",
                FindChipState {
                    active: whole_word,
                    enabled: true,
                },
                theme,
                scale,
                cx,
                |this, cx| {
                    this.update_model(cx, true, |m| m.toggle_find_whole_word());
                },
            ))
            .child(find_chip(
                "find-chip-regex",
                ".*",
                FindChipState {
                    active: use_regex,
                    enabled: true,
                },
                theme,
                scale,
                cx,
                |this, cx| {
                    this.update_model(cx, true, |m| m.toggle_find_regex());
                },
            ))
            .child(find_chip(
                "find-chip-scope",
                "In Sel",
                FindChipState {
                    active: in_selection,
                    enabled: selection_chip_enabled,
                },
                theme,
                scale,
                cx,
                |this, cx| {
                    this.update_model(cx, true, |m| m.toggle_find_in_selection());
                },
            ))
    }

    fn render_goto_bar(&mut self) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(theme.role.panel_bg))
            .border_1()
            .border_color(rgb(theme.role.border))
            .child(
                div()
                    .flex_none()
                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                    .text_color(rgb(theme.role.text_subtle))
                    .child("Line"),
            )
            .child(div().w(px(180.0)).child(self.goto_line_input.clone()))
    }

    fn render_recent_files_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme();
        let filtered_paths = self.recent_filtered_paths();
        let total = filtered_paths.len();
        let visible_paths = filtered_paths
            .into_iter()
            .take(self.recent_visible_count)
            .collect::<Vec<_>>();
        let visible = visible_paths.len();
        let has_more = total > visible;
        let empty_message = self.recent_empty_message();
        let content_search_pending = self.recent_content_search_pending();
        let entity = cx.entity();
        let recent_scroll = self.recent_scroll.clone();
        let cards = visible_paths
            .into_iter()
            .enumerate()
            .map(|(ix, path)| {
                self.render_recent_file_card(ix, path, cx)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let count_label = if total == 0 {
            "0 files".to_string()
        } else if total == 1 {
            "1 file".to_string()
        } else {
            format!("{visible}/{total} files")
        };

        div()
            .id("recent-files-view")
            .absolute()
            .left_0()
            .top_0()
            .size_full()
            .bg(rgb(theme.role.panel_bg))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .gap_3()
                    .px_3()
                    .py_3()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(metrics::px_for_scale(metrics::TAB_TEXT_SIZE, scale))
                                    .line_height(metrics::px_for_scale(
                                        metrics::TAB_TEXT_LINE_HEIGHT,
                                        scale,
                                    ))
                                    .text_color(rgb(theme.role.text))
                                    .child("Recent Files"),
                            )
                            .child(div().w(px(360.0)).child(self.recent_query_input.clone()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(metrics::px_for_scale(
                                        metrics::INPUT_TEXT_SIZE,
                                        scale,
                                    ))
                                    .text_color(rgb(theme.role.text_muted))
                                    .child(count_label),
                            )
                            .when(content_search_pending, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_size(metrics::px_for_scale(
                                            metrics::INPUT_TEXT_SIZE,
                                            scale,
                                        ))
                                        .text_color(rgb(theme.role.text_subtle))
                                        .child("Searching contents..."),
                                )
                            })
                            .child(
                                IconButton::new("recent-files-close", IconKind::Close, theme)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.close_recent_files_panel(cx);
                                        cx.stop_propagation();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("recent-files-scroll")
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.recent_scroll)
                            .child(
                                div()
                                    .on_children_prepainted({
                                        let entity = entity.clone();
                                        let recent_scroll = recent_scroll.clone();
                                        move |bounds: Vec<Bounds<Pixels>>,
                                              _window: &mut Window,
                                              cx: &mut App| {
                                            let scroll_offset = recent_scroll.offset();
                                            let card_bounds = bounds
                                                .into_iter()
                                                .take(visible)
                                                .map(|mut bounds| {
                                                    bounds.origin -= scroll_offset;
                                                    bounds
                                                })
                                                .collect::<Vec<_>>();
                                            entity.update(cx, move |this, _| {
                                                this.recent_card_bounds = card_bounds;
                                            });
                                        }
                                    })
                                    .id("recent-files-grid")
                                    .flex()
                                    .flex_wrap()
                                    .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
                                    .children(cards)
                                    .when_some(empty_message, |grid, message| {
                                        grid.child(
                                            div()
                                                .flex_none()
                                                .text_size(metrics::px_for_scale(
                                                    metrics::INPUT_TEXT_SIZE,
                                                    scale,
                                                ))
                                                .text_color(rgb(theme.role.text_muted))
                                                .child(message),
                                        )
                                    }),
                            ),
                    )
                    .when(has_more, |panel| {
                        panel.child(self.render_recent_load_more_button(cx))
                    }),
            )
    }

    fn render_recent_file_card(
        &mut self,
        ix: usize,
        path: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme();
        let selected = self.recent_selected_path.as_ref() == Some(&path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        let parent = path
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        let (preview_text, preview_color) = match self.recent_previews.get(&path) {
            Some(RecentPreviewState::Loaded(text)) => (text.clone(), theme.role.text_subtle),
            Some(RecentPreviewState::Failed(message)) => (
                format!("Preview unavailable: {message}"),
                theme.role.error_text,
            ),
            _ => ("Loading preview...".to_string(), theme.role.text_muted),
        };
        let background = if selected {
            theme.role.control_bg
        } else {
            theme.role.editor_bg
        };
        let hover_background = if selected {
            theme.role.control_bg_hover
        } else {
            theme.role.control_bg
        };
        let border = if selected {
            theme.role.accent
        } else {
            theme.role.border
        };

        div()
            .id(("recent-file-card", ix))
            .relative()
            .flex()
            .flex_col()
            .flex_grow()
            .flex_basis(px(RECENT_CARD_BASIS))
            .min_w(px(220.0))
            .max_w(px(420.0))
            .h(px(156.0))
            .gap_2()
            .px_3()
            .py_3()
            .rounded_sm()
            .bg(rgb(background))
            .border_1()
            .border_color(rgb(border))
            .cursor(CursorStyle::PointingHand)
            .hover(move |style| style.bg(rgb(hover_background)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _window, cx| {
                    this.recent_selected_path = Some(path.clone());
                    this.open_recent_path(path.clone(), cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex_none()
                    .truncate()
                    .text_size(metrics::px_for_scale(metrics::TAB_TEXT_SIZE, scale))
                    .line_height(metrics::px_for_scale(metrics::TAB_TEXT_LINE_HEIGHT, scale))
                    .text_color(rgb(theme.role.text))
                    .child(file_name),
            )
            .child(
                div()
                    .flex_none()
                    .truncate()
                    .text_size(metrics::px_for_scale(11.0, scale))
                    .line_height(metrics::px_for_scale(15.0, scale))
                    .text_color(rgb(theme.role.text_muted))
                    .child(parent),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .whitespace_normal()
                    .line_clamp(6)
                    .text_size(metrics::px_for_scale(11.0, scale))
                    .line_height(metrics::px_for_scale(15.0, scale))
                    .text_color(rgb(preview_color))
                    .child(preview_text),
            )
            .children(
                selected.then_some(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.0))
                        .rounded_sm()
                        .bg(rgb(theme.role.accent))
                        .into_any_element(),
                ),
            )
    }

    fn render_recent_load_more_button(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme();
        div()
            .id("recent-files-load-more")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(theme.role.control_bg))
            .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
            .cursor(CursorStyle::PointingHand)
            .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
            .text_color(rgb(theme.role.text))
            .on_click(cx.listener(|this, _, _window, cx| {
                this.load_more_recent_files(cx);
                cx.stop_propagation();
            }))
            .child("Load more")
    }

    fn render_editor_overlays(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let mut overlays: Vec<AnyElement> = Vec::new();
        if self.model.find().visible {
            overlays.push(self.render_find_bar(cx).into_any_element());
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
        let theme = self.theme();
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
                        paint_vertical_scrollbar(&layout, active, hovered, scale, theme, window);

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
        let theme = self.theme();
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
                        paint_horizontal_scrollbar(&layout, active, hovered, scale, theme, window);

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

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme();
        div()
            .flex_none()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .px_3()
            .py(metrics::px_for_scale(metrics::STATUS_HEIGHT_PAD, scale))
            .bg(rgb(theme.role.panel_bg))
            .border_1()
            .border_color(rgb(theme.role.border))
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .text_color(rgb(theme.role.text_subtle))
                    .child(self.model.status().to_string()),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .child(
                        IconButton::new("theme-toggle-button", IconKind::Theme, theme).on_click(
                            cx.listener(|this, _, _window, cx| {
                                this.cycle_theme(cx);
                                cx.stop_propagation();
                            }),
                        ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child(theme.name),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font(typography::primary_font())
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child(self.status_details()),
                    ),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.recent_panel_visible {
                self.close_recent_files_panel(cx);
                cx.stop_propagation();
                return;
            }
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
        let gutter_mode = self.model.gutter_mode();
        let theme = self.theme();
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
        let char_width = code_char_width(window, self.ui_scale(), theme);
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
        let cursor_line = self.model.active_tab().cursor_position().line;
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
            let mut cache = active_cache.borrow_mut();
            unwrapped_content_width(
                &mut cache,
                line_texts.as_ref(),
                revision,
                char_width,
                show_gutter,
                self.ui_scale(),
                theme,
                window,
            )
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
            .bg(rgb(theme.role.app_bg))
            .text_color(rgb(theme.role.text))
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
                                    .border_color(rgb(theme.role.border))
                                    .bg(rgb(theme.role.editor_bg))
                                    .font(typography::primary_font())
                                    .text_size(metrics::px_for_scale(
                                        metrics::CODE_FONT_SIZE,
                                        self.ui_scale(),
                                    ))
                                    .line_height(metrics::px_for_scale(
                                        metrics::ROW_HEIGHT,
                                        self.ui_scale(),
                                    ))
                                    .when(self.recent_panel_visible, |viewport| {
                                        viewport.child(self.render_recent_files_view(cx))
                                    })
                                    .when(!self.recent_panel_visible, |viewport| {
                                        viewport
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
                                                        None => {
                                                            div().h(total_content_height).w_full()
                                                        }
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
                                                                                lines:
                                                                                    line_texts
                                                                                        .as_ref(),
                                                                                revision,
                                                                                syntax_mode,
                                                                                show_gutter,
                                                                                gutter_mode,
                                                                                cursor_line,
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
                                                                                theme,
                                                                            },
                                                                            window,
                                                                        );
                                                                    if previous_wrap_columns
                                                                        != viewport_geometry
                                                                            .borrow()
                                                                            .painted_wrap_columns
                                                                    {
                                                                        cx.notify(
                                                                            prepare_entity
                                                                                .entity_id(),
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
                                                                let horizontal_scroll =
                                                                    if show_wrap {
                                                                        px(0.0)
                                                                    } else {
                                                                        scroll_left_for(
                                                                            &viewport_scroll,
                                                                        )
                                                                    };
                                                                paint_viewport(
                                                                    ViewportPaintInput {
                                                                        bounds,
                                                                        show_gutter,
                                                                        selection:
                                                                            selection.clone(),
                                                                        search_matches:
                                                                            &search_matches,
                                                                        active_search_match:
                                                                            active_search_match
                                                                                .as_ref(),
                                                                        cursor_char,
                                                                        vim_mode,
                                                                        focused: focus_handle
                                                                            .is_focused(window),
                                                                        paint_state,
                                                                        scale: ui_scale,
                                                                        horizontal_scroll,
                                                                        theme,
                                                                    },
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        )
                                                        .size_full(),
                                                    ),
                                            )
                                            .child(self.render_editor_scrollbar(
                                                scrollbar_scroll,
                                                cx,
                                            ))
                                            .when(!show_wrap, |viewport| {
                                                viewport.child(
                                                    self.render_editor_horizontal_scrollbar(
                                                        h_scrollbar_scroll,
                                                        cx,
                                                    ),
                                                )
                                            })
                                            .when(
                                                self.model.find().visible
                                                    || self.model.goto_line().is_some(),
                                                |viewport| {
                                                    viewport.child(self.render_editor_overlays(cx))
                                                },
                                            )
                                    })
                            ),
                    )
                    .child(self.render_status_bar(cx)),
            );
        self.schedule_pending_reveal(window, cx);
        self.apply_focus(window, cx);
        root
    }
}

#[derive(Clone, Copy)]
struct FindChipState {
    active: bool,
    enabled: bool,
}

fn find_chip<F>(
    id: &'static str,
    label: &'static str,
    state: FindChipState,
    theme: crate::ui::theme::Theme,
    scale: f32,
    cx: &mut Context<LstGpuiApp>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut LstGpuiApp, &mut Context<LstGpuiApp>) + 'static,
{
    let bg = if state.active {
        theme.role.accent
    } else {
        theme.role.control_bg
    };
    let hover_bg = if state.active {
        theme.role.accent
    } else {
        theme.role.control_bg_hover
    };
    let fg = if !state.enabled {
        theme.role.text_muted
    } else if state.active {
        theme.role.accent_text
    } else {
        theme.role.text_subtle
    };
    let label_id: SharedString = id.into();
    div()
        .id(label_id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .px(metrics::px_for_scale(8.0, scale))
        .h(metrics::px_for_scale(22.0, scale))
        .min_w(metrics::px_for_scale(26.0, scale))
        .rounded_sm()
        .bg(rgb(bg))
        .when(state.enabled, |s| {
            s.cursor(CursorStyle::PointingHand)
                .hover(|h| h.bg(rgb(hover_bg)))
                .on_click(cx.listener(move |this, _, _window, cx| {
                    on_click(this, cx);
                    cx.stop_propagation();
                }))
        })
        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
        .text_color(rgb(fg))
        .child(label)
}
