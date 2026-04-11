use gpui::{
    actions, point, prelude::*, px, size, App, Application, Bounds, ClipboardItem, Context, Entity,
    EntityInputHandler, FocusHandle, Focusable, KeyDownEvent, Pixels, Point, ScrollHandle,
    Subscription, UTF16Selection, Window, WindowBounds, WindowOptions,
};

mod bench_trace;
mod interactions;
mod keymap;
mod launch;
mod shell;
mod syntax;
#[cfg(test)]
mod tests;
mod viewport;

#[cfg(test)]
pub(crate) use interactions::drag_autoscroll_delta;
use interactions::DragSelectionMode;
use keymap::editor_keybindings;
use launch::{parse_launch_args, AutoBench, BenchAction, LaunchArgs};
#[cfg(test)]
pub(crate) use lst_core::selection::{
    drag_selection_range, line_range_at_char, word_range_at_char,
};
pub(crate) use lst_core::selection::{next_word_boundary, previous_word_boundary};
use lst_core::{
    document::{char_to_position, position_to_char, EditKind, Tab, UndoBoundary},
    editor_ops,
    find::FindState,
    position::Position,
    wrap::{cursor_visual_row_in_line, wrap_segments},
};
use lst_editor::{
    next_active_after_tab_close, should_refocus_editor_after_tab_close,
    vim::{self, Key as VimKey, Modifiers as VimModifiers, NamedKey as VimNamedKey},
    EditorCommand, EditorEffect, EditorModel, EditorTab as ModelEditorTab, FocusTarget, TabId,
    UNTITLED_PREFIX,
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
    line_display_char_len, line_for_visual_row, row_contains_cursor, visual_row_for_char,
    x_for_global_char, ViewportCache, ViewportGeometry,
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
        MoveWordLeft,
        MoveWordRight,
        MovePageUp,
        MovePageDown,
        MoveDocumentStart,
        MoveDocumentEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectPageUp,
        SelectPageDown,
        SelectDocumentStart,
        SelectDocumentEnd,
        MoveLineStart,
        MoveLineEnd,
        SelectLineStart,
        SelectLineEnd,
        Backspace,
        DeleteForward,
        DeleteWordBackward,
        DeleteWordForward,
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

struct EditorTab {
    model: ModelEditorTab,
    scroll: ScrollHandle,
    cache: Rc<RefCell<ViewportCache>>,
    geometry: Rc<RefCell<ViewportGeometry>>,
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
        Self::from_model(ModelEditorTab::from_doc(TabId::from_raw(0), doc))
    }

    fn from_model(model: ModelEditorTab) -> Self {
        Self {
            model,
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(ViewportCache::default())),
            geometry: Rc::new(RefCell::new(ViewportGeometry::default())),
        }
    }

    fn invalidate_visual_state(&mut self) {
        *self.cache.borrow_mut() = ViewportCache::default();
        *self.geometry.borrow_mut() = ViewportGeometry::default();
    }

    fn move_to(&mut self, offset: usize) {
        self.model.move_to(offset);
    }

    fn select_to(&mut self, offset: usize) {
        self.model.select_to(offset);
    }

    fn replace_char_range(&mut self, range: Range<usize>, new_text: &str) -> usize {
        let new_cursor = self.model.replace_char_range(range, new_text);
        self.invalidate_visual_state();
        new_cursor
    }

    fn edit(
        &mut self,
        kind: EditKind,
        boundary: UndoBoundary,
        range: Range<usize>,
        new_text: &str,
    ) -> usize {
        let new_cursor = self.model.edit(kind, boundary, range, new_text);
        self.invalidate_visual_state();
        new_cursor
    }

    fn set_text(&mut self, text: &str) {
        self.model.set_text(text);
        self.invalidate_visual_state();
    }

    fn display_line_char_len(&self, line_ix: usize) -> usize {
        line_display_char_len(&self.buffer, line_ix)
    }

    fn undo(&mut self) -> bool {
        let changed = self.model.undo();
        if changed {
            self.invalidate_visual_state();
        }
        changed
    }

    fn redo(&mut self) -> bool {
        let changed = self.model.redo();
        if changed {
            self.invalidate_visual_state();
        }
        changed
    }
}

