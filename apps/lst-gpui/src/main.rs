use gpui::{
    actions, prelude::*, px, size, App, Application, Bounds, Context, Entity, FocusHandle,
    Focusable, Modifiers, Pixels, ScrollHandle, Subscription, Window, WindowBounds, WindowOptions,
};

mod actions;
mod bench_trace;
mod cleanup;
mod crash_log;
mod editor_scrollbar;
mod editor_view;
mod input_adapter;
mod interactions;
mod keymap;
mod launch;
mod llm;
mod recent;
mod recent_panel;
mod runtime;
mod shell;
mod state_trace;
mod state_trace_adapter;
mod syntax;
#[cfg(test)]
mod tests;
mod ui;
mod viewport;

use crate::ui::{
    input_keybindings,
    theme::{current_theme, current_theme_id, metrics, Theme, ThemeId},
    InputField, InputFieldEvent,
};
#[cfg(test)]
pub(crate) use input_adapter::{char_range_to_utf16_range, utf16_range_to_char_range_in_text};
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use interactions::drag_autoscroll_delta;
use interactions::ActiveDragSelection;
use keymap::editor_keybindings;
use launch::{parse_launch_args, LaunchArgs};
use lst_editor::{
    position::Position, EditorCommand as Command, EditorModel, EditorTab as ModelEditorTab,
    FocusTarget, RevealIntent, TabId, UNTITLED_PREFIX,
};
#[cfg(not(test))]
use recent::default_recent_files_path;
use recent::RecentView;
use ropey::Rope;
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use runtime::autosave_revision_is_current;
use state_trace::StateTraceEmitter;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    process,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};
use syntax::{
    compute_syntax_highlights, syntax_mode_for_language, CachedSyntaxHighlights,
    SyntaxHighlightJobKey, SyntaxMode, SyntaxSpan,
};
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use viewport::row_contains_cursor;
use viewport::{scroll_to_left, ViewportCache, ViewportGeometry};

pub(crate) const RECENT_CARD_BASIS: f32 = 260.0;

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
        MoveTabLeft,
        MoveTabRight,
        ToggleWrap,
        ToggleLineNumberMode,
        ToggleTheme,
        CopySelection,
        CutSelection,
        PasteClipboard,
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        MoveSubwordLeft,
        MoveSubwordRight,
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
        SelectSubwordLeft,
        SelectSubwordRight,
        SelectPageUp,
        SelectPageDown,
        SelectDocumentStart,
        SelectDocumentEnd,
        MoveSmartHome,
        MoveLineStart,
        MoveLineEnd,
        SelectSmartHome,
        SelectLineStart,
        SelectLineEnd,
        Backspace,
        DeleteForward,
        DeleteWordBackward,
        DeleteWordForward,
        InsertNewline,
        InsertTab,
        OutdentSelection,
        SelectAll,
        SelectNextOccurrence,
        SelectAllOccurrences,
        SelectFindMatches,
        SkipNextOccurrence,
        PopSelectionCursor,
        AddCursorAbove,
        AddCursorBelow,
        AddCursorsToLineEnds,
        SelectLine,
        SelectParagraph,
        Undo,
        Redo,
        SwapRedoBranch,
        FindOpen,
        FindOpenReplace,
        FindNext,
        FindPrev,
        ReplaceOne,
        ReplaceAll,
        ToggleFindCase,
        ToggleFindWholeWord,
        ToggleFindRegex,
        ToggleFindInSelection,
        GotoLineOpen,
        DeleteLine,
        MoveLineUp,
        MoveLineDown,
        DuplicateLine,
        ToggleComment,
        ToggleBlockComment,
        TransposeChars,
        ToggleOvertype,
        ToggleBookmark,
        NextBookmark,
        PreviousBookmark,
        ReopenClosedTab,
        CleanupText,
        ToggleRecentFiles,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        Quit,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingAfterSave {
    CloseTab(TabId),
    Quit,
}

#[derive(Clone, Copy, Debug)]
struct EditorScrollbarDrag {
    grab_offset: Pixels,
}

