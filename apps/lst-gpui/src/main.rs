use gpui::{
    actions, canvas, div, fill, point, prelude::*, px, rgb, size, App, Application, Bounds,
    ClipboardItem, Context, CursorStyle, ElementInputHandler, EntityInputHandler, FocusHandle,
    Focusable, IntoElement, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Render, ScrollHandle, ShapedLine, SharedString, TextRun, UTF16Selection, Window,
    WindowBounds, WindowOptions,
};
use rfd::FileDialog;
use ropey::Rope;
use std::{
    cell::RefCell, collections::HashMap, fs, ops::Range, path::PathBuf, process, rc::Rc,
    time::Instant,
};

const WINDOW_WIDTH: f32 = 1360.0;
const WINDOW_HEIGHT: f32 = 860.0;
const ROW_HEIGHT: f32 = 22.0;
const GUTTER_WIDTH: f32 = 76.0;
const CODE_FONT_SIZE: f32 = 13.0;
const CURSOR_WIDTH: f32 = 2.0;
const VIEWPORT_OVERSCAN_LINES: usize = 4;
const UNTITLED_PREFIX: &str = "untitled";
const CORPUS_PATH: &str = "benchmarks/paste-corpus-20k.rs";
const PREMADE_CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/paste-corpus-20k.rs"
));

actions!(
    lst_gpui,
    [
        NewTab,
        OpenFile,
        SaveFile,
        SaveFileAs,
        CloseActiveTab,
        NextTab,
        PrevTab,
        ToggleGutter,
        ReloadCorpus,
        CopySelection,
        CutSelection,
        PasteClipboard,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        MoveLineStart,
        MoveLineEnd,
        SelectLineStart,
        SelectLineEnd,
        Backspace,
        DeleteForward,
        InsertNewline,
        InsertTab,
        SelectAll,
        Quit,
    ]
);

#[derive(Clone, Debug)]
struct OperationStats {
    label: &'static str,
    bytes: usize,
    lines: usize,
    clipboard_read_ms: Option<f64>,
    apply_ms: f64,
}

impl OperationStats {
    fn summary(&self) -> String {
        match self.clipboard_read_ms {
            Some(read_ms) => format!(
                "{} | {} bytes | {} lines | clipboard_read_ms={read_ms:.3} | apply_ms={:.3}",
                self.label, self.bytes, self.lines, self.apply_ms
            ),
            None => format!(
                "{} | {} bytes | {} lines | apply_ms={:.3}",
                self.label, self.bytes, self.lines, self.apply_ms
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BenchAction {
    Replace,
    Append,
}

impl BenchAction {
    fn action_name(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Append => "append",
        }
    }

    fn operation_label(self) -> &'static str {
        match self {
            Self::Replace => "bench_replace",
            Self::Append => "bench_append",
        }
    }
}

#[derive(Clone, Debug)]
struct AutoBench {
    action: BenchAction,
    source: String,
    text: String,
}

#[derive(Clone, Debug, Default)]
struct LaunchArgs {
    files: Vec<PathBuf>,
    auto_bench: Option<AutoBench>,
}

#[derive(Clone)]
struct CachedShapedLine {
    text: SharedString,
    shaped: ShapedLine,
}

#[derive(Default)]
struct ViewportCache {
    code_lines: HashMap<usize, CachedShapedLine>,
    gutter_lines: HashMap<usize, CachedShapedLine>,
}

#[derive(Clone)]
struct PaintedRow {
    line_ix: usize,
    row_top: Pixels,
    line_start_char: usize,
    display_end_char: usize,
    logical_end_char: usize,
    code_line: Option<ShapedLine>,
    gutter_line: Option<ShapedLine>,
}

struct ViewportPaintState {
    rows: Vec<PaintedRow>,
}

#[derive(Default)]
struct ViewportGeometry {
    bounds: Option<Bounds<Pixels>>,
    rows: Vec<PaintedRow>,
}

struct EditorTab {
    name_hint: String,
    path: Option<PathBuf>,
    buffer: Rope,
    modified: bool,
    scroll: ScrollHandle,
    cache: Rc<RefCell<ViewportCache>>,
    geometry: Rc<RefCell<ViewportGeometry>>,
    selection: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    preferred_column: Option<usize>,
}

impl EditorTab {
    fn empty(name_hint: String) -> Self {
        Self::from_text(name_hint, None, "")
    }

    fn from_path(path: PathBuf, text: &str) -> Self {
        let name_hint = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(UNTITLED_PREFIX)
            .to_string();
        Self::from_text(name_hint, Some(path), text)
    }

    fn from_text(name_hint: String, path: Option<PathBuf>, text: &str) -> Self {
        Self {
            name_hint,
            path,
            buffer: Rope::from_str(text),
            modified: false,
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(ViewportCache::default())),
            geometry: Rc::new(RefCell::new(ViewportGeometry::default())),
            selection: 0..0,
            selection_reversed: false,
            marked_range: None,
            preferred_column: None,
        }
    }

    fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| self.name_hint.clone())
    }

    fn len_chars(&self) -> usize {
        self.buffer.len_chars()
    }

    fn line_count(&self) -> usize {
        self.buffer.len_lines().max(1)
    }

    fn cursor_char(&self) -> usize {
        if self.selection_reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    fn selected_range(&self) -> Range<usize> {
        self.selection.clone()
    }

    fn has_selection(&self) -> bool {
        self.selection.start != self.selection.end
    }

    fn move_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        self.selection = offset..offset;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    fn select_to(&mut self, offset: usize) {
        let offset = offset.min(self.len_chars());
        if self.selection_reversed {
            self.selection.start = offset;
        } else {
            self.selection.end = offset;
        }
        if self.selection.end < self.selection.start {
            self.selection_reversed = !self.selection_reversed;
            self.selection = self.selection.end..self.selection.start;
        }
        self.marked_range = None;
    }

    fn select_all(&mut self) {
        let end = self.len_chars();
        self.selection = 0..end;
        self.selection_reversed = false;
        self.marked_range = None;
    }

    fn invalidate_visual_state(&mut self) {
        *self.cache.borrow_mut() = ViewportCache::default();
        *self.geometry.borrow_mut() = ViewportGeometry::default();
    }

    fn buffer_text(&self) -> String {
        self.buffer.to_string()
    }

    fn selected_text(&self) -> Option<String> {
        if self.has_selection() {
            Some(self.buffer.slice(self.selection.clone()).to_string())
        } else {
            None
        }
    }

    fn display_line_char_len(&self, line_ix: usize) -> usize {
        line_display_char_len(&self.buffer, line_ix)
    }

    fn replace_char_range(&mut self, mut range: Range<usize>, new_text: &str) -> usize {
        range.start = range.start.min(self.len_chars());
        range.end = range.end.min(self.len_chars());
        if range.start > range.end {
            range = range.end..range.start;
        }

        if range.start != range.end {
            self.buffer.remove(range.clone());
        }
        if !new_text.is_empty() {
            self.buffer.insert(range.start, new_text);
        }

        let new_cursor = range.start + new_text.chars().count();
        self.selection = new_cursor..new_cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.modified = true;
        self.preferred_column = None;
        self.invalidate_visual_state();
        new_cursor
    }
}

