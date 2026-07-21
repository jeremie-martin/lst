use gpui::{
    point, px, Bounds, Context, EntityInputHandler, KeyDownEvent, Modifiers, ModifiersChangedEvent, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, UTF16Selection, Window,
};
use lst_editor::{
    selection::{drag_selection_range, line_range_at_char, paragraph_range_at_char, word_range_at_char},
    vim::{self, Key as VimKey, Modifiers as VimModifiers, NamedKey as VimNamedKey},
    EditorCommand, InputMode, RevealIntent, Selection, TabId,
};
use ropey::Rope;
use std::{ops::Range, time::Instant};

use crate::{
    elapsed_ms,
    ui::theme::metrics,
    viewport::{
        row_contains_cursor, scroll_left_for, scroll_to_left, scroll_to_top, scroll_top_for, x_for_global_char,
    },
    workspace_action::workspace_fallback_command,
    FocusTarget, LstGpuiApp,
};

#[derive(Clone, Debug)]
pub(crate) enum DragSelectionMode {
    Character,
    Column(usize),
    Word(Range<usize>),
    Line(Range<usize>),
    Paragraph(Range<usize>),
    PendingMove {
        source: Range<usize>,
        clicked: usize,
        tab_id: TabId,
        revision: u64,
    },
    MovingSelection {
        source: Range<usize>,
        drop: usize,
        tab_id: TabId,
        revision: u64,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveDragSelection {
    mode: DragSelectionMode,
    anchor_point: Point<Pixels>,
    last_point: Point<Pixels>,
    autoscroll_active: bool,
}

impl ActiveDragSelection {
    fn new(mode: DragSelectionMode, last_point: Point<Pixels>) -> Self {
        Self {
            mode,
            anchor_point: last_point,
            last_point,
            autoscroll_active: false,
        }
    }
}

impl LstGpuiApp {
    pub(crate) fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.set_focus(FocusTarget::Editor);
        window.focus(&self.focus_handle);
        let index = self.active_char_index_for_point(event.position);
        let gutter_click = {
            let geometry = self.active_view().geometry.borrow();
            self.model.show_gutter()
                && geometry
                    .bounds
                    .is_some_and(|bounds| event.position.x < bounds.left() + geometry.gutter_width_at_paint)
        };

        if gutter_click && !event.modifiers.alt {
            let clicked = line_range_at_char(self.active_tab().buffer(), index);
            if event.modifiers.shift {
                let anchor_index = self.model.selection_set().primary().anchor();
                let anchor = line_range_at_char(self.active_tab().buffer(), anchor_index);
                self.start_drag_selection(DragSelectionMode::Line(anchor.clone()), event.position);
                self.select_active_drag_range(anchor, clicked, cx);
            } else {
                self.start_drag_selection(DragSelectionMode::Line(clicked.clone()), event.position);
                self.select_active_range(clicked, cx);
            }
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }
        if event.modifiers.alt {
            // Single Alt-click on a point already covered by a multi-cursor
            // selection toggles that cursor off. Drops through to the add
            // paths below when there's nothing to remove.
            let mut removed = false;
            self.update_model(cx, true, |model| {
                removed = model.remove_cursor_at_char(index);
            });
            if removed {
                self.cancel_drag_selection();
                cx.notify();
                return;
            }

            if let Some((_mode, range)) = self.click_selection_mode_and_range(event.click_count, index) {
                self.cancel_drag_selection();
                self.add_active_range(range, false, cx);
                self.sync_primary_selection(cx);
                self.queue_cursor_reveal(RevealIntent::NearestEdge);
                cx.notify();
                return;
            }

            self.start_drag_selection(DragSelectionMode::Column(index), event.position);
            self.update_model(cx, true, |model| {
                model.add_cursor_at_char(index);
            });
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }

        if event.modifiers.shift && event.click_count == 1 {
            self.cancel_drag_selection();
            self.update_model(cx, true, |model| {
                model.move_to_char(index, true, None);
            });
            self.sync_primary_selection(cx);
            cx.notify();
            return;
        }

        if event.click_count == 1 {
            let primary = self.model.selection();
            let source = primary.range();
            if primary.has_selection() && source.contains(&index) {
                self.start_drag_selection(
                    DragSelectionMode::PendingMove {
                        source,
                        clicked: index,
                        tab_id: self.model.active_tab_id(),
                        revision: self.active_tab().revision(),
                    },
                    event.position,
                );
                cx.notify();
                return;
            }
        }

        if let Some((mode, range)) = self.click_selection_mode_and_range(event.click_count, index) {
            self.start_drag_selection(mode, event.position);
            self.select_active_range(range, cx);
            self.sync_primary_selection(cx);
            self.schedule_drag_autoscroll(window, cx);
            cx.notify();
            return;
        }

        self.start_drag_selection(DragSelectionMode::Character, event.position);
        self.update_model(cx, true, |model| {
            model.move_to_char(index, event.modifiers.shift, None);
        });
        self.schedule_drag_autoscroll(window, cx);
        cx.notify();
    }

    fn click_selection_mode_and_range(
        &self,
        click_count: usize,
        index: usize,
    ) -> Option<(DragSelectionMode, Range<usize>)> {
        if click_count >= 4 {
            let range = paragraph_range_at_char(self.active_tab().buffer(), index);
            return Some((DragSelectionMode::Paragraph(range.clone()), range));
        }
        if click_count == 3 {
            let range = line_range_at_char(self.active_tab().buffer(), index);
            return Some((DragSelectionMode::Line(range.clone()), range));
        }
        if click_count == 2 {
            let range = word_range_at_char(self.active_tab().buffer(), index);
            return Some((DragSelectionMode::Word(range.clone()), range));
        }
        None
    }

    pub(crate) fn on_middle_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.set_focus(FocusTarget::Editor);
        window.focus(&self.focus_handle);
        self.cancel_drag_selection();

        let index = self.active_char_index_for_point(event.position);
        match cx.read_from_primary().and_then(|item| item.text()) {
            Some(text) => {
                self.update_model(cx, true, |model| {
                    model.move_to_char(index, false, None);
                    model.paste_text_characterwise(text);
                });
            }
            None => {
                self.update_model(cx, true, |model| {
                    model.clipboard_unavailable();
                });
            }
        }
    }