pub(crate) struct EditorTabView {
    revision: u64,
    scroll: ScrollHandle,
    cache: Rc<RefCell<ViewportCache>>,
    geometry: Rc<RefCell<ViewportGeometry>>,
}

impl EditorTabView {
    fn new(tab: &ModelEditorTab) -> Self {
        Self {
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
    tab_views: HashMap<TabId, EditorTabView>,
    tab_bar_scroll: ScrollHandle,
    recent_scroll: ScrollHandle,
    hovered_tab: Option<usize>,
    selection_drag: Option<ActiveDragSelection>,
    editor_scrollbar_drag: Option<EditorScrollbarDrag>,
    editor_scrollbar_hovered: bool,
    editor_horizontal_scrollbar_drag: Option<EditorScrollbarDrag>,
    editor_horizontal_scrollbar_hovered: bool,
    find_query_input: Entity<InputField>,
    find_replace_input: Entity<InputField>,
    goto_line_input: Entity<InputField>,
    recent_query_input: Entity<InputField>,
    focus_target: FocusTarget,
    focus_last_applied: FocusTarget,
    pending_after_save: Option<PendingAfterSave>,
    pending_reveal: Option<RevealIntent>,
    reveal_scheduled: bool,
    autosave_inflight: HashSet<PathBuf>,
    save_ticket_generations: HashMap<PathBuf, Arc<Mutex<u64>>>,
    autosave_started: bool,
    scratchpad_dir: Option<PathBuf>,
    recent: RecentView,
    force_editor_focus: bool,
    modifier_chord_accumulated: Modifiers,
    recent_modifier_chord: Option<(Modifiers, Instant)>,
    x11_ctrl_k_pending: bool,
    zoom_level: i32,
    state_trace: StateTraceEmitter,
    cleanup_in_flight: bool,
    cleanup_message: Option<String>,
    /// Surfaced through the state trace so real-X11 tests can click the
    /// button without depending on theme-name / status-details widths.
    cleanup_button_bounds_px: Option<Bounds<Pixels>>,
    status_details_rendered: String,
    /// Stack of recently closed file tabs, most-recent-last. `Ctrl+Shift+T`
    /// pops the top entry and reopens it with the cursor restored. Bounded
    /// so a long-lived editor session does not grow this unboundedly.
    closed_tabs_history: Vec<ClosedTabRecord>,
    _shell_subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug)]
struct ClosedTabRecord {
    path: PathBuf,
    position: Position,
}

impl LstGpuiApp {
    fn new(cx: &mut Context<Self>, launch: LaunchArgs) -> Self {
        let find_query_input = cx.new(|cx| InputField::new(cx, "Find"));
        let find_replace_input = cx.new(|cx| InputField::new(cx, "Replace"));
        let goto_line_input = cx.new(|cx| InputField::new(cx, "Line[:Column]"));
        let recent_query_input =
            cx.new(|cx| InputField::new(cx, "Search recent files").with_vertical_navigation());
        #[cfg(test)]
        let recent_files_path = launch.recent_files_path.clone();
        #[cfg(not(test))]
        let recent_files_path = default_recent_files_path();
        let scratchpad_dir = launch.scratchpad_dir.clone();
        let model = initial_model_from_launch(launch);
        let mut recent = RecentView::load(recent_files_path);
        for tab in model.tabs() {
            if !tab.is_scratchpad() {
                if let Some(path) = tab.path() {
                    recent.record(path);
                }
            }
        }

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            model,
            tab_views: HashMap::new(),
            tab_bar_scroll: ScrollHandle::new(),
            recent_scroll: ScrollHandle::new(),
            hovered_tab: None,
            selection_drag: None,
            editor_scrollbar_drag: None,
            editor_scrollbar_hovered: false,
            editor_horizontal_scrollbar_drag: None,
            editor_horizontal_scrollbar_hovered: false,
            find_query_input: find_query_input.clone(),
            find_replace_input: find_replace_input.clone(),
            goto_line_input: goto_line_input.clone(),
            recent_query_input: recent_query_input.clone(),
            focus_target: FocusTarget::Editor,
            focus_last_applied: FocusTarget::Editor,
            pending_after_save: None,
            pending_reveal: None,
            reveal_scheduled: false,
            autosave_inflight: HashSet::new(),
            save_ticket_generations: HashMap::new(),
            autosave_started: false,
            scratchpad_dir,
            recent,
            force_editor_focus: false,
            modifier_chord_accumulated: Modifiers::default(),
            recent_modifier_chord: None,
            x11_ctrl_k_pending: false,
            zoom_level: 0,
            state_trace: StateTraceEmitter::from_env(),
            cleanup_in_flight: false,
            cleanup_message: None,
            cleanup_button_bounds_px: None,
            status_details_rendered: String::new(),
            closed_tabs_history: Vec::new(),
            _shell_subscriptions: Vec::new(),
        };
        cx.set_global(ThemeId::default());
        let show_wrap = app.model.show_wrap();
        app.sync_tab_views(show_wrap);

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
        app._shell_subscriptions.push(cx.subscribe(
            &recent_query_input,
            |this, _, event: &InputFieldEvent, cx| this.handle_recent_query_input_event(event, cx),
        ));

