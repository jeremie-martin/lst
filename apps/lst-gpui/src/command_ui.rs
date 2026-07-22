use crate::{
    ui::{theme::metrics, InputFieldEvent, InputFieldNavigation},
    workspace_action::{command_specs, WorkspaceCommand},
    LstGpuiApp, WorkspaceSurface,
};
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, CursorStyle, IntoElement, MouseButton, Pixels, Point, Styled, Window,
};

const APP_MENU_ITEMS: &[&str] = &[
    "file.new_scratchpad",
    "file.open",
    "file.open_recent",
    "file.save",
    "file.save_as",
    "find.open",
    "find.replace",
    "navigation.goto_line",
    "workbench.command_palette",
    "workbench.settings",
    "file.quit",
];
const CONTEXT_MENU_ITEMS: &[&str] = &[
    "edit.cut",
    "edit.copy",
    "edit.paste",
    "selection.select_all",
    "find.open",
    "find.replace",
    "edit.toggle_line_comment",
    "workbench.command_palette",
];
const LANGUAGES: &[(lst_editor::Language, &str)] = &[
    (lst_editor::Language::Rust, "Rust"),
    (lst_editor::Language::Python, "Python"),
    (lst_editor::Language::JavaScript, "JavaScript"),
    (lst_editor::Language::Jsx, "JavaScript JSX"),
    (lst_editor::Language::TypeScript, "TypeScript"),
    (lst_editor::Language::Tsx, "TypeScript TSX"),
    (lst_editor::Language::Json, "JSON"),
    (lst_editor::Language::Jsonc, "JSON with Comments"),
    (lst_editor::Language::Toml, "TOML"),
    (lst_editor::Language::Yaml, "YAML"),
    (lst_editor::Language::Markdown, "Markdown"),
    (lst_editor::Language::Html, "HTML"),
    (lst_editor::Language::Css, "CSS"),
    (lst_editor::Language::Scss, "SCSS"),
    (lst_editor::Language::Shell, "Shell"),
    (lst_editor::Language::Bash, "Bash"),
    (lst_editor::Language::Zsh, "Zsh"),
];

