use gpui::{
    prelude::*, px, size, App, Application, Bounds, Context, Entity, FocusHandle, Focusable, Modifiers, Pixels, Point,
    ScrollHandle, Subscription, Window, WindowAppearance, WindowBounds, WindowOptions,
};

mod build_info;
mod command_ui;
mod diagnostics;
mod editor_view;
mod input;
mod launch;
mod llm;
mod recent;
mod runtime;
mod settings;
mod settings_ui;
mod shell;
mod state_trace;
mod syntax;
mod ui;
mod viewport;
mod workspace_action;

use crate::ui::{
    input_keybindings,
    theme::{current_theme, current_theme_id, metrics, typography, Theme, ThemeId},
    InputField, InputFieldEvent,
};
use input::ActiveDragSelection;
use launch::{parse_launch_args, LaunchArgs};
use lst_editor::{
    selection::identifier_range_at_char, EditorCommand as Command, EditorModel, EditorTab as ModelEditorTab, FileStamp,
    FocusTarget, InputMode, Position, RevealIntent, Selection, TabId, UNTITLED_PREFIX,
};
use recent::{default_recent_files_path, normalize_recent_path, RecentOrigin, RecentView};
use ropey::Rope;
use settings::{InputModeSetting, SettingsStore, ThemePreference};
use state_trace::StateTraceEmitter;
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    process,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Instant,
};
use syntax::{
    syntax_mode_for_language, CachedSyntaxHighlights, SyntaxInvalidation, SyntaxLanguage, SyntaxMode, TabSyntaxState,
};
use viewport::{scroll_to_left, ViewportCache, ViewportGeometry};
use workspace_action::editor_keybindings;
use workspace_action::WorkspaceCommand;

