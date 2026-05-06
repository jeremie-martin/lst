use gpui::{
    point, Bounds, Context, EntityInputHandler, KeyDownEvent, Modifiers, ModifiersChangedEvent,
    Pixels, Point, UTF16Selection, Window,
};
use lst_editor::vim::{self, Key as VimKey, Modifiers as VimModifiers, NamedKey as VimNamedKey};
use ropey::Rope;
use std::{ops::Range, time::Instant};

use crate::viewport::{code_origin_x, row_contains_cursor, scroll_left_for, x_for_global_char};
use crate::{elapsed_ms, ui::theme::metrics, LstGpuiApp};

const X11_SYNTHETIC_MODIFIER_CHORD_WINDOW_MS: u128 = 500;

impl LstGpuiApp {
    pub(crate) fn note_modifiers_changed_for_text_input(&mut self, event: &ModifiersChangedEvent) {
        if modifiers_active(event.modifiers) {
            self.modifier_chord_accumulated =
                merge_modifiers(self.modifier_chord_accumulated, event.modifiers);
            self.recent_modifier_chord = None;
        } else if modifiers_active(self.modifier_chord_accumulated) {
            self.recent_modifier_chord = Some((self.modifier_chord_accumulated, Instant::now()));
            self.modifier_chord_accumulated = Modifiers::default();
        }
    }

    pub(crate) fn maybe_handle_recent_modifier_key_action(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.editor_input_is_focused() {
            return false;
        }

        let modifiers = self.effective_modifier_chord(event.keystroke.modifiers);
        if !modifiers_active(modifiers) {
            return false;
        }

        let key = event.keystroke.key.to_ascii_lowercase();
        if self.model.vim_mode() != vim::Mode::Insert
            && vim_owns_modifier_chord(key.as_str(), modifiers)
        {
            self.x11_ctrl_k_pending = false;
            return false;
        }

        let mut preserves_ctrl_k_pending = false;
        let handled = if modifiers.control
            && modifiers.shift
            && modifiers.alt
            && !modifiers.platform
        {
            match key.as_str() {
                "down" | "up" => {
                    self.update_model(cx, true, |model| {
                        model.duplicate_line();
                    });
                    true
                }
                _ => false,
            }
        } else if modifiers.control && modifiers.shift && !modifiers.alt && !modifiers.platform {
            match key.as_str() {
                "left" => {
                    self.update_model(cx, true, |model| {
                        model.move_word(true, true);
                    });
                    true
                }
                "right" => {
                    self.update_model(cx, true, |model| {
                        model.move_word(false, true);
                    });
                    true
                }
                "l" => {
                    self.update_model(cx, true, |model| {
                        model.select_all_occurrences();
                    });
                    true
                }
                _ => false,
            }
        } else if modifiers.control && !modifiers.shift && !modifiers.alt && !modifiers.platform {
            match key.as_str() {
                "a" => {
                    self.update_model(cx, true, |model| {
                        model.select_all();
                    });
                    true
                }
                "d" => {
                    let skip = self.x11_ctrl_k_pending;
                    self.x11_ctrl_k_pending = false;
                    self.update_model(cx, true, |model| {
                        if skip {
                            model.skip_next_occurrence();
                        } else {
                            model.select_next_occurrence();
                        }
                    });
                    true
                }
                "g" => {
                    self.x11_ctrl_k_pending = false;
                    self.update_model(cx, true, |model| {
                        model.toggle_goto_line_panel();
                    });
                    true
                }
                "k" => {
                    self.x11_ctrl_k_pending = true;
                    preserves_ctrl_k_pending = true;
                    cx.notify();
                    true
                }
                _ => {
                    self.x11_ctrl_k_pending = false;
                    false
                }
            }
        } else if modifiers.shift && !modifiers.control && !modifiers.alt && !modifiers.platform {
            match key.as_str() {
                "left" => {
                    self.update_model(cx, true, |model| {
                        model.move_horizontal_by(-1, true);
                    });
                    true
                }
                "right" => {
                    self.update_model(cx, true, |model| {
                        model.move_horizontal_by(1, true);
                    });
                    true
                }
                "tab" => {
                    self.update_model(cx, true, |model| {
                        model.outdent_at_cursor();
                    });
                    true
                }
                _ => false,
            }
        } else if modifiers.platform && modifiers.shift && !modifiers.control && !modifiers.alt {
            match key.as_str() {
                "left" | "home" => {
                    self.update_model(cx, true, |model| {
                        model.move_line_boundary(false, true);
                    });
                    true
                }
                "right" | "end" => {
                    self.update_model(cx, true, |model| {
                        model.move_line_boundary(true, true);
                    });
                    true
                }
                _ => false,
            }
        } else if modifiers.alt && modifiers.shift && !modifiers.control && !modifiers.platform {
            match key.as_str() {
                "up" => {
                    self.update_model(cx, true, |model| {
                        model.add_cursor_above();
                    });
                    true
                }
                "down" => {
                    self.update_model(cx, true, |model| {
                        model.add_cursor_below();
                    });
                    true
                }
                _ => false,
            }
        } else {
            false
        };

        if handled {
            self.recent_modifier_chord = None;
            self.modifier_chord_accumulated = Modifiers::default();
            if !preserves_ctrl_k_pending {
                self.x11_ctrl_k_pending = false;
            }
            cx.stop_propagation();
        } else if !modifiers.shift || modifiers.control || modifiers.alt || modifiers.platform {
            self.recent_modifier_chord = None;
            self.modifier_chord_accumulated = Modifiers::default();
            self.x11_ctrl_k_pending = false;
        }
        handled
    }