        app
    }

    #[cfg(test)]
    fn snapshot(&self, cx: &mut Context<Self>) -> AppSnapshot {
        let page = self.recent.page();
        AppSnapshot {
            model: self.model.snapshot(),
            recent_query_input: self.recent_query_input.read(cx).text(),
            recent_panel_visible: self.recent.is_open(),
            recent_paths: self.recent.entries().to_vec(),
            recent_visible_paths: page.visible,
            recent_selected_index: page.selected_index,
            recent_empty_message: page.empty_message,
            recent_content_search_pending: self.recent.content_search_pending(),
            focus_target: self.focus_target,
            #[cfg(feature = "internal-invariants")]
            tab_view_ids: self
                .model
                .tabs()
                .iter()
                .filter(|tab| self.tab_views.contains_key(&tab.id()))
                .map(|tab| tab.id())
                .collect(),
            theme_id: current_theme_id(cx),
        }
    }

    #[cfg(test)]
    pub(crate) fn active_viewport_bounds(&self) -> Option<gpui::Bounds<Pixels>> {
        self.active_view().geometry.borrow().bounds
    }

    #[cfg(test)]
    pub(crate) fn active_painted_rows(&self) -> Vec<viewport::PaintedRow> {
        self.active_view().geometry.borrow().rows.clone()
    }

    /// `None` until the wrap layout has been built for the current tab.
    #[cfg(test)]
    pub(crate) fn observable_cursor_viewport(&self) -> Option<ObservableCursorViewport> {
        let active_view = self.active_view();
        let bounds = active_view.geometry.borrow().bounds?;
        let cache = active_view.cache.borrow();
        let layout = cache.wrap_layout.as_ref()?;
        let cursor_row = viewport::visual_row_for_char(self.active_tab(), &layout.layout)?;
        let scroll_top = viewport::scroll_top_for(&active_view.scroll);
        let max_offset = active_view.scroll.max_offset().height.max(px(0.0));
        let row_height = self.ui_px(metrics::ROW_HEIGHT);
        Some(ObservableCursorViewport {
            scroll_top: scroll_top / px(1.0),
            viewport_height: bounds.size.height / px(1.0),
            row_height: row_height / px(1.0),
            cursor_row,
            max_offset: max_offset / px(1.0),
            total_rows: layout.layout.total_rows,
        })
    }

    fn ui_scale(&self) -> f32 {
        metrics::zoom_scale(self.zoom_level)
    }

    fn ui_px(&self, value: f32) -> Pixels {
        metrics::px_for_scale(value, self.ui_scale())
    }

    fn theme(&self, cx: &App) -> Theme {
        current_theme(cx)
    }