impl LstGpuiApp {
    pub(crate) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_focus_surfaces(cx);
        self.workspace_surface = WorkspaceSurface::CommandPalette;
        self.command_palette_selected = 0;
        self.command_palette_scroll.scroll_to_item(0);
        self.command_palette_input
            .update(cx, |input, cx| input.set_text("", cx));
        let focus = self.command_palette_input.read(cx).focus_handle();
        window.focus(&focus);
        cx.notify();
    }

    pub(crate) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::Settings {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface = WorkspaceSurface::Settings;
            self.settings_overlay = crate::settings_ui::SettingsOverlay::None;
            self.settings_selection.clear();
            self.settings_search_input
                .update(cx, |input, cx| input.set_text("", cx));
            self.settings_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
            window.focus(&self.settings_search_input.read(cx).focus_handle());
            cx.notify();
        }
    }

    pub(crate) fn toggle_app_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::AppMenu {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface = WorkspaceSurface::AppMenu;
            self.workspace_surface_selected = 0;
            self.workspace_surface_scroll.scroll_to_item(0);
            cx.notify();
        }
    }

    pub(crate) fn toggle_tab_list(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::TabList {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface_selected = self.model.active_index();
            self.workspace_surface_scroll
                .scroll_to_item(self.workspace_surface_selected);
            self.workspace_surface = WorkspaceSurface::TabList;
            cx.notify();
        }
    }

    pub(crate) fn toggle_language_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::LanguageMenu {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface = WorkspaceSurface::LanguageMenu;
            self.workspace_surface_selected = language_mode_index(self.model.active_tab().language_mode());
            self.workspace_surface_scroll
                .scroll_to_item(self.workspace_surface_selected);
            cx.notify();
        }
    }

    pub(crate) fn open_context_menu(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.dismiss_focus_surfaces(cx);
        self.context_menu_position = Some(position);
        self.workspace_surface = WorkspaceSurface::ContextMenu;
        self.workspace_surface_selected = 0;
        self.workspace_surface_scroll.scroll_to_item(0);
        cx.notify();
    }

    fn set_language_mode(&mut self, mode: lst_editor::LanguageMode, cx: &mut Context<Self>) {
        self.workspace_surface = WorkspaceSurface::None;
        self.context_menu_position = None;
        self.force_editor_focus = true;
        self.update_model(cx, true, |model| model.set_active_language_mode(mode));
    }

    pub(crate) fn tab_display_label(&self, index: usize) -> String {
        let Some(tab) = self.model.tab(index) else {
            return String::new();
        };
        let base_name = tab.display_name().to_string();
        let duplicate_name = self
            .model
            .tabs()
            .iter()
            .enumerate()
            .any(|(other_index, other)| other_index != index && other.display_name() == base_name);
        if !duplicate_name {
            return base_name;
        }
        tab.path()
            .and_then(|path| path.parent())
            .and_then(|parent| parent.file_name())
            .map(|parent| format!("{base_name} — {}", parent.to_string_lossy()))
            .unwrap_or(base_name)
    }

    fn commands_for_menu(&self, ids: &[&str]) -> Vec<crate::workspace_action::CommandSpec> {
        let commands = command_specs(&self.settings.settings.keybindings);
        ids.iter()
            .filter_map(|id| commands.iter().find(|command| command.id == *id).cloned())
            .collect()
    }

    pub(crate) fn workspace_surface_item_count(&self) -> usize {
        match self.workspace_surface {
            WorkspaceSurface::TabList => self.model.tab_count(),
            WorkspaceSurface::AppMenu => self.commands_for_menu(APP_MENU_ITEMS).len(),
            WorkspaceSurface::LanguageMenu => LANGUAGES.len() + 2,
            WorkspaceSurface::ContextMenu => self.commands_for_menu(CONTEXT_MENU_ITEMS).len(),
            WorkspaceSurface::None | WorkspaceSurface::CommandPalette | WorkspaceSurface::Settings => 0,
        }
    }

    fn select_workspace_surface_row(&mut self, surface: WorkspaceSurface, index: usize, cx: &mut Context<Self>) {
        if self.workspace_surface != surface || self.workspace_surface_selected == index {
            return;
        }
        self.workspace_surface_selected = index.min(self.workspace_surface_item_count().saturating_sub(1));
        cx.notify();
    }

    fn move_workspace_surface_selection(&mut self, key: &str, cx: &mut Context<Self>) {
        let count = self.workspace_surface_item_count();
        self.workspace_surface_selected = menu_selection_after_key(self.workspace_surface_selected, count, key);
        if count == 0 {
            return;
        }
        self.workspace_surface_scroll
            .scroll_to_item(self.workspace_surface_selected);
        cx.notify();
    }

    fn activate_workspace_surface_selection(&mut self, cx: &mut Context<Self>) {
        let index = self
            .workspace_surface_selected
            .min(self.workspace_surface_item_count().saturating_sub(1));
        match self.workspace_surface {
            WorkspaceSurface::TabList => self.activate_tab_list_index(index, cx),
            WorkspaceSurface::AppMenu => {
                if let Some(command) = self
                    .commands_for_menu(APP_MENU_ITEMS)
                    .get(index)
                    .map(|spec| spec.command)
                {
                    self.queue_palette_command(command, cx);
                }
            }
            WorkspaceSurface::LanguageMenu => {
                if let Some(mode) = language_mode_at(index) {
                    self.set_language_mode(mode, cx);
                }
            }
            WorkspaceSurface::ContextMenu => {
                if let Some(command) = self
                    .commands_for_menu(CONTEXT_MENU_ITEMS)
                    .get(index)
                    .map(|spec| spec.command)
                {
                    self.queue_palette_command(command, cx);
                }
            }
            WorkspaceSurface::None | WorkspaceSurface::CommandPalette | WorkspaceSurface::Settings => {}
        }
    }

    fn activate_tab_list_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(tab_id) = self.model.tab_id_at(index) else {
            return;
        };
        self.workspace_surface = WorkspaceSurface::None;
        self.force_editor_focus = true;
        self.tab_bar_scroll.scroll_to_item(index);
        self.update_model(cx, true, |model| model.set_active_tab(tab_id));
    }

    pub(crate) fn handle_workspace_menu_key_down(&mut self, key: &str, cx: &mut Context<Self>) {
        match key {
            "escape" => self.close_workspace_surface(cx),
            "up" | "down" | "home" | "end" => self.move_workspace_surface_selection(key, cx),
            "enter" => self.activate_workspace_surface_selection(cx),
            _ => {}
        }
    }

    pub(crate) fn close_workspace_surface(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface != WorkspaceSurface::None {
            if self.workspace_surface == WorkspaceSurface::Settings {
                self.settings_overlay = crate::settings_ui::SettingsOverlay::None;
                self.settings_selection.clear();
            }
            self.workspace_surface = WorkspaceSurface::None;
            self.context_menu_position = None;
            self.force_editor_focus = true;
            cx.notify();
        }
    }

    fn dismiss_focus_surfaces(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::Settings {
            self.settings_overlay = crate::settings_ui::SettingsOverlay::None;
            self.settings_selection.clear();
        }
        self.close_recent_files_panel(cx);
        if self.model.find().visible {
            self.update_model(cx, true, |model| model.close_find_panel());
        }
        if self.model.goto_line().is_some() {
            self.update_model(cx, true, |model| model.close_goto_line_panel());
        }
        self.workspace_surface = WorkspaceSurface::None;
    }

    pub(crate) fn handle_command_palette_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(_) => {
                self.command_palette_selected = 0;
                cx.notify();
            }
            InputFieldEvent::Submitted => self.queue_selected_palette_command(cx),
            InputFieldEvent::Cancelled => self.close_workspace_surface(cx),
            InputFieldEvent::NextRequested | InputFieldEvent::Navigate(InputFieldNavigation::Down) => {
                self.move_command_palette_selection(1, cx)
            }
            InputFieldEvent::PreviousRequested | InputFieldEvent::Navigate(InputFieldNavigation::Up) => {
                self.move_command_palette_selection(-1, cx)
            }
        }
    }

    fn filtered_commands(&self, cx: &Context<Self>) -> Vec<crate::workspace_action::CommandSpec> {
        let query = self.command_palette_input.read(cx).text().trim().to_lowercase();
        let mut matches = command_specs(&self.settings.settings.keybindings)
            .into_iter()
            .filter_map(|spec| command_match_score(&spec, &query).map(|score| (score, spec)))
            .collect::<Vec<_>>();
        matches.sort_by(|(left_score, left), (right_score, right)| {
            left_score
                .cmp(right_score)
                .then_with(|| left.category.cmp(right.category))
                .then_with(|| left.title.cmp(&right.title))
        });
        matches.into_iter().map(|(_, spec)| spec).collect()
    }

    fn move_command_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let len = self.filtered_commands(cx).len();
        if len == 0 {
            self.command_palette_selected = 0;
        } else {
            self.command_palette_selected =
                (self.command_palette_selected as isize + delta).rem_euclid(len as isize) as usize;
        }
        self.command_palette_scroll
            .scroll_to_item(self.command_palette_selected);
        cx.notify();
    }

    fn queue_selected_palette_command(&mut self, cx: &mut Context<Self>) {
        let commands = self.filtered_commands(cx);
        let Some(spec) = commands.get(self.command_palette_selected.min(commands.len().saturating_sub(1))) else {
            return;
        };
        self.pending_workspace_command = Some(spec.command);
        self.workspace_surface = WorkspaceSurface::None;
        self.force_editor_focus = true;
        cx.notify();
    }

    fn queue_palette_command(&mut self, command: WorkspaceCommand, cx: &mut Context<Self>) {
        self.pending_workspace_command = Some(command);
        self.workspace_surface = WorkspaceSurface::None;
        self.context_menu_position = None;
        self.force_editor_focus = true;
        cx.notify();
    }

    pub(crate) fn render_command_palette(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let commands = self.filtered_commands(cx);
        let rows = commands
            .into_iter()
            .enumerate()
            .map(|(index, spec)| {
                let selected = index == self.command_palette_selected;
                let hover_bg = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.control_bg_hover
                };
                let command = spec.command;
                let shortcut = preferred_shortcut(&spec.shortcuts);
                div()
                    .id(("command-palette-row", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .h(metrics::px_for_scale(34.0, scale))
                    .px_3()
                    .bg(rgb(if selected {
                        theme.role.control_bg_hover
                    } else {
                        theme.role.panel_bg
                    }))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(hover_bg)))
                    .active(move |style| style.bg(rgb(theme.role.accent)).opacity(0.82))
                    .on_click(cx.listener(move |this, _, _window, cx| this.queue_palette_command(command, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_LG, scale))
                            .text_color(rgb(theme.role.text))
                            .child(spec.title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child(spec.category),
                    )
                    .when_some(shortcut, |row, shortcut| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                                .text_color(rgb(theme.role.text_subtle))
                                .child(shortcut),
                        )
                    })
                    .into_any_element()
            })
            .collect::<Vec<AnyElement>>();

        div()
            .id("command-palette-scrim")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(metrics::px_for_scale(72.0, scale))
            .bg(gpui::rgba(0x00000066))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _window, cx| this.close_workspace_surface(cx)),
            )
            .child(
                div()
                    .id("command-palette")
                    .flex()
                    .flex_col()
                    .w(metrics::px_for_scale(620.0, scale))
                    .max_w_full()
                    .overflow_hidden()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(div().p_2().child(self.command_palette_input.clone()))
                    .child(
                        div()
                            .id("command-palette-results-scroll")
                            .flex()
                            .flex_col()
                            .max_h(px(408.0))
                            .overflow_y_scroll()
                            .track_scroll(&self.command_palette_scroll)
                            .children(rows),
                    ),
            )
    }

    pub(crate) fn render_tab_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let active_index = self.model.active_index();
        let selected_index = self
            .workspace_surface_selected
            .min(self.model.tab_count().saturating_sub(1));
        let tabs = self
            .model
            .tabs()
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                (
                    index,
                    self.tab_display_label(index),
                    tab.path().map(|path| path.to_string_lossy().into_owned()),
                    tab.modified(),
                    tab.backing_file_missing(),
                )
            })
            .collect::<Vec<_>>();
        let rows = tabs
            .into_iter()
            .map(|(index, label, path, modified, missing)| {
                let selected = index == selected_index;
                let active = index == active_index;
                let background = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.panel_bg
                };
                let hover_background = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.control_bg_hover
                };
                div()
                    .id(("all-tabs-row", index))
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(metrics::px_for_scale(38.0, scale))
                    .px_3()
                    .bg(rgb(background))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(hover_background)))
                    .active(move |style| style.bg(rgb(theme.role.accent)).opacity(0.82))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.select_workspace_surface_row(WorkspaceSurface::TabList, index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.activate_tab_list_index(index, cx)))
                    .child(
                        div()
                            .flex_none()
                            .w(metrics::px_for_scale(14.0, scale))
                            .text_color(rgb(theme.role.accent))
                            .child(if active { "✓" } else { "" }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .min_w_0()
                                    .when(missing, |name| {
                                        name.child(div().flex_none().text_color(rgb(theme.role.error_text)).child("!"))
                                    })
                                    .when(modified, |name| {
                                        name.child(div().flex_none().text_color(rgb(theme.role.accent)).child("●"))
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
                                            .text_color(rgb(theme.role.text))
                                            .child(label),
                                    ),
                            )
                            .when_some(path, |column, path| {
                                column.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(metrics::px_for_scale(metrics::UI_TEXT_XS, scale))
                                        .text_color(rgb(theme.role.text_muted))
                                        .child(path),
                                )
                            }),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .id("all-tabs-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_workspace_surface(cx)),
            )
            .child(
                div()
                    .id("all-tabs-menu")
                    .absolute()
                    .top(metrics::px_for_scale(38.0, scale))
                    .right(metrics::px_for_scale(8.0, scale))
                    .w(metrics::px_for_scale(380.0, scale))
                    .max_w_full()
                    .max_h(metrics::px_for_scale(440.0, scale))
                    .overflow_y_scroll()
                    .track_scroll(&self.workspace_surface_scroll)
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
    }

    pub(crate) fn render_app_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let rows = self
            .commands_for_menu(APP_MENU_ITEMS)
            .into_iter()
            .enumerate()
            .map(|(index, spec)| {
                let selected = index == self.workspace_surface_selected;
                let background = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.panel_bg
                };
                let hover_background = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.control_bg_hover
                };
                let command = spec.command;
                div()
                    .id(("app-menu-command", index))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .h(metrics::px_for_scale(32.0, scale))
                    .px_3()
                    .bg(rgb(background))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(hover_background)))
                    .active(move |style| style.bg(rgb(theme.role.accent)).opacity(0.82))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.select_workspace_surface_row(WorkspaceSurface::AppMenu, index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.queue_palette_command(command, cx)))
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
                            .text_color(rgb(theme.role.text))
                            .child(spec.title),
                    )
                    .when_some(preferred_shortcut(&spec.shortcuts), |row, shortcut| {
                        row.child(
                            div()
                                .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                                .text_color(rgb(theme.role.text_muted))
                                .child(shortcut),
                        )
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .id("app-menu-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_workspace_surface(cx)),
            )
            .child(
                div()
                    .id("app-menu")
                    .absolute()
                    .top(metrics::px_for_scale(38.0, scale))
                    .left(metrics::px_for_scale(8.0, scale))
                    .w(metrics::px_for_scale(310.0, scale))
                    .max_h(metrics::px_for_scale(420.0, scale))
                    .overflow_y_scroll()
                    .track_scroll(&self.workspace_surface_scroll)
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
    }

    pub(crate) fn render_language_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let current = self.model.active_tab().language();
        let mode = self.model.active_tab().language_mode();
        let mut rows = Vec::with_capacity(LANGUAGES.len() + 2);
        rows.push(
            language_row(
                "language-auto",
                "Auto-detect",
                mode == lst_editor::LanguageMode::Auto,
                self.workspace_surface_selected == 0,
                theme,
                scale,
            )
            .on_click(cx.listener(|this, _, _, cx| this.set_language_mode(lst_editor::LanguageMode::Auto, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.select_workspace_surface_row(WorkspaceSurface::LanguageMenu, 0, cx);
                }
            }))
            .into_any_element(),
        );
        rows.push(
            language_row(
                "language-plain-text",
                "Plain Text",
                mode == lst_editor::LanguageMode::PlainText,
                self.workspace_surface_selected == 1,
                theme,
                scale,
            )
            .on_click(cx.listener(|this, _, _, cx| this.set_language_mode(lst_editor::LanguageMode::PlainText, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.select_workspace_surface_row(WorkspaceSurface::LanguageMenu, 1, cx);
                }
            }))
            .into_any_element(),
        );
        rows.extend(LANGUAGES.iter().enumerate().map(|(index, (language, label))| {
            let language = *language;
            language_row(
                ("language-choice", index),
                label,
                mode == lst_editor::LanguageMode::Language(language)
                    || (mode == lst_editor::LanguageMode::Auto && current == Some(language)),
                self.workspace_surface_selected == index + 2,
                theme,
                scale,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_language_mode(lst_editor::LanguageMode::Language(language), cx)
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.select_workspace_surface_row(WorkspaceSurface::LanguageMenu, index + 2, cx);
                }
            }))
            .into_any_element()
        }));

        div()
            .id("language-menu-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_workspace_surface(cx)),
            )
            .child(
                div()
                    .id("language-menu")
                    .absolute()
                    .right(metrics::px_for_scale(metrics::FLOATING_MENU_EDGE_INSET, scale))
                    .bottom(metrics::px_for_scale(42.0, scale))
                    .w(metrics::px_for_scale(280.0, scale))
                    .max_h(metrics::px_for_scale(480.0, scale))
                    .overflow_y_scroll()
                    .track_scroll(&self.workspace_surface_scroll)
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
    }

    pub(crate) fn render_context_menu(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let position = self.context_menu_position.unwrap_or_default();
        let commands = self.commands_for_menu(CONTEXT_MENU_ITEMS);
        let menu_width = metrics::px_for_scale(290.0, scale);
        let menu_height = metrics::px_for_scale((commands.len() as f32 * 32.0 + 8.0).min(360.0), scale);
        let viewport = window.viewport_size();
        let left = position.x.min((viewport.width - menu_width).max(px(0.0)));
        let top = position.y.min((viewport.height - menu_height).max(px(0.0)));
        let rows = commands
            .into_iter()
            .enumerate()
            .map(|(index, spec)| {
                let selected = index == self.workspace_surface_selected;
                let background = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.panel_bg
                };
                let hover_background = if selected {
                    theme.role.selection_bg
                } else {
                    theme.role.control_bg_hover
                };
                let command = spec.command;
                div()
                    .id(("context-menu-command", index))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .h(metrics::px_for_scale(32.0, scale))
                    .px_3()
                    .bg(rgb(background))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(hover_background)))
                    .active(move |style| style.bg(rgb(theme.role.accent)).opacity(0.82))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.select_workspace_surface_row(WorkspaceSurface::ContextMenu, index, cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.queue_palette_command(command, cx)))
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
                            .text_color(rgb(theme.role.text))
                            .child(spec.title),
                    )
                    .when_some(preferred_shortcut(&spec.shortcuts), |row, shortcut| {
                        row.child(
                            div()
                                .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                                .text_color(rgb(theme.role.text_muted))
                                .child(shortcut),
                        )
                    })
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .id("context-menu-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_workspace_surface(cx)),
            )
            .child(
                div()
                    .id("editor-context-menu")
                    .absolute()
                    .left(left)
                    .top(top)
                    .w(menu_width)
                    .max_h(metrics::px_for_scale(360.0, scale))
                    .overflow_y_scroll()
                    .track_scroll(&self.workspace_surface_scroll)
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
    }
}

