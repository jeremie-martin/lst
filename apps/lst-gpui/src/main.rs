use gpui::{
    actions, point, prelude::*, px, size, App, Application, Bounds, ClipboardItem, Context, Entity,
    EntityInputHandler, FocusHandle, Focusable, KeyDownEvent, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollHandle, Subscription, UTF16Selection, Window, WindowBounds,
    WindowOptions,
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

mod keymap;
mod launch;
mod shell;
mod syntax;
#[cfg(test)]
mod tests;
mod viewport;
#[path = "../../../src/vim.rs"]
mod vim;

use iced::{
    keyboard::{self as iced_keyboard, key::Named as IcedNamed, Modifiers as IcedModifiers},
    widget::text_editor,
};
use keymap::editor_keybindings;
use launch::{parse_launch_args, AutoBench, BenchAction, LaunchArgs};
use lst_core::{
    document::{char_to_position, position_to_char, EditKind, Tab},
    editor_ops,
    find::FindState,
    position::Position,
    wrap::{cursor_visual_row_in_line, wrap_segments},
};
use lst_ui::{input_keybindings, InputField, InputFieldEvent};
use rfd::FileDialog;
use ropey::Rope;
use std::{
    cell::RefCell,
    collections::HashSet,
    fs,
    ops::{Deref, DerefMut, Range},
    path::{Path, PathBuf},
    process,
    rc::Rc,
    time::{Duration, Instant},
};
use syntax::{
    compute_syntax_highlights, syntax_mode_for_path, CachedSyntaxHighlights, SyntaxHighlightJobKey,
    SyntaxMode, SyntaxSpan,
};
use viewport::{
    byte_index_to_char, code_char_width, code_origin_pad, ensure_wrap_layout,
    line_display_char_len, line_display_text, line_for_visual_row, row_contains_cursor,
    visual_row_for_char, x_for_global_char, ViewportCache, ViewportGeometry,
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
enum PendingFocus {
    Editor,
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

#[derive(Clone, Debug)]
struct AutosaveJob {
    path: PathBuf,
    body: String,
    revision: u64,
}

#[derive(Clone, Debug)]
enum DragSelectionMode {
    Character,
    Word(Range<usize>),
    Line(Range<usize>),
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
    tab_bar_scroll: ScrollHandle,
    hovered_tab: Option<usize>,
    show_gutter: bool,
    show_wrap: bool,
    drag_selecting: Option<DragSelectionMode>,
    find: FindState,
    goto_line: Option<String>,
    find_query_input: Entity<InputField>,
    find_replace_input: Entity<InputField>,
    goto_line_input: Entity<InputField>,
    pending_focus: Option<PendingFocus>,
    status: String,
    last_operation: OperationStats,
    vim: vim::VimState,
    autosave_inflight: HashSet<PathBuf>,
    autosave_started: bool,
    _shell_subscriptions: Vec<Subscription>,
}

impl LstGpuiApp {
    fn new(cx: &mut Context<Self>, launch: LaunchArgs) -> Self {
        let mut tabs = Vec::new();
        let mut status = "Ready.".to_string();
        let find_query_input = cx.new(|cx| InputField::new(cx, "Find"));
        let find_replace_input = cx.new(|cx| InputField::new(cx, "Replace"));
        let goto_line_input = cx.new(|cx| InputField::new(cx, "Line"));

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

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            tabs,
            active,
            next_untitled_id: 2,
            tab_bar_scroll: ScrollHandle::new(),
            hovered_tab: None,
            show_gutter: true,
            show_wrap: true,
            drag_selecting: None,
            find: FindState::new(),
            goto_line: None,
            find_query_input: find_query_input.clone(),
            find_replace_input: find_replace_input.clone(),
            goto_line_input: goto_line_input.clone(),
            pending_focus: None,
            status,
            last_operation,
            vim: vim::VimState::new(),
            autosave_inflight: HashSet::new(),
            autosave_started: false,
            _shell_subscriptions: Vec::new(),
        };

        app._shell_subscriptions.push(
            cx.subscribe(&find_query_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_find_query_input_event(event, cx)
            }),
        );
        app._shell_subscriptions.push(cx.subscribe(
            &find_replace_input,
            |this, _, event: &InputFieldEvent, cx| this.handle_find_replace_input_event(event, cx),
        ));
        app._shell_subscriptions.push(
            cx.subscribe(&goto_line_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_goto_line_input_event(event, cx)
            }),
        );

        app
    }

    fn queue_focus(&mut self, target: PendingFocus) {
        self.pending_focus = Some(target);
    }

    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.pending_focus.take() else {
            return;
        };

        match target {
            PendingFocus::Editor => window.focus(&self.focus_handle),
            PendingFocus::FindQuery => {
                let handle = self.find_query_input.read(cx).focus_handle();
                window.focus(&handle);
            }
            PendingFocus::FindReplace => {
                if self.find.show_replace {
                    let handle = self.find_replace_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(PendingFocus::FindQuery);
                }
            }
            PendingFocus::GotoLine => {
                if self.goto_line.is_some() {
                    let handle = self.goto_line_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(PendingFocus::Editor);
                }
            }
        }
    }

    fn handle_find_query_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.find.query = text.clone();
                self.reindex_find_matches_to_nearest();
                self.select_current_find_match();
                self.reveal_active_cursor();
                cx.notify();
            }
            InputFieldEvent::Submitted => {
                if self.find_next() {
                    self.reveal_active_cursor();
                    cx.notify();
                }
            }
            InputFieldEvent::Cancelled => {
                self.close_find();
                self.queue_focus(PendingFocus::Editor);
                cx.notify();
            }
            InputFieldEvent::NextRequested => {
                if self.find.show_replace {
                    self.queue_focus(PendingFocus::FindReplace);
                    cx.notify();
                }
            }
            InputFieldEvent::PreviousRequested => {}
        }
    }

    fn handle_find_replace_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.find.replacement = text.clone();
                cx.notify();
            }
            InputFieldEvent::Submitted => {
                if self.replace_one() {
                    self.reveal_active_cursor();
                    cx.notify();
                }
            }
            InputFieldEvent::Cancelled => {
                self.close_find();
                self.queue_focus(PendingFocus::Editor);
                cx.notify();
            }
            InputFieldEvent::NextRequested => {}
            InputFieldEvent::PreviousRequested => {
                self.queue_focus(PendingFocus::FindQuery);
                cx.notify();
            }
        }
    }

    fn handle_goto_line_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.goto_line = Some(text.clone());
                cx.notify();
            }
            InputFieldEvent::Submitted => {
                let changed = self.submit_goto_line();
                self.queue_focus(PendingFocus::Editor);
                if changed {
                    self.reveal_active_cursor();
                }
                cx.notify();
            }
            InputFieldEvent::Cancelled => {
                self.close_goto_line();
                self.queue_focus(PendingFocus::Editor);
                cx.notify();
            }
            InputFieldEvent::NextRequested | InputFieldEvent::PreviousRequested => {}
        }
    }

    fn ensure_active_syntax_highlights(&mut self, cx: &mut Context<Self>) {
        let active = self.active;
        let Some(tab) = self.tabs.get(active) else {
            return;
        };
        let SyntaxMode::TreeSitter(language) = syntax_mode_for_path(tab.path.as_ref()) else {
            return;
        };

        let revision = tab.revision();
        let key = SyntaxHighlightJobKey { language, revision };
        let cache = tab.cache.clone();
        {
            let cache_ref = cache.borrow();
            if cache_ref
                .syntax_highlights
                .as_ref()
                .is_some_and(|highlights| {
                    highlights.revision == revision && highlights.language == language
                })
            {
                return;
            }
            if cache_ref.syntax_highlight_inflight == Some(key) {
                return;
            }
        }

        cache.borrow_mut().syntax_highlight_inflight = Some(key);
        let source = tab.buffer_text();
        cx.spawn(async move |this, cx| {
            let lines = cx
                .background_executor()
                .spawn(async move { compute_syntax_highlights(language, &source) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.finish_syntax_highlights(active, key, cache, lines, cx);
            });
        })
        .detach();
    }

    fn finish_syntax_highlights(
        &mut self,
        active: usize,
        key: SyntaxHighlightJobKey,
        cache: Rc<RefCell<ViewportCache>>,
        lines: Vec<Vec<SyntaxSpan>>,
        cx: &mut Context<Self>,
    ) {
        let mut cache_ref = cache.borrow_mut();
        if cache_ref.syntax_highlight_inflight != Some(key) {
            return;
        }

        cache_ref.syntax_highlight_inflight = None;
        if !syntax_highlight_result_is_current(&self.tabs, active, &cache, key) {
            return;
        }

        cache_ref.syntax_highlights = Some(CachedSyntaxHighlights {
            language: key.language,
            revision: key.revision,
            lines,
        });
        cache_ref.clear_code_lines();
        drop(cache_ref);

        if self.active == active {
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

    fn open_find(&mut self, show_replace: bool, cx: &mut Context<Self>) {
        self.find.visible = true;
        self.find.show_replace = show_replace;
        if let Some(sel) = self.selected_single_line_text() {
            self.find.query = sel;
        }
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        self.find_query_input
            .update(cx, |input, cx| input.set_text(&query, cx));
        self.find_replace_input
            .update(cx, |input, cx| input.set_text(&replacement, cx));
        self.queue_focus(PendingFocus::FindQuery);
        self.reindex_find_matches_to_nearest();
    }

    fn close_find(&mut self) {
        self.find.visible = false;
    }

    fn open_goto_line(&mut self, cx: &mut Context<Self>) {
        self.goto_line = Some(String::new());
        self.goto_line_input
            .update(cx, |input, cx| input.set_text("", cx));
        self.queue_focus(PendingFocus::GotoLine);
    }

    fn close_goto_line(&mut self) {
        self.goto_line = None;
    }

    fn find_next(&mut self) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.next();
        self.select_current_find_match()
    }

    fn find_prev(&mut self) -> bool {
        self.ensure_find_matches_current();
        if self.find.matches.is_empty() {
            return false;
        }
        self.find.prev();
        self.select_current_find_match()
    }

    fn replace_one(&mut self) -> bool {
        self.ensure_find_matches_current();
        let Some((start, end)) = self.find.current_match_range() else {
            return false;
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
        true
    }

    fn replace_all_matches(&mut self) -> bool {
        if self.find.query.is_empty() {
            return false;
        }
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        let cursor = self.active_cursor_position();
        self.apply_line_edit(|lines| {
            let new_lines: Vec<String> = lines
                .iter()
                .map(|line| line.replace(&query, &replacement))
                .collect();
            if new_lines == *lines {
                return None;
            }
            *lines = new_lines;
            Some(((), cursor.line, cursor.column))
        })
        .is_some()
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

    fn close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }

        if self.tabs[index].modified {
            self.status = format!(
                "Unsaved changes in {}. Save or Save As before closing this tab.",
                self.tabs[index].display_name()
            );
            cx.notify();
            return;
        }

        let closed_active_tab = should_refocus_editor_after_tab_close(self.active, index);
        self.hovered_tab = None;
        if self.tabs.len() == 1 {
            self.tabs[0] = self.new_empty_tab();
            self.set_active_tab(0);
        } else {
            self.tabs.remove(index);
            let next_active = if index < self.active {
                self.active.saturating_sub(1)
            } else {
                self.active.min(self.tabs.len().saturating_sub(1))
            };
            self.set_active_tab(next_active);
        }

        if closed_active_tab {
            self.queue_focus(PendingFocus::Editor);
        }
        self.status = "Closed tab.".to_string();
        self.reveal_active_cursor();
        cx.notify();
    }

    fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, cx);
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
            self.queue_focus(PendingFocus::Editor);
        } else {
            self.open_find(false, cx);
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
            self.queue_focus(PendingFocus::Editor);
        } else {
            self.open_find(true, cx);
        }
        cx.notify();
    }

    fn handle_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.find_next() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_find_prev(&mut self, _: &FindPrev, _: &mut Window, cx: &mut Context<Self>) {
        if self.find_prev() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_replace_one(&mut self, _: &ReplaceOne, _: &mut Window, cx: &mut Context<Self>) {
        if self.replace_one() {
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_replace_all(&mut self, _: &ReplaceAll, _: &mut Window, cx: &mut Context<Self>) {
        if self.replace_all_matches() {
            self.reindex_find_matches_to_nearest();
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_goto_line_open(&mut self, _: &GotoLineOpen, _: &mut Window, cx: &mut Context<Self>) {
        if self.goto_line.is_some() {
            self.close_goto_line();
            self.queue_focus(PendingFocus::Editor);
        } else {
            self.open_goto_line(cx);
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
                vim::VimCommand::OpenFind => self.open_find(false, cx),
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
        let row = geometry
            .rows
            .iter()
            .rfind(|row| row_contains_cursor(row, range.start))?;
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

fn trim_display_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

fn autosave_temp_path(path: &Path, revision: u64) -> PathBuf {
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

fn syntax_highlight_result_is_current(
    tabs: &[EditorTab],
    active: usize,
    cache: &Rc<RefCell<ViewportCache>>,
    key: SyntaxHighlightJobKey,
) -> bool {
    tabs.get(active).is_some_and(|tab| {
        Rc::ptr_eq(&tab.cache, cache)
            && tab.revision() == key.revision
            && syntax_mode_for_path(tab.path.as_ref()) == SyntaxMode::TreeSitter(key.language)
    })
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

fn should_refocus_editor_after_tab_close(active_index: usize, closed_index: usize) -> bool {
    active_index == closed_index
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

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
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
        cx.bind_keys(input_keybindings());

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
                        view.update(cx, |view, cx| {
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