    pub(crate) fn on_right_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.cancel_drag_selection();
        let index = self.active_char_index_for_point(event.position);
        let inside_selection = self
            .model
            .selection_set()
            .as_slice()
            .iter()
            .any(|selection| selection.range().contains(&index));
        if !inside_selection {
            self.update_model(cx, true, |model| model.move_to_char(index, false, None));
        }
        self.open_context_menu(event.position, cx);
    }

    pub(crate) fn on_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.update_drag_selection(event, window, cx);
    }

    pub(crate) fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.finish_drag_selection(event, cx);
    }

    fn start_drag_selection(&mut self, mode: DragSelectionMode, point: Point<Pixels>) {
        self.selection_drag = Some(ActiveDragSelection::new(mode, point));
    }

    fn update_drag_selection(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !event.dragging() {
            self.cancel_drag_selection();
            return;
        }

        let Some(drag) = self.selection_drag.as_mut() else {
            return;
        };
        drag.last_point = event.position;

        let pending = match drag.mode.clone() {
            DragSelectionMode::PendingMove {
                source,
                tab_id,
                revision,
                ..
            } => Some((source, tab_id, revision, drag.anchor_point)),
            _ => None,
        };
        if let Some((source, tab_id, revision, anchor)) = pending {
            if !drag_threshold_reached(anchor, event.position, self.ui_scale()) {
                return;
            }
            let drop = self.active_char_index_for_point(event.position);
            if let Some(drag) = self.selection_drag.as_mut() {
                drag.mode = DragSelectionMode::MovingSelection {
                    source,
                    drop,
                    tab_id,
                    revision,
                };
            }
        }

        if !self.apply_drag_selection_at_point(event.position, cx) {
            return;
        }
        self.schedule_drag_autoscroll(window, cx);
        cx.notify();
    }

    fn finish_drag_selection(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.selection_drag.take() else {
            return;
        };
        match drag.mode {
            DragSelectionMode::PendingMove {
                clicked,
                tab_id,
                revision,
                ..
            } if self.model.active_tab_id() == tab_id && self.active_tab().revision() == revision => {
                self.update_model(cx, true, |model| model.move_to_char(clicked, false, None));
            }
            DragSelectionMode::MovingSelection {
                source,
                drop,
                tab_id,
                revision,
            } if self.model.active_tab_id() == tab_id && self.active_tab().revision() == revision => {
                self.update_model(cx, true, |model| {
                    model.drag_selection_to(source, drop, event.modifiers.control);
                });
            }
            _ => {}
        }
        self.sync_primary_selection(cx);
        cx.notify();
    }

    fn cancel_drag_selection(&mut self) {
        self.selection_drag = None;
    }

    fn apply_drag_selection_at_point(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let index = self.active_char_index_for_point(position);
        let mode = self.selection_drag.as_ref().map(|drag| drag.mode.clone());
        match mode {
            Some(DragSelectionMode::Character) => {
                self.update_model(cx, true, |model| {
                    model.move_to_char(index, true, None);
                });
            }
            Some(DragSelectionMode::Column(anchor)) => {
                let index = self.column_drag_head_index(anchor, index, position);
                self.update_model(cx, true, |model| {
                    model.set_rectangular_column_selection(anchor, index);
                });
            }
            Some(DragSelectionMode::Word(anchor)) => {
                let current = word_range_at_char(self.active_tab().buffer(), index);
                self.select_active_drag_range(anchor, current, cx);
            }
            Some(DragSelectionMode::Line(anchor)) => {
                let current = line_range_at_char(self.active_tab().buffer(), index);
                self.select_active_drag_range(anchor, current, cx);
            }
            Some(DragSelectionMode::Paragraph(anchor)) => {
                let current = paragraph_range_at_char(self.active_tab().buffer(), index);
                self.select_active_drag_range(anchor, current, cx);
            }
            Some(DragSelectionMode::MovingSelection { .. }) => {
                if let Some(drag) = self.selection_drag.as_mut() {
                    if let DragSelectionMode::MovingSelection { drop, .. } = &mut drag.mode {
                        *drop = index;
                    }
                }
                return true;
            }
            Some(DragSelectionMode::PendingMove { .. }) => return false,
            None => return false,
        }
        self.queue_cursor_reveal(RevealIntent::NearestEdge);
        true
    }

    fn column_drag_head_index(&self, anchor: usize, index: usize, position: Point<Pixels>) -> usize {
        let Some(drag) = self.selection_drag.as_ref() else {
            return index;
        };
        let char_width = self.active_view().geometry.borrow().painted_char_width;
        if char_width <= px(0.0) || (position.x - drag.anchor_point.x).abs() > char_width * 0.5 {
            return index;
        }

        let buffer = self.active_tab().buffer();
        let len = buffer.len_chars();
        let anchor = anchor.min(len);
        let anchor_line = buffer.char_to_line(anchor);
        let anchor_col = anchor.saturating_sub(buffer.line_to_char(anchor_line));
        let target_line = buffer.char_to_line(index.min(len));
        let target_line_len = buffer
            .line(target_line)
            .chars()
            .take_while(|ch| *ch != '\n' && *ch != '\r')
            .count();
        buffer.line_to_char(target_line) + anchor_col.min(target_line_len)
    }

    fn schedule_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.selection_drag.as_ref() else {
            return;
        };
        if matches!(&drag.mode, DragSelectionMode::PendingMove { .. }) {
            return;
        }
        if drag.autoscroll_active
            || (self.drag_autoscroll_target().is_none() && self.drag_horizontal_autoscroll_target().is_none())
        {
            return;
        }
        if let Some(drag) = self.selection_drag.as_mut() {
            drag.autoscroll_active = true;
        }
        cx.on_next_frame(window, |this, window, cx| {
            this.run_drag_autoscroll(window, cx);
        });
    }

    fn run_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.selection_drag.as_mut() else {
            return;
        };
        drag.autoscroll_active = false;

        let vertical = self.drag_autoscroll_target();
        let horizontal = self.drag_horizontal_autoscroll_target();
        if let Some(target) = vertical {
            scroll_to_top(&self.active_view().scroll, target);
        }
        if let Some(target) = horizontal {
            scroll_to_left(&self.active_view().scroll, target);
        }
        if vertical.is_some() || horizontal.is_some() {
            if let Some(position) = self.selection_drag.as_ref().map(|drag| drag.last_point) {
                self.apply_drag_selection_at_point(position, cx);
                cx.notify();
            }
        }
        self.schedule_drag_autoscroll(window, cx);
    }

    fn drag_autoscroll_target(&self) -> Option<Pixels> {
        let position = self.selection_drag.as_ref()?.last_point;
        let geometry = self.active_view().geometry.borrow();
        let bounds = geometry.bounds?;
        let delta = drag_autoscroll_delta(position, bounds, self.ui_scale())?;
        let view = self.active_view();
        let current = scroll_top_for(&view.scroll);
        let max = view.scroll.max_offset().height.max(px(0.0));
        let target = (current + delta).max(px(0.0)).min(max);
        (target != current).then_some(target)
    }

    fn drag_horizontal_autoscroll_target(&self) -> Option<Pixels> {
        if self.model.show_wrap() {
            return None;
        }
        let position = self.selection_drag.as_ref()?.last_point;
        let bounds = self.active_view().geometry.borrow().bounds?;
        let delta = drag_horizontal_autoscroll_delta(position, bounds, self.ui_scale())?;
        let view = self.active_view();
        let current = scroll_left_for(&view.scroll);
        let max = view.scroll.max_offset().width.max(px(0.0));
        let target = (current + delta).max(px(0.0)).min(max);
        (target != current).then_some(target)
    }

    fn select_active_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.update_model(cx, true, |model| {
            model.set_selection(Selection::from_range(range, false));
        });
    }

    fn add_active_range(&mut self, range: Range<usize>, reversed: bool, cx: &mut Context<Self>) {
        self.update_model(cx, true, |model| {
            model.add_selection_range(range, reversed);
        });
    }

    fn select_active_drag_range(&mut self, anchor: Range<usize>, current: Range<usize>, cx: &mut Context<Self>) {
        let (selection, reversed) = drag_selection_range(anchor, current);
        self.update_model(cx, true, |model| {
            model.set_selection(Selection::from_range(selection, reversed));
        });
    }

    pub(crate) fn selection_drag_drop_char(&self) -> Option<usize> {
        match &self.selection_drag.as_ref()?.mode {
            DragSelectionMode::MovingSelection { source, drop, .. } if !(source.start..=source.end).contains(drop) => {
                Some(*drop)
            }
            _ => None,
        }
    }
}

