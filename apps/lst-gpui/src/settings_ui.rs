use gpui::{
    div, point, prelude::*, px, rgb, AnyElement, Context, CursorStyle, IntoElement, KeyDownEvent, MouseButton,
    Stateful, Styled, Window,
};
use lst_editor::{GutterMode, InputMode};
use rfd::FileDialog;
use std::collections::{HashMap, HashSet};

use crate::{
    settings::{
        AppSettings, AutosaveMode, GuideMode, InputModeSetting, LineNumbersSetting, MatchBracketsSetting,
        RenderWhitespaceSetting, RulerColumns, SettingsStore, ThemePreference,
    },
    theme_for_preference,
    ui::{
        input_keybindings,
        theme::{metrics, typography, Theme},
        IconButton, IconKind, InputFieldEvent, InputFieldNavigation,
    },
    workspace_action::{command_specs, editor_keybindings},
    LstGpuiApp,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SettingsItem {
    InputMode,
    WordWrap,
    LineNumbers,
    CursorBlink,
    FontFamily,
    FontSize,
    MatchBrackets,
    BracketColorization,
    BracketGuides,
    HorizontalBracketGuides,
    IndentGuides,
    ActiveIndentGuide,
    RenderWhitespace,
    ControlCharacters,
    Rulers,
    SmartSelectSubwords,
    SmartSelectWhitespace,
    MultiCursorLimit,
    Theme,
    Zoom,
    Autosave,
    TrimWhitespace,
    FinalNewline,
    ScratchpadDirectory,
    Reset,
}

impl SettingsItem {
    const ALL: [Self; 25] = [
        Self::InputMode,
        Self::WordWrap,
        Self::LineNumbers,
        Self::CursorBlink,
        Self::FontFamily,
        Self::FontSize,
        Self::MatchBrackets,
        Self::BracketColorization,
        Self::BracketGuides,
        Self::HorizontalBracketGuides,
        Self::IndentGuides,
        Self::ActiveIndentGuide,
        Self::RenderWhitespace,
        Self::ControlCharacters,
        Self::Rulers,
        Self::SmartSelectSubwords,
        Self::SmartSelectWhitespace,
        Self::MultiCursorLimit,
        Self::Theme,
        Self::Zoom,
        Self::Autosave,
        Self::TrimWhitespace,
        Self::FinalNewline,
        Self::ScratchpadDirectory,
        Self::Reset,
    ];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::InputMode => "input_mode",
            Self::WordWrap => "word_wrap",
            Self::LineNumbers => "line_numbers",
            Self::CursorBlink => "cursor_blink",
            Self::FontFamily => "font_family",
            Self::FontSize => "font_size",
            Self::MatchBrackets => "match_brackets",
            Self::BracketColorization => "bracket_pair_colorization",
            Self::BracketGuides => "bracket_pair_guides",
            Self::HorizontalBracketGuides => "bracket_pair_horizontal_guides",
            Self::IndentGuides => "indent_guides",
            Self::ActiveIndentGuide => "highlight_active_indent_guide",
            Self::RenderWhitespace => "render_whitespace",
            Self::ControlCharacters => "render_control_characters",
            Self::Rulers => "rulers",
            Self::SmartSelectSubwords => "smart_select_subwords",
            Self::SmartSelectWhitespace => "smart_select_include_whitespace",
            Self::MultiCursorLimit => "multi_cursor_limit",
            Self::Theme => "theme",
            Self::Zoom => "zoom",
            Self::Autosave => "autosave",
            Self::TrimWhitespace => "trim_whitespace",
            Self::FinalNewline => "final_newline",
            Self::ScratchpadDirectory => "scratchpad_directory",
            Self::Reset => "reset",
        }
    }

    fn visible(self, query: &str, settings: &AppSettings) -> bool {
        let fields: &[&str] = match self {
            Self::InputMode => &["Editor", "Input mode", "Standard", "Vim", "keyboard editing"],
            Self::WordWrap => &["Editor", "Word wrap", "line wrapping"],
            Self::LineNumbers => &["Editor", "Line numbers", "Absolute", "Relative", "Hybrid", "gutter"],
            Self::CursorBlink => &["Editor", "Cursor blink", "caret animation"],
            Self::FontFamily => &[
                "Editor",
                "Font family",
                "typeface",
                settings.editor.font_family.as_str(),
                "TX-02 JetBrains Mono Lilex IBM Plex Mono",
            ],
            Self::FontSize => &["Editor", "Font size", "text size"],
            Self::MatchBrackets => &["Editor", "Match brackets", "Never Near Always", "delimiter"],
            Self::BracketColorization => &["Editor", "Bracket pair colorization", "rainbow", "delimiter"],
            Self::BracketGuides => &["Editor", "Bracket pair guides", "Off Active All", "vertical"],
            Self::HorizontalBracketGuides => &["Editor", "Horizontal bracket guides", "Off Active All", "delimiter"],
            Self::IndentGuides => &["Editor", "Indent guides", "indentation", "vertical"],
            Self::ActiveIndentGuide => &["Editor", "Highlight active indent guide", "indentation"],
            Self::RenderWhitespace => &[
                "Editor",
                "Render whitespace",
                "None Boundary Selection Trailing All",
                "spaces tabs",
            ],
            Self::ControlCharacters => &["Editor", "Render control characters", "bidi zero width C0 C1"],
            Self::Rulers => &["Editor", "Rulers", "columns", "vertical guides"],
            Self::SmartSelectSubwords => &["Editor", "Smart select subwords", "camel case selection"],
            Self::SmartSelectWhitespace => &["Editor", "Smart select include whitespace", "selection expand"],
            Self::MultiCursorLimit => &["Editor", "Multi cursor limit", "carets performance"],
            Self::Theme => &["Appearance", "Theme", "System", "Dark", "Light"],
            Self::Zoom => &["Appearance", "Zoom", "scale", "magnification"],
            Self::Autosave => &["Files", "Autosave", "Scratchpads", "All files", "automatic save"],
            Self::TrimWhitespace => &["Files", "Trim trailing whitespace", "spaces", "save options"],
            Self::FinalNewline => &["Files", "Ensure final newline", "end of file", "save options"],
            Self::ScratchpadDirectory => &["Files", "Scratchpad directory", "folder", "location"],
            Self::Reset => &["Configuration", "Reset settings", "defaults", "restore"],
        };
        settings_query_matches(query, fields)
    }
}

#[derive(Debug, Default)]
pub(crate) struct SettingsSelection {
    selected: Option<SettingsItem>,
    reveal_selected: bool,
}

impl SettingsSelection {
    pub(crate) fn selected(&self) -> Option<SettingsItem> {
        self.selected
    }