    pub(crate) fn maybe_handle_unmodified_key_action(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.editor_input_is_focused() {
            return false;
        }

        if modifiers_active(event.keystroke.modifiers) {
            return false;
        }

        match event.keystroke.key.as_str() {
            "pageup" | "pagedown" => {
                let down = event.keystroke.key == "pagedown";
                let wrap_columns = self.active_wrap_columns(window, cx);
                self.x11_ctrl_k_pending = false;
                self.update_model(cx, true, |model| {
                    if down {
                        model.page_down(false, wrap_columns);
                    } else {
                        model.page_up(false, wrap_columns);
                    }
                });
                cx.stop_propagation();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn maybe_handle_vim_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let effective_modifiers = self.effective_modifier_chord(event.keystroke.modifiers);
        let mods = gpui_modifiers_to_vim(effective_modifiers);
        let key = gpui_key_to_vim(event);
        let plain_vim_key = !effective_modifiers.control
            && !effective_modifiers.alt
            && !effective_modifiers.platform;
        let redo_key = key.as_ref().is_some_and(|key| {
            matches!(key, VimKey::Character(value) if value == "r") && mods.command()
        });
        let ctrl_vim_motion = key.as_ref().is_some_and(|key| {
            mods.control()
                && matches!(
                    key,
                    VimKey::Character(value)
                        if matches!(value.as_str(), "d" | "u" | "f" | "b")
                )
        });

        if event.keystroke.key == "escape" {
            self.x11_ctrl_k_pending = false;
            self.update_model(cx, true, |model| {
                model.handle_vim_escape();
            });
            cx.stop_propagation();
            return true;
        }

        if self.model.vim_mode() == vim::Mode::Insert {
            return false;
        }

        if !plain_vim_key && !redo_key && !ctrl_vim_motion {
            return false;
        }

        let Some(key) = key else {
            if plain_vim_key {
                cx.stop_propagation();
                return true;
            }
            return false;
        };

        let wrap_columns = self.active_wrap_columns(window, cx);
        self.x11_ctrl_k_pending = false;
        self.update_model(cx, true, |model| {
            model.handle_vim_key(key, mods, wrap_columns);
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
            range: char_range_to_utf16_range(tab.buffer(), &tab.selected_range()),
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
        self.x11_ctrl_k_pending = false;
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
        self.x11_ctrl_k_pending = false;
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
        self.x11_ctrl_k_pending = false;
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
        let active_view = self.active_view();
        let geometry = active_view.geometry.borrow();
        let range = utf16_range_to_char_range(tab.buffer(), &range_utf16);
        let row = geometry
            .rows
            .iter()
            .rfind(|row| row_contains_cursor(row, range.start))?;
        let origin_x = code_origin_x(
            element_bounds.left(),
            self.model.show_gutter(),
            self.ui_scale(),
            scroll_left_for(&active_view.scroll),
        );
        let start_x =
            origin_x + x_for_global_char(row, range.start).unwrap_or_else(|| gpui::px(0.0));
        let end_x = origin_x
            + x_for_global_char(row, range.end.min(row.display_end_char))
                .unwrap_or_else(|| gpui::px(0.0));
        Some(Bounds::from_corners(
            point(start_x, row.row_top),
            point(
                end_x.max(start_x + metrics::px_for_scale(metrics::CURSOR_WIDTH, self.ui_scale())),
                row.row_top + metrics::px_for_scale(metrics::ROW_HEIGHT, self.ui_scale()),
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

impl LstGpuiApp {
    fn effective_modifier_chord(&self, event_modifiers: Modifiers) -> Modifiers {
        let mut modifiers = event_modifiers;
        if let Some(recent) = self.recent_modifier_chord() {
            modifiers = merge_modifiers(modifiers, recent);
        }
        modifiers = merge_modifiers(modifiers, self.modifier_chord_accumulated);
        merge_modifiers(modifiers, x11_current_modifiers())
    }

    fn recent_modifier_chord(&self) -> Option<Modifiers> {
        self.recent_modifier_chord
            .filter(|(_, released_at)| {
                released_at.elapsed().as_millis() <= X11_SYNTHETIC_MODIFIER_CHORD_WINDOW_MS
            })
            .map(|(modifiers, _)| modifiers)
    }

    fn editor_input_is_focused(&self) -> bool {
        !self.recent.is_open() && self.focus_last_applied == crate::FocusTarget::Editor
    }
}

fn vim_owns_modifier_chord(key: &str, modifiers: Modifiers) -> bool {
    modifiers.control
        && !modifiers.shift
        && !modifiers.alt
        && !modifiers.platform
        && matches!(key, "d" | "u" | "f" | "b")
}

fn modifiers_active(modifiers: Modifiers) -> bool {
    modifiers.control || modifiers.alt || modifiers.shift || modifiers.platform
}

fn merge_modifiers(lhs: Modifiers, rhs: Modifiers) -> Modifiers {
    Modifiers {
        control: lhs.control || rhs.control,
        alt: lhs.alt || rhs.alt,
        shift: lhs.shift || rhs.shift,
        platform: lhs.platform || rhs.platform,
        function: lhs.function || rhs.function,
    }
}

#[cfg(target_os = "linux")]
fn x11_current_modifiers() -> Modifiers {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{ConnectionExt as _, KeyButMask};

    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return Modifiers::default();
    };
    let Some(screen) = conn.setup().roots.get(screen_num) else {
        return Modifiers::default();
    };
    let Ok(cookie) = conn.query_pointer(screen.root) else {
        return Modifiers::default();
    };
    let Ok(reply) = cookie.reply() else {
        return Modifiers::default();
    };

    Modifiers {
        control: reply.mask.contains(KeyButMask::CONTROL),
        alt: reply.mask.contains(KeyButMask::MOD1),
        shift: reply.mask.contains(KeyButMask::SHIFT),
        platform: reply.mask.contains(KeyButMask::MOD4),
        function: false,
    }
}

#[cfg(not(target_os = "linux"))]
fn x11_current_modifiers() -> Modifiers {
    Modifiers::default()
}

fn gpui_modifiers_to_vim(modifiers: gpui::Modifiers) -> VimModifiers {
    VimModifiers {
        command: modifiers.control || modifiers.platform,
        control: modifiers.control,
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
