use std::{borrow::Cow, ops::Range};

use gpui::{
    actions, canvas, div, fill, point, prelude::*, px, relative, rgb, size, App, Bounds, ClipboardItem, Context,
    CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PaintQuad, Pixels, Point, ShapedLine, SharedString, Style, Subscription, TextRun, UTF16Selection, UnderlineStyle,
    Window,
};
use lst_editor::selection::{
    drag_selection_range, next_subword_boundary_in_text, next_word_boundary_in_text, previous_subword_boundary_in_text,
    previous_word_boundary_in_text, word_range_in_text,
};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

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
        KeyBinding::new("ctrl-y", FieldRedo, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-z", FieldRedo, Some("InlineInput")),
        KeyBinding::new("cmd-shift-z", FieldRedo, Some("InlineInput")),
        KeyBinding::new("home", FieldHome, Some("InlineInput")),
        KeyBinding::new("end", FieldEnd, Some("InlineInput")),
        KeyBinding::new("ctrl-home", FieldHome, Some("InlineInput")),
        KeyBinding::new("ctrl-end", FieldEnd, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-home", FieldSelectHome, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-end", FieldSelectEnd, Some("InlineInput")),
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
    state: InputState,
    placeholder: SharedString,
    extra_key_context: Option<SharedString>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    field_bounds: Option<Bounds<Pixels>>,
    last_caret_bounds: Option<Bounds<Pixels>>,
    horizontal_scroll: Pixels,
    selection_drag: Option<InputDragSelection>,
    drag_generation: usize,
    vertical_navigation: bool,
    subscriptions: Vec<Subscription>,
}

struct InputDragSelection {
    mode: InputDragSelectionMode,
    last_point: Point<Pixels>,
    autoscroll_active: bool,
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

/// A field's text and its undo history. Every edit goes through here so
/// history sees each change exactly once.
struct InputState {
    text: InputText,
    history: InputHistory,
}

impl InputState {
    fn new() -> Self {
        Self {
            text: InputText::new(),
            history: InputHistory::default(),
        }
    }

    /// Synchronizes model text, preserving local state for an unchanged echo.
    fn set_text(&mut self, text: &str) -> bool {
        let changed = self.text.set_text(text);
        if changed {
            self.history.clear();
        }
        changed
    }

    fn replace(&mut self, range_utf16: Option<&Range<usize>>, new_text: &str) {
        let before = self.text.clone();
        self.text.replace_text(range_utf16, new_text);
        self.history.record_replacement(before, &self.text);
    }

    /// Deletes the selection, or else the text between the cursor and the
    /// movement's target. Undo restores the caret, not a temporary selection.
    fn delete(&mut self, movement: TextMovement) {
        self.commit_composition();
        let before = self.text.clone();
        if self.text.selected_range.is_empty() {
            self.text.select_to(self.text.movement_target(movement));
        }
        self.text.replace_text(None, "");
        self.history.record_replacement(before, &self.text);
    }

    fn compose(
        &mut self,
        range_utf16: Option<&Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&Range<usize>>,
    ) {
        let before = self.text.clone();
        self.text
            .replace_and_mark_text(range_utf16, new_text, new_selected_range_utf16);
        self.history.begin_composition(before, &self.text);
    }

    fn commit_composition(&mut self) -> bool {
        let marked = self.text.marked_range.take().is_some();
        let selection_changed = self.text.normalize_selection();
        self.history.finish_composition(&self.text);
        marked || selection_changed
    }

    fn undo(&mut self) -> bool {
        self.commit_composition();
        let snapshot = self.history.undo(self.text.clone());
        self.restore(snapshot)
    }

    fn redo(&mut self) -> bool {
        self.commit_composition();
        let snapshot = self.history.redo(self.text.clone());
        self.restore(snapshot)
    }