    pub(crate) fn selected_id(&self) -> Option<&'static str> {
        self.selected.map(SettingsItem::id)
    }

    pub(crate) fn is_active(&self) -> bool {
        self.selected.is_some()
    }

    fn select(&mut self, item: SettingsItem) {
        self.selected = Some(item);
        self.reveal_selected = true;
    }

    pub(crate) fn clear(&mut self) {
        self.selected = None;
        self.reveal_selected = false;
    }

    fn take_reveal(&mut self) -> Option<SettingsItem> {
        if !self.reveal_selected {
            return None;
        }
        self.reveal_selected = false;
        self.selected
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsOverlay {
    #[default]
    None,
    FontMenu {
        selected: FontFamilyChoice,
    },
    ResetConfirmation {
        selected: ResetConfirmationChoice,
    },
    ValueEditor {
        item: SettingsItem,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FontFamilyChoice {
    #[default]
    Tx02,
    JetBrainsMono,
    Lilex,
    IbmPlexMono,
}

impl FontFamilyChoice {
    const ALL: [Self; 4] = [Self::Tx02, Self::JetBrainsMono, Self::Lilex, Self::IbmPlexMono];

    fn from_name(name: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|choice| choice.name() == name)
            .unwrap_or_default()
    }

    fn name(self) -> &'static str {
        match self {
            Self::Tx02 => "TX-02",
            Self::JetBrainsMono => "JetBrains Mono",
            Self::Lilex => "Lilex",
            Self::IbmPlexMono => "IBM Plex Mono",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Tx02 => "settings-font-tx02",
            Self::JetBrainsMono => "settings-font-jetbrains",
            Self::Lilex => "settings-font-lilex",
            Self::IbmPlexMono => "settings-font-ibm-plex",
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Tx02 => Self::IbmPlexMono,
            Self::JetBrainsMono => Self::Tx02,
            Self::Lilex => Self::JetBrainsMono,
            Self::IbmPlexMono => Self::Lilex,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Tx02 => Self::JetBrainsMono,
            Self::JetBrainsMono => Self::Lilex,
            Self::Lilex => Self::IbmPlexMono,
            Self::IbmPlexMono => Self::Tx02,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ResetConfirmationChoice {
    #[default]
    Cancel,
    Reset,
}

impl SettingsOverlay {
    pub(crate) fn wants_surface_focus(self) -> bool {
        matches!(self, Self::FontMenu { .. } | Self::ResetConfirmation { .. })
    }
}

impl LstGpuiApp {
    pub(crate) fn persist_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_generation = self.settings_generation.wrapping_add(1);
        self.cleanup_message = match self.settings.save() {
            Ok(()) => None,
            Err(error) => Some(format!("Settings were not saved: {error}")),
        };
        cx.notify();
    }

    pub(crate) fn apply_reloaded_settings(&mut self, settings: SettingsStore, cx: &mut Context<Self>) {
        if let Some(error) = settings.error() {
            self.cleanup_message = Some(format!("Settings reload failed: {error}"));
            // Remember the broken content so the 500 ms poll reports it once
            // instead of rediscovering it (and re-notifying) every tick.
            self.settings.mark_source_seen(settings);
            cx.notify();
            return;
        }
        let values = settings.settings.clone();
        self.settings_generation = self.settings_generation.wrapping_add(1);
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
            model.set_multi_cursor_limit(values.editor.multi_cursor_limit);
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

    fn set_polish_setting(&mut self, item: SettingsItem, forward: bool, cx: &mut Context<Self>) {
        let editor = &mut self.settings.settings.editor;
        match item {
            SettingsItem::MatchBrackets => {
                editor.match_brackets = match (editor.match_brackets, forward) {
                    (MatchBracketsSetting::Never, true) | (MatchBracketsSetting::Always, false) => {
                        MatchBracketsSetting::Near
                    }
                    (MatchBracketsSetting::Near, true) | (MatchBracketsSetting::Never, false) => {
                        MatchBracketsSetting::Always
                    }
                    (MatchBracketsSetting::Always, true) | (MatchBracketsSetting::Near, false) => {
                        MatchBracketsSetting::Never
                    }
                };
            }
            SettingsItem::BracketGuides => {
                editor.bracket_pair_guides = cycle_guide_mode(editor.bracket_pair_guides, forward);
            }
            SettingsItem::HorizontalBracketGuides => {
                editor.bracket_pair_horizontal_guides =
                    cycle_guide_mode(editor.bracket_pair_horizontal_guides, forward);
            }
            SettingsItem::RenderWhitespace => {
                editor.render_whitespace = cycle_whitespace_mode(editor.render_whitespace, forward);
            }
            SettingsItem::BracketColorization => editor.bracket_pair_colorization = forward,
            SettingsItem::IndentGuides => editor.indent_guides = forward,
            SettingsItem::ActiveIndentGuide => editor.highlight_active_indent_guide = forward,
            SettingsItem::ControlCharacters => editor.render_control_characters = forward,
            SettingsItem::SmartSelectSubwords => editor.smart_select_subwords = forward,
            SettingsItem::SmartSelectWhitespace => editor.smart_select_include_whitespace = forward,
            _ => return,
        }
        self.persist_settings(cx);
    }

    fn open_settings_value_editor(&mut self, item: SettingsItem, window: &mut Window, cx: &mut Context<Self>) {
        let text = match item {
            SettingsItem::Rulers => self
                .settings
                .settings
                .editor
                .rulers
                .as_slice()
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            SettingsItem::MultiCursorLimit => self.settings.settings.editor.multi_cursor_limit.to_string(),
            _ => return,
        };
        self.settings_value_error = None;
        self.settings_overlay = SettingsOverlay::ValueEditor { item };
        self.settings_value_input
            .update(cx, |input, cx| input.set_text(&text, cx));
        window.focus(&self.settings_value_input.read(cx).focus_handle());
        cx.notify();
    }

    pub(crate) fn handle_settings_value_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(_) => {
                self.settings_value_error = None;
                cx.notify();
            }
            InputFieldEvent::Cancelled => {
                self.settings_overlay = SettingsOverlay::None;
                self.settings_value_error = None;
                cx.notify();
            }
            InputFieldEvent::Submitted => self.commit_settings_value_editor(cx),
            InputFieldEvent::NextRequested | InputFieldEvent::PreviousRequested | InputFieldEvent::Navigate(_) => {}
        }
    }

    fn commit_settings_value_editor(&mut self, cx: &mut Context<Self>) {
        let SettingsOverlay::ValueEditor { item } = self.settings_overlay else {
            return;
        };
        let text = self.settings_value_input.read(cx).text().trim().to_string();
        let result = match item {
            SettingsItem::Rulers => parse_ruler_columns(&text).map(|rulers| {
                self.settings.settings.editor.rulers = rulers;
            }),
            SettingsItem::MultiCursorLimit => text
                .parse::<usize>()
                .map_err(|_| "Enter a whole number from 1 to 10000.".to_string())
                .and_then(|limit| {
                    (1..=10_000)
                        .contains(&limit)
                        .then_some(limit)
                        .ok_or_else(|| "Enter a whole number from 1 to 10000.".to_string())
                })
                .map(|limit| {
                    self.settings.settings.editor.multi_cursor_limit = limit;
                }),
            _ => return,
        };
        match result {
            Ok(()) => {
                if item == SettingsItem::MultiCursorLimit {
                    let limit = self.settings.settings.editor.multi_cursor_limit;
                    self.update_model(cx, false, |model| model.set_multi_cursor_limit(limit));
                }
                self.settings_value_error = None;
                self.settings_overlay = SettingsOverlay::None;
                self.persist_settings(cx);
            }
            Err(error) => {
                self.settings_value_error = Some(error);
                cx.notify();
            }
        }
    }

    fn set_font_family(&mut self, family: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.settings.editor.font_family == family {
            self.dismiss_settings_overlay(window, cx);
            return;
        }
        self.settings.settings.editor.font_family = family.to_string();
        typography::set_primary_font_family(family);
        self.invalidate_editor_typography();
        self.persist_settings(cx);
        self.dismiss_settings_overlay(window, cx);
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
            view.cache.borrow_mut().invalidate_typography();
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

    fn apply_settings_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_generation = self.settings_generation.wrapping_add(1);
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
            model.set_multi_cursor_limit(settings.editor.multi_cursor_limit);
        });
        self.set_theme(
            theme_for_preference(settings.appearance.theme, cx.window_appearance()),
            cx,
        );
        self.set_zoom_level(settings.appearance.zoom_level, window, cx);
        self.reload_keybindings(cx);
        self.settings_overlay = SettingsOverlay::None;
        self.settings_selection.clear();
        self.settings_search_input
            .update(cx, |input, cx| input.set_text("", cx));
        self.focus_settings_search(window, cx);
        cx.notify();
    }

    fn focus_settings_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_selection.clear();
        window.focus(&self.settings_search_input.read(cx).focus_handle());
    }

    fn visible_settings_items(&self, cx: &Context<Self>) -> Vec<SettingsItem> {
        let query = self.settings_search_input.read(cx).text().trim().to_string();
        SettingsItem::ALL
            .into_iter()
            .filter(|item| item.visible(&query, &self.settings.settings))
            .collect()
    }

    fn select_settings_edge(&mut self, backward: bool, cx: &mut Context<Self>) {
        let visible = self.visible_settings_items(cx);
        let selected = if backward { visible.last() } else { visible.first() };
        if let Some(selected) = selected.copied() {
            self.settings_selection.select(selected);
            cx.notify();
        }
    }

    fn move_settings_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let visible = self.visible_settings_items(cx);
        if visible.is_empty() {
            self.settings_selection.clear();
            cx.notify();
            return;
        }
        let current = self
            .settings_selection
            .selected()
            .and_then(|selected| visible.iter().position(|item| *item == selected))
            .unwrap_or(if delta.is_negative() { 0 } else { visible.len() - 1 });
        let next = (current as isize + delta).rem_euclid(visible.len() as isize) as usize;
        self.settings_selection.select(visible[next]);
        cx.notify();
    }

    fn tab_settings_selection(&mut self, backward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self.visible_settings_items(cx);
        let Some(selected) = self.settings_selection.selected() else {
            self.select_settings_edge(backward, cx);
            return;
        };
        let Some(index) = visible.iter().position(|item| *item == selected) else {
            self.select_settings_edge(backward, cx);
            return;
        };
        let next = if backward {
            index.checked_sub(1)
        } else {
            (index + 1 < visible.len()).then_some(index + 1)
        };
        if let Some(next) = next {
            self.settings_selection.select(visible[next]);
            cx.notify();
        } else {
            self.focus_settings_search(window, cx);
            cx.notify();
        }
    }

    fn select_settings_boundary(&mut self, end: bool, cx: &mut Context<Self>) {
        self.select_settings_edge(end, cx);
    }

    fn toggle_font_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_overlay = match self.settings_overlay {
            SettingsOverlay::FontMenu { .. } => SettingsOverlay::None,
            _ => {
                let selected = FontFamilyChoice::from_name(&self.settings.settings.editor.font_family);
                SettingsOverlay::FontMenu { selected }
            }
        };
        if self.settings_overlay.wants_surface_focus() {
            window.focus(&self.surface_focus_handle);
        } else {
            self.focus_settings_search(window, cx);
        }
        cx.notify();
    }

    fn request_settings_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_overlay = SettingsOverlay::ResetConfirmation {
            selected: ResetConfirmationChoice::Cancel,
        };
        window.focus(&self.surface_focus_handle);
        cx.notify();
    }

    fn dismiss_settings_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_overlay = SettingsOverlay::None;
        self.settings_value_error = None;
        if self.settings_selection.is_active() {
            window.focus(&self.surface_focus_handle);
        } else {
            self.focus_settings_search(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn handle_settings_search_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(_) => {
                self.settings_overlay = SettingsOverlay::None;
                self.settings_selection.clear();
                self.settings_scroll.set_offset(point(px(0.0), px(0.0)));
                cx.notify();
            }
            InputFieldEvent::Cancelled => self.close_workspace_surface(cx),
            InputFieldEvent::Submitted | InputFieldEvent::NextRequested => self.select_settings_edge(false, cx),
            InputFieldEvent::PreviousRequested => self.select_settings_edge(true, cx),
            InputFieldEvent::Navigate(InputFieldNavigation::Up | InputFieldNavigation::Down) => {}
        }
    }

    fn adjust_settings_item(&mut self, item: SettingsItem, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            SettingsItem::InputMode => self.set_input_mode_setting(
                if forward {
                    InputModeSetting::Vim
                } else {
                    InputModeSetting::Standard
                },
                cx,
            ),
            SettingsItem::WordWrap => {
                if self.settings.settings.editor.word_wrap != forward {
                    self.toggle_word_wrap_setting(cx);
                }
            }
            SettingsItem::LineNumbers => {
                let current = self.settings.settings.editor.line_numbers;
                self.set_line_numbers_setting(
                    match (current, forward) {
                        (LineNumbersSetting::Absolute, true) | (LineNumbersSetting::Hybrid, false) => {
                            LineNumbersSetting::Relative
                        }
                        (LineNumbersSetting::Relative, true) | (LineNumbersSetting::Absolute, false) => {
                            LineNumbersSetting::Hybrid
                        }
                        (LineNumbersSetting::Hybrid, true) | (LineNumbersSetting::Relative, false) => {
                            LineNumbersSetting::Absolute
                        }
                    },
                    cx,
                );
            }
            SettingsItem::CursorBlink => {
                if self.settings.settings.editor.cursor_blink != forward {
                    self.toggle_cursor_blink_setting(cx);
                }
            }
            SettingsItem::FontFamily => {
                let current = FontFamilyChoice::from_name(&self.settings.settings.editor.font_family);
                let choice = if forward { current.next() } else { current.previous() };
                self.set_font_family(choice.name(), window, cx);
            }
            SettingsItem::FontSize => self.adjust_font_size(if forward { 1 } else { -1 }, cx),
            SettingsItem::MatchBrackets
            | SettingsItem::BracketColorization
            | SettingsItem::BracketGuides
            | SettingsItem::HorizontalBracketGuides
            | SettingsItem::IndentGuides
            | SettingsItem::ActiveIndentGuide
            | SettingsItem::RenderWhitespace
            | SettingsItem::ControlCharacters
            | SettingsItem::SmartSelectSubwords
            | SettingsItem::SmartSelectWhitespace => self.set_polish_setting(item, forward, cx),
            SettingsItem::Theme => {
                let current = self.settings.settings.appearance.theme;
                self.set_theme_setting(
                    match (current, forward) {
                        (ThemePreference::System, true) | (ThemePreference::Light, false) => ThemePreference::Dark,
                        (ThemePreference::Dark, true) | (ThemePreference::System, false) => ThemePreference::Light,
                        (ThemePreference::Light, true) | (ThemePreference::Dark, false) => ThemePreference::System,
                    },
                    cx,
                );
            }
            SettingsItem::Zoom => {
                if forward {
                    self.zoom_in(window, cx);
                } else {
                    self.zoom_out(window, cx);
                }
            }
            SettingsItem::Autosave => self.set_autosave_setting(
                if forward {
                    AutosaveMode::All
                } else {
                    AutosaveMode::Scratchpads
                },
                cx,
            ),
            SettingsItem::TrimWhitespace => {
                if self.settings.settings.files.trim_trailing_whitespace != forward {
                    self.toggle_trim_whitespace_setting(cx);
                }
            }
            SettingsItem::FinalNewline => {
                if self.settings.settings.files.ensure_final_newline != forward {
                    self.toggle_final_newline_setting(cx);
                }
            }
            SettingsItem::Rulers
            | SettingsItem::MultiCursorLimit
            | SettingsItem::ScratchpadDirectory
            | SettingsItem::Reset => {}
        }
    }

    fn activate_settings_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.settings_selection.selected() else {
            return;
        };
        match item {
            SettingsItem::InputMode => {
                let forward = self.settings.settings.editor.input_mode == InputModeSetting::Standard;
                self.adjust_settings_item(item, forward, window, cx);
            }
            SettingsItem::WordWrap => self.toggle_word_wrap_setting(cx),
            SettingsItem::CursorBlink => self.toggle_cursor_blink_setting(cx),
            SettingsItem::FontFamily => self.toggle_font_menu(window, cx),
            SettingsItem::Autosave => {
                let forward = self.settings.settings.files.autosave == AutosaveMode::Scratchpads;
                self.adjust_settings_item(item, forward, window, cx);
            }
            SettingsItem::TrimWhitespace => self.toggle_trim_whitespace_setting(cx),
            SettingsItem::FinalNewline => self.toggle_final_newline_setting(cx),
            SettingsItem::ScratchpadDirectory => self.choose_scratchpad_directory(cx),
            SettingsItem::Reset => self.request_settings_reset(window, cx),
            SettingsItem::Rulers | SettingsItem::MultiCursorLimit => {
                self.open_settings_value_editor(item, window, cx);
            }
            SettingsItem::BracketColorization
            | SettingsItem::IndentGuides
            | SettingsItem::ActiveIndentGuide
            | SettingsItem::ControlCharacters
            | SettingsItem::SmartSelectSubwords
            | SettingsItem::SmartSelectWhitespace => {
                self.adjust_settings_item(item, !polish_bool_value(&self.settings.settings, item), window, cx);
            }
            SettingsItem::MatchBrackets
            | SettingsItem::BracketGuides
            | SettingsItem::HorizontalBracketGuides
            | SettingsItem::RenderWhitespace => self.adjust_settings_item(item, true, window, cx),
            SettingsItem::LineNumbers | SettingsItem::FontSize | SettingsItem::Theme | SettingsItem::Zoom => {
                self.adjust_settings_item(item, true, window, cx);
            }
        }
    }

    pub(crate) fn handle_settings_surface_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.settings_overlay {
            SettingsOverlay::FontMenu { selected } => match event.keystroke.key.as_str() {
                "escape" | "tab" => self.dismiss_settings_overlay(window, cx),
                "up" => {
                    self.settings_overlay = SettingsOverlay::FontMenu {
                        selected: selected.previous(),
                    };
                    cx.notify();
                }
                "down" => {
                    self.settings_overlay = SettingsOverlay::FontMenu {
                        selected: selected.next(),
                    };
                    cx.notify();
                }
                "home" => {
                    self.settings_overlay = SettingsOverlay::FontMenu {
                        selected: FontFamilyChoice::Tx02,
                    };
                    cx.notify();
                }
                "end" => {
                    self.settings_overlay = SettingsOverlay::FontMenu {
                        selected: FontFamilyChoice::IbmPlexMono,
                    };
                    cx.notify();
                }
                "enter" => self.set_font_family(selected.name(), window, cx),
                _ => {}
            },
            SettingsOverlay::ResetConfirmation { selected } => match event.keystroke.key.as_str() {
                "escape" => self.dismiss_settings_overlay(window, cx),
                "left" | "right" | "tab" => {
                    let selected = match selected {
                        ResetConfirmationChoice::Cancel => ResetConfirmationChoice::Reset,
                        ResetConfirmationChoice::Reset => ResetConfirmationChoice::Cancel,
                    };
                    self.settings_overlay = SettingsOverlay::ResetConfirmation { selected };
                    cx.notify();
                }
                "enter" => match selected {
                    ResetConfirmationChoice::Cancel => self.dismiss_settings_overlay(window, cx),
                    ResetConfirmationChoice::Reset => self.apply_settings_reset(window, cx),
                },
                _ => {}
            },
            SettingsOverlay::ValueEditor { .. } => {}
            SettingsOverlay::None => {
                let modifiers = event.keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "escape" => self.close_workspace_surface(cx),
                    "/" => self.focus_settings_search(window, cx),
                    "tab" => self.tab_settings_selection(modifiers.shift, window, cx),
                    "up" if !modifiers.shift => self.move_settings_selection(-1, cx),
                    "down" if !modifiers.shift => self.move_settings_selection(1, cx),
                    "home" if !modifiers.shift => self.select_settings_boundary(false, cx),
                    "end" if !modifiers.shift => self.select_settings_boundary(true, cx),
                    "left" if !modifiers.shift => {
                        if let Some(item) = self.settings_selection.selected() {
                            self.adjust_settings_item(item, false, window, cx);
                        }
                    }
                    "right" if !modifiers.shift => {
                        if let Some(item) = self.settings_selection.selected() {
                            self.adjust_settings_item(item, true, window, cx);
                        }
                    }
                    "enter" | "space" if !modifiers.shift => self.activate_settings_item(window, cx),
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(cx);
        let scale = self.ui_scale();
        let settings = self.settings.settings.clone();
        let search_query = self.settings_search_input.read(cx).text().trim().to_string();
        let visible_items = SettingsItem::ALL
            .into_iter()
            .filter(|item| item.visible(&search_query, &settings))
            .collect::<Vec<_>>();
        let selected_item = self.settings_selection.selected();
        let show_input_mode = visible_items.contains(&SettingsItem::InputMode);
        let show_word_wrap = visible_items.contains(&SettingsItem::WordWrap);
        let show_line_numbers = visible_items.contains(&SettingsItem::LineNumbers);
        let show_cursor_blink = visible_items.contains(&SettingsItem::CursorBlink);
        let show_font_family = visible_items.contains(&SettingsItem::FontFamily);
        let show_font_size = visible_items.contains(&SettingsItem::FontSize);
        let polish_items = [
            SettingsItem::MatchBrackets,
            SettingsItem::BracketColorization,
            SettingsItem::BracketGuides,
            SettingsItem::HorizontalBracketGuides,
            SettingsItem::IndentGuides,
            SettingsItem::ActiveIndentGuide,
            SettingsItem::RenderWhitespace,
            SettingsItem::ControlCharacters,
            SettingsItem::Rulers,
            SettingsItem::SmartSelectSubwords,
            SettingsItem::SmartSelectWhitespace,
            SettingsItem::MultiCursorLimit,
        ]
        .into_iter()
        .filter(|item| visible_items.contains(item))
        .collect::<Vec<_>>();
        let show_editor = show_input_mode
            || show_word_wrap
            || show_line_numbers
            || show_cursor_blink
            || show_font_family
            || show_font_size
            || !polish_items.is_empty();
        let show_theme = visible_items.contains(&SettingsItem::Theme);
        let show_zoom = visible_items.contains(&SettingsItem::Zoom);
        let show_appearance = show_theme || show_zoom;
        let show_autosave = visible_items.contains(&SettingsItem::Autosave);
        let show_trim = visible_items.contains(&SettingsItem::TrimWhitespace);
        let show_final_newline = visible_items.contains(&SettingsItem::FinalNewline);
        let show_scratchpad_directory = visible_items.contains(&SettingsItem::ScratchpadDirectory);
        let show_files = show_autosave || show_trim || show_final_newline || show_scratchpad_directory;
        let show_settings_file =
            settings_query_matches(&search_query, &["Configuration", "Settings file", "config", "path"]);
        let show_build = settings_query_matches(&search_query, &["Configuration", "Build", "version", "commit"]);
        let show_reset = visible_items.contains(&SettingsItem::Reset);
        let show_configuration = show_settings_file || show_build || show_reset;
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
                .on_click(cx.listener(|this, _, _, cx| this.set_input_mode_setting(InputModeSetting::Standard, cx)))
                .into_any_element(),
                setting_choice(
                    "settings-input-vim",
                    "Vim",
                    settings.editor.input_mode == InputModeSetting::Vim,
                    theme,
                    scale,
                )
                .on_click(cx.listener(|this, _, _, cx| this.set_input_mode_setting(InputModeSetting::Vim, cx)))
                .into_any_element(),
            ],
            theme,
        );
        let wrap = toggle_button("settings-word-wrap", settings.editor.word_wrap, theme, scale)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_word_wrap_setting(cx)));
        let cursor_blink = toggle_button("settings-cursor-blink", settings.editor.cursor_blink, theme, scale)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_cursor_blink_setting(cx)));
        let line_numbers = segmented_control(
            [
                ("settings-lines-absolute", "Absolute", LineNumbersSetting::Absolute),
                ("settings-lines-relative", "Relative", LineNumbersSetting::Relative),
                ("settings-lines-hybrid", "Hybrid", LineNumbersSetting::Hybrid),
            ]
            .into_iter()
            .map(|(id, label, value)| {
                setting_choice(id, label, settings.editor.line_numbers == value, theme, scale)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_line_numbers_setting(value, cx)))
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
                    .on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(-1, cx))),
            )
            .child(setting_value(settings.editor.font_size.to_string(), theme, scale))
            .child(
                IconButton::new("settings-font-plus", IconKind::Plus, theme)
                    .tooltip("Increase editor font size")
                    .on_click(cx.listener(|this, _, _, cx| this.adjust_font_size(1, cx))),
            );
        let font_menu_selected = match self.settings_overlay {
            SettingsOverlay::FontMenu { selected } => Some(selected),
            _ => None,
        };
        let family_options = font_menu_selected.map(|selected| {
            FontFamilyChoice::ALL
                .into_iter()
                .map(|choice| {
                    let family = choice.name();
                    font_family_option(
                        choice.id(),
                        family,
                        choice == selected,
                        settings.editor.font_family == family,
                        theme,
                        scale,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.set_font_family(family, window, cx)))
                    .into_any_element()
                })
                .collect::<Vec<_>>()
        });
        let family = div()
            .flex()
            .flex_col()
            .items_end()
            .gap_1()
            .child(
                setting_choice(
                    "settings-font-family",
                    format!(
                        "{}  {}",
                        settings.editor.font_family,
                        if font_menu_selected.is_some() { "▴" } else { "▾" }
                    ),
                    font_menu_selected.is_some(),
                    theme,
                    scale,
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_font_menu(window, cx))),
            )
            .when_some(family_options, |family, options| {
                family.child(
                    div()
                        .id("settings-font-family-options")
                        .flex()
                        .flex_col()
                        .w(metrics::px_for_scale(190.0, scale))
                        .p_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(theme.role.border))
                        .bg(rgb(theme.role.panel_bg))
                        .children(options),
                )
            });
        let theme_control = segmented_control(
            [
                ("settings-theme-system", "System", ThemePreference::System),
                ("settings-theme-dark", "Dark", ThemePreference::Dark),
                ("settings-theme-light", "Light", ThemePreference::Light),
            ]
            .into_iter()
            .map(|(id, label, value)| {
                setting_choice(id, label, settings.appearance.theme == value, theme, scale)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_theme_setting(value, cx)))
                    .into_any_element()
            })
            .collect(),
            theme,
        );
        let zoom = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                IconButton::new("settings-zoom-minus", IconKind::Minus, theme)
                    .tooltip("Zoom out")
                    .on_click(cx.listener(|this, _, window, cx| this.zoom_out(window, cx))),
            )
            .child(setting_value(
                format!("{}%", (self.ui_scale() * 100.0).round()),
                theme,
                scale,
            ))
            .child(
                IconButton::new("settings-zoom-plus", IconKind::Plus, theme)
                    .tooltip("Zoom in")
                    .on_click(cx.listener(|this, _, window, cx| this.zoom_in(window, cx))),
            );
        let autosave = segmented_control(
            [
                ("settings-autosave-scratch", "Scratchpads", AutosaveMode::Scratchpads),
                ("settings-autosave-all", "All files", AutosaveMode::All),
            ]
            .into_iter()
            .map(|(id, label, value)| {
                setting_choice(id, label, settings.files.autosave == value, theme, scale)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_autosave_setting(value, cx)))
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
        .on_click(cx.listener(|this, _, _, cx| this.toggle_trim_whitespace_setting(cx)));
        let final_newline = toggle_button(
            "settings-final-newline",
            settings.files.ensure_final_newline,
            theme,
            scale,
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_final_newline_setting(cx)));
        let directory = setting_choice("settings-scratchpad-dir", scratchpad_path, false, theme, scale)
            .on_click(cx.listener(|this, _, _, cx| this.choose_scratchpad_directory(cx)));

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
            .filter_map(|spec| {
                let shortcut = spec.shortcuts.first().cloned().unwrap_or_else(|| "Unbound".to_string());
                let shortcut = if conflicts.contains(shortcut.as_str()) {
                    format!("Conflict: {shortcut}")
                } else {
                    shortcut
                };
                settings_query_matches(
                    &search_query,
                    &[
                        "Keybindings keyboard shortcuts commands",
                        spec.title.as_str(),
                        spec.category,
                        spec.id,
                        shortcut.as_str(),
                    ],
                )
                .then(|| {
                    setting_row(
                        spec.title,
                        setting_value(shortcut, theme, scale).into_any_element(),
                        false,
                        theme,
                        scale,
                    )
                    .into_any_element()
                })
            })
            .collect::<Vec<_>>();
        let show_keybindings = !binding_rows.is_empty();
        let has_results = show_editor || show_appearance || show_files || show_keybindings || show_configuration;

        let mut content = Vec::<AnyElement>::new();
        let mut item_scroll_indices = HashMap::<SettingsItem, usize>::new();
        if show_editor {
            content.push(settings_content_item(settings_section("Editor", theme, scale), scale));
        }
        if show_input_mode {
            item_scroll_indices.insert(SettingsItem::InputMode, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Input mode",
                    input_mode.into_any_element(),
                    selected_item == Some(SettingsItem::InputMode),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_word_wrap {
            item_scroll_indices.insert(SettingsItem::WordWrap, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Word wrap",
                    wrap.into_any_element(),
                    selected_item == Some(SettingsItem::WordWrap),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_line_numbers {
            item_scroll_indices.insert(SettingsItem::LineNumbers, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Line numbers",
                    line_numbers.into_any_element(),
                    selected_item == Some(SettingsItem::LineNumbers),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_cursor_blink {
            item_scroll_indices.insert(SettingsItem::CursorBlink, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Cursor blink",
                    cursor_blink.into_any_element(),
                    selected_item == Some(SettingsItem::CursorBlink),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_font_family {
            item_scroll_indices.insert(SettingsItem::FontFamily, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Font family",
                    family.into_any_element(),
                    selected_item == Some(SettingsItem::FontFamily),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_font_size {
            item_scroll_indices.insert(SettingsItem::FontSize, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Font size",
                    font.into_any_element(),
                    selected_item == Some(SettingsItem::FontSize),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        for item in polish_items {
            item_scroll_indices.insert(item, content.len());
            let value = polish_value(&settings, item);
            let selected = selected_item == Some(item);
            let active = polish_bool_value(&settings, item)
                || matches!(self.settings_overlay, SettingsOverlay::ValueEditor { item: active } if active == item);
            let control = setting_choice(item.id(), value, active, theme, scale).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.settings_selection.select(item);
                    this.activate_settings_item(window, cx);
                },
            ));
            content.push(settings_content_item(
                setting_row(polish_label(item), control.into_any_element(), selected, theme, scale),
                scale,
            ));
        }
        if show_appearance {
            content.push(settings_content_item(
                settings_section("Appearance", theme, scale),
                scale,
            ));
        }
        if show_theme {
            item_scroll_indices.insert(SettingsItem::Theme, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Theme",
                    theme_control.into_any_element(),
                    selected_item == Some(SettingsItem::Theme),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_zoom {
            item_scroll_indices.insert(SettingsItem::Zoom, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Zoom",
                    zoom.into_any_element(),
                    selected_item == Some(SettingsItem::Zoom),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_files {
            content.push(settings_content_item(settings_section("Files", theme, scale), scale));
        }
        if show_autosave {
            item_scroll_indices.insert(SettingsItem::Autosave, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Autosave",
                    autosave.into_any_element(),
                    selected_item == Some(SettingsItem::Autosave),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_trim {
            item_scroll_indices.insert(SettingsItem::TrimWhitespace, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Trim trailing whitespace",
                    trim.into_any_element(),
                    selected_item == Some(SettingsItem::TrimWhitespace),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_final_newline {
            item_scroll_indices.insert(SettingsItem::FinalNewline, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Ensure final newline",
                    final_newline.into_any_element(),
                    selected_item == Some(SettingsItem::FinalNewline),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_scratchpad_directory {
            item_scroll_indices.insert(SettingsItem::ScratchpadDirectory, content.len());
            content.push(settings_content_item(
                setting_row(
                    "Scratchpad directory",
                    directory.into_any_element(),
                    selected_item == Some(SettingsItem::ScratchpadDirectory),
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_keybindings {
            content.push(settings_content_item(
                settings_section("Keybindings", theme, scale),
                scale,
            ));
            content.extend(binding_rows.into_iter().map(|row| settings_content_item(row, scale)));
        }
        if show_configuration {
            content.push(settings_content_item(
                settings_section("Configuration", theme, scale),
                scale,
            ));
        }
        if show_settings_file {
            content.push(settings_content_item(
                setting_row(
                    "Settings file",
                    setting_value(config_path, theme, scale).into_any_element(),
                    false,
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if show_build {
            content.push(settings_content_item(
                setting_row(
                    "Build",
                    setting_value(crate::build_info::BUILD_IDENTITY, theme, scale).into_any_element(),
                    false,
                    theme,
                    scale,
                ),
                scale,
            ));
        }
        if let Some(error) = settings_error {
            content.push(settings_content_item(
                div()
                    .py_2()
                    .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
                    .text_color(rgb(theme.role.error_text))
                    .child(error),
                scale,
            ));
        }
        if show_reset {
            item_scroll_indices.insert(SettingsItem::Reset, content.len());
            content.push(settings_content_item(
                div().py_3().child(
                    setting_choice(
                        "settings-reset",
                        "Reset settings",
                        selected_item == Some(SettingsItem::Reset),
                        theme,
                        scale,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request_settings_reset(window, cx);
                        cx.stop_propagation();
                    })),
                ),
                scale,
            ));
        }
        if !has_results {
            content.push(settings_content_item(
                div()
                    .py_8()
                    .text_size(metrics::px_for_scale(metrics::UI_TEXT_LG, scale))
                    .text_color(rgb(theme.role.text_muted))
                    .child(format!("No settings match “{search_query}”.")),
                scale,
            ));
        }
        if let Some(item) = self.settings_selection.take_reveal() {
            if let Some(index) = item_scroll_indices.get(&item) {
                self.settings_scroll.scroll_to_item(*index);
            }
        }

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
                            .flex_none()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_TITLE, scale))
                            .text_color(rgb(theme.role.text))
                            .child("Settings"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .max_w(metrics::px_for_scale(420.0, scale))
                            .mx_4()
                            .child(self.settings_search_input.clone()),
                    )
                    .child(
                        IconButton::new("settings-close", IconKind::Close, theme)
                            .tooltip("Close settings (Esc)")
                            .on_click(cx.listener(|this, _, _, cx| this.close_workspace_surface(cx))),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .py_4()
                    .overflow_y_scroll()
                    .track_scroll(&self.settings_scroll)
                    .children(content),
            )
            .when(
                matches!(self.settings_overlay, SettingsOverlay::ResetConfirmation { .. }),
                |surface| surface.child(self.render_settings_reset_confirmation(theme, scale, cx)),
            )
            .when(
                matches!(self.settings_overlay, SettingsOverlay::ValueEditor { .. }),
                |surface| surface.child(self.render_settings_value_editor(theme, scale, cx)),
            )
    }

    fn render_settings_value_editor(&mut self, theme: Theme, scale: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let item = match self.settings_overlay {
            SettingsOverlay::ValueEditor { item } => item,
            _ => SettingsItem::Rulers,
        };
        let hint = match item {
            SettingsItem::Rulers => "Comma-separated columns from 1 to 1000; at most 16.",
            SettingsItem::MultiCursorLimit => "Whole number from 1 to 10000.",
            _ => "Enter a value.",
        };
        div()
            .id("settings-value-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .occlude()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.dismiss_settings_overlay(window, cx)),
            )
            .child(
                div()
                    .id("settings-value-editor")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w(metrics::px_for_scale(420.0, scale))
                    .max_w_full()
                    .p_4()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.control_border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_HEADING, scale))
                            .text_color(rgb(theme.role.text))
                            .child(polish_label(item)),
                    )
                    .child(self.settings_value_input.clone())
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child(hint),
                    )
                    .when_some(self.settings_value_error.clone(), |dialog, error| {
                        dialog.child(
                            div()
                                .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                                .text_color(rgb(theme.role.error_text))
                                .child(error),
                        )
                    })
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
                            .text_color(rgb(theme.role.text_muted))
                            .child("Enter to save · Esc to cancel"),
                    ),
            )
    }

    fn render_settings_reset_confirmation(
        &mut self,
        theme: Theme,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = match self.settings_overlay {
            SettingsOverlay::ResetConfirmation { selected } => selected,
            _ => ResetConfirmationChoice::Cancel,
        };
        div()
            .id("settings-reset-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x00000066))
            .occlude()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.dismiss_settings_overlay(window, cx)),
            )
            .child(
                div()
                    .id("settings-reset-confirmation")
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w(metrics::px_for_scale(420.0, scale))
                    .max_w_full()
                    .p_4()
                    .rounded_sm()
                    .border_1()
                    .border_color(rgb(theme.role.border))
                    .bg(rgb(theme.role.panel_bg))
                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_HEADING, scale))
                            .text_color(rgb(theme.role.text))
                            .child("Reset all settings?"),
                    )
                    .child(
                        div()
                            .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
                            .text_color(rgb(theme.role.text_subtle))
                            .child(
                                "This restores editor, appearance, file, and keybinding preferences to their defaults.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                setting_choice(
                                    "settings-reset-cancel",
                                    "Cancel",
                                    selected == ResetConfirmationChoice::Cancel,
                                    theme,
                                    scale,
                                )
                                .on_click(cx.listener(|this, _, window, cx| this.dismiss_settings_overlay(window, cx))),
                            )
                            .child(
                                danger_choice(
                                    "settings-reset-confirm",
                                    "Reset settings",
                                    selected == ResetConfirmationChoice::Reset,
                                    theme,
                                    scale,
                                )
                                .on_click(cx.listener(|this, _, window, cx| this.apply_settings_reset(window, cx))),
                            ),
                    ),
            )
    }
}

fn settings_query_matches(query: &str, fields: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let haystack = fields.join(" ").to_lowercase();
    query.split_whitespace().all(|term| haystack.contains(term))
}

fn cycle_guide_mode(mode: GuideMode, forward: bool) -> GuideMode {
    match (mode, forward) {
        (GuideMode::Off, true) | (GuideMode::All, false) => GuideMode::Active,
        (GuideMode::Active, true) | (GuideMode::Off, false) => GuideMode::All,
        (GuideMode::All, true) | (GuideMode::Active, false) => GuideMode::Off,
    }
}

fn cycle_whitespace_mode(mode: RenderWhitespaceSetting, forward: bool) -> RenderWhitespaceSetting {
    use RenderWhitespaceSetting::{All, Boundary, None, Selection, Trailing};
    match (mode, forward) {
        (None, true) | (Selection, false) => Boundary,
        (Boundary, true) | (Trailing, false) => Selection,
        (Selection, true) | (All, false) => Trailing,
        (Trailing, true) | (None, false) => All,
        (All, true) | (Boundary, false) => None,
    }
}

fn parse_ruler_columns(text: &str) -> Result<RulerColumns, String> {
    if text.trim().is_empty() {
        return RulerColumns::new(Vec::new());
    }
    let columns = text
        .split([',', ' ', '\t'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<u16>()
                .map_err(|_| format!("{part:?} is not a valid ruler column."))
        })
        .collect::<Result<Vec<_>, _>>()?;
    RulerColumns::new(columns)
}

fn polish_bool_value(settings: &AppSettings, item: SettingsItem) -> bool {
    match item {
        SettingsItem::BracketColorization => settings.editor.bracket_pair_colorization,
        SettingsItem::IndentGuides => settings.editor.indent_guides,
        SettingsItem::ActiveIndentGuide => settings.editor.highlight_active_indent_guide,
        SettingsItem::ControlCharacters => settings.editor.render_control_characters,
        SettingsItem::SmartSelectSubwords => settings.editor.smart_select_subwords,
        SettingsItem::SmartSelectWhitespace => settings.editor.smart_select_include_whitespace,
        _ => false,
    }
}

fn polish_value(settings: &AppSettings, item: SettingsItem) -> String {
    match item {
        SettingsItem::MatchBrackets => match settings.editor.match_brackets {
            MatchBracketsSetting::Never => "Never",
            MatchBracketsSetting::Near => "Near",
            MatchBracketsSetting::Always => "Always",
        }
        .to_string(),
        SettingsItem::BracketGuides => guide_mode_label(settings.editor.bracket_pair_guides).to_string(),
        SettingsItem::HorizontalBracketGuides => {
            guide_mode_label(settings.editor.bracket_pair_horizontal_guides).to_string()
        }
        SettingsItem::RenderWhitespace => match settings.editor.render_whitespace {
            RenderWhitespaceSetting::None => "None",
            RenderWhitespaceSetting::Boundary => "Boundary",
            RenderWhitespaceSetting::Selection => "Selection",
            RenderWhitespaceSetting::Trailing => "Trailing",
            RenderWhitespaceSetting::All => "All",
        }
        .to_string(),
        SettingsItem::Rulers => {
            let columns = settings.editor.rulers.as_slice();
            if columns.is_empty() {
                "None".to_string()
            } else {
                columns.iter().map(u16::to_string).collect::<Vec<_>>().join(", ")
            }
        }
        SettingsItem::MultiCursorLimit => settings.editor.multi_cursor_limit.to_string(),
        _ if polish_bool_value(settings, item) => "On".to_string(),
        _ => "Off".to_string(),
    }
}

fn guide_mode_label(mode: GuideMode) -> &'static str {
    match mode {
        GuideMode::Off => "Off",
        GuideMode::Active => "Active",
        GuideMode::All => "All",
    }
}

fn polish_label(item: SettingsItem) -> &'static str {
    match item {
        SettingsItem::MatchBrackets => "Match brackets",
        SettingsItem::BracketColorization => "Bracket pair colorization",
        SettingsItem::BracketGuides => "Bracket pair guides",
        SettingsItem::HorizontalBracketGuides => "Horizontal bracket guides",
        SettingsItem::IndentGuides => "Indent guides",
        SettingsItem::ActiveIndentGuide => "Highlight active indent guide",
        SettingsItem::RenderWhitespace => "Render whitespace",
        SettingsItem::ControlCharacters => "Render control characters",
        SettingsItem::Rulers => "Rulers",
        SettingsItem::SmartSelectSubwords => "Smart select subwords",
        SettingsItem::SmartSelectWhitespace => "Smart select include whitespace",
        SettingsItem::MultiCursorLimit => "Multi-cursor limit",
        _ => "Editor setting",
    }
}

fn settings_section(label: &'static str, theme: Theme, scale: f32) -> impl IntoElement {
    div()
        .pt_5()
        .pb_2()
        .border_b_1()
        .border_color(rgb(theme.role.border))
        .text_size(metrics::px_for_scale(metrics::UI_TEXT_LG, scale))
        .text_color(rgb(theme.role.text))
        .child(label)
}

fn settings_content_item(element: impl IntoElement, scale: f32) -> AnyElement {
    div()
        .w_full()
        .max_w(metrics::px_for_scale(860.0, scale))
        .mx_auto()
        .px_5()
        .child(element)
        .into_any_element()
}

fn setting_row(
    label: impl Into<String>,
    control: AnyElement,
    selected: bool,
    theme: Theme,
    scale: f32,
) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(metrics::px_for_scale(42.0, scale))
        .py_1()
        .when(!selected, |row| row.border_b_1().border_color(rgb(theme.role.border)))
        .when(selected, |row| {
            row.rounded_sm()
                .border_1()
                .border_color(rgb(theme.role.focus_outline))
                .bg(rgb(theme.role.selection_bg))
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(metrics::px_for_scale(metrics::UI_TEXT_MD, scale))
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
        .border_color(rgb(theme.role.control_border))
        .children(children)
}

fn setting_choice(
    id: &'static str,
    label: impl Into<String>,
    active: bool,
    theme: Theme,
    scale: f32,
) -> Stateful<gpui::Div> {
    let hover_bg = if active {
        theme.role.accent
    } else {
        theme.role.control_bg_hover
    };
    let pressed_bg = theme.role.selection_bg;
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
        .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(hover_bg)))
        .active(move |style| style.bg(rgb(pressed_bg)).opacity(0.82))
        .child(label.into())
}

fn font_family_option(
    id: &'static str,
    family: &'static str,
    selected: bool,
    current: bool,
    theme: Theme,
    scale: f32,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .h(metrics::px_for_scale(30.0, scale))
        .px_2()
        .rounded_sm()
        .bg(rgb(if selected {
            theme.role.control_bg_hover
        } else {
            theme.role.panel_bg
        }))
        .text_color(rgb(theme.role.text))
        .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
        .child(family)
        .when(current, |row| {
            row.child(div().text_color(rgb(theme.role.accent)).child("Selected"))
        })
}

fn danger_choice(
    id: &'static str,
    label: &'static str,
    selected: bool,
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
        .border_1()
        .border_color(rgb(if selected {
            theme.role.error_text
        } else {
            theme.role.control_border
        }))
        .bg(rgb(theme.role.control_bg))
        .text_color(rgb(theme.role.error_text))
        .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
        .child(label)
}

fn toggle_button(id: &'static str, enabled: bool, theme: Theme, scale: f32) -> Stateful<gpui::Div> {
    setting_choice(id, if enabled { "On" } else { "Off" }, enabled, theme, scale)
}

fn setting_value(value: impl Into<String>, theme: Theme, scale: f32) -> impl IntoElement {
    div()
        .max_w(metrics::px_for_scale(430.0, scale))
        .truncate()
        .text_size(metrics::px_for_scale(metrics::UI_TEXT_SM, scale))
        .text_color(rgb(theme.role.text_muted))
        .child(value.into())
}

#[cfg(test)]
mod tests {
    use super::{parse_ruler_columns, settings_query_matches, FontFamilyChoice};

    #[test]
    fn settings_search_matches_all_terms_across_labels_and_metadata() {
        assert!(settings_query_matches(
            "save shift",
            &["Keybindings", "Save As", "File", "file.save_as", "ctrl-shift-s"]
        ));
        assert!(settings_query_matches(
            "JETBRAINS font",
            &["Editor", "Font family", "JetBrains Mono"]
        ));
        assert!(!settings_query_matches("save zoom", &["Appearance", "Zoom", "scale"]));
    }

    #[test]
    fn empty_settings_search_keeps_every_row_visible() {
        assert!(settings_query_matches("", &["Editor", "Word wrap"]));
        assert!(settings_query_matches("   ", &["Files", "Autosave"]));
    }

    #[test]
    fn font_dropdown_navigation_is_bounded_to_known_families() {
        assert_eq!(FontFamilyChoice::Tx02.previous(), FontFamilyChoice::IbmPlexMono);
        assert_eq!(FontFamilyChoice::IbmPlexMono.next(), FontFamilyChoice::Tx02);
        assert_eq!(
            FontFamilyChoice::from_name("JetBrains Mono"),
            FontFamilyChoice::JetBrainsMono
        );
        assert_eq!(FontFamilyChoice::from_name("User Custom Font"), FontFamilyChoice::Tx02);
    }

    #[test]
    fn ruler_editor_normalizes_valid_columns() {
        let columns = parse_ruler_columns("120, 80  120").expect("valid ruler columns");
        assert_eq!(columns.as_slice(), &[80, 120]);
        assert!(parse_ruler_columns("").expect("empty ruler list").as_slice().is_empty());
    }

    #[test]
    fn ruler_editor_rejects_malformed_or_out_of_range_columns() {
        assert!(parse_ruler_columns("80, nope").is_err());
        assert!(parse_ruler_columns("0").is_err());
        assert!(parse_ruler_columns("1001").is_err());
    }
}