fn language_row(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    checked: bool,
    keyboard_selected: bool,
    theme: crate::ui::theme::Theme,
    scale: f32,
) -> gpui::Stateful<gpui::Div> {
    let hover_bg = if keyboard_selected {
        theme.role.accent
    } else {
        theme.role.control_bg_hover
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .h(metrics::px_for_scale(30.0, scale))
        .px_3()
        .bg(rgb(if keyboard_selected {
            theme.role.selection_bg
        } else {
            theme.role.panel_bg
        }))
        .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
        .text_color(rgb(if keyboard_selected || checked {
            theme.role.text
        } else {
            theme.role.text_subtle
        }))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(hover_bg)))
        .active(move |style| style.bg(rgb(theme.role.accent)).opacity(0.82))
        .child(
            div()
                .flex_none()
                .w(metrics::px_for_scale(18.0, scale))
                .text_color(rgb(theme.role.accent))
                .child(if checked { "✓" } else { "" }),
        )
        .child(label)
}

fn language_mode_index(mode: lst_editor::LanguageMode) -> usize {
    match mode {
        lst_editor::LanguageMode::Auto => 0,
        lst_editor::LanguageMode::PlainText => 1,
        lst_editor::LanguageMode::Language(language) => LANGUAGES
            .iter()
            .position(|(candidate, _)| *candidate == language)
            .map_or(0, |index| index + 2),
    }
}

