use gpui::{
    div, prelude::*, rgb, AnyElement, Context, CursorStyle, IntoElement, MouseButton, Stateful, Styled, Window,
};
use lst_editor::{GutterMode, InputMode};
use rfd::FileDialog;
use std::collections::{HashMap, HashSet};

use crate::{
    settings::{AutosaveMode, InputModeSetting, LineNumbersSetting, ThemePreference},
    theme_for_preference,
    ui::{
        input_keybindings,
        theme::{metrics, typography, Theme},
        IconButton, IconKind,
    },
    workspace_action::{command_specs, editor_keybindings},
    LstGpuiApp,
};

impl LstGpuiApp {
    pub(crate) fn persist_settings(&mut self, cx: &mut Context<Self>) {
        self.cleanup_message = match self.settings.save() {
            Ok(()) => None,
            Err(error) => Some(format!("Settings were not saved: {error}")),
        };
        cx.notify();
    }

    pub(crate) fn reload_settings_if_changed(&mut self, cx: &mut Context<Self>) {
        let Some(settings) = self.settings.reloaded_if_changed() else {
            return;
        };
        if let Some(error) = settings.error() {
            self.cleanup_message = Some(format!("Settings reload failed: {error}"));
            return;
        }
        let values = settings.settings.clone();
        self.settings = settings;
        self.scratchpad_dir = values.files.scratchpad_directory.clone();
        typography::set_primary_font_family(&values.editor.font_family);
        metrics::set_code_font_size(f32::from(values.editor.font_size));
        self.invalidate_editor_typography();
        if !self.input_mode_cli_override {
            let input_mode = match values.editor.input_mode {
                InputModeSetting::Standard => InputMode::Standard,
                InputModeSetting::Vim => InputMode::Vim,
            };
            self.update_model(cx, false, |model| model.set_input_mode(input_mode));
        }
        self.update_model(cx, false, |model| {
            model.set_show_wrap(values.editor.word_wrap);
            model.set_gutter_mode(match values.editor.line_numbers {
                LineNumbersSetting::Absolute => GutterMode::Absolute,
                LineNumbersSetting::Relative => GutterMode::Relative,
                LineNumbersSetting::Hybrid => GutterMode::Hybrid,
            });
        });
        self.zoom_level = values.appearance.zoom_level;
        self.set_theme(
            theme_for_preference(values.appearance.theme, cx.window_appearance()),
            cx,
        );
        self.reload_keybindings(cx);
        self.cleanup_message = Some("Settings reloaded.".to_string());
        cx.refresh_windows();
    }

    fn reload_keybindings(&self, cx: &mut Context<Self>) {
        cx.clear_key_bindings();
        cx.bind_keys(editor_keybindings(&self.settings.settings.keybindings));
        cx.bind_keys(input_keybindings());
    }

    fn choose_scratchpad_directory(&mut self, cx: &mut Context<Self>) {
        let mut dialog = FileDialog::new();
        if let Some(path) = self.scratchpad_dir.as_ref() {
            dialog = dialog.set_directory(path);
        }
        let Some(path) = dialog.pick_folder() else {
            return;
        };
        self.scratchpad_dir = Some(path.clone());
        self.settings.settings.files.scratchpad_directory = Some(path);
        self.persist_settings(cx);
    }

    fn set_input_mode_setting(&mut self, setting: InputModeSetting, cx: &mut Context<Self>) {
        self.settings.settings.editor.input_mode = setting;
        self.update_model(cx, true, |model| {
            model.set_input_mode(match setting {
                InputModeSetting::Standard => InputMode::Standard,
                InputModeSetting::Vim => InputMode::Vim,
            });
        });
        self.persist_settings(cx);
    }

    fn toggle_word_wrap_setting(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.settings.settings.editor.word_wrap;
        self.settings.settings.editor.word_wrap = enabled;
        self.update_model(cx, true, |model| model.set_show_wrap(enabled));
        self.persist_settings(cx);
    }

    fn set_line_numbers_setting(&mut self, setting: LineNumbersSetting, cx: &mut Context<Self>) {
        self.settings.settings.editor.line_numbers = setting;
        self.update_model(cx, true, |model| {
            model.set_gutter_mode(match setting {
                LineNumbersSetting::Absolute => GutterMode::Absolute,
                LineNumbersSetting::Relative => GutterMode::Relative,
                LineNumbersSetting::Hybrid => GutterMode::Hybrid,
            });
        });
        self.persist_settings(cx);
    }

