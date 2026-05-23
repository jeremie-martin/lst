use crate::ui::{
    scrollbar::ScrollbarAxis,
    theme::{metrics, typography},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, App, Bounds, Context, CursorStyle, ElementInputHandler,
    InteractiveElement, KeyDownEvent, ModifiersChangedEvent, MouseButton, MouseUpEvent, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use lst_editor::EditorCommand as Command;

use crate::recent::RecentPreviewState;
use crate::syntax::syntax_mode_for_language;
use crate::viewport::{
    buffer_content_height, code_char_width, code_origin_pad, ensure_wrap_layout, max_unwrapped_line_width,
    paint_viewport, prepare_viewport_paint_state, scroll_left_for, ViewportPaintInput, ViewportPreparation,
    WrapLayoutInput,
};
use crate::workspace_action::attach_workspace_actions;
use crate::{diagnostics, FocusTarget, LstGpuiApp, RECENT_CARD_BASIS};
use std::time::Instant;

impl LstGpuiApp {
    fn render_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(cx);
        let tab = self.model.tab(ix).expect("rendered tab index must exist");
        let active = !self.recent.is_open() && ix == self.model.active_index();
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
        let theme = self.theme(cx);
        let recent_button = IconButton::new("recent-files-button", IconKind::Recent, theme)
            .emphasized(self.recent.is_open())
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
        let theme = self.theme(cx);
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
            .key_context("Find")
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
            .occlude()
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
                FindChipSpec {
                    id: "find-chip-case",
                    label: "Aa",
                    kind: FindChipKind::CaseSensitive,
                },
                FindChipState {
                    active: case_sensitive,
                    enabled: true,
                },
                theme,
                scale,
                cx,
                |this, cx| this.execute_model_command(cx, Command::ToggleFindCaseSensitive),
            ))
            .child(find_chip(
                FindChipSpec {
                    id: "find-chip-word",
                    label: "W",
                    kind: FindChipKind::WholeWord,
                },
                FindChipState {
                    active: whole_word,
                    enabled: true,
                },
                theme,
                scale,
                cx,
                |this, cx| this.execute_model_command(cx, Command::ToggleFindWholeWord),
            ))
            .child(find_chip(
                FindChipSpec {
                    id: "find-chip-regex",
                    label: ".*",
                    kind: FindChipKind::Regex,
                },
                FindChipState {
                    active: use_regex,
                    enabled: true,
                },
                theme,
                scale,
                cx,
                |this, cx| this.execute_model_command(cx, Command::ToggleFindRegex),
            ))
            .child(find_chip(
                FindChipSpec {
                    id: "find-chip-scope",
                    label: "In Sel",
                    kind: FindChipKind::Scope,
                },
                FindChipState {
                    active: in_selection,
                    enabled: selection_chip_enabled,
                },
                theme,
                scale,
                cx,
                |this, cx| this.execute_model_command(cx, Command::ToggleFindInSelection),
            ))
    }

    fn render_goto_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
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
        let theme = self.theme(cx);
        let page = self.recent.page();
        let total = page.total;
        let visible = page.visible.len();
        let has_more = total > visible;
        let selected_index = page.selected_index;
        let empty_message = page.empty_message;
        let content_search_pending = self.recent.content_search_pending();
        let entity = cx.entity();
        let recent_scroll = self.recent_scroll.clone();
        let cards = page
            .visible
            .into_iter()
            .enumerate()
            .map(|(ix, path)| {
                self.render_recent_file_card(ix, path, selected_index == Some(ix), cx)
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
                                    .line_height(metrics::px_for_scale(metrics::TAB_TEXT_LINE_HEIGHT, scale))
                                    .text_color(rgb(theme.role.text))
                                    .child("Recent Files"),
                            )
                            .child(div().w(px(360.0)).child(self.recent_query_input.clone()))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                                    .text_color(rgb(theme.role.text_muted))
                                    .child(count_label),
                            )
                            .when(content_search_pending, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                                        .text_color(rgb(theme.role.text_subtle))
                                        .child("Searching contents..."),
                                )
                            })
                            .child(IconButton::new("recent-files-close", IconKind::Close, theme).on_click(
                                cx.listener(|this, _, _window, cx| {
                                    this.close_recent_files_panel(cx);
                                    cx.stop_propagation();
                                }),
                            )),
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
                                        move |bounds: Vec<Bounds<Pixels>>, _window: &mut Window, cx: &mut App| {
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
                                                this.recent.set_card_bounds(card_bounds);
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
                                                .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                                                .text_color(rgb(theme.role.text_muted))
                                                .child(message),
                                        )
                                    }),
                            ),
                    )
                    .when(has_more, |panel| panel.child(self.render_recent_load_more_button(cx))),
            )
    }

    fn render_recent_file_card(
        &mut self,
        ix: usize,
        path: std::path::PathBuf,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("untitled")
            .to_string();
        let parent = path
            .parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default();
        let (preview_text, preview_color) = match self.recent.preview(&path) {
            Some(RecentPreviewState::Loaded(text)) => (text.clone(), theme.role.text_subtle),
            Some(RecentPreviewState::Failed(message)) => {
                (format!("Preview unavailable: {message}"), theme.role.error_text)
            }
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
        let border = if selected { theme.role.accent } else { theme.role.border };

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
        let theme = self.theme(cx);
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
            overlays.push(self.render_goto_bar(cx).into_any_element());
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

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        self.theme_name_rendered = theme.name.to_string();
        let status_details = self.status_details();
        self.status_details_rendered = status_details.clone();
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
                    .child(
                        self.cleanup_message
                            .clone()
                            .unwrap_or_else(|| self.model.status().to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .child({
                        let entity = cx.entity();
                        div()
                            .flex_none()
                            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                                let captured = bounds.first().copied();
                                entity.update(cx, |this, _| {
                                    this.cleanup_button_bounds_px = captured;
                                });
                            })
                            .child(
                                IconButton::new("cleanup-button", IconKind::Sparkle, theme)
                                    .disabled(self.cleanup_in_flight)
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.start_cleanup(cx);
                                        cx.stop_propagation();
                                    })),
                            )
                    })
                    .child({
                        let entity = cx.entity();
                        div()
                            .flex_none()
                            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                                let captured = bounds.first().copied();
                                entity.update(cx, |this, _| {
                                    this.theme_button_bounds_px = captured;
                                });
                            })
                            .child(IconButton::new("theme-toggle-button", IconKind::Theme, theme).on_click(
                                cx.listener(|this, _, _window, cx| {
                                    this.cycle_theme(cx);
                                    cx.stop_propagation();
                                }),
                            ))
                    })
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
                            .child(status_details),
                    ),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            self.x11_ctrl_k_pending = false;
            if self.recent.is_open() {
                self.close_recent_files_panel(cx);
                cx.stop_propagation();
                return;
            }
            if self.model.goto_line().is_some() {
                self.update_model(cx, true, |model| model.close_goto_line_panel());
                cx.stop_propagation();
                return;
            }
            if self.model.find().visible {
                self.update_model(cx, true, |model| model.close_find_panel());
                cx.stop_propagation();
                return;
            }

            // Collapse multi-cursor before handing Esc to Vim. Two presses on a multi-cursor
            // set first collapse extents, then drop secondaries.
            if !self.model.selection_set().is_single() {
                let mut collapsed = false;
                self.update_model(cx, true, |model| collapsed = model.collapse_to_primary());
                if collapsed {
                    cx.stop_propagation();
                    return;
                }
            }
        }

        if self.maybe_handle_recent_modifier_key_action(event, window, cx) {
            return;
        }
        if self.maybe_handle_unmodified_key_action(event, window, cx) {
            return;
        }
        let _ = self.maybe_handle_vim_key(event, window, cx);
    }

    fn on_modifiers_changed(&mut self, event: &ModifiersChangedEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.note_modifiers_changed_for_text_input(event);
    }
}

