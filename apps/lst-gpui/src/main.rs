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

#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use interactions::drag_autoscroll_delta;
use interactions::DragSelectionMode;
use keymap::editor_keybindings;
use launch::{parse_launch_args, AutoBench, BenchAction, LaunchArgs};
#[cfg(test)]
use lst_core::document::Tab;
#[cfg(test)]
pub(crate) use lst_core::selection::{
    drag_selection_range, line_range_at_char, word_range_at_char,
};
#[cfg(test)]
pub(crate) use lst_core::selection::{next_word_boundary, previous_word_boundary};
use lst_editor::{
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
    ops::Range,
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
    byte_index_to_char, code_char_width, code_origin_pad, ensure_wrap_layout, row_contains_cursor,
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

struct EditorTabView {
    id: TabId,
    revision: u64,
    scroll: ScrollHandle,
    cache: Rc<RefCell<ViewportCache>>,
    geometry: Rc<RefCell<ViewportGeometry>>,
}

impl EditorTabView {
    fn new(tab: &ModelEditorTab) -> Self {
        Self {
            id: tab.id(),
            revision: tab.revision(),
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(ViewportCache::default())),
            geometry: Rc::new(RefCell::new(ViewportGeometry::default())),
        }
    }

    fn invalidate_visual_state(&mut self) {
        *self.cache.borrow_mut() = ViewportCache::default();
        *self.geometry.borrow_mut() = ViewportGeometry::default();
    }
}

struct LstGpuiApp {
    focus_handle: FocusHandle,
    model: EditorModel,
    tab_views: Vec<EditorTabView>,
    tab_bar_scroll: ScrollHandle,
    hovered_tab: Option<usize>,
    drag_selecting: Option<DragSelectionMode>,
    drag_last_point: Option<Point<Pixels>>,
    drag_autoscroll_active: bool,
    find_query_input: Entity<InputField>,
    find_replace_input: Entity<InputField>,
    goto_line_input: Entity<InputField>,
    pending_focus: Option<FocusTarget>,
    last_operation: OperationStats,
    autosave_inflight: HashSet<PathBuf>,
    autosave_started: bool,
    _shell_subscriptions: Vec<Subscription>,
}

impl LstGpuiApp {
    fn new(cx: &mut Context<Self>, launch: LaunchArgs) -> Self {
        let mut tabs = Vec::new();
        let mut next_tab_id = 1u64;
        let mut status = "Ready.".to_string();
        let find_query_input = cx.new(|cx| InputField::new(cx, "Find"));
        let find_replace_input = cx.new(|cx| InputField::new(cx, "Replace"));
        let goto_line_input = cx.new(|cx| InputField::new(cx, "Line"));

        if launch.auto_bench.is_some() {
            tabs.push(ModelEditorTab::from_text(
                TabId::from_raw(next_tab_id),
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
            tabs.push(ModelEditorTab::empty(
                TabId::from_raw(next_tab_id),
                format!("{UNTITLED_PREFIX}-1"),
            ));
        } else {
            for path in launch.files {
                match fs::read_to_string(&path) {
                    Ok(text) => {
                        tabs.push(ModelEditorTab::from_path(
                            TabId::from_raw(next_tab_id),
                            path,
                            &text,
                        ));
                        next_tab_id += 1;
                    }
                    Err(err) => {
                        status = format!("Failed to open {}: {err}", path.display());
                    }
                }
            }

            if tabs.is_empty() {
                tabs.push(ModelEditorTab::empty(
                    TabId::from_raw(next_tab_id),
                    format!("{UNTITLED_PREFIX}-1"),
                ));
            }
        }

        let model = EditorModel::new(tabs, status);
        let tab_views = model
            .tabs
            .iter()
            .map(EditorTabView::new)
            .collect::<Vec<_>>();
        let last_operation = OperationStats {
            label: "startup",
            bytes: model.active_tab().buffer.len_bytes(),
            lines: model.active_tab().buffer.len_lines(),
            clipboard_read_ms: None,
            apply_ms: 0.0,
        };

        eprintln!("lst_gpui {}", last_operation.summary());

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            model,
            tab_views,
            tab_bar_scroll: ScrollHandle::new(),
            hovered_tab: None,
            drag_selecting: None,
            drag_last_point: None,
            drag_autoscroll_active: false,
            find_query_input: find_query_input.clone(),
            find_replace_input: find_replace_input.clone(),
            goto_line_input: goto_line_input.clone(),
            pending_focus: None,
            last_operation,
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
                if self.model.find.show_replace {
                    let handle = self.find_replace_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(FocusTarget::FindQuery);
                }
            }
            FocusTarget::GotoLine => {
                if self.model.goto_line.is_some() {
                    let handle = self.goto_line_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(FocusTarget::Editor);
                }
            }
        }
        bench_trace::record_label("focus_applied", focus_trace_label(target));
    }

