use gpui::{
    canvas, div, prelude::*, px, rgb, Context, CursorStyle, ElementInputHandler,
    InteractiveElement, KeyDownEvent, MouseButton, MouseUpEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};
use lst_ui::{
    IconButton, IconKind, Tab as UiTab, TabBar, COLOR_BG, COLOR_BORDER, COLOR_MUTED, COLOR_PEACH,
    COLOR_SUBTEXT, COLOR_SURFACE0, COLOR_SURFACE1, COLOR_TEXT, INPUT_TEXT_SIZE, SHELL_EDGE_PAD,
    SHELL_GAP, STATUS_HEIGHT_PAD, TAB_HEIGHT,
};

use crate::syntax::syntax_mode_for_path;
use crate::viewport::{buffer_content_height, paint_viewport, prepare_viewport_paint_state};
use crate::{
    code_char_width, ensure_wrap_layout, LstGpuiApp, NewTab, PendingFocus, CODE_FONT_SIZE,
    ROW_HEIGHT, WINDOW_WIDTH,
};

impl LstGpuiApp {
    fn render_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = &self.tabs[ix];
        let active = ix == self.active;
        let show_close = active || self.hovered_tab == Some(ix);
        let dirty_marker = tab.modified.then_some(
            div()
                .flex_none()
                .text_color(rgb(COLOR_PEACH))
                .child("•")
                .into_any_element(),
        );
        let close_button: Option<IconButton> = show_close.then(|| {
            IconButton::new(("tab-close", ix), IconKind::Close)
                .emphasized(active)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.close_tab_at(ix, cx);
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
                this.set_active_tab(ix);
                this.status = format!("Switched to {}.", this.active_tab().display_name());
                this.reveal_active_cursor();
                window.focus(&this.focus_handle);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                    this.close_tab_at(ix, cx);
                    window.focus(&this.focus_handle);
                    cx.stop_propagation();
                }),
            )
            .start_slot(dirty_marker)
            .end_slot(close_button.map(IntoElement::into_any_element))
            .child(div().min_w_0().truncate().child(tab.display_name()))
    }

    fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut items = (0..self.tabs.len())
            .map(|ix| self.render_tab(ix, cx).into_any_element())
            .collect::<Vec<_>>();
        items.push(
            div()
                .flex()
                .flex_none()
                .h(px(TAB_HEIGHT))
                .px_2()
                .items_center()
                .border_r_1()
                .border_color(rgb(COLOR_BORDER))
                .child(
                    IconButton::new("new-tab-button", IconKind::Plus).on_click(cx.listener(
                        |this, _, _window, cx| {
                            this.handle_new_tab(&NewTab, _window, cx);
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
        let match_label = if self.find.matches.is_empty() {
            "0/0".to_string()
        } else {
            format!("{}/{}", self.find.current + 1, self.find.matches.len())
        };

        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(SHELL_GAP))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(COLOR_SURFACE0))
            .border_1()
            .border_color(rgb(COLOR_BORDER))
            .child(
                div()
                    .flex_none()
                    .text_size(px(INPUT_TEXT_SIZE))
                    .text_color(rgb(COLOR_SUBTEXT))
                    .child("Find"),
            )
            .child(div().w(px(280.0)).child(self.find_query_input.clone()))
            .when(self.find.show_replace, |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(px(INPUT_TEXT_SIZE))
                        .text_color(rgb(COLOR_SUBTEXT))
                        .child("Replace"),
                )
                .child(div().w(px(280.0)).child(self.find_replace_input.clone()))
            })
            .child(
                div()
                    .flex_none()
                    .font_family(".ZedMono")
                    .text_size(px(INPUT_TEXT_SIZE))
                    .text_color(rgb(COLOR_MUTED))
                    .child(match_label),
            )
    }

    fn render_goto_bar(&mut self) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(SHELL_GAP))
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(COLOR_SURFACE0))
            .border_1()
            .border_color(rgb(COLOR_BORDER))
            .child(
                div()
                    .flex_none()
                    .text_size(px(INPUT_TEXT_SIZE))
                    .text_color(rgb(COLOR_SUBTEXT))
                    .child("Line"),
            )
            .child(div().w(px(180.0)).child(self.goto_line_input.clone()))
    }

    fn render_status_bar(&self) -> impl IntoElement {
        div()
            .flex_none()
            .flex()
            .justify_between()
            .items_center()
            .gap_3()
            .px_3()
            .py(px(STATUS_HEIGHT_PAD))
            .bg(rgb(COLOR_SURFACE0))
            .border_t_1()
            .border_color(rgb(COLOR_BORDER))
            .child(
                div()
                    .truncate()
                    .text_sm()
                    .text_color(rgb(COLOR_SUBTEXT))
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(".ZedMono")
                    .text_size(px(12.0))
                    .text_color(rgb(COLOR_MUTED))
                    .child(self.status_details()),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.goto_line.is_some() {
                self.close_goto_line();
                self.queue_focus(PendingFocus::Editor);
                cx.stop_propagation();
                cx.notify();
                return;
            }
            if self.find.visible {
                self.close_find();
                self.queue_focus(PendingFocus::Editor);
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }

        let _ = self.maybe_handle_vim_key(event, cx);
    }
}