    fn toggle_cursor_blink_setting(&mut self, cx: &mut Context<Self>) {
        self.settings.settings.editor.cursor_blink = !self.settings.settings.editor.cursor_blink;
        self.persist_settings(cx);
    }

    fn cycle_font_family(&mut self, cx: &mut Context<Self>) {
        const FAMILIES: &[&str] = &["TX-02", "JetBrains Mono", "Lilex", "IBM Plex Mono"];
        let current = self.settings.settings.editor.font_family.as_str();
        let next = FAMILIES
            .iter()
            .position(|family| *family == current)
            .map_or(0, |index| (index + 1) % FAMILIES.len());
        self.settings.settings.editor.font_family = FAMILIES[next].to_string();
        typography::set_primary_font_family(FAMILIES[next]);
        self.invalidate_editor_typography();
        self.persist_settings(cx);
    }

    fn adjust_font_size(&mut self, delta: i16, cx: &mut Context<Self>) {
        let size = (self.settings.settings.editor.font_size as i16 + delta).clamp(8, 40) as u16;
        if size == self.settings.settings.editor.font_size {
            return;
        }
        self.settings.settings.editor.font_size = size;
        metrics::set_code_font_size(f32::from(size));
        self.invalidate_editor_typography();
        self.persist_settings(cx);
    }

    fn invalidate_editor_typography(&mut self) {
        for view in self.tab_views.values_mut() {
            view.invalidate_visual_state();
        }
    }

    fn set_theme_setting(&mut self, preference: ThemePreference, cx: &mut Context<Self>) {
        self.settings.settings.appearance.theme = preference;
        self.set_theme(theme_for_preference(preference, cx.window_appearance()), cx);
        self.persist_settings(cx);
    }

    fn set_autosave_setting(&mut self, mode: AutosaveMode, cx: &mut Context<Self>) {
        self.settings.settings.files.autosave = mode;
        self.persist_settings(cx);
    }

    fn toggle_trim_whitespace_setting(&mut self, cx: &mut Context<Self>) {
        self.settings.settings.files.trim_trailing_whitespace = !self.settings.settings.files.trim_trailing_whitespace;
        self.persist_settings(cx);
    }

    fn toggle_final_newline_setting(&mut self, cx: &mut Context<Self>) {
        self.settings.settings.files.ensure_final_newline = !self.settings.settings.files.ensure_final_newline;
        self.persist_settings(cx);
    }

