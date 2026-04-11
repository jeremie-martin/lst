use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgb, size, App, Bounds, ClipboardItem,
    Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::{
    COLOR_ACCENT, COLOR_BORDER, COLOR_MUTED, COLOR_SELECTION, COLOR_SURFACE1, COLOR_SURFACE2,
    COLOR_TEXT, INPUT_HEIGHT, INPUT_TEXT_SIZE,
};

actions!(
    lst_ui_input,
    [
        FieldBackspace,
        FieldDelete,
        FieldLeft,
        FieldRight,
        FieldWordLeft,
        FieldWordRight,
        FieldSelectLeft,
        FieldSelectRight,
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
        FieldSubmit,
        FieldCancel,
        FieldNext,
        FieldPrevious,
    ]
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputFieldEvent {
    Changed(String),
    Submitted,
    Cancelled,
    NextRequested,
    PreviousRequested,
}

pub fn input_keybindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("backspace", FieldBackspace, Some("InlineInput")),
        KeyBinding::new("delete", FieldDelete, Some("InlineInput")),
        KeyBinding::new("left", FieldLeft, Some("InlineInput")),
        KeyBinding::new("right", FieldRight, Some("InlineInput")),
        KeyBinding::new("ctrl-left", FieldWordLeft, Some("InlineInput")),
        KeyBinding::new("ctrl-right", FieldWordRight, Some("InlineInput")),
        KeyBinding::new("alt-left", FieldWordLeft, Some("InlineInput")),
        KeyBinding::new("alt-right", FieldWordRight, Some("InlineInput")),
        KeyBinding::new("cmd-left", FieldHome, Some("InlineInput")),
        KeyBinding::new("cmd-right", FieldEnd, Some("InlineInput")),
        KeyBinding::new("shift-left", FieldSelectLeft, Some("InlineInput")),
        KeyBinding::new("shift-right", FieldSelectRight, Some("InlineInput")),
        KeyBinding::new("ctrl-shift-left", FieldSelectWordLeft, Some("InlineInput")),
        KeyBinding::new(
            "ctrl-shift-right",
            FieldSelectWordRight,
            Some("InlineInput"),
        ),
        KeyBinding::new("alt-shift-left", FieldSelectWordLeft, Some("InlineInput")),
        KeyBinding::new("alt-shift-right", FieldSelectWordRight, Some("InlineInput")),
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
        KeyBinding::new("home", FieldHome, Some("InlineInput")),
        KeyBinding::new("end", FieldEnd, Some("InlineInput")),
        KeyBinding::new("enter", FieldSubmit, Some("InlineInput")),
        KeyBinding::new("escape", FieldCancel, Some("InlineInput")),
        KeyBinding::new("tab", FieldNext, Some("InlineInput")),
        KeyBinding::new("shift-tab", FieldPrevious, Some("InlineInput")),
    ]
}

pub struct InputField {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    drag_selecting: Option<InputDragSelectionMode>,
}

#[derive(Clone, Debug)]
enum InputDragSelectionMode {
    Character,
    Word(Range<usize>),
    All,
}