impl Deref for EditorTab {
    type Target = ModelEditorTab;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

impl DerefMut for EditorTab {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.model
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
    drag_last_point: Option<Point<Pixels>>,
    drag_autoscroll_active: bool,
    find: FindState,
    goto_line: Option<String>,
    find_query_input: Entity<InputField>,
    find_replace_input: Entity<InputField>,
    goto_line_input: Entity<InputField>,
    pending_focus: Option<FocusTarget>,
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
            drag_last_point: None,
            drag_autoscroll_active: false,
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

    fn queue_focus(&mut self, target: FocusTarget) {
        bench_trace::record_label("focus_queued", focus_trace_label(target));
        self.pending_focus = Some(target);
    }

    fn apply_pending_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.pending_focus.take() else {
            return;
        };

        match target {
            FocusTarget::Editor => window.focus(&self.focus_handle),
            FocusTarget::FindQuery => {
                let handle = self.find_query_input.read(cx).focus_handle();
                window.focus(&handle);
            }
            FocusTarget::FindReplace => {
                if self.find.show_replace {
                    let handle = self.find_replace_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(FocusTarget::FindQuery);
                }
            }
            FocusTarget::GotoLine => {
                if self.goto_line.is_some() {
                    let handle = self.goto_line_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(FocusTarget::Editor);
                }
            }
        }
        bench_trace::record_label("focus_applied", focus_trace_label(target));
    }

    fn to_editor_model(&self) -> EditorModel {
        let tabs = self.tabs.iter().map(|tab| tab.model.clone()).collect();
        let mut model = EditorModel::new(tabs, self.status.clone());
        model.active = self.active.min(model.tabs.len().saturating_sub(1));
        model.next_untitled_id = self.next_untitled_id;
        model.show_gutter = self.show_gutter;
        model.show_wrap = self.show_wrap;
        model.find = self.find.clone();
        model.goto_line = self.goto_line.clone();
        model
    }

    fn sync_from_editor_model(&mut self, model: EditorModel) {
        let old_show_wrap = self.show_wrap;
        let new_len = model.tabs.len();
        for (ix, model_tab) in model.tabs.into_iter().enumerate() {
            if ix < self.tabs.len() {
                let changed_revision = self.tabs[ix].revision() != model_tab.revision();
                self.tabs[ix].model = model_tab;
                if changed_revision || old_show_wrap != model.show_wrap {
                    self.tabs[ix].invalidate_visual_state();
                }
            } else {
                self.tabs.push(EditorTab::from_model(model_tab));
            }
        }
        self.tabs.truncate(new_len);
        self.active = model.active.min(self.tabs.len().saturating_sub(1));
        self.next_untitled_id = model.next_untitled_id;
        self.show_gutter = model.show_gutter;
        self.show_wrap = model.show_wrap;
        self.find = model.find;
        self.goto_line = model.goto_line;
        self.status = model.status;
    }

    fn apply_model_command(&mut self, command: EditorCommand, cx: &mut Context<Self>) {
        let sync_find_inputs = matches!(
            &command,
            EditorCommand::OpenFind { .. } | EditorCommand::ToggleFind { .. }
        );
        let sync_goto_input = matches!(
            &command,
            EditorCommand::OpenGotoLine | EditorCommand::ToggleGotoLine
        );
        let mut model = self.to_editor_model();
        model.apply(command);
        let effects = model.drain_effects();
        self.sync_from_editor_model(model);
        if sync_find_inputs {
            self.sync_find_inputs(cx);
        }
        if sync_goto_input {
            self.sync_goto_input(cx);
        }
        self.handle_model_effects(effects, cx);
        cx.notify();
    }

    fn sync_find_inputs(&mut self, cx: &mut Context<Self>) {
        let query = self.find.query.clone();
        let replacement = self.find.replacement.clone();
        self.find_query_input
            .update(cx, |input, cx| input.set_text(&query, cx));
        self.find_replace_input
            .update(cx, |input, cx| input.set_text(&replacement, cx));
    }

