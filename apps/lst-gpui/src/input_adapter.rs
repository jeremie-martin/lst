use gpui::{
    point, Bounds, Context, EntityInputHandler, KeyDownEvent, Pixels, Point, UTF16Selection, Window,
};
use lst_editor::vim::{self, Key as VimKey, Modifiers as VimModifiers, NamedKey as VimNamedKey};
use ropey::Rope;
use std::{ops::Range, time::Instant};

use crate::viewport::{code_origin_pad, row_contains_cursor, x_for_global_char};
use crate::{elapsed_ms, LstGpuiApp, CURSOR_WIDTH, ROW_HEIGHT};

impl LstGpuiApp {
    pub(crate) fn maybe_handle_vim_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let mods = gpui_modifiers_to_vim(event.keystroke.modifiers);
        let key = gpui_key_to_vim(event);
        let plain_vim_key = !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform;
        let redo_key = key.as_ref().is_some_and(|key| {
            matches!(key, VimKey::Character(value) if value == "r") && mods.command()
        });

        if event.keystroke.key == "escape" {
            self.update_model(cx, true, |model| {
                model.handle_vim_escape();
            });
            cx.stop_propagation();
            return true;
        }

        if self.model.vim_mode() == vim::Mode::Insert {
            return false;
        }

        if !plain_vim_key && !redo_key {
            return false;
        }

        let Some(key) = key else {
            if plain_vim_key {
                cx.stop_propagation();
                return true;
            }
            return false;
        };

        self.update_model(cx, true, |model| {
            model.handle_vim_key(key, mods);
        });
        cx.stop_propagation();
        true
    }
}

impl EntityInputHandler for LstGpuiApp {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let tab = self.active_tab();
        let range = utf16_range_to_char_range(tab.buffer(), &range_utf16);
        *actual_range = Some(char_range_to_utf16_range(tab.buffer(), &range));
        Some(tab.buffer().slice(range).to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let tab = self.active_tab();
        Some(UTF16Selection {
            range: char_range_to_utf16_range(tab.buffer(), &tab.selection()),
            reversed: tab.selection_reversed(),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let tab = self.active_tab();
        tab.marked_range()
            .map(|range| char_range_to_utf16_range(tab.buffer(), range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.update_model(cx, true, |model| {
            model.clear_marked_text();
        });
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        let range = {
            let tab = self.active_tab();
            range_utf16
                .as_ref()
                .map(|range| utf16_range_to_char_range(tab.buffer(), range))
        };
        self.update_model(cx, true, |model| {
            model.replace_text_from_input(range, text.to_string());
        });
        self.record_operation("text_input", None, elapsed_ms(apply_started));
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        let range = {
            let tab = self.active_tab();
            range_utf16
                .as_ref()
                .map(|range| utf16_range_to_char_range(tab.buffer(), range))
        };
        let selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| utf16_range_to_char_range_in_text(new_text, range));
        self.update_model(cx, true, |model| {
            model.replace_and_mark_text(range, new_text.to_string(), selected_range);
        });
        self.record_operation("ime_text_input", None, elapsed_ms(apply_started));
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let tab = self.active_tab();
        let geometry = self.active_view().geometry.borrow();
        let range = utf16_range_to_char_range(tab.buffer(), &range_utf16);
        let row = geometry
            .rows
            .iter()
            .rfind(|row| row_contains_cursor(row, range.start))?;
        let code_origin_x = element_bounds.left() + code_origin_pad(self.model.show_gutter());
        let start_x =
            code_origin_x + x_for_global_char(row, range.start).unwrap_or_else(|| gpui::px(0.0));
        let end_x = code_origin_x
            + x_for_global_char(row, range.end.min(row.display_end_char))
                .unwrap_or_else(|| gpui::px(0.0));
        Some(Bounds::from_corners(
            point(start_x, row.row_top),
            point(
                end_x.max(start_x + gpui::px(CURSOR_WIDTH)),
                row.row_top + gpui::px(ROW_HEIGHT),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let char_index = self.active_char_index_for_point(point);
        Some(char_to_utf16(self.active_tab().buffer(), char_index))
    }
}

fn gpui_modifiers_to_vim(modifiers: gpui::Modifiers) -> VimModifiers {
    VimModifiers {
        command: modifiers.control || modifiers.platform,
    }
}

fn gpui_key_to_vim(event: &KeyDownEvent) -> Option<VimKey> {
    if let Some(ch) = event.keystroke.key_char.as_deref() {
        if ch.chars().count() == 1 {
            return Some(VimKey::Character(ch.to_string()));
        }
    }

    match event.keystroke.key.as_str() {
        "left" => Some(VimKey::Named(VimNamedKey::ArrowLeft)),
        "right" => Some(VimKey::Named(VimNamedKey::ArrowRight)),
        "up" => Some(VimKey::Named(VimNamedKey::ArrowUp)),
        "down" => Some(VimKey::Named(VimNamedKey::ArrowDown)),
        "home" => Some(VimKey::Named(VimNamedKey::Home)),
        "end" => Some(VimKey::Named(VimNamedKey::End)),
        "pageup" => Some(VimKey::Named(VimNamedKey::PageUp)),
        "pagedown" => Some(VimKey::Named(VimNamedKey::PageDown)),
        "backspace" => Some(VimKey::Named(VimNamedKey::Backspace)),
        "delete" => Some(VimKey::Named(VimNamedKey::Delete)),
        "tab" => Some(VimKey::Named(VimNamedKey::Tab)),
        "enter" => Some(VimKey::Named(VimNamedKey::Enter)),
        value if value.chars().count() == 1 => Some(VimKey::Character(value.to_string())),
        _ => None,
    }
}

pub(crate) fn char_to_utf16(buffer: &Rope, char_offset: usize) -> usize {
    buffer
        .chars()
        .take(char_offset.min(buffer.len_chars()))
        .map(char::len_utf16)
        .sum()
}

fn utf16_to_char(buffer: &Rope, utf16_offset: usize) -> usize {
    let mut chars = 0usize;
    let mut utf16 = 0usize;
    for ch in buffer.chars() {
        if utf16 >= utf16_offset {
            break;
        }
        utf16 += ch.len_utf16();
        chars += 1;
    }
    chars
}

pub(crate) fn char_range_to_utf16_range(buffer: &Rope, range: &Range<usize>) -> Range<usize> {
    char_to_utf16(buffer, range.start)..char_to_utf16(buffer, range.end)
}

pub(crate) fn utf16_range_to_char_range(buffer: &Rope, range: &Range<usize>) -> Range<usize> {
    utf16_to_char(buffer, range.start)..utf16_to_char(buffer, range.end)
}

pub(crate) fn utf16_range_to_char_range_in_text(text: &str, range: &Range<usize>) -> Range<usize> {
    let buffer = Rope::from_str(text);
    utf16_range_to_char_range(&buffer, range)
}