struct LstGpuiApp {
    focus_handle: FocusHandle,
    tabs: Vec<EditorTab>,
    active: usize,
    next_untitled_id: usize,
    show_gutter: bool,
    drag_selecting: bool,
    status: String,
    last_operation: OperationStats,
}

impl LstGpuiApp {
    fn new(cx: &mut Context<Self>, launch: LaunchArgs) -> Self {
        let mut tabs = Vec::new();
        let mut status = "Ready.".to_string();

        if launch.auto_bench.is_some() {
            tabs.push(EditorTab::from_text(
                PathBuf::from(CORPUS_PATH)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("paste-corpus-20k.rs")
                    .to_string(),
                Some(PathBuf::from(CORPUS_PATH)),
                PREMADE_CORPUS,
            ));
            status = format!("Benchmark mode. Loaded {CORPUS_PATH} at startup.");
        } else if launch.files.is_empty() {
            tabs.push(EditorTab::empty(format!("{UNTITLED_PREFIX}-1")));
        } else {
            for path in launch.files {
                match fs::read_to_string(&path) {
                    Ok(text) => tabs.push(EditorTab::from_path(path, &text)),
                    Err(err) => {
                        status = format!("Failed to open {}: {err}", path.display());
                    }
                }
            }

            if tabs.is_empty() {
                tabs.push(EditorTab::empty(format!("{UNTITLED_PREFIX}-1")));
            }
        }

        let active = 0;
        let last_operation = OperationStats {
            label: "startup",
            bytes: tabs[active].buffer.len_bytes(),
            lines: tabs[active].buffer.len_lines(),
            clipboard_read_ms: None,
            apply_ms: 0.0,
        };

        eprintln!("lst_gpui {}", last_operation.summary());

        Self {
            focus_handle: cx.focus_handle(),
            tabs,
            active,
            next_untitled_id: 2,
            show_gutter: true,
            drag_selecting: false,
            status,
            last_operation,
        }
    }

    fn button(
        label: &'static str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> impl IntoElement {
        div()
            .id(label)
            .flex_none()
            .cursor_pointer()
            .px_3()
            .py_1()
            .bg(rgb(0x1F6F78))
            .text_color(rgb(0xFFF9F0))
            .active(|style| style.opacity(0.85))
            .child(label.to_string())
            .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
    }