fn drag_threshold_reached(from: Point<Pixels>, to: Point<Pixels>, scale: f32) -> bool {
    let threshold = metrics::px_for_scale(4.0, scale);
    (to.x - from.x).abs() >= threshold || (to.y - from.y).abs() >= threshold
}

pub(crate) fn drag_autoscroll_delta(position: Point<Pixels>, bounds: Bounds<Pixels>, scale: f32) -> Option<Pixels> {
    const EDGE_PX: f32 = 36.0;
    let edge = metrics::px_for_scale(EDGE_PX, scale);
    let top_edge = bounds.top() + edge;
    let bottom_edge = bounds.bottom() - edge;

    if position.y < top_edge {
        let distance = ((top_edge - position.y) / px(1.0)).min(EDGE_PX * scale * 2.0);
        let rows = 0.5 + distance / (EDGE_PX * scale);
        Some(-metrics::px_for_scale(
            (metrics::row_height() * rows).min(metrics::row_height() * 3.0),
            scale,
        ))
    } else if position.y > bottom_edge {
        let distance = ((position.y - bottom_edge) / px(1.0)).min(EDGE_PX * scale * 2.0);
        let rows = 0.5 + distance / (EDGE_PX * scale);
        Some(metrics::px_for_scale(
            (metrics::row_height() * rows).min(metrics::row_height() * 3.0),
            scale,
        ))
    } else {
        None
    }
}