    fn sync_tab_views(&mut self, old_show_wrap: bool) {
        let mut old_views = std::mem::take(&mut self.tab_views);
        self.tab_views = self
            .model
            .tabs
            .iter()
            .map(|tab| {
                let mut view = old_views
                    .iter()
                    .position(|view| view.id == tab.id())
                    .map(|ix| old_views.swap_remove(ix))
                    .unwrap_or_else(|| EditorTabView::new(tab));
                if view.revision != tab.revision() || old_show_wrap != self.model.show_wrap {
                    view.revision = tab.revision();
                    view.invalidate_visual_state();
                }
                view
            })
            .collect();
        if self
            .hovered_tab
            .is_some_and(|ix| ix >= self.model.tabs.len())
        {
            self.hovered_tab = None;
        }
    }

    fn apply_model_command(&mut self, command: EditorCommand, cx: &mut Context<Self>) {
        let old_show_wrap = self.model.show_wrap;
        let sync_find_inputs = matches!(
            &command,
            EditorCommand::OpenFind { .. } | EditorCommand::ToggleFind { .. }
        );
        let sync_goto_input = matches!(
            &command,
            EditorCommand::OpenGotoLine | EditorCommand::ToggleGotoLine
        );
        self.model.apply(command);
        self.sync_tab_views(old_show_wrap);
        let effects = self.model.drain_effects();
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
        let query = self.model.find.query.clone();
        let replacement = self.model.find.replacement.clone();
        self.find_query_input
            .update(cx, |input, cx| input.set_text(&query, cx));
        self.find_replace_input
            .update(cx, |input, cx| input.set_text(&replacement, cx));
    }

    fn find_input_state(&self) -> (bool, bool, String, String) {
        (
            self.model.find.visible,
            self.model.find.show_replace,
            self.model.find.query.clone(),
            self.model.find.replacement.clone(),
        )
    }

    fn sync_find_inputs_if_changed(
        &mut self,
        old_state: (bool, bool, String, String),
        cx: &mut Context<Self>,
    ) {
        if self.find_input_state() != old_state {
            self.sync_find_inputs(cx);
        }
    }