    fn reset_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.settings.reset() {
            self.cleanup_message = Some(format!("Settings were not reset: {error}"));
            cx.notify();
            return;
        }
        let settings = self.settings.settings.clone();
        self.scratchpad_dir = settings.files.scratchpad_directory.clone();
        typography::set_primary_font_family(&settings.editor.font_family);
        metrics::set_code_font_size(f32::from(settings.editor.font_size));
        self.invalidate_editor_typography();
        self.update_model(cx, true, |model| {
            model.set_input_mode(InputMode::Standard);
            model.set_show_wrap(settings.editor.word_wrap);
            model.set_gutter_mode(GutterMode::Absolute);
        });
        self.set_theme(
            theme_for_preference(settings.appearance.theme, cx.window_appearance()),
            cx,
        );
        self.set_zoom_level(settings.appearance.zoom_level, window, cx);
        self.reload_keybindings(cx);
        cx.notify();
    }

    pub(crate) fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(cx);
        let scale = self.ui_scale();
        let settings = self.settings.settings.clone();
        let config_path = self
            .settings
            .path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Unavailable".to_string());
        let settings_error = self.settings.error().map(ToOwned::to_owned);
        let scratchpad_path = settings
            .files
            .scratchpad_directory
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "System default".to_string());

        let input_mode = segmented_control(
            vec![
                setting_choice(
                    "settings-input-standard",
                    "Standard",
                    settings.editor.input_mode == InputModeSetting::Standard,
                    theme,
                    scale,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.set_input_mode_setting(InputModeSetting::Standard, cx)),
                )
                .into_any_element(),
                setting_choice(
                    "settings-input-vim",
                    "Vim",
                    settings.editor.input_mode == InputModeSetting::Vim,
                    theme,
                    scale,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.set_input_mode_setting(InputModeSetting::Vim, cx)),
                )
                .into_any_element(),
            ],
            theme,
        );
        let wrap = toggle_button("settings-word-wrap", settings.editor.word_wrap, theme, scale).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.toggle_word_wrap_setting(cx)),
        );
        let cursor_blink = toggle_button("settings-cursor-blink", settings.editor.cursor_blink, theme, scale)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.toggle_cursor_blink_setting(cx)),
            );
        let line_numbers = segmented_control(
            [
                ("settings-lines-absolute", "Absolute", LineNumbersSetting::Absolute),
                ("settings-lines-relative", "Relative", LineNumbersSetting::Relative),
                ("settings-lines-hybrid", "Hybrid", LineNumbersSetting::Hybrid),
            ]
            .into_iter()
            .map(|(id, label, value)| {
                setting_choice(id, label, settings.editor.line_numbers == value, theme, scale)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.set_line_numbers_setting(value, cx)),
                    )
                    .into_any_element()
            })
            .collect(),
            theme,
        );
        let font = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                IconButton::new("settings-font-minus", IconKind::Minus, theme)
                    .tooltip("Decrease editor font size")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.adjust_font_size(-1, cx)),
                    ),
            )
            .child(setting_value(settings.editor.font_size.to_string(), theme, scale))
            .child(
                IconButton::new("settings-font-plus", IconKind::Plus, theme)
                    .tooltip("Increase editor font size")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.adjust_font_size(1, cx)),
                    ),
            );
        let family = setting_choice(
            "settings-font-family",
            settings.editor.font_family.clone(),
            false,
            theme,
            scale,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.cycle_font_family(cx)),
        );
        let theme_control = segmented_control(
            [
                ("settings-theme-system", "System", ThemePreference::System),
                ("settings-theme-dark", "Dark", ThemePreference::Dark),
                ("settings-theme-light", "Light", ThemePreference::Light),
            ]
            .into_iter()
            .map(|(id, label, value)| {
                setting_choice(id, label, settings.appearance.theme == value, theme, scale)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.set_theme_setting(value, cx)),
                    )
                    .into_any_element()
            })
            .collect(),
            theme,
        );
        let autosave = segmented_control(
            [
                ("settings-autosave-scratch", "Scratchpads", AutosaveMode::Scratchpads),
                ("settings-autosave-all", "All files", AutosaveMode::All),
            ]
            .into_iter()
            .map(|(id, label, value)| {
                setting_choice(id, label, settings.files.autosave == value, theme, scale)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.set_autosave_setting(value, cx)),
                    )
                    .into_any_element()
            })
            .collect(),
            theme,
        );
        let trim = toggle_button(
            "settings-trim-whitespace",
            settings.files.trim_trailing_whitespace,
            theme,
            scale,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.toggle_trim_whitespace_setting(cx)),
        );
        let final_newline = toggle_button(
            "settings-final-newline",
            settings.files.ensure_final_newline,
            theme,
            scale,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.toggle_final_newline_setting(cx)),
        );
        let directory = setting_choice("settings-scratchpad-dir", scratchpad_path, false, theme, scale).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.choose_scratchpad_directory(cx)),
        );

        let mut binding_owners = HashMap::<&str, usize>::new();
        for bindings in settings.keybindings.values() {
            for binding in bindings {
                *binding_owners.entry(binding.as_str()).or_default() += 1;
            }
        }
        let conflicts = binding_owners
            .into_iter()
            .filter_map(|(binding, owners)| (owners > 1).then_some(binding))
            .collect::<HashSet<_>>();
        let binding_rows = command_specs(&settings.keybindings)
            .into_iter()
            .map(|spec| {
                let shortcut = spec.shortcuts.first().cloned().unwrap_or_else(|| "Unbound".to_string());
                let shortcut = if conflicts.contains(shortcut.as_str()) {
                    format!("Conflict: {shortcut}")
                } else {
                    shortcut
                };
                setting_row(
                    spec.title,
                    setting_value(shortcut, theme, scale).into_any_element(),
                    theme,
                    scale,
                )
                .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .id("settings-surface")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(rgb(theme.role.app_bg))
            .occlude()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(metrics::px_for_scale(46.0, scale))
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(theme.role.border))
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(16.0, scale))
                            .text_color(rgb(theme.role.text))
                            .child("Settings"),
                    )
                    .child(
                        IconButton::new("settings-close", IconKind::Close, theme)
                            .tooltip("Close settings (Esc)")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.close_workspace_surface(cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.settings_scroll)
                    .child(
                        div()
                            .w_full()
                            .max_w(metrics::px_for_scale(860.0, scale))
                            .mx_auto()
                            .px_5()
                            .py_4()
                            .flex()
                            .flex_col()
                            .child(settings_section("Editor", theme, scale))
                            .child(setting_row("Input mode", input_mode.into_any_element(), theme, scale))
                            .child(setting_row("Word wrap", wrap.into_any_element(), theme, scale))
                            .child(setting_row(
                                "Line numbers",
                                line_numbers.into_any_element(),
                                theme,
                                scale,
                            ))
                            .child(setting_row(
                                "Cursor blink",
                                cursor_blink.into_any_element(),
                                theme,
                                scale,
                            ))
                            .child(setting_row("Font family", family.into_any_element(), theme, scale))
                            .child(setting_row("Font size", font.into_any_element(), theme, scale))
                            .child(settings_section("Appearance", theme, scale))
                            .child(setting_row("Theme", theme_control.into_any_element(), theme, scale))
                            .child(setting_row(
                                "Zoom",
                                setting_value(format!("{}%", (self.ui_scale() * 100.0).round()), theme, scale)
                                    .into_any_element(),
                                theme,
                                scale,
                            ))
                            .child(settings_section("Files", theme, scale))
                            .child(setting_row("Autosave", autosave.into_any_element(), theme, scale))
                            .child(setting_row(
                                "Trim trailing whitespace",
                                trim.into_any_element(),
                                theme,
                                scale,
                            ))
                            .child(setting_row(
                                "Ensure final newline",
                                final_newline.into_any_element(),
                                theme,
                                scale,
                            ))
                            .child(setting_row(
                                "Scratchpad directory",
                                directory.into_any_element(),
                                theme,
                                scale,
                            ))
                            .child(settings_section("Keybindings", theme, scale))
                            .children(binding_rows)
                            .child(settings_section("Configuration", theme, scale))
                            .child(setting_row(
                                "Settings file",
                                setting_value(config_path, theme, scale).into_any_element(),
                                theme,
                                scale,
                            ))
                            .when_some(settings_error, |column, error| {
                                column.child(
                                    div()
                                        .py_2()
                                        .text_size(metrics::px_for_scale(12.0, scale))
                                        .text_color(rgb(theme.role.error_text))
                                        .child(error),
                                )
                            })
                            .child(div().py_3().child(
                                setting_choice("settings-reset", "Reset settings", false, theme, scale).on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| this.reset_settings(window, cx)),
                                ),
                            )),
                    ),
            )
    }
}

