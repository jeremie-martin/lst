use gpui::{
    actions, prelude::*, px, size, App, Application, Bounds, ClipboardItem, Context, Entity,
    FocusHandle, Focusable, Pixels, Point, ScrollHandle, Subscription, Window, WindowBounds,
    WindowOptions,
};

mod actions;
mod bench_trace;
mod crash_log;
mod input_adapter;
mod interactions;
mod keymap;
mod launch;
mod recent;
mod runtime;
mod shell;
mod syntax;
#[cfg(test)]
mod tests;
mod ui;
mod viewport;

use crate::ui::{
    input_keybindings,
    theme::{current_theme, current_theme_id, metrics, Theme, ThemeId},
    InputField, InputFieldEvent, InputFieldNavigation,
};
#[cfg(test)]
pub(crate) use input_adapter::{char_range_to_utf16_range, utf16_range_to_char_range_in_text};
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use interactions::drag_autoscroll_delta;
use interactions::ActiveDragSelection;
use keymap::editor_keybindings;
use launch::{parse_launch_args, LaunchArgs};
use lst_editor::{
    EditorModel, EditorTab as ModelEditorTab, FocusTarget, RevealIntent, TabId, UNTITLED_PREFIX,
};
#[cfg(not(test))]
use recent::default_recent_files_path;
use recent::{
    normalize_recent_path, read_recent_preview, search_recent_content, ApplyPreviewOutcome,
    RecentPreviewRead, RecentSelectionMove, RecentView,
};
use ropey::Rope;
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use runtime::autosave_revision_is_current;
use runtime::clipboard::{ExitClipboard, SubprocessExitClipboard};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use syntax::{
    compute_syntax_highlights, syntax_mode_for_language, CachedSyntaxHighlights,
    SyntaxHighlightJobKey, SyntaxMode, SyntaxSpan,
};
#[cfg(all(test, feature = "internal-invariants"))]
pub(crate) use viewport::row_contains_cursor;
use viewport::{
    byte_index_to_char, code_char_width, code_origin_pad, ensure_wrap_layout, line_display_text,
    reset_scroll, scroll_left_for, scroll_to_left, scroll_to_top, scroll_top_for,
    visual_row_for_char, x_for_display_char, ViewportCache, ViewportGeometry, WrapLayoutInput,
};

pub(crate) const RECENT_CARD_BASIS: f32 = 260.0;
#[cfg(not(test))]
const RECENT_CONTENT_SEARCH_DEBOUNCE_MS: u64 = 200;

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
    grab_offset_y: Pixels,
}

#[derive(Clone, Copy, Debug)]
struct EditorHorizontalScrollbarDrag {
    grab_offset_x: Pixels,
}

struct EditorTabView {
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
    editor_horizontal_scrollbar_drag: Option<EditorHorizontalScrollbarDrag>,
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
    autosave_started: bool,
    scratchpad_dir: Option<PathBuf>,
    recent: RecentView,
    force_editor_focus: bool,
    zoom_level: i32,
    exit_clipboard: Arc<dyn ExitClipboard>,
    _shell_subscriptions: Vec<Subscription>,
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
            autosave_started: false,
            scratchpad_dir,
            recent,
            force_editor_focus: false,
            zoom_level: 0,
            exit_clipboard: Arc::new(SubprocessExitClipboard),
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
            find_query_input: self.find_query_input.read(cx).text(),
            find_replace_input: self.find_replace_input.read(cx).text(),
            goto_line_input: self.goto_line_input.read(cx).text(),
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
            zoom_level: self.zoom_level,
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
        let cursor_row = visual_row_for_char(self.active_tab(), &layout.layout)?;
        let scroll_top = scroll_top_for(&active_view.scroll);
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
        self.sync_viewport_state();
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

    pub(crate) fn toggle_recent_files_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.recent.is_open() {
            self.close_recent_files_panel(cx);
            return;
        }