fn drag_horizontal_autoscroll_delta(position: Point<Pixels>, bounds: Bounds<Pixels>, scale: f32) -> Option<Pixels> {
    const EDGE_PX: f32 = 36.0;
    let edge = metrics::px_for_scale(EDGE_PX, scale);
    let left_edge = bounds.left() + edge;
    let right_edge = bounds.right() - edge;
    let max_step = metrics::px_for_scale(metrics::row_height() * 3.0, scale);
    if position.x < left_edge {
        let distance = ((left_edge - position.x) / px(1.0)).min(EDGE_PX * scale * 2.0);
        Some(-metrics::px_for_scale(metrics::row_height() * (0.5 + distance / (EDGE_PX * scale)), scale).min(max_step))
    } else if position.x > right_edge {
        let distance = ((position.x - right_edge) / px(1.0)).min(EDGE_PX * scale * 2.0);
        Some(metrics::px_for_scale(metrics::row_height() * (0.5 + distance / (EDGE_PX * scale)), scale).min(max_step))
    } else {
        None
    }
}

const X11_SYNTHETIC_MODIFIER_CHORD_WINDOW_MS: u128 = 500;

impl LstGpuiApp {
    pub(crate) fn clear_x11_modifier_chord_state(&mut self) {
        self.recent_modifier_chord = None;
        self.modifier_chord_accumulated = Modifiers::default();
        self.x11_ctrl_k_pending = false;
    }