    fn restore(&mut self, snapshot: Option<InputText>) -> bool {
        let Some(mut snapshot) = snapshot else {
            return false;
        };
        snapshot.marked_range = None;
        self.text = snapshot;
        true
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

        let text = single_line_text(text);
        if self.content.as_ref() == text.as_ref() {
            return false;
        }
        self.content = SharedString::from(text.into_owned());
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

    fn normalize_selection(&mut self) -> bool {
        let previous = self.selected_range.clone();
        self.selected_range = if previous.is_empty() {
            let offset = round_grapheme_boundary(self.content.as_ref(), previous.start, true);
            offset..offset
        } else {
            round_grapheme_boundary(self.content.as_ref(), previous.start, false)
                ..round_grapheme_boundary(self.content.as_ref(), previous.end, true)
        };
        self.selected_range != previous
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

        let new_text = single_line_text(new_text);
        self.content =
            (self.content[0..range.start].to_owned() + new_text.as_ref() + &self.content[range.end..]).into();
        let cursor = round_grapheme_boundary(self.content.as_ref(), range.start + new_text.len(), true);
        self.selected_range = cursor..cursor;
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

        let selected_range = new_selected_range_utf16.map(|range_utf16| {
            let start = utf8_offset_from_utf16(new_text, range_utf16.start);
            let end = utf8_offset_from_utf16(new_text, range_utf16.end);
            single_line_text(&new_text[..start]).len()..single_line_text(&new_text[..end]).len()
        });
        let new_text = single_line_text(new_text);
        self.content =
            (self.content[0..range.start].to_owned() + new_text.as_ref() + &self.content[range.end..]).into();
        self.marked_range = (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        self.selected_range = selected_range
            .map(|new_range| new_range.start + range.start..new_range.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
    }
}

fn single_line_text(text: &str) -> Cow<'_, str> {
    const LINE_BREAKS: [char; 7] = ['\r', '\n', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'];
    let Some(first_break) = text.find(LINE_BREAKS) else {
        return Cow::Borrowed(text);
    };
    let mut normalized = String::with_capacity(text.len());
    normalized.push_str(&text[..first_break]);
    let mut chars = text[first_break..].chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
        }
        normalized.push(if LINE_BREAKS.contains(&ch) { ' ' } else { ch });
    }
    Cow::Owned(normalized)
}

fn round_grapheme_boundary(text: &str, offset: usize, up: bool) -> usize {
    let offset = offset.min(text.len());
    let mut cursor = GraphemeCursor::new(offset, text.len(), true);
    if cursor.is_boundary(text, 0).expect("complete field context") {
        return offset;
    }
    if up {
        cursor.next_boundary(text, 0)
    } else {
        cursor.prev_boundary(text, 0)
    }
    .expect("complete field context")
    .expect("an interior grapheme has both boundaries")
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
            state: InputState::new(),
            placeholder: placeholder.into(),
            extra_key_context: None,
            last_layout: None,
            last_bounds: None,
            field_bounds: None,
            last_caret_bounds: None,
            horizontal_scroll: px(0.0),
            selection_drag: None,
            drag_generation: 0,
            vertical_navigation: false,
            subscriptions: Vec::new(),
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
        if self.state.set_text(text) {
            self.last_layout = None;
            self.cancel_drag_selection();
            cx.notify();
        }
    }

    /// Starts a new use of this field, even if its initial text is unchanged.
    pub fn reset(&mut self, text: &str, cx: &mut Context<Self>) {
        self.state = InputState::new();
        self.state.set_text(text);
        self.last_layout = None;
        self.last_caret_bounds = None;
        self.horizontal_scroll = px(0.0);
        self.cancel_drag_selection();
        cx.notify();
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.state.commit_composition();
        self.cancel_drag_selection();
        self.state.text.select_all();
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn text(&self) -> &str {
        self.state.text.content.as_ref()
    }

    pub(crate) fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.field_bounds
    }

    pub(crate) fn text_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds
    }