    fn tab_button(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = &self.tabs[ix];
        let active = ix == self.active;
        let label = if tab.modified {
            format!("{}*", tab.display_name())
        } else {
            tab.display_name()
        };

        div()
            .id(("tab", ix))
            .cursor_pointer()
            .px_3()
            .py_2()
            .bg(if active { rgb(0x1C6B74) } else { rgb(0xD9D0C3) })
            .text_color(if active { rgb(0xFFF9F0) } else { rgb(0x2B211A) })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active = ix;
                this.active_tab_mut().preferred_column = None;
                this.status = format!("Switched to {}.", this.active_tab().display_name());
                this.reveal_active_cursor();
                cx.notify();
            }))
    }

    fn active_tab(&self) -> &EditorTab {
        &self.tabs[self.active]
    }

    fn active_tab_mut(&mut self) -> &mut EditorTab {
        &mut self.tabs[self.active]
    }

    fn new_empty_tab(&mut self) -> EditorTab {
        let name = format!("{UNTITLED_PREFIX}-{}", self.next_untitled_id);
        self.next_untitled_id += 1;
        EditorTab::empty(name)
    }

    fn active_cursor_line_col(&self) -> (usize, usize) {
        char_to_line_col(&self.active_tab().buffer, self.active_tab().cursor_char())
    }

    fn metrics_line(&self) -> String {
        let tab = self.active_tab();
        let (line, column) = self.active_cursor_line_col();
        let path = tab
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| tab.display_name());
        format!(
            "{} | {} bytes | {} lines | line {} col {} | gutter={} | last={}",
            path,
            tab.buffer.len_bytes(),
            tab.buffer.len_lines(),
            line + 1,
            column + 1,
            if self.show_gutter { "on" } else { "off" },
            self.last_operation.summary()
        )
    }

    fn shortcut_line(&self) -> &'static str {
        "Ctrl-N new | Ctrl-O open | Ctrl-S save | Ctrl-Shift-S save as | Ctrl-W close | Ctrl-C/X/V clipboard | Ctrl-G gutter | Ctrl-Q quit"
    }

    fn record_operation(
        &mut self,
        label: &'static str,
        clipboard_read_ms: Option<f64>,
        apply_ms: f64,
    ) {
        let tab = self.active_tab();
        self.last_operation = OperationStats {
            label,
            bytes: tab.buffer.len_bytes(),
            lines: tab.buffer.len_lines(),
            clipboard_read_ms,
            apply_ms,
        };
        self.status = self.last_operation.summary();
        eprintln!("lst_gpui {}", self.last_operation.summary());
    }

    fn replace_active_text(
        &mut self,
        label: &'static str,
        text: &str,
        clipboard_read_ms: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let path = tab.path.clone();
            let name_hint = tab.display_name();
            *tab = EditorTab::from_text(name_hint, path, text);
        }
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn append_active_text(
        &mut self,
        label: &'static str,
        text: &str,
        clipboard_read_ms: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let end = tab.len_chars();
            tab.replace_char_range(end..end, text);
            tab.modified = false;
        }
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn insert_text_at_selection(
        &mut self,
        label: &'static str,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let range = tab
                .marked_range
                .clone()
                .unwrap_or_else(|| tab.selected_range());
            tab.replace_char_range(range, text);
        }
        self.record_operation(label, None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn delete_selected_or_previous(&mut self, cx: &mut Context<Self>) {
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let range = if tab.has_selection() {
                tab.selected_range()
            } else {
                let cursor = tab.cursor_char();
                if cursor == 0 {
                    return;
                }
                cursor - 1..cursor
            };
            tab.replace_char_range(range, "");
        }
        self.record_operation("backspace", None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn delete_selected_or_next(&mut self, cx: &mut Context<Self>) {
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let range = if tab.has_selection() {
                tab.selected_range()
            } else {
                let cursor = tab.cursor_char();
                if cursor >= tab.len_chars() {
                    return;
                }
                cursor..cursor + 1
            };
            tab.replace_char_range(range, "");
        }
        self.record_operation("delete", None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn insert_newline(&mut self, cx: &mut Context<Self>) {
        let indent = {
            let tab = self.active_tab();
            let (line, _) = char_to_line_col(&tab.buffer, tab.cursor_char());
            line_indent_prefix(&tab.buffer, line)
        };
        self.insert_text_at_selection("newline", &format!("\n{indent}"), cx);
    }

    fn insert_tab(&mut self, cx: &mut Context<Self>) {
        self.insert_text_at_selection("tab", "    ", cx);
    }

    fn move_horizontal(&mut self, delta: isize, select: bool, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        let target = if delta.is_negative() {
            tab.cursor_char().saturating_sub(delta.unsigned_abs())
        } else {
            (tab.cursor_char() + delta as usize).min(tab.len_chars())
        };
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        self.reveal_active_cursor();
        cx.notify();
    }

    fn move_vertical(&mut self, delta: isize, select: bool, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        let cursor = tab.cursor_char();
        let (line, column) = char_to_line_col(&tab.buffer, cursor);
        let preferred = tab.preferred_column.unwrap_or(column);
        let target_line = if delta.is_negative() {
            line.saturating_sub(delta.unsigned_abs())
        } else {
            (line + delta as usize).min(tab.line_count().saturating_sub(1))
        };
        let target_column = preferred.min(tab.display_line_char_len(target_line));
        let target = tab.buffer.line_to_char(target_line) + target_column;
        tab.preferred_column = Some(preferred);
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        self.reveal_active_cursor();
        cx.notify();
    }

    fn move_line_boundary(&mut self, to_end: bool, select: bool, cx: &mut Context<Self>) {
        let tab = self.active_tab_mut();
        let cursor = tab.cursor_char();
        let (line, _) = char_to_line_col(&tab.buffer, cursor);
        let target = if to_end {
            tab.buffer.line_to_char(line) + tab.display_line_char_len(line)
        } else {
            tab.buffer.line_to_char(line)
        };
        tab.preferred_column = None;
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        self.reveal_active_cursor();
        cx.notify();
    }

    fn copy_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.active_tab().selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        cx.write_to_primary(ClipboardItem::new_string(text));
        self.status = "Copied selection.".to_string();
        cx.notify();
    }

    fn cut_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.active_tab().selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
        cx.write_to_primary(ClipboardItem::new_string(text));
        let apply_started = Instant::now();
        let range = self.active_tab().selected_range();
        self.active_tab_mut().replace_char_range(range, "");
        self.record_operation("cut", None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let read_started = Instant::now();
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.status = "Clipboard does not currently contain plain text.".to_string();
            cx.notify();
            return;
        };

        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let range = tab
                .marked_range
                .clone()
                .unwrap_or_else(|| tab.selected_range());
            tab.replace_char_range(range, &text);
        }
        self.record_operation(
            "paste_clipboard",
            Some(elapsed_ms(read_started)),
            elapsed_ms(apply_started),
        );
        self.reveal_active_cursor();
        cx.notify();
    }

    fn load_corpus(&mut self, cx: &mut Context<Self>) {
        self.replace_active_text("load_corpus", PREMADE_CORPUS, None, cx);
        self.active_tab_mut().modified = false;
    }

    fn open_files(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = FileDialog::new().pick_files() else {
            return;
        };

        let start_len = self.tabs.len();
        for path in paths {
            match fs::read_to_string(&path) {
                Ok(text) => self.tabs.push(EditorTab::from_path(path, &text)),
                Err(err) => {
                    self.status = format!("Failed to open {}: {err}", path.display());
                }
            }
        }

        if self.tabs.len() > start_len {
            self.active = self.tabs.len() - 1;
            self.status = format!("Opened {} tab(s).", self.tabs.len() - start_len);
        }
        self.reveal_active_cursor();
        cx.notify();
    }

    fn save_active(&mut self, cx: &mut Context<Self>) {
        if self.active_tab().path.is_none() {
            self.save_active_as(cx);
            return;
        }

        let path = self.active_tab().path.clone().expect("checked above");
        let body = self.active_tab().buffer_text();
        match fs::write(&path, body) {
            Ok(()) => {
                self.active_tab_mut().modified = false;
                self.status = format!("Saved {}.", path.display());
            }
            Err(err) => {
                self.status = format!("Failed to save {}: {err}", path.display());
            }
        }
        cx.notify();
    }

    fn save_active_as(&mut self, cx: &mut Context<Self>) {
        let suggested = self.active_tab().display_name();
        let Some(path) = FileDialog::new().set_file_name(&suggested).save_file() else {
            return;
        };
        let body = self.active_tab().buffer_text();
        match fs::write(&path, body) {
            Ok(()) => {
                let tab = self.active_tab_mut();
                tab.path = Some(path.clone());
                tab.modified = false;
                self.status = format!("Saved {}.", path.display());
            }
            Err(err) => {
                self.status = format!("Failed to save {}: {err}", path.display());
            }
        }
        cx.notify();
    }

    fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if self.tabs.len() == 1 {
            self.tabs[0] = self.new_empty_tab();
            self.active = 0;
            self.status = "Closed tab.".to_string();
            cx.notify();
            return;
        }

        self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        self.status = "Closed tab.".to_string();
        self.reveal_active_cursor();
        cx.notify();
    }

    fn reveal_active_cursor(&self) {
        let tab = self.active_tab();
        let viewport_bounds = tab.scroll.bounds();
        if viewport_bounds.size.height <= px(0.) {
            return;
        }

        let cursor_line = tab.buffer.char_to_line(tab.cursor_char());
        let caret_top = px((cursor_line as f32) * ROW_HEIGHT);
        let caret_bottom = caret_top + px(ROW_HEIGHT);
        let scroll_top = {
            let offset_y = -tab.scroll.offset().y;
            if offset_y > px(0.) {
                offset_y
            } else {
                px(0.)
            }
        };
        let margin = px(ROW_HEIGHT * 2.0);
        let viewport_height = viewport_bounds.size.height;

        let target = if caret_top < scroll_top + margin {
            Some((caret_top - margin).max(px(0.0)))
        } else if caret_bottom > scroll_top + viewport_height - margin {
            Some((caret_bottom + margin - viewport_height).max(px(0.0)))
        } else {
            None
        };

        if let Some(target) = target {
            tab.scroll.set_offset(point(px(0.0), -target));
        }
    }

    fn sync_primary_selection(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.active_tab().selected_text() {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
    }

    fn active_char_index_for_point(&self, point: Point<Pixels>) -> usize {
        let geometry = self.active_tab().geometry.borrow();
        let Some(bounds) = geometry.bounds else {
            return self.active_tab().cursor_char();
        };
        let code_origin_x = bounds.left()
            + if self.show_gutter {
                px(GUTTER_WIDTH)
            } else {
                px(12.0)
            };

        let row = if geometry.rows.is_empty() {
            return 0;
        } else if point.y <= geometry.rows[0].row_top {
            &geometry.rows[0]
        } else {
            geometry
                .rows
                .iter()
                .find(|row| point.y >= row.row_top && point.y < row.row_top + px(ROW_HEIGHT))
                .unwrap_or_else(|| geometry.rows.last().expect("checked above"))
        };

        let x = if point.x > code_origin_x {
            point.x - code_origin_x
        } else {
            px(0.0)
        };

        if let Some(code_line) = row.code_line.as_ref() {
            let byte_index = code_line.closest_index_for_x(x);
            let line_char = byte_index_to_char(code_line.text.as_ref(), byte_index);
            (row.line_start_char + line_char).min(row.display_end_char)
        } else {
            row.line_start_char
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.drag_selecting = true;
        let index = self.active_char_index_for_point(event.position);
        if event.modifiers.shift {
            self.active_tab_mut().select_to(index);
        } else {
            let tab = self.active_tab_mut();
            tab.move_to(index);
            tab.preferred_column = None;
        }
        self.reveal_active_cursor();
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.drag_selecting {
            return;
        }
        let index = self.active_char_index_for_point(event.position);
        self.active_tab_mut().select_to(index);
        self.reveal_active_cursor();
        cx.notify();
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.drag_selecting = false;
        self.sync_primary_selection(cx);
        cx.notify();
    }

    fn run_auto_bench(
        &mut self,
        bench: AutoBench,
        window: &mut Window,
        cx: &mut Context<Self>,
        startup_to_action_ms: f64,
        process_started: Instant,
    ) {
        let action_started = Instant::now();

        match bench.action {
            BenchAction::Replace => {
                self.replace_active_text(bench.action.operation_label(), &bench.text, None, cx)
            }
            BenchAction::Append => {
                self.append_active_text(bench.action.operation_label(), &bench.text, None, cx)
            }
        }

        let operation = self.last_operation.clone();
        let action = bench.action;
        let source = bench.source;

        window.on_next_frame(move |_window, cx| {
            eprintln!(
                "lst_gpui bench action={} source={} startup_to_action_ms={startup_to_action_ms:.3} action_to_next_frame_ms={:.3} total_wall_ms={:.3} final_bytes={} final_lines={} apply_ms={:.3}",
                action.action_name(),
                source,
                elapsed_ms(action_started),
                elapsed_ms(process_started),
                operation.bytes,
                operation.lines,
                operation.apply_ms,
            );
            cx.quit();
        });
    }

    fn handle_quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn handle_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        let tab = self.new_empty_tab();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.status = "Created a new tab.".to_string();
        cx.notify();
    }

    fn handle_open_file(&mut self, _: &OpenFile, _: &mut Window, cx: &mut Context<Self>) {
        self.open_files(cx);
    }

    fn handle_save_file(&mut self, _: &SaveFile, _: &mut Window, cx: &mut Context<Self>) {
        self.save_active(cx);
    }

    fn handle_save_file_as(&mut self, _: &SaveFileAs, _: &mut Window, cx: &mut Context<Self>) {
        self.save_active_as(cx);
    }

    fn handle_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_active_tab(cx);
    }

    fn handle_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            self.active = (self.active + 1) % self.tabs.len();
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_prev_tab(&mut self, _: &PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            self.active = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_toggle_gutter(&mut self, _: &ToggleGutter, _: &mut Window, cx: &mut Context<Self>) {
        self.show_gutter = !self.show_gutter;
        self.status = if self.show_gutter {
            "Line gutter enabled.".to_string()
        } else {
            "Line gutter disabled.".to_string()
        };
        cx.notify();
    }

    fn handle_reload_corpus(&mut self, _: &ReloadCorpus, _: &mut Window, cx: &mut Context<Self>) {
        self.load_corpus(cx);
    }

    fn handle_copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection_to_clipboard(cx);
    }

    fn handle_cut_selection(&mut self, _: &CutSelection, _: &mut Window, cx: &mut Context<Self>) {
        self.cut_selection_to_clipboard(cx);
    }

    fn handle_paste_clipboard(
        &mut self,
        _: &PasteClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste_from_clipboard(cx);
    }

    fn handle_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab().has_selection() {
            self.move_horizontal(-1, false, cx);
        } else {
            let start = self.active_tab().selection.start;
            self.active_tab_mut().move_to(start);
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab().has_selection() {
            self.move_horizontal(1, false, cx);
        } else {
            let end = self.active_tab().selection.end;
            self.active_tab_mut().move_to(end);
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, cx);
    }

    fn handle_move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, cx);
    }

    fn handle_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(-1, true, cx);
    }

    fn handle_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(1, true, cx);
    }

    fn handle_select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, cx);
    }

    fn handle_select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, cx);
    }

    fn handle_move_line_start(
        &mut self,
        _: &MoveLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_line_boundary(false, false, cx);
    }

    fn handle_move_line_end(&mut self, _: &MoveLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_line_boundary(true, false, cx);
    }

    fn handle_select_line_start(
        &mut self,
        _: &SelectLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_line_boundary(false, true, cx);
    }

    fn handle_select_line_end(
        &mut self,
        _: &SelectLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_line_boundary(true, true, cx);
    }

    fn handle_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_or_previous(cx);
    }

    fn handle_delete_forward(&mut self, _: &DeleteForward, _: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_or_next(cx);
    }

    fn handle_insert_newline(&mut self, _: &InsertNewline, _: &mut Window, cx: &mut Context<Self>) {
        self.insert_newline(cx);
    }

    fn handle_insert_tab(&mut self, _: &InsertTab, _: &mut Window, cx: &mut Context<Self>) {
        self.insert_tab(cx);
    }

    fn handle_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.active_tab_mut().select_all();
        self.sync_primary_selection(cx);
        cx.notify();
    }
}