    fn clear_recent_x11_modifier_chord(&mut self) {
        self.recent_modifier_chord = None;
        self.modifier_chord_accumulated = Modifiers::default();
    }

    pub(crate) fn note_modifiers_changed_for_text_input(&mut self, event: &ModifiersChangedEvent) {
        if modifiers_active(event.modifiers) {
            self.modifier_chord_accumulated = merge_modifiers(self.modifier_chord_accumulated, event.modifiers);
            self.recent_modifier_chord = None;
        } else if modifiers_active(self.modifier_chord_accumulated) {
            self.recent_modifier_chord = Some((self.modifier_chord_accumulated, Instant::now()));
            self.modifier_chord_accumulated = Modifiers::default();
        }
    }

    pub(crate) fn maybe_handle_recent_modifier_key_action(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.editor_input_is_focused() {
            return false;
        }

        let vim_enabled = self.model.input_mode() == InputMode::Vim;
        let insert_mode = !vim_enabled || self.model.vim_mode() == vim::Mode::Insert;
        let modifiers = if insert_mode {
            self.effective_current_modifiers(event.keystroke.modifiers)
        } else {
            self.effective_modifier_chord(event.keystroke.modifiers)
        };
        if !modifiers_active(modifiers) {
            if insert_mode {
                self.clear_recent_x11_modifier_chord();
            }
            return false;
        }

        let key = event.keystroke.key.to_ascii_lowercase();
        if insert_mode && key == "enter" {
            self.x11_ctrl_k_pending = false;
            self.execute_model_command(cx, EditorCommand::InsertNewline);
            self.clear_recent_x11_modifier_chord();
            cx.stop_propagation();
            return true;
        }
        if !insert_mode && vim_owns_modifier_chord(key.as_str(), modifiers) {
            self.x11_ctrl_k_pending = false;
            return false;
        }

        if let Some(command) = workspace_fallback_command(&key, modifiers) {
            self.dispatch_workspace_command(command, window, cx);
            cx.stop_propagation();
            true
        } else {
            if !modifiers.shift || modifiers.control || modifiers.alt || modifiers.platform {
                self.recent_modifier_chord = None;
                self.modifier_chord_accumulated = Modifiers::default();
                self.x11_ctrl_k_pending = false;
            }
            false
        }
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

        if let Some(command) = workspace_fallback_command(event.keystroke.key.as_str(), Modifiers::default()) {
            self.dispatch_workspace_command(command, window, cx);
            cx.stop_propagation();
            true
        } else {
            false
        }
    }

    pub(crate) fn maybe_handle_vim_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.editor_input_is_focused() || self.model.input_mode() != InputMode::Vim {
            return false;
        }