fn settings_section(label: &'static str, theme: Theme, scale: f32) -> impl IntoElement {
    div()
        .pt_5()
        .pb_2()
        .border_b_1()
        .border_color(rgb(theme.role.border))
        .text_size(metrics::px_for_scale(13.0, scale))
        .text_color(rgb(theme.role.text))
        .child(label)
}

fn setting_row(label: impl Into<String>, control: AnyElement, theme: Theme, scale: f32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(metrics::px_for_scale(42.0, scale))
        .py_1()
        .border_b_1()
        .border_color(rgb(theme.role.border))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(metrics::px_for_scale(12.0, scale))
                .text_color(rgb(theme.role.text_subtle))
                .child(label.into()),
        )
        .child(control)
}

fn segmented_control(children: Vec<AnyElement>, theme: Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(rgb(theme.role.border))
        .children(children)
}

fn setting_choice(
    id: &'static str,
    label: impl Into<String>,
    active: bool,
    theme: Theme,
    scale: f32,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(metrics::px_for_scale(28.0, scale))
        .px_3()
        .rounded_sm()
        .bg(rgb(if active {
            theme.role.accent
        } else {
            theme.role.control_bg
        }))
        .text_color(rgb(if active {
            theme.role.accent_text
        } else {
            theme.role.text
        }))
        .text_size(metrics::px_for_scale(11.0, scale))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
        .child(label.into())
}

fn toggle_button(id: &'static str, enabled: bool, theme: Theme, scale: f32) -> Stateful<gpui::Div> {
    setting_choice(id, if enabled { "On" } else { "Off" }, enabled, theme, scale)
}

fn setting_value(value: impl Into<String>, theme: Theme, scale: f32) -> impl IntoElement {
    div()
        .max_w(metrics::px_for_scale(430.0, scale))
        .truncate()
        .text_size(metrics::px_for_scale(11.0, scale))
        .text_color(rgb(theme.role.text_muted))
        .child(value.into())
}