impl Render for LstGpuiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_active_syntax_state();

        let show_gutter = self.model.show_gutter();
        let show_wrap = self.model.show_wrap();
        let gutter_mode = self.model.gutter_mode();
        let theme = self.theme(cx);
        let scale = self.ui_scale();
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
            .unwrap_or_else(|| metrics::px_for_scale(metrics::WINDOW_WIDTH - 48.0, scale));
        let char_width = {
            let mut cache = active_cache.borrow_mut();
            code_char_width(&mut cache, window, scale, theme)
        };
        let show_search_decorations = self.model.find().visible;
        let (revision, syntax_mode, buffer, selection_set, search_matches, active_search_match) = {
            let active_tab = self.model.active_tab();
            (
                active_tab.revision(),
                syntax_mode_for_language(active_tab.language()),
                active_tab.buffer().clone(),
                active_tab.selection_set().clone(),
                if show_search_decorations {
                    self.model.find_match_ranges()
                } else {
                    Vec::new()
                },
                show_search_decorations
                    .then(|| self.model.active_find_match_range())
                    .flatten(),
            )
        };
        let cursor_line = self.model.active_tab().cursor_position().line;
        let cursor_lines: Vec<usize> = {
            let tab = self.model.active_tab();
            let buffer = tab.buffer();
            let mut lines: Vec<usize> = tab
                .selection_set()
                .as_slice()
                .iter()
                .map(|selection| buffer.char_to_line(selection.head().min(buffer.len_chars())))
                .collect();
            lines.sort_unstable();
            lines.dedup();
            lines
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
                    scale,
                },
            );
            buffer_content_height(layout.total_rows, scale)
        };
        let total_content_width = (!show_wrap).then(|| {
            let mut cache = active_cache.borrow_mut();
            let width = max_unwrapped_line_width(
                &mut cache,
                line_texts.as_ref(),
                revision,
                char_width,
                scale,
                theme,
                window,
            );
            code_origin_pad(show_gutter, scale) + width + char_width * 2.0
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
                    .px(metrics::px_for_scale(metrics::SHELL_EDGE_PAD, self.ui_scale()))
                    .py(metrics::px_for_scale(metrics::SHELL_EDGE_PAD, self.ui_scale()))
                    .gap_2()
                    .child(self.render_tab_strip(cx))
                    .child(
                        div()
                            .flex_grow()
                            .track_focus(&self.focus_handle)
                            .key_context("Editor")
                            .on_key_down(cx.listener(Self::on_key_down))
                            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
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
                                    .text_size(metrics::px_for_scale(metrics::CODE_FONT_SIZE, self.ui_scale()))
                                    .line_height(metrics::px_for_scale(metrics::ROW_HEIGHT, self.ui_scale()))
                                    .when(self.recent.is_open(), |viewport| {
                                        viewport.child(self.render_recent_files_view(cx))
                                    })
                                    .when(!self.recent.is_open(), |viewport| {
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
                                                        Some(width) => div().h(total_content_height).w(width),
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
                                                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                                                    .on_mouse_down(
                                                        MouseButton::Middle,
                                                        cx.listener(Self::on_middle_mouse_down),
                                                    )
                                                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                                                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                                                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                                                    .child(
                                                        canvas(
                                                            {
                                                                let viewport_scroll = viewport_scroll.clone();
                                                                move |bounds, window, cx| {
                                                                    let prepare_started =
                                                                        diagnostics::trace_enabled().then(Instant::now);
                                                                    let previous_wrap_columns =
                                                                        viewport_geometry.borrow().painted_wrap_columns;
                                                                    let paint_state = prepare_viewport_paint_state(
                                                                        ViewportPreparation {
                                                                            buffer: &buffer,
                                                                            lines: line_texts.as_ref(),
                                                                            revision,
                                                                            syntax_mode,
                                                                            show_gutter,
                                                                            gutter_mode,
                                                                            cursor_line,
                                                                            cursor_lines: &cursor_lines,
                                                                            show_wrap,
                                                                            viewport_scroll: &viewport_scroll,
                                                                            viewport_cache: &viewport_cache,
                                                                            viewport_geometry: &viewport_geometry,
                                                                            bounds,
                                                                            char_width,
                                                                            scale: ui_scale,
                                                                            theme,
                                                                        },
                                                                        window,
                                                                    );
                                                                    if let Some(started) = prepare_started {
                                                                        diagnostics::record_ms(
                                                                            "viewport_prepare_ms",
                                                                            started.elapsed().as_secs_f64() * 1000.0,
                                                                        );
                                                                    }
                                                                    if previous_wrap_columns
                                                                        != viewport_geometry
                                                                            .borrow()
                                                                            .painted_wrap_columns
                                                                    {
                                                                        cx.notify(prepare_entity.entity_id());
                                                                    }
                                                                    prepare_entity.update(cx, |this, cx| {
                                                                        if this.status_details()
                                                                            != this.status_details_rendered
                                                                        {
                                                                            cx.notify();
                                                                        }
                                                                        this.emit_state_trace(window);
                                                                    });
                                                                    paint_state
                                                                }
                                                            },
                                                            move |bounds, paint_state, window, cx| {
                                                                let paint_started =
                                                                    diagnostics::trace_enabled().then(Instant::now);
                                                                window.handle_input(
                                                                    &focus_handle,
                                                                    ElementInputHandler::new(bounds, entity.clone()),
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
                                                                        selection_set: selection_set.clone(),
                                                                        search_matches: &search_matches,
                                                                        active_search_match: active_search_match
                                                                            .as_ref(),
                                                                        vim_mode,
                                                                        focused: focus_handle.is_focused(window),
                                                                        paint_state,
                                                                        scale: ui_scale,
                                                                        horizontal_scroll,
                                                                        theme,
                                                                    },
                                                                    window,
                                                                    cx,
                                                                );
                                                                if let Some(started) = paint_started {
                                                                    diagnostics::record_ms(
                                                                        "viewport_paint_ms",
                                                                        started.elapsed().as_secs_f64() * 1000.0,
                                                                    );
                                                                }
                                                            },
                                                        )
                                                        .size_full(),
                                                    ),
                                            )
                                            .child(self.render_editor_scrollbar(
                                                ScrollbarAxis::Vertical,
                                                scrollbar_scroll,
                                                cx,
                                            ))
                                            .when(!show_wrap, |viewport| {
                                                viewport.child(self.render_editor_scrollbar(
                                                    ScrollbarAxis::Horizontal,
                                                    h_scrollbar_scroll,
                                                    cx,
                                                ))
                                            })
                                            .when(
                                                self.model.find().visible || self.model.goto_line().is_some(),
                                                |viewport| viewport.child(self.render_editor_overlays(cx)),
                                            )
                                    }),
                            ),
                    )
                    .child(self.render_status_bar(cx)),
            );
        self.schedule_pending_reveal(window, cx);
        self.apply_focus(window, cx);
        if self.recent.is_open() {
            self.emit_state_trace(window);
        }
        root
    }
}

