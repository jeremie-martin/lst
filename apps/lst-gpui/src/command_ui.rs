use crate::{
    ui::{theme::metrics, InputFieldEvent, InputFieldNavigation},
    workspace_action::{command_specs, WorkspaceCommand},
    LstGpuiApp, WorkspaceSurface,
};
use gpui::{
    div, prelude::*, px, rgb, AnyElement, Context, CursorStyle, IntoElement, MouseButton, Pixels, Point, Window,
};

impl LstGpuiApp {
    pub(crate) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_focus_surfaces(cx);
        self.workspace_surface = WorkspaceSurface::CommandPalette;
        self.command_palette_selected = 0;
        self.command_palette_input
            .update(cx, |input, cx| input.set_text("", cx));
        let focus = self.command_palette_input.read(cx).focus_handle();
        window.focus(&focus);
        cx.notify();
    }

    pub(crate) fn toggle_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::Settings {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface = WorkspaceSurface::Settings;
            cx.notify();
        }
    }

    pub(crate) fn toggle_app_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::AppMenu {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface = WorkspaceSurface::AppMenu;
            cx.notify();
        }
    }

    pub(crate) fn toggle_language_menu(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface == WorkspaceSurface::LanguageMenu {
            self.close_workspace_surface(cx);
        } else {
            self.dismiss_focus_surfaces(cx);
            self.workspace_surface = WorkspaceSurface::LanguageMenu;
            cx.notify();
        }
    }

    pub(crate) fn open_context_menu(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.dismiss_focus_surfaces(cx);
        self.context_menu_position = Some(position);
        self.workspace_surface = WorkspaceSurface::ContextMenu;
        cx.notify();
    }

    fn set_language_mode(&mut self, mode: lst_editor::LanguageMode, cx: &mut Context<Self>) {
        self.workspace_surface = WorkspaceSurface::None;
        self.context_menu_position = None;
        self.force_editor_focus = true;
        self.update_model(cx, true, |model| model.set_active_language_mode(mode));
    }

    pub(crate) fn close_workspace_surface(&mut self, cx: &mut Context<Self>) {
        if self.workspace_surface != WorkspaceSurface::None {
            self.workspace_surface = WorkspaceSurface::None;
            self.context_menu_position = None;
            self.force_editor_focus = true;
            cx.notify();
        }
    }

    fn dismiss_focus_surfaces(&mut self, cx: &mut Context<Self>) {
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
            .take(12)
            .enumerate()
            .map(|(index, spec)| {
                let selected = index == self.command_palette_selected;
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
                    .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _window, cx| this.queue_palette_command(command, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(metrics::px_for_scale(13.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child(spec.title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(metrics::px_for_scale(11.0, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child(spec.category),
                    )
                    .when_some(shortcut, |row, shortcut| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(metrics::px_for_scale(11.0, scale))
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
                            .flex()
                            .flex_col()
                            .max_h(px(408.0))
                            .overflow_y_hidden()
                            .children(rows),
                    ),
            )
    }

    pub(crate) fn render_app_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        const ITEMS: &[&str] = &[
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
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let commands = command_specs(&self.settings.settings.keybindings);
        let rows = ITEMS
            .iter()
            .filter_map(|id| commands.iter().find(|command| command.id == *id))
            .enumerate()
            .map(|(index, spec)| {
                let command = spec.command;
                div()
                    .id(("app-menu-command", index))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .h(metrics::px_for_scale(32.0, scale))
                    .px_3()
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.queue_palette_command(command, cx)),
                    )
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child(spec.title.clone()),
                    )
                    .when_some(preferred_shortcut(&spec.shortcuts), |row, shortcut| {
                        row.child(
                            div()
                                .text_size(metrics::px_for_scale(11.0, scale))
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
            .bg(gpui::rgba(0x00000022))
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
        use lst_editor::Language;
        const LANGUAGES: &[(Language, &str)] = &[
            (Language::Rust, "Rust"),
            (Language::Python, "Python"),
            (Language::JavaScript, "JavaScript"),
            (Language::Jsx, "JavaScript JSX"),
            (Language::TypeScript, "TypeScript"),
            (Language::Tsx, "TypeScript TSX"),
            (Language::Json, "JSON"),
            (Language::Jsonc, "JSON with Comments"),
            (Language::Toml, "TOML"),
            (Language::Yaml, "YAML"),
            (Language::Markdown, "Markdown"),
            (Language::Html, "HTML"),
            (Language::Css, "CSS"),
            (Language::Scss, "SCSS"),
            (Language::Shell, "Shell"),
            (Language::Bash, "Bash"),
            (Language::Zsh, "Zsh"),
        ];
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
                theme,
                scale,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.set_language_mode(lst_editor::LanguageMode::Auto, cx)),
            )
            .into_any_element(),
        );
        rows.push(
            language_row(
                "language-plain-text",
                "Plain Text",
                mode == lst_editor::LanguageMode::PlainText,
                theme,
                scale,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.set_language_mode(lst_editor::LanguageMode::PlainText, cx)),
            )
            .into_any_element(),
        );
        rows.extend(LANGUAGES.iter().enumerate().map(|(index, (language, label))| {
            let language = *language;
            language_row(
                ("language-choice", index),
                label,
                mode == lst_editor::LanguageMode::Language(language)
                    || (mode == lst_editor::LanguageMode::Auto && current == Some(language)),
                theme,
                scale,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.set_language_mode(lst_editor::LanguageMode::Language(language), cx)
                }),
            )
            .into_any_element()
        }));

        div()
            .id("language-menu-scrim")
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000022))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_workspace_surface(cx)),
            )
            .child(
                div()
                    .id("language-menu")
                    .absolute()
                    .right(metrics::px_for_scale(16.0, scale))
                    .bottom(metrics::px_for_scale(42.0, scale))
                    .w(metrics::px_for_scale(280.0, scale))
                    .py_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
    }

    pub(crate) fn render_context_menu(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        const ITEMS: &[&str] = &[
            "edit.cut",
            "edit.copy",
            "edit.paste",
            "selection.select_all",
            "find.open",
            "find.replace",
            "edit.toggle_line_comment",
            "workbench.command_palette",
        ];
        let scale = self.ui_scale();
        let theme = self.theme(cx);
        let position = self.context_menu_position.unwrap_or_default();
        let left = position.x.min(metrics::px_for_scale(1040.0, scale));
        let top = position.y.min(metrics::px_for_scale(640.0, scale));
        let commands = command_specs(&self.settings.settings.keybindings);
        let rows = ITEMS
            .iter()
            .filter_map(|id| commands.iter().find(|command| command.id == *id))
            .enumerate()
            .map(|(index, spec)| {
                let command = spec.command;
                div()
                    .id(("context-menu-command", index))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .h(metrics::px_for_scale(32.0, scale))
                    .px_3()
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.queue_palette_command(command, cx)),
                    )
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(12.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child(spec.title.clone()),
                    )
                    .when_some(preferred_shortcut(&spec.shortcuts), |row, shortcut| {
                        row.child(
                            div()
                                .text_size(metrics::px_for_scale(11.0, scale))
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
                    .w(metrics::px_for_scale(290.0, scale))
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
    selected: bool,
    theme: crate::ui::theme::Theme,
    scale: f32,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(metrics::px_for_scale(30.0, scale))
        .px_3()
        .bg(rgb(if selected {
            theme.role.control_bg
        } else {
            theme.role.panel_bg
        }))
        .text_size(metrics::px_for_scale(12.0, scale))
        .text_color(rgb(if selected {
            theme.role.text
        } else {
            theme.role.text_subtle
        }))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
        .child(label)
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
    use super::fuzzy_subsequence_score;

    #[test]
    fn fuzzy_subsequence_prefers_contiguous_matches() {
        let contiguous = fuzzy_subsequence_score("show command palette", "command").unwrap();
        let sparse = fuzzy_subsequence_score("c-o-m-m-a-n-d", "command").unwrap();
        assert!(contiguous < sparse);
        assert!(fuzzy_subsequence_score("show command palette", "scpal").is_some());
        assert!(fuzzy_subsequence_score("show command palette", "xyz").is_none());
    }
}
