//! Cached tab-strip view. The root supplies all render inputs together;
//! callbacks operate on current editor state through a weak owner handle.

use crate::ui::{
    theme::{metrics, Theme},
    IconButton, IconKind, Tab as UiTab, TabBar,
};
use crate::{FocusTarget, LstGpuiApp};
use gpui::{
    div, prelude::*, rgb, AnyView, App, Bounds, Context, IntoElement, MouseButton, MouseUpEvent, Pixels, Render,
    ScrollHandle, StyleRefinement, WeakEntity, Window,
};
use lst_editor::{EditorCommand as Command, TabId};

#[derive(PartialEq, Eq)]
struct TabProps {
    id: TabId,
    label: String,
    modified: bool,
    missing: bool,
    saving: bool,
    active: bool,
    show_close: bool,
}

#[derive(PartialEq, Eq)]
struct TabStripProps {
    theme: Theme,
    zoom_level: i32,
    tabs: Vec<TabProps>,
    recent_open: bool,
    tab_list_open: bool,
    app_menu_open: bool,
}

pub(crate) struct TabStrip {
    parent: WeakEntity<LstGpuiApp>,
    props: TabStripProps,
    scroll: ScrollHandle,
}

impl LstGpuiApp {
    pub(crate) fn render_tab_strip(&mut self, cx: &mut Context<Self>) -> AnyView {
        let props = TabStripProps {
            theme: self.theme(cx),
            zoom_level: self.zoom_level,
            tabs: self
                .model
                .tabs()
                .iter()
                .enumerate()
                .map(|(ix, tab)| {
                    let active = !self.recent.is_open() && ix == self.model.active_index();
                    TabProps {
                        id: tab.id(),
                        label: self.tab_display_label(ix),
                        modified: tab.modified(),
                        missing: tab.backing_file_missing(),
                        saving: tab.path().is_some_and(|path| self.save_inflight.contains_key(path)),
                        active,
                        show_close: active || self.hovered_tab == Some(ix),
                    }
                })
                .collect(),
            recent_open: self.recent.is_open(),
            tab_list_open: self.workspace_surface == crate::WorkspaceSurface::TabList,
            app_menu_open: self.workspace_surface == crate::WorkspaceSurface::AppMenu,
        };
        let mut changed = false;
        let view = if let Some(view) = &self.tab_strip_view {
            if view.read(cx).props != props {
                changed = true;
                view.update(cx, |view, cx| {
                    view.props = props;
                    cx.notify();
                });
            }
            view.clone()
        } else {
            let parent = cx.entity().downgrade();
            let scroll = self.tab_bar_scroll.clone();
            let view = cx.new(|_| TabStrip { parent, props, scroll });
            self.tab_strip_view = Some(view.clone());
            view
        };
        let view = AnyView::from(view);
        if changed {
            // Notifications raised during the parent render are processed on
            // the next draw. Render changed props immediately in this frame.
            view
        } else {
            view.cached(StyleRefinement::default().w_full().h(self.ui_px(metrics::TAB_HEIGHT)))
        }
    }
}

impl Render for TabStrip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_strip()
    }
}

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

