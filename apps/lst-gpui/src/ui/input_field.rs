use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, size, App, Bounds, ClipboardItem, Context, CursorStyle,
    Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use lst_editor::selection::{
    drag_selection_range, next_subword_boundary_in_text, next_word_boundary_in_text, previous_subword_boundary_in_text,
    previous_word_boundary_in_text, word_range_in_text,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::ui::theme::{current_theme, metrics};

actions!(
    lst_gpui_input,
    [
        FieldBackspace,
        FieldDelete,
        FieldDeleteWordLeft,
        FieldDeleteWordRight,
        FieldLeft,
        FieldRight,
        FieldSubwordLeft,
        FieldSubwordRight,
        FieldWordLeft,
        FieldWordRight,
        FieldSelectLeft,
        FieldSelectRight,
        FieldSelectSubwordLeft,
        FieldSelectSubwordRight,
        FieldSelectWordLeft,
        FieldSelectWordRight,
        FieldSelectAll,
        FieldHome,
        FieldEnd,
        FieldSelectHome,
        FieldSelectEnd,
        FieldPaste,
        FieldCopy,
        FieldCut,
        FieldUndo,
        FieldRedo,
        FieldSubmit,
        FieldCancel,
        FieldNext,
        FieldPrevious,
        FieldUp,
        FieldDown,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFieldNavigation {
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputFieldEvent {
    Changed(String),
    Submitted,
    Cancelled,
    NextRequested,
    PreviousRequested,
    Navigate(InputFieldNavigation),
}

pub fn input_keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("backspace", FieldBackspace, Some("InlineInput")),
        KeyBinding::new("delete", FieldDelete, Some("InlineInput")),
        KeyBinding::new("ctrl-backspace", FieldDeleteWordLeft, Some("InlineInput")),
        KeyBinding::new("ctrl-delete", FieldDeleteWordRight, Some("InlineInput")),
        KeyBinding::new("left", FieldLeft, Some("InlineInput")),
        KeyBinding::new("right", FieldRight, Some("InlineInput")),
        KeyBinding::new("ctrl-left", FieldWordLeft, Some("InlineInput")),
        KeyBinding::new("ctrl-right", FieldWordRight, Some("InlineInput")),
        KeyBinding::new("alt-left", FieldSubwordLeft, Some("InlineInput")),
        KeyBinding::new("alt-right", FieldSubwordRight, Some("InlineInput")),
        KeyBinding::new("cmd-left", FieldHome, Some("InlineInput")),
        KeyBinding::new("cmd-right", FieldEnd, Some("InlineInput")),
        KeyBinding::new("shift-left", FieldSelectLeft, Some("InlineInput")),
        KeyBinding::new("shift-right", FieldSelectRight, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-left", FieldSelectWordLeft, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-right", FieldSelectWordRight, Some("InlineInput")),
        KeyBinding::new("alt-shift-left", FieldSelectSubwordLeft, Some("InlineInput")),
        KeyBinding::new("alt-shift-right", FieldSelectSubwordRight, Some("InlineInput")),
        KeyBinding::new("shift-home", FieldSelectHome, Some("InlineInput")),
        KeyBinding::new("shift-end", FieldSelectEnd, Some("InlineInput")),
        KeyBinding::new("cmd-shift-left", FieldSelectHome, Some("InlineInput")),
        KeyBinding::new("cmd-shift-right", FieldSelectEnd, Some("InlineInput")),
        KeyBinding::new("ctrl-a", FieldSelectAll, Some("InlineInput")),
        KeyBinding::new("cmd-a", FieldSelectAll, Some("InlineInput")),
        KeyBinding::new("ctrl-c", FieldCopy, Some("InlineInput")),
        KeyBinding::new("cmd-c", FieldCopy, Some("InlineInput")),
        KeyBinding::new("ctrl-v", FieldPaste, Some("InlineInput")),
        KeyBinding::new("cmd-v", FieldPaste, Some("InlineInput")),
        KeyBinding::new("ctrl-x", FieldCut, Some("InlineInput")),
        KeyBinding::new("cmd-x", FieldCut, Some("InlineInput")),
        KeyBinding::new("ctrl-z", FieldUndo, Some("InlineInput")),
        KeyBinding::new("cmd-z", FieldUndo, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-z", FieldRedo, Some("InlineInput")),
        KeyBinding::new("cmd-shift-z", FieldRedo, Some("InlineInput")),
        KeyBinding::new("home", FieldHome, Some("InlineInput")),
        KeyBinding::new("end", FieldEnd, Some("InlineInput")),
        KeyBinding::new("enter", FieldSubmit, Some("InlineInput")),
        KeyBinding::new("escape", FieldCancel, Some("InlineInput")),
        KeyBinding::new("tab", FieldNext, Some("InlineInput")),
        KeyBinding::new("shift-tab", FieldPrevious, Some("InlineInput")),
        KeyBinding::new("up", FieldUp, Some("InlineInput")),
        KeyBinding::new("down", FieldDown, Some("InlineInput")),
    ]
}

pub struct InputField {
    focus_handle: FocusHandle,
    text: InputText,
    placeholder: SharedString,
    extra_key_context: Option<SharedString>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    horizontal_scroll: Pixels,
    selection_drag: Option<InputDragSelectionMode>,
    history: InputHistory,
    vertical_navigation: bool,
}

#[derive(Clone, Debug)]
enum InputDragSelectionMode {
    Character,
    Word(Range<usize>),
    All,
}

#[derive(Clone, Copy)]
enum TextMovement {
    PreviousGrapheme,
    NextGrapheme,
    PreviousSubword,
    NextSubword,
    PreviousWord,
    NextWord,
    Start,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InputText {
    content: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
}

const INPUT_HISTORY_LIMIT: usize = 100;

#[derive(Default)]
struct InputHistory {
    undo_stack: Vec<InputText>,
    redo_stack: Vec<InputText>,
    composition_before: Option<InputText>,
}

impl InputHistory {
    fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.composition_before = None;
    }

    fn begin_composition(&mut self, before: InputText, after: &InputText) {
        if before == *after || self.composition_before.is_some() {
            return;
        }
        self.composition_before = Some(before);
        self.redo_stack.clear();
    }

    fn record_replacement(&mut self, before: InputText, after: &InputText) {
        let before = self.composition_before.take().unwrap_or(before);
        self.record_text_change(before, after);
    }

    fn finish_composition(&mut self, current: &InputText) {
        if let Some(before) = self.composition_before.take() {
            self.record_text_change(before, current);
        }
    }

    fn record_text_change(&mut self, before: InputText, after: &InputText) {
        if before.content == after.content {
            return;
        }
        if self.undo_stack.last() != Some(&before) {
            push_bounded(&mut self.undo_stack, before);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self, current: InputText) -> Option<InputText> {
        let snapshot = self.undo_stack.pop()?;
        push_bounded(&mut self.redo_stack, current);
        Some(snapshot)
    }

    fn redo(&mut self, current: InputText) -> Option<InputText> {
        let snapshot = self.redo_stack.pop()?;
        push_bounded(&mut self.undo_stack, current);
        Some(snapshot)
    }
}

fn push_bounded(stack: &mut Vec<InputText>, snapshot: InputText) {
    stack.push(snapshot);
    if stack.len() > INPUT_HISTORY_LIMIT {
        stack.remove(0);
    }
}

impl InputText {
    fn new() -> Self {
        Self {
            content: SharedString::new_static(""),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
        }
    }

    fn set_text(&mut self, text: &str) -> bool {
        if self.content.as_ref() == text {
            return false;
        }

        self.content = SharedString::from(text.to_string());
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        true
    }

    fn select_all(&mut self) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
    }

    fn select_to(&mut self, offset: usize) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
    }

    fn select_range(&mut self, range: Range<usize>, reversed: bool) {
        self.selected_range = range.start.min(self.content.len())..range.end.min(self.content.len());
        self.selection_reversed = reversed;
        self.marked_range = None;
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        previous_word_boundary_in_text(self.content.as_ref(), offset)
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        next_word_boundary_in_text(self.content.as_ref(), offset)
    }

    fn previous_subword_boundary(&self, offset: usize) -> usize {
        previous_subword_boundary_in_text(self.content.as_ref(), offset)
    }

    fn next_subword_boundary(&self, offset: usize) -> usize {
        next_subword_boundary_in_text(self.content.as_ref(), offset)
    }

    fn word_range_at_offset(&self, offset: usize) -> Range<usize> {
        word_range_in_text(self.content.as_ref(), offset)
    }

    fn movement_target(&self, movement: TextMovement) -> usize {
        match movement {
            TextMovement::PreviousGrapheme => self.previous_boundary(self.cursor_offset()),
            TextMovement::NextGrapheme => self.next_boundary(self.cursor_offset()),
            TextMovement::PreviousSubword => self.previous_subword_boundary(self.cursor_offset()),
            TextMovement::NextSubword => self.next_subword_boundary(self.cursor_offset()),
            TextMovement::PreviousWord => self.previous_word_boundary(self.cursor_offset()),
            TextMovement::NextWord => self.next_word_boundary(self.cursor_offset()),
            TextMovement::Start => 0,
            TextMovement::End => self.content.len(),
        }
    }

    fn move_cursor(&mut self, movement: TextMovement, select: bool) {
        let target = if !select && !self.selected_range.is_empty() {
            match movement {
                TextMovement::PreviousGrapheme | TextMovement::PreviousSubword | TextMovement::PreviousWord => {
                    self.selected_range.start
                }
                TextMovement::NextGrapheme | TextMovement::NextSubword | TextMovement::NextWord => {
                    self.selected_range.end
                }
                TextMovement::Start => 0,
                TextMovement::End => self.content.len(),
            }
        } else {
            self.movement_target(movement)
        };

        if select {
            self.select_to(target);
        } else {
            self.move_to(target);
        }
    }

    fn selected_text(&self) -> Option<String> {
        (!self.selected_range.is_empty()).then(|| self.content[self.selected_range.clone()].to_string())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf8_offset_from_utf16(self.content.as_ref(), offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn replace_text(&mut self, range_utf16: Option<&Range<usize>>, new_text: &str) {
        let range = range_utf16
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content = (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range = None;
    }

    fn replace_and_mark_text(
        &mut self,
        range_utf16: Option<&Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&Range<usize>>,
    ) {
        let range = range_utf16
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content = (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
        self.marked_range = (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .map(|range_utf16| {
                utf8_offset_from_utf16(new_text, range_utf16.start)..utf8_offset_from_utf16(new_text, range_utf16.end)
            })
            .map(|new_range| new_range.start + range.start..new_range.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
    }
}

fn utf8_offset_from_utf16(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

impl InputField {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            text: InputText::new(),
            placeholder: placeholder.into(),
            extra_key_context: None,
            last_layout: None,
            last_bounds: None,
            horizontal_scroll: px(0.0),
            selection_drag: None,
            history: InputHistory::default(),
            vertical_navigation: false,
        }
    }

    pub fn with_key_context(mut self, key_context: impl Into<SharedString>) -> Self {
        self.extra_key_context = Some(key_context.into());
        self
    }

    pub fn with_vertical_navigation(mut self) -> Self {
        self.vertical_navigation = true;
        self
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.text.set_text(text) {
            self.history.clear();
            self.last_layout = None;
            cx.notify();
        }
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.text.select_all();
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn text(&self) -> &str {
        self.text.content.as_ref()
    }

    fn emit_changed(&self, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::Changed(self.text.content.to_string()));
    }

    fn finish_composition_for_history(&mut self) {
        self.text.marked_range = None;
        self.history.finish_composition(&self.text);
    }

    fn restore_history_snapshot(&mut self, mut snapshot: InputText, cx: &mut Context<Self>) {
        snapshot.marked_range = None;
        self.text = snapshot;
        self.selection_drag = None;
        self.last_layout = None;
        self.emit_changed(cx);
        cx.notify();
    }

    fn replace_text_recording(
        &mut self,
        before: InputText,
        range_utf16: Option<&Range<usize>>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) {
        self.text.replace_text(range_utf16, new_text);
        self.history.record_replacement(before, &self.text);
        self.last_layout = None;
        self.emit_changed(cx);
        cx.notify();
    }

    fn move_text(&mut self, movement: TextMovement, select: bool, cx: &mut Context<Self>) {
        self.text.move_cursor(movement, select);
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.text.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.text.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left() + self.horizontal_scroll)
    }
    fn left(&mut self, _: &FieldLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::PreviousGrapheme, false, cx);
    }
    fn right(&mut self, _: &FieldRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::NextGrapheme, false, cx);
    }
    fn word_left(&mut self, _: &FieldWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::PreviousWord, false, cx);
    }
    fn word_right(&mut self, _: &FieldWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::NextWord, false, cx);
    }
    fn subword_left(&mut self, _: &FieldSubwordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::PreviousSubword, false, cx);
    }
    fn subword_right(&mut self, _: &FieldSubwordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::NextSubword, false, cx);
    }
    fn select_left(&mut self, _: &FieldSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::PreviousGrapheme, true, cx);
    }
    fn select_right(&mut self, _: &FieldSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::NextGrapheme, true, cx);
    }
    fn select_word_left(&mut self, _: &FieldSelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::PreviousWord, true, cx);
    }
    fn select_word_right(&mut self, _: &FieldSelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::NextWord, true, cx);
    }
    fn select_subword_left(&mut self, _: &FieldSelectSubwordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::PreviousSubword, true, cx);
    }
    fn select_subword_right(&mut self, _: &FieldSelectSubwordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::NextSubword, true, cx);
    }
    fn select_all_action(&mut self, _: &FieldSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all(cx);
    }
    fn home(&mut self, _: &FieldHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::Start, false, cx);
    }
    fn end(&mut self, _: &FieldEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::End, false, cx);
    }
    fn select_home(&mut self, _: &FieldSelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::Start, true, cx);
    }
    fn select_end(&mut self, _: &FieldSelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_text(TextMovement::End, true, cx);
    }

    fn backspace(&mut self, _: &FieldBackspace, _window: &mut Window, cx: &mut Context<Self>) {
        let before = self.text.clone();
        if self.text.selected_range.is_empty() {
            self.text
                .select_to(self.text.movement_target(TextMovement::PreviousGrapheme));
        }
        self.replace_text_recording(before, None, "", cx);
    }

    fn delete(&mut self, _: &FieldDelete, _window: &mut Window, cx: &mut Context<Self>) {
        let before = self.text.clone();
        if self.text.selected_range.is_empty() {
            self.text
                .select_to(self.text.movement_target(TextMovement::NextGrapheme));
        }
        self.replace_text_recording(before, None, "", cx);
    }

    fn delete_word_left(&mut self, _: &FieldDeleteWordLeft, _window: &mut Window, cx: &mut Context<Self>) {
        let before = self.text.clone();
        if self.text.selected_range.is_empty() {
            self.text
                .select_to(self.text.movement_target(TextMovement::PreviousWord));
        }
        self.replace_text_recording(before, None, "", cx);
    }

    fn delete_word_right(&mut self, _: &FieldDeleteWordRight, _window: &mut Window, cx: &mut Context<Self>) {
        let before = self.text.clone();
        if self.text.selected_range.is_empty() {
            self.text.select_to(self.text.movement_target(TextMovement::NextWord));
        }
        self.replace_text_recording(before, None, "", cx);
    }

    fn paste(&mut self, _: &FieldPaste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &FieldCopy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.text.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &FieldCut, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.text.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.replace_text_in_range(None, "", window, cx);
        }
    }
    fn undo(&mut self, _: &FieldUndo, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition_for_history();
        if let Some(snapshot) = self.history.undo(self.text.clone()) {
            self.restore_history_snapshot(snapshot, cx);
        }
    }
    fn redo(&mut self, _: &FieldRedo, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_composition_for_history();
        if let Some(snapshot) = self.history.redo(self.text.clone()) {
            self.restore_history_snapshot(snapshot, cx);
        }
    }
    fn submit(&mut self, _: &FieldSubmit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::Submitted);
    }
    fn cancel(&mut self, _: &FieldCancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::Cancelled);
    }
    fn next(&mut self, _: &FieldNext, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::NextRequested);
    }
    fn previous(&mut self, _: &FieldPrevious, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::PreviousRequested);
    }
    fn up(&mut self, _: &FieldUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.vertical_navigation {
            cx.emit(InputFieldEvent::Navigate(InputFieldNavigation::Up));
        }
    }
    fn down(&mut self, _: &FieldDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.vertical_navigation {
            cx.emit(InputFieldEvent::Navigate(InputFieldNavigation::Down));
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        window.focus(&self.focus_handle);
        let offset = self.index_for_mouse_position(event.position);
        if event.click_count >= 3 {
            self.start_drag_selection(InputDragSelectionMode::All);
            self.select_all(cx);
            return;
        }
        if event.click_count == 2 {
            let range = self.text.word_range_at_offset(offset);
            self.start_drag_selection(InputDragSelectionMode::Word(range.clone()));
            self.text.select_range(range, false);
            cx.notify();
            return;
        }

        self.start_drag_selection(InputDragSelectionMode::Character);
        if event.modifiers.shift {
            self.text.select_to(offset);
        } else {
            self.text.move_to(offset);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        // `on_mouse_up_out` runs during capture. Only consume that release
        // when this field actually owns an in-progress selection drag;
        // otherwise a focused field would swallow every button click elsewhere
        // in the workspace before the target receives mouse-up.
        if self.selection_drag.is_some() {
            cx.stop_propagation();
            self.cancel_drag_selection();
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        self.update_drag_selection(event, cx);
    }

    fn start_drag_selection(&mut self, mode: InputDragSelectionMode) {
        self.selection_drag = Some(mode);
    }

    fn cancel_drag_selection(&mut self) {
        self.selection_drag = None;
    }

    fn update_drag_selection(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !event.dragging() {
            self.cancel_drag_selection();
            return;
        }

        let offset = self.index_for_mouse_position(event.position);
        match self.selection_drag.clone() {
            Some(InputDragSelectionMode::Character) => {
                self.text.select_to(offset);
                cx.notify();
            }
            Some(InputDragSelectionMode::Word(anchor)) => {
                let current = self.text.word_range_at_offset(offset);
                let (range, reversed) = drag_selection_range(anchor, current);
                self.text.select_range(range, reversed);
                cx.notify();
            }
            Some(InputDragSelectionMode::All) => self.select_all(cx),
            None => {}
        }
    }
}

impl EventEmitter<InputFieldEvent> for InputField {}

impl EntityInputHandler for InputField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.text.range_from_utf16(&range_utf16);
        actual_range.replace(self.text.range_to_utf16(&range));
        Some(self.text.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.text.range_to_utf16(&self.text.selected_range),
            reversed: self.text.selection_reversed,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.text
            .marked_range
            .as_ref()
            .map(|range| self.text.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.text.marked_range = None;
        self.history.finish_composition(&self.text);
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.text.clone();
        self.replace_text_recording(before, range_utf16.as_ref(), new_text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.text.clone();
        self.text
            .replace_and_mark_text(range_utf16.as_ref(), new_text, new_selected_range_utf16.as_ref());
        self.history.begin_composition(before, &self.text);
        self.last_layout = None;
        self.emit_changed(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.text.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start) - self.horizontal_scroll,
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end) - self.horizontal_scroll,
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(point.x - line_point.x + self.horizontal_scroll)?;
        Some(self.text.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<InputField>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    horizontal_scroll: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let rem_size = window.rem_size();
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = metrics::px_for_rem(metrics::INPUT_TEXT_LINE_HEIGHT, rem_size).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = current_theme(cx);
        let input = self.input.read(cx);
        let content = input.text.content.clone();
        let content_is_empty = content.is_empty();
        let selected_range = input.text.selected_range.clone();
        let cursor = input.text.cursor_offset();

        let (display_text, text_color) = if content_is_empty {
            (input.placeholder.clone(), rgb(theme.role.text_muted))
        } else {
            (content, rgb(theme.role.text))
        };

        let run = TextRun {
            len: display_text.len(),
            font: window.text_style().font(),
            color: text_color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.text.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = metrics::px_for_rem(metrics::INPUT_TEXT_SIZE, window.rem_size());
        let line = window.text_system().shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let caret_margin = metrics::px_for_rem(4.0, window.rem_size());
        let visible_width = bounds.size.width.max(px(1.0));
        let max_scroll = (line.width - visible_width + caret_margin).max(px(0.0));
        let mut horizontal_scroll = if content_is_empty {
            px(0.0)
        } else {
            input.horizontal_scroll.min(max_scroll)
        };
        if cursor_pos < horizontal_scroll + caret_margin {
            horizontal_scroll = (cursor_pos - caret_margin).max(px(0.0));
        } else if cursor_pos > horizontal_scroll + visible_width - caret_margin {
            horizontal_scroll = (cursor_pos - visible_width + caret_margin).min(max_scroll);
        }
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos - horizontal_scroll, bounds.top()),
                        size(
                            metrics::px_for_rem(2.0, window.rem_size()),
                            bounds.bottom() - bounds.top(),
                        ),
                    ),
                    rgb(theme.role.accent),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start) - horizontal_scroll,
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end) - horizontal_scroll,
                            bounds.bottom(),
                        ),
                    ),
                    rgb(theme.role.selection_bg),
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
            horizontal_scroll,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("input line prepainted");
        let paint_origin = point(bounds.origin.x - prepaint.horizontal_scroll, bounds.origin.y);
        let _ = line.paint(paint_origin, bounds.size.height, window, cx);
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.horizontal_scroll = prepaint.horizontal_scroll;
        });
    }
}