fn language_mode_at(index: usize) -> Option<lst_editor::LanguageMode> {
    match index {
        0 => Some(lst_editor::LanguageMode::Auto),
        1 => Some(lst_editor::LanguageMode::PlainText),
        index => LANGUAGES
            .get(index - 2)
            .map(|(language, _)| lst_editor::LanguageMode::Language(*language)),
    }
}

fn menu_selection_after_key(current: usize, count: usize, key: &str) -> usize {
    if count == 0 {
        return 0;
    }
    match key {
        "up" => current.min(count - 1).saturating_sub(1),
        "down" => (current.min(count - 1) + 1).min(count - 1),
        "home" => 0,
        "end" => count - 1,
        _ => current.min(count - 1),
    }
}

fn command_match_score(spec: &crate::workspace_action::CommandSpec, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let haystack = format!("{} {} {}", spec.title, spec.category, spec.id).to_lowercase();
    query.split_whitespace().try_fold(0usize, |score, part| {
        fuzzy_subsequence_score(&haystack, part).map(|part_score| score + part_score)
    })
}

fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<usize> {
    if let Some(index) = haystack.find(needle) {
        return Some(index);
    }
    let mut search_from = 0usize;
    let mut score = 0usize;
    let mut previous = None;
    for needle_char in needle.chars() {
        let relative = haystack[search_from..].find(needle_char)?;
        let index = search_from + relative;
        score += previous.map_or(index, |previous| index.saturating_sub(previous + 1));
        previous = Some(index);
        search_from = index + needle_char.len_utf8();
    }
    Some(score + haystack.len().saturating_sub(needle.len()))
}