    /// The visible selection in UTF-8 bytes.
    pub(crate) fn selected_range(&self) -> Range<usize> {
        self.state.text.selected_range.clone()
    }

    pub(crate) fn horizontal_scroll(&self) -> Pixels {
        self.horizontal_scroll
    }

    pub(crate) fn caret_bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_caret_bounds
    }

    fn emit_changed(&self, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::Changed(self.state.text.content.to_string()));
    }

    fn text_changed(&mut self, cx: &mut Context<Self>) {
        self.last_layout = None;
        self.cancel_drag_selection();
        self.emit_changed(cx);
        cx.notify();
    }

    fn delete_toward(&mut self, movement: TextMovement, cx: &mut Context<Self>) {
        self.state.delete(movement);
        self.text_changed(cx);
    }

    fn history_step_taken(&mut self, cx: &mut Context<Self>) {
        self.selection_drag = None;
        self.text_changed(cx);
    }

    fn move_text(&mut self, movement: TextMovement, select: bool, cx: &mut Context<Self>) {
        self.state.commit_composition();
        self.cancel_drag_selection();
        self.state.text.move_cursor(movement, select);
        cx.notify();
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.state.text.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
            return 0;
        };
        let x = position.x - bounds.left() + self.horizontal_scroll;
        let offset = line.closest_index_for_x(x);
        let left = round_grapheme_boundary(self.text(), offset, false);
        let right = round_grapheme_boundary(self.text(), offset, true);
        if x - line.x_for_index(left) <= line.x_for_index(right) - x {
            left
        } else {
            right
        }
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
        self.delete_toward(TextMovement::PreviousGrapheme, cx);
    }

    fn delete(&mut self, _: &FieldDelete, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_toward(TextMovement::NextGrapheme, cx);
    }

    fn delete_word_left(&mut self, _: &FieldDeleteWordLeft, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_toward(TextMovement::PreviousWord, cx);
    }

    fn delete_word_right(&mut self, _: &FieldDeleteWordRight, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_toward(TextMovement::NextWord, cx);
    }

    fn paste(&mut self, _: &FieldPaste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.state.commit_composition();
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &FieldCopy, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.commit_composition() {
            cx.notify();
        }
        if let Some(text) = self.state.text.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &FieldCut, window: &mut Window, cx: &mut Context<Self>) {
        if self.state.commit_composition() {
            cx.notify();
        }
        if let Some(text) = self.state.text.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.replace_text_in_range(None, "", window, cx);
        }
    }
    fn undo(&mut self, _: &FieldUndo, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.undo() {
            self.history_step_taken(cx);
        }
    }
    fn redo(&mut self, _: &FieldRedo, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.redo() {
            self.history_step_taken(cx);
        }
    }
    fn finish_interaction(&mut self, cx: &mut Context<Self>) {
        let composing = self.state.commit_composition();
        let dragging = self.selection_drag.take().is_some();
        if composing || dragging {
            cx.notify();
        }
    }

    fn submit(&mut self, _: &FieldSubmit, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_interaction(cx);
        cx.emit(InputFieldEvent::Submitted);
    }
    fn cancel(&mut self, _: &FieldCancel, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_interaction(cx);
        cx.emit(InputFieldEvent::Cancelled);
    }
    fn next(&mut self, _: &FieldNext, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_interaction(cx);
        cx.emit(InputFieldEvent::NextRequested);
    }
    fn previous(&mut self, _: &FieldPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.finish_interaction(cx);
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
        window.focus(&self.focus_handle, cx);
        self.state.commit_composition();
        let offset = self.index_for_mouse_position(event.position);
        if event.click_count >= 3 {
            self.start_drag_selection(InputDragSelectionMode::All, event.position);
            self.state.text.select_all();
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            let range = self.state.text.word_range_at_offset(offset);
            self.start_drag_selection(InputDragSelectionMode::Word(range.clone()), event.position);
            self.state.text.select_range(range, false);
            cx.notify();
            return;
        }

        self.start_drag_selection(InputDragSelectionMode::Character, event.position);
        if event.modifiers.shift {
            self.state.text.select_to(offset);
        } else {
            self.state.text.move_to(offset);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus_handle.is_focused(window) || !window.is_window_active() {
            self.cancel_drag_selection();
            return;
        }
        if self.selection_drag.is_some() {
            self.apply_drag_selection(event.position, cx);
            self.cancel_drag_selection();
            cx.stop_propagation();
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !event.dragging() || !self.focus_handle.is_focused(window) || !window.is_window_active() {
            self.cancel_drag_selection();
            return;
        }
        let Some(drag) = self.selection_drag.as_mut() else {
            return;
        };
        drag.last_point = event.position;
        self.apply_drag_selection(event.position, cx);
        self.schedule_drag_autoscroll(window, cx);
        cx.stop_propagation();
    }

    fn start_drag_selection(&mut self, mode: InputDragSelectionMode, position: Point<Pixels>) {
        self.drag_generation = self.drag_generation.wrapping_add(1);
        self.selection_drag = Some(InputDragSelection {
            mode,
            last_point: position,
            autoscroll_active: false,
        });
    }

    fn cancel_drag_selection(&mut self) {
        self.selection_drag = None;
    }

    fn apply_drag_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let offset = self.index_for_mouse_position(position);
        let mode = self.selection_drag.as_ref().map(|drag| drag.mode.clone());
        match mode {
            Some(InputDragSelectionMode::Character) => self.state.text.select_to(offset),
            Some(InputDragSelectionMode::Word(anchor)) => {
                let current = self.state.text.word_range_at_offset(offset);
                let (range, reversed) = drag_selection_range(anchor, current);
                self.state.text.select_range(range, reversed);
            }
            Some(InputDragSelectionMode::All) => self.state.text.select_all(),
            None => return,
        }
        cx.notify();
    }

    fn drag_scroll_target(&self, window: &Window) -> Option<Pixels> {
        let drag = self.selection_drag.as_ref()?;
        if matches!(drag.mode, InputDragSelectionMode::All) {
            return None;
        }
        let bounds = self.last_bounds?;
        let line = self.last_layout.as_ref()?;
        let distance = if drag.last_point.x < bounds.left() {
            drag.last_point.x - bounds.left()
        } else if drag.last_point.x > bounds.right() {
            drag.last_point.x - bounds.right()
        } else {
            return None;
        };
        let scale = metrics::scale_for_rem(window.rem_size());
        let magnitude = (distance.abs() / 3.0).max(px(scale)).min(px(36.0 * scale));
        let delta = if distance < px(0.0) { -magnitude } else { magnitude };
        let max_scroll = (line.width - bounds.size.width + metrics::px_for_rem(4.0, window.rem_size())).max(px(0.0));
        let target = (self.horizontal_scroll + delta).max(px(0.0)).min(max_scroll);
        (target != self.horizontal_scroll).then_some(target)
    }

    fn schedule_drag_autoscroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection_drag.as_ref().is_none_or(|drag| drag.autoscroll_active)
            || self.drag_scroll_target(window).is_none()
        {
            return;
        }
        if let Some(drag) = self.selection_drag.as_mut() {
            drag.autoscroll_active = true;
        }
        let generation = self.drag_generation;
        cx.on_next_frame(window, move |this, window, cx| {
            this.run_drag_autoscroll(generation, window, cx);
        });
    }

    fn run_drag_autoscroll(&mut self, generation: usize, window: &mut Window, cx: &mut Context<Self>) {
        if generation != self.drag_generation {
            return;
        }
        if !window.is_window_active() || !self.focus_handle.contains_focused(window, cx) {
            self.cancel_drag_selection();
            return;
        }
        let Some(drag) = self.selection_drag.as_mut() else {
            return;
        };
        drag.autoscroll_active = false;
        if let Some(target) = self.drag_scroll_target(window) {
            self.horizontal_scroll = target;
            if let Some(position) = self.selection_drag.as_ref().map(|drag| drag.last_point) {
                self.apply_drag_selection(position, cx);
            }
        }
        self.schedule_drag_autoscroll(window, cx);
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
        let range = self.state.text.range_from_utf16(&range_utf16);
        actual_range.replace(self.state.text.range_to_utf16(&range));
        Some(self.state.text.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.state.text.range_to_utf16(&self.state.text.selected_range),
            reversed: self.state.text.selection_reversed,
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.state
            .text
            .marked_range
            .as_ref()
            .map(|range| self.state.text.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.state.commit_composition() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.replace(range_utf16.as_ref(), new_text);
        self.text_changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state
            .compose(range_utf16.as_ref(), new_text, new_selected_range_utf16.as_ref());
        self.text_changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let bounds = self.last_bounds?;
        let last_layout = self.last_layout.as_ref()?;
        let range = self.state.text.range_from_utf16(&range_utf16);
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
        if !self.field_bounds?.contains(&point) {
            return None;
        }
        self.last_layout.as_ref()?;
        Some(self.state.text.offset_to_utf16(self.index_for_mouse_position(point)))
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
        let content = input.state.text.content.clone();
        let content_is_empty = content.is_empty();
        let selected_range = input.state.text.selected_range.clone();
        let cursor = input.state.text.cursor_offset();

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
        let runs = if let Some(marked_range) = input.state.text.marked_range.as_ref() {
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
        if input.selection_drag.is_none() {
            if cursor_pos < horizontal_scroll + caret_margin {
                horizontal_scroll = (cursor_pos - caret_margin).max(px(0.0));
            } else if cursor_pos > horizontal_scroll + visible_width - caret_margin {
                horizontal_scroll = (cursor_pos - visible_width + caret_margin).min(max_scroll);
            }
        }
        let caret_bounds = Bounds::new(
            point(bounds.left() + cursor_pos - horizontal_scroll, bounds.top()),
            size(metrics::px_for_rem(2.0, window.rem_size()), bounds.size.height),
        );
        let (selection, cursor) = if selected_range.is_empty() {
            (None, Some(fill(caret_bounds, rgb(theme.role.accent))))
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

        self.input.update(cx, |input, _| {
            input.last_layout = Some(line.clone());
            input.last_bounds = Some(bounds);
            input.last_caret_bounds = Some(caret_bounds);
            input.horizontal_scroll = horizontal_scroll;
        });
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
        let _ = line.paint(
            paint_origin,
            bounds.size.height,
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        let input_for_move = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase.capture() && input_for_move.read(cx).selection_drag.is_some() {
                input_for_move.update(cx, |input, cx| input.on_mouse_move(event, window, cx));
            }
        });
        let input_for_up = self.input.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase.capture() && event.button == MouseButton::Left && input_for_up.read(cx).selection_drag.is_some() {
                input_for_up.update(cx, |input, cx| input.on_mouse_up(event, window, cx));
            }
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
        if self.subscriptions.is_empty() {
            self.subscriptions
                .push(cx.on_blur(&self.focus_handle, window, |input, _, cx| {
                    input.cancel_drag_selection();
                    if input.state.commit_composition() {
                        cx.notify();
                    }
                }));
            self.subscriptions
                .push(cx.observe_window_activation(window, |input, window, _| {
                    if !window.is_window_active() {
                        input.cancel_drag_selection();
                    }
                }));
        }
        let entity = cx.entity();
        let entity_for_bounds = entity.clone();
        let focused = self.focus_handle.is_focused(window);
        let theme = current_theme(cx);
        let border = if focused {
            rgb(theme.role.focus_outline)
        } else {
            rgb(theme.role.control_border)
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
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity_for_bounds.update(cx, |input, _| input.field_bounds = Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(TextElement { input: entity })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_the_text_and_caret_of_each_edit() {
        let mut state = InputState::new();
        state.replace(None, "a");
        state.replace(None, "b");
        assert_eq!(state.text.content.as_ref(), "ab");

        state.text.move_to(0);
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "a");
        assert_eq!(state.text.selected_range, 1..1);
        assert!(state.redo());
        assert_eq!(state.text.content.as_ref(), "ab");
        assert!(!state.redo());
    }

    #[test]
    fn local_history_is_bounded() {
        let mut state = InputState::new();
        for _ in 0..INPUT_HISTORY_LIMIT + 20 {
            state.replace(None, "x");
        }
        let mut undo_count = 0;
        while state.undo() {
            undo_count += 1;
        }
        assert_eq!(undo_count, INPUT_HISTORY_LIMIT);
        assert_eq!(state.text.content.len(), 20);
    }

    #[test]
    fn delete_history_restores_the_caret_before_the_temporary_delete_selection() {
        let mut state = InputState::new();
        assert!(state.set_text("ab"));
        state.delete(TextMovement::PreviousGrapheme);
        assert_eq!(state.text.content.as_ref(), "a");

        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "ab");
        assert_eq!(state.text.selected_range, 2..2);
    }

    #[test]
    fn ime_updates_form_one_committed_history_step() {
        let mut state = InputState::new();
        assert!(state.set_text("pre"));
        state.compose(None, "x", None);
        state.compose(None, "xy", None);
        assert_eq!(state.text.content.as_ref(), "prexy");
        assert_eq!(state.text.marked_range, Some(3..5));

        state.commit_composition();
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "pre");
        assert!(!state.undo(), "composition is not committed per IME tick");
        assert!(state.redo());
        assert_eq!(state.text.content.as_ref(), "prexy");
        assert_eq!(state.text.marked_range, None);
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

    #[test]
    fn every_text_entry_boundary_normalizes_single_line_breaks() {
        for separator in ["\r\n", "\r", "\n", "\u{b}", "\u{c}", "\u{85}", "\u{2028}", "\u{2029}"] {
            let multiline = format!("a{separator}b");
            let mut state = InputState::new();
            state.set_text(&multiline);
            assert_eq!(state.text.content.as_ref(), "a b", "programmatic {separator:?}");
            state.text.select_all();
            state.replace(None, &multiline);
            assert_eq!(state.text.content.as_ref(), "a b", "committed {separator:?}");
            state.text.select_all();
            state.compose(None, &multiline, None);
            assert_eq!(state.text.content.as_ref(), "a b", "composition {separator:?}");
            assert_eq!(state.text.marked_range, Some(0..3));
        }
        assert_eq!(single_line_text("a\r\nb\r\nc\nd").as_ref(), "a b c d");
        assert!(matches!(single_line_text("plain 🦀 text"), Cow::Borrowed(_)));
    }

    #[test]
    fn canonical_model_echo_preserves_composition_selection_and_history() {
        let mut state = InputState::new();
        state.set_text("base");
        state.compose(None, "x\r\n🦀y", Some(&(3..5)));
        let before_echo = state.text.clone();
        assert_eq!(state.text.content.as_ref(), "basex 🦀y");
        assert_eq!(state.text.selected_range, 6..10);
        assert!(!state.set_text("basex 🦀y"));
        assert!(!state.set_text("basex\r\n🦀y"));
        assert_eq!(state.text, before_echo);

        state.compose(None, "xy", None);
        state.commit_composition();
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "base");
        assert!(!state.undo());
        assert!(state.redo());
        assert_eq!(state.text.content.as_ref(), "basexy");
    }

    #[test]
    fn composition_offsets_map_crlf_and_utf16_surrogates_without_rounding_graphemes() {
        for (selection, expected) in [
            (0..0, 4..4),
            (1..2, 5..6),
            (2..3, 6..6),
            (3..5, 6..10),
            (4..4, 10..10),
            (6..99, 11..11),
        ] {
            let mut text = InputText::new();
            text.set_text("pré");
            text.replace_and_mark_text(None, "x\r\n🦀y", Some(&selection));
            assert_eq!(text.content.as_ref(), "préx 🦀y");
            assert_eq!(text.marked_range, Some(4..11));
            assert_eq!(text.selected_range, expected, "UTF-16 {selection:?}");
        }
    }

    #[test]
    fn committed_edits_round_the_caret_after_joining_neighboring_graphemes() {
        let mut state = InputState::new();
        state.set_text("👩💻");
        state.text.move_to("👩".len());
        state.replace(None, "\u{200d}");
        assert_eq!(state.text.content.as_ref(), "👩‍💻");
        assert_eq!(state.text.selected_range, "👩‍💻".len().."👩‍💻".len());
        state.delete(TextMovement::NextGrapheme);
        assert_eq!(state.text.content.as_ref(), "👩‍💻");
        state.delete(TextMovement::PreviousGrapheme);
        assert_eq!(state.text.content.as_ref(), "");
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "👩‍💻");

        state.set_text("🇫x🇷");
        state.text.select_range(4..5, false);
        state.replace(None, "");
        assert_eq!(state.text.content.as_ref(), "🇫🇷");
        assert_eq!(state.text.selected_range, 8..8);
        state.delete(TextMovement::PreviousGrapheme);
        assert_eq!(state.text.content.as_ref(), "");
    }

    #[test]
    fn preedit_can_select_inside_a_grapheme_until_composition_is_committed() {
        let mut state = InputState::new();
        state.set_text("👩💻");
        state.text.move_to(4);
        state.compose(None, "\u{200d}", None);
        assert_eq!(state.text.selected_range, 7..7);
        assert_eq!(state.text.marked_range, Some(4..7));
        assert!(!state.set_text("👩‍💻"));
        assert_eq!(state.text.selected_range, 7..7);
        assert!(state.commit_composition());
        assert_eq!(state.text.selected_range, 11..11);
        assert_eq!(state.text.marked_range, None);
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "👩💻");
        assert_eq!(state.text.selected_range, 4..4);
        assert!(state.redo());
        assert_eq!(state.text.content.as_ref(), "👩‍💻");
        assert_eq!(state.text.selected_range, 11..11);
    }

    #[test]
    fn command_deletion_commits_preedit_before_editing_the_selection() {
        let mut state = InputState::new();
        state.set_text("base");
        state.compose(None, "xy", None);
        state.delete(TextMovement::PreviousGrapheme);
        assert_eq!(state.text.content.as_ref(), "basex");
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "basexy");
        assert_eq!(state.text.marked_range, None);
        assert!(state.undo());
        assert_eq!(state.text.content.as_ref(), "base");
    }

    #[test]
    fn grapheme_rounding_preserves_exact_boundaries_and_complete_unicode_context() {
        let text = "é👩‍💻🇫🇷🇩🇪क्ष";
        let boundaries = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain([text.len()])
            .collect::<Vec<_>>();
        for &boundary in &boundaries {
            assert_eq!(round_grapheme_boundary(text, boundary, false), boundary);
            assert_eq!(round_grapheme_boundary(text, boundary, true), boundary);
        }
        for (offset, _) in text.char_indices() {
            let floor = *boundaries
                .iter()
                .rev()
                .find(|&&boundary| boundary <= offset)
                .expect("starts at zero");
            let ceil = *boundaries
                .iter()
                .find(|&&boundary| boundary >= offset)
                .expect("includes text end");
            assert_eq!(round_grapheme_boundary(text, offset, false), floor);
            assert_eq!(round_grapheme_boundary(text, offset, true), ceil);
        }
    }
}