    fn sync_goto_input(&mut self, cx: &mut Context<Self>) {
        let text = self.goto_line.clone().unwrap_or_default();
        self.goto_line_input
            .update(cx, |input, cx| input.set_text(&text, cx));
    }

    fn handle_model_effects(&mut self, effects: Vec<EditorEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                EditorEffect::Focus(target) => self.queue_focus(target),
                EditorEffect::RevealCursor => self.reveal_active_cursor(),
                EditorEffect::WriteClipboard(text) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                EditorEffect::WritePrimary(text) => {
                    cx.write_to_primary(ClipboardItem::new_string(text));
                }
                EditorEffect::ReadClipboard => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.apply_model_command(EditorCommand::PasteText(text), cx);
                    } else {
                        self.status =
                            "Clipboard does not currently contain plain text.".to_string();
                    }
                }
                EditorEffect::OpenFiles => self.open_files_from_dialog(cx),
                EditorEffect::SaveFile { path, body } => {
                    let command = match fs::write(&path, body) {
                        Ok(()) => EditorCommand::SaveFinished { path },
                        Err(err) => EditorCommand::SaveFailed {
                            path,
                            message: err.to_string(),
                        },
                    };
                    self.apply_model_command(command, cx);
                }
                EditorEffect::SaveFileAs {
                    suggested_name,
                    body,
                } => {
                    let Some(path) = FileDialog::new().set_file_name(&suggested_name).save_file()
                    else {
                        continue;
                    };
                    let command = match fs::write(&path, body) {
                        Ok(()) => EditorCommand::SaveFinished { path },
                        Err(err) => EditorCommand::SaveFailed {
                            path,
                            message: err.to_string(),
                        },
                    };
                    self.apply_model_command(command, cx);
                }
                EditorEffect::AutosaveFile { .. } => {}
            }
        }
    }

    fn open_files_from_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = FileDialog::new().pick_files() else {
            return;
        };
        let mut opened = Vec::new();
        for path in paths {
            match fs::read_to_string(&path) {
                Ok(text) => opened.push((path, text)),
                Err(err) => self.apply_model_command(
                    EditorCommand::OpenFileFailed {
                        path,
                        message: err.to_string(),
                    },
                    cx,
                ),
            }
        }
        if !opened.is_empty() {
            self.apply_model_command(EditorCommand::OpenFiles(opened), cx);
        }
    }

    fn handle_find_query_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.apply_model_command(EditorCommand::SetFindQueryAndSelect(text.clone()), cx);
            }
            InputFieldEvent::Submitted => {
                self.apply_model_command(EditorCommand::FindNext, cx);
            }
            InputFieldEvent::Cancelled => {
                self.apply_model_command(EditorCommand::CloseFind, cx);
            }
            InputFieldEvent::NextRequested => {
                if self.find.show_replace {
                    self.queue_focus(FocusTarget::FindReplace);
                    cx.notify();
                }
            }
            InputFieldEvent::PreviousRequested => {}
        }
    }

    fn handle_find_replace_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.apply_model_command(EditorCommand::SetFindReplacement(text.clone()), cx);
            }
            InputFieldEvent::Submitted => {
                self.apply_model_command(EditorCommand::ReplaceOne, cx);
            }
            InputFieldEvent::Cancelled => {
                self.apply_model_command(EditorCommand::CloseFind, cx);
            }
            InputFieldEvent::NextRequested => {}
            InputFieldEvent::PreviousRequested => {
                self.queue_focus(FocusTarget::FindQuery);
                cx.notify();
            }
        }
    }

    fn handle_goto_line_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.apply_model_command(EditorCommand::SetGotoLine(text.clone()), cx);
            }
            InputFieldEvent::Submitted => {
                self.apply_model_command(EditorCommand::SubmitGotoLine, cx);
            }
            InputFieldEvent::Cancelled => {
                self.apply_model_command(EditorCommand::CloseGotoLine, cx);
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

    fn vim_cursor_position(&self) -> Position {
        let cursor = self.active_cursor_position();
        Position {
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
            bench_trace::record_ms("find_reindex_ms", 0.0);
            bench_trace::record_usize("find_match_count", 0);
            bench_trace::record_usize("find_query_len", 0);
            return;
        }
        let text = self.active_tab().buffer.to_string();
        let reindex_started = Instant::now();
        self.find.compute_matches_in_text(&text);
        bench_trace::record_ms("find_reindex_ms", elapsed_ms(reindex_started));
        bench_trace::record_usize("find_match_count", self.find.matches.len());
        bench_trace::record_usize("find_query_len", self.find.query.chars().count());
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
        self.queue_focus(FocusTarget::FindQuery);
        self.reindex_find_matches_to_nearest();
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

        self.tabs[self.active].push_undo_snapshot(EditKind::Other, UndoBoundary::Break);
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
        bench_trace::record_operation(
            label,
            self.last_operation.bytes,
            self.last_operation.lines,
            clipboard_read_ms,
            apply_ms,
        );
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

    fn active_page_rows(&self) -> usize {
        let height = self
            .active_tab()
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.height)
            .filter(|height| *height > px(0.0))
            .unwrap_or_else(|| px(WINDOW_HEIGHT));
        ((height / px(ROW_HEIGHT)) as usize)
            .saturating_sub(2)
            .max(1)
    }

    fn move_page(&mut self, down: bool, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.active_page_rows() as isize;
        let delta = if down { rows } else { -rows };
        self.move_vertical(delta, select, window, cx);
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
            let next_active = next_active_after_tab_close(self.tabs.len(), self.active, index);
            self.tabs.remove(index);
            self.set_active_tab(next_active);
        }

        if closed_active_tab {
            self.queue_focus(FocusTarget::Editor);
        }
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
        self.apply_model_command(EditorCommand::NewTab, cx);
    }

    fn handle_open_file(&mut self, _: &OpenFile, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::RequestOpenFiles, cx);
    }

    fn handle_save_file(&mut self, _: &SaveFile, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::RequestSave, cx);
    }

    fn handle_save_file_as(&mut self, _: &SaveFileAs, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::RequestSaveAs, cx);
    }

    fn handle_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::CloseTab(self.active), cx);
    }

    fn handle_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::NextTab, cx);
    }

    fn handle_prev_tab(&mut self, _: &PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::PrevTab, cx);
    }

    fn handle_toggle_wrap(&mut self, _: &ToggleWrap, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::ToggleWrap, cx);
    }

    fn handle_copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::CopySelection, cx);
    }

    fn handle_cut_selection(&mut self, _: &CutSelection, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::CutSelection, cx);
    }

    fn handle_paste_clipboard(
        &mut self,
        _: &PasteClipboard,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::RequestPaste, cx);
    }

    fn handle_move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab().has_selection() {
            self.apply_model_command(
                EditorCommand::MoveHorizontal {
                    delta: -1,
                    select: false,
                },
                cx,
            );
        } else {
            let start = self.active_tab().selection.start;
            self.active_tab_mut().move_to(start);
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab().has_selection() {
            self.apply_model_command(
                EditorCommand::MoveHorizontal {
                    delta: 1,
                    select: false,
                },
                cx,
            );
        } else {
            let end = self.active_tab().selection.end;
            self.active_tab_mut().move_to(end);
            self.reveal_active_cursor();
            cx.notify();
        }
    }

    fn handle_move_word_left(&mut self, _: &MoveWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: true,
                select: false,
            },
            cx,
        );
    }

    fn handle_move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: false,
                select: false,
            },
            cx,
        );
    }

    fn handle_move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, false, window, cx);
    }

    fn handle_move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, false, window, cx);
    }

    fn handle_move_page_up(&mut self, _: &MovePageUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_page(false, false, window, cx);
    }

    fn handle_move_page_down(
        &mut self,
        _: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(true, false, window, cx);
    }

    fn handle_move_document_start(
        &mut self,
        _: &MoveDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: false,
                select: false,
            },
            cx,
        );
    }

    fn handle_move_document_end(
        &mut self,
        _: &MoveDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: true,
                select: false,
            },
            cx,
        );
    }

    fn handle_select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(
            EditorCommand::MoveHorizontal {
                delta: -1,
                select: true,
            },
            cx,
        );
    }

    fn handle_select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(
            EditorCommand::MoveHorizontal {
                delta: 1,
                select: true,
            },
            cx,
        );
    }

    fn handle_select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: true,
                select: true,
            },
            cx,
        );
    }

    fn handle_select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveWord {
                backward: false,
                select: true,
            },
            cx,
        );
    }

    fn handle_select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(-1, true, window, cx);
    }

    fn handle_select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        self.move_vertical(1, true, window, cx);
    }

    fn handle_select_page_up(
        &mut self,
        _: &SelectPageUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(false, true, window, cx);
    }

    fn handle_select_page_down(
        &mut self,
        _: &SelectPageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_page(true, true, window, cx);
    }

    fn handle_select_document_start(
        &mut self,
        _: &SelectDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: false,
                select: true,
            },
            cx,
        );
    }

    fn handle_select_document_end(
        &mut self,
        _: &SelectDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveDocumentBoundary {
                to_end: true,
                select: true,
            },
            cx,
        );
    }

    fn handle_move_line_start(
        &mut self,
        _: &MoveLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: false,
                select: false,
            },
            cx,
        );
    }

    fn handle_move_line_end(&mut self, _: &MoveLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: true,
                select: false,
            },
            cx,
        );
    }

    fn handle_select_line_start(
        &mut self,
        _: &SelectLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: false,
                select: true,
            },
            cx,
        );
    }

    fn handle_select_line_end(
        &mut self,
        _: &SelectLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(
            EditorCommand::MoveLineBoundary {
                to_end: true,
                select: true,
            },
            cx,
        );
    }

    fn handle_backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::Backspace, cx);
    }

    fn handle_delete_forward(&mut self, _: &DeleteForward, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::DeleteForward, cx);
    }

    fn handle_delete_word_backward(
        &mut self,
        _: &DeleteWordBackward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DeleteWord { backward: true }, cx);
    }

    fn handle_delete_word_forward(
        &mut self,
        _: &DeleteWordForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::DeleteWord { backward: false }, cx);
    }

    fn handle_insert_newline(&mut self, _: &InsertNewline, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::InsertNewline, cx);
    }

    fn handle_insert_tab(&mut self, _: &InsertTab, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::InsertTab, cx);
    }

    fn handle_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::SelectAll, cx);
        self.sync_primary_selection(cx);
    }

    fn handle_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::Undo, cx);
    }

    fn handle_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::Redo, cx);
    }

    fn handle_find_open(&mut self, _: &FindOpen, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(
            EditorCommand::ToggleFind {
                show_replace: false,
            },
            cx,
        );
    }

    fn handle_find_open_replace(
        &mut self,
        _: &FindOpenReplace,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_model_command(EditorCommand::ToggleFind { show_replace: true }, cx);
    }

    fn handle_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::FindNext, cx);
    }

    fn handle_find_prev(&mut self, _: &FindPrev, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::FindPrev, cx);
    }

    fn handle_replace_one(&mut self, _: &ReplaceOne, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::ReplaceOne, cx);
    }

    fn handle_replace_all(&mut self, _: &ReplaceAll, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::ReplaceAll, cx);
    }

    fn handle_goto_line_open(&mut self, _: &GotoLineOpen, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::ToggleGotoLine, cx);
    }

    fn handle_delete_line(&mut self, _: &DeleteLine, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::DeleteLine, cx);
    }

    fn handle_move_line_up(&mut self, _: &MoveLineUp, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::MoveLineUp, cx);
    }

    fn handle_move_line_down(&mut self, _: &MoveLineDown, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::MoveLineDown, cx);
    }

    fn handle_duplicate_line(&mut self, _: &DuplicateLine, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::DuplicateLine, cx);
    }

    fn handle_toggle_comment(&mut self, _: &ToggleComment, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(EditorCommand::ToggleComment, cx);
    }

    fn maybe_handle_vim_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let mods = gpui_modifiers_to_vim(event.keystroke.modifiers);
        let key = gpui_key_to_vim(event);
        let plain_vim_key = !event.keystroke.modifiers.control
            && !event.keystroke.modifiers.alt
            && !event.keystroke.modifiers.platform;
        let redo_key = key.as_ref().is_some_and(|key| {
            matches!(key, VimKey::Character(value) if value == "r") && mods.command()
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
                    self.active_tab_mut().set_cursor_position(position, None);
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

    fn vim_find_next_from_cursor(&mut self, position: Position) -> Option<Position> {
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
        Some(Position {
            line: m.line,
            column: m.col,
        })
    }

    fn vim_find_prev_from_cursor(&mut self, position: Position) -> Option<Position> {
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
        Some(Position {
            line: m.line,
            column: m.col,
        })
    }

    fn apply_vim_select(&mut self, anchor: Position, head: Position) {
        let tab = self.active_tab_mut();
        let anchor_char = position_to_char(&tab.buffer, anchor);
        let head_char = position_to_char(&tab.buffer, head);
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

    fn move_to_vim_search_target(&mut self, target: Position) {
        if matches!(self.vim.mode, vim::Mode::Visual | vim::Mode::VisualLine) {
            let snapshot = self.vim_snapshot();
            if let vim::VimCommand::Select { anchor, head } =
                self.vim.selection_command(target, &snapshot)
            {
                self.apply_vim_select(anchor, head);
            }
        } else {
            self.active_tab_mut().set_cursor_position(target, None);
        }
    }

    fn vim_delete_range(&mut self, from: Position, to: Position) -> String {
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

    fn vim_extract_range(&mut self, from: Position, to: Position) -> String {
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

    fn vim_transform_case_range(&mut self, from: Position, to: Position, uppercase: bool) {
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
            let boundary = if text.chars().any(char::is_whitespace) {
                UndoBoundary::Break
            } else {
                UndoBoundary::Merge
            };
            tab.edit(kind, boundary, range, text);
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
            tab.edit(EditKind::Other, UndoBoundary::Break, range, new_text);
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

fn delete_selection_or_word_range(tab: &Tab, backward: bool) -> Option<Range<usize>> {
    if tab.has_selection() {
        return Some(tab.selected_range());
    }

    let cursor = tab.cursor_char();
    let target = if backward {
        previous_word_boundary(&tab.buffer, cursor)
    } else {
        next_word_boundary(&tab.buffer, cursor)
    };
    (target != cursor).then_some(target.min(cursor)..target.max(cursor))
}

fn char_to_line_col(buffer: &Rope, char_offset: usize) -> (usize, usize) {
    let char_offset = char_offset.min(buffer.len_chars());
    let line = buffer.char_to_line(char_offset);
    let line_start = buffer.line_to_char(line);
    (line, char_offset - line_start)
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

fn vim_position_lt(a: Position, b: Position) -> bool {
    (a.line, a.column) < (b.line, b.column)
}

fn inclusive_position_to_exclusive_char(buffer: &Rope, position: Position) -> usize {
    let line = position.line.min(buffer.len_lines().saturating_sub(1));
    let line_start = buffer.line_to_char(line);
    let display_len = line_display_char_len(buffer, line);
    if display_len == 0 {
        return line_start;
    }
    line_start + (position.column.min(display_len.saturating_sub(1)) + 1).min(display_len)
}

fn extract_text_range(lines: &[String], from: &Position, to: &Position) -> String {
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

fn remove_text_range(lines: &mut Vec<String>, from: &Position, to: &Position) {
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

fn focus_trace_label(target: FocusTarget) -> &'static str {
    match target {
        FocusTarget::Editor => "editor",
        FocusTarget::FindQuery => "find_query",
        FocusTarget::FindReplace => "find_replace",
        FocusTarget::GotoLine => "goto_line",
    }
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
        let window_title = launch
            .window_title
            .clone()
            .unwrap_or_else(|| "lst GPUI".into());
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(window_title.into()),
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
