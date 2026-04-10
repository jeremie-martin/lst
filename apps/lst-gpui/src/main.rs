use gpui::{
    actions, canvas, div, fill, point, prelude::*, px, rgb, size, App, Application, Bounds,
    ClipboardItem, Context, CursorStyle, ElementInputHandler, EntityInputHandler, FocusHandle,
    Focusable, IntoElement, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ScrollHandle, ShapedLine, SharedString, TextRun,
    UTF16Selection, Window, WindowBounds, WindowOptions,
};
extern crate self as iced;
pub use iced_core::keyboard;
pub mod widget {
    pub mod text_editor {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct Position {
            pub line: usize,
            pub column: usize,
        }
    }
}

#[path = "../../../src/vim.rs"]
mod vim;

use iced::{
    keyboard::{self as iced_keyboard, key::Named as IcedNamed, Modifiers as IcedModifiers},
    widget::text_editor,
};
use lst_core::{
    document::{char_to_position, position_to_char, EditKind, Tab},
    editor_ops,
    find::FindState,
    position::Position,
    wrap::{cursor_visual_row_in_line, visual_line_count, wrap_columns_with_gutter, wrap_segments},
};
use rfd::FileDialog;
use ropey::Rope;
use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut, Range},
    path::PathBuf,
    process,
    rc::Rc,
    sync::LazyLock,
    time::{Duration, Instant},
};
use tree_sitter_highlight::{
    Highlight as TreeSitterHighlight, HighlightConfiguration,
    HighlightEvent as TreeSitterHighlightEvent, Highlighter as TreeSitterHighlighter,
};

const WINDOW_WIDTH: f32 = 1360.0;
const WINDOW_HEIGHT: f32 = 860.0;
const ROW_HEIGHT: f32 = 22.0;
const GUTTER_WIDTH: f32 = 76.0;
const CODE_FONT_SIZE: f32 = 13.0;
const CURSOR_WIDTH: f32 = 2.0;
const VIEWPORT_OVERSCAN_LINES: usize = 6;
const EDITOR_LEFT_PAD: f32 = 18.0;
const EDITOR_RIGHT_PAD: f32 = 28.0;
const GUTTER_LEFT_PAD: f32 = 12.0;
const GUTTER_SEPARATOR_WIDTH: f32 = 14.0;
const WRAP_CHAR_WIDTH_FALLBACK: f32 = 7.8;
const UNTITLED_PREFIX: &str = "untitled";
const CORPUS_PATH: &str = "benchmarks/paste-corpus-20k.rs";
const PREMADE_CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/paste-corpus-20k.rs"
));
const COLOR_BG: u32 = 0x11111B;
const COLOR_SURFACE0: u32 = 0x181825;
const COLOR_SURFACE1: u32 = 0x1E1E2E;
const COLOR_SURFACE2: u32 = 0x313244;
const COLOR_BORDER: u32 = 0x45475A;
const COLOR_TEXT: u32 = 0xCDD6F4;
const COLOR_SUBTEXT: u32 = 0xA6ADC8;
const COLOR_MUTED: u32 = 0x6C7086;
const COLOR_ACCENT: u32 = 0x89B4FA;
const COLOR_GREEN: u32 = 0xA6E3A1;
const COLOR_YELLOW: u32 = 0xF9E2AF;
const COLOR_PEACH: u32 = 0xFAB387;
const COLOR_PINK: u32 = 0xF5C2E7;
const COLOR_MAUVE: u32 = 0xCBA6F7;
const COLOR_SAPPHIRE: u32 = 0x74C7EC;
const COLOR_LAVENDER: u32 = 0xB4BEFE;
const COLOR_SELECTION: u32 = 0x585B70;
const COLOR_CARET: u32 = 0xF5E0DC;
const COLOR_CURRENT_LINE: u32 = 0x181B2B;
const COLOR_GUTTER: u32 = 0x161622;
const TREE_SITTER_CAPTURE_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constructor",
    "escape",
    "function",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "string",
    "type",
    "variable",
];