        let pending_search = self.recent.open();
        let seeded_query = self.recent.query().to_string();
        reset_scroll(&self.recent_scroll);
        self.recent_query_input
            .update(cx, |input, cx| input.set_text(&seeded_query, cx));
        let focus_handle = self.recent_query_input.read(cx).focus_handle();
        window.focus(&focus_handle);
        if let Some(generation) = pending_search {
            self.schedule_recent_content_search(generation, cx);
        }
        self.spawn_recent_previews(cx);
        cx.notify();
    }

    fn close_recent_files_panel(&mut self, cx: &mut Context<Self>) {
        if self.recent.is_open() {
            self.recent.close();
            self.force_editor_focus = true;
            cx.notify();
        }
    }

    fn handle_recent_query_input_event(&mut self, event: &InputFieldEvent, cx: &mut Context<Self>) {
        match event {
            InputFieldEvent::Changed(text) => {
                self.update_recent_query(text.clone(), cx);
                cx.notify();
            }
            InputFieldEvent::Submitted => {
                if let Some(path) = self.recent.selected_path() {
                    self.open_recent_path(path, cx);
                }
            }
            InputFieldEvent::Cancelled => self.close_recent_files_panel(cx),
            InputFieldEvent::NextRequested => {
                self.move_recent_selection(RecentSelectionMove::Next, cx);
            }
            InputFieldEvent::PreviousRequested => {
                self.move_recent_selection(RecentSelectionMove::Previous, cx);
            }
            InputFieldEvent::Navigate(navigation) => match navigation {
                InputFieldNavigation::Up => {
                    self.move_recent_selection(RecentSelectionMove::RowPrevious, cx);
                }
                InputFieldNavigation::Down => {
                    self.move_recent_selection(RecentSelectionMove::RowNext, cx);
                }
            },
        }
    }

    fn update_recent_query(&mut self, text: String, cx: &mut Context<Self>) {
        let pending_search = self.recent.set_query(text);
        reset_scroll(&self.recent_scroll);
        self.spawn_recent_previews(cx);
        if let Some(generation) = pending_search {
            self.schedule_recent_content_search(generation, cx);
        }
    }

    fn move_recent_selection(&mut self, movement: RecentSelectionMove, cx: &mut Context<Self>) {
        if let Some(index) = self.recent.move_selection(movement) {
            self.scroll_recent_selection_into_view(index);
        }
        cx.notify();
    }

    fn scroll_recent_selection_into_view(&self, index: usize) {
        let Some(bounds) = self.recent.card_bounds_for(index) else {
            return;
        };

        let viewport = self.recent_scroll.bounds();
        if viewport.size.height <= px(0.0) {
            return;
        }

        let offset_y = self.recent_scroll.offset().y;
        let target = if bounds.top() + offset_y < viewport.top() {
            bounds.top() - viewport.top()
        } else if bounds.bottom() + offset_y > viewport.bottom() {
            bounds.bottom() - viewport.bottom()
        } else {
            return;
        };
        scroll_to_top(&self.recent_scroll, target);
    }

    fn load_more_recent_files(&mut self, cx: &mut Context<Self>) {
        self.recent.load_more();
        self.spawn_recent_previews(cx);
        cx.notify();
    }

    fn spawn_recent_previews(&mut self, cx: &mut Context<Self>) {
        for path in self.recent.paths_to_load_previews() {
            cx.spawn(async move |this, cx| {
                let preview_path = path.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move { read_recent_preview(&preview_path) })
                    .await;
                let _ = this.update(cx, |view, cx| view.finish_recent_preview(path, result, cx));
            })
            .detach();
        }
    }

    fn schedule_recent_content_search(&mut self, generation: u64, cx: &mut Context<Self>) {
        let query = self.recent.query().trim().to_lowercase();
        if query.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(recent_content_search_debounce())
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.recent.search_still_relevant(generation, &query) {
                    view.start_recent_content_search(query, cx);
                }
            });
        })
        .detach();
    }

    fn start_recent_content_search(&mut self, query: String, cx: &mut Context<Self>) {
        if !self.recent.start_content_search(query.clone()) {
            return;
        }

        let paths = self.recent.entries().to_vec();
        cx.spawn(async move |this, cx| {
            let search_query = query.clone();
            let matches = cx
                .background_executor()
                .spawn(async move { search_recent_content(paths, &search_query) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.finish_recent_content_search(query, matches, cx);
            });
        })
        .detach();
    }

    fn finish_recent_content_search(
        &mut self,
        query: String,
        matches: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if self.recent.finish_content_search(query, matches) {
            self.spawn_recent_previews(cx);
            cx.notify();
        }
    }

    fn finish_recent_preview(
        &mut self,
        path: PathBuf,
        result: RecentPreviewRead,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.recent.apply_preview(path, result), ApplyPreviewOutcome::Pruned) {
            self.spawn_recent_previews(cx);
        }
        cx.notify();
    }

    fn open_recent_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = normalize_recent_path(&path);
        if self.activate_existing_tab_for_path(&path, cx) {
            self.recent.record(&path);
            self.close_recent_files_panel(cx);
            return;
        }

        match runtime::read_file_with_stamp(&path) {
            Ok((text, stamp)) => {
                let opened_path = path.clone();
                self.update_model(cx, true, |model| {
                    model.open_files_with_stamps(vec![(opened_path, text, Some(stamp))]);
                });
                self.recent.record(&path);
                self.close_recent_files_panel(cx);
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    self.recent.prune_path(&path);
                    self.spawn_recent_previews(cx);
                }
                self.update_model(cx, true, |model| {
                    model.open_file_failed(path, err.to_string());
                });
            }
        }
    }

    fn activate_existing_tab_for_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let Some(tab_id) = self.model.tabs().iter().find_map(|tab| {
            let tab_path = tab.path()?;
            (normalize_recent_path(tab_path) == path).then_some(tab.id())
        }) else {
            return false;
        };

        self.update_model(cx, true, |model| {
            model.set_active_tab(tab_id);
        });
        true
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
                self.update_model(cx, true, EditorModel::find_next_match);
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
                self.update_model(cx, true, EditorModel::replace_current_match);
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
                self.update_model(cx, true, EditorModel::submit_goto_line_input);
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
        self.tab_views
            .get(&self.model.active_tab_id())
            .expect("active tab must have a tab view")
    }

    fn active_cursor_line_col(&self) -> (usize, usize) {
        char_to_line_col(self.active_tab().buffer(), self.active_tab().cursor_char())
    }

    fn selection_summary(&self) -> Option<String> {
        let selected = self.active_tab().selected_range();
        (selected.start != selected.end)
            .then(|| format!("Sel {}", selected.end.saturating_sub(selected.start)))
    }

    fn painted_wrap_columns(&self) -> Option<usize> {
        self.active_view().geometry.borrow().painted_wrap_columns
    }

    fn status_details(&self) -> String {
        let tab = self.active_tab();
        let (line, column) = self.active_cursor_line_col();
        let mut parts = vec![
            self.model.vim_mode().label().to_string(),
            format!("Ln {}", line + 1),
            format!("Col {}", column + 1),
            if self.model.show_wrap() {
                self.painted_wrap_columns()
                    .map(|columns| format!("Wrap {columns} cols"))
                    .unwrap_or_else(|| "Wrap".to_string())
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
        if self.zoom_level != 0 {
            parts.push(format!("Zoom {:.0}%", self.ui_scale() * 100.0));
        }
        if self.model.find().visible {
            let current = if self.model.find().matches.is_empty() {
                0
            } else {
                self.model.find().active.map_or(0, |index| index + 1)
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
        let wrap_columns = self.active_wrap_columns(window, cx);
        self.update_model(cx, true, |model| {
            model.move_display_rows_by(delta, select, wrap_columns);
        });
    }

    fn active_wrap_columns(&mut self, window: &mut Window, cx: &App) -> usize {
        if !self.model.show_wrap() {
            return usize::MAX;
        }

        let (geometry, cache) = {
            let active_view = self.active_view();
            (active_view.geometry.clone(), active_view.cache.clone())
        };
        let viewport_width = geometry
            .borrow()
            .bounds
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| self.ui_px(metrics::WINDOW_WIDTH - 48.0));
        let char_width = code_char_width(window, self.ui_scale(), self.theme(cx));
        let revision = self.model.active_tab().revision();
        let lines = self.model.active_tab_lines();
        let layout = {
            let mut cache = cache.borrow_mut();
            ensure_wrap_layout(
                &mut cache,
                WrapLayoutInput {
                    lines: lines.as_ref(),
                    revision,
                    viewport_width,
                    char_width,
                    show_gutter: self.model.show_gutter(),
                    show_wrap: self.model.show_wrap(),
                    scale: self.ui_scale(),
                },
            )
        };
        layout.wrap_columns
    }

    fn move_page(&mut self, down: bool, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        let wrap_columns = self.active_wrap_columns(window, cx);
        self.update_model(cx, true, |model| {
            if down {
                model.page_down(select, wrap_columns);
            } else {
                model.page_up(select, wrap_columns);
            }
        });
    }

    fn sync_viewport_state(&mut self) {
        let bounds = self.active_view().geometry.borrow().bounds;
        let Some(bounds) = bounds else {
            return;
        };
        let row_height = self.ui_px(metrics::ROW_HEIGHT);
        if row_height <= px(0.0) || bounds.size.height <= px(0.0) {
            return;
        }
        let rows = ((bounds.size.height / row_height).floor() as usize).max(1);
        let scroll_top = scroll_top_for(&self.active_view().scroll);
        let top = (scroll_top / row_height).floor() as usize;
        self.model.set_viewport_rows(rows);
        self.model.set_viewport_top(top);
    }

    fn queue_cursor_reveal(&mut self, intent: RevealIntent) {
        self.pending_reveal = Some(intent);
    }

    fn schedule_pending_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_reveal.is_none() || self.reveal_scheduled {
            return;
        }

        self.reveal_scheduled = true;
        cx.on_next_frame(window, |this, window, cx| {
            this.reveal_scheduled = false;
            this.flush_pending_reveal(window, cx);
        });
        cx.notify();
    }

    fn flush_pending_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(intent) = self.pending_reveal.take() else {
            return;
        };

        if !self.try_reveal_active_cursor(intent, window, cx) {
            self.pending_reveal = Some(intent);
            self.schedule_pending_reveal(window, cx);
        }
    }

    /// `cx.on_next_frame` may not fire under `run_until_parked` before the next paint commits.
    #[cfg(test)]
    fn flush_pending_reveal_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal_scheduled = false;
        self.flush_pending_reveal(window, cx);
    }

    fn active_cursor_visual_row(&self) -> Option<usize> {
        let tab = self.active_tab();
        let view = self.active_view();

        if self.model.show_wrap() {
            let cache = view.cache.borrow();
            let cached = cache.wrap_layout.as_ref()?;
            if cached.revision != tab.revision() || !cached.layout.show_wrap {
                return None;
            }
            visual_row_for_char(tab, &cached.layout)
        } else {
            Some(tab.buffer().char_to_line(tab.cursor_char()))
        }
    }

    fn try_reveal_active_cursor(&self, intent: RevealIntent, window: &mut Window, cx: &App) -> bool {
        let view = self.active_view();
        let viewport_bounds = {
            let geometry = view.geometry.borrow();
            let Some(bounds) = geometry.bounds else {
                return false;
            };
            bounds
        };
        if viewport_bounds.size.height <= px(0.) {
            return false;
        }

        let Some(visual_row) = self.active_cursor_visual_row() else {
            return false;
        };

        let row_height = self.ui_px(metrics::ROW_HEIGHT);
        let caret_top = row_height * visual_row as f32;
        let caret_bottom = caret_top + row_height;
        let scroll_top = scroll_top_for(&view.scroll);
        let viewport_height = viewport_bounds.size.height;
        let margin = row_height * self.model.viewport().effective_scrolloff() as f32;

        let target = match intent {
            RevealIntent::NearestEdge => {
                if caret_top < scroll_top + margin {
                    Some(caret_top - margin)
                } else if caret_bottom > scroll_top + viewport_height - margin {
                    Some(caret_bottom + margin - viewport_height)
                } else {
                    None
                }
            }
            RevealIntent::Center => Some(caret_top - (viewport_height - row_height) / 2.0),
            RevealIntent::Top => Some(caret_top - margin),
            RevealIntent::Bottom => Some(caret_bottom + margin - viewport_height),
        };

        if let Some(target) = target {
            scroll_to_top(&view.scroll, target);
        }

        if !self.model.show_wrap() {
            self.try_reveal_active_cursor_horizontally(view, viewport_bounds, window, cx);
        }
        true
    }

    fn try_reveal_active_cursor_horizontally(
        &self,
        view: &EditorTabView,
        viewport_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &App,
    ) {
        let geometry = view.geometry.borrow();
        let char_width = geometry.painted_char_width;
        if char_width <= px(0.0) {
            return;
        }

        let scroll_left = scroll_left_for(&view.scroll);
        let pad = code_origin_pad(self.model.show_gutter(), self.ui_scale());
        let visible_width = (viewport_bounds.size.width - pad).max(px(0.0));
        if visible_width <= px(0.0) {
            return;
        }

        let visible_cols = ((visible_width / px(1.0)) / (char_width / px(1.0))).floor() as usize;
        if visible_cols == 0 {
            return;
        }

        let cursor_x = self.active_cursor_rendered_x(char_width, window, cx);

        let raw_margin = self.model.viewport().sidescrolloff;
        let margin_cols = if visible_cols <= 1 {
            0
        } else {
            raw_margin.min((visible_cols - 1) / 2)
        };
        let margin = char_width * margin_cols as f32;

        let target_x = if cursor_x < scroll_left + margin {
            Some(cursor_x - margin)
        } else if cursor_x > scroll_left + visible_width - margin {
            Some(cursor_x + margin - visible_width)
        } else {
            None
        };

        if let Some(target_x) = target_x {
            drop(geometry);
            scroll_to_left(&view.scroll, target_x);
        }
    }

    fn active_cursor_rendered_x(&self, char_width: Pixels, window: &mut Window, cx: &App) -> Pixels {
        let tab = self.active_tab();
        let cursor = tab.cursor_char().min(tab.buffer().len_chars());
        let line = tab.buffer().char_to_line(cursor);
        let line_start = tab.buffer().line_to_char(line);
        let display_text = line_display_text(tab.buffer(), line);
        let column = cursor
            .saturating_sub(line_start)
            .min(display_text.as_ref().chars().count());
        x_for_display_char(
            display_text.as_ref(),
            column,
            char_width,
            self.ui_scale(),
            self.theme(cx),
            window,
        )
    }

    fn sync_primary_selection(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.active_tab().selected_text() {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
    }

    fn active_char_index_for_point(&self, point: Point<Pixels>) -> usize {
        let active_view = self.active_view();
        let geometry = active_view.geometry.borrow();
        let Some(bounds) = geometry.bounds else {
            return self.active_tab().cursor_char();
        };
        // If the scroll has moved since the last paint (e.g. a Reveal just
        // repositioned the viewport), `geometry.rows` describes the previous
        // scroll position and mapping a click through it would land on the
        // wrong row. Bail and keep the cursor where it is; the next paint
        // will refresh `geometry.rows` and subsequent clicks behave normally.
        // Skip the guard during drag-autoscroll: the drag loop imperatively
        // nudges scroll between frames and still needs the cursor to track.
        const SCROLL_STALE_THRESHOLD: f32 = 0.5;
        let current_scroll_top = scroll_top_for(&active_view.scroll);
        let current_scroll_left = scroll_left_for(&active_view.scroll);
        if self.selection_drag.is_none() {
            if (current_scroll_top - geometry.scroll_top_at_paint).abs()
                > px(SCROLL_STALE_THRESHOLD)
                || (current_scroll_left - geometry.scroll_left_at_paint).abs()
                    > px(SCROLL_STALE_THRESHOLD)
            {
                return self.active_tab().cursor_char();
            }
        }
        let code_origin_x =
            bounds.left() + code_origin_pad(self.model.show_gutter(), self.ui_scale());

        let row = if geometry.rows.is_empty() {
            return 0;
        } else if point.y <= geometry.rows[0].row_top {
            &geometry.rows[0]
        } else {
            geometry
                .rows
                .iter()
                .find(|row| {
                    point.y >= row.row_top
                        && point.y < row.row_top + self.ui_px(metrics::ROW_HEIGHT)
                })
                .unwrap_or_else(|| geometry.rows.last().expect("checked above"))
        };

        let x = if point.x >= code_origin_x {
            point.x - code_origin_x + current_scroll_left
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
    pub(crate) zoom_level: i32,
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
        match runtime::create_scratchpad_note(launch.scratchpad_dir.as_deref()) {
            Ok((path, file_stamp)) => {
                tabs.push(ModelEditorTab::scratchpad_with_stamp(
                    TabId::from_raw(next_tab_id),
                    path,
                    file_stamp,
                ));
            }
            Err(err) => {
                status = format!("Failed to create scratchpad: {err}");
                tabs.push(ModelEditorTab::empty(
                    TabId::from_raw(next_tab_id),
                    format!("{UNTITLED_PREFIX}-1"),
                ));
            }
        }
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
            match runtime::create_scratchpad_note(launch.scratchpad_dir.as_deref()) {
                Ok((path, file_stamp)) => {
                    tabs.push(ModelEditorTab::scratchpad_with_stamp(
                        TabId::from_raw(next_tab_id),
                        path,
                        file_stamp,
                    ));
                }
                Err(err) => {
                    status = format!("{status}; failed to create scratchpad: {err}");
                    tabs.push(ModelEditorTab::empty(
                        TabId::from_raw(next_tab_id),
                        format!("{UNTITLED_PREFIX}-1"),
                    ));
                }
            }
        }
    }

    let first = tabs.remove(0);
    EditorModel::from_tabs(first, tabs, status)
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

fn recent_content_search_debounce() -> Duration {
    #[cfg(test)]
    {
        Duration::from_millis(0)
    }
    #[cfg(not(test))]
    {
        Duration::from_millis(RECENT_CONTENT_SEARCH_DEBOUNCE_MS)
    }
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