pub(crate) const RECENT_CARD_BASIS: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitSaveContinuation {
    CloseTab,
    QuitSingle,
    QuitReview,
    QuitScratchpad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingExitSave {
    tab_id: TabId,
    continuation: ExitSaveContinuation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScratchpadClipboardPayload {
    tab_id: TabId,
    revision: u64,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClosePromptIntent {
    CloseTab,
    Quit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ClosePromptStatus {
    Reviewing,
    Saving,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ClosePrompt {
    tab_id: TabId,
    intent: ClosePromptIntent,
    status: ClosePromptStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuitReviewDecision {
    Save,
    Discard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum QuitReviewItemStatus {
    Pending,
    Saving,
    Saved,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QuitReviewItem {
    tab_id: TabId,
    identity: String,
    decision: QuitReviewDecision,
    status: QuitReviewItemStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QuitReview {
    items: Vec<QuitReviewItem>,
    selected_index: usize,
    saving_scratchpad: bool,
    failed_scratchpad: Option<TabId>,
    message: Option<String>,
}

/// A non-modal warning tied to one open document. The disk stamp is the
/// exact version the user is deciding about, so dismissing one warning never
/// suppresses a later external edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileConflictNotice {
    tab_id: TabId,
    path: PathBuf,
    disk_stamp: FileStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CleanupConfirmation {
    tab_id: TabId,
    revision: u64,
}

impl QuitReview {
    fn new(items: impl IntoIterator<Item = (TabId, String)>) -> Self {
        let items = items
            .into_iter()
            .map(|(tab_id, identity)| QuitReviewItem {
                tab_id,
                identity,
                decision: QuitReviewDecision::Save,
                status: QuitReviewItemStatus::Pending,
            })
            .collect::<Vec<_>>();
        debug_assert!(items.len() >= 2, "quit review is only for multiple dirty documents");
        Self {
            items,
            selected_index: 0,
            saving_scratchpad: false,
            failed_scratchpad: None,
            message: None,
        }
    }

    fn is_running(&self) -> bool {
        self.saving_scratchpad
            || self
                .items
                .iter()
                .any(|item| matches!(item.status, QuitReviewItemStatus::Saving))
    }
}

#[cfg(test)]
mod quit_review_state_tests {
    use super::*;

    #[test]
    fn review_defaults_every_document_to_pending_save() {
        let review = QuitReview::new([
            (TabId::from_raw(1), "/tmp/one.txt".to_string()),
            (TabId::from_raw(2), "/tmp/two.txt".to_string()),
        ]);

        assert_eq!(review.selected_index, 0);
        assert!(!review.is_running());
        assert!(review
            .items
            .iter()
            .all(|item| item.decision == QuitReviewDecision::Save));
        assert!(review
            .items
            .iter()
            .all(|item| matches!(item.status, QuitReviewItemStatus::Pending)));
    }

    #[test]
    fn saving_item_or_scratchpad_makes_review_transaction_running() {
        let mut review = QuitReview::new([
            (TabId::from_raw(1), "/tmp/one.txt".to_string()),
            (TabId::from_raw(2), "/tmp/two.txt".to_string()),
        ]);
        review.items[1].status = QuitReviewItemStatus::Saving;
        assert!(review.is_running());

        review.items[1].status = QuitReviewItemStatus::Saved;
        review.saving_scratchpad = true;
        assert!(review.is_running());
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkspaceSurface {
    #[default]
    None,
    CommandPalette,
    Settings,
    TabList,
    AppMenu,
    LanguageMenu,
    ContextMenu,
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
    syntax_state: Option<crate::syntax::TabSyntaxState>,
}

impl EditorTabView {
    fn new(tab: &ModelEditorTab) -> Self {
        Self {
            revision: tab.revision(),
            scroll: ScrollHandle::new(),
            cache: Rc::new(RefCell::new(ViewportCache::default())),
            geometry: Rc::new(RefCell::new(ViewportGeometry::default())),
            syntax_state: None,
        }
    }

    fn invalidate_visual_state(&mut self) {
        self.cache.borrow_mut().invalidate_content_layout();
        // geometry is intentionally NOT reset here: every field gets
        // overwritten by the next viewport prepare anyway, and resetting
        // makes the status bar paint one frame of stale "no painted
        // geometry" state (e.g. wrap-column count dropping to "Wrap"
        // instead of "Wrap 80 cols") on every edit, since the status bar
        // runs above the viewport canvas in the render tree.
        //
        // syntax_state is intentionally NOT cleared here either: it owns
        // the persistent tree-sitter Tree that makes incremental
        // reparses cheap. Cleared only on language change or
        // unrecoverable parse failure, both handled inside
        // sync_active_syntax_state.
    }
}

struct LstGpuiApp {
    focus_handle: FocusHandle,
    /// Real focus target for non-input workspace surfaces. This is separate
    /// from the editor input handle so IME/text events cannot mutate the
    /// document behind menus, settings, or close prompts.
    surface_focus_handle: FocusHandle,
    find_query_focus_handle: FocusHandle,
    find_replace_focus_handle: FocusHandle,
    goto_line_focus_handle: FocusHandle,
    recent_focus_handle: FocusHandle,
    command_palette_focus_handle: FocusHandle,
    settings_search_focus_handle: FocusHandle,
    window_title_override: Option<String>,
    model: EditorModel,
    tab_views: HashMap<TabId, EditorTabView>,
    tab_bar_scroll: ScrollHandle,
    command_palette_scroll: ScrollHandle,
    workspace_surface_scroll: ScrollHandle,
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
    command_palette_input: Entity<InputField>,
    settings_search_input: Entity<InputField>,
    settings_scroll: ScrollHandle,
    quit_review_scroll: ScrollHandle,
    settings_overlay: settings_ui::SettingsOverlay,
    settings_selection: settings_ui::SettingsSelection,
    workspace_surface: WorkspaceSurface,
    /// The keyboard/hover selection for whichever non-input workspace menu
    /// is currently open. Only one such surface can exist at a time, so a
    /// single index keeps selection ownership aligned with the surface.
    workspace_surface_selected: usize,
    command_palette_selected: usize,
    pending_workspace_command: Option<WorkspaceCommand>,
    context_menu_position: Option<Point<Pixels>>,
    focus_target: FocusTarget,
    focus_last_applied: FocusTarget,
    pending_exit_save: Option<PendingExitSave>,
    close_prompt: Option<ClosePrompt>,
    quit_review: Option<QuitReview>,
    exit_discarded_revisions: HashMap<TabId, u64>,
    pending_reveal: Option<RevealIntent>,
    reveal_scheduled: bool,
    autosave_inflight: HashSet<PathBuf>,
    autosave_observed_revisions: HashMap<TabId, (u64, Instant)>,
    save_inflight: HashMap<PathBuf, usize>,
    queued_saves: HashMap<PathBuf, runtime::QueuedSaveJob>,
    save_ticket_generations: HashMap<PathBuf, Arc<Mutex<u64>>>,
    file_conflicts: HashMap<TabId, FileConflictNotice>,
    autosave_started: bool,
    maintenance_inflight: bool,
    settings_reload_inflight: bool,
    settings_generation: u64,
    scratchpad_dir: Option<PathBuf>,
    recent: RecentView,
    force_editor_focus: bool,
    modifier_chord_accumulated: Modifiers,
    recent_modifier_chord: Option<(Modifiers, Instant)>,
    x11_ctrl_k_pending: bool,
    zoom_level: i32,
    state_trace: StateTraceEmitter,
    cleanup_in_flight: bool,
    cleanup_confirmation: Option<CleanupConfirmation>,
    cleanup_message: Option<String>,
    clipboard_quit_bypass: Option<ScratchpadClipboardPayload>,
    /// A passive word highlight exists only after focus or a cursor-only
    /// transition. Its source revision makes an edit invalidate it by
    /// construction instead of letting paint infer a new query from the
    /// post-edit caret position.
    passive_occurrence_query: Option<PassiveOccurrenceQuery>,
    cursor_visible: bool,
    /// Surfaced through the state trace so real-X11 tests can click the
    /// find chips without relying on fixed shell geometry.
    find_chip_bounds_px: FindChipBounds,
    /// Surfaced through the state trace so real-X11 tests can click tab-strip
    /// chrome without relying on fixed shell geometry.
    app_menu_button_bounds_px: Option<Bounds<Pixels>>,
    all_tabs_button_bounds_px: Option<Bounds<Pixels>>,
    recent_button_bounds_px: Option<Bounds<Pixels>>,
    new_tab_button_bounds_px: Option<Bounds<Pixels>>,
    file_conflict_button_bounds_px: FileConflictButtonBounds,
    /// Surfaced through the state trace so real-X11 tests can click the
    /// button without depending on theme-name / status-details widths.
    cleanup_button_bounds_px: Option<Bounds<Pixels>>,
    /// Surfaced through the state trace so real-X11 tests can click the
    /// visible theme button without depending on fixed status-bar geometry.
    theme_button_bounds_px: Option<Bounds<Pixels>>,
    theme_name_rendered: String,
    status_details_rendered: String,
    /// Stack of recently closed path-backed tabs, most-recent-last. `Ctrl+Shift+T`
    /// pops the top entry and reopens it with the cursor restored. Bounded
    /// so a long-lived editor session does not grow this unboundedly.
    closed_tabs_history: Vec<ClosedTabRecord>,
    settings: SettingsStore,
    input_mode_cli_override: bool,
    _shell_subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PassiveOccurrenceQuery {
    tab_id: TabId,
    revision: u64,
    word: String,
}

#[derive(Clone, Debug)]
struct ClosedTabRecord {
    path: PathBuf,
    position: Position,
    origin: RecentOrigin,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FindChipBounds {
    case_sensitive: Option<Bounds<Pixels>>,
    whole_word: Option<Bounds<Pixels>>,
    regex: Option<Bounds<Pixels>>,
    scope: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FileConflictButtonBounds {
    reload: Option<Bounds<Pixels>>,
    keep_mine: Option<Bounds<Pixels>>,
    save_as: Option<Bounds<Pixels>>,
    dismiss: Option<Bounds<Pixels>>,
}

impl LstGpuiApp {
    fn new(cx: &mut Context<Self>, launch: LaunchArgs, settings: SettingsStore) -> Self {
        typography::set_primary_font_family(&settings.settings.editor.font_family);
        metrics::set_code_font_size(f32::from(settings.settings.editor.font_size));
        let initial_theme = theme_for_preference(settings.settings.appearance.theme, cx.window_appearance());
        cx.set_global(initial_theme);
        let find_query_input = cx.new(|cx| InputField::new(cx, "Find").with_key_context("Find"));
        let find_replace_input = cx.new(|cx| InputField::new(cx, "Replace").with_key_context("Find"));
        let goto_line_input = cx.new(|cx| InputField::new(cx, "Line[:Column]"));
        let recent_query_input = cx.new(|cx| InputField::new(cx, "Search recent files").with_vertical_navigation());
        let command_palette_input = cx.new(|cx| {
            InputField::new(cx, "Type a command")
                .with_key_context("CommandPalette")
                .with_vertical_navigation()
        });
        let settings_search_input =
            cx.new(|cx| InputField::new(cx, "Search settings and keybindings").with_key_context("Settings"));
        let recent_focus_handle = recent_query_input.read(cx).focus_handle();
        let command_palette_focus_handle = command_palette_input.read(cx).focus_handle();
        let settings_search_focus_handle = settings_search_input.read(cx).focus_handle();
        let find_query_focus_handle = find_query_input.read(cx).focus_handle();
        let find_replace_focus_handle = find_replace_input.read(cx).focus_handle();
        let goto_line_focus_handle = goto_line_input.read(cx).focus_handle();
        let recent_files_path = default_recent_files_path();
        let scratchpad_dir = launch
            .scratchpad_dir
            .clone()
            .or_else(|| settings.settings.files.scratchpad_directory.clone());
        let mut model = initial_model_from_launch(launch.clone(), scratchpad_dir.as_deref());
        let configured_mode = match settings.settings.editor.input_mode {
            InputModeSetting::Standard => InputMode::Standard,
            InputModeSetting::Vim => InputMode::Vim,
        };
        model.set_input_mode(launch.input_mode.unwrap_or(configured_mode));
        model.set_show_wrap(settings.settings.editor.word_wrap);
        model.set_gutter_mode(match settings.settings.editor.line_numbers {
            settings::LineNumbersSetting::Absolute => lst_editor::GutterMode::Absolute,
            settings::LineNumbersSetting::Relative => lst_editor::GutterMode::Relative,
            settings::LineNumbersSetting::Hybrid => lst_editor::GutterMode::Hybrid,
        });
        let mut recent = RecentView::load(recent_files_path);
        for tab in model.tabs() {
            if !tab.is_scratchpad() {
                if let Some(path) = tab.path() {
                    recent.record_with_origin(path, RecentOrigin::Regular);
                }
            }
        }

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            surface_focus_handle: cx.focus_handle(),
            find_query_focus_handle,
            find_replace_focus_handle,
            goto_line_focus_handle,
            recent_focus_handle,
            command_palette_focus_handle,
            settings_search_focus_handle,
            window_title_override: launch.window_title.clone(),
            model,
            tab_views: HashMap::new(),
            tab_bar_scroll: ScrollHandle::new(),
            command_palette_scroll: ScrollHandle::new(),
            workspace_surface_scroll: ScrollHandle::new(),
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
            command_palette_input: command_palette_input.clone(),
            settings_search_input: settings_search_input.clone(),
            settings_scroll: ScrollHandle::new(),
            quit_review_scroll: ScrollHandle::new(),
            settings_overlay: settings_ui::SettingsOverlay::None,
            settings_selection: settings_ui::SettingsSelection::default(),
            workspace_surface: WorkspaceSurface::None,
            workspace_surface_selected: 0,
            command_palette_selected: 0,
            pending_workspace_command: None,
            context_menu_position: None,
            focus_target: FocusTarget::Editor,
            focus_last_applied: FocusTarget::Editor,
            pending_exit_save: None,
            close_prompt: None,
            quit_review: None,
            exit_discarded_revisions: HashMap::new(),
            pending_reveal: None,
            reveal_scheduled: false,
            autosave_inflight: HashSet::new(),
            autosave_observed_revisions: HashMap::new(),
            save_inflight: HashMap::new(),
            queued_saves: HashMap::new(),
            save_ticket_generations: HashMap::new(),
            file_conflicts: HashMap::new(),
            autosave_started: false,
            maintenance_inflight: false,
            settings_reload_inflight: false,
            settings_generation: 0,
            scratchpad_dir,
            recent,
            force_editor_focus: false,
            modifier_chord_accumulated: Modifiers::default(),
            recent_modifier_chord: None,
            x11_ctrl_k_pending: false,
            zoom_level: settings.settings.appearance.zoom_level,
            state_trace: StateTraceEmitter::from_env(),
            cleanup_in_flight: false,
            cleanup_confirmation: None,
            cleanup_message: None,
            clipboard_quit_bypass: None,
            passive_occurrence_query: None,
            cursor_visible: true,
            find_chip_bounds_px: FindChipBounds::default(),
            app_menu_button_bounds_px: None,
            all_tabs_button_bounds_px: None,
            recent_button_bounds_px: None,
            new_tab_button_bounds_px: None,
            file_conflict_button_bounds_px: FileConflictButtonBounds::default(),
            cleanup_button_bounds_px: None,
            theme_button_bounds_px: None,
            theme_name_rendered: initial_theme.theme().name.to_string(),
            status_details_rendered: String::new(),
            closed_tabs_history: Vec::new(),
            input_mode_cli_override: launch.input_mode.is_some(),
            settings,
            _shell_subscriptions: Vec::new(),
        };
        let show_wrap = app.model.show_wrap();
        app.sync_tab_views(show_wrap);
        // The production window focuses the editor immediately after
        // construction, which is itself a valid occurrence trigger.
        app.refresh_passive_occurrence_query();

        app._shell_subscriptions
            .push(cx.subscribe(&find_query_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_find_query_input_event(event, cx)
            }));
        app._shell_subscriptions.push(
            cx.subscribe(&find_replace_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_find_replace_input_event(event, cx)
            }),
        );
        app._shell_subscriptions
            .push(cx.subscribe(&goto_line_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_goto_line_input_event(event, cx)
            }));
        app._shell_subscriptions.push(
            cx.subscribe(&recent_query_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_recent_query_input_event(event, cx)
            }),
        );
        app._shell_subscriptions.push(
            cx.subscribe(&command_palette_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_command_palette_input_event(event, cx)
            }),
        );
        app._shell_subscriptions.push(
            cx.subscribe(&settings_search_input, |this, _, event: &InputFieldEvent, cx| {
                this.handle_settings_search_input_event(event, cx)
            }),
        );

        app
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

    fn set_zoom_level(&mut self, level: i32, window: &mut Window, cx: &mut Context<Self>) {
        let level = level.clamp(metrics::MIN_ZOOM_LEVEL, metrics::MAX_ZOOM_LEVEL);
        if self.zoom_level == level {
            return;
        }

        self.zoom_level = level;
        self.settings.settings.appearance.zoom_level = level;
        window.set_rem_size(self.ui_px(metrics::BASE_REM_SIZE));
        for view in self.tab_views.values_mut() {
            view.invalidate_visual_state();
        }
        self.persist_settings(cx);
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
            if target == FocusTarget::Editor {
                self.refresh_passive_occurrence_query();
            } else {
                self.passive_occurrence_query = None;
            }
            diagnostics::record_label("focus_queued", focus_trace_label(target));
            self.focus_target = target;
        }
    }

    fn apply_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_prompt.is_some() || self.quit_review.is_some() || self.cleanup_confirmation.is_some() {
            if !self.surface_focus_handle.is_focused(window) {
                window.focus(&self.surface_focus_handle);
            }
            return;
        }
        if self.workspace_surface == WorkspaceSurface::Settings {
            let handle = if self.settings_overlay.wants_surface_focus() || self.settings_selection.is_active() {
                self.surface_focus_handle.clone()
            } else {
                self.settings_search_input.read(cx).focus_handle()
            };
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
            return;
        }
        if matches!(
            self.workspace_surface,
            WorkspaceSurface::TabList
                | WorkspaceSurface::AppMenu
                | WorkspaceSurface::LanguageMenu
                | WorkspaceSurface::ContextMenu
        ) {
            if !self.surface_focus_handle.is_focused(window) {
                window.focus(&self.surface_focus_handle);
            }
            return;
        }
        if self.workspace_surface == WorkspaceSurface::CommandPalette {
            let handle = self.command_palette_input.read(cx).focus_handle();
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
            return;
        }
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
            self.refresh_passive_occurrence_query();
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
            diagnostics::record_label(label, focus_trace_label(target));
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
        self.file_conflicts
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
        if self.hovered_tab.is_some_and(|ix| ix >= self.model.tab_count()) {
            self.hovered_tab = None;
        }
    }

    fn active_scratchpad_clipboard_payload(&self) -> Option<ScratchpadClipboardPayload> {
        let tab = self.model.active_tab();
        if !tab.is_scratchpad() {
            return None;
        }
        let text = tab.buffer_text();
        (!text.trim().is_empty()).then(|| ScratchpadClipboardPayload {
            tab_id: tab.id(),
            revision: tab.revision(),
            text,
        })
    }

    /// Cheap identity of what PRIMARY would hold: the active tab, its edit
    /// revision, and its primary selection. Changes exactly when the selected
    /// text could have changed, without materializing it.
    fn primary_selection_state(&self) -> (lst_editor::TabId, u64, lst_editor::Selection) {
        let tab = self.model.active_tab();
        (tab.id(), tab.revision(), tab.selection())
    }

    fn refresh_passive_occurrence_query(&mut self) {
        let tab = self.model.active_tab();
        let selection = tab.selection();
        self.passive_occurrence_query = (!selection.has_selection())
            .then(|| identifier_range_at_char(tab.buffer(), selection.head()))
            .flatten()
            .map(|range| PassiveOccurrenceQuery {
                tab_id: tab.id(),
                revision: tab.revision(),
                word: tab.buffer().slice(range).to_string(),
            });
    }

    fn reconcile_passive_occurrence_query(&mut self, old: &(TabId, u64, Selection), new: &(TabId, u64, Selection)) {
        if old.0 != new.0 {
            self.refresh_passive_occurrence_query();
        } else if old.1 != new.1 {
            // Text input, edits, undo, reload, and every other buffer mutation
            // invalidate the trigger. A later explicit cursor move or focus
            // transition creates a fresh query.
            self.passive_occurrence_query = None;
        } else if old.2 != new.2 {
            self.refresh_passive_occurrence_query();
        }
    }

    fn update_model(
        &mut self,
        cx: &mut Context<Self>,
        notify_after_update: bool,
        update: impl FnOnce(&mut EditorModel),
    ) {
        self.cursor_visible = true;
        if !self.cleanup_in_flight {
            self.cleanup_message = None;
        }
        self.sync_viewport_state();
        let old_show_wrap = self.model.show_wrap();
        let old_active_index = self.model.active_index();
        let old_find_state = self.find_input_state();
        let old_goto_line = self.model.goto_line().map(ToOwned::to_owned);
        let old_primary_state = self.primary_selection_state();
        update(&mut self.model);
        if self
            .clipboard_quit_bypass
            .as_ref()
            .is_some_and(|bypass| Some(bypass) != self.active_scratchpad_clipboard_payload().as_ref())
        {
            self.clipboard_quit_bypass = None;
        }
        if self.model.active_index() != old_active_index {
            self.tab_bar_scroll.scroll_to_item(self.model.active_index());
        }
        let new_primary_state = self.primary_selection_state();
        self.reconcile_passive_occurrence_query(&old_primary_state, &new_primary_state);
        // Own the X11 PRIMARY selection only when the selection changed within
        // the same tab: switching tabs must not clobber another application's
        // selection with this tab's stale one. Comparing (tab, revision,
        // selection) also avoids materializing the selected text on every
        // input event just to detect a change.
        if new_primary_state != old_primary_state && new_primary_state.0 == old_primary_state.0 {
            if let Some(text) = self.model.active_tab().selected_text().filter(|text| !text.is_empty()) {
                cx.write_to_primary(gpui::ClipboardItem::new_string(text));
            }
        }
        self.sync_tab_views(old_show_wrap);
        self.sync_active_syntax_state();
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

    /// Keeps the active tab's syntax highlights in sync with the buffer.
    /// Runs synchronously on the UI thread: an initial parse takes a few
    /// ms for a typical file, an incremental reparse takes microseconds
    /// thanks to the persistent `tree_sitter::Tree`. Called once per
    /// `update_model` (catches edits + tab switches) and once before each
    /// paint via `ensure_active_syntax_state` (catches files that were
    /// opened without an edit cycle).
    fn sync_active_syntax_state(&mut self) {
        let active_id = self.model.active_tab_id();
        let (language, revision, buffer) = {
            let tab = self.model.active_tab();
            (tab.language(), tab.revision(), tab.buffer().clone())
        };
        let syntax_mode = syntax_mode_for_language(language);
        let delta = self.model.take_active_buffer_delta();
        let Some(view) = self.tab_views.get_mut(&active_id) else {
            return;
        };

        let SyntaxMode::TreeSitter(syntax_lang) = syntax_mode else {
            view.syntax_state = None;
            let mut cache = view.cache.borrow_mut();
            let previous_line_count = cache
                .wrap_layout
                .as_ref()
                .map(|layout| layout.layout.line_row_starts.len().saturating_sub(1));
            let invalidation = SyntaxInvalidation::from_buffer_delta(&buffer, &delta, previous_line_count);
            cache.patch_wrap_layout(&buffer, revision, &invalidation);
            if cache.syntax_highlights.is_some() {
                cache.syntax_highlights = None;
                cache.clear_code_lines();
            }
            return;
        };

        // Drop the existing state when the language switched out from under
        // us so we don't feed the wrong parser an incremental edit.
        let language_changed = view
            .syntax_state
            .as_ref()
            .is_some_and(|state| state.language != syntax_lang);
        if language_changed {
            view.syntax_state = None;
        }

        let already_current = view
            .syntax_state
            .as_ref()
            .is_some_and(|state| state.language == syntax_lang && state.revision == revision);
        if already_current {
            if !syntax_cache_is_current(&view.cache.borrow(), syntax_lang, revision) {
                if let Some(state) = view.syntax_state.as_ref() {
                    let mut cache = view.cache.borrow_mut();
                    refresh_syntax_cache(&mut cache, state, &buffer, revision, SyntaxInvalidation::Full);
                }
            }
            return;
        }

        let invalidation = match view.syntax_state.as_mut() {
            None => {
                // Freshly-opened tab or recovery after a parse failure /
                // language switch. The delta we drained at line above is
                // discarded on purpose: parse_initial reads the whole
                // buffer, so any pending edits are already reflected.
                let _ = delta;
                view.syntax_state = TabSyntaxState::parse_initial(syntax_lang, &buffer, revision);
                SyntaxInvalidation::Full
            }
            Some(state) => state.update(&buffer, delta, revision),
        };
        let Some(state) = view.syntax_state.as_ref() else {
            // parse_initial failed (grammar / ABI mismatch). Wipe any
            // stale spans so the renderer doesn't keep painting last
            // language's colors using the byte-length guard.
            let mut cache = view.cache.borrow_mut();
            if cache.syntax_highlights.is_some() {
                cache.syntax_highlights = None;
                cache.clear_code_lines();
            }
            return;
        };
        let mut cache = view.cache.borrow_mut();
        refresh_syntax_cache(&mut cache, state, &buffer, revision, invalidation);
    }

    /// Render-path entry point: ensures the active tab has up-to-date
    /// highlights, even for files that were opened without going through
    /// an `update_model` cycle (e.g. on first paint). Cheap when nothing
    /// has changed.
    pub(crate) fn ensure_active_syntax_state(&mut self) {
        self.sync_active_syntax_state();
    }

    fn execute_model_command(&mut self, cx: &mut Context<Self>, command: Command) {
        let trace_label = command_trace_label(command);
        let trace_started = diagnostics::trace_enabled().then(Instant::now);
        self.update_model(cx, true, |model| model.execute(command));
        if let Some(label) = trace_label {
            diagnostics::record_label("command_complete", label);
            if let Some(started) = trace_started {
                diagnostics::record_ms(&format!("command_{label}_ms"), started.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }

    /// On panel open / show_replace flip, select the prefilled query so
    /// typing replaces it (VS Code "selection becomes search term, ready
    /// to overwrite" parity).
    fn select_query_after_panel_open(&mut self, old_find_state: (bool, bool, String, String), cx: &mut Context<Self>) {
        let (old_visible, _, _, _) = old_find_state;
        let now_visible = self.model.find().visible;
        let just_opened = now_visible && !old_visible;
        if !just_opened {
            return;
        }
        if self.model.find().query.is_empty() {
            return;
        }
        self.find_query_input.update(cx, |input, cx| input.select_all(cx));
    }

    fn find_input_state(&self) -> (bool, bool, String, String) {
        (
            self.model.find().visible,
            self.model.find().show_replace,
            self.model.find().query.clone(),
            self.model.find().replacement.clone(),
        )
    }

    fn sync_find_inputs_if_changed(&mut self, old_state: (bool, bool, String, String), cx: &mut Context<Self>) {
        let current = self.find_input_state();
        if current.2 != old_state.2 {
            let query = current.2;
            self.find_query_input.update(cx, |input, cx| input.set_text(&query, cx));
        }
        if current.3 != old_state.3 {
            let replacement = current.3;
            self.find_replace_input
                .update(cx, |input, cx| input.set_text(&replacement, cx));
        }
    }

    fn sync_goto_input(&mut self, cx: &mut Context<Self>) {
        let text = self.model.goto_line().unwrap_or_default().to_string();
        self.goto_line_input.update(cx, |input, cx| input.set_text(&text, cx));
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
                self.update_model(cx, true, EditorModel::submit_find_query);
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
}

fn theme_for_preference(preference: ThemePreference, appearance: WindowAppearance) -> ThemeId {
    match preference {
        ThemePreference::Dark => ThemeId::Dark,
        ThemePreference::Light => ThemeId::Light,
        ThemePreference::System => match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeId::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeId::Light,
        },
    }
}

fn initial_model_from_launch(launch: LaunchArgs, scratchpad_dir: Option<&std::path::Path>) -> EditorModel {
    let mut tabs = Vec::new();
    let mut next_tab_id = 1u64;
    let mut status = "Ready.".to_string();

    if launch.files.is_empty() {
        tabs.push(scratchpad_or_empty_tab(
            TabId::from_raw(next_tab_id),
            scratchpad_dir,
            &mut status,
        ));
    } else {
        let mut opened_paths = HashSet::new();
        for path in launch.files {
            if !opened_paths.insert(normalize_recent_path(&path)) {
                continue;
            }
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
                scratchpad_dir,
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

fn syntax_cache_is_current(cache: &ViewportCache, language: SyntaxLanguage, revision: u64) -> bool {
    cache
        .syntax_highlights
        .as_ref()
        .is_some_and(|highlights| highlights.language == language && highlights.revision == revision)
}

fn refresh_syntax_cache(
    cache: &mut ViewportCache,
    state: &TabSyntaxState,
    buffer: &Rope,
    revision: u64,
    invalidation: SyntaxInvalidation,
) {
    let line_count = buffer.len_lines();
    cache.patch_wrap_layout(buffer, revision, &invalidation);
    let can_patch = !invalidation.is_full()
        && cache.syntax_highlights.as_ref().is_some_and(|highlights| {
            highlights.language == state.language
                && highlights.lines.len() == line_count
                && highlights.line_byte_lens.len() == line_count
        });
    let line_range = if can_patch {
        invalidation.line_range(line_count)
    } else {
        0..line_count
    };
    let (lines, line_byte_lens) = state.compute_spans_for_lines(line_range.clone());
    if can_patch {
        let highlights = cache
            .syntax_highlights
            .as_mut()
            .expect("can_patch requires an existing syntax cache");
        highlights.lines[line_range.clone()].clone_from_slice(&lines);
        highlights.line_byte_lens[line_range].clone_from_slice(&line_byte_lens);
        highlights.revision = revision;
    } else {
        cache.syntax_highlights = Some(CachedSyntaxHighlights {
            language: state.language,
            revision,
            lines,
            line_byte_lens,
        });
    }
    cache.clear_code_lines();
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

fn command_trace_label(command: Command) -> Option<&'static str> {
    match command {
        Command::SelectAll => Some("select_all"),
        Command::NextTab => Some("next_tab"),
        Command::PrevTab => Some("previous_tab"),
        Command::RequestPaste => Some("request_paste"),
        Command::RequestSave => Some("request_save"),
        _ => None,
    }
}

pub(crate) fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    diagnostics::install();

    let launch = parse_launch_args();
    let has_graphical_env = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();

    if !has_graphical_env {
        eprintln!("lst requires a graphical session. Run it from a real X11 or Wayland desktop.");
        process::exit(1);
    }

    Application::new().run(move |cx: &mut App| {
        if let Err(error) = cx
            .text_system()
            .add_fonts(vec![Cow::Borrowed(lucide_icons::LUCIDE_FONT_BYTES)])
        {
            eprintln!("failed to load bundled Lucide icons: {error}");
        }
        let settings = SettingsStore::load();
        cx.bind_keys(editor_keybindings(&settings.settings.keybindings));
        cx.bind_keys(input_keybindings());
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(metrics::WINDOW_WIDTH), px(metrics::WINDOW_HEIGHT)), cx);
        let launch = launch.clone();
        let window_title = launch.window_title.clone().unwrap_or_else(|| "lst".into());
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
                cx.new(move |cx| LstGpuiApp::new(cx, launch, settings))
            },
        ) {
            Ok(window) => window,
            Err(err) => {
                eprintln!(
                    "lst failed to open a GPUI window: {err}. On this host, Xvfb is not sufficient because GPUI \
                     surface creation requires a real presentation backend."
                );
                process::exit(1);
            }
        };

        window
            .update(cx, |view, window, cx| {
                window.set_window_title(&window_title);
                window.set_rem_size(view.ui_px(metrics::BASE_REM_SIZE));
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

#[cfg(test)]
mod syntax_cache_tests {
    use super::*;
    use crate::ui::theme::SyntaxRole;
    use std::path::PathBuf;

    #[test]
    fn rebuild_syntax_cache_repopulates_invalidated_current_state() {
        let source = "fn main() { let value = \"hi\"; }\n";
        let tab = ModelEditorTab::from_path_with_stamp(TabId::from_raw(1), PathBuf::from("test.rs"), source, None);
        let state = TabSyntaxState::parse_initial(SyntaxLanguage::Rust, tab.buffer(), tab.revision())
            .expect("rust parser should be available");
        let mut cache = ViewportCache::default();

        assert!(!syntax_cache_is_current(&cache, SyntaxLanguage::Rust, tab.revision()));
        refresh_syntax_cache(
            &mut cache,
            &state,
            tab.buffer(),
            tab.revision(),
            SyntaxInvalidation::Full,
        );

        let highlights = cache
            .syntax_highlights
            .as_ref()
            .expect("syntax cache should be repopulated");
        assert!(syntax_cache_is_current(&cache, SyntaxLanguage::Rust, tab.revision()));
        assert!(
            highlights.lines[0].iter().any(|span| span.role == SyntaxRole::Keyword),
            "{:?}",
            highlights.lines[0]
        );
    }
}