impl Focusable for LstGpuiApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
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
        let range = utf16_range_to_char_range(&tab.buffer, &range_utf16);
        *actual_range = Some(char_range_to_utf16_range(&tab.buffer, &range));
        Some(tab.buffer.slice(range).to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let tab = self.active_tab();
        Some(UTF16Selection {
            range: char_range_to_utf16_range(&tab.buffer, &tab.selection),
            reversed: tab.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let tab = self.active_tab();
        tab.marked_range
            .as_ref()
            .map(|range| char_range_to_utf16_range(&tab.buffer, range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.active_tab_mut().marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let range = range_utf16
                .as_ref()
                .map(|range| utf16_range_to_char_range(&tab.buffer, range))
                .or_else(|| tab.marked_range.clone())
                .unwrap_or_else(|| tab.selected_range());
            tab.replace_char_range(range, text);
        }
        self.record_operation("text_input", None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
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
        let apply_started = Instant::now();
        {
            let tab = self.active_tab_mut();
            let range = range_utf16
                .as_ref()
                .map(|range| utf16_range_to_char_range(&tab.buffer, range))
                .or_else(|| tab.marked_range.clone())
                .unwrap_or_else(|| tab.selected_range());

            let inserted_start = range.start;
            tab.replace_char_range(range, new_text);
            if !new_text.is_empty() {
                let marked_end = inserted_start + new_text.chars().count();
                tab.marked_range = Some(inserted_start..marked_end);
            } else {
                tab.marked_range = None;
            }

            tab.selection = new_selected_range_utf16
                .as_ref()
                .map(|range| utf16_range_to_char_range(&tab.buffer, range))
                .map(|range| inserted_start + range.start..inserted_start + range.end)
                .unwrap_or_else(|| {
                    let cursor = inserted_start + new_text.chars().count();
                    cursor..cursor
                });
            tab.selection_reversed = false;
        }
        self.record_operation("ime_text_input", None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let tab = self.active_tab();
        let geometry = tab.geometry.borrow();
        let range = utf16_range_to_char_range(&tab.buffer, &range_utf16);
        let row = geometry.rows.iter().find(|row| {
            range.start >= row.line_start_char && range.start <= row.logical_end_char
        })?;
        let code_origin_x = element_bounds.left()
            + if self.show_gutter {
                px(GUTTER_WIDTH)
            } else {
                px(12.0)
            };
        let start_x =
            code_origin_x + x_for_global_char(row, range.start).unwrap_or_else(|| px(0.0));
        let end_x = code_origin_x
            + x_for_global_char(row, range.end.min(row.display_end_char))
                .unwrap_or_else(|| px(0.0));
        Some(Bounds::from_corners(
            point(start_x, row.row_top),
            point(
                end_x.max(start_x + px(CURSOR_WIDTH)),
                row.row_top + px(ROW_HEIGHT),
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
        Some(char_to_utf16(&self.active_tab().buffer, char_index))
    }
}

impl Render for LstGpuiApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_tab = self.active_tab();
        let total_content_height = buffer_content_height(active_tab.line_count());
        let buffer = active_tab.buffer.clone();
        let selection = active_tab.selection.clone();
        let cursor_char = active_tab.cursor_char();
        let show_gutter = self.show_gutter;
        let viewport_scroll = active_tab.scroll.clone();
        let viewport_cache = active_tab.cache.clone();
        let viewport_geometry = active_tab.geometry.clone();
        let focus_handle = self.focus_handle.clone();
        let entity = cx.entity();

        div()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::handle_new_tab))
            .on_action(cx.listener(Self::handle_open_file))
            .on_action(cx.listener(Self::handle_save_file))
            .on_action(cx.listener(Self::handle_save_file_as))
            .on_action(cx.listener(Self::handle_close_active_tab))
            .on_action(cx.listener(Self::handle_next_tab))
            .on_action(cx.listener(Self::handle_prev_tab))
            .on_action(cx.listener(Self::handle_toggle_gutter))
            .on_action(cx.listener(Self::handle_reload_corpus))
            .on_action(cx.listener(Self::handle_copy_selection))
            .on_action(cx.listener(Self::handle_cut_selection))
            .on_action(cx.listener(Self::handle_paste_clipboard))
            .on_action(cx.listener(Self::handle_move_left))
            .on_action(cx.listener(Self::handle_move_right))
            .on_action(cx.listener(Self::handle_move_up))
            .on_action(cx.listener(Self::handle_move_down))
            .on_action(cx.listener(Self::handle_select_left))
            .on_action(cx.listener(Self::handle_select_right))
            .on_action(cx.listener(Self::handle_select_up))
            .on_action(cx.listener(Self::handle_select_down))
            .on_action(cx.listener(Self::handle_move_line_start))
            .on_action(cx.listener(Self::handle_move_line_end))
            .on_action(cx.listener(Self::handle_select_line_start))
            .on_action(cx.listener(Self::handle_select_line_end))
            .on_action(cx.listener(Self::handle_backspace))
            .on_action(cx.listener(Self::handle_delete_forward))
            .on_action(cx.listener(Self::handle_insert_newline))
            .on_action(cx.listener(Self::handle_insert_tab))
            .on_action(cx.listener(Self::handle_select_all))
            .on_action(cx.listener(Self::handle_quit))
            .size_full()
            .bg(rgb(0xEFE6D7))
            .text_color(rgb(0x231A12))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .px_4()
                    .py_3()
                    .bg(rgb(0xF7F1E6))
                    .border_b_1()
                    .border_color(rgb(0xC8BBA7))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_grow()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("lst GPUI Rewrite"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x685C50))
                                    .child(
                                        "Custom Ropey editor core, fast custom-painted viewport, tabs, clipboard, and file I/O.",
                                    ),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x8A3B12))
                                    .child(self.metrics_line()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x685C50))
                                    .child(self.shortcut_line()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x685C50))
                                    .child(self.status.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .gap_2()
                            .child(Self::button("New", cx, |this, cx| {
                                let tab = this.new_empty_tab();
                                this.tabs.push(tab);
                                this.active = this.tabs.len() - 1;
                                this.status = "Created a new tab.".to_string();
                                cx.notify();
                            }))
                            .child(Self::button("Open", cx, |this, cx| {
                                this.open_files(cx)
                            }))
                            .child(Self::button("Save", cx, |this, cx| {
                                this.save_active(cx)
                            }))
                            .child(Self::button("Save As", cx, |this, cx| {
                                this.save_active_as(cx)
                            }))
                            .child(Self::button("Load 20k corpus", cx, |this, cx| {
                                this.load_corpus(cx)
                            }))
                            .child(Self::button("Toggle gutter", cx, |this, cx| {
                                this.show_gutter = !this.show_gutter;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .bg(rgb(0xE4D8C7))
                    .children((0..self.tabs.len()).map(|ix| self.tab_button(ix, cx))),
            )
            .child(
                div().flex_grow().p_3().child(
                    div()
                        .id("buffer-viewport")
                        .relative()
                        .h_full()
                        .w_full()
                        .border_1()
                        .border_color(rgb(0xC8BBA7))
                        .bg(rgb(0xFFFDF8))
                        .font_family(".ZedMono")
                        .text_size(px(CODE_FONT_SIZE))
                        .line_height(px(ROW_HEIGHT))
                        .child(
                            div()
                                .id("buffer-scroll")
                                .overflow_y_scroll()
                                .absolute()
                                .left_0()
                                .top_0()
                                .size_full()
                                .track_scroll(&viewport_scroll)
                                .child(div().h(total_content_height).w_full()),
                        )
                        .child(
                            div()
                                .id("buffer-overlay")
                                .absolute()
                                .left_0()
                                .top_0()
                                .size_full()
                                .cursor(CursorStyle::IBeam)
                                .block_mouse_except_scroll()
                                .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(Self::on_mouse_up),
                                )
                                .on_mouse_move(cx.listener(Self::on_mouse_move))
                                .child(
                                    canvas(
                                        move |bounds, window, _cx| {
                                            prepare_viewport_paint_state(
                                                &buffer,
                                                show_gutter,
                                                &viewport_scroll,
                                                &viewport_cache,
                                                &viewport_geometry,
                                                bounds,
                                                window,
                                            )
                                        },
                                        move |bounds, paint_state, window, cx| {
                                            window.handle_input(
                                                &focus_handle,
                                                ElementInputHandler::new(bounds, entity.clone()),
                                                cx,
                                            );
                                            paint_viewport(
                                                bounds,
                                                show_gutter,
                                                selection.clone(),
                                                cursor_char,
                                                focus_handle.is_focused(window),
                                                paint_state,
                                                window,
                                                cx,
                                            );
                                        },
                                    )
                                    .size_full(),
                                ),
                        ),
                ),
            )
    }
}

