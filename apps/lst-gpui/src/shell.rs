use crate::ui::{
    scrollbar::ScrollbarAxis,
    theme::{metrics, typography},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, App, Bounds, Context, CursorStyle, ElementInputHandler,
    InteractiveElement, KeyDownEvent, ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement,
    Pixels, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Window,
};
use lst_editor::{EditorCommand as Command, TabId};

use crate::recent::{RecentFilter, RecentOrigin, RecentPresentation, RecentPreviewState};
use crate::syntax::syntax_mode_for_language;
use crate::viewport::{
    buffer_content_height, code_char_width, code_origin_pad, ensure_wrap_layout, max_unwrapped_line_width,
    paint_viewport, prepare_viewport_paint_state, scroll_left_for, ViewportPaintInput, ViewportPreparation,
    WrapLayoutInput,
};
use crate::workspace_action::attach_workspace_actions;
use crate::{
    diagnostics, runtime::tab_identity, ClosePromptStatus, FocusTarget, LstGpuiApp, QuitReviewDecision,
    QuitReviewItemStatus,
};
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
        let tab_name = self.tab_display_label(ix);
        let modified = tab.modified();
        let backing_file_missing = tab.backing_file_missing();
        let saving = tab.path().is_some_and(|path| self.save_inflight.contains_key(path));
        let active = !self.recent.is_open() && ix == self.model.active_index();
        let show_close = active || self.hovered_tab == Some(ix);
        let close_button: Option<IconButton> = show_close.then(|| {
            IconButton::new(("tab-close", ix), IconKind::Close, theme)
                .emphasized(active)
                .tooltip("Close tab (Ctrl+W)")
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .when(backing_file_missing, |label| {
                        label.child(div().flex_none().text_color(rgb(theme.role.error_text)).child("!"))
                    })
                    .when(saving && !backing_file_missing, |label| {
                        label.child(div().flex_none().text_color(rgb(theme.role.accent)).child("↻"))
                    })
                    .when(modified && !saving && !backing_file_missing, |label| {
                        label.child(div().flex_none().text_color(rgb(theme.role.accent)).child("●"))
                    })
                    .child(div().min_w_0().truncate().child(tab_name)),
            )
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
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_recent_files_panel(window, cx);
                            cx.stop_propagation();
                        })),
                )
        };
        let items = (0..self.model.tab_count())
            .map(|ix| self.render_tab(ix, cx).into_any_element())
            .collect::<Vec<_>>();
        let all_tabs_button = div()
            .flex()
            .flex_none()
            .h(metrics::px_for_scale(metrics::TAB_HEIGHT, scale))
            .px_1()
            .items_center()
            .border_l_1()
            .border_color(rgb(theme.role.border))
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                    let captured = bounds.first().copied();
                    entity.update(cx, |this, _| {
                        this.all_tabs_button_bounds_px = captured;
                    });
                }
            })
            .child(
                IconButton::new("all-tabs-button", IconKind::ChevronDown, theme)
                    .emphasized(self.workspace_surface == crate::WorkspaceSurface::TabList)
                    .tooltip("Show all open tabs")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_tab_list(cx);
                        cx.stop_propagation();
                    })),
            );
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
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.request_new_tab(cx);
                        cx.stop_propagation();
                    })),
            )
            .into_any_element();
        let end_controls = div()
            .flex()
            .h_full()
            .items_center()
            .child(all_tabs_button)
            .child(new_tab_button);

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
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_app_menu(cx);
                                cx.stop_propagation();
                            })),
                    ),
            )
            .child(recent_button);

        TabBar::new("editor-tabs", theme)
            .start_child(start_controls)
            .end_child(end_controls)
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
        let actions_enabled = !find.matches.is_empty() && error.is_none();

        let disclosure = IconButton::new(
            "find-replace-disclosure",
            if show_replace {
                IconKind::ChevronDown
            } else {
                IconKind::ChevronRight
            },
            theme,
        )
        .tooltip(if show_replace { "Hide replace" } else { "Show replace" })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.update_model(cx, true, |model| model.set_find_replace_visible(!show_replace));
            cx.stop_propagation();
        }));

        let query_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .child(disclosure)
            .child(div().flex_1().min_w(px(160.0)).child(self.find_query_input.clone()))
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
                    .disabled(!actions_enabled)
                    .tooltip("Previous match (Shift+F3)")
                    .when(actions_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.execute_model_command(cx, Command::FindPrev);
                            cx.stop_propagation();
                        }))
                    }),
            )
            .child(
                IconButton::new("find-next", IconKind::ChevronDown, theme)
                    .disabled(!actions_enabled)
                    .tooltip("Next match (F3)")
                    .when(actions_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.execute_model_command(cx, Command::FindNext);
                            cx.stop_propagation();
                        }))
                    }),
            )
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
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_model(cx, true, |model| model.close_find_panel());
                        cx.stop_propagation();
                    })),
            );

        let replace_row = show_replace.then(|| {
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
                // Align the replacement field exactly under the query field.
                .child(
                    div()
                        .flex_none()
                        .w(metrics::px_for_scale(metrics::ICON_BUTTON_SIZE, scale)),
                )
                .child(div().flex_1().min_w(px(160.0)).child(self.find_replace_input.clone()))
                .child(
                    IconButton::new("find-replace-one", IconKind::Replace, theme)
                        .disabled(!actions_enabled)
                        .tooltip("Replace current match (Enter in Replace)")
                        .when(actions_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.execute_model_command(cx, Command::ReplaceCurrentMatch);
                                cx.stop_propagation();
                            }))
                        }),
                )
                .child(
                    IconButton::new("find-replace-all", IconKind::ReplaceAll, theme)
                        .disabled(!actions_enabled)
                        .tooltip("Replace all matches (Ctrl+Alt+Enter)")
                        .when(actions_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.execute_model_command(cx, Command::ReplaceAllMatches);
                                cx.stop_propagation();
                            }))
                        }),
                )
        });

        div()
            .key_context("Find")
            .flex_none()
            .flex()
            .flex_col()
            .gap(metrics::px_for_scale(metrics::SHELL_GAP, scale))
            .w(px(650.0))
            .max_w_full()
            .px_3()
            .py_2()
            .rounded_sm()
            .bg(rgb(theme.role.panel_bg))
            .border_1()
            .border_color(rgb(theme.role.border))
            .occlude()
            .child(query_row)
            .children(replace_row)
            .when_some(error, |panel, err| {
                panel.child(
                    div()
                        .pl(metrics::px_for_scale(
                            metrics::ICON_BUTTON_SIZE + metrics::SHELL_GAP,
                            scale,
                        ))
                        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                        .text_color(rgb(theme.role.error_text))
                        .truncate()
                        .child(err),
                )
            })
    }

    fn render_goto_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        div()
            .flex_none()
            .flex()
            .flex_wrap()
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

    fn render_recent_cards_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let filters = self.render_recent_filter_segments(cx);
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
            "0 items".to_string()
        } else if total == 1 {
            "1 item".to_string()
        } else {
            format!("{visible}/{total} items")
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
                                    .child("Recent"),
                            )
                            .child(filters)
                            .child(div().flex_1().min_w(px(180.0)).child(self.recent_query_input.clone()))
                            .child(
                                div()
                                    .flex_none()
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
                                    .tooltip("Close recent (Esc)")
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

    fn render_recent_filter_segments(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let current = self.recent.filter();
        let segments = [RecentFilter::All, RecentFilter::Files, RecentFilter::Scratchpads]
            .into_iter()
            .enumerate()
            .map(|(index, filter)| {
                let active = current == filter;
                div()
                    .id(("recent-filter", index))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(metrics::px_for_scale(26.0, scale))
                    .px_2()
                    .bg(rgb(if active {
                        theme.role.control_bg_hover
                    } else {
                        theme.role.panel_bg
                    }))
                    .text_size(metrics::px_for_scale(11.0, scale))
                    .text_color(rgb(if active { theme.role.text } else { theme.role.text_muted }))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(theme.role.control_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_recent_filter(filter, cx);
                        cx.stop_propagation();
                    }))
                    .child(filter.label())
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .flex()
            .flex_none()
            .overflow_hidden()
            .rounded_sm()
            .border_1()
            .border_color(rgb(theme.role.border))
            .children(segments)
    }

    fn render_recent_quick_picker(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let page = self.recent.page();
        let total = page.total;
        let visible = page.visible.len();
        let selected_index = page.selected_index;
        let empty_message = page.empty_message;
        let searching = self.recent.content_search_pending();
        let entity = cx.entity();
        let recent_scroll = self.recent_scroll.clone();
        let filters = self.render_recent_filter_segments(cx);
        let rows = page
            .visible
            .into_iter()
            .enumerate()
            .map(|(index, path)| {
                let selected = selected_index == Some(index);
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("untitled")
                    .to_string();
                let parent = path
                    .parent()
                    .map(|parent| parent.display().to_string())
                    .unwrap_or_default();
                let origin = self.recent.origin_for_path(&path).unwrap_or(RecentOrigin::Regular);
                div()
                    .id(("recent-quick-row", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(metrics::px_for_scale(42.0, scale))
                    .px_3()
                    .bg(rgb(if selected {
                        theme.role.control_bg_hover
                    } else {
                        theme.role.panel_bg
                    }))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(theme.role.control_bg)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_recent_path(path.clone(), cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(metrics::px_for_scale(12.0, scale))
                                    .text_color(rgb(theme.role.text))
                                    .child(file_name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(metrics::px_for_scale(10.0, scale))
                                    .text_color(rgb(theme.role.text_muted))
                                    .child(parent),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(theme.role.control_bg))
                            .text_size(metrics::px_for_scale(10.0, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child(match origin {
                                RecentOrigin::Regular => "File",
                                RecentOrigin::Scratchpad => "Scratchpad",
                            }),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let count_label = match total {
            0 => "No items".to_string(),
            1 => "1 item".to_string(),
            _ => format!("{total} items"),
        };

        div()
            .id("recent-quick-scrim")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(metrics::px_for_scale(42.0, scale))
            .bg(gpui::rgba(0x00000055))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_recent_files_panel(cx)),
            )
            .child(
                div()
                    .id("recent-quick-picker")
                    .flex()
                    .flex_col()
                    .w(metrics::px_for_scale(680.0, scale))
                    .max_w_full()
                    .overflow_hidden()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .p_2()
                            .child(div().flex_1().min_w_0().child(self.recent_query_input.clone()))
                            .child(
                                IconButton::new("recent-quick-close", IconKind::Close, theme)
                                    .tooltip("Close quick open (Esc)")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_recent_files_panel(cx);
                                        cx.stop_propagation();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_2()
                            .pb_2()
                            .child(filters)
                            .child(
                                div()
                                    .text_size(metrics::px_for_scale(10.0, scale))
                                    .text_color(rgb(theme.role.text_muted))
                                    .child(if searching {
                                        "Searching contents…".to_string()
                                    } else {
                                        count_label
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("recent-quick-scroll")
                            .max_h(metrics::px_for_scale(420.0, scale))
                            .overflow_y_scroll()
                            .track_scroll(&self.recent_scroll)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .on_children_prepainted(
                                        move |bounds: Vec<Bounds<Pixels>>, _window: &mut Window, cx: &mut App| {
                                            let scroll_offset = recent_scroll.offset();
                                            let row_bounds = bounds
                                                .into_iter()
                                                .take(visible)
                                                .map(|mut bounds| {
                                                    bounds.origin -= scroll_offset;
                                                    bounds
                                                })
                                                .collect::<Vec<_>>();
                                            entity.update(cx, move |this, _| {
                                                this.recent.set_card_bounds(row_bounds);
                                            });
                                        },
                                    )
                                    .children(rows)
                                    .when_some(empty_message, |list, message| {
                                        list.child(
                                            div()
                                                .px_3()
                                                .py_3()
                                                .text_size(metrics::px_for_scale(12.0, scale))
                                                .text_color(rgb(theme.role.text_muted))
                                                .child(message),
                                        )
                                    }),
                            ),
                    ),
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
        let origin = self.recent.origin_for_path(&path).unwrap_or(RecentOrigin::Regular);
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
            .on_click(cx.listener(move |this, _, _window, cx| {
                this.open_recent_path(path.clone(), cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_none()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(metrics::px_for_scale(metrics::TAB_TEXT_SIZE, scale))
                            .line_height(metrics::px_for_scale(metrics::TAB_TEXT_LINE_HEIGHT, scale))
                            .text_color(rgb(theme.role.text))
                            .child(file_name),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::px_for_scale(10.0, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child(match origin {
                                RecentOrigin::Regular => "File",
                                RecentOrigin::Scratchpad => "Scratchpad",
                            }),
                    ),
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

    fn render_file_conflict_banner(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(cx);
        let scale = self.ui_scale();
        let notice = self
            .active_file_conflict()
            .cloned()
            .expect("conflict banner is rendered only for an active notice");
        let tab_id = notice.tab_id;
        let identity = notice.path.display().to_string();
        let entity = cx.entity();
        self.file_conflict_button_bounds_px = crate::FileConflictButtonBounds::default();

        let reload = div()
            .flex_none()
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, window, cx| {
                    let captured = bounds.first().copied();
                    entity.update(cx, |this, _| {
                        this.file_conflict_button_bounds_px.reload = captured;
                        this.emit_state_trace(window);
                    });
                }
            })
            .child(
                file_conflict_button("file-conflict-reload", "Reload", false, theme, scale).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.reload_file_conflict(tab_id, cx);
                        this.set_focus(FocusTarget::Editor);
                        window.focus(&this.focus_handle);
                        cx.stop_propagation();
                    },
                )),
            );
        let keep_mine = div()
            .flex_none()
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, window, cx| {
                    let captured = bounds.first().copied();
                    entity.update(cx, |this, _| {
                        this.file_conflict_button_bounds_px.keep_mine = captured;
                        this.emit_state_trace(window);
                    });
                }
            })
            .child(
                file_conflict_button("file-conflict-keep-mine", "Keep Mine", true, theme, scale).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.keep_file_conflict_local(tab_id, cx);
                        this.set_focus(FocusTarget::Editor);
                        window.focus(&this.focus_handle);
                        cx.stop_propagation();
                    },
                )),
            );
        let save_as = div()
            .flex_none()
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, window, cx| {
                    let captured = bounds.first().copied();
                    entity.update(cx, |this, _| {
                        this.file_conflict_button_bounds_px.save_as = captured;
                        this.emit_state_trace(window);
                    });
                }
            })
            .child(
                file_conflict_button("file-conflict-save-as", "Save As…", false, theme, scale).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.save_file_conflict_as(tab_id, cx);
                        this.set_focus(FocusTarget::Editor);
                        window.focus(&this.focus_handle);
                        cx.stop_propagation();
                    },
                )),
            );
        let dismiss = div()
            .flex_none()
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, window, cx| {
                let captured = bounds.first().copied();
                entity.update(cx, |this, _| {
                    this.file_conflict_button_bounds_px.dismiss = captured;
                    this.emit_state_trace(window);
                });
            })
            .child(
                file_conflict_button("file-conflict-dismiss", "Dismiss", false, theme, scale).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.dismiss_file_conflict(tab_id, cx);
                        this.set_focus(FocusTarget::Editor);
                        window.focus(&this.focus_handle);
                        cx.stop_propagation();
                    },
                )),
            );

        div()
            .id("file-conflict-banner")
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .min_h(metrics::px_for_scale(42.0, scale))
            .px_3()
            .py_2()
            .border_1()
            .border_color(rgb(theme.role.error_text))
            .rounded_sm()
            .bg(rgb(theme.role.panel_bg))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child("This file changed on disk"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(metrics::px_for_scale(11.0, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child(identity),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(reload)
                    .child(save_as)
                    .child(dismiss)
                    .child(keep_mine),
            )
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        self.theme_name_rendered = theme.name.to_string();
        let status_details = self.status_details();
        self.status_details_rendered = status_details.clone();
        self.cleanup_button_bounds_px = None;
        self.theme_button_bounds_px = None;
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
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.toggle_language_menu(cx);
                                cx.stop_propagation();
                            }))
                            .child(
                                self.model
                                    .active_tab()
                                    .language()
                                    .map(|language| format!("{language:?}"))
                                    .unwrap_or_else(|| "Plain Text".to_string()),
                            ),
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
        let prompt = self
            .close_prompt
            .as_ref()
            .expect("close prompt is rendered only while open");
        let identity = self
            .close_prompt
            .as_ref()
            .and_then(|prompt| self.model.tab_by_id(prompt.tab_id))
            .map(tab_identity)
            .unwrap_or_else(|| "this file".to_string());
        let saving = matches!(prompt.status, ClosePromptStatus::Saving);
        let failure = match &prompt.status {
            ClosePromptStatus::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let button = |id: &'static str, label: &'static str, emphasized: bool, enabled: bool| {
            div()
                .id(id)
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
                } else if !enabled {
                    theme.role.text_muted
                } else {
                    theme.role.text
                }))
                .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                .when(enabled, |button| {
                    button
                        .cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                })
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
                    .w(metrics::px_for_scale(560.0, scale))
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
                            .child("Save changes before closing?"),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child(identity),
                    )
                    .when(saving, |panel| {
                        panel.child(
                            div()
                                .text_size(metrics::px_for_scale(12.0, scale))
                                .text_color(rgb(theme.role.text_muted))
                                .child("Saving…"),
                        )
                    })
                    .when_some(failure, |panel, message| {
                        panel.child(
                            div()
                                .whitespace_normal()
                                .text_size(metrics::px_for_scale(12.0, scale))
                                .text_color(rgb(theme.role.error_text))
                                .child(format!("Save failed: {message}")),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(button("close-prompt-cancel", "Cancel (Esc)", false, !saving).when(
                                !saving,
                                |button| {
                                    button.on_click(cx.listener(|this, _, _window, cx| this.cancel_close_prompt(cx)))
                                },
                            ))
                            .child(button("close-prompt-discard", "Discard (D)", false, !saving).when(
                                !saving,
                                |button| {
                                    button.on_click(
                                        cx.listener(|this, _, _window, cx| this.confirm_close_prompt_discard(cx)),
                                    )
                                },
                            ))
                            .child(button("close-prompt-save", "Save (Enter)", true, !saving).when(
                                !saving,
                                |button| {
                                    button.on_click(
                                        cx.listener(|this, _, _window, cx| this.confirm_close_prompt_save(cx)),
                                    )
                                },
                            )),
                    ),
            )
    }

    fn render_quit_review(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let review = self
            .quit_review
            .clone()
            .expect("quit review is rendered only while open");
        let running = review.is_running();
        let rows = review
            .items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let selected = index == review.selected_index;
                let decision = match item.decision {
                    QuitReviewDecision::Save => "Save",
                    QuitReviewDecision::Discard => "Discard",
                };
                let (status, status_color) = match item.status {
                    QuitReviewItemStatus::Pending => (
                        if item.decision == QuitReviewDecision::Save {
                            "Pending".to_string()
                        } else {
                            "Will discard".to_string()
                        },
                        theme.role.text_muted,
                    ),
                    QuitReviewItemStatus::Saving => ("Saving…".to_string(), theme.role.accent),
                    QuitReviewItemStatus::Saved => ("Saved".to_string(), theme.role.text_muted),
                    QuitReviewItemStatus::Failed(ref message) => (format!("Failed: {message}"), theme.role.error_text),
                };
                let locked = running || matches!(item.status, QuitReviewItemStatus::Saved);
                div()
                    .id(("quit-review-item", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .border_1()
                    .border_color(rgb(if selected { theme.role.accent } else { theme.role.border }))
                    .rounded_sm()
                    .bg(rgb(if selected {
                        theme.role.control_bg
                    } else {
                        theme.role.panel_bg
                    }))
                    .when(!locked, |row| {
                        row.cursor(CursorStyle::PointingHand)
                            .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_quit_review_item(index, cx);
                                cx.stop_propagation();
                            }))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(metrics::px_for_scale(12.0, scale))
                                    .text_color(rgb(theme.role.text))
                                    .child(item.identity),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(metrics::px_for_scale(11.0, scale))
                                    .text_color(rgb(status_color))
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .min_w(metrics::px_for_scale(76.0, scale))
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .bg(rgb(if item.decision == QuitReviewDecision::Save {
                                theme.role.accent
                            } else {
                                theme.role.control_bg
                            }))
                            .text_size(metrics::px_for_scale(11.0, scale))
                            .text_color(rgb(if item.decision == QuitReviewDecision::Save {
                                theme.role.accent_text
                            } else {
                                theme.role.text_subtle
                            }))
                            .child(decision),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let button = |id: &'static str, label: &'static str, emphasized: bool, enabled: bool| {
            div()
                .id(id)
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
                } else if enabled {
                    theme.role.text
                } else {
                    theme.role.text_muted
                }))
                .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
                .when(enabled, |button| {
                    button
                        .cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                })
                .child(label)
        };

        div()
            .id("quit-review-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000088))
            .occlude()
            .child(
                div()
                    .id("quit-review")
                    .flex()
                    .flex_col()
                    .w(metrics::px_for_scale(720.0, scale))
                    .max_w_full()
                    .max_h(metrics::px_for_scale(560.0, scale))
                    .gap_3()
                    .p_4()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(16.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child("Review changes before quitting"),
                    )
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child("Choose Save or Discard for each document. Use ↑/↓ and Space to change a choice."),
                    )
                    .child(
                        div()
                            .id("quit-review-items")
                            .flex_1()
                            .min_h(px(0.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.quit_review_scroll)
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(rows),
                    )
                    .when_some(review.message, |panel, message| {
                        panel.child(
                            div()
                                .whitespace_normal()
                                .text_size(metrics::px_for_scale(12.0, scale))
                                .text_color(rgb(
                                    if message.starts_with("Some") || message.starts_with("Could not") {
                                        theme.role.error_text
                                    } else {
                                        theme.role.text_muted
                                    },
                                ))
                                .child(message),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                button("quit-review-cancel", "Cancel", false, !running).when(!running, |button| {
                                    button.on_click(cx.listener(|this, _, _, cx| this.cancel_quit_review(cx)))
                                }),
                            )
                            .child(button("quit-review-discard-all", "Discard All", false, !running).when(
                                !running,
                                |button| {
                                    button.on_click(
                                        cx.listener(|this, _, _, cx| this.confirm_quit_review_discard_all(cx)),
                                    )
                                },
                            ))
                            .child(
                                button("quit-review-save-selected", "Save Selected", true, !running).when(
                                    !running,
                                    |button| {
                                        button.on_click(
                                            cx.listener(|this, _, _, cx| this.confirm_quit_review_save_selected(cx)),
                                        )
                                    },
                                ),
                            ),
                    ),
            )
    }

    fn render_cleanup_confirmation(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let identity = self
            .cleanup_confirmation
            .and_then(|confirmation| self.model.tab_by_id(confirmation.tab_id))
            .map(tab_identity)
            .unwrap_or_else(|| "this document".to_string());

        div()
            .id("cleanup-confirmation-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000088))
            .occlude()
            .child(
                div()
                    .id("cleanup-confirmation")
                    .flex()
                    .flex_col()
                    .w(metrics::px_for_scale(560.0, scale))
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
                            .child("Clean up the entire document?"),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child(format!(
                                "No text is selected in {identity}. The complete document will be sent to the configured AI service."
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                file_conflict_button("cleanup-confirm-cancel", "Cancel (Esc)", false, theme, scale)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel_cleanup_confirmation(cx);
                                            this.set_focus(FocusTarget::Editor);
                                            window.focus(&this.focus_handle);
                                        })),
                            )
                            .child(
                                file_conflict_button(
                                    "cleanup-confirm-submit",
                                    "Clean Entire Document (Enter)",
                                    true,
                                    theme,
                                    scale,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_cleanup_whole_document(cx);
                                        this.set_focus(FocusTarget::Editor);
                                        window.focus(&this.focus_handle);
                                    })),
                            ),
                    ),
            )
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.quit_review.is_some() {
            if modal_key_is_unmodified(event) {
                match event.keystroke.key.as_str() {
                    "escape" => self.cancel_quit_review(cx),
                    "enter" => self.confirm_quit_review_save_selected(cx),
                    "d" => self.confirm_quit_review_discard_all(cx),
                    "space" => self.toggle_selected_quit_review_item(cx),
                    "up" => self.move_quit_review_selection(false, cx),
                    "down" => self.move_quit_review_selection(true, cx),
                    _ => {}
                }
            }
            cx.stop_propagation();
            return;
        }
        if self.close_prompt.is_some() {
            if modal_key_is_unmodified(event) {
                match event.keystroke.key.as_str() {
                    "escape" => self.cancel_close_prompt(cx),
                    "enter" => self.confirm_close_prompt_save(cx),
                    "d" => self.confirm_close_prompt_discard(cx),
                    _ => {}
                }
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

    fn on_surface_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.surface_focus_handle.is_focused(window) {
            return;
        }
        if self.quit_review.is_some() {
            if modal_key_is_unmodified(event) {
                match event.keystroke.key.as_str() {
                    "escape" => self.cancel_quit_review(cx),
                    "enter" => self.confirm_quit_review_save_selected(cx),
                    "d" => self.confirm_quit_review_discard_all(cx),
                    "space" => self.toggle_selected_quit_review_item(cx),
                    "up" => self.move_quit_review_selection(false, cx),
                    "down" => self.move_quit_review_selection(true, cx),
                    _ => {}
                }
            }
        } else if self.close_prompt.is_some() {
            if modal_key_is_unmodified(event) {
                match event.keystroke.key.as_str() {
                    "escape" => self.cancel_close_prompt(cx),
                    "enter" => self.confirm_close_prompt_save(cx),
                    "d" => self.confirm_close_prompt_discard(cx),
                    _ => {}
                }
            }
        } else if self.cleanup_confirmation.is_some() {
            if modal_key_is_unmodified(event) {
                match event.keystroke.key.as_str() {
                    "escape" => self.cancel_cleanup_confirmation(cx),
                    "enter" => self.confirm_cleanup_whole_document(cx),
                    _ => {}
                }
            }
        } else if self.workspace_surface == crate::WorkspaceSurface::Settings {
            self.handle_settings_surface_key_down(event, window, cx);
        } else if matches!(
            self.workspace_surface,
            crate::WorkspaceSurface::TabList
                | crate::WorkspaceSurface::AppMenu
                | crate::WorkspaceSurface::LanguageMenu
                | crate::WorkspaceSurface::ContextMenu
        ) {
            self.handle_workspace_menu_key_down(&event.keystroke.key, cx);
        } else if event.keystroke.key == "escape" {
            self.close_workspace_surface(cx);
        }
        // A visible workspace surface owns the keyboard even when a key has
        // no operation there. Never pass it to the editor underneath.
        cx.stop_propagation();
    }

    fn on_modifiers_changed(&mut self, event: &ModifiersChangedEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.note_modifiers_changed_for_text_input(event);
    }
}