    fn sync_goto_input(&mut self, cx: &mut Context<Self>) {
        let text = self.model.goto_line.clone().unwrap_or_default();
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
                    let read_started = Instant::now();
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        let clipboard_read_ms = elapsed_ms(read_started);
                        let apply_started = Instant::now();
                        self.apply_model_command(EditorCommand::PasteText(text), cx);
                        self.record_operation(
                            "paste_clipboard",
                            Some(clipboard_read_ms),
                            elapsed_ms(apply_started),
                        );
                    } else {
                        self.model.status =
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
                let reindex_started = Instant::now();
                self.apply_model_command(EditorCommand::SetFindQueryAndSelect(text.clone()), cx);
                self.record_find_metrics(elapsed_ms(reindex_started));
            }
            InputFieldEvent::Submitted => {
                self.apply_model_command(EditorCommand::FindNext, cx);
            }
            InputFieldEvent::Cancelled => {
                self.apply_model_command(EditorCommand::CloseFind, cx);
            }
            InputFieldEvent::NextRequested => {
                if self.model.find.show_replace {
                    self.queue_focus(FocusTarget::FindReplace);
                    cx.notify();
                }
            }
            InputFieldEvent::PreviousRequested => {}
        }
    }

    fn record_find_metrics(&self, reindex_ms: f64) {
        bench_trace::record_ms("find_reindex_ms", reindex_ms);
        bench_trace::record_usize("find_match_count", self.model.find.matches.len());
        bench_trace::record_usize("find_query_len", self.model.find.query.chars().count());
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
        let active = self.model.active;
        let Some(tab) = self.model.tabs.get(active) else {
            return;
        };
        let SyntaxMode::TreeSitter(language) = syntax_mode_for_path(tab.path.as_ref()) else {
            return;
        };

        let revision = tab.revision();
        let key = SyntaxHighlightJobKey { language, revision };
        let cache = self.tab_views[active].cache.clone();
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
        if !syntax_highlight_result_is_current(
            &self.model.tabs,
            &self.tab_views,
            active,
            &cache,
            key,
        ) {
            return;
        }

        cache_ref.syntax_highlights = Some(CachedSyntaxHighlights {
            language: key.language,
            revision: key.revision,
            lines,
        });
        cache_ref.clear_code_lines();
        drop(cache_ref);

        if self.model.active == active {
            cx.notify();
        }
    }

    fn active_tab(&self) -> &ModelEditorTab {
        self.model.active_tab()
    }

    fn active_view(&self) -> &EditorTabView {
        &self.tab_views[self.model.active]
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
            .model
            .tabs
            .iter()
            .filter(|tab| tab.modified)
            .filter_map(|tab| {
                let path = tab.path.clone()?;
                if !autosave_revision_is_current(&self.model.tabs, &path, tab.revision()) {
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
                if !autosave_revision_is_current(&self.model.tabs, &job.path, job.revision) {
                    let _ = fs::remove_file(&temp_path);
                    cx.notify();
                    return;
                }

                match fs::rename(&temp_path, &job.path) {
                    Ok(()) => {
                        self.apply_model_command(
                            EditorCommand::AutosaveFinished {
                                path: job.path,
                                revision: job.revision,
                            },
                            cx,
                        );
                    }
                    Err(err) => {
                        let _ = fs::remove_file(&temp_path);
                        self.apply_model_command(
                            EditorCommand::AutosaveFailed {
                                path: job.path,
                                message: err.to_string(),
                            },
                            cx,
                        );
                    }
                }
            }
            Err(err) => {
                self.apply_model_command(
                    EditorCommand::AutosaveFailed {
                        path: job.path,
                        message: err.to_string(),
                    },
                    cx,
                );
            }
        }
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
            self.model.vim.mode.label().to_string(),
            format!("Ln {}", line + 1),
            format!("Col {}", column + 1),
            if self.model.show_wrap {
                "Wrap".to_string()
            } else {
                "No Wrap".to_string()
            },
            format!("{} lines", tab.line_count()),
        ];
        let pending = self.model.vim.pending_display();
        if !pending.is_empty() {
            parts.push(pending);
        }
        if let Some(selection) = self.selection_summary() {
            parts.push(selection);
        }
        if self.model.find.visible {
            let current = if self.model.find.matches.is_empty() {
                0
            } else {
                self.model.find.current + 1
            };
            parts.push(format!("Match {current}/{}", self.model.find.matches.len()));
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
        let old_show_wrap = self.model.show_wrap;
        {
            let tab = self.model.active_tab_mut();
            let id = tab.id();
            let name_hint = tab.display_name();
            *tab = ModelEditorTab::from_text(id, name_hint, None, text);
        }
        if !self.model.find.query.is_empty() {
            self.model.reindex_find_matches_to_nearest();
        }
        self.sync_tab_views(old_show_wrap);
        if let Some(view) = self.tab_views.get_mut(self.model.active) {
            view.invalidate_visual_state();
        }
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.model.status = format!("Loaded {} lines.", self.active_tab().line_count());
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
        let old_show_wrap = self.model.show_wrap;
        {
            let tab = self.model.active_tab_mut();
            let end = tab.len_chars();
            tab.replace_char_range(end..end, text);
            tab.modified = false;
        }
        if !self.model.find.query.is_empty() {
            self.model.reindex_find_matches_to_nearest();
        }
        self.sync_tab_views(old_show_wrap);
        self.record_operation(label, clipboard_read_ms, elapsed_ms(apply_started));
        self.model.status = format!("Appended {} lines.", text.lines().count());
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
        let wrap_columns = self.active_wrap_columns(window);
        self.apply_model_command(
            EditorCommand::MoveDisplayRows {
                delta,
                select,
                wrap_columns,
            },
            cx,
        );
    }

    fn active_wrap_columns(&mut self, window: &mut Window) -> usize {
        if !self.model.show_wrap {
            return usize::MAX;
        }

        let active = self.model.active;
        let viewport_width = self.tab_views[active]
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| px(WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window);
        let revision = self.model.tabs[active].revision();
        let lines = self.model.tabs[active].lines();
        let layout = {
            let mut cache = self.tab_views[active].cache.borrow_mut();
            ensure_wrap_layout(
                &mut cache,
                lines.as_ref(),
                revision,
                viewport_width,
                char_width,
                self.model.show_gutter,
                self.model.show_wrap,
            )
        };
        layout.wrap_columns
    }

    fn active_page_rows(&self) -> usize {
        let height = self
            .active_view()
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

    fn close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.model.tabs.len() {
            return;
        }

        self.hovered_tab = None;
        self.apply_model_command(EditorCommand::CloseTab(index), cx);
    }

    fn reveal_active_cursor(&self) {
        let tab = self.active_tab();
        let view = self.active_view();
        let viewport_bounds = view.scroll.bounds();
        if viewport_bounds.size.height <= px(0.) {
            return;
        }

        let visual_row = view
            .cache
            .borrow()
            .wrap_layout
            .as_ref()
            .and_then(|cached| visual_row_for_char(tab, &cached.layout))
            .unwrap_or_else(|| tab.buffer.char_to_line(tab.cursor_char()));
        let caret_top = px((visual_row as f32) * ROW_HEIGHT);
        let caret_bottom = caret_top + px(ROW_HEIGHT);
        let scroll_top = {
            let offset_y = -view.scroll.offset().y;
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
            view.scroll.set_offset(point(px(0.0), -target));
        }
    }

    fn sync_primary_selection(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.active_tab().selected_text() {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
    }

    fn active_char_index_for_point(&self, point: Point<Pixels>) -> usize {
        let geometry = self.active_view().geometry.borrow();
        let Some(bounds) = geometry.bounds else {
            return self.active_tab().cursor_char();
        };
        let code_origin_x = bounds.left() + code_origin_pad(self.model.show_gutter);

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
        self.apply_model_command(EditorCommand::CloseTab(self.model.active), cx);
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
        self.apply_model_command(EditorCommand::MoveHorizontalCollapse { backward: true }, cx);
    }

    fn handle_move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_command(
            EditorCommand::MoveHorizontalCollapse { backward: false },
            cx,
        );
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
            let old_show_wrap = self.model.show_wrap;
            let old_find_state = self.find_input_state();
            self.model.handle_vim_escape();
            self.sync_tab_views(old_show_wrap);
            self.sync_find_inputs_if_changed(old_find_state, cx);
            let effects = self.model.drain_effects();
            self.handle_model_effects(effects, cx);
            cx.stop_propagation();
            cx.notify();
            return true;
        }

        if self.model.vim.mode == vim::Mode::Insert {
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

        let old_show_wrap = self.model.show_wrap;
        let old_find_state = self.find_input_state();
        self.model.handle_vim_key(key, mods);
        self.sync_tab_views(old_show_wrap);
        self.sync_find_inputs_if_changed(old_find_state, cx);
        let effects = self.model.drain_effects();
        self.handle_model_effects(effects, cx);
        cx.stop_propagation();
        cx.notify();
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
        self.apply_model_command(EditorCommand::ClearMarkedText, cx);
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
                .map(|range| utf16_range_to_char_range(&tab.buffer, range))
        };
        self.apply_model_command(
            EditorCommand::ReplaceTextFromInput {
                range,
                text: text.to_string(),
            },
            cx,
        );
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
                .map(|range| utf16_range_to_char_range(&tab.buffer, range))
        };
        let selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range| utf16_range_to_char_range_in_text(new_text, range));
        self.apply_model_command(
            EditorCommand::ReplaceAndMarkText {
                range,
                text: new_text.to_string(),
                selected_range,
            },
            cx,
        );
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
        let range = utf16_range_to_char_range(&tab.buffer, &range_utf16);
        let row = geometry
            .rows
            .iter()
            .rfind(|row| row_contains_cursor(row, range.start))?;
        let code_origin_x = element_bounds.left() + code_origin_pad(self.model.show_gutter);
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

fn autosave_revision_is_current(tabs: &[ModelEditorTab], path: &PathBuf, revision: u64) -> bool {
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
    tabs: &[ModelEditorTab],
    tab_views: &[EditorTabView],
    active: usize,
    cache: &Rc<RefCell<ViewportCache>>,
    key: SyntaxHighlightJobKey,
) -> bool {
    tabs.get(active).is_some_and(|tab| {
        tab_views.get(active).is_some_and(|view| {
            Rc::ptr_eq(&view.cache, cache)
                && tab.revision() == key.revision
                && syntax_mode_for_path(tab.path.as_ref()) == SyntaxMode::TreeSitter(key.language)
        })
    })
}

#[cfg(test)]
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

fn utf16_range_to_char_range_in_text(text: &str, range: &Range<usize>) -> Range<usize> {
    let buffer = Rope::from_str(text);
    utf16_range_to_char_range(&buffer, range)
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