fn buffer_content_height(line_count: usize) -> Pixels {
    px((line_count.max(1) as f32) * ROW_HEIGHT)
}

fn line_display_text(buffer: &Rope, line_ix: usize) -> SharedString {
    let mut line = buffer.line(line_ix).to_string();
    while matches!(line.as_bytes().last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    SharedString::from(line)
}

fn line_display_char_len(buffer: &Rope, line_ix: usize) -> usize {
    line_display_text(buffer, line_ix).chars().count()
}

fn visible_line_range(
    scroll_top: Pixels,
    viewport_height: Pixels,
    total_lines: usize,
) -> Range<usize> {
    let start =
        ((scroll_top / px(ROW_HEIGHT)).floor() as usize).saturating_sub(VIEWPORT_OVERSCAN_LINES);
    let end = (((scroll_top + viewport_height) / px(ROW_HEIGHT)).ceil() as usize)
        .saturating_add(VIEWPORT_OVERSCAN_LINES)
        .min(total_lines.max(1));
    start..end.max(start.saturating_add(1))
}

fn shape_cached_line(
    cache: &mut HashMap<usize, CachedShapedLine>,
    line_ix: usize,
    text: SharedString,
    base_run: &TextRun,
    font_size: Pixels,
    window: &mut Window,
) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(&line_ix) {
        if cached.text == text {
            return Some(cached.shaped.clone());
        }
    }

    let shaped = window.text_system().shape_line(
        text.clone(),
        font_size,
        &[TextRun {
            len: text.len(),
            ..base_run.clone()
        }],
        None,
    );

    cache.insert(
        line_ix,
        CachedShapedLine {
            text,
            shaped: shaped.clone(),
        },
    );
    Some(shaped)
}