fn modal_key_is_unmodified(event: &KeyDownEvent) -> bool {
    let modifiers = event.keystroke.modifiers;
    !modifiers.control && !modifiers.alt && !modifiers.shift && !modifiers.platform && !modifiers.function
}

impl Render for LstGpuiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(command) = self.pending_workspace_command.take() {
            self.dispatch_workspace_command(command, window, cx);
        }
        self.ensure_active_syntax_state();
        if self.window_title_override.is_none() {
            let active_tab = self.model.active_tab();
            let dirty = if active_tab.modified() || active_tab.backing_file_missing() {
                "● "
            } else {
                ""
            };
            window.set_window_title(&format!("{dirty}{} — lst", active_tab.display_name()));
        }

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
        let viewport_height = active_geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.height)
            .unwrap_or_else(|| metrics::px_for_scale(metrics::WINDOW_HEIGHT - 120.0, scale));
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
        let drop_cursor = self.selection_drag_drop_char();
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
            buffer_content_height(layout.total_rows, scale) + viewport_height * 0.4
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
        let recent_presentation = self.recent.presentation();
        let recent_cards_open = self.recent.is_open() && recent_presentation == RecentPresentation::Cards;
        let recent_quick_open = self.recent.is_open() && recent_presentation == RecentPresentation::Quick;
        let file_conflict_open = !self.recent.is_open() && self.active_file_conflict().is_some();
        if !file_conflict_open {
            self.file_conflict_button_bounds_px = crate::FileConflictButtonBounds::default();
        }
        if !self.model.find().visible {
            self.find_chip_bounds_px = crate::FindChipBounds::default();
        }

        let surface_focus_handle = self.surface_focus_handle.clone();
        let root = attach_workspace_actions(div().flex().flex_col().key_context("Workspace"), cx)
            .size_full()
            .track_focus(&surface_focus_handle)
            .on_key_down(cx.listener(Self::on_surface_key_down))
            .bg(rgb(theme.role.app_bg))
            .text_color(rgb(theme.role.text))
            .child(
                div()
                    .flex_grow()
                    .flex()
                    .flex_col()
                    .px(metrics::px_for_scale(metrics::SHELL_EDGE_PAD, self.ui_scale()))
                    .py(metrics::px_for_scale(metrics::SHELL_EDGE_PAD, self.ui_scale()))
                    .gap_2()
                    .child(self.render_tab_strip(cx))
                    .when(file_conflict_open, |column| {
                        column.child(self.render_file_conflict_banner(cx))
                    })
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
                                    .when(recent_cards_open, |viewport| {
                                        viewport.child(self.render_recent_cards_view(cx))
                                    })
                                    .when(!recent_cards_open, |viewport| {
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
                                                            this.on_right_mouse_down(event, cx);
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
                                                                        drop_cursor,
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
                                    })
                                    .when(recent_quick_open, |viewport| {
                                        viewport.child(self.render_recent_quick_picker(cx))
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
            .when(self.workspace_surface == crate::WorkspaceSurface::TabList, |root| {
                root.child(self.render_tab_list(cx))
            })
            .when(self.workspace_surface == crate::WorkspaceSurface::AppMenu, |root| {
                root.child(self.render_app_menu(cx))
            })
            .when(
                self.workspace_surface == crate::WorkspaceSurface::LanguageMenu,
                |root| root.child(self.render_language_menu(cx)),
            )
            .when(self.workspace_surface == crate::WorkspaceSurface::ContextMenu, |root| {
                root.child(self.render_context_menu(window, cx))
            })
            .when(self.close_prompt.is_some(), |root| {
                root.child(self.render_close_prompt(cx))
            })
            .when(self.quit_review.is_some(), |root| {
                root.child(self.render_quit_review(cx))
            })
            .when(self.cleanup_confirmation.is_some(), |root| {
                root.child(self.render_cleanup_confirmation(cx))
            });
        self.schedule_pending_reveal(window, cx);
        self.apply_focus(window, cx);
        if self.recent.is_open()
            || self.close_prompt.is_some()
            || self.quit_review.is_some()
            || self.cleanup_confirmation.is_some()
        {
            self.emit_state_trace(window);
        }
        root
    }
}

fn file_conflict_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    theme: crate::ui::theme::Theme,
    scale: f32,
) -> Stateful<gpui::Div> {
    let background = if primary {
        theme.role.accent
    } else {
        theme.role.control_bg
    };
    let foreground = if primary {
        theme.role.accent_text
    } else {
        theme.role.text
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(metrics::px_for_scale(28.0, scale))
        .px_3()
        .rounded_sm()
        .bg(rgb(background))
        .text_size(metrics::px_for_scale(11.0, scale))
        .text_color(rgb(foreground))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
        .active(move |style| style.opacity(0.82))
        .child(label)
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
                .on_click(cx.listener(move |this, _, _window, cx| {
                    on_click(this, cx);
                    cx.stop_propagation();
                }))
        })
        .text_size(metrics::px_for_scale(metrics::INPUT_TEXT_SIZE, scale))
        .text_color(rgb(fg))
        .child(spec.label)
}