static TREE_SITTER_RUST_CONFIG: LazyLock<HighlightConfiguration> = LazyLock::new(|| {
    let mut config = HighlightConfiguration::new(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
    .expect("embedded tree-sitter Rust highlight query should be valid");
    config.configure(TREE_SITTER_CAPTURE_NAMES);
    config
});

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
        ToggleWrap,
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
        Undo,
        Redo,
        FindOpen,
        FindOpenReplace,
        FindNext,
        FindPrev,
        ReplaceOne,
        ReplaceAll,
        GotoLineOpen,
        DeleteLine,
        MoveLineUp,
        MoveLineDown,
        DuplicateLine,
        ToggleComment,
        Quit,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayFocus {
    FindQuery,
    FindReplace,
    GotoLine,
}

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

#[derive(Clone, Debug)]
struct AutosaveJob {
    path: PathBuf,
    body: String,
    revision: u64,
}

#[derive(Clone)]
struct CachedShapedLine {
    text: SharedString,
    style_key: u64,
    shaped: ShapedLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SyntaxMode {
    Plain,
    TreeSitterRust,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SyntaxSpan {
    start: usize,
    end: usize,
    color: u32,
}

#[derive(Clone)]
struct CachedRustHighlights {
    revision: u64,
    lines: Vec<Vec<SyntaxSpan>>,
}

struct ViewportCache {
    code_lines: HashMap<(usize, usize, usize), CachedShapedLine>,
    gutter_lines: HashMap<usize, CachedShapedLine>,
    rust_highlights: Option<CachedRustHighlights>,
    rust_highlight_inflight_revision: Option<u64>,
    wrap_layout: Option<WrapLayout>,
}

impl Default for ViewportCache {
    fn default() -> Self {
        Self {
            code_lines: HashMap::new(),
            gutter_lines: HashMap::new(),
            rust_highlights: None,
            rust_highlight_inflight_revision: None,
            wrap_layout: None,
        }
    }
}

#[derive(Clone)]
struct PaintedRow {
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

#[derive(Clone, Debug)]
enum DragSelectionMode {
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
}

#[derive(Default)]
struct ViewportGeometry {
    bounds: Option<Bounds<Pixels>>,
    rows: Vec<PaintedRow>,
}

#[derive(Clone)]
struct WrapLayout {
    revision: u64,
    show_wrap: bool,
    wrap_columns: usize,
    line_row_starts: Vec<usize>,
    total_rows: usize,
}

struct EditorTab {
    doc: Tab,
    scroll: ScrollHandle,
    cache: Rc<RefCell<ViewportCache>>,
    geometry: Rc<RefCell<ViewportGeometry>>,
    marked_range: Option<Range<usize>>,
}

impl EditorTab {
    fn empty(name_hint: String) -> Self {
        Self::from_doc(Tab::empty(name_hint))
    }

    fn from_path(path: PathBuf, text: &str) -> Self {
        Self::from_doc(Tab::from_path(path, text))
    }

    fn from_text(name_hint: String, path: Option<PathBuf>, text: &str) -> Self {
        Self::from_doc(Tab::from_text(name_hint, path, text, false))
    }

    fn from_doc(doc: Tab) -> Self {
        Self {
            doc,
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(ViewportCache::default())),
            geometry: Rc::new(RefCell::new(ViewportGeometry::default())),
            marked_range: None,
        }
    }

    fn invalidate_visual_state(&mut self) {
        *self.cache.borrow_mut() = ViewportCache::default();
        *self.geometry.borrow_mut() = ViewportGeometry::default();
    }

    fn move_to(&mut self, offset: usize) {
        self.doc.move_to(offset);
        self.marked_range = None;
    }

    fn select_to(&mut self, offset: usize) {
        self.doc.select_to(offset);
        self.marked_range = None;
    }

    fn replace_char_range(&mut self, range: Range<usize>, new_text: &str) -> usize {
        let new_cursor = self.doc.replace_char_range(range, new_text);
        self.marked_range = None;
        self.invalidate_visual_state();
        new_cursor
    }

    fn set_text(&mut self, text: &str) {
        self.doc.set_text(text);
        self.marked_range = None;
        self.invalidate_visual_state();
    }

    fn display_line_char_len(&self, line_ix: usize) -> usize {
        line_display_char_len(&self.buffer, line_ix)
    }

    fn undo(&mut self) -> bool {
        let changed = self.doc.undo();
        if changed {
            self.marked_range = None;
            self.invalidate_visual_state();
        }
        changed
    }

    fn redo(&mut self) -> bool {
        let changed = self.doc.redo();
        if changed {
            self.marked_range = None;
            self.invalidate_visual_state();
        }
        changed
    }
}

impl Deref for EditorTab {
    type Target = Tab;

    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}

impl DerefMut for EditorTab {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.doc
    }
}

struct LstGpuiApp {
    focus_handle: FocusHandle,
    tabs: Vec<EditorTab>,
    active: usize,
    next_untitled_id: usize,
    show_gutter: bool,
    show_wrap: bool,
    drag_selecting: Option<DragSelectionMode>,
    find: FindState,
    goto_line: Option<String>,
    overlay_focus: Option<OverlayFocus>,
    find_query_cursor: usize,
    find_replace_cursor: usize,
    goto_line_cursor: usize,
    status: String,
    last_operation: OperationStats,
    vim: vim::VimState,
    autosave_inflight: HashSet<PathBuf>,
    autosave_started: bool,
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
            show_wrap: true,
            drag_selecting: None,
            find: FindState::new(),
            goto_line: None,
            overlay_focus: None,
            find_query_cursor: 0,
            find_replace_cursor: 0,
            goto_line_cursor: 0,
            status,
            last_operation,
            vim: vim::VimState::new(),
            autosave_inflight: HashSet::new(),
            autosave_started: false,
        }
    }

    fn tab_button(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = &self.tabs[ix];
        let active = ix == self.active;
        let label = if tab.modified {
            format!("{} •", tab.display_name())
        } else {
            tab.display_name()
        };

        div()
            .id(("tab", ix))
            .cursor_pointer()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(if active {
                rgb(COLOR_ACCENT)
            } else {
                rgb(COLOR_BORDER)
            })
            .bg(if active {
                rgb(COLOR_SURFACE2)
            } else {
                rgb(COLOR_SURFACE0)
            })
            .text_color(if active {
                rgb(COLOR_TEXT)
            } else {
                rgb(COLOR_SUBTEXT)
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_active_tab(ix);
                this.status = format!("Switched to {}.", this.active_tab().display_name());
                this.reveal_active_cursor();
                cx.notify();
            }))
    }

    fn ensure_active_rust_highlights(&mut self, cx: &mut Context<Self>) {
        let active = self.active;
        let Some(tab) = self.tabs.get(active) else {
            return;
        };
        if syntax_mode_for_path(tab.path.as_ref()) != SyntaxMode::TreeSitterRust {
            return;
        }

        let revision = tab.revision();
        let cache = tab.cache.clone();
        {
            let cache_ref = cache.borrow();
            if cache_ref
                .rust_highlights
                .as_ref()
                .is_some_and(|highlights| highlights.revision == revision)
            {
                return;
            }
            if cache_ref.rust_highlight_inflight_revision.is_some() {
                return;
            }
        }

        cache.borrow_mut().rust_highlight_inflight_revision = Some(revision);
        let source = tab.buffer_text();
        cx.spawn(async move |this, cx| {
            let lines = cx
                .background_executor()
                .spawn(async move {
                    let mut highlighter = TreeSitterHighlighter::new();
                    highlight_rust_source(&mut highlighter, &source)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.finish_rust_highlights(active, revision, cache, lines, cx);
            });
        })
        .detach();
    }

    fn finish_rust_highlights(
        &mut self,
        active: usize,
        revision: u64,
        cache: Rc<RefCell<ViewportCache>>,
        lines: Vec<Vec<SyntaxSpan>>,
        cx: &mut Context<Self>,
    ) {
        let mut cache_ref = cache.borrow_mut();
        if cache_ref.rust_highlight_inflight_revision != Some(revision) {
            return;
        }

        cache_ref.rust_highlight_inflight_revision = None;
        cache_ref.rust_highlights = Some(CachedRustHighlights { revision, lines });
        cache_ref.code_lines.clear();
        drop(cache_ref);

        if self.active == active
            && self
                .tabs
                .get(active)
                .is_some_and(|tab| Rc::ptr_eq(&tab.cache, &cache))
        {
            cx.notify();
        }
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

    fn set_active_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.active = index;
        self.vim.on_tab_switch();
        self.active_tab_mut().preferred_column = None;
        self.reindex_find_matches_to_nearest();
    }

    fn active_cursor_position(&self) -> Position {
        char_to_position(&self.active_tab().buffer, self.active_tab().cursor_char())
    }

    fn active_tab_revision(&self) -> u64 {
        self.active_tab().revision()
    }

    fn vim_cursor_position(&self) -> text_editor::Position {
        let cursor = self.active_cursor_position();
        text_editor::Position {
            line: cursor.line,
            column: cursor.column,
        }
    }

    fn vim_snapshot(&mut self) -> vim::TextSnapshot {
        let lines = self.tabs[self.active].lines();
        vim::TextSnapshot {
            lines,
            cursor: self.vim_cursor_position(),
        }
    }

    fn start_background_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.autosave_started {
            return;
        }
        self.autosave_started = true;
        let view = cx.entity();
        window
            .spawn(cx, async move |cx| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                if view.update(cx, |view, cx| view.autosave_tick(cx)).is_err() {
                    break;
                }
            })
            .detach();
    }

    fn autosave_tick(&mut self, cx: &mut Context<Self>) {
        let mut seen_paths = HashSet::new();
        let jobs: Vec<AutosaveJob> = self
            .tabs
            .iter()
            .filter(|tab| tab.modified)
            .filter_map(|tab| {
                let path = tab.path.clone()?;
                if !autosave_revision_is_current(&self.tabs, &path, tab.revision()) {
                    return None;
                }
                if self.autosave_inflight.contains(&path) || !seen_paths.insert(path.clone()) {
                    return None;
                }
                Some(AutosaveJob {
                    path,
                    body: tab.buffer_text(),
                    revision: tab.revision(),
                })
            })
            .collect();

        if jobs.is_empty() {
            return;
        }

        for job in jobs {
            self.autosave_inflight.insert(job.path.clone());
            cx.spawn({
                let job = job.clone();
                async move |this, cx| {
                    let temp_path = autosave_temp_path(&job.path, job.revision);
                    let body = job.body.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move { fs::write(&temp_path, &body).map(|_| temp_path) })
                        .await;
                    let _ = this.update(cx, |view, cx| view.finish_autosave(job, result, cx));
                }
            })
            .detach();
        }
    }

    fn finish_autosave(
        &mut self,
        job: AutosaveJob,
        result: std::io::Result<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.autosave_inflight.remove(&job.path);
        match result {
            Ok(temp_path) => {
                if !autosave_revision_is_current(&self.tabs, &job.path, job.revision) {
                    let _ = fs::remove_file(&temp_path);
                    cx.notify();
                    return;
                }

                match fs::rename(&temp_path, &job.path) {
                    Ok(()) => {
                        for tab in &mut self.tabs {
                            if tab.path.as_ref() == Some(&job.path)
                                && tab.revision() == job.revision
                            {
                                tab.modified = false;
                            }
                        }
                        if self.active_tab().path.as_ref() == Some(&job.path)
                            && self.active_tab().revision() == job.revision
                        {
                            self.status = format!("Autosaved {}.", job.path.display());
                        }
                    }
                    Err(err) => {
                        let _ = fs::remove_file(&temp_path);
                        if self.active_tab().path.as_ref() == Some(&job.path) {
                            self.status =
                                format!("Autosave failed for {}: {err}", job.path.display());
                        }
                    }
                }
            }
            Err(err) => {
                if self.active_tab().path.as_ref() == Some(&job.path) {
                    self.status = format!("Autosave failed for {}: {err}", job.path.display());
                }
            }
        }
        cx.notify();
    }

    fn selected_find_match_start(&self) -> Option<Position> {
        if self.find.query.is_empty() {
            return None;
        }
        let tab = self.active_tab();
        if !tab.has_selection() {
            return None;
        }
        let selected = tab.selected_range();
        if selected.end.saturating_sub(selected.start) != self.find.query.chars().count() {
            return None;
        }
        Some(char_to_position(&tab.buffer, selected.start))
    }

    fn align_find_current_to_visible_match(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }

        if let Some(start) = self.selected_find_match_start() {
            if self.find.select_exact(&start) {
                return;
            }
        }

        let pos = self.active_cursor_position();
        self.find.find_nearest(&pos);
    }

    fn reindex_find_matches(&mut self) {
        if self.find.query.is_empty() {
            self.find.clear_results();
            return;
        }
        let text = self.active_tab().buffer.to_string();
        self.find.compute_matches_in_text(&text);
        self.find.finish_reindex(self.active_tab_revision());
    }

    fn reindex_find_matches_to_nearest(&mut self) {
        self.reindex_find_matches();
        if !self.find.matches.is_empty() {
            self.align_find_current_to_visible_match();
        }
    }

    fn ensure_find_matches_current(&mut self) {
        if self.find.is_stale(self.active_tab_revision()) {
            self.reindex_find_matches();
        }
    }

    fn select_current_find_match(&mut self) -> bool {
        let Some((start, end)) = self.find.current_match_range() else {
            return false;
        };
        self.active_tab_mut().set_cursor_position(end, Some(start));
        true
    }

    fn sync_find_after_edit(&mut self) {
        if self.find.visible && !self.find.query.is_empty() {
            self.reindex_find_matches_to_nearest();
        } else if !self.find.query.is_empty() {
            self.find.mark_dirty();
        }
    }

    fn selected_single_line_text(&self) -> Option<String> {
        let text = self.active_tab().selected_text()?;
        (!text.contains('\n')).then_some(text)
    }

    fn open_find(&mut self, show_replace: bool) {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(sel) = self.selected_single_line_text() {
            self.find.query = sel;
        }
        self.find_query_cursor = self.find.query.chars().count();
        self.find_replace_cursor = self.find.replacement.chars().count();
        self.overlay_focus = Some(OverlayFocus::FindQuery);
        self.reindex_find_matches_to_nearest();
    }

    fn close_find(&mut self) {
        self.find.visible = false;
        self.overlay_focus = None;
    }

    fn open_goto_line(&mut self) {
        self.goto_line = Some(String::new());
        self.goto_line_cursor = 0;
        self.overlay_focus = Some(OverlayFocus::GotoLine);
    }

    fn close_goto_line(&mut self) {
        self.goto_line = None;
        self.overlay_focus = None;
    }

    fn replace_active_lines(&mut self, lines: Vec<String>, cursor_line: usize, cursor_col: usize) {
        let active = self.active;
        {
            let tab = &mut self.tabs[active];
            let newline = preferred_newline_for_buffer(&tab.buffer);
            tab.set_text(&lines.join(newline));
            tab.modified = true;
            let cursor = position_to_char(
                &tab.buffer,
                Position {
                    line: cursor_line,
                    column: cursor_col,
                },
            );
            tab.move_to(cursor);
        }
        self.sync_find_after_edit();
    }

    fn move_active_cursor(&mut self, cursor_line: usize, cursor_col: usize, select: bool) {
        let position = Position {
            line: cursor_line,
            column: cursor_col,
        };
        let active = self.active;
        let anchor = if select {
            Some(char_to_position(
                &self.tabs[active].buffer,
                self.tabs[active].cursor_char(),
            ))
        } else {
            None
        };
        self.tabs[active].set_cursor_position(position, anchor);
    }

    fn apply_line_edit<R, F>(&mut self, edit: F) -> Option<R>
    where
        F: FnOnce(&mut Vec<String>) -> Option<(R, usize, usize)>,
    {
        let cached_lines = self.tabs[self.active].lines();
        let mut lines: Vec<String> = cached_lines.iter().cloned().collect();
        let (result, cursor_line, cursor_col) = edit(&mut lines)?;
        if lines.as_slice() == cached_lines.as_ref() {
            let cursor = self.active_cursor_position();
            if cursor.line == cursor_line && cursor.column == cursor_col {
                return None;
            }
            self.move_active_cursor(cursor_line, cursor_col, false);
            return Some(result);
        }

        self.tabs[self.active].push_undo_snapshot(EditKind::Other, true);
        self.replace_active_lines(lines, cursor_line, cursor_col);
        Some(result)
    }

    fn active_cursor_line_col(&self) -> (usize, usize) {
        char_to_line_col(&self.active_tab().buffer, self.active_tab().cursor_char())
    }

    fn selection_summary(&self) -> Option<String> {
        let selected = self.active_tab().selected_range();
        (selected.start != selected.end)
            .then(|| format!("Sel {}", selected.end.saturating_sub(selected.start)))
    }

    fn status_details(&self) -> String {
        let tab = self.active_tab();
        let (line, column) = self.active_cursor_line_col();
        let mut parts = vec![
            self.vim.mode.label().to_string(),
            format!("Ln {}", line + 1),
            format!("Col {}", column + 1),
            if self.show_wrap {
                "Wrap".to_string()
            } else {
                "No Wrap".to_string()
            },
            format!("{} lines", tab.line_count()),
        ];
        let pending = self.vim.pending_display();
        if !pending.is_empty() {
            parts.push(pending);
        }
        if let Some(selection) = self.selection_summary() {
            parts.push(selection);
        }
        if self.find.visible {
            let current = if self.find.matches.is_empty() {
                0
            } else {
                self.find.current + 1
            };
            parts.push(format!("Match {current}/{}", self.find.matches.len()));
        }
        parts.join("  ")
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
            let name_hint = tab.display_name();
            *tab = EditorTab::from_text(name_hint, None, text);
        }
        self.sync_find_after_edit();
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.status = format!("Loaded {} lines.", self.active_tab().line_count());
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
        self.sync_find_after_edit();
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.status = format!("Appended {} lines.", text.lines().count());
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
            let kind = if text.is_empty() {
                EditKind::Delete
            } else {
                EditKind::Insert
            };
            tab.push_undo_snapshot(kind, text.chars().any(char::is_whitespace));
            let range = tab
                .marked_range
                .clone()
                .unwrap_or_else(|| tab.selected_range());
            tab.replace_char_range(range, text);
        }
        self.sync_find_after_edit();
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
            tab.push_undo_snapshot(EditKind::Delete, false);
            tab.replace_char_range(range, "");
        }
        self.sync_find_after_edit();
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
            tab.push_undo_snapshot(EditKind::Delete, false);
            tab.replace_char_range(range, "");
        }
        self.sync_find_after_edit();
        self.record_operation("delete", None, elapsed_ms(apply_started));
        self.reveal_active_cursor();
        cx.notify();
    }

    fn insert_newline(&mut self, cx: &mut Context<Self>) {
        let (newline, indent) = {
            let tab = self.active_tab();
            let (line, _) = char_to_line_col(&tab.buffer, tab.cursor_char());
            (
                preferred_newline_for_buffer(&tab.buffer),
                line_indent_prefix(&tab.buffer, line),
            )
        };
        self.insert_text_at_selection("newline", &format!("{newline}{indent}"), cx);
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

    fn move_vertical(
        &mut self,
        delta: isize,
        select: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_visual_row(delta, select, window) {
            self.reveal_active_cursor();
            cx.notify();
            return;
        }

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

    fn move_visual_row(&mut self, delta: isize, select: bool, window: &mut Window) -> bool {
        if !self.show_wrap {
            return false;
        }

        let active = self.active;
        let viewport_width = self.tabs[active]
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| px(WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window);
        let revision = self.tabs[active].revision();
        let lines = self.tabs[active].lines();
        let layout = {
            let mut cache = self.tabs[active].cache.borrow_mut();
            ensure_wrap_layout(
                &mut cache,
                lines.as_ref(),
                revision,
                viewport_width,
                char_width,
                self.show_gutter,
                self.show_wrap,
            )
        };

        let tab = &mut self.tabs[active];
        let cursor = tab.cursor_char();
        let line = tab.buffer.char_to_line(cursor.min(tab.buffer.len_chars()));
        let line_start = tab.buffer.line_to_char(line);
        let display_text = trim_display_line(lines[line].as_str());
        let column = cursor
            .saturating_sub(line_start)
            .min(display_text.chars().count());
        let segment_row = cursor_visual_row_in_line(display_text, column, layout.wrap_columns);
        let visual_row = layout.line_row_starts[line] + segment_row;
        let target_visual_row = if delta.is_negative() {
            visual_row.saturating_sub(delta.unsigned_abs())
        } else {
            (visual_row + delta as usize).min(layout.total_rows.saturating_sub(1))
        };

        if target_visual_row == visual_row {
            return false;
        }

        let segments = wrap_segments(display_text, layout.wrap_columns);
        let current_segment = segments
            .get(segment_row)
            .unwrap_or_else(|| segments.last().expect("wrap returns at least one segment"));
        let preferred = tab
            .preferred_column
            .unwrap_or(column.saturating_sub(current_segment.start_col));
        let target_line = line_for_visual_row(&layout, target_visual_row);
        let target_text = trim_display_line(lines[target_line].as_str());
        let target_segments = wrap_segments(target_text, layout.wrap_columns);
        let target_row_in_line = target_visual_row - layout.line_row_starts[target_line];
        let target_segment = target_segments.get(target_row_in_line).unwrap_or_else(|| {
            target_segments
                .last()
                .expect("wrap returns at least one segment")
        });
        let target_col =
            target_segment.start_col + preferred.min(target_segment.text.chars().count());
        let target = tab.buffer.line_to_char(target_line) + target_col;
        tab.preferred_column = Some(preferred);
        if select {
            tab.select_to(target);
        } else {
            tab.move_to(target);
        }
        true
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
        self.active_tab_mut()
            .push_undo_snapshot(EditKind::Delete, true);
        self.active_tab_mut().replace_char_range(range, "");
        self.sync_find_after_edit();
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
            tab.push_undo_snapshot(EditKind::Insert, text.chars().any(char::is_whitespace));
            let range = tab
                .marked_range
                .clone()
                .unwrap_or_else(|| tab.selected_range());
            tab.replace_char_range(range, &text);
        }
        self.sync_find_after_edit();
        self.record_operation(
            "paste_clipboard",
            Some(elapsed_ms(read_started)),
            elapsed_ms(apply_started),
        );
        self.status = format!("Pasted {} line(s).", text.lines().count());
        self.reveal_active_cursor();
        cx.notify();
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
            self.set_active_tab(self.tabs.len() - 1);
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
        if self.active_tab().modified {
            self.status = "Unsaved changes. Save or Save As before closing this tab.".to_string();
            cx.notify();
            return;
        }

        if self.tabs.len() == 1 {
            self.tabs[0] = self.new_empty_tab();
            self.set_active_tab(0);
            self.status = "Closed tab.".to_string();
            cx.notify();
            return;
        }

        self.tabs.remove(self.active);
        self.set_active_tab(self.active.min(self.tabs.len().saturating_sub(1)));
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

        let visual_row = tab
            .cache
            .borrow()
            .wrap_layout
            .as_ref()
            .and_then(|layout| visual_row_for_char(tab, layout))
            .unwrap_or_else(|| tab.buffer.char_to_line(tab.cursor_char()));
        let caret_top = px((visual_row as f32) * ROW_HEIGHT);
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
        let code_origin_x = bounds.left() + code_origin_pad(self.show_gutter);

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
        let index = self.active_char_index_for_point(event.position);
        if event.click_count >= 3 {
            let line_range = line_range_at_char(&self.active_tab().buffer, index);
            self.drag_selecting = Some(DragSelectionMode::Line(line_range.clone()));
            self.select_active_range(line_range);
            self.sync_primary_selection(cx);
            cx.notify();
            return;
        }
        if event.click_count == 2 {
            let word_range = word_range_at_char(&self.active_tab().buffer, index);
            self.drag_selecting = Some(DragSelectionMode::Word(word_range.clone()));
            self.select_active_range(word_range);
            self.sync_primary_selection(cx);
            cx.notify();
            return;
        }

        self.drag_selecting = Some(DragSelectionMode::Character);
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
        let index = self.active_char_index_for_point(event.position);
        match self.drag_selecting.clone() {
            Some(DragSelectionMode::Character) => self.active_tab_mut().select_to(index),
            Some(DragSelectionMode::Word(anchor)) => {
                let current = word_range_at_char(&self.active_tab().buffer, index);
                self.select_active_drag_range(anchor, current);
            }
            Some(DragSelectionMode::Line(anchor)) => {
                let current = line_range_at_char(&self.active_tab().buffer, index);
                self.select_active_drag_range(anchor, current);
            }
            None => return,
        }
        self.reveal_active_cursor();
        cx.notify();
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.drag_selecting = None;
        self.sync_primary_selection(cx);
        cx.notify();
    }

    fn select_active_range(&mut self, range: Range<usize>) {
        let tab = self.active_tab_mut();
        let end = tab.len_chars();
        tab.selection = range.start.min(end)..range.end.min(end);
        tab.selection_reversed = false;
        tab.preferred_column = None;
        tab.marked_range = None;
    }

    fn select_active_drag_range(&mut self, anchor: Range<usize>, current: Range<usize>) {
        let (selection, reversed) = drag_selection_range(anchor, current);
        let tab = self.active_tab_mut();
        let end = tab.len_chars();
        tab.selection = selection.start.min(end)..selection.end.min(end);
        tab.selection_reversed = reversed;
        tab.preferred_column = None;
        tab.marked_range = None;
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
        self.set_active_tab(self.tabs.len() - 1);
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
            self.set_active_tab((self.active + 1) % self.tabs.len());
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_prev_tab(&mut self, _: &PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() > 1 {
            let prev = if self.active == 0 {
                self.tabs.len() - 1
            } else {
                self.active - 1
            };
            self.set_active_tab(prev);
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_toggle_wrap(&mut self, _: &ToggleWrap, _: &mut Window, cx: &mut Context<Self>) {
        self.show_wrap = !self.show_wrap;
        self.active_tab_mut().invalidate_visual_state();
        self.status = if self.show_wrap {
            "Soft wrap enabled.".to_string()
        } else {
            "Soft wrap disabled.".to_string()
        };
        self.reveal_active_cursor();
        cx.notify();
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

    fn handle_move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, window, cx);
    }

    fn handle_move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, window, cx);
    }

    fn handle_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(-1, true, cx);
    }

    fn handle_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_horizontal(1, true, cx);
    }

    fn handle_select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, window, cx);
    }

    fn handle_select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, window, cx);
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

    fn handle_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_mut().undo() {
            self.sync_find_after_edit();
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.active_tab_mut().redo() {
            self.sync_find_after_edit();
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_find_open(&mut self, _: &FindOpen, _: &mut Window, cx: &mut Context<Self>) {
        if self.find.visible && !self.find.show_replace {
            self.close_find();
        } else {
            self.open_find(false);
        }
        cx.notify();
    }

    fn handle_find_open_replace(
        &mut self,
        _: &FindOpenReplace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.find.visible && self.find.show_replace {
            self.close_find();
        } else {
            self.open_find(true);
        }
        cx.notify();
    }

    fn handle_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return;
        }
        self.find.next();
        self.select_current_find_match();
        self.reveal_active_cursor();
        cx.notify();
    }

    fn handle_find_prev(&mut self, _: &FindPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return;
        }
        self.find.prev();
        self.select_current_find_match();
        self.reveal_active_cursor();
        cx.notify();
    }

    fn handle_replace_one(&mut self, _: &ReplaceOne, _: &mut Window, cx: &mut Context<Self>) {
        self.ensure_find_matches_current();
        let Some((start, end)) = self.find.current_match_range() else {
            return;
        };
        let replacement = self.find.replacement.clone();
        let range = {
            let tab = self.active_tab();
            position_to_char(&tab.buffer, start)..position_to_char(&tab.buffer, end)
        };
        self.active_tab_mut()
            .push_undo_snapshot(EditKind::Other, true);
        self.active_tab_mut()
            .replace_char_range(range, &replacement);
        self.sync_find_after_edit();
        self.select_current_find_match();
        self.reveal_active_cursor();
        cx.notify();
    }

    fn handle_replace_all(&mut self, _: &ReplaceAll, _: &mut Window, cx: &mut Context<Self>) {
        if self.find.query.is_empty() {
            return;
        }
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        let cursor = self.active_cursor_position();
        let changed = self.apply_line_edit(|lines| {
            let new_lines: Vec<String> = lines
                .iter()
                .map(|line| line.replace(&query, &replacement))
                .collect();
            if new_lines == *lines {
                return None;
            }
            *lines = new_lines;
            Some(((), cursor.line, cursor.column))
        });
        if changed.is_some() {
            self.reindex_find_matches_to_nearest();
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_goto_line_open(&mut self, _: &GotoLineOpen, _: &mut Window, cx: &mut Context<Self>) {
        if self.goto_line.is_some() {
            self.close_goto_line();
        } else {
            self.open_goto_line();
        }
        cx.notify();
    }

    fn handle_delete_line(&mut self, _: &DeleteLine, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.active_cursor_position();
        let changed = self.apply_line_edit(|lines| {
            let line = editor_ops::delete_line(lines, pos.line);
            Some(((), line, pos.column))
        });
        if changed.is_some() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_move_line_up(&mut self, _: &MoveLineUp, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.active_cursor_position();
        let changed = self.apply_line_edit(|lines| {
            let line = editor_ops::move_line_up(lines, pos.line)?;
            Some(((), line, pos.column))
        });
        if changed.is_some() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_move_line_down(&mut self, _: &MoveLineDown, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.active_cursor_position();
        let changed = self.apply_line_edit(|lines| {
            let line = editor_ops::move_line_down(lines, pos.line)?;
            Some(((), line, pos.column))
        });
        if changed.is_some() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_duplicate_line(&mut self, _: &DuplicateLine, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.active_cursor_position();
        let changed = self.apply_line_edit(|lines| {
            let line = editor_ops::duplicate_line(lines, pos.line);
            Some(((), line, pos.column))
        });
        if changed.is_some() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_toggle_comment(&mut self, _: &ToggleComment, _: &mut Window, cx: &mut Context<Self>) {
        let prefix = self
            .active_tab()
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| editor_ops::comment_prefix(e.to_string_lossy().as_ref()))
            .unwrap_or("//");
        let selected = self.active_tab().selected_range();
        let cursor = self.active_cursor_position();
        let start = char_to_position(&self.active_tab().buffer, selected.start);
        let end = char_to_position(&self.active_tab().buffer, selected.end);
        let first = start.line.min(end.line);
        let last = start.line.max(end.line);
        let changed = self.apply_line_edit(|lines| {
            let (line, col) =
                editor_ops::toggle_comment(lines, first, last, cursor.line, cursor.column, prefix);
            Some(((), line, col))
        });
        if changed.is_some() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn maybe_handle_vim_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let mods = gpui_modifiers_to_iced(event.keystroke.modifiers);
        let key = gpui_key_to_iced(event);
        let plain_vim_key = !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform;
        let redo_key = key.as_ref().is_some_and(|key| {
            matches!(key.as_ref(), iced_keyboard::Key::Character("r")) && mods.command()
        });

        if event.keystroke.key == "escape" {
            let snapshot = self.vim_snapshot();
            let commands = self
                .vim
                .enter_normal_from_escape(snapshot.cursor, &snapshot);
            self.execute_vim_commands(commands, cx);
            cx.stop_propagation();
            cx.notify();
            return true;
        }

        if self.vim.mode == vim::Mode::Insert {
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

        let snapshot = self.vim_snapshot();
        let commands = self.vim.handle_key(&key, mods, &snapshot);
        self.execute_vim_commands(commands, cx);
        cx.stop_propagation();
        cx.notify();
        true
    }

    fn execute_vim_commands(&mut self, commands: Vec<vim::VimCommand>, cx: &mut Context<Self>) {
        for cmd in commands {
            match cmd {
                vim::VimCommand::Noop => {}
                vim::VimCommand::MoveTo(position) => {
                    self.active_tab_mut()
                        .set_cursor_position(position_from_vim(position), None);
                }
                vim::VimCommand::Select { anchor, head } => self.apply_vim_select(anchor, head),
                vim::VimCommand::DeleteRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                }
                vim::VimCommand::DeleteLines { first, last } => {
                    let deleted = self.vim_delete_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                }
                vim::VimCommand::ChangeRange { from, to } => {
                    let deleted = self.vim_delete_range(from, to);
                    self.vim.register = vim::Register::Char(deleted);
                    self.vim.mode = vim::Mode::Insert;
                }
                vim::VimCommand::ChangeLines { first, last } => {
                    let deleted = self.vim_change_lines(first, last);
                    self.vim.register = vim::Register::Line(deleted);
                    self.vim.mode = vim::Mode::Insert;
                }
                vim::VimCommand::YankRange { from, to } => {
                    self.vim.register = vim::Register::Char(self.vim_extract_range(from, to));
                }
                vim::VimCommand::YankLines { first, last } => {
                    self.vim.register = vim::Register::Line(self.vim_extract_lines(first, last));
                }
                vim::VimCommand::EnterInsert => self.vim.mode = vim::Mode::Insert,
                vim::VimCommand::PasteAfter => self.vim_paste(false),
                vim::VimCommand::PasteBefore => self.vim_paste(true),
                vim::VimCommand::OpenLineBelow => {
                    self.vim_open_line(false);
                    self.vim.mode = vim::Mode::Insert;
                }
                vim::VimCommand::OpenLineAbove => {
                    self.vim_open_line(true);
                    self.vim.mode = vim::Mode::Insert;
                }
                vim::VimCommand::JoinLines { count } => self.vim_join_lines(count),
                vim::VimCommand::ReplaceChar { ch, count } => self.vim_replace_char(ch, count),
                vim::VimCommand::Undo => {
                    if self.active_tab_mut().undo() {
                        self.sync_find_after_edit();
                    }
                }
                vim::VimCommand::Redo => {
                    if self.active_tab_mut().redo() {
                        self.sync_find_after_edit();
                    }
                }
                vim::VimCommand::OpenFind => self.open_find(false),
                vim::VimCommand::FindNext => {
                    self.ensure_find_matches_current();
                    if let Some(target) = self.vim_find_next_from_cursor(self.vim_cursor_position())
                    {
                        self.move_to_vim_search_target(target);
                    }
                }
                vim::VimCommand::FindPrev => {
                    self.ensure_find_matches_current();
                    if let Some(target) = self.vim_find_prev_from_cursor(self.vim_cursor_position())
                    {
                        self.move_to_vim_search_target(target);
                    }
                }
                vim::VimCommand::SearchWordUnderCursor { word, forward } => {
                    self.find.query = word;
                    self.reindex_find_matches();
                    let cursor = self.vim_cursor_position();
                    let target = if forward {
                        self.vim_find_next_from_cursor(cursor)
                    } else {
                        self.vim_find_prev_from_cursor(cursor)
                    };
                    if let Some(target) = target {
                        self.move_to_vim_search_target(target);
                    }
                }
                vim::VimCommand::TransformCaseRange {
                    from,
                    to,
                    uppercase,
                } => self.vim_transform_case_range(from, to, uppercase),
                vim::VimCommand::TransformCaseLines {
                    first,
                    last,
                    uppercase,
                } => self.vim_transform_case_lines(first, last, uppercase),
            }
        }

        self.reveal_active_cursor();
        self.sync_primary_selection(cx);
    }

    fn vim_find_next_from_cursor(
        &mut self,
        position: text_editor::Position,
    ) -> Option<text_editor::Position> {
        let index = self
            .find
            .matches
            .iter()
            .position(|m| {
                m.line > position.line || (m.line == position.line && m.col > position.column)
            })
            .or_else(|| (!self.find.matches.is_empty()).then_some(0))?;
        self.find.current = index;
        let m = self.find.matches[index];
        Some(text_editor::Position {
            line: m.line,
            column: m.col,
        })
    }

    fn vim_find_prev_from_cursor(
        &mut self,
        position: text_editor::Position,
    ) -> Option<text_editor::Position> {
        let index = self
            .find
            .matches
            .iter()
            .rposition(|m| {
                m.line < position.line || (m.line == position.line && m.col < position.column)
            })
            .or_else(|| self.find.matches.len().checked_sub(1))?;
        self.find.current = index;
        let m = self.find.matches[index];
        Some(text_editor::Position {
            line: m.line,
            column: m.col,
        })
    }

    fn apply_vim_select(&mut self, anchor: text_editor::Position, head: text_editor::Position) {
        let tab = self.active_tab_mut();
        let anchor_char = position_to_char(&tab.buffer, position_from_vim(anchor));
        let head_char = position_to_char(&tab.buffer, position_from_vim(head));
        let anchor_end = inclusive_position_to_exclusive_char(&tab.buffer, anchor);
        let head_end = inclusive_position_to_exclusive_char(&tab.buffer, head);
        if vim_position_lt(head, anchor) {
            tab.selection = head_char..anchor_end.max(head_char);
            tab.selection_reversed = true;
        } else {
            tab.selection = anchor_char..head_end.max(anchor_char);
            tab.selection_reversed = false;
        }
        tab.marked_range = None;
        tab.preferred_column = None;
    }

    fn move_to_vim_search_target(&mut self, target: text_editor::Position) {
        if matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine) {
            let snapshot = self.vim_snapshot();
            if let vim::VimCommand::Select { anchor, head } =
                self.vim.selection_command(target, &snapshot)
            {
                self.apply_vim_select(anchor, head);
            }
        } else {
            self.active_tab_mut()
                .set_cursor_position(position_from_vim(target), None);
        }
    }

    fn vim_delete_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
    ) -> String {
        self.apply_line_edit(|lines| {
            let deleted = extract_text_range(lines, &from, &to);
            remove_text_range(lines, &from, &to);
            let cursor_col = from.column.min(
                lines
                    .get(from.line)
                    .map_or(0, |line| line.chars().count().saturating_sub(1)),
            );
            Some((deleted, from.line, cursor_col))
        })
        .unwrap_or_default()
    }

    fn vim_delete_lines(&mut self, first: usize, last: usize) -> String {
        self.apply_line_edit(|lines| {
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            let deleted = lines[first..=last].join("\n");
            lines.drain(first..=last);
            if lines.is_empty() {
                lines.push(String::new());
            }
            let cursor_line = first.min(lines.len().saturating_sub(1));
            Some((deleted, cursor_line, 0))
        })
        .unwrap_or_default()
    }

    fn vim_change_lines(&mut self, first: usize, last: usize) -> String {
        self.apply_line_edit(|lines| {
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            let indent: String = lines[first]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();
            let deleted = lines[first..=last].join("\n");
            lines.drain(first..=last);
            lines.insert(first, indent.clone());
            Some((deleted, first, indent.chars().count()))
        })
        .unwrap_or_default()
    }

    fn vim_extract_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
    ) -> String {
        let lines = self.tabs[self.active].lines();
        extract_text_range(lines.as_ref(), &from, &to)
    }

    fn vim_extract_lines(&mut self, first: usize, last: usize) -> String {
        let lines = self.tabs[self.active].lines();
        let first = first.min(lines.len().saturating_sub(1));
        let last = last.min(lines.len().saturating_sub(1));
        lines[first..=last].join("\n")
    }

    fn vim_paste(&mut self, before: bool) {
        match self.vim.register.clone() {
            vim::Register::Empty => {}
            vim::Register::Char(paste_text) => {
                let cursor = self.vim_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let line_chars: Vec<char> = lines[cursor.line].chars().collect();
                    let insert_col = if before {
                        cursor.column.min(line_chars.len())
                    } else {
                        (cursor.column + 1).min(line_chars.len())
                    };
                    let prefix: String = line_chars[..insert_col].iter().collect();
                    let suffix: String = line_chars[insert_col..].iter().collect();
                    let paste_lines: Vec<&str> = paste_text.split('\n').collect();
                    if paste_lines.len() == 1 {
                        lines[cursor.line] = format!("{prefix}{}{suffix}", paste_lines[0]);
                        let cursor_col =
                            insert_col + paste_lines[0].chars().count().saturating_sub(1);
                        return Some(((), cursor.line, cursor_col));
                    }

                    let first_new = format!("{prefix}{}", paste_lines[0]);
                    let last_new = format!("{}{suffix}", paste_lines.last().unwrap_or(&""));
                    let mut new_lines: Vec<String> = lines[..cursor.line].to_vec();
                    new_lines.push(first_new);
                    for paste_line in &paste_lines[1..paste_lines.len() - 1] {
                        new_lines.push((*paste_line).to_string());
                    }
                    new_lines.push(last_new);
                    new_lines.extend(lines[cursor.line + 1..].iter().cloned());
                    let cursor_line = cursor.line + paste_lines.len() - 1;
                    let cursor_col = paste_lines
                        .last()
                        .unwrap_or(&"")
                        .chars()
                        .count()
                        .saturating_sub(1);
                    *lines = new_lines;
                    Some(((), cursor_line, cursor_col))
                });
            }
            vim::Register::Line(paste_text) => {
                let cursor = self.vim_cursor_position();
                let _ = self.apply_line_edit(|lines| {
                    let insert_at = if before { cursor.line } else { cursor.line + 1 };
                    lines.splice(
                        insert_at..insert_at,
                        paste_text.split('\n').map(String::from),
                    );
                    let indent = lines.get(insert_at).map_or(0, |line| {
                        line.chars().take_while(|c| c.is_whitespace()).count()
                    });
                    Some(((), insert_at, indent))
                });
            }
        }
    }

    fn vim_open_line(&mut self, above: bool) {
        let pos = self.vim_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let indent: String = lines.get(pos.line).map_or(String::new(), |line| {
                line.chars().take_while(|c| c.is_whitespace()).collect()
            });
            let idx = if above { pos.line } else { pos.line + 1 };
            lines.insert(idx, indent.clone());
            Some(((), idx, indent.chars().count()))
        });
    }

    fn vim_join_lines(&mut self, count: usize) {
        let pos = self.vim_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            if pos.line + 1 >= lines.len() {
                return None;
            }

            let join_end = (pos.line + count).min(lines.len() - 1);
            let mut joined = lines[pos.line].trim_end().to_string();
            let join_col = joined.chars().count();
            for line in lines.drain((pos.line + 1)..=join_end) {
                let trimmed = line.trim_start();
                if !trimmed.is_empty() {
                    joined.push(' ');
                    joined.push_str(trimmed);
                }
            }
            lines[pos.line] = joined;
            Some(((), pos.line, join_col))
        });
    }

    fn vim_replace_char(&mut self, ch: char, count: usize) {
        let pos = self.vim_cursor_position();
        let _ = self.apply_line_edit(|lines| {
            let chars: Vec<char> = lines
                .get(pos.line)
                .map_or(Vec::new(), |line| line.chars().collect());
            if pos.column + count > chars.len() {
                return None;
            }
            let mut new_chars = chars;
            for ix in 0..count {
                new_chars[pos.column + ix] = ch;
            }
            lines[pos.line] = new_chars.into_iter().collect();
            Some(((), pos.line, pos.column + count - 1))
        });
    }

    fn vim_transform_case_range(
        &mut self,
        from: text_editor::Position,
        to: text_editor::Position,
        uppercase: bool,
    ) {
        let _ = self.apply_line_edit(|lines| {
            editor_ops::transform_case_range(
                lines,
                from.line,
                from.column,
                to.line,
                to.column,
                uppercase,
            );
            Some(((), from.line, from.column))
        });
    }

    fn vim_transform_case_lines(&mut self, first: usize, last: usize, uppercase: bool) {
        let _ = self.apply_line_edit(|lines| {
            if lines.is_empty() {
                return None;
            }
            let first = first.min(lines.len().saturating_sub(1));
            let last = last.min(lines.len().saturating_sub(1));
            for line in &mut lines[first..=last] {
                *line = if uppercase {
                    line.to_uppercase()
                } else {
                    line.to_lowercase()
                };
            }
            Some(((), first, 0))
        });
    }

    fn submit_goto_line(&mut self) -> bool {
        let Some(text) = self.goto_line.as_ref() else {
            return false;
        };
        let Ok(line_num) = text.trim().parse::<usize>() else {
            self.close_goto_line();
            return false;
        };
        let target = line_num
            .saturating_sub(1)
            .min(self.active_tab().line_count().saturating_sub(1));
        self.active_tab_mut().set_cursor_position(
            Position {
                line: target,
                column: 0,
            },
            None,
        );
        self.close_goto_line();
        true
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            if self.goto_line.is_some() {
                self.close_goto_line();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            if self.find.visible {
                self.close_find();
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }

        let Some(focus) = self.overlay_focus else {
            let _ = self.maybe_handle_vim_key(event, cx);
            return;
        };

        match focus {
            OverlayFocus::FindQuery => {
                if handle_overlay_input(
                    &event.keystroke,
                    &mut self.find.query,
                    &mut self.find_query_cursor,
                ) {
                    self.reindex_find_matches_to_nearest();
                    self.select_current_find_match();
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                match event.keystroke.key.as_str() {
                    "tab" => {
                        if self.find.show_replace {
                            self.overlay_focus = Some(OverlayFocus::FindReplace);
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        self.handle_find_next(&FindNext, _window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }
            OverlayFocus::FindReplace => {
                if handle_overlay_input(
                    &event.keystroke,
                    &mut self.find.replacement,
                    &mut self.find_replace_cursor,
                ) {
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                match event.keystroke.key.as_str() {
                    "tab" => {
                        self.overlay_focus = Some(OverlayFocus::FindQuery);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        self.handle_replace_one(&ReplaceOne, _window, cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }
            OverlayFocus::GotoLine => {
                if let Some(text) = self.goto_line.as_mut() {
                    if handle_overlay_input(&event.keystroke, text, &mut self.goto_line_cursor) {
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                }
                if event.keystroke.key == "enter" {
                    let changed = self.submit_goto_line();
                    cx.stop_propagation();
                    if changed {
                        self.reveal_active_cursor();
                        cx.notify();
                    } else {
                        cx.notify();
                    }
                }
            }
        }
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
            let kind = if text.is_empty() {
                EditKind::Delete
            } else {
                EditKind::Insert
            };
            tab.push_undo_snapshot(kind, text.chars().any(char::is_whitespace));
            tab.replace_char_range(range, text);
        }
        self.sync_find_after_edit();
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
            tab.push_undo_snapshot(EditKind::Other, true);
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
        self.sync_find_after_edit();
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
        let row = geometry.rows.iter().rfind(|row| {
            range.start >= row.line_start_char && range.start <= row.logical_end_char
        })?;
        let code_origin_x = element_bounds.left() + code_origin_pad(self.show_gutter);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_active_rust_highlights(cx);

        let active = self.active;
        let show_gutter = self.show_gutter;
        let show_wrap = self.show_wrap;
        let viewport_width = self.tabs[active]
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| px(WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window);
        let revision = self.tabs[active].revision();
        let line_texts = self.tabs[active].lines();
        let total_content_height = {
            let mut cache = self.tabs[active].cache.borrow_mut();
            let layout = ensure_wrap_layout(
                &mut cache,
                line_texts.as_ref(),
                revision,
                viewport_width,
                char_width,
                show_gutter,
                show_wrap,
            );
            buffer_content_height(layout.total_rows)
        };
        let active_tab = &self.tabs[active];
        let syntax_mode = syntax_mode_for_path(active_tab.path.as_ref());
        let buffer = active_tab.buffer.clone();
        let selection = active_tab.selection.clone();
        let cursor_char = active_tab.cursor_char();
        let viewport_scroll = active_tab.scroll.clone();
        let viewport_cache = active_tab.cache.clone();
        let viewport_geometry = active_tab.geometry.clone();
        let focus_handle = self.focus_handle.clone();
        let entity = cx.entity();
        let status = self.status.clone();
        let status_details = self.status_details();
        let vim_mode = self.vim.mode;
        let find_match_label = if self.find.matches.is_empty() {
            "0/0".to_string()
        } else {
            format!("{}/{}", self.find.current + 1, self.find.matches.len())
        };

        div()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::handle_new_tab))
            .on_action(cx.listener(Self::handle_open_file))
            .on_action(cx.listener(Self::handle_save_file))
            .on_action(cx.listener(Self::handle_save_file_as))
            .on_action(cx.listener(Self::handle_close_active_tab))
            .on_action(cx.listener(Self::handle_next_tab))
            .on_action(cx.listener(Self::handle_prev_tab))
            .on_action(cx.listener(Self::handle_toggle_wrap))
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
            .on_action(cx.listener(Self::handle_undo))
            .on_action(cx.listener(Self::handle_redo))
            .on_action(cx.listener(Self::handle_find_open))
            .on_action(cx.listener(Self::handle_find_open_replace))
            .on_action(cx.listener(Self::handle_find_next))
            .on_action(cx.listener(Self::handle_find_prev))
            .on_action(cx.listener(Self::handle_replace_one))
            .on_action(cx.listener(Self::handle_replace_all))
            .on_action(cx.listener(Self::handle_goto_line_open))
            .on_action(cx.listener(Self::handle_delete_line))
            .on_action(cx.listener(Self::handle_move_line_up))
            .on_action(cx.listener(Self::handle_move_line_down))
            .on_action(cx.listener(Self::handle_duplicate_line))
            .on_action(cx.listener(Self::handle_toggle_comment))
            .on_action(cx.listener(Self::handle_quit))
            .size_full()
            .bg(rgb(COLOR_BG))
            .text_color(rgb(COLOR_TEXT))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .bg(rgb(COLOR_SURFACE0))
                    .border_b_1()
                    .border_color(rgb(COLOR_BORDER))
                    .children((0..self.tabs.len()).map(|ix| self.tab_button(ix, cx))),
            )
            .child(
                div()
                    .flex_none()
                    .when(self.find.visible, |container| {
                        container.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_4()
                                .py_2()
                                .bg(rgb(COLOR_SURFACE1))
                                .border_b_1()
                                .border_color(rgb(COLOR_BORDER))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .font_family(".ZedMono")
                                        .text_size(px(12.0))
                                        .child(format!(
                                            "{} find {}",
                                            if self.overlay_focus == Some(OverlayFocus::FindQuery) {
                                                ">"
                                            } else {
                                                " "
                                            },
                                            self.find.query
                                        ))
                                        .when(self.find.show_replace, |column| {
                                            column.child(format!(
                                                "{} replace {}",
                                                if self.overlay_focus
                                                    == Some(OverlayFocus::FindReplace)
                                                {
                                                    ">"
                                                } else {
                                                    " "
                                                },
                                                self.find.replacement
                                            ))
                                        })
                                        .child(format!("matches {find_match_label}")),
                                )
                                .child(
                                    div()
                                        .flex_grow()
                                        .text_right()
                                        .font_family(".ZedMono")
                                        .text_size(px(12.0))
                                        .text_color(rgb(COLOR_MUTED))
                                        .child(if self.find.show_replace {
                                            "enter next  shift+f3 prev  tab field  esc close"
                                        } else {
                                            "enter next  shift+f3 prev  ctrl+h replace  esc close"
                                        }),
                                ),
                        )
                    })
                    .when(self.goto_line.is_some(), |container| {
                        container.child(
                            div()
                                .flex()
                                .justify_between()
                                .gap_3()
                                .px_4()
                                .py_2()
                                .bg(rgb(COLOR_SURFACE1))
                                .border_b_1()
                                .border_color(rgb(COLOR_BORDER))
                                .font_family(".ZedMono")
                                .text_size(px(12.0))
                                .child(format!(
                                    "{} goto {}",
                                    if self.overlay_focus == Some(OverlayFocus::GotoLine) {
                                        ">"
                                    } else {
                                        " "
                                    },
                                    self.goto_line.clone().unwrap_or_default()
                                ))
                                .child(
                                    div()
                                        .flex_grow()
                                        .text_right()
                                        .text_color(rgb(COLOR_MUTED))
                                        .child("enter jump  esc close"),
                                ),
                        )
                    }),
            )
            .child(
                div().flex_grow().px_3().pb_3().child(
                    div()
                        .id("buffer-viewport")
                        .relative()
                        .h_full()
                        .w_full()
                        .border_1()
                        .border_color(rgb(COLOR_BORDER))
                        .bg(rgb(COLOR_SURFACE1))
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
                                .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                                .on_mouse_move(cx.listener(Self::on_mouse_move))
                                .child(
                                    canvas(
                                        move |bounds, window, _cx| {
                                            prepare_viewport_paint_state(
                                                &buffer,
                                                line_texts.as_ref(),
                                                revision,
                                                syntax_mode,
                                                show_gutter,
                                                show_wrap,
                                                &viewport_scroll,
                                                &viewport_cache,
                                                &viewport_geometry,
                                                bounds,
                                                char_width,
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
                                                vim_mode,
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
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .bg(rgb(COLOR_SURFACE0))
                    .border_t_1()
                    .border_color(rgb(COLOR_BORDER))
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .text_color(rgb(COLOR_SUBTEXT))
                            .child(status),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(".ZedMono")
                            .text_size(px(12.0))
                            .text_color(rgb(COLOR_MUTED))
                            .child(status_details),
                    ),
            )
    }
}

fn buffer_content_height(visual_rows: usize) -> Pixels {
    px((visual_rows.max(1) as f32) * ROW_HEIGHT)
}

fn trim_display_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
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

fn autosave_temp_path(path: &PathBuf, revision: u64) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("buffer");
    path.with_file_name(format!(
        ".{file_name}.lst-gpui-autosave-{}-{revision}.tmp",
        process::id()
    ))
}

fn autosave_revision_is_current(tabs: &[EditorTab], path: &PathBuf, revision: u64) -> bool {
    let mut matched: Option<u64> = None;
    for tab in tabs {
        if tab.path.as_ref() != Some(path) {
            continue;
        }
        if matched.is_some() {
            return false;
        }
        matched = Some(tab.revision());
    }
    matched == Some(revision)
}

fn syntax_mode_for_path(path: Option<&PathBuf>) -> SyntaxMode {
    match path
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
    {
        Some("rs") => SyntaxMode::TreeSitterRust,
        _ => SyntaxMode::Plain,
    }
}

fn tree_sitter_color_for_capture(index: usize) -> Option<u32> {
    match TREE_SITTER_CAPTURE_NAMES.get(index).copied() {
        Some("attribute") => Some(COLOR_YELLOW),
        Some("comment") => Some(COLOR_MUTED),
        Some("constant") => Some(COLOR_PEACH),
        Some("constructor") => Some(COLOR_SAPPHIRE),
        Some("escape") => Some(COLOR_PINK),
        Some("function") => Some(COLOR_ACCENT),
        Some("keyword") => Some(COLOR_MAUVE),
        Some("module") => Some(COLOR_LAVENDER),
        Some("number") => Some(COLOR_PEACH),
        Some("operator") => Some(COLOR_SAPPHIRE),
        Some("property") => Some(COLOR_LAVENDER),
        Some("punctuation") => Some(COLOR_BORDER),
        Some("string") => Some(COLOR_GREEN),
        Some("type") => Some(COLOR_YELLOW),
        Some("variable") => None,
        _ => None,
    }
}

fn char_to_byte_index(text: &str, char_ix: usize) -> usize {
    if char_ix == 0 {
        return 0;
    }

    text.char_indices()
        .nth(char_ix)
        .map(|(byte_ix, _)| byte_ix)
        .unwrap_or(text.len())
}

fn push_rust_highlight_span(
    lines: &mut [Vec<SyntaxSpan>],
    line_starts: &[usize],
    display_ends: &[usize],
    mut start: usize,
    end: usize,
    color: u32,
) {
    while start < end {
        let line_ix = line_starts
            .partition_point(|offset| *offset <= start)
            .saturating_sub(1);
        let line_start = line_starts[line_ix];
        let display_end = display_ends[line_ix];
        let next_line_start = line_starts.get(line_ix + 1).copied().unwrap_or(end);
        let visible_end = end.min(display_end);

        if start < visible_end {
            lines[line_ix].push(SyntaxSpan {
                start: start - line_start,
                end: visible_end - line_start,
                color,
            });
        }

        if end <= next_line_start {
            break;
        }
        start = next_line_start;
    }
}

fn highlight_rust_source(
    highlighter: &mut TreeSitterHighlighter,
    source: &str,
) -> Vec<Vec<SyntaxSpan>> {
    let mut line_starts = vec![0usize];
    let mut display_ends = Vec::new();
    let bytes = source.as_bytes();
    let mut ix = 0usize;
    let mut line_start = 0usize;

    while ix < bytes.len() {
        if bytes[ix] == b'\n' {
            let display_end = if ix > line_start && bytes[ix - 1] == b'\r' {
                ix - 1
            } else {
                ix
            };
            display_ends.push(display_end);
            ix += 1;
            line_start = ix;
            line_starts.push(line_start);
            continue;
        }
        ix += 1;
    }
    display_ends.push(source.strip_suffix('\r').map_or(source.len(), str::len));

    let mut lines = vec![Vec::new(); line_starts.len()];
    let Ok(events) =
        highlighter.highlight(&TREE_SITTER_RUST_CONFIG, source.as_bytes(), None, |_| None)
    else {
        return lines;
    };

    let mut stack: Vec<TreeSitterHighlight> = Vec::new();
    for event in events {
        match event {
            Ok(TreeSitterHighlightEvent::HighlightStart(highlight)) => stack.push(highlight),
            Ok(TreeSitterHighlightEvent::HighlightEnd) => {
                let _ = stack.pop();
            }
            Ok(TreeSitterHighlightEvent::Source { start, end }) if start < end => {
                let Some(color) = stack
                    .last()
                    .and_then(|highlight| tree_sitter_color_for_capture(highlight.0))
                else {
                    continue;
                };
                push_rust_highlight_span(
                    &mut lines,
                    &line_starts,
                    &display_ends,
                    start,
                    end,
                    color,
                );
            }
            Ok(TreeSitterHighlightEvent::Source { .. }) => {}
            Err(_) => return vec![Vec::new(); line_starts.len()],
        }
    }

    lines
}

fn line_syntax_spans(
    cache: &mut ViewportCache,
    revision: u64,
    line_ix: usize,
    syntax_mode: SyntaxMode,
) -> Vec<SyntaxSpan> {
    match syntax_mode {
        SyntaxMode::Plain => Vec::new(),
        SyntaxMode::TreeSitterRust => cache
            .rust_highlights
            .as_ref()
            .filter(|highlights| highlights.revision == revision)
            .and_then(|highlights| highlights.lines.get(line_ix))
            .cloned()
            .unwrap_or_default(),
    }
}

fn text_runs_for_segment(
    line_text: &str,
    segment_start_col: usize,
    segment_end_col: usize,
    spans: &[SyntaxSpan],
    base_run: &TextRun,
) -> (Vec<TextRun>, u64) {
    let segment_start = char_to_byte_index(line_text, segment_start_col);
    let segment_end = char_to_byte_index(line_text, segment_end_col);
    let segment_len = segment_end.saturating_sub(segment_start);

    let mut local_spans = Vec::new();
    for span in spans {
        let start = span.start.max(segment_start);
        let end = span.end.min(segment_end);
        if start < end {
            local_spans.push(SyntaxSpan {
                start: start - segment_start,
                end: end - segment_start,
                color: span.color,
            });
        }
    }

    let mut hasher = DefaultHasher::new();
    local_spans.hash(&mut hasher);
    let style_key = hasher.finish();

    let mut runs = Vec::new();
    let mut cursor = 0;
    for span in local_spans {
        if cursor < span.start {
            runs.push(TextRun {
                len: span.start - cursor,
                ..base_run.clone()
            });
        }
        runs.push(TextRun {
            len: span.end - span.start,
            color: rgb(span.color).into(),
            ..base_run.clone()
        });
        cursor = span.end;
    }

    if cursor < segment_len {
        runs.push(TextRun {
            len: segment_len - cursor,
            ..base_run.clone()
        });
    }

    if runs.is_empty() {
        runs.push(TextRun {
            len: segment_len,
            ..base_run.clone()
        });
    }

    (runs, style_key)
}

fn code_origin_pad(show_gutter: bool) -> Pixels {
    if show_gutter {
        px(GUTTER_WIDTH)
    } else {
        px(EDITOR_LEFT_PAD)
    }
}

fn code_char_width(window: &mut Window) -> Pixels {
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let probe = SharedString::from("00000000");
    let shaped = window.text_system().shape_line(
        probe.clone(),
        font_size,
        &[TextRun {
            len: probe.len(),
            font: style.font(),
            color: rgb(COLOR_TEXT).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }],
        None,
    );

    if shaped.width > px(0.0) {
        shaped.width / probe.chars().count() as f32
    } else {
        px(WRAP_CHAR_WIDTH_FALLBACK)
    }
}

fn wrap_columns_for_viewport(
    viewport_width: Pixels,
    line_count: usize,
    char_width: Pixels,
    show_gutter: bool,
    show_wrap: bool,
) -> usize {
    if !show_wrap {
        return usize::MAX;
    }

    wrap_columns_with_gutter(
        viewport_width / px(1.0),
        (char_width / px(1.0)).max(WRAP_CHAR_WIDTH_FALLBACK),
        line_count,
        show_gutter,
        EDITOR_LEFT_PAD,
        EDITOR_RIGHT_PAD,
        GUTTER_LEFT_PAD,
        GUTTER_SEPARATOR_WIDTH,
    )
}

fn ensure_wrap_layout(
    cache: &mut ViewportCache,
    lines: &[String],
    revision: u64,
    viewport_width: Pixels,
    char_width: Pixels,
    show_gutter: bool,
    show_wrap: bool,
) -> WrapLayout {
    let wrap_columns = wrap_columns_for_viewport(
        viewport_width,
        lines.len(),
        char_width,
        show_gutter,
        show_wrap,
    );
    if let Some(layout) = cache.wrap_layout.as_ref() {
        if layout.revision == revision
            && layout.wrap_columns == wrap_columns
            && layout.show_wrap == show_wrap
            && layout.line_row_starts.len() == lines.len() + 1
        {
            return layout.clone();
        }
    }

    cache.code_lines.clear();

    let mut line_row_starts = Vec::with_capacity(lines.len() + 1);
    let mut total_rows = 0usize;
    line_row_starts.push(0);
    for line in lines {
        let display = trim_display_line(line);
        total_rows += if show_wrap {
            visual_line_count(display, wrap_columns)
        } else {
            1
        };
        line_row_starts.push(total_rows);
    }

    let layout = WrapLayout {
        revision,
        show_wrap,
        wrap_columns,
        line_row_starts,
        total_rows: total_rows.max(1),
    };
    cache.wrap_layout = Some(layout.clone());
    layout
}

fn visible_visual_row_range(
    scroll_top: Pixels,
    viewport_height: Pixels,
    total_rows: usize,
) -> Range<usize> {
    let start =
        ((scroll_top / px(ROW_HEIGHT)).floor() as usize).saturating_sub(VIEWPORT_OVERSCAN_LINES);
    let end = (((scroll_top + viewport_height) / px(ROW_HEIGHT)).ceil() as usize)
        .saturating_add(VIEWPORT_OVERSCAN_LINES)
        .min(total_rows.max(1));
    start..end.max(start.saturating_add(1))
}

fn line_for_visual_row(layout: &WrapLayout, visual_row: usize) -> usize {
    layout
        .line_row_starts
        .partition_point(|start| *start <= visual_row)
        .saturating_sub(1)
        .min(layout.line_row_starts.len().saturating_sub(2))
}

fn shape_cached_line(
    cache: &mut HashMap<usize, CachedShapedLine>,
    line_ix: usize,
    text: SharedString,
    style_key: u64,
    base_run: &TextRun,
    font_size: Pixels,
    window: &mut Window,
) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(&line_ix) {
        if cached.text == text && cached.style_key == style_key {
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
            style_key,
            shaped: shaped.clone(),
        },
    );
    Some(shaped)
}

fn shape_cached_segment(
    cache: &mut HashMap<(usize, usize, usize), CachedShapedLine>,
    key: (usize, usize, usize),
    text: SharedString,
    runs: &[TextRun],
    style_key: u64,
    font_size: Pixels,
    window: &mut Window,
) -> Option<ShapedLine> {
    if text.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(&key) {
        if cached.text == text && cached.style_key == style_key {
            return Some(cached.shaped.clone());
        }
    }

    let shaped = window
        .text_system()
        .shape_line(text.clone(), font_size, runs, None);

    cache.insert(
        key,
        CachedShapedLine {
            text,
            style_key,
            shaped: shaped.clone(),
        },
    );
    Some(shaped)
}

fn prepare_viewport_paint_state(
    buffer: &Rope,
    lines: &[String],
    revision: u64,
    syntax_mode: SyntaxMode,
    show_gutter: bool,
    show_wrap: bool,
    viewport_scroll: &ScrollHandle,
    viewport_cache: &Rc<RefCell<ViewportCache>>,
    viewport_geometry: &Rc<RefCell<ViewportGeometry>>,
    bounds: Bounds<Pixels>,
    char_width: Pixels,
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
    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let code_run = TextRun {
        len: 0,
        font: style.font(),
        color: rgb(COLOR_TEXT).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let gutter_run = TextRun {
        len: 0,
        font: style.font(),
        color: rgb(COLOR_MUTED).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    let mut cache = viewport_cache.borrow_mut();
    let layout = ensure_wrap_layout(
        &mut cache,
        lines,
        revision,
        bounds.size.width,
        char_width,
        show_gutter,
        show_wrap,
    );
    let visible_rows = visible_visual_row_range(scroll_top, viewport_height, layout.total_rows);
    let first_line = line_for_visual_row(&layout, visible_rows.start);
    let last_visible_line = line_for_visual_row(&layout, visible_rows.end.saturating_sub(1));
    cache
        .code_lines
        .retain(|(line_ix, _, _), _| *line_ix >= first_line && *line_ix <= last_visible_line);
    cache.gutter_lines.retain(|line_ix, _| {
        show_gutter && *line_ix >= first_line && *line_ix <= last_visible_line
    });

    let mut rows = Vec::new();
    for line_ix in first_line..=last_visible_line {
        let display_source = trim_display_line(&lines[line_ix]);
        let highlight_spans = line_syntax_spans(&mut cache, revision, line_ix, syntax_mode);
        let display_len = display_source.chars().count();
        let logical_end_char = if line_ix + 1 < buffer.len_lines() {
            buffer.line_to_char(line_ix + 1)
        } else {
            buffer.len_chars()
        };
        let line_start_char = buffer.line_to_char(line_ix);
        let segments = if show_wrap {
            wrap_segments(display_source, layout.wrap_columns)
        } else {
            vec![lst_core::wrap::WrappedSegment {
                start_col: 0,
                end_col: display_len,
                text: display_source.to_string(),
            }]
        };
        let segment_count = segments.len();

        for (segment_ix, segment) in segments.into_iter().enumerate() {
            let visual_row = layout.line_row_starts[line_ix] + segment_ix;
            if !visible_rows.contains(&visual_row) {
                continue;
            }

            let row_top = bounds.top() + px((visual_row as f32) * ROW_HEIGHT) - scroll_top;
            let segment_start_char = line_start_char + segment.start_col;
            let segment_end_char = line_start_char + segment.end_col;
            let (code_runs, style_key) = text_runs_for_segment(
                display_source,
                segment.start_col,
                segment.end_col,
                &highlight_spans,
                &code_run,
            );
            let code_line = shape_cached_segment(
                &mut cache.code_lines,
                (line_ix, segment.start_col, segment.end_col),
                SharedString::from(segment.text),
                &code_runs,
                style_key,
                font_size,
                window,
            );
            let gutter_line = if show_gutter && segment_ix == 0 {
                shape_cached_line(
                    &mut cache.gutter_lines,
                    line_ix,
                    SharedString::from(format!("{:>6}", line_ix + 1)),
                    0,
                    &gutter_run,
                    font_size,
                    window,
                )
            } else {
                None
            };

            rows.push(PaintedRow {
                row_top,
                line_start_char: segment_start_char,
                display_end_char: segment_end_char,
                logical_end_char: if segment_ix + 1 == segment_count {
                    logical_end_char
                } else {
                    segment_end_char
                },
                code_line,
                gutter_line,
            });
        }
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
    vim_mode: vim::Mode,
    focused: bool,
    paint_state: ViewportPaintState,
    window: &mut Window,
    cx: &mut App,
) {
    let line_height = window.line_height();
    let gutter_origin_x = bounds.left() + px(GUTTER_LEFT_PAD);
    let gutter_width = px(GUTTER_WIDTH - GUTTER_LEFT_PAD - 8.0);
    let code_origin_x = bounds.left() + code_origin_pad(show_gutter);

    for row in paint_state.rows {
        let cursor_in_row =
            cursor_char >= row.line_start_char && cursor_char <= row.logical_end_char;
        let row_bounds = Bounds::new(
            point(bounds.left(), row.row_top),
            size(bounds.size.width, px(ROW_HEIGHT)),
        );
        window.paint_quad(fill(
            row_bounds,
            if cursor_in_row {
                rgb(COLOR_CURRENT_LINE)
            } else {
                rgb(COLOR_SURFACE1)
            },
        ));

        if show_gutter {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left(), row.row_top),
                    size(px(GUTTER_WIDTH), px(ROW_HEIGHT)),
                ),
                rgb(COLOR_GUTTER),
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
                    rgb(COLOR_SELECTION),
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

        if focused && selection.start == selection.end && cursor_in_row {
            let cursor_x = code_origin_x
                + x_for_global_char(&row, cursor_char.min(row.display_end_char))
                    .unwrap_or_else(|| px(0.0));
            let cursor_width = if vim_mode == vim::Mode::Normal {
                let next_x = code_origin_x
                    + x_for_global_char(
                        &row,
                        (cursor_char + 1).min(row.display_end_char.max(cursor_char + 1)),
                    )
                    .unwrap_or_else(|| cursor_x + px(CODE_FONT_SIZE * 0.55));
                (next_x - cursor_x).max(px(CURSOR_WIDTH * 2.0))
            } else {
                px(CURSOR_WIDTH)
            };
            window.paint_quad(fill(
                Bounds::new(
                    point(cursor_x, row.row_top),
                    size(cursor_width, px(ROW_HEIGHT)),
                ),
                if vim_mode == vim::Mode::Normal {
                    rgb(COLOR_SELECTION)
                } else {
                    rgb(COLOR_CARET)
                },
            ));
        }
    }
}

fn visual_row_for_char(tab: &EditorTab, layout: &WrapLayout) -> Option<usize> {
    let cursor = tab.cursor_char().min(tab.buffer.len_chars());
    let line = tab.buffer.char_to_line(cursor);
    let line_start = tab.buffer.line_to_char(line);
    let display_text = line_display_text(&tab.buffer, line);
    let column = cursor
        .saturating_sub(line_start)
        .min(display_text.chars().count());
    let row_in_line = if layout.show_wrap {
        cursor_visual_row_in_line(display_text.as_ref(), column, layout.wrap_columns)
    } else {
        0
    };
    layout
        .line_row_starts
        .get(line)
        .copied()
        .map(|row| row + row_in_line)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Whitespace,
    Word,
    Symbol,
}

fn token_class(ch: char) -> TokenClass {
    if ch.is_whitespace() {
        TokenClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        TokenClass::Word
    } else {
        TokenClass::Symbol
    }
}

fn word_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let line = buffer.char_to_line(clamped);
    let line_start = buffer.line_to_char(line);
    let display_text = line_display_text(buffer, line);
    let chars: Vec<char> = display_text.chars().collect();
    if chars.is_empty() {
        return clamped..clamped;
    }

    let local = clamped
        .saturating_sub(line_start)
        .min(chars.len().saturating_sub(1));
    let class = token_class(chars[local]);
    let mut start = local;
    while start > 0 && token_class(chars[start - 1]) == class {
        start -= 1;
    }
    let mut end = local + 1;
    while end < chars.len() && token_class(chars[end]) == class {
        end += 1;
    }
    (line_start + start)..(line_start + end)
}

fn line_range_at_char(buffer: &Rope, char_index: usize) -> Range<usize> {
    let clamped = char_index.min(buffer.len_chars());
    let line = buffer.char_to_line(clamped);
    let start = buffer.line_to_char(line);
    let end = if line + 1 < buffer.len_lines() {
        buffer.line_to_char(line + 1)
    } else {
        buffer.len_chars()
    };
    start..end
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

fn preferred_newline_for_buffer(buffer: &Rope) -> &'static str {
    let mut chars = buffer.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                return "\r\n";
            }
            return "\n";
        }
        if ch == '\n' {
            return "\n";
        }
    }
    "\n"
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

fn gpui_modifiers_to_iced(modifiers: gpui::Modifiers) -> IcedModifiers {
    let mut result = IcedModifiers::NONE;
    if modifiers.shift {
        result |= IcedModifiers::SHIFT;
    }
    if modifiers.control {
        result |= IcedModifiers::CTRL;
    }
    if modifiers.alt {
        result |= IcedModifiers::ALT;
    }
    if modifiers.platform {
        result |= IcedModifiers::LOGO;
    }
    result
}

fn gpui_key_to_iced(event: &KeyDownEvent) -> Option<iced_keyboard::Key> {
    if let Some(ch) = event.keystroke.key_char.as_deref() {
        if ch.chars().count() == 1 {
            return Some(iced_keyboard::Key::Character(ch.into()));
        }
    }

    match event.keystroke.key.as_str() {
        "escape" => Some(iced_keyboard::Key::Named(IcedNamed::Escape)),
        "left" => Some(iced_keyboard::Key::Named(IcedNamed::ArrowLeft)),
        "right" => Some(iced_keyboard::Key::Named(IcedNamed::ArrowRight)),
        "up" => Some(iced_keyboard::Key::Named(IcedNamed::ArrowUp)),
        "down" => Some(iced_keyboard::Key::Named(IcedNamed::ArrowDown)),
        "home" => Some(iced_keyboard::Key::Named(IcedNamed::Home)),
        "end" => Some(iced_keyboard::Key::Named(IcedNamed::End)),
        "pageup" => Some(iced_keyboard::Key::Named(IcedNamed::PageUp)),
        "pagedown" => Some(iced_keyboard::Key::Named(IcedNamed::PageDown)),
        "backspace" => Some(iced_keyboard::Key::Named(IcedNamed::Backspace)),
        "delete" => Some(iced_keyboard::Key::Named(IcedNamed::Delete)),
        "tab" => Some(iced_keyboard::Key::Named(IcedNamed::Tab)),
        "enter" => Some(iced_keyboard::Key::Named(IcedNamed::Enter)),
        value if value.chars().count() == 1 => Some(iced_keyboard::Key::Character(value.into())),
        _ => None,
    }
}

fn position_from_vim(position: text_editor::Position) -> Position {
    Position {
        line: position.line,
        column: position.column,
    }
}

fn vim_position_lt(a: text_editor::Position, b: text_editor::Position) -> bool {
    (a.line, a.column) < (b.line, b.column)
}

fn inclusive_position_to_exclusive_char(buffer: &Rope, position: text_editor::Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let display_len = line_display_char_len(buffer, line);
    if display_len == 0 {
        return line_start;
    }
    line_start + (position.column.min(display_len.saturating_sub(1)) + 1).min(display_len)
}

fn extract_text_range(
    lines: &[String],
    from: &text_editor::Position,
    to: &text_editor::Position,
) -> String {
    if from.line >= lines.len() || to.line >= lines.len() {
        return String::new();
    }
    if from.line == to.line {
        let chars: Vec<char> = lines[from.line].chars().collect();
        let start = from.column.min(chars.len());
        let end = (to.column + 1).min(chars.len());
        if start >= end {
            return String::new();
        }
        chars[start..end].iter().collect()
    } else {
        let mut result = String::new();
        let first: Vec<char> = lines[from.line].chars().collect();
        result.extend(&first[from.column.min(first.len())..]);
        for line in lines.iter().take(to.line).skip(from.line + 1) {
            result.push('\n');
            result.push_str(line);
        }
        result.push('\n');
        let last: Vec<char> = lines[to.line].chars().collect();
        result.extend(&last[..(to.column + 1).min(last.len())]);
        result
    }
}

fn remove_text_range(
    lines: &mut Vec<String>,
    from: &text_editor::Position,
    to: &text_editor::Position,
) {
    if from.line >= lines.len() || to.line >= lines.len() {
        return;
    }
    if from.line == to.line {
        let chars: Vec<char> = lines[from.line].chars().collect();
        let start = from.column.min(chars.len());
        let end = (to.column + 1).min(chars.len());
        let remaining: String = chars[..start].iter().chain(chars[end..].iter()).collect();
        lines[from.line] = remaining;
    } else {
        let first: Vec<char> = lines[from.line].chars().collect();
        let last: Vec<char> = lines[to.line].chars().collect();
        let prefix: String = first[..from.column.min(first.len())].iter().collect();
        let suffix: String = last[(to.column + 1).min(last.len())..].iter().collect();
        lines[from.line] = format!("{prefix}{suffix}");
        if from.line < to.line {
            lines.drain((from.line + 1)..=to.line);
        }
    }
}

fn handle_overlay_input(
    keystroke: &gpui::Keystroke,
    text: &mut String,
    cursor: &mut usize,
) -> bool {
    match keystroke.key.as_str() {
        "backspace" => {
            if *cursor == 0 {
                return true;
            }
            let start_char = cursor.saturating_sub(1);
            let start = char_to_byte(text, start_char);
            let end = char_to_byte(text, *cursor);
            text.replace_range(start..end, "");
            *cursor = start_char;
            return true;
        }
        "delete" => {
            let char_count = text.chars().count();
            if *cursor >= char_count {
                return true;
            }
            let start = char_to_byte(text, *cursor);
            let end = char_to_byte(text, *cursor + 1);
            text.replace_range(start..end, "");
            return true;
        }
        "left" => {
            *cursor = cursor.saturating_sub(1);
            return true;
        }
        "right" => {
            *cursor = (*cursor + 1).min(text.chars().count());
            return true;
        }
        "home" => {
            *cursor = 0;
            return true;
        }
        "end" => {
            *cursor = text.chars().count();
            return true;
        }
        _ => {}
    }

    if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
        return false;
    }

    let inserted = keystroke
        .key_char
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "\n" && *value != "\t")
        .map(str::to_string)
        .or_else(|| {
            let value = keystroke.key.as_str();
            (value.chars().count() == 1).then(|| value.to_string())
        });

    let Some(inserted) = inserted else {
        return false;
    };
    let byte = char_to_byte(text, *cursor);
    text.insert_str(byte, &inserted);
    *cursor += inserted.chars().count();
    true
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

fn editor_keybindings() -> Vec<KeyBinding> {
    vec![
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
        KeyBinding::new("alt-z", ToggleWrap, None),
        KeyBinding::new("ctrl-c", CopySelection, None),
        KeyBinding::new("cmd-c", CopySelection, None),
        KeyBinding::new("ctrl-x", CutSelection, None),
        KeyBinding::new("cmd-x", CutSelection, None),
        KeyBinding::new("ctrl-v", PasteClipboard, None),
        KeyBinding::new("cmd-v", PasteClipboard, None),
        KeyBinding::new("ctrl-z", Undo, None),
        KeyBinding::new("cmd-z", Undo, None),
        KeyBinding::new("ctrl-y", Redo, None),
        KeyBinding::new("cmd-shift-z", Redo, None),
        KeyBinding::new("ctrl-f", FindOpen, None),
        KeyBinding::new("cmd-f", FindOpen, None),
        KeyBinding::new("ctrl-h", FindOpenReplace, None),
        KeyBinding::new("cmd-h", FindOpenReplace, None),
        KeyBinding::new("f3", FindNext, None),
        KeyBinding::new("shift-f3", FindPrev, None),
        KeyBinding::new("ctrl-g", GotoLineOpen, None),
        KeyBinding::new("cmd-g", GotoLineOpen, None),
        KeyBinding::new("alt-up", MoveLineUp, None),
        KeyBinding::new("alt-down", MoveLineDown, None),
        KeyBinding::new("ctrl-shift-k", DeleteLine, None),
        KeyBinding::new("cmd-shift-k", DeleteLine, None),
        KeyBinding::new("ctrl-shift-d", DuplicateLine, None),
        KeyBinding::new("cmd-shift-d", DuplicateLine, None),
        KeyBinding::new("ctrl-/", ToggleComment, None),
        KeyBinding::new("cmd-/", ToggleComment, None),
        KeyBinding::new("left", MoveLeft, None),
        KeyBinding::new("right", MoveRight, None),
        KeyBinding::new("up", MoveUp, None),
        KeyBinding::new("down", MoveDown, None),
        KeyBinding::new("shift-left", SelectLeft, None),
        KeyBinding::new("shift-right", SelectRight, None),
        KeyBinding::new("shift-up", SelectUp, None),
        KeyBinding::new("shift-down", SelectDown, None),
        KeyBinding::new("ctrl-up", SelectUp, None),
        KeyBinding::new("ctrl-down", SelectDown, None),
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
    ]
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
        cx.bind_keys(editor_keybindings());

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
                view.start_background_tasks(window, cx);
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;

    fn has_binding<A: gpui::Action + 'static>(keystroke: &str) -> bool {
        let typed = [Keystroke::parse(keystroke).expect("valid test keystroke")];
        editor_keybindings().iter().any(|binding| {
            binding.match_keystrokes(&typed) == Some(false) && binding.action().as_any().is::<A>()
        })
    }

    #[test]
    fn autosave_revision_requires_a_unique_matching_tab() {
        let path = PathBuf::from("/tmp/example.rs");
        let tab = EditorTab::from_path(path.clone(), "fn main() {}\n");

        assert!(autosave_revision_is_current(&[tab], &path, 0));

        let mut stale_tab = EditorTab::from_path(path.clone(), "fn main() {}\n");
        stale_tab.replace_char_range(0..0, "// ");
        assert!(!autosave_revision_is_current(&[stale_tab], &path, 0));

        let first = EditorTab::from_path(path.clone(), "one\n");
        let second = EditorTab::from_path(path.clone(), "two\n");
        assert!(!autosave_revision_is_current(&[first, second], &path, 0));
    }

    #[test]
    fn rust_highlighting_keeps_multiline_comment_context() {
        let mut highlighter = TreeSitterHighlighter::new();
        let lines = highlight_rust_source(
            &mut highlighter,
            "/* first line\nsecond line */\nlet x = 1;\n",
        );

        assert!(lines[0].iter().any(|span| span.color == COLOR_MUTED));
        assert!(lines[1].iter().any(|span| span.color == COLOR_MUTED));
        assert!(lines[2].iter().all(|span| span.color != COLOR_MUTED));
    }

    #[test]
    fn drag_selection_range_extends_forward_from_anchor_token() {
        let (selection, reversed) = drag_selection_range(6..11, 12..17);

        assert_eq!(selection, 6..17);
        assert!(!reversed);
    }

    #[test]
    fn drag_selection_range_extends_backward_from_anchor_token() {
        let (selection, reversed) = drag_selection_range(6..11, 0..5);

        assert_eq!(selection, 0..11);
        assert!(reversed);
    }

    #[test]
    fn ctrl_arrow_aliases_expand_vertical_selection() {
        assert!(has_binding::<SelectUp>("ctrl-up"));
        assert!(has_binding::<SelectDown>("ctrl-down"));
    }
}
