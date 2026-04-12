use crate::ui::{
    theme::{metrics, role},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, Context, CursorStyle, ElementInputHandler,
    InteractiveElement, KeyDownEvent, MouseButton, MouseUpEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};

use crate::actions::attach_workspace_actions;
use crate::syntax::syntax_mode_for_path;
use crate::viewport::{buffer_content_height, paint_viewport, prepare_viewport_paint_state};
use crate::{code_char_width, ensure_wrap_layout, LstGpuiApp};

impl LstGpuiApp {
    fn render_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.model.tab(ix).expect("rendered tab index must exist");
        let active = ix == self.model.active_index();
        let show_close = active || self.hovered_tab == Some(ix);
        let dirty_marker = tab.modified().then_some(
            div()
                .flex_none()
                .text_color(rgb(role::DIRTY))
                .child("•")
                .into_any_element(),
        );
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
                this.persistent_overlay_focus = None;
                this.update_model(cx, true, |model| {
                    model.set_active_tab(ix);
                });
                window.focus(&this.focus_handle);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                    this.persistent_overlay_focus = None;
                    this.request_close_tab_at(ix, cx);
                    window.focus(&this.focus_handle);
                    cx.stop_propagation();
                }),
            )
            .start_slot(dirty_marker)
            .end_slot(close_button.map(IntoElement::into_any_element))
            .child(div().min_w_0().truncate().child(tab.display_name()))
    }

    fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut items = (0..self.model.tab_count())
            .map(|ix| self.render_tab(ix, cx).into_any_element())
            .collect::<Vec<_>>();
        items.push(
            div()
                .flex()
                .flex_none()
                .h(px(metrics::TAB_HEIGHT))
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
        let find = self.model.find();
        let match_label = if find.matches.is_empty() {
            "0/0".to_string()
        } else {
            format!("{}/{}", find.current + 1, find.matches.len())
        };

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(metrics::SHELL_GAP))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(role::PANEL_BG))
            .border_1()
            .border_color(rgb(role::BORDER))
            .child(
                div()
                    .flex_none()
                    .text_size(px(metrics::INPUT_TEXT_SIZE))
                    .text_color(rgb(role::TEXT_SUBTLE))
                    .child("Find"),
            )
            .child(div().w(px(280.0)).child(self.find_query_input.clone()))
            .when(find.show_replace, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(px(metrics::INPUT_TEXT_SIZE))
                        .text_color(rgb(role::TEXT_SUBTLE))
                        .child("Replace"),
                )
                .child(div().w(px(280.0)).child(self.find_replace_input.clone()))
            })
            .child(
                div()
                    .flex_none()
                    .font_family(".ZedMono")
                    .text_size(px(metrics::INPUT_TEXT_SIZE))
                    .text_color(rgb(role::TEXT_MUTED))
                    .child(match_label),
            )
    }

    fn render_goto_bar(&mut self) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(metrics::SHELL_GAP))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(role::PANEL_BG))
            .border_1()
            .border_color(rgb(role::BORDER))
            .child(
                div()
                    .flex_none()
                    .text_size(px(metrics::INPUT_TEXT_SIZE))
                    .text_color(rgb(role::TEXT_SUBTLE))
                    .child("Line"),
            )
            .child(div().w(px(180.0)).child(self.goto_line_input.clone()))
    }

    fn render_editor_overlays(&mut self) -> impl IntoElement {
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
            .top(px(metrics::SHELL_GAP))
            .right(px(metrics::SHELL_GAP))
            .flex()
            .flex_col()
            .gap_2()
            .children(overlays)
    }

    fn render_status_bar(&self) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .px_3()
            .py(px(metrics::STATUS_HEIGHT_PAD))
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
                    .font_family(".ZedMono")
                    .text_size(px(12.0))
                    .text_color(rgb(role::TEXT_MUTED))
                    .child(self.status_details()),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
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

        let _ = self.maybe_handle_vim_key(event, cx);
    }
}

impl Render for LstGpuiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_active_syntax_highlights(cx);

        let active = self.model.active_index();
        let show_gutter = self.model.show_gutter();
        let show_wrap = self.model.show_wrap();
        let viewport_width = self.tab_views[active]
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| px(metrics::WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window);
        let (revision, syntax_mode, buffer, selection, cursor_char) = {
            let active_tab = self.model.active_tab();
            (
                active_tab.revision(),
                syntax_mode_for_path(active_tab.path()),
                active_tab.buffer_clone(),
                active_tab.selection(),
                active_tab.cursor_char(),
            )
        };
        let line_texts = self.model.active_tab_lines();
        let total_content_height = {
            let mut cache = self.tab_views[active].cache.borrow_mut();
            let layout = ensure_wrap_layout(
                &mut cache,
                line_texts.as_ref(),
                revision,
                viewport_width,
                char_width,
                show_gutter,
                show_wrap,
            );
            buffer_content_height(layout.total_rows)
        };
        let active_view = &self.tab_views[active];
        let viewport_scroll = active_view.scroll.clone();
        let viewport_cache = active_view.cache.clone();
        let viewport_geometry = active_view.geometry.clone();
        let focus_handle = self.focus_handle.clone();
        let entity = cx.entity();
        let vim_mode = self.model.vim_mode();

        let root = attach_workspace_actions(div().flex().flex_col().key_context("Workspace"), cx)
            .size_full()
            .bg(rgb(role::APP_BG))
            .text_color(rgb(role::TEXT))
            .child(
                div()
                    .flex_grow()
                    .flex()
                    .flex_col()
                    .px(px(metrics::SHELL_EDGE_PAD))
                    .py(px(metrics::SHELL_EDGE_PAD))
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
                                    .font_family(".ZedMono")
                                    .text_size(px(metrics::CODE_FONT_SIZE))
                                    .line_height(px(metrics::ROW_HEIGHT))
                                    .child(
                                        div()
                                            .id("buffer-scroll")
                                            .overflow_y_scroll()
                                            .absolute()
                                            .left_0()
                                            .top_0()
                                            .size_full()
                                            .track_scroll(&viewport_scroll)
                                            .child(div().h(total_content_height).w_full()),
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
                                                    move |bounds, window, _cx| {
                                                        prepare_viewport_paint_state(
                                                            &buffer,
                                                            line_texts.as_ref(),
                                                            revision,
                                                            syntax_mode,
                                                            show_gutter,
                                                            show_wrap,
                                                            &viewport_scroll,
                                                            &viewport_cache,
                                                            &viewport_geometry,
                                                            bounds,
                                                            char_width,
                                                            window,
                                                        )
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
                                                        paint_viewport(
                                                            bounds,
                                                            show_gutter,
                                                            selection.clone(),
                                                            cursor_char,
                                                            vim_mode,
                                                            focus_handle.is_focused(window),
                                                            paint_state,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )
                                                .size_full(),
                                            ),
                                    )
                                    .when(
                                        self.model.find().visible
                                            || self.model.goto_line().is_some(),
                                        |viewport| viewport.child(self.render_editor_overlays()),
                                    ),
                            ),
                    )
                    .child(self.render_status_bar()),
            );
        self.apply_pending_focus(window, cx);
        self.maintain_overlay_focus(window, cx);
        root
    }
}