impl TabStrip {
    fn listener<E: ?Sized>(
        &self,
        f: impl Fn(&mut LstGpuiApp, &E, &mut Window, &mut Context<LstGpuiApp>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let parent = self.parent.clone();
        move |event, window, cx| {
            let _ = parent.update(cx, |app, cx| f(app, event, window, cx));
        }
    }
    fn render_tab(&self, ix: usize) -> impl IntoElement {
        let theme = self.props.theme;
        let tab = &self.props.tabs[ix];
        let tab_id = tab.id;
        let tab_name = tab.label.clone();
        let modified = tab.modified;
        let backing_file_missing = tab.missing;
        let saving = tab.saving;
        let active = tab.active;
        let show_close = tab.show_close;
        let close_button: Option<IconButton> = show_close.then(|| {
            IconButton::new(("tab-close", ix), IconKind::Close, theme)
                .emphasized(active)
                .tooltip("Close tab (Ctrl+W)")
                .on_click(self.listener(move |this, _, _window, cx| {
                    this.request_close_tab_at(ix, cx);
                    cx.stop_propagation();
                }))
        });

        UiTab::new(("tab", ix), theme)
            .active(active)
            .separator_before(ix > 0)
            .on_hover(self.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_tab = Some(ix);
                } else if this.hovered_tab == Some(ix) {
                    this.hovered_tab = None;
                }
                cx.notify();
            }))
            .on_click(self.listener(move |this, _, window, cx| {
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
                self.listener(move |this, _: &MouseUpEvent, window, cx| {
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
            .on_drop(self.listener(move |this, drag: &TabDrag, _, cx| {
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
            .when(backing_file_missing, |tab| {
                tab.child(div().flex_none().text_color(rgb(theme.role.error_text)).child("!"))
            })
            .when(saving && !backing_file_missing, |tab| {
                tab.child(div().flex_none().text_color(rgb(theme.role.accent)).child("↻"))
            })
            .when(modified && !saving && !backing_file_missing, |tab| {
                tab.child(div().flex_none().text_color(rgb(theme.role.accent)).child("●"))
            })
            .child(div().min_w_0().truncate().child(tab_name))
    }

    fn render_strip(&self) -> impl IntoElement {
        let theme = self.props.theme;
        let entity = self.parent.clone();
        // Button bounds are captured once per group from the group's
        // children, so no button needs a wrapper element of its own; the
        // former wrappers' side padding is the buttons' margin.
        let recent_button = IconButton::new("recent-files-button", IconKind::Recent, theme)
            .emphasized(self.props.recent_open)
            .tooltip("Open recent (Ctrl+R)")
            .on_click(self.listener(|this, _, window, cx| {
                this.toggle_recent_files_panel(window, cx);
                cx.stop_propagation();
            }));
        let items = (0..self.props.tabs.len())
            .map(|ix| self.render_tab(ix).into_any_element())
            .collect::<Vec<_>>();
        let all_tabs_button = IconButton::new("all-tabs-button", IconKind::ChevronDown, theme)
            .mx_1()
            .emphasized(self.props.tab_list_open)
            .tooltip("Show all open tabs")
            .on_click(self.listener(|this, _, _, cx| {
                this.toggle_tab_list(cx);
                cx.stop_propagation();
            }));
        let new_tab_button = IconButton::new("new-tab-button", IconKind::Plus, theme)
            .mx_2()
            .tooltip("New scratchpad (Ctrl+N)")
            .on_click(self.listener(|this, _, _window, cx| {
                this.request_new_tab(cx);
                cx.stop_propagation();
            }));
        let end_controls = div()
            .flex()
            .h_full()
            .items_center()
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                    let all_tabs = bounds.first().copied();
                    let new_tab = bounds.get(1).copied();
                    let _ = entity.update(cx, |this, _| {
                        this.all_tabs_button_bounds_px = all_tabs;
                        this.new_tab_button_bounds_px = new_tab;
                    });
                }
            })
            .child(all_tabs_button)
            .child(new_tab_button);

        let start_controls = div()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .on_children_prepainted({
                let entity = entity.clone();
                move |bounds: Vec<Bounds<Pixels>>, _window, cx| {
                    let app_menu = bounds.first().copied();
                    let recent = bounds.get(1).copied();
                    let _ = entity.update(cx, |this, _| {
                        this.app_menu_button_bounds_px = app_menu;
                        this.recent_button_bounds_px = recent;
                    });
                }
            })
            .child(
                IconButton::new("app-menu-button", IconKind::Menu, theme)
                    .emphasized(self.props.app_menu_open)
                    .tooltip("Application menu")
                    .on_click(self.listener(|this, _, _, cx| {
                        this.toggle_app_menu(cx);
                        cx.stop_propagation();
                    })),
            )
            .child(recent_button);

        TabBar::new("editor-tabs", theme)
            .start_child(start_controls)
            .end_child(end_controls)
            .track_scroll(&self.scroll)
            .children(items)
    }
}