impl Render for LstGpuiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending_focus(window, cx);
        self.ensure_active_syntax_highlights(cx);

        let active = self.active;
        let show_gutter = self.show_gutter;
        let show_wrap = self.show_wrap;
        let viewport_width = self.tabs[active]
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| px(WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window);
        let revision = self.tabs[active].revision();
        let line_texts = self.tabs[active].lines();
        let total_content_height = {
            let mut cache = self.tabs[active].cache.borrow_mut();
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
        let active_tab = &self.tabs[active];
        let syntax_mode = syntax_mode_for_path(active_tab.path.as_ref());
        let buffer = active_tab.buffer.clone();
        let selection = active_tab.selection.clone();
        let cursor_char = active_tab.cursor_char();
        let viewport_scroll = active_tab.scroll.clone();
        let viewport_cache = active_tab.cache.clone();
        let viewport_geometry = active_tab.geometry.clone();
        let focus_handle = self.focus_handle.clone();
        let entity = cx.entity();
        let vim_mode = self.vim.mode;

        div()
            .flex()
            .flex_col()
            .key_context("Workspace")
            .on_action(cx.listener(Self::handle_new_tab))
            .on_action(cx.listener(Self::handle_open_file))
            .on_action(cx.listener(Self::handle_save_file))
            .on_action(cx.listener(Self::handle_save_file_as))
            .on_action(cx.listener(Self::handle_close_active_tab))
            .on_action(cx.listener(Self::handle_next_tab))
            .on_action(cx.listener(Self::handle_prev_tab))
            .on_action(cx.listener(Self::handle_toggle_wrap))
            .on_action(cx.listener(Self::handle_copy_selection))
            .on_action(cx.listener(Self::handle_cut_selection))
            .on_action(cx.listener(Self::handle_paste_clipboard))
            .on_action(cx.listener(Self::handle_move_left))
            .on_action(cx.listener(Self::handle_move_right))
            .on_action(cx.listener(Self::handle_move_word_left))
            .on_action(cx.listener(Self::handle_move_word_right))
            .on_action(cx.listener(Self::handle_move_up))
            .on_action(cx.listener(Self::handle_move_down))
            .on_action(cx.listener(Self::handle_move_page_up))
            .on_action(cx.listener(Self::handle_move_page_down))
            .on_action(cx.listener(Self::handle_move_document_start))
            .on_action(cx.listener(Self::handle_move_document_end))
            .on_action(cx.listener(Self::handle_select_left))
            .on_action(cx.listener(Self::handle_select_right))
            .on_action(cx.listener(Self::handle_select_word_left))
            .on_action(cx.listener(Self::handle_select_word_right))
            .on_action(cx.listener(Self::handle_select_up))
            .on_action(cx.listener(Self::handle_select_down))
            .on_action(cx.listener(Self::handle_select_page_up))
            .on_action(cx.listener(Self::handle_select_page_down))
            .on_action(cx.listener(Self::handle_select_document_start))
            .on_action(cx.listener(Self::handle_select_document_end))
            .on_action(cx.listener(Self::handle_move_line_start))
            .on_action(cx.listener(Self::handle_move_line_end))
            .on_action(cx.listener(Self::handle_select_line_start))
            .on_action(cx.listener(Self::handle_select_line_end))
            .on_action(cx.listener(Self::handle_backspace))
            .on_action(cx.listener(Self::handle_delete_forward))
            .on_action(cx.listener(Self::handle_insert_newline))
            .on_action(cx.listener(Self::handle_insert_tab))
            .on_action(cx.listener(Self::handle_select_all))
            .on_action(cx.listener(Self::handle_undo))
            .on_action(cx.listener(Self::handle_redo))
            .on_action(cx.listener(Self::handle_find_open))
            .on_action(cx.listener(Self::handle_find_open_replace))
            .on_action(cx.listener(Self::handle_find_next))
            .on_action(cx.listener(Self::handle_find_prev))
            .on_action(cx.listener(Self::handle_replace_one))
            .on_action(cx.listener(Self::handle_replace_all))
            .on_action(cx.listener(Self::handle_goto_line_open))
            .on_action(cx.listener(Self::handle_delete_line))
            .on_action(cx.listener(Self::handle_move_line_up))
            .on_action(cx.listener(Self::handle_move_line_down))
            .on_action(cx.listener(Self::handle_duplicate_line))
            .on_action(cx.listener(Self::handle_toggle_comment))
            .on_action(cx.listener(Self::handle_quit))
            .size_full()
            .bg(rgb(COLOR_BG))
            .text_color(rgb(COLOR_TEXT))
            .child(
                div()
                    .flex_grow()
                    .flex()
                    .flex_col()
                    .px(px(SHELL_EDGE_PAD))
                    .py(px(SHELL_EDGE_PAD))
                    .gap_2()
                    .child(self.render_tab_strip(cx))
                    .when(self.find.visible, |shell| {
                        shell.child(self.render_find_bar())
                    })
                    .when(self.goto_line.is_some(), |shell| {
                        shell.child(self.render_goto_bar())
                    })
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
                                    .border_color(rgb(COLOR_BORDER))
                                    .bg(rgb(COLOR_SURFACE1))
                                    .font_family(".ZedMono")
                                    .text_size(px(CODE_FONT_SIZE))
                                    .line_height(px(ROW_HEIGHT))
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
                                    ),
                            ),
                    )
                    .child(self.render_status_bar()),
            )
    }
}