    fn set_theme(&mut self, theme_id: ThemeId, cx: &mut Context<Self>) {
        if current_theme_id(cx) == theme_id {
            return;
        }

        cx.set_global(theme_id);
        for view in self.tab_views.values_mut() {
            view.cache.borrow_mut().clear_shaped_lines();
        }
        cx.refresh_windows();
    }

    fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        self.set_theme(current_theme_id(cx).next(), cx);
    }

    fn set_zoom_level(&mut self, level: i32, window: &mut Window, cx: &mut Context<Self>) {
        let level = level.clamp(metrics::MIN_ZOOM_LEVEL, metrics::MAX_ZOOM_LEVEL);
        if self.zoom_level == level {
            return;
        }

        self.zoom_level = level;
        window.set_rem_size(self.ui_px(metrics::BASE_REM_SIZE));
        for view in self.tab_views.values_mut() {
            view.invalidate_visual_state();
        }
        cx.notify();
    }

    fn zoom_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom_level(self.zoom_level.saturating_add(1), window, cx);
    }

    fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom_level(self.zoom_level.saturating_sub(1), window, cx);
    }

    fn zoom_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom_level(0, window, cx);
    }

    pub(crate) fn set_focus(&mut self, target: FocusTarget) {
        if self.focus_target != target {
            bench_trace::record_label("focus_queued", focus_trace_label(target));
            self.focus_target = target;
        }
    }

    fn apply_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recent.is_open() {
            let handle = self.recent_query_input.read(cx).focus_handle();
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
            return;
        }

        if self.force_editor_focus {
            window.focus(&self.focus_handle);
            self.focus_last_applied = FocusTarget::Editor;
            self.force_editor_focus = false;
            return;
        }

        let target = self.focus_target;
        let just_changed = target != self.focus_last_applied;
        if matches!(target, FocusTarget::Editor) && !just_changed {
            return;
        }
        let handle = self.handle_for(target, cx);
        let needs_focus = just_changed || !handle.is_focused(window);
        if needs_focus {
            window.focus(&handle);
            let label = if just_changed {
                "focus_applied"
            } else {
                "focus_maintained"
            };
            bench_trace::record_label(label, focus_trace_label(target));
        }
        self.focus_last_applied = target;
    }

    /// Invariant: every `Focus(target)` emitter mutates the model into `target`'s
    /// renderable state in the same call before queueing the effect, so this is total.
    fn handle_for(&self, target: FocusTarget, cx: &mut Context<Self>) -> FocusHandle {
        match target {
            FocusTarget::Editor => self.focus_handle.clone(),
            FocusTarget::FindQuery => self.find_query_input.read(cx).focus_handle(),
            FocusTarget::FindReplace => self.find_replace_input.read(cx).focus_handle(),
            FocusTarget::GotoLine => self.goto_line_input.read(cx).focus_handle(),
        }
    }

    fn sync_tab_views(&mut self, old_show_wrap: bool) {
        let show_wrap = self.model.show_wrap();
        let tabs = self.model.tabs();
        self.tab_views
            .retain(|tab_id, _| tabs.iter().any(|tab| tab.id() == *tab_id));
        for tab in tabs {
            let view = self
                .tab_views
                .entry(tab.id())
                .or_insert_with(|| EditorTabView::new(tab));
            if view.revision != tab.revision() || old_show_wrap != show_wrap {
                view.revision = tab.revision();
                view.invalidate_visual_state();
                if old_show_wrap != show_wrap {
                    scroll_to_left(&view.scroll, px(0.0));
                }
            }
        }
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
        if !self.cleanup_in_flight {
            self.cleanup_message = None;
        }
        self.sync_viewport_state();
        let old_show_wrap = self.model.show_wrap();
        let old_find_state = self.find_input_state();
        let old_goto_line = self.model.goto_line().map(ToOwned::to_owned);
        update(&mut self.model);
        self.sync_tab_views(old_show_wrap);
        let effects = self.model.drain_effects();
        self.sync_find_inputs_if_changed(old_find_state.clone(), cx);
        if self.model.goto_line() != old_goto_line.as_deref() {
            self.sync_goto_input(cx);
        }
        self.select_query_after_panel_open(old_find_state, cx);
        self.handle_model_effects(effects, cx);
        if notify_after_update {
            cx.notify();
        }
    }

    fn execute_model_command(&mut self, cx: &mut Context<Self>, command: Command) {
        self.update_model(cx, true, |model| model.execute(command));
    }

    /// On panel open / show_replace flip, select the prefilled query so
    /// typing replaces it (VS Code "selection becomes search term, ready
    /// to overwrite" parity).
    fn select_query_after_panel_open(
        &mut self,
        old_find_state: (bool, bool, String, String),
        cx: &mut Context<Self>,
    ) {
        let (old_visible, old_show_replace, _, _) = old_find_state;
        let now_visible = self.model.find().visible;
        let now_show_replace = self.model.find().show_replace;
        let just_opened = now_visible && (!old_visible || old_show_replace != now_show_replace);
        if !just_opened {
            return;
        }
        if self.model.find().query.is_empty() {
            return;
        }
        self.find_query_input
            .update(cx, |input, cx| input.select_all(cx));
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
                    model.update_find_query_and_activate(text.clone());
                });
                self.record_find_metrics(elapsed_ms(reindex_started));
            }
            InputFieldEvent::Submitted => {
                self.execute_model_command(cx, Command::FindNext);
            }
            InputFieldEvent::Cancelled => {
                self.update_model(cx, true, EditorModel::close_find_panel);
            }
            InputFieldEvent::NextRequested => {
                if self.model.find().show_replace {
                    self.set_focus(FocusTarget::FindReplace);
                    cx.notify();
                }
            }
            InputFieldEvent::PreviousRequested => {}
            InputFieldEvent::Navigate(_) => {}
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
                self.execute_model_command(cx, Command::ReplaceCurrentMatch);
            }
            InputFieldEvent::Cancelled => {
                self.update_model(cx, true, EditorModel::close_find_panel);
            }
            InputFieldEvent::NextRequested => {}
            InputFieldEvent::PreviousRequested => {
                self.set_focus(FocusTarget::FindQuery);
                cx.notify();
            }
            InputFieldEvent::Navigate(_) => {}
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
                self.execute_model_command(cx, Command::SubmitGotoLine);
            }
            InputFieldEvent::Cancelled => {
                self.update_model(cx, true, EditorModel::close_goto_line_panel);
            }
            InputFieldEvent::NextRequested | InputFieldEvent::PreviousRequested => {}
            InputFieldEvent::Navigate(_) => {}
        }
    }

    fn ensure_active_syntax_highlights(&mut self, cx: &mut Context<Self>) {
        let tab = self.model.active_tab();
        let SyntaxMode::TreeSitter(language) = syntax_mode_for_language(tab.language()) else {
            return;
        };

        let tab_id = tab.id();
        let revision = tab.revision();
        let key = SyntaxHighlightJobKey { language, revision };
        let cache = self.active_view().cache.clone();
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
            if cache_ref.syntax_highlight_inflight.is_some() {
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
                view.finish_syntax_highlights(tab_id, key, cache, lines, cx);
            });
        })
        .detach();
    }

    fn finish_syntax_highlights(
        &mut self,
        tab_id: TabId,
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
        if !syntax_highlight_result_is_current(&self.model, &self.tab_views, tab_id, &cache, key) {
            if self.model.active_tab_id() == tab_id {
                cx.notify();
            }
            return;
        }

        cache_ref.syntax_highlights = Some(CachedSyntaxHighlights {
            language: key.language,
            revision: key.revision,
            lines,
        });
        cache_ref.clear_code_lines();
        drop(cache_ref);

        if self.model.active_tab_id() == tab_id {
            cx.notify();
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppSnapshot {
    pub(crate) model: lst_editor::EditorSnapshot,
    pub(crate) recent_query_input: String,
    pub(crate) recent_panel_visible: bool,
    pub(crate) recent_paths: Vec<PathBuf>,
    pub(crate) recent_visible_paths: Vec<PathBuf>,
    pub(crate) recent_selected_index: Option<usize>,
    pub(crate) recent_empty_message: Option<String>,
    pub(crate) recent_content_search_pending: bool,
    pub(crate) focus_target: FocusTarget,
    #[cfg(feature = "internal-invariants")]
    pub(crate) tab_view_ids: Vec<TabId>,
    pub(crate) theme_id: ThemeId,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservableCursorViewport {
    pub(crate) scroll_top: f32,
    pub(crate) viewport_height: f32,
    pub(crate) row_height: f32,
    pub(crate) cursor_row: usize,
    pub(crate) max_offset: f32,
    pub(crate) total_rows: usize,
}

fn initial_model_from_launch(launch: LaunchArgs) -> EditorModel {
    let mut tabs = Vec::new();
    let mut next_tab_id = 1u64;
    let mut status = "Ready.".to_string();

    if launch.files.is_empty() {
        tabs.push(scratchpad_or_empty_tab(
            TabId::from_raw(next_tab_id),
            launch.scratchpad_dir.as_deref(),
            &mut status,
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
            tabs.push(scratchpad_or_empty_tab(
                TabId::from_raw(next_tab_id),
                launch.scratchpad_dir.as_deref(),
                &mut status,
            ));
        }
    }

    let first = tabs.remove(0);
    EditorModel::from_tabs(first, tabs, status)
}

fn scratchpad_or_empty_tab(
    tab_id: TabId,
    scratchpad_dir: Option<&std::path::Path>,
    status: &mut String,
) -> ModelEditorTab {
    match runtime::create_scratchpad_note(scratchpad_dir) {
        Ok((path, file_stamp)) => ModelEditorTab::scratchpad_with_stamp(tab_id, path, file_stamp),
        Err(err) => {
            *status = if status == "Ready." {
                format!("Failed to create scratchpad: {err}")
            } else {
                format!("{status}; failed to create scratchpad: {err}")
            };
            ModelEditorTab::empty(tab_id, format!("{UNTITLED_PREFIX}-1"))
        }
    }
}

impl Focusable for LstGpuiApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn syntax_highlight_result_is_current(
    model: &EditorModel,
    tab_views: &HashMap<TabId, EditorTabView>,
    tab_id: TabId,
    cache: &Rc<RefCell<ViewportCache>>,
    key: SyntaxHighlightJobKey,
) -> bool {
    model.tab_by_id(tab_id).is_some_and(|tab| {
        tab_views.get(&tab_id).is_some_and(|view| {
            Rc::ptr_eq(&view.cache, cache)
                && tab.revision() == key.revision
                && syntax_mode_for_language(tab.language()) == SyntaxMode::TreeSitter(key.language)
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
    crash_log::install();

    let launch = parse_launch_args();
    let has_graphical_env =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();

    if !has_graphical_env {
        eprintln!("lst requires a graphical session. Run it from a real X11 or Wayland desktop.");
        process::exit(1);
    }

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(editor_keybindings());
        cx.bind_keys(input_keybindings());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(
            None,
            size(px(metrics::WINDOW_WIDTH), px(metrics::WINDOW_HEIGHT)),
            cx,
        );
        let launch = launch.clone();
        let window_title = launch
            .window_title
            .clone()
            .unwrap_or_else(|| "lst".into());
        let window = match cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(window_title.clone().into()),
                    ..Default::default()
                }),
                app_id: Some("lst".to_string()),
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
                    "lst failed to open a GPUI window: {err}. On this host, Xvfb is not sufficient because GPUI surface creation requires a real presentation backend."
                );
                process::exit(1);
            }
        };

        window
            .update(cx, |view, window, cx| {
                window.set_window_title(&window_title);
                let entity = cx.entity();
                window.on_window_should_close(cx, move |_window, cx| {
                    let entity = entity.clone();
                    entity.update(cx, |view, cx| {
                        view.request_quit(cx);
                    });
                    false
                });
                window.focus(&view.focus_handle(cx));
                window.activate_window();
                cx.activate(true);
                view.start_background_tasks(window, cx);
            })
            .unwrap();
    });
}