impl InputField {
    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::new_static(""),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            drag_selecting: None,
        }
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = SharedString::from(text.to_string());
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        cx.notify();
    }

    pub fn text(&self) -> String {
        self.content.to_string()
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    pub fn move_cursor_to_end(&mut self, cx: &mut Context<Self>) {
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.content = SharedString::new_static("");
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        cx.notify();
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    fn emit_changed(&self, cx: &mut Context<Self>) {
        cx.emit(InputFieldEvent::Changed(self.content.to_string()));
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn select_range(&mut self, range: Range<usize>, reversed: bool, cx: &mut Context<Self>) {
        self.selected_range =
            range.start.min(self.content.len())..range.end.min(self.content.len());
        self.selection_reversed = reversed;
        self.marked_range = None;
        cx.notify();
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

    fn word_range_at_offset(&self, offset: usize) -> Range<usize> {
        word_range_in_text(self.content.as_ref(), offset)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
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

    fn left(&mut self, _: &FieldLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &FieldRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn word_left(&mut self, _: &FieldWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_word_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn word_right(&mut self, _: &FieldWordRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_word_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &FieldSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &FieldSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_word_left(
        &mut self,
        _: &FieldSelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn select_word_right(
        &mut self,
        _: &FieldSelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn select_all_action(&mut self, _: &FieldSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_all(cx);
    }

    fn home(&mut self, _: &FieldHome, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &FieldEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &FieldSelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &FieldSelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &FieldBackspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &FieldDelete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &FieldPaste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &FieldCopy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &FieldCut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
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

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let offset = self.index_for_mouse_position(event.position);
        if event.click_count >= 3 {
            self.drag_selecting = Some(InputDragSelectionMode::All);
            self.select_all(cx);
            return;
        }
        if event.click_count == 2 {
            let range = self.word_range_at_offset(offset);
            self.drag_selecting = Some(InputDragSelectionMode::Word(range.clone()));
            self.select_range(range, false, cx);
            return;
        }

        self.drag_selecting = Some(InputDragSelectionMode::Character);
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag_selecting = None;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.index_for_mouse_position(event.position);
        match self.drag_selecting.clone() {
            Some(InputDragSelectionMode::Character) => self.select_to(offset, cx),
            Some(InputDragSelectionMode::Word(anchor)) => {
                let current = self.word_range_at_offset(offset);
                let (range, reversed) = drag_selection_range(anchor, current);
                self.select_range(range, reversed, cx);
            }
            Some(InputDragSelectionMode::All) => self.select_all(cx),
            None => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputTokenClass {
    Whitespace,
    Word,
    Symbol,
}

fn input_token_class(ch: char) -> InputTokenClass {
    if ch.is_whitespace() {
        InputTokenClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        InputTokenClass::Word
    } else {
        InputTokenClass::Symbol
    }
}

fn previous_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = chars.partition_point(|(byte, _)| *byte < offset.min(text.len()));
    while index > 0 && input_token_class(chars[index - 1].1) == InputTokenClass::Whitespace {
        index -= 1;
    }
    if index == 0 {
        return 0;
    }

    let class = input_token_class(chars[index - 1].1);
    while index > 0 && input_token_class(chars[index - 1].1) == class {
        index -= 1;
    }
    chars.get(index).map_or(0, |(byte, _)| *byte)
}

fn next_word_boundary_in_text(text: &str, offset: usize) -> usize {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut index = chars.partition_point(|(byte, _)| *byte < offset.min(text.len()));
    while index < chars.len() && input_token_class(chars[index].1) == InputTokenClass::Whitespace {
        index += 1;
    }
    if index == chars.len() {
        return text.len();
    }

    let class = input_token_class(chars[index].1);
    while index < chars.len() && input_token_class(chars[index].1) == class {
        index += 1;
    }
    chars.get(index).map_or(text.len(), |(byte, _)| *byte)
}

fn char_index_containing_offset(text: &str, offset: usize) -> Option<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.is_empty() {
        return None;
    }

    let offset = offset.min(text.len());
    if offset == text.len() {
        return Some(chars.len() - 1);
    }

    chars.iter().enumerate().find_map(|(index, (start, _))| {
        let end = chars.get(index + 1).map_or(text.len(), |(byte, _)| *byte);
        (offset >= *start && offset < end).then_some(index)
    })
}

fn word_range_in_text(text: &str, offset: usize) -> Range<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let Some(local) = char_index_containing_offset(text, offset) else {
        return 0..0;
    };

    let class = input_token_class(chars[local].1);
    let mut start = local;
    while start > 0 && input_token_class(chars[start - 1].1) == class {
        start -= 1;
    }
    let mut end = local + 1;
    while end < chars.len() && input_token_class(chars[end].1) == class {
        end += 1;
    }

    let start_byte = chars[start].0;
    let end_byte = chars.get(end).map_or(text.len(), |(byte, _)| *byte);
    start_byte..end_byte
}

fn drag_selection_range(anchor: Range<usize>, current: Range<usize>) -> (Range<usize>, bool) {
    if current.start < anchor.start {
        (current.start..anchor.end.max(current.end), true)
    } else {
        (
            anchor.start.min(current.start)..current.end.max(anchor.end),
            false,
        )
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
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_layout = None;
        self.emit_changed(cx);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
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
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
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
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<InputField>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
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
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = px(INPUT_HEIGHT - 10.0).into();
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
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), rgb(COLOR_MUTED))
        } else {
            (content, rgb(COLOR_TEXT))
        };

        let run = TextRun {
            len: display_text.len(),
            font: window.text_style().font(),
            color: text_color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
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

        let font_size = px(INPUT_TEXT_SIZE);
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.0), bounds.bottom() - bounds.top()),
                    ),
                    rgb(COLOR_ACCENT),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgb(COLOR_SELECTION),
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
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
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("input line prepainted");
        let _ = line.paint(bounds.origin, window.line_height(), window, cx);
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
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
        let border = if focused {
            rgb(COLOR_ACCENT)
        } else {
            rgb(COLOR_BORDER)
        };

        div()
            .track_focus(&self.focus_handle)
            .key_context("InlineInput")
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all_action))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .relative()
            .w_full()
            .h(px(INPUT_HEIGHT))
            .px_3()
            .items_center()
            .rounded_sm()
            .bg(if focused {
                rgb(COLOR_SURFACE2)
            } else {
                rgb(COLOR_SURFACE1)
            })
            .border_1()
            .border_color(border)
            .hover(|style| style.bg(rgb(COLOR_SURFACE2)))
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
    use gpui::Keystroke;

    fn has_binding<A: gpui::Action + 'static>(keystroke: &str) -> bool {
        let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
        input_keybindings().iter().any(|binding| {
            binding.match_keystrokes(&typed) == Some(false) && binding.action().as_any().is::<A>()
        })
    }

    #[test]
    fn input_keybindings_include_overlay_navigation_actions() {
        let names = input_keybindings()
            .into_iter()
            .map(|binding| binding.action().name())
            .collect::<Vec<_>>();

        assert!(names.iter().any(|name| name.ends_with("FieldSubmit")));
        assert!(names.iter().any(|name| name.ends_with("FieldCancel")));
        assert!(names.iter().any(|name| name.ends_with("FieldNext")));
        assert!(names.iter().any(|name| name.ends_with("FieldPrevious")));
    }

    #[test]
    fn input_keybindings_include_standard_word_and_boundary_actions() {
        assert!(has_binding::<FieldWordLeft>("ctrl-left"));
        assert!(has_binding::<FieldWordRight>("ctrl-right"));
        assert!(has_binding::<FieldSelectWordLeft>("ctrl-shift-left"));
        assert!(has_binding::<FieldSelectWordRight>("ctrl-shift-right"));
        assert!(has_binding::<FieldSelectHome>("shift-home"));
        assert!(has_binding::<FieldSelectEnd>("shift-end"));
    }

    #[test]
    fn input_word_ranges_group_words_symbols_and_whitespace() {
        let text = "alpha beta::gamma";

        assert_eq!(word_range_in_text(text, 7), 6..10);
        assert_eq!(word_range_in_text(text, 10), 10..12);
        assert_eq!(word_range_in_text(text, 5), 5..6);
    }

    #[test]
    fn input_word_boundaries_are_utf8_safe() {
        let text = "one γamma two";

        assert_eq!(next_word_boundary_in_text(text, 0), 3);
        assert_eq!(next_word_boundary_in_text(text, 3), "one γamma".len());
        assert_eq!(
            previous_word_boundary_in_text(text, "one γamma".len()),
            "one ".len()
        );
    }

    #[test]
    fn input_word_drag_selection_extends_from_anchor_word() {
        let (selection, reversed) = drag_selection_range(6..10, 13..18);

        assert_eq!(selection, 6..18);
        assert!(!reversed);

        let (selection, reversed) = drag_selection_range(6..10, 0..5);
        assert_eq!(selection, 0..10);
        assert!(reversed);
    }
}
