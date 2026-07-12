use crate::ui::{
    scrollbar::ScrollbarAxis,
    theme::{metrics, typography},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, App, Bounds, Context, CursorStyle, ElementInputHandler,
    InteractiveElement, KeyDownEvent, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    Pixels, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use lst_editor::{EditorCommand as Command, TabId};

use crate::recent::RecentPreviewState;
use crate::syntax::syntax_mode_for_language;
use crate::viewport::{
    buffer_content_height, code_char_width, code_origin_pad, ensure_wrap_layout, max_unwrapped_line_width,
    paint_viewport, prepare_viewport_paint_state, scroll_left_for, ViewportPaintInput, ViewportPreparation,
    WrapLayoutInput,
};
use crate::workspace_action::attach_workspace_actions;
use crate::{diagnostics, FocusTarget, LstGpuiApp};
use std::time::Instant;

#[derive(Clone)]
struct TabDrag {
    tab_id: TabId,
    name: String,
    theme: crate::ui::theme::Theme,
}

impl Render for TabDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_sm()
            .border_1()
            .border_color(rgb(self.theme.role.border))
            .bg(rgb(self.theme.role.panel_bg))
            .text_color(rgb(self.theme.role.text))
            .child(self.name.clone())
    }
}

impl LstGpuiApp {
    fn render_tab(&mut self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(cx);
        let tab = self.model.tab(ix).expect("rendered tab index must exist");
        let tab_id = tab.id();
        let tab_name = tab.display_name();
        let active = !self.recent.is_open() && ix == self.model.active_index();
        let show_close = active || self.hovered_tab == Some(ix);
        let close_button: Option<IconButton> = show_close.then(|| {
            IconButton::new(("tab-close", ix), IconKind::Close, theme)
                .emphasized(active)
                .tooltip("Close tab (Ctrl+W)")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _window, cx| {
                        this.request_close_tab_at(ix, cx);
                        cx.stop_propagation();
                    }),
                )
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
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
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                    this.set_focus(FocusTarget::Editor);
                    this.request_close_tab_at(ix, cx);
                    window.focus(&this.focus_handle);
                    cx.stop_propagation();
                }),
            )
            .on_drag(
                TabDrag {
                    tab_id,
                    name: tab_name.clone(),
                    theme,
                },
                |drag: &TabDrag, _, _, cx| cx.new(|_| drag.clone()),
            )
            .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
                let source =
                    (0..this.model.tab_count()).find(|index| this.model.tab_id_at(*index) == Some(drag.tab_id));
                let Some(source) = source else {
                    return;
                };
                let delta = ix as isize - source as isize;
                if delta == 0 {
                    return;
                }
                this.update_model(cx, true, |model| {
                    model.set_active_tab(drag.tab_id);
                    model.execute(Command::MoveActiveTab(delta));
                });
            }))
            .end_slot(close_button.map(IntoElement::into_any_element))
            .child(tab_name)
    }

    fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let entity = cx.entity();
        let recent_button = {
            let entity = entity.clone();
            div()
                .flex_none()
                .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                    let captured = bounds.first().copied();
                    entity.update(cx, |this, _| {
                        this.recent_button_bounds_px = captured;
                    });
                })
                .child(
                    IconButton::new("recent-files-button", IconKind::Recent, theme)
                        .emphasized(self.recent.is_open())
                        .tooltip("Open recent (Ctrl+R)")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.toggle_recent_files_panel(window, cx);
                                cx.stop_propagation();
                            }),
                        ),
                )
        };
        let items = (0..self.model.tab_count())
            .map(|ix| self.render_tab(ix, cx).into_any_element())
            .collect::<Vec<_>>();
        let new_tab_button = div()
            .flex()
            .flex_none()
            .h(metrics::px_for_scale(metrics::TAB_HEIGHT, scale))
            .px_2()
            .items_center()
            .border_r_1()
            .border_color(rgb(theme.role.border))
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                    let captured = bounds.first().copied();
                    entity.update(cx, |this, _| {
                        this.new_tab_button_bounds_px = captured;
                    });
                }
            })
            .child(
                IconButton::new("new-tab-button", IconKind::Plus, theme)
                    .tooltip("New scratchpad (Ctrl+N)")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _window, cx| {
                            this.request_new_tab(cx);
                            cx.stop_propagation();
                        }),
                    ),
            )
            .into_any_element();

        let start_controls = div()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .child(
                div()
                    .flex_none()
                    .on_children_prepainted({
                        let entity = entity.clone();
                        move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                            let captured = bounds.first().copied();
                            entity.update(cx, |this, _| {
                                this.app_menu_button_bounds_px = captured;
                            });
                        }
                    })
                    .child(
                        IconButton::new("app-menu-button", IconKind::Menu, theme)
                            .emphasized(self.workspace_surface == crate::WorkspaceSurface::AppMenu)
                            .tooltip("Application menu")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_app_menu(cx);
                                    cx.stop_propagation();
                                }),
                            ),
                    ),
            )
            .child(recent_button);

        TabBar::new("editor-tabs", theme)
            .start_child(start_controls)
            .end_child(new_tab_button)
            .track_scroll(&self.tab_bar_scroll)
            .active_child(self.model.active_index())
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
            .child(
                IconButton::new("find-previous", IconKind::ChevronUp, theme)
                    .tooltip("Previous match (Shift+F3)")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.execute_model_command(cx, Command::FindPrev);
                            cx.stop_propagation();
                        }),
                    ),
            )
            .child(
                IconButton::new("find-next", IconKind::ChevronDown, theme)
                    .tooltip("Next match (F3)")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.execute_model_command(cx, Command::FindNext);
                            cx.stop_propagation();
                        }),
                    ),
            )
            .when(show_replace, |row| {
                row.child(
                    IconButton::new("find-replace-one", IconKind::Replace, theme)
                        .tooltip("Replace current match (Enter in Replace)")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.execute_model_command(cx, Command::ReplaceCurrentMatch);
                                cx.stop_propagation();
                            }),
                        ),
                )
                .child(
                    IconButton::new("find-replace-all", IconKind::ReplaceAll, theme)
                        .tooltip("Replace all matches (Ctrl+Alt+Enter)")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.execute_model_command(cx, Command::ReplaceAllMatches);
                                cx.stop_propagation();
                            }),
                        ),
                )
            })
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
            .child(
                IconButton::new("find-close", IconKind::Close, theme)
                    .tooltip("Close find (Esc)")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.update_model(cx, true, |model| model.close_find_panel());
                            cx.stop_propagation();
                        }),
                    ),
            )
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
                            .child(
                                IconButton::new("recent-files-close", IconKind::Close, theme)
                                    .tooltip("Close recent files (Esc)")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.close_recent_files_panel(cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
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
            .flex_basis(px(crate::RECENT_CARD_BASIS))
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| {
                    this.load_more_recent_files(cx);
                    cx.stop_propagation();
                }),
            )
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
                                    .tooltip("Clean up text with AI (Ctrl+Shift+R)")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.start_cleanup(cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
                            )
                    })
                    .child(
                        IconButton::new("settings-button", IconKind::Settings, theme)
                            .emphasized(self.workspace_surface == crate::WorkspaceSurface::Settings)
                            .tooltip("Settings (Ctrl+,)")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_settings(window, cx);
                                    cx.stop_propagation();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("language-mode-button")
                            .flex()
                            .items_center()
                            .h(metrics::px_for_scale(22.0, scale))
                            .px_2()
                            .rounded_sm()
                            .text_size(metrics::px_for_scale(11.0, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .cursor(CursorStyle::PointingHand)
                            .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_language_menu(cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                self.model
                                    .active_tab()
                                    .language()
                                    .map(|language| format!("{language:?}"))
                                    .unwrap_or_else(|| "Plain Text".to_string()),
                            ),
                    )
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
                            .child(
                                IconButton::new("theme-toggle-button", IconKind::Theme, theme)
                                    .tooltip("Change theme")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _window, cx| {
                                            this.cycle_theme(cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
                            )
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

    fn render_close_prompt(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let name = self
            .close_prompt
            .and_then(|prompt| self.model.tab_by_id(prompt.tab_id))
            .map(|tab| tab.display_name().to_string())
            .unwrap_or_else(|| "this file".to_string());
        let button = |label: &'static str, emphasized: bool| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(metrics::px_for_scale(30.0, scale))
                .px_3()
                .rounded_sm()
                .border_1()
                .border_color(rgb(if emphasized {
                    theme.role.accent
                } else {
                    theme.role.border
                }))
                .bg(rgb(if emphasized {
                    theme.role.accent
                } else {
                    theme.role.control_bg
                }))
                .text_color(rgb(if emphasized {
                    theme.role.accent_text
                } else {
                    theme.role.text
                }))
                .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                .cursor(CursorStyle::PointingHand)
                .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                .child(label)
        };

        div()
            .id("close-prompt-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000088))
            .occlude()
            .child(
                div()
                    .id("close-prompt")
                    .flex()
                    .flex_col()
                    .w(metrics::px_for_scale(420.0, scale))
                    .max_w_full()
                    .gap_3()
                    .p_4()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(15.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child(format!("Save changes to {name}?")),
                    )
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child("Your changes will be lost if you don't save them."),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("Cancel", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _window, cx| this.cancel_close_prompt(cx)),
                            ))
                            .child(button("Don't Save", false).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _window, cx| this.confirm_close_prompt_discard(cx)),
                            ))
                            .child(button("Save", true).on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _window, cx| this.confirm_close_prompt_save(cx)),
                            )),
                    ),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_prompt.is_some() {
            match event.keystroke.key.as_str() {
                "escape" => self.cancel_close_prompt(cx),
                "enter" => self.confirm_close_prompt_save(cx),
                _ => {}
            }
            cx.stop_propagation();
            return;
        }
        if event.keystroke.key == "escape" {
            self.x11_ctrl_k_pending = false;
            if self.workspace_surface != crate::WorkspaceSurface::None {
                self.close_workspace_surface(cx);
                cx.stop_propagation();
                return;
            }
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

            if self.model.input_mode() == lst_editor::InputMode::Standard {
                self.update_model(cx, true, |model| {
                    model.cancel_standard_selection();
                });
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
        if let Some(command) = self.pending_workspace_command.take() {
            self.dispatch_workspace_command(command, window, cx);
        }
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
        let cursor_visible = self.cursor_visible;
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
                                    .text_size(metrics::px_for_scale(metrics::code_font_size(), self.ui_scale()))
                                    .line_height(metrics::px_for_scale(metrics::row_height(), self.ui_scale()))
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
                                                        MouseButton::Right,
                                                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                                            this.open_context_menu(event.position, cx);
                                                            cx.stop_propagation();
                                                        }),
                                                    )
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
                                                                        cursor_visible,
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
        let root = root
            .when(
                self.workspace_surface == crate::WorkspaceSurface::CommandPalette,
                |root| root.child(self.render_command_palette(cx)),
            )
            .when(self.workspace_surface == crate::WorkspaceSurface::Settings, |root| {
                root.child(self.render_settings(cx))
            })
            .when(self.workspace_surface == crate::WorkspaceSurface::AppMenu, |root| {
                root.child(self.render_app_menu(cx))
            })
            .when(
                self.workspace_surface == crate::WorkspaceSurface::LanguageMenu,
                |root| root.child(self.render_language_menu(cx)),
            )
            .when(self.workspace_surface == crate::WorkspaceSurface::ContextMenu, |root| {
                root.child(self.render_context_menu(cx))
            })
            .when(self.close_prompt.is_some(), |root| {
                root.child(self.render_close_prompt(cx))
            });
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