#[derive(Clone, Copy)]
struct FindChipState {
    active: bool,
    enabled: bool,
}

#[derive(Clone, Copy)]
enum FindChipKind {
    CaseSensitive,
    WholeWord,
    Regex,
    Scope,
}

#[derive(Clone, Copy)]
struct FindChipSpec {
    id: &'static str,
    label: &'static str,
    kind: FindChipKind,
}

fn find_chip<F>(
    spec: FindChipSpec,
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
    let label_id: SharedString = spec.id.into();
    let entity = cx.entity();
    div()
        .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, window, cx| {
            let captured = bounds.first().copied();
            entity.update(cx, |this, _| {
                match spec.kind {
                    FindChipKind::CaseSensitive => {
                        this.find_chip_bounds_px.case_sensitive = captured;
                    }
                    FindChipKind::WholeWord => {
                        this.find_chip_bounds_px.whole_word = captured;
                    }
                    FindChipKind::Regex => {
                        this.find_chip_bounds_px.regex = captured;
                    }
                    FindChipKind::Scope => {
                        this.find_chip_bounds_px.scope = captured;
                    }
                }
                this.emit_state_trace(window);
            });
        })
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
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _window, cx| {
                        on_click(this, cx);
                        cx.stop_propagation();
                    }),
                )
        })
        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
        .text_color(rgb(fg))
        .child(spec.label)
}