fn prepare_viewport_paint_state(
    buffer: &Rope,
    show_gutter: bool,
    viewport_scroll: &ScrollHandle,
    viewport_cache: &Rc<RefCell<ViewportCache>>,
    viewport_geometry: &Rc<RefCell<ViewportGeometry>>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) -> ViewportPaintState {
    let viewport_height = if bounds.size.height > px(0.0) {
        bounds.size.height
    } else {
        px(WINDOW_HEIGHT)
    };
    let scroll_top = {
        let offset_y = -viewport_scroll.offset().y;
        if offset_y > px(0.0) {
            offset_y
        } else {
            px(0.0)
        }
    };
    let visible = visible_line_range(scroll_top, viewport_height, buffer.len_lines());
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let code_run = TextRun {
        len: 0,
        font: style.font(),
        color: rgb(0x201A16).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let gutter_run = TextRun {
        len: 0,
        font: style.font(),
        color: rgb(0x8D7F70).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let mut rows = Vec::with_capacity(visible.len());
    let mut cache = viewport_cache.borrow_mut();
    cache
        .code_lines
        .retain(|line_ix, _| visible.contains(line_ix));
    cache
        .gutter_lines
        .retain(|line_ix, _| show_gutter && visible.contains(line_ix));

    for line_ix in visible {
        let row_top = bounds.top() + px((line_ix as f32) * ROW_HEIGHT) - scroll_top;
        let line_start_char = buffer.line_to_char(line_ix);
        let display_text = line_display_text(buffer, line_ix);
        let display_end_char = line_start_char + display_text.chars().count();
        let logical_end_char = if line_ix + 1 < buffer.len_lines() {
            buffer.line_to_char(line_ix + 1)
        } else {
            buffer.len_chars()
        };
        let code_line = shape_cached_line(
            &mut cache.code_lines,
            line_ix,
            display_text,
            &code_run,
            font_size,
            window,
        );
        let gutter_line = if show_gutter {
            shape_cached_line(
                &mut cache.gutter_lines,
                line_ix,
                SharedString::from(format!("{:>6}", line_ix + 1)),
                &gutter_run,
                font_size,
                window,
            )
        } else {
            None
        };

        rows.push(PaintedRow {
            line_ix,
            row_top,
            line_start_char,
            display_end_char,
            logical_end_char,
            code_line,
            gutter_line,
        });
    }

    *viewport_geometry.borrow_mut() = ViewportGeometry {
        bounds: Some(bounds),
        rows: rows.clone(),
    };

    ViewportPaintState { rows }
}

fn paint_viewport(
    bounds: Bounds<Pixels>,
    show_gutter: bool,
    selection: Range<usize>,
    cursor_char: usize,
    focused: bool,
    paint_state: ViewportPaintState,
    window: &mut Window,
    cx: &mut App,
) {
    let line_height = window.line_height();
    let gutter_origin_x = bounds.left() + px(8.0);
    let gutter_width = px(GUTTER_WIDTH - 16.0);
    let code_origin_x = bounds.left()
        + if show_gutter {
            px(GUTTER_WIDTH)
        } else {
            px(12.0)
        };

    for row in paint_state.rows {
        let row_bounds = Bounds::new(
            point(bounds.left(), row.row_top),
            size(bounds.size.width, px(ROW_HEIGHT)),
        );
        let row_background = if row.line_ix % 2 == 0 {
            rgb(0xFFFDF8)
        } else {
            rgb(0xF6EFE4)
        };
        window.paint_quad(fill(row_bounds, row_background));

        if show_gutter {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left(), row.row_top),
                    size(px(GUTTER_WIDTH), px(ROW_HEIGHT)),
                ),
                rgb(0xF1E7D8),
            ));
        }

        if selection.start != selection.end
            && selection.end > row.line_start_char
            && selection.start < row.logical_end_char
        {
            let start = selection
                .start
                .max(row.line_start_char)
                .min(row.display_end_char);
            let end = selection.end.min(row.display_end_char);
            if end > start {
                let start_x =
                    code_origin_x + x_for_global_char(&row, start).unwrap_or_else(|| px(0.0));
                let end_x = code_origin_x + x_for_global_char(&row, end).unwrap_or_else(|| px(0.0));
                window.paint_quad(fill(
                    Bounds::from_corners(
                        point(start_x, row.row_top),
                        point(
                            end_x.max(start_x + px(CURSOR_WIDTH)),
                            row.row_top + px(ROW_HEIGHT),
                        ),
                    ),
                    rgb(0xBFD7EA),
                ));
            }
        }

        if let Some(gutter_line) = row.gutter_line.as_ref() {
            let gutter_x = gutter_origin_x + (gutter_width - gutter_line.width);
            let _ = gutter_line.paint(point(gutter_x, row.row_top), line_height, window, cx);
        }

        if let Some(code_line) = row.code_line.as_ref() {
            let _ = code_line.paint(point(code_origin_x, row.row_top), line_height, window, cx);
        }

        if focused
            && selection.start == selection.end
            && cursor_char >= row.line_start_char
            && cursor_char <= row.logical_end_char
        {
            let cursor_x = code_origin_x
                + x_for_global_char(&row, cursor_char.min(row.display_end_char))
                    .unwrap_or_else(|| px(0.0));
            window.paint_quad(fill(
                Bounds::new(
                    point(cursor_x, row.row_top),
                    size(px(CURSOR_WIDTH), px(ROW_HEIGHT)),
                ),
                rgb(0x1C6B74),
            ));
        }
    }
}

