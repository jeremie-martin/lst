use gpui::{
    actions, point, prelude::*, px, size, App, Application, Bounds, ClipboardItem, Context, Entity,
    FocusHandle, Focusable, Pixels, Point, ScrollHandle, Subscription, Window, WindowBounds,
    WindowOptions,
};

mod actions;
mod bench_trace;
mod input_adapter;
mod interactions;
mod keymap;
mod launch;
mod runtime;
mod shell;
mod syntax;
#[cfg(test)]
mod tests;
mod ui;
mod viewport;

use crate::ui::{input_keybindings, InputField, InputFieldEvent};
#[cfg(test)]
pub(crate) use input_adapter::{char_range_to_utf16_range, utf16_range_to_char_range_in_text};
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use interactions::drag_autoscroll_delta;
use interactions::DragSelectionMode;
use keymap::editor_keybindings;
use launch::{parse_launch_args, LaunchArgs};
use lst_editor::{EditorModel, EditorTab as ModelEditorTab, FocusTarget, TabId, UNTITLED_PREFIX};
use ropey::Rope;
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use runtime::autosave_revision_is_current;
use std::{cell::RefCell, collections::HashSet, path::PathBuf, process, rc::Rc, time::Instant};
use syntax::{
    compute_syntax_highlights, syntax_mode_for_path, CachedSyntaxHighlights, SyntaxHighlightJobKey,
    SyntaxMode, SyntaxSpan,
};
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use viewport::row_contains_cursor;
use viewport::{
    byte_index_to_char, code_char_width, code_origin_pad, ensure_wrap_layout, visual_row_for_char,
    ViewportCache, ViewportGeometry,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingAfterSave {
    CloseTab(TabId),
    Quit,
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
    persistent_overlay_focus: Option<FocusTarget>,
    pending_after_save: Option<PendingAfterSave>,
    autosave_inflight: HashSet<PathBuf>,
    autosave_started: bool,
    _shell_subscriptions: Vec<Subscription>,
}

impl LstGpuiApp {
    fn new(cx: &mut Context<Self>, launch: LaunchArgs) -> Self {
        let find_query_input = cx.new(|cx| InputField::new(cx, "Find"));
        let find_replace_input = cx.new(|cx| InputField::new(cx, "Replace"));
        let goto_line_input = cx.new(|cx| InputField::new(cx, "Line"));
        let model = initial_model_from_launch(launch);
        let tab_views = model
            .tabs()
            .iter()
            .map(EditorTabView::new)
            .collect::<Vec<_>>();

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
            persistent_overlay_focus: None,
            pending_after_save: None,
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

    #[cfg(test)]
    fn snapshot(&self, cx: &mut Context<Self>) -> AppSnapshot {
        AppSnapshot {
            model: self.model.snapshot(),
            find_query_input: self.find_query_input.read(cx).text(),
            find_replace_input: self.find_replace_input.read(cx).text(),
            goto_line_input: self.goto_line_input.read(cx).text(),
            pending_focus: self.pending_focus,
            tab_view_ids: self.tab_views.iter().map(|view| view.id).collect(),
        }
    }

    fn queue_focus(&mut self, target: FocusTarget) {
        bench_trace::record_label("focus_queued", focus_trace_label(target));
        self.persistent_overlay_focus = match target {
            FocusTarget::FindQuery | FocusTarget::FindReplace | FocusTarget::GotoLine => {
                Some(target)
            }
            FocusTarget::Editor => None,
        };
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
                if self.model.find().show_replace {
                    let handle = self.find_replace_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(FocusTarget::FindQuery);
                }
            }
            FocusTarget::GotoLine => {
                if self.model.goto_line().is_some() {
                    let handle = self.goto_line_input.read(cx).focus_handle();
                    window.focus(&handle);
                } else {
                    self.pending_focus = Some(FocusTarget::Editor);
                }
            }
        }
        bench_trace::record_label("focus_applied", focus_trace_label(target));
    }

    fn maintain_overlay_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.persistent_overlay_focus else {
            return;
        };
        let Some(handle) = self.focus_handle_for_target(target, cx) else {
            return;
        };
        if !handle.is_focused(window) {
            window.focus(&handle);
            bench_trace::record_label("focus_maintained", focus_trace_label(target));
        }
    }

    fn focus_handle_for_target(
        &self,
        target: FocusTarget,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        match target {
            FocusTarget::Editor => Some(self.focus_handle.clone()),
            FocusTarget::FindQuery => self
                .model
                .find()
                .visible
                .then(|| self.find_query_input.read(cx).focus_handle()),
            FocusTarget::FindReplace => (self.model.find().visible
                && self.model.find().show_replace)
                .then(|| self.find_replace_input.read(cx).focus_handle()),
            FocusTarget::GotoLine => self
                .model
                .goto_line()
                .is_some()
                .then(|| self.goto_line_input.read(cx).focus_handle()),
        }
    }

    fn sync_tab_views(&mut self, old_show_wrap: bool) {
        let mut old_views = std::mem::take(&mut self.tab_views);
        self.tab_views = self
            .model
            .tabs()
            .iter()
            .map(|tab| {
                let mut view = old_views
                    .iter()
                    .position(|view| view.id == tab.id())
                    .map(|ix| old_views.swap_remove(ix))
                    .unwrap_or_else(|| EditorTabView::new(tab));
                if view.revision != tab.revision() || old_show_wrap != self.model.show_wrap() {
                    view.revision = tab.revision();
                    view.invalidate_visual_state();
                }
                view
            })
            .collect();
        if self
            .hovered_tab
            .is_some_and(|ix| ix >= self.model.tab_count())
        {
            self.hovered_tab = None;
        }
    }

    fn update_model(
        &mut self,
        cx: &mut Context<Self>,
        notify_after_update: bool,
        update: impl FnOnce(&mut EditorModel),
    ) {
        let old_show_wrap = self.model.show_wrap();
        let old_find_state = self.find_input_state();
        let old_goto_line = self.model.goto_line().map(ToOwned::to_owned);
        update(&mut self.model);
        self.sync_tab_views(old_show_wrap);
        let effects = self.model.drain_effects();
        self.sync_find_inputs_if_changed(old_find_state, cx);
        if self.model.goto_line() != old_goto_line.as_deref() {
            self.sync_goto_input(cx);
        }
        self.handle_model_effects(effects, cx);
        if notify_after_update {
            cx.notify();
        }
    }

    fn sync_find_inputs(&mut self, cx: &mut Context<Self>) {
        let query = self.model.find().query.clone();
        let replacement = self.model.find().replacement.clone();
        self.find_query_input
            .update(cx, |input, cx| input.set_text(&query, cx));
        self.find_replace_input
            .update(cx, |input, cx| input.set_text(&replacement, cx));
    }

    fn find_input_state(&self) -> (bool, bool, String, String) {
        (
            self.model.find().visible,
            self.model.find().show_replace,
            self.model.find().query.clone(),
            self.model.find().replacement.clone(),
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
        let text = self.model.goto_line().unwrap_or_default().to_string();
        self.goto_line_input
            .update(cx, |input, cx| input.set_text(&text, cx));
    }

    fn handle_find_query_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                let reindex_started = Instant::now();
                self.update_model(cx, true, |model| {
                    model.update_find_query_and_select(text.clone());
                });
                self.record_find_metrics(elapsed_ms(reindex_started));
            }
            InputFieldEvent::Submitted => {
                self.update_model(cx, true, EditorModel::find_next_match);
            }
            InputFieldEvent::Cancelled => {
                self.update_model(cx, true, EditorModel::close_find_panel);
            }
            InputFieldEvent::NextRequested => {
                if self.model.find().show_replace {
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
                self.update_model(cx, true, |model| {
                    model.update_find_replacement(text.clone());
                });
            }
            InputFieldEvent::Submitted => {
                self.update_model(cx, true, EditorModel::replace_current_match);
            }
            InputFieldEvent::Cancelled => {
                self.update_model(cx, true, EditorModel::close_find_panel);
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
                self.update_model(cx, true, |model| {
                    model.update_goto_line(text.clone());
                });
            }
            InputFieldEvent::Submitted => {
                self.update_model(cx, true, EditorModel::submit_goto_line_input);
            }
            InputFieldEvent::Cancelled => {
                self.update_model(cx, true, EditorModel::close_goto_line_panel);
            }
            InputFieldEvent::NextRequested | InputFieldEvent::PreviousRequested => {}
        }
    }

    fn ensure_active_syntax_highlights(&mut self, cx: &mut Context<Self>) {
        let active = self.model.active_index();
        let Some(tab) = self.model.tab(active) else {
            return;
        };
        let SyntaxMode::TreeSitter(language) = syntax_mode_for_path(tab.path()) else {
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
            self.model.tabs(),
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

        if self.model.active_index() == active {
            cx.notify();
        }
    }

    fn active_tab(&self) -> &ModelEditorTab {
        self.model.active_tab()
    }

    fn record_find_metrics(&self, reindex_ms: f64) {
        bench_trace::record_ms("find_reindex_ms", reindex_ms);
        bench_trace::record_usize("find_match_count", self.model.find().matches.len());
        bench_trace::record_usize("find_query_len", self.model.find().query.chars().count());
    }

    pub(crate) fn record_operation(
        &self,
        label: &'static str,
        clipboard_read_ms: Option<f64>,
        apply_ms: f64,
    ) {
        let tab = self.active_tab();
        bench_trace::record_operation(
            label,
            tab.buffer().len_bytes(),
            tab.line_count(),
            clipboard_read_ms,
            apply_ms,
        );
    }

    fn active_view(&self) -> &EditorTabView {
        &self.tab_views[self.model.active_index()]
    }

    fn active_cursor_line_col(&self) -> (usize, usize) {
        char_to_line_col(self.active_tab().buffer(), self.active_tab().cursor_char())
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
            self.model.vim_mode().label().to_string(),
            format!("Ln {}", line + 1),
            format!("Col {}", column + 1),
            if self.model.show_wrap() {
                "Wrap".to_string()
            } else {
                "No Wrap".to_string()
            },
            format!("{} lines", tab.line_count()),
        ];
        let pending = self.model.vim_pending_display();
        if !pending.is_empty() {
            parts.push(pending);
        }
        if let Some(selection) = self.selection_summary() {
            parts.push(selection);
        }
        if self.model.find().visible {
            let current = if self.model.find().matches.is_empty() {
                0
            } else {
                self.model.find().current + 1
            };
            parts.push(format!(
                "Match {current}/{}",
                self.model.find().matches.len()
            ));
        }
        parts.join("  ")
    }

    fn move_vertical(
        &mut self,
        delta: isize,
        select: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let wrap_columns = self.active_wrap_columns(window);
        self.update_model(cx, true, |model| {
            model.move_display_rows_by(delta, select, wrap_columns);
        });
    }

    fn active_wrap_columns(&mut self, window: &mut Window) -> usize {
        if !self.model.show_wrap() {
            return usize::MAX;
        }

        let active = self.model.active_index();
        let viewport_width = self.tab_views[active]
            .geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| px(WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window);
        let revision = self.model.active_tab().revision();
        let lines = self.model.active_tab_lines();
        let layout = {
            let mut cache = self.tab_views[active].cache.borrow_mut();
            ensure_wrap_layout(
                &mut cache,
                lines.as_ref(),
                revision,
                viewport_width,
                char_width,
                self.model.show_gutter(),
                self.model.show_wrap(),
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
            .unwrap_or_else(|| tab.buffer().char_to_line(tab.cursor_char()));
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
        let code_origin_x = bounds.left() + code_origin_pad(self.model.show_gutter());

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
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppSnapshot {
    pub(crate) model: lst_editor::EditorSnapshot,
    pub(crate) find_query_input: String,
    pub(crate) find_replace_input: String,
    pub(crate) goto_line_input: String,
    pub(crate) pending_focus: Option<FocusTarget>,
    pub(crate) tab_view_ids: Vec<TabId>,
}

fn initial_model_from_launch(launch: LaunchArgs) -> EditorModel {
    let mut tabs = Vec::new();
    let mut next_tab_id = 1u64;
    let mut status = "Ready.".to_string();

    if launch.files.is_empty() {
        tabs.push(ModelEditorTab::empty(
            TabId::from_raw(next_tab_id),
            format!("{UNTITLED_PREFIX}-1"),
        ));
    } else {
        for path in launch.files {
            match runtime::read_file_with_stamp(&path) {
                Ok((text, file_stamp)) => {
                    tabs.push(ModelEditorTab::from_path_with_stamp(
                        TabId::from_raw(next_tab_id),
                        path,
                        &text,
                        Some(file_stamp),
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

    EditorModel::new(tabs, status)
}

impl Focusable for LstGpuiApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
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
                && syntax_mode_for_path(tab.path()) == SyntaxMode::TreeSitter(key.language)
        })
    })
}

fn char_to_line_col(buffer: &Rope, char_offset: usize) -> (usize, usize) {
    let char_offset = char_offset.min(buffer.len_chars());
    let line = buffer.char_to_line(char_offset);
    let line_start = buffer.line_to_char(line);
    (line, char_offset - line_start)
}

fn focus_trace_label(target: FocusTarget) -> &'static str {
    match target {
        FocusTarget::Editor => "editor",
        FocusTarget::FindQuery => "find_query",
        FocusTarget::FindReplace => "find_replace",
        FocusTarget::GotoLine => "goto_line",
    }
}

pub(crate) fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let launch = parse_launch_args();
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

        window
            .update(cx, |view, window, cx| {
                let entity = cx.entity();
                window.on_window_should_close(cx, move |_window, cx| {
                    entity.update(cx, |view, cx| {
                        if view.model.first_dirty_tab_index().is_none() {
                            true
                        } else {
                            view.request_quit(cx);
                            false
                        }
                    })
                });
                window.focus(&view.focus_handle(cx));
                cx.activate(true);
                view.start_background_tasks(window, cx);
            })
            .unwrap();
    });
}