        let effective_modifiers = self.effective_modifier_chord(event.keystroke.modifiers);
        let mods = gpui_modifiers_to_vim(effective_modifiers);
        let key = gpui_key_to_vim(event);
        let plain_vim_key = !effective_modifiers.control && !effective_modifiers.alt && !effective_modifiers.platform;
        let redo_key = key
            .as_ref()
            .is_some_and(|key| matches!(key, VimKey::Character(value) if value == "r") && mods.command);
        let ctrl_vim_motion = key.as_ref().is_some_and(|key| {
            mods.control
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
            let key_is_enter = matches!(key.as_ref(), Some(VimKey::Named(VimNamedKey::Enter)));
            let modified_enter = key_is_enter
                && (effective_modifiers.shift
                    || effective_modifiers.control
                    || effective_modifiers.alt
                    || effective_modifiers.platform);
            if modified_enter {
                self.x11_ctrl_k_pending = false;
                self.execute_model_command(cx, EditorCommand::InsertNewline);
                self.clear_recent_x11_modifier_chord();
                cx.stop_propagation();
                return true;
            }
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

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        let tab = self.active_tab();
        tab.marked_range()
            .map(|range| char_range_to_utf16_range(tab.buffer(), range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor_input_is_focused() {
            return;
        }
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
        if !self.editor_input_is_focused() {
            return;
        }
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
        if !text.is_empty() {
            self.clear_recent_x11_modifier_chord();
        }
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
        if !self.editor_input_is_focused() {
            return;
        }
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
        if !new_text.is_empty() {
            self.clear_recent_x11_modifier_chord();
        }
        self.record_operation("ime_text_input", None, elapsed_ms(apply_started));
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
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
        let origin_x = geometry.code_origin_x_at_paint;
        let start_x = origin_x + x_for_global_char(row, range.start).unwrap_or_else(|| gpui::px(0.0));
        let end_x =
            origin_x + x_for_global_char(row, range.end.min(row.display_end_char)).unwrap_or_else(|| gpui::px(0.0));
        Some(Bounds::from_corners(
            point(start_x, row.row_top),
            point(
                end_x.max(start_x + metrics::px_for_scale(metrics::CURSOR_WIDTH, self.ui_scale())),
                row.row_top + metrics::px_for_scale(metrics::row_height(), self.ui_scale()),
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
        let mut modifiers = self.effective_current_modifiers(event_modifiers);
        if let Some(recent) = self.recent_modifier_chord() {
            modifiers = merge_modifiers(modifiers, recent);
        }
        modifiers
    }

    fn effective_current_modifiers(&self, event_modifiers: Modifiers) -> Modifiers {
        let modifiers = merge_modifiers(event_modifiers, self.modifier_chord_accumulated);
        merge_modifiers(modifiers, x11_current_modifiers())
    }

    fn recent_modifier_chord(&self) -> Option<Modifiers> {
        self.recent_modifier_chord
            .filter(|(_, released_at)| released_at.elapsed().as_millis() <= X11_SYNTHETIC_MODIFIER_CHORD_WINDOW_MS)
            .map(|(modifiers, _)| modifiers)
    }

    fn editor_input_is_focused(&self) -> bool {
        !self.recent.is_open()
            && self.workspace_surface == crate::WorkspaceSurface::None
            && self.close_prompt.is_none()
            && self.quit_review.is_none()
            && self.cleanup_confirmation.is_none()
            && self.focus_last_applied == crate::FocusTarget::Editor
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
    buffer.char_to_utf16_cu(char_offset.min(buffer.len_chars()))
}

pub(crate) fn char_range_to_utf16_range(buffer: &Rope, range: &Range<usize>) -> Range<usize> {
    char_to_utf16(buffer, range.start)..char_to_utf16(buffer, range.end)
}

fn utf16_to_char(buffer: &Rope, utf16_offset: usize) -> usize {
    buffer.utf16_cu_to_char(utf16_offset.min(buffer.len_utf16_cu()))
}

pub(crate) fn utf16_range_to_char_range(buffer: &Rope, range: &Range<usize>) -> Range<usize> {
    utf16_to_char(buffer, range.start)..utf16_to_char(buffer, range.end)
}

pub(crate) fn utf16_range_to_char_range_in_text(text: &str, range: &Range<usize>) -> Range<usize> {
    // Walk `text` directly instead of building a Rope per IME composition tick.
    // The composition string is typically 1-10 chars; a Rope here would allocate
    // a tree just to count UTF-16 units.
    let endpoint = |target: usize| -> usize {
        let mut utf16 = 0usize;
        let mut chars = 0usize;
        for c in text.chars() {
            if utf16 >= target {
                return chars;
            }
            utf16 += c.len_utf16();
            chars += 1;
        }
        chars
    };
    endpoint(range.start)..endpoint(range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_move_threshold_is_four_scaled_pixels_on_either_axis() {
        let origin = point(px(10.0), px(20.0));
        assert!(!drag_threshold_reached(origin, point(px(13.9), px(20.0)), 1.0));
        assert!(drag_threshold_reached(origin, point(px(14.0), px(20.0)), 1.0));
        assert!(!drag_threshold_reached(origin, point(px(10.0), px(27.9)), 2.0));
        assert!(drag_threshold_reached(origin, point(px(10.0), px(28.0)), 2.0));
    }
}