fn x_for_global_char(row: &PaintedRow, global_char: usize) -> Option<Pixels> {
    let local_char = global_char.saturating_sub(row.line_start_char);
    let code_line = row.code_line.as_ref()?;
    let byte = char_to_byte(code_line.text.as_ref(), local_char);
    Some(code_line.x_for_index(byte))
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn byte_index_to_char(text: &str, byte_index: usize) -> usize {
    text[..byte_index.min(text.len())].chars().count()
}

fn char_to_line_col(buffer: &Rope, char_offset: usize) -> (usize, usize) {
    let char_offset = char_offset.min(buffer.len_chars());
    let line = buffer.char_to_line(char_offset);
    let line_start = buffer.line_to_char(line);
    (line, char_offset - line_start)
}

fn line_indent_prefix(buffer: &Rope, line_ix: usize) -> String {
    line_display_text(buffer, line_ix)
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect()
}

fn char_to_utf16(buffer: &Rope, char_offset: usize) -> usize {
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

fn char_range_to_utf16_range(buffer: &Rope, range: &Range<usize>) -> Range<usize> {
    char_to_utf16(buffer, range.start)..char_to_utf16(buffer, range.end)
}

fn utf16_range_to_char_range(buffer: &Rope, range: &Range<usize>) -> Range<usize> {
    utf16_to_char(buffer, range.start)..utf16_to_char(buffer, range.end)
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn usage() -> &'static str {
    "Usage:
  cargo run
  cargo run -- file1.rs file2.md
  cargo run -- --bench-replace-corpus
  cargo run -- --bench-append-corpus
  cargo run -- --bench-replace-file /path/to/file.rs
  cargo run -- --bench-append-file /path/to/file.rs"
}

fn parse_launch_args() -> LaunchArgs {
    let mut args = LaunchArgs::default();
    let mut iter = std::env::args().skip(1);

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{}", usage());
                process::exit(0);
            }
            "--bench-replace-corpus" => {
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Replace,
                    source: CORPUS_PATH.to_string(),
                    text: PREMADE_CORPUS.to_string(),
                });
            }
            "--bench-append-corpus" => {
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Append,
                    source: CORPUS_PATH.to_string(),
                    text: PREMADE_CORPUS.to_string(),
                });
            }
            "--bench-replace-file" => {
                let Some(path) = iter.next() else {
                    eprintln!("missing file path for --bench-replace-file\n\n{}", usage());
                    process::exit(2);
                };
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => {
                        eprintln!("failed to read benchmark file {path}: {err}");
                        process::exit(2);
                    }
                };
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Replace,
                    source: path,
                    text,
                });
            }
            "--bench-append-file" => {
                let Some(path) = iter.next() else {
                    eprintln!("missing file path for --bench-append-file\n\n{}", usage());
                    process::exit(2);
                };
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => {
                        eprintln!("failed to read benchmark file {path}: {err}");
                        process::exit(2);
                    }
                };
                args.auto_bench = Some(AutoBench {
                    action: BenchAction::Append,
                    source: path,
                    text,
                });
            }
            _ if arg.starts_with("--") => {
                eprintln!("unknown argument: {arg}\n\n{}", usage());
                process::exit(2);
            }
            _ => args.files.push(PathBuf::from(arg)),
        }
    }

    args
}