impl Focusable for InputField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let focused = self.focus_handle.is_focused(window);
        let theme = current_theme(cx);
        let border = if focused {
            rgb(theme.role.accent)
        } else {
            rgb(theme.role.border)
        };
        let key_context = self
            .extra_key_context
            .as_ref()
            .map(|extra| format!("InlineInput {}", extra.as_ref()))
            .unwrap_or_else(|| "InlineInput".to_string());

        div()
            .track_focus(&self.focus_handle)
            .key_context(key_context.as_str())
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::subword_left))
            .on_action(cx.listener(Self::subword_right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_subword_left))
            .on_action(cx.listener(Self::select_subword_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all_action))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .relative()
            .flex()
            .w_full()
            .h(metrics::px_for_rem(metrics::INPUT_HEIGHT, window.rem_size()))
            .px(metrics::px_for_rem(metrics::INPUT_HORIZONTAL_PAD, window.rem_size()))
            .items_center()
            .overflow_hidden()
            .line_height(metrics::px_for_rem(metrics::INPUT_TEXT_LINE_HEIGHT, window.rem_size()))
            .rounded_sm()
            .bg(if focused {
                rgb(theme.role.control_bg_hover)
            } else {
                rgb(theme.role.control_bg)
            })
            .border_1()
            .border_color(border)
            .hover(move |style| style.bg(rgb(theme.role.control_bg_hover)))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(TextElement { input: entity })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_text(history: &mut InputHistory, text: &mut InputText, inserted: &str) {
        let before = text.clone();
        text.replace_text(None, inserted);
        history.record_replacement(before, text);
    }

    #[test]
    fn local_history_undoes_and_redoes_text_without_recording_cursor_motion() {
        let mut text = InputText::new();
        let mut history = InputHistory::default();
        type_text(&mut history, &mut text, "a");
        type_text(&mut history, &mut text, "b");
        assert_eq!(text.content.as_ref(), "ab");
        assert_eq!(history.undo_stack.len(), 2);

        text.move_to(0);
        assert_eq!(history.undo_stack.len(), 2, "cursor movement is not an edit");
        text = history.undo(text.clone()).expect("second typing edit is undoable");
        assert_eq!(text.content.as_ref(), "a");
        assert_eq!(text.selected_range, 1..1);
        text = history.redo(text.clone()).expect("undone typing edit is redoable");
        assert_eq!(text.content.as_ref(), "ab");
    }

    #[test]
    fn local_history_is_bounded() {
        let mut text = InputText::new();
        let mut history = InputHistory::default();
        for _ in 0..INPUT_HISTORY_LIMIT + 20 {
            type_text(&mut history, &mut text, "x");
        }
        assert_eq!(history.undo_stack.len(), INPUT_HISTORY_LIMIT);

        let mut undo_count = 0;
        while let Some(snapshot) = history.undo(text.clone()) {
            text = snapshot;
            undo_count += 1;
        }
        assert_eq!(undo_count, INPUT_HISTORY_LIMIT);
    }

    #[test]
    fn delete_history_restores_the_caret_before_the_temporary_delete_selection() {
        let mut text = InputText::new();
        assert!(text.set_text("ab"));
        let mut history = InputHistory::default();
        let before = text.clone();
        text.select_to(text.movement_target(TextMovement::PreviousGrapheme));
        text.replace_text(None, "");
        history.record_replacement(before, &text);
        assert_eq!(text.content.as_ref(), "a");

        text = history.undo(text.clone()).expect("backspace is undoable");
        assert_eq!(text.content.as_ref(), "ab");
        assert_eq!(text.selected_range, 2..2);
    }

    #[test]
    fn ime_updates_form_one_committed_history_step() {
        let mut text = InputText::new();
        assert!(text.set_text("pre"));
        let mut history = InputHistory::default();

        let before = text.clone();
        text.replace_and_mark_text(None, "x", None);
        history.begin_composition(before, &text);
        let intermediate = text.clone();
        text.replace_and_mark_text(None, "xy", None);
        history.begin_composition(intermediate, &text);
        assert_eq!(text.content.as_ref(), "prexy");
        assert_eq!(text.marked_range, Some(3..5));
        assert!(
            history.undo_stack.is_empty(),
            "composition is not committed per IME tick"
        );

        text.marked_range = None;
        history.finish_composition(&text);
        assert_eq!(history.undo_stack.len(), 1);
        text = history.undo(text.clone()).expect("committed composition is undoable");
        assert_eq!(text.content.as_ref(), "pre");
        text = history.redo(text.clone()).expect("committed composition is redoable");
        assert_eq!(text.content.as_ref(), "prexy");
        assert_eq!(text.marked_range, None);
    }

    #[test]
    fn ime_relative_selection_uses_the_composition_text_utf16_coordinates() {
        let mut text = InputText::new();
        assert!(text.set_text("pré"));
        text.replace_and_mark_text(None, "🦀x", Some(&(2..3)));
        assert_eq!(text.content.as_ref(), "pré🦀x");
        assert_eq!(text.marked_range, Some(4..9));
        assert_eq!(text.selected_range, 8..9);
    }
}