fn preferred_shortcut(shortcuts: &[String]) -> Option<String> {
    shortcuts
        .iter()
        .find(|shortcut| shortcut.starts_with("ctrl-") || !shortcut.starts_with("cmd-"))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::{fuzzy_subsequence_score, language_mode_at, language_mode_index, menu_selection_after_key, LANGUAGES};

    #[test]
    fn fuzzy_subsequence_prefers_contiguous_matches() {
        let contiguous = fuzzy_subsequence_score("show command palette", "command").unwrap();
        let sparse = fuzzy_subsequence_score("c-o-m-m-a-n-d", "command").unwrap();
        assert!(contiguous < sparse);
        assert!(fuzzy_subsequence_score("show command palette", "scpal").is_some());
        assert!(fuzzy_subsequence_score("show command palette", "xyz").is_none());
    }

    #[test]
    fn workspace_menu_navigation_clamps_and_supports_home_and_end() {
        assert_eq!(menu_selection_after_key(0, 4, "up"), 0);
        assert_eq!(menu_selection_after_key(0, 4, "down"), 1);
        assert_eq!(menu_selection_after_key(3, 4, "down"), 3);
        assert_eq!(menu_selection_after_key(2, 4, "home"), 0);
        assert_eq!(menu_selection_after_key(1, 4, "end"), 3);
        assert_eq!(menu_selection_after_key(20, 4, "left"), 3);
        assert_eq!(menu_selection_after_key(20, 0, "down"), 0);
    }

    #[test]
    fn every_language_menu_row_round_trips_through_its_index() {
        let modes = std::iter::once(lst_editor::LanguageMode::Auto)
            .chain(std::iter::once(lst_editor::LanguageMode::PlainText))
            .chain(
                LANGUAGES
                    .iter()
                    .map(|(language, _)| lst_editor::LanguageMode::Language(*language)),
            );
        for mode in modes {
            let index = language_mode_index(mode);
            assert_eq!(language_mode_at(index), Some(mode));
        }
        assert_eq!(language_mode_at(LANGUAGES.len() + 2), None);
    }
}