fn main() {
    let launch = parse_launch_args();
    let auto_bench = launch.auto_bench.clone();
    let process_started = Instant::now();
    let has_graphical_env =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();

    if !has_graphical_env {
        eprintln!(
            "lst_gpui requires a graphical session. Run it from a real X11 or Wayland desktop."
        );
        process::exit(1);
    }

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-n", NewTab, None),
            KeyBinding::new("cmd-n", NewTab, None),
            KeyBinding::new("ctrl-o", OpenFile, None),
            KeyBinding::new("cmd-o", OpenFile, None),
            KeyBinding::new("ctrl-s", SaveFile, None),
            KeyBinding::new("cmd-s", SaveFile, None),
            KeyBinding::new("ctrl-shift-s", SaveFileAs, None),
            KeyBinding::new("cmd-shift-s", SaveFileAs, None),
            KeyBinding::new("ctrl-w", CloseActiveTab, None),
            KeyBinding::new("cmd-w", CloseActiveTab, None),
            KeyBinding::new("ctrl-tab", NextTab, None),
            KeyBinding::new("cmd-shift-]", NextTab, None),
            KeyBinding::new("ctrl-shift-tab", PrevTab, None),
            KeyBinding::new("cmd-shift-[", PrevTab, None),
            KeyBinding::new("ctrl-g", ToggleGutter, None),
            KeyBinding::new("cmd-g", ToggleGutter, None),
            KeyBinding::new("ctrl-r", ReloadCorpus, None),
            KeyBinding::new("cmd-r", ReloadCorpus, None),
            KeyBinding::new("ctrl-c", CopySelection, None),
            KeyBinding::new("cmd-c", CopySelection, None),
            KeyBinding::new("ctrl-x", CutSelection, None),
            KeyBinding::new("cmd-x", CutSelection, None),
            KeyBinding::new("ctrl-v", PasteClipboard, None),
            KeyBinding::new("cmd-v", PasteClipboard, None),
            KeyBinding::new("left", MoveLeft, None),
            KeyBinding::new("right", MoveRight, None),
            KeyBinding::new("up", MoveUp, None),
            KeyBinding::new("down", MoveDown, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("shift-up", SelectUp, None),
            KeyBinding::new("shift-down", SelectDown, None),
            KeyBinding::new("home", MoveLineStart, None),
            KeyBinding::new("end", MoveLineEnd, None),
            KeyBinding::new("shift-home", SelectLineStart, None),
            KeyBinding::new("shift-end", SelectLineEnd, None),
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", DeleteForward, None),
            KeyBinding::new("enter", InsertNewline, None),
            KeyBinding::new("tab", InsertTab, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("cmd-a", SelectAll, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("cmd-q", Quit, None),
        ]);

        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        let launch = launch.clone();
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("lst GPUI".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                let launch = launch.clone();
                cx.new(move |cx| LstGpuiApp::new(cx, launch))
            },
        ) {
            Ok(window) => window,
            Err(err) => {
                eprintln!(
                    "lst_gpui failed to open a GPUI window: {err}. On this host, Xvfb is not sufficient because GPUI surface creation requires a real presentation backend."
                );
                process::exit(1);
            }
        };

        let view = window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
                cx.entity()
            })
            .unwrap();

        if let Some(bench) = auto_bench.clone() {
            window
                .update(cx, move |_view, window, _cx| {
                    let view = view.clone();
                    let bench = bench.clone();
                    window.on_next_frame(move |window, cx| {
                        let startup_to_action_ms = elapsed_ms(process_started);
                        let _ = view.update(cx, |view, cx| {
                            view.run_auto_bench(
                                bench,
                                window,
                                cx,
                                startup_to_action_ms,
                                process_started,
                            );
                        });
                    });
                })
                .unwrap();
        }
    });
}
